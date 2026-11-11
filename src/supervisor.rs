//! Deterministic emulation of syscalls delivered by a seccomp listener.

use std::{
    collections::{BTreeMap, VecDeque},
    fs, io,
    os::fd::{AsRawFd, OwnedFd, RawFd},
};

use anyhow::{Context, Result};
use nix::{
    errno::Errno,
    sys::{
        signal::{Signal, kill},
        wait::{WaitPidFlag, WaitStatus, waitpid},
    },
    unistd::Pid,
};
use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha20Rng;

use crate::{
    clock::{TimeWheel, TimerEvent, VirtualClock},
    interceptor::{
        FutexManager, FutexWaiter, SECCOMP_USER_NOTIF_FLAG_CONTINUE, is_id_valid,
        recv_notification, seccomp_notif, seccomp_notif_resp, send_response,
    },
    vm,
};

struct State {
    rng: ChaCha20Rng,
    clock: VirtualClock,
    wheel: TimeWheel,
    threads: BTreeMap<Pid, ThreadState>,
    futex_mgr: FutexManager,
    pending_responses: VecDeque<seccomp_notif_resp>,
}

trait MemoryReader {
    fn read_bytes(&mut self, pid: Pid, address: usize, len: usize) -> nix::Result<Vec<u8>>;

    fn read_u32(&mut self, pid: Pid, address: usize) -> nix::Result<u32> {
        let bytes = self.read_bytes(pid, address, std::mem::size_of::<u32>())?;
        Ok(u32::from_ne_bytes(
            bytes.try_into().map_err(|_| Errno::EFAULT)?,
        ))
    }
}

struct RemoteMemory;

impl MemoryReader for RemoteMemory {
    fn read_bytes(&mut self, pid: Pid, address: usize, len: usize) -> nix::Result<Vec<u8>> {
        vm::read_remote_bytes(pid, address, len)
    }

    fn read_u32(&mut self, pid: Pid, address: usize) -> nix::Result<u32> {
        vm::read_remote_val::<u32>(pid, address)
    }
}

#[cfg(test)]
impl<F: FnMut(Pid, usize, usize) -> nix::Result<Vec<u8>>> MemoryReader for F {
    fn read_bytes(&mut self, pid: Pid, address: usize, len: usize) -> nix::Result<Vec<u8>> {
        self(pid, address, len)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThreadState {
    Runnable,
    Waiting,
    Sleeping(u64),
    FutexWaiting(u64),
    Yielded,
}

impl ThreadState {
    fn notification(self) -> Option<u64> {
        match self {
            Self::Sleeping(id) | Self::FutexWaiting(id) => Some(id),
            _ => None,
        }
    }
}

impl State {
    fn new(seed: u64) -> Self {
        Self {
            rng: ChaCha20Rng::seed_from_u64(seed),
            clock: VirtualClock::default(),
            wheel: TimeWheel::default(),
            threads: BTreeMap::new(),
            futex_mgr: FutexManager::default(),
            pending_responses: VecDeque::new(),
        }
    }

    fn begin_notification(&mut self, notif: &seccomp_notif) {
        let tid = Pid::from_raw(notif.pid as i32);
        if let Some(id) = self
            .threads
            .get(&tid)
            .and_then(|state| state.notification())
        {
            self.futex_mgr.remove_by_notif_id(id);
        }
        // A new syscall from a sleeper means its previous notification was
        // canceled (for example by a signal), even if the TID was reused.
        self.wheel.retain(|event| event.tid != tid);
        self.threads.insert(tid, ThreadState::Runnable);
    }

    fn refresh_threads(&mut self, tasks: BTreeMap<Pid, bool>) {
        for (tid, state) in &self.threads {
            if !tasks.contains_key(tid) {
                if let Some(id) = state.notification() {
                    self.futex_mgr.remove_by_notif_id(id);
                }
            }
        }
        self.threads.retain(|tid, _| tasks.contains_key(tid));
        for (tid, runnable) in tasks {
            let state = self.threads.entry(tid).or_insert(ThreadState::Waiting);
            if state.notification().is_none() {
                *state = if runnable {
                    ThreadState::Runnable
                } else {
                    ThreadState::Waiting
                };
            }
        }
        self.wheel
            .retain(|event| self.threads.contains_key(&event.tid));
    }

    fn cancel_invalid_notifications(&mut self, mut valid: impl FnMut(u64) -> bool) {
        for state in self.threads.values_mut() {
            if let Some(id) = state.notification() {
                if !valid(id) {
                    self.futex_mgr.remove_by_notif_id(id);
                    self.wheel.retain(|event| event.notif_id != id);
                    *state = ThreadState::Runnable;
                }
            }
        }
    }

    fn warp_if_idle(&mut self) -> bool {
        if self
            .threads
            .values()
            .any(|state| matches!(state, ThreadState::Runnable | ThreadState::Yielded))
        {
            return false;
        }
        if let Some(deadline) = self.wheel.earliest_wake_time() {
            self.clock.advance_to(deadline);
            return true;
        }
        false
    }

    fn take_expired(&mut self) -> Vec<TimerEvent> {
        let expired = self.wheel.pop_expired(self.clock.now_ns());
        for event in &expired {
            if self
                .threads
                .get(&event.tid)
                .and_then(|state| state.notification())
                == Some(event.notif_id)
            {
                let futex_timeout =
                    self.threads[&event.tid] == ThreadState::FutexWaiting(event.notif_id);
                self.futex_mgr.remove_by_notif_id(event.notif_id);
                self.threads.insert(event.tid, ThreadState::Runnable);
                self.pending_responses.push_back(seccomp_notif_resp {
                    id: event.notif_id,
                    error: if futex_timeout { -libc::ETIMEDOUT } else { 0 },
                    ..Default::default()
                });
            }
        }
        expired
    }

    fn emulate(&mut self, notif: &seccomp_notif) -> nix::Result<Option<seccomp_notif_resp>> {
        self.emulate_with_memory(notif, &mut RemoteMemory)
    }

    #[cfg(test)]
    fn emulate_with_reader(
        &mut self,
        notif: &seccomp_notif,
        mut read: impl FnMut(Pid, usize, usize) -> nix::Result<Vec<u8>>,
    ) -> nix::Result<Option<seccomp_notif_resp>> {
        self.emulate_with_memory(notif, &mut read)
    }

    fn emulate_with_memory(
        &mut self,
        notif: &seccomp_notif,
        read: &mut impl MemoryReader,
    ) -> nix::Result<Option<seccomp_notif_resp>> {
        // pid is the notifying thread ID, which can differ from the launched PID.
        let pid = Pid::from_raw(notif.pid as i32);
        let args = notif.data.args;
        let mut response = seccomp_notif_resp {
            id: notif.id,
            ..Default::default()
        };
        match i64::from(notif.data.nr) {
            libc::SYS_sched_yield => {
                self.threads.insert(pid, ThreadState::Yielded);
                self.clock.advance_tick();
            }
            libc::SYS_futex => return self.emulate_futex(notif, read),
            libc::SYS_getrandom => {
                let len = args[1] as usize;
                let mut bytes = Vec::new();
                bytes.try_reserve_exact(len).map_err(|_| Errno::ENOMEM)?;
                bytes.resize(len, 0);
                self.rng.fill_bytes(&mut bytes);
                vm::write_remote_bytes(pid, args[0] as usize, &bytes)?;
                response.val = len as i64;
            }
            libc::SYS_clock_gettime => {
                let _clk_id = args[0];
                self.clock.advance_tick();
                let time = self.clock.now_timespec();
                vm::write_remote_val(pid, args[1] as usize, &time)?;
            }
            libc::SYS_nanosleep | libc::SYS_clock_nanosleep => {
                let address = if i64::from(notif.data.nr) == libc::SYS_nanosleep {
                    args[0]
                } else {
                    args[2]
                };
                let bytes =
                    read.read_bytes(pid, address as usize, std::mem::size_of::<libc::timespec>())?;
                let req = decode_timespec(&bytes)?;
                let sleep_ns = sleep_duration_ns(&req)?;
                let wake_time_ns = self
                    .clock
                    .now_ns()
                    .checked_add(sleep_ns)
                    .ok_or(Errno::EINVAL)?;
                self.wheel.add_timer(TimerEvent {
                    wake_time_ns,
                    notif_id: notif.id,
                    tid: pid,
                });
                self.threads.insert(pid, ThreadState::Sleeping(notif.id));
                // The event loop drains queued syscalls and refreshes live task
                // states before deciding whether it can advance virtual time.
                return Ok(None);
            }
            libc::SYS_gettimeofday => {
                self.clock.advance_tick();
                if args[0] != 0 {
                    let time = self.clock.now_timeval();
                    vm::write_remote_val(pid, args[0] as usize, &time)?;
                }
            }
            _ => response.flags = SECCOMP_USER_NOTIF_FLAG_CONTINUE,
        }
        Ok(Some(response))
    }

    fn emulate_futex(
        &mut self,
        notif: &seccomp_notif,
        read: &mut impl MemoryReader,
    ) -> nix::Result<Option<seccomp_notif_resp>> {
        let args = notif.data.args;
        let pid = Pid::from_raw(notif.pid as i32);
        let uaddr = args[0] as usize;
        let op = (args[1] as i32) & !libc::FUTEX_PRIVATE_FLAG & !libc::FUTEX_CLOCK_REALTIME;
        let val = args[2] as u32;
        let timeout_addr = args[3] as usize;
        let val3 = args[5] as u32;
        let mut response = seccomp_notif_resp {
            id: notif.id,
            ..Default::default()
        };
        let bitset = if matches!(op, libc::FUTEX_WAIT_BITSET | libc::FUTEX_WAKE_BITSET) {
            if val3 == 0 {
                return Err(Errno::EINVAL);
            }
            val3
        } else {
            u32::MAX
        };
        if matches!(
            op,
            libc::FUTEX_WAIT | libc::FUTEX_WAIT_BITSET | libc::FUTEX_WAKE | libc::FUTEX_WAKE_BITSET
        ) && uaddr % std::mem::align_of::<u32>() != 0
        {
            return Err(Errno::EINVAL);
        }
        match op {
            libc::FUTEX_WAIT | libc::FUTEX_WAIT_BITSET => {
                let current_val = read.read_u32(pid, uaddr)?;
                if current_val != val {
                    response.error = -libc::EAGAIN;
                    return Ok(Some(response));
                }
                // Validate everything before adding either queue entry.
                let deadline = if timeout_addr != 0 {
                    let bytes =
                        read.read_bytes(pid, timeout_addr, std::mem::size_of::<libc::timespec>())?;
                    let duration = sleep_duration_ns(&decode_timespec(&bytes)?)?;
                    // WAIT uses a relative timeout; WAIT_BITSET uses an
                    // absolute deadline on the same virtual clock timeline.
                    let deadline = if op == libc::FUTEX_WAIT_BITSET {
                        duration
                    } else {
                        self.clock
                            .now_ns()
                            .checked_add(duration)
                            .ok_or(Errno::EINVAL)?
                    };
                    if deadline <= self.clock.now_ns() {
                        response.error = -libc::ETIMEDOUT;
                        return Ok(Some(response));
                    }
                    Some(deadline)
                } else {
                    None
                };
                self.futex_mgr.wait(
                    uaddr,
                    FutexWaiter {
                        tid: pid,
                        notif_id: notif.id,
                        bitset,
                    },
                );
                if let Some(wake_time_ns) = deadline {
                    self.wheel.add_timer(TimerEvent {
                        wake_time_ns,
                        notif_id: notif.id,
                        tid: pid,
                    });
                }
                self.threads
                    .insert(pid, ThreadState::FutexWaiting(notif.id));
                return Ok(None);
            }
            libc::FUTEX_WAKE | libc::FUTEX_WAKE_BITSET => {
                let woken = self.futex_mgr.wake(uaddr, val as usize, bitset);
                response.val = woken.len() as i64;
                for waiter in woken {
                    self.wheel.retain(|event| event.notif_id != waiter.notif_id);
                    self.threads.insert(waiter.tid, ThreadState::Runnable);
                    self.pending_responses.push_back(seccomp_notif_resp {
                        id: waiter.notif_id,
                        ..Default::default()
                    });
                }
            }
            _ => response.flags = SECCOMP_USER_NOTIF_FLAG_CONTINUE,
        }
        Ok(Some(response))
    }
}

fn decode_timespec(bytes: &[u8]) -> nix::Result<libc::timespec> {
    if bytes.len() != std::mem::size_of::<libc::timespec>() {
        return Err(Errno::EFAULT);
    }
    // SAFETY: the buffer has the native timespec size; integer fields accept
    // every bit pattern. Unaligned reads do not require Vec<u8> alignment.
    Ok(unsafe { bytes.as_ptr().cast::<libc::timespec>().read_unaligned() })
}

fn sleep_duration_ns(req: &libc::timespec) -> nix::Result<u64> {
    if req.tv_sec < 0 || !(0..1_000_000_000).contains(&req.tv_nsec) {
        return Err(Errno::EINVAL);
    }
    (req.tv_sec as u64)
        .checked_mul(1_000_000_000)
        .and_then(|seconds| seconds.checked_add(req.tv_nsec as u64))
        .ok_or(Errno::EINVAL)
}

fn task_is_runnable(stat: &str) -> io::Result<bool> {
    // comm may contain spaces and parentheses, so split after its closing ')'.
    let (_, fields) = stat
        .rsplit_once(") ")
        .ok_or_else(|| io::Error::other("invalid task stat"))?;
    Ok(fields.starts_with('R'))
}

fn thread_snapshot(child: Pid) -> io::Result<BTreeMap<Pid, bool>> {
    let mut tasks = BTreeMap::new();
    for entry in fs::read_dir(format!("/proc/{child}/task"))? {
        let entry = entry?;
        let Some(tid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<i32>().ok())
        else {
            continue;
        };
        match fs::read_to_string(entry.path().join("stat")) {
            Ok(stat) => {
                tasks.insert(Pid::from_raw(tid), task_is_runnable(&stat)?);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (), // thread exited
            Err(error) => return Err(error),
        }
    }
    Ok(tasks)
}

fn errno(error: &anyhow::Error) -> Option<i32> {
    error
        .downcast_ref::<io::Error>()
        .and_then(io::Error::raw_os_error)
}

fn child_status(child: Pid, flags: Option<WaitPidFlag>) -> nix::Result<Option<i32>> {
    loop {
        match waitpid(child, flags) {
            Ok(WaitStatus::Exited(_, code)) => return Ok(Some(code)),
            Ok(WaitStatus::Signaled(_, signal, _)) => return Ok(Some(128 + signal as i32)),
            Ok(_) => return Ok(None),
            Err(Errno::EINTR) => continue,
            Err(error) => return Err(error),
        }
    }
}

fn poll_listener(fd: RawFd, timeout_ms: i32) -> Result<i16> {
    let mut event = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: event points to one writable pollfd.
    if unsafe { libc::poll(&mut event, 1, timeout_ms) } < 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EINTR) {
            return Ok(0);
        }
        return Err(error).context("polling seccomp listener");
    }
    Ok(event.revents)
}

fn reply(fd: RawFd, response: &seccomp_notif_resp) -> Result<()> {
    loop {
        match send_response(fd, response) {
            Ok(()) => return Ok(()),
            // A signal can cancel a syscall between validation and SEND.
            Err(error) if errno(&error) == Some(libc::ENOENT) => return Ok(()),
            Err(error) if errno(&error) == Some(libc::EINTR) => continue,
            Err(error) => return Err(error),
        }
    }
}

fn wake_expired(state: &mut State, fd: RawFd) -> Result<()> {
    state.take_expired();
    flush_responses(state, fd)
}

fn flush_responses(state: &mut State, fd: RawFd) -> Result<()> {
    while let Some(response) = state.pending_responses.front() {
        reply(fd, response)?;
        state.pending_responses.pop_front();
    }
    Ok(())
}

fn supervise(child: Pid, notif_fd: &OwnedFd, seed: u64) -> Result<i32> {
    let mut state = State::new(seed);
    supervise_loop(child, notif_fd, &mut state, State::emulate)
}

fn supervise_loop(
    child: Pid,
    notif_fd: &OwnedFd,
    state: &mut State,
    mut emulate: impl FnMut(&mut State, &seccomp_notif) -> nix::Result<Option<seccomp_notif_resp>>,
) -> Result<i32> {
    let fd = notif_fd.as_raw_fd();
    loop {
        if let Some(code) = child_status(child, Some(WaitPidFlag::WNOHANG))? {
            return Ok(code);
        }

        state.cancel_invalid_notifications(|id| is_id_valid(fd, id));
        wake_expired(state, fd)?;
        // First drain pending notifications, including those from previously
        // unseen threads, before considering an idle-time warp.
        let mut revents = poll_listener(fd, 0)?;
        if revents == 0 && !state.wheel.is_empty() {
            match thread_snapshot(child) {
                Ok(tasks) => state.refresh_threads(tasks),
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error).context("reading child thread states"),
            }
            state.cancel_invalid_notifications(|id| is_id_valid(fd, id));
            // Task states and notification readiness are sampled separately.
            // Recheck readiness after the snapshot to avoid skipping a request.
            revents = poll_listener(fd, 0)?;
            if revents == 0 && state.warp_if_idle() {
                wake_expired(state, fd)?;
                continue;
            }
        }
        if revents == 0 {
            // Polling bounds exit detection; pending timers also need frequent
            // task-state refreshes for threads entering an untrapped futex wait.
            revents = poll_listener(fd, if state.wheel.is_empty() { 50 } else { 1 })?;
        }
        if revents == 0 {
            continue;
        }
        if revents & libc::POLLHUP != 0 {
            // No filtered tasks remain. Reap the launched child.
            loop {
                if let Some(code) = child_status(child, None)? {
                    return Ok(code);
                }
            }
        }
        anyhow::ensure!(
            revents & (libc::POLLERR | libc::POLLNVAL) == 0,
            "seccomp listener poll failed: {:#x}",
            revents
        );
        if revents & libc::POLLIN == 0 {
            continue;
        }

        let notification = match recv_notification(fd) {
            Ok(notification) => notification,
            Err(error) if matches!(errno(&error), Some(libc::EINTR | libc::ENOENT)) => continue,
            Err(error) => return Err(error),
        };
        // The target may have received a signal while the notification waited.
        if !is_id_valid(fd, notification.id) {
            continue;
        }
        state.begin_notification(&notification);
        let response = match emulate(state, &notification) {
            Ok(response) => response,
            Err(error) => Some(seccomp_notif_resp {
                id: notification.id,
                error: -(error as i32),
                ..Default::default()
            }),
        };
        // Wake queued futex waiters before returning the count to the waker.
        flush_responses(state, fd)?;
        if let Some(response) = response {
            reply(fd, &response)?;
        }
        wake_expired(state, fd)?;
    }
}

/// Supervise intercepted syscalls and reap the child, returning its exit code.
/// Signal deaths use the shell convention of 128 plus the signal number.
pub fn run_supervisor(child: Pid, notif_fd: OwnedFd, seed: u64) -> Result<i32> {
    let result = supervise(child, &notif_fd, seed);
    if result.is_err() {
        // A failed supervisor must not leave its target blocked in a syscall.
        let _ = kill(child, Signal::SIGKILL);
        let _ = child_status(child, None);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn notification(nr: i64, args: [u64; 6]) -> seccomp_notif {
        seccomp_notif {
            id: 42,
            pid: Pid::this().as_raw() as u32,
            data: crate::interceptor::seccomp_data {
                nr: nr as i32,
                args,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn request_bytes(seconds: i64, nanoseconds: i64) -> Vec<u8> {
        [seconds.to_ne_bytes(), nanoseconds.to_ne_bytes()].concat()
    }

    fn futex_notification(tid: i32, id: u64, args: [u64; 6]) -> seccomp_notif {
        let mut notif = notification(libc::SYS_futex, args);
        notif.pid = tid as u32;
        notif.id = id;
        notif
    }

    fn emulate_futex_request(
        state: &mut State,
        notif: &seccomp_notif,
        word: u32,
        timeout: (i64, i64),
    ) -> nix::Result<Option<seccomp_notif_resp>> {
        state.begin_notification(notif);
        state.emulate_with_reader(notif, |pid, address, len| {
            assert_eq!(pid.as_raw() as u32, notif.pid);
            match (address, len) {
                (0x1000, 4) => Ok(word.to_ne_bytes().to_vec()),
                (0x2000, 16) => Ok(request_bytes(timeout.0, timeout.1)),
                _ => panic!("unexpected memory read at {address:#x}, length {len}"),
            }
        })
    }

    #[test]
    fn futex_wait_compares_word_before_enqueuing() {
        let mut state = State::new(0);
        let notif = futex_notification(
            1,
            1,
            [
                0x1000,
                (libc::FUTEX_WAIT | libc::FUTEX_PRIVATE_FLAG) as u64,
                7,
                0,
                0,
                0,
            ],
        );
        let response = emulate_futex_request(&mut state, &notif, 8, (0, 0))
            .unwrap()
            .unwrap();
        assert_eq!(
            (response.val, response.error, response.flags),
            (0, -libc::EAGAIN, 0)
        );
        assert!(state.futex_mgr.wake(0x1000, 10, u32::MAX).is_empty());
        assert!(state.wheel.is_empty());
        assert!(
            emulate_futex_request(&mut state, &notif, 7, (0, 0))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            state.threads[&Pid::from_raw(1)],
            ThreadState::FutexWaiting(1)
        );
        assert!(!state.warp_if_idle()); // no deadline for an untimed wait
    }

    #[test]
    fn futex_wake_uses_fifo_masks_and_returns_actual_count() {
        let mut state = State::new(0);
        for (id, mask) in [(1, 1), (2, 2), (3, 3)] {
            let notif = futex_notification(
                id,
                id as u64,
                [0x1000, libc::FUTEX_WAIT_BITSET as u64, 7, 0, 0, mask],
            );
            assert!(
                emulate_futex_request(&mut state, &notif, 7, (0, 0))
                    .unwrap()
                    .is_none()
            );
        }
        let zero = futex_notification(4, 4, [0x1000, libc::FUTEX_WAKE as u64, 0, 0, 0, 0]);
        assert_eq!(
            emulate_futex_request(&mut state, &zero, 0, (0, 0))
                .unwrap()
                .unwrap()
                .val,
            0
        );
        assert!(state.pending_responses.is_empty());
        let wake = futex_notification(
            4,
            5,
            [
                0x1000,
                (libc::FUTEX_WAKE_BITSET | libc::FUTEX_PRIVATE_FLAG) as u64,
                1,
                0,
                0,
                2,
            ],
        );
        assert_eq!(
            emulate_futex_request(&mut state, &wake, 0, (0, 0))
                .unwrap()
                .unwrap()
                .val,
            1
        );
        let reply = state.pending_responses.pop_front().unwrap();
        assert_eq!((reply.id, reply.val, reply.error), (2, 0, 0));
        assert_eq!(state.threads[&Pid::from_raw(2)], ThreadState::Runnable);
        assert_eq!(
            state.threads[&Pid::from_raw(1)],
            ThreadState::FutexWaiting(1)
        );
        let wake_all = futex_notification(4, 6, [0x1000, libc::FUTEX_WAKE as u64, 100, 0, 0, 0]);
        assert_eq!(
            emulate_futex_request(&mut state, &wake_all, 0, (0, 0))
                .unwrap()
                .unwrap()
                .val,
            2
        );
        assert_eq!(
            state
                .pending_responses
                .iter()
                .map(|resp| resp.id)
                .collect::<Vec<_>>(),
            [1, 3]
        );
        assert_eq!(
            emulate_futex_request(&mut state, &wake_all, 0, (0, 0))
                .unwrap()
                .unwrap()
                .val,
            0
        );
    }

    #[test]
    fn futex_timeouts_are_relative_for_wait_and_absolute_for_bitset() {
        for (op, expected) in [
            (libc::FUTEX_WAIT, 600),
            (libc::FUTEX_WAIT_BITSET, 500),
            (libc::FUTEX_WAIT_BITSET | libc::FUTEX_CLOCK_REALTIME, 500),
        ] {
            let mut state = State::new(0);
            state.clock.advance_to(100);
            let notif = futex_notification(1, 1, [0x1000, op as u64, 7, 0x2000, 0, 1]);
            assert!(
                emulate_futex_request(&mut state, &notif, 7, (0, 500))
                    .unwrap()
                    .is_none()
            );
            assert_eq!(state.wheel.earliest_wake_time(), Some(expected));
            assert!(state.warp_if_idle());
            assert_eq!(state.take_expired().len(), 1);
            let response = state.pending_responses.pop_front().unwrap();
            assert_eq!(
                (response.id, response.error, response.val),
                (1, -libc::ETIMEDOUT, 0)
            );
            assert_eq!(state.threads[&Pid::from_raw(1)], ThreadState::Runnable);
            assert!(state.futex_mgr.wake(0x1000, 10, u32::MAX).is_empty());
            assert!(state.take_expired().is_empty());
            assert!(state.pending_responses.is_empty());
        }
    }

    #[test]
    fn futex_wake_cancels_timeout_without_a_second_reply() {
        let mut state = State::new(0);
        let wait = futex_notification(1, 1, [0x1000, libc::FUTEX_WAIT as u64, 7, 0x2000, 0, 0]);
        emulate_futex_request(&mut state, &wait, 7, (10, 0)).unwrap();
        let wake = futex_notification(2, 2, [0x1000, libc::FUTEX_WAKE as u64, 1, 0, 0, 0]);
        assert_eq!(
            emulate_futex_request(&mut state, &wake, 0, (0, 0))
                .unwrap()
                .unwrap()
                .val,
            1
        );
        assert!(state.wheel.is_empty());
        state.clock.advance_to(20_000_000_000);
        assert!(state.take_expired().is_empty());
        let response = state.pending_responses.pop_front().unwrap();
        assert_eq!((response.id, response.error), (1, 0));
        assert!(state.pending_responses.is_empty());
    }

    #[test]
    fn futex_cancellation_cleans_timed_and_untimed_waits() {
        let mut state = State::new(0);
        for (id, timeout) in [(1, 0), (2, 0x2000)] {
            let wait = futex_notification(
                id,
                id as u64,
                [0x1000, libc::FUTEX_WAIT as u64, 7, timeout, 0, 0],
            );
            emulate_futex_request(&mut state, &wait, 7, (1, 0)).unwrap();
        }
        state.cancel_invalid_notifications(|_| false);
        assert!(state.wheel.is_empty());
        assert!(state.futex_mgr.wake(0x1000, 10, u32::MAX).is_empty());
        assert!(state.pending_responses.is_empty());
        // A new syscall and a disappearing task also cancel their old waits.
        let wait = futex_notification(1, 3, [0x1000, libc::FUTEX_WAIT as u64, 7, 0x2000, 0, 0]);
        emulate_futex_request(&mut state, &wait, 7, (1, 0)).unwrap();
        let mut next = notification(libc::SYS_sched_yield, [0; 6]);
        next.pid = 1;
        state.begin_notification(&next);
        assert!(state.wheel.is_empty());
        assert!(state.futex_mgr.wake(0x1000, 10, u32::MAX).is_empty());
        emulate_futex_request(&mut state, &wait, 7, (1, 0)).unwrap();
        state.refresh_threads(BTreeMap::new());
        assert!(state.wheel.is_empty());
        assert!(state.futex_mgr.wake(0x1000, 10, u32::MAX).is_empty());
    }

    #[test]
    fn futex_invalid_requests_leave_no_pending_wait() {
        let mut state = State::new(0);
        for op in [libc::FUTEX_WAIT_BITSET, libc::FUTEX_WAKE_BITSET] {
            let notif = futex_notification(1, 1, [0x1000, op as u64, 7, 0, 0, 0]);
            assert_eq!(
                emulate_futex_request(&mut state, &notif, 7, (0, 0)).unwrap_err(),
                Errno::EINVAL
            );
        }
        let unaligned = futex_notification(1, 1, [0x1001, libc::FUTEX_WAIT as u64, 7, 0, 0, 0]);
        assert_eq!(
            emulate_futex_request(&mut state, &unaligned, 7, (0, 0)).unwrap_err(),
            Errno::EINVAL
        );
        let wait = futex_notification(1, 1, [0x1000, libc::FUTEX_WAIT as u64, 7, 0x2000, 0, 0]);
        for invalid in [(-1, 0), (0, -1), (0, 1_000_000_000), (i64::MAX, 0)] {
            assert_eq!(
                emulate_futex_request(&mut state, &wait, 7, invalid).unwrap_err(),
                Errno::EINVAL
            );
        }
        assert_eq!(
            state
                .emulate_with_reader(&wait, |_, _, _| Err(Errno::EFAULT))
                .unwrap_err(),
            Errno::EFAULT
        );
        assert_eq!(
            state
                .emulate_with_reader(&wait, |_, _, _| Ok(vec![0; 3]))
                .unwrap_err(),
            Errno::EFAULT
        );
        assert!(state.wheel.is_empty());
        assert!(state.futex_mgr.wake(0x1000, 10, u32::MAX).is_empty());
    }

    #[test]
    fn futex_immediate_timeout_and_mismatch_precedence() {
        let mut state = State::new(0);
        state.clock.advance_to(100);
        for (op, timeout) in [
            (libc::FUTEX_WAIT, (0, 0)),
            (libc::FUTEX_WAIT_BITSET, (0, 50)),
        ] {
            let wait = futex_notification(1, 1, [0x1000, op as u64, 7, 0x2000, 0, 1]);
            let response = emulate_futex_request(&mut state, &wait, 7, timeout)
                .unwrap()
                .unwrap();
            assert_eq!(response.error, -libc::ETIMEDOUT);
            let response = state
                .emulate_with_reader(&wait, |_, address, _| {
                    assert_eq!(address, 0x1000, "mismatch must not read timeout");
                    Ok(8_u32.to_ne_bytes().to_vec())
                })
                .unwrap()
                .unwrap();
            assert_eq!(response.error, -libc::EAGAIN);
        }
        assert!(state.wheel.is_empty());
        assert!(state.futex_mgr.wake(0x1000, 10, u32::MAX).is_empty());
    }

    #[test]
    fn yield_ticks_the_clock_and_expires_futex_waits() {
        let mut state = State::new(0);
        let wait = futex_notification(1, 1, [0x1000, libc::FUTEX_WAIT as u64, 7, 0x2000, 0, 0]);
        emulate_futex_request(&mut state, &wait, 7, (0, 100)).unwrap();
        let mut yield_call = notification(libc::SYS_sched_yield, [0; 6]);
        yield_call.pid = 2;
        state.begin_notification(&yield_call);
        let response = state.emulate(&yield_call).unwrap().unwrap();
        assert_eq!((response.val, response.error, response.flags), (0, 0, 0));
        assert_eq!(state.threads[&Pid::from_raw(2)], ThreadState::Yielded);
        assert_eq!(state.clock.now_ns(), 100);
        assert_eq!(state.take_expired().len(), 1);
        assert_eq!(
            state.pending_responses.pop_front().unwrap().error,
            -libc::ETIMEDOUT
        );
    }

    #[test]
    fn unhandled_futex_operations_continue_in_kernel() {
        let mut state = State::new(0);
        let notif = futex_notification(1, 1, [0x1000, libc::FUTEX_REQUEUE as u64, 0, 0, 0, 0]);
        let response = state
            .emulate_with_reader(&notif, |_, _, _| panic!("unexpected read"))
            .unwrap()
            .unwrap();
        assert_eq!(response.flags, SECCOMP_USER_NOTIF_FLAG_CONTINUE);
    }

    fn schedule(state: &mut State, tid: i32, id: u64, ns: i64) {
        let mut notif = notification(libc::SYS_nanosleep, [0; 6]);
        notif.pid = tid as u32;
        notif.id = id;
        state.begin_notification(&notif);
        let response = state
            .emulate_with_reader(&notif, |_, _, _| Ok(request_bytes(0, ns)))
            .unwrap();
        assert!(
            response.is_none(),
            "sleep must remain pending until its deadline"
        );
    }

    #[test]
    fn validates_timespec_and_duration_overflow() {
        for (sec, ns) in [
            (-1, 0),
            (0, -1),
            (0, 1_000_000_000),
            (i64::MAX, 0),
            (18_446_744_073, 709_551_616),
        ] {
            let req = libc::timespec {
                tv_sec: sec,
                tv_nsec: ns,
            };
            assert_eq!(sleep_duration_ns(&req), Err(Errno::EINVAL));
        }
        for (sec, ns, expected) in [
            (0, 0, 0),
            (1, 999_999_999, 1_999_999_999),
            (18_446_744_073, 709_551_615, u64::MAX),
        ] {
            let req = libc::timespec {
                tv_sec: sec,
                tv_nsec: ns,
            };
            assert_eq!(sleep_duration_ns(&req), Ok(expected));
        }
    }

    #[test]
    fn sleep_syscalls_read_the_correct_request_argument() {
        for (nr, expected_address) in [(libc::SYS_nanosleep, 11), (libc::SYS_clock_nanosleep, 33)] {
            let mut state = State::new(0);
            let notif = notification(nr, [11, 22, 33, 44, 0, 0]);
            state.begin_notification(&notif);
            let response = state
                .emulate_with_reader(&notif, |pid, address, len| {
                    assert_eq!(pid.as_raw() as u32, notif.pid);
                    assert_eq!(address, expected_address);
                    assert_eq!(len, std::mem::size_of::<libc::timespec>());
                    Ok(request_bytes(2, 345))
                })
                .unwrap();
            assert!(response.is_none());
            assert_eq!(state.clock.now_ns(), 0);
            assert_eq!(state.wheel.earliest_wake_time(), Some(2_000_000_345));
        }
    }

    #[test]
    fn bad_requests_do_not_enqueue_or_advance_time() {
        let notif = notification(libc::SYS_nanosleep, [0; 6]);
        let mut state = State::new(0);
        state.begin_notification(&notif);
        assert_eq!(
            state
                .emulate_with_reader(&notif, |_, _, _| Err(Errno::EFAULT))
                .unwrap_err(),
            Errno::EFAULT
        );
        assert_eq!(
            state
                .emulate_with_reader(&notif, |_, _, _| Ok(vec![0; 8]))
                .unwrap_err(),
            Errno::EFAULT
        );
        assert_eq!(
            state
                .emulate_with_reader(&notif, |_, _, _| Ok(request_bytes(-1, 0)))
                .unwrap_err(),
            Errno::EINVAL
        );
        assert_eq!(state.clock.now_ns(), 0);
        assert!(state.wheel.is_empty());
        assert_eq!(state.threads[&Pid::this()], ThreadState::Runnable);
        state.clock.advance_to(u64::MAX);
        assert_eq!(
            state
                .emulate_with_reader(&notif, |_, _, _| Ok(request_bytes(0, 1)))
                .unwrap_err(),
            Errno::EINVAL
        );
        assert!(state.wheel.is_empty());
    }

    #[test]
    fn lone_sleeper_warps_and_wakes_exactly_once() {
        let mut state = State::new(0);
        schedule(&mut state, 1, 42, 999_999_999);
        assert!(state.warp_if_idle());
        assert_eq!(state.clock.now_ns(), 999_999_999);
        let expired = state.take_expired();
        assert_eq!(
            expired,
            [TimerEvent {
                wake_time_ns: 999_999_999,
                notif_id: 42,
                tid: Pid::from_raw(1)
            }]
        );
        assert_eq!(state.threads[&Pid::from_raw(1)], ThreadState::Runnable);
        assert!(state.take_expired().is_empty());
        assert!(!state.warp_if_idle());
        schedule(&mut state, 1, 43, 0);
        assert_eq!(state.take_expired()[0].notif_id, 43);
        assert_eq!(state.clock.now_ns(), 999_999_999);
    }

    #[test]
    fn runnable_threads_prevent_warp_and_shortest_sleep_wakes_first() {
        let mut state = State::new(0);
        state.refresh_threads(BTreeMap::from([
            (Pid::from_raw(1), true),
            (Pid::from_raw(2), true),
        ]));
        schedule(&mut state, 1, 1, 500);
        assert!(!state.warp_if_idle());
        assert_eq!(state.clock.now_ns(), 0);
        schedule(&mut state, 2, 2, 200);
        assert!(state.warp_if_idle());
        assert_eq!(state.clock.now_ns(), 200);
        assert_eq!(state.take_expired()[0].tid, Pid::from_raw(2));
        assert_eq!(state.wheel.len(), 1);
        assert!(!state.warp_if_idle());
        // The awakened worker blocks in an untrapped futex/join operation.
        state.refresh_threads(BTreeMap::from([
            (Pid::from_raw(1), false),
            (Pid::from_raw(2), false),
        ]));
        assert!(state.warp_if_idle());
        assert_eq!(state.clock.now_ns(), 500);
        assert_eq!(state.take_expired()[0].tid, Pid::from_raw(1));
    }

    #[test]
    fn clock_ticks_expire_pending_timers_without_a_warp() {
        let mut state = State::new(0);
        schedule(&mut state, 1, 1, 200);
        let mut clock_read = notification(libc::SYS_gettimeofday, [0; 6]);
        clock_read.pid = 2;
        state.begin_notification(&clock_read);
        state.emulate(&clock_read).unwrap();
        assert!(state.take_expired().is_empty());
        state.emulate(&clock_read).unwrap();
        assert_eq!(state.take_expired()[0].notif_id, 1);
        assert_eq!(state.clock.now_ns(), 200);
    }

    #[test]
    fn equal_deadlines_wake_together() {
        let mut state = State::new(0);
        schedule(&mut state, 1, 2, 100);
        schedule(&mut state, 2, 1, 100);
        assert!(state.warp_if_idle());
        let events = state.take_expired();
        assert_eq!(
            events
                .iter()
                .map(|event| event.notif_id)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        assert!(state.wheel.is_empty());
        assert_eq!(state.clock.now_ns(), 100);
    }

    #[test]
    fn canceled_notifications_and_exited_threads_leave_no_timers() {
        let mut state = State::new(0);
        schedule(&mut state, 1, 1, 100);
        state.cancel_invalid_notifications(|_| false);
        assert!(state.wheel.is_empty());
        assert!(!state.warp_if_idle());
        schedule(&mut state, 1, 2, 100);
        state.cancel_invalid_notifications(|id| id == 2);
        assert_eq!(state.wheel.len(), 1);
        let mut next = notification(libc::SYS_getrandom, [0; 6]);
        next.pid = 1;
        state.begin_notification(&next);
        assert!(state.wheel.is_empty());
        schedule(&mut state, 1, 3, 100);
        state.refresh_threads(BTreeMap::new());
        assert!(state.wheel.is_empty());
        assert!(state.threads.is_empty());
        assert!(!state.warp_if_idle());
        assert_eq!(state.clock.now_ns(), 0);
    }

    #[test]
    fn newly_discovered_workers_prevent_premature_warp() {
        let mut state = State::new(0);
        schedule(&mut state, 1, 1, 100);
        state.refresh_threads(BTreeMap::from([
            (Pid::from_raw(1), false),
            (Pid::from_raw(2), true),
        ]));
        assert_eq!(state.threads[&Pid::from_raw(1)], ThreadState::Sleeping(1));
        assert!(!state.warp_if_idle());
    }

    #[test]
    fn proc_task_states_handle_names_with_parentheses() {
        assert!(task_is_runnable("1 (a ) tricky (name)) R 0 1").unwrap());
        for status in ['S', 'D', 'T', 'Z'] {
            assert!(!task_is_runnable(&format!("1 (thread name) {status} 0 1")).unwrap());
        }
        assert!(task_is_runnable("invalid").is_err());
        assert!(
            thread_snapshot(Pid::this())
                .unwrap()
                .contains_key(&Pid::this())
        );
    }

    #[test]
    fn random_bytes_are_seeded_and_written() {
        if !vm::memory_access_available() {
            return;
        }
        let mut a = [0_u8; 32];
        let mut b = [0_u8; 32];
        let response = State::new(7)
            .emulate(&notification(
                libc::SYS_getrandom,
                [a.as_mut_ptr() as u64, 32, 0, 0, 0, 0],
            ))
            .unwrap();
        State::new(7)
            .emulate(&notification(
                libc::SYS_getrandom,
                [b.as_mut_ptr() as u64, 32, 0, 0, 0, 0],
            ))
            .unwrap();
        assert_eq!(response.unwrap().id, 42);
        assert_eq!(response.unwrap().val, 32);
        assert_eq!(a, b);
        State::new(8)
            .emulate(&notification(
                libc::SYS_getrandom,
                [b.as_mut_ptr() as u64, 32, 0, 0, 0, 0],
            ))
            .unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn clocks_and_sleeps_advance_virtual_time() {
        if !vm::memory_access_available() {
            return;
        }
        let mut state = State::new(7);
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let clock = notification(
            libc::SYS_clock_gettime,
            [
                libc::CLOCK_MONOTONIC as u64,
                std::ptr::from_mut(&mut ts) as u64,
                0,
                0,
                0,
                0,
            ],
        );
        assert_eq!(state.emulate(&clock).unwrap().unwrap().val, 0);
        assert_eq!((ts.tv_sec, ts.tv_nsec), (0, 100));
        for nr in [libc::SYS_nanosleep, libc::SYS_clock_nanosleep] {
            let sleep = notification(nr, [0; 6]);
            state.begin_notification(&sleep);
            assert!(
                state
                    .emulate_with_reader(&sleep, |_, _, _| Ok(request_bytes(0, 10_000_000)))
                    .unwrap()
                    .is_none()
            );
            assert!(state.warp_if_idle());
            assert_eq!(state.take_expired().len(), 1);
        }
        state.emulate(&clock).unwrap();
        assert_eq!((ts.tv_sec, ts.tv_nsec), (0, 20_000_200));
        let mut tv = libc::timeval {
            tv_sec: 0,
            tv_usec: 0,
        };
        state
            .emulate(&notification(
                libc::SYS_gettimeofday,
                [std::ptr::from_mut(&mut tv) as u64, 0, 0, 0, 0, 0],
            ))
            .unwrap();
        assert_eq!((tv.tv_sec, tv.tv_usec), (0, 20_000));
        state
            .emulate(&notification(libc::SYS_gettimeofday, [0; 6]))
            .unwrap();
        assert_eq!(state.clock.now_ns(), 20_000_400);
    }

    #[test]
    fn invalid_memory_and_unhandled_syscalls() {
        let mut state = State::new(0);
        if vm::memory_access_available() {
            assert_eq!(
                state
                    .emulate(&notification(libc::SYS_clock_gettime, [0; 6]))
                    .unwrap_err(),
                Errno::EFAULT
            );
        }
        let response = state
            .emulate(&notification(libc::SYS_getpid, [0; 6]))
            .unwrap();
        assert_eq!(response.unwrap().flags, SECCOMP_USER_NOTIF_FLAG_CONTINUE);
    }

    #[test]
    fn null_timeval_advances_one_tick() {
        let mut state = State::new(0);
        let response = state
            .emulate(&notification(libc::SYS_gettimeofday, [0; 6]))
            .unwrap()
            .unwrap();
        assert_eq!((response.val, response.error, response.flags), (0, 0, 0));
        assert_eq!(state.clock.now_ns(), 100);
    }

    #[test]
    fn reaps_child_and_preserves_exit_status() {
        if !crate::launcher::exec_scrubbing_available() {
            return;
        }
        let (child, fd) =
            crate::launcher::launch_child(Path::new("/bin/sh"), &["-c".into(), "exit 23".into()])
                .unwrap();
        assert_eq!(run_supervisor(child, fd, 7).unwrap(), 23);
        assert_eq!(waitpid(child, None), Err(Errno::ECHILD));
    }

    #[test]
    fn real_notifications_sleep_and_wake_without_wall_time_delay() {
        use nix::{
            sys::socket::{AddressFamily, SockFlag, SockType, socketpair},
            unistd::{ForkResult, fork},
        };

        let futex_word = 7_u32;
        let requests = [
            libc::timespec {
                tv_sec: -1,
                tv_nsec: 0,
            },
            libc::timespec {
                tv_sec: 20,
                tv_nsec: 0,
            },
            libc::timespec {
                tv_sec: 40,
                tv_nsec: 0,
            },
            libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
        ];
        let (_filters, program) = crate::interceptor::create_filter_prog();
        let (parent_socket, child_socket) = socketpair(
            AddressFamily::Unix,
            SockType::SeqPacket,
            None,
            SockFlag::SOCK_CLOEXEC,
        )
        .unwrap();

        // SAFETY: the child uses stack data, syscalls, and fd passing, then
        // _exit; it does not touch test harness locks or run Rust destructors.
        let child = match unsafe { fork() }.unwrap() {
            ForkResult::Child => {
                // SAFETY: all pointer arguments below refer to live stack
                // objects; null outputs are supported by these syscalls.
                unsafe {
                    libc::close(parent_socket.as_raw_fd());
                    libc::alarm(5); // bound regressions that would stall a sleeper
                    if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) < 0 {
                        libc::_exit(1);
                    }
                    let fd = libc::syscall(
                        libc::SYS_seccomp,
                        libc::SECCOMP_SET_MODE_FILTER,
                        libc::SECCOMP_FILTER_FLAG_NEW_LISTENER,
                        &program,
                    ) as RawFd;
                    if fd < 0 || crate::launcher::send_fd(child_socket.as_raw_fd(), fd).is_err() {
                        libc::_exit(2);
                    }
                    libc::close(fd);
                    libc::close(child_socket.as_raw_fd());
                    let no_remainder = std::ptr::null_mut::<libc::timespec>();
                    if libc::syscall(libc::SYS_nanosleep, &requests[0], no_remainder) != -1
                        || Errno::last() != Errno::EINVAL
                    {
                        libc::_exit(3);
                    }
                    if libc::syscall(libc::SYS_nanosleep, &requests[1], no_remainder) != 0 {
                        libc::_exit(4);
                    }
                    if libc::syscall(
                        libc::SYS_gettimeofday,
                        std::ptr::null_mut::<libc::timeval>(),
                        std::ptr::null_mut::<libc::timezone>(),
                    ) != 0
                    {
                        libc::_exit(5);
                    }
                    if libc::syscall(
                        libc::SYS_clock_nanosleep,
                        libc::CLOCK_MONOTONIC,
                        0,
                        &requests[2],
                        no_remainder,
                    ) != 0
                    {
                        libc::_exit(6);
                    }
                    if libc::syscall(libc::SYS_nanosleep, &requests[3], no_remainder) != 0 {
                        libc::_exit(7);
                    }
                    if libc::syscall(
                        libc::SYS_futex,
                        &futex_word,
                        libc::FUTEX_WAIT | libc::FUTEX_PRIVATE_FLAG,
                        7_u32,
                        &requests[1],
                        0_usize,
                        0_u32,
                    ) != -1
                        || Errno::last() != Errno::ETIMEDOUT
                    {
                        libc::_exit(8);
                    }
                    if libc::syscall(libc::SYS_sched_yield) != 0 {
                        libc::_exit(9);
                    }
                    if libc::syscall(
                        libc::SYS_futex,
                        &futex_word,
                        libc::FUTEX_WAKE | libc::FUTEX_PRIVATE_FLAG,
                        1_u32,
                        0_usize,
                        0_usize,
                        0_u32,
                    ) != 0
                    {
                        libc::_exit(10);
                    }
                    libc::_exit(0);
                }
            }
            ForkResult::Parent { child } => child,
        };
        drop(child_socket);
        let listener = crate::launcher::recv_fd(parent_socket.as_raw_fd());
        if listener.is_err() {
            let _ = waitpid(child, None);
        }
        let listener = listener.expect("child must transfer its seccomp listener");
        let mut reads = 0;
        let mut state = State::new(0);
        let result = supervise_loop(child, &listener, &mut state, |state, notif| {
            state.emulate_with_reader(notif, |pid, address, len| {
                // The fork preserves these addresses. Supply the known request
                // bytes so the real notification loop is tested even where
                // the enclosing sandbox denies process_vm_readv.
                if pid != child {
                    return Err(Errno::EFAULT);
                }
                if address == std::ptr::from_ref(&futex_word) as usize && len == 4 {
                    reads += 1;
                    return Ok(futex_word.to_ne_bytes().to_vec());
                }
                if len != std::mem::size_of::<libc::timespec>() {
                    return Err(Errno::EFAULT);
                }
                let req = requests
                    .iter()
                    .find(|req| std::ptr::from_ref(*req) as usize == address)
                    .ok_or(Errno::EFAULT)?;
                reads += 1;
                Ok(request_bytes(req.tv_sec, req.tv_nsec))
            })
        });
        if result.is_err() {
            let _ = kill(child, Signal::SIGKILL);
            let _ = waitpid(child, None);
        }
        assert_eq!(result.unwrap(), 0);
        assert_eq!(reads, 6);
        assert_eq!(state.clock.now_ns(), 80_000_000_200);
        assert!(state.wheel.is_empty());
        assert!(
            state
                .futex_mgr
                .wake(std::ptr::from_ref(&futex_word) as usize, 1, u32::MAX)
                .is_empty()
        );
        assert_eq!(waitpid(child, None), Err(Errno::ECHILD));
    }

    #[test]
    fn reaps_signaled_child() {
        if !crate::launcher::exec_scrubbing_available() {
            return;
        }
        let (child, fd) = crate::launcher::launch_child(
            Path::new("/bin/sh"),
            &["-c".into(), "kill -TERM $$".into()],
        )
        .unwrap();
        assert_eq!(run_supervisor(child, fd, 7).unwrap(), 128 + libc::SIGTERM);
    }
}

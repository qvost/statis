//! Deterministic emulation of syscalls delivered by a seccomp listener.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs, io,
    os::fd::{AsRawFd, OwnedFd, RawFd},
};

use anyhow::{Context, Result};
use nix::{
    errno::Errno,
    sys::{
        ptrace,
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
        FaultConfig, FutexManager, FutexWaiter, IO_SYSCALLS, SECCOMP_USER_NOTIF_FLAG_CONTINUE,
        is_id_valid, recv_notification, seccomp_notif, seccomp_notif_resp, send_response, tsc,
    },
    scheduler::{DeterministicScheduler, ThreadState as SchedulerThreadState, pmu},
    telemetry::ExecutionTelemetry,
    vm,
};

#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    pub seed: u64,
    pub quantum: u64,
    pub fault_config: FaultConfig,
}

struct State {
    rng: ChaCha20Rng,
    fault_config: FaultConfig,
    scheduler: DeterministicScheduler,
    telemetry: ExecutionTelemetry,
    clock: VirtualClock,
    wheel: TimeWheel,
    threads: BTreeMap<Pid, ThreadState>,
    futex_mgr: FutexManager,
    pending_responses: VecDeque<seccomp_notif_resp>,
    tracees: BTreeSet<Pid>,
    new_tracees: BTreeSet<Pid>,
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
    fn new(seed: u64, quantum: u64, fault_config: FaultConfig) -> Self {
        Self {
            rng: ChaCha20Rng::seed_from_u64(seed),
            fault_config,
            scheduler: DeterministicScheduler::new(seed, quantum),
            telemetry: ExecutionTelemetry::default(),
            clock: VirtualClock::default(),
            wheel: TimeWheel::default(),
            threads: BTreeMap::new(),
            futex_mgr: FutexManager::default(),
            pending_responses: VecDeque::new(),
            tracees: BTreeSet::new(),
            new_tracees: BTreeSet::new(),
        }
    }

    fn register_thread(&mut self, tid: Pid) {
        if self.scheduler.thread_mut(tid).is_some() {
            return;
        }
        self.scheduler.register_thread(tid);
        match pmu::attach_pmu_to_thread(tid, self.scheduler.quantum()) {
            Ok(fd) => self.scheduler.thread_mut(tid).unwrap().pmu_fd = Some(fd),
            Err(error) => log::debug!("PMU unavailable for thread {tid}: {error}"),
        }
    }

    fn select_next_thread(&mut self) -> Option<Pid> {
        for (&tid, state) in &self.threads {
            if let Some(thread) = self.scheduler.thread_mut(tid) {
                thread.state = if matches!(state, ThreadState::Runnable | ThreadState::Yielded) {
                    SchedulerThreadState::Runnable
                } else {
                    SchedulerThreadState::Waiting
                };
            }
        }
        self.scheduler.pick_next_runnable()
    }

    fn begin_notification(&mut self, notif: &seccomp_notif) {
        let tid = Pid::from_raw(notif.pid as i32);
        self.register_thread(tid);
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
        self.refresh_threads_with_memory(tasks, &mut RemoteMemory);
    }

    fn refresh_threads_with_memory(
        &mut self,
        tasks: BTreeMap<Pid, bool>,
        read: &mut impl MemoryReader,
    ) {
        let mut exited = BTreeSet::new();
        for (tid, state) in &self.threads {
            if !tasks.contains_key(tid) {
                exited.insert(tid.as_raw() as u32);
                if let Some(id) = state.notification() {
                    self.futex_mgr.remove_by_notif_id(id);
                }
            }
        }
        for tid in self.threads.keys().filter(|tid| !tasks.contains_key(tid)) {
            self.scheduler.remove_thread(*tid);
        }
        self.threads.retain(|tid, _| tasks.contains_key(tid));
        for (tid, runnable) in tasks {
            self.register_thread(tid);
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

        let woken = self.futex_mgr.waiters_for_each(|waiter| {
            // clear_child_tid is an implicit kernel wake, so no FUTEX_WAKE
            // notification will arrive. A nonzero expected word becoming zero
            // also covers workers that exited before we first observed them.
            exited.contains(&waiter.expected_val)
                || (waiter.expected_val != 0 && read.read_u32(waiter.tid, waiter.uaddr) == Ok(0))
        });
        for waiter in woken {
            self.wake_futex_waiter(waiter);
        }
    }

    fn wake_futex_waiter(&mut self, waiter: FutexWaiter) {
        self.wheel.retain(|event| event.notif_id != waiter.notif_id);
        if self.threads.get(&waiter.tid) == Some(&ThreadState::FutexWaiting(waiter.notif_id)) {
            self.threads.insert(waiter.tid, ThreadState::Runnable);
            self.pending_responses.push_back(seccomp_notif_resp {
                id: waiter.notif_id,
                ..Default::default()
            });
        }
    }

    fn cancel_invalid_notifications(&mut self, mut valid: impl FnMut(u64) -> bool) {
        for state in self.threads.values_mut() {
            if let Some(id) = state.notification()
                && !valid(id)
            {
                self.futex_mgr.remove_by_notif_id(id);
                self.wheel.retain(|event| event.notif_id != id);
                *state = ThreadState::Runnable;
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
        let name = match i64::from(notif.data.nr) {
            libc::SYS_read => "read",
            libc::SYS_write => "write",
            libc::SYS_connect => "connect",
            libc::SYS_accept | libc::SYS_accept4 => "accept",
            libc::SYS_sendto => "sendto",
            libc::SYS_recvfrom => "recvfrom",
            libc::SYS_sendmsg => "sendmsg",
            libc::SYS_recvmsg => "recvmsg",
            libc::SYS_sched_yield => "sched_yield",
            libc::SYS_futex => "futex",
            libc::SYS_getrandom => "getrandom",
            libc::SYS_clock_gettime => "clock_gettime",
            libc::SYS_clock_nanosleep => "clock_nanosleep",
            libc::SYS_nanosleep => "nanosleep",
            libc::SYS_gettimeofday => "gettimeofday",
            libc::SYS_epoll_wait => "epoll_wait",
            libc::SYS_epoll_pwait => "epoll_pwait",
            libc::SYS_poll => "poll",
            libc::SYS_ppoll => "ppoll",
            _ => "other",
        };
        self.telemetry.record_syscall(name);
        // pid is the notifying thread ID, which can differ from the launched PID.
        let pid = Pid::from_raw(notif.pid as i32);
        let args = notif.data.args;
        if IO_SYSCALLS.contains(&(notif.data.nr as u32))
            && let Some(err) =
                self.fault_config
                    .should_fault(i64::from(notif.data.nr), &args, &mut self.rng)
        {
            self.telemetry.record_fault(name);
            return Ok(Some(seccomp_notif_resp {
                id: notif.id,
                val: 0,
                error: err,
                flags: 0,
            }));
        }
        let mut response = seccomp_notif_resp {
            id: notif.id,
            ..Default::default()
        };
        match i64::from(notif.data.nr) {
            libc::SYS_epoll_wait | libc::SYS_epoll_pwait | libc::SYS_poll => {
                let timeout_arg = if i64::from(notif.data.nr) == libc::SYS_poll {
                    2
                } else {
                    3
                };
                let timeout_ms = args[timeout_arg] as i32;
                if timeout_ms <= 0 {
                    response.flags = SECCOMP_USER_NOTIF_FLAG_CONTINUE;
                } else {
                    self.defer_timeout(notif, timeout_ms as u64 * 1_000_000)?;
                    return Ok(None);
                }
            }
            libc::SYS_ppoll => {
                if args[2] == 0 {
                    response.flags = SECCOMP_USER_NOTIF_FLAG_CONTINUE;
                } else {
                    let bytes = read.read_bytes(
                        pid,
                        args[2] as usize,
                        std::mem::size_of::<libc::timespec>(),
                    )?;
                    let timeout_ns = sleep_duration_ns(&decode_timespec(&bytes)?)?;
                    self.defer_timeout(notif, timeout_ns)?;
                    return Ok(None);
                }
            }
            libc::SYS_sched_yield => {
                self.threads.insert(pid, ThreadState::Yielded);
                self.select_next_thread();
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
                self.defer_timeout(notif, sleep_ns)?;
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

    fn defer_timeout(&mut self, notif: &seccomp_notif, timeout_ns: u64) -> nix::Result<()> {
        let wake_time_ns = self
            .clock
            .now_ns()
            .checked_add(timeout_ns)
            .ok_or(Errno::EINVAL)?;
        let tid = Pid::from_raw(notif.pid as i32);
        self.wheel.add_timer(TimerEvent {
            wake_time_ns,
            notif_id: notif.id,
            tid,
        });
        self.threads.insert(tid, ThreadState::Sleeping(notif.id));
        // The event loop drains queued syscalls and refreshes live task states
        // before warping to the earliest deadline when no threads are runnable.
        Ok(())
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
        ) && !uaddr.is_multiple_of(std::mem::align_of::<u32>())
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
                        uaddr,
                        expected_val: val,
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
                    self.wake_futex_waiter(waiter);
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
    // A ptrace stop is work pending in the supervisor, not an idle sleeper.
    // Keep it runnable so a timer warp cannot skip an unprocessed TSC fault.
    Ok(fields.starts_with('R') || fields.starts_with('t'))
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
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    || error.raw_os_error() == Some(libc::ESRCH) => {} // thread exited
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

fn handle_signal_stop(
    state: &mut State,
    tid: Pid,
    signal: Signal,
    initial_stop: bool,
    emulate_tsc: impl FnOnce(Pid, u64) -> Result<bool>,
    resume: impl FnOnce(Pid, Option<Signal>) -> nix::Result<()>,
) -> Result<()> {
    let deliver = if signal == Signal::SIGSEGV {
        anyhow::ensure!(
            emulate_tsc(tid, state.clock.now_ns())?,
            "thread {tid} received a genuine SIGSEGV (instruction is not RDTSC/RDTSCP)"
        );
        // Clock reads retain their 100 ns tick. Timestamp faults cost 50 ns.
        state
            .clock
            .advance_to(state.clock.now_ns().saturating_add(50));
        state.threads.insert(tid, ThreadState::Runnable);
        None
    } else if initial_stop && signal == Signal::SIGSTOP {
        // Auto-attached clone/fork children start with a synthetic SIGSTOP.
        None
    } else {
        Some(signal)
    };
    resume(tid, deliver).context("resuming ptrace signal stop")
}

fn drain_wait_events(child: Pid, state: &mut State) -> Result<Option<i32>> {
    // Bound each drain so a tight RDTSC loop cannot starve seccomp responses.
    for _ in 0..64 {
        let mut progress = false;
        // Wait only for this runtime's tasks; waitpid(-1) could steal unrelated
        // children, including children owned by concurrent library tests.
        for tid in state.tracees.iter().copied().collect::<Vec<_>>() {
            let status = match waitpid(tid, Some(WaitPidFlag::WNOHANG | WaitPidFlag::__WALL)) {
                Ok(WaitStatus::StillAlive) => continue,
                Ok(status) => status,
                Err(Errno::EINTR) => {
                    progress = true;
                    continue;
                }
                Err(Errno::ECHILD | Errno::ESRCH) if tid != child => {
                    state.tracees.remove(&tid);
                    state.new_tracees.remove(&tid);
                    continue;
                }
                Err(error) => return Err(error).context("waiting for supervised task"),
            };
            progress = true;
            match status {
                WaitStatus::Exited(pid, code) => {
                    state.tracees.remove(&pid);
                    state.new_tracees.remove(&pid);
                    if pid == child {
                        return Ok(Some(code));
                    }
                }
                WaitStatus::Signaled(pid, signal, _) => {
                    state.tracees.remove(&pid);
                    state.new_tracees.remove(&pid);
                    if pid == child {
                        return Ok(Some(128 + signal as i32));
                    }
                }
                WaitStatus::Stopped(pid, signal) => {
                    let initial_stop = state.new_tracees.remove(&pid);
                    handle_signal_stop(
                        state,
                        pid,
                        signal,
                        initial_stop,
                        tsc::handle_tsc_fault,
                        ptrace::cont,
                    )?;
                }
                WaitStatus::PtraceEvent(pid, _, event) => {
                    if matches!(
                        event,
                        libc::PTRACE_EVENT_CLONE
                            | libc::PTRACE_EVENT_FORK
                            | libc::PTRACE_EVENT_VFORK
                    ) {
                        let new_tid = i32::try_from(ptrace::getevent(pid)?)
                            .context("ptrace child TID does not fit pid_t")?;
                        let new_tid = Pid::from_raw(new_tid);
                        state.tracees.insert(new_tid);
                        state.new_tracees.insert(new_tid);
                    }
                    // Exec events and clone events are synthetic traps, not
                    // application SIGTRAP signals, so do not reinject SIGTRAP.
                    ptrace::cont(pid, None).context("resuming ptrace event")?;
                }
                WaitStatus::PtraceSyscall(pid) => ptrace::cont(pid, None)?,
                _ => (),
            }
        }
        if !progress {
            return Ok(None);
        }
    }
    Ok(None)
}

fn kill_and_reap(child: Pid, state: &mut State) {
    // Include tasks whose clone event may not yet have been drained.
    if let Ok(tasks) = thread_snapshot(child) {
        state.tracees.extend(tasks.keys().copied());
    }
    state.tracees.insert(child);
    for tid in &state.tracees {
        let _ = kill(*tid, Signal::SIGKILL);
    }
    // Reap traced workers before the leader, which can wait for their deaths.
    let mut tids: Vec<_> = state
        .tracees
        .iter()
        .copied()
        .filter(|tid| *tid != child)
        .collect();
    tids.push(child);
    for tid in tids {
        loop {
            match waitpid(tid, Some(WaitPidFlag::__WALL)) {
                Ok(WaitStatus::Exited(..) | WaitStatus::Signaled(..))
                | Err(Errno::ECHILD | Errno::ESRCH) => break,
                Err(Errno::EINTR) => continue,
                Err(_) => break,
                Ok(_) => {
                    let _ = ptrace::cont(tid, Some(Signal::SIGKILL));
                }
            }
        }
    }
    state.tracees.clear();
    state.new_tracees.clear();
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

fn supervise_loop(
    child: Pid,
    notif_fd: &OwnedFd,
    state: &mut State,
    mut emulate: impl FnMut(&mut State, &seccomp_notif) -> nix::Result<Option<seccomp_notif_resp>>,
) -> Result<i32> {
    let fd = notif_fd.as_raw_fd();
    state.tracees.insert(child);
    loop {
        if let Some(code) = drain_wait_events(child, state)? {
            return Ok(code);
        }

        state.cancel_invalid_notifications(|id| is_id_valid(fd, id));
        wake_expired(state, fd)?;
        // First drain pending notifications, including those from previously
        // unseen threads, before considering an idle-time warp.
        let mut revents = poll_listener(fd, 0)?;
        if revents == 0 && (!state.wheel.is_empty() || !state.futex_mgr.is_empty()) {
            match thread_snapshot(child) {
                Ok(tasks) => state.refresh_threads(tasks),
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error).context("reading child thread states"),
            }
            // A thread can fault while the task snapshot is being collected.
            if let Some(code) = drain_wait_events(child, state)? {
                return Ok(code);
            }
            state.cancel_invalid_notifications(|id| is_id_valid(fd, id));
            wake_expired(state, fd)?;
            // Exit wakes must be delivered even for joins with no timeout.
            flush_responses(state, fd)?;
            // Task states and notification readiness are sampled separately.
            // Recheck readiness after the snapshot to avoid skipping a request.
            revents = poll_listener(fd, 0)?;
            if revents == 0 && state.warp_if_idle() {
                wake_expired(state, fd)?;
                continue;
            }
        }
        if revents == 0 {
            // Bound exit detection and refresh task states promptly for virtual
            // timer warps and implicit kernel futex wakes on thread exit.
            // Ptrace signal stops need service even without a seccomp request.
            revents = poll_listener(fd, 1)?;
        }
        if revents == 0 {
            continue;
        }
        if revents & libc::POLLHUP != 0 {
            // Drain final ptrace events and exits on the next iteration.
            continue;
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
pub fn run_supervisor(child: Pid, notif_fd: OwnedFd, seed: u64, quantum: u64) -> Result<i32> {
    run_supervisor_with_telemetry(
        child,
        notif_fd,
        seed,
        quantum,
        &mut ExecutionTelemetry::default(),
    )
}

/// Run the supervisor and return execution statistics, including on errors.
pub fn run_supervisor_with_telemetry(
    child: Pid,
    notif_fd: OwnedFd,
    seed: u64,
    quantum: u64,
    telemetry: &mut ExecutionTelemetry,
) -> Result<i32> {
    run_supervisor_with_config(
        child,
        notif_fd,
        SupervisorConfig {
            seed,
            quantum,
            fault_config: FaultConfig::default(),
        },
        telemetry,
    )
}

pub fn run_supervisor_with_config(
    child: Pid,
    notif_fd: OwnedFd,
    config: SupervisorConfig,
    telemetry: &mut ExecutionTelemetry,
) -> Result<i32> {
    let mut state = State::new(config.seed, config.quantum, config.fault_config);
    state.telemetry = std::mem::take(telemetry);
    if state.telemetry.real_start.is_none() {
        state.telemetry.real_start = Some(std::time::Instant::now());
    }
    state.telemetry.virtual_start_ns = state.clock.now_ns();
    let result = supervise_loop(child, &notif_fd, &mut state, State::emulate);
    if result.is_err() {
        kill_and_reap(child, &mut state);
    }
    state.telemetry.exit_code = result.as_ref().ok().copied();
    state.telemetry.finish(state.clock.now_ns());
    *telemetry = state.telemetry;
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn io_faults_are_returned_and_counted() {
        use crate::interceptor::FaultErrno;
        let config = FaultConfig {
            fault_rate: 1.0,
            fault_types: vec![FaultErrno::Eio],
        };
        let mut state = State::new(3, 10_000, config);
        let notif = notification(libc::SYS_read, [3, 0, 0, 0, 0, 0]);
        let response = state.emulate(&notif).unwrap().unwrap();
        assert_eq!(response.error, -libc::EIO);
        assert_eq!(response.flags, 0);
        assert_eq!(state.telemetry.fault_counts["read"], 1);
        state.fault_config.fault_rate = 0.0;
        let response = state.emulate(&notif).unwrap().unwrap();
        assert_eq!(response.flags, SECCOMP_USER_NOTIF_FLAG_CONTINUE);
        assert_eq!(state.telemetry.syscall_counts["read"], 2);
    }

    #[test]
    fn io_fault_configuration_preserves_standard_streams_and_non_io_syscalls() {
        let mut state = State::new(
            3,
            10_000,
            FaultConfig {
                fault_rate: 1.0,
                fault_types: vec![crate::interceptor::FaultErrno::Eio],
            },
        );
        for nr in [libc::SYS_read, libc::SYS_write] {
            for fd in 0..=2 {
                let response = state
                    .emulate(&notification(nr, [fd, 0, 0, 0, 0, 0]))
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    (response.flags, response.error),
                    (SECCOMP_USER_NOTIF_FLAG_CONTINUE, 0)
                );
            }
        }
        let response = state
            .emulate(&notification(libc::SYS_gettimeofday, [0; 6]))
            .unwrap()
            .unwrap();
        assert_eq!((response.val, response.error, response.flags), (0, 0, 0));
        assert_eq!(state.clock.now_ns(), 100);
        assert!(state.telemetry.fault_counts.is_empty());
    }

    #[test]
    fn test_deterministic_fault_injection() {
        use rand::{Rng, SeedableRng};
        use rand_chacha::ChaCha20Rng;

        #[derive(Clone, Copy)]
        struct FaultConfig {
            fault_rate: f64,
            seed: u64,
        }

        fn evaluate(config: FaultConfig) -> Vec<bool> {
            let mut rng = ChaCha20Rng::seed_from_u64(config.seed);
            (0..100).map(|_| rng.gen_bool(config.fault_rate)).collect()
        }

        let first = FaultConfig {
            fault_rate: 0.3,
            seed: 0xCAFEBABE,
        };
        let same = FaultConfig {
            fault_rate: 0.3,
            seed: 0xCAFEBABE,
        };
        let different = FaultConfig {
            fault_rate: 0.3,
            seed: 0xDEADBEEF,
        };
        let decisions = evaluate(first);
        assert_eq!(decisions, evaluate(same));
        assert_ne!(decisions, evaluate(different));
        assert!(
            evaluate(FaultConfig {
                fault_rate: 0.0,
                seed: first.seed
            })
            .iter()
            .all(|fault| !fault)
        );
    }

    use super::*;
    use std::path::Path;

    fn millisecond_wait(nr: i64, timeout_ms: i32) -> seccomp_notif {
        let mut args = [0; 6];
        // Give the other possible timeout argument a conflicting value.
        args[2] = u64::MAX;
        args[3] = u64::MAX;
        args[if nr == libc::SYS_poll { 2 } else { 3 }] = timeout_ms as u32 as u64;
        notification(nr, args)
    }

    #[test]
    fn finite_event_waits_use_millisecond_deadlines_and_timeout_successfully() {
        for (nr, name) in [
            (libc::SYS_poll, "poll"),
            (libc::SYS_epoll_wait, "epoll_wait"),
            (libc::SYS_epoll_pwait, "epoll_pwait"),
        ] {
            for timeout_ms in [1, 37, i32::MAX] {
                let mut state = State::new(0, 10_000, FaultConfig::default());
                state.clock.advance_to(123);
                let notif = millisecond_wait(nr, timeout_ms);
                let tid = Pid::from_raw(notif.pid as i32);
                state.begin_notification(&notif);
                assert!(
                    state
                        .emulate_with_reader(&notif, |_, _, _| panic!("unexpected read"))
                        .unwrap()
                        .is_none()
                );
                let deadline = 123 + timeout_ms as u64 * 1_000_000;
                assert_eq!(state.clock.now_ns(), 123);
                assert_eq!(state.wheel.earliest_wake_time(), Some(deadline));
                assert_eq!(state.threads[&tid], ThreadState::Sleeping(notif.id));
                assert!(state.warp_if_idle());
                assert_eq!(state.clock.now_ns(), deadline);
                assert_eq!(state.take_expired().len(), 1);
                let response = state.pending_responses.pop_front().unwrap();
                assert_eq!(
                    (response.id, response.val, response.error, response.flags),
                    (notif.id, 0, 0, 0)
                );
                assert_eq!(state.threads[&tid], ThreadState::Runnable);
                assert!(state.take_expired().is_empty());
                assert!(state.pending_responses.is_empty());
                assert_eq!(state.telemetry.syscall_counts[name], 1);
                assert!(!state.telemetry.syscall_counts.contains_key("other"));
            }
        }
    }

    #[test]
    fn zero_and_infinite_event_waits_continue_in_kernel() {
        for (nr, name) in [
            (libc::SYS_poll, "poll"),
            (libc::SYS_epoll_wait, "epoll_wait"),
            (libc::SYS_epoll_pwait, "epoll_pwait"),
        ] {
            let mut state = State::new(0, 10_000, FaultConfig::default());
            for timeout in [0, -1, i32::MIN] {
                let notif = millisecond_wait(nr, timeout);
                let response = state
                    .emulate_with_reader(&notif, |_, _, _| panic!("unexpected read"))
                    .unwrap()
                    .unwrap();
                assert_eq!(response.flags, SECCOMP_USER_NOTIF_FLAG_CONTINUE);
                assert_eq!((response.val, response.error), (0, 0));
                assert!(state.wheel.is_empty());
                assert_eq!(state.clock.now_ns(), 0);
            }
            assert_eq!(state.telemetry.syscall_counts[name], 3);
        }
    }

    #[test]
    fn event_wait_timeout_is_a_signed_32_bit_argument() {
        let mut state = State::new(0, 10_000, FaultConfig::default());
        let mut notif = millisecond_wait(libc::SYS_poll, 0);
        notif.data.args[2] = 0xffff_ffff_0000_0002;
        assert!(state.emulate(&notif).unwrap().is_none());
        assert_eq!(state.wheel.earliest_wake_time(), Some(2_000_000));
    }

    #[test]
    fn ppoll_reads_timespec_or_continues_for_a_null_timeout() {
        let mut state = State::new(0, 10_000, FaultConfig::default());
        let null = notification(libc::SYS_ppoll, [0; 6]);
        let response = state
            .emulate_with_reader(&null, |_, _, _| panic!("NULL timeout"))
            .unwrap()
            .unwrap();
        assert_eq!(response.flags, SECCOMP_USER_NOTIF_FLAG_CONTINUE);
        assert!(state.wheel.is_empty());
        state.clock.advance_to(42);
        let notif = notification(libc::SYS_ppoll, [0, 0, 0x2000, 0x3000, 8, 0]);
        state.begin_notification(&notif);
        assert!(
            state
                .emulate_with_reader(&notif, |pid, address, len| {
                    assert_eq!((pid.as_raw() as u32, address, len), (notif.pid, 0x2000, 16));
                    Ok(request_bytes(2, 345))
                })
                .unwrap()
                .is_none()
        );
        assert_eq!(state.wheel.earliest_wake_time(), Some(2_000_000_387));
        assert!(state.warp_if_idle());
        state.take_expired();
        let response = state.pending_responses.pop_front().unwrap();
        assert_eq!((response.val, response.error, response.flags), (0, 0, 0));
        assert_eq!(state.telemetry.syscall_counts["ppoll"], 2);
    }

    #[test]
    fn zero_ppoll_timeout_expires_without_advancing_time() {
        let mut state = State::new(0, 10_000, FaultConfig::default());
        state.clock.advance_to(42);
        let notif = notification(libc::SYS_ppoll, [0, 0, 0x2000, 0, 0, 0]);
        state.begin_notification(&notif);
        assert!(
            state
                .emulate_with_reader(&notif, |_, _, _| Ok(request_bytes(0, 0)))
                .unwrap()
                .is_none()
        );
        assert_eq!(state.take_expired().len(), 1);
        let response = state.pending_responses.pop_front().unwrap();
        assert_eq!(
            (response.id, response.val, response.error),
            (notif.id, 0, 0)
        );
        assert_eq!(state.clock.now_ns(), 42);
    }

    #[test]
    fn event_waits_validate_timespec_and_deadline_overflow_before_enqueuing() {
        let mut state = State::new(0, 10_000, FaultConfig::default());
        let ppoll = notification(libc::SYS_ppoll, [0, 0, 0x2000, 0, 0, 0]);
        state.begin_notification(&ppoll);
        for (seconds, nanos) in [(-1, 0), (0, -1), (0, 1_000_000_000), (i64::MAX, 0)] {
            assert_eq!(
                state
                    .emulate_with_reader(&ppoll, |_, _, _| Ok(request_bytes(seconds, nanos)))
                    .unwrap_err(),
                Errno::EINVAL
            );
        }
        assert_eq!(
            state
                .emulate_with_reader(&ppoll, |_, _, _| Err(Errno::EFAULT))
                .unwrap_err(),
            Errno::EFAULT
        );
        assert_eq!(
            state
                .emulate_with_reader(&ppoll, |_, _, _| Ok(vec![0; 8]))
                .unwrap_err(),
            Errno::EFAULT
        );
        state.clock.advance_to(u64::MAX);
        assert_eq!(
            state
                .emulate_with_reader(&ppoll, |_, _, _| Ok(request_bytes(0, 1)))
                .unwrap_err(),
            Errno::EINVAL
        );
        for nr in [libc::SYS_poll, libc::SYS_epoll_wait, libc::SYS_epoll_pwait] {
            assert_eq!(
                state.emulate(&millisecond_wait(nr, 1)).unwrap_err(),
                Errno::EINVAL
            );
        }
        assert!(state.wheel.is_empty());
        assert!(state.pending_responses.is_empty());
        assert_eq!(state.threads[&Pid::this()], ThreadState::Runnable);
    }

    #[test]
    fn event_waits_defer_warp_until_idle_and_wake_by_deadline() {
        let mut state = State::new(0, 10_000, FaultConfig::default());
        state.refresh_threads(BTreeMap::from([
            (Pid::from_raw(1), true),
            (Pid::from_raw(2), true),
        ]));
        let mut poll = millisecond_wait(libc::SYS_poll, 20);
        poll.pid = 1;
        poll.id = 1;
        state.begin_notification(&poll);
        state.emulate(&poll).unwrap();
        assert!(!state.warp_if_idle());
        assert_eq!(state.clock.now_ns(), 0);
        let mut epoll = millisecond_wait(libc::SYS_epoll_wait, 10);
        epoll.pid = 2;
        epoll.id = 2;
        state.begin_notification(&epoll);
        state.emulate(&epoll).unwrap();
        assert!(state.warp_if_idle());
        assert_eq!(state.clock.now_ns(), 10_000_000);
        assert_eq!(state.take_expired()[0].notif_id, 2);
        assert!(!state.warp_if_idle()); // the newly awakened thread can run
        state.refresh_threads(BTreeMap::from([
            (Pid::from_raw(1), false),
            (Pid::from_raw(2), false),
        ]));
        assert!(state.warp_if_idle());
        assert_eq!(state.clock.now_ns(), 20_000_000);
        assert_eq!(state.take_expired()[0].notif_id, 1);
        assert_eq!(
            state
                .pending_responses
                .iter()
                .map(|resp| resp.id)
                .collect::<Vec<_>>(),
            [2, 1]
        );
    }

    #[test]
    fn canceled_event_waits_do_not_receive_timeout_responses() {
        for nr in [
            libc::SYS_poll,
            libc::SYS_epoll_wait,
            libc::SYS_epoll_pwait,
            libc::SYS_ppoll,
        ] {
            let mut state = State::new(0, 10_000, FaultConfig::default());
            let notif = if nr == libc::SYS_ppoll {
                notification(nr, [0, 0, 0x2000, 0, 0, 0])
            } else {
                millisecond_wait(nr, 1000)
            };
            state.begin_notification(&notif);
            assert!(
                state
                    .emulate_with_reader(&notif, |_, _, _| Ok(request_bytes(1, 0)))
                    .unwrap()
                    .is_none()
            );
            state.cancel_invalid_notifications(|_| false);
            state.clock.advance_to(2_000_000_000);
            assert!(state.take_expired().is_empty());
            assert!(state.pending_responses.is_empty());
        }
    }

    #[test]
    fn timestamp_faults_advance_fifty_ns_and_resume_without_sigsegv() {
        let mut state = State::new(0, 10_000, FaultConfig::default());
        let tid = Pid::from_raw(123);
        for expected_cycles in [0, 50] {
            handle_signal_stop(
                &mut state,
                tid,
                Signal::SIGSEGV,
                false,
                |pid, cycles| {
                    assert_eq!((pid, cycles), (tid, expected_cycles));
                    Ok(true)
                },
                |pid, signal| {
                    assert_eq!((pid, signal), (tid, None));
                    Ok(())
                },
            )
            .unwrap();
        }
        assert_eq!(state.clock.now_ns(), 100);
        assert_eq!(state.threads[&tid], ThreadState::Runnable);
        assert_eq!(state.clock.advance_tick(), 200); // normal clock reads keep their tick
    }

    #[test]
    fn real_sigsegv_is_an_error_and_is_not_resumed() {
        let mut state = State::new(0, 10_000, FaultConfig::default());
        let error = handle_signal_stop(
            &mut state,
            Pid::from_raw(123),
            Signal::SIGSEGV,
            false,
            |_, _| Ok(false),
            |_, _| panic!("must not resume a genuine fault"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("genuine SIGSEGV"));
        assert_eq!(state.clock.now_ns(), 0);
    }

    #[test]
    fn tsc_inspection_and_resume_errors_propagate() {
        let mut state = State::new(0, 10_000, FaultConfig::default());
        let error = handle_signal_stop(
            &mut state,
            Pid::from_raw(123),
            Signal::SIGSEGV,
            false,
            |_, _| Err(Errno::EFAULT.into()),
            |_, _| panic!("inspection failed"),
        )
        .unwrap_err();
        assert_eq!(error.downcast_ref::<Errno>(), Some(&Errno::EFAULT));
        assert_eq!(state.clock.now_ns(), 0);
        let error = handle_signal_stop(
            &mut state,
            Pid::from_raw(123),
            Signal::SIGSEGV,
            false,
            |_, _| Ok(true),
            |_, _| Err(Errno::ESRCH),
        )
        .unwrap_err();
        assert_eq!(error.downcast_ref::<Errno>(), Some(&Errno::ESRCH));
    }

    #[test]
    fn forwards_signals_but_suppresses_initial_clone_stop() {
        let mut state = State::new(0, 10_000, FaultConfig::default());
        for (signal, initial, expected) in [
            (Signal::SIGSTOP, true, None),
            (Signal::SIGTERM, false, Some(Signal::SIGTERM)),
            (Signal::SIGTRAP, false, Some(Signal::SIGTRAP)),
        ] {
            handle_signal_stop(
                &mut state,
                Pid::from_raw(123),
                signal,
                initial,
                |_, _| panic!("not a timestamp fault"),
                |_, delivered| {
                    assert_eq!(delivered, expected);
                    Ok(())
                },
            )
            .unwrap();
        }
        assert_eq!(state.clock.now_ns(), 0);
    }

    #[test]
    fn timestamp_fault_at_end_of_timeline_does_not_wrap() {
        let mut state = State::new(0, 10_000, FaultConfig::default());
        state.clock.advance_to(u64::MAX - 25);
        handle_signal_stop(
            &mut state,
            Pid::from_raw(123),
            Signal::SIGSEGV,
            false,
            |_, cycles| {
                assert_eq!(cycles, u64::MAX - 25);
                Ok(true)
            },
            |_, _| Ok(()),
        )
        .unwrap();
        assert_eq!(state.clock.now_ns(), u64::MAX);
    }

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
    fn exiting_worker_wakes_join_and_cancels_any_timeout() {
        for timeout in [0, 0x2000] {
            let mut state = State::new(0, 10_000, FaultConfig::default());
            let join = futex_notification(
                1,
                101,
                [
                    0x1000,
                    (libc::FUTEX_WAIT_BITSET | libc::FUTEX_CLOCK_REALTIME) as u64,
                    7,
                    timeout,
                    0,
                    u32::MAX as u64,
                ],
            );
            assert!(
                emulate_futex_request(&mut state, &join, 7, (10, 0))
                    .unwrap()
                    .is_none()
            );
            state
                .threads
                .insert(Pid::from_raw(7), ThreadState::Runnable);
            state.refresh_threads_with_memory(
                BTreeMap::from([(Pid::from_raw(1), false)]),
                &mut |_, _, _| -> nix::Result<Vec<u8>> {
                    panic!("known exited TID needs no memory read")
                },
            );
            assert_eq!(state.threads[&Pid::from_raw(1)], ThreadState::Runnable);
            assert!(!state.threads.contains_key(&Pid::from_raw(7)));
            assert!(state.futex_mgr.is_empty());
            assert!(state.wheel.is_empty());
            let reply = state.pending_responses.pop_front().unwrap();
            assert_eq!(
                (reply.id, reply.val, reply.error, reply.flags),
                (101, 0, 0, 0)
            );
            state.clock.advance_to(20_000_000_000);
            assert!(state.take_expired().is_empty());
            state.refresh_threads_with_memory(
                BTreeMap::from([(Pid::from_raw(1), true)]),
                &mut |_, _, _| -> nix::Result<Vec<u8>> { panic!("wait was already removed") },
            );
            assert!(state.pending_responses.is_empty());
        }
    }

    #[test]
    fn cleared_tid_wakes_even_if_worker_exited_before_discovery() {
        let mut state = State::new(0, 10_000, FaultConfig::default());
        let join = futex_notification(1, 101, [0x1000, libc::FUTEX_WAIT as u64, 7, 0, 0, 0]);
        emulate_futex_request(&mut state, &join, 7, (0, 0)).unwrap();
        state.refresh_threads_with_memory(
            BTreeMap::from([(Pid::from_raw(1), false)]),
            &mut |pid, address, len| {
                assert_eq!((pid, address, len), (Pid::from_raw(1), 0x1000, 4));
                Ok(0_u32.to_ne_bytes().to_vec())
            },
        );
        assert!(state.futex_mgr.is_empty());
        assert_eq!(state.pending_responses.pop_front().unwrap().id, 101);
    }

    #[test]
    fn live_or_unreadable_futex_words_remain_waiting() {
        let mut state = State::new(0, 10_000, FaultConfig::default());
        let join = futex_notification(1, 101, [0x1000, libc::FUTEX_WAIT as u64, 7, 0, 0, 0]);
        emulate_futex_request(&mut state, &join, 7, (0, 0)).unwrap();
        let tasks = BTreeMap::from([(Pid::from_raw(1), false)]);
        state.refresh_threads_with_memory(tasks.clone(), &mut |_, _, _| {
            Ok(7_u32.to_ne_bytes().to_vec())
        });
        state.refresh_threads_with_memory(tasks, &mut |_, _, _| Err(Errno::EFAULT));
        assert_eq!(
            state.threads[&Pid::from_raw(1)],
            ThreadState::FutexWaiting(101)
        );
        assert!(!state.futex_mgr.is_empty());
        assert!(state.pending_responses.is_empty());
    }

    #[test]
    fn unrelated_exit_does_not_wake_an_unchanged_zero_wait() {
        let mut state = State::new(0, 10_000, FaultConfig::default());
        let wait = futex_notification(1, 101, [0x1000, libc::FUTEX_WAIT as u64, 0, 0, 0, 0]);
        emulate_futex_request(&mut state, &wait, 0, (0, 0)).unwrap();
        state
            .threads
            .insert(Pid::from_raw(7), ThreadState::Runnable);
        state.refresh_threads_with_memory(
            BTreeMap::from([(Pid::from_raw(1), false)]),
            &mut |_, _, _| Ok(0_u32.to_ne_bytes().to_vec()),
        );
        assert_eq!(
            state.threads[&Pid::from_raw(1)],
            ThreadState::FutexWaiting(101)
        );
        assert!(!state.futex_mgr.is_empty());
        assert!(state.pending_responses.is_empty());
    }

    #[test]
    fn dead_waiter_is_removed_without_sending_a_response() {
        let mut state = State::new(0, 10_000, FaultConfig::default());
        let wait = futex_notification(1, 101, [0x1000, libc::FUTEX_WAIT as u64, 7, 0x2000, 0, 0]);
        emulate_futex_request(&mut state, &wait, 7, (10, 0)).unwrap();
        state.refresh_threads_with_memory(BTreeMap::new(), &mut |_,
                                                                 _,
                                                                 _|
         -> nix::Result<Vec<u8>> {
            panic!("dead waiter's memory must not be read")
        });
        assert!(state.futex_mgr.is_empty());
        assert!(state.wheel.is_empty());
        assert!(state.pending_responses.is_empty());
    }

    #[test]
    fn real_clear_child_tid_exit_releases_an_untimed_join() {
        use nix::{
            sys::socket::{AddressFamily, SockFlag, SockType, socketpair},
            unistd::{ForkResult, fork},
        };

        extern "C" fn worker(_: *mut libc::c_void) -> libc::c_int {
            // SAFETY: no pointers or inherited Rust runtime state are accessed.
            // SYS_exit terminates just this thread, triggering clear_child_tid.
            unsafe {
                libc::syscall(libc::SYS_sched_yield);
                libc::syscall(libc::SYS_exit, 0);
            }
            0
        }

        let mut stack = [0_u128; 4096]; // 64 KiB, aligned for the x86_64 ABI
        let mut child_tid = 0_i32;
        let tid_address = std::ptr::from_mut(&mut child_tid) as usize;
        let (_filters, program) = crate::interceptor::create_filter_prog();
        let (parent_socket, child_socket) = socketpair(
            AddressFamily::Unix,
            SockType::SeqPacket,
            None,
            SockFlag::SOCK_CLOEXEC,
        )
        .unwrap();
        // SAFETY: the child only uses stack data, syscalls, and fd passing;
        // neither it nor its clone returns to the test harness.
        let child = match unsafe { fork() }.unwrap() {
            ForkResult::Child => unsafe {
                libc::close(parent_socket.as_raw_fd());
                libc::alarm(5); // regressions must fail instead of hanging the suite
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
                let flags = libc::CLONE_VM
                    | libc::CLONE_FS
                    | libc::CLONE_FILES
                    | libc::CLONE_SIGHAND
                    | libc::CLONE_THREAD
                    | libc::CLONE_SYSVSEM
                    | libc::CLONE_PARENT_SETTID
                    | libc::CLONE_CHILD_CLEARTID;
                let tid = libc::clone(
                    worker,
                    stack.as_mut_ptr().add(stack.len()).cast(),
                    flags,
                    std::ptr::null_mut(),
                    tid_address as *mut libc::pid_t,
                    std::ptr::null_mut::<libc::c_void>(),
                    tid_address as *mut libc::pid_t,
                );
                if tid < 0 {
                    libc::_exit(3);
                }
                if libc::syscall(
                    libc::SYS_futex,
                    &child_tid,
                    libc::FUTEX_WAIT_BITSET | libc::FUTEX_CLOCK_REALTIME,
                    tid,
                    0_usize,
                    0_usize,
                    u32::MAX,
                ) != 0
                {
                    libc::_exit(4);
                }
                // The worker must have actually exited and had its TID cleared.
                if std::ptr::read_volatile(&child_tid) != 0 {
                    libc::_exit(5);
                }
                libc::_exit(0);
            },
            ForkResult::Parent { child } => child,
        };
        drop(child_socket);
        let listener = crate::launcher::recv_fd(parent_socket.as_raw_fd());
        if listener.is_err() {
            let _ = waitpid(child, None);
        }
        let listener = listener.expect("child must transfer its seccomp listener");
        let mut state = State::new(0, 10_000, FaultConfig::default());
        let mut joins = 0;
        let result = supervise_loop(child, &listener, &mut state, |state, notif| {
            if i64::from(notif.data.nr) == libc::SYS_futex {
                joins += 1;
                // Supply the known comparison value for the initial enqueue;
                // the real exiting worker and task snapshots drive the wake.
                // This keeps the regression executable when process_vm is denied.
                state.emulate_with_reader(notif, |pid, address, len| {
                    if pid == child && address == tid_address && len == 4 {
                        Ok((notif.data.args[2] as u32).to_ne_bytes().to_vec())
                    } else {
                        Err(Errno::EFAULT)
                    }
                })
            } else {
                state.emulate(notif)
            }
        });
        if result.is_err() {
            let _ = kill(child, Signal::SIGKILL);
            let _ = waitpid(child, None);
        }
        assert_eq!(result.unwrap(), 0);
        assert_eq!(joins, 1);
        assert_eq!(state.clock.now_ns(), 100); // only the worker's sched_yield
        assert!(state.futex_mgr.is_empty());
        assert!(state.wheel.is_empty());
        assert!(state.pending_responses.is_empty());
        assert_eq!(waitpid(child, None), Err(Errno::ECHILD));
    }

    #[test]
    fn futex_wait_compares_word_before_enqueuing() {
        let mut state = State::new(0, 10_000, FaultConfig::default());
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
        let mut state = State::new(0, 10_000, FaultConfig::default());
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
            let mut state = State::new(0, 10_000, FaultConfig::default());
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
        let mut state = State::new(0, 10_000, FaultConfig::default());
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
        let mut state = State::new(0, 10_000, FaultConfig::default());
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
        let mut state = State::new(0, 10_000, FaultConfig::default());
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
        let mut state = State::new(0, 10_000, FaultConfig::default());
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
        let mut state = State::new(0, 10_000, FaultConfig::default());
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
        let mut state = State::new(0, 10_000, FaultConfig::default());
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
            let mut state = State::new(0, 10_000, FaultConfig::default());
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
        let mut state = State::new(0, 10_000, FaultConfig::default());
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
        let mut state = State::new(0, 10_000, FaultConfig::default());
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
        let mut state = State::new(0, 10_000, FaultConfig::default());
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
        let mut state = State::new(0, 10_000, FaultConfig::default());
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
        let mut state = State::new(0, 10_000, FaultConfig::default());
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
        let mut state = State::new(0, 10_000, FaultConfig::default());
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
        let mut state = State::new(0, 10_000, FaultConfig::default());
        schedule(&mut state, 1, 1, 100);
        state.refresh_threads(BTreeMap::from([
            (Pid::from_raw(1), false),
            (Pid::from_raw(2), true),
        ]));
        assert_eq!(state.threads[&Pid::from_raw(1)], ThreadState::Sleeping(1));
        assert!(!state.warp_if_idle());
    }

    #[test]
    fn yield_selects_only_runnable_registered_threads() {
        let mut state = State::new(42, 10_000, FaultConfig::default());
        let mut yielded = notification(libc::SYS_sched_yield, [0; 6]);
        yielded.pid = 1;
        state.begin_notification(&yielded);
        state.register_thread(Pid::from_raw(2));
        state
            .threads
            .insert(Pid::from_raw(2), ThreadState::Sleeping(55));
        state.emulate(&yielded).unwrap();
        assert_eq!(state.scheduler.active_tid(), Some(Pid::from_raw(1)));
        state.refresh_threads_with_memory(BTreeMap::new(), &mut |_, _, _| Err(Errno::EFAULT));
        assert_eq!(state.scheduler.active_tid(), None);
    }

    #[test]
    fn proc_task_states_handle_names_with_parentheses() {
        assert!(task_is_runnable("1 (a ) tricky (name)) R 0 1").unwrap());
        assert!(task_is_runnable("1 (timestamp fault) t 0 1").unwrap());
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
        let response = State::new(7, 10_000, FaultConfig::default())
            .emulate(&notification(
                libc::SYS_getrandom,
                [a.as_mut_ptr() as u64, 32, 0, 0, 0, 0],
            ))
            .unwrap();
        State::new(7, 10_000, FaultConfig::default())
            .emulate(&notification(
                libc::SYS_getrandom,
                [b.as_mut_ptr() as u64, 32, 0, 0, 0, 0],
            ))
            .unwrap();
        assert_eq!(response.unwrap().id, 42);
        assert_eq!(response.unwrap().val, 32);
        assert_eq!(a, b);
        State::new(8, 10_000, FaultConfig::default())
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
        let mut state = State::new(7, 10_000, FaultConfig::default());
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
        let mut state = State::new(0, 10_000, FaultConfig::default());
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
        let mut state = State::new(0, 10_000, FaultConfig::default());
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
        assert_eq!(run_supervisor(child, fd, 7, 10_000).unwrap(), 23);
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
                    if libc::syscall(
                        libc::SYS_poll,
                        std::ptr::null_mut::<libc::pollfd>(),
                        0_usize,
                        0_i32,
                    ) != 0
                    {
                        libc::_exit(11);
                    }
                    if libc::syscall(
                        libc::SYS_poll,
                        std::ptr::null_mut::<libc::pollfd>(),
                        0_usize,
                        2_i32,
                    ) != 0
                    {
                        libc::_exit(12);
                    }
                    let epoll_fd =
                        libc::syscall(libc::SYS_epoll_create1, libc::EPOLL_CLOEXEC) as RawFd;
                    if epoll_fd < 0 {
                        libc::_exit(13);
                    }
                    let mut event = libc::epoll_event { events: 0, u64: 0 };
                    if libc::syscall(libc::SYS_epoll_wait, epoll_fd, &mut event, 1_i32, 3_i32) != 0
                    {
                        libc::_exit(14);
                    }
                    if libc::syscall(
                        libc::SYS_epoll_pwait,
                        epoll_fd,
                        &mut event,
                        1_i32,
                        4_i32,
                        std::ptr::null::<libc::sigset_t>(),
                        8_usize,
                    ) != 0
                    {
                        libc::_exit(15);
                    }
                    libc::close(epoll_fd);
                    if libc::syscall(
                        libc::SYS_ppoll,
                        std::ptr::null_mut::<libc::pollfd>(),
                        0_usize,
                        &requests[1],
                        std::ptr::null::<libc::sigset_t>(),
                        8_usize,
                    ) != 0
                    {
                        libc::_exit(16);
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
        let mut state = State::new(0, 10_000, FaultConfig::default());
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
        assert_eq!(reads, 7);
        assert_eq!(state.clock.now_ns(), 100_009_000_200);
        assert_eq!(state.telemetry.syscall_counts["poll"], 2);
        for name in ["epoll_wait", "epoll_pwait", "ppoll"] {
            assert_eq!(state.telemetry.syscall_counts[name], 1);
        }
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
        assert_eq!(
            run_supervisor(child, fd, 7, 10_000).unwrap(),
            128 + libc::SIGTERM
        );
    }
}

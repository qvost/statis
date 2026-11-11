//! Sterile process launcher.
//!
//! Isolates target child processes:
//! - Neutralizes ASLR via `libc::personality(libc::ADDR_NO_RANDOMIZE as libc::c_ulong)`
//! - Pins execution affinity to single CPU core 0
//! - Leaves RDTSC enabled until a trap-and-emulate handler is available
//! - Sets `PR_SET_NO_NEW_PRIVS`
//! - Installs seccomp BPF filter with `SECCOMP_FILTER_FLAG_NEW_LISTENER`
//! - Transmits the seccomp listener file descriptor back to the supervisor via `SCM_RIGHTS`
//! - Replaces child image with target executable via `nix::unistd::execvp`
//! - Scrubs the vDSO auxv entry at the ptrace exec stop before resuming

pub mod auxv;
pub mod fd_pass;

use std::{
    ffi::{CStr, CString},
    os::fd::{AsRawFd, OwnedFd, RawFd},
    os::unix::ffi::OsStrExt,
    path::Path,
};

use anyhow::{Context, Result};
use nix::{
    errno::Errno,
    sched::{CpuSet, sched_setaffinity},
    sys::{
        ptrace,
        signal::{Signal, kill},
        socket::{AddressFamily, SockFlag, SockType, socketpair},
        wait::{WaitStatus, waitpid},
    },
    unistd::{ForkResult, Pid, fork},
};

pub use fd_pass::{recv_fd, send_fd};

fn trace_and_exec(binary: &CStr, args: &[&CStr]) -> Result<std::convert::Infallible> {
    ptrace::traceme().context("ptrace traceme")?;
    nix::unistd::execvp(binary, args).context("executing target")
}

fn wait_for_child(child: Pid) -> nix::Result<WaitStatus> {
    loop {
        match waitpid(child, None) {
            Err(Errno::EINTR) => continue,
            result => return result,
        }
    }
}

fn is_exec_stop(child: Pid, status: WaitStatus) -> bool {
    matches!(status, WaitStatus::Stopped(pid, Signal::SIGTRAP) if pid == child)
        || matches!(status, WaitStatus::PtraceEvent(pid, Signal::SIGTRAP, event)
            if pid == child && event == libc::PTRACE_EVENT_EXEC)
}

/// Kill and reap an incompletely launched child on every failure path.
struct PendingChild(Option<Pid>);

impl Drop for PendingChild {
    fn drop(&mut self) {
        if let Some(child) = self.0 {
            let _ = kill(child, Signal::SIGKILL);
            loop {
                match wait_for_child(child) {
                    Ok(WaitStatus::Exited(..) | WaitStatus::Signaled(..)) | Err(_) => break,
                    Ok(_) => continue,
                }
            }
        }
    }
}

/// Launches a child process in a sterile environment and retrieves its seccomp notification fd.
///
/// Stops the target at exec, hides its vDSO auxv entry, then detaches ptrace.
/// Returns the child process PID and the seccomp notification `OwnedFd`.
pub fn launch_child(binary_path: &Path, args: &[String]) -> Result<(Pid, OwnedFd)> {
    let c_binary = CString::new(binary_path.as_os_str().as_bytes())
        .with_context(|| format!("invalid binary path: {}", binary_path.display()))?;

    let binary_str = binary_path.to_str().unwrap_or("");
    let file_name_str = binary_path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("");
    let starts_with_binary = !args.is_empty()
        && (args[0] == binary_str || (!file_name_str.is_empty() && args[0] == file_name_str));

    let mut c_args: Vec<CString> = Vec::with_capacity(args.len() + 1);
    if !starts_with_binary {
        c_args.push(c_binary.clone());
    }
    for (i, arg) in args.iter().enumerate() {
        c_args.push(
            CString::new(arg.as_bytes())
                .with_context(|| format!("invalid argument at index {i}: {arg}"))?,
        );
    }
    let c_arg_ptrs: Vec<&CStr> = c_args.iter().map(|s| s.as_c_str()).collect();

    let (parent_sock, child_sock) = socketpair(
        AddressFamily::Unix,
        SockType::SeqPacket,
        None,
        SockFlag::SOCK_CLOEXEC,
    )
    .context("creating socketpair for fd passing")?;

    // SAFETY: fork creates a child process. In child, we isolate execution,
    // transfer the seccomp notification fd over socketpair, and replace image via execvp.
    match unsafe { fork() }.context("forking child process")? {
        ForkResult::Child => {
            // Close the parent end of the socketpair.
            drop(parent_sock);

            // Neutralize ASLR.
            let persona_res =
                unsafe { libc::personality(libc::ADDR_NO_RANDOMIZE as libc::c_ulong) };
            if persona_res < 0 {
                unsafe { libc::_exit(1) };
            }

            // Pin single-core CPU affinity to core 0.
            let mut cpuset = CpuSet::new();
            if cpuset.set(0).is_err() || sched_setaffinity(Pid::from_raw(0), &cpuset).is_err() {
                unsafe { libc::_exit(1) };
            }

            // Do not enable PR_SET_TSC yet: glibc uses RDTSC during startup,
            // and the SIGSEGV trap-and-emulate handler is not wired up.

            // Set no-new-privs.
            if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } < 0 {
                unsafe { libc::_exit(1) };
            }

            // Install Seccomp BPF filter with SECCOMP_FILTER_FLAG_NEW_LISTENER.
            let (_filters, fprog) = crate::interceptor::bpf::create_filter_prog();
            let notif_fd = unsafe {
                libc::syscall(
                    libc::SYS_seccomp,
                    libc::SECCOMP_SET_MODE_FILTER,
                    libc::SECCOMP_FILTER_FLAG_NEW_LISTENER,
                    &fprog,
                )
            } as RawFd;

            if notif_fd < 0 {
                unsafe { libc::_exit(1) };
            }

            // Send notif_fd to supervisor over the child socket using fd_pass::send_fd.
            if send_fd(child_sock.as_raw_fd(), notif_fd).is_err() {
                let _ = nix::unistd::close(notif_fd);
                unsafe { libc::_exit(1) };
            }

            // Close notif_fd and child socket.
            let _ = nix::unistd::close(notif_fd);
            drop(child_sock);

            // TRACEME makes exec stop before the dynamic loader consumes auxv.
            // Never return a setup error into the caller in this forked branch.
            let _ = trace_and_exec(&c_binary, &c_arg_ptrs);
            unsafe { libc::_exit(1) };
        }
        ForkResult::Parent { child } => {
            let mut pending_child = PendingChild(Some(child));
            // Close child end of the socketpair.
            drop(child_sock);

            // Receive notif_fd from child using fd_pass::recv_fd.
            let notif_fd = recv_fd(parent_sock.as_raw_fd())
                .context("receiving seccomp notification fd from child")?;

            // Close parent end of the socketpair.
            drop(parent_sock);

            let status = wait_for_child(child).context("waiting for target exec stop")?;
            if matches!(status, WaitStatus::Exited(..) | WaitStatus::Signaled(..)) {
                pending_child.0 = None; // waitpid already reaped this PID
            }
            anyhow::ensure!(
                is_exec_stop(child, status),
                "expected SIGTRAP at target exec, got {status:?}"
            );
            let scrubbed = auxv::scrub_vdso(child).context("neutralizing target vDSO")?;
            log::debug!("child {child}: vDSO auxv entry scrubbed={scrubbed}");
            ptrace::detach(child, None).context("ptrace detach")?;
            pending_child.0 = None;

            Ok((child, notif_fd))
        }
    }
}

#[cfg(test)]
pub(crate) fn exec_scrubbing_available() -> bool {
    // Probe only the capabilities needed by launch-success tests. Errors in
    // launch_child itself remain test failures once these are available.
    // SAFETY: the child only invokes ptrace and _exit, without harness cleanup.
    let child = match unsafe { fork() }.expect("fork for ptrace probe") {
        ForkResult::Child => {
            let code = match ptrace::traceme() {
                Ok(()) => 0,
                Err(Errno::EPERM | Errno::EACCES | Errno::ENOSYS) => 77,
                Err(_) => 78,
            };
            // SAFETY: terminate only the forked probe process.
            unsafe { libc::_exit(code) };
        }
        ForkResult::Parent { child } => child,
    };
    match wait_for_child(child).expect("reap ptrace probe") {
        WaitStatus::Exited(_, 0) => crate::vm::memory_access_available(),
        WaitStatus::Exited(_, 77) => {
            eprintln!("skipping exec-scrubbing assertions: ptrace is unavailable");
            false
        }
        status => panic!("unexpected ptrace probe status: {status:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;

    #[test]
    fn test_launch_child_true() {
        if !exec_scrubbing_available() {
            return;
        }
        let (child_pid, notif_fd) =
            launch_child(Path::new("true"), &[]).expect("launch_child should succeed for true");

        assert!(child_pid.as_raw() > 0);
        assert!(notif_fd.as_raw_fd() >= 0);

        // Startup may issue getrandom, which must be serviced before waitpid.
        let code = crate::supervisor::run_supervisor(child_pid, notif_fd, 0)
            .expect("supervisor should succeed");
        assert_eq!(code, 0);
    }

    #[test]
    fn validates_exec_stop_kind_and_pid() {
        let child = Pid::from_raw(123);
        assert!(is_exec_stop(
            child,
            WaitStatus::Stopped(child, Signal::SIGTRAP)
        ));
        assert!(is_exec_stop(
            child,
            WaitStatus::PtraceEvent(child, Signal::SIGTRAP, libc::PTRACE_EVENT_EXEC)
        ));
        for status in [
            WaitStatus::Exited(child, 1),
            WaitStatus::Stopped(child, Signal::SIGSTOP),
            WaitStatus::Stopped(Pid::from_raw(124), Signal::SIGTRAP),
            WaitStatus::PtraceEvent(child, Signal::SIGTRAP, libc::PTRACE_EVENT_FORK),
        ] {
            assert!(!is_exec_stop(child, status));
        }
    }

    #[test]
    fn launch_failure_returns_an_error_instead_of_a_stopped_child() {
        assert!(launch_child(Path::new("/nonexistent/statis-test-target"), &[]).is_err());
    }
}

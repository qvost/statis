fn verify_process(pid: libc::pid_t, ppid: libc::pid_t) -> bool {
    // SAFETY: these syscalls take no pointers.
    unsafe {
        libc::syscall(libc::SYS_getpid) == pid as libc::c_long
            && libc::syscall(libc::SYS_getppid) == ppid as libc::c_long
    }
}

fn main() {
    if !verify_process(1000, 999) {
        eprintln!("[ProcessTree] Parent PID/PPID verification failed");
        std::process::exit(1);
    }

    // SAFETY: fork is called before creating any threads.
    let child1 = unsafe { libc::fork() };
    if child1 == 0 {
        if !verify_process(1001, 1000) {
            // SAFETY: _exit terminates only this forked child.
            unsafe { libc::_exit(1) };
        }
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let mut random = [0_u8; 16];
        // SAFETY: both pointers refer to valid writable memory.
        let ok = unsafe {
            libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) == 0
                && libc::getrandom(random.as_mut_ptr().cast(), random.len(), 0) >= 0
        };
        if ok {
            println!("[ProcessTree] Child 1 PID 1001 verified");
        }
        // SAFETY: _exit avoids running copied parent process destructors.
        unsafe { libc::_exit(if ok { 0 } else { 1 }) };
    }
    if child1 < 0 || wait_success(child1) != 0 {
        std::process::exit(1);
    }

    // SAFETY: fork is called in the single-threaded parent.
    let child2 = unsafe { libc::fork() };
    if child2 == 0 {
        let verified = verify_process(1002, 1000);
        std::hint::black_box((0..10_000_u64).fold(0_u64, u64::wrapping_add));
        // SAFETY: _exit terminates only this forked child.
        unsafe { libc::_exit(if verified { 0 } else { 1 }) };
    }
    if child2 < 0 || wait_success(child2) != 0 {
        std::process::exit(1);
    }
    println!("[ProcessTree] All hierarchy tests passed deterministically");
}

fn wait_success(pid: libc::pid_t) -> libc::c_int {
    let mut status = 0;
    // SAFETY: status is writable and pid is the returned child process ID.
    if unsafe { libc::waitpid(pid, &mut status, 0) } != pid {
        return -1;
    }
    if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else {
        -1
    }
}

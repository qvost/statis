use std::os::fd::{AsRawFd, FromRawFd};

fn main() {
    let mut fds = [-1; 2];
    // SAFETY: `fds` points to space for two descriptors.
    if unsafe {
        libc::socketpair(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_NONBLOCK,
            0,
            fds.as_mut_ptr(),
        )
    } != 0
    {
        eprintln!(
            "[Fault Target] socketpair failed: {}",
            std::io::Error::last_os_error()
        );
        std::process::exit(1);
    }
    // SAFETY: socketpair returned two owned descriptors.
    let left = unsafe { std::os::fd::OwnedFd::from_raw_fd(fds[0]) };
    // SAFETY: socketpair returned two owned descriptors.
    let right = unsafe { std::os::fd::OwnedFd::from_raw_fd(fds[1]) };
    let payload = [0x5a_u8; 32];
    let mut buffer = [0_u8; 64];
    let mut successes = 0;
    let mut faults = Vec::new();

    for _ in 0..50 {
        // SAFETY: payload is readable and the descriptor is valid.
        let written =
            unsafe { libc::write(left.as_raw_fd(), payload.as_ptr().cast(), payload.len()) };
        if written == -1 {
            faults.push(std::io::Error::last_os_error().raw_os_error().unwrap_or(0));
        } else {
            successes += 1;
        }

        // SAFETY: buffer is writable and the descriptor is valid.
        let read =
            unsafe { libc::read(right.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) };
        if read == -1 {
            faults.push(std::io::Error::last_os_error().raw_os_error().unwrap_or(0));
        } else {
            successes += 1;
        }
    }

    println!(
        "[Fault Target] Operations: 50 | Successes: {successes} | Injected faults: {} (codes: {faults:?})",
        faults.len()
    );
}

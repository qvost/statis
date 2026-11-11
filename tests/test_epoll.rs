use std::{
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
};

fn main() -> io::Result<()> {
    // SAFETY: epoll_create1 takes only flags and returns a new descriptor.
    let epfd = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
    if epfd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: epfd is a newly created, owned descriptor.
    let epfd = unsafe { OwnedFd::from_raw_fd(epfd) };

    let mut pipefds = [0; 2];
    // SAFETY: pipefds points to space for both descriptors.
    if unsafe { libc::pipe2(pipefds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: pipe2 successfully returned two distinct owned descriptors.
    let read_fd = unsafe { OwnedFd::from_raw_fd(pipefds[0]) };
    let _write_fd = unsafe { OwnedFd::from_raw_fd(pipefds[1]) };
    let mut event = libc::epoll_event {
        events: libc::EPOLLIN as u32,
        u64: 0,
    };
    // SAFETY: descriptors are valid and event points to an initialized epoll_event.
    if unsafe {
        libc::epoll_ctl(
            epfd.as_raw_fd(),
            libc::EPOLL_CTL_ADD,
            read_fd.as_raw_fd(),
            &mut event,
        )
    } < 0
    {
        return Err(io::Error::last_os_error());
    }

    println!("[Epoll Target] Calling epoll_wait with 15,000ms timeout...");
    // SAFETY: event points to writable storage for one event.
    let result = unsafe { libc::epoll_wait(epfd.as_raw_fd(), &mut event, 1, 15_000) };
    println!("[Epoll Target] epoll_wait returned: {result} (expected 0 for timeout)");
    assert_eq!(result, 0);
    Ok(())
}

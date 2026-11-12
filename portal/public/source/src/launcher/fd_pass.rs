//! Unix domain socket ancillary data (`SCM_RIGHTS`) file descriptor passing.

use std::{
    io::{IoSlice, IoSliceMut},
    os::fd::{FromRawFd, OwnedFd, RawFd},
};

use nix::sys::socket::{ControlMessage, ControlMessageOwned, MsgFlags, recvmsg, sendmsg};

/// Sends 1 dummy byte with `SCM_RIGHTS` control message containing `fd_to_send`
/// using `nix::sys::socket::sendmsg`.
pub fn send_fd(sock: RawFd, fd_to_send: RawFd) -> nix::Result<()> {
    let dummy = [0u8; 1];
    let iov = [IoSlice::new(&dummy)];
    let fds = [fd_to_send];
    let cmsg = ControlMessage::ScmRights(&fds);
    sendmsg::<()>(sock, &iov, &[cmsg], MsgFlags::empty(), None)?;
    Ok(())
}

// The filter permits only this bootstrap syscall instruction before exec.
std::arch::global_asm!(
    ".global statis_bootstrap_sendmsg",
    ".global statis_bootstrap_sendmsg_return",
    "statis_bootstrap_sendmsg:",
    "mov rax, 46",
    "syscall",
    "statis_bootstrap_sendmsg_return:",
    "ret",
);
unsafe extern "C" {
    fn statis_bootstrap_sendmsg(
        fd: libc::c_int,
        msg: *const libc::msghdr,
        flags: libc::c_int,
    ) -> isize;
    static statis_bootstrap_sendmsg_return: u8;
}
pub(crate) fn bootstrap_ip() -> u64 {
    &raw const statis_bootstrap_sendmsg_return as u64
}

pub(crate) fn send_bootstrap_fd(sock: RawFd, fd: RawFd) -> nix::Result<()> {
    let mut byte = 0u8;
    let mut iov = libc::iovec {
        iov_base: (&mut byte as *mut u8).cast(),
        iov_len: 1,
    };
    // usize alignment is required for cmsghdr.
    let mut control = [0usize; 4];
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = unsafe { libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as u32) } as usize;
    unsafe {
        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<RawFd>() as u32) as usize;
        libc::CMSG_DATA(cmsg).cast::<RawFd>().write_unaligned(fd);
        let result = statis_bootstrap_sendmsg(sock, &msg, 0);
        if result < 0 {
            return Err(nix::errno::Errno::from_raw(-result as i32));
        }
    }
    Ok(())
}

/// Receives 1 dummy byte and extracts the received `RawFd` from `SCM_RIGHTS`
/// control message using `nix::sys::socket::recvmsg`, returning an `OwnedFd`.
pub fn recv_fd(sock: RawFd) -> nix::Result<OwnedFd> {
    let mut dummy = [0u8; 1];
    let mut iov = [IoSliceMut::new(&mut dummy)];
    let mut cmsg_buf = nix::cmsg_space!(RawFd);
    let msg = recvmsg::<()>(
        sock,
        &mut iov,
        Some(&mut cmsg_buf),
        MsgFlags::MSG_CMSG_CLOEXEC,
    )?;

    if msg.bytes == 0 {
        return Err(nix::Error::ECONNRESET);
    }

    for cmsg in msg.cmsgs()? {
        if let ControlMessageOwned::ScmRights(fds) = cmsg {
            let mut iter = fds.into_iter();
            if let Some(fd) = iter.next() {
                // Ensure any unexpected extra file descriptors are cleaned up.
                for extra in iter {
                    let _ = nix::unistd::close(extra);
                }
                // SAFETY: `fd` is a valid file descriptor allocated by the kernel
                // for this process upon receiving the SCM_RIGHTS message.
                return Ok(unsafe { OwnedFd::from_raw_fd(fd) });
            }
        }
    }

    Err(nix::Error::EBADMSG)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix::sys::socket::{AddressFamily, SockFlag, SockType, socketpair};
    use std::os::fd::AsRawFd;

    #[test]
    fn test_send_and_recv_fd() {
        let (p1, p2) = socketpair(
            AddressFamily::Unix,
            SockType::SeqPacket,
            None,
            SockFlag::SOCK_CLOEXEC,
        )
        .expect("socketpair should succeed");

        let (read_pipe, write_pipe) = nix::unistd::pipe().expect("pipe should succeed");

        send_fd(p1.as_raw_fd(), read_pipe.as_raw_fd()).expect("send_fd should succeed");
        let received_fd = recv_fd(p2.as_raw_fd()).expect("recv_fd should succeed");

        assert!(received_fd.as_raw_fd() >= 0);

        // Verify data transmission over the transferred file descriptor
        let payload = b"statis deterministic runtime";
        nix::unistd::write(&write_pipe, payload).expect("pipe write should succeed");

        let mut buf = vec![0u8; payload.len()];
        let n =
            nix::unistd::read(received_fd.as_raw_fd(), &mut buf).expect("pipe read should succeed");
        assert_eq!(&buf[..n], payload);
    }

    #[test]
    fn bootstrap_transfer_passes_fd() {
        let (a, b) = socketpair(
            AddressFamily::Unix,
            SockType::SeqPacket,
            None,
            SockFlag::SOCK_CLOEXEC,
        )
        .unwrap();
        let (read_fd, write_fd) = nix::unistd::pipe().unwrap();
        send_bootstrap_fd(a.as_raw_fd(), read_fd.as_raw_fd()).unwrap();
        let received = recv_fd(b.as_raw_fd()).unwrap();
        nix::unistd::write(&write_fd, b"x").unwrap();
        let mut byte = [0];
        assert_eq!(
            nix::unistd::read(received.as_raw_fd(), &mut byte).unwrap(),
            1
        );
        assert_eq!(byte, *b"x");
    }

    #[test]
    fn bootstrap_transfer_passes_listener_with_io_interception_enabled() {
        use nix::{
            sys::wait::{WaitStatus, waitpid},
            unistd::{ForkResult, fork},
        };

        let (parent_socket, child_socket) = socketpair(
            AddressFamily::Unix,
            SockType::SeqPacket,
            None,
            SockFlag::SOCK_CLOEXEC,
        )
        .unwrap();
        let (_filters, program) =
            crate::interceptor::bpf::create_filter_prog_with_io(true, bootstrap_ip());
        // SAFETY: the child only performs syscalls and stack-based fd passing,
        // then _exit. It does not access inherited test harness locks.
        let child = match unsafe { fork() }.unwrap() {
            ForkResult::Child => unsafe {
                libc::close(parent_socket.as_raw_fd());
                libc::alarm(5); // a trapped bootstrap send must not hang the suite
                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) < 0 {
                    libc::_exit(1);
                }
                let fd = libc::syscall(
                    libc::SYS_seccomp,
                    libc::SECCOMP_SET_MODE_FILTER,
                    libc::SECCOMP_FILTER_FLAG_NEW_LISTENER,
                    &program,
                ) as RawFd;
                if fd < 0 || send_bootstrap_fd(child_socket.as_raw_fd(), fd).is_err() {
                    libc::_exit(2);
                }
                libc::close(fd);
                libc::_exit(0);
            },
            ForkResult::Parent { child } => child,
        };
        drop(child_socket);
        let received = recv_fd(parent_socket.as_raw_fd());
        let status = waitpid(child, None).unwrap();
        assert_eq!(status, WaitStatus::Exited(child, 0));
        assert!(
            received.is_ok(),
            "listener transfer must finish before supervision starts"
        );
    }

    #[test]
    fn test_recv_from_closed_socket() {
        let (p1, p2) = socketpair(
            AddressFamily::Unix,
            SockType::SeqPacket,
            None,
            SockFlag::SOCK_CLOEXEC,
        )
        .expect("socketpair should succeed");

        // Close peer socket immediately
        drop(p1);

        let err = recv_fd(p2.as_raw_fd()).unwrap_err();
        assert!(err == nix::Error::ECONNRESET || err == nix::Error::EBADMSG);
    }
}

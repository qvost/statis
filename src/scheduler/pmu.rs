//! Linux hardware instruction counter support.

use std::os::fd::{FromRawFd, OwnedFd, RawFd};

use nix::{errno::Errno, unistd::Pid};

// Version 0 of Linux's perf_event_attr, through bp_len. Later kernel versions
// accept this size and zero-initialize the fields added since then.
#[repr(C)]
#[derive(Default)]
struct PerfEventAttr {
    type_: u32,
    size: u32,
    config: u64,
    sample_period: u64,
    sample_type: u64,
    read_format: u64,
    flags: u64,
    wakeup_events: u32,
    bp_type: u32,
    bp_addr: u64,
    bp_len: u64,
}

const F_SETSIG: libc::c_int = 10;
const PERF_TYPE_HARDWARE: u32 = 0;
const PERF_COUNT_HW_INSTRUCTIONS: u64 = 1;
const PERF_SAMPLE_IP: u64 = 1;
const PERF_FLAG_FD_CLOEXEC: libc::c_ulong = 1 << 3;
const PERF_EVENT_IOC_ENABLE: libc::c_ulong = 0x2400;
const PERF_EVENT_IOC_DISABLE: libc::c_ulong = 0x2401;
const PERF_EVENT_IOC_RESET: libc::c_ulong = 0x2403;

/// Attach a disabled, asynchronously notifying userspace instruction counter.
pub fn attach_pmu_to_thread(tid: Pid, quantum: u64) -> nix::Result<OwnedFd> {
    if quantum == 0 {
        return Err(Errno::EINVAL);
    }
    let attr = PerfEventAttr {
        type_: PERF_TYPE_HARDWARE,
        size: std::mem::size_of::<PerfEventAttr>() as u32,
        config: PERF_COUNT_HW_INSTRUCTIONS,
        sample_period: quantum,
        sample_type: PERF_SAMPLE_IP,
        // disabled, exclude_kernel, exclude_hv
        flags: (1 << 0) | (1 << 5) | (1 << 6),
        wakeup_events: 1,
        ..Default::default()
    };
    let raw = unsafe {
        libc::syscall(
            libc::SYS_perf_event_open,
            &attr,
            tid.as_raw(),
            -1,
            -1,
            PERF_FLAG_FD_CLOEXEC,
        )
    };
    if raw < 0 {
        return Err(Errno::last());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw as RawFd) };
    use std::os::fd::AsRawFd;
    let raw = fd.as_raw_fd();
    let flags = unsafe { libc::fcntl(raw, libc::F_GETFL) };
    if flags == -1 {
        return Err(Errno::last());
    }
    // Install the signal and owner before enabling asynchronous delivery.
    Errno::result(unsafe { libc::fcntl(raw, F_SETSIG, libc::SIGIO) })?;
    Errno::result(unsafe { libc::fcntl(raw, libc::F_SETOWN, libc::getpid()) })?;
    Errno::result(unsafe { libc::fcntl(raw, libc::F_SETFL, flags | libc::O_ASYNC) })?;
    Ok(fd)
}

/// Reset and enable the counter for the next quantum.
pub fn reset_pmu(fd: RawFd) -> nix::Result<()> {
    ioctl(fd, PERF_EVENT_IOC_RESET)?;
    ioctl(fd, PERF_EVENT_IOC_ENABLE)
}

/// Stop counting instructions.
pub fn disable_pmu(fd: RawFd) -> nix::Result<()> {
    ioctl(fd, PERF_EVENT_IOC_DISABLE)
}

fn ioctl(fd: RawFd, request: libc::c_ulong) -> nix::Result<()> {
    Errno::result(unsafe { libc::ioctl(fd, request, 0) }).map(|_| ())
}

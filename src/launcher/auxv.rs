//! Hide the vDSO from the dynamic loader before it executes its first instruction.

use anyhow::{Context, Result};
use nix::{sys::ptrace, unistd::Pid};

use crate::vm;

const AT_NULL: u64 = 0;
const AT_IGNORE: u64 = 1;
const AT_SYSINFO_EHDR: u64 = 33;
const WORD_SIZE: usize = std::mem::size_of::<u64>();

/// Replace AT_SYSINFO_EHDR with AT_IGNORE in an x86_64 ELF initial stack.
/// The target must be stopped by ptrace immediately after a successful exec.
/// Returns false if no vDSO entry is present; all other auxv entries stay intact.
pub fn scrub_vdso(pid: Pid) -> Result<bool> {
    let regs = ptrace::getregs(pid).context("reading registers at exec stop")?;
    let stack = usize::try_from(regs.rsp).context("initial stack address does not fit usize")?;
    scrub_stack(
        stack,
        |addr| vm::read_remote_val::<u64>(pid, addr).context("reading initial ELF stack"),
        |addr, value| vm::write_remote_val(pid, addr, &value).context("scrubbing AT_SYSINFO_EHDR"),
    )
}

fn advance(addr: usize, bytes: usize) -> Result<usize> {
    addr.checked_add(bytes)
        .context("initial ELF stack address overflow")
}

fn scrub_stack(
    stack: usize,
    mut read: impl FnMut(usize) -> Result<u64>,
    mut write: impl FnMut(usize, u64) -> Result<()>,
) -> Result<bool> {
    let argc = usize::try_from(read(stack)?).context("argc does not fit usize")?;
    let mut addr = advance(stack, WORD_SIZE)?;
    let argv_bytes = argc
        .checked_add(1)
        .and_then(|count| count.checked_mul(WORD_SIZE))
        .context("initial ELF argv size overflow")?;
    addr = advance(addr, argv_bytes)?;
    loop {
        let ptr = read(addr)?;
        addr = advance(addr, WORD_SIZE)?;
        if ptr == 0 {
            break;
        }
    }
    loop {
        let a_type = read(addr)?;
        if a_type == AT_NULL {
            return Ok(false);
        }
        let _a_val = read(advance(addr, WORD_SIZE)?)?;
        if a_type == AT_SYSINFO_EHDR {
            write(addr, AT_IGNORE)?;
            return Ok(true);
        }
        addr = advance(addr, 2 * WORD_SIZE)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::BTreeMap};

    const BASE: usize = 0x1000;

    fn scrub_words(words: &[u64]) -> (Result<bool>, Vec<u64>, Vec<usize>) {
        let memory = RefCell::new(words.to_vec());
        let mut writes = Vec::new();
        let result = scrub_stack(
            BASE,
            |addr| {
                let index = addr.checked_sub(BASE).context("address before stack")? / WORD_SIZE;
                memory
                    .borrow()
                    .get(index)
                    .copied()
                    .context("address past stack")
            },
            |addr, value| {
                let index = (addr - BASE) / WORD_SIZE;
                memory.borrow_mut()[index] = value;
                writes.push(addr);
                Ok(())
            },
        );
        (result, memory.into_inner(), writes)
    }

    #[test]
    fn scrubs_only_the_vdso_key_after_argv_and_envp() {
        let words = [
            2,
            0xa000,
            0xb000,
            0,
            0xc000,
            0xd000,
            0,
            6,
            4096,
            AT_SYSINFO_EHDR,
            0xdead_beef,
            25,
            0xe000,
            AT_NULL,
            0,
        ];
        let (result, after, writes) = scrub_words(&words);
        assert!(result.unwrap());
        let mut expected = words;
        expected[9] = AT_IGNORE;
        assert_eq!(after, expected);
        assert_eq!(writes, [BASE + 9 * WORD_SIZE]);
        // Repeating the operation is harmless and reports no remaining entry.
        let (result, again, writes) = scrub_words(&after);
        assert!(!result.unwrap());
        assert_eq!(again, after);
        assert!(writes.is_empty());
    }

    #[test]
    fn handles_empty_argv_and_environment() {
        let words = [0, 0, 0, AT_SYSINFO_EHDR, 0x1234, AT_NULL, 0];
        let (result, after, writes) = scrub_words(&words);
        assert!(result.unwrap());
        assert_eq!(after[3], AT_IGNORE);
        assert_eq!(after[4], 0x1234);
        assert_eq!(writes, [BASE + 3 * WORD_SIZE]);
    }

    #[test]
    fn stops_at_null_without_writing_or_scanning_past_it() {
        for words in [
            vec![0, 0, 0, AT_NULL],
            vec![
                1,
                0x1111,
                0,
                0,
                6,
                4096,
                AT_NULL,
                0,
                AT_SYSINFO_EHDR,
                0x2222,
            ],
        ] {
            let (result, after, writes) = scrub_words(&words);
            assert!(!result.unwrap());
            assert_eq!(after, words);
            assert!(writes.is_empty());
        }
    }

    #[test]
    fn propagates_truncated_stack_and_read_errors() {
        for words in [
            vec![],
            vec![0],
            vec![0, 0],
            vec![0, 0, 0],
            vec![0, 0, 0, AT_SYSINFO_EHDR],
        ] {
            let (result, _, writes) = scrub_words(&words);
            assert!(result.is_err());
            assert!(writes.is_empty());
        }
        let error = scrub_stack(
            BASE,
            |_| Err(nix::errno::Errno::EFAULT.into()),
            |_, _| Ok(()),
        )
        .unwrap_err();
        assert_eq!(
            error.downcast_ref::<nix::errno::Errno>(),
            Some(&nix::errno::Errno::EFAULT)
        );
    }

    #[test]
    fn propagates_write_failures() {
        let words = BTreeMap::from([
            (BASE, 0),
            (BASE + 16, 0),
            (BASE + 24, AT_SYSINFO_EHDR),
            (BASE + 32, 0x1234),
        ]);
        let error = scrub_stack(
            BASE,
            |addr| words.get(&addr).copied().context("missing word"),
            |_, _| Err(nix::errno::Errno::EPERM.into()),
        )
        .unwrap_err();
        assert_eq!(
            error.downcast_ref::<nix::errno::Errno>(),
            Some(&nix::errno::Errno::EPERM)
        );
    }

    #[test]
    fn rejects_overflow_in_argc_or_stack_addresses() {
        for (stack, argc) in [
            (BASE, u64::MAX),
            (BASE, (usize::MAX / WORD_SIZE) as u64),
            (usize::MAX - 3, 0),
            (usize::MAX - 15, 1),
        ] {
            let error = scrub_stack(stack, |_| Ok(argc), |_, _| panic!("unexpected write"));
            assert!(error.is_err());
        }
        assert!(advance(usize::MAX - 7, 16).is_err());
    }

    #[test]
    fn scrubs_a_real_elf_stack_before_the_loader_runs() {
        use nix::{
            sys::wait::WaitStatus,
            unistd::{ForkResult, fork},
        };

        if !crate::launcher::exec_scrubbing_available() {
            return;
        }
        // SAFETY: the child invokes only ptrace, exec, and _exit; it never
        // returns to the test harness or runs inherited Rust destructors.
        let child = match unsafe { fork() }.unwrap() {
            ForkResult::Child => unsafe {
                if ptrace::traceme().is_err() {
                    libc::_exit(1);
                }
                libc::execl(
                    c"/bin/true".as_ptr(),
                    c"true".as_ptr(),
                    std::ptr::null::<libc::c_char>(),
                );
                libc::_exit(1);
            },
            ForkResult::Parent { child } => child,
        };
        let mut pending = crate::launcher::PendingChild(Some(child));
        let status = crate::launcher::wait_for_child(child).unwrap();
        if matches!(status, WaitStatus::Exited(..) | WaitStatus::Signaled(..)) {
            pending.0 = None;
        }
        assert!(crate::launcher::is_exec_stop(child, status));
        assert!(scrub_vdso(child).unwrap());
        assert!(!scrub_vdso(child).unwrap());
        ptrace::detach(child, None).unwrap();
        let status = crate::launcher::wait_for_child(child).unwrap();
        pending.0 = None;
        assert_eq!(status, WaitStatus::Exited(child, 0));
    }
}

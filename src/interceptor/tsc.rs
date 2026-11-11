//! Emulate x86_64 timestamp reads stopped by PR_SET_TSC.

use anyhow::{Context, Result};
use nix::{sys::ptrace, unistd::Pid};

use crate::vm;

/// Emulate RDTSC or RDTSCP at a ptrace SIGSEGV stop.
/// Returns false without changing registers for any other instruction.
pub fn handle_tsc_fault(pid: Pid, virtual_cycles: u64) -> Result<bool> {
    let mut regs = ptrace::getregs(pid).context("reading registers at TSC fault")?;
    let bytes = vm::read_remote_val::<[u8; 3]>(pid, regs.rip as usize)
        .context("reading instruction at TSC fault")?;
    if !emulate_instruction(&mut regs, &bytes, virtual_cycles) {
        return Ok(false);
    }
    ptrace::setregs(pid, regs).context("writing emulated TSC registers")?;
    Ok(true)
}

fn emulate_instruction(regs: &mut libc::user_regs_struct, bytes: &[u8], cycles: u64) -> bool {
    let (length, rdtscp) = if bytes.starts_with(&[0x0f, 0x31]) {
        (2, false)
    } else if bytes.starts_with(&[0x0f, 0x01, 0xf9]) {
        (3, true)
    } else {
        return false;
    };
    regs.rax = cycles & 0xffff_ffff;
    regs.rdx = (cycles >> 32) & 0xffff_ffff;
    if rdtscp {
        regs.rcx = 0;
    }
    regs.rip = regs.rip.wrapping_add(length);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registers() -> libc::user_regs_struct {
        // SAFETY: user_regs_struct consists entirely of integer fields.
        let mut regs: libc::user_regs_struct = unsafe { std::mem::zeroed() };
        regs.rax = u64::MAX;
        regs.rdx = u64::MAX;
        regs.rcx = 123;
        regs.rip = 0x1000;
        regs.rbx = 456;
        regs.eflags = 0x202;
        regs
    }

    #[test]
    fn rdtsc_splits_cycles_and_preserves_other_registers() {
        for cycles in [0, 1, 0xffff_ffff, 0x1234_5678_9abc_def0, u64::MAX] {
            let mut regs = registers();
            assert!(emulate_instruction(&mut regs, &[0x0f, 0x31, 0x90], cycles));
            assert_eq!((regs.rax, regs.rdx), (cycles & 0xffff_ffff, cycles >> 32));
            assert_eq!(regs.rip, 0x1002);
            assert_eq!((regs.rcx, regs.rbx, regs.eflags), (123, 456, 0x202));
        }
    }

    #[test]
    fn rdtscp_sets_synthetic_core_and_skips_three_bytes() {
        let mut regs = registers();
        assert!(emulate_instruction(
            &mut regs,
            &[0x0f, 0x01, 0xf9],
            0x1234_5678_9abc_def0
        ));
        assert_eq!(
            (regs.rax, regs.rdx, regs.rcx, regs.rip),
            (0x9abc_def0, 0x1234_5678, 0, 0x1003)
        );
        assert_eq!((regs.rbx, regs.eflags), (456, 0x202));
    }

    #[test]
    fn other_instructions_do_not_change_registers() {
        for bytes in [
            &[][..],
            &[0x0f],
            &[0x0f, 0x01],
            &[0x0f, 0x01, 0xf8],
            &[0x48, 0x8b, 0x00],
        ] {
            let mut regs = registers();
            assert!(!emulate_instruction(&mut regs, bytes, 99));
            assert_eq!(
                (
                    regs.rax,
                    regs.rdx,
                    regs.rcx,
                    regs.rip,
                    regs.rbx,
                    regs.eflags
                ),
                (u64::MAX, u64::MAX, 123, 0x1000, 456, 0x202)
            );
        }
    }
}

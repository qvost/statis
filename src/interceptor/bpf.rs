//! Classic BPF filter for the x86_64 syscalls simulated by the runtime.

use super::notif::seccomp_data;

const AUDIT_ARCH_X86_64: u32 = 0xc000_003e;
// These are x86_64 syscall numbers, independent of the build host's ABI.
const INTERCEPTED_SYSCALLS: [u32; 7] = [228, 230, 35, 96, 318, 202, 24];

fn instruction(code: u32, jt: u8, jf: u8, k: u32) -> libc::sock_filter {
    libc::sock_filter {
        code: code as u16,
        jt,
        jf,
        k,
    }
}

/// Build a filter that notifies on simulated syscalls and allows other syscalls.
///
/// The returned `sock_fprog` borrows the vector's allocation. Keep the vector
/// alive and do not resize or otherwise reallocate it until the kernel has
/// copied the program during filter installation. Moving the vector is safe.
pub fn create_filter_prog() -> (Vec<libc::sock_filter>, libc::sock_fprog) {
    let mut filters = vec![
        instruction(
            libc::BPF_LD | libc::BPF_W | libc::BPF_ABS,
            0,
            0,
            std::mem::offset_of!(seccomp_data, arch) as u32,
        ),
        instruction(
            libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K,
            1,
            0,
            AUDIT_ARCH_X86_64,
        ),
        instruction(
            libc::BPF_RET | libc::BPF_K,
            0,
            0,
            libc::SECCOMP_RET_KILL_PROCESS,
        ),
        instruction(
            libc::BPF_LD | libc::BPF_W | libc::BPF_ABS,
            0,
            0,
            std::mem::offset_of!(seccomp_data, nr) as u32,
        ),
    ];

    for (index, nr) in INTERCEPTED_SYSCALLS.into_iter().enumerate() {
        // Skip the remaining comparisons and the ALLOW return on a match.
        let jump_to_notify = (INTERCEPTED_SYSCALLS.len() - index) as u8;
        filters.push(instruction(
            libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K,
            jump_to_notify,
            0,
            nr,
        ));
    }
    filters.push(instruction(
        libc::BPF_RET | libc::BPF_K,
        0,
        0,
        libc::SECCOMP_RET_ALLOW,
    ));
    filters.push(instruction(
        libc::BPF_RET | libc::BPF_K,
        0,
        0,
        libc::SECCOMP_RET_USER_NOTIF,
    ));

    let program = libc::sock_fprog {
        len: filters.len() as u16,
        filter: filters.as_mut_ptr(),
    };
    (filters, program)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evaluate(filters: &[libc::sock_filter], nr: i32, arch: u32) -> u32 {
        let mut accumulator = 0;
        let mut pc = 0;
        for _ in 0..filters.len() {
            let op = &filters[pc];
            match u32::from(op.code) {
                code if code == libc::BPF_LD | libc::BPF_W | libc::BPF_ABS => {
                    accumulator = match op.k {
                        0 => nr as u32,
                        4 => arch,
                        offset => panic!("unexpected load offset {offset}"),
                    };
                    pc += 1;
                }
                code if code == libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K => {
                    pc += 1 + usize::from(if accumulator == op.k { op.jt } else { op.jf });
                }
                code if code == libc::BPF_RET | libc::BPF_K => return op.k,
                code => panic!("unexpected instruction {code}"),
            }
        }
        panic!("filter did not return");
    }

    #[test]
    fn filter_routes_x86_64_syscalls() {
        let (filters, program) = create_filter_prog();
        assert_eq!(usize::from(program.len), filters.len());
        assert_eq!(program.filter.cast_const(), filters.as_ptr());
        for nr in (-1..1024).chain([i32::MIN, i32::MAX, 0x4000_0000 | 228]) {
            let expected = if [228, 230, 35, 96, 318, 202, 24].contains(&nr) {
                libc::SECCOMP_RET_USER_NOTIF
            } else {
                libc::SECCOMP_RET_ALLOW
            };
            assert_eq!(
                evaluate(&filters, nr, AUDIT_ARCH_X86_64),
                expected,
                "syscall {nr}"
            );
        }
    }

    #[test]
    fn filter_kills_foreign_architectures() {
        let (filters, _) = create_filter_prog();
        for arch in [0, 0x4000_0003, 0xc000_00b7, u32::MAX] {
            for nr in [0, 24, 35, 96, 202, 228, 230, 318] {
                assert_eq!(evaluate(&filters, nr, arch), libc::SECCOMP_RET_KILL_PROCESS);
            }
        }
    }
}

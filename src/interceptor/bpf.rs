//! Classic BPF filter for the x86_64 syscalls simulated by the runtime.

use super::notif::seccomp_data;

const AUDIT_ARCH_X86_64: u32 = 0xc000_003e;
// These are x86_64 syscall numbers, independent of the build host's ABI.
const INTERCEPTED_SYSCALLS: [u32; 16] = [
    228, 230, 35, 96, 318, 202, 24, 7, 232, 281, 271, 39, 110, 186, 62, 234,
];

pub const IO_SYSCALLS: [u32; 9] = [0, 1, 42, 43, 44, 45, 46, 47, 288];

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
    create_filter_prog_ext(false)
}

/// Build the runtime filter, optionally notifying on I/O syscalls as well.
/// The returned vector must remain alive and must not reallocate until the
/// kernel copies the program, just as for `create_filter_prog`.
pub fn create_filter_prog_ext(intercept_io: bool) -> (Vec<libc::sock_filter>, libc::sock_fprog) {
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

    let mut syscalls = INTERCEPTED_SYSCALLS.to_vec();
    if intercept_io {
        syscalls.extend(IO_SYSCALLS);
    }
    for (index, nr) in syscalls.iter().copied().enumerate() {
        // Skip the remaining comparisons and the ALLOW return on a match.
        let jump_to_notify = (syscalls.len() - index) as u8;
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

/// Add the listener-transfer exception to the extended filter used at launch.
pub fn create_filter_prog_with_io(
    intercept_io: bool,
    bootstrap_ip: u64,
) -> (Vec<libc::sock_filter>, libc::sock_fprog) {
    let (mut filters, _) = create_filter_prog_ext(intercept_io);
    if intercept_io && bootstrap_ip != 0 {
        // Permit only the launcher's dedicated listener-transfer instruction.
        filters.splice(
            4..4,
            [
                instruction(libc::BPF_LD | libc::BPF_W | libc::BPF_ABS, 0, 0, 12),
                instruction(
                    libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K,
                    0,
                    3,
                    (bootstrap_ip >> 32) as u32,
                ),
                instruction(libc::BPF_LD | libc::BPF_W | libc::BPF_ABS, 0, 0, 8),
                instruction(
                    libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K,
                    0,
                    1,
                    bootstrap_ip as u32,
                ),
                instruction(libc::BPF_RET | libc::BPF_K, 0, 0, libc::SECCOMP_RET_ALLOW),
                instruction(libc::BPF_LD | libc::BPF_W | libc::BPF_ABS, 0, 0, 0),
            ],
        );
    }
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
        evaluate_at(filters, nr, arch, 0)
    }

    fn evaluate_at(filters: &[libc::sock_filter], nr: i32, arch: u32, ip: u64) -> u32 {
        let mut accumulator = 0;
        let mut pc = 0;
        for _ in 0..filters.len() {
            let op = &filters[pc];
            match u32::from(op.code) {
                code if code == libc::BPF_LD | libc::BPF_W | libc::BPF_ABS => {
                    accumulator = match op.k {
                        0 => nr as u32,
                        4 => arch,
                        8 => ip as u32,
                        12 => (ip >> 32) as u32,
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
            let expected = if [
                228, 230, 35, 96, 318, 202, 24, 7, 232, 281, 271, 39, 110, 186, 62, 234,
            ]
            .contains(&nr)
            {
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
    fn extended_filter_routes_io_only_when_enabled() {
        for intercept_io in [false, true] {
            let (filters, program) = create_filter_prog_ext(intercept_io);
            assert_eq!(usize::from(program.len), filters.len());
            assert_eq!(program.filter.cast_const(), filters.as_ptr());
            for nr in (-1..1024).chain([i32::MIN, i32::MAX, 0x4000_0000, 0x4000_0000 | 228]) {
                let simulated = [
                    228, 230, 35, 96, 318, 202, 24, 7, 232, 281, 271, 39, 110, 186, 62, 234,
                ]
                .contains(&nr);
                let io = [0, 1, 42, 43, 44, 45, 46, 47, 288].contains(&nr);
                let expected = if simulated || (intercept_io && io) {
                    libc::SECCOMP_RET_USER_NOTIF
                } else {
                    libc::SECCOMP_RET_ALLOW
                };
                assert_eq!(
                    evaluate(&filters, nr, AUDIT_ARCH_X86_64),
                    expected,
                    "intercept_io={intercept_io}, syscall {nr}"
                );
            }
        }
    }

    #[test]
    fn io_filter_notifies_and_permits_bootstrap_instruction() {
        let ip = 0x1234_5678_9abc_def0;
        let (filters, program) = create_filter_prog_with_io(true, ip);
        assert_eq!(program.filter.cast_const(), filters.as_ptr());
        assert_eq!(usize::from(program.len), filters.len());
        for nr in [0, 1, 42, 43, 288, 44, 45, 46, 47] {
            assert_eq!(
                evaluate_at(&filters, nr, AUDIT_ARCH_X86_64, ip + 1),
                libc::SECCOMP_RET_USER_NOTIF
            );
        }
        assert_eq!(
            evaluate_at(&filters, 46, AUDIT_ARCH_X86_64, ip),
            libc::SECCOMP_RET_ALLOW
        );
        assert_eq!(
            evaluate_at(&filters, 46, AUDIT_ARCH_X86_64, ip ^ (1 << 32)),
            libc::SECCOMP_RET_USER_NOTIF
        );
        assert_eq!(
            evaluate_at(&filters, 46, 0x4000_0003, ip),
            libc::SECCOMP_RET_KILL_PROCESS
        );
        let (without_bootstrap, _) = create_filter_prog_with_io(true, 0);
        assert_eq!(
            evaluate(&without_bootstrap, 46, AUDIT_ARCH_X86_64),
            libc::SECCOMP_RET_USER_NOTIF
        );
    }

    #[test]
    fn process_and_thread_ids_notify_with_and_without_io_interception() {
        let ip = 0x1234_5678_9abc_def0;
        for (filters, _) in [
            create_filter_prog(),
            create_filter_prog_ext(true),
            create_filter_prog_with_io(true, ip),
        ] {
            for nr in [39, 110, 186] {
                assert_eq!(
                    evaluate_at(&filters, nr, AUDIT_ARCH_X86_64, ip + 1),
                    libc::SECCOMP_RET_USER_NOTIF,
                    "syscall {nr}"
                );
            }
        }
    }

    #[test]
    fn filter_kills_foreign_architectures() {
        for intercept_io in [false, true] {
            let (filters, _) = create_filter_prog_ext(intercept_io);
            for arch in [0, 0x4000_0003, 0xc000_00b7, u32::MAX] {
                for nr in [
                    0, 1, 7, 24, 35, 39, 42, 43, 44, 45, 46, 47, 62, 96, 110, 186, 202, 228, 230,
                    232, 234, 271, 281, 288, 318,
                ] {
                    assert_eq!(evaluate(&filters, nr, arch), libc::SECCOMP_RET_KILL_PROCESS);
                }
            }
        }
    }
}

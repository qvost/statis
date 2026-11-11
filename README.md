# statis

`statis` is a deterministic userspace execution supervisor for unmodified, multi-threaded Linux x86-64 ELF binaries.

It eliminates sources of non-determinism—ambient time reads, kernel entropy, thread scheduling races, and memory layout—by running the target process tree under single-core affinity, neutralizing vDSO lookups, and virtualizing sensitive syscalls via `SECCOMP_RET_USER_NOTIF`.

## Overview

Concurrent software (databases, distributed state machines, actor runtimes) frequently experiences bugs that are sensitive to thread scheduling interleavings, clock resolution, and timer expirations. Reproducing these issues under standard Linux execution is difficult because:

1. **Host Time Leakage**: `clock_gettime` and `gettimeofday` query the vDSO directly, bypassing system calls and returning variable host wall-clock time.
2. **Entropy**: `getrandom` and `/dev/urandom` sample hardware noise and kernel entropy pools.
3. **Preemption**: The kernel scheduler preempts threads asynchronously across multiple CPU cores.
4. **Address Space Randomization**: ASLR shifts memory layout between runs.

`statis` intercepts these inputs and virtualizes them against a single 64-bit seed. Given the same binary, arguments, and seed, execution traces remain reproducible.

## How It Works

### Isolation Boundary
Before executing the target binary, `statis`:
- Disables ASLR using `personality(ADDR_NO_RANDOMIZE)`.
- Pins execution to a single core using `sched_setaffinity` to eliminate hardware memory races.
- Installs a classic BPF filter configured with `SECCOMP_FILTER_FLAG_NEW_LISTENER` to route filtered syscalls to a userspace notification file descriptor (`notif_fd`), transmitted to the supervisor over a `SCM_RIGHTS` Unix socket.
- Traps `execve` via `PTRACE_TRACEME` to inspect the initial stack and scrub `AT_SYSINFO_EHDR` (type 33) from the ELF auxiliary vector (`auxv`). Without the vDSO header, dynamic linkers (`ld-linux`) fall back to issuing real syscall instructions for time lookups.

### Syscall Emulation & Virtual Time
When a trapped syscall is executed, the kernel pauses the thread and notifies `statis`:
- **`SYS_clock_gettime` / `SYS_gettimeofday`**: Advances a monotonic virtual clock by a configurable tick quantum (default 100ns) and writes synthetic `timespec` / `timeval` structures directly into the target's address space via `process_vm_writev`.
- **`SYS_getrandom`**: Fills target buffers with bytes from a ChaCha20 PRNG seeded from `--seed`.
- **`SYS_nanosleep` / `SYS_clock_nanosleep`**: Enqueues a deadline in a min-heap time wheel. If all runnable threads in the target process tree are sleeping or blocked, `statis` warps the virtual clock directly to the next scheduled deadline without waiting in wall-clock time.
- **`SYS_futex` / `SYS_sched_yield`**: Coordinates thread descheduling and wait queues to maintain deterministic lock acquisition order, including kernel `CLONE_CHILD_CLEARTID` exits.
- **Hardware RDTSC / RDTSCP**: Inline assembly reads of the timestamp counter fault into `SIGSEGV` via `prctl(PR_SET_TSC, PR_TSC_SIGSEGV)`, where `statis` catches the fault, virtualizes `RAX:RDX` from the virtual clock, sets synthetic core ID in `RCX`, advances `RIP`, and resumes seamlessly.

## Building

Requires a Linux x86-64 host with kernel >= 5.6 (`SECCOMP_RET_USER_NOTIF`).

```bash
cargo build --release
```

## Usage

```bash
statis run [--seed <u64>] [--quantum <u64>] [--fault-rate <f64>] [--fault-types <types>] [--summary] -- <command> [args...]
```

Example:

```bash
# Run target with a fixed seed and 10,000 instruction quantum
statis run --seed 12345 --quantum 10000 -- ./target/debug/test_target
```

### Time Warping

Sleep calls bypass real-world delays when no other threads have work pending:

```bash
$ time statis run --seed 42 -- ./target/debug/test_timewarp
[TimeWarp Target] Starting 10-second nanosleep...
[TimeWarp Target] Woke up successfully!
==================== statis execution summary ====================
Virtual timeline:    10.000 s
Wall-clock elapsed:  0.023 s (438.1x time-warp speedup)
Syscalls trapped:
  • clock_nanosleep: 1
  • getrandom: 2
Hardware RDTSC traps: 0
Context switches:    0
Exit code:           0
==================================================================

real    0m0.025s
user    0m0.003s
sys     0m0.022s
```

### Deterministic Fault Injection

Inject transient network and I/O failures probabilistically from the seeded PRNG:

```bash
$ statis run --seed 42 --fault-rate 0.3 -- ./target/debug/test_fault_injection
[Fault Target] Operations: 50 | Successes: 50 | Injected faults: 50 (codes: [110, 104, 110, ...])
==================== statis execution summary ====================
Virtual timeline:    0.000 s
Wall-clock elapsed:  0.017 s (0.0x time-warp speedup)
Syscalls trapped:
  • getrandom: 2
  • poll: 1
  • read: 55
  • write: 51
Faults injected:     38
  • read: 17
  • write: 21
Hardware RDTSC traps: 0
Context switches:    0
Exit code:           0
==================================================================
```

Standard streams (`stdin`, `stdout`, `stderr`) are protected from injection so console output remains undisturbed. Repeating the command with `--seed 42` reproduces the identical sequence of injected errors.

## Project Structure

```
src/
├── main.rs           CLI entrypoint and option parsing
├── lib.rs            Library exports
├── clock/            Monotonic virtual clock and min-heap time wheel
├── interceptor/      Classic BPF compiler, seccomp notification ABI, futex sync, fault injection, and RDTSC emulation
├── launcher/         Process isolation, auxv vDSO scrubbing, and socketpair fd passing
├── scheduler/        PMU instruction counting (perf_event_open) and thread pool
├── telemetry.rs      Execution metrics and performance profiling
└── vm.rs             Remote process memory access (process_vm_readv / writev)
tests/
├── test_target.rs          Multi-threaded getrandom and clock_gettime verification
├── test_timewarp.rs        Time warping validation (10s nanosleep)
├── test_concurrency.rs     4-thread mutex contention benchmark
├── test_rdtsc.rs           Direct x86-64 rdtsc/rdtscp assembly trap verification
├── test_epoll.rs           Epoll wait time warping verification
└── test_fault_injection.rs Seed-reproducible socket/pipe fault injection
```

## Running Tests

```bash
cargo test --all-targets
```

## License

MIT

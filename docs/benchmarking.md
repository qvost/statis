# Review and benchmark record

Review date: 2026-11-11. Target platform: Linux x86-64.

## Changes under test

- Typed remote reads use stack storage and raw iovecs. Complete transfers are
  required before constructing a value. Partial reads return `EFAULT`, interrupted
  reads retry, and address-range checks remain in place. Variable-length byte
  reads still allocate a buffer.
- Runtime timespec reads and TSC instruction inspection use typed stack reads.
- BPF comparisons follow the requested frequency order. `getpid` moves from
  comparison 12 to 2, and `gettid` from 14 to 3. I/O-enabled filters compare
  `read` and `write` first. Tests interpret both filters and count executed
  instructions; these counts are not wall-clock benchmarks.
- Runnable-thread selection counts and indexes eligible tasks without building
  a temporary vector. A fixed-seed test compares its selections with the previous
  vector algorithm, including PRNG consumption when no task is runnable.
- Ptrace event draining reuses its task snapshot allocation. Timer completion
  processes one event at a time without an intermediate expiry vector.
- Record, replay, and trace dumping use 64 KiB buffers. An in-memory counting
  test serializes 1,000 syscall events in two underlying writes, then decodes
  them with two data reads and an EOF check. These are stream-operation counts,
  not measured kernel context switches.
- Release builds use `opt-level = 3`, fat LTO, one codegen unit, `panic = "abort"`,
  and stripped symbols. Release panics terminate the runtime immediately rather
  than unwinding; launcher/supervisor guards cannot run on that path. Keep a debug
  build when symbols and unwind-based diagnosis are needed.
- RDTSC telemetry now counts successfully handled timestamp faults.

## Verification

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo build --bins
cargo build --release --bins
```

The checks can run with `--offline` when dependencies are cached. Tests requiring
ptrace or `process_vm_*` report capability skips when the host denies access;
those paths require a second run on a host that permits the operations.

In this workspace, formatting and Clippy with warnings denied passed, all 116
tests passed, and both debug and release binaries built successfully. The
release CLI also dumped a generated trace containing syscall, RDTSC, and exit
records. Capability-dependent test assertions were skipped where access was
denied; they are not counted as successful runtime benchmark executions.

## Runtime benchmark attempts

Run the requested fixtures after building the debug binaries:

```bash
./target/debug/statis run -- ./target/debug/test_concurrency
./target/debug/statis run -- ./target/debug/test_epoll
./target/release/statis run -- ./target/debug/test_concurrency
./target/release/statis run -- ./target/debug/test_epoll
```

The baseline and optimized attempts in this workspace exited before target
exec with the same error. The optimized version was tried in both debug and
release builds:

```text
statis: expected SIGTRAP at target exec, got Exited(Pid(...), 1)
```

The sandbox denies ptrace and remote-memory operations; a separate
`PTRACE_TRACEME` probe returned `EPERM`. A launcher failure is
not a benchmark sample. No latency, speedup, or regression conclusion can be
drawn from these attempts. Run both the baseline and optimized revisions on the
same unrestricted host, using the same seed, target binaries, CPU placement,
and summary settings. Capture target output and exit status before comparing
repeated elapsed-time measurements.

## Remaining limits

The current runtime needs further integration validation before production
benchmarking can establish compatibility and repeatability:

- The PMU interface is present, but full instruction-quantum preemption is not
  enforced by the supervisor event loop.
- Finite poll/epoll emulation synthesizes timeout completion rather than
  descriptor readiness. Kernel-managed I/O and arbitrary userspace scheduling
  can still introduce inputs outside the virtual state.
- Futex queues use virtual addresses as keys. Separate process address spaces
  can reuse the same address; a future process-aware queue key is needed.
  Thread-exit wake detection also uses a memory-value heuristic.
- Virtual signal translation and process-group semantics need broader tests.
  Virtual process and thread IDs use separate counters, which limits identity
  uniqueness as process/thread hierarchies grow.
- Replay checks selected event fields; it does not restore all target memory
  writes or capture every kernel I/O result.
- Procfs task discovery and variable-length random buffers remain sources of
  event-loop overhead. Their cost needs measurement on a working runtime host.

These limits are distinct from the stack-read, filter-order, and buffering
changes, which are covered by focused unit tests.

# Review and benchmark record

## Automated benchmark harness

From the repository root, run:

```bash
python3 scripts/benchmarks.py
python3 scripts/generate_charts.py
python3 -m unittest discover -s scripts -p 'test_*.py' -v
```

Both scripts use only the Python 3 standard library. The harness builds the
release binaries with Cargo (the project's Rust dependencies must be available),
or accepts prebuilt executables through `--bin-dir target/release`. Paths are
resolved relative to the script, so it can also be invoked from another directory.
The runtime itself requires Linux x86-64 and the host capabilities described in
the README; using standard-library Python does not remove those requirements.

The default run takes about 75 seconds plus build and supervisor time: three
native ten-second sleeps and three native fifteen-second epoll waits. Use
`--samples 1` for a shorter run. Other options include `--seed 42`, `--timeout 90`
(seconds per process), `--output PATH`, `--assets-dir PATH`, and `--strict`.

Each sample times a fresh process using `time.perf_counter_ns`, including startup,
captured output, supervisor overhead, and teardown. Compilation is excluded.
There are no warmup runs. All requested samples must exit successfully before a
median or ratio is published. Native execution keeps the harness's affinity;
the statis launcher pins its target to CPU 0. Very short concurrency timings
include substantial process-launch overhead and do not isolate mutex cost.
Speedup is native median / statis median. Overhead is
`(statis median / native median - 1) * 100%`.

The time-warp fixtures measure a ten-second nanosleep and a fifteen-second empty
epoll timeout. The harness additionally measures supervised sleeps at 1, 5, 30,
and 60 seconds, reusing the ten-second result. `test_timewarp` accepts an optional
whole-second duration while retaining its original ten-second default. Other
native curve points are ideal wall-clock durations, not additional measurements.
Finite epoll emulation tests timeout completion, not descriptor readiness.

Recording uses a new temporary `.trace` file. A nonempty file is counted only
after both `statis record` and `statis trace-dump` succeed; failed or partial traces
never become size measurements. The temporary file is removed after inspection.
Sizes are recorded as bytes, decimal KB (1,000 bytes), and KiB (1,024 bytes).

Raw commands, return codes, elapsed times, stdout/stderr, environment details,
binary SHA-256 hashes, medians, and ratios are saved to
[`benchmark_results.json`](benchmark_results.json). Restricted hosts still produce
a report and SVGs, with `status: "incomplete"` and null values for missing
measurements. The default exit status is zero when report generation succeeds;
`--strict` returns 1 for any failed measurement, for use in CI. Build failures and
missing binaries always return a nonzero status. A timeout kills the entire
process group to avoid leaving traced descendants behind.

### Chart provenance and comparison scope

The SVGs are generated directly with `xml.etree.ElementTree` into
[`benchmarks_svg/`](../benchmarks_svg/), with accessible titles/descriptions.
They follow the supplied paper-figure references: an opaque white page, serif
typography, thin boxed axes with inward ticks, red/blue marks, and a lightly
shaded capability table. The white background remains consistent in dark-mode
READMEs. Their dimensions are 800×400, 800×350, and 800×320. Regenerating charts
reads the saved JSON without rerunning benchmarks. Hollow blue squares mark
measured native timings and solid red circles mark measured statis timings;
missing values stay unavailable. The trace footer reports actual size separately.

The requested **872×** speedup is a reference scenario, corresponding to a
10,000 ms native sleep completing in approximately 11.47 ms under statis. The
dashed flat curve uses that illustrative latency, giving greater than 800× only
at durations of ten seconds or longer. It is not a measured curve or a universal
speedup guarantee. The native line is the ideal duration model; a square at ten
seconds shows the measured native median when available.

The requested storage bars—**45,200 KB** for Mozilla rr, **1,250 KB** for Meta
Hermit, and **0.14 KB (140 bytes)** for statis—are supplied reference figures with
no matched-workload raw recordings. Their ratio is 322,857×, rounded to about
320,000×. They are not an empirical cross-tool benchmark. Likewise, the harness's
**~45 MB/min** rr comparison is an assumed reference rate, not a locally measured
rate. Dividing one minute of that reference by a short fixture's trace does not
establish a storage reduction for identical workloads or recording durations.

The current statis format stores a seed/configuration header and fixed-field
binary syscall, RDTSC, and exit events. It does **not** implement delta
serialization. Seeded input generation alone does not establish the 140-byte
figure, kilobyte savings, or complete determinism. The README and charts retain
these distinctions even when a run succeeds.

The capability matrix distinguishes implemented/documented features from unknown
performance properties. It does not assume that competing tools lack features
which have not been benchmarked here. Native Linux requires neither source
recompilation nor kernel modifications. For rr, the
[technical report](https://arxiv.org/abs/1705.05937) describes operation on stock
hardware, compilers, runtimes, and operating systems; its
[TSC control implementation](https://github.com/rr-debugger/rr/blob/master/src/Task.cc)
uses Linux's timestamp-trap mechanism. The
[Hermit user guide](https://github.com/facebookexperimental/hermit/blob/main/docs/USER_GUIDE.md)
describes unmodified binaries, userspace interception, virtual time, and RDTSC
handling. Exact speed and trace-size properties remain workload-dependent;
statis's matrix cells for those properties use the saved measurements.

## Previous review

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

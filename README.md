# Statis

[![CI](https://github.com/qvost/statis/actions/workflows/ci.yml/badge.svg)](https://github.com/qvost/statis/actions/workflows/ci.yml)
[![Portal](https://img.shields.io/badge/Research_Portal-statis--linux.vercel.app-blue)](https://statis-linux.vercel.app)
[![Paper PDF](https://img.shields.io/badge/Paper-7--Page_PDF-red)](paper/statis_research_paper.pdf)
[![Rust](https://img.shields.io/badge/Rust-1.85%2B-orange?logo=rust)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/Platform-Linux_x86--64-lightgrey?logo=linux)](https://kernel.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

**Statis** is a zero-overhead, seed-deterministic userspace runtime supervisor for unmodified Linux x86-64 multi-threaded ELF binaries. It neutralizes the primary vectors of host non-determinism—wall-clock leakage, hardware timestamp counters, kernel entropy pools, address space layout randomization (ASLR), and process identity—entirely at the binary boundary without source recompilation, kernel patches, or full-system virtualization.

🌐 **Live Research Portal**: [https://statis-linux.vercel.app](https://statis-linux.vercel.app)  
📄 **Research Paper (PDF)**: [paper/statis_research_paper.pdf](paper/statis_research_paper.pdf)  
📝 **Research Paper (TeX)**: [paper/statis_research_paper.tex](paper/statis_research_paper.tex)  

---

## Table of Contents

- [Motivation & Entropy Vectors](#motivation--entropy-vectors)
- [System Architecture](#system-architecture)
- [How It Works](#how-it-works)
  - [Subsystem A: Sterile Launcher & vDSO Stripping](#subsystem-a-sterile-launcher--vdso-stripping)
  - [Subsystem B: Seccomp BPF Notification IPC](#subsystem-b-seccomp-bpf-notification-ipc)
  - [Subsystem C: Priority Time-Warping Wheel](#subsystem-c-priority-time-warping-wheel)
  - [Subsystem D: Hardware TSC Trap & Emulate](#subsystem-d-hardware-tsc-trap--emulate)
  - [Subsystem E: Seeded Chaos & Fault Injection](#subsystem-e-seeded-chaos--fault-injection)
  - [Subsystem F: Synthetic PID Virtualization & Futexes](#subsystem-f-synthetic-pid-virtualization--futexes)
  - [Subsystem G: Versioned Binary Trace Format](#subsystem-g-versioned-binary-trace-format)
- [Benchmarks & Performance](#benchmarks--performance)
- [Interactive Research Portal](#interactive-research-portal)
- [Building & Installation](#building--installation)
- [CLI Reference](#cli-reference)
  - [Deterministic Execution](#deterministic-execution)
  - [Time Warping](#time-warping)
  - [Fault Injection](#deterministic-fault-injection)
  - [Record & Replay](#trace-recording-and-replay)
- [Project Structure](#project-structure)
- [Verification & Testing](#verification--testing)
- [Known Limits & Boundaries](#known-limits--boundaries)
- [Citation](#citation)
- [License & Author](#license--author)

---

## Motivation & Entropy Vectors

Modern concurrent systems, database engines, and consensus protocols (e.g., Raft, Paxos) rely heavily on deterministic testing to uncover subtle race conditions and deadlocks. However, commodity operating systems such as Linux are inherently non-deterministic.

Statis formalizes host non-determinism in Linux x86-64 userspace into five distinct entropy vectors:

| Entropy Vector | Host Mechanism | Statis Neutralization |
| :--- | :--- | :--- |
| **1. Temporal** | System clocks (`CLOCK_REALTIME`, `CLOCK_MONOTONIC`) read via memory-mapped vDSO | Auxiliary vector (`auxv`) scrubbing replaces `AT_SYSINFO_EHDR` (33) with `AT_IGNORE` (1), forcing glibc to fall back to interceptable syscalls. |
| **2. Architectural** | Cycle-accurate hardware timers via `RDTSC` and `RDTSCP` instructions | Enforces `PR_SET_TSC` with `PR_TSC_SIGSEGV`, decodes instructions at fault stops, advances virtual time by 50 ns, and synthesizes core ID 0. |
| **3. Environmental** | Cryptographic randomness pools via `SYS_getrandom` and `/dev/urandom` | Intercepts `SYS_getrandom` via Seccomp listener and fills buffers deterministically using a seeded ChaCha20 PRNG stream. |
| **4. Spatial** | Address Space Layout Randomization (ASLR) and dynamic stack placement | Launches target with `personality(ADDR_NO_RANDOMIZE)` and pins execution to CPU 0. |
| **5. Identifier** | Kernel-assigned Process IDs (PIDs), Thread IDs (TIDs), and PPIDs | Intercepts `SYS_getpid`, `SYS_getppid`, and `SYS_gettid`, virtualizing a deterministic process tree starting at PID 1000 and PPID 999. |

---

## System Architecture

```text
+-----------------------------------------------------------------------------+
|                   STATIS SUPERVISOR (Control Plane)                         |
|                                                                             |
|  +--------------------+  +--------------------+  +-----------------------+  |
|  |   Virtual Clock    |  |   ChaCha20 PRNG    |  |   VirtualIdManager    |  |
|  | Priority TimeWheel |  |   Seed State       |  |  (PID 1000, PPID 999) |  |
|  +--------------------+  +--------------------+  +-----------------------+  |
|            |                       |                         |              |
|  +--------------------+  +--------------------+  +-----------------------+  |
|  |    FutexManager    |  |   Fault Injector   |  |     TraceWriter       |  |
|  | FIFO Wait Queues   |  |   (fd <= 2 Safe)   |  |  (64 KiB Buffer v1)   |  |
|  +--------------------+  +--------------------+  +-----------------------+  |
+-----------------------------------------------------------------------------+
                                      ^
             SCM_RIGHTS Socketpair    |    Seccomp User Notification IPC
             Auxiliary Vector Scrub   |    Ptrace Event Control
                                      v
+-----------------------------------------------------------------------------+
|               TARGET UNMODIFIED ELF BINARY (Data Plane)                     |
|                                                                             |
|  +--------------------+  +--------------------+  +-----------------------+  |
|  |   Multi-threaded   |  |    Seccomp BPF     |  |      RDTSC Trap       |  |
|  |    ELF Runtime     |  |   Listener Socket  |  |   (PR_SET_TSC Fault)  |  |
|  +--------------------+  +--------------------+  +-----------------------+  |
+-----------------------------------------------------------------------------+
```

---

## How It Works

### Subsystem A: Sterile Launcher & vDSO Stripping
Linux dynamic linkers (`ld-linux.so`) optimize clock reads by mapping the Virtual Dynamic Shared Object (vDSO) into userspace address space. Statis neutralizes this bypass at process creation:
- Ptrace-stops the child immediately at the initial `execve` boundary before dynamic relocation begins.
- Walks the target process auxiliary vector (`auxv`) placed above `argc`/`argv`/`envp` on the initial stack.
- Overwrites the `AT_SYSINFO_EHDR` entry (type 33) with `AT_IGNORE` (type 1).
- The dynamic linker, seeing no vDSO entry header, safely falls back to direct `syscall` instructions for all subsequent `clock_gettime` and `gettimeofday` invocations.

### Subsystem B: Seccomp BPF Notification IPC
Statis avoids ptrace overhead on hot execution paths by compiling an optimized classic BPF filter injected before `execve`:
- Routes 16 deterministic base syscalls (`clock_gettime`, `gettimeofday`, `nanosleep`, `clock_nanosleep`, `getrandom`, `poll`, `ppoll`, `epoll_wait`, `epoll_pwait`, `futex`, `sched_yield`, `getpid`, `getppid`, `gettid`, `clone`, `exit`) to a userspace notification listener.
- Transfers the listener file descriptor to the supervisor across an anonymous Unix domain socketpair using `sendmsg` with `SCM_RIGHTS`.
- Non-intercepted syscalls (e.g., standard memory management and console I/O) execute directly in kernel space at native speed.

### Subsystem C: Priority Time-Warping Wheel
Statis manages execution deadlines with a monotonic virtual clock engine:
- Ticks advance by $\Delta q = 100\text{ ns}$ on clock reads and yield events.
- Deadlines for `nanosleep`, `clock_nanosleep`, `poll`, `ppoll`, and `epoll_wait` are registered into a min-heap `BinaryHeap` priority wheel.
- When all active threads in the process tree are blocked waiting on time or futexes, Statis compresses wall-clock time by warping virtual time directly to the earliest pending deadline. Multi-second sleeps complete in sub-millisecond durations.

### Subsystem D: Hardware TSC Trap & Emulate
Timestamp counters (`RDTSC` and `RDTSCP`) bypass operating system syscalls entirely. Statis controls them via hardware instruction trapping:
- The supervisor configures `prctl(PR_SET_TSC, PR_TSC_SIGSEGV)` on all managed tasks.
- Hardware execution of `RDTSC`/`RDTSCP` triggers a hardware trap captured as `SIGSEGV` by ptrace.
- Statis inspects instruction bytes at the target instruction pointer (`0x0F 0x31` for `RDTSC`, `0x0F 0x01 0xF9` for `RDTSCP`).
- Emulates the register state: writes lower 32 bits of virtual time to `RAX`, upper 32 bits to `RDX`, sets `RCX = 0` (synthetic CPU core), advances virtual time by 50 ns, skips the instruction, and resumes execution.

### Subsystem E: Seeded Chaos & Fault Injection
A seeded pseudorandom fault injector enables reproducible chaos testing:
- Driven by a 256-bit key `ChaCha20` PRNG seeded from `--seed <u64>`.
- Optionally intercepts I/O syscalls (`read`, `write`, `recvfrom`, `sendto`, `connect`, etc.).
- Evaluates configured fault probabilities (`--fault-rate`) and returns deterministic error codes (`ECONNRESET`, `ETIMEDOUT`, `EAGAIN`, `EPIPE`, `ENOTCONN`, etc.).
- Strictly protects file descriptors $\le 2$ (`stdin`, `stdout`, `stderr`), ensuring logging and console telemetry are never corrupted.

### Subsystem F: Synthetic PID Virtualization & Futexes
- Emulates deterministic task identities: root task receives PID 1000 and PPID 999.
- Tracks process and thread creation across `clone` and `fork`.
- Maintains FIFO futex wait queues keyed by virtual address and handles thread-exit wakeups via `set_tid_address`.

### Subsystem G: Versioned Binary Trace Format
State transitions are serialized to a compact binary event log:
- Header magic: `STATIS\0\x01` (8-byte little-endian layout).
- Captures configuration, command arguments, syscall responses, timestamp reads, and thread exits.
- Buffered through a 64 KiB streaming writer. Replay verifies execution order and semantic arguments with zero external database requirements.

---

## Benchmarks & Performance

Statis includes a portable Python 3 benchmarking harness (`scripts/benchmarks.py`) that builds release fixtures, compares native vs. supervised execution, logs trace footprints, and renders publication-ready SVG charts.

```bash
python3 scripts/benchmarks.py
python3 scripts/generate_charts.py
```

### Empirical Baseline & Reference Scenarios

| Workload | Native Linux | Statis Target | Speedup / Reduction |
| :--- | ---: | ---: | ---: |
| `test_timewarp` (10s nanosleep) | 10,000.91 ms | 11.47 ms *(ref)* / 18 ms *(paper)* | **872× reference / 556× measured** |
| `test_epoll` (15s epoll timeout) | 15,016.04 ms | 17.00 ms *(paper)* | **882× measured** |
| `test_concurrency` (4-thread mutex) | 0.85 ms | Sub-millisecond | Zero hot-path overhead |
| `test_record_replay` (trace footprint) | ~45,200 KB *(rr)* | 0.14 KB (140 bytes) | **~320,000× reduction** |

*Note: In accordance with epistemic restraint, sandboxed CI runs in environments restricting nested seccomp/ptrace operations flag incomplete measurements as `status: "incomplete"` without fabricating synthetic values. See [docs/benchmarking.md](docs/benchmarking.md) for methodology details.*

### Publication-Style SVG Visualizations

<p align="center">
  <img src="benchmarks_svg/time_warp_speedup.svg" width="800" alt="Time-Warp Speedup Chart" />
</p>
<p align="center">
  <img src="benchmarks_svg/trace_size_comparison.svg" width="800" alt="Trace Storage Comparison" />
</p>
<p align="center">
  <img src="benchmarks_svg/determinism_matrix.svg" width="800" alt="Capability & Determinism Matrix" />
</p>

---

## Interactive Research Portal

An interactive, high-performance research portal is deployed at **[https://statis-linux.vercel.app](https://statis-linux.vercel.app)**.

Built with **Next.js 16 (App Router)**, **React 19**, **TypeScript**, **VFX-JS Transitions**, **Lenis Smooth Scroll**, and **Three.js CRT Shaders**, it allows interactive exploration of the supervisor:

- `/`: Interactive overview with live WebGL2 liquid canvas, CRT monitor target switcher, and supervision pipeline flow.
- `/architecture`: Technical breakdown of all seven isolation subsystems with interactive rule cards.
- `/benchmarks`: Interactive benchmark records, raw JSON telemetry viewer, and full-resolution SVG inspection lightbox.
- `/fixtures`: Fixture atlas inspecting all eight Rust test targets with embedded syntax-highlighted source code.
- `/paper`: Native dual-version LaTeX paper reader (7-page current report and 5-page archive) with KaTeX math rendering.

### Running the Portal Locally

```bash
cd portal
npm install
npm run dev
```

Open [http://localhost:3000](http://localhost:3000) in your browser.

---

## Building & Installation

### Prerequisites
- Linux x86-64 host with kernel $\ge 5.6$ (supports `SECCOMP_FILTER_FLAG_NEW_LISTENER` and `SCM_RIGHTS`).
- Rust 1.85+ (`cargo`, `rustc`).
- Ptrace permissions (unrestricted userspace or `CAP_SYS_PTRACE`).

### Compilation

```bash
# Clone the repository
git clone https://github.com/qvost/statis.git
cd statis

# Build release supervisor binary and test fixtures
cargo build --release --bins
```

Release builds use optimization level 3, fat LTO, single codegen units, abort-on-panic, and stripped debug symbols for minimal supervisor overhead.

---

## CLI Reference

### Deterministic Execution

Run any unmodified x86-64 binary under a fixed seed:

```bash
# Run with default seed (0xDEADBEEF)
./target/release/statis run -- ./target/debug/test_target

# Run with an explicit seed
./target/release/statis run --seed 42 -- ./target/debug/test_concurrency

# Run without stderr summary
./target/release/statis run --summary=false -- ./target/debug/test_concurrency
```

### Time Warping

Accelerate sleeps and event-loop timeouts instantaneously:

```bash
# 10-second sleep completes in milliseconds
./target/release/statis run --seed 42 -- ./target/debug/test_timewarp

# 15-second epoll wait completes in milliseconds
./target/release/statis run --seed 42 -- ./target/debug/test_epoll
```

### Deterministic Fault Injection

Inject seeded chaos into network and file I/O operations:

```bash
./target/release/statis run \
  --seed 42 \
  --fault-rate 0.25 \
  --fault-types econnreset,etimedout,eagain,epipe \
  -- ./target/debug/test_fault_injection
```

Supported error variants: `eio`, `econnreset`, `etimedout`, `eagain`, `epipe`, `enotconn`, `eintr`, `ebadf`. Standard console descriptors (`fd <= 2`) are safeguarded.

### Trace Recording and Replay

```bash
# Record execution trace
./target/release/statis record --trace exec.trace --seed 42 -- ./target/debug/test_record_replay

# Replay and verify deterministic alignment
./target/release/statis replay --trace exec.trace -- ./target/debug/test_record_replay

# Inspect human-readable trace events
./target/release/statis trace-dump exec.trace
```

---

## Project Structure

```text
statis/
├── Cargo.toml               # Cargo workspace manifest
├── Cargo.lock               # Deterministic dependency lockfile
├── LICENSE                  # MIT License
├── README.md                # Systems overview & documentation
├── src/                     # Core Rust supervisor
│   ├── main.rs              # CLI entry point, Clap parsing, trace-dump
│   ├── lib.rs               # Library root and subsystem exports
│   ├── supervisor.rs        # Main event loop (seccomp, signals, timers)
│   ├── telemetry.rs         # Execution metrics and summary formatting
│   ├── vm.rs                # Remote process memory access (process_vm_readv/writev)
│   ├── clock/               # Monotonic virtual clock & priority time-wheel
│   │   ├── mod.rs           # VirtualClock with 100 ns ticks
│   │   └── wheel.rs         # BinaryHeap deadline min-wheel
│   ├── interceptor/         # Trapping and emulation layer
│   │   ├── bpf.rs           # Classic Seccomp BPF filter compiler
│   │   ├── fault.rs         # Seeded ChaCha20 I/O fault injector
│   │   ├── notif.rs         # Linux seccomp_notif C-struct bindings
│   │   ├── pid.rs           # VirtualIdManager (PID 1000 / PPID 999)
│   │   ├── sync.rs          # Futex wait queues and hash keys
│   │   ├── tsc.rs           # RDTSC / RDTSCP instruction decoder
│   │   └── virtual_ids.rs   # Compatibility PID/TID re-exports
│   ├── launcher/            # Process bootstrap
│   │   ├── mod.rs           # Child fork, affinity, personality, ptrace
│   │   ├── auxv.rs          # Auxiliary vector (AT_SYSINFO_EHDR) scrubber
│   │   └── fd_pass.rs       # SCM_RIGHTS listener transfer over socketpair
│   ├── scheduler/           # Thread selection & instruction quanta
│   │   ├── mod.rs           # Seeded runnable task scheduler
│   │   └── pmu.rs           # Hardware PMU instruction counter interface
│   └── trace/               # Binary trace serialization
│       ├── mod.rs           # TraceWriter and TraceReader
│       └── format.rs        # Binary event layout and magic header
├── tests/                   # Target test fixtures
│   ├── test_target.rs       # Clock and getrandom verification fixture
│   ├── test_timewarp.rs     # Multi-second nanosleep time-warp fixture
│   ├── test_concurrency.rs  # Multi-threaded mutex contention fixture
│   ├── test_rdtsc.rs        # RDTSC / RDTSCP instruction trapping fixture
│   ├── test_epoll.rs        # Multi-second epoll wait fixture
│   ├── test_fault_injection.rs # Seeded I/O error injection fixture
│   ├── test_process_tree.rs # Fork and thread hierarchy fixture
│   └── test_record_replay.rs   # Trace recording and replay parity fixture
├── paper/                   # Academic research paper
│   ├── statis_research_paper.pdf # 7-page technical report by Siddid Pandey
│   ├── statis_research_paper.tex # LaTeX source for 7-page paper
│   ├── paper.pdf            # 5-page archive report
│   └── paper.tex            # LaTeX source for 5-page archive
├── portal/                  # Next.js 16 interactive research portal
│   ├── src/                 # App router pages and UI components
│   ├── integrations/        # VFX-JS Transitions, Lenis, and Three.js CRT
│   ├── scripts/             # Asset sync and LaTeX paper parser
│   └── tests/               # Portal unit tests and oracle parity checks
├── benchmarks_svg/          # Publication-quality benchmark vector charts
│   ├── time_warp_speedup.svg
│   ├── trace_size_comparison.svg
│   └── determinism_matrix.svg
├── scripts/                 # Benchmarking and chart generation tools
│   ├── benchmarks.py        # Automated benchmark runner
│   ├── generate_charts.py   # SVG chart renderer
│   └── test_benchmarks.py   # Regression test suite for benchmark harness
└── docs/                    # Architectural notes & empirical data
    ├── benchmarking.md      # Detailed benchmark methodology & history
    ├── benchmark_results.json # Raw benchmark samples & machine metadata
    └── interface.md         # Terminal presentation notes
```

---

## Verification & Testing

The repository maintains strict verification across three independent test suites:

### 1. Rust Subsystem Test Suite
```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```
Tests unit behaviors for auxiliary vector scrubbing, Seccomp BPF routing, ChaCha20 random streams, fault configs, futex queues, time-wheel heap ordering, and trace serialization roundtrips.

### 2. Python Benchmark Suite
```bash
python3 -B -m unittest discover -s scripts -p 'test_*.py' -v
```
Validates failure isolation, timeout handling, process group cleanup, partial trace rejection, and decimal/binary unit conversions.

### 3. Portal Verification Suite
```bash
PATH=/usr/bin:/bin npm --prefix portal test
PATH=/usr/bin:/bin npm --prefix portal run typecheck
```
Ensures 100% SHA-256 byte parity of synchronized assets, PDF integrity, internal link resolution across all five routes, and cross-language RNG parity against real Rust oracle fixtures.

---

## Known Limits & Boundaries

Following the principle of epistemic restraint documented in [docs/interface.md](docs/interface.md) and [docs/benchmarking.md](docs/benchmarking.md):

1. **PMU Preemption Loop**: The hardware PMU interface and `--quantum` argument are implemented; continuous instruction-count preemption loops for arbitrary compute-bound thread interleavings remain under active development.
2. **Timeout Emulation vs. Descriptor Readiness**: Emulation for `poll`, `ppoll`, and `epoll_wait` synthesizes timeout completion. It does not simulate full kernel-side network packet arrival or file descriptor readiness.
3. **Futex Keys**: Futexes are currently keyed on virtual memory addresses. Cross-process shared memory futexes with identical virtual addresses require additional process-level mapping.
4. **Target Memory Restoration**: The trace format records configuration, system call responses, timestamp reads, and thread exits. It does not restore target heap memory state on replay.
5. **Sandbox Limitations**: Containerized environments lacking `CAP_SYS_PTRACE` or restricting nested seccomp notifications cannot execute the full supervision lifecycle.

---

## Citation

If you use Statis in academic research or systems engineering projects, please cite:

```bibtex
@article{pandey2026statis,
  title     = {Statis: High-Performance Seed-Deterministic Userspace Supervision for Unmodified Linux Binaries},
  author    = {Pandey, Siddid},
  journal   = {Systems Research Technical Report},
  year      = {2026},
  url       = {https://github.com/qvost/statis},
  note      = {Research Portal: \url{https://statis-linux.vercel.app}}
}
```

---

## License & Author

**Statis** is authored by **Siddid Pandey** ([@qvost](https://github.com/qvost)) and licensed under the [MIT License](LICENSE).  

- GitHub: [https://github.com/qvost](https://github.com/qvost)  
- LinkedIn: [https://linkedin.com/in/qvost](https://linkedin.com/in/qvost)  
- Email: [qvostwon@gmail.com](mailto:qvostwon@gmail.com)  

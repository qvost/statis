# Observed verification — 2026-11-12

Status: **working, statically prerendered recovery build; full graphics installation and browser acceptance remain blocked.** Do not treat this checkout as a fully verified production release.

## Runtime and dependency environment

Observed executables:

```text
/usr/bin/node: v26.13.1
/usr/bin/npm: 12.2.2 (when /usr/bin precedes fnm in PATH)
fnm shell default: v20.20.2 / npm 10.8.2
```

All build/check commands used `PATH=/usr/bin:/bin`.

Registry/network attempts:

```text
curl: (6) Could not resolve host: registry.npmjs.org
npm error code ENOTCACHED
npm error request to https://registry.npmjs.org/@vfx-js/effects/-/effects-1.4.4.tgz failed: cache mode is 'only-if-cached' but no cached response is available.
```

User authorization to enable network access did not change the executor's enforced network policy. Seven requested packages remain unavailable: `@vfx-js/core`, `@vfx-js/react`, `@vfx-js/transitions`, `@gsap/react`, `lenis`, `three`, `@types/three`. They are declared dependencies. No replacement package or stub is installed under those names. Full adapter syntax transpilation passes, but their external-package type compatibility and runtime behavior could not be verified.

The exact observed `npm --prefix portal run verify:stack` diagnostic is:

```text
Full graphics stack unavailable: @vfx-js/core, @vfx-js/react, @vfx-js/transitions, @gsap/react, lenis, three, @types/three.
Run npm install with registry access, then rebuild. Offline builds use explicit native/Canvas fallbacks; this is not a verified full-stack release.
```

PP Neue Montreal / Inter / Fragment Mono font binaries are not available. Local font-face declarations fall back to system fonts without failed HTTP requests. Supplied reference component pages were inaccessible; local components implement the supplied specification, without claiming a pixel-verified reproduction.

## Production build and TypeScript

Command: `npm --prefix portal run build` — successfully completed the optimized production build with build-time TypeScript checking enabled. Latest exact observed final output:

```text
✓ Compiled successfully in 736ms
  Running TypeScript ...
  Finished TypeScript in 1559ms ...
  Collecting page data using 2 workers ...
  Generating static pages using 2 workers (0/7) ...
  Generating static pages using 2 workers (1/7)
  Generating static pages using 2 workers (3/7)
  Generating static pages using 2 workers (5/7)
✓ Generating static pages using 2 workers (7/7) in 247ms
  Finalizing page optimization ...
  Collecting build traces ...

Route (app)
┌ ○ /
├ ○ /_not-found
├ ○ /architecture
├ ○ /benchmarks
├ ○ /fixtures
└ ○ /paper

○  (Static)  prerendered as static content
```

`experimental.useTypeScriptCli: false` uses the supported compiler API with full type checking. This executor returns empty captured output from detached TypeScript CLI subprocesses, which made the default Next configuration report `Could not parse output from TypeScript's --showConfig`. The compiler-API setting resolves that environment issue. The build's experiment configuration display is informational.

One later build attempt reported `Another next build process is already running`; the existing build lock was left alone, and generated artifacts were preserved. A sequential retry was used.

Commands: `npm --prefix portal run typecheck` and `npx --prefix portal tsc --noEmit --project portal/tsconfig.json` — exit 0, no TypeScript diagnostics. The bare requested `npx --prefix portal tsc --noEmit` prints compiler help from the repository root under npm 12; the explicit project form actually checks the portal.

## Portal assertions

Command: `npm --prefix portal test`:

```text
✔ All synchronized artifacts are byte-faithful to their SHA-256 manifest
✔ All SVGs have nonzero intrinsic dimensions and the supplied PDFs are intact
✔ Five built routes resolve every local link and resource to a generated route or asset
✔ Saved benchmark numbers preserve incomplete supervised results and exact native lock arrays
✔ ChaCha20 matches Rust across block boundaries / seed 0
✔ Fault outcomes match actual FaultConfig::should_fault / seed 0
✔ ChaCha20 matches Rust across block boundaries / seed 42
✔ Fault outcomes match actual FaultConfig::should_fault / seed 42
✔ ChaCha20 matches Rust across block boundaries / seed 3735928559
✔ Fault outcomes match actual FaultConfig::should_fault / seed 3735928559
✔ ChaCha20 matches Rust across block boundaries / seed 18446744073709551615
✔ Fault outcomes match actual FaultConfig::should_fault / seed 18446744073709551615
✔ Seed range preserves unsigned 64-bit values without number rounding
✔ Dissolve order is a repeatable permutation and changes with seed
✔ CLI modes expose only flags accepted by the actual Clap variants
✔ Trace reader matches the real Rust trace-dump for all three event tags
✔ Trace reader rejects truncated records, unknown tags, oversized files, and invalid magic
✔ Both LaTeX readers retain sections, equations, figures, tables, pending studies and references
ℹ tests 18
ℹ suites 0
ℹ pass 18
ℹ fail 0
ℹ cancelled 0
ℹ skipped 0
ℹ todo 0
```

Timing suffixes vary per run and are omitted above; assertion names and counts are exact. Golden RNG and fault data were generated using the actual Rust `rand_chacha 0.3.1`, `rand_core 0.6.4`, and `statis::interceptor::FaultConfig`. The format fixture was written by actual `TraceWriter`, then dumped with the real release `statis trace-dump`. Oracle regeneration also completed:

```text
Generated four Rust ChaCha20 / FaultConfig vectors and a TraceWriter format fixture.
```

The generated sources include 42 repository artifacts plus two author-supplied artifacts (the supplied PDF also exists in the repository artifact set), eight Cargo fixture programs, 98 blocks in the primary seven-page reader, and the earlier report's full parsed reader.

PDFs inspected with `pdfinfo`:

| Artifact | Pages | Bytes | SHA-256 |
| --- | ---: | ---: | --- |
| Original `paper/paper.pdf` | 5 | 112000 | `13d02fb69114b04d4c062db595fba9c35041b9dd0694712fccc8f2d8e9873b6f` |
| Supplied `statis_research_paper.pdf` | 7 | 418907 | `1ab09985ce76a028832e20995f4373c0a0af897ca0c10ee450fbfee50f753279` |

All three SVGs retain nonzero intrinsic dimensions, accessible titles, and exact source bytes. Every generated route's local asset/link target exists. These are filesystem assertions, **not** observed HTTP responses or browser bounding boxes.

## Server and Chromium — blocked, not passed

Command: `npm --prefix portal run start` — exit 1:

```text
⨯ Failed to start server
Error: listen EPERM: operation not permitted 127.0.0.1:3000
    at ignore-listed frames {
  code: 'EPERM',
  errno: -1,
  syscall: 'listen',
  address: '127.0.0.1',
  port: 3000
}
```

Command: `npm --prefix portal run test:browser` — exit 1, Chromium launch fails before any page can be opened:

```text
browserType.launch: Target page, context or browser has been closed
FATAL:content/browser/sandbox_host_linux.cc:41] Check failed: . shutdown: Operation not permitted (1)
Browser verification: blocked_or_failed; 0 checks.
```

The browser report is at ignored `portal/test-results/browser-report.json`. The executable suite is retained in `tests/browser.mjs`. No claim is made of HTTP 200 responses, zero runtime console errors/404s, desktop/mobile bounding boxes or overflow, real shader rendering, boot count/replay, route-transition behavior, or live header theme flips. Those checks require an unrestricted server/browser run.

## Fresh benchmark attempt

Command actually run from the repository root:

```sh
python3 scripts/benchmarks.py --samples 1 --output /tmp/statis-portal-host-benchmark.json --assets-dir /tmp/statis-portal-host-charts
```

The output/asset overrides preserve the supplied three-sample report and original SVGs. They do not change workloads or measurement behavior. This executor could not run outside its enforced sandbox. Exit 0 means report generation succeeded; it does not mean supervised execution succeeded.

Exact observed summary:

```text
Median wall-clock time, including process startup (ms)
| Workload | Native ms | statis ms | Speedup | Overhead |
| --- | ---: | ---: | ---: | ---: |
| test_timewarp | 10001.037 | error | n/a | n/a |
| test_epoll | 15015.708 | error | n/a | n/a |
| test_concurrency | 0.870 | error | n/a | n/a |

Trace: error; no valid size measured
rr reference: ~45 MB/min (assumed; different workload and duration).
Report: /tmp/statis-portal-host-benchmark.json
Charts: /tmp/statis-portal-host-charts
Status: incomplete
```

Each supervised fixture reported `statis: expected SIGTRAP at target exec, got Exited(Pid(...), 1)`. Actual native seconds: timewarp `10.001037478`, epoll `15.015707538`, concurrency `0.000870364`. Supervised medians and trace sizes remain null. The raw setup report is retained in `data/host-attempt.json`, separately from the original three-sample baseline. No measured SVGs were regenerated from fabricated supervised values.

## Rust hygiene

The requested commands completed successfully:

```text
cargo fmt --check
[exit 0, no output]

cargo clippy --all-targets -- -D warnings
    Checking statis v0.1.0 (/home/qvost/Projects/statis)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.97s

cargo test --all-targets
    Finished `test` profile [unoptimized + debuginfo] target(s) in 5.08s
running 116 tests
test result: ok. 116 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
```

The main binary and each of the eight fixture targets also reported `running 0 tests` with successful results, as defined by the crate. Capability-sensitive tests can skip assertions internally when the sandbox denies operations; the passing unit suite does not establish completed supervision. Rust source files, Cargo.toml, and Cargo.lock were not modified by this portal implementation.

## Remaining acceptance work

Run `npm run setup:graphics` with registry access, confirm the resulting package versions and adapter typecheck, and run `npm run verify` with a usable Chromium/server environment. Supply licensed/self-hosted typography for exact font rendering. Run benchmarks on an unrestricted Linux host before publishing supervised timing or trace-size claims. The existing suite deliberately fails the full-stack gate while prerequisites remain unavailable.

## Editorial theme revision — current follow-up

Changes: one shared paper/charcoal/moss theme across the five routes; a home-page halftone wave with pause and reduced-motion support; section-level, smoothed hero pointer tracking across foreground text; a new cache-distinct favicon URL; ZoomBlur in the selector and selected architecture/paper/benchmark links, with a native recovery approximation.

Observed revision checks:

```text
npm --prefix portal run build
✓ Compiled successfully in 1462ms
  Finished TypeScript in 1610ms ...
✓ Generating static pages using 2 workers (7/7) in 273ms
[all five routes statically prerendered; exit 0]

npm --prefix portal run typecheck
[exit 0; no diagnostics]

npm --prefix portal test
ℹ tests 18
ℹ pass 18
ℹ fail 0

git diff --check
[no diagnostics]

npm --prefix portal run test:browser
Browser verification: blocked_or_failed; 0 checks.
```

Chromium still cannot launch under the enforced sandbox. The browser suite now includes continuous pointer movement over hero text without text displacement, shared page background colors, the new favicon endpoint, halftone-wave layout/pause, and the updated paper-surface header behavior. These browser assertions remain unexecuted. VFX-JS/Lenis/Three.js registry dependencies remain unavailable, so real ZoomBlur shader rendering is not claimed as verified.

## Homepage stability and dissolve removal

Removed all six overview PixelDissolve instances, eliminating 900 animated cells and their six scroll triggers from that route. Applied on-demand, cached hero drawing; opt-in lower-resolution liquid rendering; static homepage CRT and click-only target changes; batched, lower-density halftone drawing; static overview research prose/pillars; and reduced header geometry reads. The wave preserves its current shape when paused. Detail-page dissolve components remain in place.

The production build, TypeScript check, and 18 existing assertions passed, including a new built-route assertion that the overview contains no pixel bands. Browser regressions now check idle hero repaint cessation, opt-in liquid behavior, and click-only CRT target selection. Chromium launch remains blocked by sandbox restrictions; no observed frame-rate or interaction-latency improvement is claimed.

## Reader-first and editorial finishing pass

The final Paper section is now the lazily loaded PDF iframe and its open/download actions. The primary web reader and collapsible archive precede it. The overview's decorative metadata banners are omitted at render time, while source access, controls and benchmark caveats remain. Shared page introductions no longer show the portal version. Overview charts expand deliberately on click, and navigation exposes aria-current.

Observed checks:

```text
Production build: exit 0; compiled successfully in 1397ms
Build-time TypeScript: finished in 1500ms
Static generation: 7/7 in 364ms; all five routes generated
Standalone TypeScript: exit 0, no diagnostics
Portal suite: 18 passed, 0 failed
Browser script syntax: passed
git diff --check: no diagnostics
Decorative version stamp present: false
Overview research counter present: false
Reader precedes final PDF: true
Lazy PDF iframe: true
```

The generated-route checks cover the new ordering and absence of decorative labels. Live visual verification remains subject to the previously documented Chromium sandbox restriction; no new browser pass is claimed.

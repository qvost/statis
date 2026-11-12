# Statis Research Portal

Independent Next.js App Router + TypeScript application. Run commands from the Rust repository root; the crate does not depend on Node or the portal.

```sh
# Node 26.13.1 / npm 12.2.2. This host's fnm shell defaults to Node 20:
export PATH=/usr/bin:/bin:$PATH
npm --prefix portal install
npm --prefix portal run dev
npm --prefix portal run build
npm --prefix portal run typecheck
npm --prefix portal test
npm --prefix portal run start
```

`npx --prefix portal tsc --noEmit` runs from the caller's working directory in npm 12. From the repository root, use `npx --prefix portal tsc --noEmit --project portal/tsconfig.json`.

The development and production servers bind loopback by default. Use the normal Next CLI flags for other hosting arrangements. No deployment, account, backend execution, or analytics is configured.

## Full graphics stack and offline recovery

The requested VFX-JS, Lenis, GSAP React, and Three.js packages were absent from the local cache. This executor blocks network DNS, TCP listeners, and Chromium startup. The application builds using explicit native/Canvas fallbacks. **This is not yet a verified full-stack production release.**

With registry access:

```sh
cd portal
npm run setup:graphics
npm run verify:stack
npm run build
npm run typecheck
npm test
# In a second terminal: npm run start
npm run test:browser
```

The requested effects are declared dependencies; `verify:stack` and `verify` fail if any are missing. The generated offline recovery imports let existing cached foundation packages build while registry installation remains blocked. `setup:graphics` installs current real packages and records exact versions in package.json/package-lock.json once the registry is reachable. Do not represent an offline fallback build as testing the unavailable packages. The full adapters in `integrations/` follow the [VFX-JS transition API](https://github.com/fand/vfx-js/blob/main/docs/TRANSITION.md); their installed-package type compatibility and runtime rendering remain unverified until registry access is available.

`predev` / `prebuild` run `scripts/sync-assets.mjs`. The script checks installed packages and generates imports for the full adapters or explicit fallbacks. No stub package, CDN bundle, external GLB, image demo, video, API key, or registry credential is used. No authorized `@compronents` registry entry was present. Components are independent local implementations of the requested visual specification; unavailable reference pages could not be visually inspected.

Typography requests PP Neue Montreal, Inter, and Fragment Mono through local font faces, then falls back to system sans/monospace. These font binaries were not supplied or cached. To self-host licensed fonts, put them in `public/fonts/` and replace the `local()` sources in `globals.css` with matching local URLs. No broken remote font requests are emitted.

## Content provenance

- `/`, `/architecture`, `/benchmarks`, `/fixtures`, `/paper` are statically prerendered.
- The sync script reads `Cargo.toml` to enumerate all eight fixture binaries, embeds their actual Rust sources, and serves downloadable source files for every module.
- It copies the three repository SVGs, original PDF/TeX, benchmark JSON, methodology, interface notes, README, Cargo manifest and MIT license. `public/asset-manifest.json` records their SHA-256 hashes.
- The supplied paper at `/home/qvost/Downloads/statis_research_paper.pdf` is **seven pages**, distinct from the **five-page** repository report. Its matching `.tex` was found alongside it. Both author-supplied files are preserved in `data/paper/`, then copied reproducibly to `public/paper/`; building does not depend on Downloads. The supplied report is the primary viewer/download. Both versions have complete native readers parsed from their respective TeX sources. Original claims, pending studies, comparison tables, and layout-placeholder coordinates retain their provenance.
- The original repository `paper/paper.pdf` remains at that exact public URL. Existing root paper files are untouched.
- The original three-sample `docs/benchmark_results.json` is preserved. No supervised measurements were invented. All attempt stdout/stderr, exact lock arrays, medians, trace errors and methodology are inspectable.
- `data/host-attempt.json` is the independent one-sample setup attempt. It is displayed separately; the command exited zero because report generation completed, while measurement status remained `incomplete`.
- The 872× illustration, rr 45,200 KB, Hermit 1,250 KB and Statis 0.14 KB are reference scenarios. The paper’s reported timings and placeholder storage coordinates do not become portal benchmark measurements. Fixed-field trace serialization has no delta compression.

## Interactions and safeguards

Eight SVG text orbits count to 100. Skip, replay, Escape, reduced-motion bypass, a JavaScript deadline, and a CSS deadline prevent a stuck boot overlay. The main DOM is always explicitly visible. Every shader is decorative; source text, figures, CLI commands and report content remain readable without graphics.

The complete stack uses one GSAP ticker for Lenis and ScrollTrigger, a VFX App Router outlet and `TransitionIn`, link `transition()` specs, and a separate `useVFXTransition().run()` for boot. Presets have live controls. Pixel bands are 25×6 and use a seeded ChaCha20 Fisher–Yates permutation. The liquid background is six actual WebGL2 framebuffer passes. The CRT builds its own curved Three.js mesh and canvas texture; the recovery renderer uses Canvas2D. The QVOST halftone ground is procedural Canvas2D with no media dependencies.

The fixture atlas has a centered live minimap, native momentum scrolling and restrained glyph parallax. The CLI builder respects run-only `--summary`, record configuration, replay header settings, trace-dump arguments, decimal Clap seed syntax, eight errno variants and the `etimeout` alias. The fault preview exactly matches Rust’s seed expansion and 64-bit fault rule **for an isolated stream of eligible I/O calls**; it does not run Linux binaries. Runtime getrandom changes shared RNG consumption. Read/write on fd ≤ 2 remain protected.

The trace inspector processes files locally, enforces an 8 MiB limit, rejects malformed/truncated data, and limits display to 5,000 events. Use `statis trace-dump` for larger files. No uploads occur.

## Validation

```sh
npm --prefix portal run build
npm --prefix portal run typecheck
npm --prefix portal test
npm --prefix portal run test:oracle # requires cached Rust deps + release statis
npm --prefix portal run verify:stack
npm --prefix portal run test:browser # needs a running server and Chromium permissions
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

The Node suite uses `--test-isolation=none` because this executor suppresses captured child-process stdout; individual assertions are then visible. It validates golden data generated by **the real Rust libraries**, including ChaCha20 block boundaries, FaultConfig outcomes, and the actual CLI trace dump. The asset suite inspects every built route and local resource target. Intrinsic SVG dimensions and generated route inspection do not establish browser bounding boxes or HTTP 200 responses.

The browser suite checks desktop/mobile routes, boot skip/replay, actual asset responses, chart zoom, visibility, header theme changes, console errors/warnings, overflow, profile URLs, and reduced-motion/no-WebGL recovery. It writes `test-results/browser-report.json`. A blocked launch is a failure, never a pass.

See [VERIFICATION.md](VERIFICATION.md) for observed results and [FILES_CHANGED.md](FILES_CHANGED.md) for the complete file inventory.

## Credits

Visual references: [Settlement Layer Page](https://ui.aryank.space/pages/settlement-layer-page), the named components from [ui.aryank.space](https://ui.aryank.space/), and [Manaslu Labs](https://manaslulabs.xyz/). The research record follows the epistemic restraint documented in `docs/interface.md`. VFX-JS is by AMAGI and licensed MIT. The portal preserves the repository’s MIT license; paper content is reproduced from the user-supplied sources.

## Editorial theme revision

All five routes now share `src/app/research-theme.css`: warm paper, charcoal text, moss accents, serif display headings, and consistent instrument panels. The original SVG figures retain their source colors. The home page includes a procedural [Halftone Wave Hero](https://ui.aryank.space/components/halftone-wave-hero) interpretation with pause, viewport/visibility suspension, and reduced-motion support. Reference pages were unreachable in this executor, so this is a local interpretation, not a visually verified clone.

Hero pointer tracking listens on the enclosing section, so text overlays no longer interrupt the field. Coordinates are relative to the same section, displacement is bounded, and interpolation is independent of the hovered child. Animation loops also pause out of view and avoid duplicate RAF chains.

The favicon is `/statis-mark.svg`. Crossfade is the default navigation mode. ZoomBlur is available in the display selector and explicitly applied to the hero architecture link, the paper navigation/reader links, and the wave's benchmark link. The installed adapter uses VFX-JS `ZoomBlur`; the recovery adapter uses a native blur-and-scale reveal. Both respect reduced motion.

## Homepage performance pass

The overview no longer mounts the six pixel dissolve bands (900 cells). Detail pages retain their existing transitions. The radial field caches 160 rays into eight paths, repaints only while pointer easing is active, caps drawing at 30 fps and DPR at 1.25, and stops when settled. Its pointer coordinates remain relative to the hero, including over text.

Liquid stats are off by default and require explicit opt-in; hovering no longer compiles shaders. Enabled liquid rendering is bounded to 960 × 480 at 20 fps while visible. The home CRT uses a static Canvas display, selects targets on click, and has no hover tilt. The halftone wave batches dot fills per row, uses up to 100 × 24 dots at 20 fps, and preserves its phase when paused. Homepage research copy and pillar paths render without scroll scrubbing. Header scroll updates inspect only the footer boundary, and native ZoomBlur targets the introduction rather than the whole long page.

## Reader-first finishing pass

The Paper route presents the complete web reader first, the collapsible earlier report next, and the original PDF viewer/download as its final section. The PDF iframe is lazy-loaded. Intro links jump directly to the abstract or the PDF section; primary “Read Paper” links open the reader.

The overview omits decorative version stamps, research counters, uppercase section labels, and chart/module tags. Source links, functional controls, reference-scenario wording and benchmark diagnostics remain available. Shared page introductions are simplified, navigation indicates the current page to assistive technology, and overview charts expand on click. The earlier performance changes remain in place.

# Statis Research Paper

This directory contains the academic research papers and LaTeX sources for **Statis**.

## Documents

| Document | Pages | Description | Source | PDF |
| :--- | :---: | :--- | :--- | :--- |
| **Current Technical Report** | 7 | Comprehensive paper by Siddid Pandey detailing the 5 non-determinism vectors, sterile launcher, Seccomp BPF notification IPC, priority time-wheel, and microbenchmarks. | [`statis_research_paper.tex`](statis_research_paper.tex) | [`statis_research_paper.pdf`](statis_research_paper.pdf) |
| **Conference Draft** | 5 | ACM SIGCONF-style 5-page preliminary report. | [`paper.tex`](paper.tex) | [`paper.pdf`](paper.pdf) |

---

## Abstract

> Non-determinism in concurrent software testing remains a fundamental challenge in systems engineering, leading to irreducible flake rates in distributed protocols, databases, and actor systems. Existing record-replay debuggers and deterministic execution frameworks either impose crippling performance overheads, require kernel modifications, or bloat execution traces to gigabytes per second. We present **Statis**, a novel zero-overhead, seed-deterministic userspace runtime supervisor for unmodified Linux x86-64 multi-threaded ELF binaries. Statis neutralizes the five primary vectors of host non-determinism—wall-clock leakage, hardware timestamp counters, kernel entropy, address space layout randomization, and process identity virtualization—at the binary boundary without source recompilation or kernel patches. Leveraging Seccomp BPF filters with new listener IPC, a monotonic priority time-wheel, and hardware PMU preemption, Statis achieves instant time-warping exceeding $500\times$ speedups on idle-heavy workloads while maintaining a compact deterministic trace footprint measured in kilobytes rather than gigabytes.

---

## Compiling the LaTeX Source

To compile `statis_research_paper.tex` into PDF using a standard TeX Live distribution:

```bash
pdflatex statis_research_paper.tex
pdflatex statis_research_paper.tex
```

Required LaTeX packages:
- `geometry`, `microtype`, `amsmath`, `amssymb`, `booktabs`, `graphicx`, `xcolor`
- `enumitem`, `caption`, `algorithm2e`, `tikz`, `pgfplots`, `hyperref`

---

## Citation

```bibtex
@article{pandey2026statis,
  title     = {Statis: High-Performance Seed-Deterministic Userspace Supervision for Unmodified Linux Binaries},
  author    = {Pandey, Siddid},
  journal   = {Systems Research Technical Report},
  year      = {2026},
  url       = {https://github.com/qvost/statis},
  note      = {Interactive Research Portal: \url{https://statis-linux.vercel.app}}
}
```

# Terminal presentation

The interface is a terminal, not a dashboard. Command names, flags, and the
README's subject order stay intact. Target stdout remains separate from the
execution summary on stderr.

## Direction

Product: Linux runtime experiments for systems developers.

Character: precise, restrained, inspectable.

World: terminal instruments and execution logs.

Moments: virtual-time measurements and syscall traces.

Signature: aligned numbers with explicit units.

## Audit and changes

The previous summary used long separator banners and decorative list markers.
Metrics mixed labels, counts, and units. The README described performance and
determinism more broadly than the current implementation could demonstrate.

The summary now groups time, syscalls, faults, and runtime counts using blank
lines and aligned numeric columns. It inherits the terminal's font and colors;
there are no icons or color-only distinctions. Zero elapsed duration omits the
ratio, and missing timing data says `unavailable`. Empty syscall statistics
say `No trapped syscalls`. Trace dumps retain one event per line.

`Virtual / elapsed` names a ratio of simulated time to elapsed time. It does not
claim a speedup against an unsupervised baseline. The README describes current
emulation boundaries and links to the benchmark record instead of presenting
unmeasured latency figures.

The summary can also be written through `ExecutionTelemetry::write_summary`.
This keeps output inspectable without launching a traced process. The default
stderr summary remains best effort so a closed output stream does not replace
the target's exit status.

## Source and adaptation

This presentation pass applies the audit, direction, spacing, hierarchy, and
plain-language guidance from [unslop](https://github.com/heydamir/unslop) by
Damir Sokolovsky, licensed under
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
The guidance was adapted to an existing Rust CLI and Markdown documentation;
browser layouts, imagery, and motion do not apply to this interface. The skill
was consulted rather than vendored or installed as a runtime dependency.

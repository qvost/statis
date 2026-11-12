#!/usr/bin/env python3
"""Generate paper-style SVG charts using only the Python standard library.

Measured values come exclusively from benchmark_results.json. Requested design
figures remain visibly labeled reference scenarios, including on restricted hosts.
"""

import argparse
import json
import math
from pathlib import Path
import xml.etree.ElementTree as ET


ROOT = Path(__file__).resolve().parents[1]
NS = "http://www.w3.org/2000/svg"
BLUE = "#3188bd"
RED = "#c4382b"
ET.register_namespace("", NS)


def node(parent, tag, text=None, **attributes):
    element = ET.SubElement(parent, "{" + NS + "}" + tag,
                            {key.replace("_", "-"): str(value) for key, value in attributes.items()})
    if text is not None:
        element.text = str(text)
    return element


class Chart:
    def __init__(self, height, title, description):
        self.root = ET.Element("{" + NS + "}svg", {
            "width": "800", "height": str(height), "viewBox": "0 0 800 " + str(height),
            "role": "img", "aria-labelledby": "title description",
        })
        node(self.root, "title", title, id="title")
        node(self.root, "desc", description, id="description")
        node(self.root, "style", """
            :root { color-scheme: only light; }
            text { font-family: 'Times New Roman', 'Liberation Serif', 'Nimbus Roman', serif;
                   fill: #111; font-size: 17px; font-weight: 400; }
            .small { font-size: 13px; }
            .note { font-size: 14px; }
            .label { font-size: 21px; }
            .muted { fill: #444; }
            .grid { stroke: #c9c9c9; stroke-width: 0.5; }
            .axis { stroke: #111; stroke-width: 0.8; }
            .native { stroke: #3188bd; }
            .reference, .measured { stroke: #c4382b; }
            .accent { fill: #c4382b; }
        """)
        # An opaque white page preserves the reference figures' appearance in dark READMEs.
        node(self.root, "rect", x=0, y=0, width=800, height=height, fill="white")

    def text(self, x, y, value, css="", anchor="start", **attributes):
        return node(self.root, "text", value, x=x, y=y, text_anchor=anchor,
                    **{"class": css}, **attributes)

    def line(self, x1, y1, x2, y2, css="axis", **attributes):
        return node(self.root, "line", x1=x1, y1=y1, x2=x2, y2=y2,
                    **{"class": css}, **attributes)

    def curve(self, points, css, dashed=False):
        return node(self.root, "polyline", points=" ".join("{:.2f},{:.2f}".format(x, y)
                                                          for x, y in points),
                    fill="none", stroke_width=2.2, stroke_linejoin="round",
                    stroke_dasharray="7 6" if dashed else "none", **{"class": css})

    def marker(self, x, y, color, square=False, hollow=False):
        attributes = {"fill": "white" if hollow else color, "stroke": color, "stroke_width": 1.3}
        if square:
            return node(self.root, "rect", x=x - 3.5, y=y - 3.5, width=7, height=7, **attributes)
        return node(self.root, "circle", cx=x, cy=y, r=3.8, **attributes)

    def frame(self, left, top, right, bottom):
        node(self.root, "rect", x=left, y=top, width=right - left, height=bottom - top,
             fill="none", stroke="#111", stroke_width=0.8)

    def power_label(self, x, y, exponent):
        label = self.text(x, y, "10", anchor="end")
        node(label, "tspan", exponent, dy=-6, font_size=11)

    def log_ticks(self, left, top, right, bottom, low, high, y, grid=True):
        for exponent in range(low, high + 1):
            cy = y(10 ** exponent)
            if grid:
                self.line(left, cy, right, cy, "grid")
            self.line(left, cy, left + 7, cy)
            self.line(right - 7, cy, right, cy)
            self.power_label(left - 12, cy + 5, exponent)
            if exponent < high:
                for multiple in range(2, 10):
                    minor = y(multiple * 10 ** exponent)
                    self.line(left, minor, left + 4, minor, stroke_width=0.4)
                    self.line(right - 4, minor, right, minor, stroke_width=0.4)

    def save(self, path):
        if hasattr(ET, "indent"):
            ET.indent(self.root, space="  ")
        ET.ElementTree(self.root).write(path, encoding="utf-8", xml_declaration=True)
        ET.parse(path)


def time_warp_chart(results, path):
    chart = Chart(400, "Time-warp acceleration",
                  "Paper-style log-scale plot. Native durations are an ideal model; the dashed "
                  "statis curve is an illustrative 872x reference at ten seconds, not an empirical "
                  "result. Solid red circles, if present, are measured statis medians.")
    sweep = results.get("time_warp_sweep", [])
    measured = [(p["virtual_seconds"], p["statis"]["median_seconds"] * 1000)
                for p in sweep if p["statis"].get("median_seconds") is not None]
    native = results.get("workloads", {}).get("test_timewarp", {}).get("native", {})
    native_ms = native.get("median_seconds")
    native_ms = native_ms * 1000 if native_ms is not None else None
    values = [value for _, value in measured] + ([native_ms] if native_ms is not None else [])
    low = min(0, math.floor(math.log10(min(values)))) if values else 0
    high = max(5, math.ceil(math.log10(max(values)))) if values else 5
    left, top, right, bottom = 91, 84, 766, 282

    def x(seconds):
        return left + seconds / 62 * (right - left)

    def y(milliseconds):
        return bottom - (math.log10(milliseconds) - low) / (high - low) * (bottom - top)

    node(chart.root, "rect", x=106, y=12, width=640, height=59, fill="white", stroke="#111", stroke_width=0.8)
    for cx, cy, css, label, square, hollow, dashed in (
        (122, 31, "native", "Native Linux (ideal duration)", True, False, False),
        (441, 31, "reference", "statis (872× reference)", False, True, True),
    ):
        chart.line(cx, cy, cx + 29, cy, css, stroke_width=2, stroke_dasharray="6 4" if dashed else "none")
        chart.marker(cx + 14, cy, BLUE if square else RED, square, hollow)
        chart.text(cx + 40, cy + 5, label)
    if native_ms is not None:
        chart.marker(136, 54, BLUE, square=True, hollow=True)
        chart.text(162, 59, "Native Linux (measured at 10s)", "note")
    if measured:
        chart.marker(455, 54, RED)
        chart.text(481, 59, "statis (measured median)", "note")
    else:
        chart.text(441, 59, "statis measurements unavailable", "note muted")

    chart.log_ticks(left, top, right, bottom, low, high, y)
    for seconds in (1, 5, 10, 30, 60):
        chart.line(x(seconds), bottom - 7, x(seconds), bottom)
        chart.line(x(seconds), top, x(seconds), top + 7)
        chart.text(x(seconds), 306, seconds, anchor="middle")
    for seconds in range(0, 61, 5):
        if seconds not in (1, 5, 10, 30, 60):
            chart.line(x(seconds), bottom - 4, x(seconds), bottom, stroke_width=0.4)
            chart.line(x(seconds), top, x(seconds), top + 4, stroke_width=0.4)
    chart.frame(left, top, right, bottom)
    chart.text(428, 334, "workload virtual duration (seconds)", "label", "middle")
    chart.text(26, 184, "wall-clock elapsed time (ms)", "label", "middle", transform="rotate(-90 26 184)")

    # Sample the ideal model densely so a log axis does not introduce artificial corners.
    chart.curve([(x(i / 2), y(i / 2 * 1000)) for i in range(2, 121)], "native")
    reference_ms = 10_000 / 872
    chart.curve([(x(1), y(reference_ms)), (x(60), y(reference_ms))], "reference", True)
    for seconds in (1, 5, 10, 30, 60):
        chart.marker(x(seconds), y(seconds * 1000), BLUE, square=True)
        chart.marker(x(seconds), y(reference_ms), RED, hollow=True)
    chart.text(470, y(reference_ms) - 12, "872× at 10s; 11.47 ms reference", "note accent")
    # Never connect missing samples in a way that suggests observed scaling.
    for seconds, milliseconds in measured:
        chart.marker(x(seconds), y(milliseconds), RED)
    if native_ms is not None:
        chart.marker(x(10), y(native_ms), BLUE, square=True, hollow=True)
    speedup = results.get("workloads", {}).get("test_timewarp", {}).get("speedup")
    footer = ("Measured 10s sleep: {:.1f}× native / statis.".format(speedup)
              if speedup is not None else "Supervised measurement unavailable; dashed curve is illustrative, not observed.")
    chart.text(91, 363, footer, "note")
    chart.text(91, 385, "Logarithmic Y axis. Reference exceeds 800× at plotted durations ≥10s.", "small muted")
    chart.save(path)


def trace_chart(results, path):
    chart = Chart(350, "Trace storage comparison",
                  "Paper-style log-scale bars. Supplied reference scenarios: Mozilla rr 45200 "
                  "decimal KB, Meta Hermit 1250 KB, and statis 0.14 KB (140 bytes). These are not "
                  "matched-workload measurements. Actual fixture size is reported separately.")
    left, top, right, bottom = 91, 43, 766, 230

    def y(kilobytes):
        return bottom - (math.log10(kilobytes) + 2) / 8 * (bottom - top)

    chart.text(91, 22, "Supplied storage reference scenario", "note")
    chart.log_ticks(left, top, right, bottom, -2, 6, y)
    for cx, name, kb, label, color in (
        (212, "Mozilla rr", 45200, "45,200 KB (45.2 MB)", BLUE),
        (427, "Meta Hermit", 1250, "1,250 KB (1.25 MB)", "#d5d5d5"),
        (642, "statis", 0.14, "0.14 KB (140 bytes)", RED),
    ):
        node(chart.root, "rect", x=cx - 30, y=y(kb), width=60, height=bottom - y(kb),
             fill=color, stroke="#111", stroke_width=0.7)
        chart.text(cx, y(kb) - 10, label, "note", "middle")
        chart.text(cx, 256, name, anchor="middle")
        chart.line(cx, top, cx, top + 6)
        chart.line(cx, bottom - 6, cx, bottom)
    chart.frame(left, top, right, bottom)
    chart.text(26, 137, "trace storage (KB)", "label", "middle", transform="rotate(-90 26 137)")
    chart.text(595, 72, "≈320,000× reference reduction", "note accent", "middle")
    chart.text(91, 281, "45,200 / 0.14 = 322,857×; decimal KB (1,000 bytes), logarithmic Y axis.", "note")
    trace = results.get("trace", {})
    actual = ("Measured statis fixture: {:,} bytes ({:.3f} KB).".format(trace["size_bytes"], trace["size_kb"])
              if trace.get("size_bytes") is not None else "Measured statis fixture: unavailable; failed recordings are excluded.")
    chart.text(91, 306, actual, "note")
    chart.text(91, 331, "Reference bars are not a matched-workload benchmark; 140 bytes is unverified.", "small muted")
    chart.save(path)


def matrix_chart(results, path):
    chart = Chart(320, "Determinism capability matrix",
                  "Paper-style capability table with pale red cells for supported features. Native "
                  "Linux, Mozilla rr and Meta Hermit require neither kernel modifications nor source "
                  "recompilation. rr, Hermit and statis handle RDTSC. Unknown properties are marked ?.")
    centers = (386, 495, 604, 713)
    chart.line(24, 22, 768, 22, stroke_width=1.6)
    chart.text(24, 49, "Capability")
    for cx, label in zip(centers, ("Native Linux", "Mozilla rr", "Meta Hermit", "statis")):
        chart.text(cx, 49, label, anchor="middle")
    chart.line(24, 62, 768, 62, stroke_width=0.8)
    speedups = [w["speedup"] for name, w in results.get("workloads", {}).items()
                if name in ("test_timewarp", "test_epoll") and w.get("speedup") is not None]
    warp = ("Yes" if min(speedups) > 500 else "No") if len(speedups) == 2 else "?"
    size = results.get("trace", {}).get("size_bytes")
    footprint = ("Yes" if size < 1_000_000 else "No") if size is not None else "?"
    rows = (
        ("Zero Kernel Modifications", ("Yes", "Yes", "Yes", "Yes")),
        ("Zero Source Recompilation", ("Yes", "Yes", "Yes", "Yes")),
        ("Instant Time-Warping (>500×)", ("No", "?", "?", warp)),
        ("Kilobyte Trace Footprint", ("—", "?", "?", footprint)),
        ("Hardware RDTSC Trap & Emulate", ("No", "Yes", "Yes", "Yes")),
        ("Seeded Deterministic Fault Injection", ("No", "?", "?", "Yes")),
    )
    for index, (label, statuses) in enumerate(rows):
        cy = 86 + index * 32
        chart.text(24, cy + 5, label)
        for cx, status in zip(centers, statuses):
            if status == "Yes":
                node(chart.root, "rect", x=cx - 49, y=cy - 14, width=98, height=27, fill="#f0cfcb")
            chart.text(cx, cy + 5, status, anchor="middle")
    chart.line(24, 268, 768, 268, stroke_width=1.6)
    chart.text(24, 290, "Shading = documented or implemented support; ? = unverified; — = not applicable.", "note")
    chart.text(24, 312, "Speed and size depend on workload. Sources and runtime limits: docs/benchmarking.md.", "small muted")
    chart.save(path)


def generate_charts(results, assets_dir):
    assets_dir = Path(assets_dir)
    assets_dir.mkdir(parents=True, exist_ok=True)
    for name, generate in (("time_warp_speedup", time_warp_chart),
                           ("trace_size_comparison", trace_chart),
                           ("determinism_matrix", matrix_chart)):
        generate(results, assets_dir / (name + ".svg"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--results", type=Path, default=ROOT / "docs/benchmark_results.json")
    parser.add_argument("--assets-dir", type=Path, default=ROOT / "benchmarks_svg")
    args = parser.parse_args()
    results = json.loads(args.results.read_text(encoding="utf-8"))
    generate_charts(results, args.assets_dir)
    print("Generated three SVG charts in " + str(args.assets_dir))


if __name__ == "__main__":
    main()

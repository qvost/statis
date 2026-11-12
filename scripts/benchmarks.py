#!/usr/bin/env python3
"""Benchmark real fixtures with Python's standard library (Linux x86-64).

Builds release binaries with Cargo, times complete processes with perf_counter,
and keeps failed attempts out of medians and ratios. Host restrictions produce
an explicit incomplete report; --strict makes that a nonzero exit for CI.
"""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import signal
import statistics
import subprocess
import sys
import tempfile
import time

from generate_charts import generate_charts


ROOT = Path(__file__).resolve().parents[1]
BINARIES = ("statis", "test_timewarp", "test_epoll", "test_concurrency", "test_record_replay")
DURATIONS = (1, 5, 10, 30, 60)


def run_command(command, timeout, cwd=ROOT):
    """Capture diagnostics and kill the entire target process group on timeout."""
    command = [str(part) for part in command]
    start = time.perf_counter_ns()
    try:
        process = subprocess.Popen(
            command, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            start_new_session=True,
        )
    except OSError as error:
        return {"command": command, "status": "error", "returncode": None,
                "elapsed_seconds": (time.perf_counter_ns() - start) / 1e9,
                "stdout": "", "stderr": str(error)}
    timed_out = False
    try:
        stdout, stderr = process.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        timed_out = True
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        stdout, stderr = process.communicate()
    elapsed = (time.perf_counter_ns() - start) / 1e9
    return {
        "command": command,
        "status": "timeout" if timed_out else ("ok" if process.returncode == 0 else "error"),
        "returncode": process.returncode, "elapsed_seconds": elapsed,
        "stdout": stdout.decode("utf-8", errors="replace"),
        "stderr": stderr.decode("utf-8", errors="replace"),
    }


def measure(command, samples, timeout):
    attempts = []
    for _ in range(samples):
        attempts.append(run_command(command, timeout))
        if attempts[-1]["status"] != "ok":
            break
    complete = len(attempts) == samples and all(a["status"] == "ok" for a in attempts)
    return {
        "status": "ok" if complete else attempts[-1]["status"],
        "requested_samples": samples, "attempts": attempts,
        "median_seconds": statistics.median(a["elapsed_seconds"] for a in attempts)
        if complete else None,
    }


def ratio(numerator, denominator):
    if numerator is None or denominator is None or denominator <= 0:
        return None
    return numerator / denominator


def build_binaries(bin_dir):
    if bin_dir is not None:
        paths = {name: bin_dir.resolve() / name for name in BINARIES}
        build = {"status": "provided", "command": None}
    else:
        print("Building release fixtures (not included in timings)...", file=sys.stderr, flush=True)
        build = run_command(
            ["cargo", "build", "--release", "--bins", "--message-format=json"], 600,
        )
        paths = {}
        for line in build.pop("stdout").splitlines():
            try:
                event = json.loads(line)
            except ValueError:
                continue
            name = event.get("target", {}).get("name")
            if name in BINARIES and event.get("executable"):
                paths[name] = Path(event["executable"])
        if build["status"] != "ok":
            raise RuntimeError("release build failed: " + build["stderr"])
    for name in BINARIES:
        path = paths.get(name)
        if path is None or not path.is_file() or not os.access(path, os.X_OK):
            raise RuntimeError("missing executable: " + str(path or name))
    return paths, build


def collect(paths, samples, timeout, seed):
    results = {"workloads": {}, "time_warp_sweep": []}
    for name, virtual_seconds in (("test_timewarp", 10), ("test_epoll", 15),
                                  ("test_concurrency", None)):
        print("Measuring {} ({} sample(s) per mode)...".format(name, samples),
              file=sys.stderr, flush=True)
        target = [paths[name]]
        native = measure(target, samples, timeout)
        supervised = measure([paths["statis"], "run", "--seed", seed,
                              "--summary=false", "--"] + target, samples, timeout)
        speedup = ratio(native["median_seconds"], supervised["median_seconds"])
        overhead = ratio(supervised["median_seconds"], native["median_seconds"])
        results["workloads"][name] = {
            "virtual_seconds": virtual_seconds, "native": native, "statis": supervised,
            "speedup": speedup, "overhead_factor": overhead,
            "overhead_percent": (overhead - 1) * 100 if overhead is not None else None,
        }

    main_sleep = results["workloads"]["test_timewarp"]["statis"]
    for seconds in DURATIONS:
        if seconds == 10:
            measurement = main_sleep
        elif main_sleep["status"] != "ok":
            measurement = {"status": "skipped", "median_seconds": None, "attempts": [],
                           "reason": "ten-second supervised fixture did not complete"}
        else:
            print("Measuring supervised {}s nanosleep...".format(seconds),
                  file=sys.stderr, flush=True)
            measurement = measure([paths["statis"], "run", "--seed", seed,
                                   "--summary=false", "--", paths["test_timewarp"], seconds],
                                  samples, timeout)
        results["time_warp_sweep"].append({
            "virtual_seconds": seconds, "native_ideal_seconds": seconds,
            "statis": measurement,
        })

    print("Recording trace fixture...", file=sys.stderr, flush=True)
    with tempfile.TemporaryDirectory(prefix="statis-benchmark-") as directory:
        trace_path = Path(directory) / "record_replay.trace"
        recording = run_command([paths["statis"], "record", "--trace", trace_path,
                                 "--seed", seed, "--", paths["test_record_replay"]], timeout)
        size = None
        validation = None
        if recording["status"] == "ok":
            if trace_path.is_file() and trace_path.stat().st_size > 0:
                validation = run_command([paths["statis"], "trace-dump", trace_path], timeout)
                if validation["status"] == "ok":
                    size = trace_path.stat().st_size
            if size is None:
                recording["status"] = "invalid_trace"
        results["trace"] = {
            "status": recording["status"], "recording": recording,
            "validation": validation, "size_bytes": size,
            "size_kb": size / 1000 if size is not None else None,
            "size_kib": size / 1024 if size is not None else None,
            "rr_reference_bytes_per_minute": 45_000_000,
            "rr_one_minute_reference_ratio": ratio(45_000_000, size),
            "comparison_note": "Assumed rr rate, not a matched-workload measurement; "
                               "the short fixture is not a one-minute recording.",
        }
    return results


def print_summary(results):
    def elapsed(measurement):
        seconds = measurement["median_seconds"]
        return "{:.3f}".format(seconds * 1000) if seconds is not None else measurement["status"]

    print("\nMedian wall-clock time, including process startup (ms)")
    print("| Workload | Native ms | statis ms | Speedup | Overhead |")
    print("| --- | ---: | ---: | ---: | ---: |")
    for name, workload in results["workloads"].items():
        speedup = "{:.2f}x".format(workload["speedup"]) if workload["speedup"] else "n/a"
        overhead = ("{:+.1f}%".format(workload["overhead_percent"])
                    if workload["overhead_percent"] is not None else "n/a")
        print("| {} | {} | {} | {} | {} |".format(
            name, elapsed(workload["native"]), elapsed(workload["statis"]), speedup, overhead))
        for mode in ("native", "statis"):
            measurement = workload[mode]
            if measurement["status"] != "ok":
                print("{} {}: {}".format(name, mode, measurement["attempts"][-1]["stderr"].strip()),
                      file=sys.stderr)
    trace = results["trace"]
    print("\nTrace: " + ("{} bytes ({:.3f} KB / {:.3f} KiB)".format(
        trace["size_bytes"], trace["size_kb"], trace["size_kib"])
        if trace["size_bytes"] is not None else trace["status"] + "; no valid size measured"))
    print("rr reference: ~45 MB/min (assumed; different workload and duration).")
    if trace["rr_one_minute_reference_ratio"] is not None:
        print("One minute of that reference / this fixture: {:.1f}x (illustrative only).".format(
            trace["rr_one_minute_reference_ratio"]))
    else:
        print(trace["recording"]["stderr"].strip(), file=sys.stderr)


def positive_int(value):
    number = int(value)
    if number < 1:
        raise argparse.ArgumentTypeError("must be a positive integer")
    return number


def positive_seconds(value):
    number = float(value)
    if not math.isfinite(number) or number <= 0:
        raise argparse.ArgumentTypeError("must be a finite positive number")
    return number


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--samples", type=positive_int, default=3)
    parser.add_argument("--timeout", type=positive_seconds, default=90,
                        help="per-process timeout in seconds (default: 90)")
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--bin-dir", type=Path, help="use prebuilt binaries instead of building")
    parser.add_argument("--output", type=Path, default=ROOT / "docs/benchmark_results.json")
    parser.add_argument("--assets-dir", type=Path, default=ROOT / "benchmarks_svg")
    parser.add_argument("--strict", action="store_true", help="exit 1 if any measurement fails")
    args = parser.parse_args()
    if not 0 <= args.seed < 2 ** 64:
        parser.error("--seed must fit in an unsigned 64-bit integer")
    if sys.platform != "linux" or platform.machine() not in ("x86_64", "amd64"):
        parser.error("statis requires Linux x86-64")
    paths, build = build_binaries(args.bin_dir)
    results = collect(paths, args.samples, args.timeout, args.seed)
    complete = (
        all(w[mode]["status"] == "ok" for w in results["workloads"].values()
            for mode in ("native", "statis"))
        and all(p["statis"]["status"] == "ok" for p in results["time_warp_sweep"])
        and results["trace"]["status"] == "ok"
    )
    results.update({
        "schema_version": 1, "status": "complete" if complete else "incomplete",
        "recorded_at_utc": datetime.now(timezone.utc).isoformat(),
        "environment": {"platform": platform.platform(), "machine": platform.machine(),
                        "python": platform.python_version(), "cpu_count": os.cpu_count(),
                        "cpu_affinity": sorted(os.sched_getaffinity(0))},
        "configuration": {"samples": args.samples, "timeout_seconds": args.timeout,
                          "seed": args.seed, "build": build,
                          "binaries_sha256": {name: hashlib.sha256(path.read_bytes()).hexdigest()
                                              for name, path in paths.items()}},
        "methodology": [
            "Sequential fresh processes; perf_counter_ns includes startup, output capture and teardown.",
            "No warmups; median only when all requested samples exit zero; inspect raw stdout/stderr.",
            "Native 10s sleep and 15s epoll measured; other native curve points are ideal durations.",
            "statis pins the target to CPU 0; native execution retains the harness CPU affinity.",
            "Finite epoll emulation benchmarks timeout completion, not I/O readiness.",
            "Trace size requires successful recording and trace-dump; partial files are excluded.",
        ],
    })
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(results, indent=2, allow_nan=False) + "\n", encoding="utf-8")
    generate_charts(results, args.assets_dir)
    print_summary(results)
    print("\nReport: {}\nCharts: {}\nStatus: {}".format(
        args.output, args.assets_dir, results["status"]))
    return 1 if args.strict and not complete else 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, RuntimeError) as error:
        print("benchmark error: " + str(error), file=sys.stderr)
        sys.exit(1)

"""Regression checks for measurement validity; no supervised-host access needed."""

import contextlib
import io
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import patch
import xml.etree.ElementTree as ET

import benchmarks
from generate_charts import generate_charts


def attempt(status="ok", elapsed=0.01):
    return {"status": status, "elapsed_seconds": elapsed, "returncode": 0 if status == "ok" else 1,
            "stdout": "fixture output", "stderr": "" if status == "ok" else "launch denied"}


def measurement(status="ok"):
    return {"status": status, "median_seconds": 0.01 if status == "ok" else None,
            "attempts": [attempt(status)]}


class BenchmarkTests(unittest.TestCase):
    def test_nonzero_exit_keeps_diagnostics_and_is_not_a_timing_sample(self):
        command = [sys.executable, "-c",
                   "import sys; print('target'); print('denied', file=sys.stderr); sys.exit(7)"]
        result = benchmarks.measure(command, 3, 5)
        self.assertEqual(result["status"], "error")
        self.assertIsNone(result["median_seconds"])
        self.assertEqual(len(result["attempts"]), 1)
        self.assertEqual(result["attempts"][0]["returncode"], 7)
        self.assertIn("target", result["attempts"][0]["stdout"])
        self.assertIn("denied", result["attempts"][0]["stderr"])

    def test_partial_success_is_not_reported_as_a_complete_median(self):
        with patch.object(benchmarks, "run_command", side_effect=[attempt(), attempt("error")]):
            result = benchmarks.measure(["unused"], 3, 5)
        self.assertIsNone(result["median_seconds"])
        self.assertEqual(len(result["attempts"]), 2)

    def test_timeout_kills_descendants_that_hold_output_pipes_open(self):
        source = ("import subprocess, sys, time; "
                  "subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(5)']); "
                  "print('started', flush=True); time.sleep(5)")
        started = time.monotonic()
        result = benchmarks.run_command([sys.executable, "-c", source], 0.2)
        self.assertEqual(result["status"], "timeout")
        self.assertLess(time.monotonic() - started, 3)
        self.assertIn("started", result["stdout"])

    def collect_with_recording(self, status, validation_status="ok"):
        def record(command, timeout):
            if "record" in command:
                path = Path(command[command.index("--trace") + 1])
                path.write_bytes(b"x" * 140)
                return attempt(status)
            return attempt(validation_status)

        paths = {name: Path("/fake") / name for name in benchmarks.BINARIES}
        with patch.object(benchmarks, "measure", return_value=measurement("error")), \
                patch.object(benchmarks, "run_command", side_effect=record) as run, \
                contextlib.redirect_stderr(io.StringIO()):
            result = benchmarks.collect(paths, 1, 5, 42)
        return result, run.call_count

    def test_failed_recording_excludes_even_a_nonempty_partial_trace(self):
        result, calls = self.collect_with_recording("error")
        self.assertIsNone(result["trace"]["size_bytes"])
        self.assertIsNone(result["trace"]["rr_one_minute_reference_ratio"])
        self.assertEqual(calls, 1)
        self.assertTrue(all(point["statis"]["median_seconds"] is None
                            for point in result["time_warp_sweep"]))

    def test_undecodable_successful_recording_has_no_size(self):
        result, calls = self.collect_with_recording("ok", "error")
        self.assertEqual(result["trace"]["status"], "invalid_trace")
        self.assertIsNone(result["trace"]["size_bytes"])
        self.assertEqual(calls, 2)

    def test_valid_trace_reports_decimal_kb_and_binary_kib(self):
        result, calls = self.collect_with_recording("ok")
        self.assertEqual(calls, 2)
        self.assertEqual(result["trace"]["size_bytes"], 140)
        self.assertEqual(result["trace"]["size_kb"], 0.14)
        self.assertEqual(result["trace"]["size_kib"], 140 / 1024)

    def test_unknown_results_cannot_be_presented_as_observed_speedups(self):
        with tempfile.TemporaryDirectory() as directory:
            generate_charts({}, directory)
            paths = sorted(Path(directory).glob("*.svg"))
            self.assertEqual(len(paths), 3)
            text = " ".join(" ".join(ET.parse(path).getroot().itertext()) for path in paths)
            self.assertIn("illustrative, not observed", text)
            self.assertIn("failed recordings are excluded", text)
            self.assertNotIn("Measured 10s sleep:", text)

    def test_incomplete_report_is_saved_but_strict_mode_fails_ci(self):
        for strict in (False, True):
            with self.subTest(strict=strict), tempfile.TemporaryDirectory() as directory:
                output = Path(directory) / "results.json"
                arguments = ["benchmarks.py", "--output", str(output),
                             "--assets-dir", str(Path(directory) / "assets")]
                if strict:
                    arguments.append("--strict")
                results, _ = self.collect_with_recording("error")
                with patch.object(sys, "argv", arguments), \
                        patch.object(benchmarks, "build_binaries", return_value=({}, {})), \
                        patch.object(benchmarks, "collect", return_value=results), \
                        contextlib.redirect_stdout(io.StringIO()), \
                        contextlib.redirect_stderr(io.StringIO()):
                    code = benchmarks.main()
                self.assertEqual(code, 1 if strict else 0)
                self.assertIn('"status": "incomplete"', output.read_text())
                self.assertEqual(len(list((Path(directory) / "assets").glob("*.svg"))), 3)


if __name__ == "__main__":
    unittest.main()

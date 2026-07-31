"""Benchmark 审计报告验证器的契约测试。"""

import hashlib
import json
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
VERIFY = ROOT / "scripts" / "benchmark" / "verify-report.sh"


def digest(value: str) -> str:
    return hashlib.sha256(value.encode()).hexdigest()


def report() -> dict:
    config = "batch_size=1000\nseed=7"
    data = "fixture:smoke:v1"
    return {
        "schema_version": "jcmem.benchmark.audit.v1",
        "benchmark": {"name": "harness-smoke", "version": "1"},
        "git": {"commit": "a" * 40, "dirty": False},
        "platform": {
            "rust": "rustc 1.97.1",
            "python": "3.14.0",
            "os": "macOS",
            "cpu": "Apple Silicon",
        },
        "data": {
            "source": "repository-fixture",
            "canonical": data,
            "digest": digest(data),
            "case_count": 1,
        },
        "config": {"canonical": config, "digest": digest(config)},
        "timing": {
            "started_at": "2026-08-01T00:00:00+00:00",
            "finished_at": "2026-08-01T00:00:01+00:00",
        },
        "raw_counts": {
            "cases": 1,
            "fragments": 1,
            "evidence": 1,
            "results": 1,
        },
        "derived_metrics": {"recall_p50_ms": 10, "write_throughput_per_second": 100},
        "smoke": True,
        "non_release": False,
    }


class VerifyReportTests(unittest.TestCase):
    def run_validator(self, value: dict) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report.json"
            path.write_text(json.dumps(value), encoding="utf-8")
            return subprocess.run(
                ["bash", str(VERIFY), str(path)],
                cwd=ROOT,
                check=False,
                capture_output=True,
                text=True,
            )

    def test_tampered_config_digest_is_rejected(self) -> None:
        value = report()
        value["config"]["digest"] = "0" * 64
        result = self.run_validator(value)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("config digest", result.stderr)

    def test_missing_raw_counts_are_rejected(self) -> None:
        value = report()
        del value["raw_counts"]
        result = self.run_validator(value)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("raw_counts", result.stderr)

    def test_fabricated_platform_field_is_rejected(self) -> None:
        value = report()
        value["platform"]["fabricated_provider"] = "not-collected"
        result = self.run_validator(value)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("platform", result.stderr)

    def test_dirty_long_run_requires_explicit_non_release(self) -> None:
        value = report()
        value["git"]["dirty"] = True
        value["smoke"] = False
        result = self.run_validator(value)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("dirty", result.stderr)

        value["non_release"] = True
        result = self.run_validator(value)
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()

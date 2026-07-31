#!/usr/bin/env bash

# 运行仓库内可重复 Smoke，并生成经过审计的 JSON 报告。
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/../.." && pwd)"
output="target/benchmarks/benchmark-smoke.json"

while (($# > 0)); do
  case "$1" in
    --output)
      (($# >= 2)) || { echo "--output 缺少值" >&2; exit 2; }
      output="$2"
      shift 2
      ;;
    --help)
      echo "用法：$0 [--output 审计报告路径]"
      exit 0
      ;;
    *)
      echo "未知参数：$1" >&2
      echo "用法：$0 [--output 审计报告路径]" >&2
      exit 2
      ;;
  esac
done

if [[ "$output" != /* ]]; then
  output="$repo_root/$output"
fi

cd "$repo_root"
started_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
cargo test -p jc-memory-benchmark-harness --all-targets --all-features
cargo test -p jc-memory-million-fragments --all-targets --all-features
finished_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

mkdir -p "$(dirname -- "$output")"
python3 - "$repo_root" "$output" "$started_at" "$finished_at" <<'PY'
import hashlib
import json
import platform
import subprocess
import sys
from pathlib import Path


root = Path(sys.argv[1])
output = Path(sys.argv[2])
started_at, finished_at = sys.argv[3:5]
fixture_paths = [
    root / "benchmarks/longmemeval/fixtures/smoke.json",
    root / "benchmarks/locomo/fixtures/smoke.json",
    root / "benchmarks/memory-poisoning/fixtures/smoke.json",
    root / "benchmarks/cross-year/manifest.json",
    root / "benchmarks/model-swap/manifest.json",
]


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def count_fixture(path: Path) -> tuple[int, int, int]:
    document = json.loads(path.read_text(encoding="utf-8"))
    if "cases" not in document:
        return int(document.get("case_count", 0)), 0, 0
    cases = document["cases"]
    fragments = sum(
        len(session.get("turns", []))
        for case in cases
        for session in case.get("sessions", [])
    )
    evidence = sum(
        len(case.get("expected", {}).get("evidence_ids", [])) for case in cases
    )
    return len(cases), fragments, evidence


case_count = fragment_count = evidence_count = 0
canonical_lines = []
for path in fixture_paths:
    relative = path.relative_to(root).as_posix()
    canonical_lines.append(f"{relative}:{sha256(path)}")
    cases, fragments, evidence = count_fixture(path)
    case_count += cases
    fragment_count += fragments
    evidence_count += evidence


def command(*arguments: str) -> str:
    return subprocess.check_output(arguments, cwd=root, text=True).strip()


commit = command("git", "rev-parse", "HEAD")
dirty = bool(command("git", "status", "--porcelain", "--untracked-files=all"))
config_canonical = "cargo-test:harness,million-fragments;fixture-count:v1"
data_canonical = "\n".join(canonical_lines)
report = {
    "schema_version": "jcmem.benchmark.audit.v1",
    "benchmark": {"name": "benchmark-gates", "version": "smoke-v1"},
    "git": {"commit": commit, "dirty": dirty},
    "platform": {
        "rust": command("rustc", "--version"),
        "python": platform.python_version(),
        "os": f"{platform.system()} {platform.release()}",
        "cpu": platform.machine(),
    },
    "data": {
        "source": "repository-smoke-fixtures",
        "canonical": data_canonical,
        "digest": hashlib.sha256(data_canonical.encode()).hexdigest(),
        "case_count": case_count,
    },
    "config": {
        "canonical": config_canonical,
        "digest": hashlib.sha256(config_canonical.encode()).hexdigest(),
    },
    "timing": {"started_at": started_at, "finished_at": finished_at},
    "raw_counts": {
        "cases": case_count,
        "fragments": fragment_count,
        "evidence": evidence_count,
        "results": 0,
    },
    "derived_metrics": {
        "harness_contract_tests": 1,
        "million_fragments_recovery_tests": 1,
        "production_results": None,
    },
    "smoke": True,
    "non_release": False,
}
output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
PY

"$script_dir/verify-report.sh" "$output"
printf 'Smoke 审计报告：%s\n' "$output"

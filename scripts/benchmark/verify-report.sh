#!/usr/bin/env bash

# 验证 Benchmark 审计报告的结构、摘要和发布真实性边界。
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/../.." && pwd)"
schema_path="$repo_root/benchmarks/schema/report.schema.json"

if (($# != 1)); then
  echo "用法：$0 <audit-report.json>" >&2
  exit 2
fi

report_path="$1"
python3 - "$schema_path" "$report_path" <<'PY'
import hashlib
import json
import math
import re
import sys
from datetime import datetime
from pathlib import Path


schema_path = Path(sys.argv[1])
report_path = Path(sys.argv[2])


def fail(message: str) -> None:
    print(f"报告验证失败：{message}", file=sys.stderr)
    raise SystemExit(1)


def reject_constant(value: str) -> None:
    raise ValueError(f"不允许的 JSON 常量：{value}")


def reject_duplicate_keys(pairs: list[tuple[str, object]]) -> dict[str, object]:
    value: dict[str, object] = {}
    for key, item in pairs:
        if key in value:
            raise ValueError(f"重复字段：{key}")
        value[key] = item
    return value


try:
    json.loads(schema_path.read_text(encoding="utf-8"))
    document = json.loads(
        report_path.read_text(encoding="utf-8"),
        object_pairs_hook=reject_duplicate_keys,
        parse_constant=reject_constant,
    )
except (OSError, json.JSONDecodeError, ValueError) as error:
    fail(f"JSON 无效：{error}")


def object_with_keys(value: object, name: str, required: set[str]) -> dict[str, object]:
    if not isinstance(value, dict):
        fail(f"{name} 必须是对象")
    actual = set(value)
    missing = required - actual
    extra = actual - required
    if missing:
        fail(f"{name} 缺少字段：{', '.join(sorted(missing))}")
    if extra:
        fail(f"{name} 包含不允许字段：{', '.join(sorted(extra))}")
    return value


def non_empty_string(value: object, name: str) -> str:
    if not isinstance(value, str) or not value:
        fail(f"{name} 必须是非空字符串")
    return value


def boolean(value: object, name: str) -> bool:
    if not isinstance(value, bool):
        fail(f"{name} 必须是布尔值")
    return value


def non_negative_integer(value: object, name: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        fail(f"{name} 必须是非负整数")
    return value


if not isinstance(document, dict):
    fail("报告顶层必须是对象")
required = {
    "schema_version",
    "benchmark",
    "git",
    "platform",
    "data",
    "config",
    "timing",
    "raw_counts",
    "derived_metrics",
    "smoke",
    "non_release",
}
actual = set(document)
if actual - required:
    fail(f"顶层包含不允许字段：{', '.join(sorted(actual - required))}")
if required - actual:
    fail(f"顶层缺少字段：{', '.join(sorted(required - actual))}")
if document["schema_version"] != "jcmem.benchmark.audit.v1":
    fail("schema_version 不匹配")

object_with_keys(document["benchmark"], "benchmark", {"name", "version"})
non_empty_string(document["benchmark"]["name"], "benchmark.name")
non_empty_string(document["benchmark"]["version"], "benchmark.version")

git = object_with_keys(document["git"], "git", {"commit", "dirty"})
commit = non_empty_string(git["commit"], "git.commit")
if not re.fullmatch(r"[0-9a-f]{40}", commit):
    fail("git.commit 必须是 40 位小写 SHA-1")
dirty = boolean(git["dirty"], "git.dirty")

platform = object_with_keys(document["platform"], "platform", {"rust", "python", "os", "cpu"})
for name, value in platform.items():
    non_empty_string(value, f"platform.{name}")

data = object_with_keys(document["data"], "data", {"source", "canonical", "digest", "case_count"})
non_empty_string(data["source"], "data.source")
data_canonical = non_empty_string(data["canonical"], "data.canonical")
data_digest = non_empty_string(data["digest"], "data.digest")
if not re.fullmatch(r"[0-9a-f]{64}", data_digest):
    fail("data.digest 必须是 64 位小写 SHA-256")
if hashlib.sha256(data_canonical.encode()).hexdigest() != data_digest:
    fail("data digest 不匹配")
data_case_count = non_negative_integer(data["case_count"], "data.case_count")

config = object_with_keys(document["config"], "config", {"canonical", "digest"})
config_canonical = non_empty_string(config["canonical"], "config.canonical")
config_digest = non_empty_string(config["digest"], "config.digest")
if not re.fullmatch(r"[0-9a-f]{64}", config_digest):
    fail("config digest 必须是 64 位小写 SHA-256")
if hashlib.sha256(config_canonical.encode()).hexdigest() != config_digest:
    fail("config digest 不匹配")

timing = object_with_keys(document["timing"], "timing", {"started_at", "finished_at"})
try:
    started = datetime.fromisoformat(
        non_empty_string(timing["started_at"], "timing.started_at").replace("Z", "+00:00")
    )
    finished = datetime.fromisoformat(
        non_empty_string(timing["finished_at"], "timing.finished_at").replace("Z", "+00:00")
    )
except ValueError as error:
    fail(f"时间必须是 ISO-8601：{error}")
if started.tzinfo is None or finished.tzinfo is None:
    fail("时间必须包含时区")
if finished < started:
    fail("finished_at 不能早于 started_at")

raw_counts = object_with_keys(
    document["raw_counts"], "raw_counts", {"cases", "fragments", "evidence", "results"}
)
for name, value in raw_counts.items():
    non_negative_integer(value, f"raw_counts.{name}")
if raw_counts["cases"] != data_case_count:
    fail("data.case_count 与 raw_counts.cases 不一致")

derived_metrics = document["derived_metrics"]
if not isinstance(derived_metrics, dict) or not derived_metrics:
    fail("derived_metrics 必须包含至少一个指标")
for name, value in derived_metrics.items():
    non_empty_string(name, "derived_metrics.name")
    if value is not None and (
        isinstance(value, bool)
        or not isinstance(value, (int, float))
        or not math.isfinite(value)
    ):
        fail(f"derived_metrics.{name} 必须是有限数字或 null")

smoke = boolean(document["smoke"], "smoke")
non_release = boolean(document["non_release"], "non_release")
if dirty and not smoke and not non_release:
    fail("dirty 长测必须显式标记 non_release=true")

print(f"报告验证通过：{report_path}")
PY

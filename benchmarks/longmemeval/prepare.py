#!/usr/bin/env python3
"""将 LongMemEval 原始 JSON 规范化为版本化 Benchmark Case。"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


TOP_LEVEL_KEYS = {"schema_version", "dataset", "version", "license", "source", "smoke", "cases"}
CASE_KEYS = {"id", "sessions", "query", "expected", "tags"}


def _require_mapping(value: Any, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ValueError(f"{label} 必须是对象")
    return value


def prepare(input_path: Path, output_path: Path) -> None:
    document = _require_mapping(json.loads(input_path.read_text(encoding="utf-8")), "Fixture")
    unknown = set(document) - TOP_LEVEL_KEYS
    if unknown:
        raise ValueError(f"Fixture 存在未知字段: {sorted(unknown)}")
    if document.get("dataset") != "longmemeval":
        raise ValueError("Fixture dataset 必须是 longmemeval")
    cases = document.get("cases")
    if not isinstance(cases, list) or not cases:
        raise ValueError("Fixture cases 不能为空")

    normalized: list[dict[str, Any]] = []
    seen: set[str] = set()
    for raw_case in cases:
        case = _require_mapping(raw_case, "Case")
        unknown_case = set(case) - CASE_KEYS
        if unknown_case:
            raise ValueError(f"Case 存在未知字段: {sorted(unknown_case)}")
        case_id = case.get("id")
        if not isinstance(case_id, str) or not case_id.strip() or case_id in seen:
            raise ValueError(f"Case ID 无效或重复: {case_id!r}")
        seen.add(case_id)
        sessions = case.get("sessions")
        if not isinstance(sessions, list) or not sessions:
            raise ValueError(f"Case {case_id} 必须包含 Session")
        normalized.append(
            {
                "id": case_id,
                "sessions": sessions,
                "query": case.get("query", {}),
                "expected": case.get("expected", {}),
                "tags": sorted(set(case.get("tags", []))),
            }
        )

    output = {
        "schema_version": document.get("schema_version"),
        "dataset": "longmemeval",
        "version": document.get("version"),
        "license": document.get("license"),
        "source": document.get("source"),
        "smoke": bool(document.get("smoke")),
        "cases": normalized,
    }
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(
        json.dumps(output, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def main() -> None:
    parser = argparse.ArgumentParser(description="规范化 LongMemEval Benchmark Fixture")
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    prepare(args.input, args.output)


if __name__ == "__main__":
    main()

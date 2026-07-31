#!/usr/bin/env bash

# 校验 GitHub Actions YAML；优先使用 actionlint/yamllint，缺失时保留本地静态兜底。
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/../.." && pwd)"
workflow_dir="${1:-$repo_root/.github/workflows}"

if [[ ! -d "$workflow_dir" ]]; then
  echo "YAML 目录不存在：$workflow_dir" >&2
  exit 2
fi

shopt -s nullglob
files=("$workflow_dir"/*.yml "$workflow_dir"/*.yaml)
if ((${#files[@]} == 0)); then
  echo "YAML 目录没有工作流文件：$workflow_dir" >&2
  exit 2
fi

if command -v actionlint >/dev/null 2>&1; then
  actionlint "${files[@]}"
fi
if command -v yamllint >/dev/null 2>&1; then
  yamllint -d "{extends: relaxed, rules: {line-length: disable}}" "${files[@]}"
fi

for file in "${files[@]}"; do
  if ! ruby -e 'require "yaml"; YAML.parse_file(ARGV.fetch(0))' "$file" >/dev/null 2>&1; then
    echo "YAML 解析失败：$file" >&2
    exit 1
  fi
done

python3 - "${files[@]}" <<'PY'
import re
import sys
from pathlib import Path


for name in sys.argv[1:]:
    path = Path(name)
    text = path.read_text(encoding="utf-8")
    if not re.search(r"^name:\s*\S+", text, re.MULTILINE) or not re.search(r"^jobs:\s*$", text, re.MULTILINE):
        print(f"YAML 工作流缺少 name/jobs：{path}", file=sys.stderr)
        raise SystemExit(1)
    for reference in re.findall(r"uses:\s*([^\s#]+)", text):
        if not re.fullmatch(r"[^@]+@[0-9a-f]{40}", reference):
            print(f"Action 未固定完整 SHA：{path}: {reference}", file=sys.stderr)
            raise SystemExit(1)
PY

printf 'YAML 工作流校验通过：%d 个文件\n' "${#files[@]}"

#!/usr/bin/env bash

# 运行可恢复的 Million Fragments 基准；长测必须显式指定输出文件。
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/../.." && pwd)"
fragments=""
output=""

usage() {
  cat <<'EOF'
用法：scripts/benchmark/million-fragments.sh --fragments 数量 --output 报告路径

参数：
  --fragments 数量  要生成的 Fragment 数量
  --output 路径     JSON 报告输出路径
  --help            显示帮助
EOF
}

while (($# > 0)); do
  case "$1" in
    --fragments)
      (($# >= 2)) || { echo "--fragments 缺少值" >&2; exit 2; }
      fragments="$2"
      shift 2
      ;;
    --output)
      (($# >= 2)) || { echo "--output 缺少值" >&2; exit 2; }
      output="$2"
      shift 2
      ;;
    --help)
      usage
      exit 0
      ;;
    *)
      echo "未知参数：$1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$fragments" || -z "$output" ]]; then
  echo "必须同时指定 --fragments 和 --output" >&2
  usage >&2
  exit 2
fi

if [[ ! "$fragments" =~ ^[0-9]+$ || "$fragments" == 0 ]]; then
  echo "--fragments 必须是大于 0 的整数" >&2
  exit 2
fi

cd "$repo_root"
cargo run --quiet -p jc-memory-million-fragments -- \
  --fragments "$fragments" \
  --output "$output"

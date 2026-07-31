#!/usr/bin/env bash
set -euo pipefail

# 只获取 manifest 声明的版本，并要求调用方明确接受许可。
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CACHE_DIR="${ROOT_DIR}/.cache/benchmarks"
DATASET=""
ACCEPT_LICENSE=""

usage() {
  echo "用法：$0 --dataset <longmemeval|locomo> --accept-license <许可字符串> [--cache-dir <目录>]" >&2
}

while (($# > 0)); do
  case "$1" in
    --dataset)
      DATASET="${2:?缺少 dataset 值}"
      shift 2
      ;;
    --accept-license)
      ACCEPT_LICENSE="${2:?缺少 license 值}"
      shift 2
      ;;
    --cache-dir)
      CACHE_DIR="${2:?缺少 cache 目录}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "未知参数：$1" >&2
      usage
      exit 2
      ;;
  esac
done

if [[ "$DATASET" != "longmemeval" && "$DATASET" != "locomo" ]]; then
  echo "dataset 必须是 longmemeval 或 locomo" >&2
  exit 2
fi
if [[ -z "$ACCEPT_LICENSE" ]]; then
  echo "必须显式传入 --accept-license" >&2
  exit 2
fi

MANIFEST="${ROOT_DIR}/benchmarks/${DATASET}/manifest.json"
read_manifest() {
  python3 - "$MANIFEST" "$1" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as manifest_file:
    manifest = json.load(manifest_file)
print(manifest[sys.argv[2]])
PY
}

LICENSE="$(read_manifest license)"
if [[ "$ACCEPT_LICENSE" != "$LICENSE" ]]; then
  echo "许可确认不匹配：需要 ${LICENSE}" >&2
  exit 3
fi
SOURCE_URL="$(read_manifest source_url)"
EXPECTED_SHA="$(read_manifest sha256)"
FIXTURE="$(read_manifest fixture)"
TARGET_DIR="${CACHE_DIR}/${DATASET}"
TARGET="${TARGET_DIR}/$(basename "$FIXTURE")"
mkdir -p "$TARGET_DIR"

if [[ "$SOURCE_URL" == repository://* ]]; then
  cp "${ROOT_DIR}/benchmarks/${DATASET}/${FIXTURE}" "$TARGET"
else
  curl --fail --location --retry 2 --output "$TARGET" "$SOURCE_URL"
fi

ACTUAL_SHA="$(shasum -a 256 "$TARGET" | awk '{print $1}')"
if [[ "$ACTUAL_SHA" != "$EXPECTED_SHA" ]]; then
  echo "下载内容 SHA-256 不匹配" >&2
  rm -f "$TARGET"
  exit 4
fi
printf '已校验 %s Fixture：%s\n' "$DATASET" "$TARGET"

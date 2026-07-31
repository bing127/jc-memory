#!/usr/bin/env bash

# 为 Release 目录中的所有可发布文件生成 SHA-256 清单。
set -euo pipefail

input=""
while (($# > 0)); do
  case "$1" in
    --input)
      (($# >= 2)) || { echo "--input 缺少值" >&2; exit 2; }
      input="$2"
      shift 2
      ;;
    --help)
      echo "用法：$0 --input <Release 目录>"
      exit 0
      ;;
    *)
      echo "未知参数：$1" >&2
      exit 2
      ;;
  esac
done

if [[ -z "$input" || ! -d "$input" ]]; then
  echo "Release 目录不存在" >&2
  exit 2
fi

files=()
while IFS= read -r file; do
  files+=("$file")
done < <(cd "$input" && find . -type f ! -name SHA256SUMS -print | sort)
if ((${#files[@]} == 0)); then
  echo "Release 目录没有可校验文件" >&2
  exit 1
fi

(
  cd "$input"
  for file in "${files[@]}"; do
    shasum -a 256 "$file"
  done
) > "$input/SHA256SUMS"
printf 'Checksum 清单已生成：%s\n' "$input/SHA256SUMS"

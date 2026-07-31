#!/usr/bin/env bash

# 构建 SDK 私有 Artifact；只生成本地文件，不调用任何公共发布服务。
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/../.." && pwd)"
version=""
output=""
allow_dirty=0
skip_swift=0

while (($# > 0)); do
  case "$1" in
    --version) version="$2"; shift 2 ;;
    --output) output="$2"; shift 2 ;;
    --allow-dirty) allow_dirty=1; shift ;;
    --skip-swift) skip_swift=1; shift ;;
    --help)
      echo "用法：$0 --version <semver> --output <目录> [--allow-dirty] [--skip-swift]"
      exit 0
      ;;
    *) echo "未知参数：$1" >&2; exit 2 ;;
  esac
done

if [[ -z "$version" || -z "$output" ]]; then
  echo "必须指定 --version 和 --output" >&2
  exit 2
fi

version_args=(--version "$version")
if [[ "$allow_dirty" == "1" ]]; then
  version_args+=(--allow-dirty)
fi
"$script_dir/version.sh" "${version_args[@]}"

mkdir -p "$output/artifacts"
cd "$repo_root"

corepack pnpm --dir sdk/node install --frozen-lockfile
corepack pnpm --dir sdk/node build
tar -czf "$output/artifacts/jiangcheng-memory-node-$version.tar.gz" \
  -C sdk/node package.json dist npm

python3 -m build --wheel --outdir "$output/artifacts" sdk/python

tar -czf "$output/artifacts/jiangcheng-memory-go-$version.tar.gz" \
  sdk/go/go.mod sdk/go/memory

gradle -p sdk/kotlin jar --no-daemon
gradle -p sdk/java jar --no-daemon
find sdk/kotlin/build/libs -maxdepth 1 -type f -name '*.jar' -exec cp {} "$output/artifacts/" \;
find sdk/java/build/libs -maxdepth 1 -type f -name '*.jar' -exec cp {} "$output/artifacts/" \;

if [[ "$skip_swift" != "1" ]]; then
  swift build --package-path sdk/swift
  tar -czf "$output/artifacts/jiangcheng-memory-swift-$version.tar.gz" \
    -C sdk/swift Package.swift Sources Tests
fi

printf 'SDK Artifact 构建完成：%s\n' "$output/artifacts"

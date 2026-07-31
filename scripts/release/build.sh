#!/usr/bin/env bash

# 构建指定目标的 jcmem、jcmemd、Server 和 MCP Rust 二进制。
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/../.." && pwd)"
version=""
target=""
output=""
allow_dirty=0

while (($# > 0)); do
  case "$1" in
    --version) version="$2"; shift 2 ;;
    --target) target="$2"; shift 2 ;;
    --output) output="$2"; shift 2 ;;
    --allow-dirty) allow_dirty=1; shift ;;
    --help)
      echo "用法：$0 --version <semver> --target <target> --output <目录> [--allow-dirty]"
      exit 0
      ;;
    *) echo "未知参数：$1" >&2; exit 2 ;;
  esac
done

if [[ -z "$version" || -z "$target" || -z "$output" ]]; then
  echo "必须指定 --version、--target 和 --output" >&2
  exit 2
fi

version_args=(--version "$version")
if [[ "$allow_dirty" == "1" ]]; then
  version_args+=(--allow-dirty)
fi
"$script_dir/version.sh" "${version_args[@]}"

case "$target" in
  linux-x64) rust_target="x86_64-unknown-linux-gnu" ;;
  linux-arm64) rust_target="aarch64-unknown-linux-gnu" ;;
  macos-x64) rust_target="x86_64-apple-darwin" ;;
  macos-arm64) rust_target="aarch64-apple-darwin" ;;
  windows-x64) rust_target="x86_64-pc-windows-msvc" ;;
  *) echo "不支持的 Release target：$target" >&2; exit 2 ;;
esac

if ! rustup target list --installed | grep -qx "$rust_target"; then
  echo "Rust target 未安装：$rust_target；CI 构建机必须先安装目标工具链" >&2
  exit 2
fi

mkdir -p "$output/artifacts"
cd "$repo_root"
cargo build --locked --release --target "$rust_target" \
  -p jc-memory-sidecar -p jc-memory-server -p jc-memory-mcp

binary_dir="$repo_root/target/$rust_target/release"
extension=""
if [[ "$target" == windows-* ]]; then
  extension=".exe"
fi
for binary in jcmem jcmemd jc-memory-server jc-memory-mcp; do
  source="$binary_dir/$binary$extension"
  test -f "$source"
  cp "$source" "$output/artifacts/$binary-$version-$target$extension"
done

printf 'Rust Artifact 构建完成：%s\n' "$output/artifacts"

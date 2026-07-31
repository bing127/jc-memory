#!/usr/bin/env bash

# 构建未签名移动端输入；签名和目标平台发布由私有平台流程负责。
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/../.." && pwd)"
version=""
output=""
allow_dirty=0
target=""

while (($# > 0)); do
  case "$1" in
    --version) version="$2"; shift 2 ;;
    --output) output="$2"; shift 2 ;;
    --allow-dirty) allow_dirty=1; shift ;;
    --target) target="$2"; shift 2 ;;
    --help)
      echo "用法：$0 --version <semver> --target <mobile-android|mobile-swift> --output <目录> [--allow-dirty]"
      exit 0
      ;;
    *) echo "未知参数：$1" >&2; exit 2 ;;
  esac
done

if [[ -z "$version" || -z "$output" || -z "$target" ]]; then
  echo "必须指定 --version、--target 和 --output" >&2
  exit 2
fi
version_args=(--version "$version")
if [[ "$allow_dirty" == "1" ]]; then
  version_args+=(--allow-dirty)
fi
"$script_dir/version.sh" "${version_args[@]}"

mkdir -p "$output/artifacts"
cd "$repo_root"
case "$target" in
  mobile-android)
    gradle -p sdk/kotlin/android assemble --no-daemon
    cp sdk/kotlin/android/build/outputs/aar/*.aar "$output/artifacts/"
    ;;
  mobile-swift)
    swift build --package-path sdk/swift
    tar -czf "$output/artifacts/jiangcheng-memory-mobile-swift-$version-unsigned.tar.gz" \
      -C sdk/swift Package.swift Sources Tests
    ;;
  *)
    echo "不支持的 Mobile target：$target" >&2
    exit 2
    ;;
esac

cat > "$output/mobile-signing.json" <<EOF
{
  "schema_version": "jcmem.mobile-signing.v1",
  "version": "$version",
  "status": "unsigned",
  "interface": "detached-signature-v1",
  "reason": "移动端签名必须在私有发布平台完成"
}
EOF
printf '未签名 Mobile Artifact 构建完成：%s\n' "$output/artifacts"

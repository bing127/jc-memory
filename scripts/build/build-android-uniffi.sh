#!/usr/bin/env bash
set -euo pipefail

# 为 Android AAR 构建四类 UniFFI native slice；脚本不下载工具链或伪造缺失产物。
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
output_dir="$repo_root/sdk/kotlin/android/src/main/jniLibs"
ndk_root="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}"

if [[ -z "$ndk_root" || ! -d "$ndk_root" ]]; then
  printf '缺少 ANDROID_NDK_HOME/ANDROID_NDK_ROOT，无法构建 Android native slices\n' >&2
  exit 2
fi
llvm_root="$(find "$ndk_root/toolchains/llvm/prebuilt" -mindepth 1 -maxdepth 1 -type d -print -quit)"
if [[ -z "$llvm_root" || ! -d "$llvm_root/bin" ]]; then
  printf 'NDK 缺少 LLVM prebuilt 工具链: %s\n' "$ndk_root" >&2
  exit 2
fi

android_targets=(
  "arm64-v8a:aarch64-linux-android"
  "armeabi-v7a:armv7-linux-androideabi"
  "x86_64:x86_64-linux-android"
  "x86:i686-linux-android"
)

cd "$repo_root"
"$repo_root/scripts/build/build-java-uniffi.sh"
for entry in "${android_targets[@]}"; do
  abi="${entry%%:*}"
  target="${entry#*:}"
  if ! rustup target list --installed | rg -qx "$target"; then
    printf 'Rust target 未安装: %s\n' "$target" >&2
    exit 2
  fi
  case "$target" in
    armv7-linux-androideabi) compiler="armv7a-linux-androideabi24-clang" ;;
    *) compiler="${target}24-clang" ;;
  esac
  linker="$llvm_root/bin/$compiler"
  test -x "$linker"
  target_key="$(printf '%s' "$target" | tr '[:lower:]-' '[:upper:]_')"
  cc_key="$(printf '%s' "$target" | tr '-' '_')"
  export "CARGO_TARGET_${target_key}_LINKER=$linker"
  export "CC_${cc_key}=$linker"
  RUSTC="$(rustup which rustc)" cargo build --locked --release --target "$target" -p jc-memory-uniffi --lib
  source="$repo_root/target/$target/release/libjc_memory_uniffi.so"
  test -f "$source"
  mkdir -p "$output_dir/$abi"
  cp "$source" "$output_dir/$abi/libjc_memory_uniffi.so"
done

printf 'Android native slices 已写入: %s\n' "$output_dir"

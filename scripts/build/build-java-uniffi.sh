#!/usr/bin/env bash
set -euo pipefail

# 使用仓库锁定的 UniFFI 版本生成 Kotlin/JVM binding。
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"

case "$(uname -s)" in
  Darwin)
    native_library="$target_dir/debug/libjc_memory_uniffi.dylib"
    ;;
  Linux)
    native_library="$target_dir/debug/libjc_memory_uniffi.so"
    ;;
  MINGW*|MSYS*|CYGWIN*)
    native_library="$target_dir/debug/jc_memory_uniffi.dll"
    ;;
  *)
    echo "不支持的构建主机: $(uname -s)" >&2
    exit 2
    ;;
esac

cd "$repo_root"
cargo build --locked -p jc-memory-uniffi --lib
test -f "$native_library"
cargo run --locked -p jc-memory-uniffi --bin jc-memory-uniffi-bindgen -- \
  generate --library "$native_library" --language kotlin --no-format \
  --out-dir "$repo_root/sdk/java/src/main/kotlin"

# UniFFI 生成器当前会保留部分行尾空白，提交前统一清理以保持差异门禁稳定。
generated_file="$repo_root/sdk/java/src/main/kotlin/uniffi/jc_memory_uniffi/jc_memory_uniffi.kt"
perl -pi -e 's/[ \t]+$//' "$generated_file"
perl -0777 -pi -e 's/\n+\z/\n/' "$generated_file"

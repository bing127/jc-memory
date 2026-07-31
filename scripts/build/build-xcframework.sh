#!/usr/bin/env bash
set -euo pipefail

# 该脚本接受已生成的 UniFFI Framework 输入；在 macOS 当前主机上可选地先生成一个本机 slice。
root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
input_dir="${1:-${root_dir}/target/uniffi}"
output_dir="${2:-${root_dir}/target/xcframework}"

if [[ "$(uname -s)" == "Darwin" && ! -d "${input_dir}" ]]; then
  "${root_dir}/scripts/build/build-swift-uniffi.sh"
  native_library="${CARGO_TARGET_DIR:-${root_dir}/target}/debug/libjc_memory_uniffi.dylib"
  host_framework="${input_dir}/macos-arm64/JiangchengMemoryUniffi.framework"
  mkdir -p "${host_framework}/Headers"
  cp "${native_library}" "${host_framework}/JiangchengMemoryUniffi"
  cp "${root_dir}/sdk/swift/Sources/jc_memory_uniffiFFI/jc_memory_uniffiFFI.h" "${host_framework}/Headers/"
  cat > "${host_framework}/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.jiangcheng.memory.uniffi</string>
<key>CFBundleName</key><string>JiangchengMemoryUniffi</string>
<key>CFBundlePackageType</key><string>FMWK</string>
<key>CFBundleVersion</key><string>0.1.0</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
</dict></plist>
PLIST
elif [[ ! -d "${input_dir}" ]]; then
  printf '缺少 UniFFI 输入目录: %s\n' "${input_dir}" >&2
  exit 2
fi

mkdir -p "${output_dir}"
framework_args=()
while IFS= read -r framework; do
  framework_args+=( -framework "${framework}" )
done < <(find "${input_dir}" -maxdepth 3 -type d -name '*.framework' -print)
if [[ "${#framework_args[@]}" -eq 0 ]]; then
  printf '没有找到待合并的 .framework；请先构建 memory-uniffi slices\n' >&2
  exit 2
fi

xcodebuild -create-xcframework \
  "${framework_args[@]}" \
  -output "${output_dir}/JiangchengMemory.xcframework"

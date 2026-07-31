#!/usr/bin/env bash
set -euo pipefail

# 该脚本只接受已生成的 UniFFI XCFramework 输入，不自行猜测或下载原生库。
root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
input_dir="${1:-${root_dir}/target/uniffi}"
output_dir="${2:-${root_dir}/target/xcframework}"

if [[ ! -d "${input_dir}" ]]; then
  printf '缺少 UniFFI 输入目录: %s\n' "${input_dir}" >&2
  exit 2
fi

mkdir -p "${output_dir}"
framework_args=()
while IFS= read -r framework; do
  framework_args+=( -framework "${framework}" )
done < <(find "${input_dir}" -maxdepth 1 -type d -name '*.framework' -print)
if [[ "${#framework_args[@]}" -eq 0 ]]; then
  printf '没有找到待合并的 .framework；请先构建 memory-uniffi slices\n' >&2
  exit 2
fi

xcodebuild -create-xcframework \
  "${framework_args[@]}" \
  -output "${output_dir}/JiangchengMemory.xcframework"

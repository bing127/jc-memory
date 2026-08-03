#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../../.." && pwd)"
lock_file="${repo_root}/integrations/hermes/hermes.lock.yaml"
hermes_root="${repo_root}/external/hermes-agent"

yaml_value() {
  local key="$1"
  awk -F': ' -v key="${key}" '$1 == key { print $2; exit }' "${lock_file}"
}

repository="$(yaml_value repository)"
branch="$(yaml_value branch)"
commit="$(yaml_value commit)"
desktop_path="$(yaml_value desktop_path)"

if [[ ! -d "${hermes_root}/.git" ]]; then
  if [[ "${1:-}" == "--check" ]]; then
    echo "Hermes 尚未克隆到 ${hermes_root}" >&2
    exit 1
  fi
  mkdir -p "$(dirname "${hermes_root}")"
  git clone --depth 1 --filter=blob:none --branch "${branch}" "${repository}" "${hermes_root}"
fi

actual_origin="$(git -C "${hermes_root}" remote get-url origin)"
actual_commit="$(git -C "${hermes_root}" rev-parse HEAD)"

if [[ "${actual_origin}" != "${repository}" ]]; then
  echo "Hermes origin 不匹配：${actual_origin}" >&2
  exit 1
fi

if [[ "${actual_commit}" != "${commit}" ]]; then
  echo "Hermes Commit 不匹配：期望 ${commit}，实际 ${actual_commit}" >&2
  echo "为避免覆盖现有修改，bootstrap 不会自动 reset。" >&2
  exit 1
fi

required_paths=(
  "${desktop_path}/package.json"
  "agent/memory_provider.py"
  "plugins/memory/__init__.py"
)

for required_path in "${required_paths[@]}"; do
  if [[ ! -f "${hermes_root}/${required_path}" ]]; then
    echo "Hermes 契约文件不存在：${required_path}" >&2
    exit 1
  fi
done

echo "Hermes Desktop 宿主校验通过：${commit}"

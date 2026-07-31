#!/usr/bin/env bash

# 将已构建文件封装为私有 Release，生成 Manifest、SBOM 和构建元数据。
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/../.." && pwd)"
version=""
target=""
input=""
output=""
allow_dirty=0

while (($# > 0)); do
  case "$1" in
    --version) version="$2"; shift 2 ;;
    --target) target="$2"; shift 2 ;;
    --input) input="$2"; shift 2 ;;
    --output) output="$2"; shift 2 ;;
    --allow-dirty) allow_dirty=1; shift ;;
    --help)
      echo "用法：$0 --version <semver> --target <target> --input <构建目录> --output <Release 目录> [--allow-dirty]"
      exit 0
      ;;
    *) echo "未知参数：$1" >&2; exit 2 ;;
  esac
done

if [[ -z "$version" || -z "$target" || -z "$input" || -z "$output" ]]; then
  echo "必须指定 --version、--target、--input 和 --output" >&2
  exit 2
fi

version_args=(--version "$version")
if [[ "$allow_dirty" == "1" ]]; then
  version_args+=(--allow-dirty)
fi
"$script_dir/version.sh" "${version_args[@]}"

mkdir -p "$output/artifacts"
if [[ -d "$input/artifacts" ]]; then
  find "$input/artifacts" -maxdepth 1 -type f -exec cp {} "$output/artifacts/" \;
fi
if ! find "$output/artifacts" -maxdepth 1 -type f -print -quit | grep -q .; then
  echo "没有可打包的 Artifact" >&2
  exit 1
fi

python3 - "$repo_root" "$version" "$target" "$output" <<'PY'
import json
import platform
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path


root = Path(sys.argv[1])
version, target, output = sys.argv[2], sys.argv[3], Path(sys.argv[4])
artifact_paths = sorted(
    path.relative_to(output).as_posix()
    for path in (output / "artifacts").iterdir()
    if path.is_file()
)
commit = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
dirty = bool(
    subprocess.check_output(
        ["git", "-C", str(root), "status", "--porcelain", "--untracked-files=all"],
        text=True,
    ).strip()
)
metadata = {
    "schema_version": "jcmem.release-metadata.v1",
    "version": version,
    "target": target,
    "git_commit": commit,
    "git_dirty": dirty,
    "rust": (
        subprocess.check_output(["rustc", "--version"], text=True).strip()
        if shutil.which("rustc")
        else "unavailable"
    ),
    "os": platform.platform(),
    "cpu": platform.machine(),
    "built_at": datetime.now(timezone.utc).isoformat(),
    "license": "PRIVATE",
    "signing": {"status": "unsigned", "interface": "detached-signature-v1"},
}
(output / "build-metadata.json").write_text(
    json.dumps(metadata, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
)

components = []
try:
    import tomllib

    for package in tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8")).get("package", []):
        components.append({"type": "library", "name": package["name"], "version": package["version"]})
except (OSError, ValueError, KeyError):
    components = []
sbom = {
    "bomFormat": "CycloneDX",
    "specVersion": "1.5",
    "version": 1,
    "metadata": {"component": {"type": "application", "name": "jiangcheng-memory", "version": version}},
    "components": components,
}
(output / "sbom.cdx.json").write_text(
    json.dumps(sbom, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
)

manifest = {
    "schema_version": "jcmem.release.v1",
    "version": version,
    "target": target,
    "license": "PRIVATE",
    "artifacts": artifact_paths,
    "sbom": "sbom.cdx.json",
    "metadata": "build-metadata.json",
    "public_publish": False,
}
(output / "release-manifest.json").write_text(
    json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
)
PY

"$script_dir/checksums.sh" --input "$output"
printf '私有 Release 已封装：%s\n' "$output"

#!/usr/bin/env bash

# 验证私有 Release Manifest、Artifact、SBOM、元数据和 SHA-256 清单。
set -euo pipefail

input=""
version=""
while (($# > 0)); do
  case "$1" in
    --input)
      (($# >= 2)) || { echo "--input 缺少值" >&2; exit 2; }
      input="$2"
      shift 2
      ;;
    --version)
      (($# >= 2)) || { echo "--version 缺少值" >&2; exit 2; }
      version="$2"
      shift 2
      ;;
    --help)
      echo "用法：$0 --input <Release 目录> --version <semver>"
      exit 0
      ;;
    *)
      echo "未知参数：$1" >&2
      exit 2
      ;;
  esac
done

if [[ -z "$input" || -z "$version" ]]; then
  echo "必须指定 --input 和 --version" >&2
  exit 2
fi

python3 - "$input" "$version" <<'PY'
import hashlib
import json
import sys
from pathlib import Path


root = Path(sys.argv[1])
expected_version = sys.argv[2]
manifest_path = root / "release-manifest.json"
if not manifest_path.is_file():
    raise SystemExit("缺少 release-manifest.json")
manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
if manifest.get("schema_version") != "jcmem.release.v1":
    raise SystemExit("Release Manifest Schema 不匹配")
if manifest.get("version") != expected_version:
    raise SystemExit("Release Manifest 版本不一致")
if manifest.get("license") != "PRIVATE":
    raise SystemExit("Release 必须声明 PRIVATE License")
if manifest.get("public_publish") is not False:
    raise SystemExit("Release 必须明确禁止公开发布")

for key in ["sbom", "metadata"]:
    path = root / manifest.get(key, "")
    if not path.is_file():
        raise SystemExit(f"缺少 Release {key} 文件：{path}")
artifacts = manifest.get("artifacts")
if not isinstance(artifacts, list) or not artifacts:
    raise SystemExit("Release Manifest 缺少 Artifact 列表")
for relative in artifacts:
    path = root / relative
    if not path.is_file():
        raise SystemExit(f"缺少 Artifact：{relative}")

checksum_path = root / "SHA256SUMS"
if not checksum_path.is_file():
    raise SystemExit("缺少 Checksum 清单")
checksum_entries = set()
for line in checksum_path.read_text(encoding="utf-8").splitlines():
    try:
        digest, relative = line.split(maxsplit=1)
    except ValueError:
        raise SystemExit("Checksum 清单格式错误")
    relative = relative.removeprefix("*").removeprefix("./")
    path = (root / relative).resolve()
    try:
        path.relative_to(root.resolve())
    except ValueError:
        raise SystemExit(f"Checksum 路径越界：{relative}")
    if not path.is_file():
        raise SystemExit(f"Checksum 引用文件不存在：{relative}")
    actual = hashlib.sha256(path.read_bytes()).hexdigest()
    if actual != digest:
        raise SystemExit(f"Checksum 不匹配：{relative}")
    checksum_entries.add(relative)
required_entries = set(artifacts) | {manifest["sbom"], manifest["metadata"], "release-manifest.json"}
missing_entries = sorted(required_entries - checksum_entries)
if missing_entries:
    raise SystemExit(f"Checksum 清单缺少：{', '.join(missing_entries)}")
print(f"Release 验证通过：{root}（{len(artifacts)} 个 Artifact）")
PY

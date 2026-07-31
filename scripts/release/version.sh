#!/usr/bin/env bash

# 校验发布版本、所有 SDK 版本和工作树状态；不修改源文件或创建 Git tag。
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/../.." && pwd)"
version=""
allow_dirty=0

while (($# > 0)); do
  case "$1" in
    --version)
      (($# >= 2)) || { echo "--version 缺少值" >&2; exit 2; }
      version="$2"
      shift 2
      ;;
    --root)
      (($# >= 2)) || { echo "--root 缺少值" >&2; exit 2; }
      repo_root="$(cd -- "$2" && pwd)"
      shift 2
      ;;
    --allow-dirty)
      allow_dirty=1
      shift
      ;;
    --help)
      echo "用法：$0 --version <semver> [--root <目录>] [--allow-dirty]"
      exit 0
      ;;
    *)
      echo "未知参数：$1" >&2
      exit 2
      ;;
  esac
done

if [[ -z "$version" ]]; then
  echo "必须指定 --version" >&2
  exit 2
fi
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]]; then
  echo "版本不是合法 SemVer：$version" >&2
  exit 1
fi

if [[ "$allow_dirty" != "1" ]] && [[ -n "$(git -C "$repo_root" status --porcelain --untracked-files=all)" ]]; then
  echo "工作树 dirty；发布构建默认拒绝，调试时显式使用 --allow-dirty" >&2
  exit 1
fi

python3 - "$repo_root" "$version" <<'PY'
import json
import re
import sys
from pathlib import Path


root = Path(sys.argv[1])
expected = sys.argv[2]
versions: dict[str, str] = {}


def add(name: str, value: str) -> None:
    versions[name] = value


def read_json(path: Path) -> None:
    document = json.loads(path.read_text(encoding="utf-8"))
    if isinstance(document.get("version"), str):
        add(path.relative_to(root).as_posix(), document["version"])


def read_gradle(path: Path) -> None:
    text = path.read_text(encoding="utf-8")
    match = re.search(r'\bversion\s*=\s*["\']([^"\']+)["\']', text)
    if match:
        add(path.relative_to(root).as_posix(), match.group(1))


workspace = re.search(
    r'^version\s*=\s*["\']([^"\']+)["\']',
    (root / "Cargo.toml").read_text(encoding="utf-8"),
    re.MULTILINE,
)
if not workspace:
    raise SystemExit("Cargo.toml 缺少 workspace version")
add("Cargo.toml", workspace.group(1))

for path in [root / "models/pyproject.toml", root / "sdk/python/pyproject.toml"]:
    text = path.read_text(encoding="utf-8")
    match = re.search(r'^version\s*=\s*["\']([^"\']+)["\']', text, re.MULTILINE)
    if not match:
        raise SystemExit(f"{path} 缺少 project version")
    add(path.relative_to(root).as_posix(), match.group(1))

for path in [root / "sdk/node/package.json", *sorted((root / "sdk/node/npm").glob("*/package.json"))]:
    read_json(path)
for path in [root / "sdk/java/build.gradle.kts", root / "sdk/kotlin/build.gradle.kts", root / "sdk/kotlin/android/build.gradle.kts"]:
    read_gradle(path)

mismatches = {name: value for name, value in versions.items() if value != expected}
if mismatches:
    details = ", ".join(f"{name}={value}" for name, value in sorted(mismatches.items()))
    raise SystemExit(f"版本不一致：期望 {expected}；{details}")
print(f"版本校验通过：{expected}（{len(versions)} 个版本来源）")
PY

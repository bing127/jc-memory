#!/usr/bin/env bash
set -euo pipefail

# 统一运行 Rust Fixture 校验和 Protobuf lint；跨语言 SDK 共用同一入口。
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo test -p jc-memory-protocol --test sdk_contract_fixtures
buf lint proto

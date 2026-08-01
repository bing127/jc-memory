#!/usr/bin/env bash
set -euo pipefail

# 统一运行 Rust Fixture、Protobuf、六种 SDK 和五种 Adapter 门禁。
# 外部平台不可用时默认记录跳过；CI 可设置 STRICT_SDK_CONTRACTS=1 使跳过变为失败。
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

strict="${STRICT_SDK_CONTRACTS:-0}"
server_pid=""
server_data=""

cleanup() {
  if [[ -n "$server_pid" ]]; then
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
  if [[ -n "$server_data" ]]; then
    rm -rf "$server_data"
  fi
}
trap cleanup EXIT

run_required() {
  local label="$1"
  shift
  printf '\n[门禁] %s\n' "$label"
  "$@"
}

run_optional() {
  local label="$1"
  local command_name="$2"
  shift 2
  if ! command -v "$command_name" >/dev/null 2>&1; then
    printf '[跳过] %s：缺少命令 %s\n' "$label" "$command_name"
    if [[ "$strict" == "1" ]]; then
      return 1
    fi
    return 0
  fi
  run_required "$label" "$@"
}

run_required "Rust Fixture 校验" cargo test -p jc-memory-protocol --test sdk_contract_fixtures
run_required "Protobuf lint" buf lint proto

# 用临时目录和合成 Token 启动一次本地 Server，只验证健康端点和生命周期。
if [[ "${JC_MEMORY_SDK_CONTRACTS_SKIP_SERVER:-0}" != "1" ]]; then
  server_data="$(mktemp -d)"
  JC_MEMORY_BIND="127.0.0.1:18787" \
  JC_MEMORY_GRPC_BIND="127.0.0.1:18788" \
  JC_MEMORY_DATA_DIR="$server_data" \
  JC_MEMORY_API_KEY="sk-jcmemory-87s92kdp5vqz4mnh3cr0xfbj6gwy1aue7t" \
  JC_MEMORY_NAMESPACE_ID="00000000-0000-7000-8000-000000000001" \
    cargo run -p jc-memory-server >/tmp/jc-memory-sdk-contracts-server.log 2>&1 &
  server_pid=$!
  health_ok=0
  for _ in {1..120}; do
    if curl --fail --silent http://127.0.0.1:18787/healthz >/dev/null 2>&1; then
      printf '\n[门禁] 本地 Server 健康检查通过（仅 Fixture/本机验证）\n'
      health_ok=1
      break
    fi
    sleep 0.25
  done
  if [[ "$health_ok" != "1" ]]; then
    cat /tmp/jc-memory-sdk-contracts-server.log >&2
    printf '[错误] 本地 Server 未在健康检查窗口内就绪\n' >&2
    exit 1
  fi
fi

run_optional "Node SDK Fixture 与类型检查" corepack corepack pnpm --dir sdk/node test
run_optional "Node SDK 类型检查" corepack corepack pnpm --dir sdk/node typecheck
if [[ -x .venv/bin/pytest ]]; then
  run_required "Python SDK Fixture" .venv/bin/pytest sdk/python/tests -q
else
  run_optional "Python SDK Fixture" python3 python3 -m pytest sdk/python/tests -q
fi
run_optional "Go SDK Fixture" go bash -c 'cd sdk/go && go test ./...'
run_optional "Kotlin JVM SDK Fixture" gradle gradle -p sdk/kotlin test --no-daemon
run_optional "Swift SDK Fixture" swift swift test --package-path sdk/swift
run_optional "Java SDK Fixture" gradle gradle -p sdk/java test --no-daemon

run_required "Adapter Fixture 与 Round-trip" cargo test -p jc-memory-adapter-hermes -p jc-memory-adapter-openclaw -p jc-memory-adapter-mem0 -p jc-memory-adapter-graphiti -p jc-memory-adapter-dify
printf '\n跨语言结果使用版本化 Fixture；本脚本不宣称外部平台或未安装工具的实机验证。\n'

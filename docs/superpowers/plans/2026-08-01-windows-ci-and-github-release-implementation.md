# Windows CI 定向验证与 GitHub Release 自动发布实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 支持手动只运行 Windows Rust 门禁，修复最后一个 Windows Named Pipe 测试，并在全部私有发布构建成功后自动发布 GitHub Release。

**Architecture:** `Rust Quality` 通过 `workflow_dispatch.windows_only` 选择单元素或三元素 matrix；普通 push/PR 仍运行三平台。`Private Release` 保留现有并行构建与 Actions Artifacts，新增最终聚合 job，以 Draft 作为上传事务边界，全部附件上传后自动切换为 Published。

**Tech Stack:** GitHub Actions YAML、GitHub CLI、Python `unittest` 工作流契约、Rust/Tokio Windows Named Pipe 测试。

---

### Task 1: 为 Windows-only 与自动 Release 写失败契约

**Files:**
- Modify: `scripts/ci/tests/test_workflows.py`
- Test: `scripts/ci/tests/test_workflows.py`

- [ ] **Step 1: 新增 Windows-only 失败契约**

在 `WorkflowContractTests` 中加入：

```python
def test_rust_quality_supports_windows_only_manual_dispatch(self) -> None:
    workflow = (WORKFLOW_DIR / "rust.yml").read_text(encoding="utf-8")
    self.assertIn("windows_only:", workflow)
    self.assertIn("type: boolean", workflow)
    self.assertIn('default: false', workflow)
    self.assertIn('["windows-latest"]', workflow)
    self.assertIn('["ubuntu-latest","windows-latest","macos-latest"]', workflow)
```

- [ ] **Step 2: 新增 Release 聚合失败契约**

```python
def test_private_release_publishes_only_after_all_packages_succeed(self) -> None:
    workflow = (WORKFLOW_DIR / "release.yml").read_text(encoding="utf-8")
    self.assertIn("contents: write", workflow)
    self.assertIn("publish:", workflow)
    self.assertRegex(workflow, r"needs:\s*\[validate, native, sdks, mobile\]")
    self.assertIn("actions/download-artifact@", workflow)
    self.assertIn("gh release create", workflow)
    self.assertIn("gh release upload", workflow)
    self.assertIn("gh release edit", workflow)

def test_private_release_uses_draft_as_atomic_publish_boundary(self) -> None:
    workflow = (WORKFLOW_DIR / "release.yml").read_text(encoding="utf-8")
    create = workflow.index("gh release create")
    upload = workflow.index("gh release upload")
    publish = workflow.index('gh release edit "$tag" --draft=false')
    self.assertLess(create, upload)
    self.assertLess(upload, publish)
    self.assertIn('isDraft', workflow)
    self.assertIn('--clobber', workflow)
```

- [ ] **Step 3: 运行新增契约并确认 RED**

Run:

```bash
python3 -m unittest \
  scripts.ci.tests.test_workflows.WorkflowContractTests.test_rust_quality_supports_windows_only_manual_dispatch \
  scripts.ci.tests.test_workflows.WorkflowContractTests.test_private_release_publishes_only_after_all_packages_succeed \
  scripts.ci.tests.test_workflows.WorkflowContractTests.test_private_release_uses_draft_as_atomic_publish_boundary
```

Expected: 3 项失败，分别指出缺少 `windows_only`、`contents: write` 或 `publish`。

### Task 2: 实现 Windows-only matrix

**Files:**
- Modify: `.github/workflows/rust.yml`
- Test: `scripts/ci/tests/test_workflows.py`

- [ ] **Step 1: 为手动触发增加布尔输入**

```yaml
  workflow_dispatch:
    inputs:
      windows_only:
        description: "仅运行 windows-latest，供 Windows 专项排障"
        required: true
        default: false
        type: boolean
```

- [ ] **Step 2: 根据输入生成 matrix**

```yaml
      matrix:
        os: ${{ fromJSON(inputs.windows_only && '["windows-latest"]' || '["ubuntu-latest","windows-latest","macos-latest"]') }}
```

`inputs.windows_only` 在 push/PR 上求值为 false，因此这些触发器继续运行三平台。

- [ ] **Step 3: 运行 Windows-only 契约**

Run:

```bash
python3 -m unittest scripts.ci.tests.test_workflows.WorkflowContractTests.test_rust_quality_supports_windows_only_manual_dispatch
```

Expected: PASS。

### Task 3: 实现 GitHub Release 最终聚合发布

**Files:**
- Modify: `.github/workflows/release.yml`
- Test: `scripts/ci/tests/test_workflows.py`

- [ ] **Step 1: 授予最小发布权限**

把 `permissions.contents` 改为 `write`，保留 `actions: write`。

- [ ] **Step 2: 下载并封装全部已验证 Artifact**

新增依赖全部构建 job 的 `publish`：

```yaml
  publish:
    name: Publish GitHub Release
    needs: [validate, native, sdks, mobile]
    runs-on: ubuntu-24.04
    timeout-minutes: 20
    steps:
      - name: 下载全部私有 Release Artifact
        uses: actions/download-artifact@d3f86a106a0bac45b974a628896c90dbdf5c8093
        with:
          path: target/release-downloads
      - name: 封装 GitHub Release 附件
        shell: bash
        run: |
          set -euo pipefail
          mkdir -p target/github-release
          found=0
          for directory in target/release-downloads/jcmem-${{ inputs.version }}-*; do
            test -d "$directory"
            name="$(basename "$directory")"
            tar -C "$directory" -czf "target/github-release/${name}.tar.gz" .
            found=1
          done
          test "$found" = 1
```

- [ ] **Step 3: 用 Draft 完成可恢复上传后自动发布**

```yaml
      - name: 自动发布 GitHub Release
        env:
          GH_TOKEN: ${{ github.token }}
        shell: bash
        run: |
          set -euo pipefail
          tag="v${{ inputs.version }}"
          if release_state="$(gh release view "$tag" --json isDraft --jq .isDraft 2>/dev/null)"; then
            test "$release_state" = true
          else
            create_args=("$tag" --verify-tag --draft --title "JiangCheng Memory $tag" --generate-notes)
            if [[ '${{ inputs.prerelease }}' == 'true' ]]; then
              create_args+=(--prerelease)
            fi
            gh release create "${create_args[@]}"
          fi
          gh release upload "$tag" target/github-release/*.tar.gz --clobber
          if [[ '${{ inputs.prerelease }}' == 'true' ]]; then
            gh release edit "$tag" --draft=false --prerelease
          else
            gh release edit "$tag" --draft=false
          fi
```

已 Published 的同 tag 会因 `test "$release_state" = true` 失败而保持不可变；上传中断留下的 Draft 可由 rerun 恢复。

- [ ] **Step 4: 运行 Release 契约并确认 GREEN**

Run:

```bash
python3 -m unittest \
  scripts.ci.tests.test_workflows.WorkflowContractTests.test_private_release_publishes_only_after_all_packages_succeed \
  scripts.ci.tests.test_workflows.WorkflowContractTests.test_private_release_uses_draft_as_atomic_publish_boundary
```

Expected: PASS。

### Task 4: 修复 Windows Named Pipe 名称契约

**Files:**
- Modify: `services/memory-sidecar/src/transport.rs`
- Test: `services/memory-sidecar/src/transport.rs`

- [ ] **Step 1: 保留 GitHub Windows RED 证据**

GitHub job `91339804464` 已稳定失败于 `windows_endpoint_mapping_is_stable_and_bounded`：测试把 `jc-memory-<hash>` 整段当作纯十六进制。

- [ ] **Step 2: 按真实协议检查摘要**

把最后一个反斜杠切分断言替换为：

```rust
let suffix = same
    .strip_prefix(r"\\.\pipe\jc-memory-")
    .expect("Named Pipe 名称必须保留固定前缀");
assert_eq!(suffix.len(), 16);
assert!(suffix.chars().all(|value| value.is_ascii_hexdigit()));
```

- [ ] **Step 3: 运行本地 sidecar 门禁**

Run:

```bash
cargo fmt --all -- --check
cargo clippy -p jc-memory-sidecar --all-targets --all-features -- -D warnings
cargo test -p jc-memory-sidecar --all-features
RUSTDOCFLAGS='-D warnings' cargo doc -p jc-memory-sidecar --no-deps
```

Expected: 全部 exit 0；本地 macOS 不描述为 Windows 实机证明。

### Task 5: 全量静态验证、提交和一次性推送

**Files:**
- Modify: `.github/workflows/rust.yml`
- Modify: `.github/workflows/release.yml`
- Modify: `scripts/ci/tests/test_workflows.py`
- Modify: `services/memory-sidecar/src/transport.rs`
- Create: `docs/superpowers/plans/2026-08-01-windows-ci-and-github-release-implementation.md`

- [ ] **Step 1: 运行全部工作流契约**

```bash
python3 -m unittest scripts.ci.tests.test_workflows
bash scripts/ci/validate-workflows.sh
```

Expected: 全部测试通过，7 个 YAML 文件校验通过。

- [ ] **Step 2: 检查范围和空白错误**

```bash
git diff --check -- \
  .github/workflows/rust.yml \
  .github/workflows/release.yml \
  scripts/ci/tests/test_workflows.py \
  services/memory-sidecar/src/transport.rs \
  docs/superpowers/plans/2026-08-01-windows-ci-and-github-release-implementation.md
git status --short
```

Expected: 无空白错误；仅暂存本计划列出的文件，保留 runtime、server、web 等用户修改。

- [ ] **Step 3: 提交并一次性推送**

```bash
git add -- \
  .github/workflows/rust.yml \
  .github/workflows/release.yml \
  scripts/ci/tests/test_workflows.py \
  services/memory-sidecar/src/transport.rs \
  docs/superpowers/plans/2026-08-01-windows-ci-and-github-release-implementation.md
git commit -m "feat(ci): target Windows checks and publish releases"
git push origin master
```

Expected: 设计提交和实施提交一起推送，只触发一次新的 master CI 批次。

### Task 6: GitHub 实机验证

**Files:**
- No repository changes.

- [ ] **Step 1: 等待 master push 的全部质量 workflow**

通过 GitHub Actions API 检查当前 HEAD 的 6 个 workflow；Rust Quality 的 Ubuntu、Windows、macOS job 和其他 5 个 workflow 均须成功。

- [ ] **Step 2: 手动触发 Windows-only**

向 `actions/workflows/rust.yml/dispatches` 发送：

```json
{"ref":"master","inputs":{"windows_only":"true"}}
```

Expected: 新 run 只包含 `Rust windows-latest` 一个 job，且完成 fmt、严格 Clippy、workspace 测试、Python 绑定测试和文档。

- [ ] **Step 3: 验证 Release Workflow 可调度边界**

读取 tag、CHANGELOG 和已有 Releases。只有在存在经用户确认的未发布 `v<version>` tag 时才实际触发 `Private Release`；否则报告 Workflow 契约已通过、真实发布待指定版本，不擅自创建 tag 或 Release。

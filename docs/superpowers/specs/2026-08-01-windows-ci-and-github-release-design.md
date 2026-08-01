# Windows CI 定向验证与 GitHub Release 自动发布设计

## 目标

本设计收敛三个相互关联的交付问题：

1. Windows 专属失败可以通过手动触发只运行 `windows-latest`，避免每轮诊断同时消耗 Ubuntu 和 macOS runner。
2. 当前 Windows Named Pipe 名称测试按真实名称结构验证固定长度十六进制摘要。
3. `Private Release` 的全部平台与 SDK 构建成功后，自动把完整私有发布包发布到 GitHub Releases，而不只保留 30 天的 Actions Artifact。

仓库 `bing127/jc-memory` 当前为 private。发布包继续保留 `PRIVATE` License、`public_publish: false`、SBOM、构建元数据、Manifest 和 SHA-256 校验清单，不改变其私有分发属性。

## 非目标

- 不让普通 `push` 或 Pull Request 自动创建 Release。
- 不跳过 Ubuntu、macOS 或供应链门禁作为最终合入标准。
- 不把未签名移动端包描述为已签名产物。
- 不改变 Fragment、Event、EvidenceRef 或其他权威数据语义。
- 不修改当前任务之外的 runtime、server 或 web 工作树内容。

## Windows-only 手动运行

`Rust Quality` 保留现有 `push`、`pull_request` 和 `workflow_dispatch` 触发器，并为 `workflow_dispatch` 增加布尔输入 `windows_only`，默认值为 `false`。

- `push`、`pull_request` 或 `windows_only=false`：matrix 仍为 Ubuntu、Windows、macOS。
- `workflow_dispatch` 且 `windows_only=true`：matrix 只包含 `windows-latest`。
- 每个 job 仍执行固定 Rust 1.97.1、fmt、严格 Clippy、Buf、workspace 测试、Python 绑定测试和 workspace 文档。

这样可在修复 Windows 专属问题时手动选择单平台运行；最终提交仍由常规 `master` push 完成三平台验证。

## Windows Named Pipe 测试修复

实际管道名格式为：

```text
\\.\pipe\jc-memory-<16 位十六进制摘要>
```

现有测试按最后一个反斜杠切分后，把 `jc-memory-<hash>` 整段误判为纯十六进制。测试应先移除完整固定前缀，再验证剩余部分长度为 16 且全部为 ASCII 十六进制字符。

生产映射算法和 Named Pipe 连接行为不变。GitHub Windows runner 上已经通过的连接、取消恢复和安全导出测试继续保留。

## GitHub Release 发布闭环

`Private Release` 继续只允许手动触发，并继续要求：

- `version` 是合法 SemVer。
- `ref` 指向已经存在的 `v<version>` tag。
- tag、CHANGELOG、License 和源码版本一致。

现有 native、SDK 和 mobile jobs 继续独立构建、封装、验证并上传 Actions Artifacts。新增最终 `publish` job，依赖 `validate`、`native`、`sdks`、`mobile` 全部成功，任何构建或验证失败都会阻止发布。

`publish` job 的流程为：

1. 下载本次 workflow 的全部 Actions Artifacts。
2. 将每个已验证发布目录封装为独立 `.tar.gz`，包内保留 Artifact、Manifest、SBOM、构建元数据和 SHA-256 清单。
3. 验证至少生成一个发布包，且文件名来自固定的 `jcmem-<version>-<target>` Artifact 名称。
4. 为 `v<version>` 创建 Draft Release；如果上次发布上传中断且同 tag 仍是 Draft，则复用该 Draft 并覆盖同名附件。
5. 如果同 tag 已经是正式发布状态，则拒绝覆盖，避免静默修改已发布版本。
6. 上传全部发布包成功后，将 Draft 自动切换为 Published。
7. `prerelease=true` 时发布为 Published Prerelease；否则发布为普通正式 Release。

Draft 只是上传过程中的事务边界。正常成功路径不会要求人工点击 Publish。

## 权限与可见性

`Private Release` 将 `contents` 权限从 `read` 调整为 `write`，用于创建 Release 和上传附件；`actions: write` 继续用于 Artifact 生命周期。其他质量 workflow 保持只读权限。

由于仓库是 private，GitHub Release 及附件只对有仓库访问权限的成员可见。发布包内部的 `public_publish: false` 保持不变。

## 失败处理与幂等性

- 构建或验证失败：不创建 Release。
- 下载、封装或上传失败：保留 Draft，附件不会以正式 Release 暴露。
- 重跑同一 tag：允许复用 Draft 并通过 `--clobber` 覆盖同名附件。
- 同一 tag 已 Published：发布 job 明确失败，不修改现有 Release。
- 发布完成：Actions Artifacts 仍按现有配置保留 30 天，GitHub Release 附件长期保留。

## 测试与验收

行为变更按 TDD 执行：先新增并运行会失败的工作流契约测试，再修改 YAML 和 Windows 测试。

本地门禁：

- `python3 -m unittest scripts.ci.tests.test_workflows`
- `bash scripts/ci/validate-workflows.sh`
- `cargo fmt --all -- --check`
- `cargo clippy -p jc-memory-sidecar --all-targets --all-features -- -D warnings`
- `cargo test -p jc-memory-sidecar --all-features`
- `RUSTDOCFLAGS='-D warnings' cargo doc -p jc-memory-sidecar --no-deps`
- `git diff --check`

GitHub 实机验收：

1. 手动运行 `Rust Quality` 且 `windows_only=true`，确认只创建 Windows job 并完成全部门禁。
2. 常规 `master` push 的 Rust matrix 仍创建三个系统 job。
3. 使用已有测试 tag 运行 `Private Release`，确认所有构建成功前无 Published Release，成功后 Releases 页面出现对应版本和全部附件。
4. 失败 Fixture 或契约测试不得描述为真实 GitHub Release 实机验证。

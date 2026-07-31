# Java/JVM UniFFI Embedded Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task with verification checkpoints.

**Goal:** 为 Java SDK 增加可在当前主机验证的 UniFFI Embedded 调用、native library SHA-256 校验和 classloader 生命周期管理。

**Architecture:** `memory-uniffi` 只包装 `memory-core::EmbeddedCommandBus`，通过 UniFFI 生成 Kotlin/JVM binding；Java `NativeLibraryLoader` 负责校验并加载 native library，`EmbeddedMemoryClient` 负责 Java API、错误和 handle 生命周期。远程 HTTP API 不改变。

**Tech Stack:** Rust 2024、UniFFI、Kotlin/JVM 2.1、Java 17、Gradle、JUnit 5、SHA-256。

---

### Task 1: 建立 memory-uniffi Rust binding 契约

**Files:**
- Create: `crates/memory-uniffi/Cargo.toml`
- Create: `crates/memory-uniffi/src/lib.rs`
- Create: `crates/memory-uniffi/src/jc_memory.udl`
- Modify: `Cargo.toml`
- Test: `crates/memory-uniffi/src/lib.rs`

- [ ] **Step 1: 写 Rust 失败测试**：用 `tempfile` 创建 engine，读取 `sdk/contracts/fixtures/recall-exact.json`，调用 `JcMemoryEngine::execute`，断言 response 的 `request_id` 和 `response.response`；另测 close 后返回 `closed`。
- [ ] **Step 2: 运行 `cargo test -p jc-memory-uniffi`，确认 crate 或类型缺失的红灯。**
- [x] **Step 3: 使用 UniFFI proc-macro metadata 暴露 Rust object**：构造函数打开 `MemoryEngine`，`execute` 反序列化 `MemoryRequest` 并通过 `EmbeddedCommandBus` 执行，返回 JSON；binding 内部使用 `shutdown` 避免与 Kotlin `AutoCloseable.close()` 冲突，所有错误转稳定 code/detail。
- [x] **Step 4: 运行 `cargo test -p jc-memory-uniffi`、`cargo fmt --check` 和 `cargo clippy -p jc-memory-uniffi --all-targets --all-features -- -D warnings`。**
- [x] **Step 5: 提交 `feat(sdk-uniffi): 增加 JVM Embedded Rust binding`，正文说明 UniFFI API、句柄生命周期和错误映射。**

### Task 2: 生成并验证 Kotlin/JVM binding

**Files:**
- Create: `sdk/java/src/main/kotlin/com/jiangcheng/memory/uniffi/jc_memory.kt`
- Create: `sdk/java/src/test/kotlin/com/jiangcheng/memory/UniFFIBindingContractTest.kt`
- Modify: `sdk/java/build.gradle.kts`
- Create: `scripts/build/build-java-uniffi.sh`

- [x] **Step 1: 写生成契约失败测试**：测试检查生成 binding 包含 `JcMemoryEngine`、`execute` 和 `shutdown`，缺失时失败。
- [x] **Step 2: 运行 `gradle -p sdk/java test --no-daemon`，确认 binding 文件尚不存在的红灯。**
- [x] **Step 3: 使用固定 UniFFI 0.32 版本生成 Kotlin binding**：`scripts/build/build-java-uniffi.sh` 构建 cdylib 后运行仓库内 `jc-memory-uniffi-bindgen`，输出到 `sdk/java/src/main/kotlin/uniffi/jc_memory_uniffi/`，不依赖用户 PATH 中未锁定的生成器版本。
- [x] **Step 4: 将 Java 工程切换为 Kotlin/JVM + Java 双源集，加入 JNA 运行时，运行 binding 契约测试，并执行 `git diff --check`。**
- [x] **Step 5: 提交 `build(sdk-java): 接入 UniFFI JVM binding 生成`，正文说明生成命令、版本和输出边界。**

### Task 3: 实现 NativeLibraryLoader

**Files:**
- Create: `sdk/java/src/main/java/com/jiangcheng/memory/NativeLibraryLoadException.java`
- Create: `sdk/java/src/main/java/com/jiangcheng/memory/NativeLibraryLoader.java`
- Create: `sdk/java/src/test/java/com/jiangcheng/memory/NativeLibraryLoaderTest.java`

- [x] **Step 1: 写失败测试**：覆盖缺失文件、摘要不匹配、大小写规范化、同 classloader 同路径缓存、不同 classloader 隔离、链接错误映射和 classloader 生命周期边界。
- [x] **Step 2: 运行 loader 测试，确认类和行为缺失的红灯。**
- [x] **Step 3: 实现 SHA-256 校验和 classloader/path 缓存**：使用 `MessageDigest`，路径必须是普通文件；校验通过后才调用可注入的 `NativeLibrarySystem.load`，生产实现委托 `System.load`。
- [x] **Step 4: 运行 `gradle -p sdk/java test --no-daemon`，确认所有 loader 测试通过。**
- [x] **Step 5: 提交 `feat(sdk-java): 增加 native loader 校验边界`，正文说明 SHA-256、缓存键和不可卸载语义。**

### Task 4: 接入 Java EmbeddedMemoryClient

**Files:**
- Create: `sdk/java/src/main/java/com/jiangcheng/memory/EmbeddedMemoryClient.java`
- Create: `sdk/java/src/test/java/com/jiangcheng/memory/EmbeddedMemoryClientTest.java`
- Modify: `sdk/java/src/main/java/com/jiangcheng/memory/JiangchengMemoryError.java`
- Modify: `docs/sdk/java.md`

- [x] **Step 1: 写失败测试**：用真实当前主机 native library 和 fixture 验证 Embedded recall；覆盖 close 幂等、关闭后调用、错误 code 和 loader checksum 错误。
- [x] **Step 2: 运行 Embedded 测试，确认 facade/native artifact 缺失的红灯。**
- [x] **Step 3: 实现 facade**：加载 native library 后创建生成 binding 的 `JcMemoryEngine`，`recall`/`execute` 保留原始 JSON，`close` 幂等且不尝试卸载 JVM library。
- [x] **Step 4: 构建当前主机 `jc-memory-uniffi` cdylib，计算 SHA-256，运行 Gradle Embedded 测试和 Rust 全部相关测试。**
- [x] **Step 5: 更新 Java 文档和 M5 计划，勾选已实际验证的 JVM Embedded、loader、checksum、classloader 生命周期；保留 Android/native slices 未勾选。**
- [ ] **Step 6: 提交 `feat(sdk-java): 接入 UniFFI JVM Embedded 客户端`，正文说明当前主机验证和未覆盖平台。**

### Task 5: 收口门禁与远程推送

**Files:**
- Modify: `docs/superpowers/specs/2026-07-31-java-uniffi-embedded-design.md`
- Modify: `docs/superpowers/plans/2026-07-31-java-uniffi-embedded-implementation.md`

- [ ] **Step 1: 运行 `cargo fmt --check`、`cargo test -p jc-memory-uniffi`、严格 Clippy、`gradle -p sdk/java clean test --no-daemon` 和 `git diff --check`。**
- [ ] **Step 2: 检查提交历史均为中文标题 + 中文正文，确认 `.superpowers/` 和构建产物未被暂存。**
- [ ] **Step 3: 提交 `docs(sdk-java): 更新 UniFFI Embedded 验证状态`，正文写明已验证项和未验证平台。**
- [ ] **Step 4: 推送 `feat/m5-sdk-adapters` 到私有 GitHub。**

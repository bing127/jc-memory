# 私有平台包

这些目录是 napi-rs 构建时使用的可选平台包入口，不代表已经发布到 npm。发布任务必须先由私有 CI 构建对应 `.node` 文件，再把文件和这里的 `package.json` 一起打包。

当前配置覆盖 macOS ARM64/x64、Linux x64/ARM64 和 Windows x64；未在当前主机验证的目标只在 CI 中标记为待验证。

# ONNX 模型部署

M6 Task 2 提供一个可重复的最小 ONNX Fixture，作为 Python 训练工程与 Rust `memory-neural` 之间的契约样本。模型输入是 `features` 动态 batch，输出固定为 `router_logits`、`importance` 和 `contradiction_logits`。

## 生成与验证

```bash
models/.venv/bin/python -m models.export.fixture
models/.venv/bin/python -m pytest models/tests/test_export.py -q
cargo test -p jc-memory-neural --test onnx
```

导出使用固定 opset 17、明确的输入/输出名称和动态 batch 轴。Manifest 保存模型 ID、版本、私有许可证、模型 SHA-256、输入输出 Schema、训练数据摘要、设备支持、最大 batch 和超时配置。文件被篡改、Manifest 缺字段或 Shape 不匹配时，Python 与 Rust 都 fail closed。

## Rust Provider

`jc-memory-neural::onnx::OnnxProvider::load` 先读取 Manifest，再校验 ONNX 文件 SHA-256，最后创建 CPU ONNX Runtime Session 并检查输入输出名称。`infer` 对输入 batch 和 Shape 做前置限制，主 Session 使用 Manifest timeout；超时、运行时错误和 Circuit Breaker 打开时返回稳定的数值 Fallback，并通过 `FallbackResult.reason` 保留降级原因。

当前验证只覆盖 macOS ARM64 当前主机的 CPU Fixture。CUDA、CoreML、DirectML、Linux/Windows 原生分发、生产模型质量和外部训练数据不在本次验证范围，不能把 Fixture 结果描述为全平台或生产效果。

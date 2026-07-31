# M6 模型训练工程

本目录只负责离线训练、评测和 Checkpoint，不参与 Rust Core 的运行时语义。Task 1 使用合成数值 Fixture 验证配置、稳定数据切分、CPU 多任务训练和 Checkpoint 校验；不包含真实 Token、原始敏感正文或生产模型权重。

## 创建环境

```bash
uv venv models/.venv --python 3.12
uv pip install --python models/.venv/bin/python -e 'models[dev]'
```

## 测试与 Smoke

```bash
models/.venv/bin/python -m pytest models/tests -q
models/.venv/bin/python -m models.training.smoke \
  --config models/configs/smoke.yaml \
  --output /tmp/jc-memory-m6-smoke.pt
```

Smoke 输出只证明当前主机 CPU 路径可重复运行两 epoch；它不代表生产模型质量、GPU/ONNX Runtime 或其他平台已经验证。

## Checkpoint 约束

Checkpoint 保存模型状态、优化器状态、配置、schema version、git commit、指标和文件内 SHA-256 尾标记。加载时先校验完整序列化内容，再校验配置和模型结构；篡改、截断、未知敏感字段、绝对路径和结构不一致都会 fail closed。

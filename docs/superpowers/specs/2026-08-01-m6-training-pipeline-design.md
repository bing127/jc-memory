# M6 Task 1 可重复模型训练工程设计

## 目标

建立独立的 Python 训练工程，为后续 ONNX 导出、模型一致性验证和 Benchmark 提供可重复的小型多任务模型。训练工程只负责离线训练、评测和 Checkpoint，不改变 Rust Core 的权威记录或模型无关 Memory Packet 语义。

## 范围与边界

本次只实现 M6 Task 1：

- YAML/JSON `TrainingConfig` 读取和严格校验；
- 以样本 ID 为输入的稳定数据切分和 DataLoader worker 随机种子；
- 覆盖 Router、Importance、Contradiction Fixture 的小型 PyTorch 多任务模型；
- 固定 Seed 的 CPU 两 epoch Smoke 训练；
- 带结构校验、敏感字段拒绝和 SHA-256 的 Checkpoint round-trip。

本次不实现 ONNX 导出、Rust `OnnxProvider`、生产训练数据、外部数据集下载或模型质量结论。训练输入使用版本化数值特征 Fixture，不读取或持久化真实 Token、绝对路径和原始敏感正文。

## 方案

新增独立 `models` Python 包，避免复用 Python SDK 的运行时依赖边界：

- `models/pyproject.toml` 声明 Python 3.11+、PyTorch、NumPy、PyYAML 和测试依赖；
- `models/training/config.py` 定义不可变配置对象，限制 batch、epoch、学习率、输入维度和 split 比例，并拒绝绝对路径、Token 字段和值；
- `models/training/data.py` 将带稳定 `sample_id` 的 Fixture 转为数值张量，使用 `sha256(seed + sample_id)` 排序切分，确保输入顺序变化不影响 split；
- `models/training/model.py` 使用共享 MLP 编码器和三个 head：Router 分类、Importance 回归、Contradiction 二分类；
- `models/training/train.py` 统一设置 Python、NumPy 和 PyTorch seed，固定 CPU 运算，记录每个 epoch 的损失和样本数；
- `models/training/checkpoint.py` 以明确 schema 保存模型状态、优化器状态、配置、schema version、git commit 和指标，写入后计算并保存 checkpoint 自身 SHA-256，加载时重新计算并拒绝不一致文件。

训练结果只作为可重复 Fixture 产物，不能自动变成 Verified Fact。后续 Task 2 通过稳定的输入输出 Schema 将该模型导出为 ONNX，并继续由 Rust Fallback 作为离线语义兜底。

## 错误处理

- 配置缺失、类型错误、范围越界、未知字段、绝对路径和 Token 内容统一在加载阶段失败；
- 空数据集、空样本 ID、重复样本 ID 和无效标签在构造 Dataset 时失败；
- Checkpoint schema version、模型结构、配置摘要或 SHA-256 不一致时拒绝恢复；
- 训练失败不生成看似有效的最终 Checkpoint，临时文件使用同目录原子替换；
- 错误信息只包含字段名和稳定原因，不回显 Token、原始正文或绝对路径。

## 测试策略

先写失败测试，再实现最小代码：

1. 同一 seed 和样本 ID 得到相同 split，不同 seed 得到不同 split；
2. 同一 seed 的两次 CPU Smoke 训练得到相同参数摘要和指标；
3. Checkpoint 保存后可恢复模型/优化器/配置，篡改字节、结构或敏感字段时被拒绝；
4. 配置 YAML/JSON round-trip 保持相同 canonical 内容；
5. 两 epoch CPU 训练命令在当前环境成功，并输出可审计的 schema version、git commit 和指标。

## 验收边界

验收只代表当前主机的 Python CPU Smoke 验证，不代表生产模型效果、GPU 加速、ONNX Runtime、跨平台构建或外部数据集实机兼容。所有未运行的目标平台和长训练都必须在后续文档中保持未勾选。

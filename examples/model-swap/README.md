# Model Swap Smoke

`jc-memory-benchmark-harness` 中的 `DeterministicModel` 用两个不同的回复前缀表达同一个结构化输入。Model Swap 只比较 Fragment、Evidence 和 CurrentState 的 canonical SHA-256，最终回复文本允许不同。

当前 Smoke 使用仓库内合成输入，不代表外部模型质量或生产负载验证。

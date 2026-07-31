"""用于 Router、Importance 和 Contradiction Fixture 的多任务模型。"""

from __future__ import annotations

from hashlib import sha256

from torch import Tensor, nn

from .config import TrainingConfig


class MultiTaskModel(nn.Module):
    """共享 MLP 编码器和三个明确命名的输出 head。"""

    def __init__(self, input_dim: int, hidden_dim: int, router_classes: int) -> None:
        super().__init__()
        self.encoder = nn.Sequential(nn.Linear(input_dim, hidden_dim), nn.ReLU())
        self.router_head = nn.Linear(hidden_dim, router_classes)
        self.importance_head = nn.Linear(hidden_dim, 1)
        self.contradiction_head = nn.Linear(hidden_dim, 2)

    @classmethod
    def from_config(cls, config: TrainingConfig) -> MultiTaskModel:
        """按配置创建网络结构。"""
        return cls(config.input_dim, config.hidden_dim, config.router_classes)

    def forward(self, features: Tensor) -> dict[str, Tensor]:
        hidden = self.encoder(features)
        return {
            "router_logits": self.router_head(hidden),
            "importance": self.importance_head(hidden).squeeze(-1),
            "contradiction_logits": self.contradiction_head(hidden),
        }


def parameter_digest(model: nn.Module) -> str:
    """按参数名、类型、形状和字节计算稳定摘要。"""
    digest = sha256()
    for name, tensor in sorted(model.state_dict().items()):
        value = tensor.detach().cpu().contiguous()
        digest.update(name.encode("utf-8"))
        digest.update(str(value.dtype).encode("ascii"))
        digest.update(repr(tuple(value.shape)).encode("ascii"))
        digest.update(value.numpy().tobytes())
    return digest.hexdigest()

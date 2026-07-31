//! 可版本化的 Capability 注册、授权和运行统计。

mod model;
mod registry;
mod stats;

pub use model::{
    CapabilityDescriptor, CapabilityError, CapabilityExecutor, CapabilityKind, CapabilityRequest,
    Permission, RiskLevel,
};
pub use registry::{CapabilityRegistry, ResolvedCapability};
pub use stats::{CapabilityRanking, CapabilityStats, CapabilityStatsStore};

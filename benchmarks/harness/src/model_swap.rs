//! Deterministic Model Swap 结构化一致性比较。

use jc_memory_protocol::{FragmentId, MemoryPacket};
use serde::Serialize;
use sha2::{Digest, Sha256};

/// 一个模型的输出；最终文本不参与一致性判断。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelOutput {
    /// 模型可以不同的最终回复文本。
    pub final_response: String,
    /// Fragment/Evidence/CurrentState 的 canonical SHA-256。
    pub canonical_hash: String,
}

/// 使用固定回复前缀的确定性表达器。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeterministicModel {
    response_prefix: String,
}

impl DeterministicModel {
    /// 创建一个确定性表达器。
    #[must_use]
    pub fn new(response_prefix: &str) -> Self {
        Self {
            response_prefix: response_prefix.to_owned(),
        }
    }

    /// 对同一结构化输入生成可比较的输出。
    #[must_use]
    pub fn infer(&self, fragment_ids: &[FragmentId], packet: &MemoryPacket) -> ModelOutput {
        let snapshot = Snapshot {
            fragment_ids: sorted_strings(fragment_ids.iter().map(ToString::to_string)),
            evidence_ids: sorted_strings(
                packet
                    .evidence
                    .iter()
                    .filter_map(|evidence| evidence.evidence_ref_id.map(|id| id.to_string())),
            ),
            current_state_ids: sorted_strings(
                packet
                    .current_states
                    .iter()
                    .map(|state| state.state_id.to_string()),
            ),
        };
        let canonical = serde_json::to_vec(&snapshot).unwrap_or_default();
        ModelOutput {
            final_response: format!(
                "{}: {} 个 Fragment",
                self.response_prefix,
                fragment_ids.len()
            ),
            canonical_hash: hex::encode(Sha256::digest(canonical)),
        }
    }
}

/// 只比较结构化 canonical hash，忽略最终回复文本。
#[must_use]
pub fn compare(left: &ModelOutput, right: &ModelOutput) -> bool {
    left.canonical_hash == right.canonical_hash
}

#[derive(Serialize)]
#[allow(clippy::struct_field_names)]
struct Snapshot {
    fragment_ids: Vec<String>,
    evidence_ids: Vec<String>,
    current_state_ids: Vec<String>,
}

fn sorted_strings<I>(values: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut values: Vec<_> = values.into_iter().collect();
    values.sort_unstable();
    values
}

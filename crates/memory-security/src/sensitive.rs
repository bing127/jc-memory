//! 敏感数据的确定性低成本检测。

use jc_memory_protocol::CognitiveFragment;

use crate::{
    injection::content_text,
    policy::QuarantinePolicy,
    quarantine::{Detection, RiskLabel},
};

const DETECTOR_VERSION: &str = "sensitive-rules/1";

pub(crate) fn sensitive_data(
    fragment: &CognitiveFragment,
    policy: &QuarantinePolicy,
) -> Option<Detection> {
    let text = content_text(&fragment.content).to_lowercase();
    policy.sensitive_data().matches(&text).then_some(Detection {
        label: RiskLabel::SensitiveData,
        score: policy.sensitive_data().score(),
        detector_version: DETECTOR_VERSION,
        note: "敏感数据规则命中了凭据或私钥材料",
    })
}

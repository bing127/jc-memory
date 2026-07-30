//! 敏感数据的确定性低成本检测。

use jc_memory_protocol::CognitiveFragment;

use crate::{
    injection::content_text,
    quarantine::{Detection, RiskLabel},
};

const DETECTOR_VERSION: &str = "sensitive-rules/1";

pub(crate) fn sensitive_data(fragment: &CognitiveFragment) -> Option<Detection> {
    let text = content_text(&fragment.content).to_lowercase();
    [
        "-----begin private key-----",
        "api_key=",
        "api-key=",
        "password=",
        "access_token=",
        "secret_key=",
    ]
    .iter()
    .any(|marker| text.contains(marker))
    .then_some(Detection {
        label: RiskLabel::SensitiveData,
        score: 0.9,
        detector_version: DETECTOR_VERSION,
        note: "敏感数据规则命中了凭据或私钥材料",
    })
}

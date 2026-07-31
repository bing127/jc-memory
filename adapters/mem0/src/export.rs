//! Mem0 ID 映射清单导出。

use crate::{Mem0Error, Mem0IdMappingManifest, import::MANIFEST_VERSION};

/// 将 ID mapping manifest 导出为稳定 JSON。
///
/// # Errors
///
/// Manifest 无法序列化时返回错误。
pub fn export_manifest(manifest: &Mem0IdMappingManifest) -> Result<String, Mem0Error> {
    Ok(serde_json::to_string_pretty(manifest)?)
}

/// 解析并校验一个 ID mapping manifest。
///
/// # Errors
///
/// JSON 无法解析或 `manifest_version` 不受支持时返回错误。
pub fn parse_manifest(text: &str) -> Result<Mem0IdMappingManifest, Mem0Error> {
    let manifest: Mem0IdMappingManifest = serde_json::from_str(text)?;
    if manifest.manifest_version != MANIFEST_VERSION {
        return Err(Mem0Error::UnsupportedManifestVersion(
            manifest.manifest_version,
        ));
    }
    Ok(manifest)
}

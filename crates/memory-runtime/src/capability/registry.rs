//! Capability 注册表与 ACL 解析。

use std::{collections::BTreeMap, sync::Arc};

use jc_memory_protocol::{AccessContext, Grant, Operation};
use semver::Version;

use super::{
    CapabilityDescriptor, CapabilityError, CapabilityExecutor, CapabilityRequest, Permission,
};

struct Entry {
    descriptor: CapabilityDescriptor,
    executor: Arc<dyn CapabilityExecutor>,
}

/// 解析后可执行的 Capability。
pub struct ResolvedCapability {
    /// 经过版本和 ACL 校验的描述符。
    pub descriptor: CapabilityDescriptor,
    /// 对应的异步执行器。
    pub executor: Arc<dyn CapabilityExecutor>,
}

/// 进程内的版本化 Capability 注册表。
pub struct CapabilityRegistry {
    entries: std::sync::RwLock<BTreeMap<String, BTreeMap<Version, Arc<Entry>>>>,
}

impl CapabilityRegistry {
    /// 创建空注册表。
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: std::sync::RwLock::new(BTreeMap::new()),
        }
    }

    /// 注册一个不可重复的 ID 与版本组合。
    ///
    /// # Errors
    ///
    /// 描述符不合法、ID 与版本已存在或注册表锁不可用时返回错误。
    pub fn register(
        &self,
        descriptor: CapabilityDescriptor,
        executor: Arc<dyn CapabilityExecutor>,
    ) -> Result<(), CapabilityError> {
        descriptor.validate()?;
        let version = descriptor.version.clone();
        let id = descriptor.id.clone();
        let mut entries = self
            .entries
            .write()
            .map_err(|_| CapabilityError::Execution("capability registry lock poisoned".into()))?;
        let versions = entries.entry(id.clone()).or_default();
        if versions.contains_key(&version) {
            return Err(CapabilityError::Duplicate(id, version));
        }
        versions.insert(
            version,
            Arc::new(Entry {
                descriptor,
                executor,
            }),
        );
        Ok(())
    }

    /// 按请求版本选择最高兼容版本，并检查调用者的 Scope Grant。
    ///
    /// # Errors
    ///
    /// 请求无效、版本不存在或调用者缺少声明权限对应的 Grant 时返回错误。
    pub fn resolve(
        &self,
        request: &CapabilityRequest,
        access: &AccessContext,
    ) -> Result<ResolvedCapability, CapabilityError> {
        if request.id.trim().is_empty() {
            return Err(CapabilityError::InvalidRequest(
                "capability id must be non-empty".into(),
            ));
        }
        let entries = self
            .entries
            .read()
            .map_err(|_| CapabilityError::Execution("capability registry lock poisoned".into()))?;
        let versions = entries
            .get(&request.id)
            .ok_or_else(|| CapabilityError::NotFound(request.id.clone()))?;
        let entry = versions
            .iter()
            .rev()
            .find(|(version, _)| request.version.matches(version))
            .map(|(_, entry)| Arc::clone(entry))
            .ok_or_else(|| CapabilityError::VersionMismatch(request.id.clone()))?;
        for permission in &entry.descriptor.permissions {
            if !has_permission_grant(permission, access) {
                return Err(CapabilityError::PermissionDenied(permission.clone()));
            }
        }
        Ok(ResolvedCapability {
            descriptor: entry.descriptor.clone(),
            executor: Arc::clone(&entry.executor),
        })
    }

    /// 读取已注册版本对应的执行器，供统计或调度层复用。
    ///
    /// # Errors
    ///
    /// 描述符未注册或注册表锁不可用时返回错误。
    pub fn executor(
        &self,
        descriptor: &CapabilityDescriptor,
    ) -> Result<Arc<dyn CapabilityExecutor>, CapabilityError> {
        let entries = self
            .entries
            .read()
            .map_err(|_| CapabilityError::Execution("capability registry lock poisoned".into()))?;
        let entry = entries
            .get(&descriptor.id)
            .and_then(|versions| versions.get(&descriptor.version))
            .ok_or_else(|| CapabilityError::NotFound(descriptor.id.clone()))?;
        Ok(Arc::clone(&entry.executor))
    }
}

impl Default for CapabilityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn has_permission_grant(permission: &Permission, access: &AccessContext) -> bool {
    let operation = match permission {
        Permission::ReadMemory => Operation::Recall,
        Permission::WriteMemory => Operation::Mutate,
        Permission::Network
        | Permission::Filesystem
        | Permission::Process
        | Permission::ExternalSideEffect => Operation::Admin,
    };
    let grant = Grant::for_scope(operation, &access.scope);
    access.grants.contains(&grant)
}

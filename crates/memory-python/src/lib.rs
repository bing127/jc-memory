//! Python 的 `PyO3` 嵌入式边界。
//!
//! `PyO3` 宏会生成 Python C-API 所需的 `unsafe` glue。该例外仅限于此绑定 crate；核心
//! Rust crate 继续使用工作区的 `unsafe_code = forbid`，所有命令仍交给 Core `CommandBus`。

use jc_memory_core::{CommandBus, EmbeddedCommandBus, MemoryConfig, MemoryEngine};
use jc_memory_protocol::{
    AccessContext, Grant, MemoryCommand, MemoryRequest, MemoryScope, Operation,
};
use pyo3::exceptions::PyRuntimeError;
use pyo3::types::{PyModule, PyModuleMethods};
use pyo3::{Bound, Py, PyAny, PyResult, Python, pyclass, pymethods, pymodule};
use pyo3_async_runtimes::tokio::future_into_py;

/// Python 进程内复用的 Rust Core 引擎句柄。
#[pyclass]
pub struct PythonMemoryEngine {
    bus: EmbeddedCommandBus,
    principal_id: String,
}

/// 异步打开本地嵌入式引擎并返回 Python 句柄。
#[pymethods]
impl PythonMemoryEngine {
    /// 执行一个 JSON Request Envelope，并返回 JSON Response Envelope。
    ///
    /// # Errors
    ///
    /// 请求格式、Core ACL 或持久化执行失败时返回 Python 异常。
    pub fn execute<'py>(
        &self,
        py: Python<'py>,
        request_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let bus = self.bus.clone();
        let principal_id = self.principal_id.clone();
        future_into_py(py, async move {
            execute_json(bus, principal_id, request_json)
                .await
                .map_err(|error| PyRuntimeError::new_err(error.to_string()))
        })
    }

    /// 关闭底层共享资源；重复关闭保持幂等。
    ///
    /// # Errors
    ///
    /// Core 资源无法安全关闭时返回 Python 异常。
    pub fn close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let bus = self.bus.clone();
        future_into_py(py, async move {
            bus.shutdown()
                .await
                .map_err(|error| PyRuntimeError::new_err(error.to_string()))
        })
    }
}

/// 异步构造 `PythonMemoryEngine`，避免在已有事件循环中阻塞。
#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PythonMemoryEngine>()?;
    module.add_function(pyo3::wrap_pyfunction!(open_memory_engine, module)?)?;
    Ok(())
}

#[pyo3::pyfunction]
fn open_memory_engine(
    py: Python<'_>,
    data_dir: String,
    principal_id: String,
) -> PyResult<Bound<'_, PyAny>> {
    future_into_py(py, async move {
        let engine = MemoryEngine::open(MemoryConfig::local(data_dir))
            .await
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        Python::attach(|py| {
            Py::new(
                py,
                PythonMemoryEngine {
                    bus: EmbeddedCommandBus::new(engine),
                    principal_id,
                },
            )
        })
    })
}

async fn execute_json(
    bus: EmbeddedCommandBus,
    principal_id: String,
    request_json: String,
) -> Result<String, PythonBridgeError> {
    let request: MemoryRequest = serde_json::from_str(&request_json)?;
    let scope: MemoryScope = request.access_scope.clone().into();
    let mut access = AccessContext::new(principal_id, scope.clone());
    access.request_id = request.request_id.clone();
    access
        .grants
        .insert(Grant::for_scope(operation_for(&request.command), &scope));
    let response = bus.execute(&access, request).await?;
    Ok(serde_json::to_string(&response)?)
}

fn operation_for(command: &MemoryCommand) -> Operation {
    match command {
        MemoryCommand::Observe(_) | MemoryCommand::Commit(_) => Operation::Observe,
        MemoryCommand::Recall(_) | MemoryCommand::Stats(_) => Operation::Recall,
        MemoryCommand::Explain(_) => Operation::Explain,
        MemoryCommand::Correct(_) | MemoryCommand::Forget(_) => Operation::Mutate,
    }
}

#[derive(Debug, thiserror::Error)]
enum PythonBridgeError {
    #[error("请求 Envelope 无效: {0}")]
    Request(#[from] serde_json::Error),
    #[error("Core 命令执行失败: {0}")]
    Core(#[from] jc_memory_core::CommandError),
}

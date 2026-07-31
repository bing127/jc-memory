//! Go 和其他 C ABI 调用方使用的受控嵌入式边界。
//!
//! 只有本 crate 允许绑定层所需的原始指针操作。每一处 `unsafe` 都对应 C 头文件中的
//! 长度、所有权或生命周期不变量；Core 命令、ACL 和持久化仍由安全 Rust API 执行。
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::{
    collections::HashMap,
    ffi::CStr,
    os::raw::c_char,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr, slice,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use jc_memory_core::{CommandBus, EmbeddedCommandBus, MemoryConfig, MemoryEngine};
use jc_memory_protocol::{
    AccessContext, Grant, MemoryCommand, MemoryRequest, MemoryScope, Operation,
};
use tokio::runtime::Runtime;
use tokio_util::sync::CancellationToken;

struct HandleState {
    runtime: Runtime,
    bus: EmbeddedCommandBus,
    principal_id: String,
}

/// C ABI 句柄；调用方必须在 destroy 后放弃指针。
pub struct JcMemoryHandle {
    state: Mutex<Option<HandleState>>,
    generation: AtomicU64,
    cancellations: Mutex<HashMap<u64, (u64, CancellationToken)>>,
}

/// 创建一个本地嵌入式句柄，失败返回 NULL。
#[unsafe(no_mangle)]
pub extern "C" fn jcmemory_create(
    data_dir: *const c_char,
    principal_id: *const c_char,
) -> *mut JcMemoryHandle {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let data_dir = c_string(data_dir)?;
        let principal_id = c_string(principal_id)?;
        let runtime = Runtime::new().ok()?;
        let engine = runtime
            .block_on(MemoryEngine::open(MemoryConfig::local(data_dir)))
            .ok()?;
        Some(Box::new(JcMemoryHandle {
            state: Mutex::new(Some(HandleState {
                runtime,
                bus: EmbeddedCommandBus::new(engine),
                principal_id,
            })),
            generation: AtomicU64::new(1),
            cancellations: Mutex::new(HashMap::new()),
        }))
    }));
    match result {
        Ok(Some(handle)) => Box::into_raw(handle),
        _ => ptr::null_mut(),
    }
}

/// 关闭并释放句柄；NULL 输入安全地忽略。
#[unsafe(no_mangle)]
pub extern "C" fn jcmemory_destroy(handle: *mut JcMemoryHandle) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if handle.is_null() {
            return;
        }
        // SAFETY: 调用方只能对仍然存活且尚未 destroy 的句柄调用 destroy；头文件明确该生命周期。
        let handle = unsafe { Box::from_raw(handle) };
        handle.generation.fetch_add(1, Ordering::AcqRel);
        if let Ok(mut cancellations) = handle.cancellations.lock() {
            for (_, token) in cancellations.values() {
                token.cancel();
            }
            cancellations.clear();
        }
        if let Ok(mut state) = handle.state.lock()
            && let Some(state) = state.take()
        {
            let _ = state.runtime.block_on(state.bus.shutdown());
        }
    }));
}

/// 执行一个 JSON Request Envelope，并返回新分配的 JSON 缓冲区。
#[unsafe(no_mangle)]
pub extern "C" fn jcmemory_execute(
    handle: *mut JcMemoryHandle,
    request: *const u8,
    request_len: usize,
    out_len: *mut usize,
) -> *mut u8 {
    jcmemory_execute_with_token(handle, 0, request, request_len, out_len)
}

/// 带取消 token 的执行变体；token 为 0 时不注册取消任务。
#[unsafe(no_mangle)]
pub extern "C" fn jcmemory_execute_with_token(
    handle: *mut JcMemoryHandle,
    token: u64,
    request: *const u8,
    request_len: usize,
    out_len: *mut usize,
) -> *mut u8 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if handle.is_null() || request.is_null() || out_len.is_null() {
            return None;
        }
        // SAFETY: C 调用方必须保证 request 指向 request_len 个可读字节；out_len 可写一个 usize。
        let (handle, bytes) = unsafe { (&*handle, slice::from_raw_parts(request, request_len)) };
        let request = serde_json::from_slice::<MemoryRequest>(bytes).ok()?;
        let mut state = handle.state.lock().ok()?;
        let state = state.as_mut()?;
        let generation = handle.generation.load(Ordering::Acquire);
        let cancellation = if token == 0 {
            None
        } else {
            let value = CancellationToken::new();
            handle
                .cancellations
                .lock()
                .ok()?
                .insert(token, (generation, value.clone()));
            Some(value)
        };
        let response = state
            .runtime
            .block_on(execute_request(
                state.bus.clone(),
                state.principal_id.clone(),
                request,
                cancellation.clone(),
            ))
            .ok()?;
        if let Some(cancellation) = cancellation {
            if cancellation.is_cancelled() {
                return None;
            }
            if let Ok(mut values) = handle.cancellations.lock() {
                values.remove(&token);
            }
        }
        let output = serde_json::to_vec(&response).ok()?;
        // SAFETY: out_len 由上面的非空检查保证可写；output 的所有权移交给 C 调用方。
        unsafe { *out_len = output.len() };
        Some(output)
    }));
    let output = result.ok().flatten().unwrap_or_default();
    if out_len.is_null() {
        return ptr::null_mut();
    }
    if output.is_empty() {
        // SAFETY: out_len 已确认非空，失败时把长度归零。
        unsafe { *out_len = 0 };
        return ptr::null_mut();
    }
    let output = output.into_boxed_slice();
    Box::into_raw(output).cast::<u8>()
}

/// 取消指定 token；不存在或已完成的 token 返回 0。
#[unsafe(no_mangle)]
pub extern "C" fn jcmemory_cancel(handle: *mut JcMemoryHandle, token: u64) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if handle.is_null() || token == 0 {
            return 0;
        }
        // SAFETY: 仅借用仍存活句柄，读取不跨越调用边界保存裸指针。
        let handle = unsafe { &*handle };
        let Ok(values) = handle.cancellations.lock() else {
            return 0;
        };
        let generation = handle.generation.load(Ordering::Acquire);
        if let Some((owner_generation, value)) = values.get(&token)
            && *owner_generation == generation
        {
            value.cancel();
            return 1;
        }
        0
    }));
    result.unwrap_or(0)
}

/// 释放 execute 返回的缓冲区；长度必须与返回时的 `out_len` 一致。
#[unsafe(no_mangle)]
pub extern "C" fn jcmemory_free_buffer(buffer: *mut u8, buffer_len: usize) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if buffer.is_null() {
            return;
        }
        // SAFETY: execute 使用同样长度的 Box<[u8]> 分配；调用方必须原样传回长度。
        let slice = ptr::slice_from_raw_parts_mut(buffer, buffer_len);
        unsafe { drop(Box::from_raw(slice)) };
    }));
}

fn c_string(value: *const c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }
    // SAFETY: C 调用方必须提供 NUL 结尾 UTF-8 字符串；无效 UTF-8 直接拒绝。
    unsafe { CStr::from_ptr(value).to_str().ok().map(str::to_owned) }
}

async fn execute_request(
    bus: EmbeddedCommandBus,
    principal_id: String,
    request: MemoryRequest,
    cancellation: Option<CancellationToken>,
) -> Result<jc_memory_protocol::ResponseEnvelope, jc_memory_core::CommandError> {
    let scope: MemoryScope = request.access_scope.clone().into();
    let mut access = AccessContext::new(principal_id, scope.clone());
    access.request_id = request.request_id.clone();
    access
        .grants
        .insert(Grant::for_scope(operation_for(&request.command), &scope));
    let execute = bus.execute(&access, request);
    if let Some(cancellation) = cancellation {
        tokio::select! {
            () = cancellation.cancelled() => Err(jc_memory_core::CommandError::RequestIdMismatch),
            result = execute => result,
        }
    } else {
        execute.await
    }
}

fn operation_for(command: &MemoryCommand) -> Operation {
    match command {
        MemoryCommand::Observe(_) | MemoryCommand::Commit(_) => Operation::Observe,
        MemoryCommand::Recall(_) | MemoryCommand::Stats(_) => Operation::Recall,
        MemoryCommand::Explain(_) => Operation::Explain,
        MemoryCommand::Correct(_) | MemoryCommand::Forget(_) => Operation::Mutate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::CString, fs};

    #[test]
    fn null_inputs_are_rejected_without_panicking() {
        assert!(jcmemory_create(ptr::null(), ptr::null()).is_null());
        let mut output_len = 7;
        assert!(jcmemory_execute(ptr::null_mut(), ptr::null(), 0, &raw mut output_len).is_null());
        assert_eq!(output_len, 0);
        assert_eq!(jcmemory_cancel(ptr::null_mut(), 1), 0);
        jcmemory_free_buffer(ptr::null_mut(), 0);
    }

    #[test]
    fn execute_round_trip_and_buffer_ownership_are_stable() {
        let directory = tempfile::tempdir().unwrap();
        let path = CString::new(directory.path().to_str().unwrap()).unwrap();
        let principal = CString::new("ffi-test").unwrap();
        let handle = jcmemory_create(path.as_ptr(), principal.as_ptr());
        assert!(!handle.is_null());
        let fixture = fs::read("../../sdk/contracts/fixtures/recall-exact.json").unwrap();
        let fixture: serde_json::Value = serde_json::from_slice(&fixture).unwrap();
        let fixture = serde_json::to_vec(&fixture["request"]).unwrap();
        let parsed: MemoryRequest = serde_json::from_slice(&fixture).unwrap();
        assert_eq!(parsed.request_id, "fixture-recall-exact");
        let mut output_len = 0;
        let output = jcmemory_execute(handle, fixture.as_ptr(), fixture.len(), &raw mut output_len);
        assert!(!output.is_null());
        // SAFETY: output 和长度刚由 execute 返回，下面只读并随后交还 free_buffer。
        let response = unsafe { std::slice::from_raw_parts(output, output_len) };
        let value: serde_json::Value = serde_json::from_slice(response).unwrap();
        assert_eq!(value["request_id"], "fixture-recall-exact");
        assert_eq!(value["response"]["response"], "recall");
        jcmemory_free_buffer(output, output_len);
        jcmemory_destroy(handle);
    }
}

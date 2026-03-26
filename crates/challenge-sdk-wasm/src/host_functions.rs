use alloc::vec;
use alloc::vec::Vec;

const RESPONSE_BUF_SMALL: usize = 4096;
const RESPONSE_BUF_MEDIUM: usize = 5 * 1024 * 1024;
const RESPONSE_BUF_LARGE: usize = 4 * 1024 * 1024;

#[link(wasm_import_module = "platform_network")]
extern "C" {
    fn http_get(req_ptr: i32, req_len: i32, resp_ptr: i32, resp_len: i32) -> i32;
    fn http_post(req_ptr: i32, req_len: i32, resp_ptr: i32, resp_len: i32, extra: i32) -> i32;
    fn dns_resolve(req_ptr: i32, req_len: i32, resp_ptr: i32) -> i32;
}

#[link(wasm_import_module = "platform_storage")]
extern "C" {
    fn storage_get(key_ptr: i32, key_len: i32, value_ptr: i32) -> i32;
    fn storage_set(key_ptr: i32, key_len: i32, value_ptr: i32, value_len: i32) -> i32;
    fn storage_get_cross(
        cid_ptr: i32,
        cid_len: i32,
        key_ptr: i32,
        key_len: i32,
        value_ptr: i32,
    ) -> i32;
    fn storage_list_prefix(prefix_ptr: i32, prefix_len: i32, result_ptr: i32, limit: i32) -> i32;
    fn storage_count_prefix(prefix_ptr: i32, prefix_len: i32) -> i64;
}

#[link(wasm_import_module = "platform_terminal")]
extern "C" {
    fn terminal_exec(cmd_ptr: i32, cmd_len: i32, result_ptr: i32, result_len: i32) -> i32;
    fn terminal_read_file(path_ptr: i32, path_len: i32, buf_ptr: i32, buf_len: i32) -> i32;
    fn terminal_write_file(path_ptr: i32, path_len: i32, data_ptr: i32, data_len: i32) -> i32;
    fn terminal_list_dir(path_ptr: i32, path_len: i32, buf_ptr: i32, buf_len: i32) -> i32;
    fn terminal_get_time() -> i64;
    fn terminal_random_seed(buf_ptr: i32, buf_len: i32) -> i32;
}

pub fn host_http_get(request: &[u8]) -> Result<Vec<u8>, i32> {
    let mut response_buf = vec![0u8; 10 * 1024 * 1024]; // 10MB for large API responses
                                                        // SAFETY: FFI call to the WASM host. The host guarantees:
                                                        // - request pointer and length are valid for reads
                                                        // - response_buf pointer and length are valid for writes
                                                        // - The host writes at most response_buf.len() bytes
    let status = unsafe {
        http_get(
            request.as_ptr() as i32,
            request.len() as i32,
            response_buf.as_mut_ptr() as i32,
            response_buf.len() as i32,
        )
    };
    if status < 0 {
        return Err(status);
    }
    response_buf.truncate(status as usize);
    Ok(response_buf)
}

pub fn host_http_post(request: &[u8], body: &[u8]) -> Result<Vec<u8>, i32> {
    let mut response_buf = vec![0u8; RESPONSE_BUF_MEDIUM];
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - request pointer and length are valid for reads
    // - response_buf pointer and length are valid for writes
    // - The host writes at most response_buf.len() bytes
    let status = unsafe {
        http_post(
            request.as_ptr() as i32,
            request.len() as i32,
            response_buf.as_mut_ptr() as i32,
            response_buf.len() as i32,
            body.len() as i32,
        )
    };
    if status < 0 {
        return Err(status);
    }
    response_buf.truncate(status as usize);
    Ok(response_buf)
}

pub fn host_dns_resolve(request: &[u8]) -> Result<Vec<u8>, i32> {
    let mut response_buf = vec![0u8; RESPONSE_BUF_SMALL];
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - request pointer and length are valid for reads
    // - response_buf pointer is valid for writes
    // - The host writes at most response_buf.len() bytes
    let status = unsafe {
        dns_resolve(
            request.as_ptr() as i32,
            request.len() as i32,
            response_buf.as_mut_ptr() as i32,
        )
    };
    if status < 0 {
        return Err(status);
    }
    response_buf.truncate(status as usize);
    Ok(response_buf)
}

pub fn host_storage_get(key: &[u8]) -> Result<Vec<u8>, i32> {
    let mut value_buf = vec![0u8; RESPONSE_BUF_MEDIUM];
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - key pointer and length are valid for reads
    // - value_buf pointer is valid for writes
    // - The host writes at most value_buf.len() bytes
    let status = unsafe {
        storage_get(
            key.as_ptr() as i32,
            key.len() as i32,
            value_buf.as_mut_ptr() as i32,
        )
    };
    if status < 0 {
        return Err(status);
    }
    value_buf.truncate(status as usize);
    Ok(value_buf)
}

pub fn host_storage_set(key: &[u8], value: &[u8]) -> Result<(), i32> {
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - key pointer and length are valid for reads
    // - value pointer and length are valid for reads
    let status = unsafe {
        storage_set(
            key.as_ptr() as i32,
            key.len() as i32,
            value.as_ptr() as i32,
            value.len() as i32,
        )
    };
    if status < 0 {
        return Err(status);
    }
    Ok(())
}

pub fn host_storage_get_cross(challenge_id: &[u8], key: &[u8]) -> Result<Vec<u8>, i32> {
    let mut value_buf = vec![0u8; RESPONSE_BUF_MEDIUM];
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - challenge_id and key pointers/lengths are valid for reads
    // - value_buf pointer is valid for writes
    // - The host writes at most value_buf.len() bytes
    let status = unsafe {
        storage_get_cross(
            challenge_id.as_ptr() as i32,
            challenge_id.len() as i32,
            key.as_ptr() as i32,
            key.len() as i32,
            value_buf.as_mut_ptr() as i32,
        )
    };
    if status < 0 {
        return Err(status);
    }
    value_buf.truncate(status as usize);
    Ok(value_buf)
}

/// List all key-value pairs matching a prefix. Returns bincode-encoded Vec<(Vec<u8>, Vec<u8>)>.
pub fn host_storage_list_prefix(prefix: &[u8], limit: i32) -> Result<Vec<u8>, i32> {
    let mut result_buf = vec![0u8; RESPONSE_BUF_LARGE];
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - prefix pointer and length are valid for reads
    // - result_buf pointer and length are valid for writes
    // - The host writes at most result_buf.len() bytes
    let status = unsafe {
        storage_list_prefix(
            prefix.as_ptr() as i32,
            prefix.len() as i32,
            result_buf.as_mut_ptr() as i32,
            limit,
        )
    };
    if status < 0 {
        return Err(status);
    }
    result_buf.truncate(status as usize);
    Ok(result_buf)
}

/// Count keys matching a prefix.
pub fn host_storage_count_prefix(prefix: &[u8]) -> Result<u64, i32> {
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - prefix pointer and length are valid for reads
    // - Returns a valid i64 count value
    let count = unsafe { storage_count_prefix(prefix.as_ptr() as i32, prefix.len() as i32) };
    if count < 0 {
        return Err(count as i32);
    }
    Ok(count as u64)
}

pub fn host_terminal_exec(request: &[u8]) -> Result<Vec<u8>, i32> {
    let mut result_buf = vec![0u8; RESPONSE_BUF_LARGE];
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - request pointer and length are valid for reads
    // - result_buf pointer and length are valid for writes
    // - The host writes at most result_buf.len() bytes
    let status = unsafe {
        terminal_exec(
            request.as_ptr() as i32,
            request.len() as i32,
            result_buf.as_mut_ptr() as i32,
            result_buf.len() as i32,
        )
    };
    if status < 0 {
        return Err(status);
    }
    result_buf.truncate(status as usize);
    Ok(result_buf)
}

pub fn host_read_file(path: &[u8]) -> Result<Vec<u8>, i32> {
    let mut buf = vec![0u8; RESPONSE_BUF_LARGE];
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - path pointer and length are valid for reads
    // - buf pointer and length are valid for writes
    // - The host writes at most buf.len() bytes
    let status = unsafe {
        terminal_read_file(
            path.as_ptr() as i32,
            path.len() as i32,
            buf.as_mut_ptr() as i32,
            buf.len() as i32,
        )
    };
    if status < 0 {
        return Err(status);
    }
    buf.truncate(status as usize);
    Ok(buf)
}

pub fn host_write_file(path: &[u8], data: &[u8]) -> Result<(), i32> {
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - path pointer and length are valid for reads
    // - data pointer and length are valid for reads
    let status = unsafe {
        terminal_write_file(
            path.as_ptr() as i32,
            path.len() as i32,
            data.as_ptr() as i32,
            data.len() as i32,
        )
    };
    if status < 0 {
        return Err(status);
    }
    Ok(())
}

pub fn host_list_dir(path: &[u8]) -> Result<Vec<u8>, i32> {
    let mut buf = vec![0u8; RESPONSE_BUF_MEDIUM];
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - path pointer and length are valid for reads
    // - buf pointer and length are valid for writes
    // - The host writes at most buf.len() bytes
    let status = unsafe {
        terminal_list_dir(
            path.as_ptr() as i32,
            path.len() as i32,
            buf.as_mut_ptr() as i32,
            buf.len() as i32,
        )
    };
    if status < 0 {
        return Err(status);
    }
    buf.truncate(status as usize);
    Ok(buf)
}

pub fn host_get_time() -> i64 {
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - terminal_get_time returns a valid i64 timestamp
    unsafe { terminal_get_time() }
}

pub fn host_random_seed(buf: &mut [u8]) -> Result<(), i32> {
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - buf pointer and length are valid for writes
    // - The host writes exactly buf.len() random bytes
    let status = unsafe { terminal_random_seed(buf.as_mut_ptr() as i32, buf.len() as i32) };
    if status < 0 {
        return Err(status);
    }
    Ok(())
}

#[link(wasm_import_module = "platform_sandbox")]
extern "C" {
    fn sandbox_exec(req_ptr: i32, req_len: i32, resp_ptr: i32, resp_len: i32) -> i32;
    fn get_timestamp() -> i64;
    fn log_message(level: i32, msg_ptr: i32, msg_len: i32);
    fn env_get(key_ptr: i32, key_len: i32, val_ptr: i32, val_len: i32) -> i32;
}

pub fn host_sandbox_exec(request: &[u8]) -> Result<Vec<u8>, i32> {
    let mut response_buf = vec![0u8; RESPONSE_BUF_LARGE];
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - request pointer and length are valid for reads
    // - response_buf pointer and length are valid for writes
    // - The host writes at most response_buf.len() bytes
    let status = unsafe {
        sandbox_exec(
            request.as_ptr() as i32,
            request.len() as i32,
            response_buf.as_mut_ptr() as i32,
            response_buf.len() as i32,
        )
    };
    if status < 0 {
        return Err(status);
    }
    response_buf.truncate(status as usize);
    Ok(response_buf)
}

pub fn host_get_timestamp() -> i64 {
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - get_timestamp returns a valid i64 timestamp
    unsafe { get_timestamp() }
}

pub fn host_log(level: u8, msg: &str) {
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - msg pointer and length are valid for reads
    // - The function does not modify WASM memory
    unsafe { log_message(level as i32, msg.as_ptr() as i32, msg.len() as i32) }
}

pub fn host_env_get(key: &str) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; 4096];
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - key pointer and length are valid for reads
    // - buf pointer and length are valid for writes
    // - The host writes at most buf.len() bytes
    let status = unsafe {
        env_get(
            key.as_ptr() as i32,
            key.len() as i32,
            buf.as_mut_ptr() as i32,
            buf.len() as i32,
        )
    };
    if status <= 0 {
        return None;
    }
    buf.truncate(status as usize);
    Some(buf)
}

#[link(wasm_import_module = "platform_llm")]
extern "C" {
    fn llm_chat_completion(req_ptr: i32, req_len: i32, resp_ptr: i32, resp_len: i32) -> i32;
    fn llm_is_available() -> i32;
}

pub fn host_llm_chat_completion(request: &[u8]) -> Result<Vec<u8>, i32> {
    let mut response_buf = vec![0u8; RESPONSE_BUF_LARGE];
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - request pointer and length are valid for reads
    // - response_buf pointer and length are valid for writes
    // - The host writes at most response_buf.len() bytes
    let status = unsafe {
        llm_chat_completion(
            request.as_ptr() as i32,
            request.len() as i32,
            response_buf.as_mut_ptr() as i32,
            response_buf.len() as i32,
        )
    };
    if status < 0 {
        return Err(status);
    }
    response_buf.truncate(status as usize);
    Ok(response_buf)
}

pub fn host_llm_is_available() -> bool {
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - llm_is_available returns a valid i32 (0 or 1)
    unsafe { llm_is_available() == 1 }
}

#[link(wasm_import_module = "platform_consensus")]
extern "C" {
    fn consensus_get_epoch() -> i64;
    fn consensus_get_validators(buf_ptr: i32, buf_len: i32) -> i32;
    fn consensus_propose_weight(uid: i32, weight: i32) -> i32;
    fn consensus_get_votes(buf_ptr: i32, buf_len: i32) -> i32;
    fn consensus_get_state_hash(buf_ptr: i32) -> i32;
    fn consensus_get_submission_count() -> i32;
    fn consensus_get_block_height() -> i64;
    fn consensus_get_subnet_challenges(buf_ptr: i32, buf_len: i32) -> i32;
    fn consensus_get_llm_validators(buf_ptr: i32, buf_len: i32) -> i32;
    fn consensus_get_registered_hotkeys(buf_ptr: i32, buf_len: i32) -> i32;
}

pub fn host_consensus_get_epoch() -> i64 {
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - consensus_get_epoch returns a valid i64 epoch number
    unsafe { consensus_get_epoch() }
}

pub fn host_consensus_get_validators() -> Result<Vec<u8>, i32> {
    let mut buf = vec![0u8; RESPONSE_BUF_MEDIUM];
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - buf pointer and length are valid for writes
    // - The host writes at most buf.len() bytes
    let status = unsafe { consensus_get_validators(buf.as_mut_ptr() as i32, buf.len() as i32) };
    if status < 0 {
        return Err(status);
    }
    buf.truncate(status as usize);
    Ok(buf)
}

pub fn host_consensus_propose_weight(uid: i32, weight: i32) -> Result<(), i32> {
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - The function accepts valid i32 uid and weight values
    let status = unsafe { consensus_propose_weight(uid, weight) };
    if status < 0 {
        return Err(status);
    }
    Ok(())
}

pub fn host_consensus_get_votes() -> Result<Vec<u8>, i32> {
    let mut buf = vec![0u8; RESPONSE_BUF_MEDIUM];
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - buf pointer and length are valid for writes
    // - The host writes at most buf.len() bytes
    let status = unsafe { consensus_get_votes(buf.as_mut_ptr() as i32, buf.len() as i32) };
    if status < 0 {
        return Err(status);
    }
    buf.truncate(status as usize);
    Ok(buf)
}

pub fn host_consensus_get_state_hash() -> Result<[u8; 32], i32> {
    let mut buf = [0u8; 32];
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - buf pointer is valid for writes of exactly 32 bytes
    let status = unsafe { consensus_get_state_hash(buf.as_mut_ptr() as i32) };
    if status < 0 {
        return Err(status);
    }
    Ok(buf)
}

pub fn host_consensus_get_submission_count() -> i32 {
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - consensus_get_submission_count returns a valid i32 count
    unsafe { consensus_get_submission_count() }
}

pub fn host_consensus_get_block_height() -> i64 {
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - consensus_get_block_height returns a valid i64 block height
    unsafe { consensus_get_block_height() }
}

pub fn host_consensus_get_subnet_challenges() -> Result<Vec<u8>, i32> {
    let mut buf = vec![0u8; RESPONSE_BUF_MEDIUM];
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - buf pointer and length are valid for writes
    // - The host writes at most buf.len() bytes
    let status =
        unsafe { consensus_get_subnet_challenges(buf.as_mut_ptr() as i32, buf.len() as i32) };
    if status < 0 {
        return Err(status);
    }
    buf.truncate(status as usize);
    Ok(buf)
}

/// Get the list of validators that have LLM capability (Chutes API key).
/// Returns JSON-encoded list of hotkey strings.
pub fn host_consensus_get_llm_validators() -> Result<Vec<u8>, i32> {
    let mut buf = vec![0u8; RESPONSE_BUF_MEDIUM];
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - buf pointer and length are valid for writes
    // - The host writes at most buf.len() bytes
    let status = unsafe { consensus_get_llm_validators(buf.as_mut_ptr() as i32, buf.len() as i32) };
    if status < 0 {
        return Err(status);
    }
    buf.truncate(status as usize);
    Ok(buf)
}

/// Get the list of all registered hotkeys from the subnet metagraph.
/// Returns JSON-encoded list of hotkey strings.
pub fn host_consensus_get_registered_hotkeys() -> Result<Vec<u8>, i32> {
    let mut buf = vec![0u8; RESPONSE_BUF_MEDIUM];
    // SAFETY: FFI call to the WASM host. The host guarantees:
    // - buf pointer and length are valid for writes
    // - The host writes at most buf.len() bytes
    let status =
        unsafe { consensus_get_registered_hotkeys(buf.as_mut_ptr() as i32, buf.len() as i32) };
    if status < 0 {
        return Err(status);
    }
    buf.truncate(status as usize);
    Ok(buf)
}

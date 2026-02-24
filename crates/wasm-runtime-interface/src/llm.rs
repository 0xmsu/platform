//! LLM Host Functions for WASM Challenges
//!
//! Acts as a pure proxy: the challenge WASM builds the complete OpenAI-compatible
//! request. The host adds authentication, forces `stream: false`, forwards to the
//! LLM endpoint, and returns the full response.
//!
//! # Host Functions
//!
//! - `llm_chat_completion(req_ptr, req_len, resp_ptr, resp_len) -> i32` — Proxy chat completion
//! - `llm_is_available() -> i32` — Check if LLM proxy is available (has API key)

use crate::runtime::{HostFunctionRegistrar, RuntimeState, WasmRuntimeError};
use serde::{Deserialize, Serialize};
use std::fmt;
use tracing::warn;
use wasmtime::{Caller, Linker, Memory};

const MAX_CHAT_REQUEST_SIZE: u64 = 4 * 1024 * 1024;
const LLM_REQUEST_TIMEOUT_SECS: u64 = 120;

pub const HOST_LLM_NAMESPACE: &str = "platform_llm";
pub const HOST_LLM_CHAT_COMPLETION: &str = "llm_chat_completion";
pub const HOST_LLM_IS_AVAILABLE: &str = "llm_is_available";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum LlmHostStatus {
    Success = 0,
    Disabled = -1,
    InvalidRequest = -2,
    ApiError = -3,
    BufferTooSmall = -4,
    RateLimited = -5,
    InternalError = -100,
}

impl LlmHostStatus {
    pub fn to_i32(self) -> i32 {
        self as i32
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct LlmPolicy {
    pub enabled: bool,
    #[serde(skip)]
    pub api_key: Option<String>,
    pub endpoint: String,
    pub max_requests: u32,
    pub allowed_models: Vec<String>,
}

impl fmt::Debug for LlmPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LlmPolicy")
            .field("enabled", &self.enabled)
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .field("endpoint", &self.endpoint)
            .field("max_requests", &self.max_requests)
            .field("allowed_models", &self.allowed_models)
            .finish()
    }
}

impl Default for LlmPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: None,
            endpoint: "https://llm.chutes.ai/v1/chat/completions".to_string(),
            max_requests: 10,
            allowed_models: Vec::new(),
        }
    }
}

impl LlmPolicy {
    pub fn with_api_key(api_key: String) -> Self {
        Self {
            enabled: true,
            api_key: Some(api_key),
            ..Default::default()
        }
    }

    pub fn is_available(&self) -> bool {
        self.enabled && self.api_key.is_some()
    }
}

pub struct LlmState {
    pub policy: LlmPolicy,
    pub requests_made: u32,
}

impl LlmState {
    pub fn new(policy: LlmPolicy) -> Self {
        Self {
            policy,
            requests_made: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LlmHostFunctions;

impl LlmHostFunctions {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LlmHostFunctions {
    fn default() -> Self {
        Self::new()
    }
}

impl HostFunctionRegistrar for LlmHostFunctions {
    fn register(&self, linker: &mut Linker<RuntimeState>) -> Result<(), WasmRuntimeError> {
        linker
            .func_wrap(
                HOST_LLM_NAMESPACE,
                HOST_LLM_CHAT_COMPLETION,
                |mut caller: Caller<RuntimeState>,
                 req_ptr: i32,
                 req_len: i32,
                 resp_ptr: i32,
                 resp_len: i32|
                 -> i32 {
                    handle_chat_completion(&mut caller, req_ptr, req_len, resp_ptr, resp_len)
                },
            )
            .map_err(|err| WasmRuntimeError::HostFunction(err.to_string()))?;

        linker
            .func_wrap(
                HOST_LLM_NAMESPACE,
                HOST_LLM_IS_AVAILABLE,
                |caller: Caller<RuntimeState>| -> i32 { handle_is_available(&caller) },
            )
            .map_err(|err| WasmRuntimeError::HostFunction(err.to_string()))?;

        Ok(())
    }
}

fn handle_is_available(caller: &Caller<RuntimeState>) -> i32 {
    let state = &caller.data().llm_state;
    if state.policy.is_available() {
        1
    } else {
        0
    }
}

/// Pure proxy: deserialize SDK request (bincode), convert to OpenAI JSON,
/// force stream:false, add auth, forward, parse response, return bincode.
fn handle_chat_completion(
    caller: &mut Caller<RuntimeState>,
    req_ptr: i32,
    req_len: i32,
    resp_ptr: i32,
    resp_len: i32,
) -> i32 {
    let policy_available;
    let requests_made;
    let max_requests;
    {
        let state = &caller.data().llm_state;
        policy_available = state.policy.is_available();
        requests_made = state.requests_made;
        max_requests = state.policy.max_requests;
    }

    if !policy_available {
        return LlmHostStatus::Disabled.to_i32();
    }

    if requests_made >= max_requests {
        return LlmHostStatus::RateLimited.to_i32();
    }

    if req_ptr < 0 || req_len < 0 || resp_ptr < 0 || resp_len < 0 {
        return LlmHostStatus::InvalidRequest.to_i32();
    }

    let request_bytes = match read_wasm_memory(caller, req_ptr, req_len as usize) {
        Ok(b) => b,
        Err(err) => {
            warn!(error = %err, "llm proxy: failed to read request from wasm memory");
            return LlmHostStatus::InternalError.to_i32();
        }
    };

    if request_bytes.len() as u64 > MAX_CHAT_REQUEST_SIZE {
        return LlmHostStatus::InvalidRequest.to_i32();
    }

    let api_key;
    let endpoint;
    {
        let state = &caller.data().llm_state;
        api_key = match &state.policy.api_key {
            Some(k) => k.clone(),
            None => return LlmHostStatus::Disabled.to_i32(),
        };
        endpoint = state.policy.endpoint.clone();
    }

    // Deserialize the SDK request (bincode-encoded LlmRequest from challenge-sdk-wasm)
    let sdk_req: SdkRequest = match bincode::deserialize(&request_bytes) {
        Ok(r) => r,
        Err(_) => return LlmHostStatus::InvalidRequest.to_i32(),
    };

    // Validate model against allowed list
    {
        let state = &caller.data().llm_state;
        let allowed = &state.policy.allowed_models;
        if !allowed.is_empty() && !allowed.contains(&sdk_req.model) {
            warn!(model = %sdk_req.model, "llm proxy: model not in allowed list");
            return LlmHostStatus::InvalidRequest.to_i32();
        }
    }

    // Build OpenAI-compatible JSON, force stream: false
    let openai_json = build_openai_request(&sdk_req);
    let json_body = match serde_json::to_vec(&openai_json) {
        Ok(b) => b,
        Err(_) => return LlmHostStatus::InvalidRequest.to_i32(),
    };

    // Forward to LLM endpoint
    let client = reqwest::blocking::Client::new();
    let http_response = match client
        .post(&endpoint)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", api_key))
        .body(json_body)
        .timeout(std::time::Duration::from_secs(LLM_REQUEST_TIMEOUT_SECS))
        .send()
    {
        Ok(r) => r,
        Err(err) => {
            warn!(error = %err, "llm proxy: HTTP request failed");
            return LlmHostStatus::ApiError.to_i32();
        }
    };

    let response_body = match http_response.bytes() {
        Ok(b) => b.to_vec(),
        Err(err) => {
            warn!(error = %err, "llm proxy: failed to read response body");
            return LlmHostStatus::ApiError.to_i32();
        }
    };

    // Parse OpenAI response and convert to SDK response (bincode)
    let sdk_response = match parse_openai_response(&response_body) {
        Ok(r) => r,
        Err(err) => {
            warn!(error = %err, "llm proxy: failed to parse response");
            return LlmHostStatus::ApiError.to_i32();
        }
    };

    let response_bytes = match bincode::serialize(&sdk_response) {
        Ok(b) => b,
        Err(_) => return LlmHostStatus::InternalError.to_i32(),
    };

    if response_bytes.len() > resp_len as usize {
        return LlmHostStatus::BufferTooSmall.to_i32();
    }

    if let Err(err) = write_wasm_memory(caller, resp_ptr, &response_bytes) {
        warn!(error = %err, "llm proxy: failed to write response to wasm memory");
        return LlmHostStatus::InternalError.to_i32();
    }

    caller.data_mut().llm_state.requests_made += 1;

    response_bytes.len() as i32
}

// --- SDK types (mirroring challenge-sdk-wasm/src/llm_types.rs) ---

#[derive(Deserialize)]
struct SdkRequest {
    model: String,
    messages: Vec<SdkMessage>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    top_p: Option<f32>,
    frequency_penalty: Option<f32>,
    presence_penalty: Option<f32>,
    stop: Option<Vec<String>>,
    tools: Option<Vec<SdkTool>>,
    tool_choice: Option<SdkToolChoice>,
    response_format: Option<SdkResponseFormat>,
}

#[derive(Deserialize)]
struct SdkMessage {
    role: String,
    content: Option<String>,
    name: Option<String>,
    tool_calls: Option<Vec<SdkToolCall>>,
    tool_call_id: Option<String>,
}

#[derive(Deserialize)]
struct SdkTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: SdkFunctionDef,
}

#[derive(Deserialize)]
struct SdkFunctionDef {
    name: String,
    description: Option<String>,
    parameters: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SdkToolChoice {
    Auto,
    None,
    Required,
    Specific { function: SdkToolChoiceFunction },
}

#[derive(Deserialize)]
struct SdkToolChoiceFunction {
    name: String,
}

#[derive(Deserialize)]
struct SdkResponseFormat {
    #[serde(rename = "type")]
    format_type: String,
}

#[derive(Deserialize)]
struct SdkToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: SdkFunctionCall,
}

#[derive(Deserialize)]
struct SdkFunctionCall {
    name: String,
    arguments: String,
}

// --- OpenAI request/response types ---

#[derive(Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    stream: bool, // always false
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frequency_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    presence_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct OpenAiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

fn build_openai_request(sdk: &SdkRequest) -> OpenAiRequest {
    let messages = sdk
        .messages
        .iter()
        .map(|m| {
            let tool_calls = m.tool_calls.as_ref().map(|tcs| {
                tcs.iter()
                    .map(|tc| {
                        serde_json::json!({
                            "id": tc.id,
                            "type": tc.call_type,
                            "function": {
                                "name": tc.function.name,
                                "arguments": tc.function.arguments,
                            }
                        })
                    })
                    .collect()
            });
            OpenAiMessage {
                role: m.role.clone(),
                content: m.content.clone(),
                name: m.name.clone(),
                tool_calls,
                tool_call_id: m.tool_call_id.clone(),
            }
        })
        .collect();

    let tools = sdk.tools.as_ref().map(|ts| {
        ts.iter()
            .map(|t| {
                let params: serde_json::Value = t
                    .function
                    .parameters
                    .as_ref()
                    .and_then(|p| serde_json::from_str(p).ok())
                    .unwrap_or(serde_json::json!({}));
                serde_json::json!({
                    "type": t.tool_type,
                    "function": {
                        "name": t.function.name,
                        "description": t.function.description,
                        "parameters": params,
                    }
                })
            })
            .collect()
    });

    let tool_choice = sdk.tool_choice.as_ref().map(|tc| match tc {
        SdkToolChoice::Auto => serde_json::json!("auto"),
        SdkToolChoice::None => serde_json::json!("none"),
        SdkToolChoice::Required => serde_json::json!("required"),
        SdkToolChoice::Specific { function } => {
            serde_json::json!({"type": "function", "function": {"name": function.name}})
        }
    });

    let response_format = sdk
        .response_format
        .as_ref()
        .map(|rf| serde_json::json!({"type": rf.format_type}));

    OpenAiRequest {
        model: sdk.model.clone(),
        messages,
        stream: false, // always non-streaming for WASM
        max_tokens: sdk.max_tokens,
        temperature: sdk.temperature,
        top_p: sdk.top_p,
        frequency_penalty: sdk.frequency_penalty,
        presence_penalty: sdk.presence_penalty,
        stop: sdk.stop.clone(),
        tools,
        tool_choice,
        response_format,
    }
}

// --- OpenAI response parsing ---

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Option<Vec<OpenAiChoice>>,
    usage: Option<OpenAiUsage>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: Option<OpenAiRespMessage>,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiRespMessage {
    content: Option<String>,
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Deserialize)]
struct OpenAiToolCall {
    id: Option<String>,
    #[serde(rename = "type")]
    call_type: Option<String>,
    function: Option<OpenAiFunctionCall>,
}

#[derive(Deserialize)]
struct OpenAiFunctionCall {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiUsage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    total_tokens: Option<u32>,
}

#[derive(Serialize)]
struct SdkResponse {
    content: Option<String>,
    tool_calls: Vec<SdkResponseToolCall>,
    usage: Option<SdkUsage>,
    finish_reason: Option<String>,
}

#[derive(Serialize)]
struct SdkResponseToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: SdkResponseFunctionCall,
}

#[derive(Serialize)]
struct SdkResponseFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Serialize)]
struct SdkUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

fn parse_openai_response(body: &[u8]) -> Result<SdkResponse, String> {
    let resp: OpenAiResponse =
        serde_json::from_slice(body).map_err(|e| format!("JSON parse error: {e}"))?;

    let choice = resp.choices.and_then(|mut c| {
        if c.is_empty() {
            None
        } else {
            Some(c.remove(0))
        }
    });

    let (content, tool_calls_raw, finish_reason) = match choice {
        Some(c) => {
            let fr = c.finish_reason;
            match c.message {
                Some(msg) => (msg.content, msg.tool_calls.unwrap_or_default(), fr),
                None => (None, Vec::new(), fr),
            }
        }
        None => (None, Vec::new(), None),
    };

    let tool_calls = tool_calls_raw
        .into_iter()
        .filter_map(|tc| {
            let func = tc.function?;
            Some(SdkResponseToolCall {
                id: tc.id.unwrap_or_default(),
                call_type: tc.call_type.unwrap_or_else(|| "function".to_string()),
                function: SdkResponseFunctionCall {
                    name: func.name.unwrap_or_default(),
                    arguments: func.arguments.unwrap_or_default(),
                },
            })
        })
        .collect();

    let usage = resp.usage.map(|u| SdkUsage {
        prompt_tokens: u.prompt_tokens.unwrap_or(0),
        completion_tokens: u.completion_tokens.unwrap_or(0),
        total_tokens: u.total_tokens.unwrap_or(0),
    });

    Ok(SdkResponse {
        content,
        tool_calls,
        usage,
        finish_reason,
    })
}

fn read_wasm_memory(
    caller: &mut Caller<RuntimeState>,
    ptr: i32,
    len: usize,
) -> Result<Vec<u8>, String> {
    if ptr < 0 {
        return Err("negative pointer".to_string());
    }
    let ptr = ptr as usize;
    let memory = get_memory(caller).ok_or_else(|| "memory export not found".to_string())?;
    let end = ptr
        .checked_add(len)
        .ok_or_else(|| "pointer overflow".to_string())?;
    let data = memory.data(caller);
    if end > data.len() {
        return Err("memory read out of bounds".to_string());
    }
    Ok(data[ptr..end].to_vec())
}

fn write_wasm_memory(
    caller: &mut Caller<RuntimeState>,
    ptr: i32,
    bytes: &[u8],
) -> Result<(), String> {
    if ptr < 0 {
        return Err("negative pointer".to_string());
    }
    let ptr = ptr as usize;
    let memory = get_memory(caller).ok_or_else(|| "memory export not found".to_string())?;
    let end = ptr
        .checked_add(bytes.len())
        .ok_or_else(|| "pointer overflow".to_string())?;
    let data = memory.data_mut(caller);
    if end > data.len() {
        return Err("memory write out of bounds".to_string());
    }
    data[ptr..end].copy_from_slice(bytes);
    Ok(())
}

fn get_memory(caller: &mut Caller<RuntimeState>) -> Option<Memory> {
    let memory_export = caller.data().memory_export.clone();
    caller
        .get_export(&memory_export)
        .and_then(|export| export.into_memory())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_host_status_values() {
        assert_eq!(LlmHostStatus::Success.to_i32(), 0);
        assert_eq!(LlmHostStatus::Disabled.to_i32(), -1);
        assert_eq!(LlmHostStatus::InvalidRequest.to_i32(), -2);
        assert_eq!(LlmHostStatus::ApiError.to_i32(), -3);
        assert_eq!(LlmHostStatus::BufferTooSmall.to_i32(), -4);
        assert_eq!(LlmHostStatus::RateLimited.to_i32(), -5);
        assert_eq!(LlmHostStatus::InternalError.to_i32(), -100);
    }

    #[test]
    fn test_llm_policy_default() {
        let policy = LlmPolicy::default();
        assert!(!policy.enabled);
        assert!(policy.api_key.is_none());
        assert!(!policy.is_available());
    }

    #[test]
    fn test_llm_policy_with_api_key() {
        let policy = LlmPolicy::with_api_key("test-key".to_string());
        assert!(policy.enabled);
        assert!(policy.is_available());
        assert_eq!(policy.api_key, Some("test-key".to_string()));
    }

    #[test]
    fn test_llm_state_creation() {
        let state = LlmState::new(LlmPolicy::default());
        assert_eq!(state.requests_made, 0);
        assert!(!state.policy.is_available());
    }

    #[test]
    fn test_llm_policy_debug_redacts_api_key() {
        let policy = LlmPolicy::with_api_key("super-secret-key-12345".to_string());
        let debug_output = format!("{:?}", policy);
        assert!(!debug_output.contains("super-secret-key-12345"));
        assert!(debug_output.contains("[REDACTED]"));
    }

    #[test]
    fn test_llm_policy_serialize_skips_api_key() {
        let policy = LlmPolicy::with_api_key("secret-key".to_string());
        let serialized = bincode::serialize(&policy).unwrap();
        let deserialized: LlmPolicy = bincode::deserialize(&serialized).unwrap();
        assert!(deserialized.api_key.is_none());
    }

    #[test]
    fn test_parse_openai_response_with_tool_calls() {
        let response_json = serde_json::json!({
            "id": "chatcmpl-123",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc123",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"location\": \"Paris\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 50,
                "completion_tokens": 20,
                "total_tokens": 70
            }
        });

        let body = serde_json::to_vec(&response_json).unwrap();
        let sdk_resp = parse_openai_response(&body).unwrap();

        assert!(sdk_resp.content.is_none());
        assert_eq!(sdk_resp.tool_calls.len(), 1);
        assert_eq!(sdk_resp.tool_calls[0].id, "call_abc123");
        assert_eq!(sdk_resp.tool_calls[0].function.name, "get_weather");
        assert_eq!(sdk_resp.finish_reason, Some("tool_calls".to_string()));
        assert_eq!(sdk_resp.usage.unwrap().total_tokens, 70);
    }

    #[test]
    fn test_parse_openai_response_text_only() {
        let response_json = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Hello, world!"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });

        let body = serde_json::to_vec(&response_json).unwrap();
        let sdk_resp = parse_openai_response(&body).unwrap();

        assert_eq!(sdk_resp.content, Some("Hello, world!".to_string()));
        assert!(sdk_resp.tool_calls.is_empty());
        assert_eq!(sdk_resp.finish_reason, Some("stop".to_string()));
    }
}

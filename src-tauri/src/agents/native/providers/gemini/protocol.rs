//! Pure request building and response mapping for the Gemini API.
//!
//! Nothing here performs I/O, so every branch — a safety block, a malformed
//! chunk, an oversized stream, a 429 — is reachable from a fixture.

use crate::agents::native::{
    AlfredToolKind, AlfredToolRequest, AlfredToolStatus, NativeContextRole, NativeErrorCode,
    NativeEventLimits, NativeRuntimeError, NativeToolCapabilitySet, NativeTurnRequest,
};
use serde_json::{json, Map, Value};
use std::path::PathBuf;

/// Upper bound on `data:` lines accepted from one stream.
pub const MAX_STREAM_CHUNKS: usize = 4_096;
/// Upper bound on the bytes of one `data:` payload.
pub const MAX_CHUNK_BYTES: usize = 256 * 1024;
/// Upper bound on function-call rounds inside one turn.
pub const MAX_TOOL_ROUNDS: usize = 8;
/// Upper bound on the JSON arguments Gemini may attach to one function call.
pub const MAX_FUNCTION_ARGS_BYTES: usize = 32 * 1024;
/// Upper bound on calls accepted from one model response.
pub const MAX_FUNCTION_CALLS_PER_ROUND: usize = 16;
/// The SSE decoder never buffers more than one bounded provider event.
pub const MAX_SSE_FRAME_BYTES: usize = MAX_CHUNK_BYTES + 4 * 1024;
/// Alfred's per-tool limits remain tighter than the provider request timeout.
pub const TOOL_TIMEOUT_MS: u64 = 120_000;
pub const TOOL_OUTPUT_BYTES: usize = 128 * 1024;

/// The Alfred-owned tools exposed to Gemini as function declarations.
///
/// Gemini calls Alfred's tools; Alfred never exposes a provider-native
/// filesystem or shell, and never forwards a header or secret out of workflow
/// JSON. Only capabilities the turn actually granted are declared.
const TOOL_DECLARATIONS: [(&str, AlfredToolKind, &str); 6] = [
    (
        "alfred_read_file",
        AlfredToolKind::FileRead,
        "Read a UTF-8 file inside the workspace.",
    ),
    (
        "alfred_list_directory",
        AlfredToolKind::DirectoryList,
        "List a directory inside the workspace.",
    ),
    (
        "alfred_write_file",
        AlfredToolKind::FileWrite,
        "Write a UTF-8 file inside the workspace.",
    ),
    (
        "alfred_edit_file",
        AlfredToolKind::FileEdit,
        "Replace a span of a file inside the workspace.",
    ),
    (
        "alfred_run_shell",
        AlfredToolKind::Shell,
        "Run one shell command inside the workspace.",
    ),
    (
        "alfred_apply_patch",
        AlfredToolKind::ApplyPatch,
        "Apply a unified diff inside the workspace.",
    ),
];

/// Resolves a declared function name back to its Alfred tool kind.
pub fn tool_kind_for(name: &str) -> Option<AlfredToolKind> {
    TOOL_DECLARATIONS
        .iter()
        .find(|(declared, _, _)| *declared == name)
        .map(|(_, kind, _)| *kind)
}

fn kind_granted(kind: AlfredToolKind, capabilities: &NativeToolCapabilitySet) -> bool {
    match kind {
        AlfredToolKind::FileRead
        | AlfredToolKind::FileWrite
        | AlfredToolKind::FileEdit
        | AlfredToolKind::DirectoryList => capabilities.filesystem,
        AlfredToolKind::Shell | AlfredToolKind::Process => capabilities.shell,
        AlfredToolKind::ApplyPatch => capabilities.patch,
        AlfredToolKind::Mcp => capabilities.mcp,
        AlfredToolKind::Subagent => capabilities.subagents,
        AlfredToolKind::MemoryLookup => capabilities.memory_lookup,
    }
}

fn declaration_schema(kind: AlfredToolKind, description: &str) -> Value {
    let (properties, required): (Value, Value) = match kind {
        AlfredToolKind::Shell => (
            json!({
                "path": { "type": "string", "description": "Workspace-relative working directory." },
                "command": { "type": "array", "items": { "type": "string" }, "description": "Argument vector." }
            }),
            json!(["path", "command"]),
        ),
        AlfredToolKind::FileWrite | AlfredToolKind::FileEdit | AlfredToolKind::ApplyPatch => (
            json!({
                "path": { "type": "string", "description": "Workspace-relative path." },
                "content": { "type": "string", "description": "New content or unified diff." }
            }),
            json!(["path", "content"]),
        ),
        _ => (
            json!({ "path": { "type": "string", "description": "Workspace-relative path." } }),
            json!(["path"]),
        ),
    };
    json!({
        "name": "",
        "description": description,
        "parameters": { "type": "object", "properties": properties, "required": required }
    })
}

/// Builds the `tools` array for the granted capabilities, or `None` when the
/// turn granted no tools at all.
pub fn build_tools(capabilities: &NativeToolCapabilitySet) -> Option<Value> {
    let declarations = TOOL_DECLARATIONS
        .iter()
        .filter(|(_, kind, _)| kind_granted(*kind, capabilities))
        .map(|(name, kind, description)| {
            let mut declaration = declaration_schema(*kind, description);
            declaration["name"] = json!(name);
            declaration
        })
        .collect::<Vec<_>>();
    if declarations.is_empty() {
        None
    } else {
        Some(json!([{ "functionDeclarations": declarations }]))
    }
}

/// One entry of the running conversation Alfred sends back each round.
#[derive(Debug, Clone, PartialEq)]
pub enum GeminiHistoryEntry {
    /// A model turn that asked for function calls.
    ModelFunctionCalls(Vec<GeminiFunctionCall>),
    /// Visible model text plus function calls from one response. Provider
    /// thought text is intentionally absent; opaque thought signatures remain
    /// attached to the function-call parts that require replay.
    ModelResponse {
        text_parts: Vec<String>,
        calls: Vec<GeminiFunctionCall>,
    },
    /// Alfred's results for those calls.
    ToolResults(Vec<GeminiFunctionResult>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeminiFunctionCall {
    /// Provider call id. Newer models populate this and require it echoed.
    pub id: Option<String>,
    pub name: String,
    pub args: Map<String, Value>,
    /// Opaque provider signature that must be replayed for Gemini thinking
    /// models. It is never emitted as reasoning or inspected by Alfred.
    pub thought_signature: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeminiFunctionResult {
    pub id: Option<String>,
    pub name: String,
    pub status: &'static str,
    pub output: String,
}

/// Maps Alfred's context blocks and the running history into a request body.
///
/// System and Skill blocks become `systemInstruction`; user and assistant
/// blocks become `contents`. Nothing else from the request reaches the wire —
/// in particular no workflow-supplied header, key, or URL.
pub fn build_generate_request(
    request: &NativeTurnRequest,
    history: &[GeminiHistoryEntry],
) -> Value {
    let mut system = Vec::new();
    let mut contents = Vec::new();

    for block in &request.context {
        match block.role {
            NativeContextRole::System | NativeContextRole::Skill => {
                system.push(json!({ "text": block.content }));
            }
            NativeContextRole::User => {
                contents.push(json!({ "role": "user", "parts": [{ "text": block.content }] }));
            }
            NativeContextRole::Assistant => {
                contents.push(json!({ "role": "model", "parts": [{ "text": block.content }] }));
            }
            // A tool block from an earlier harness turn is replayed as plain
            // user text: Alfred cannot reconstruct the provider's call ids.
            NativeContextRole::Tool => {
                contents.push(json!({
                    "role": "user",
                    "parts": [{ "text": block.content }]
                }));
            }
        }
    }

    for entry in history {
        match entry {
            GeminiHistoryEntry::ModelFunctionCalls(calls) => {
                let parts = calls.iter().map(function_call_part).collect::<Vec<_>>();
                contents.push(json!({ "role": "model", "parts": parts }));
            }
            GeminiHistoryEntry::ModelResponse { text_parts, calls } => {
                let mut parts = text_parts
                    .iter()
                    .map(|text| json!({ "text": text }))
                    .collect::<Vec<_>>();
                parts.extend(calls.iter().map(function_call_part));
                contents.push(json!({ "role": "model", "parts": parts }));
            }
            GeminiHistoryEntry::ToolResults(results) => {
                let parts = results.iter().map(function_result_part).collect::<Vec<_>>();
                contents.push(json!({ "role": "user", "parts": parts }));
            }
        }
    }

    let mut body = json!({ "contents": contents });
    if !system.is_empty() {
        body["systemInstruction"] = json!({ "parts": system });
    }
    if let Some(tools) = build_tools(&request.tool_capabilities) {
        body["tools"] = tools;
        // AUTO lets the model answer without a tool. ANY would force a call and
        // turn a plain question into a permission prompt.
        body["toolConfig"] = json!({ "functionCallingConfig": { "mode": "AUTO" } });
    }
    body
}

fn function_call_part(call: &GeminiFunctionCall) -> Value {
    let mut function = json!({ "name": call.name, "args": call.args });
    if let Some(id) = call.id.as_deref() {
        function["id"] = json!(id);
    }
    let mut part = json!({ "functionCall": function });
    if let Some(signature) = call.thought_signature.as_deref() {
        part["thoughtSignature"] = json!(signature);
    }
    part
}

fn function_result_part(result: &GeminiFunctionResult) -> Value {
    let mut function = json!({
        "name": result.name,
        "response": { "status": result.status, "output": result.output }
    });
    if let Some(id) = result.id.as_deref() {
        function["id"] = json!(id);
    }
    json!({ "functionResponse": function })
}

/// A decoded piece of one streamed chunk.
#[derive(Debug, Clone, PartialEq)]
pub enum GeminiChunkEvent {
    Text(String),
    FunctionCall(GeminiFunctionCall),
    /// The provider refused. Never a successful empty turn.
    Blocked {
        reason: String,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
    },
    Finished {
        reason: String,
    },
}

/// Decodes one SSE `data:` payload.
///
/// Returns the events it carried, in order. An unparsable payload, an
/// oversized payload, or a payload carrying an API-level `error` object is an
/// error, not an empty result.
pub fn parse_stream_chunk(
    payload: &str,
    limits: &NativeEventLimits,
) -> Result<Vec<GeminiChunkEvent>, NativeRuntimeError> {
    if payload.len() > MAX_CHUNK_BYTES {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::EventLimitExceeded,
            "gemini stream chunk exceeded its byte limit",
            false,
        ));
    }
    let value: Value = serde_json::from_str(payload).map_err(|_| malformed())?;
    let object = value.as_object().ok_or_else(malformed)?;

    // An error object can arrive mid-stream after a 200 response line.
    if let Some(error) = object.get("error") {
        let status = error
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("UNKNOWN");
        let code = error.get("code").and_then(Value::as_u64).unwrap_or(0);
        return Err(error_for_status(u16::try_from(code).unwrap_or(0), status));
    }

    let mut events = Vec::new();

    if let Some(reason) = object
        .get("promptFeedback")
        .and_then(|feedback| feedback.get("blockReason"))
        .and_then(Value::as_str)
    {
        if reason != "BLOCK_REASON_UNSPECIFIED" {
            events.push(GeminiChunkEvent::Blocked {
                reason: sanitize_enum(reason),
            });
            return Ok(events);
        }
    }

    if let Some(usage) = object.get("usageMetadata").and_then(Value::as_object) {
        events.push(GeminiChunkEvent::Usage {
            input_tokens: usage
                .get("promptTokenCount")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            output_tokens: usage
                .get("candidatesTokenCount")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        });
    }

    let candidates = match object.get("candidates") {
        None | Some(Value::Null) => return Ok(events),
        Some(Value::Array(values)) => values,
        Some(_) => return Err(malformed()),
    };

    // Only the first candidate is consumed: Alfred requests one and a second
    // would silently interleave two answers.
    let Some(candidate) = candidates.first() else {
        return Ok(events);
    };

    // A candidate-level safety/filter finish may still carry withheld or
    // partial parts. Refuse the candidate before emitting any of that text.
    let finish_reason = candidate.get("finishReason").and_then(Value::as_str);
    if let Some(reason) = finish_reason.filter(|reason| is_blocking_finish(reason)) {
        events.push(GeminiChunkEvent::Blocked {
            reason: sanitize_enum(reason),
        });
        return Ok(events);
    }

    if let Some(parts) = candidate
        .get("content")
        .and_then(|content| content.get("parts"))
    {
        let parts = parts.as_array().ok_or_else(malformed)?;
        for part in parts {
            let part = part.as_object().ok_or_else(malformed)?;
            if let Some(text) = part.get("text") {
                let text = text.as_str().ok_or_else(malformed)?;
                if text.len() > limits.max_text_bytes {
                    return Err(NativeRuntimeError::new(
                        NativeErrorCode::EventLimitExceeded,
                        "gemini text part exceeded the native turn text limit",
                        false,
                    ));
                }
                if !text.is_empty() {
                    events.push(GeminiChunkEvent::Text(text.to_owned()));
                }
            }
            if let Some(call) = part.get("functionCall") {
                let mut call = parse_function_call(call)?;
                if let Some(signature) = part.get("thoughtSignature") {
                    let signature = signature.as_str().ok_or_else(malformed)?;
                    if signature.is_empty() || signature.len() > MAX_FUNCTION_ARGS_BYTES {
                        return Err(malformed());
                    }
                    call.thought_signature = Some(signature.to_owned());
                }
                events.push(GeminiChunkEvent::FunctionCall(call));
            }
        }
    }

    if let Some(reason) = finish_reason {
        events.push(GeminiChunkEvent::Finished {
            reason: sanitize_enum(reason),
        });
    }

    Ok(events)
}

/// Translates one provider function call into the shared Alfred tool request.
/// The shared host remains the authority for capability, path, permission,
/// approval, timeout, output, and secret checks.
pub fn alfred_tool_request(
    call: &GeminiFunctionCall,
    fallback_id: String,
) -> Result<AlfredToolRequest, NativeRuntimeError> {
    let kind = tool_kind_for(&call.name).ok_or_else(|| {
        NativeRuntimeError::new(
            NativeErrorCode::CapabilityUnsupported,
            "gemini requested a tool Alfred does not expose",
            false,
        )
    })?;
    let mut request = AlfredToolRequest::new(
        call.id.clone().unwrap_or(fallback_id),
        kind,
        call.name.clone(),
    );
    request.timeout_ms = TOOL_TIMEOUT_MS;
    request.max_output_bytes = TOOL_OUTPUT_BYTES;
    let mut remaining = call.args.clone();
    if let Some(path) = remaining.remove("path") {
        let path = path.as_str().ok_or_else(|| {
            NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "gemini tool path must be a string",
                false,
            )
        })?;
        request.path = Some(PathBuf::from(path));
    }
    if let Some(command) = remaining.remove("command") {
        let command = command.as_array().ok_or_else(|| {
            NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "gemini shell command must be an argument array",
                false,
            )
        })?;
        request.arguments = command
            .iter()
            .map(|argument| {
                argument.as_str().map(str::to_owned).ok_or_else(|| {
                    NativeRuntimeError::new(
                        NativeErrorCode::InvalidRequest,
                        "gemini shell arguments must be strings",
                        false,
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
    }
    request.input = remaining;
    Ok(request)
}

/// Maps a shared Alfred tool outcome back to a bounded Gemini function result.
pub fn function_result(
    call: &GeminiFunctionCall,
    result: &crate::agents::native::AlfredToolResult,
) -> GeminiFunctionResult {
    let status = match result.status {
        AlfredToolStatus::Completed => "completed",
        AlfredToolStatus::Denied => "denied",
        AlfredToolStatus::Cancelled => "cancelled",
        AlfredToolStatus::TimedOut => "timed_out",
        AlfredToolStatus::Failed => "failed",
    };
    GeminiFunctionResult {
        id: call.id.clone(),
        name: call.name.clone(),
        status,
        output: result.output.clone(),
    }
}

/// Incremental SSE decoder for `streamGenerateContent?alt=sse`.
///
/// Network chunks do not align with SSE frames. This decoder accepts split
/// UTF-8/JSON, ignores comments and metadata fields, and returns only bounded
/// `data:` payloads. A trailing partial frame is rejected by [`Self::finish`].
#[derive(Default)]
pub struct GeminiSseDecoder {
    buffer: Vec<u8>,
}

impl GeminiSseDecoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>, NativeRuntimeError> {
        self.buffer.extend_from_slice(bytes);
        let mut payloads = Vec::new();
        while let Some((frame_end, delimiter_len)) = find_sse_delimiter(&self.buffer) {
            if frame_end > MAX_SSE_FRAME_BYTES {
                return Err(oversized_sse());
            }
            let frame = self.buffer[..frame_end].to_vec();
            self.buffer.drain(..frame_end + delimiter_len);
            if let Some(payload) = decode_sse_frame(&frame)? {
                payloads.push(payload);
            }
        }
        if self.buffer.len() > MAX_SSE_FRAME_BYTES {
            return Err(oversized_sse());
        }
        Ok(payloads)
    }

    pub fn finish(self) -> Result<(), NativeRuntimeError> {
        if self.buffer.iter().all(u8::is_ascii_whitespace) {
            Ok(())
        } else {
            Err(NativeRuntimeError::new(
                NativeErrorCode::InvalidEvent,
                "gemini response stream ended with a partial SSE frame",
                false,
            ))
        }
    }
}

fn find_sse_delimiter(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = buffer.windows(2).position(|window| window == b"\n\n");
    let crlf = buffer.windows(4).position(|window| window == b"\r\n\r\n");
    match (lf, crlf) {
        (Some(left), Some(right)) if left <= right => Some((left, 2)),
        (Some(_), Some(right)) => Some((right, 4)),
        (Some(left), None) => Some((left, 2)),
        (None, Some(right)) => Some((right, 4)),
        (None, None) => None,
    }
}

fn decode_sse_frame(frame: &[u8]) -> Result<Option<String>, NativeRuntimeError> {
    let frame = std::str::from_utf8(frame).map_err(|_| malformed())?;
    let mut data_lines = Vec::new();
    for raw_line in frame.lines() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(data) = line.strip_prefix("data:") {
            data_lines.push(data.strip_prefix(' ').unwrap_or(data));
        } else if !(line.starts_with("event:")
            || line.starts_with("id:")
            || line.starts_with("retry:"))
        {
            return Err(malformed());
        }
    }
    if data_lines.is_empty() {
        return Ok(None);
    }
    let payload = data_lines.join("\n");
    if payload == "[DONE]" {
        return Ok(None);
    }
    if payload.len() > MAX_CHUNK_BYTES {
        return Err(oversized_sse());
    }
    Ok(Some(payload))
}

fn oversized_sse() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::EventLimitExceeded,
        "gemini SSE frame exceeded its byte limit",
        false,
    )
}

/// Finish reasons that mean the provider withheld content.
///
/// Treating any of these as a normal stop is exactly the "blocked response as
/// success" failure Plan 038 prohibits.
fn is_blocking_finish(reason: &str) -> bool {
    matches!(
        reason,
        "SAFETY"
            | "RECITATION"
            | "PROHIBITED_CONTENT"
            | "BLOCKED_PROMPT"
            | "BLOCKLIST"
            | "IMAGE_SAFETY"
            | "SPII"
            | "LANGUAGE"
            | "LANGUAGE_NOT_SUPPORTED"
            | "IMAGE_PROHIBITED_CONTENT"
            | "IMAGE_RECITATION"
            | "ESCALATION"
    )
}

fn parse_function_call(value: &Value) -> Result<GeminiFunctionCall, NativeRuntimeError> {
    let object = value.as_object().ok_or_else(malformed)?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(malformed)?;
    if name.is_empty() || name.len() > 128 {
        return Err(malformed());
    }
    let id = optional_safe_id(object.get("id"))?;
    let args = match object.get("args") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(map)) => map.clone(),
        Some(_) => return Err(malformed()),
    };
    let encoded = serde_json::to_vec(&args).map_err(|_| malformed())?;
    if encoded.len() > MAX_FUNCTION_ARGS_BYTES {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::EventLimitExceeded,
            "gemini function call arguments exceeded their byte limit",
            false,
        ));
    }
    Ok(GeminiFunctionCall {
        id,
        name: name.to_owned(),
        args,
        thought_signature: None,
    })
}

fn optional_safe_id(value: Option<&Value>) -> Result<Option<String>, NativeRuntimeError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let id = value.as_str().ok_or_else(malformed)?;
    let valid = !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'));
    if valid {
        Ok(Some(id.to_owned()))
    } else {
        Err(malformed())
    }
}

/// Maps an HTTP status onto the closed Alfred error vocabulary.
pub fn error_for_status(status: u16, detail: &str) -> NativeRuntimeError {
    let detail = sanitize_enum(detail);
    match status {
        400 => NativeRuntimeError::new(
            NativeErrorCode::InvalidRequest,
            format!("gemini rejected the request: {detail}"),
            false,
        ),
        // 401 and 403 are the same user-visible situation: the stored key no
        // longer authorizes this call, whether it was revoked, deleted, or
        // restricted. Both send the user back to Settings.
        401 | 403 => NativeRuntimeError::new(
            NativeErrorCode::AccountUnavailable,
            format!("gemini rejected the stored API key; reconnect the account: {detail}"),
            false,
        ),
        404 => NativeRuntimeError::new(
            NativeErrorCode::ModelUnavailable,
            format!("gemini model is unavailable for this account: {detail}"),
            false,
        ),
        429 => NativeRuntimeError::new(
            NativeErrorCode::ProviderUnavailable,
            format!("gemini rate limit or quota exhausted: {detail}"),
            true,
        ),
        500..=599 => NativeRuntimeError::new(
            NativeErrorCode::ProviderUnavailable,
            format!("gemini is temporarily unavailable: {detail}"),
            true,
        ),
        _ => NativeRuntimeError::new(
            NativeErrorCode::ProviderUnavailable,
            format!("gemini returned an unexpected status {status}: {detail}"),
            false,
        ),
    }
}

/// The stable Alfred error for a provider content block.
pub fn blocked_error(reason: &str) -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::ProviderUnavailable,
        format!(
            "gemini blocked this turn ({}); no output was produced",
            sanitize_enum(reason)
        ),
        false,
    )
}

fn malformed() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::InvalidEvent,
        "gemini stream chunk was malformed",
        false,
    )
}

/// Keeps provider-supplied enum-ish strings to a short, boring token so a
/// hostile payload cannot smuggle text into an Alfred error.
fn sanitize_enum(value: &str) -> String {
    let cleaned = value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | ' ')
        })
        .take(64)
        .collect::<String>();
    if cleaned.trim().is_empty() {
        "unspecified".into()
    } else {
        cleaned.trim().to_ascii_lowercase()
    }
}

/// Maps the model catalog body onto bounded native models.
pub fn parse_models(
    body: &str,
) -> Result<Vec<crate::agents::native::NativeModel>, NativeRuntimeError> {
    let value: Value = serde_json::from_str(body).map_err(|_| model_catalog_invalid())?;
    let models = value
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(model_catalog_invalid)?;
    let mapped = models
        .iter()
        .filter(|model| {
            model
                .get("supportedGenerationMethods")
                .and_then(Value::as_array)
                .is_some_and(|methods| {
                    methods
                        .iter()
                        .any(|method| method.as_str() == Some("generateContent"))
                })
        })
        .filter_map(|model| {
            let name = model.get("name").and_then(Value::as_str)?;
            let id = name.strip_prefix("models/").unwrap_or(name);
            let label = model
                .get("displayName")
                .and_then(Value::as_str)
                .filter(|label| !label.is_empty())
                .unwrap_or(id);
            (!id.is_empty() && id.len() <= 256 && label.len() <= 256).then(|| {
                crate::agents::native::NativeModel {
                    id: id.to_owned(),
                    label: label.to_owned(),
                }
            })
        })
        .collect::<Vec<_>>();
    if mapped.is_empty() {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::ModelUnavailable,
            "gemini published no generateContent models for this account",
            false,
        ));
    }
    Ok(mapped)
}

fn model_catalog_invalid() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::ModelUnavailable,
        "gemini model catalog response was malformed",
        false,
    )
}

/// Bounds the number of chunks one stream may deliver.
pub fn enforce_chunk_budget(seen: usize) -> Result<(), NativeRuntimeError> {
    if seen > MAX_STREAM_CHUNKS {
        Err(NativeRuntimeError::new(
            NativeErrorCode::EventLimitExceeded,
            "gemini stream exceeded its chunk budget",
            false,
        ))
    } else {
        Ok(())
    }
}

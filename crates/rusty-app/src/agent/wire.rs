//! Claude Code's stream-json wire, both ways: the lines its print mode writes on stdout
//! (`system`, `stream_event`, `assistant`, `user`, `control_request`, `control_response`,
//! `result`, `rate_limit_event`) parsed into the events a client renders, and the lines
//! a client writes on its stdin (a user message, a permission answer, the interrupt,
//! the mode and model changes). The shapes are those of `claude` 2.1.274 as the probe
//! `scripts/probe-claude-wire.sh` read them on this box (TICKET-031); the 2.1.260 lines
//! of TICKET-025 still parse.

use serde_json::Value;

/// Rusty's tools the note pane's agent may call without asking: the reads. A write
/// prompts.
pub const READ_TOOLS: &[&str] = &[
    "brain_read_page",
    "brain_search",
    "brain_list_pages",
    "brain_get_links",
    "brain_tags",
    "brain_tree",
    "brain_render",
    "brain_stats",
    "brain_due",
    "brain_get_timeline",
    "brain_page_types",
    "brain_resolve_slug",
    "brain_unresolved",
    "brain_graph",
    "brain_semantic_status",
    "brain_ask",
    "list_tasks",
    "list_task_groups",
    "list_notes",
    "read_note",
    "list_memories",
    "search_conversations",
    "skill_list",
    "skill_view",
    "script_list",
    "script_view",
    "settings_list",
    "setting_get",
];

/// The permission modes `--permission-mode` and `set_permission_mode` accept on 2.1.274
/// (`manual` is the listed name of `default`, and both work).
pub const PERMISSION_MODES: &[&str] = &[
    "default",
    "manual",
    "acceptEdits",
    "plan",
    "auto",
    "dontAsk",
    "bypassPermissions",
];

/// What the process said, one line of stream-json at a time.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A turn begins (the line comes with every turn, not at start-up): the session id,
    /// the model and the permission mode in force.
    Init {
        session_id: String,
        model: String,
        permission_mode: String,
    },
    /// The process is waiting on the API (`requesting`).
    Status(String),
    /// The permission mode changed, after a `set_permission_mode`.
    ModeChanged(String),
    /// The model is thinking; how many tokens so far, by the CLI's estimate. The
    /// thinking itself does not travel: its text is empty on the wire.
    ThinkingTokens(i64),
    BlockStart {
        kind: String,
        name: String,
        id: String,
    },
    TextDelta(String),
    TextFinal(String),
    ToolInput {
        id: String,
        name: String,
        input: String,
    },
    ToolResult {
        id: String,
        text: String,
        is_error: bool,
    },
    /// The agent wants to use a tool; `meta` is a JSON object with the `tool_use_id`,
    /// the `display_name` and the `permission_suggestions` the CLI offered.
    Permission {
        request_id: String,
        tool: String,
        input: String,
        description: String,
        meta: String,
    },
    /// The CLI answered one of the client's control requests.
    ControlAck {
        request_id: String,
        ok: bool,
        error: String,
    },
    TurnDone {
        ok: bool,
        cost_usd: f64,
        num_turns: i64,
        text: String,
        duration_ms: i64,
    },
    /// The account's rate limit windows, as a fraction used (`five_hour`, `seven_day`).
    RateLimit {
        five_hour: f64,
        seven_day: f64,
    },
    Notice(String),
}

fn str_of(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or("").to_string()
}

/// A tool result's content: a string, or text blocks joined.
fn content_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// The text between `<local-command-stdout>` tags, when a line carries one.
fn local_command_text(text: &str) -> Option<String> {
    let start = text.find("<local-command-stdout>")? + "<local-command-stdout>".len();
    let end = text[start..]
        .find("</local-command-stdout>")
        .map(|i| start + i)
        .unwrap_or(text.len());
    Some(text[start..end].trim().to_string())
}

/// Whether a line matters only while it streams: the deltas of a block (text, thinking,
/// a tool's input) and the thinking-token estimates. The `assistant` line that follows
/// carries the whole block, so a log without them loses nothing on replay.
pub fn is_live_only(v: &Value) -> bool {
    match v["type"].as_str().unwrap_or("") {
        "stream_event" => v["event"]["type"].as_str() == Some("content_block_delta"),
        "system" => v["subtype"].as_str() == Some("thinking_tokens"),
        _ => false,
    }
}

/// The events in one line of stream-json; a line that says nothing to a client gives
/// none, and a line that is not JSON gives none too. The host parses values it has
/// already read; the tests read the probe's lines through this.
#[cfg(test)]
pub fn parse_line(line: &str) -> Vec<Event> {
    match serde_json::from_str::<Value>(line) {
        Ok(v) => parse_value(&v),
        Err(_) => Vec::new(),
    }
}

/// The events in one parsed line of stream-json.
pub fn parse_value(v: &Value) -> Vec<Event> {
    let mut out = Vec::new();
    match v["type"].as_str().unwrap_or("") {
        "system" => match v["subtype"].as_str().unwrap_or("") {
            "init" => out.push(Event::Init {
                session_id: str_of(v, "session_id"),
                model: str_of(v, "model"),
                permission_mode: str_of(v, "permissionMode"),
            }),
            "status" => {
                if let Some(mode) = v["permissionMode"].as_str() {
                    out.push(Event::ModeChanged(mode.to_string()));
                }
                if let Some(status) = v["status"].as_str() {
                    out.push(Event::Status(status.to_string()));
                }
            }
            "thinking_tokens" => {
                out.push(Event::ThinkingTokens(
                    v["estimated_tokens"].as_i64().unwrap_or(0),
                ));
            }
            "permission_denied" => {
                let tool = str_of(v, "tool_name");
                out.push(Event::Notice(if tool.is_empty() {
                    "A permission was denied.".to_string()
                } else {
                    format!("A permission for {tool} was denied.")
                }));
            }
            "compact_boundary" => out.push(Event::Notice("The context was compacted.".into())),
            "api_retry" => {
                let attempt = v["attempt"].as_i64().unwrap_or(0);
                let max = v["max_retries"].as_i64().unwrap_or(0);
                let error = str_of(v, "error");
                out.push(Event::Notice(format!(
                    "Retrying the API, attempt {attempt} of {max}{}",
                    if error.is_empty() {
                        String::new()
                    } else {
                        format!(": {error}")
                    }
                )));
            }
            _ => {}
        },
        "stream_event" => {
            let event = &v["event"];
            match event["type"].as_str().unwrap_or("") {
                "content_block_start" => {
                    let block = &event["content_block"];
                    let kind = str_of(block, "type");
                    if kind == "text" || kind == "tool_use" || kind == "thinking" {
                        out.push(Event::BlockStart {
                            kind,
                            name: str_of(block, "name"),
                            id: str_of(block, "id"),
                        });
                    }
                }
                "content_block_delta" if event["delta"]["type"].as_str() == Some("text_delta") => {
                    out.push(Event::TextDelta(str_of(&event["delta"], "text")));
                }
                _ => {}
            }
        }
        "assistant" => {
            if let Some(blocks) = v["message"]["content"].as_array() {
                for block in blocks {
                    match block["type"].as_str().unwrap_or("") {
                        "tool_use" => out.push(Event::ToolInput {
                            id: str_of(block, "id"),
                            name: str_of(block, "name"),
                            input: block["input"].to_string(),
                        }),
                        "text" => out.push(Event::TextFinal(str_of(block, "text"))),
                        _ => {}
                    }
                }
            }
        }
        "user" => match &v["message"]["content"] {
            Value::Array(blocks) => {
                for block in blocks {
                    match block["type"].as_str().unwrap_or("") {
                        "tool_result" => out.push(Event::ToolResult {
                            id: str_of(block, "tool_use_id"),
                            text: content_text(&block["content"]),
                            is_error: block["is_error"].as_bool().unwrap_or(false),
                        }),
                        // The interrupt leaves a synthetic user line behind; the user's
                        // own messages are echoed by the host, not read back here.
                        "text"
                            if block["text"].as_str() == Some("[Request interrupted by user]") =>
                        {
                            out.push(Event::Notice("Interrupted.".into()));
                        }
                        _ => {}
                    }
                }
            }
            // A local command (`set_model`, for one) echoes what it printed.
            Value::String(text) => {
                if let Some(said) = local_command_text(text).filter(|s| !s.is_empty()) {
                    out.push(Event::Notice(said));
                }
            }
            _ => {}
        },
        "control_request" => {
            let request = &v["request"];
            if request["subtype"].as_str() == Some("can_use_tool") {
                let meta = serde_json::json!({
                    "tool_use_id": request["tool_use_id"],
                    "display_name": request["display_name"],
                    "permission_suggestions": request["permission_suggestions"],
                });
                out.push(Event::Permission {
                    request_id: str_of(v, "request_id"),
                    tool: str_of(request, "tool_name"),
                    input: request["input"].to_string(),
                    description: str_of(request, "description"),
                    meta: meta.to_string(),
                });
            }
        }
        "control_response" => {
            let response = &v["response"];
            let ok = response["subtype"].as_str() == Some("success");
            out.push(Event::ControlAck {
                request_id: str_of(response, "request_id"),
                ok,
                error: str_of(response, "error"),
            });
            if let Some(mode) = response["response"]["mode"].as_str() {
                out.push(Event::ModeChanged(mode.to_string()));
            }
        }
        "result" => {
            let is_error = v["is_error"].as_bool().unwrap_or(false);
            let subtype = v["subtype"].as_str().unwrap_or("");
            let ok = !is_error && subtype == "success";
            let text = if ok {
                str_of(v, "result")
            } else {
                let given = v["result"]
                    .as_str()
                    .or_else(|| v["error"].as_str())
                    .map(str::to_string)
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        v["errors"].as_array().map(|errors| {
                            errors
                                .iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join("; ")
                        })
                    })
                    .unwrap_or_default();
                if given.is_empty() {
                    format!("The turn ended with {subtype}.")
                } else {
                    given
                }
            };
            out.push(Event::TurnDone {
                ok,
                cost_usd: v["total_cost_usd"].as_f64().unwrap_or(0.0),
                num_turns: v["num_turns"].as_i64().unwrap_or(0),
                text,
                duration_ms: v["duration_ms"].as_i64().unwrap_or(0),
            });
        }
        "rate_limit_event" => {
            let windows = &v["rate_limit_info"]["unifiedWindows"];
            out.push(Event::RateLimit {
                five_hour: windows["five_hour"]["utilization"].as_f64().unwrap_or(0.0),
                seven_day: windows["seven_day"]["utilization"].as_f64().unwrap_or(0.0),
            });
        }
        _ => {}
    }
    out
}

/// One user message, as a line.
pub fn user_message(text: &str) -> String {
    serde_json::json!({
        "type": "user",
        "message": { "role": "user", "content": [{ "type": "text", "text": text }] }
    })
    .to_string()
}

/// The answer to a `can_use_tool` request: allow with the input as given (plus whatever
/// `extra_json` adds, such as `updatedPermissions` for "allow always"), or deny with the
/// message `extra_json` carries, or a plain one.
pub fn control_response(
    request_id: &str,
    allow: bool,
    input_json: &str,
    extra_json: &str,
) -> String {
    let extra = serde_json::from_str::<Value>(extra_json)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    let mut response = if allow {
        let input =
            serde_json::from_str::<Value>(input_json).unwrap_or_else(|_| serde_json::json!({}));
        serde_json::json!({ "behavior": "allow", "updatedInput": input })
    } else {
        let message = extra["message"]
            .as_str()
            .filter(|m| !m.trim().is_empty())
            .unwrap_or("Declined in Rusty.");
        serde_json::json!({ "behavior": "deny", "message": message })
    };
    if allow {
        if let Some(updates) = extra.get("updatedPermissions") {
            response["updatedPermissions"] = updates.clone();
        }
    }
    serde_json::json!({
        "type": "control_response",
        "response": { "subtype": "success", "request_id": request_id, "response": response }
    })
    .to_string()
}

/// A request to stop the running turn; the process stays.
pub fn interrupt_request(n: u64) -> String {
    serde_json::json!({
        "type": "control_request",
        "request_id": format!("rusty-interrupt-{n}"),
        "request": { "subtype": "interrupt" }
    })
    .to_string()
}

/// A request to change the permission mode for the turns to come.
pub fn set_permission_mode_request(n: u64, mode: &str) -> String {
    serde_json::json!({
        "type": "control_request",
        "request_id": format!("rusty-mode-{n}"),
        "request": { "subtype": "set_permission_mode", "mode": mode }
    })
    .to_string()
}

/// A request to change the model for the turns to come.
pub fn set_model_request(n: u64, model: &str) -> String {
    serde_json::json!({
        "type": "control_request",
        "request_id": format!("rusty-model-{n}"),
        "request": { "subtype": "set_model", "model": model }
    })
    .to_string()
}

/// The MCP configuration handed to the process: Rusty's server over HTTP.
pub fn mcp_config(mcp_url: &str) -> String {
    serde_json::json!({ "mcpServers": { "rusty": { "type": "http", "url": mcp_url } } }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_line_reads_the_2_1_260_probe() {
        // Lines as `claude` 2.1.260 wrote them on this box (TICKET-025), trimmed.
        assert_eq!(
            parse_line(
                r#"{"type":"system","subtype":"init","cwd":"/x","session_id":"62faa927","tools":["Bash"]}"#
            ),
            vec![Event::Init {
                session_id: "62faa927".into(),
                model: String::new(),
                permission_mode: String::new()
            }]
        );
        assert_eq!(
            parse_line(
                r#"{"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}},"session_id":"s"}"#
            ),
            vec![Event::BlockStart {
                kind: "text".into(),
                name: String::new(),
                id: String::new()
            }]
        );
        assert_eq!(
            parse_line(
                r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"Write","input":{}}}}"#
            ),
            vec![Event::BlockStart {
                kind: "tool_use".into(),
                name: "Write".into(),
                id: "toolu_1".into()
            }]
        );
        assert_eq!(
            parse_line(
                r#"{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":" from the probe"}}}"#
            ),
            vec![Event::TextDelta(" from the probe".into())]
        );
        assert!(parse_line(r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":""}}}"#).is_empty());
        assert_eq!(
            parse_line(
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":""},{"type":"tool_use","id":"toolu_1","name":"Write","input":{"file_path":"probe.txt","content":"ok"}}]}}"#
            ),
            vec![Event::ToolInput {
                id: "toolu_1".into(),
                name: "Write".into(),
                input: r#"{"content":"ok","file_path":"probe.txt"}"#.into()
            }]
        );
        assert_eq!(
            parse_line(
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hello from the probe"}]}}"#
            ),
            vec![Event::TextFinal("hello from the probe".into())]
        );
        assert_eq!(
            parse_line(
                r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"probe-ok","is_error":false}]}}"#
            ),
            vec![Event::ToolResult {
                id: "toolu_1".into(),
                text: "probe-ok".into(),
                is_error: false
            }]
        );
        assert_eq!(
            parse_line(
                r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t","content":[{"type":"text","text":"a"},{"type":"text","text":"b"}],"is_error":true}]}}"#
            ),
            vec![Event::ToolResult {
                id: "t".into(),
                text: "a\nb".into(),
                is_error: true
            }]
        );
        assert_eq!(
            parse_line(
                r#"{"type":"result","subtype":"error_max_turns","is_error":true,"num_turns":25}"#
            ),
            vec![Event::TurnDone {
                ok: false,
                cost_usd: 0.0,
                num_turns: 25,
                text: "The turn ended with error_max_turns.".into(),
                duration_ms: 0
            }]
        );
        assert_eq!(
            parse_line(r#"{"type":"system","subtype":"permission_denied","tool_name":"Write"}"#),
            vec![Event::Notice("A permission for Write was denied.".into())]
        );
        assert!(parse_line("not json").is_empty());
    }

    #[test]
    fn parse_line_reads_the_2_1_274_probe() {
        // Lines as `claude` 2.1.274 wrote them on 2026-09-17, scrubbed of paths.
        assert_eq!(
            parse_line(
                r#"{"type":"system","subtype":"init","cwd":"/x","session_id":"009c6823","tools":["Bash"],"mcp_servers":[{"name":"rusty","status":"connected","source":"dynamic"}],"model":"claude-haiku-4-5-20251001","permissionMode":"default","slash_commands":["brief"],"claude_code_version":"2.1.274","capabilities":["interrupt_receipt_v1","interrupt_cancel_queued_v1","msg_lifecycle_v1"]}"#
            ),
            vec![Event::Init {
                session_id: "009c6823".into(),
                model: "claude-haiku-4-5-20251001".into(),
                permission_mode: "default".into()
            }]
        );
        assert_eq!(
            parse_line(
                r#"{"type":"system","subtype":"status","status":"requesting","session_id":"s","uuid":"u"}"#
            ),
            vec![Event::Status("requesting".into())]
        );
        assert_eq!(
            parse_line(
                r#"{"type":"system","subtype":"status","status":null,"permissionMode":"acceptEdits","uuid":"u","session_id":"s"}"#
            ),
            vec![Event::ModeChanged("acceptEdits".into())]
        );
        assert_eq!(
            parse_line(
                r#"{"type":"system","subtype":"thinking_tokens","estimated_tokens":259,"estimated_tokens_delta":159,"session_id":"s","uuid":"u"}"#
            ),
            vec![Event::ThinkingTokens(259)]
        );
        assert_eq!(
            parse_line(
                r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}},"session_id":"s","parent_tool_use_id":null,"uuid":"u"}"#
            ),
            vec![Event::BlockStart {
                kind: "thinking".into(),
                name: String::new(),
                id: String::new()
            }]
        );
        assert!(parse_line(
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"EtAD"}},"session_id":"s"}"#
        )
        .is_empty());
        assert_eq!(
            parse_line(
                r#"{"type":"assistant","message":{"model":"claude-haiku-4-5-20251001","id":"msg_1","type":"message","role":"assistant","content":[{"type":"thinking","thinking":"","signature":"EtAD"}],"stop_reason":null},"parent_tool_use_id":null,"session_id":"s","uuid":"u"}"#
            ),
            Vec::<Event>::new()
        );
        assert_eq!(
            parse_line(
                r#"{"type":"control_request","request_id":"d7e1a10a","request":{"subtype":"can_use_tool","tool_name":"Write","display_name":"Write","input":{"file_path":"/x/hello.txt","content":"hello"},"description":"hello.txt","permission_suggestions":[{"type":"setMode","mode":"acceptEdits","destination":"session"}],"tool_use_id":"toolu_01F"}}"#
            ),
            vec![Event::Permission {
                request_id: "d7e1a10a".into(),
                tool: "Write".into(),
                input: r#"{"content":"hello","file_path":"/x/hello.txt"}"#.into(),
                description: "hello.txt".into(),
                meta: r#"{"display_name":"Write","permission_suggestions":[{"destination":"session","mode":"acceptEdits","type":"setMode"}],"tool_use_id":"toolu_01F"}"#.into()
            }]
        );
        assert_eq!(
            parse_line(
                r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"Not that file.","is_error":true,"tool_use_id":"toolu_011"}]},"parent_tool_use_id":null,"session_id":"s","uuid":"u","timestamp":"2026-09-17T21:14:43.009Z"}"#
            ),
            vec![Event::ToolResult {
                id: "toolu_011".into(),
                text: "Not that file.".into(),
                is_error: true
            }]
        );
        assert_eq!(
            parse_line(
                r#"{"type":"control_response","response":{"subtype":"success","request_id":"rusty-interrupt-1","response":{"still_queued":[]}}}"#
            ),
            vec![Event::ControlAck {
                request_id: "rusty-interrupt-1".into(),
                ok: true,
                error: String::new()
            }]
        );
        assert_eq!(
            parse_line(
                r#"{"type":"control_response","response":{"subtype":"success","request_id":"rusty-mode-1","response":{"mode":"acceptEdits"}}}"#
            ),
            vec![
                Event::ControlAck {
                    request_id: "rusty-mode-1".into(),
                    ok: true,
                    error: String::new()
                },
                Event::ModeChanged("acceptEdits".into())
            ]
        );
        assert_eq!(
            parse_line(
                r#"{"type":"control_response","response":{"subtype":"error","request_id":"rusty-mode-bogus","error":"Cannot set permission mode: must be one of acceptEdits, auto, bypassPermissions, default, dontAsk, plan"}}"#
            ),
            vec![Event::ControlAck {
                request_id: "rusty-mode-bogus".into(),
                ok: false,
                error: "Cannot set permission mode: must be one of acceptEdits, auto, bypassPermissions, default, dontAsk, plan".into()
            }]
        );
        assert_eq!(
            parse_line(
                r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]},"parent_tool_use_id":null,"session_id":"s","uuid":"u"}"#
            ),
            vec![Event::Notice("Interrupted.".into())]
        );
        assert_eq!(
            parse_line(
                r#"{"type":"user","message":{"role":"user","content":"<local-command-stdout>Set model to `haiku (claude-haiku-4-5-20251001)`</local-command-stdout>"},"session_id":"s","parent_tool_use_id":null,"uuid":"u","isReplay":true}"#
            ),
            vec![Event::Notice(
                "Set model to `haiku (claude-haiku-4-5-20251001)`".into()
            )]
        );
        assert!(parse_line(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"Reply with the single word ping."}]},"session_id":"s","parent_tool_use_id":null,"isReplay":true}"#
        )
        .is_empty());
        assert_eq!(
            parse_line(
                r#"{"type":"result","subtype":"success","is_error":false,"num_turns":2,"total_cost_usd":0.0263693,"result":"Done.","session_id":"s","duration_ms":5562,"duration_api_ms":6210,"permission_denials":[],"terminal_reason":"completed","queued_turn_count":0,"result_index":0}"#
            ),
            vec![Event::TurnDone {
                ok: true,
                cost_usd: 0.0263693,
                num_turns: 2,
                text: "Done.".into(),
                duration_ms: 5562
            }]
        );
        assert_eq!(
            parse_line(
                r#"{"type":"result","subtype":"error_during_execution","is_error":true,"num_turns":2,"total_cost_usd":0.000973,"duration_ms":2875,"terminal_reason":"aborted_streaming","permission_denials":[]}"#
            ),
            vec![Event::TurnDone {
                ok: false,
                cost_usd: 0.000973,
                num_turns: 2,
                text: "The turn ended with error_during_execution.".into(),
                duration_ms: 2875
            }]
        );
        assert_eq!(
            parse_line(
                r#"{"type":"result","subtype":"error_during_execution","duration_ms":0,"is_error":true,"num_turns":0,"session_id":"00000000","total_cost_usd":0,"errors":["No conversation found with session ID: 00000000"],"result_index":0}"#
            ),
            vec![Event::TurnDone {
                ok: false,
                cost_usd: 0.0,
                num_turns: 0,
                text: "No conversation found with session ID: 00000000".into(),
                duration_ms: 0
            }]
        );
        assert_eq!(
            parse_line(
                r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1789690200,"rateLimitType":"five_hour","unifiedWindows":{"five_hour":{"utilization":0.07,"resetsAt":1789690200},"seven_day":{"utilization":0.63,"resetsAt":1789894800}}},"uuid":"u","session_id":"s"}"#
            ),
            vec![Event::RateLimit {
                five_hour: 0.07,
                seven_day: 0.63
            }]
        );
        assert_eq!(
            parse_line(r#"{"type":"system","subtype":"compact_boundary","session_id":"s"}"#),
            vec![Event::Notice("The context was compacted.".into())]
        );
        assert_eq!(
            parse_line(
                r#"{"type":"system","subtype":"api_retry","attempt":2,"max_retries":10,"retry_delay_ms":2000,"error":"overloaded","session_id":"s"}"#
            ),
            vec![Event::Notice(
                "Retrying the API, attempt 2 of 10: overloaded".into()
            )]
        );
    }

    #[test]
    fn live_only_lines_are_the_deltas_and_the_thinking_estimates() {
        let delta: Value = serde_json::from_str(r#"{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"a"}}}"#).unwrap();
        assert!(is_live_only(&delta));
        let estimate: Value = serde_json::from_str(
            r#"{"type":"system","subtype":"thinking_tokens","estimated_tokens":50}"#,
        )
        .unwrap();
        assert!(is_live_only(&estimate));
        let start: Value = serde_json::from_str(r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}}"#).unwrap();
        assert!(!is_live_only(&start));
        let init: Value =
            serde_json::from_str(r#"{"type":"system","subtype":"init","session_id":"s"}"#).unwrap();
        assert!(!is_live_only(&init));
        let result: Value =
            serde_json::from_str(r#"{"type":"result","subtype":"success"}"#).unwrap();
        assert!(!is_live_only(&result));
    }

    #[test]
    fn messages_out_match_the_probe() {
        let user: Value = serde_json::from_str(&user_message("hi there")).unwrap();
        assert_eq!(user["type"], "user");
        assert_eq!(user["message"]["role"], "user");
        assert_eq!(user["message"]["content"][0]["text"], "hi there");
        let allow: Value =
            serde_json::from_str(&control_response("r-1", true, r#"{"file_path":"a"}"#, "{}"))
                .unwrap();
        assert_eq!(allow["type"], "control_response");
        assert_eq!(allow["response"]["subtype"], "success");
        assert_eq!(allow["response"]["request_id"], "r-1");
        assert_eq!(allow["response"]["response"]["behavior"], "allow");
        assert_eq!(
            allow["response"]["response"]["updatedInput"]["file_path"],
            "a"
        );
        assert!(allow["response"]["response"]
            .get("updatedPermissions")
            .is_none());
        let always: Value = serde_json::from_str(&control_response(
            "r-3",
            true,
            "{}",
            r#"{"updatedPermissions":[{"type":"setMode","mode":"acceptEdits","destination":"session"}]}"#,
        ))
        .unwrap();
        assert_eq!(
            always["response"]["response"]["updatedPermissions"][0]["mode"],
            "acceptEdits"
        );
        let deny: Value =
            serde_json::from_str(&control_response("r-2", false, "nonsense", "")).unwrap();
        assert_eq!(deny["response"]["response"]["behavior"], "deny");
        assert_eq!(
            deny["response"]["response"]["message"],
            "Declined in Rusty."
        );
        let reason: Value = serde_json::from_str(&control_response(
            "r-4",
            false,
            "{}",
            r#"{"message":"Not that file."}"#,
        ))
        .unwrap();
        assert_eq!(reason["response"]["response"]["message"], "Not that file.");
        let stop: Value = serde_json::from_str(&interrupt_request(4)).unwrap();
        assert_eq!(stop["type"], "control_request");
        assert_eq!(stop["request"]["subtype"], "interrupt");
        assert_eq!(stop["request_id"], "rusty-interrupt-4");
        let mode: Value = serde_json::from_str(&set_permission_mode_request(2, "plan")).unwrap();
        assert_eq!(mode["request"]["subtype"], "set_permission_mode");
        assert_eq!(mode["request"]["mode"], "plan");
        assert_eq!(mode["request_id"], "rusty-mode-2");
        let model: Value = serde_json::from_str(&set_model_request(1, "sonnet")).unwrap();
        assert_eq!(model["request"]["subtype"], "set_model");
        assert_eq!(model["request"]["model"], "sonnet");
        let config: Value = serde_json::from_str(&mcp_config("http://127.0.0.1:4174/mcp")).unwrap();
        assert_eq!(config["mcpServers"]["rusty"]["type"], "http");
    }
}

//! Faithful event projection of reachable Store leaves; tool output is not chat.
use super::*;
pub(super) struct Projection<'a> {
    locator: &'a str,
    previous: Option<String>,
    leaf_message: Option<String>,
    pub events: Vec<ImportedEvent>,
    pub relations: Vec<EventRelation>,
    pub issues: Vec<ImportIssue>,
}
impl<'a> Projection<'a> {
    pub fn new(locator: &'a str) -> Self {
        Self {
            locator,
            previous: None,
            leaf_message: None,
            events: Vec::new(),
            relations: Vec::new(),
            issues: Vec::new(),
        }
    }
    pub fn issue(&mut self, code: &str, hash: &str, message: &str) {
        self.issues.push(
            ImportIssue::new(code, message)
                .with_span(hash.to_string())
                .with_error(ErrorCode::PartialHistory),
        );
    }
    fn base(
        &self,
        hash: &str,
        occurrence: usize,
        block: usize,
        timestamp: Option<String>,
    ) -> EventBase {
        EventBase {
            id: format!("cursor:store:{hash}:{occurrence}:{block}"),
            ordinal: self.events.len() as u32,
            spans: vec![SourceSpan {
                artifact_id: self.locator.into(),
                record: hash.into(),
                block: Some(block as u32),
            }],
            time_origin: if timestamp.is_some() {
                TimeOrigin::Source
            } else {
                TimeOrigin::Unknown
            },
            timestamp,
        }
    }
    fn push(&mut self, event: ImportedEvent) {
        if matches!(event, ImportedEvent::Message { .. }) {
            self.leaf_message = Some(event.id().to_string());
        }
        link_prev(&mut self.relations, event.id(), &mut self.previous);
        self.events.push(event);
    }
    pub fn message(&mut self, hash: &str, occurrence: usize, object: &Value) {
        self.leaf_message = None;
        let role = object
            .get("role")
            .or_else(|| object.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let timestamp = super::super::cursor_store::bubble_timestamp(object);
        let content = object.get("content").or_else(|| object.get("text"));
        if role == "tool" {
            self.tool_leaf(hash, occurrence, object, timestamp);
            return;
        }
        let mrole = match role {
            "user" | "human" => MessageRole::User,
            "assistant" | "ai" => MessageRole::Assistant,
            "system" => MessageRole::System,
            _ => {
                self.issue(
                    "unknown-role",
                    hash,
                    "selected Store leaf has unsupported role",
                );
                self.unsupported(hash, occurrence, 0, "store-role", timestamp);
                return;
            }
        };
        let mut blocks = match content {
            Some(Value::Array(blocks)) => blocks.clone(),
            Some(Value::String(value)) if !value.is_empty() => {
                vec![serde_json::json!({"type":"text","text":value})]
            }
            None | Some(Value::Null) | Some(Value::String(_)) => Vec::new(),
            Some(_) => vec![serde_json::json!({"type":"store-content"})],
        };
        if let Some(calls) = object.get("tool_calls").and_then(Value::as_array) {
            for call in calls {
                blocks.push(serde_json::json!({
                    "type":"tool_call", "toolCallId":call.get("id"),
                    "name":call.pointer("/function/name").or_else(|| call.get("name")),
                    "arguments":call.pointer("/function/arguments").or_else(|| call.get("arguments"))
                }));
            }
        } else if object
            .get("tool_calls")
            .is_some_and(|calls| !calls.is_null())
        {
            blocks.push(serde_json::json!({"type":"store-tool-calls"}));
        }
        if blocks.is_empty() {
            self.issue(
                "unknown-block",
                hash,
                "selected Store message has no readable content",
            );
            self.unsupported(hash, occurrence, 0, "store-empty-content", timestamp);
            return;
        }
        for (index, block) in blocks.iter().enumerate() {
            let base = self.base(hash, occurrence, index, timestamp.clone());
            let kind = block.get("type").and_then(Value::as_str).unwrap_or("");
            let event = match kind {
                "text" if block.get("text").and_then(Value::as_str).is_some() => {
                    ImportedEvent::Message {
                        base,
                        role: mrole,
                        blocks: vec![ContentBlock::text(block["text"].as_str().unwrap())],
                        complete: true,
                    }
                }
                "tool_use" | "tool_call" | "tool-call" => {
                    let call_id = block
                        .get("toolCallId")
                        .or_else(|| block.get("id"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let name = block
                        .get("toolName")
                        .or_else(|| block.get("name"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if name.is_empty() {
                        self.issue("unknown-block", hash, "Store tool call has no name");
                        self.unsupported(hash, occurrence, index, kind, timestamp.clone());
                        continue;
                    }
                    let input = block
                        .get("args")
                        .or_else(|| block.get("input"))
                        .or_else(|| block.get("arguments"))
                        .cloned()
                        .unwrap_or(Value::Null);
                    let input = input
                        .as_str()
                        .and_then(|raw| serde_json::from_str(raw).ok())
                        .unwrap_or(input);
                    ImportedEvent::ToolCall {
                        base,
                        call_id: call_id.into(),
                        name: name.into(),
                        input: ToolInput::Json { value: input },
                        parent_message_id: self.leaf_message.clone(),
                    }
                }
                "tool_result" | "tool-result" => {
                    let call_id = block
                        .get("tool_use_id")
                        .or_else(|| block.get("toolCallId"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let value = block
                        .get("content")
                        .or_else(|| block.get("result"))
                        .or_else(|| block.get("text"))
                        .unwrap_or(&Value::Null);
                    ImportedEvent::ToolResult {
                        base,
                        call_id: call_id.into(),
                        blocks: vec![ContentBlock::text(text(value))],
                        outcome: outcome(block),
                    }
                }
                _ => {
                    self.issue("unknown-block", hash, "selected Store block is unsupported");
                    self.unsupported(hash, occurrence, index, kind, timestamp.clone());
                    continue;
                }
            };
            self.push(event);
            if matches!(kind, "tool_use" | "tool_call" | "tool-call") {
                if let Some(result) = block.get("result").filter(|value| !value.is_null()) {
                    let base = self.base(hash, occurrence, blocks.len() + index, timestamp.clone());
                    let call_id = block
                        .get("toolCallId")
                        .or_else(|| block.get("id"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    self.push(ImportedEvent::ToolResult {
                        base,
                        call_id: call_id.into(),
                        blocks: vec![ContentBlock::text(text(result))],
                        outcome: outcome(block),
                    });
                }
            }
        }
    }

    fn tool_leaf(
        &mut self,
        hash: &str,
        occurrence: usize,
        object: &Value,
        timestamp: Option<String>,
    ) {
        let root_call = object
            .get("tool_call_id")
            .or_else(|| object.get("toolCallId"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let content = object.get("content").or_else(|| object.get("text"));
        if let Some(blocks) = content.and_then(Value::as_array) {
            for (index, block) in blocks.iter().enumerate() {
                let kind = block.get("type").and_then(Value::as_str).unwrap_or("");
                let value = match kind {
                    "tool_result" | "tool-result" => block
                        .get("content")
                        .or_else(|| block.get("result"))
                        .or_else(|| block.get("text")),
                    "text" => block.get("text"),
                    _ => {
                        self.issue(
                            "unknown-block",
                            hash,
                            "Store tool result block is unsupported",
                        );
                        self.unsupported(hash, occurrence, index, kind, timestamp.clone());
                        continue;
                    }
                };
                let call_id = block
                    .get("tool_use_id")
                    .or_else(|| block.get("toolCallId"))
                    .and_then(Value::as_str)
                    .unwrap_or(root_call);
                let base = self.base(hash, occurrence, index, timestamp.clone());
                let result_outcome =
                    if block.get("is_error").is_some() || block.get("outcome").is_some() {
                        outcome(block)
                    } else {
                        outcome(object)
                    };
                self.push(ImportedEvent::ToolResult {
                    base,
                    call_id: call_id.into(),
                    blocks: vec![ContentBlock::text(value.map(text).unwrap_or_default())],
                    outcome: result_outcome,
                });
            }
            if blocks.is_empty() {
                self.issue(
                    "unknown-block",
                    hash,
                    "Store tool result contains no readable blocks",
                );
                self.unsupported(hash, occurrence, 0, "store-empty-tool-result", timestamp);
            }
        } else {
            let base = self.base(hash, occurrence, 0, timestamp);
            self.push(ImportedEvent::ToolResult {
                base,
                call_id: root_call.into(),
                blocks: vec![ContentBlock::text(content.map(text).unwrap_or_default())],
                outcome: outcome(object),
            });
        }
    }
    fn unsupported(
        &mut self,
        hash: &str,
        occurrence: usize,
        block: usize,
        source_type: &str,
        timestamp: Option<String>,
    ) {
        let base = self.base(hash, occurrence, block, timestamp);
        self.push(ImportedEvent::Unsupported {
            base,
            source_type: source_type.into(),
            payload: None,
        });
    }
}
fn text(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}
fn outcome(value: &Value) -> ToolOutcome {
    match value.get("is_error").and_then(Value::as_bool) {
        Some(true) => ToolOutcome::Error,
        Some(false) => ToolOutcome::Success,
        None => match value.get("outcome").and_then(Value::as_str) {
            Some("success") => ToolOutcome::Success,
            Some("error") => ToolOutcome::Error,
            Some("cancelled") => ToolOutcome::Cancelled,
            _ => ToolOutcome::Unknown,
        },
    }
}

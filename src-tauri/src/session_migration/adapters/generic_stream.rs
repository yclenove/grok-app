use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{fence_open, heading_role, normalize_role, prefix_role};
use crate::session_migration::budget::JSON_NEST_MAX;
use crate::session_migration::errors::{ErrorCode, ImportError};
use crate::session_migration::types::{
    AdapterOutput, ArtifactEvidence, ContentBlock, EventBase, EventRelation, ImportIssue,
    ImportOutcome, ImportedEvent, Inventory, MessageRole, NormalizedGraph, Provider,
    RelationConfidence, RelationKind, SourceEvidence, SourceSpan, TimeOrigin, NORMALIZER_VERSION,
};

pub(super) type StreamingReader = BufReader<ObservedReader<std::io::Take<File>>>;

pub(super) struct ObservedReader<R> {
    inner: R,
    hash: Sha256,
    bytes_read: u64,
    prefix: Vec<u8>,
    prefix_resolved: bool,
    first_non_whitespace: Option<u8>,
    json_mode: bool,
    depth: usize,
    in_string: bool,
    escaped: bool,
    depth_exceeded: bool,
    utf8_tail: Vec<u8>,
    invalid_utf8: bool,
}

impl<R> ObservedReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            hash: Sha256::new(),
            bytes_read: 0,
            prefix: Vec::with_capacity(3),
            prefix_resolved: false,
            first_non_whitespace: None,
            json_mode: false,
            depth: 0,
            in_string: false,
            escaped: false,
            depth_exceeded: false,
            utf8_tail: Vec::new(),
            invalid_utf8: false,
        }
    }

    fn observe(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if !self.prefix_resolved {
                self.prefix.push(byte);
                if self.prefix.len() < 3 {
                    continue;
                }
                let prefix = std::mem::take(&mut self.prefix);
                self.prefix_resolved = true;
                let start = usize::from(prefix.as_slice() == b"\xef\xbb\xbf") * 3;
                for &pending in &prefix[start..] {
                    self.observe_logical_byte(pending);
                }
                continue;
            }
            self.observe_logical_byte(byte);
        }
    }

    fn finish_prefix(&mut self) {
        if self.prefix_resolved {
            return;
        }
        self.prefix_resolved = true;
        let prefix = std::mem::take(&mut self.prefix);
        for byte in prefix {
            self.observe_logical_byte(byte);
        }
    }

    fn validate_utf8(&mut self, bytes: &[u8]) {
        if self.invalid_utf8 {
            return;
        }
        let mut combined = Vec::with_capacity(self.utf8_tail.len() + bytes.len());
        combined.extend_from_slice(&self.utf8_tail);
        combined.extend_from_slice(bytes);
        self.utf8_tail.clear();
        match std::str::from_utf8(&combined) {
            Ok(_) => {}
            Err(error) if error.error_len().is_some() => self.invalid_utf8 = true,
            Err(error) => self
                .utf8_tail
                .extend_from_slice(&combined[error.valid_up_to()..]),
        }
    }

    fn observe_logical_byte(&mut self, byte: u8) {
        if self.first_non_whitespace.is_none() {
            if byte.is_ascii_whitespace() {
                return;
            }
            self.first_non_whitespace = Some(byte);
            self.json_mode = matches!(byte, b'{' | b'[');
        }
        if !self.json_mode || self.depth_exceeded {
            return;
        }
        if self.in_string {
            if self.escaped {
                self.escaped = false;
            } else if byte == b'\\' {
                self.escaped = true;
            } else if byte == b'"' {
                self.in_string = false;
            }
            return;
        }
        match byte {
            b'"' => self.in_string = true,
            b'{' | b'[' => {
                self.depth += 1;
                self.depth_exceeded = self.depth > JSON_NEST_MAX;
            }
            b'}' | b']' => self.depth = self.depth.saturating_sub(1),
            _ => {}
        }
    }
}

impl<R: Read> Read for ObservedReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = self.inner.read(buffer)?;
        if count == 0 {
            self.finish_prefix();
            if !self.utf8_tail.is_empty() {
                self.invalid_utf8 = true;
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "file is not UTF-8",
                ));
            }
            return Ok(0);
        }
        self.bytes_read = self.bytes_read.saturating_add(count as u64);
        self.hash.update(&buffer[..count]);
        self.validate_utf8(&buffer[..count]);
        self.observe(&buffer[..count]);
        if self.depth_exceeded || self.invalid_utf8 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                if self.depth_exceeded {
                    "JSON nesting exceeds 64 levels"
                } else {
                    "file is not UTF-8"
                },
            ));
        }
        Ok(count)
    }
}

pub(super) fn streaming_reader(path: &Path, len: u64) -> Result<StreamingReader, ImportError> {
    let file = File::open(path).map_err(|error| {
        ImportError::new(
            ErrorCode::SourceNotFound,
            format!("{}: {error}", path.display()),
        )
    })?;
    Ok(BufReader::with_capacity(
        64 * 1024,
        ObservedReader::new(file.take(len)),
    ))
}

pub(super) fn skip_bom_and_leading_whitespace(
    reader: &mut StreamingReader,
) -> Result<Option<u8>, ImportError> {
    let mut at_start = true;
    loop {
        let (consume, first) = {
            let buffer = match reader.fill_buf() {
                Ok(buffer) => buffer,
                Err(error) => {
                    let error = if reader.get_ref().depth_exceeded {
                        ImportError::new(ErrorCode::ResourceLimit, "JSON nesting exceeds 64 levels")
                    } else if reader.get_ref().invalid_utf8 {
                        ImportError::new(ErrorCode::FormatUnsupported, "file is not UTF-8")
                    } else {
                        ImportError::new(ErrorCode::SourceNotFound, error.to_string())
                    };
                    return Err(error);
                }
            };
            if buffer.is_empty() {
                return Ok(None);
            }
            let mut start = 0;
            if at_start && buffer.starts_with(b"\xef\xbb\xbf") {
                start = 3;
            }
            let whitespace = buffer[start..]
                .iter()
                .take_while(|byte| byte.is_ascii_whitespace())
                .count();
            let consume = start + whitespace;
            (consume, buffer.get(consume).copied())
        };
        reader.consume(consume);
        at_start = false;
        if first.is_some() {
            return Ok(first);
        }
    }
}

pub(super) fn finish_streaming_reader(
    mut reader: StreamingReader,
    path: &Path,
    expected_len: u64,
) -> Result<String, ImportError> {
    std::io::copy(&mut reader, &mut std::io::sink()).map_err(|error| {
        if reader.get_ref().depth_exceeded {
            ImportError::new(ErrorCode::ResourceLimit, "JSON nesting exceeds 64 levels")
        } else if reader.get_ref().invalid_utf8 {
            ImportError::new(ErrorCode::FormatUnsupported, "file is not UTF-8")
        } else {
            ImportError::new(ErrorCode::SourceNotFound, error.to_string())
        }
    })?;
    let observed = reader.into_inner();
    if observed.bytes_read != expected_len
        || std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0) != expected_len
    {
        return Err(ImportError::new(
            ErrorCode::SourceChanged,
            "source changed while it was being read",
        ));
    }
    Ok(hex::encode(observed.hash.finalize()))
}

pub(super) fn normalize_file(
    path: &Path,
    locator: &str,
    len: u64,
) -> Result<AdapterOutput, ImportError> {
    let mut reader = streaming_reader(path, len)?;
    let first = skip_bom_and_leading_whitespace(&mut reader)?
        .ok_or_else(|| ImportError::new(ErrorCode::FormatUnsupported, "empty transcript"))?;
    if matches!(first, b'{' | b'[') {
        parse_json_stream(reader, path, locator, len, first)
    } else {
        parse_markdown_stream(reader, path, locator, len)
    }
}

#[derive(Default)]
enum MessageArrayField {
    #[default]
    Missing,
    Array(Vec<Value>),
    Other,
}

impl<'de> Deserialize<'de> for MessageArrayField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(match Value::deserialize(deserializer)? {
            Value::Array(values) => Self::Array(values),
            _ => Self::Other,
        })
    }
}

#[derive(Default, Deserialize)]
struct GenericJsonEnvelope {
    #[serde(default)]
    messages: MessageArrayField,
    #[serde(default)]
    conversation: MessageArrayField,
    #[serde(default)]
    items: MessageArrayField,
    #[serde(default)]
    usage: Option<Value>,
    #[serde(default, rename = "token_count")]
    token_count: Option<Value>,
    #[serde(default, rename = "tokenCount")]
    token_count_camel: Option<Value>,
    #[serde(default, rename = "total_tokens")]
    total_tokens: Option<Value>,
    #[serde(default, rename = "totalTokens")]
    total_tokens_camel: Option<Value>,
}

fn numeric_value_as_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|number| u64::try_from(number).ok()))
        .or_else(|| {
            value
                .as_f64()
                .and_then(|number| (number.is_finite() && number >= 0.0).then_some(number as u64))
        })
}

fn source_tokens(envelope: &GenericJsonEnvelope) -> u64 {
    envelope
        .usage
        .as_ref()
        .and_then(|usage| {
            usage
                .get("total_tokens")
                .or_else(|| usage.get("totalTokens"))
                .and_then(numeric_value_as_u64)
        })
        .or_else(|| envelope.token_count.as_ref().and_then(numeric_value_as_u64))
        .or_else(|| {
            envelope
                .token_count_camel
                .as_ref()
                .and_then(numeric_value_as_u64)
        })
        .or_else(|| {
            envelope
                .total_tokens
                .as_ref()
                .and_then(numeric_value_as_u64)
        })
        .or_else(|| {
            envelope
                .total_tokens_camel
                .as_ref()
                .and_then(numeric_value_as_u64)
        })
        .unwrap_or(0)
}

fn choose_message_array(envelope: GenericJsonEnvelope) -> Result<Vec<Value>, ImportError> {
    for field in [envelope.messages, envelope.conversation, envelope.items] {
        match field {
            MessageArrayField::Missing => continue,
            MessageArrayField::Array(values) => return Ok(values),
            MessageArrayField::Other => break,
        }
    }
    Err(ImportError::new(
        ErrorCode::FormatUnsupported,
        "JSON object has no messages/conversation/items array",
    ))
}

fn parse_json_stream(
    mut reader: StreamingReader,
    path: &Path,
    locator: &str,
    len: u64,
    first: u8,
) -> Result<AdapterOutput, ImportError> {
    if first == b'[' {
        let values: Vec<Value> = serde_json::from_reader(&mut reader).map_err(|error| {
            if reader.get_ref().depth_exceeded {
                ImportError::new(ErrorCode::ResourceLimit, "JSON nesting exceeds 64 levels")
            } else {
                ImportError::new(
                    ErrorCode::FormatUnsupported,
                    format!("invalid JSON array: {error}"),
                )
            }
        })?;
        let sha256 = finish_streaming_reader(reader, path, len)?;
        return messages_from_owned_json_array(values, locator, len, sha256, "json-array");
    }

    let envelope: GenericJsonEnvelope = serde_json::from_reader(&mut reader).map_err(|error| {
        if reader.get_ref().depth_exceeded {
            ImportError::new(ErrorCode::ResourceLimit, "JSON nesting exceeds 64 levels")
        } else {
            ImportError::new(
                ErrorCode::FormatUnsupported,
                format!("invalid JSON object: {error}"),
            )
        }
    })?;
    let source_tokens = source_tokens(&envelope);
    let values = choose_message_array(envelope)?;
    let sha256 = finish_streaming_reader(reader, path, len)?;
    let mut output = messages_from_owned_json_array(values, locator, len, sha256, "json-object")?;
    output.inventory.source_tokens = source_tokens;
    Ok(output)
}

fn take_first_present(item: &mut serde_json::Map<String, Value>, keys: &[&str]) -> Option<Value> {
    for key in keys {
        if item.contains_key(*key) {
            return item.remove(*key);
        }
    }
    None
}

fn value_into_string(value: Option<Value>) -> Option<String> {
    match value {
        Some(Value::String(text)) => Some(text),
        _ => None,
    }
}

fn extract_text_owned(item: &mut serde_json::Map<String, Value>) -> String {
    let content = match item.remove("content") {
        Some(Value::String(text)) => return text,
        other => other,
    };
    if let Some(text) = value_into_string(item.remove("text")) {
        return text;
    }
    if let Some(text) = value_into_string(item.remove("message")) {
        return text;
    }
    let Some(Value::Array(parts)) = content else {
        return String::new();
    };
    let mut texts = Vec::new();
    for part in parts {
        match part {
            Value::String(text) => texts.push(text),
            Value::Object(mut object) => {
                if let Some(text) = value_into_string(object.remove("text")) {
                    texts.push(text);
                }
            }
            _ => {}
        }
    }
    texts.join("\n")
}

fn messages_from_owned_json_array(
    values: Vec<Value>,
    locator: &str,
    byte_length: u64,
    sha256: String,
    fingerprint: &str,
) -> Result<AdapterOutput, ImportError> {
    let mut events = Vec::with_capacity(values.len());
    let mut relations = Vec::with_capacity(values.len().saturating_sub(1));
    let mut previous: Option<String> = None;
    let mut ordinal = 0u32;
    let mut issues = Vec::new();
    for (index, value) in values.into_iter().enumerate() {
        let mut item = match value {
            Value::Object(item) => item,
            _ => serde_json::Map::new(),
        };
        let role_value = take_first_present(&mut item, &["role", "author"]);
        let role_raw = role_value.as_ref().and_then(Value::as_str).unwrap_or("");
        let Some(role) = normalize_role(role_raw) else {
            issues.push(
                ImportIssue::new("unknown-role", format!("row {index} has role '{role_raw}'"))
                    .with_span(format!("index:{index}")),
            );
            continue;
        };
        let text = extract_text_owned(&mut item);
        if text.trim().is_empty() {
            continue;
        }
        let timestamp = value_into_string(take_first_present(
            &mut item,
            &["timestamp", "createdAt", "created_at"],
        ));
        let id = value_into_string(item.remove("id"))
            .map(|id| format!("generic:{id}"))
            .unwrap_or_else(|| format!("generic:{index}"));
        events.push(ImportedEvent::Message {
            base: EventBase {
                id: id.clone(),
                ordinal,
                spans: vec![SourceSpan {
                    artifact_id: locator.into(),
                    record: format!("index:{index}"),
                    block: None,
                }],
                timestamp: timestamp.clone(),
                time_origin: if timestamp.is_some() {
                    TimeOrigin::Source
                } else {
                    TimeOrigin::Unknown
                },
            },
            role,
            blocks: vec![ContentBlock::text(text)],
            complete: true,
        });
        if let Some(parent) = previous {
            relations.push(EventRelation {
                kind: RelationKind::Parent,
                from: id.clone(),
                to: parent,
                confidence: RelationConfidence::Inferred,
            });
        }
        previous = Some(id);
        ordinal += 1;
    }
    if events.is_empty() {
        return Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            "JSON array had no user/assistant messages",
        ));
    }
    Ok(finish_stream_output(
        events,
        relations,
        issues,
        locator,
        byte_length,
        sha256,
        fingerprint,
        Path::new(locator)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned()),
    ))
}

fn take_trimmed_string(buffer: &mut String) -> String {
    while buffer.chars().next_back().is_some_and(char::is_whitespace) {
        buffer.pop();
    }
    let start = buffer
        .char_indices()
        .find_map(|(index, ch)| (!ch.is_whitespace()).then_some(index))
        .unwrap_or(buffer.len());
    if start > 0 {
        buffer.drain(..start);
    }
    std::mem::take(buffer)
}

fn flush_markdown_message(
    role: Option<MessageRole>,
    buffer: &mut String,
    events: &mut Vec<ImportedEvent>,
    relations: &mut Vec<EventRelation>,
    previous: &mut Option<String>,
    ordinal: &mut u32,
    locator: &str,
) {
    let Some(role) = role else { return };
    let body = take_trimmed_string(buffer);
    if body.is_empty() {
        return;
    }
    let id = format!("generic:md:{ordinal}");
    events.push(ImportedEvent::Message {
        base: EventBase {
            id: id.clone(),
            ordinal: *ordinal,
            spans: vec![SourceSpan {
                artifact_id: locator.into(),
                record: format!("md:{ordinal}"),
                block: None,
            }],
            timestamp: None,
            time_origin: TimeOrigin::Unknown,
        },
        role,
        blocks: vec![ContentBlock::text(body)],
        complete: true,
    });
    if let Some(parent) = previous.take() {
        relations.push(EventRelation {
            kind: RelationKind::Parent,
            from: id.clone(),
            to: parent,
            confidence: RelationConfidence::Inferred,
        });
    }
    *previous = Some(id);
    *ordinal += 1;
}

fn append_markdown_line(buffer: &mut String, line: &str) {
    buffer.push_str(line);
    buffer.push('\n');
}

fn parse_markdown_stream(
    mut reader: StreamingReader,
    path: &Path,
    locator: &str,
    len: u64,
) -> Result<AdapterOutput, ImportError> {
    let mut events = Vec::new();
    let mut relations = Vec::new();
    let mut previous: Option<String> = None;
    let mut ordinal = 0u32;
    let mut current_role: Option<MessageRole> = None;
    let mut body = String::new();
    let mut in_fence = false;
    let mut fence_mark: Option<String> = None;
    let mut line = String::new();
    loop {
        line.clear();
        let count = reader.read_line(&mut line).map_err(|error| {
            if error.kind() == std::io::ErrorKind::InvalidData {
                ImportError::new(ErrorCode::FormatUnsupported, "file is not UTF-8")
            } else {
                ImportError::new(ErrorCode::SourceNotFound, error.to_string())
            }
        })?;
        if count == 0 {
            break;
        }
        while line.ends_with('\n') || line.ends_with('\r') {
            line.pop();
        }
        let trimmed = line.trim();
        if let Some(mark) = fence_open(trimmed) {
            if in_fence && fence_mark.as_deref() == Some(mark) {
                in_fence = false;
                fence_mark = None;
            } else if !in_fence {
                in_fence = true;
                fence_mark = Some(mark.to_string());
            }
            if current_role.is_some() {
                append_markdown_line(&mut body, &line);
            }
            continue;
        }
        if in_fence {
            if current_role.is_some() {
                append_markdown_line(&mut body, &line);
            }
            continue;
        }
        if let Some((role, rest)) = heading_role(trimmed).or_else(|| prefix_role(trimmed)) {
            flush_markdown_message(
                current_role,
                &mut body,
                &mut events,
                &mut relations,
                &mut previous,
                &mut ordinal,
                locator,
            );
            current_role = Some(role);
            if let Some(rest) = rest.filter(|rest| !rest.is_empty()) {
                append_markdown_line(&mut body, &rest);
            }
            continue;
        }
        if current_role.is_some() {
            append_markdown_line(&mut body, &line);
        }
    }
    flush_markdown_message(
        current_role,
        &mut body,
        &mut events,
        &mut relations,
        &mut previous,
        &mut ordinal,
        locator,
    );
    if events.is_empty() {
        return Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            "Could not parse transcript. Use JSON messages array or markdown ## User / ## Assistant sections.",
        ));
    }
    let sha256 = finish_streaming_reader(reader, path, len)?;
    Ok(finish_stream_output(
        events,
        relations,
        Vec::new(),
        locator,
        len,
        sha256,
        "markdown",
        Path::new(locator)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned()),
    ))
}

// This is the format-neutral assembly boundary; its scalar evidence fields map
// directly to SourceEvidence and remain explicit for auditability.
#[allow(clippy::too_many_arguments)]
fn finish_stream_output(
    events: Vec<ImportedEvent>,
    relations: Vec<EventRelation>,
    issues: Vec<ImportIssue>,
    locator: &str,
    byte_length: u64,
    sha256: String,
    fingerprint: &str,
    title: Option<String>,
) -> AdapterOutput {
    let native_session_id = if locator.trim().is_empty() {
        "generic".into()
    } else {
        locator.to_string()
    };
    let mut output = AdapterOutput {
        evidence: SourceEvidence {
            provider: Provider::GenericFile,
            source_version: None,
            schema_fingerprint: fingerprint.into(),
            adapter_version: NORMALIZER_VERSION.into(),
            platform: crate::session_migration::detect::current_platform(),
            artifacts: vec![ArtifactEvidence {
                artifact_id: locator.into(),
                kind: "file".into(),
                relative_locator: locator.into(),
                byte_length,
                sha256,
                complete_prefix_sha256: None,
            }],
        },
        graph: NormalizedGraph {
            events,
            relations,
            active_leaf: None,
            native_session_id,
            title,
            cwd: None,
        },
        inventory: Inventory::default(),
        issues,
        assets: Vec::new(),
        outcome: ImportOutcome::Complete,
    };
    output.graph.active_leaf = output
        .graph
        .events
        .last()
        .map(|event| event.id().to_string());
    output.recount();
    if !output.issues.is_empty() {
        output.outcome = ImportOutcome::Partial;
    }
    output
}

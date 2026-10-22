//! Bounded JSON value scanning for the production generic-file importer.
//!
//! Serde's `Value` representation is convenient for small fixtures, but a
//! reader-backed deserializer can allocate an entire message before the
//! normalizer gets a chance to enforce the archive record limit. This module
//! first isolates one JSON value with a hard byte budget, then hands that one
//! value to the existing normalizer. Envelope arrays use the same path.

use std::cmp::min;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use serde::de::DeserializeSeed;
use serde_json::Value;
use uuid::Uuid;

use super::generic_spool::normalize_item;
use super::generic_stream::StreamingReader;
use crate::session_migration::budget::JSON_RECORD_MAX_BYTES;
use crate::session_migration::errors::{ErrorCode, ImportError};
use crate::session_migration::prepared_builder::PreparedRevisionBuilder;
use crate::session_migration::types::{
    EventRelation, ImportedEvent, RelationConfidence, RelationKind,
};

const KEY_MAX_BYTES: usize = 64 * 1024;

#[derive(Default)]
pub(super) struct JsonSummary {
    pub(super) relations: Vec<EventRelation>,
    pub(super) issues: Vec<crate::session_migration::types::ImportIssue>,
    pub(super) event_count: u32,
    pub(super) active_leaf: Option<String>,
}

struct EventSpool {
    path: PathBuf,
    writer: Option<BufWriter<File>>,
}

impl EventSpool {
    fn create(parent: &Path) -> Result<Self, ImportError> {
        let path = parent.join(format!(".generic-candidate-{}.jsonl", Uuid::new_v4()));
        let writer = File::create(&path).map(BufWriter::new).map_err(io_error)?;
        Ok(Self {
            path,
            writer: Some(writer),
        })
    }

    fn push(&mut self, event: &ImportedEvent) -> Result<(), ImportError> {
        let bytes = serde_json::to_vec(event).map_err(parse_error)?;
        if bytes.len() > JSON_RECORD_MAX_BYTES {
            return Err(ImportError::new(
                ErrorCode::ResourceLimit,
                "normalized event exceeds 32 MiB archive record limit",
            ));
        }
        let writer = self.writer.as_mut().ok_or_else(|| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "candidate spool is already finalized",
            )
        })?;
        writer.write_all(&bytes).map_err(io_error)?;
        writer.write_all(b"\n").map_err(io_error)
    }

    fn replay(mut self, builder: &mut PreparedRevisionBuilder) -> Result<(), ImportError> {
        if let Some(mut writer) = self.writer.take() {
            writer.flush().map_err(io_error)?;
        }
        let file = File::open(&self.path).map_err(io_error)?;
        let mut reader = BufReader::new(file);
        let mut line = Vec::new();
        loop {
            line.clear();
            let count = reader
                .by_ref()
                .take((JSON_RECORD_MAX_BYTES + 2) as u64)
                .read_until(b'\n', &mut line)
                .map_err(io_error)?;
            if count == 0 {
                break;
            }
            if line.len() > JSON_RECORD_MAX_BYTES + 1 {
                return Err(ImportError::new(
                    ErrorCode::ResourceLimit,
                    "candidate event exceeds 32 MiB archive record limit",
                ));
            }
            if line.ends_with(b"\n") {
                line.pop();
            }
            let event: ImportedEvent = serde_json::from_slice(&line).map_err(parse_error)?;
            builder.push_event(&event)?;
        }
        Ok(())
    }
}

impl Drop for EventSpool {
    fn drop(&mut self) {
        self.writer.take();
        let _ = fs::remove_file(&self.path);
    }
}

struct Candidate {
    spool: EventSpool,
    summary: JsonSummary,
}

enum CandidateField {
    Other,
    Array(Candidate),
}

fn peek(reader: &mut StreamingReader) -> Result<Option<u8>, ImportError> {
    reader
        .fill_buf()
        .map_err(stream_error)
        .map(|buffer| buffer.first().copied())
}

fn take(reader: &mut StreamingReader) -> Result<Option<u8>, ImportError> {
    let byte = peek(reader)?;
    if byte.is_some() {
        reader.consume(1);
    }
    Ok(byte)
}

fn skip_ws(reader: &mut StreamingReader) -> Result<(), ImportError> {
    loop {
        let count = {
            let buffer = reader.fill_buf().map_err(stream_error)?;
            buffer
                .iter()
                .take_while(|byte| byte.is_ascii_whitespace())
                .count()
        };
        if count == 0 {
            return Ok(());
        }
        reader.consume(count);
    }
}

fn expect(reader: &mut StreamingReader, expected: u8) -> Result<(), ImportError> {
    match take(reader)? {
        Some(actual) if actual == expected => Ok(()),
        Some(actual) => Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            format!(
                "invalid JSON: expected '{}', found '{}'",
                expected as char, actual as char
            ),
        )),
        None => Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            "invalid JSON: unexpected end of input",
        )),
    }
}

fn capture_byte(captured: &mut Option<Vec<u8>>, byte: u8, limit: usize) -> Result<(), ImportError> {
    let Some(buffer) = captured else {
        return Ok(());
    };
    if buffer.len() >= limit {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "JSON value exceeds 32 MiB archive record limit",
        ));
    }
    buffer.push(byte);
    Ok(())
}

fn read_string(
    reader: &mut StreamingReader,
    limit: Option<usize>,
) -> Result<Option<Vec<u8>>, ImportError> {
    let mut captured = limit.map(|limit| Vec::with_capacity(min(limit, 64 * 1024)));
    let quote = take(reader)?;
    if quote != Some(b'"') {
        return Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            "invalid JSON: expected a string",
        ));
    }
    capture_byte(&mut captured, b'"', limit.unwrap_or(usize::MAX))?;
    let mut escaped = false;
    loop {
        let byte = take(reader)?.ok_or_else(|| {
            ImportError::new(
                ErrorCode::FormatUnsupported,
                "invalid JSON: unterminated string",
            )
        })?;
        capture_byte(&mut captured, byte, limit.unwrap_or(usize::MAX))?;
        if escaped {
            escaped = false;
            continue;
        }
        if byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            return Ok(captured);
        } else if byte < 0x20 {
            return Err(ImportError::new(
                ErrorCode::FormatUnsupported,
                "invalid JSON: control character in string",
            ));
        }
    }
}

fn read_compound(
    reader: &mut StreamingReader,
    first: u8,
    limit: Option<usize>,
) -> Result<Option<Vec<u8>>, ImportError> {
    let mut captured = limit.map(|limit| Vec::with_capacity(min(limit, 64 * 1024)));
    let mut stack = vec![first];
    capture_byte(&mut captured, first, limit.unwrap_or(usize::MAX))?;
    let mut in_string = false;
    let mut escaped = false;
    while !stack.is_empty() {
        let byte = take(reader)?.ok_or_else(|| {
            ImportError::new(
                ErrorCode::FormatUnsupported,
                "invalid JSON: unterminated compound value",
            )
        })?;
        capture_byte(&mut captured, byte, limit.unwrap_or(usize::MAX))?;
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            } else if byte < 0x20 {
                return Err(ImportError::new(
                    ErrorCode::FormatUnsupported,
                    "invalid JSON: control character in string",
                ));
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => stack.push(byte),
            b'}' | b']' => {
                let expected = if byte == b'}' { b'{' } else { b'[' };
                if stack.pop() != Some(expected) {
                    return Err(ImportError::new(
                        ErrorCode::FormatUnsupported,
                        "invalid JSON: mismatched closing delimiter",
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(captured)
}

fn read_primitive(
    reader: &mut StreamingReader,
    limit: Option<usize>,
) -> Result<Option<Vec<u8>>, ImportError> {
    let mut captured = limit.map(|limit| Vec::with_capacity(min(limit, 256)));
    let mut count = 0usize;
    loop {
        let byte = match peek(reader)? {
            Some(byte) if !byte.is_ascii_whitespace() && !matches!(byte, b',' | b']' | b'}') => {
                byte
            }
            _ => break,
        };
        let _ = take(reader)?;
        count = count.saturating_add(1);
        capture_byte(&mut captured, byte, limit.unwrap_or(usize::MAX))?;
    }
    if count == 0 {
        return Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            "invalid JSON: expected a value",
        ));
    }
    Ok(captured)
}

/// Reads one complete JSON value, retaining at most `limit` bytes when a
/// capture is requested. With `None`, the value is skipped without allocation.
fn read_value(
    reader: &mut StreamingReader,
    limit: Option<usize>,
) -> Result<Option<Vec<u8>>, ImportError> {
    skip_ws(reader)?;
    if limit.is_none() {
        skip_valid_value(reader)?;
        return Ok(None);
    }
    let first = peek(reader)?.ok_or_else(|| {
        ImportError::new(
            ErrorCode::FormatUnsupported,
            "invalid JSON: expected a value",
        )
    })?;
    match first {
        b'"' => read_string(reader, limit),
        b'{' | b'[' => {
            let _ = take(reader)?;
            read_compound(reader, first, limit)
        }
        _ => read_primitive(reader, limit),
    }
}

fn invalid_json(message: impl Into<String>) -> ImportError {
    ImportError::new(ErrorCode::FormatUnsupported, message)
}

fn skip_valid_value(reader: &mut StreamingReader) -> Result<(), ImportError> {
    skip_ws(reader)?;
    match peek(reader)? {
        Some(b'"') => skip_valid_string(reader).map(|_| ()),
        Some(b'{') => skip_valid_object(reader),
        Some(b'[') => skip_valid_array(reader),
        Some(b't') => skip_literal(reader, b"true"),
        Some(b'f') => skip_literal(reader, b"false"),
        Some(b'n') => skip_literal(reader, b"null"),
        Some(b'-' | b'0'..=b'9') => skip_number(reader),
        Some(other) => Err(invalid_json(format!(
            "invalid JSON value starting with '{}'",
            other as char
        ))),
        None => Err(invalid_json("invalid JSON: expected a value")),
    }
}

fn skip_valid_string(reader: &mut StreamingReader) -> Result<(), ImportError> {
    expect(reader, b'"')?;
    loop {
        let byte =
            take(reader)?.ok_or_else(|| invalid_json("invalid JSON: unterminated string"))?;
        match byte {
            b'"' => return Ok(()),
            b'\\' => match take(reader)?
                .ok_or_else(|| invalid_json("invalid JSON: unterminated escape"))?
            {
                b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                b'u' => {
                    for _ in 0..4 {
                        let digit = take(reader)?.ok_or_else(|| {
                            invalid_json("invalid JSON: incomplete unicode escape")
                        })?;
                        if !digit.is_ascii_hexdigit() {
                            return Err(invalid_json("invalid JSON: malformed unicode escape"));
                        }
                    }
                }
                _ => return Err(invalid_json("invalid JSON: unsupported string escape")),
            },
            0x00..=0x1f => {
                return Err(invalid_json("invalid JSON: control character in string"));
            }
            _ => {}
        }
    }
}

fn skip_valid_array(reader: &mut StreamingReader) -> Result<(), ImportError> {
    expect(reader, b'[')?;
    skip_ws(reader)?;
    if peek(reader)? == Some(b']') {
        let _ = take(reader)?;
        return Ok(());
    }
    loop {
        skip_valid_value(reader)?;
        skip_ws(reader)?;
        match take(reader)? {
            Some(b',') => {
                skip_ws(reader)?;
                if peek(reader)? == Some(b']') {
                    return Err(invalid_json("invalid JSON: trailing comma in array"));
                }
            }
            Some(b']') => return Ok(()),
            _ => return Err(invalid_json("invalid JSON: expected ',' or ']'")),
        }
    }
}

fn skip_valid_object(reader: &mut StreamingReader) -> Result<(), ImportError> {
    expect(reader, b'{')?;
    skip_ws(reader)?;
    if peek(reader)? == Some(b'}') {
        let _ = take(reader)?;
        return Ok(());
    }
    loop {
        if peek(reader)? != Some(b'"') {
            return Err(invalid_json("invalid JSON: object key must be a string"));
        }
        skip_valid_string(reader)?;
        skip_ws(reader)?;
        expect(reader, b':')?;
        skip_valid_value(reader)?;
        skip_ws(reader)?;
        match take(reader)? {
            Some(b',') => {
                skip_ws(reader)?;
                if peek(reader)? == Some(b'}') {
                    return Err(invalid_json("invalid JSON: trailing comma in object"));
                }
            }
            Some(b'}') => return Ok(()),
            _ => return Err(invalid_json("invalid JSON: expected ',' or '}'")),
        }
    }
}

fn skip_literal(reader: &mut StreamingReader, expected: &[u8]) -> Result<(), ImportError> {
    for &byte in expected {
        if take(reader)? != Some(byte) {
            return Err(invalid_json("invalid JSON literal"));
        }
    }
    ensure_value_delimiter(reader)
}

fn take_digits(reader: &mut StreamingReader) -> Result<usize, ImportError> {
    let mut count = 0usize;
    while peek(reader)?.is_some_and(|byte| byte.is_ascii_digit()) {
        let _ = take(reader)?;
        count = count.saturating_add(1);
    }
    Ok(count)
}

fn skip_number(reader: &mut StreamingReader) -> Result<(), ImportError> {
    if peek(reader)? == Some(b'-') {
        let _ = take(reader)?;
    }
    match take(reader)? {
        Some(b'0') => {
            if peek(reader)?.is_some_and(|byte| byte.is_ascii_digit()) {
                return Err(invalid_json("invalid JSON number with a leading zero"));
            }
        }
        Some(b'1'..=b'9') => {
            let _ = take_digits(reader)?;
        }
        _ => return Err(invalid_json("invalid JSON number")),
    }
    if peek(reader)? == Some(b'.') {
        let _ = take(reader)?;
        if take_digits(reader)? == 0 {
            return Err(invalid_json("invalid JSON number fraction"));
        }
    }
    if matches!(peek(reader)?, Some(b'e' | b'E')) {
        let _ = take(reader)?;
        if matches!(peek(reader)?, Some(b'+' | b'-')) {
            let _ = take(reader)?;
        }
        if take_digits(reader)? == 0 {
            return Err(invalid_json("invalid JSON number exponent"));
        }
    }
    ensure_value_delimiter(reader)
}

fn ensure_value_delimiter(reader: &mut StreamingReader) -> Result<(), ImportError> {
    match peek(reader)? {
        None | Some(b',' | b']' | b'}') => Ok(()),
        Some(byte) if byte.is_ascii_whitespace() => Ok(()),
        _ => Err(invalid_json("invalid JSON value delimiter")),
    }
}

fn process_item<F>(
    raw: Vec<u8>,
    source_index: usize,
    locator: &str,
    summary: &mut JsonSummary,
    mut push: F,
) -> Result<(), ImportError>
where
    F: FnMut(&ImportedEvent) -> Result<(), ImportError>,
{
    let value: Value = serde_json::from_slice(&raw).map_err(|error| {
        ImportError::new(
            ErrorCode::FormatUnsupported,
            format!("invalid JSON message at index {source_index}: {error}"),
        )
    })?;
    if let Some(event) = normalize_item(
        value,
        source_index,
        summary.event_count,
        locator,
        &mut summary.issues,
    ) {
        push(&event)?;
        if let Some(parent) = summary.active_leaf.take() {
            summary.relations.push(EventRelation {
                kind: RelationKind::Parent,
                from: event.id().to_string(),
                to: parent,
                confidence: RelationConfidence::Inferred,
            });
        }
        summary.active_leaf = Some(event.id().to_string());
        summary.event_count = summary.event_count.saturating_add(1);
    }
    Ok(())
}

fn parse_array<F>(
    reader: &mut StreamingReader,
    locator: &str,
    mut push: F,
) -> Result<JsonSummary, ImportError>
where
    F: FnMut(&ImportedEvent) -> Result<(), ImportError>,
{
    expect(reader, b'[')?;
    let mut summary = JsonSummary::default();
    skip_ws(reader)?;
    if peek(reader)? == Some(b']') {
        let _ = take(reader)?;
        return Ok(summary);
    }
    let mut source_index = 0usize;
    loop {
        let raw = read_value(reader, Some(JSON_RECORD_MAX_BYTES))?.ok_or_else(|| {
            ImportError::new(
                ErrorCode::FormatUnsupported,
                "invalid JSON: missing array item",
            )
        })?;
        process_item(raw, source_index, locator, &mut summary, &mut push)?;
        source_index = source_index.saturating_add(1);
        skip_ws(reader)?;
        match take(reader)? {
            Some(b',') => {
                skip_ws(reader)?;
                if peek(reader)? == Some(b']') {
                    return Err(ImportError::new(
                        ErrorCode::FormatUnsupported,
                        "invalid JSON: trailing comma in array",
                    ));
                }
            }
            Some(b']') => return Ok(summary),
            Some(other) => {
                return Err(ImportError::new(
                    ErrorCode::FormatUnsupported,
                    format!(
                        "invalid JSON: expected ',' or ']', found '{}'",
                        other as char
                    ),
                ));
            }
            None => {
                return Err(ImportError::new(
                    ErrorCode::FormatUnsupported,
                    "invalid JSON: unterminated array",
                ));
            }
        }
    }
}

fn parse_candidate(
    reader: &mut StreamingReader,
    parent: &Path,
    locator: &str,
) -> Result<CandidateField, ImportError> {
    skip_ws(reader)?;
    if peek(reader)? != Some(b'[') {
        let _ = read_value(reader, None)?;
        return Ok(CandidateField::Other);
    }
    let mut spool = EventSpool::create(parent)?;
    let summary = parse_array(reader, locator, |event| spool.push(event))?;
    Ok(CandidateField::Array(Candidate { spool, summary }))
}

fn number_value(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|number| u64::try_from(number).ok()))
        .or_else(|| {
            value
                .as_f64()
                .and_then(|number| (number.is_finite() && number >= 0.0).then_some(number as u64))
        })
}

fn usage_value(raw: &[u8]) -> Result<Option<u64>, ImportError> {
    let mut deserializer = serde_json::Deserializer::from_slice(raw);
    let usage = super::generic_spool_tokens::UsageSeed
        .deserialize(&mut deserializer)
        .map_err(parse_error)?;
    deserializer.end().map_err(parse_error)?;
    Ok(usage.value())
}

fn scalar_value(raw: &[u8]) -> Result<Option<u64>, ImportError> {
    let value: Value = serde_json::from_slice(raw).map_err(parse_error)?;
    Ok(number_value(&value))
}

fn choose(
    messages: Option<CandidateField>,
    conversation: Option<CandidateField>,
    items: Option<CandidateField>,
    source_tokens: u64,
) -> Result<(Candidate, u64), ImportError> {
    for field in [messages, conversation, items] {
        let Some(field) = field else { continue };
        return match field {
            CandidateField::Array(candidate) if candidate.summary.event_count > 0 => {
                Ok((candidate, source_tokens))
            }
            CandidateField::Array(_) => Err(ImportError::new(
                ErrorCode::FormatUnsupported,
                "JSON array had no user/assistant messages",
            )),
            CandidateField::Other => Err(ImportError::new(
                ErrorCode::FormatUnsupported,
                "JSON object has no messages/conversation/items array",
            )),
        };
    }
    Err(ImportError::new(
        ErrorCode::FormatUnsupported,
        "JSON object has no messages/conversation/items array",
    ))
}

fn parse_object(
    reader: &mut StreamingReader,
    parent: &Path,
    locator: &str,
) -> Result<(Candidate, u64), ImportError> {
    expect(reader, b'{')?;
    let mut messages = None;
    let mut conversation = None;
    let mut items = None;
    let mut usage: Option<Option<u64>> = None;
    let mut token_count: Option<Option<u64>> = None;
    let mut token_count_camel: Option<Option<u64>> = None;
    let mut total_tokens: Option<Option<u64>> = None;
    let mut total_tokens_camel: Option<Option<u64>> = None;
    skip_ws(reader)?;
    if peek(reader)? != Some(b'}') {
        loop {
            let key = read_value(reader, Some(KEY_MAX_BYTES))?.ok_or_else(|| {
                ImportError::new(ErrorCode::FormatUnsupported, "missing object key")
            })?;
            let key: String = serde_json::from_slice(&key).map_err(parse_error)?;
            skip_ws(reader)?;
            expect(reader, b':')?;
            match key.as_str() {
                "messages" => {
                    if messages.is_some() {
                        return Err(ImportError::new(
                            ErrorCode::FormatUnsupported,
                            "duplicate JSON field: messages",
                        ));
                    }
                    messages = Some(parse_candidate(reader, parent, locator)?);
                }
                "conversation" => {
                    if conversation.is_some() {
                        return Err(ImportError::new(
                            ErrorCode::FormatUnsupported,
                            "duplicate JSON field: conversation",
                        ));
                    }
                    conversation = Some(parse_candidate(reader, parent, locator)?);
                }
                "items" => {
                    if items.is_some() {
                        return Err(ImportError::new(
                            ErrorCode::FormatUnsupported,
                            "duplicate JSON field: items",
                        ));
                    }
                    items = Some(parse_candidate(reader, parent, locator)?);
                }
                "usage" => {
                    if usage.is_some() {
                        return Err(ImportError::new(
                            ErrorCode::FormatUnsupported,
                            "duplicate JSON field: usage",
                        ));
                    }
                    let raw =
                        read_value(reader, Some(JSON_RECORD_MAX_BYTES))?.ok_or_else(|| {
                            ImportError::new(ErrorCode::FormatUnsupported, "missing usage value")
                        })?;
                    usage = Some(usage_value(&raw)?);
                }
                "token_count" => {
                    if token_count.is_some() {
                        return Err(ImportError::new(
                            ErrorCode::FormatUnsupported,
                            "duplicate JSON field: token_count",
                        ));
                    }
                    let raw = read_value(reader, Some(KEY_MAX_BYTES))?.ok_or_else(|| {
                        ImportError::new(ErrorCode::FormatUnsupported, "missing token_count value")
                    })?;
                    token_count = Some(scalar_value(&raw)?);
                }
                "tokenCount" => {
                    if token_count_camel.is_some() {
                        return Err(ImportError::new(
                            ErrorCode::FormatUnsupported,
                            "duplicate JSON field: tokenCount",
                        ));
                    }
                    let raw = read_value(reader, Some(KEY_MAX_BYTES))?.ok_or_else(|| {
                        ImportError::new(ErrorCode::FormatUnsupported, "missing tokenCount value")
                    })?;
                    token_count_camel = Some(scalar_value(&raw)?);
                }
                "total_tokens" => {
                    if total_tokens.is_some() {
                        return Err(ImportError::new(
                            ErrorCode::FormatUnsupported,
                            "duplicate JSON field: total_tokens",
                        ));
                    }
                    let raw = read_value(reader, Some(KEY_MAX_BYTES))?.ok_or_else(|| {
                        ImportError::new(ErrorCode::FormatUnsupported, "missing total_tokens value")
                    })?;
                    total_tokens = Some(scalar_value(&raw)?);
                }
                "totalTokens" => {
                    if total_tokens_camel.is_some() {
                        return Err(ImportError::new(
                            ErrorCode::FormatUnsupported,
                            "duplicate JSON field: totalTokens",
                        ));
                    }
                    let raw = read_value(reader, Some(KEY_MAX_BYTES))?.ok_or_else(|| {
                        ImportError::new(ErrorCode::FormatUnsupported, "missing totalTokens value")
                    })?;
                    total_tokens_camel = Some(scalar_value(&raw)?);
                }
                _ => {
                    let _ = read_value(reader, None)?;
                }
            }
            skip_ws(reader)?;
            match take(reader)? {
                Some(b',') => {
                    skip_ws(reader)?;
                    if peek(reader)? == Some(b'}') {
                        return Err(ImportError::new(
                            ErrorCode::FormatUnsupported,
                            "invalid JSON: trailing comma in object",
                        ));
                    }
                }
                Some(b'}') => break,
                Some(other) => {
                    return Err(ImportError::new(
                        ErrorCode::FormatUnsupported,
                        format!(
                            "invalid JSON: expected ',' or '}}', found '{}'",
                            other as char
                        ),
                    ));
                }
                None => {
                    return Err(ImportError::new(
                        ErrorCode::FormatUnsupported,
                        "invalid JSON: unterminated object",
                    ));
                }
            }
        }
    } else {
        let _ = take(reader)?;
    }
    let source_tokens = usage
        .and_then(|value| value)
        .or_else(|| token_count.and_then(|value| value))
        .or_else(|| token_count_camel.and_then(|value| value))
        .or_else(|| total_tokens.and_then(|value| value))
        .or_else(|| total_tokens_camel.and_then(|value| value))
        .unwrap_or(0);
    choose(messages, conversation, items, source_tokens)
}

fn ensure_end(reader: &mut StreamingReader) -> Result<(), ImportError> {
    skip_ws(reader)?;
    if peek(reader)?.is_some() {
        return Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            "invalid JSON: trailing data",
        ));
    }
    Ok(())
}

pub(super) fn prepare_json(
    reader: &mut StreamingReader,
    parent: &Path,
    locator: &str,
    builder: &mut PreparedRevisionBuilder,
    first: u8,
) -> Result<(JsonSummary, u64, &'static str), ImportError> {
    let (summary, source_tokens, fingerprint) = match first {
        b'[' => {
            let summary = parse_array(reader, locator, |event| builder.push_event(event))?;
            (summary, 0, "json-array")
        }
        b'{' => {
            let (candidate, source_tokens) = parse_object(reader, parent, locator)?;
            let summary = candidate.summary;
            candidate.spool.replay(builder)?;
            (summary, source_tokens, "json-object")
        }
        _ => {
            return Err(ImportError::new(
                ErrorCode::FormatUnsupported,
                "JSON root must be an object or array",
            ));
        }
    };
    ensure_end(reader)?;
    Ok((summary, source_tokens, fingerprint))
}

fn stream_error(error: std::io::Error) -> ImportError {
    if error.to_string().contains("file is not UTF-8") {
        ImportError::new(ErrorCode::FormatUnsupported, error.to_string())
    } else if error.kind() == std::io::ErrorKind::InvalidData
        || error.to_string().contains("JSON nesting exceeds")
    {
        ImportError::new(ErrorCode::ResourceLimit, error.to_string())
    } else {
        ImportError::new(ErrorCode::SourceNotFound, error.to_string())
    }
}

fn io_error(error: impl std::fmt::Display) -> ImportError {
    ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string())
}

fn parse_error(error: impl std::fmt::Display) -> ImportError {
    ImportError::new(ErrorCode::FormatUnsupported, error.to_string())
}

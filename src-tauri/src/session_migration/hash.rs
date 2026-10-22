//! Canonical hashing. Integers, object key order, and newlines are fixed.

use sha2::{Digest, Sha256};

use super::types::ImportedEvent;

pub struct ContentDigestBuilder {
    hash: Sha256,
    event_count: usize,
}

impl ContentDigestBuilder {
    pub fn new() -> Self {
        let mut hash = Sha256::new();
        hash.update(b"{\"events\":[");
        Self {
            hash,
            event_count: 0,
        }
    }

    pub fn push(&mut self, event: &ImportedEvent) -> Result<(), serde_json::Error> {
        if self.event_count != 0 {
            self.hash.update(b",");
        }
        let value = serde_json::to_value(event)?;
        self.hash.update(canonical_json(&value).as_bytes());
        self.event_count += 1;
        Ok(())
    }

    pub fn finish(mut self, options: &serde_json::Value) -> String {
        self.hash.update(b"],\"normalizer\":");
        self.hash.update(
            serde_json::to_string(super::types::NORMALIZER_VERSION)
                .expect("version serializes")
                .as_bytes(),
        );
        self.hash.update(b",\"options\":");
        self.hash.update(canonical_json(options).as_bytes());
        self.hash.update(b"}");
        hex::encode(self.hash.finalize())
    }
}

impl Default for ContentDigestBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

pub fn sha256_file(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read;
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(hex::encode(hash.finalize()))
}

/// Deterministic JSON: sorted object keys, compact, LF, no trailing newline.
pub fn canonical_json(value: &serde_json::Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::Null => out.push_str("null"),
        serde_json::Value::Bool(true) => out.push_str("true"),
        serde_json::Value::Bool(false) => out.push_str("false"),
        serde_json::Value::Number(n) => out.push_str(&n.to_string()),
        serde_json::Value::String(s) => {
            out.push_str(&serde_json::Value::String(s.clone()).to_string())
        }
        serde_json::Value::Array(arr) => {
            out.push('[');
            for (i, v) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(v, out);
            }
            out.push(']');
        }
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::Value::String((*k).clone()).to_string());
                out.push(':');
                write_canonical(&map[*k], out);
            }
            out.push('}');
        }
    }
}

#[cfg(test)]
pub fn content_digest(events: &[ImportedEvent], options: &serde_json::Value) -> String {
    let mut digest = ContentDigestBuilder::new();
    for event in events {
        digest.push(event).expect("event serializes");
    }
    digest.finish(options)
}

pub fn normalized_digest_from_content(
    adapter: &super::types::AdapterOutput,
    content_digest: &str,
) -> String {
    options_hash(&serde_json::json!({
        "events": content_digest,
        "relations": adapter.graph.relations, "activeLeaf": adapter.graph.active_leaf,
        "nativeSessionId": adapter.graph.native_session_id, "cwd": adapter.graph.cwd,
        "assets": adapter.assets, "outcome": adapter.outcome,
    }))
}

#[cfg(test)]
pub fn normalized_digest(
    adapter: &super::types::AdapterOutput,
    options: &serde_json::Value,
) -> String {
    normalized_digest_from_content(adapter, &content_digest(&adapter.graph.events, options))
}

pub fn options_hash(value: &serde_json::Value) -> String {
    sha256_bytes(canonical_json(value).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_key_order_is_stable() {
        let a = serde_json::json!({"b": 1, "a": 2});
        let b = serde_json::json!({"a": 2, "b": 1});
        assert_eq!(canonical_json(&a), canonical_json(&b));
        assert_eq!(canonical_json(&a), r#"{"a":2,"b":1}"#);
    }

    #[test]
    fn newline_in_string_is_escaped() {
        let v = serde_json::json!("a\nb");
        assert_eq!(canonical_json(&v), r#""a\nb""#);
    }

    #[test]
    fn incremental_hash_matches_canonical_v1_serialization() {
        let events: Vec<ImportedEvent> = serde_json::from_value(serde_json::json!([
            {"kind":"message","id":"a","ordinal":0,"spans":[],"timeOrigin":"unknown",
             "role":"user","blocks":[{"kind":"text","text":"中文\nquote \" [ ]"}],"complete":true}
        ]))
        .unwrap();
        let options = serde_json::json!({"z":1,"a":[2,3]});
        let original = serde_json::json!({"events":events,"options":options,"normalizer":super::super::types::NORMALIZER_VERSION});
        assert_eq!(
            content_digest(&events, &options),
            sha256_bytes(canonical_json(&original).as_bytes())
        );
    }
}

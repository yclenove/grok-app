//! Strict worker success page/observation parsing. Missing identity never
//! falls back to empty strings, 1, request echo, or an implied empty node list.

use serde_json::Value;

use super::{ManagedNode, ManagedObservation, ManagedPage, WorkerError};
use crate::protocol::{
    JS_MAX_SAFE_INTEGER, OBSERVATION_ARIA_CHARS, OBSERVATION_JSON_BYTES, OBSERVATION_NAME_CHARS,
    OBSERVATION_NODE_CAP, OBSERVATION_PNG_B64_CAP, OBSERVATION_ROLE_CHARS, OBSERVATION_TITLE_CHARS,
    OBSERVATION_URL_CHARS,
};

const PNG_SIGNATURE: &[u8] = &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

const ID_MAX_LEN: usize = 256;

pub fn parse_worker_success(
    status: u16,
    content_type: Option<&str>,
    body: &[u8],
) -> Result<Value, WorkerError> {
    if !(200..300).contains(&status) {
        return Err(WorkerError::invalid_response(status));
    }
    if !content_type_is_json(content_type) {
        return Err(WorkerError::invalid_response(status));
    }
    if body.len() > OBSERVATION_JSON_BYTES + crate::protocol::OBSERVATION_PNG_B64_CAP + 8192 {
        return Err(WorkerError::invalid_response(status));
    }
    let value: Value =
        serde_json::from_slice(body).map_err(|_| WorkerError::invalid_response(status))?;
    if value.get("ok") != Some(&Value::Bool(true)) {
        return Err(WorkerError::invalid_response(status));
    }
    Ok(value)
}

pub fn parse_managed_page(value: &Value) -> Result<ManagedPage, WorkerError> {
    Ok(ManagedPage {
        page_id: required_id(value, "pageId")?,
        page_generation: required_generation(value)?,
        url: optional_bounded_text(value, "url", OBSERVATION_URL_CHARS)?,
        popup: value.get("popup").and_then(Value::as_bool).unwrap_or(false),
    })
}

pub fn parse_managed_observation(value: &Value) -> Result<ManagedObservation, WorkerError> {
    let nodes = required_nodes(value)?;
    let image = parse_image(value)?;
    if value.get("textOnly") == Some(&Value::Bool(true)) && image.png_base64.is_some() {
        return Err(WorkerError::invalid_response(200));
    }
    Ok(ManagedObservation {
        page_id: required_id(value, "pageId")?,
        page_generation: required_generation(value)?,
        snapshot_id: required_id(value, "snapshotId")?,
        url: optional_bounded_text(value, "url", OBSERVATION_URL_CHARS)?,
        title: optional_bounded_text(value, "title", OBSERVATION_TITLE_CHARS)?,
        aria: optional_bounded_text(value, "aria", OBSERVATION_ARIA_CHARS)?,
        nodes,
        truncated: value
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        text_only: image.png_base64.is_none(),
        image_width: image.width,
        image_height: image.height,
        image_content_id: image.content_id,
        png_base64: image.png_base64,
        image_omitted_reason: image.omitted,
    })
}

pub fn model_visible_url(url: &str) -> String {
    url.split(['?', '#']).next().unwrap_or(url).to_string()
}

pub fn model_observation_view(obs: &ManagedObservation) -> serde_json::Value {
    serde_json::json!({
        "snapshotId": obs.snapshot_id,
        "pageGeneration": obs.page_generation,
        "url": model_visible_url(&obs.url),
        "title": obs.title,
        "aria": obs.aria,
        "nodes": obs.nodes.iter().map(|node| serde_json::json!({
            "elementRef": node.element_ref,
            "role": node.role,
            "name": node.name,
            "disabled": node.disabled,
            "truncated": node.truncated,
        })).collect::<Vec<_>>(),
        "truncated": obs.truncated,
        "textOnly": obs.text_only,
        "image": if obs.png_base64.is_some() {
            serde_json::json!({
                "width": obs.image_width,
                "height": obs.image_height,
                "contentId": obs.image_content_id,
            })
        } else {
            serde_json::Value::Null
        },
        "imageOmittedReason": obs.image_omitted_reason,
    })
}

pub fn coordinate_action_allowed(obs: &ManagedObservation) -> bool {
    !obs.text_only
        && obs.image_width > 0
        && obs.image_height > 0
        && obs.png_base64.as_ref().is_some_and(|png| !png.is_empty())
}

struct ParsedImage {
    width: u32,
    height: u32,
    content_id: String,
    png_base64: Option<String>,
    omitted: Option<String>,
}

fn parse_image(value: &Value) -> Result<ParsedImage, WorkerError> {
    let omitted = value
        .get("imageOmittedReason")
        .and_then(Value::as_str)
        .map(str::to_string);
    if let Some(b64) = value.get("pngBase64").and_then(Value::as_str) {
        if b64.len() > OBSERVATION_PNG_B64_CAP {
            return Err(WorkerError::invalid_response(200));
        }
        let bytes = decode_base64(b64).ok_or_else(|| WorkerError::invalid_response(200))?;
        let header = inspect_png(&bytes).ok_or_else(|| WorkerError::invalid_response(200))?;
        let meta = value.get("image");
        let width = meta
            .and_then(|m| m.get("width"))
            .and_then(Value::as_u64)
            .unwrap_or(header.0 as u64) as u32;
        let height = meta
            .and_then(|m| m.get("height"))
            .and_then(Value::as_u64)
            .unwrap_or(header.1 as u64) as u32;
        if width != header.0 || height != header.1 {
            return Err(WorkerError::invalid_response(200));
        }
        let content_id = meta
            .and_then(|m| m.get("contentId"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if content_id.is_empty() {
            return Err(WorkerError::invalid_response(200));
        }
        return Ok(ParsedImage {
            width,
            height,
            content_id,
            png_base64: Some(b64.to_string()),
            omitted: None,
        });
    }
    Ok(ParsedImage {
        width: 0,
        height: 0,
        content_id: String::new(),
        png_base64: None,
        omitted,
    })
}

fn inspect_png(buf: &[u8]) -> Option<(u32, u32)> {
    if buf.len() < 33 || buf.get(..8) != Some(PNG_SIGNATURE) {
        return None;
    }
    if &buf[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(buf[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(buf[20..24].try_into().ok()?);
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height))
}

fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let filtered: String = input.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if !filtered.len().is_multiple_of(4) {
        return None;
    }
    let table = |c: u8| -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let bytes = filtered.as_bytes();
    let mut out = Vec::with_capacity(filtered.len() / 4 * 3);
    let mut i = 0;
    while i < bytes.len() {
        let pad2 = bytes[i + 2] == b'=';
        let pad3 = bytes[i + 3] == b'=';
        let a = table(bytes[i])?;
        let b = table(bytes[i + 1])?;
        let c = if pad2 { 0 } else { table(bytes[i + 2])? };
        let d = if pad3 { 0 } else { table(bytes[i + 3])? };
        out.push((a << 2) | (b >> 4));
        if !pad2 {
            out.push((b << 4) | (c >> 2));
        }
        if !pad3 {
            out.push((c << 6) | d);
        }
        i += 4;
    }
    Some(out)
}

fn content_type_is_json(content_type: Option<&str>) -> bool {
    let Some(raw) = content_type else {
        return false;
    };
    let media = raw
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    media == "application/json"
}

fn required_id(value: &Value, key: &str) -> Result<String, WorkerError> {
    let Some(text) = value.get(key).and_then(Value::as_str) else {
        return Err(WorkerError::invalid_response(200));
    };
    let text = text.trim();
    if text.is_empty() || text.len() > ID_MAX_LEN {
        return Err(WorkerError::invalid_response(200));
    }
    Ok(text.to_string())
}

fn required_generation(value: &Value) -> Result<u64, WorkerError> {
    let Some(generation) = value.get("pageGeneration").and_then(Value::as_u64) else {
        return Err(WorkerError::invalid_response(200));
    };
    if !(1..=JS_MAX_SAFE_INTEGER).contains(&generation) {
        return Err(WorkerError::invalid_response(200));
    }
    Ok(generation)
}

fn optional_bounded_text(
    value: &Value,
    key: &str,
    max_chars: usize,
) -> Result<String, WorkerError> {
    let Some(raw) = value.get(key) else {
        return Ok(String::new());
    };
    let Some(text) = raw.as_str() else {
        return Err(WorkerError::invalid_response(200));
    };
    if text.chars().count() > max_chars {
        return Err(WorkerError::invalid_response(200));
    }
    Ok(text.to_string())
}

fn required_nodes(value: &Value) -> Result<Vec<ManagedNode>, WorkerError> {
    let Some(rows) = value.get("nodes").and_then(Value::as_array) else {
        return Err(WorkerError::invalid_response(200));
    };
    if rows.len() > OBSERVATION_NODE_CAP {
        return Err(WorkerError::invalid_response(200));
    }
    let mut nodes = Vec::with_capacity(rows.len());
    for row in rows {
        nodes.push(parse_node(row)?);
    }
    Ok(nodes)
}

fn parse_node(value: &Value) -> Result<ManagedNode, WorkerError> {
    let element_ref = required_id(value, "elementRef")?;
    let role = required_bounded(value, "role", OBSERVATION_ROLE_CHARS)?;
    let name = required_bounded(value, "name", OBSERVATION_NAME_CHARS)?;
    Ok(ManagedNode {
        element_ref,
        role,
        name,
        disabled: value
            .get("disabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        truncated: value
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn required_bounded(value: &Value, key: &str, max_chars: usize) -> Result<String, WorkerError> {
    let Some(text) = value.get(key).and_then(Value::as_str) else {
        return Err(WorkerError::invalid_response(200));
    };
    if text.chars().count() > max_chars {
        return Err(WorkerError::invalid_response(200));
    }
    Ok(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_obs() -> Value {
        serde_json::json!({
            "ok": true,
            "pageId": "page-1",
            "pageGeneration": 2,
            "snapshotId": "snap-1",
            "url": "https://example.test/",
            "title": "Example",
            "aria": "- button \"Go\"",
            "nodes": [{
                "elementRef": "ref-1",
                "role": "button",
                "name": "Go",
                "disabled": false
            }],
            "truncated": false,
            "textOnly": true
        })
    }

    fn err_code(result: Result<Value, WorkerError>) -> String {
        result.expect_err("must fail closed").code
    }

    #[test]
    fn two_xx_requires_top_level_ok_true() {
        let body = serde_json::to_vec(&valid_obs()).unwrap();
        parse_worker_success(200, Some("application/json"), &body).unwrap();
        for value in [
            serde_json::json!({"pageId":"page-1","pageGeneration":1,"snapshotId":"s","nodes":[]}),
            serde_json::json!({"ok": false, "pageId":"page-1","pageGeneration":1,"snapshotId":"s","nodes":[]}),
            serde_json::json!({"ok": "true", "pageId":"page-1","pageGeneration":1,"snapshotId":"s","nodes":[]}),
            serde_json::json!({"ok": 1, "pageId":"page-1","pageGeneration":1,"snapshotId":"s","nodes":[]}),
            serde_json::json!({"ok": null, "pageId":"page-1","pageGeneration":1,"snapshotId":"s","nodes":[]}),
        ] {
            let bytes = serde_json::to_vec(&value).unwrap();
            assert_eq!(
                err_code(parse_worker_success(200, Some("application/json"), &bytes)),
                "invalid_worker_response"
            );
        }
        assert_eq!(
            err_code(parse_worker_success(
                200,
                Some("text/plain"),
                &serde_json::to_vec(&valid_obs()).unwrap()
            )),
            "invalid_worker_response"
        );
    }

    #[test]
    fn missing_identity_does_not_fallback() {
        let mut missing_snapshot = valid_obs();
        missing_snapshot
            .as_object_mut()
            .unwrap()
            .remove("snapshotId");
        assert!(parse_managed_observation(&missing_snapshot).is_err());

        let mut missing_nodes = valid_obs();
        missing_nodes.as_object_mut().unwrap().remove("nodes");
        assert!(parse_managed_observation(&missing_nodes).is_err());

        let mut empty_page = valid_obs();
        empty_page["pageId"] = Value::String(String::new());
        assert!(parse_managed_observation(&empty_page).is_err());

        let mut gen_one_fallback = valid_obs();
        gen_one_fallback
            .as_object_mut()
            .unwrap()
            .remove("pageGeneration");
        assert!(parse_managed_page(&gen_one_fallback).is_err());
        assert!(parse_managed_observation(&gen_one_fallback).is_err());

        let mut gen_zero = valid_obs();
        gen_zero["pageGeneration"] = Value::from(0);
        assert!(parse_managed_page(&gen_zero).is_err());

        let mut gen_overflow = valid_obs();
        gen_overflow["pageGeneration"] = Value::from(JS_MAX_SAFE_INTEGER + 1);
        assert!(parse_managed_page(&gen_overflow).is_err());

        let mut echo = valid_obs();
        echo.as_object_mut().unwrap().remove("pageId");
        assert!(parse_managed_observation(&echo).is_err());
        assert!(parse_managed_page(&echo).is_err());
    }

    #[test]
    fn observation_caps_fail_closed() {
        let mut too_many = valid_obs();
        let nodes = (0..=OBSERVATION_NODE_CAP)
            .map(|i| {
                serde_json::json!({
                    "elementRef": format!("ref-{i}"),
                    "role": "button",
                    "name": format!("{i}")
                })
            })
            .collect::<Vec<_>>();
        too_many["nodes"] = Value::Array(nodes);
        assert!(parse_managed_observation(&too_many).is_err());

        let mut long_role = valid_obs();
        long_role["nodes"][0]["role"] = Value::String("r".repeat(OBSERVATION_ROLE_CHARS + 1));
        assert!(parse_managed_observation(&long_role).is_err());

        let parsed = parse_managed_observation(&valid_obs()).unwrap();
        assert_eq!(parsed.snapshot_id, "snap-1");
        assert_eq!(parsed.nodes.len(), 1);
        assert_eq!(parsed.nodes[0].element_ref, "ref-1");
        assert_eq!(parsed.nodes[0].role, "button");
    }

    const PNG_1X1_B64: &str =
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQD3A+hkAAAAAElFTkSuQmCC";

    #[test]
    fn png_signature_geometry_and_model_text_omit_base64() {
        let mut value = valid_obs();
        value["textOnly"] = Value::Bool(false);
        value["pngBase64"] = Value::String(PNG_1X1_B64.into());
        value["image"] = serde_json::json!({"width":1,"height":1,"contentId":"img-1"});
        let parsed = parse_managed_observation(&value).unwrap();
        assert!(!parsed.text_only);
        assert_eq!(parsed.image_width, 1);
        assert_eq!(parsed.image_height, 1);
        assert_eq!(parsed.png_base64.as_deref(), Some(PNG_1X1_B64));
        assert!(coordinate_action_allowed(&parsed));
        let model = model_observation_view(&parsed).to_string();
        assert!(!model.contains(PNG_1X1_B64));
        assert!(!model.contains("pngBase64"));
        assert!(!model.contains("pageId"));
        assert!(!model.contains("page-1"));

        let mut bad = value.clone();
        bad["pngBase64"] = Value::String("not-a-png".into());
        assert!(parse_managed_observation(&bad).is_err());

        let mut mismatch = value.clone();
        mismatch["image"]["width"] = Value::from(9);
        assert!(parse_managed_observation(&mismatch).is_err());

        let text_only = parse_managed_observation(&valid_obs()).unwrap();
        assert!(text_only.text_only);
        assert!(!coordinate_action_allowed(&text_only));
    }
}

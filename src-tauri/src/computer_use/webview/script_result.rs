//! Script replies may contain page-controlled data. Never echo them into errors/traces.

const MAX_SCRIPT_REPLY: usize = 512 * 1024;

pub(super) fn parse(raw: &str) -> Result<serde_json::Value, String> {
    if raw.len() > MAX_SCRIPT_REPLY {
        return Err("WebView script reply exceeds the size limit".into());
    }
    let value: serde_json::Value =
        serde_json::from_str(raw.trim()).map_err(|_| "WebView script reply is not valid JSON")?;
    // WebView implementations can return one JSON-encoded string envelope.
    let value = if let Some(inner) = value.as_str() {
        serde_json::from_str(inner)
            .map_err(|_| "WebView script reply envelope is not valid JSON")?
    } else {
        value
    };
    if !value.is_object() {
        return Err("WebView script reply must be an object".into());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_object_or_one_native_string_envelope() {
        let object = serde_json::json!({"ok": true});
        assert_eq!(parse(&object.to_string()).unwrap(), object);
        let envelope = serde_json::to_string(&object.to_string()).unwrap();
        assert_eq!(parse(&envelope).unwrap(), object);
    }

    #[test]
    fn malformed_and_oversized_replies_never_expose_page_content() {
        let secret = "PRIVATE_INPUT_SENTINEL";
        for raw in [
            format!("{{{secret}"),
            serde_json::to_string(&format!("{{{secret}")).unwrap(),
            serde_json::json!({"input": format!("{secret}{}", "x".repeat(MAX_SCRIPT_REPLY))})
                .to_string(),
        ] {
            let error = parse(&raw).unwrap_err();
            assert!(!error.contains(secret));
            assert!(error.len() < 100);
        }
    }

    #[test]
    fn rejects_scalar_array_and_recursive_string_envelopes() {
        for raw in ["null", "42", "true", "[]", "\"false\"", "\"\\\"{}\\\"\""] {
            assert!(parse(raw).is_err());
        }
    }
}

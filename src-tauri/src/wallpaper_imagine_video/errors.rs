//! Only interpret the upstream tool error envelope, never model text or response bodies.

use serde_json::Value;

pub(super) fn tool_failure(update: &Value, tool: &str) -> &'static str {
    let Some(output) = update.get("rawOutput") else {
        return "imagine_failed";
    };
    if output.get("error").and_then(Value::as_str) != Some("tool_execution_failed") {
        return "imagine_failed";
    }
    let Some(message) = output.get("message").and_then(Value::as_str) else {
        return "imagine_failed";
    };
    // Grok Build's ACP adapter drops ToolError.details and retains Display only.
    // Match the tool-owned prefix and status, ignoring the untrusted HTTP body.
    let prefixes: &[&str] = match tool {
        "image_gen" => &["Image generation failed with HTTP "],
        "image_edit" => &["Image edit failed with HTTP "],
        "image_to_video" => &[
            "Video generation failed with HTTP ",
            "Video poll failed with HTTP ",
        ],
        _ => return "imagine_failed",
    };
    let Some(status) = prefixes
        .iter()
        .find_map(|prefix| message.strip_prefix(prefix))
    else {
        return "imagine_failed";
    };
    let Some((digits, suffix)) = status.split_at_checked(3) else {
        return "imagine_failed";
    };
    if !digits.bytes().all(|c| c.is_ascii_digit())
        || !(suffix.starts_with(' ') || suffix.starts_with(':'))
    {
        return "imagine_failed";
    }
    match digits.parse::<u16>() {
        Ok(401) => "auth_required",
        Ok(403) => "imagine_access_denied",
        Ok(408 | 504) => "timeout",
        Ok(429) => "imagine_rate_limited",
        Ok(400 | 402 | 404 | 413 | 415 | 422) => "imagine_request_rejected",
        Ok(500..=599) => "imagine_upstream_failed",
        _ => "imagine_failed",
    }
}

//! Display metadata from transcript records, never from tool outputs or sidechains.

use serde_json::Value;

fn short_title<'a>(texts: impl Iterator<Item = &'a str>) -> Option<String> {
    let title: String = texts
        .flat_map(str::split_whitespace)
        .flat_map(|word| std::iter::once(' ').chain(word.chars().filter(|c| !c.is_control())))
        .skip(1)
        .take(80)
        .collect();
    let title = title.trim_end();
    (!title.is_empty()).then(|| title.to_owned())
}

pub(super) fn prompt_title(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => short_title(std::iter::once(text.as_str())),
        Value::Array(blocks) => short_title(blocks.iter().filter_map(|block| {
            match block.get("type").and_then(Value::as_str).unwrap_or("text") {
                "text" | "input_text" | "output_text" => block.get("text").and_then(Value::as_str),
                _ => None,
            }
        })),
        _ => None,
    }
}

fn named_title(record: &Value, key: &str) -> Option<String> {
    record
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// Claude's last user-set title wins over generated titles. The prompt is
/// only a fallback; an empty later metadata record must not erase a title.
#[derive(Default)]
pub(crate) struct ClaudeTitles {
    custom: Option<String>,
    generated: Option<String>,
    last_prompt: Option<String>,
    summary: Option<String>,
    first_prompt: Option<String>,
}

impl ClaudeTitles {
    pub(crate) fn observe(&mut self, record: &Value) {
        if record.get("isSidechain").and_then(Value::as_bool) == Some(true)
            || record.get("isMeta").and_then(Value::as_bool) == Some(true)
        {
            return;
        }
        for (field, target) in [
            ("customTitle", &mut self.custom),
            ("aiTitle", &mut self.generated),
            ("lastPrompt", &mut self.last_prompt),
            ("summary", &mut self.summary),
        ] {
            if let Some(title) = named_title(record, field) {
                *target = Some(title);
            }
        }
        if self.first_prompt.is_none()
            && record.get("type").and_then(Value::as_str) == Some("user")
            && record.get("isCompactSummary").and_then(Value::as_bool) != Some(true)
        {
            self.first_prompt = record.pointer("/message/content").and_then(prompt_title);
        }
    }

    pub(crate) fn finish(self) -> Option<String> {
        self.custom
            .or(self.generated)
            .or(self.last_prompt)
            .or(self.summary)
            .or(self.first_prompt)
    }
}

use std::collections::HashSet;

use serde_json::{json, Value};
use url::Url;

use super::{SourcePage, MAX_WEB_SEARCH_CALLS, PAGES_PER_LANE};
use crate::wallpaper_responses_client::{self, ErrorKind};

pub(super) fn parse_source_pages(value: &Value) -> Vec<SourcePage> {
    let Some(raw_pages) = value.get("pages").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let mut pages = Vec::new();
    for raw in raw_pages.iter().take(PAGES_PER_LANE * 2) {
        let Some(url) = raw
            .get("url")
            .and_then(Value::as_str)
            .and_then(safe_page_url)
        else {
            continue;
        };
        if !seen.insert(url.clone()) {
            continue;
        }
        pages.push(SourcePage {
            url,
            title: raw
                .get("title")
                .and_then(Value::as_str)
                .and_then(clean_model_text),
            summary: raw
                .get("summary")
                .and_then(Value::as_str)
                .and_then(clean_model_text),
        });
        if pages.len() == PAGES_PER_LANE {
            break;
        }
    }
    pages
}

pub(super) fn validate_web_search_tool_calls(output: &[Value]) -> Result<u32, (ErrorKind, u32)> {
    let calls = wallpaper_responses_client::count_tool_calls(output, "web_search");
    match calls {
        0 => Err((ErrorKind::ToolNotCalled, calls)),
        1..=MAX_WEB_SEARCH_CALLS => Ok(calls),
        _ => Err((ErrorKind::ToolBudgetExceeded, calls)),
    }
}

fn safe_page_url(raw: &str) -> Option<String> {
    let url = Url::parse(raw.trim()).ok()?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    url.host_str()?;
    Some(url.to_string())
}

fn clean_model_text(value: &str) -> Option<String> {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if value.is_empty() {
        None
    } else {
        Some(value.chars().take(200).collect())
    }
}

pub(super) fn responses_request(query: &str, exclusions: &[String], lane_index: usize) -> Value {
    json!({
        "model": wallpaper_responses_client::MODEL,
        "input": responses_prompt(query, exclusions, lane_index),
        "tools": [{ "type": "web_search" }],
        "tool_choice": "auto",
        "max_tool_calls": MAX_WEB_SEARCH_CALLS,
        "reasoning": {
            "effort": wallpaper_responses_client::EFFORT,
            "summary": "concise"
        },
        "text": {
            "format": {
                "type": "json_schema",
                "name": "wallpaper_source_pages",
                "strict": true,
                "schema": {
                    "type": "object",
                    "properties": {
                        "pages": {
                            "type": "array",
                            "maxItems": PAGES_PER_LANE,
                            "items": {
                                "type": "object",
                                "properties": {
                                    "url": { "type": "string" },
                                    "title": { "type": "string" },
                                    "summary": { "type": "string" }
                                },
                                "required": ["url", "title", "summary"],
                                "additionalProperties": false
                            }
                        }
                    },
                    "required": ["pages"],
                    "additionalProperties": false
                }
            }
        },
        "store": false
    })
}

pub(super) fn responses_prompt(query: &str, exclusions: &[String], lane_index: usize) -> String {
    let lane = match lane_index {
        1 => "Primary lane: find the strongest direct photographic or visual interpretation.",
        2 => "Bilingual variation lane: retain useful original-language terms, translate the topic into concise English search terms, and search complementary composition, lighting, season, or cultural variants in both forms.",
        _ => "Load-more lane: use fresh long-tail and bilingual variants, avoiding the initial results.",
    };
    let exclusions = if exclusions.is_empty() {
        "none".to_string()
    } else {
        exclusions.join(",")
    };
    format!(
        r#"Find public webpages that visibly publish high-quality still images suitable for desktop wallpaper.

User topic: {query}
Strategy: {lane}
Opaque source ids already shown: {exclusions}

Use one web_search call when it can find enough real pages. If it cannot, use at most {MAX_WEB_SEARCH_CALLS} calls total and stop as soon as you have {PAGES_PER_LANE} distinct source pages. Do not stop at one or two pages when more real matches are available, but return fewer rather than inventing a URL. Return real HTTPS source webpages, not image CDN URLs, search-result pages, social login walls, or pages that require authentication. Prefer pages whose initial HTML exposes an original or high-resolution primary image through og:image, twitter:image, or JSON-LD without JavaScript, cookies, or anti-bot challenges. Prefer photographer portfolios, editorial photo pages, museums, public institutions, and image-detail pages with a clear primary image. Skip Google/Bing image result pages, watermarked or paid-stock previews, stock index pages without a specific image, memes, screenshots, text cards, ads, and low-resolution thumbnails. Return metadata only."#
    )
}

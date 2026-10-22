//! Host-origin metadata belongs to a single send, never a session-global slot.
//!
//! A cancelled, rejected or queued send cannot mark an intervening user turn.
//! Durable external queue entries are marked when actually drained, not when
//! enqueued. Both external and PR-monitor origins render as `auto_wake`.

pub const MARKER: &str = "auto_wake";

pub(crate) struct SendPrompt {
    pub text: String,
    pub origin: Option<&'static str>,
}

impl From<String> for SendPrompt {
    fn from(text: String) -> Self {
        Self { text, origin: None }
    }
}

pub(crate) fn marker(origin: Option<&str>) -> Option<String> {
    origin
        .filter(|origin| !origin.trim().is_empty())
        .map(|_| MARKER.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_is_explicit_not_derived_from_text() {
        for origin in ["external", "pr_monitor"] {
            assert_eq!(marker(Some(origin)).as_deref(), Some(MARKER));
        }
        let user = SendPrompt::from("the same automatic follow-up text".to_string());
        assert!(marker(user.origin).is_none());
        assert!(marker(Some("  ")).is_none());
    }

    #[test]
    fn dropped_or_interleaved_sends_cannot_mark_a_later_user_prompt() {
        let injected = SendPrompt {
            text: "same".into(),
            origin: Some("pr_monitor"),
        };
        let intervening = SendPrompt::from("same".to_string());
        assert!(marker(intervening.origin).is_none());
        drop(injected);
        assert!(marker(SendPrompt::from("same".to_string()).origin).is_none());
    }
}

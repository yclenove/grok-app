//! Decode cached fields independently of COM. Unsupported optional fields stay empty.
use super::{Patterns, Properties};
use gcu_core::model::{NodeValue, Rect};

/// Typed cached-field reader. Native failures are recorded as metadata by the source.
/// Getters never fetch current values or pattern interfaces.
pub trait Fields {
    /// Cached boolean; absence must not grant a capability.
    fn boolean(&mut self, id: i32) -> Option<bool>;
    /// Cached integer.
    fn integer(&mut self, id: i32) -> Option<i32>;
    /// Cached string.
    fn string(&mut self, id: i32) -> Option<String>;
    /// Cached finite number.
    fn number(&mut self, id: i32) -> Option<f64>;
    /// Cached physical screen rectangle.
    fn frame(&mut self) -> Option<Rect>;
    /// Cached runtime identity.
    fn runtime_id(&mut self) -> Option<Vec<i32>>;
    /// First native field failure: call, HRESULT/type and elapsed time, without payloads.
    fn failure(&self) -> Option<String>;
}

/// Decode one property bag. Optional field errors do not hide unread subtrees.
/// Unknown password status remains incomplete, so ADR-0004 still refuses images.
pub fn decode(r: &mut impl Fields) -> Properties {
    let password = r.boolean(30019);
    // Unknown IsPassword is privacy-critical: suppress values and screenshots.
    let control_type = r.integer(30003);
    let patterns = Patterns {
        invoke: r.boolean(30031).unwrap_or(false),
        toggle: r.boolean(30041).unwrap_or(false),
        selection_item: r.boolean(30036).unwrap_or(false),
        expand_collapse: r.boolean(30028).unwrap_or(false),
        value: r.boolean(30043).unwrap_or(false),
        range_value: r.boolean(30033).unwrap_or(false),
        scroll: r.boolean(30034).unwrap_or(false),
        scroll_item: r.boolean(30035).unwrap_or(false),
        text: r.boolean(30040).unwrap_or(false),
        legacy: r.boolean(30090).unwrap_or(false),
    };
    let name = r.string(30005);
    let identifier = r.string(30011);
    let class_name = r.string(30012);
    let framework = r.string(30024);
    let aria_role = r.string(30101);
    let help = r.string(30013);
    let enabled = r.boolean(30010).unwrap_or(false);
    let focusable = r.boolean(30009).unwrap_or(false);
    let focused = r.boolean(30008).unwrap_or(false);
    let offscreen = r.boolean(30022).unwrap_or(false);
    let required = r.boolean(30025).unwrap_or(false);
    let modal = r.boolean(30077).unwrap_or(false);
    let selected = r.boolean(30079).unwrap_or(false);
    let toggle_state = if patterns.toggle {
        r.integer(30086)
    } else {
        None
    };
    let expand_state = if patterns.expand_collapse {
        r.integer(30070)
    } else {
        None
    };
    let read_only = if patterns.value {
        r.boolean(30046)
    } else if patterns.range_value {
        r.boolean(30048)
    } else {
        None
    };
    let value = if password != Some(false) {
        None
    } else if patterns.value {
        r.string(30045).map(NodeValue::Text)
    } else if patterns.range_value {
        match (r.number(30047), r.number(30049), r.number(30050)) {
            (Some(value), Some(min), Some(max)) => Some(NodeValue::Range { value, min, max }),
            _ => None,
        }
    } else {
        None
    };
    let frame = r.frame();
    let runtime_id = r.runtime_id().unwrap_or_default();
    Properties {
        runtime_id,
        control_type: control_type.unwrap_or(0),
        aria_role,
        name,
        identifier,
        class_name,
        framework,
        help,
        frame,
        enabled,
        focusable,
        focused,
        offscreen,
        password: password.unwrap_or(true),
        required,
        modal,
        selected,
        toggle_state,
        expand_state,
        read_only,
        value,
        patterns,
        partial: password.is_none(),
        read_failure: r.failure().or_else(|| {
            password
                .is_none()
                .then(|| "GetCachedPropertyValueEx(30019): IsPassword absent".into())
        }),
    }
}

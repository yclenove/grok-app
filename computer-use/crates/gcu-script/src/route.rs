//! Map each `cu.*` call onto the same router entry as a single tool and batch.

use gcu_core::{
    action::{ActionRouter, ActionState, ToolCall, ToolOutcome},
    session::Session,
};
use gcu_protocol::{
    result::Outcome,
    tools::{
        BatchStep, ClickArgs, Condition, ElementCondition, GetAppStateParams, MouseButton,
        PressKeyArgs, ScreenshotMode, SetValueArgs, TextLimit, TypeTextArgs, WaitForArgs,
    },
    ErrorCode, ToolError,
};

use crate::rpc::{ElementView, Op, Reply, StateView};

const ROLES: &[&str] = &[
    "standard window",
    "pop up button",
    "progress bar",
    "search field",
    "scroll area",
    "scroll bar",
    "split group",
    "static text",
    "combo box",
    "menu item",
    "text field",
    "text area",
    "tree item",
    "check box",
    "list item",
    "tab list",
    "web area",
    "document",
    "toolbar",
    "popover",
    "unknown",
    "dialog",
    "button",
    "canvas",
    "column",
    "group",
    "heading",
    "image",
    "link",
    "list",
    "menu",
    "menu bar",
    "outline",
    "radio",
    "sheet",
    "slider",
    "stepper",
    "switch",
    "table",
    "tree",
    "window",
    "cell",
    "row",
    "tab",
];

/// Whether a wire code ends the script the way [`gcu_core::batch`] ends a batch.
///
/// Blocking: none. Timeout: none. Cancellation: none.
#[must_use]
pub fn is_hard_code(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::StoppedByUser
            | ErrorCode::ApprovalDenied
            | ErrorCode::ApprovalTimeout
            | ErrorCode::AppDenied
            | ErrorCode::StaleElement
            | ErrorCode::StaleGeometry
            | ErrorCode::StaleScreenshot
            | ErrorCode::Timeout
            | ErrorCode::WindowGone
            | ErrorCode::WindowNotFound
            | ErrorCode::WindowMinimized
    )
}

/// Convert one router outcome. Unknown and hard stops set the latch bits.
///
/// Blocking: none. Timeout: none. Cancellation: none. Does not replay.
#[must_use]
pub fn outcome_to_response(out: &ToolOutcome) -> Reply {
    let unknown = out.action.outcome == Outcome::Unknown;
    let code = out.error.as_ref().map(|error| error.code);
    let hard = unknown || code.is_some_and(is_hard_code);
    Reply {
        code: code
            .map(|code| code.as_str().to_owned())
            .unwrap_or_default(),
        message: out
            .error
            .as_ref()
            .map(|error| error.message.clone())
            .unwrap_or_default(),
        hard_stop: hard,
        unknown,
        state: out.state.as_ref().map(view_of_action),
    }
}

/// Perform one script op on a router whose caller already holds `session`'s queue.
///
/// Uses [`ActionRouter::observe_enqueued`] and [`ActionRouter::execute_enqueued`].
/// Calling [`ActionRouter::observe`] or [`ActionRouter::execute`] here would deadlock.
/// Native calls keep the 5s ceiling. Unknown results are returned, not retried.
///
/// Blocking: the router call. Timeout: native 5s, `wait_for` up to its own horizon.
/// Cancellation: the router observes Stop and session close.
pub async fn dispatch(router: &ActionRouter, session: &Session, call_id: &str, op: Op) -> Reply {
    match op {
        Op::App { name } => observe(router, session, name, false).await,
        Op::State { app, screenshot } => observe(router, session, app, screenshot).await,
        Op::SetValue { app, index, value } => {
            act(
                router,
                session,
                call_id,
                app,
                BatchStep::SetValue(SetValueArgs {
                    element_index: index,
                    value,
                }),
            )
            .await
        }
        Op::Press { app, key } => {
            let args = PressKeyArgs { key };
            if args.validate().is_err() {
                return Reply::fail("invalid_key", "invalid key", false, false);
            }
            act(router, session, call_id, app, BatchStep::PressKey(args)).await
        }
        Op::Click { app, index } => {
            act(
                router,
                session,
                call_id,
                app,
                BatchStep::Click(ClickArgs {
                    element_index: Some(index),
                    x: None,
                    y: None,
                    screenshot_id: None,
                    click_count: 1,
                    mouse_button: MouseButton::Left,
                }),
            )
            .await
        }
        Op::TypeText { app, text, index } => {
            act(
                router,
                session,
                call_id,
                app,
                BatchStep::TypeText(TypeTextArgs {
                    text,
                    element_index: index,
                }),
            )
            .await
        }
        Op::WaitElement {
            app,
            role,
            name,
            timeout_ms,
        } => {
            let timeout_ms = timeout_ms.clamp(1, 30_000);
            act(
                router,
                session,
                call_id,
                app,
                BatchStep::WaitFor(WaitForArgs {
                    until: Condition::Element(ElementCondition {
                        role: normalize_role(&role),
                        name,
                    }),
                    timeout_ms,
                }),
            )
            .await
        }
        Op::WaitText {
            app,
            text,
            present,
            timeout_ms,
        } => {
            let timeout_ms = timeout_ms.clamp(1, 30_000);
            let until = if present {
                Condition::TextPresent(text)
            } else {
                Condition::TextAbsent(text)
            };
            act(
                router,
                session,
                call_id,
                app,
                BatchStep::WaitFor(WaitForArgs { until, timeout_ms }),
            )
            .await
        }
    }
}

async fn observe(router: &ActionRouter, session: &Session, app: String, screenshot: bool) -> Reply {
    let params = GetAppStateParams {
        app,
        window: None,
        include_screenshot: ScreenshotMode::Explicit(screenshot),
        text_limit: TextLimit::default(),
        max_tree_nodes: 1200,
        reason: None,
    };
    match router.observe_enqueued(session, params).await {
        Ok(state) => Reply::clear(Some(view_of_action(&state))),
        Err(error) => error_reply(&error),
    }
}

async fn act(
    router: &ActionRouter,
    session: &Session,
    call_id: &str,
    app: String,
    action: BatchStep,
) -> Reply {
    if let Err(error) = action.validate() {
        return Reply::fail("capability_unavailable", error.0, false, false);
    }
    let call = ToolCall::new(call_id, app, action);
    outcome_to_response(&router.execute_enqueued(session, call).await)
}

fn error_reply(error: &ToolError) -> Reply {
    Reply {
        code: error.code.as_str().to_owned(),
        message: error.message.clone(),
        hard_stop: is_hard_code(error.code),
        unknown: false,
        state: error.state.as_ref().map(|state| StateView {
            text: String::new(),
            focused: state.focused,
            elements: state
                .elements
                .iter()
                .flatten()
                .map(|element| ElementView {
                    index: element.index,
                    role: element.role.clone(),
                    name: element.name.clone().unwrap_or_default(),
                })
                .collect(),
        }),
    }
}

fn view_of_action(state: &ActionState) -> StateView {
    let elements = if let Some(elements) = &state.state.elements {
        elements
            .iter()
            .map(|element| ElementView {
                index: element.index,
                role: element.role.clone(),
                name: element.name.clone().unwrap_or_default(),
            })
            .collect()
    } else {
        parse_elements(&state.text)
    };
    StateView {
        text: state.text.clone(),
        focused: state.state.focused,
        elements,
    }
}

fn normalize_role(role: &str) -> String {
    role.trim().to_ascii_lowercase().replace(' ', "_")
}

/// Parse rendered lines of the form `{index} {role} ({states})? {name}?`.
pub(crate) fn parse_elements(text: &str) -> Vec<ElementView> {
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let Some((index, rest)) = split_index(trimmed) else {
            continue;
        };
        let (body, name_after) = split_states(rest);
        let (role, name) = split_role(body);
        let name = if name.is_empty() { name_after } else { &name };
        out.push(ElementView {
            index,
            role,
            name: name.trim().to_owned(),
        });
    }
    out
}

fn split_index(line: &str) -> Option<(u32, &str)> {
    let (head, rest) = line.split_once(' ')?;
    let index = head.parse().ok()?;
    Some((index, rest.trim()))
}

fn split_states(rest: &str) -> (&str, &str) {
    let Some(start) = rest.find(" (") else {
        return (rest, "");
    };
    let Some(end) = rest[start + 2..].find(')') else {
        return (rest, "");
    };
    let end = start + 2 + end;
    let body = rest[..start].trim();
    let name = rest[end + 1..].trim();
    (body, name)
}

fn split_role(body: &str) -> (String, String) {
    let lower = body.to_ascii_lowercase();
    let mut best: Option<&str> = None;
    for role in ROLES {
        if lower.starts_with(role) && best.is_none_or(|current| role.len() > current.len()) {
            let boundary = body.is_char_boundary(role.len());
            let next = body.get(role.len()..).unwrap_or("");
            if boundary && (next.is_empty() || next.starts_with(' ')) {
                best = Some(role);
            }
        }
    }
    match best {
        Some(role) => {
            let name = body.get(role.len()..).unwrap_or("").trim();
            (role.to_owned(), name.to_owned())
        }
        None => (body.to_owned(), String::new()),
    }
}

/// JSON object the prelude parses. Not used as an eval string.
pub(crate) fn state_json(state: &StateView) -> String {
    let mut elements = String::from("[");
    for (i, element) in state.elements.iter().enumerate() {
        if i > 0 {
            elements.push(',');
        }
        elements.push_str(&format!(
            "{{\"index\":{},\"role\":{},\"name\":{}}}",
            element.index,
            json_string(&element.role),
            json_string(&element.name)
        ));
    }
    elements.push(']');
    let focused = match state.focused {
        Some(index) => index.to_string(),
        None => "null".to_owned(),
    };
    format!(
        "{{\"text\":{},\"focused\":{},\"elements\":{elements}}}",
        json_string(&state.text),
        focused
    )
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(ch))),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

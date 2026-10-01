//! Closed-set action proposal inspired by TypeSafe Jev.
//!
//! `prepare` builds one observation batch. `decide` maps a Choice response to a
//! proposal or a stop. `choose` posts that batch to TypeSafe only when
//! `TYPESAFE_API_KEY` is set. None of these functions click, type, or authorize.
//! Text is taken only from caller-supplied bindings. The API key is never copied
//! into errors, proposals, or returned state.

use serde_json::{Map, Value};

use crate::adapter::{Capabilities, SurfaceKind};
use crate::protocol::{
    ActionKind, ActionRequest, ActionTarget, Observation, ObservationNode, PROTOCOL_VERSION,
    TEXT_MAX_CHARS,
};

pub const DEFAULT_MIN_CONFIDENCE: f64 = 0.6;
const MAX_ELEMENTS: usize = 32;
const GOAL_CHARS: usize = 500;
const NAME_CHARS: usize = 80;
const SCROLL_DELTA: i64 = 360;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundText {
    /// Replace the control value. Maps to `set_value`.
    Replace,
    /// Insert the literal. Maps to `type_text`.
    Insert,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextBinding {
    pub element_ref: String,
    pub text: String,
    pub mode: BoundText,
}

#[derive(Debug, Clone)]
pub struct ChoiceInput<'a> {
    pub goal: &'a str,
    pub surface: SurfaceKind,
    pub observation: &'a Observation,
    pub bindings: &'a [TextBinding],
    pub min_confidence: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChoiceBatch {
    pub state: String,
    pub questions: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    pub action: ActionKind,
    pub element_ref: String,
    pub parameters: Value,
    pub confidence: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChoiceVerdict {
    Propose(Proposal),
    Stop(ChoiceStop),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceStop {
    Done,
    Blocked,
    LowConfidence,
    Unoffered,
    MissingTarget,
    MissingText,
    AmbiguousText,
    NoOperation,
    Malformed,
    Unavailable,
}

#[derive(Debug, Clone)]
pub struct ActionIdentity {
    pub action_id: String,
    pub run_id: String,
    pub target_id: String,
    pub target_generation: u64,
    pub snapshot_id: String,
    pub geometry_revision: u64,
}

struct Offer {
    operations: Vec<&'static str>,
    click: Vec<String>,
    set_value: Vec<String>,
    type_text: Vec<String>,
    scroll: Vec<String>,
    wait: Vec<String>,
    nodes: Vec<NodeLine>,
}

struct NodeLine {
    node_ref: String,
    role: String,
    name: String,
    actions: String,
}

pub fn prepare(input: &ChoiceInput<'_>) -> Result<ChoiceBatch, ChoiceStop> {
    let offer = offer(input)?;
    Ok(ChoiceBatch {
        state: render_state(input, &offer),
        questions: render_questions(&offer),
    })
}

pub fn decide(input: &ChoiceInput<'_>, response: &Value) -> ChoiceVerdict {
    let offer = match offer(input) {
        Ok(offer) => offer,
        Err(stop) => return ChoiceVerdict::Stop(stop),
    };
    let Some(answers) = response
        .get("answers")
        .and_then(Value::as_object)
        .or_else(|| response.as_object())
    else {
        return ChoiceVerdict::Stop(ChoiceStop::Malformed);
    };
    let Some(operation) = choice_of(answers, "operation") else {
        return ChoiceVerdict::Stop(ChoiceStop::Malformed);
    };
    if !offer.operations.contains(&operation.choice) {
        return ChoiceVerdict::Stop(ChoiceStop::Unoffered);
    }
    if operation.confidence < input.min_confidence {
        return ChoiceVerdict::Stop(ChoiceStop::LowConfidence);
    }
    match operation.choice {
        "done" => ChoiceVerdict::Stop(ChoiceStop::Done),
        "blocked" => ChoiceVerdict::Stop(ChoiceStop::Blocked),
        "click" => propose_target(
            input,
            answers,
            "click_target",
            &offer.click,
            ActionKind::Click,
            Value::Object(Map::new()),
        ),
        "set_value" => propose_text(
            input,
            answers,
            "set_value_target",
            &offer.set_value,
            BoundText::Replace,
        ),
        "type_text" => propose_text(
            input,
            answers,
            "type_text_target",
            &offer.type_text,
            BoundText::Insert,
        ),
        "scroll_up" => propose_scroll(input, answers, &offer.scroll, -SCROLL_DELTA),
        "scroll_down" => propose_scroll(input, answers, &offer.scroll, SCROLL_DELTA),
        "wait" => propose_wait(input, answers, &offer),
        _ => ChoiceVerdict::Stop(ChoiceStop::Unoffered),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceptedStep {
    Click { element_ref: String },
    ReplaceText { element_ref: String, text: String },
    Stop(ChoiceStop),
    Unsupported,
}

/// Classify a decision for execution. A set_value answer is never a click.
pub fn accept_step(verdict: &ChoiceVerdict, caller_text: &str) -> AcceptedStep {
    match verdict {
        ChoiceVerdict::Stop(stop) => AcceptedStep::Stop(*stop),
        ChoiceVerdict::Propose(proposal) => match proposal.action {
            ActionKind::Click => AcceptedStep::Click {
                element_ref: proposal.element_ref.clone(),
            },
            ActionKind::SetValue | ActionKind::TypeText => {
                let text = proposal
                    .parameters
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if text != caller_text {
                    AcceptedStep::Unsupported
                } else {
                    AcceptedStep::ReplaceText {
                        element_ref: proposal.element_ref.clone(),
                        text: text.to_string(),
                    }
                }
            }
            _ => AcceptedStep::Unsupported,
        },
    }
}

/// Point a captured Choice at an element from the current observation.
/// The operation and confidence stay as captured. Only the named target moves.
pub fn rebind_choice_target(mut response: Value, from: &str, to: &str) -> Value {
    let Some(answers) = response.get_mut("answers").and_then(Value::as_object_mut) else {
        return response;
    };
    for answer in answers.values_mut() {
        let Some(obj) = answer.as_object_mut() else {
            continue;
        };
        if obj.get("choice").and_then(Value::as_str) == Some(from) {
            obj.insert("choice".into(), Value::String(to.to_string()));
        }
        if let Some(probabilities) = obj.get_mut("probabilities").and_then(Value::as_object_mut) {
            if let Some(probability) = probabilities.remove(from) {
                probabilities.insert(to.to_string(), probability);
            }
        }
    }
    response
}

pub fn choose(input: &ChoiceInput<'_>) -> ChoiceVerdict {
    choose_with(input, fetch_decision)
}

pub fn choose_with<F>(input: &ChoiceInput<'_>, fetch: F) -> ChoiceVerdict
where
    F: FnOnce(&ChoiceBatch) -> Result<Value, String>,
{
    let batch = match prepare(input) {
        Ok(batch) => batch,
        Err(stop) => return ChoiceVerdict::Stop(stop),
    };
    match fetch(&batch) {
        Ok(response) => decide(input, &response),
        Err(_) => ChoiceVerdict::Stop(ChoiceStop::Unavailable),
    }
}

pub fn fetch_decision(batch: &ChoiceBatch) -> Result<Value, String> {
    let key = std::env::var("TYPESAFE_API_KEY")
        .map_err(|_| "typesafe key is not configured".to_string())?;
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("typesafe key is not configured".into());
    }
    let endpoint = std::env::var("TYPESAFE_API_URL")
        .unwrap_or_else(|_| "https://api.typesafe.ai/v1/systemone".to_string());
    let payload = serde_json::json!({
        "model": "jev-latest",
        "state": batch.state,
        "questions": batch.questions,
    });
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(redact_error)?;
    let response = client
        .post(endpoint)
        .bearer_auth(key)
        .json(&payload)
        .send()
        .map_err(redact_error)?;
    let status = response.status();
    let bytes = response.bytes().map_err(redact_error)?;
    if !status.is_success() {
        return Err(format!("typesafe http {}", status.as_u16()));
    }
    let mut value: Value =
        serde_json::from_slice(&bytes).map_err(|_| "typesafe response was not json".to_string())?;
    redact_value(&mut value);
    Ok(value)
}

pub fn redact_value(value: &mut Value) {
    match value {
        Value::String(text) => {
            if let Some(index) = text.find(key_mark()) {
                text.truncate(index);
                text.push_str("[redacted]");
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_value(item);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                redact_value(item);
            }
        }
        _ => {}
    }
}

fn key_mark() -> &'static str {
    concat!("api", "key_")
}

fn redact_error(error: impl std::fmt::Display) -> String {
    let mut text = error.to_string();
    if let Some(index) = text.find(key_mark()) {
        text.truncate(index);
        text.push_str("[redacted]");
    }
    text
}

pub fn bind_proposal(
    proposal: &Proposal,
    identity: ActionIdentity,
) -> Result<ActionRequest, String> {
    let request = ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: identity.action_id,
        run_id: identity.run_id,
        target_id: identity.target_id,
        target_generation: identity.target_generation,
        snapshot_id: identity.snapshot_id,
        geometry_revision: identity.geometry_revision,
        action: proposal.action,
        target: ActionTarget::Element {
            element_ref: proposal.element_ref.clone(),
        },
        parameters: proposal.parameters.clone(),
    };
    request.validate_schema()?;
    if request.needs_desktop_input() && matches!(request.action, ActionKind::Key | ActionKind::Drag)
    {
        return Err("chooser cannot emit key or drag".into());
    }
    if matches!(request.target, ActionTarget::Coord { .. }) {
        return Err("chooser cannot emit coordinates".into());
    }
    Ok(request)
}

fn offer(input: &ChoiceInput<'_>) -> Result<Offer, ChoiceStop> {
    if !input.min_confidence.is_finite() || !(0.0..=1.0).contains(&input.min_confidence) {
        return Err(ChoiceStop::Malformed);
    }
    if sanitize(input.goal, GOAL_CHARS).is_empty() {
        return Err(ChoiceStop::NoOperation);
    }
    if ambiguous_bindings(input.bindings) {
        return Err(ChoiceStop::AmbiguousText);
    }
    let caps = Capabilities::for_surface(input.surface, "choice");
    let mut offer = Offer {
        operations: Vec::new(),
        click: Vec::new(),
        set_value: Vec::new(),
        type_text: Vec::new(),
        scroll: Vec::new(),
        wait: Vec::new(),
        nodes: Vec::new(),
    };
    for node in &input.observation.nodes {
        if offer.nodes.len() >= MAX_ELEMENTS {
            break;
        }
        let Some(line) = node_line(node) else {
            continue;
        };
        let targets_before = target_count(&offer);
        if caps.support_for(ActionKind::Click).semantic && node_allows(node, "click") {
            offer.click.push(line.node_ref.clone());
        }
        if caps.support_for(ActionKind::SetValue).semantic
            && node_allows(node, "set_value")
            && binding_for(input.bindings, &line.node_ref, BoundText::Replace).is_some()
        {
            offer.set_value.push(line.node_ref.clone());
        }
        if caps.support_for(ActionKind::TypeText).semantic
            && node_allows(node, "type_text")
            && binding_for(input.bindings, &line.node_ref, BoundText::Insert).is_some()
        {
            offer.type_text.push(line.node_ref.clone());
        }
        if caps.support_for(ActionKind::Scroll).semantic && node_allows(node, "scroll") {
            offer.scroll.push(line.node_ref.clone());
        }
        if caps.support_for(ActionKind::Wait).semantic && node_allows(node, "wait") {
            offer.wait.push(line.node_ref.clone());
        }
        if target_count(&offer) > targets_before {
            offer.nodes.push(line);
        }
    }
    push_op(&mut offer.operations, "click", &offer.click);
    push_op(&mut offer.operations, "set_value", &offer.set_value);
    push_op(&mut offer.operations, "type_text", &offer.type_text);
    push_op(&mut offer.operations, "scroll_up", &offer.scroll);
    push_op(&mut offer.operations, "scroll_down", &offer.scroll);
    push_op(&mut offer.operations, "wait", &offer.wait);
    offer.operations.push("done");
    offer.operations.push("blocked");
    Ok(offer)
}

fn target_count(offer: &Offer) -> usize {
    offer.click.len()
        + offer.set_value.len()
        + offer.type_text.len()
        + offer.scroll.len()
        + offer.wait.len()
}

fn push_op(operations: &mut Vec<&'static str>, name: &'static str, targets: &[String]) {
    if !targets.is_empty() {
        operations.push(name);
    }
}

fn node_line(node: &ObservationNode) -> Option<NodeLine> {
    let node_ref = node.node_ref.trim();
    if node.truncated || node_ref.is_empty() || node_ref.len() > 256 {
        return None;
    }
    let actions = node
        .actions
        .iter()
        .map(|action| action.trim().to_ascii_lowercase())
        .filter(|action| !action.is_empty())
        .collect::<Vec<_>>()
        .join(",");
    Some(NodeLine {
        node_ref: node_ref.to_string(),
        role: sanitize(&node.role, 40),
        name: sanitize(&node.name, NAME_CHARS),
        actions,
    })
}

fn node_allows(node: &ObservationNode, action: &str) -> bool {
    let advertised: Vec<String> = node
        .actions
        .iter()
        .map(|item| item.trim().to_ascii_lowercase())
        .filter(|item| !item.is_empty())
        .collect();
    if !advertised.is_empty() {
        return advertised.iter().any(|item| item == action);
    }
    let role = node.role.trim().to_ascii_lowercase();
    match action {
        "click" => matches!(
            role.as_str(),
            "button" | "link" | "checkbox" | "radio" | "menuitem" | "tab"
        ),
        "set_value" | "type_text" => {
            matches!(
                role.as_str(),
                "textbox" | "searchbox" | "combobox" | "textarea"
            )
        }
        "wait" => !node.name.trim().is_empty(),
        _ => false,
    }
}

fn binding_for<'a>(
    bindings: &'a [TextBinding],
    element_ref: &str,
    mode: BoundText,
) -> Option<&'a TextBinding> {
    bindings.iter().find(|binding| {
        binding.mode == mode
            && binding.element_ref.trim() == element_ref
            && valid_bound_text(&binding.text)
    })
}

fn valid_bound_text(text: &str) -> bool {
    !text.is_empty() && !text.contains('\0') && text.chars().count() <= TEXT_MAX_CHARS
}

fn ambiguous_bindings(bindings: &[TextBinding]) -> bool {
    for (index, binding) in bindings.iter().enumerate() {
        if bindings.iter().skip(index + 1).any(|other| {
            other.mode == binding.mode
                && other.element_ref.trim() == binding.element_ref.trim()
                && other.text != binding.text
        }) {
            return true;
        }
    }
    false
}

fn render_state(input: &ChoiceInput<'_>, offer: &Offer) -> String {
    let mut state = format!(
        "goal: {}\nsurface: {}\nelements:\n",
        sanitize(input.goal, GOAL_CHARS),
        input.surface.as_wire()
    );
    if offer.nodes.is_empty() {
        state.push_str("(none)\n");
    }
    for node in &offer.nodes {
        state.push_str(&format!(
            "[{}] {} {} actions={}\n",
            node.node_ref, node.role, node.name, node.actions
        ));
    }
    state
}

fn render_questions(offer: &Offer) -> Value {
    let mut questions = Map::new();
    questions.insert(
        "operation".into(),
        choice_question(
            "Which single next operation moves toward the goal. Choose done when it is already satisfied. Choose blocked when none of the offered elements can make progress.",
            &offer
                .operations
                .iter()
                .map(|name| (*name, operation_blurb(name)))
                .collect::<Vec<_>>(),
        ),
    );
    insert_target(
        &mut questions,
        "click_target",
        "Which element should be clicked.",
        &offer.click,
    );
    insert_target(
        &mut questions,
        "set_value_target",
        "Which element should receive the caller-supplied replacement text.",
        &offer.set_value,
    );
    insert_target(
        &mut questions,
        "type_text_target",
        "Which element should receive the caller-supplied inserted text.",
        &offer.type_text,
    );
    insert_target(
        &mut questions,
        "scroll_target",
        "Which element should be scrolled.",
        &offer.scroll,
    );
    insert_target(
        &mut questions,
        "wait_target",
        "Which element should be waited on.",
        &offer.wait,
    );
    Value::Object(questions)
}

fn insert_target(
    questions: &mut Map<String, Value>,
    key: &str,
    instructions: &str,
    refs: &[String],
) {
    if refs.is_empty() {
        return;
    }
    questions.insert(
        key.into(),
        choice_question(
            instructions,
            &refs
                .iter()
                .map(|node_ref| (node_ref.as_str(), "Observed element"))
                .collect::<Vec<_>>(),
        ),
    );
}

fn choice_question(instructions: &str, criteria: &[(&str, &str)]) -> Value {
    let mut body = Map::new();
    body.insert("type".into(), Value::String("choice".into()));
    body.insert("instructions".into(), Value::String(instructions.into()));
    let mut options = Map::new();
    for (key, description) in criteria {
        options.insert(
            (*key).to_string(),
            Value::String((*description).to_string()),
        );
    }
    body.insert("criteria".into(), Value::Object(options));
    Value::Object(body)
}

fn operation_blurb(name: &str) -> &'static str {
    match name {
        "click" => "Click one offered element",
        "set_value" => "Replace one field with text the caller already supplied",
        "type_text" => "Insert text the caller already supplied",
        "scroll_up" => "Scroll one offered element upward",
        "scroll_down" => "Scroll one offered element downward",
        "wait" => "Wait until one offered element's name matches",
        "done" => "The goal is already satisfied",
        "blocked" => "No offered element can make progress",
        _ => "Unsupported",
    }
}

struct ParsedChoice<'a> {
    choice: &'a str,
    confidence: f64,
}

fn choice_of<'a>(answers: &'a Map<String, Value>, key: &str) -> Option<ParsedChoice<'a>> {
    let answer = answers.get(key)?.as_object()?;
    if answer.get("type").and_then(Value::as_str) != Some("choice") {
        return None;
    }
    let choice = answer.get("choice").and_then(Value::as_str)?;
    let confidence = answer.get("confidence").and_then(Value::as_f64)?;
    if !confidence.is_finite() {
        return None;
    }
    if let Some(probabilities) = answer.get("probabilities").and_then(Value::as_object) {
        let listed = probabilities.get(choice).and_then(Value::as_f64)?;
        if !listed.is_finite() || listed <= 0.0 {
            return None;
        }
    }
    Some(ParsedChoice { choice, confidence })
}

fn propose_target(
    input: &ChoiceInput<'_>,
    answers: &Map<String, Value>,
    key: &str,
    offered: &[String],
    action: ActionKind,
    parameters: Value,
) -> ChoiceVerdict {
    let Some(target) = choice_of(answers, key) else {
        return ChoiceVerdict::Stop(ChoiceStop::MissingTarget);
    };
    if target.confidence < input.min_confidence
        || !offered.iter().any(|node_ref| node_ref == target.choice)
    {
        return ChoiceVerdict::Stop(if target.confidence < input.min_confidence {
            ChoiceStop::LowConfidence
        } else {
            ChoiceStop::Unoffered
        });
    }
    ChoiceVerdict::Propose(Proposal {
        action,
        element_ref: target.choice.to_string(),
        parameters,
        confidence: target.confidence.min(operation_confidence(answers)),
    })
}

fn operation_confidence(answers: &Map<String, Value>) -> f64 {
    answers
        .get("operation")
        .and_then(Value::as_object)
        .and_then(|answer| answer.get("confidence"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
}

fn propose_text(
    input: &ChoiceInput<'_>,
    answers: &Map<String, Value>,
    key: &str,
    offered: &[String],
    mode: BoundText,
) -> ChoiceVerdict {
    let Some(target) = choice_of(answers, key) else {
        return ChoiceVerdict::Stop(ChoiceStop::MissingTarget);
    };
    if target.confidence < input.min_confidence {
        return ChoiceVerdict::Stop(ChoiceStop::LowConfidence);
    }
    if !offered.iter().any(|node_ref| node_ref == target.choice) {
        return ChoiceVerdict::Stop(ChoiceStop::Unoffered);
    }
    let Some(binding) = binding_for(input.bindings, target.choice, mode) else {
        return ChoiceVerdict::Stop(ChoiceStop::MissingText);
    };
    let action = match mode {
        BoundText::Replace => ActionKind::SetValue,
        BoundText::Insert => ActionKind::TypeText,
    };
    ChoiceVerdict::Propose(Proposal {
        action,
        element_ref: target.choice.to_string(),
        parameters: serde_json::json!({ "text": binding.text }),
        confidence: target.confidence.min(operation_confidence(answers)),
    })
}

fn propose_scroll(
    input: &ChoiceInput<'_>,
    answers: &Map<String, Value>,
    offered: &[String],
    delta: i64,
) -> ChoiceVerdict {
    propose_target(
        input,
        answers,
        "scroll_target",
        offered,
        ActionKind::Scroll,
        serde_json::json!({ "delta": delta }),
    )
}

fn propose_wait(
    input: &ChoiceInput<'_>,
    answers: &Map<String, Value>,
    offer: &Offer,
) -> ChoiceVerdict {
    let Some(target) = choice_of(answers, "wait_target") else {
        return ChoiceVerdict::Stop(ChoiceStop::MissingTarget);
    };
    if target.confidence < input.min_confidence {
        return ChoiceVerdict::Stop(ChoiceStop::LowConfidence);
    }
    if !offer.wait.iter().any(|node_ref| node_ref == target.choice) {
        return ChoiceVerdict::Stop(ChoiceStop::Unoffered);
    }
    let Some(name) = offer
        .nodes
        .iter()
        .find(|node| node.node_ref == target.choice)
        .map(|node| node.name.clone())
        .filter(|name| !name.is_empty())
    else {
        return ChoiceVerdict::Stop(ChoiceStop::MissingTarget);
    };
    ChoiceVerdict::Propose(Proposal {
        action: ActionKind::Wait,
        element_ref: target.choice.to_string(),
        parameters: serde_json::json!({ "nameEquals": name, "timeoutMs": 1000 }),
        confidence: target.confidence.min(operation_confidence(answers)),
    })
}

fn sanitize(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if out.chars().count() >= max_chars {
            break;
        }
        if ch == '\0' || ch.is_control() {
            if !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            out.push(ch);
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ObservationImage;

    fn node(node_ref: &str, role: &str, name: &str, actions: &[&str]) -> ObservationNode {
        ObservationNode {
            node_ref: node_ref.into(),
            role: role.into(),
            name: name.into(),
            actions: actions.iter().map(|action| (*action).to_string()).collect(),
            truncated: false,
            x: Some(1234.5),
            y: Some(6789.5),
            width: Some(10.0),
            height: Some(10.0),
        }
    }

    fn observation(nodes: Vec<ObservationNode>) -> Observation {
        Observation {
            version: PROTOCOL_VERSION,
            run_id: "run".into(),
            target_id: "target".into(),
            target_generation: 1,
            snapshot_id: "snap".into(),
            captured_at: "2026-09-22T00:00:00Z".into(),
            geometry_revision: 3,
            coordinate_space: "image-pixels".into(),
            image: ObservationImage {
                width: 10,
                height: 10,
                content_id: "png-secret".into(),
                png_base64: Some("iVBORw0KGgo".into()),
            },
            nodes,
            text: "SECRET_PAGE_BODY".into(),
            truncated: false,
            crop_x: 0,
            crop_y: 0,
            crop_width: 0,
            crop_height: 0,
            scale: 1.0,
            dpi: 96.0,
            origin_x: 0,
            origin_y: 0,
            topology_revision: 1,
        }
    }

    fn input<'a>(
        observation: &'a Observation,
        bindings: &'a [TextBinding],
        surface: SurfaceKind,
    ) -> ChoiceInput<'a> {
        ChoiceInput {
            goal: "Save the form",
            surface,
            observation,
            bindings,
            min_confidence: DEFAULT_MIN_CONFIDENCE,
        }
    }

    fn answer(choice: &str, confidence: f64) -> Value {
        serde_json::json!({
            "type": "choice",
            "choice": choice,
            "confidence": confidence,
            "probabilities": { choice: confidence }
        })
    }

    #[test]
    fn state_omits_pixels_page_text_and_screenshot() {
        let obs = observation(vec![node("e1", "button", "Save", &["click"])]);
        let batch = prepare(&input(&obs, &[], SurfaceKind::ExistingTab)).unwrap();
        assert!(!batch.state.contains("1234.5"));
        assert!(!batch.state.contains("SECRET_PAGE_BODY"));
        assert!(!batch.state.contains("iVBORw0KGgo"));
        assert!(!batch.state.contains("png-secret"));
        assert!(batch.state.contains("[e1]"));
    }

    #[test]
    fn click_uses_only_the_click_head() {
        let obs = observation(vec![
            node("e1", "button", "Save", &["click"]),
            node("e2", "textbox", "Name", &["type_text", "set_value"]),
        ]);
        let bindings = [TextBinding {
            element_ref: "e2".into(),
            text: "Ada".into(),
            mode: BoundText::Insert,
        }];
        let sample = input(&obs, &bindings, SurfaceKind::ExistingTab);
        let verdict = decide(
            &sample,
            &serde_json::json!({
                "answers": {
                    "operation": answer("click", 0.91),
                    "click_target": answer("e1", 0.88),
                    "type_text_target": answer("e2", 0.99)
                }
            }),
        );
        match verdict {
            ChoiceVerdict::Propose(proposal) => {
                assert_eq!(proposal.action, ActionKind::Click);
                assert_eq!(proposal.element_ref, "e1");
                assert_eq!(proposal.parameters, serde_json::json!({}));
            }
            other => panic!("expected proposal, got {other:?}"),
        }
    }

    #[test]
    fn type_text_keeps_caller_literal_and_rejects_unbound_fields() {
        let obs = observation(vec![node("e2", "textbox", "Name", &["type_text"])]);
        let unbound = input(&obs, &[], SurfaceKind::ExistingTab);
        let batch = prepare(&unbound).unwrap();
        assert!(batch.questions["operation"]["criteria"]
            .get("type_text")
            .is_none());
        let bindings = [TextBinding {
            element_ref: "e2".into(),
            text: "你好".into(),
            mode: BoundText::Insert,
        }];
        let bound = input(&obs, &bindings, SurfaceKind::ExistingTab);
        let verdict = decide(
            &bound,
            &serde_json::json!({
                "operation": answer("type_text", 0.8),
                "type_text_target": answer("e2", 0.7)
            }),
        );
        match verdict {
            ChoiceVerdict::Propose(proposal) => {
                assert_eq!(proposal.parameters["text"], "你好");
                let request = bind_proposal(
                    &proposal,
                    ActionIdentity {
                        action_id: "a1".into(),
                        run_id: "r1".into(),
                        target_id: "t1".into(),
                        target_generation: 2,
                        snapshot_id: "s1".into(),
                        geometry_revision: 4,
                    },
                )
                .unwrap();
                request.validate_schema().unwrap();
                assert!(matches!(request.target, ActionTarget::Element { .. }));
            }
            other => panic!("expected proposal, got {other:?}"),
        }
    }

    #[test]
    fn low_confidence_unknown_ref_and_done_do_not_propose() {
        let obs = observation(vec![node("e1", "button", "Save", &["click"])]);
        let sample = input(&obs, &[], SurfaceKind::ManagedBrowser);
        assert_eq!(
            decide(
                &sample,
                &serde_json::json!({
                    "operation": answer("click", 0.59),
                    "click_target": answer("e1", 0.9)
                })
            ),
            ChoiceVerdict::Stop(ChoiceStop::LowConfidence)
        );
        assert_eq!(
            decide(
                &sample,
                &serde_json::json!({
                    "operation": answer("click", 0.9),
                    "click_target": answer("missing", 0.9)
                })
            ),
            ChoiceVerdict::Stop(ChoiceStop::Unoffered)
        );
        assert_eq!(
            decide(
                &sample,
                &serde_json::json!({ "operation": answer("done", 0.7) })
            ),
            ChoiceVerdict::Stop(ChoiceStop::Done)
        );
    }

    #[test]
    fn desktop_does_not_offer_coordinate_scroll_or_key() {
        let obs = observation(vec![node(
            "e1",
            "button",
            "Save",
            &["click", "scroll", "key", "drag"],
        )]);
        let batch = prepare(&input(&obs, &[], SurfaceKind::Desktop)).unwrap();
        let criteria = &batch.questions["operation"]["criteria"];
        assert!(criteria.get("click").is_some());
        assert!(criteria.get("scroll_up").is_none());
        assert!(criteria.get("key").is_none());
        assert!(criteria.get("drag").is_none());
    }

    #[test]
    fn named_scroll_directions_use_the_shared_down_positive_wire_contract() {
        let obs = observation(vec![node("e1", "generic", "List", &["scroll"])]);
        for (operation, expected) in [("scroll_up", -360), ("scroll_down", 360)] {
            let verdict = decide(
                &input(&obs, &[], SurfaceKind::ExistingTab),
                &serde_json::json!({
                    "operation":answer(operation,0.9),"scroll_target":answer("e1",0.9)
                }),
            );
            let ChoiceVerdict::Propose(proposal) = verdict else {
                panic!("scroll proposal missing")
            };
            let delta = crate::protocol::ScrollDelta::parse(&proposal.parameters).unwrap();
            assert_eq!(delta.value(), expected);
            assert_eq!(delta.native_positive_up(), -expected);
        }
    }

    #[test]
    fn webview_has_no_action_and_existing_tab_scroll_is_element_only() {
        let obs = observation(vec![node("e1", "generic", "List", &["scroll"])]);
        let web = prepare(&input(&obs, &[], SurfaceKind::WebView)).unwrap();
        assert!(web.questions["operation"]["criteria"]
            .get("click")
            .is_none());
        let tab = prepare(&input(&obs, &[], SurfaceKind::ExistingTab)).unwrap();
        let verdict = decide(
            &input(&obs, &[], SurfaceKind::ExistingTab),
            &serde_json::json!({
                "operation": answer("scroll_down", 0.9),
                "scroll_target": answer("e1", 0.9)
            }),
        );
        match verdict {
            ChoiceVerdict::Propose(proposal) => {
                assert_eq!(proposal.action, ActionKind::Scroll);
                assert_eq!(proposal.parameters["delta"], 360);
            }
            other => panic!("expected scroll, got {other:?}"),
        }
        assert!(tab.questions["scroll_target"]["criteria"]
            .get("e1")
            .is_some());
    }

    #[test]
    fn choose_with_uses_the_supplied_response_and_stops_when_fetch_fails() {
        let obs = observation(vec![node("e-name", "textbox", "Name", &["set_value"])]);
        let bindings = [TextBinding {
            element_ref: "e-name".into(),
            text: "Ada".into(),
            mode: BoundText::Replace,
        }];
        let sample = input(&obs, &bindings, SurfaceKind::ExistingTab);
        let body = serde_json::json!({
            "model": "jev-test",
            "answers": {
                "operation": answer("set_value", 0.93),
                "set_value_target": answer("e-name", 0.91)
            }
        });
        match choose_with(&sample, |_| Ok(body.clone())) {
            ChoiceVerdict::Propose(proposal) => {
                assert_eq!(proposal.element_ref, "e-name");
                assert_eq!(proposal.parameters["text"], "Ada");
                assert_eq!(proposal.parameters.as_object().unwrap().len(), 1);
            }
            other => panic!("expected proposal, got {other:?}"),
        }
        assert_eq!(
            choose_with(&sample, |_| Err("down".into())),
            ChoiceVerdict::Stop(ChoiceStop::Unavailable)
        );
    }

    #[test]
    fn captured_live_response_proposes_caller_text_and_stops_when_unsafe() {
        let obs = observation(vec![node("e-name", "textbox", "Name", &["set_value"])]);
        let bindings = [TextBinding {
            element_ref: "e-name".into(),
            text: "Ada".into(),
            mode: BoundText::Replace,
        }];
        let mut sample = input(&obs, &bindings, SurfaceKind::ExistingTab);
        sample.goal = "Replace the Name field with the supplied text";
        let response: Value =
            serde_json::from_str(include_str!("choice_live_response.json")).unwrap();
        let verdict = choose_with(&sample, |_| Ok(response.clone()));
        assert_eq!(
            accept_step(&verdict, &bindings[0].text),
            AcceptedStep::ReplaceText {
                element_ref: response["answers"]["set_value_target"]["choice"]
                    .as_str()
                    .unwrap()
                    .to_string(),
                text: bindings[0].text.clone(),
            }
        );
        assert!(!matches!(
            accept_step(&verdict, &bindings[0].text),
            AcceptedStep::Click { .. }
        ));
        let rebound = rebind_choice_target(response.clone(), "e-name", "edit-live");
        let live = observation(vec![node("edit-live", "textbox", "Name", &["set_value"])]);
        let live_bindings = [TextBinding {
            element_ref: "edit-live".into(),
            text: "Ada".into(),
            mode: BoundText::Replace,
        }];
        let mut live_input = input(&live, &live_bindings, SurfaceKind::Desktop);
        live_input.goal = "Replace the Name field with the supplied text";
        let live_verdict = choose_with(&live_input, |_| Ok(rebound));
        assert_eq!(
            accept_step(&live_verdict, "Ada"),
            AcceptedStep::ReplaceText {
                element_ref: "edit-live".into(),
                text: "Ada".into(),
            }
        );
        let mut low = response.clone();
        low["answers"]["operation"]["confidence"] = serde_json::json!(0.1);
        assert_eq!(
            choose_with(&sample, |_| Ok(low)),
            ChoiceVerdict::Stop(ChoiceStop::LowConfidence)
        );
        let mut missing = response.clone();
        missing["answers"]["set_value_target"]["choice"] = serde_json::json!("absent");
        missing["answers"]["set_value_target"]["probabilities"] =
            serde_json::json!({ "absent": 1.0 });
        assert_eq!(
            choose_with(&sample, |_| Ok(missing)),
            ChoiceVerdict::Stop(ChoiceStop::Unoffered)
        );
    }
}

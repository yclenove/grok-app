//! AT-SPI over the session's local accessibility bus. References are opaque,
//! snapshot-scoped and bound to an XRes-proven process, unique bus owner and
//! exact accessible ancestry. No key-map changes, clipboard use or pixel fallback.
use crate::client::{err, Geometry};
use grok_computer_use_core::adapter::{AdapterActResult, DispatchRequest};
use grok_computer_use_core::protocol::{ActionKind, ActionTarget, ObservationNode};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};
use zbus::blocking::{connection::Builder, Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

#[path = "accessibility_call.rs"]
mod call;
#[path = "accessibility_text.rs"]
mod text_edit;
#[path = "accessibility_wait.rs"]
mod wait;

type Object = (String, OwnedObjectPath);
const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
const COMPONENT: &str = "org.a11y.atspi.Component";
const ACTION: &str = "org.a11y.atspi.Action";
const TEXT: &str = "org.a11y.atspi.Text";
const EDITABLE: &str = "org.a11y.atspi.EditableText";
const ROOT: &str = "/org/a11y/atspi/accessible/root";
const MAX_NODES: usize = 256;
const MAX_TEXT: usize = 32768;
// Wire AtspiStateType, not ATK's different enum numbering.
// GNOME at-spi2-core/atspi/atspi-constants.h: SENSITIVE=24, SHOWING=25, VISIBLE=30.
const DEFUNCT: usize = 6;
const EDITABLE_STATE: usize = 7;
const ENABLED: usize = 8;
const SENSITIVE: usize = 24;
const SHOWING: usize = 25;
const VISIBLE: usize = 30;

#[derive(Default)]
pub(crate) struct Accessibility {
    connection: Option<Connection>,
    snapshots: VecDeque<Snapshot>,
    pub calls: call::NativeCalls,
}

struct Snapshot {
    run: String,
    id: String,
    target: String,
    geometry: Geometry,
    pid: u32,
    title: String,
    root: Object,
    nodes: HashMap<String, Node>,
}

struct Node {
    object: Object,
    // Immediate parent first; the final entry is the exact native-window root.
    ancestry: Vec<Object>,
    name: String,
    role: u32,
    actions: Vec<String>,
    bounds: Option<(i32, i32, i32, i32)>,
}

fn proxy<'a>(
    connection: &'a Connection,
    object: &'a Object,
    interface: &'static str,
) -> Result<Proxy<'a>, String> {
    zbus::blocking::proxy::Builder::<Proxy<'a>>::new(connection)
        .destination(object.0.as_str())
        .map_err(err)?
        .path(object.1.as_str())
        .map_err(err)?
        .interface(interface)
        .map_err(err)?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .map_err(err)
}

fn input_message<'a>(
    connection: &'a Connection,
    object: &'a Object,
    interface: &'static str,
    method: &'static str,
) -> Result<zbus::message::Builder<'a>, String> {
    zbus::Message::method_call(object.1.as_str(), method)
        .map_err(err)?
        .sender(
            connection
                .unique_name()
                .ok_or("missing local bus identity")?,
        )
        .map_err(err)?
        .destination(object.0.as_str())
        .map_err(err)?
        .interface(interface)
        .map_err(err)
}

fn children(connection: &Connection, object: &Object) -> Result<Vec<Object>, String> {
    let result: Vec<Object> = proxy(connection, object, ACCESSIBLE)?
        .call("GetChildren", &())
        .map_err(err)?;
    if result.len() > MAX_NODES {
        return Err("AT-SPI child list exceeds bounded traversal".into());
    }
    Ok(result)
}

fn states(connection: &Connection, object: &Object) -> Result<Vec<u32>, String> {
    proxy(connection, object, ACCESSIBLE)?
        .call("GetState", &())
        .map_err(err)
}

fn state(bits: &[u32], index: usize) -> bool {
    bits.get(index / 32)
        .is_some_and(|v| v & (1 << (index % 32)) != 0)
}

fn actionable(bits: &[u32]) -> bool {
    visible(bits) && state(bits, ENABLED) && state(bits, SENSITIVE)
}

fn visible(bits: &[u32]) -> bool {
    !state(bits, DEFUNCT) && state(bits, SHOWING) && state(bits, VISIBLE)
}

fn bounded_display(text: &str, limit: usize) -> (String, bool) {
    match text.char_indices().nth(limit) {
        Some((end, _)) => (text[..end].to_owned(), true),
        None => (text.to_owned(), false),
    }
}

fn extents(connection: &Connection, object: &Object) -> Result<(i32, i32, i32, i32), String> {
    proxy(connection, object, COMPONENT)?
        .call("GetExtents", &(0u32,))
        .map_err(err)
}

fn process(connection: &Connection, bus: &str) -> Result<u32, String> {
    if !bus.starts_with(':') {
        return Err("AT-SPI requires a unique bus owner".into());
    }
    Proxy::new(
        connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .map_err(err)?
    .call("GetConnectionUnixProcessID", &(bus,))
    .map_err(err)
}

fn root_matches(
    connection: &Connection,
    root: &Object,
    g: Geometry,
    title: &str,
) -> Result<bool, String> {
    let accessible = proxy(connection, root, ACCESSIBLE)?;
    let name: String = accessible.get_property("Name").map_err(err)?;
    let role: u32 = accessible.call("GetRole", &()).map_err(err)?;
    Ok(matches!(role, 16 | 23 | 69)
        && name == title
        && !state(&states(connection, root)?, DEFUNCT)
        && extents(connection, root)?
            == (
                i32::from(g.x),
                i32::from(g.y),
                i32::from(g.width),
                i32::from(g.height),
            ))
}

fn window_root(
    connection: &Connection,
    g: Geometry,
    pid: u32,
    title: &str,
) -> Result<Object, String> {
    let registry = (
        "org.a11y.atspi.Registry".into(),
        OwnedObjectPath::try_from(ROOT).map_err(err)?,
    );
    let mut matches = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    for app in children(connection, &registry)? {
        if Instant::now() >= deadline {
            return Err("AT-SPI discovery deadline".into());
        }
        if process(connection, &app.0).ok() != Some(pid) {
            continue;
        }
        for root in children(connection, &app)? {
            if Instant::now() >= deadline {
                return Err("AT-SPI discovery deadline".into());
            }
            if root.0 == app.0 && root_matches(connection, &root, g, title)? {
                matches.push(root);
            }
        }
    }
    if matches.len() != 1 {
        return Err("AT-SPI window binding missing or ambiguous; no fallback".into());
    }
    Ok(matches.remove(0))
}

fn click_index(connection: &Connection, object: &Object) -> Result<Option<i32>, String> {
    let actions: Vec<(String, String, String)> = proxy(connection, object, ACTION)?
        .call("GetActions", &())
        .map_err(err)?;
    Ok(primary_action_index(&actions))
}

fn primary_action_index(actions: &[(String, String, String)]) -> Option<i32> {
    if actions.len() > 64 {
        return None;
    }
    // GTK exposes click, press and release together. Only a complete primary
    // action is eligible; never treat a bare press as a completed click.
    for name in ["click", "activate"] {
        let indices: Vec<_> = actions
            .iter()
            .enumerate()
            .filter(|(_, a)| a.0.eq_ignore_ascii_case(name))
            .map(|(i, _)| i as i32)
            .collect();
        match indices.as_slice() {
            [index] => return Some(*index),
            [] => {}
            _ => return None,
        }
    }
    None
}

fn read_text(connection: &Connection, object: &Object) -> Result<String, String> {
    let text = proxy(connection, object, TEXT)?;
    let count: i32 = text.get_property("CharacterCount").map_err(err)?;
    if !(0..=MAX_TEXT as i32).contains(&count) {
        return Err("AT-SPI text exceeds read limit".into());
    }
    let value: String = text.call("GetText", &(0i32, count)).map_err(err)?;
    if value.len() > MAX_TEXT || value.chars().count() != count as usize {
        return Err("AT-SPI text changed or exceeds read limit".into());
    }
    Ok(value)
}

fn inserted(before: &str, offset: i32, text: &str) -> Result<String, String> {
    if offset < 0 || before.len().saturating_add(text.len()) > MAX_TEXT {
        return Err("AT-SPI text or caret exceeds bounds".into());
    }
    let byte = before
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(before.len()))
        .nth(offset as usize)
        .ok_or("AT-SPI caret is outside text")?;
    Ok(format!("{}{}{}", &before[..byte], text, &before[byte..]))
}

fn validate_node(connection: &Connection, snapshot: &Snapshot, node: &Node) -> Result<(), String> {
    if process(connection, &node.object.0)? != snapshot.pid
        || window_root(connection, snapshot.geometry, snapshot.pid, &snapshot.title)?
            != snapshot.root
    {
        return Err("AT-SPI native window binding changed".into());
    }
    let accessible = proxy(connection, &node.object, ACCESSIBLE)?;
    let role: u32 = accessible.call("GetRole", &()).map_err(err)?;
    let name: String = accessible.get_property("Name").map_err(err)?;
    if role != node.role || name != node.name || !actionable(&states(connection, &node.object)?) {
        return Err("AT-SPI node identity or availability changed".into());
    }
    let mut current = node.object.clone();
    for parent in &node.ancestry {
        let actual: Object = proxy(connection, &current, ACCESSIBLE)?
            .get_property("Parent")
            .map_err(err)?;
        if &actual != parent || !children(connection, parent)?.contains(&current) {
            return Err("AT-SPI node ancestry changed".into());
        }
        current = parent.clone();
    }
    if current != snapshot.root {
        return Err("AT-SPI reference escaped native root".into());
    }
    Ok(())
}

impl Accessibility {
    fn connect(&mut self) -> Result<&Connection, String> {
        if self.connection.is_none() {
            let session = Builder::session()
                .map_err(err)?
                .method_timeout(Duration::from_secs(1))
                .build()
                .map_err(err)?;
            let address: String =
                Proxy::new(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus")
                    .map_err(err)?
                    .call("GetAddress", &())
                    .map_err(err)?;
            // Never forward a user's accessibility identity to a remote bus.
            if !address.starts_with("unix:") || address.contains(';') {
                return Err("only one local Unix accessibility bus is supported".into());
            }
            self.connection = Some(
                Builder::address(address.as_str())
                    .map_err(err)?
                    .method_timeout(Duration::from_secs(1))
                    .build()
                    .map_err(err)?,
            );
        }
        Ok(self.connection.as_ref().expect("AT-SPI connected"))
    }

    pub fn available(&mut self) -> bool {
        self.connect().is_ok()
    }

    pub fn observe(
        &mut self,
        run: &str,
        target: &str,
        snapshot_id: &str,
        g: Geometry,
        pid: u32,
        title: &str,
    ) -> Result<(Vec<ObservationNode>, bool), String> {
        if self.calls.uncertain {
            return Err("AT-SPI native completion pending; no new semantic snapshot".into());
        }
        // Failed replacement observations must not preserve old model refs.
        self.snapshots
            .retain(|s| s.run != run || s.target != target);
        let connection = self.connect()?;
        let root = window_root(connection, g, pid, title)?;
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut pending = VecDeque::from([(root.clone(), Vec::new())]);
        let mut seen = HashSet::new();
        let mut nodes = HashMap::new();
        let mut result = Vec::new();
        let mut clipped = false;
        while let Some((object, ancestry)) = pending.pop_front() {
            if result.len() >= MAX_NODES || Instant::now() >= deadline {
                pending.push_front((object, ancestry));
                break;
            }
            if object.0 != root.0 || !seen.insert(object.clone()) {
                continue;
            }
            let accessible = proxy(connection, &object, ACCESSIBLE)?;
            let bits = states(connection, &object)?;
            if state(&bits, DEFUNCT) {
                continue;
            }
            let role: u32 = accessible.call("GetRole", &()).map_err(err)?;
            // Do not expose names/values or actions of protected input controls.
            if role == 40 {
                continue;
            }
            let name: String = accessible.get_property("Name").map_err(err)?;
            if name.len() > MAX_TEXT {
                return Err("AT-SPI name exceeds traversal budget".into());
            }
            let role_name: String = accessible.call("GetRoleName", &()).map_err(err)?;
            let interfaces: Vec<String> = accessible.call("GetInterfaces", &()).map_err(err)?;
            let mut actions = Vec::new();
            if actionable(&bits) {
                if interfaces.iter().any(|s| s == ACTION)
                    && click_index(connection, &object)?.is_some()
                {
                    actions.push("click".into());
                }
                if state(&bits, EDITABLE_STATE)
                    && interfaces.iter().any(|s| s == EDITABLE)
                    && interfaces.iter().any(|s| s == TEXT)
                {
                    actions.extend(["set_value".into(), "type_text".into()]);
                }
            }
            let rect = if interfaces.iter().any(|s| s == COMPONENT) {
                extents(connection, &object).ok()
            } else {
                None
            };
            if wait::eligible(&bits, rect, g) {
                actions.push("wait".into());
            }
            let node_ref = format!("atspi-{}", uuid::Uuid::new_v4());
            let (display_role, role_clipped) = bounded_display(&role_name, 128);
            let (display_name, name_clipped) = bounded_display(&name, 512);
            let node_clipped = role_clipped || name_clipped;
            clipped |= node_clipped;
            result.push(ObservationNode {
                node_ref: node_ref.clone(),
                role: display_role,
                name: display_name,
                actions: actions.clone(),
                truncated: node_clipped,
                x: rect.map(|r| f64::from(r.0 - i32::from(g.x))),
                y: rect.map(|r| f64::from(r.1 - i32::from(g.y))),
                width: rect.map(|r| f64::from(r.2)),
                height: rect.map(|r| f64::from(r.3)),
            });
            let mut next_ancestry = vec![object.clone()];
            next_ancestry.extend(ancestry.clone());
            if next_ancestry.len() < 32 {
                for child in children(connection, &object)? {
                    if pending.len() + result.len() >= MAX_NODES {
                        clipped = true;
                        break;
                    }
                    pending.push_back((child, next_ancestry.clone()));
                }
            } else {
                clipped = true;
            }
            drop(accessible);
            nodes.insert(
                node_ref,
                Node {
                    object,
                    ancestry,
                    name,
                    role,
                    actions,
                    bounds: rect,
                },
            );
        }
        let truncated = clipped || !pending.is_empty();
        self.snapshots
            .retain(|s| s.run != run || s.target != target);
        if self.snapshots.len() >= 8 {
            self.snapshots.pop_front();
        }
        self.snapshots.push_back(Snapshot {
            run: run.into(),
            id: snapshot_id.into(),
            target: target.into(),
            geometry: g,
            pid,
            title: title.into(),
            root,
            nodes,
        });
        Ok((result, truncated))
    }

    /// The caller revalidates X11 lifetime/geometry/focus immediately before the
    /// each native write; no X server grab is held while GTK services D-Bus.
    pub fn act(
        &mut self,
        req: &DispatchRequest,
        mut validate_native: impl FnMut() -> Result<(), String>,
    ) -> Result<AdapterActResult, String> {
        if req.action == ActionKind::Wait {
            return wait::run(self, req, validate_native);
        }
        if self.calls.uncertain {
            return Err("AT-SPI completion remains unknown".into());
        }
        let ActionTarget::Element { element_ref } = &req.target else {
            return Err("AT-SPI requires an observed element reference".into());
        };
        let snapshot = self
            .snapshots
            .iter()
            .find(|s| {
                s.run == req.run_id
                    && s.target == req.target_id
                    && s.id == req.snapshot_id
                    && s.geometry.revision() == req.geometry_revision
            })
            .ok_or("AT-SPI snapshot is stale")?;
        let node = snapshot
            .nodes
            .get(element_ref)
            .ok_or("AT-SPI reference is not in this snapshot")?;
        let connection = self.connection.as_ref().ok_or("AT-SPI disconnected")?;
        let action_name = match req.action {
            ActionKind::Click => "click",
            ActionKind::SetValue => "set_value",
            ActionKind::TypeText => "type_text",
            _ => return Err("AT-SPI action unsupported".into()),
        };
        if !node.actions.iter().any(|a| a == action_name) {
            return Err("AT-SPI node did not advertise this action".into());
        }
        req.cancellation.check()?;
        validate_native()?;
        validate_node(connection, snapshot, node)?;
        if req.action == ActionKind::TypeText {
            return text_edit::run(
                connection,
                snapshot,
                node,
                req,
                &mut self.calls,
                validate_native,
            );
        }
        let mut expected = None;
        let message = match req.action {
            ActionKind::Click => {
                if req
                    .parameters
                    .get("count")
                    .and_then(|v| v.as_u64())
                    .is_some_and(|n| n != 1)
                    || req
                        .parameters
                        .get("button")
                        .and_then(|v| v.as_str())
                        .is_some_and(|b| b != "left")
                {
                    return Err("AT-SPI semantic click is a single primary action".into());
                }
                let index = click_index(connection, &node.object)?
                    .ok_or("AT-SPI primary action changed")?;
                input_message(connection, &node.object, ACTION, "DoAction")?
                    .build(&(index,))
                    .map_err(err)?
            }
            ActionKind::SetValue => {
                if !state(&states(connection, &node.object)?, EDITABLE_STATE) {
                    return Err("AT-SPI node is no longer editable".into());
                }
                let value = req
                    .parameters
                    .get("text")
                    .or_else(|| req.parameters.get("value"))
                    .and_then(|v| v.as_str())
                    .ok_or("text is required")?;
                if value.len() > MAX_TEXT || value.contains('\0') {
                    return Err("text exceeds AT-SPI bounds".into());
                }
                expected = Some(value.to_owned());
                input_message(connection, &node.object, EDITABLE, "SetTextContents")?
                    .build(&(value,))
                    .map_err(err)?
            }
            _ => unreachable!(),
        };
        // Timeout/disconnection does NOT prove the toolkit finished executing.
        let applied = self.calls.call(connection, message, || {
            validate_native()?;
            req.cancellation.check()
        })?;
        if !applied {
            return Err("AT-SPI toolkit rejected the action".into());
        }
        let verified = match expected {
            Some(value) => read_text(connection, &node.object).is_ok_and(|actual| actual == value),
            None => false,
        };
        Ok(AdapterActResult {
            applied: true,
            outcome: None,
            postcondition_ok: verified,
            verifiable: verified,
            detail: if verified {
                "AT-SPI text verified by exact native readback"
            } else {
                "AT-SPI action returned; postcondition not verified"
            }
            .into(),
        })
    }

    pub fn native_completion(
        &mut self,
    ) -> Option<grok_computer_use_core::native_action::NativeActionRecovery> {
        let completion = self.calls.take_completion()?;
        self.snapshots.clear();
        Some(completion)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clipped_display_is_explicit_and_preserves_unicode_boundaries() {
        assert_eq!(bounded_display("中🙂文", 2), ("中🙂".into(), true));
        assert_eq!(bounded_display("中🙂", 2), ("中🙂".into(), false));
        assert_eq!(bounded_display("", 0), (String::new(), false));
        assert_eq!(bounded_display("中", 0), (String::new(), true));
    }

    #[test]
    fn unicode_offsets_are_characters_not_utf8_bytes() {
        assert_eq!(inserted("中🙂文", 2, "测试").unwrap(), "中🙂测试文");
        assert_eq!(inserted("中", 0, "a").unwrap(), "a中");
        assert_eq!(inserted("中", 1, "a").unwrap(), "中a");
        assert!(inserted("中", -1, "a").is_err());
        assert!(inserted("中", 2, "a").is_err());
    }
    #[test]
    fn defunct_disabled_or_hidden_controls_never_admit_writes() {
        let ready = (1 << ENABLED) | (1 << SENSITIVE) | (1 << SHOWING) | (1 << VISIBLE);
        assert!(actionable(&[ready]));
        assert!(!actionable(&[ready | (1 << 6)]));
        for bit in [ENABLED, SENSITIVE, SHOWING, VISIBLE] {
            assert!(!actionable(&[ready & !(1 << bit)]));
        }
        assert!(!actionable(&[]));
        // Disabled but visible labels remain eligible for read-only Wait.
        assert!(visible(&[(1 << SHOWING) | (1 << VISIBLE)]));
        assert!(!visible(&[(1 << SENSITIVE) | (1 << SHOWING)]));
    }
    #[test]
    fn insertion_limits_result_not_only_input() {
        assert!(inserted(&"x".repeat(MAX_TEXT), 0, "中").is_err());
    }
    #[test]
    fn gtk_press_release_are_not_ambiguous_complete_clicks() {
        let actions = |names: &[&str]| {
            names
                .iter()
                .map(|s| (s.to_string(), String::new(), String::new()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            primary_action_index(&actions(&["click", "press", "release"])),
            Some(0)
        );
        assert_eq!(primary_action_index(&actions(&["Click"])), Some(0));
        assert_eq!(
            primary_action_index(&actions(&["press", "activate", "release"])),
            Some(1)
        );
        assert_eq!(primary_action_index(&actions(&["press", "release"])), None);
        assert_eq!(primary_action_index(&actions(&["click", "click"])), None);
    }
}

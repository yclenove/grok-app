//! Independent native event/image oracle. This is deliberately unavailable in
//! product builds, and refuses ordinary desktop displays even in probe builds.
use crate::{client::err, ComputerUseAdapter, DispatchRequest, LinuxAdapter};
use grok_computer_use_core::adapter::ActionScope;
use grok_computer_use_core::protocol::{ActionKind, ActionTarget};
use serde_json::json;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{self, ConnectionExt, EventMask, WindowClass};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

pub fn run_native_selftest() -> Result<(), String> {
    if std::env::var("GROK_CU_X11_FIXTURE").as_deref() != Ok("owned-xvfb") {
        return Err(
            "owned Xvfb runner required; refusing ordinary desktop fixture mutation".into(),
        );
    }
    crate::require_native_x11()?;
    let (conn, screen) = x11rb::connect(None).map_err(err)?;
    let root = conn.setup().roots[screen].root;
    let frame = window(&conn, root, 70, 55, 400, 300)?;
    let app = window(&conn, frame, 17, 23, 200, 160)?;
    let other = window(&conn, root, 600, 50, 150, 150)?;
    title(&conn, app, "CU owned X11 target")?;
    title(&conn, other, "CU owned X11 other")?;
    let atom = conn
        .intern_atom(false, b"_NET_CLIENT_LIST_STACKING")
        .map_err(err)?
        .reply()
        .map_err(err)?
        .atom;
    conn.change_property32(
        xproto::PropMode::REPLACE,
        root,
        atom,
        xproto::AtomEnum::WINDOW,
        &[app, other],
    )
    .map_err(err)?
    .check()
    .map_err(err)?;
    for id in [frame, app, other] {
        conn.map_window(id).map_err(err)?.check().map_err(err)?;
    }
    focus(&conn, app)?;
    let adapter = LinuxAdapter::new();
    let targets = adapter.list_targets()?;
    let target = targets
        .iter()
        .find(|t| t.title == "CU owned X11 target")
        .ok_or("fixture target not enumerated")?
        .target_id
        .clone();
    let observation = adapter.observe_for_run("owned-x11-run", &target)?;
    if (observation.origin_x, observation.origin_y) != (87, 78) {
        return Err("reparented client root-origin mismatch".into());
    }
    let data = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        observation
            .image
            .png_base64
            .as_ref()
            .ok_or("missing native screenshot")?,
    )
    .map_err(err)?;
    let image = image::load_from_memory(&data).map_err(err)?.to_rgb8();
    if image.dimensions() != (200, 160) || image.get_pixel(10, 10).0 != [128, 64, 32] {
        return Err("native screenshot did not preserve the actual window pixels".into());
    }
    println!("PASS reparented root coordinates and independently decoded native pixels");
    drain(&conn)?;
    let mut request = DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "owned-x11-run".into(),
        action_id: "click".into(),
        generation: 1,
        target_id: target.clone(),
        target_generation: 1,
        snapshot_id: observation.snapshot_id,
        geometry_revision: observation.geometry_revision,
        action: ActionKind::Click,
        target: ActionTarget::Coord { x: 20.0, y: 30.0 },
        parameters: json!({"count":2}),
        scope: ActionScope::Directed,
    };
    let mut foreign = request.clone();
    foreign.run_id = "foreign-x11-run".into();
    assert_rejected(&adapter, &conn, &foreign, "foreign model screenshot")?;
    foreign = request.clone();
    foreign.snapshot_id = "forged-snapshot".into();
    assert_rejected(&adapter, &conn, &foreign, "forged model screenshot")?;
    let unscoped = adapter.observe(&target)?;
    foreign.snapshot_id = unscoped.snapshot_id;
    assert_rejected(&adapter, &conn, &foreign, "unscoped preview screenshot")?;
    let preview = adapter.capture_for_run(
        "owned-x11-run",
        &target,
        grok_computer_use_core::adapter::CaptureOptions {
            for_model: false,
            screenshot: true,
            managed_request: None,
            cancellation: Default::default(),
        },
    )?;
    foreign.snapshot_id = preview.snapshot_id;
    assert_rejected(&adapter, &conn, &foreign, "preview screenshot")?;
    println!(
        "PASS foreign, forged and preview-only snapshots never authorize native coordinate input"
    );
    let no_image = adapter.capture_for_run(
        "no-image-run",
        &target,
        grok_computer_use_core::adapter::CaptureOptions::model(false),
    )?;
    foreign.run_id = no_image.run_id;
    foreign.snapshot_id = no_image.snapshot_id;
    assert_rejected(&adapter, &conn, &foreign, "screenshot-disabled model")?;
    adapter.release_target_for_run("no-image-run", &target);
    println!(
        "PASS screenshot-disabled model and another run's release preserve strict image authority"
    );
    adapter.act(&request)?;
    let events = drain(&conn)?;
    if presses(&events,app,1) != 2 || !events.iter().any(|e| matches!(e,Event::ButtonPress(e) if e.event == app && e.event_x == 20 && e.event_y == 30)) {
        return Err("native double click did not arrive twice at the exact client point".into());
    }
    for (button, detail) in [("middle", 2), ("right", 3)] {
        request.parameters = json!({"button":button});
        adapter.act(&request)?;
        if presses(&drain(&conn)?, app, detail) != 1 {
            return Err("native named button mismatch".into());
        }
    }
    println!("PASS directed left/double/middle/right click event counts");
    request.action = ActionKind::Scroll;
    // Protocol/Choice down is positive, X11 button 5 is down; 4 is up.
    for (delta, detail, count) in [(240, 5, 2), (-120, 4, 1)] {
        request.parameters = json!({"delta":delta});
        adapter.act(&request)?;
        if presses(&drain(&conn)?, app, detail) != count {
            return Err("native wheel direction/count mismatch".into());
        }
    }
    println!("PASS protocol positive-down/negative-up native scroll direction and count");
    request.parameters = json!({"delta":0});
    let pointer_before = conn
        .query_pointer(root)
        .map_err(err)?
        .reply()
        .map_err(err)?;
    let result = adapter.act(&request)?;
    let events = drain(&conn)?;
    let pointer_after = conn
        .query_pointer(root)
        .map_err(err)?
        .reply()
        .map_err(err)?;
    if result.applied
        || (pointer_before.root_x, pointer_before.root_y)
            != (pointer_after.root_x, pointer_after.root_y)
        || events.iter().any(|e| {
            matches!(
                e,
                Event::ButtonPress(_) | Event::ButtonRelease(_) | Event::MotionNotify(_)
            )
        })
    {
        return Err("zero scroll must report not-applied without moving or injecting input".into());
    }
    for parameters in [
        json!({}),
        json!({"delta":1.5}),
        json!({"delta":2401}),
        json!({"delta":120,"extra":1}),
    ] {
        request.parameters = parameters;
        assert_rejected(&adapter, &conn, &request, "invalid native scroll")?;
    }
    println!("PASS zero scroll truthful no-op and invalid scroll zero native input");
    request.action = ActionKind::Drag;
    request.parameters = json!({"toX":100,"toY":90});
    adapter.act(&request)?;
    let events = drain(&conn)?;
    if presses(&events,app,1) != 1 || !events.iter().any(|e| matches!(e,Event::ButtonRelease(e) if e.event==app && e.event_x==100 && e.event_y==90)) {
        return Err("native drag endpoint/release mismatch".into());
    }
    println!("PASS native drag down/path/up at exact destination");
    request.action = ActionKind::Key;
    for (name, symbol) in [
        ("enter", 0xff0d),
        ("tab", 0xff09),
        ("escape", 0xff1b),
        ("space", 0x20),
        ("left", 0xff51),
        ("up", 0xff52),
        ("right", 0xff53),
        ("down", 0xff54),
        ("backspace", 0xff08),
        ("delete", 0xffff),
        ("home", 0xff50),
        ("end", 0xff57),
        ("pageup", 0xff55),
        ("pagedown", 0xff56),
    ] {
        let expected = key_code(&conn, symbol)?;
        request.parameters = json!({"key":name});
        adapter.act(&request)?;
        let keys: Vec<_> = drain(&conn)?
            .into_iter()
            .filter_map(|event| match event {
                Event::KeyPress(e) => Some((2, e.event, e.detail)),
                Event::KeyRelease(e) => Some((3, e.event, e.detail)),
                _ => None,
            })
            .collect();
        if keys != [(2, app, expected), (3, app, expected)] {
            return Err(format!("native {name} keycode/press/release mismatch"));
        }
    }
    println!("PASS all 14 named keys: exact native keycodes and press/release pairs");
    request.action = ActionKind::Click;
    request.parameters = json!({});
    for (name, symbol) in [("CapsLock", 0xffe5), ("NumLock", 0xff7f)] {
        let code = key_code(&conn, symbol)?;
        let before = conn
            .query_pointer(root)
            .map_err(err)?
            .reply()
            .map_err(err)?
            .mask;
        key_event(&conn, root, 2, code)?;
        key_event(&conn, root, 3, code)?;
        drain(&conn)?;
        let result = (|| {
            let locked = conn
                .query_pointer(root)
                .map_err(err)?
                .reply()
                .map_err(err)?
                .mask;
            if locked == before || u16::from(locked) & 0xff == 0 {
                return Err(format!(
                    "{name}: native fixture did not enable a lock modifier"
                ));
            }
            adapter.act(&request)?;
            if presses(&drain(&conn)?, app, 1) != 1 {
                return Err(format!("{name}: click did not arrive exactly once"));
            }
            Ok(())
        })();
        key_event(&conn, root, 2, code)?;
        key_event(&conn, root, 3, code)?;
        drain(&conn)?;
        if conn
            .query_pointer(root)
            .map_err(err)?
            .reply()
            .map_err(err)?
            .mask
            != before
        {
            return Err(format!("{name}: fixture failed to restore lock state"));
        }
        result?;
    }
    println!("PASS latched CapsLock and NumLock do not block native pointer input");
    for (name, symbol) in [("Shift", 0xffe1), ("a", 0x61)] {
        let code = key_code(&conn, symbol)?;
        key_event(&conn, root, 2, code)?;
        drain(&conn)?;
        let result = assert_rejected(&adapter, &conn, &request, name);
        let keymap = conn.query_keymap().map_err(err)?.reply().map_err(err)?.keys;
        key_event(&conn, root, 3, code)?;
        drain(&conn)?;
        result?;
        if keymap[usize::from(code / 8)] & (1 << (code % 8)) == 0 {
            return Err(format!("{name}: adapter released a user-owned key"));
        }
    }
    println!("PASS physically held modifier and ordinary key reject input without release");
    focus(&conn, other)?;
    assert_rejected(&adapter, &conn, &request, "focus drift")?;
    focus(&conn, app)?;
    let overlay = window(&conn, root, 87, 78, 100, 100)?;
    conn.map_window(overlay)
        .map_err(err)?
        .check()
        .map_err(err)?;
    assert_rejected(&adapter, &conn, &request, "occluded target")?;
    conn.destroy_window(overlay)
        .map_err(err)?
        .check()
        .map_err(err)?;
    println!("PASS focus drift and occlusion: zero native input");
    conn.configure_window(app, &xproto::ConfigureWindowAux::new().x(30))
        .map_err(err)?
        .check()
        .map_err(err)?;
    assert_rejected(&adapter, &conn, &request, "moved target")?;
    conn.configure_window(app, &xproto::ConfigureWindowAux::new().x(17))
        .map_err(err)?
        .check()
        .map_err(err)?;
    assert_rejected(&adapter, &conn, &request, "moved-back target")?;
    let refreshed = adapter.observe_for_run("owned-x11-run", &target)?;
    request.geometry_revision = refreshed.geometry_revision;
    request.snapshot_id = refreshed.snapshot_id;
    println!("PASS geometry epoch remains retired after moving back");
    conn.configure_window(frame, &xproto::ConfigureWindowAux::new().x(90))
        .map_err(err)?
        .check()
        .map_err(err)?;
    assert_rejected(&adapter, &conn, &request, "moved parent")?;
    conn.configure_window(frame, &xproto::ConfigureWindowAux::new().x(70))
        .map_err(err)?
        .check()
        .map_err(err)?;
    assert_rejected(&adapter, &conn, &request, "moved-back parent")?;
    let refreshed = adapter.observe_for_run("owned-x11-run", &target)?;
    request.geometry_revision = refreshed.geometry_revision;
    request.snapshot_id = refreshed.snapshot_id;
    println!("PASS parent movement retires the child snapshot even after moving back");
    request.target = ActionTarget::Element {
        element_ref: "root".into(),
    };
    assert_rejected(&adapter, &conn, &request, "semantic fallback")?;
    request.target = ActionTarget::Coord { x: 20.0, y: 30.0 };
    request.action = ActionKind::Drag;
    request.parameters = json!({"toX":100,"toY":999});
    assert_rejected(&adapter, &conn, &request, "out-of-bounds drag")?;
    request.action = ActionKind::Key;
    request.parameters = json!({"key":"alt+f4"});
    assert_rejected(&adapter, &conn, &request, "unsupported key")?;
    request.action = ActionKind::Click;
    request.parameters = json!({});
    println!("PASS unsupported semantic/key and bad destination: zero input");
    request.cancellation.cancel();
    assert_rejected(&adapter, &conn, &request, "cancelled request")?;
    request.cancellation = Default::default();
    request.generation = 3;
    adapter.abort(&request.run_id, 2)?;
    assert_rejected(&adapter, &conn, &request, "stopped observation")?;
    let refreshed = adapter.observe_for_run("owned-x11-run", &target)?;
    request.geometry_revision = refreshed.geometry_revision;
    request.snapshot_id = refreshed.snapshot_id;
    adapter.act(&request)?;
    drain(&conn)?;
    adapter.abort(&request.run_id, 2)?;
    adapter.act(&request)?;
    if presses(&drain(&conn)?, app, 1) != 1 {
        return Err("late old abort disrupted new request".into());
    }
    println!("PASS canceled request rejection and late-old-abort isolation");
    conn.xtest_fake_input(4, 1, 0, root, 0, 0, 0)
        .map_err(err)?
        .check()
        .map_err(err)?;
    drain(&conn)?;
    let held_result = assert_rejected(&adapter, &conn, &request, "user-held button");
    conn.xtest_fake_input(5, 1, 0, root, 0, 0, 0)
        .map_err(err)?
        .check()
        .map_err(err)?;
    held_result?;
    drain(&conn)?;
    println!("PASS user-held button prevents adapter input");
    conn.unmap_window(app).map_err(err)?.check().map_err(err)?;
    conn.map_window(app).map_err(err)?.check().map_err(err)?;
    if adapter.target_alive(&target) {
        return Err("unmapped/remapped lifetime retained old authority".into());
    }
    let remapped = adapter
        .list_targets()?
        .into_iter()
        .find(|t| t.title == "CU owned X11 target")
        .ok_or("remapped target unavailable")?;
    if remapped.target_id == target {
        return Err("native remap did not rotate identity".into());
    }
    println!("PASS unmap/remap revokes target identity");
    conn.destroy_window(app)
        .map_err(err)?
        .check()
        .map_err(err)?;
    create_window(&conn, app, frame, 17, 23, 200, 160)?;
    title(&conn, app, "CU owned X11 target")?;
    conn.map_window(app).map_err(err)?.check().map_err(err)?;
    if adapter.target_alive(&remapped.target_id) {
        return Err("reused XID inherited old target authority".into());
    }
    let replacement = adapter
        .list_targets()?
        .into_iter()
        .find(|t| t.title == "CU owned X11 target")
        .ok_or("replacement target unavailable")?;
    if replacement.target_id == remapped.target_id {
        return Err("reused XID retained identity".into());
    }
    if !adapter.is_idle(&request.run_id) {
        return Err("terminal native operations left occupancy behind".into());
    }
    println!("PASS destroy/recreate same XID cannot inherit authorization");
    conn.unmap_window(frame)
        .map_err(err)?
        .check()
        .map_err(err)?;
    conn.map_window(frame).map_err(err)?.check().map_err(err)?;
    if adapter.target_alive(&replacement.target_id) {
        return Err("hidden/remapped parent retained child authorization".into());
    }
    let restored = adapter
        .list_targets()?
        .into_iter()
        .find(|t| t.title == "CU owned X11 target")
        .ok_or("restored child unavailable")?;
    if restored.target_id == replacement.target_id {
        return Err("remapped parent did not rotate child identity".into());
    }
    println!("PASS hidden/remapped parent revokes its child target identity");
    // Connection-owned windows disappear when this fixture connection closes.
    Ok(())
}

fn window(c: &RustConnection, parent: u32, x: i16, y: i16, w: u16, h: u16) -> Result<u32, String> {
    let id = c.generate_id().map_err(err)?;
    create_window(c, id, parent, x, y, w, h)?;
    Ok(id)
}
fn create_window(
    c: &RustConnection,
    id: u32,
    parent: u32,
    x: i16,
    y: i16,
    w: u16,
    h: u16,
) -> Result<(), String> {
    c.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        id,
        parent,
        x,
        y,
        w,
        h,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &xproto::CreateWindowAux::new()
            .background_pixel(0x804020)
            .event_mask(
                EventMask::BUTTON_PRESS
                    | EventMask::BUTTON_RELEASE
                    | EventMask::POINTER_MOTION
                    | EventMask::KEY_PRESS
                    | EventMask::KEY_RELEASE
                    | EventMask::STRUCTURE_NOTIFY,
            ),
    )
    .map_err(err)?
    .check()
    .map_err(err)
}
fn title(c: &RustConnection, id: u32, value: &str) -> Result<(), String> {
    c.change_property8(
        xproto::PropMode::REPLACE,
        id,
        xproto::AtomEnum::WM_NAME,
        xproto::AtomEnum::STRING,
        value.as_bytes(),
    )
    .map_err(err)?
    .check()
    .map_err(err)
}
fn focus(c: &RustConnection, id: u32) -> Result<(), String> {
    c.set_input_focus(xproto::InputFocus::PARENT, id, x11rb::CURRENT_TIME)
        .map_err(err)?
        .check()
        .map_err(err)
}
fn key_code(c: &RustConnection, symbol: u32) -> Result<u8, String> {
    let setup = c.setup();
    let keys = c
        .get_keyboard_mapping(setup.min_keycode, setup.max_keycode - setup.min_keycode + 1)
        .map_err(err)?
        .reply()
        .map_err(err)?;
    if keys.keysyms_per_keycode == 0 {
        return Err("fixture keyboard mapping is empty".into());
    }
    let index = keys
        .keysyms
        .chunks_exact(usize::from(keys.keysyms_per_keycode))
        .position(|entry| entry[0] == symbol)
        .ok_or("fixture key absent")?;
    setup
        .min_keycode
        .checked_add(u8::try_from(index).map_err(err)?)
        .ok_or("fixture keycode overflow".into())
}
fn key_event(c: &RustConnection, root: u32, kind: u8, code: u8) -> Result<(), String> {
    c.xtest_fake_input(kind, code, 0, root, 0, 0, 0)
        .map_err(err)?
        .check()
        .map_err(err)
}
fn drain(c: &RustConnection) -> Result<Vec<Event>, String> {
    c.get_input_focus().map_err(err)?.reply().map_err(err)?;
    let mut result = vec![];
    while let Some(event) = c.poll_for_event().map_err(err)? {
        result.push(event);
        if result.len() > 4096 {
            return Err("fixture event overflow".into());
        }
    }
    Ok(result)
}
fn presses(events: &[Event], target: u32, detail: u8) -> usize {
    events
        .iter()
        .filter(|e| matches!(e,Event::ButtonPress(e) if e.event==target && e.detail==detail))
        .count()
}
fn assert_rejected(
    adapter: &LinuxAdapter,
    c: &RustConnection,
    request: &DispatchRequest,
    label: &str,
) -> Result<(), String> {
    drain(c)?;
    if adapter.act(request).is_ok() {
        return Err(format!("{label}: expected explicit rejection"));
    }
    if drain(c)?.iter().any(|e| {
        matches!(
            e,
            Event::ButtonPress(_)
                | Event::ButtonRelease(_)
                | Event::KeyPress(_)
                | Event::KeyRelease(_)
                | Event::MotionNotify(_)
        )
    }) {
        return Err(format!("{label}: rejection injected native input"));
    }
    Ok(())
}

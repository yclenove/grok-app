use super::*;
use gcu_core::model::{NodeAction, Role, States};
use serde_json::json;

pub async fn run(
    id: &str,
    driver: &WindowsDriver,
    fixture: &Fixture,
    win: &WindowRef,
) -> CaseResult {
    if requires_foreground(id)
        && driver.capabilities().delivery.foreground
            == gcu_protocol::capabilities::ForegroundCapability::Unavailable
    {
        let report = driver.doctor().await;
        return foreground_unavailable(
            &report.issues,
            super::super::unsafe_mouse_probe::decision()?,
        );
    }
    if requires_foreground(id) {
        if let Some(skip) = silent_mouse_environment(super::super::unsafe_mouse_probe::decision()?)
        {
            return skip;
        }
    }
    let result = run_case(id, driver, fixture, win).await;
    // ADR 0013: the foreground capability stays open while only the mouse proof
    // is missing, and the action-time proof refuses instead. Judge that refusal
    // with the same measured-environment decision: it may Skip only when the
    // same-process mouse matrix saw no callbacks at all, and fails whenever a
    // working probe exists.
    if requires_foreground(id) {
        if let Err(error) = &result {
            if error.contains("capability unavailable") {
                let report = driver.doctor().await;
                return foreground_unavailable(
                    &report.issues,
                    super::super::unsafe_mouse_probe::decision()?,
                );
            }
        }
    }
    result
}

async fn run_case(
    id: &str,
    driver: &WindowsDriver,
    fixture: &Fixture,
    win: &WindowRef,
) -> CaseResult {
    match id {
        "C01" => {
            require(driver.list_apps().await.map_err(|e| e.to_string())?.iter().any(|a| a.pid == Some(fixture.pid())), "fixture absent from list_apps")?;
            for query in [&win.app.info.name, &win.app.info.id, &std::env::var("GCU_FIXTURE_EXE").map_err(|e| e.to_string())?] {
                require(driver.resolve_app(&AppQuery(query.clone())).await.map_err(|e| e.to_string())? == win.app, "name/ID/path changed identity")?;
            }
        }
        "C02" => return Ok(Outcome::Skip("fixture: executable has no Start Menu launch record; launching an already running app is a no-op, not a launch oracle".into())),
        "C03" => {
            let raw = tree(driver, win).await?;
            let root = raw.root.as_ref().ok_or("empty UIA tree")?;
            for (id, roles) in [
                ("name_field", &[Role::TextField][..]),
                ("multiline_field", &[Role::TextField, Role::TextArea][..]),
                ("agree_checkbox", &[Role::CheckBox][..]),
                ("slider_field", &[Role::Slider][..]),
                ("counter_button", &[Role::Button][..]),
                ("send_button", &[Role::Button][..]),
                ("submit_button", &[Role::Button][..]),
                ("open_modal_button", &[Role::Button][..]),
                ("grid_canvas", &[Role::Image][..]),
            ] {
                let control = find(root, id).ok_or_else(|| format!("missing {id}; last tree={raw:?}"))?;
                require(roles.contains(&control.role), format!("{id}: unexpected role {:?}", control.role))?;
            }
            require(find(root, "grid_canvas").unwrap().children.is_empty(), "canvas must have no accessible cells")?;
        }
        "C04" => return capture_occluded(driver, fixture, win).await,
        "C05" => {
            let before = fixture.read()?["counter"].as_u64().ok_or("counter missing")?;
            press(driver, win, "counter_button").await?;
            fixture.wait("counter exactly once", |v| v["counter"] == before + 1).await?;
            // A read barrier after the state write catches queued duplicate effects.
            tree(driver, win).await?;
            require(fixture.read()?["counter"] == before + 1, "counter replayed")?;
        }
        "C06" => {
            let grid = node(driver, win, "grid_canvas").await?.frame.ok_or("grid frame missing")?;
            let before = fixture.read()?["grid_hit"].clone();
            let (col, row) = if before == json!([2,3]) { (1,2) } else { (2,3) };
            let at = WindowPoint { x: grid.x + (f64::from(col) + 0.25) * grid.w / 8., y: grid.y + (f64::from(row) + 0.25) * grid.h / 6. };
            let capture = driver.capture(win, &Default::default(), &Cancel::default()).await.map_err(|e| e.to_string())?;
            let pixels = decode(&capture.bytes)?;
            check_grid_pixel(&pixels, capture.width, at, col, row)?;
            click(driver, win, at).await?;
            fixture.wait("grid pixel coordinate", |v| v["grid_hit"] == json!([col,row])).await?;
        }
        "C07" => {
            let field = node(driver, win, "name_field").await?;
            if !field.states.contains(States::SETTABLE) { return Ok(Outcome::Skip("provider: egui name_field has no writable UIA ValuePattern; foreground text is covered by C08/C09".into())); }
            perform(driver, DriverAction::SetValue { el: field.handle, value: "UIA 中文 🦀".into() }).await?;
            fixture.wait("SetValue text", |v| v["text"] == "UIA 中文 🦀").await?;
        }
        "C08" => {
            focus(driver, fixture, win, "multiline_field").await?;
            clear(driver, fixture, win, "multiline").await?;
            // BMP text, a zero-width joiner and a newline (a real Enter key). Supplementary-plane
            // characters are not part of this assertion: on win-test (2026-10-21) the egui/winit fixture
            // dropped them for every order of the two surrogate key events (`live_surrogate_probe`).
            let input = "中文 \u{200d}\nsecond line";
            text(driver, win, input).await?;
            fixture.wait("Unicode bytes and newline", |v| v["multiline"].as_str() == Some(input)).await?;
            return Ok(Outcome::Skip("provider: egui/winit ignores supplementary-plane characters (emoji) sent as VK_PACKET surrogate pairs in all six measured event orders; BMP Unicode, a zero-width joiner and newline (Enter key) are verified".into()));
        }
        "C09" => {
            focus(driver, fixture, win, "name_field").await?;
            text(driver, win, "clear-me").await?;
            fixture.wait("nonempty before clear", |v| v["text"].as_str().is_some_and(|s| s.ends_with("clear-me"))).await?;
            clear(driver, fixture, win, "text").await?;
        }
        "C10" => {
            let before = fixture.read()?["checked"].as_bool().ok_or("checked missing")?;
            perform(driver, DriverAction::SecondaryAction { el: node(driver, win, "agree_checkbox").await?.handle, action: NodeAction::Toggle }).await?;
            fixture.wait("checkbox toggled", |v| v["checked"] == !before).await?;
        }
        "C11" => return dropdown(driver, fixture, win).await,
        "C12" => {
            perform(driver, DriverAction::SetValue { el: node(driver, win, "slider_field").await?.handle, value: "37".into() }).await?;
            fixture.wait("slider range value", |v| v["slider"] == 37.0).await?;
        }
        "C13" => {
            let before = fixture.read()?["list_visible_range"][0].as_u64().ok_or("visible_range missing")?;
            for page in 1..=2 {
                let raw = tree(driver, win).await?;
                let Some(list) = virtual_scroller(raw.root.as_ref().ok_or("root missing")?) else {
                    return Ok(Outcome::Skip("provider: virtual list lacks UIA ScrollPattern; wheel detents do not prove the two-page C13 contract".into()));
                };
                let prior = fixture.read()?["list_visible_range"][0].as_u64().unwrap();
                perform(driver, DriverAction::ScrollElement { el: list.handle.clone(), dir: Dir::Down, pages: 1.0 }).await?;
                fixture.wait(&format!("list page {page}"), |v| v["list_visible_range"][0].as_u64().is_some_and(|n| n > prior)).await?;
            }
            require(fixture.read()?["list_visible_range"][0].as_u64().unwrap() > before, "list did not move")?;
        }
        "C14" => return modal(driver, fixture, win).await,
        "C15" => {
            let start = center(&node(driver, win, "drag_source").await?)?;
            let end = center(&node(driver, win, "drop_target").await?)?;
            require(fixture.read()?["drop_ok"] == false, "drag oracle already true")?;
            perform(driver, DriverAction::PointerDrag { win: win.clone(), path: vec![start, end], route: PointerRoute::AllowForeground }).await?;
            fixture.wait("drop target", |v| v["drop_ok"] == true).await?;
        }
        "C16" => return background::virtual_element(driver, fixture, win).await,
        "C16-BUTTON" => return background::native_button(driver).await,
        "C17" => return safety::geometry_gate(driver, fixture, win).await,
        "C18" => return removed_element(driver, fixture, win).await,
        "C19" => return closed_window(driver, fixture, win).await,
        "C20" => return safety::redaction(driver, fixture, win).await,
        "C21" => return overlay(driver, fixture, win).await,
        "C22" => return safety::worker_timeout(driver, fixture, win).await,
        "C23" => return Ok(Outcome::Skip("fixture: public DriverAction cannot hold Shift across requests; parent-ledger interrupted-input coverage is in tests/input and tests/stop (not counted as native key-release evidence)".into())),
        "C24" => return virtual_list(driver, fixture, win).await,
        "C25" => return Ok(Outcome::Skip("scope: same-geometry pixel replacement requires a fixture render-control channel; driver has no screenshot-ID gate; core router regression covers this separately".into())),
        "C26" => return safety::execution_surfaces(driver, fixture, win).await,
        "C27" => return Ok(Outcome::Skip("fixture: native physical hotkey/UAC/blocked-FFI latency matrix needs a native fault fixture; deterministic listener clocks run separately; W-HOTKEY tests real registration conflict".into())),
        "W-UIPI" => return safety::uipi(fixture).await,
        "W-HOTKEY" => return safety::hotkey_conflict(driver, fixture, win).await,
        _ => return Err(format!("unregistered contract {id}")),
    }
    Ok(Outcome::Passed)
}

pub async fn focus(
    driver: &WindowsDriver,
    fixture: &Fixture,
    win: &WindowRef,
    id: &str,
) -> Result<(), String> {
    click(driver, win, center(&node(driver, win, id).await?)?).await?;
    fixture
        .wait("external focused_widget", |v| v["focused_widget"] == id)
        .await?;
    Ok(())
}
pub async fn clear_for_probe(
    driver: &WindowsDriver,
    fixture: &Fixture,
    win: &WindowRef,
) -> Result<(), String> {
    clear(driver, fixture, win, "multiline").await
}
async fn clear(
    driver: &WindowsDriver,
    fixture: &Fixture,
    win: &WindowRef,
    field: &str,
) -> Result<(), String> {
    key(driver, win, "ctrl+a").await?;
    key(driver, win, "BackSpace").await?;
    fixture.wait("cleared field", |v| v[field] == "").await?;
    Ok(())
}
pub fn named_prefix<'a>(root: &'a RawNode, prefix: &str) -> Option<&'a RawNode> {
    if root.name.as_deref().is_some_and(|n| n.starts_with(prefix)) {
        Some(root)
    } else {
        root.children.iter().find_map(|n| named_prefix(n, prefix))
    }
}
fn virtual_scroller(root: &RawNode) -> Option<&RawNode> {
    root.children.iter().find_map(virtual_scroller).or_else(|| {
        (root.actions.contains(&NodeAction::ScrollDown) && named_prefix(root, "Item 00").is_some())
            .then_some(root)
    })
}
pub fn decode(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .map_err(|e| e.to_string())?;
    let mut pixels = vec![0; reader.output_buffer_size().ok_or("PNG buffer limit")?];
    let info = reader.next_frame(&mut pixels).map_err(|e| e.to_string())?;
    require(
        info.color_type == png::ColorType::Rgba,
        "expected RGBA window PNG",
    )?;
    pixels.truncate(info.buffer_size());
    Ok(pixels)
}
fn check_grid_pixel(
    pixels: &[u8],
    width: u32,
    at: WindowPoint,
    col: i32,
    row: i32,
) -> Result<(), String> {
    let i = (at.y as usize * width as usize + at.x as usize) * 4;
    let expected = [
        ((col * 31 + 45) % 180 + 50) as u8,
        ((row * 41 + 65) % 180 + 50) as u8,
        (((col + row) * 29 + 85) % 180 + 50) as u8,
    ];
    require(
        pixels.get(i..i + 3) == Some(expected.as_slice()),
        format!(
            "grid ({col},{row}) at {at:?}: expected {expected:?}, last pixel={:?}",
            pixels.get(i..i + 3)
        ),
    )
}
async fn capture_occluded(
    driver: &WindowsDriver,
    fixture: &Fixture,
    win: &WindowRef,
) -> CaseResult {
    let grid = node(driver, win, "grid_canvas")
        .await?
        .frame
        .ok_or("grid frame missing")?;
    let geometry = unsafe_desktop::window_rect(win, fixture.pid())?;
    let cover = unsafe_desktop::Cover::start(geometry)?;
    // Occlusion is a Z-order property, independent of foreground ownership.
    for (col, row) in [(0, 0), (2, 3), (7, 5)] {
        cover.require_occludes(
            geometry.x + grid.x + (f64::from(col) + 0.25) * grid.w / 8.,
            geometry.y + grid.y + (f64::from(row) + 0.25) * grid.h / 6.,
        )?;
    }
    let capture = driver
        .capture(win, &Default::default(), &Cancel::default())
        .await
        .map_err(|e| e.to_string())?;
    require(
        f64::from(capture.width) == geometry.w && f64::from(capture.height) == geometry.h,
        format!(
            "capture {}x{} != external HWND {geometry:?}",
            capture.width, capture.height
        ),
    )?;
    let pixels = decode(&capture.bytes)?;
    for (col, row) in [(0, 0), (2, 3), (7, 5)] {
        check_grid_pixel(
            &pixels,
            capture.width,
            WindowPoint {
                x: grid.x + (f64::from(col) + 0.25) * grid.w / 8.,
                y: grid.y + (f64::from(row) + 0.25) * grid.h / 6.,
            },
            col,
            row,
        )?;
    }
    Ok(Outcome::Passed)
}
async fn dropdown(driver: &WindowsDriver, fixture: &Fixture, win: &WindowRef) -> CaseResult {
    let raw = tree(driver, win).await?;
    fn combo(n: &RawNode) -> Option<&RawNode> {
        if matches!(n.role, Role::ComboBox | Role::PopUpButton) {
            Some(n)
        } else {
            n.children.iter().find_map(combo)
        }
    }
    let combo = combo(raw.root.as_ref().ok_or("root missing")?).ok_or("dropdown missing")?;
    click(driver, win, center(combo)?).await?;
    let item = poll_async(
        &mut DesktopClock::default(),
        Duration::from_secs(5),
        "third popup item",
        async || {
            let raw = tree(driver, win).await?;
            named_prefix(raw.root.as_ref().ok_or("root missing")?, "Item 3 / 第三项")
                .cloned()
                .ok_or_else(|| format!("third popup item missing; last tree={raw:?}"))
        },
        |_| true,
    )
    .await?;
    click(driver, win, center(&item)?).await?;
    fixture
        .wait("third dropdown selection", |v| v["combo_selected"] == 2)
        .await?;
    Ok(Outcome::Passed)
}
async fn modal(driver: &WindowsDriver, fixture: &Fixture, win: &WindowRef) -> CaseResult {
    let count = fixture.read()?["modal_closed_count"].as_u64().unwrap();
    press(driver, win, "open_modal_button").await?;
    fixture
        .wait("modal open", |v| v["modal_open"] == true)
        .await?;
    let raw = tree(driver, win).await?;
    fn modal_flag(n: &RawNode) -> bool {
        n.states.contains(States::MODAL) || n.children.iter().any(modal_flag)
    }
    let marked = raw.root.as_ref().is_some_and(modal_flag);
    let priority = if marked {
        let processed = gcu_core::tree::Pipeline::default().run(raw, &Default::default());
        !processed.popups.is_empty()
            && processed.nodes().into_iter().any(|(_, n)| {
                n.node.name.as_deref() == Some("modal_confirm_button")
                    && n.flags.contains(gcu_core::model::NodeFlags::MODAL_CONTENT)
            })
    } else {
        false
    };
    press(driver, win, "modal_confirm_button").await?;
    fixture
        .wait("modal confirmed", |v| {
            v["modal_open"] == false && v["modal_closed_count"] == count + 1
        })
        .await?;
    if !marked {
        return Ok(Outcome::Skip("provider: egui Window lacks UIA IsModal; lifecycle exercised but modal-priority contract needs a native modal fixture".into()));
    }
    require(
        priority,
        "modal priority omitted the confirm control or its modal flags",
    )?;
    Ok(Outcome::Passed)
}
async fn removed_element(driver: &WindowsDriver, fixture: &Fixture, win: &WindowRef) -> CaseResult {
    let old = node(driver, win, "counter_button").await?;
    let count = fixture.read()?["counter"].clone();
    press(driver, win, "Canvas Grid / 画布网格 🎨").await?;
    fixture
        .wait("canvas tab removes counter", |v| v["tab"] == 1)
        .await?;
    let result = driver
        .perform(&DriverAction::Press { el: old.handle }, &Cancel::default())
        .await;
    let restore = press(driver, win, "All Controls / 全部控件 🎛️").await;
    require(
        matches!(result, Err(DriverError::StaleElement)),
        format!("deleted element: {result:?}"),
    )?;
    require(
        fixture.read()?["counter"] == count,
        "deleted element had an effect",
    )?;
    restore?;
    fixture
        .wait("restore all controls", |v| v["tab"] == 0)
        .await?;
    Ok(Outcome::Passed)
}
async fn closed_window(driver: &WindowsDriver, fixture: &Fixture, win: &WindowRef) -> CaseResult {
    // Run last: close the exact observed fixture while another own window is live.
    let cover = unsafe_desktop::Cover::start(unsafe_desktop::window_rect(win, fixture.pid())?)?;
    let before = fixture.read()?["counter"].clone();
    let other_inputs = cover.input_count();
    unsafe_desktop::close(win, fixture.pid())?;
    poll(
        &mut DesktopClock::default(),
        Duration::from_secs(5),
        "closed fixture HWND",
        || Ok(unsafe_desktop::pid(&win.id)),
        |pid| pid.is_none(),
    )
    .await?;
    // Closing the fixture can itself change foreground. Measure after close,
    // immediately before the rejected action; no activation is needed here.
    let foreground_before = unsafe_desktop::foreground();
    let result = driver
        .perform(
            &DriverAction::PointerClick {
                win: win.clone(),
                at: WindowPoint { x: 10., y: 10. },
                button: Button::Left,
                count: 1,
                route: PointerRoute::TargetedOnly,
            },
            &Cancel::default(),
        )
        .await;
    require(
        matches!(result, Err(DriverError::WindowGone)),
        format!("closed HWND: {result:?}"),
    )?;
    require(
        fixture.read()?["counter"] == before,
        "closed fixture changed",
    )?;
    require(
        cover.input_count() == other_inputs,
        "closed-window action sent input to the other window",
    )?;
    require(
        unsafe_desktop::foreground() == foreground_before,
        "closed-window action changed foreground",
    )?;
    Ok(Outcome::Passed)
}
async fn overlay(driver: &WindowsDriver, fixture: &Fixture, win: &WindowRef) -> CaseResult {
    if !driver.capabilities().overlay_excluded_from_capture {
        return Ok(Outcome::Skip(
            "capability: overlay_excluded_from_capture=false".into(),
        ));
    }
    use gcu_driver_windows::overlay::{OverlayExclusion, RealOverlayApi};
    let exclusion = OverlayExclusion::probe(Arc::new(RealOverlayApi)).await;
    require(
        exclusion.available(),
        "declared overlay exclusion failed its native probe",
    )?;
    let grid = node(driver, win, "grid_canvas")
        .await?
        .frame
        .ok_or("grid frame missing")?;
    let cover = unsafe_desktop::Cover::start(unsafe_desktop::window_rect(win, fixture.pid())?)?;
    require(
        exclusion.exclude(cover.hwnd).await,
        "display affinity not applied to own overlay",
    )?;
    let capture = driver
        .capture(win, &Default::default(), &Cancel::default())
        .await
        .map_err(|e| e.to_string())?;
    check_grid_pixel(
        &decode(&capture.bytes)?,
        capture.width,
        WindowPoint {
            x: grid.x + grid.w / 32.,
            y: grid.y + grid.h / 24.,
        },
        0,
        0,
    )?;
    Ok(Outcome::Passed)
}
async fn virtual_list(driver: &WindowsDriver, fixture: &Fixture, win: &WindowRef) -> CaseResult {
    let last = fixture.read()?;
    let start = last["list_visible_range"][0]
        .as_u64()
        .ok_or("range start")?;
    let end = last["list_visible_range"][1].as_u64().ok_or("range end")?;
    require(
        start < end && end <= 2000 && end - start < 50,
        format!("invalid virtual range: {last}"),
    )?;
    let tree = tree(driver, win).await?; // Driver TreeOpts carries the native deadline.
    fn rows(n: &RawNode, out: &mut Vec<String>) {
        if n.name
            .as_deref()
            .is_some_and(|s| s.starts_with("Item ") && s.contains("列表项"))
        {
            out.push(n.name.clone().unwrap());
        }
        for c in &n.children {
            rows(c, out);
        }
    }
    let mut names = Vec::new();
    rows(tree.root.as_ref().ok_or("root missing")?, &mut names);
    require(
        !names.is_empty() && names.len() <= (end - start) as usize,
        format!("virtual rows {names:?}, range [{start},{end})"),
    )?;
    for name in names {
        let row: u64 = name[5..9]
            .parse()
            .map_err(|e| format!("invalid row: {e}"))?;
        require(
            (start..end).contains(&row),
            format!("out-of-range row {row}"),
        )?;
    }
    Ok(Outcome::Passed)
}

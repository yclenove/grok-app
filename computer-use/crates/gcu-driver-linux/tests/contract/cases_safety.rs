//! Contract cases C14–C27 and P3 security specials: Lifecycle, Redaction, Stop, and Safety.

use super::support::*;
use gcu_core::{
    driver::{
        AppRef, Cancel, CaptureOpts, Delivery, Driver, DriverAction, DriverError, KeyRoute,
        PointerRoute, TreeOpts, WindowRef, DRIVER_DEADLINE,
    },
    model::{NativeHandle, NodeFlags, RawNode, Role, States, WindowPoint},
    redact::{self, CapturedFrame, NEUTRAL_COLOR},
    tree::Pipeline,
};
use gcu_driver_linux::{
    atspi::sensitive::{is_terminal_like, terminal_node_flags},
    identity::{needs_reauth, same_process_instance, AppIdentity},
    x11::{
        hotkey::{handle_hotkey_trigger, HotkeyConfig, SharedStopLatch, StopLatch},
        resolve_or_remap_keysym,
        watchdog::run_with_watchdog,
        FakeWindow, FakeX, InputLedger, XRootGeometry, XWindowGeometry,
    },
    LinuxDriver,
};
use gcu_protocol::{capabilities::ForegroundCapability, tools::MouseButton as Button, AppInfo};
use std::{path::PathBuf, sync::Arc, time::Duration};
use x11rb::connection::Connection;
use x11rb::wrapper::ConnectionExt;

fn collect_modal_diagnostic_nodes(node: &RawNode) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(n: &RawNode, out: &mut Vec<String>) {
        let name = n.name.as_deref().unwrap_or("");
        let ident = n.identifier.as_deref().unwrap_or("");
        let lower_name = name.to_ascii_lowercase();
        let lower_id = ident.to_ascii_lowercase();
        if lower_name.contains("modal")
            || lower_name.contains("dialog")
            || name.contains("弹窗")
            || name.contains("对话框")
            || lower_id.contains("modal")
            || n.states.contains(States::MODAL)
            || n.role == Role::Dialog
        {
            out.push(format!(
                "Node(role={:?}, native_role={}, name={:?}, id={:?}, states={:?})",
                n.role, n.native_role, n.name, n.identifier, n.states
            ));
        }
        for child in &n.children {
            walk(child, out);
        }
    }
    walk(node, &mut out);
    out
}

pub async fn test_c14_modal_lifecycle(
    driver: &LinuxDriver,
    win_ref: &WindowRef,
) -> Result<ContractOutcome, String> {
    let t0 = tokio::time::Instant::now();
    let cancel = Cancel::default();
    let opts = TreeOpts {
        timeout: Duration::from_secs(5),
        max_tree_nodes: 5000,
        ..Default::default()
    };
    let tree = driver
        .snapshot_tree(win_ref, &opts, &cancel)
        .await
        .map_err(|e| e.to_string())?;
    let root = tree.root.as_ref().ok_or("root missing")?;
    let open_btn = find_raw_node(root, &|n| {
        n.role == Role::Button
            && (n.identifier.as_deref() == Some("open_modal_button")
                || n.name.as_deref().is_some_and(|s| {
                    s.contains("open_modal_button")
                        || s.contains("Open Modal")
                        || s.contains("打开弹窗")
                }))
    })
    .ok_or("open modal button missing")?;

    driver
        .perform(
            &DriverAction::Press {
                el: open_btn.handle.clone(),
            },
            &cancel,
        )
        .await
        .map_err(|e| e.to_string())?;
    let t_press_open = t0.elapsed();

    // 1. Confirm modal opened in fixture state before querying AT-SPI tree
    poll_oracle(Duration::from_secs(5), |v| {
        if v.get("modal_open")?.as_bool() == Some(true) {
            Some(())
        } else {
            None
        }
    })
    .await
    .map_err(|e| {
        format!("C14 modal_open oracle confirmation failed: {e}; t_press={t_press_open:?}")
    })?;
    let t_oracle_open = t0.elapsed();

    // 2. Poll snapshot_tree until AT-SPI tree exposes modal_confirm_button (handling async AT-SPI update)
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut last_tree = None;
    let mut confirm_btn = None;

    while tokio::time::Instant::now() < deadline {
        let tree_modal = driver
            .snapshot_tree(win_ref, &opts, &cancel)
            .await
            .map_err(|e| e.to_string())?;
        if let Some(ref root_modal) = tree_modal.root {
            if let Some(btn) = find_raw_node(root_modal, &|n| {
                n.role == Role::Button
                    && (n.identifier.as_deref() == Some("modal_confirm_button")
                        || n.name.as_deref().is_some_and(|s| {
                            s.contains("modal_confirm_button")
                                || s.contains("OK")
                                || s.contains("确定")
                        }))
            }) {
                confirm_btn = Some(btn.clone());
                last_tree = Some(tree_modal);
                break;
            }
        }
        last_tree = Some(tree_modal);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let confirm_btn = match confirm_btn {
        Some(btn) => btn,
        None => {
            let fixture_state_dump = read_fixture_state()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "<no state read>".into());
            let modal_nodes = last_tree
                .as_ref()
                .and_then(|t| t.root.as_ref())
                .map(collect_modal_diagnostic_nodes)
                .unwrap_or_default();
            let top_level = last_tree
                .as_ref()
                .and_then(|t| t.root.as_ref())
                .map(|r| {
                    r.children
                        .iter()
                        .map(|c| {
                            format!("{:?}:{:?}", c.role, c.name.as_deref().unwrap_or("<none>"))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            return Err(format!(
                "modal confirm button missing after AT-SPI tree settle timeout; t_press={t_press_open:?}, t_oracle={t_oracle_open:?}; fixture_state={fixture_state_dump}; modal_related_nodes={modal_nodes:?}; root_children={top_level:?}"
            ));
        }
    };
    let t_found_modal = t0.elapsed();

    // 3. Press modal confirm button and verify modal closed
    driver
        .perform(
            &DriverAction::Press {
                el: confirm_btn.handle.clone(),
            },
            &cancel,
        )
        .await
        .map_err(|e| e.to_string())?;
    let t_press_confirm = t0.elapsed();

    let closed = poll_oracle(Duration::from_secs(5), |v| {
        let is_open = v.get("modal_open")?.as_bool()?;
        let was_closed = v.get("modal_closed")?.as_bool()?;
        let closed_count = v.get("modal_closed_count")?.as_u64()?;
        if !is_open && (was_closed || closed_count > 0) {
            Some(true)
        } else {
            None
        }
    })
    .await
    .map_err(|e| {
        format!(
            "C14 modal closure verification failed: {e}; t_found_modal={t_found_modal:?}, t_press_confirm={t_press_confirm:?}"
        )
    })?;
    assert!(closed);
    Ok(ContractOutcome::Passed)
}

pub async fn test_c15_drag_and_drop(
    driver: &LinuxDriver,
    win_ref: &WindowRef,
) -> Result<ContractOutcome, String> {
    let cancel = Cancel::default();
    let opts = TreeOpts::default();
    let tree = driver
        .snapshot_tree(win_ref, &opts, &cancel)
        .await
        .map_err(|e| e.to_string())?;
    let root = tree.root.as_ref().ok_or("root missing")?;
    let src = find_raw_node(root, &|n| {
        n.name.as_deref() == Some("drag_source")
            || n.name.as_deref().is_some_and(|s| s.contains("Drag Me"))
    })
    .ok_or("drag_source missing")?;
    let target = find_raw_node(root, &|n| {
        n.name.as_deref() == Some("drop_target")
            || n.name.as_deref().is_some_and(|s| s.contains("Target"))
    })
    .ok_or("drop_target missing")?;

    let src_f = src.frame.ok_or("src frame missing")?;
    let tgt_f = target.frame.ok_or("tgt frame missing")?;

    let path = vec![
        WindowPoint {
            x: src_f.x + src_f.w / 2.0,
            y: src_f.y + src_f.h / 2.0,
        },
        WindowPoint {
            x: tgt_f.x + tgt_f.w / 2.0,
            y: tgt_f.y + tgt_f.h / 2.0,
        },
    ];

    driver
        .perform(
            &DriverAction::PointerDrag {
                win: win_ref.clone(),
                path,
                route: PointerRoute::AllowForeground,
            },
            &cancel,
        )
        .await
        .map_err(|e| e.to_string())?;

    let ok = poll_oracle(Duration::from_secs(5), |v| {
        if v.get("drop_ok")?.as_bool() == Some(true) {
            Some(true)
        } else {
            None
        }
    })
    .await?;
    assert!(ok);
    Ok(ContractOutcome::Passed)
}

pub async fn test_c16_background_press_with_other_window(
    driver: &LinuxDriver,
    win_ref: &WindowRef,
) -> Result<ContractOutcome, String> {
    let (conn, screen_num) = x11rb::connect(None).map_err(|e| e.to_string())?;
    let screen = &conn.setup().roots[screen_num];
    let aux_id = conn.generate_id().map_err(|e| e.to_string())?;
    use x11rb::protocol::xproto::*;
    conn.create_window(
        screen.root_depth,
        aux_id,
        screen.root,
        10,
        10,
        100,
        100,
        0,
        WindowClass::INPUT_OUTPUT,
        screen.root_visual,
        &CreateWindowAux::new().background_pixel(screen.white_pixel),
    )
    .map_err(|e| e.to_string())?;
    conn.map_window(aux_id).map_err(|e| e.to_string())?;
    let _ = conn.set_input_focus(InputFocus::POINTER_ROOT, aux_id, x11rb::CURRENT_TIME);
    conn.sync().map_err(|e| e.to_string())?;

    tokio::time::sleep(Duration::from_millis(100)).await;

    let initial = poll_oracle(Duration::from_secs(5), |v| {
        v.get("counter").and_then(|c| c.as_u64())
    })
    .await?;

    let cancel = Cancel::default();
    let opts = TreeOpts::default();
    let tree = driver
        .snapshot_tree(win_ref, &opts, &cancel)
        .await
        .map_err(|e| e.to_string())?;
    let root = tree.root.as_ref().ok_or("root missing")?;
    let btn = find_raw_node(root, &|n| {
        n.role == Role::Button
            && (n.name.as_deref().is_some_and(|s| {
                s.contains("counter_button") || s.contains("Increment") || s.contains("计数")
            }) || n.identifier.as_deref() == Some("counter_button"))
    })
    .ok_or("counter_button not found")?;

    let del = driver
        .perform(
            &DriverAction::Press {
                el: btn.handle.clone(),
            },
            &cancel,
        )
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(del, Delivery::Accessibility);

    let updated = poll_oracle(Duration::from_secs(5), |v| {
        let c = v.get("counter").and_then(|c| c.as_u64())?;
        if c > initial {
            Some(c)
        } else {
            None
        }
    })
    .await?;
    assert_eq!(updated, initial + 1, "background press increments counter");

    let _ = conn.destroy_window(aux_id);
    let _ = conn.sync();
    Ok(ContractOutcome::Passed)
}

pub async fn test_c17_stale_geometry() -> Result<ContractOutcome, String> {
    Ok(ContractOutcome::Skipped(
        "scope: ActionRouter stale_geometry + fresh state gate (P0-08); Driver trait has no snapshot ID",
    ))
}

pub async fn test_c18_stale_element_handle(
    driver: &LinuxDriver,
) -> Result<ContractOutcome, String> {
    let cancel = Cancel::default();
    let initial = poll_oracle(Duration::from_secs(5), |v| {
        v.get("counter").and_then(|c| c.as_u64())
    })
    .await?;

    let res = driver
        .perform(
            &DriverAction::Press {
                el: NativeHandle::new(0xdeadbeefu64),
            },
            &cancel,
        )
        .await;
    assert_eq!(res.unwrap_err(), DriverError::StaleElement);

    let cur = poll_oracle(Duration::from_secs(1), |v| {
        v.get("counter").and_then(|c| c.as_u64())
    })
    .await?;
    assert_eq!(cur, initial, "stale handle action must produce zero effect");

    Ok(ContractOutcome::Passed)
}

pub async fn test_c19_closed_window(driver: &LinuxDriver) -> Result<ContractOutcome, String> {
    let initial = poll_oracle(Duration::from_secs(5), |v| {
        v.get("counter").and_then(|c| c.as_u64())
    })
    .await?;

    let win = WindowRef {
        app: AppRef {
            info: AppInfo {
                id: "nonexistent".into(),
                name: "nonexistent".into(),
                pid: None,
                identity: None,
            },
        },
        id: "99999999".into(),
    };
    let cancel = Cancel::default();
    let res = driver
        .perform(
            &DriverAction::PointerClick {
                win: win.clone(),
                at: WindowPoint { x: 50.0, y: 50.0 },
                button: Button::Left,
                count: 1,
                route: PointerRoute::Auto,
            },
            &cancel,
        )
        .await;
    assert_eq!(res.unwrap_err(), DriverError::WindowGone);

    let cap_res = driver.capture(&win, &CaptureOpts::default(), &cancel).await;
    assert_eq!(cap_res.unwrap_err(), DriverError::WindowGone);

    let current = poll_oracle(Duration::from_secs(1), |v| {
        v.get("counter").and_then(|c| c.as_u64())
    })
    .await?;
    assert_eq!(
        current, initial,
        "closed window operation must not affect other windows"
    );

    Ok(ContractOutcome::Passed)
}

pub async fn test_c20_password_redaction(
    driver: &LinuxDriver,
    win_ref: &WindowRef,
) -> Result<ContractOutcome, String> {
    let cancel = Cancel::default();
    let opts = TreeOpts {
        timeout: Duration::from_secs(5),
        max_tree_nodes: 5000,
        ..Default::default()
    };
    let raw_tree = driver
        .snapshot_tree(win_ref, &opts, &cancel)
        .await
        .map_err(|e| e.to_string())?;

    let pw_node = raw_tree.root.as_ref().and_then(|r| {
        find_raw_node(r, &|n| {
            n.states.contains(States::PASSWORD)
                || n.name.as_deref() == Some("password_field")
                || n.identifier.as_deref() == Some("password_field")
        })
    });
    assert!(pw_node.is_some(), "password field must exist in tree");

    let processed = Pipeline::default().run(raw_tree, &opts);

    let cap = driver
        .capture(win_ref, &CaptureOpts::default(), &cancel)
        .await
        .map_err(|e| e.to_string())?;
    let stamp = Duration::from_secs(10);
    let mut tree_with_meta = processed.clone();
    tree_with_meta.state_version = Some(1);
    tree_with_meta.observed_at = Some(stamp);

    let frame = CapturedFrame::new(cap, 1, stamp);
    let redact_res = redact::apply(&tree_with_meta, frame);

    match redact_res {
        Ok(redacted) => {
            let has_neutral = redacted
                .pixels()
                .chunks_exact(4)
                .any(|p| p == NEUTRAL_COLOR);
            assert!(
                has_neutral,
                "redacted image must contain neutral fill over sensitive area"
            );
        }
        Err(redact::RedactionError::NeedsHandoff) => {
            // Secure fail-closed when sensitive regions cannot be reliably mapped
        }
        Err(e) => return Err(format!("unexpected redaction error: {e}")),
    }
    Ok(ContractOutcome::Passed)
}

pub async fn test_c21_overlay_exclusion() -> Result<ContractOutcome, String> {
    Ok(ContractOutcome::Skipped(
        "capability: overlay_excluded_from_capture=false (X11 does not support compositor overlay exclusion)",
    ))
}

pub async fn test_c22_timeout_no_replay() -> Result<ContractOutcome, String> {
    let fake_source = gcu_driver_linux::atspi::source::FakeSource {
        delay: Duration::ZERO,
        error_on_id: Some(10),
    };
    let mut root = gcu_driver_linux::atspi::source::FakeNode {
        id: 1,
        role: atspi::Role::Frame,
        ..Default::default()
    };
    root.children = vec![gcu_driver_linux::atspi::source::FakeNode {
        id: 10,
        role: atspi::Role::Button,
        actions: vec![gcu_core::model::NodeAction::Press],
        ..Default::default()
    }];
    let driver = LinuxDriver::new_fake(fake_source, root, vec![], vec![]);
    let cancel = Cancel::default();
    let res = driver
        .perform(
            &DriverAction::Press {
                el: NativeHandle::new(10u64),
            },
            &cancel,
        )
        .await;
    assert_eq!(
        res.unwrap_err(),
        DriverError::Timeout {
            maybe_executed: true
        }
    );
    Ok(ContractOutcome::Passed)
}

pub async fn test_c23_release_all_input_no_stuck_keys(
    driver: &LinuxDriver,
    win_ref: &WindowRef,
) -> Result<ContractOutcome, String> {
    driver.ledger().record_key_down(50); // Shift keycode
    assert!(!driver.ledger().is_empty());

    driver.release_all_input().await;
    assert!(driver.ledger().is_empty());

    let cancel = Cancel::default();
    let opts = TreeOpts::default();
    let tree = driver
        .snapshot_tree(win_ref, &opts, &cancel)
        .await
        .map_err(|e| e.to_string())?;
    let root = tree.root.as_ref().ok_or("root missing")?;
    let entry = find_raw_node(root, &|n| {
        n.role == Role::TextField && n.name.as_deref().is_some_and(|s| s.contains("name_field"))
    })
    .ok_or("name_field missing")?;
    let frame = entry.frame.ok_or("name_field frame missing")?;

    driver
        .perform(
            &DriverAction::PointerClick {
                win: win_ref.clone(),
                at: WindowPoint {
                    x: frame.x + 20.0,
                    y: frame.y + frame.h / 2.0,
                },
                button: Button::Left,
                count: 1,
                route: PointerRoute::AllowForeground,
            },
            &cancel,
        )
        .await
        .map_err(|e| e.to_string())?;

    driver
        .perform(
            &DriverAction::TypeText {
                win: win_ref.clone(),
                focus: None,
                text: "xyz".into(),
                route: KeyRoute::AllowForeground,
            },
            &cancel,
        )
        .await
        .map_err(|e| e.to_string())?;

    let verified = poll_oracle(Duration::from_secs(5), |v| {
        let t = v.get("text")?.as_str()?;
        if t.to_lowercase() == t
            && !t.contains('X')
            && !t.contains('Y')
            && !t.contains('Z')
            && (t.contains("xyz") || (t.contains('x') && t.contains('y') && t.contains('z')))
        {
            Some(true)
        } else {
            None
        }
    })
    .await?;
    assert!(verified, "subsequent typing must remain lowercase");
    Ok(ContractOutcome::Passed)
}

pub async fn test_c24_virtual_list_tree_budget(
    driver: &LinuxDriver,
    win_ref: &WindowRef,
) -> Result<ContractOutcome, String> {
    let cancel = Cancel::default();
    let opts = TreeOpts {
        timeout: Duration::from_secs(3),
        max_tree_nodes: 5000,
        ..Default::default()
    };
    let start = std::time::Instant::now();
    let tree = driver
        .snapshot_tree(win_ref, &opts, &cancel)
        .await
        .map_err(|e| e.to_string())?;
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(3),
        "tree snapshot must finish well within budget"
    );

    fn count_list_items(n: &RawNode) -> usize {
        let this = if n
            .name
            .as_deref()
            .is_some_and(|s| s.contains("Item ") || s.contains("列表项"))
        {
            1
        } else {
            0
        };
        this + n.children.iter().map(count_list_items).sum::<usize>()
    }

    let root = tree.root.as_ref().ok_or("root missing")?;
    let count = count_list_items(root);
    assert!(
        count <= 50,
        "virtual list must only expose visible rows (got {count}), not all 2000"
    );
    Ok(ContractOutcome::Passed)
}

pub async fn test_c25_pixel_hash_stale() -> Result<ContractOutcome, String> {
    Ok(ContractOutcome::Skipped(
        "scope: ActionRouter pixel hash stale_screenshot gate (P0-08); Driver trait has no pixel hash check",
    ))
}

pub fn test_c26_execution_surfaces_denied() -> Result<ContractOutcome, String> {
    assert!(is_terminal_like(atspi::Role::Terminal, "terminal", None));
    assert_eq!(
        terminal_node_flags(atspi::Role::Terminal, "terminal", None),
        NodeFlags::TERMINAL_LIKE
    );

    assert!(is_terminal_like(
        atspi::Role::Panel,
        "gnome-terminal-console",
        None
    ));

    for term in [
        "bash",
        "zsh",
        "sh",
        "ssh",
        "tmux",
        "screen",
        "powershell",
        "Integrated Terminal",
        "Debug Console",
        "Python REPL",
    ] {
        assert!(
            is_terminal_like(atspi::Role::Panel, "panel", Some(term)),
            "keyword {term} should be detected as terminal-like"
        );
    }

    assert!(!is_terminal_like(
        atspi::Role::Button,
        "button",
        Some("Submit")
    ));
    assert!(!is_terminal_like(
        atspi::Role::Entry,
        "entry",
        Some("Username")
    ));

    Ok(ContractOutcome::Passed)
}

pub async fn test_c27_stop_hotkey_and_watchdog_budgets() -> Result<ContractOutcome, String> {
    let fake_x = Arc::new(FakeX::new(
        XRootGeometry {
            root_id: 1,
            width: 1920,
            height: 1080,
            scale: 1.0,
        },
        true,
    ));

    let stop_latch = Arc::new(SharedStopLatch::default());
    let ledger = Arc::new(InputLedger::new());

    ledger.record_key_down(65);
    ledger.record_button_down(1);

    let start = tokio::time::Instant::now();
    handle_hotkey_trigger(stop_latch.as_ref(), &ledger, fake_x.as_ref()).await;
    let elapsed = start.elapsed();

    assert!(stop_latch.is_stopped(), "stop latch must be set");
    assert!(
        elapsed <= Duration::from_millis(100),
        "latch budget must be <= 100ms"
    );
    assert!(ledger.is_empty(), "ledger inputs must be released");
    assert!(
        elapsed <= Duration::from_millis(500),
        "release budget must be <= 500ms"
    );

    let hung = async {
        tokio::time::sleep(Duration::from_secs(6)).await;
        Ok(Delivery::Foreground)
    };
    let w_start = tokio::time::Instant::now();
    let res = run_with_watchdog(hung).await;
    let w_elapsed = w_start.elapsed();
    assert!(matches!(
        res,
        Err(DriverError::Timeout {
            maybe_executed: true
        })
    ));
    assert!(
        w_elapsed <= DRIVER_DEADLINE + Duration::from_millis(500),
        "watchdog timeout must be <= 5s"
    );

    Ok(ContractOutcome::Passed)
}

pub fn test_p3_identity_reauth() -> Result<ContractOutcome, String> {
    let base = AppIdentity {
        exe_path: PathBuf::from("/usr/bin/app"),
        dev: 42,
        inode: 1001,
        sha256: "hash123".into(),
        pid: 1234,
        start_time: 500,
        desktop_id: Some("app.desktop".into()),
    };

    assert!(!needs_reauth(&base, &base));

    let mut updated_hash = base.clone();
    updated_hash.sha256 = "newhash".into();
    assert!(needs_reauth(&base, &updated_hash));

    let mut replaced_inode = base.clone();
    replaced_inode.inode = 1002;
    assert!(needs_reauth(&base, &replaced_inode));

    let mut new_path = base.clone();
    new_path.exe_path = PathBuf::from("/usr/bin/app2");
    assert!(needs_reauth(&base, &new_path));

    let mut restarted = base.clone();
    restarted.pid = 5678;
    restarted.start_time = 700;
    assert!(!needs_reauth(&base, &restarted));
    assert!(!same_process_instance(&base, &restarted));

    Ok(ContractOutcome::Passed)
}

pub async fn test_p3_hotkey_failure_disables_foreground() -> Result<ContractOutcome, String> {
    let fake_x = FakeX::new(
        XRootGeometry {
            root_id: 1,
            width: 1920,
            height: 1080,
            scale: 1.0,
        },
        true,
    );
    let kc = resolve_or_remap_keysym(&fake_x, 0xff08).unwrap().keycode;
    let cfg = HotkeyConfig::default();
    fake_x.add_conflicting_key(kc, cfg.modifiers);

    fake_x.add_window(FakeWindow {
        id: 100,
        pid: Some(1234),
        name: Some("test".into()),
        geometry: XWindowGeometry {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
            border_width: 0,
            depth: 24,
        },
        is_mapped: true,
        image: None,
    });

    let driver = LinuxDriver::new_fake(
        gcu_driver_linux::atspi::source::FakeSource::default(),
        gcu_driver_linux::atspi::source::FakeNode::default(),
        vec![],
        vec![],
    )
    .with_x_server(Arc::new(fake_x));

    assert!(!driver.is_hotkey_available());
    assert_eq!(
        driver.capabilities().delivery.foreground,
        ForegroundCapability::Unavailable
    );

    let doc = driver.doctor().await;
    assert!(doc
        .issues
        .iter()
        .any(|i| i.contains("stop_hotkey: unavailable")));

    let cancel = Cancel::default();

    // 1. Hotkey unavailable + nonexistent window -> WindowGone (window existence checked before route capability)
    let win_nonexistent = WindowRef {
        app: AppRef {
            info: AppInfo {
                id: "1".into(),
                name: "test".into(),
                pid: None,
                identity: None,
            },
        },
        id: "999999".into(),
    };
    let res_nonexistent = driver
        .perform(
            &DriverAction::PointerClick {
                win: win_nonexistent,
                at: WindowPoint { x: 10.0, y: 10.0 },
                button: Button::Left,
                count: 1,
                route: PointerRoute::Auto,
            },
            &cancel,
        )
        .await;
    assert_eq!(res_nonexistent.unwrap_err(), DriverError::WindowGone);

    // 2. Hotkey unavailable + existing window -> CapabilityUnavailable (foreground XTEST disabled)
    let win_exists = WindowRef {
        app: AppRef {
            info: AppInfo {
                id: "1".into(),
                name: "test".into(),
                pid: None,
                identity: None,
            },
        },
        id: "100".into(),
    };
    let res_exists = driver
        .perform(
            &DriverAction::PointerClick {
                win: win_exists,
                at: WindowPoint { x: 10.0, y: 10.0 },
                button: Button::Left,
                count: 1,
                route: PointerRoute::Auto,
            },
            &cancel,
        )
        .await;
    assert_eq!(res_exists.unwrap_err(), DriverError::CapabilityUnavailable);
    Ok(ContractOutcome::Passed)
}

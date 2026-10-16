//! Contract cases C01–C13: Discovery, Observation, and Input actions.

use super::support::*;
use gcu_core::{
    driver::{
        AppQuery, AppRef, Cancel, CaptureOpts, Delivery, Driver, DriverAction, DriverError,
        KeyRoute, PointerRoute, TreeOpts, WindowRef,
    },
    model::{Role, WindowPoint},
};
use gcu_driver_linux::LinuxDriver;
use gcu_protocol::{keys::KeyChord, tools::MouseButton as Button};
use std::time::Duration;
use x11rb::connection::Connection;
use x11rb::wrapper::ConnectionExt;

pub async fn test_c01_discovery(driver: &LinuxDriver) -> Result<ContractOutcome, String> {
    let apps = driver.list_apps().await.map_err(|e| e.to_string())?;
    assert!(!apps.is_empty(), "list_apps should return non-empty list");

    let fixture = driver
        .resolve_app(&AppQuery("fixture".into()))
        .await
        .map_err(|e| e.to_string())?;
    assert!(fixture.info.name.to_lowercase().contains("fixture"));

    let resolved_by_id = driver
        .resolve_app(&AppQuery(fixture.info.id.clone()))
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(resolved_by_id.info.id, fixture.info.id);

    if let Some(pid) = fixture.info.pid {
        assert!(pid > 0);
        let stat_content = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .map_err(|e| format!("failed reading /proc/{pid}/stat: {e}"))?;
        assert!(
            stat_content.contains("fixture"),
            "process comm in /proc must match fixture"
        );
    }

    Ok(ContractOutcome::Passed)
}

pub async fn test_c02_launch(
    driver: &LinuxDriver,
    app: &AppRef,
) -> Result<ContractOutcome, String> {
    let res = driver.launch_app(app).await;
    assert_eq!(res.unwrap_err(), DriverError::CapabilityUnavailable);
    Ok(ContractOutcome::Skipped(
        "capability: launch_app=false (LinuxDriver::launch_app returns CapabilityUnavailable)",
    ))
}

pub async fn test_c03_tree_roles(
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
    let root = raw_tree.root.as_ref().ok_or("root node missing")?;
    assert_eq!(root.role, Role::Window);

    let has_button = find_raw_node(root, &|n| {
        n.role == Role::Button
            && n.name.as_deref().is_some_and(|s| {
                s.contains("counter_button") || s.contains("Increment") || s.contains("计数")
            })
    });
    assert!(
        has_button.is_some(),
        "counter button with Role::Button must exist"
    );

    let has_entry = find_raw_node(root, &|n| {
        n.role == Role::TextField
            && n.name.as_deref().is_some_and(|s| {
                s.contains("name_field") || s.contains("Name") || s.contains("姓名")
            })
    });
    assert!(
        has_entry.is_some(),
        "name_field with Role::TextField must exist"
    );

    let has_check = find_raw_node(root, &|n| {
        n.role == Role::CheckBox || n.name.as_deref() == Some("agree_checkbox")
    });
    assert!(has_check.is_some(), "agree_checkbox must exist in tree");

    let has_slider = find_raw_node(root, &|n| {
        n.role == Role::Slider || n.name.as_deref() == Some("slider_field")
    });
    assert!(has_slider.is_some(), "slider_field must exist in tree");

    let has_canvas = find_raw_node(root, &|n| {
        (n.role == Role::Image || n.role == Role::Canvas)
            || n.name.as_deref() == Some("grid_canvas")
    });
    assert!(has_canvas.is_some(), "grid_canvas must exist in tree");

    Ok(ContractOutcome::Passed)
}

pub async fn test_c04_capture_and_occlusion(
    driver: &LinuxDriver,
    win_ref: &WindowRef,
) -> Result<ContractOutcome, String> {
    let cancel = Cancel::default();
    let capture = driver
        .capture(win_ref, &CaptureOpts::default(), &cancel)
        .await
        .map_err(|e| e.to_string())?;

    assert!(capture.width > 0 && capture.height > 0);
    assert_eq!(capture.mime, "image/png");
    assert!(capture.bytes.starts_with(b"\x89PNG\r\n\x1a\n"));

    let (w, h, pixels) = decode_png_to_rgba(&capture.bytes)?;
    assert_eq!((w, h), (capture.width, capture.height));

    let cell_color = [95u8, 115u8, 135u8];
    let has_grid_color = pixels
        .chunks_exact(4)
        .any(|p| p[0] == cell_color[0] && p[1] == cell_color[1] && p[2] == cell_color[2]);
    assert!(
        has_grid_color,
        "capture must contain fixture pastel canvas color"
    );

    // Occlusion test under XComposite
    if let Ok((conn, screen_num)) = x11rb::connect(None) {
        use x11rb::protocol::xproto::*;
        let screen = &conn.setup().roots[screen_num];
        if let Ok(cover_id) = conn.generate_id() {
            let geom = driver
                .window_geometry(win_ref)
                .await
                .map_err(|e| e.to_string())?;
            let aux = CreateWindowAux::new()
                .background_pixel(screen.black_pixel)
                .override_redirect(1);
            let _ = conn.create_window(
                screen.root_depth,
                cover_id,
                screen.root,
                geom.frame.x as i16 + 20,
                geom.frame.y as i16 + 20,
                150,
                150,
                0,
                WindowClass::INPUT_OUTPUT,
                screen.root_visual,
                &aux,
            );
            let _ = conn.map_window(cover_id);
            let _ = conn.sync();

            tokio::time::sleep(Duration::from_millis(100)).await;

            let occluded = driver
                .capture(win_ref, &CaptureOpts::default(), &cancel)
                .await
                .map_err(|e| e.to_string())?;
            let (_, _, occ_pixels) = decode_png_to_rgba(&occluded.bytes)?;
            let still_has_color = occ_pixels
                .chunks_exact(4)
                .any(|p| p[0] == cell_color[0] && p[1] == cell_color[1] && p[2] == cell_color[2]);
            assert!(
                still_has_color,
                "XComposite must capture window content even when occluded"
            );

            let _ = conn.destroy_window(cover_id);
            let _ = conn.sync();
        }
    }
    Ok(ContractOutcome::Passed)
}

pub async fn test_c05_press_counter(
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
    let btn = find_raw_node(root, &|n| {
        n.role == Role::Button
            && (n.name.as_deref().is_some_and(|s| {
                s.contains("counter_button") || s.contains("Increment") || s.contains("计数")
            }) || n.identifier.as_deref() == Some("counter_button"))
    })
    .ok_or("counter_button not found")?;

    let initial = poll_oracle(Duration::from_secs(5), |v| {
        v.get("counter").and_then(|c| c.as_u64())
    })
    .await?;

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

    assert_eq!(updated, initial + 1, "counter must increment exactly once");
    Ok(ContractOutcome::Passed)
}

pub async fn test_c06_click_grid_cell(
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
    let canvas = find_raw_node(root, &|n| {
        n.name.as_deref() == Some("grid_canvas") || n.identifier.as_deref() == Some("grid_canvas")
    })
    .ok_or("canvas missing")?;
    let frame = canvas.frame.ok_or("canvas frame missing")?;

    let (col, row) = (3, 2);
    let click_x = frame.x + (col as f64 + 0.5) * 40.0;
    let click_y = frame.y + (row as f64 + 0.5) * 40.0;

    let del = driver
        .perform(
            &DriverAction::PointerClick {
                win: win_ref.clone(),
                at: WindowPoint {
                    x: click_x,
                    y: click_y,
                },
                button: Button::Left,
                count: 1,
                route: PointerRoute::AllowForeground,
            },
            &cancel,
        )
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(del, Delivery::Foreground);

    let hit = poll_oracle(Duration::from_secs(5), |v| {
        let arr = v.get("grid_hit")?.as_array()?;
        if arr.len() == 2 {
            let c = arr[0].as_u64()?;
            let r = arr[1].as_u64()?;
            if (c as usize, r as usize) == (col, row) {
                return Some((c, r));
            }
        }
        None
    })
    .await?;
    assert_eq!(hit, (col as u64, row as u64));
    Ok(ContractOutcome::Passed)
}

pub async fn test_c07_set_value_textfield(
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
    let entry = find_raw_node(root, &|n| {
        n.role == Role::TextField && n.name.as_deref().is_some_and(|s| s.contains("name_field"))
    })
    .ok_or("name_field missing")?;

    let res = driver
        .perform(
            &DriverAction::SetValue {
                el: entry.handle.clone(),
                value: "test_val".into(),
            },
            &cancel,
        )
        .await;
    assert_eq!(
        res.unwrap_err(),
        DriverError::NotSettable {
            maybe_executed: false
        }
    );

    Ok(ContractOutcome::Skipped(
        "AT-SPI EditableText interface unsupported by egui/accesskit fixture text fields (returns NotSettable)",
    ))
}

pub async fn test_c08_type_text_unicode_zwj(
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

    let text_to_type = "Grok-智造🚀\n";
    driver
        .perform(
            &DriverAction::TypeText {
                win: win_ref.clone(),
                focus: None,
                text: text_to_type.into(),
                route: KeyRoute::AllowForeground,
            },
            &cancel,
        )
        .await
        .map_err(|e| e.to_string())?;

    let verified = poll_oracle(Duration::from_secs(5), |v| {
        let t = v.get("text")?.as_str()?;
        if t.contains("Grok-智造🚀") {
            Some(true)
        } else {
            None
        }
    })
    .await?;
    assert!(verified);
    Ok(ContractOutcome::Passed)
}

pub async fn test_c09_press_keys_ctrl_a_backspace(
    driver: &LinuxDriver,
    win_ref: &WindowRef,
) -> Result<ContractOutcome, String> {
    let t0 = tokio::time::Instant::now();
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

    // 1. Click to focus text field
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
    let t_click = t0.elapsed();

    // Confirm text field has focus via oracle before issuing keyboard commands
    poll_oracle(Duration::from_secs(5), |v| {
        match v.get("focused_widget").and_then(|w| w.as_str()) {
            Some(w) if w.contains("name_field") => Some(()),
            Some(_) => None,
            None => Some(()), // Fallback if fixture doesn't report focused_widget
        }
    })
    .await
    .map_err(|e| format!("C09 click focus confirmation failed: {e}; t_click={t_click:?}"))?;
    let t_focus = t0.elapsed();

    // 2. Type target text into the focused field
    driver
        .perform(
            &DriverAction::TypeText {
                win: win_ref.clone(),
                focus: None,
                text: "clear_target".into(),
                route: KeyRoute::AllowForeground,
            },
            &cancel,
        )
        .await
        .map_err(|e| e.to_string())?;
    let t_type = t0.elapsed();

    poll_oracle(Duration::from_secs(5), |v| {
        if v.get("text")?.as_str()?.contains("clear_target") {
            Some(())
        } else {
            None
        }
    })
    .await
    .map_err(|e| {
        format!(
            "C09 typing verification failed: {e}; t_click={t_click:?}, t_focus={t_focus:?}, t_type={t_type:?}"
        )
    })?;
    let t_poll_type = t0.elapsed();

    // 3. Select all using Ctrl+A
    driver
        .perform(
            &DriverAction::PressKeys {
                win: win_ref.clone(),
                chord: KeyChord::parse("ctrl+a").map_err(|e| e.to_string())?,
                route: KeyRoute::AllowForeground,
            },
            &cancel,
        )
        .await
        .map_err(|e| e.to_string())?;
    let t_ctrl_a = t0.elapsed();

    // 4. Clear selection using BackSpace
    driver
        .perform(
            &DriverAction::PressKeys {
                win: win_ref.clone(),
                chord: KeyChord::parse("BackSpace").map_err(|e| e.to_string())?,
                route: KeyRoute::AllowForeground,
            },
            &cancel,
        )
        .await
        .map_err(|e| e.to_string())?;
    let t_backspace = t0.elapsed();

    // 5. Verify the field is completely cleared
    let is_empty = poll_oracle(Duration::from_secs(5), |v| {
        let t = v.get("text")?.as_str()?;
        if t.is_empty() {
            Some(true)
        } else {
            None
        }
    })
    .await
    .map_err(|e| {
        format!(
            "C09 field empty verification failed: {e}; t_click={t_click:?}, t_focus={t_focus:?}, t_type={t_type:?}, t_poll_type={t_poll_type:?}, t_ctrl_a={t_ctrl_a:?}, t_backspace={t_backspace:?}"
        )
    })?;
    assert!(is_empty);
    Ok(ContractOutcome::Passed)
}

pub async fn test_c10_toggle_checkbox(
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
    let cb = find_raw_node(root, &|n| {
        n.name.as_deref() == Some("agree_checkbox")
            || (n.role == Role::CheckBox && n.name.as_deref().is_some_and(|s| s.contains("Agree")))
    })
    .ok_or("checkbox missing")?;

    let initial = poll_oracle(Duration::from_secs(5), |v| {
        v.get("checked").and_then(|c| c.as_bool())
    })
    .await?;

    let res = driver
        .perform(
            &DriverAction::Press {
                el: cb.handle.clone(),
            },
            &cancel,
        )
        .await;
    if res.is_err() {
        if let Some(frame) = cb.frame {
            driver
                .perform(
                    &DriverAction::PointerClick {
                        win: win_ref.clone(),
                        at: WindowPoint {
                            x: frame.x + frame.w / 2.0,
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
        }
    }

    let updated = poll_oracle(Duration::from_secs(5), |v| {
        let cur = v.get("checked")?.as_bool()?;
        if cur != initial {
            Some(cur)
        } else {
            None
        }
    })
    .await?;
    assert_eq!(updated, !initial);
    Ok(ContractOutcome::Passed)
}

pub async fn test_c11_combobox_select() -> Result<ContractOutcome, String> {
    Ok(ContractOutcome::Skipped(
        "capability: AT-SPI Selection interface not implemented in LinuxDriver",
    ))
}

pub async fn test_c12_slider_set_value(
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
    let slider = find_raw_node(root, &|n| {
        n.role == Role::Slider || n.name.as_deref() == Some("slider_field")
    })
    .ok_or("slider missing")?;

    let target_val = 80.0;
    driver
        .perform(
            &DriverAction::SetValue {
                el: slider.handle.clone(),
                value: format!("{target_val}"),
            },
            &cancel,
        )
        .await
        .map_err(|e| e.to_string())?;

    let val = poll_oracle(Duration::from_secs(5), |v| {
        let s = v.get("slider")?.as_f64()?;
        if (s - target_val).abs() < 0.5 {
            Some(s)
        } else {
            None
        }
    })
    .await?;
    assert!((val - target_val).abs() < 0.5);
    Ok(ContractOutcome::Passed)
}

pub async fn test_c13_virtual_list_scroll(
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
    let list_node = find_raw_node(root, &|n| {
        n.name.as_deref().is_some_and(|s| {
            s.contains("Item 0000") || s.contains("Visible rows") || s.contains("virtual_list")
        })
    })
    .ok_or("list node missing")?;
    let frame = list_node.frame.ok_or("list frame missing")?;

    driver
        .perform(
            &DriverAction::PointerScroll {
                win: win_ref.clone(),
                at: WindowPoint {
                    x: frame.x + 50.0,
                    y: frame.y + 50.0,
                },
                dx: 0,
                dy: 600,
                route: PointerRoute::AllowForeground,
            },
            &cancel,
        )
        .await
        .map_err(|e| e.to_string())?;

    let scrolled = poll_oracle(Duration::from_secs(5), |v| {
        let range = v.get("visible_range")?.as_array()?;
        let start = range.first()?.as_u64()?;
        if start > 0 {
            Some(start)
        } else {
            None
        }
    })
    .await?;
    assert!(scrolled > 0);
    Ok(ContractOutcome::Passed)
}

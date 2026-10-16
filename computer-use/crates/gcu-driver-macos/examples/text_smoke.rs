//! Native opt-in oracle. Initialize AppKit on main before entering the runtime.
#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if !gcu_driver_macos::input::ensure_appkit() {
        return Err("main-thread AppKit initialization failed".into());
    }
    let mode = std::env::args()
        .nth(1)
        .ok_or("mode: textedit | electron | clipboard | paste")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()?;
    runtime.block_on(native::run(&mode))
}

#[cfg(target_os = "macos")]
mod native {
    use gcu_core::{
        driver::{AppQuery, Cancel, Driver, DriverAction, KeyRoute, TreeOpts, WindowRef},
        model::{NodeValue, RawNode},
    };
    use gcu_driver_macos::{
        input::{
            clipboard::{Pasteboard, PasteboardContent},
            NativePasteboard,
        },
        MacosDriver,
    };
    use std::time::Duration;

    pub async fn run(mode: &str) -> Result<(), Box<dyn std::error::Error>> {
        if mode == "clipboard" {
            tokio::task::spawn_blocking(|| -> gcu_core::driver::Result<()> {
                let board = NativePasteboard::named(&format!("gcu-t{}", std::process::id()));
                let original = board.read()?;
                let data = PasteboardContent(vec![vec![
                    (
                        "public.utf8-plain-text".into(),
                        "中文👩🏽‍💻e\u{301}\n".as_bytes().to_vec(),
                    ),
                    ("public.rtf".into(), b"{\\rtf1 fixture}".to_vec()),
                ]]);
                let owned = board
                    .replace_if(&original, &data)?
                    .ok_or(gcu_core::driver::DriverError::InvalidRequest)?;
                if board.read()?.content != data {
                    return Err(gcu_core::driver::DriverError::InvalidRequest);
                }
                if board.replace_if(&owned, &original.content)?.is_none() {
                    return Err(gcu_core::driver::DriverError::InvalidRequest);
                }
                let current = board.read()?;
                if current.content != original.content {
                    return Err(gcu_core::driver::DriverError::InvalidRequest);
                }
                let ours = board
                    .replace_if(&current, &data)?
                    .ok_or(gcu_core::driver::DriverError::InvalidRequest)?;
                let foreign = board
                    .replace_if(&ours, &PasteboardContent::text("foreign"))?
                    .ok_or(gcu_core::driver::DriverError::InvalidRequest)?;
                if board.replace_if(&ours, &original.content)?.is_some() {
                    return Err(gcu_core::driver::DriverError::InvalidRequest);
                }
                if board.read()?.content != foreign.content {
                    return Err(gcu_core::driver::DriverError::InvalidRequest);
                }
                let _ = board.replace_if(&foreign, &original.content)?;
                Ok(())
            })
            .await??;
            return Ok(());
        }
        let (app_id, route) = match mode {
            "textedit" => ("com.apple.TextEdit", KeyRoute::TargetedOnly),
            "paste" => ("com.apple.TextEdit", KeyRoute::AllowForeground),
            "electron" => ("com.microsoft.VSCode", KeyRoute::AllowForeground),
            _ => return Err("unknown mode".into()),
        };
        let driver = MacosDriver::new().await?;
        let app = driver.resolve_app(&AppQuery::from(app_id)).await?;
        let windows = driver.list_windows(Some(&app)).await?;
        // Use a fresh blank scratch document with this exact title. Never use a
        // terminal, REPL, task/debug panel, or an existing user document.
        let window = windows
            .iter()
            .find(|w| w.title.contains("gcu-text-fixture"))
            .ok_or("open a blank scratch document named gcu-text-fixture")?;
        let win = WindowRef {
            app,
            id: window.id.clone(),
        };
        let payload = if mode == "paste" {
            if std::env::var("GCU_PASTE_FIXTURE").as_deref() != Ok("1") {
                return Err(
                    "set GCU_PASTE_FIXTURE=1 only in the disposable fixture desktop".into(),
                );
            }
            tokio::task::spawn_blocking(|| -> gcu_core::driver::Result<()> {
                let before = NativePasteboard::general().read()?;
                if before.content != PasteboardContent::text("gcu-clipboard-fixture") {
                    return Err(gcu_core::driver::DriverError::InvalidRequest);
                }
                Ok(())
            })
            .await??;
            "中文👩🏽‍💻e\u{301}".repeat(300)
        } else {
            "中文👩🏽‍💻e\u{301}\nfixture-end".into()
        };
        let action = DriverAction::TypeText {
            win: win.clone(),
            focus: None,
            text: payload.clone(),
            route,
        };
        let delivery = if mode == "paste" {
            driver
                .perform_text(
                    &action,
                    gcu_driver_macos::input::text::TextOptions {
                        allow_clipboard: true,
                        sensitive_text: false,
                        high_risk_target: false,
                    },
                    &Cancel::default(),
                )
                .await?
        } else {
            driver.perform(&action, &Cancel::default()).await?
        };
        if mode == "paste" {
            if delivery != gcu_core::driver::Delivery::Clipboard {
                return Err("clipboard delivery was not used".into());
            }
            tokio::task::spawn_blocking(|| -> gcu_core::driver::Result<()> {
                if NativePasteboard::general().read()?.content
                    != PasteboardContent::text("gcu-clipboard-fixture")
                {
                    return Err(gcu_core::driver::DriverError::InvalidRequest);
                }
                Ok(())
            })
            .await??;
        } else if !matches!(
            delivery,
            gcu_core::driver::Delivery::Accessibility
                | gcu_core::driver::Delivery::TargetedEvent
                | gcu_core::driver::Delivery::Foreground
        ) {
            return Err("unexpected text delivery".into());
        }
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let raw = driver
                    .snapshot_tree(
                        &win,
                        &TreeOpts {
                            text_limit: None,
                            ..TreeOpts::default()
                        },
                        &Cancel::default(),
                    )
                    .await?;
                if raw
                    .root
                    .as_ref()
                    .is_some_and(|root| contains(root, &payload))
                {
                    return Ok::<_, gcu_core::driver::DriverError>(());
                }
                tokio::time::sleep_until(tokio::time::Instant::now() + Duration::from_millis(10))
                    .await;
            }
        })
        .await??;
        driver.release_all_input().await;
        Ok(())
    }
    fn contains(node: &RawNode, expected: &str) -> bool {
        matches!(&node.value, Some(NodeValue::Text(value)) if value.contains(expected))
            || node.children.iter().any(|n| contains(n, expected))
    }
}

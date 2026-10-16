//! Run only on an unelevated disposable Windows desktop with synthetic data.
use super::*;
use gcu_driver_windows::input::clipboard::{ClipboardApi, NativeClipboard};

#[tokio::test]
#[ignore = "requires disposable Windows clipboard; also launches an isolated test process"]
async fn native_named_mutex_two_processes_do_not_overlap() {
    let board = NativeClipboard::default();
    let receipt = board
        .stage_text(
            "GCU synthetic mutex owner".into(),
            &Cancel::default(),
            std::time::Instant::now() + Duration::from_secs(4),
        )
        .await
        .unwrap();
    let executable = std::env::current_exe().unwrap();
    let status = tokio::process::Command::new(executable)
        .args([
            "native_named_mutex_contender",
            "--ignored",
            "--test-threads=1",
        ])
        .env("GCU_CLIPBOARD_CONTENDER", "1")
        .status()
        .await
        .unwrap();
    assert!(status.success());
    assert!(board
        .matches(receipt, "GCU synthetic mutex owner".into())
        .await
        .unwrap());
    board.restore().await.unwrap();
    assert!(!board.pending());
}
#[tokio::test]
#[ignore = "only spawned by native_named_mutex_two_processes_do_not_overlap"]
async fn native_named_mutex_contender() {
    if std::env::var("GCU_CLIPBOARD_CONTENDER").as_deref() != Ok("1") {
        return;
    }
    let board = NativeClipboard::default();
    assert_eq!(
        board
            .stage_text(
                "GCU synthetic contender".into(),
                &Cancel::default(),
                std::time::Instant::now() + Duration::from_millis(200)
            )
            .await,
        Err(DriverError::Timeout {
            maybe_executed: false
        })
    );
    assert!(!board.pending());
}
#[tokio::test]
#[ignore = "open a blank disposable Notepad editor and focus it first; requires built gcu.exe"]
async fn native_notepad_long_text_paste_preserves_document() {
    use gcu_driver_windows::worker::{ExecMode, WorkerConfig};
    let executable = std::path::PathBuf::from(std::env::var_os("GCU_WORKER_EXE").unwrap());
    assert!(executable.is_absolute());
    let mode = if std::env::var("GCU_TEST_EXEC_MODE").as_deref() == Ok("in_process") {
        ExecMode::InProcess
    } else {
        ExecMode::Worker
    };
    let driver = if mode == ExecMode::Worker {
        gcu_driver_windows::WindowsDriver::monitored(
            WorkerConfig::new(executable),
            Default::default(),
            Arc::new(crate::common::TestStop::default()),
        )
        .await
        .unwrap()
    } else {
        gcu_driver_windows::WindowsDriver::new_in_process()
            .await
            .unwrap()
    };
    let app = driver
        .resolve_app(&AppQuery::from("notepad.exe"))
        .await
        .unwrap();
    let focus = driver
        .focused_element(&app)
        .await
        .unwrap()
        .expect("focus the blank Notepad editor");
    let info = driver.key_window(&app).await.unwrap();
    let win = WindowRef { app, id: info.id };
    let text = "GCU 中文 😀 document ".repeat(600);
    let request = DriverAction::TypeText {
        win: win.clone(),
        focus: Some(focus),
        text: text.clone(),
        route: KeyRoute::AllowForeground,
    };
    if mode == ExecMode::InProcess {
        assert_eq!(
            driver.perform(&request, &Cancel::default()).await,
            Err(DriverError::CapabilityUnavailable)
        );
        driver.release_all_input().await;
        return;
    }
    assert_eq!(
        driver.perform(&request, &Cancel::default()).await,
        Ok(Delivery::Clipboard)
    );
    let tree = driver
        .snapshot_tree(
            &win,
            &TreeOpts {
                text_limit: Some(65_536),
                ..Default::default()
            },
            &Cancel::default(),
        )
        .await
        .unwrap();
    fn contains(node: &gcu_core::model::RawNode, text: &str) -> bool {
        matches!(&node.value, Some(gcu_core::model::NodeValue::Text(value)) if value.contains(text))
            || node
                .text_extras
                .as_ref()
                .and_then(|v| v.text.as_ref())
                .is_some_and(|v| v.contains(text))
            || node.children.iter().any(|child| contains(child, text))
    }
    assert!(tree.root.as_ref().is_some_and(|node| contains(node, &text)));
    driver.release_all_input().await;
}

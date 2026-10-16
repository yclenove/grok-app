//! Actual killable subprocess + parent-owned FakeClipboard recovery.
use super::*;
use gcu_driver_windows::input::{clipboard::ClipboardApi, fake::FakeInput};
use gcu_driver_windows::worker::WorkerConfig;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
#[path = "worker_stop.rs"]
mod stop;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!(
                "gc-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn config(&self, fault: &str) -> WorkerConfig {
        let mut config = WorkerConfig::new(env!("CARGO_BIN_EXE_gcu-test-worker"));
        config.environment = vec![
            ("GCU_TEST_DIR".into(), self.0.as_os_str().to_owned()),
            ("GCU_TEST_FAULT".into(), fault.into()),
        ];
        config.timeout = Duration::from_millis(900);
        config
    }
    async fn held(&self) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while !self.0.join("held").exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Parent {
    clipboard: Arc<FakeClipboard>,
    input: FakeInput,
}
#[async_trait::async_trait]
impl gcu_driver_windows::input::api::InputReleaser for Parent {
    fn clipboard(&self) -> Option<Arc<dyn gcu_driver_windows::input::clipboard::ClipboardApi>> {
        Some(self.clipboard.clone())
    }
    async fn release(&self, ledger: Arc<gcu_driver_windows::input::ledger::InputLedger>) {
        gcu_driver_windows::input::api::InputApi::release(&self.input, ledger).await;
    }
}
#[tokio::test]
async fn worker_stop_cancel_timeout_crash_restore_parent_backup_without_retry() {
    for ending in ["stop", "cancel", "timeout", "crash", "abandon"] {
        let fixture = Fixture::new();
        let board = Arc::new(FakeClipboard::new(Arc::new(Desktop::default())));
        let release = Arc::new(Parent {
            clipboard: board.clone(),
            input: FakeInput::default(),
        });
        let fault = if ending == "crash" {
            "input_clipboard_crash"
        } else {
            "input_clipboard_hang"
        };
        let driver = Arc::new(
            gcu_driver_windows::WindowsDriver::isolated_with_releaser(
                fixture.config(fault),
                release,
            )
            .await
            .unwrap()
            .with_safety(crate::common::fake_guard()),
        );
        let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
        let info = driver.key_window(&app).await.unwrap();
        let win = WindowRef { app, id: info.id };
        let cancel = Cancel::default();
        let c = cancel.clone();
        let d = driver.clone();
        let task = tokio::spawn(async move { d.perform(&request(&win), &c).await });
        fixture.held().await;
        if ending == "stop" {
            driver.release_all_input().await;
        }
        if ending == "cancel" {
            cancel.cancel();
        }
        if ending == "abandon" {
            task.abort();
        }
        if ending == "abandon" {
            assert!(task.await.unwrap_err().is_cancelled());
        } else {
            assert!(task.await.unwrap().unwrap_err().maybe_executed());
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            while board.pending() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(board.contents(), original());
        assert_eq!(board.writes(), 1);
        let calls = std::fs::read_to_string(fixture.0.join("calls")).unwrap();
        assert_eq!(calls.lines().count(), 1, "mutation never replayed");
        driver.release_all_input().await;
    }
}
#[tokio::test]
async fn worker_stop_preserves_external_copy_instead_of_parent_backup() {
    let fixture = Fixture::new();
    let board = Arc::new(FakeClipboard::new(Arc::new(Desktop::default())));
    let release = Arc::new(Parent {
        clipboard: board.clone(),
        input: FakeInput::default(),
    });
    let driver = Arc::new(
        gcu_driver_windows::WindowsDriver::isolated_with_releaser(
            fixture.config("input_clipboard_hang"),
            release,
        )
        .await
        .unwrap()
        .with_safety(crate::common::fake_guard()),
    );
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    let info = driver.key_window(&app).await.unwrap();
    let win = WindowRef { app, id: info.id };
    let d = driver.clone();
    let task = tokio::spawn(async move { d.perform(&request(&win), &Cancel::default()).await });
    fixture.held().await;
    board.copy_external();
    driver.release_all_input().await;
    assert!(task.await.unwrap().unwrap_err().maybe_executed());
    assert_eq!(board.contents(), external());
    assert_eq!(board.restores(), 0);
}

#[tokio::test]
async fn worker_stop_partial_paste_parent_releases_ctrl_and_restores_clipboard() {
    let fixture = Fixture::new();
    let board = Arc::new(FakeClipboard::new(Arc::new(Desktop::default())));
    let release = Arc::new(Parent {
        clipboard: board.clone(),
        input: FakeInput::default(),
    });
    let driver = Arc::new(
        gcu_driver_windows::WindowsDriver::isolated_with_releaser(
            fixture.config("input_clipboard_key_hang"),
            release.clone(),
        )
        .await
        .unwrap()
        .with_safety(crate::common::fake_guard()),
    );
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    let info = driver.key_window(&app).await.unwrap();
    let win = WindowRef { app, id: info.id };
    let d = driver.clone();
    let task = tokio::spawn(async move { d.perform(&request(&win), &Cancel::default()).await });
    fixture.held().await;
    driver.release_all_input().await;
    assert!(task.await.unwrap().unwrap_err().maybe_executed());
    assert_eq!(board.contents(), original());
    let events = release.input.state().sent;
    assert_eq!(
        events.len(),
        2,
        "parent reserves all possible downs before child dispatch"
    );
    assert!(events
        .iter()
        .any(|s| matches!(s.event, Event::Key(17, false, ..))));
    assert!(events
        .iter()
        .all(|s| matches!(s.event, Event::Key(_, false, ..))));
    assert_eq!(board.writes(), 1);
}

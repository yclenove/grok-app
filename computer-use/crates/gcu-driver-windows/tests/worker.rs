//! Protocol adversaries and subprocess lifecycle; no real desktop/account data.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_driver_windows::worker::{codec, ExecMode};

#[tokio::test]
async fn codec_invalid_truncated_oversized_frames_return_errors() {
    for bytes in [
        vec![0],
        vec![0, 0, 0, 0],
        vec![255; 4],
        vec![0, 0, 0, 8, b'{'],
        vec![0, 0, 0, 1, b'!'],
    ] {
        assert!(
            codec::read_frame::<_, serde_json::Value>(&mut bytes.as_slice())
                .await
                .is_err()
        );
    }
    assert!(
        codec::read_frame::<_, serde_json::Value>(&mut [].as_slice())
            .await
            .unwrap()
            .is_none()
    );
}
#[tokio::test]
async fn codec_multiple_valid_frames_round_trip_without_overread() {
    let mut bytes = Vec::new();
    for value in [
        serde_json::json!({"hello": "世界"}),
        serde_json::json!([1, 2]),
    ] {
        codec::write_frame(&mut bytes, &value).await.unwrap();
    }
    let mut input = bytes.as_slice();
    assert_eq!(
        codec::read_frame::<_, serde_json::Value>(&mut input)
            .await
            .unwrap()
            .unwrap(),
        serde_json::json!({"hello": "世界"})
    );
    assert_eq!(
        codec::read_frame::<_, serde_json::Value>(&mut input)
            .await
            .unwrap()
            .unwrap(),
        serde_json::json!([1, 2])
    );
}
#[test]
fn exec_mode_default_is_worker_and_configuration_accepts_both_modes() {
    assert_eq!(ExecMode::default(), ExecMode::Worker);
    for (name, mode) in [
        ("worker", ExecMode::Worker),
        ("isolated", ExecMode::Worker),
        ("in_process", ExecMode::InProcess),
    ] {
        assert_eq!(
            serde_json::from_value::<ExecMode>(serde_json::json!(name)).unwrap(),
            mode
        );
    }
}
#[tokio::test]
async fn startup_missing_executable_returns_not_executed() {
    use gcu_core::driver::DriverError;
    use gcu_driver_windows::worker::{Supervisor, WorkerConfig};
    let result = Supervisor::start(WorkerConfig::new("/gcu-missing-worker")).await;
    assert!(matches!(
        result,
        Err(DriverError::Crashed {
            maybe_executed: false
        })
    ));
}
#[cfg(all(feature = "test-worker-faults", debug_assertions))]
#[path = "worker_support/process.rs"]
mod process;

#[cfg(windows)]
#[path = "worker_support/native.rs"]
mod native;

//! Isolated debug exit-stage evidence, never an automatic recovery or exit bypass.

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn record(phase: &'static str) {
    if std::env::var("GROK_CU_APP_HARNESS").ok().as_deref() != Some("1")
        || crate::paths::isolated_app_instance_id().is_none()
    {
        return;
    }
    let result = (|| -> Result<(), String> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "probe exit timestamp unavailable")?
            .as_micros();
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(crate::paths::app_data_root().join("native-exit-phases.jsonl"))
            .map_err(|_| "probe exit trace unavailable")?;
        let mut bytes = serde_json::to_vec(&serde_json::json!({
            "phase":phase,"processId":std::process::id(),"timestampUnixMicros":timestamp
        }))
        .map_err(|_| "probe exit trace invalid")?;
        bytes.push(b'\n');
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| "probe exit trace publication failed")?;
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("{error}");
    }
}

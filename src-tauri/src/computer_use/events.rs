//! Thin shot gallery. Ownership and redaction checks live in `gcu-host-protocol`.

use std::path::PathBuf;

use gcu_host_protocol::events::{Fault, Gallery, OwnedPaths};
use serde::Serialize;

use super::ComputerUseHost;
use crate::paths::app_data_root;

/// Session-owned redacted screenshots under app data.
///
/// `open` does not create the directory. The first admitted image does.
#[derive(Clone)]
pub(crate) struct Shots {
    gallery: Gallery,
}

impl Shots {
    pub(crate) fn open() -> Self {
        Self {
            gallery: Gallery::open(app_data_root().join("computer-use").join("shots")),
        }
    }

    pub(crate) fn gallery(&self) -> Gallery {
        self.gallery.clone()
    }

    pub(crate) fn paths(&self, session: &str, id: &str) -> Result<OwnedPaths, Fault> {
        self.gallery.paths(session, id)
    }
}

/// Paths the chat card may preview. Both are absolute files from the gallery.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShotDto {
    thumb_path: Option<String>,
    full_path: Option<String>,
}

pub(crate) fn fault_code(fault: Fault) -> String {
    match fault {
        Fault::Foreign => "foreign",
        Fault::Missing => "missing",
        Fault::Rejected => "rejected",
    }
    .to_owned()
}

/// Return a redacted derivative for this session, fetching the full image once.
///
/// A foreign id never reaches the engine. Filesystem admit runs on a blocking
/// thread. The engine read uses the supervisor's five-second budget. Dropping
/// this future does not cancel an engine read already sent.
pub(crate) async fn tool_shot(
    host: &ComputerUseHost,
    session_id: &str,
    id: &str,
) -> Result<ShotDto, String> {
    let credential = host
        .lifecycle
        .current(session_id)
        .ok_or_else(|| fault_code(Fault::Missing))?;
    match host.shots.paths(session_id, id) {
        Err(Fault::Foreign) => return Err(fault_code(Fault::Foreign)),
        Err(Fault::Rejected) => return Err(fault_code(Fault::Rejected)),
        Ok(paths) if paths.full.is_some() => return dto(paths),
        Ok(_) | Err(Fault::Missing) => {}
    }
    let fetched = {
        let inner = host.inner.lock().await;
        let Some(control) = inner.control.as_ref() else {
            return cached_or_missing(host, session_id, id);
        };
        control.screenshot(session_id, id).await
    };
    match fetched {
        Ok(image) => {
            let lifecycle = host.lifecycle.clone();
            let session = session_id.to_owned();
            let shot_id = id.to_owned();
            let admitted = tauri::async_runtime::spawn_blocking(move || {
                lifecycle.admit_full(&session, &credential, &shot_id, &image)
            })
            .await;
            match admitted {
                Ok(Ok(_)) => match host.shots.paths(session_id, id) {
                    Ok(paths) => dto(paths),
                    Err(fault) => Err(fault_code(fault)),
                },
                Ok(Err(fault)) => Err(fault_code(fault)),
                Err(_) => Err(fault_code(Fault::Rejected)),
            }
        }
        Err(_) => match host.shots.paths(session_id, id) {
            Ok(paths) => dto(paths),
            Err(Fault::Foreign) => Err(fault_code(Fault::Foreign)),
            Err(_) => Err(fault_code(Fault::Missing)),
        },
    }
}

fn cached_or_missing(
    host: &ComputerUseHost,
    session_id: &str,
    id: &str,
) -> Result<ShotDto, String> {
    match host.shots.paths(session_id, id) {
        Ok(paths) => dto(paths),
        Err(Fault::Foreign) => Err(fault_code(Fault::Foreign)),
        Err(_) => Err(fault_code(Fault::Missing)),
    }
}

fn dto(paths: OwnedPaths) -> Result<ShotDto, String> {
    let thumb_path = utf8_path(paths.thumb)?;
    let full_path = utf8_path(paths.full)?;
    if thumb_path.is_none() && full_path.is_none() {
        return Err(fault_code(Fault::Missing));
    }
    Ok(ShotDto {
        thumb_path,
        full_path,
    })
}

fn utf8_path(path: Option<PathBuf>) -> Result<Option<String>, String> {
    match path {
        None => Ok(None),
        Some(path) => path
            .to_str()
            .map(|text| Some(text.to_owned()))
            .ok_or_else(|| fault_code(Fault::Rejected)),
    }
}

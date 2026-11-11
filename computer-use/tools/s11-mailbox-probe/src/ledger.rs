//! Append-only attempt ledger outside the volume. Intent is synced before the effect.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use crate::budget::{is_code, LEDGER_CAP};
use crate::jsonutil::Buf;
use crate::osutil::flag_nofollow;

#[derive(Clone, Debug)]
pub struct Event {
    pub kind: &'static str,
    pub attempt: String,
    pub step: &'static str,
    pub resource: &'static str,
    pub status: Option<i32>,
    pub reason: &'static str,
    pub dev: Option<u64>,
    pub ino: Option<u64>,
}

pub struct Ledger {
    file: File,
    bytes: usize,
}

impl Ledger {
    pub fn create(path: &Path) -> Result<Self, &'static str> {
        if let Some(parent) = path.parent() {
            crate::osutil::mkdir_0700(parent).or_else(|_| {
                let meta = std::fs::symlink_metadata(parent).map_err(|_| "ledger-dir")?;
                if meta.file_type().is_symlink() {
                    return Err("ledger-dir");
                }
                Ok(())
            })?;
        }
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        opts.custom_flags(flag_nofollow());
        let file = opts.open(path).map_err(|_| "ledger-create")?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|_| "ledger-mode")?;
        Ok(Self { file, bytes: 0 })
    }

    pub fn append(&mut self, ev: &Event) -> Result<(), &'static str> {
        if !is_code(ev.kind) || !is_code(ev.step) || !is_code(ev.resource) {
            return Err("ledger-code");
        }
        if !ev.reason.is_empty() && !is_code(ev.reason) {
            return Err("ledger-code");
        }
        if !crate::sha256::is_hex32(&ev.attempt) {
            return Err("ledger-attempt");
        }
        let mut buf = Buf::new(2048);
        buf.push_raw(b"{");
        buf.key("v");
        buf.u64(1);
        buf.comma();
        buf.key("kind");
        buf.code(ev.kind);
        buf.comma();
        buf.key("attempt");
        buf.string(&ev.attempt);
        buf.comma();
        buf.key("step");
        buf.code(ev.step);
        buf.comma();
        buf.key("resource");
        buf.code(ev.resource);
        if let Some(status) = ev.status {
            buf.comma();
            buf.key("status");
            if status < 0 {
                return Err("ledger-status");
            }
            buf.u64(status as u64);
        }
        if !ev.reason.is_empty() {
            buf.comma();
            buf.key("reason");
            buf.code(ev.reason);
        }
        if let Some(dev) = ev.dev {
            buf.comma();
            buf.key("dev");
            buf.u64(dev);
        }
        if let Some(ino) = ev.ino {
            buf.comma();
            buf.key("ino");
            buf.u64(ino);
        }
        buf.push_raw(b"}\n");
        let line = buf.finish()?;
        let text = std::str::from_utf8(&line).map_err(|_| "ledger-json")?;
        let body = text.trim_end_matches('\n');
        crate::jsonutil::parse(body, &crate::jsonutil::Limits::ledger())
            .map_err(|_| "ledger-json")?;
        if self.bytes.saturating_add(line.len()) > LEDGER_CAP {
            return Err("ledger-cap");
        }
        self.file.write_all(&line).map_err(|_| "ledger-io")?;
        self.file.sync_all().map_err(|_| "ledger-sync")?;
        self.bytes += line.len();
        Ok(())
    }

    pub fn intent(
        &mut self,
        attempt: &str,
        step: &'static str,
        resource: &'static str,
    ) -> Result<(), &'static str> {
        self.append(&Event {
            kind: "intent",
            attempt: attempt.to_string(),
            step,
            resource,
            status: None,
            reason: "",
            dev: None,
            ino: None,
        })
    }

    pub fn effect(
        &mut self,
        attempt: &str,
        step: &'static str,
        resource: &'static str,
        status: i32,
    ) -> Result<(), &'static str> {
        self.append(&Event {
            kind: "effect",
            attempt: attempt.to_string(),
            step,
            resource,
            status: Some(status),
            reason: "",
            dev: None,
            ino: None,
        })
    }

    pub fn identity(
        &mut self,
        attempt: &str,
        step: &'static str,
        resource: &'static str,
        dev: u64,
        ino: u64,
    ) -> Result<(), &'static str> {
        self.append(&Event {
            kind: "identity",
            attempt: attempt.to_string(),
            step,
            resource,
            status: None,
            reason: "",
            dev: Some(dev),
            ino: Some(ino),
        })
    }

    pub fn quarantine(
        &mut self,
        attempt: &str,
        step: &'static str,
        reason: &'static str,
    ) -> Result<(), &'static str> {
        self.append(&Event {
            kind: "quarantine",
            attempt: attempt.to_string(),
            step,
            resource: "none",
            status: None,
            reason,
            dev: None,
            ino: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_line_is_durable_before_a_later_effect() {
        let dir = std::env::temp_dir().join(format!("s11-ledger-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("events.jsonl");
        let mut ledger = Ledger::create(&path).unwrap();
        let id = "0123456789abcdef0123456789abcdef";
        ledger.intent(id, "hdiutil-create", "I").unwrap();
        let mid = std::fs::read_to_string(&path).unwrap();
        assert!(mid.contains("\"kind\":\"intent\""));
        assert!(!mid.contains("\"kind\":\"effect\""));
        ledger.effect(id, "hdiutil-create", "I", 0).unwrap();
        ledger.identity(id, "hdiutil-create", "I", 7, 9).unwrap();
        let all = std::fs::read_to_string(&path).unwrap();
        let intent_at = all.find("\"kind\":\"intent\"").unwrap();
        let effect_at = all.find("\"kind\":\"effect\"").unwrap();
        let ident_at = all.find("\"kind\":\"identity\"").unwrap();
        assert!(intent_at < effect_at && effect_at < ident_at);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! Report JSON. Fields are case, subcase, samples, observation points, max,
//! errors, unknown reason, and the fixed evidence tag. No host paths.

use std::path::Path;

use crate::budget::{is_code, EVIDENCE, REPORT_CAP};
use crate::cases::Judgement;
use crate::jsonutil::Buf;
use crate::osutil::file_0600;

#[derive(Clone, Debug)]
pub struct SubRow {
    pub id: &'static str,
    pub attempt: String,
    pub judgement: Judgement,
    pub cleanup_absent: bool,
}

pub fn render(case: &str, status: &str, rows: &[SubRow]) -> Result<Vec<u8>, &'static str> {
    if !is_code(case) || !is_code(status) {
        return Err("report-code");
    }
    let mut buf = Buf::new(REPORT_CAP);
    buf.push_raw(b"{");
    buf.key("v");
    buf.u64(1);
    buf.comma();
    buf.key("evidence");
    buf.string(EVIDENCE);
    buf.comma();
    buf.key("case");
    buf.code(case);
    buf.comma();
    buf.key("status");
    buf.code(status);
    buf.comma();
    buf.key("subcases");
    buf.push_raw(b"[");
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            buf.comma();
        }
        write_row(&mut buf, row)?;
    }
    buf.push_raw(b"]}");
    let bytes = buf.finish()?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "report-utf8")?;
    if text.contains("/Users") || text.contains("/home") || text.contains("sparseimage") {
        return Err("report-private");
    }
    Ok(bytes)
}

fn write_row(buf: &mut Buf, row: &SubRow) -> Result<(), &'static str> {
    if !is_code(row.id) || !crate::sha256::is_hex32(&row.attempt) {
        return Err("report-code");
    }
    let j = &row.judgement;
    if !j.unknown_reason.is_empty() && !is_code(j.unknown_reason) {
        return Err("report-code");
    }
    if !is_code(j.verdict) {
        return Err("report-code");
    }
    buf.push_raw(b"{");
    buf.key("subcase");
    buf.code(row.id);
    buf.comma();
    buf.key("attempt");
    buf.string(&row.attempt);
    buf.comma();
    buf.key("samples");
    buf.u64(j.samples);
    buf.comma();
    buf.key("observation_points");
    buf.u64(j.observation_points);
    buf.comma();
    buf.key("max");
    buf.push_raw(b"{");
    buf.key("open_us");
    buf.u64(j.max_open_us);
    buf.comma();
    buf.key("read_us");
    buf.u64(j.max_read_us);
    buf.comma();
    buf.key("publish_us");
    buf.u64(j.max_publish_us);
    buf.comma();
    buf.key("rtt_us");
    buf.u64(j.max_rtt_us);
    buf.push_raw(b"}");
    buf.comma();
    buf.key("errors");
    buf.codes(&j.errors);
    buf.comma();
    buf.key("notes");
    buf.codes(&j.notes);
    buf.comma();
    buf.key("unknown_reason");
    buf.code(if j.unknown_reason.is_empty() {
        "none"
    } else {
        j.unknown_reason
    });
    buf.comma();
    buf.key("verdict");
    buf.code(j.verdict);
    buf.comma();
    buf.key("cleanup_absent");
    buf.bool(row.cleanup_absent);
    buf.push_raw(b"}");
    Ok(())
}

pub fn aggregate_status(rows: &[SubRow], dry_run: bool) -> &'static str {
    if dry_run {
        return "dry_run";
    }
    if rows.is_empty() {
        return "not_run";
    }
    if rows.iter().any(|r| r.judgement.verdict == "quarantine") {
        return "quarantine";
    }
    if rows.iter().any(|r| r.judgement.verdict == "ambiguous") {
        return "ambiguous";
    }
    if rows.iter().any(|r| r.judgement.verdict == "failed") {
        return "failed";
    }
    if rows.iter().any(|r| r.judgement.verdict == "block") {
        return "block";
    }
    if rows.iter().any(|r| r.judgement.verdict == "unknown") {
        return "unknown";
    }
    "ok"
}

pub fn store(path: &Path, bytes: &[u8]) -> Result<(), &'static str> {
    let mut file = file_0600(path).map_err(|_| "report-write")?;
    std::io::Write::write_all(&mut file, bytes).map_err(|_| "report-write")?;
    file.sync_all().map_err(|_| "report-write")?;
    Ok(())
}

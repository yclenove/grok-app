//! Report JSON. Paths and p99 are rejected so they cannot leak into the file.

use crate::budget::{EVIDENCE, EXPORT_LABEL, REPORT_CAP};
use crate::cases::Row;
use crate::jsonutil::Buf;

pub fn render(
    case: &str,
    verdict: &str,
    cleanup_absent: bool,
    reason: Option<&str>,
    rows: &[Row],
) -> Result<Vec<u8>, &'static str> {
    let mut buf = Buf::new(REPORT_CAP);
    buf.push_raw(b"{");
    buf.key("v");
    buf.u64(1);
    buf.comma();
    buf.key("evidence");
    buf.string(EVIDENCE);
    buf.comma();
    buf.key("export");
    buf.string(EXPORT_LABEL);
    buf.comma();
    buf.key("case");
    buf.code(case);
    buf.comma();
    buf.key("verdict");
    buf.code(verdict);
    buf.comma();
    buf.key("cleanup_absent");
    buf.bool(cleanup_absent);
    buf.comma();
    buf.key("unknown_reason");
    match reason {
        Some(r) => buf.code(r),
        None => buf.null(),
    }
    buf.comma();
    buf.key("rows");
    buf.push_raw(b"[");
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            buf.comma();
        }
        write_row(&mut buf, row);
    }
    buf.push_raw(b"]}");
    let bytes = buf.finish()?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "report")?;
    if text.contains("/Users") || text.contains("/home") || text.contains("p99") {
        return Err("report");
    }
    Ok(bytes)
}

fn write_row(buf: &mut Buf, row: &Row) {
    buf.push_raw(b"{");
    buf.key("subcase");
    buf.code(row.subcase);
    buf.comma();
    buf.key("verdict");
    buf.code(row.verdict);
    buf.comma();
    buf.key("samples");
    buf.u64(row.samples);
    buf.comma();
    buf.key("observation_points");
    buf.u64(row.observation_points);
    buf.comma();
    opt(buf, "open_us", row.open_us);
    opt(buf, "write_us", row.write_us);
    opt(buf, "fsync_us", row.fsync_us);
    opt(buf, "rename_us", row.rename_us);
    opt(buf, "read_us", row.read_us);
    opt(buf, "publish_us", row.publish_us);
    opt(buf, "rtt_us", row.rtt_us);
    opt(buf, "guest_read_us", row.guest_read_us);
    opt(buf, "guest_publish_us", row.guest_publish_us);
    buf.key("errors");
    buf.codes(&row.errors);
    buf.comma();
    buf.key("notes");
    buf.codes(&row.notes);
    buf.comma();
    buf.key("unknown_reason");
    match row.unknown_reason {
        Some(r) => buf.code(r),
        None => buf.null(),
    }
    buf.push_raw(b"}");
}

fn opt(buf: &mut Buf, key: &str, value: Option<u64>) {
    buf.key(key);
    match value {
        Some(v) => buf.u64(v),
        None => buf.null(),
    }
    buf.comma();
}

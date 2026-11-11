//! C1 / C2 / C4 / C8 load, observation, and verdicts.
//! Clocks are local `Instant` values. Patterns come from `pattern`, not from the peer.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::budget::{is_code, C1_ITERS, FRAME_LEN, RTT_BLOCK_US, SLICE_NOTE_US};
use crate::pattern;
use crate::publish;
use crate::slot::{self, SlotError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    C1,
    C2,
    C4,
    C8,
}

impl Case {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "C1" => Some(Self::C1),
            "C2" => Some(Self::C2),
            "C4" => Some(Self::C4),
            "C8" => Some(Self::C8),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::C1 => "C1",
            Self::C2 => "C2",
            Self::C4 => "C4",
            Self::C8 => "C8",
        }
    }

    pub fn subcases(self) -> &'static [&'static str] {
        match self {
            Self::C1 => &["rename-latency"],
            Self::C2 => &["file-sync", "dir-sync", "rename-boundary"],
            Self::C4 => &["create", "attach"],
            Self::C8 => &["subjects"],
        }
    }
}

#[derive(Clone, Debug)]
pub struct Sample {
    pub direction: u8,
    pub seq: u8,
    pub open_us: u64,
    pub read_us: u64,
    pub publish_us: u64,
    pub rtt_us: u64,
    pub complete: bool,
    pub torn: bool,
    pub lost: bool,
}

#[derive(Clone, Debug)]
pub struct Judgement {
    pub samples: u64,
    pub observation_points: u64,
    pub max_open_us: u64,
    pub max_read_us: u64,
    pub max_publish_us: u64,
    pub max_rtt_us: u64,
    pub errors: Vec<&'static str>,
    pub notes: Vec<&'static str>,
    pub unknown_reason: &'static str,
    pub verdict: &'static str,
    pub stop_next: bool,
}

impl Judgement {
    fn blank(verdict: &'static str, reason: &'static str, stop: bool) -> Self {
        Self {
            samples: 0,
            observation_points: 0,
            max_open_us: 0,
            max_read_us: 0,
            max_publish_us: 0,
            max_rtt_us: 0,
            errors: Vec::new(),
            notes: Vec::new(),
            unknown_reason: reason,
            verdict,
            stop_next: stop,
        }
    }
}

pub fn judge_c1(
    samples: &[Sample],
    guest_digest: Option<&str>,
    expected_digest: &str,
) -> Judgement {
    let mut max_open = 0u64;
    let mut max_read = 0u64;
    let mut max_pub = 0u64;
    let mut max_rtt = 0u64;
    let mut errors = Vec::new();
    let mut notes = Vec::new();
    for sample in samples {
        max_open = max_open.max(sample.open_us);
        max_read = max_read.max(sample.read_us);
        max_pub = max_pub.max(sample.publish_us);
        max_rtt = max_rtt.max(sample.rtt_us);
        if sample.torn {
            errors.push("torn-frame");
        }
        if sample.lost || !sample.complete {
            errors.push("lost-update");
        }
    }
    if max_open >= SLICE_NOTE_US || max_read >= SLICE_NOTE_US || max_pub >= SLICE_NOTE_US {
        notes.push("slice-over-100ms");
    }
    let digest_ok = guest_digest == Some(expected_digest);
    if guest_digest.is_some() && !digest_ok {
        errors.push("digest-mismatch");
    }
    let full = samples.len() == usize::from(C1_ITERS) * 2
        && (1..=C1_ITERS).all(|seq| {
            samples
                .iter()
                .any(|s| s.direction == 1 && s.seq == seq && s.complete && !s.torn)
                && samples
                    .iter()
                    .any(|s| s.direction == 2 && s.seq == seq && s.complete && !s.torn)
        });
    let verdict;
    let reason;
    let stop;
    if errors
        .iter()
        .any(|e| *e == "torn-frame" || *e == "digest-mismatch")
    {
        verdict = "failed";
        reason = "";
        stop = true;
    } else if !full || errors.contains(&"lost-update") {
        verdict = "unknown";
        reason = "incomplete-samples";
        stop = true;
    } else if max_rtt >= RTT_BLOCK_US {
        verdict = "block";
        reason = "";
        errors.push("rtt-no-schedule-margin");
        stop = true;
    } else {
        verdict = "ok";
        reason = "";
        stop = false;
    }
    Judgement {
        samples: samples.len() as u64,
        observation_points: 4,
        max_open_us: max_open,
        max_read_us: max_read,
        max_publish_us: max_pub,
        max_rtt_us: max_rtt,
        errors,
        notes,
        unknown_reason: reason,
        verdict,
        stop_next: stop,
    }
}

pub fn judge_c2(
    boundary: &str,
    publishes: u64,
    saw_complete: bool,
    mixed: bool,
    acked: bool,
) -> Judgement {
    let mut j = Judgement::blank("unknown", "durability-not-claimed", false);
    j.samples = publishes;
    j.observation_points = 3;
    j.notes.push("power-loss-not-inferred");
    if !matches!(boundary, "file-sync" | "dir-sync" | "rename-boundary") || publishes != 3 {
        j.unknown_reason = "bad-boundary";
        j.verdict = "unknown";
        j.stop_next = true;
        return j;
    }
    if mixed {
        j.errors.push("torn-frame");
        j.verdict = "failed";
        j.unknown_reason = "";
        j.stop_next = true;
        return j;
    }
    if acked {
        j.errors.push("false-ack");
        j.verdict = "failed";
        j.unknown_reason = "";
        j.stop_next = true;
        return j;
    }
    if !saw_complete {
        j.notes.push("bytes-absent");
    }
    j.verdict = "not-ack";
    j.stop_next = false;
    j
}

#[derive(Clone, Debug)]
pub struct ProbeItem {
    pub id: &'static str,
    pub state: &'static str,
}

pub fn judge_c4(items: &[ProbeItem]) -> Judgement {
    let mut j = Judgement::blank("unknown", "", true);
    j.samples = items.len() as u64;
    j.observation_points = items.len() as u64;
    j.notes.push("host-flags-are-not-guest-sandbox");
    if items.len() > 12 {
        j.errors.push("probe-over-cap");
        j.verdict = "failed";
        return j;
    }
    if items.is_empty() {
        j.unknown_reason = "no-probes";
        return j;
    }
    if items
        .iter()
        .any(|it| !is_code(it.id) || !matches!(it.state, "pass" | "fail" | "unknown"))
    {
        j.errors.push("probe-schema");
        j.verdict = "failed";
        return j;
    }
    if items.iter().any(|it| it.state == "fail") {
        j.errors.push("mount-gate");
        j.verdict = "failed";
        return j;
    }
    if items.iter().any(|it| it.state == "unknown") || items.len() < 12 {
        j.unknown_reason = "probe-unknown";
        j.verdict = "unknown";
        // A full set of probes that includes an unknown item is a recorded result.
        // Fewer than 12 means the probe itself did not finish, so the next subcase waits.
        j.stop_next = items.len() < 12;
        return j;
    }
    j.verdict = "ok";
    j.stop_next = false;
    j.unknown_reason = "";
    j
}

#[derive(Clone, Debug)]
pub struct SubjectObs {
    pub non_admin: u64,
    pub ops: u64,
    pub chown: &'static str,
    pub created_account: bool,
    pub elevated: bool,
}

pub fn judge_c8(obs: &SubjectObs) -> Judgement {
    let mut j = Judgement::blank("unknown", "single-non-admin-subject", true);
    j.samples = obs.ops;
    j.observation_points = 4;
    if obs.created_account || obs.elevated {
        j.verdict = "failed";
        j.unknown_reason = "";
        j.errors.push("elevation-forbidden");
        return j;
    }
    if obs.ops > 12 {
        j.verdict = "failed";
        j.unknown_reason = "";
        j.errors.push("ops-over-cap");
        return j;
    }
    if obs.non_admin < 2 {
        j.unknown_reason = "single-non-admin-subject";
        if obs.ops != 0 {
            j.errors.push("ops-without-second-subject");
            j.verdict = "failed";
        }
        return j;
    }
    j.unknown_reason = "second-subject-needs-elevation";
    if obs.chown != "denied" {
        j.notes.push("chown-not-denied");
    }
    j
}

pub fn count_non_admin(unique: &str, admin_group: &str) -> Result<u64, &'static str> {
    let mut admins = Vec::new();
    for tok in admin_group.split_whitespace() {
        if tok == "GroupMembership" || tok == ":" {
            continue;
        }
        if tok
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            admins.push(tok.to_string());
        }
    }
    let mut count = 0u64;
    for line in unique.lines() {
        let mut parts = line.split_whitespace();
        let name = parts.next().ok_or("subject-parse")?;
        let uid: u64 = parts
            .next()
            .ok_or("subject-parse")?
            .parse()
            .map_err(|_| "subject-parse")?;
        if parts.next().is_some() {
            return Err("subject-parse");
        }
        if name.starts_with('_') || name == "root" || name == "daemon" || uid < 500 {
            continue;
        }
        if admins.iter().any(|a| a == name) {
            continue;
        }
        count += 1;
    }
    Ok(count)
}

fn host_paths(volume: &Path) -> (PathBuf, PathBuf, PathBuf) {
    (
        volume.join("host-stage"),
        volume.join("mailbox").join("host-write"),
        volume.join("mailbox").join("guest-write"),
    )
}

fn wait_frame(dir: &Path, name: &str, expect: &[u8], limit: Duration) -> (u64, bool, bool) {
    let start = Instant::now();
    let mut mixed = false;
    loop {
        let open_at = Instant::now();
        match slot::read_named(dir, name, expect.len() as u64, expect.len() as u64) {
            Ok(got) => {
                let read_us = open_at.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
                if got == expect {
                    return (read_us, true, false);
                }
                // A full older frame is still the previous publish. Wait for the new bytes.
                // A non-spec buffer can be a rename observed mid-update; keep polling.
                if !pattern::is_spec_frame(&got) {
                    mixed = true;
                }
                if start.elapsed() >= limit {
                    return if mixed {
                        (read_us, false, true)
                    } else {
                        (0, false, false)
                    };
                }
            }
            Err(SlotError::NotFile)
            | Err(SlotError::Symlink)
            | Err(SlotError::HardLink)
            | Err(SlotError::Name) => {
                return (0, false, true);
            }
            Err(SlotError::Fsid { .. }) => {
                mixed = true;
                if start.elapsed() >= limit {
                    return (0, false, true);
                }
            }
            Err(SlotError::Size) | Err(SlotError::Io { .. }) | Err(SlotError::Replaced) => {
                if start.elapsed() >= limit {
                    return if mixed {
                        (0, false, true)
                    } else {
                        (0, false, false)
                    };
                }
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
}

pub fn drive_c1(
    volume: &Path,
    visibility: Duration,
) -> Result<(Vec<Sample>, String), &'static str> {
    let (stage, host_write, guest_write) = host_paths(volume);
    let mut samples = Vec::new();
    let mut published = Vec::new();
    for seq in 1..=C1_ITERS {
        let bytes = pattern::frame(1, 1, seq);
        let t0 = Instant::now();
        let times = publish::publish(&stage, &host_write, "c1-frame", &bytes)?;
        let expect = pattern::frame(1, 2, seq);
        let (read_us, complete, torn) = wait_frame(&guest_write, "c1-frame", &expect, visibility);
        let rtt_us = t0.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
        let publish_us = times
            .write_us
            .saturating_add(times.fsync_us)
            .saturating_add(times.rename_us);
        samples.push(Sample {
            direction: 1,
            seq,
            open_us: times.write_us,
            read_us: times.write_us,
            publish_us,
            rtt_us,
            complete,
            torn,
            lost: !complete && !torn,
        });
        published.push(bytes);
        if !complete {
            break;
        }
        samples.push(Sample {
            direction: 2,
            seq,
            open_us: read_us,
            read_us,
            publish_us: read_us,
            rtt_us,
            complete: true,
            torn: false,
            lost: false,
        });
    }
    Ok((samples, pattern::concat_digest(&published)))
}

pub fn guest_c1(export_root: &Path, visibility: Duration) -> Result<(), &'static str> {
    let host_write = export_root.join("host-write");
    let guest_write = export_root.join("guest-write");
    let stage = guest_write.clone();
    let mut seen = Vec::new();
    let mut max_read = 0u64;
    let mut max_pub = 0u64;
    let mut torn = 0u64;
    let mut lost = 0u64;
    for seq in 1..=C1_ITERS {
        let expect = pattern::frame(1, 1, seq);
        let (read_us, complete, is_torn) = wait_frame(&host_write, "c1-frame", &expect, visibility);
        max_read = max_read.max(read_us);
        if !complete {
            if is_torn {
                torn += 1;
            } else {
                lost += 1;
            }
            break;
        }
        seen.push(expect);
        let bytes = pattern::frame(1, 2, seq);
        let times = publish::publish(&stage, &guest_write, "c1-frame", &bytes)?;
        max_pub = max_pub.max(
            times
                .write_us
                .saturating_add(times.fsync_us)
                .saturating_add(times.rename_us),
        );
    }
    let digest = pattern::concat_digest(&seen);
    let body = guest_c1_json(seen.len() as u64, torn, lost, &digest, max_read, max_pub)?;
    slot::write_result_slot(&guest_write, body.as_bytes()).map_err(|_| "slot-write")?;
    Ok(())
}

fn guest_c1_json(
    samples: u64,
    torn: u64,
    lost: u64,
    digest: &str,
    max_read: u64,
    max_pub: u64,
) -> Result<String, &'static str> {
    let mut buf = crate::jsonutil::Buf::new(4096);
    buf.push_raw(b"{");
    buf.key("v");
    buf.u64(1);
    buf.comma();
    buf.key("op");
    buf.code("c1");
    buf.comma();
    buf.key("samples");
    buf.u64(samples);
    buf.comma();
    buf.key("torn");
    buf.u64(torn);
    buf.comma();
    buf.key("lost");
    buf.u64(lost);
    buf.comma();
    buf.key("digest");
    buf.string(digest);
    buf.comma();
    buf.key("max_read_us");
    buf.u64(max_read);
    buf.comma();
    buf.key("max_publish_us");
    buf.u64(max_pub);
    buf.push_raw(b"}");
    let bytes = buf.finish()?;
    String::from_utf8(bytes).map_err(|_| "json-utf8")
}

pub fn guest_c2(export_root: &Path, boundary: &str) -> Result<(), &'static str> {
    if !matches!(boundary, "file-sync" | "dir-sync" | "rename-boundary") {
        return Err("bad-boundary");
    }
    let guest_write = export_root.join("guest-write");
    for seq in 1..=3 {
        let bytes = pattern::frame(2, 2, seq);
        let tmp = guest_write.join(format!("c2-{seq}.part"));
        std::fs::write(&tmp, bytes).map_err(|_| "c2-write")?;
        match boundary {
            "file-sync" => {
                let f = std::fs::File::open(&tmp).map_err(|_| "c2-open")?;
                f.sync_all().map_err(|_| "c2-fsync")?;
            }
            "dir-sync" => {
                let d = std::fs::File::open(&guest_write).map_err(|_| "c2-open")?;
                let _ = d.sync_all();
            }
            _ => {}
        }
        std::fs::rename(
            guest_write.join(format!("c2-{seq}.part")),
            guest_write.join("c2-frame"),
        )
        .map_err(|_| "c2-rename")?;
        if boundary == "rename-boundary" {
            let d = std::fs::File::open(&guest_write).map_err(|_| "c2-open")?;
            let _ = d.sync_all();
        }
    }
    // Planned exit fault: no result slot, so the host must not treat this as an ACK.
    Err("fault-before-ack")
}

pub fn c2_frame_status(export_root: &Path) -> (bool, bool) {
    let path_dir = export_root.join("guest-write");
    match slot::read_named(&path_dir, "c2-frame", FRAME_LEN as u64, FRAME_LEN as u64) {
        Ok(got) => {
            let complete = (1..=3).any(|seq| got == pattern::frame(2, 2, seq));
            (!complete, complete)
        }
        Err(SlotError::Io { .. }) => (false, false),
        Err(_) => (true, false),
    }
}

pub fn parse_c1_slot(bytes: &[u8]) -> Result<(u64, String), &'static str> {
    let text = std::str::from_utf8(bytes).map_err(|_| "slot-utf8")?;
    let val = crate::jsonutil::parse(text, &crate::jsonutil::Limits::result_slot())?;
    crate::jsonutil::require_keys(
        &val,
        &[
            "v",
            "op",
            "samples",
            "torn",
            "lost",
            "digest",
            "max_read_us",
            "max_publish_us",
        ],
    )?;
    if val.get("v").and_then(|v| v.int()) != Some(1)
        || val.get("op").and_then(|v| v.str()) != Some("c1")
    {
        return Err("slot-schema");
    }
    let digest = val
        .get("digest")
        .and_then(|v| v.str())
        .ok_or("slot-schema")?;
    if !crate::sha256::is_hex64(digest) {
        return Err("slot-schema");
    }
    let samples = val
        .get("samples")
        .and_then(|v| v.int())
        .ok_or("slot-schema")?;
    Ok((samples, digest.to_string()))
}

/// Own-file operations for the current subject only. Never creates an account.
pub fn self_ops(dir: &Path) -> Result<(u64, &'static str), &'static str> {
    if volume_is_dir(dir).is_err() {
        crate::osutil::mkdir_0700(dir).map_err(|_| "self-ops")?;
    }
    let file = dir.join("own-file");
    let renamed = dir.join("own-renamed");
    let mut ops = 0u64;
    std::fs::write(&file, b"s11").map_err(|_| "self-ops")?;
    ops += 1;
    let mode = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(&file, mode).map_err(|_| "self-ops")?;
    ops += 1;
    std::fs::rename(&file, &renamed).map_err(|_| "self-ops")?;
    ops += 1;
    let _ = crate::volume::lstat(&renamed)?;
    ops += 1;
    let chown = crate::osutil::chown_other(&renamed)?;
    ops += 1;
    if ops > 12 {
        return Err("ops-over-cap");
    }
    Ok((ops, chown))
}

pub fn observe_c8_text(
    unique: &str,
    admin_group: &str,
    work: Option<&Path>,
) -> Result<SubjectObs, &'static str> {
    let non_admin = count_non_admin(unique, admin_group)?;
    if non_admin < 2 {
        return Ok(SubjectObs {
            non_admin,
            ops: 0,
            chown: "skipped",
            created_account: false,
            elevated: false,
        });
    }
    let Some(work) = work else {
        return Ok(SubjectObs {
            non_admin,
            ops: 0,
            chown: "skipped",
            created_account: false,
            elevated: false,
        });
    };
    let (ops, chown) = self_ops(work)?;
    Ok(SubjectObs {
        non_admin,
        ops,
        chown,
        created_account: false,
        elevated: false,
    })
}

pub fn c8_slot_json(obs: &SubjectObs) -> Result<Vec<u8>, &'static str> {
    if !matches!(obs.chown, "denied" | "skipped" | "allowed" | "error") {
        return Err("c8-chown");
    }
    let mut buf = crate::jsonutil::Buf::new(4096);
    buf.push_raw(b"{");
    buf.key("v");
    buf.u64(1);
    buf.comma();
    buf.key("op");
    buf.code("c8");
    buf.comma();
    buf.key("non_admin");
    buf.u64(obs.non_admin);
    buf.comma();
    buf.key("ops");
    buf.u64(obs.ops);
    buf.comma();
    buf.key("chown");
    buf.code(obs.chown);
    buf.comma();
    buf.key("created_account");
    buf.bool(obs.created_account);
    buf.comma();
    buf.key("elevated");
    buf.bool(obs.elevated);
    buf.push_raw(b"}");
    buf.finish()
}

pub fn parse_c8_slot(bytes: &[u8]) -> Result<SubjectObs, &'static str> {
    let text = std::str::from_utf8(bytes).map_err(|_| "slot-utf8")?;
    let val = crate::jsonutil::parse(text, &crate::jsonutil::Limits::result_slot())?;
    crate::jsonutil::require_keys(
        &val,
        &[
            "v",
            "op",
            "non_admin",
            "ops",
            "chown",
            "created_account",
            "elevated",
        ],
    )?;
    if val.get("v").and_then(|v| v.int()) != Some(1)
        || val.get("op").and_then(|v| v.str()) != Some("c8")
    {
        return Err("slot-schema");
    }
    let chown = chown_code(
        val.get("chown")
            .and_then(|v| v.str())
            .ok_or("slot-schema")?,
    )?;
    Ok(SubjectObs {
        non_admin: val
            .get("non_admin")
            .and_then(|v| v.int())
            .ok_or("slot-schema")?,
        ops: val.get("ops").and_then(|v| v.int()).ok_or("slot-schema")?,
        chown,
        created_account: val
            .get("created_account")
            .and_then(|v| v.bool())
            .ok_or("slot-schema")?,
        elevated: val
            .get("elevated")
            .and_then(|v| v.bool())
            .ok_or("slot-schema")?,
    })
}

fn chown_code(s: &str) -> Result<&'static str, &'static str> {
    match s {
        "denied" => Ok("denied"),
        "skipped" => Ok("skipped"),
        "allowed" => Ok("allowed"),
        "error" => Ok("error"),
        _ => Err("slot-schema"),
    }
}

fn volume_is_dir(path: &Path) -> Result<(), &'static str> {
    let st = crate::volume::lstat(path)?;
    if st.is_dir && !st.is_link {
        Ok(())
    } else {
        Err("self-ops")
    }
}

pub fn prepare_volume(volume: &Path) -> Result<(), &'static str> {
    for rel in [
        "host-stage",
        "mailbox/host-write",
        "mailbox/guest-write",
        ".fseventsd",
    ] {
        let path = volume.join(rel);
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                osutil_mkdir(parent)?;
            }
        }
        if !path.exists() {
            osutil_mkdir(&path)?;
        }
    }
    let marker = volume.join(".metadata_never_index");
    if !marker.exists() {
        std::fs::write(&marker, b"").map_err(|_| "marker")?;
    }
    let nolog = volume.join(".fseventsd").join("no_log");
    if !nolog.exists() {
        std::fs::write(&nolog, b"").map_err(|_| "marker")?;
    }
    Ok(())
}

fn osutil_mkdir(path: &Path) -> Result<(), &'static str> {
    crate::osutil::mkdir_0700(path).map_err(|_| "mkdir")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c1_judge_blocks_on_rtt_and_fails_on_tear() {
        let mut samples = Vec::new();
        for seq in 1..=8 {
            for direction in [1u8, 2] {
                samples.push(Sample {
                    direction,
                    seq,
                    open_us: 10,
                    read_us: 10,
                    publish_us: 10,
                    rtt_us: 100,
                    complete: true,
                    torn: false,
                    lost: false,
                });
            }
        }
        let j = judge_c1(&samples, Some("ab"), "ab");
        assert_eq!(j.verdict, "ok");
        assert!(!j.stop_next);
        samples[0].rtt_us = RTT_BLOCK_US;
        let j = judge_c1(&samples, Some("ab"), "ab");
        assert_eq!(j.verdict, "block");
        assert!(j.errors.contains(&"rtt-no-schedule-margin"));
        samples[0].torn = true;
        let j = judge_c1(&samples, Some("ab"), "ab");
        assert_eq!(j.verdict, "failed");
    }

    #[test]
    fn c2_c4_c8_verdicts() {
        let j = judge_c2("file-sync", 3, true, false, false);
        assert_eq!(j.verdict, "not-ack");
        assert!(j.notes.contains(&"power-loss-not-inferred"));
        assert_eq!(
            judge_c2("file-sync", 3, true, false, true).verdict,
            "failed"
        );
        let items = vec![
            ProbeItem {
                id: "create-exit",
                state: "unknown"
            };
            12
        ];
        assert_eq!(judge_c4(&items).verdict, "unknown");
        assert_eq!(judge_c4(&items[..1]).unknown_reason, "probe-unknown");
        let one = SubjectObs {
            non_admin: 1,
            ops: 0,
            chown: "skipped",
            created_account: false,
            elevated: false,
        };
        assert_eq!(judge_c8(&one).unknown_reason, "single-non-admin-subject");
        let bad = SubjectObs {
            non_admin: 1,
            ops: 1,
            chown: "skipped",
            created_account: false,
            elevated: false,
        };
        assert_eq!(judge_c8(&bad).verdict, "failed");
        let two = SubjectObs {
            non_admin: 2,
            ops: 12,
            chown: "denied",
            created_account: false,
            elevated: false,
        };
        assert_eq!(
            judge_c8(&two).unknown_reason,
            "second-subject-needs-elevation"
        );
        let elev = SubjectObs {
            created_account: true,
            ..two
        };
        assert_eq!(judge_c8(&elev).verdict, "failed");
    }

    #[test]
    fn one_subject_does_not_touch_the_filesystem() {
        let dir = std::env::temp_dir().join(format!(
            "s11-c8-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let obs = observe_c8_text("eval 502\n", "GroupMembership: root", Some(&dir)).unwrap();
        assert_eq!(obs.ops, 0);
        assert_eq!(obs.chown, "skipped");
        assert!(!dir.exists());
        let obs =
            observe_c8_text("eval 502\nroleb 503\n", "GroupMembership: root", Some(&dir)).unwrap();
        assert!(obs.ops > 0 && obs.ops <= 12);
        assert!(matches!(obs.chown, "denied" | "allowed" | "error"));
        assert!(!obs.created_account && !obs.elevated);
        let back = parse_c8_slot(&c8_slot_json(&obs).unwrap()).unwrap();
        assert_eq!(back.non_admin, obs.non_admin);
        assert_eq!(back.ops, obs.ops);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn subject_count_ignores_system_and_admin_names() {
        let unique = "root 0\ndaemon 1\n_foo 501\neval 502\nroleb 503\nadminuser 504\n";
        let group = "GroupMembership: root adminuser";
        assert_eq!(count_non_admin(unique, group).unwrap(), 2);
    }

    #[test]
    fn c1_exchange_matches_and_c2_does_not_ack() {
        let dir = std::env::temp_dir().join(format!(
            "s11-c1-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        prepare_volume(&dir).unwrap();
        let export = dir.join("mailbox");
        let guest = thread::spawn(move || guest_c1(&export, Duration::from_secs(3)));
        let (samples, digest) = drive_c1(&dir, Duration::from_secs(3)).unwrap();
        guest.join().unwrap().unwrap();
        let slot = slot::read_result_slot(&dir.join("mailbox").join("guest-write")).unwrap();
        let (_n, guest_digest) = parse_c1_slot(&slot).unwrap();
        let judged = judge_c1(&samples, Some(&guest_digest), &digest);
        assert_eq!(judged.verdict, "ok", "{judged:?}");
        assert_eq!(judged.samples, 16);

        let c2 = std::env::temp_dir().join(format!("s11-c2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&c2);
        std::fs::create_dir(&c2).unwrap();
        prepare_volume(&c2).unwrap();
        let export = c2.join("mailbox");
        assert_eq!(
            guest_c2(&export, "file-sync").unwrap_err(),
            "fault-before-ack"
        );
        assert!(slot::read_result_slot(&export.join("guest-write")).is_err());
        let (mixed, complete) = c2_frame_status(&export);
        assert!(complete && !mixed);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&c2);
    }
}

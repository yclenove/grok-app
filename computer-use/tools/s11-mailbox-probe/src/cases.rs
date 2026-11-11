//! C1, C2, and C8 judgements. Torn frames stick. Missing evidence stays unknown.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::fs::File;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use crate::budget::{C1_ITERS, FRAME_LEN, RTT_BLOCK_US, SLICE_NOTE_US, SLOT_CAP, VISIBILITY_MS};
use crate::jsonutil::{self, Buf, Limits, Val};
use crate::osutil::{self, Held};
use crate::pattern::{self, frame};
use crate::publish::{self, Cursor, PubTimes};
use crate::runner::{host_cmd, Runner};
use crate::sha256::is_hex64;
use crate::slot::{self, SlotError, TimedRead};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    C1,
    C2,
    C8,
}

impl Case {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "C1" => Some(Case::C1),
            "C2" => Some(Case::C2),
            "C8" => Some(Case::C8),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Case::C1 => "C1",
            Case::C2 => "C2",
            Case::C8 => "C8",
        }
    }

    pub fn subcases(self) -> &'static [&'static str] {
        match self {
            Case::C1 => &["rename-latency"],
            Case::C2 => &["file-sync", "dir-sync", "rename-boundary"],
            Case::C8 => &["subjects"],
        }
    }
}

#[derive(Clone, Debug)]
pub struct Row {
    pub subcase: &'static str,
    pub verdict: &'static str,
    pub samples: u64,
    pub observation_points: u64,
    pub open_us: Option<u64>,
    pub write_us: Option<u64>,
    pub fsync_us: Option<u64>,
    pub rename_us: Option<u64>,
    pub read_us: Option<u64>,
    pub publish_us: Option<u64>,
    pub rtt_us: Option<u64>,
    pub guest_read_us: Option<u64>,
    pub guest_publish_us: Option<u64>,
    pub errors: Vec<&'static str>,
    pub notes: Vec<&'static str>,
    pub unknown_reason: Option<&'static str>,
}

impl Row {
    pub fn not_run(subcase: &'static str) -> Self {
        Self {
            subcase,
            verdict: "not_run",
            samples: 0,
            observation_points: 0,
            open_us: None,
            write_us: None,
            fsync_us: None,
            rename_us: None,
            read_us: None,
            publish_us: None,
            rtt_us: None,
            guest_read_us: None,
            guest_publish_us: None,
            errors: Vec::new(),
            notes: Vec::new(),
            unknown_reason: None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Sample {
    open_us: Option<u64>,
    write_us: Option<u64>,
    fsync_us: Option<u64>,
    rename_us: Option<u64>,
    read_us: Option<u64>,
    publish_us: Option<u64>,
    rtt_us: Option<u64>,
}

pub fn fold_verdicts(items: &[&str]) -> &'static str {
    let exec: Vec<&str> = items.iter().copied().filter(|v| *v != "not_run").collect();
    if items.contains(&"quarantine") {
        return "quarantine";
    }
    if exec.contains(&"failed") {
        return "failed";
    }
    if exec.contains(&"unknown") {
        return "unknown";
    }
    if !exec.is_empty() && exec.iter().all(|v| *v == "not-ack") {
        return "not-ack";
    }
    if exec.contains(&"block") {
        return "block";
    }
    if !exec.is_empty() && exec.iter().all(|v| *v == "ok") {
        return "ok";
    }
    if exec.is_empty() {
        "not_run"
    } else {
        "unknown"
    }
}

fn max_of(samples: &[Sample], pick: fn(&Sample) -> Option<u64>) -> Option<u64> {
    samples.iter().filter_map(pick).max()
}

fn points(samples: &[Sample], extra: u64) -> u64 {
    let mut n = extra;
    for s in samples {
        for v in [
            s.open_us,
            s.write_us,
            s.fsync_us,
            s.rename_us,
            s.read_us,
            s.publish_us,
            s.rtt_us,
        ] {
            if v.is_some() {
                n += 1;
            }
        }
    }
    n
}

fn notes_for(samples: &[Sample]) -> Vec<&'static str> {
    let hot = samples.iter().any(|s| {
        [
            s.open_us,
            s.write_us,
            s.fsync_us,
            s.rename_us,
            s.read_us,
            s.publish_us,
        ]
        .into_iter()
        .flatten()
        .any(|v| v >= SLICE_NOTE_US)
    });
    if hot {
        vec!["slice-over-100ms"]
    } else {
        Vec::new()
    }
}

#[derive(Clone)]
struct GuestC1 {
    samples: u64,
    torn: u64,
    lost: u64,
    digest: String,
    max_read_us: u64,
    max_publish_us: u64,
}

fn judge_c1(samples: &[Sample], guest: Option<&GuestC1>, torn: bool, expect: &str) -> Row {
    let mut row = Row::not_run("rename-latency");
    row.samples = samples.len() as u64;
    row.observation_points = points(samples, u64::from(guest.is_some()));
    row.open_us = max_of(samples, |s| s.open_us);
    row.write_us = max_of(samples, |s| s.write_us);
    row.fsync_us = max_of(samples, |s| s.fsync_us);
    row.rename_us = max_of(samples, |s| s.rename_us);
    row.read_us = max_of(samples, |s| s.read_us);
    row.publish_us = max_of(samples, |s| s.publish_us);
    row.rtt_us = max_of(samples, |s| s.rtt_us);
    row.notes = notes_for(samples);
    if let Some(g) = guest {
        row.guest_read_us = Some(g.max_read_us);
        row.guest_publish_us = Some(g.max_publish_us);
    }
    let guest_torn = guest.map(|g| g.torn > 0).unwrap_or(false);
    if torn || guest_torn {
        row.verdict = "failed";
        row.errors.push("torn");
        return row;
    }
    let Some(g) = guest else {
        row.verdict = "unknown";
        row.unknown_reason = Some("no-guest-summary");
        return row;
    };
    if g.lost > 0 || (g.samples == u64::from(C1_ITERS) && g.digest != expect) {
        row.verdict = "failed";
        row.errors.push(if g.lost > 0 { "lost" } else { "digest" });
        return row;
    }
    if g.samples != u64::from(C1_ITERS) || samples.len() != usize::from(C1_ITERS) * 2 {
        row.verdict = "unknown";
        row.unknown_reason = Some("incomplete");
        return row;
    }
    if samples
        .iter()
        .any(|s| s.rtt_us.unwrap_or(0) >= RTT_BLOCK_US)
    {
        row.verdict = "block";
        return row;
    }
    row.verdict = "ok";
    row
}

enum Wait {
    Ready(TimedRead),
    Torn,
    Unknown,
    Timeout,
}

fn wait_frame(
    dir: &File,
    dev: u64,
    fsid: Option<[i32; 2]>,
    name: &str,
    tag: u8,
    direction: u8,
    seq: u8,
) -> Wait {
    let deadline = Instant::now() + Duration::from_millis(VISIBILITY_MS);
    loop {
        match slot::read_slot(dir, dev, fsid, name, FRAME_LEN) {
            Ok(read) => match class_frame(&read.bytes, tag, direction, seq) {
                Class::Exact => return Wait::Ready(read),
                Class::Old => {}
                Class::Torn => return Wait::Torn,
            },
            // The next sequence is published by rename over this name. An open
            // of the previous inode then sees nlink 0. That read is not a frame.
            Err(SlotError::Absent) | Err(SlotError::Replaced) => {}
            Err(err) if err.integrity() => return Wait::Torn,
            Err(_) => return Wait::Unknown,
        }
        if Instant::now() >= deadline {
            return Wait::Timeout;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

enum Class {
    Exact,
    Old,
    Torn,
}

fn class_frame(bytes: &[u8], tag: u8, direction: u8, seq: u8) -> Class {
    if pattern::is_spec_frame(bytes) && bytes[6] == tag && bytes[7] == direction && bytes[8] == seq
    {
        Class::Exact
    } else if pattern::is_spec_frame(bytes)
        && bytes[6] == tag
        && bytes[7] == direction
        && bytes[8] < seq
    {
        Class::Old
    } else {
        Class::Torn
    }
}

fn open_sub(mailbox: &Path, name: &str) -> Result<Held, &'static str> {
    osutil::verify_private_leaf(&mailbox.join(name))
}

pub fn guest_c1(mailbox: &Path) -> Result<(), &'static str> {
    let host = open_sub(mailbox, "host-write")?;
    let guest = open_sub(mailbox, "guest-write")?;
    let mb = osutil::verify_private_leaf(mailbox)?;
    let fsid = mb.fsid.ok_or("fsid-unknown")?;
    let mut torn = 0u64;
    let mut lost = 0u64;
    let mut samples = 0u64;
    let mut max_read = 0u64;
    let mut max_pub = 0u64;
    let mut got_frames = Vec::new();
    let mut cursor = Cursor::default();
    for seq in 1..=C1_ITERS {
        let started = Instant::now();
        match wait_frame(&host.file, mb.dev, Some(fsid), "c1-frame", 1, 1, seq) {
            Wait::Ready(read) => {
                max_read = max_read.max(slot::us(started.elapsed()));
                got_frames.push(read.bytes);
                samples += 1;
                let body = frame(1, 2, seq);
                let t0 = Instant::now();
                publish::publish(
                    &guest.file,
                    &guest.file,
                    guest.dev,
                    Some(fsid),
                    "c1-frame.tmp",
                    "c1-frame",
                    &body,
                    &mut cursor,
                    true,
                )
                .map_err(|_| "publish")?;
                max_pub = max_pub.max(slot::us(t0.elapsed()));
            }
            Wait::Torn => {
                torn += 1;
                break;
            }
            Wait::Timeout => {
                lost += 1;
                break;
            }
            Wait::Unknown => return Err("read"),
        }
    }
    let digest = pattern::concat_digest(
        &got_frames
            .iter()
            .map(|b| {
                let mut a = [0u8; FRAME_LEN];
                a.copy_from_slice(b);
                a
            })
            .collect::<Vec<_>>(),
    );
    let mut buf = Buf::new(SLOT_CAP);
    buf.push_raw(b"{");
    buf.key("v");
    buf.u64(1);
    buf.comma();
    buf.key("op");
    buf.code("c1-summary");
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
    buf.string(&digest);
    buf.comma();
    buf.key("max_read_us");
    buf.u64(max_read);
    buf.comma();
    buf.key("max_publish_us");
    buf.u64(max_pub);
    buf.push_raw(b"}");
    let bytes = buf.finish().map_err(|_| "summary")?;
    let mut summary = Cursor::default();
    publish::publish(
        &mb.file,
        &mb.file,
        mb.dev,
        Some(fsid),
        "probe-result.tmp",
        "probe-result",
        &bytes,
        &mut summary,
        true,
    )
    .map_err(|_| "summary")?;
    Ok(())
}

pub fn drive_c1(mailbox: &Path, staging: &Path) -> Row {
    let mb = match osutil::verify_private_leaf(mailbox) {
        Ok(h) => h,
        Err(_) => return unknown_row("rename-latency", "private-root"),
    };
    let Some(fsid) = mb.fsid else {
        return unknown_row("rename-latency", "fsid-unknown");
    };
    let host = match open_sub(mailbox, "host-write") {
        Ok(h) => h,
        Err(err) => return unknown_row("rename-latency", err),
    };
    let guest = match open_sub(mailbox, "guest-write") {
        Ok(h) => h,
        Err(err) => return unknown_row("rename-latency", err),
    };
    let stage = match osutil::verify_private_leaf(staging) {
        Ok(h) => h,
        Err(err) => return unknown_row("rename-latency", err),
    };
    let mut samples = Vec::new();
    let mut torn = false;
    let mut cursor = Cursor::default();
    for seq in 1..=C1_ITERS {
        let body = frame(1, 1, seq);
        let started = Instant::now();
        let times = match publish::publish(
            &stage.file,
            &host.file,
            mb.dev,
            Some(fsid),
            "c1-frame.tmp",
            "c1-frame",
            &body,
            &mut cursor,
            true,
        ) {
            Ok(t) => t,
            Err(err) if err.integrity() => {
                torn = true;
                break;
            }
            Err(_) => return unknown_row("rename-latency", "publish"),
        };
        match wait_frame(&guest.file, mb.dev, Some(fsid), "c1-frame", 1, 2, seq) {
            Wait::Ready(read) => {
                let rtt = slot::us(started.elapsed());
                samples.push(dir1_sample(&times, rtt));
                samples.push(Sample {
                    open_us: Some(read.open_us),
                    write_us: None,
                    fsync_us: None,
                    rename_us: None,
                    read_us: Some(read.read_us),
                    publish_us: None,
                    rtt_us: None,
                });
            }
            Wait::Torn => {
                torn = true;
                break;
            }
            Wait::Timeout => return unknown_row("rename-latency", "visibility"),
            Wait::Unknown => return unknown_row("rename-latency", "read"),
        }
    }
    let guest_summary = match wait_frame_raw(&mb.file, mb.dev, Some(fsid), "probe-result") {
        Wait::Ready(read) => parse_c1(&read.bytes).ok(),
        _ => None,
    };
    let expect = expected_c1_digest();
    let mut row = judge_c1(&samples, guest_summary.as_ref(), torn, &expect);
    if torn {
        row.verdict = "failed";
        row.unknown_reason = None;
    }
    row
}

fn dir1_sample(times: &PubTimes, rtt: u64) -> Sample {
    Sample {
        open_us: Some(times.open_us),
        write_us: Some(times.write_us),
        fsync_us: Some(times.fsync_us),
        rename_us: Some(times.rename_us),
        read_us: None,
        publish_us: Some(publish::sum_pub(times)),
        rtt_us: Some(rtt),
    }
}

fn wait_frame_raw(dir: &File, dev: u64, fsid: Option<[i32; 2]>, name: &str) -> Wait {
    let deadline = Instant::now() + Duration::from_millis(VISIBILITY_MS);
    loop {
        match slot::read_slot(dir, dev, fsid, name, SLOT_CAP) {
            Ok(read) => return Wait::Ready(read),
            Err(SlotError::Absent) => {}
            Err(err) if err.integrity() => return Wait::Torn,
            Err(_) => return Wait::Unknown,
        }
        if Instant::now() >= deadline {
            return Wait::Timeout;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn expected_c1_digest() -> String {
    let frames: Vec<[u8; FRAME_LEN]> = (1..=C1_ITERS).map(|seq| frame(1, 1, seq)).collect();
    pattern::concat_digest(&frames)
}

fn parse_c1(bytes: &[u8]) -> Result<GuestC1, &'static str> {
    let text = std::str::from_utf8(bytes).map_err(|_| "summary")?;
    let val = jsonutil::parse(text, &Limits::result_slot())?;
    jsonutil::require_keys(
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
    if val.get("v").and_then(Val::int) != Some(1)
        || val.get("op").and_then(Val::str) != Some("c1-summary")
    {
        return Err("summary");
    }
    let digest = val.get("digest").and_then(Val::str).ok_or("summary")?;
    if !is_hex64(digest) {
        return Err("summary");
    }
    Ok(GuestC1 {
        samples: val.get("samples").and_then(Val::int).ok_or("summary")?,
        torn: val.get("torn").and_then(Val::int).ok_or("summary")?,
        lost: val.get("lost").and_then(Val::int).ok_or("summary")?,
        digest: digest.to_string(),
        max_read_us: val.get("max_read_us").and_then(Val::int).ok_or("summary")?,
        max_publish_us: val
            .get("max_publish_us")
            .and_then(Val::int)
            .ok_or("summary")?,
    })
}

pub(crate) fn unknown_row(sub: &'static str, reason: &'static str) -> Row {
    let mut row = Row::not_run(sub);
    row.verdict = "unknown";
    row.unknown_reason = Some(reason);
    row
}

pub fn guest_c2(mailbox: &Path, boundary: &str) -> Result<(), &'static str> {
    if !matches!(boundary, "file-sync" | "dir-sync" | "rename-boundary") {
        return Err("subcase");
    }
    let mb = osutil::verify_private_leaf(mailbox)?;
    let fsid = mb.fsid.ok_or("fsid-unknown")?;
    let mut cursor = Cursor::default();
    for seq in 1..=2 {
        let body = frame(2, 1, seq);
        publish::publish(
            &mb.file,
            &mb.file,
            mb.dev,
            Some(fsid),
            "c2-part",
            "c2-frame",
            &body,
            &mut cursor,
            true,
        )
        .map_err(|_| "publish")?;
    }
    match boundary {
        "file-sync" => {
            write_part(&mb.file, &frame(2, 1, 3))?;
            write_progress(&mb, fsid, boundary, 2, 0, false)?;
        }
        "dir-sync" => {
            let errno = rename_seq3(&mb.file, mb.dev, fsid, &mut cursor, true)?;
            write_progress(&mb, fsid, boundary, 3, errno, errno == 0)?;
        }
        "rename-boundary" => {
            write_progress(&mb, fsid, boundary, 3, 0, false)?;
            rename_seq3(&mb.file, mb.dev, fsid, &mut cursor, false)?;
        }
        _ => return Err("subcase"),
    }
    Err("fault-exit")
}

fn write_part(dir: &File, bytes: &[u8]) -> Result<(), &'static str> {
    let mut file = osutil::create_0600_at(dir, "c2-part").map_err(|_| "part")?;
    osutil::write_all_fd(&mut file, bytes).map_err(|_| "part")?;
    osutil::fsync_fd(&file).map_err(|_| "part")?;
    Ok(())
}

fn rename_seq3(
    dir: &File,
    dev: u64,
    fsid: [i32; 2],
    cursor: &mut Cursor,
    sync_dir: bool,
) -> Result<u64, &'static str> {
    let body = frame(2, 1, 3);
    publish::publish(
        dir,
        dir,
        dev,
        Some(fsid),
        "c2-part",
        "c2-frame",
        &body,
        cursor,
        false,
    )
    .map_err(|_| "publish")?;
    if sync_dir {
        match osutil::fsync_fd(dir) {
            Ok(()) => Ok(0),
            Err(err) => Ok(err.raw_os_error().unwrap_or(1) as u64),
        }
    } else {
        Ok(0)
    }
}

fn write_progress(
    mb: &Held,
    fsid: [i32; 2],
    boundary: &str,
    completed: u64,
    errno: u64,
    synced: bool,
) -> Result<(), &'static str> {
    let mut buf = Buf::new(SLOT_CAP);
    buf.push_raw(b"{");
    buf.key("v");
    buf.u64(1);
    buf.comma();
    buf.key("op");
    buf.code("c2-progress");
    buf.comma();
    buf.key("boundary");
    buf.code(boundary);
    buf.comma();
    buf.key("completed");
    buf.u64(completed);
    buf.comma();
    buf.key("errno");
    buf.u64(errno);
    buf.comma();
    buf.key("dir_synced");
    buf.u64(u64::from(synced));
    buf.push_raw(b"}");
    let bytes = buf.finish().map_err(|_| "progress")?;
    let mut cursor = Cursor::default();
    publish::publish(
        &mb.file,
        &mb.file,
        mb.dev,
        Some(fsid),
        "c2-progress.tmp",
        "c2-progress",
        &bytes,
        &mut cursor,
        true,
    )
    .map_err(|_| "progress")?;
    Ok(())
}

struct C2Obs {
    seq: Option<u8>,
    part: bool,
    completed: Option<u64>,
    errno: Option<u64>,
    synced: Option<bool>,
    boundary: Option<String>,
    ack: bool,
    bad: bool,
    unreadable: bool,
    saw: u64,
}

pub fn observe_c2(mailbox: &Path, sub: &str, ssh_status: Option<i32>) -> Row {
    let mb = match osutil::verify_private_leaf(mailbox) {
        Ok(h) => h,
        Err(_) => return unknown_row(leak_sub(sub), "private-root"),
    };
    let Some(fsid) = mb.fsid else {
        return unknown_row(leak_sub(sub), "fsid-unknown");
    };
    let obs = read_c2(&mb.file, mb.dev, fsid);
    judge_c2(leak_sub(sub), &obs, ssh_status)
}

fn leak_sub(sub: &str) -> &'static str {
    match sub {
        "file-sync" => "file-sync",
        "dir-sync" => "dir-sync",
        "rename-boundary" => "rename-boundary",
        "subjects" => "subjects",
        "rename-latency" => "rename-latency",
        _ => "subcase",
    }
}

fn read_c2(dir: &File, dev: u64, fsid: [i32; 2]) -> C2Obs {
    let mut obs = C2Obs {
        seq: None,
        part: false,
        completed: None,
        errno: None,
        synced: None,
        boundary: None,
        ack: false,
        bad: false,
        unreadable: false,
        saw: 0,
    };
    match slot::read_slot(dir, dev, Some(fsid), "c2-frame", FRAME_LEN) {
        Ok(read) => {
            obs.saw += 1;
            if pattern::is_spec_frame(&read.bytes) && read.bytes[6] == 2 && read.bytes[7] == 1 {
                obs.seq = Some(read.bytes[8]);
            } else {
                obs.bad = true;
            }
        }
        Err(SlotError::Absent) => {}
        Err(err) if err.integrity() => obs.bad = true,
        Err(_) => obs.unreadable = true,
    }
    match slot::read_slot(dir, dev, Some(fsid), "c2-part", FRAME_LEN) {
        Ok(_) => {
            obs.part = true;
            obs.saw += 1;
        }
        Err(SlotError::Absent) => {}
        Err(err) if err.integrity() => obs.bad = true,
        Err(_) => obs.unreadable = true,
    }
    match slot::read_slot(dir, dev, Some(fsid), "c2-progress", SLOT_CAP) {
        Ok(read) => {
            obs.saw += 1;
            if parse_progress(&read.bytes, &mut obs).is_err() {
                obs.unreadable = true;
            }
        }
        Err(SlotError::Absent) => obs.unreadable = true,
        Err(err) if err.integrity() => obs.bad = true,
        Err(_) => obs.unreadable = true,
    }
    match slot::read_slot(dir, dev, Some(fsid), "probe-result", SLOT_CAP) {
        Ok(_) => obs.ack = true,
        Err(SlotError::Absent) => {}
        Err(err) if err.integrity() => obs.bad = true,
        Err(_) => obs.unreadable = true,
    }
    obs
}

fn parse_progress(bytes: &[u8], obs: &mut C2Obs) -> Result<(), &'static str> {
    let text = std::str::from_utf8(bytes).map_err(|_| "progress")?;
    let val = jsonutil::parse(text, &Limits::result_slot())?;
    jsonutil::require_keys(
        &val,
        &["v", "op", "boundary", "completed", "errno", "dir_synced"],
    )?;
    if val.get("op").and_then(Val::str) != Some("c2-progress") {
        return Err("progress");
    }
    obs.boundary = Some(
        val.get("boundary")
            .and_then(Val::str)
            .ok_or("progress")?
            .to_string(),
    );
    obs.completed = val.get("completed").and_then(Val::int);
    obs.errno = val.get("errno").and_then(Val::int);
    obs.synced = Some(val.get("dir_synced").and_then(Val::int).ok_or("progress")? == 1);
    Ok(())
}

fn judge_c2(sub: &'static str, obs: &C2Obs, ssh_status: Option<i32>) -> Row {
    let mut row = Row::not_run(sub);
    row.samples = obs.completed.unwrap_or(0);
    row.observation_points = obs.saw;
    if obs.bad || obs.ack {
        row.verdict = "failed";
        row.errors
            .push(if obs.ack { "false-ack" } else { "protocol" });
        return row;
    }
    if obs.unreadable || obs.boundary.as_deref() != Some(sub) {
        row.verdict = "unknown";
        row.unknown_reason = Some(if obs.unreadable {
            "unreadable"
        } else {
            "fault-location"
        });
        return row;
    }
    if ssh_status != Some(86) {
        row.verdict = "unknown";
        row.unknown_reason = Some("ssh-status");
        return row;
    }
    if sub == "dir-sync" && obs.errno != Some(0) {
        row.verdict = "unknown";
        row.unknown_reason = Some("sync-error");
        return row;
    }
    let ok = match sub {
        "file-sync" => {
            obs.part && obs.seq == Some(2) && obs.completed == Some(2) && obs.synced == Some(false)
        }
        "dir-sync" => {
            !obs.part && obs.seq == Some(3) && obs.completed == Some(3) && obs.synced == Some(true)
        }
        "rename-boundary" => {
            !obs.part && obs.seq == Some(3) && obs.completed == Some(3) && obs.synced == Some(false)
        }
        _ => false,
    };
    if ok {
        row.verdict = "not-ack";
    } else {
        row.verdict = "unknown";
        row.unknown_reason = Some("fault-location");
    }
    row
}

pub fn judge_c8(
    non_admin: u64,
    ops: u64,
    created: bool,
    elevated: bool,
) -> (&'static str, Option<&'static str>) {
    if ops != 0 || created || elevated {
        ("failed", Some("subject-policy"))
    } else if non_admin <= 1 {
        ("unknown", Some("single-principal"))
    } else {
        ("unknown", Some("mapping-not-measured"))
    }
}

pub fn non_admin_count(users: &str, admins: &str) -> Result<usize, &'static str> {
    let admin_names = parse_admins(admins)?;
    let mut n = 0usize;
    for line in users.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let name = parts.next().ok_or("dscl")?;
        let uid: u32 = parts.next().ok_or("dscl")?.parse().map_err(|_| "dscl")?;
        if parts.next().is_some() {
            return Err("dscl");
        }
        if uid >= 500
            && name != "root"
            && name != "daemon"
            && !name.starts_with('_')
            && !admin_names.iter().any(|a| a == name)
        {
            n += 1;
        }
    }
    Ok(n)
}

fn parse_admins(text: &str) -> Result<Vec<String>, &'static str> {
    let line = text
        .lines()
        .find(|l| l.contains("GroupMembership"))
        .ok_or("dscl")?;
    Ok(line
        .split_whitespace()
        .filter(|tok| !tok.contains(':'))
        .map(|s| s.to_string())
        .collect())
}

pub fn guest_c8(runner: &mut dyn Runner, mailbox: &Path) -> Result<(), &'static str> {
    let users = run_text(runner, &crate::argv::dscl_users())?;
    let admins = run_text(runner, &crate::argv::dscl_admin_group())?;
    let n = non_admin_count(&users, &admins)? as u64;
    let (verdict, reason) = judge_c8(n, 0, false, false);
    let _ = (verdict, reason);
    let mb = osutil::verify_private_leaf(mailbox)?;
    let fsid = mb.fsid.ok_or("fsid-unknown")?;
    let mut buf = Buf::new(SLOT_CAP);
    buf.push_raw(b"{");
    buf.key("v");
    buf.u64(1);
    buf.comma();
    buf.key("op");
    buf.code("c8");
    buf.comma();
    buf.key("non_admin");
    buf.u64(n);
    buf.comma();
    buf.key("ops");
    buf.u64(0);
    buf.comma();
    buf.key("created_account");
    buf.bool(false);
    buf.comma();
    buf.key("elevated");
    buf.bool(false);
    buf.push_raw(b"}");
    let bytes = buf.finish().map_err(|_| "c8")?;
    let mut cursor = Cursor::default();
    publish::publish(
        &mb.file,
        &mb.file,
        mb.dev,
        Some(fsid),
        "probe-result.tmp",
        "probe-result",
        &bytes,
        &mut cursor,
        true,
    )
    .map_err(|_| "c8")?;
    Ok(())
}

fn run_text(runner: &mut dyn Runner, argv: &[String]) -> Result<String, &'static str> {
    let cmd = host_cmd(
        argv.to_vec(),
        Duration::from_millis(crate::budget::MS_QUERY),
        Vec::new(),
        None,
        false,
    )?;
    let out = runner.run(&cmd);
    let text = crate::runner::trusted_text(&out)?;
    Ok(text.to_string())
}

pub fn observe_c8(mailbox: &Path) -> Row {
    let mb = match osutil::verify_private_leaf(mailbox) {
        Ok(h) => h,
        Err(_) => return unknown_row("subjects", "private-root"),
    };
    let Some(fsid) = mb.fsid else {
        return unknown_row("subjects", "fsid-unknown");
    };
    let read = match slot::read_slot(&mb.file, mb.dev, Some(fsid), "probe-result", SLOT_CAP) {
        Ok(read) => read,
        Err(SlotError::Absent) => return unknown_row("subjects", "no-slot"),
        Err(err) if err.integrity() => {
            let mut row = unknown_row("subjects", "protocol");
            row.verdict = "failed";
            row.errors.push("protocol");
            return row;
        }
        Err(_) => return unknown_row("subjects", "unreadable"),
    };
    let text = match std::str::from_utf8(&read.bytes) {
        Ok(t) => t,
        Err(_) => return unknown_row("subjects", "unreadable"),
    };
    let val = match jsonutil::parse(text, &Limits::result_slot()) {
        Ok(v) => v,
        Err(_) => return unknown_row("subjects", "unreadable"),
    };
    if jsonutil::require_keys(
        &val,
        &["v", "op", "non_admin", "ops", "created_account", "elevated"],
    )
    .is_err()
        || val.get("op").and_then(Val::str) != Some("c8")
    {
        return unknown_row("subjects", "unreadable");
    }
    let n = val.get("non_admin").and_then(Val::int).unwrap_or(0);
    let ops = val.get("ops").and_then(Val::int).unwrap_or(1);
    let created = val
        .get("created_account")
        .and_then(Val::bool)
        .unwrap_or(true);
    let elevated = val.get("elevated").and_then(Val::bool).unwrap_or(true);
    let (verdict, reason) = judge_c8(n, ops, created, elevated);
    let mut row = Row::not_run("subjects");
    row.verdict = verdict;
    row.unknown_reason = reason;
    row.samples = 0;
    row.observation_points = 1;
    row
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch(tag: &str) -> std::path::PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let home = std::env::var("HOME").unwrap();
        let p =
            std::path::PathBuf::from(format!("{home}/.s11m2a-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir(&p).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
        p
    }

    fn sample_pair() -> Sample {
        Sample {
            open_us: Some(10),
            write_us: Some(10),
            fsync_us: Some(10),
            rename_us: Some(10),
            read_us: None,
            publish_us: Some(40),
            rtt_us: Some(20),
        }
    }

    #[test]
    fn c1_rules_and_aggregate() {
        let mut samples = Vec::new();
        for _ in 0..8 {
            samples.push(sample_pair());
            samples.push(Sample {
                open_us: Some(5),
                read_us: Some(5),
                ..sample_pair()
            });
        }
        samples.iter_mut().skip(1).step_by(2).for_each(|s| {
            s.write_us = None;
            s.fsync_us = None;
            s.rename_us = None;
            s.publish_us = None;
            s.rtt_us = None;
        });
        let guest = GuestC1 {
            samples: 8,
            torn: 0,
            lost: 0,
            digest: expected_c1_digest(),
            max_read_us: 15,
            max_publish_us: 30,
        };
        assert_eq!(
            judge_c1(&samples, None, false, &guest.digest).verdict,
            "unknown"
        );
        assert_eq!(
            judge_c1(&samples, None, false, &guest.digest).unknown_reason,
            Some("no-guest-summary")
        );
        assert_eq!(
            judge_c1(&samples, Some(&guest), true, &guest.digest).verdict,
            "failed"
        );
        let mut bad = guest.clone();
        bad.torn = 1;
        assert_eq!(
            judge_c1(&samples, Some(&bad), false, &guest.digest).verdict,
            "failed"
        );
        let mut slow = samples.clone();
        slow[0].rtt_us = Some(RTT_BLOCK_US);
        assert_eq!(
            judge_c1(&slow, Some(&guest), false, &guest.digest).verdict,
            "block"
        );
        assert_eq!(
            judge_c1(&samples, Some(&guest), false, &guest.digest).verdict,
            "ok"
        );
        assert_eq!(fold_verdicts(&["not-ack", "not-ack", "not-ack"]), "not-ack");
        assert_eq!(fold_verdicts(&["not-ack", "unknown"]), "unknown");
        assert_eq!(fold_verdicts(&["not-ack", "not_run"]), "not-ack");
        assert_eq!(fold_verdicts(&["failed", "unknown"]), "failed");
        assert_eq!(fold_verdicts(&["quarantine", "failed"]), "quarantine");
        assert_eq!(fold_verdicts(&["not_run", "not_run"]), "not_run");
        assert_eq!(fold_verdicts(&["ok"]), "ok");
    }

    #[test]
    fn torn_frame_fails_without_waiting_out_the_window() {
        let root = scratch("torn");
        let held = osutil::verify_private_leaf(&root).unwrap();
        let mut f = osutil::create_0600_at(&held.file, "c1-frame").unwrap();
        osutil::write_all_fd(&mut f, b"mixed-bytes").unwrap();
        drop(f);
        let started = Instant::now();
        let got = wait_frame(&held.file, held.dev, held.fsid, "c1-frame", 1, 1, 1);
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(matches!(got, Wait::Torn));
        let _ = osutil::unlink_at(&held.file, "c1-frame");
        let _ = std::fs::remove_dir(&root);
    }

    #[test]
    fn local_c1_exchange_reports_only_measured_times() {
        let root = scratch("c1");
        let mb = root.join("mailbox");
        std::fs::create_dir(&mb).unwrap();
        std::fs::set_permissions(&mb, std::fs::Permissions::from_mode(0o700)).unwrap();
        for name in ["host-write", "guest-write"] {
            std::fs::create_dir(mb.join(name)).unwrap();
            std::fs::set_permissions(mb.join(name), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let stage = root.join("stage");
        std::fs::create_dir(&stage).unwrap();
        std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mb2 = mb.clone();
        let guest = thread::spawn(move || guest_c1(&mb2));
        let row = drive_c1(&mb, &stage);
        guest.join().unwrap().unwrap();
        assert_eq!(row.verdict, "ok", "{:?}", row.unknown_reason);
        assert!(row.read_us.is_some());
        assert!(row.guest_read_us.is_some());
        assert_eq!(row.samples, 16);
        let _ = std::fs::remove_file(mb.join("probe-result"));
        let _ = std::fs::remove_file(mb.join("host-write/c1-frame"));
        let _ = std::fs::remove_file(mb.join("guest-write/c1-frame"));
        for name in ["host-write", "guest-write"] {
            let _ = std::fs::remove_dir(mb.join(name));
        }
        let _ = std::fs::remove_dir(&mb);
        let _ = std::fs::remove_dir(&stage);
        let _ = std::fs::remove_dir(&root);
    }

    #[test]
    fn c2_file_sync_counts_two_and_c8_does_not_invent_a_subject() {
        let mb = scratch("c2");
        let err = guest_c2(&mb, "file-sync").unwrap_err();
        assert_eq!(err, "fault-exit");
        let row = observe_c2(&mb, "file-sync", Some(86));
        assert_eq!(row.verdict, "not-ack", "{:?}", row.unknown_reason);
        assert_eq!(row.samples, 2);
        assert!(mb.join("c2-part").is_file());
        let other = scratch("c2b");
        assert_eq!(guest_c2(&other, "dir-sync").unwrap_err(), "fault-exit");
        assert_eq!(observe_c2(&other, "dir-sync", Some(86)).verdict, "not-ack");
        assert_eq!(observe_c2(&other, "dir-sync", Some(1)).verdict, "unknown");
        let third = scratch("c2c");
        assert_eq!(
            guest_c2(&third, "rename-boundary").unwrap_err(),
            "fault-exit"
        );
        let row = observe_c2(&third, "rename-boundary", Some(86));
        assert_eq!(row.verdict, "not-ack", "{:?}", row.unknown_reason);
        assert_eq!(row.samples, 3);
        assert_eq!(judge_c8(1, 0, false, false).1, Some("single-principal"));
        assert_eq!(judge_c8(2, 0, false, false).1, Some("mapping-not-measured"));
        assert_eq!(judge_c8(2, 1, false, false).0, "failed");
        assert_eq!(judge_c8(1, 0, true, false).0, "failed");
        let users = "eval 501\nother 503\nroot 0\n_svc 210\ndaemon 1\nadmin 502\n";
        let admins = "GroupMembership: admin root\n";
        assert_eq!(non_admin_count(users, admins).unwrap(), 2);
        let one = "eval 501\nroot 0\n";
        assert_eq!(non_admin_count(one, "GroupMembership: admin\n").unwrap(), 1);
        let c8 = scratch("c8");
        let mut fake = crate::runner::Fake::new(vec![
            (
                "UniqueID",
                crate::runner::CmdOut::ok_text("eval 501\nroot 0\n"),
            ),
            (
                "GroupMembership",
                crate::runner::CmdOut::ok_text("GroupMembership: admin\n"),
            ),
        ]);
        guest_c8(&mut fake, &c8).unwrap();
        let row = observe_c8(&c8);
        assert_eq!(row.verdict, "unknown");
        assert_eq!(row.unknown_reason, Some("single-principal"));
        assert!(!c8.join("work").exists());
        assert_eq!(row.samples, 0);
    }
}

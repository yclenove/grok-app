use crate::frame::{self, FRAME_LEN};
use crate::io::{self, Dir, RdErr, Times};
use crate::sha256::hex_digest;
use std::io::{self as stdio, ErrorKind};
use std::os::unix::io::AsRawFd;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const C1_KEYS: &[&str] =
    &["v", "op", "role", "verdict", "reason", "n", "torn", "lost", "integrity", "latency", "vis_us", "rtt_us", "slice_us", "w_slice_us", "open_us", "write_us", "fsync_us", "rename_us", "dir_sync_us", "read_us", "peer", "digest"];
const C2_KEYS: &[&str] = &["v", "op", "role", "boundary", "verdict", "reason", "planned", "attempted", "renamed", "visible", "part", "dir_synced"];
const C8_KEYS: &[&str] = &["v", "op", "role", "verdict", "reason", "n", "ops", "created", "elevated"];

struct Opt {
    cmd: &'static str,
    boundary: &'static str,
    dir: String,
    role: &'static str,
    count: u8,
    end: Instant,
}
struct Clock {
    open_us: Option<u64>,
    write_us: Option<u64>,
    fsync_us: Option<u64>,
    rename_us: Option<u64>,
    dir_sync_us: Option<u64>,
    read_us: Option<u64>,
    slice_us: Option<u64>,
}
impl Clock {
    fn new() -> Self {
        Self { open_us: None, write_us: None, fsync_us: None, rename_us: None, dir_sync_us: None, read_us: None, slice_us: None }
    }
    fn note(&mut self, t: &Times) {
        bump(&mut self.open_us, t.open_us);
        bump(&mut self.write_us, t.write_us);
        bump(&mut self.fsync_us, t.fsync_us);
        bump(&mut self.rename_us, t.rename_us);
        if let Some(v) = t.dir_sync_us {
            bump(&mut self.dir_sync_us, v);
        }
        bump(&mut self.slice_us, t.slice_us);
    }
    fn note_read(&mut self, open_us: u64, read_us: u64, slice_us: u64) {
        bump(&mut self.open_us, open_us);
        bump(&mut self.read_us, read_us);
        bump(&mut self.slice_us, slice_us);
    }
}
pub fn execute(args: &[String]) -> Result<i32, &'static str> {
    let opt = parse(args)?;
    if io::past(opt.end) {
        return Ok(deadline_only(&opt));
    }
    let code = match opt.cmd {
        "writer" => c1_writer(&opt),
        "reader" => c1_reader(&opt),
        "c2" if opt.role == "guest" => c2_guest(&opt),
        "c2" => c2_host(&opt),
        "c8" if opt.role == "guest" => c8_guest(&opt),
        "c8" => c8_host(&opt),
        _ => return Err("usage"),
    };
    Ok(code)
}
fn parse(args: &[String]) -> Result<Opt, &'static str> {
    if args.is_empty() {
        return Err("usage");
    }
    let (cmd, boundary, rest) = match args[0].as_str() {
        "writer" => ("writer", "-", &args[1..]),
        "reader" => ("reader", "-", &args[1..]),
        "c8" => ("c8", "-", &args[1..]),
        "c2" => {
            let boundary = match args.get(1).map(String::as_str).unwrap_or("") {
                "file-sync" => "file-sync",
                "dir-sync" => "dir-sync",
                "rename" => "rename",
                _ => return Err("usage"),
            };
            ("c2", boundary, &args[2..])
        }
        _ => return Err("usage"),
    };
    let mut dir = None;
    let mut role = None;
    let mut count = None;
    let mut timeout = None;
    let mut i = 0;
    while i < rest.len() {
        let val = rest.get(i + 1).ok_or("usage")?;
        match rest[i].as_str() {
            "--dir" => dir = Some(val.clone()),
            "--role" => {
                role = Some(match val.as_str() {
                    "host" => "host",
                    "guest" => "guest",
                    _ => return Err("usage"),
                });
            }
            "--count" => count = Some(small(val, 1, 8)?),
            "--timeout" => timeout = Some(small(val, 0, 120)?),
            _ => return Err("usage"),
        }
        i += 2;
    }
    let timeout = u64::from(timeout.ok_or("usage")?);
    Ok(Opt { cmd, boundary, dir: dir.ok_or("usage")?, role: role.ok_or("usage")?, count: count.ok_or("usage")?, end: Instant::now() + Duration::from_secs(timeout) })
}
fn small(val: &str, lo: u8, hi: u8) -> Result<u8, &'static str> {
    if val.is_empty() || val.len() > 3 || !val.bytes().all(|b| b.is_ascii_digit()) {
        return Err("usage");
    }
    let n: u8 = val.parse().map_err(|_| "usage")?;
    if n < lo || n > hi {
        Err("usage")
    } else {
        Ok(n)
    }
}
fn num(v: Option<u64>) -> String {
    v.map_or_else(|| "-".to_string(), |n| n.to_string())
}
fn bump(slot: &mut Option<u64>, v: u64) {
    *slot = Some(slot.map_or(v, |old| old.max(v)));
}
fn numeric(v: &str) -> bool {
    !v.is_empty() && v.len() <= 7 && v.bytes().all(|b| b.is_ascii_digit())
}
fn decode<'a>(line: &'a str, keys: &[&str]) -> Option<Vec<(&'a str, &'a str)>> {
    if line.is_empty() || line.contains('\n') {
        return None;
    }
    let mut got = Vec::with_capacity(keys.len());
    for tok in line.split(' ') {
        let (key, val) = tok.split_once('=')?;
        if key.is_empty() || val.is_empty() || !keys.contains(&key) {
            return None;
        }
        if got.iter().any(|(have, _)| *have == key) {
            return None;
        }
        got.push((key, val));
    }
    if got.len() != keys.len() {
        return None;
    }
    Some(got)
}
fn rec_text(bytes: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(bytes).ok()?;
    let text = text.strip_suffix('\n').unwrap_or(text);
    if text.is_empty() || text.contains('\n') {
        None
    } else {
        Some(text)
    }
}
fn field<'a>(rec: &[(&'a str, &'a str)], key: &str) -> &'a str {
    rec.iter().find(|(k, _)| *k == key).map(|(_, v)| *v).unwrap_or("")
}
fn child_dir(root: &Dir, name: &str) -> Result<Dir, RdErr> {
    let dir = root.open_child_dir(name)?;
    if dir.dev() != root.dev() {
        return Err(RdErr::Replaced);
    }
    Ok(dir)
}
fn leaf(role: &str) -> &'static str {
    if role == "host" {
        "host-write"
    } else {
        "guest-write"
    }
}
fn direction(role: &str) -> u8 {
    if role == "host" {
        1
    } else {
        2
    }
}
fn peer_role(role: &str) -> &'static str {
    if role == "host" {
        "guest"
    } else {
        "host"
    }
}
fn c1_line(role: &str, verdict: &str, reason: &str, counts: (u64, u64), marks: (&str, &str, &str), clock: &Clock, tail: (u8, &str)) -> String {
    let (n, torn) = counts;
    let (integrity, latency, w_slice) = marks;
    let (peer, digest) = tail;
    format!(
        "v=1 op=c1 role={role} verdict={verdict} reason={reason} n={n} torn={torn} lost=- integrity={integrity} latency={latency} vis_us=- rtt_us=- slice_us={} w_slice_us={w_slice} open_us={} write_us={} fsync_us={} rename_us={} dir_sync_us={} read_us={} peer={peer} digest={digest}",
        num(clock.slice_us),
        num(clock.open_us),
        num(clock.write_us),
        num(clock.fsync_us),
        num(clock.rename_us),
        num(clock.dir_sync_us),
        num(clock.read_us)
    )
}
fn c2_line(role: &str, boundary: &str, verdict: &str, reason: &str, counts: [&str; 4], part: &str, dir_synced: &str) -> String {
    let [planned, attempted, renamed, visible] = counts;
    format!("v=1 op=c2 role={role} boundary={boundary} verdict={verdict} reason={reason} planned={planned} attempted={attempted} renamed={renamed} visible={visible} part={part} dir_synced={dir_synced}")
}
fn c8_line(role: &str, verdict: &str, reason: &str, n: &str) -> String {
    format!("v=1 op=c8 role={role} verdict={verdict} reason={reason} n={n} ops=0 created=0 elevated=0")
}
fn say(line: &str) {
    println!("{line}");
    let _ = stdio::Write::flush(&mut stdio::stdout());
}
fn store(dir: &Dir, stem: &str, line: &str, end: Option<Instant>) -> Result<(), RdErr> {
    if line.is_empty() || line.contains('\n') || line.len() > 4095 {
        return Err(RdErr::Size);
    }
    let mut bytes = Vec::with_capacity(line.len() + 1);
    bytes.extend_from_slice(line.as_bytes());
    bytes.push(b'\n');
    let tmp = format!("{stem}.tmp");
    let published = if let Some(end) = end { dir.publish(&tmp, stem, &bytes, true, end) } else { dir.publish_diag(&tmp, stem, &bytes, true) };
    published.map(|_| ())
}
fn finish(dir: &Dir, stem: &str, line: &str, verdict: &str, end: Instant, late: &str) -> i32 {
    let success = verdict == "ok" || verdict == "not-ack";
    if success && io::past(end) {
        let _ = store(dir, stem, late, None);
        say(late);
        return 1;
    }
    let already = io::past(end);
    let gate = if success { Some(end) } else { None };
    match store(dir, stem, line, gate) {
        Ok(()) if verdict == "failed" || !io::past(end) => {
            say(line);
            if success {
                0
            } else {
                1
            }
        }
        Ok(()) | Err(_) => {
            // A reason chosen after the original end stays. A store that itself crosses that end does not.
            let rewrite = if !already && io::past(end) { late } else { line };
            let _ = store(dir, stem, rewrite, None);
            say(rewrite);
            1
        }
    }
}
fn slot(opt: &Opt) -> String {
    format!("result-{}", opt.role)
}
fn c1_unknown(opt: &Opt, reason: &str, n: u64, torn: u64, integrity: &str, clock: &Clock) -> String {
    c1_line(opt.role, "unknown", reason, (n, torn), (integrity, "not_measured", "-"), clock, (0, "-"))
}
fn c2_unknown(opt: &Opt, reason: &str, planned: &str, attempted: &str, renamed: &str, visible: &str, part: &str) -> String {
    c2_line(opt.role, opt.boundary, "unknown", reason, [planned, attempted, renamed, visible], part, "-")
}
fn bare_unknown(opt: &Opt, reason: &str) -> String {
    match opt.cmd {
        "c2" => c2_unknown(opt, reason, &opt.count.to_string(), "-", "-", "-", "absent"),
        "c8" => c8_line(opt.role, "unknown", reason, "0"),
        _ => c1_unknown(opt, reason, 0, 0, "-", &Clock::new()),
    }
}
fn late_line(opt: &Opt) -> String {
    bare_unknown(opt, "deadline")
}
fn unk(opt: &Opt, root: &Dir, reason: &str) -> i32 {
    let line = bare_unknown(opt, reason);
    let late = late_line(opt);
    finish(root, &slot(opt), &line, "unknown", opt.end, &late)
}
fn open_root(opt: &Opt) -> Result<Dir, i32> {
    match Dir::open_path(&opt.dir) {
        Ok(dir) => Ok(dir),
        Err(_) => {
            say(&bare_unknown(opt, "io"));
            Err(1)
        }
    }
}
fn deadline_only(opt: &Opt) -> i32 {
    let line = late_line(opt);
    if let Ok(dir) = Dir::open_path(&opt.dir) {
        return finish(&dir, &slot(opt), &line, "unknown", opt.end, &line);
    }
    say(&line);
    1
}
fn wait_more(end: Instant) -> bool {
    if io::past(end) {
        return false;
    }
    let remain = end.saturating_duration_since(Instant::now());
    if remain.is_zero() {
        return false;
    }
    thread::sleep(Duration::from_millis(20).min(remain));
    !io::past(end)
}
fn c1_done(root: &Dir, opt: &Opt, verdict: &str, reason: &str, body: (u64, u64, &str, &str), clock: &Clock, tail: (&str, u8, &str)) -> i32 {
    let (n, torn, integrity, latency) = body;
    let (w_slice, peer, digest) = tail;
    let line = c1_line(opt.role, verdict, reason, (n, torn), (integrity, latency, w_slice), clock, (peer, digest));
    let late = c1_unknown(opt, "deadline", n, torn, integrity, clock);
    finish(root, &slot(opt), &line, verdict, opt.end, &late)
}
fn c1_writer(opt: &Opt) -> i32 {
    let root = match open_root(opt) {
        Ok(dir) => dir,
        Err(code) => return code,
    };
    let sub = match child_dir(&root, leaf(opt.role)) {
        Ok(dir) => dir,
        Err(RdErr::Replaced) => return c1_done(&root, opt, "failed", "replaced", (0, 0, "bad", "not_measured"), &Clock::new(), ("-", 0, "-")),
        Err(_) => return unk(opt, &root, "io"),
    };
    let mut clock = Clock::new();
    let mut frames = Vec::with_capacity(opt.count as usize);
    for seq in 1..=opt.count {
        if io::past(opt.end) {
            return c1_done(&root, opt, "unknown", "deadline", (frames.len() as u64, 0, "-", "not_measured"), &clock, ("-", 0, "-"));
        }
        let bytes = frame::frame(1, direction(opt.role), seq);
        match sub.publish(&format!("f-{seq}.tmp"), &format!("f-{seq}"), &bytes, true, opt.end) {
            Ok(t) => {
                clock.note(&t);
                if io::past(opt.end) {
                    return c1_done(&root, opt, "unknown", "deadline", (frames.len() as u64, 0, "-", "not_measured"), &clock, ("-", 0, "-"));
                }
            }
            Err(RdErr::Deadline) => return c1_done(&root, opt, "unknown", "deadline", (frames.len() as u64, 0, "-", "not_measured"), &clock, ("-", 0, "-")),
            Err(RdErr::Replaced) => return c1_done(&root, opt, "failed", "replaced", (frames.len() as u64, 1, "bad", "not_measured"), &clock, ("-", 0, "-")),
            Err(_) => return c1_done(&root, opt, "unknown", "io", (frames.len() as u64, 0, "-", "not_measured"), &clock, ("-", 0, "-")),
        }
        frames.push(bytes);
    }
    let digest = digest_of(&frames);
    let line = c1_line(opt.role, "unknown", "partial-coverage", (u64::from(opt.count), 0), ("ok", "not_measured", "-"), &clock, (0, digest.as_str()));
    let late = c1_unknown(opt, "deadline", u64::from(opt.count), 0, "ok", &clock);
    if io::past(opt.end) {
        return finish(&root, &slot(opt), &late, "unknown", opt.end, &late);
    }
    if store(&root, &format!("summary-{}", opt.role), &line, Some(opt.end)).is_err() || io::past(opt.end) {
        return finish(&root, &slot(opt), &late, "unknown", opt.end, &late);
    }
    finish(&root, &slot(opt), &line, "unknown", opt.end, &late)
}
fn digest_of(frames: &[[u8; FRAME_LEN]]) -> String {
    let mut all = Vec::with_capacity(frames.len() * FRAME_LEN);
    for frame in frames {
        all.extend_from_slice(frame);
    }
    hex_digest(&all)
}
fn c1_reader(opt: &Opt) -> i32 {
    let root = match open_root(opt) {
        Ok(dir) => dir,
        Err(code) => return code,
    };
    let other = peer_role(opt.role);
    let sub = match child_dir(&root, leaf(other)) {
        Ok(dir) => dir,
        Err(RdErr::Replaced) => return c1_done(&root, opt, "failed", "replaced", (0, 1, "bad", "not_measured"), &Clock::new(), ("-", 0, "-")),
        Err(_) => return unk(opt, &root, "io"),
    };
    let mut clock = Clock::new();
    loop {
        if io::past(opt.end) {
            return c1_done(&root, opt, "unknown", "deadline", (0, 0, "-", "not_measured"), &clock, ("-", 0, "-"));
        }
        let mut frames = Vec::new();
        let mut missing = false;
        for seq in 1..=opt.count {
            if io::past(opt.end) {
                return c1_done(&root, opt, "unknown", "deadline", (frames.len() as u64, 0, "-", "not_measured"), &clock, ("-", 0, "-"));
            }
            match sub.read_file(&format!("f-{seq}"), FRAME_LEN, opt.end) {
                Ok((bytes, open_us, read_us, slice_us)) => {
                    clock.note_read(open_us, read_us, slice_us);
                    match frame::classify(&bytes) {
                        Ok((1, dir, s)) if s == seq && dir == direction(other) => {
                            if io::past(opt.end) {
                                return c1_done(&root, opt, "unknown", "deadline", (frames.len() as u64, 0, "-", "not_measured"), &clock, ("-", 0, "-"));
                            }
                            let mut buf = [0u8; FRAME_LEN];
                            buf.copy_from_slice(&bytes);
                            frames.push(buf);
                        }
                        _ => return c1_done(&root, opt, "failed", "torn", (frames.len() as u64, 1, "bad", "not_measured"), &clock, ("-", 0, "-")),
                    }
                }
                Err(RdErr::Absent) => missing = true,
                Err(RdErr::Deadline) => return c1_done(&root, opt, "unknown", "deadline", (frames.len() as u64, 0, "-", "not_measured"), &clock, ("-", 0, "-")),
                Err(RdErr::Replaced) => return c1_done(&root, opt, "failed", "replaced", (frames.len() as u64, 1, "bad", "not_measured"), &clock, ("-", 0, "-")),
                Err(RdErr::Symlink | RdErr::NotFile | RdErr::HardLink | RdErr::Size) => {
                    return c1_done(&root, opt, "failed", "nofollow", (frames.len() as u64, 1, "bad", "not_measured"), &clock, ("-", 0, "-"));
                }
                Err(RdErr::Io | RdErr::Name) => return c1_done(&root, opt, "unknown", "io", (frames.len() as u64, 0, "-", "not_measured"), &clock, ("-", 0, "-")),
            }
        }
        if missing {
            if !wait_more(opt.end) {
                return c1_done(&root, opt, "unknown", "deadline", (frames.len() as u64, 0, "-", "not_measured"), &clock, ("-", 0, "-"));
            }
            continue;
        }
        let digest = digest_of(&frames);
        if io::past(opt.end) {
            return c1_done(&root, opt, "unknown", "deadline", (frames.len() as u64, 0, "ok", "not_measured"), &clock, ("-", 0, digest.as_str()));
        }
        match root.read_file(&format!("summary-{other}"), 4096, opt.end) {
            Err(RdErr::Absent) => {
                if !wait_more(opt.end) {
                    return c1_done(&root, opt, "unknown", "no-peer-summary", (frames.len() as u64, 0, "ok", "not_measured"), &clock, ("-", 0, digest.as_str()));
                }
            }
            Err(RdErr::Deadline) => return c1_done(&root, opt, "unknown", "deadline", (frames.len() as u64, 0, "ok", "not_measured"), &clock, ("-", 0, digest.as_str())),
            Err(RdErr::Io | RdErr::Name) => return c1_done(&root, opt, "unknown", "io", (frames.len() as u64, 0, "ok", "not_measured"), &clock, ("-", 0, digest.as_str())),
            Err(RdErr::Replaced) => return c1_done(&root, opt, "failed", "replaced", (frames.len() as u64, 0, "bad", "not_measured"), &clock, ("-", 0, digest.as_str())),
            Err(_) => return c1_done(&root, opt, "failed", "nofollow", (frames.len() as u64, 0, "bad", "not_measured"), &clock, ("-", 0, digest.as_str())),
            Ok((bytes, open_us, read_us, slice_us)) => {
                clock.note_read(open_us, read_us, slice_us);
                if io::past(opt.end) {
                    return c1_done(&root, opt, "unknown", "deadline", (frames.len() as u64, 0, "ok", "not_measured"), &clock, ("-", 0, digest.as_str()));
                }
                return judge_summary(&root, opt, &clock, &digest, &bytes, other);
            }
        }
    }
}
fn judge_summary(root: &Dir, opt: &Opt, clock: &Clock, digest: &str, bytes: &[u8], other: &str) -> i32 {
    let n = u64::from(opt.count);
    let Some(text) = rec_text(bytes) else {
        return c1_done(root, opt, "failed", "bad-record", (n, 0, "bad", "not_measured"), clock, ("-", 0, digest));
    };
    let Some(rec) = decode(text, C1_KEYS) else {
        return c1_done(root, opt, "failed", "bad-record", (n, 0, "bad", "not_measured"), clock, ("-", 0, digest));
    };
    let torn = field(&rec, "torn");
    let shape = field(&rec, "v") == "1" && field(&rec, "op") == "c1" && field(&rec, "role") == other && field(&rec, "n") == opt.count.to_string() && field(&rec, "digest") == digest;
    if !shape || torn != "0" || field(&rec, "integrity") == "bad" {
        return c1_done(root, opt, "failed", "bad-record", (n, u64::from(torn == "1"), "bad", "not_measured"), clock, ("-", 0, digest));
    }
    let w_slice = field(&rec, "slice_us");
    let w_slice = if numeric(w_slice) || w_slice == "-" { w_slice } else { "-" };
    // Surviving bytes are an integrity observation. Visibility and round trip were not measured.
    c1_done(root, opt, "unknown", "partial-coverage", (n, 0, "ok", "not_measured"), clock, (w_slice, 1, digest))
}
fn c2_stop(root: &Dir, opt: &Opt, verdict: &str, reason: &str, attempted: u64, renamed: u64, part: &str) -> i32 {
    let planned = opt.count.to_string();
    let attempted = attempted.to_string();
    let renamed = renamed.to_string();
    let line = if verdict == "failed" { c2_line(opt.role, opt.boundary, verdict, reason, [&planned, &attempted, &renamed, &renamed], part, "-") } else { c2_unknown(opt, reason, &planned, &attempted, &renamed, &renamed, part) };
    if let Ok(evidence) = child_dir(root, "evidence") {
        let _ = store(&evidence, "progress", &line, None);
    }
    let late = c2_unknown(opt, "deadline", &planned, &attempted, &renamed, &renamed, part);
    finish(root, &slot(opt), &line, verdict, opt.end, &late)
}
fn c2_guest(opt: &Opt) -> i32 {
    let root = match open_root(opt) {
        Ok(dir) => dir,
        Err(code) => return code,
    };
    let data = match child_dir(&root, "data") {
        Ok(dir) => dir,
        Err(RdErr::Replaced) => return c2_stop(&root, opt, "failed", "replaced", 0, 0, "absent"),
        Err(_) => return unk(opt, &root, "io"),
    };
    let mut attempted = 0u64;
    let mut renamed = 0u64;
    for seq in 1..=opt.count {
        if io::past(opt.end) {
            return c2_stop(&root, opt, "unknown", "deadline", attempted, renamed, "absent");
        }
        attempted += 1;
        let bytes = frame::frame(2, 2, seq);
        let last = seq == opt.count;
        let step = if !last {
            data.publish(&format!("f-{seq}.tmp"), &format!("f-{seq}"), &bytes, true, opt.end)
        } else if opt.boundary == "file-sync" {
            data.write_fsync("part", &bytes, opt.end)
        } else {
            data.publish(&format!("f-{seq}.tmp"), &format!("f-{seq}"), &bytes, opt.boundary == "dir-sync", opt.end)
        };
        match step {
            Ok(_) => {
                if !last || opt.boundary != "file-sync" {
                    renamed += 1;
                }
                if io::past(opt.end) {
                    return c2_stop(&root, opt, "unknown", "deadline", attempted, renamed, "absent");
                }
            }
            Err(RdErr::Deadline) => return c2_stop(&root, opt, "unknown", "deadline", attempted, renamed, "absent"),
            Err(RdErr::Replaced) => return c2_stop(&root, opt, "failed", "replaced", attempted, renamed, "absent"),
            Err(_) => return c2_stop(&root, opt, "unknown", "io", attempted, renamed, "absent"),
        }
    }
    let planned = opt.count.to_string();
    let attempted_s = attempted.to_string();
    let renamed_s = renamed.to_string();
    let dir_synced = if opt.boundary == "dir-sync" { "1" } else { "0" };
    let part = if opt.boundary == "file-sync" { "ok" } else { "absent" };
    let line = c2_line(opt.role, opt.boundary, "not-ack", "-", [&planned, &attempted_s, &renamed_s, &renamed_s], part, dir_synced);
    let late = c2_unknown(opt, "deadline", &planned, &attempted_s, &renamed_s, &renamed_s, part);
    if io::past(opt.end) {
        return c2_stop(&root, opt, "unknown", "deadline", attempted, renamed, part);
    }
    let evidence = match child_dir(&root, "evidence") {
        Ok(dir) => dir,
        Err(_) => return c2_stop(&root, opt, "unknown", "io", attempted, renamed, part),
    };
    if store(&evidence, "progress", &line, Some(opt.end)).is_err() || io::past(opt.end) {
        let _ = store(&evidence, "progress", &late, None);
        return finish(&root, &slot(opt), &late, "unknown", opt.end, &late);
    }
    finish(&root, &slot(opt), &line, "not-ack", opt.end, &late)
}
fn c2_host(opt: &Opt) -> i32 {
    let root = match open_root(opt) {
        Ok(dir) => dir,
        Err(code) => return code,
    };
    let data = match child_dir(&root, "data") {
        Ok(dir) => dir,
        Err(RdErr::Replaced) => return c2_host_line(&root, opt, "failed", "replaced", ("-", "-"), "0", "absent"),
        Err(_) => return unk(opt, &root, "io"),
    };
    let mut visible = 0u64;
    for seq in 1..=opt.count {
        if io::past(opt.end) {
            return c2_host_line(&root, opt, "unknown", "deadline", ("-", "-"), &visible.to_string(), "absent");
        }
        match data.read_file(&format!("f-{seq}"), FRAME_LEN, opt.end) {
            Ok((bytes, _, _, _)) => match frame::classify(&bytes) {
                Ok((2, 2, s)) if s == seq => {
                    if io::past(opt.end) {
                        return c2_host_line(&root, opt, "unknown", "deadline", ("-", "-"), &visible.to_string(), "absent");
                    }
                    visible += 1;
                }
                _ => return c2_host_line(&root, opt, "failed", "torn", ("-", "-"), &visible.to_string(), "absent"),
            },
            Err(RdErr::Absent) => {}
            Err(RdErr::Deadline) => return c2_host_line(&root, opt, "unknown", "deadline", ("-", "-"), &visible.to_string(), "absent"),
            Err(RdErr::Replaced) => return c2_host_line(&root, opt, "failed", "replaced", ("-", "-"), &visible.to_string(), "absent"),
            Err(RdErr::Symlink | RdErr::NotFile | RdErr::HardLink | RdErr::Size) => {
                return c2_host_line(&root, opt, "failed", "nofollow", ("-", "-"), &visible.to_string(), "absent");
            }
            Err(_) => return c2_host_line(&root, opt, "unknown", "io", ("-", "-"), &visible.to_string(), "absent"),
        }
    }
    if io::past(opt.end) {
        return c2_host_line(&root, opt, "unknown", "deadline", ("-", "-"), &visible.to_string(), "absent");
    }
    let part_state = match data.read_file("part", FRAME_LEN, opt.end) {
        Err(RdErr::Absent) => "absent",
        Err(RdErr::Deadline) => return c2_host_line(&root, opt, "unknown", "deadline", ("-", "-"), &visible.to_string(), "absent"),
        Err(RdErr::Replaced) => return c2_host_line(&root, opt, "failed", "replaced", ("-", "-"), &visible.to_string(), "absent"),
        Err(RdErr::Symlink | RdErr::NotFile | RdErr::HardLink | RdErr::Size) => return c2_host_line(&root, opt, "failed", "nofollow", ("-", "-"), &visible.to_string(), "absent"),
        Err(_) => return c2_host_line(&root, opt, "unknown", "io", ("-", "-"), &visible.to_string(), "absent"),
        Ok((bytes, _, _, _)) => match frame::classify(&bytes) {
            Ok((2, 2, s)) if s == opt.count => "ok",
            _ => "bad",
        },
    };
    if io::past(opt.end) {
        return c2_host_line(&root, opt, "unknown", "deadline", ("-", "-"), &visible.to_string(), part_state);
    }
    let evidence = match child_dir(&root, "evidence") {
        Ok(dir) => dir,
        Err(_) => return c2_host_line(&root, opt, "unknown", "io", ("-", "-"), &visible.to_string(), part_state),
    };
    let progress_text = match evidence.read_file("progress", 4096, opt.end) {
        Ok((bytes, _, _, _)) => rec_text(&bytes).map(str::to_string),
        Err(RdErr::Absent) => None,
        Err(RdErr::Deadline) => return c2_host_line(&root, opt, "unknown", "deadline", ("-", "-"), &visible.to_string(), part_state),
        Err(RdErr::Replaced) => return c2_host_line(&root, opt, "failed", "replaced", ("-", "-"), &visible.to_string(), part_state),
        Err(RdErr::Symlink | RdErr::NotFile | RdErr::HardLink | RdErr::Size) => return c2_host_line(&root, opt, "failed", "nofollow", ("-", "-"), &visible.to_string(), part_state),
        Err(_) => return c2_host_line(&root, opt, "unknown", "io", ("-", "-"), &visible.to_string(), part_state),
    };
    let progress = progress_text.as_deref().and_then(|text| decode(text, C2_KEYS));
    let (attempted, renamed) = match &progress {
        Some(rec) => (field(rec, "attempted"), field(rec, "renamed")),
        None => ("-", "-"),
    };
    if part_state == "bad" {
        return c2_host_line(&root, opt, "failed", "bad-part", (attempted, renamed), &visible.to_string(), "bad");
    }
    if io::past(opt.end) {
        return c2_host_line(&root, opt, "unknown", "deadline", (attempted, renamed), &visible.to_string(), part_state);
    }
    if progress.as_ref().is_some_and(|rec| c2_progress_ok(rec, opt, visible, part_state)) {
        return c2_host_line(&root, opt, "not-ack", "-", (attempted, renamed), &visible.to_string(), part_state);
    }
    let reason = if progress.is_none() { "no-peer-summary" } else { "bad-record" };
    c2_host_line(&root, opt, "unknown", reason, (attempted, renamed), &visible.to_string(), part_state)
}
fn c2_progress_ok(rec: &[(&str, &str)], opt: &Opt, visible: u64, part_state: &str) -> bool {
    let want_renamed = if opt.boundary == "file-sync" { u64::from(opt.count - 1) } else { u64::from(opt.count) };
    let want_part = if opt.boundary == "file-sync" { "ok" } else { "absent" };
    let want_dir = if opt.boundary == "dir-sync" { "1" } else { "0" };
    field(rec, "v") == "1"
        && field(rec, "op") == "c2"
        && field(rec, "role") == "guest"
        && field(rec, "boundary") == opt.boundary
        && field(rec, "verdict") == "not-ack"
        && field(rec, "planned") == opt.count.to_string()
        && field(rec, "attempted") == opt.count.to_string()
        && field(rec, "renamed") == want_renamed.to_string()
        && field(rec, "visible") == want_renamed.to_string()
        && field(rec, "dir_synced") == want_dir
        && field(rec, "part") == want_part
        && part_state == want_part
        && visible == want_renamed
}
fn c2_host_line(root: &Dir, opt: &Opt, verdict: &str, reason: &str, guest: (&str, &str), visible: &str, part: &str) -> i32 {
    let (attempted, renamed) = guest;
    let planned = opt.count.to_string();
    let dir_synced = if verdict == "not-ack" {
        if opt.boundary == "dir-sync" {
            "1"
        } else {
            "0"
        }
    } else {
        "-"
    };
    let line = c2_line("host", opt.boundary, verdict, reason, [&planned, attempted, renamed, visible], part, dir_synced);
    let late = c2_line("host", opt.boundary, "unknown", "deadline", [&planned, attempted, renamed, visible], part, "-");
    finish(root, "result-host", &line, verdict, opt.end, &late)
}
fn c8_guest(opt: &Opt) -> i32 {
    let root = match open_root(opt) {
        Ok(dir) => dir,
        Err(code) => return code,
    };
    if io::past(opt.end) {
        return unk(opt, &root, "deadline");
    }
    let n = count_principals(opt.end);
    if io::past(opt.end) {
        return unk(opt, &root, "deadline");
    }
    let (verdict, reason) = judge_c8(n);
    let shown = n.map(|v| v.to_string()).unwrap_or_else(|| "0".to_string());
    let line = c8_line("guest", verdict, reason, &shown);
    let late = c8_line("guest", "unknown", "deadline", &shown);
    finish(&root, "result-guest", &line, verdict, opt.end, &late)
}
fn c8_host(opt: &Opt) -> i32 {
    let root = match open_root(opt) {
        Ok(dir) => dir,
        Err(code) => return code,
    };
    if io::past(opt.end) {
        return unk(opt, &root, "deadline");
    }
    match root.read_file("result-guest", 4096, opt.end) {
        Err(RdErr::Absent) => unk(opt, &root, "no-peer-summary"),
        Err(RdErr::Deadline) => unk(opt, &root, "deadline"),
        Err(RdErr::Io | RdErr::Name) => unk(opt, &root, "io"),
        Err(_) => finish(&root, "result-host", &c8_line("host", "failed", "nofollow", "0"), "failed", opt.end, &c8_line("host", "unknown", "deadline", "0")),
        Ok((bytes, _, _, _)) => {
            if io::past(opt.end) {
                return unk(opt, &root, "deadline");
            }
            let Some(text) = rec_text(&bytes) else {
                return finish(&root, "result-host", &c8_line("host", "failed", "bad-record", "0"), "failed", opt.end, &c8_line("host", "unknown", "deadline", "0"));
            };
            let Some(rec) = decode(text, C8_KEYS) else {
                return finish(&root, "result-host", &c8_line("host", "failed", "bad-record", "0"), "failed", opt.end, &c8_line("host", "unknown", "deadline", "0"));
            };
            let n = field(&rec, "n");
            let n = if numeric(n) { n } else { "0" };
            let policy = field(&rec, "op") != "c8" || field(&rec, "role") != "guest" || field(&rec, "ops") != "0" || field(&rec, "created") != "0" || field(&rec, "elevated") != "0" || field(&rec, "verdict") != "unknown";
            if policy {
                return finish(&root, "result-host", &c8_line("host", "failed", "subject-policy", n), "failed", opt.end, &c8_line("host", "unknown", "deadline", n));
            }
            let reason = field(&rec, "reason");
            let line = c8_line("host", "unknown", reason, n);
            let late = c8_line("host", "unknown", "deadline", n);
            finish(&root, "result-host", &line, "unknown", opt.end, &late)
        }
    }
}
pub fn judge_c8(n: Option<usize>) -> (&'static str, &'static str) {
    match n {
        Some(1) => ("unknown", "single-principal"),
        Some(n) if n >= 2 => ("unknown", "mapping-not-measured"),
        _ => ("unknown", "principal-unknown"),
    }
}
pub fn count_non_admin(users: &str, admins: &str) -> Result<usize, ()> {
    let Some(line) = admins.lines().find(|l| l.contains("GroupMembership")) else {
        return Err(());
    };
    let admin_names: Vec<&str> = line.split_whitespace().filter(|tok| !tok.contains(':')).collect();
    let mut n = 0usize;
    for line in users.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let name = parts.next().ok_or(())?;
        let uid: u32 = parts.next().ok_or(())?.parse().map_err(|_| ())?;
        if parts.next().is_some() {
            return Err(());
        }
        if uid >= 500 && name != "root" && name != "daemon" && !name.starts_with('_') && !admin_names.contains(&name) {
            n += 1;
        }
    }
    Ok(n)
}
fn count_principals(end: Instant) -> Option<usize> {
    let users = bounded_out(&["/usr/bin/dscl", ".", "-list", "/Users", "UniqueID"], end)?;
    let admins = bounded_out(&["/usr/bin/dscl", ".", "-read", "/Groups/admin", "GroupMembership"], end)?;
    count_non_admin(&users, &admins).ok()
}

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}
extern "C" {
    #[link_name = "read"]
    fn fd_read(fd: i32, buf: *mut std::ffi::c_void, count: usize) -> isize;
    #[cfg(target_os = "linux")]
    fn poll(fds: *mut PollFd, nfds: usize, timeout: i32) -> i32;
    #[cfg(not(target_os = "linux"))]
    fn poll(fds: *mut PollFd, nfds: u32, timeout: i32) -> i32;
    #[cfg(test)]
    fn pipe(fds: *mut i32) -> i32;
    #[cfg(test)]
    fn close(fd: i32) -> i32;
}
fn poll_ready(fd: i32, timeout_ms: i32) -> stdio::Result<bool> {
    let mut pfd = PollFd { fd, events: 1, revents: 0 };
    let rc = unsafe {
        #[cfg(target_os = "linux")]
        {
            poll(&mut pfd, 1usize, timeout_ms)
        }
        #[cfg(not(target_os = "linux"))]
        {
            poll(&mut pfd, 1u32, timeout_ms)
        }
    };
    if rc < 0 {
        let err = stdio::Error::last_os_error();
        if err.kind() == ErrorKind::Interrupted {
            return Ok(false);
        }
        return Err(err);
    }
    Ok(rc > 0 && pfd.revents & 1 != 0)
}
fn drain_fd(fd: i32, end: Instant) -> Option<String> {
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        if out.len() > 65536 || io::past(end) {
            return None;
        }
        let remain_ms = end.saturating_duration_since(Instant::now()).as_millis();
        if remain_ms == 0 {
            return None;
        }
        let wait = i32::try_from(remain_ms.min(50)).unwrap_or(50);
        match poll_ready(fd, wait) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(_) => return None,
        }
        let n = unsafe { fd_read(fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n == 0 {
            break;
        }
        if n < 0 {
            let err = stdio::Error::last_os_error();
            if err.kind() == ErrorKind::Interrupted {
                continue;
            }
            return None;
        }
        let n = usize::try_from(n).ok()?;
        out.extend_from_slice(&buf[..n]);
    }
    String::from_utf8(out).ok()
}
fn try_wait_bounded(child: &mut Child) -> bool {
    let until = Instant::now() + Duration::from_millis(200);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) if Instant::now() >= until => return false,
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(_) => return false,
        }
    }
}
fn bounded_out(argv: &[&str], end: Instant) -> Option<String> {
    if argv.is_empty() || io::past(end) {
        return None;
    }
    let mut child = Command::new(argv[0]).args(&argv[1..]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let stdout = child.stdout.take()?;
    loop {
        if io::past(end) {
            let _ = child.kill();
            let _ = try_wait_bounded(&mut child);
            return None;
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return drain_fd(stdout.as_raw_fd(), end),
            Ok(Some(_)) => return None,
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(_) => {
                let _ = child.kill();
                let _ = try_wait_bounded(&mut child);
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch(tag: &str) -> std::path::PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("s11m-{tag}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(p.join("host-write")).unwrap();
        fs::create_dir_all(p.join("guest-write")).unwrap();
        fs::create_dir_all(p.join("data")).unwrap();
        fs::create_dir_all(p.join("evidence")).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        fs::canonicalize(&p).unwrap()
    }
    fn run(dir: &std::path::Path, args: &[&str]) -> i32 {
        let owned: Vec<String> = args.iter().map(|arg| if *arg == "DIR" { dir.to_str().unwrap().to_string() } else { (*arg).to_string() }).collect();
        execute(&owned).unwrap()
    }
    fn text(dir: &std::path::Path, name: &str) -> String {
        fs::read_to_string(dir.join(name)).unwrap()
    }
    struct HookGuard;
    impl Drop for HookGuard {
        fn drop(&mut self) {
            io::test_clear_hook();
        }
    }

    #[test]
    fn deadline_writes_unknown_without_invented_times() {
        let dir = scratch("dead");
        let code = run(&dir, &["writer", "--dir", "DIR", "--role", "guest", "--count", "8", "--timeout", "0"]);
        assert_eq!(code, 1);
        let raw = fs::read(dir.join("result-guest")).unwrap();
        assert_eq!(raw.iter().filter(|b| **b == b'\n').count(), 1);
        assert_eq!(*raw.last().unwrap(), b'\n');
        let line = String::from_utf8(raw).unwrap();
        assert!(line.contains("verdict=unknown"), "{line}");
        assert!(line.contains("reason=deadline"), "{line}");
        assert!(line.contains("open_us=-"), "{line}");
        assert!(line.contains("write_us=-"), "{line}");
        assert!(line.contains("lost=-"), "{line}");
        assert!(!line.contains("verdict=ok"), "{line}");
        assert!(!line.contains("lost=0"), "{line}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reader_keeps_integrity_and_does_not_pass_unmeasured_latency() {
        let dir = scratch("c1");
        assert_eq!(run(&dir, &["writer", "--dir", "DIR", "--role", "guest", "--count", "1", "--timeout", "2"]), 1);
        assert_eq!(run(&dir, &["writer", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 1);
        assert_eq!(run(&dir, &["reader", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 1);
        let partial = text(&dir, "result-host");
        assert!(partial.contains("verdict=unknown"), "{partial}");
        assert!(partial.contains("reason=partial-coverage"), "{partial}");
        assert!(partial.contains("integrity=ok"), "{partial}");
        assert!(partial.contains("latency=not_measured"), "{partial}");
        assert!(partial.contains("peer=1"), "{partial}");
        assert!(partial.contains("vis_us=-") && partial.contains("rtt_us=-") && partial.contains("lost=-"), "{partial}");
        assert!(!partial.contains("verdict=ok"), "{partial}");
        let summary = text(&dir, "summary-guest");
        assert!(decode(summary.trim_end_matches('\n'), C1_KEYS).is_some(), "{summary}");
        fs::remove_file(dir.join("summary-guest")).unwrap();
        assert_ne!(run(&dir, &["reader", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "1"]), 0);
        let missing = text(&dir, "result-host");
        assert!(missing.contains("reason=no-peer-summary"), "{missing}");
        assert!(!missing.contains("verdict=ok"), "{missing}");
        let mut bytes = fs::read(dir.join("guest-write/f-1")).unwrap();
        bytes[80] ^= 0xff;
        fs::write(dir.join("guest-write/f-1"), bytes).unwrap();
        fs::write(dir.join("summary-guest"), summary.as_bytes()).unwrap();
        assert_ne!(run(&dir, &["reader", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 0);
        let torn = text(&dir, "result-host");
        assert!(torn.contains("verdict=failed") && torn.contains("reason=torn"), "{torn}");
        fs::write(dir.join("guest-write/f-1"), frame::frame(1, 2, 1)).unwrap();
        let mut contradicted = summary.trim_end_matches('\n').to_string();
        contradicted = contradicted.replace("torn=0", "torn=1");
        fs::write(dir.join("summary-guest"), format!("{contradicted}\n")).unwrap();
        assert_ne!(run(&dir, &["reader", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 0);
        let bad = text(&dir, "result-host");
        assert!(bad.contains("verdict=failed") && bad.contains("reason=bad-record"), "{bad}");
        assert!(!bad.contains("verdict=ok"), "{bad}");
        let claimed = summary.trim_end_matches('\n').replace("latency=not_measured", "latency=measured").replace("verdict=unknown", "verdict=ok");
        fs::write(dir.join("summary-guest"), format!("{claimed}\n")).unwrap();
        assert_ne!(run(&dir, &["reader", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 0);
        let still = text(&dir, "result-host");
        assert!(still.contains("reason=partial-coverage") && still.contains("latency=not_measured"), "{still}");
        assert!(!still.contains("verdict=ok"), "{still}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn record_framing_rejects_duplicates_and_extra_newlines() {
        let line = c8_line("host", "unknown", "single-principal", "1");
        assert!(decode(&line, C8_KEYS).is_some());
        assert!(decode(&format!("{line} op=c8"), C8_KEYS).is_none());
        assert!(decode(&line.replace("reason=single-principal", "reason=single-principal reason=other"), C8_KEYS).is_none());
        assert!(decode("v=1 op=c8", C8_KEYS).is_none());
        let raw = format!("{line}\n");
        assert_eq!(rec_text(raw.as_bytes()).unwrap(), line);
        assert!(rec_text(line.as_bytes()).is_some());
        assert!(rec_text(format!("{line}\n\n").as_bytes()).is_none());
        assert!(rec_text(format!("{line}\ntrailing").as_bytes()).is_none());
    }

    #[test]
    fn late_publish_read_and_result_store_stay_deadline() {
        let _guard = HookGuard;
        let dir = scratch("late-pub");
        io::test_set_hook(Box::new(|| thread::sleep(Duration::from_millis(1100))));
        let started = Instant::now();
        let code = run(&dir, &["writer", "--dir", "DIR", "--role", "guest", "--count", "1", "--timeout", "1"]);
        assert!(started.elapsed() < Duration::from_secs(3));
        assert_eq!(code, 1);
        let line = text(&dir, "result-guest");
        assert!(line.contains("reason=deadline"), "{line}");
        assert!(!line.contains("verdict=ok") && !line.contains("partial-coverage"), "{line}");
        io::test_clear_hook();

        let dir = scratch("late-sum");
        assert_eq!(run(&dir, &["writer", "--dir", "DIR", "--role", "guest", "--count", "1", "--timeout", "2"]), 1);
        io::test_set_hook(Box::new(|| {
            io::test_set_hook(Box::new(|| thread::sleep(Duration::from_millis(1100))));
        }));
        let started = Instant::now();
        let code = run(&dir, &["reader", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "1"]);
        assert!(started.elapsed() < Duration::from_secs(3));
        assert_eq!(code, 1);
        let line = text(&dir, "result-host");
        assert!(line.contains("reason=deadline"), "{line}");
        assert!(!line.contains("verdict=ok") && !line.contains("partial-coverage"), "{line}");
        io::test_clear_hook();

        let dir = scratch("late-store");
        assert_eq!(run(&dir, &["writer", "--dir", "DIR", "--role", "guest", "--count", "1", "--timeout", "2"]), 1);
        io::test_set_hook(Box::new(|| {
            io::test_set_hook(Box::new(|| {
                io::test_set_hook(Box::new(|| thread::sleep(Duration::from_millis(1100))));
            }));
        }));
        let started = Instant::now();
        let code = run(&dir, &["reader", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "1"]);
        assert!(started.elapsed() < Duration::from_secs(3));
        assert_eq!(code, 1);
        let line = text(&dir, "result-host");
        assert!(line.contains("reason=deadline"), "{line}");
        assert!(!line.contains("verdict=ok") && !line.contains("reason=partial-coverage"), "{line}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_sync_cut_counts_two_renames_and_rejects_a_bad_part() {
        let dir = scratch("c2");
        assert_eq!(run(&dir, &["c2", "file-sync", "--dir", "DIR", "--role", "guest", "--count", "3", "--timeout", "2"]), 0);
        assert!(!dir.join("data/f-3").exists());
        assert!(dir.join("data/f-1").is_file());
        assert!(dir.join("data/f-2").is_file());
        let part = fs::read(dir.join("data/part")).unwrap();
        assert_eq!(part, frame::frame(2, 2, 3));
        let progress = text(&dir, "evidence/progress");
        assert!(progress.contains("planned=3") && progress.contains("attempted=3") && progress.contains("renamed=2") && progress.contains("visible=2"), "{progress}");
        assert!(progress.contains("dir_synced=0"), "{progress}");
        assert!(progress.contains("verdict=not-ack"), "{progress}");
        assert!(progress.ends_with('\n') && progress.matches('\n').count() == 1, "{progress:?}");
        assert_eq!(run(&dir, &["c2", "file-sync", "--dir", "DIR", "--role", "host", "--count", "3", "--timeout", "2"]), 0);
        let host = text(&dir, "result-host");
        assert!(host.contains("verdict=not-ack"), "{host}");
        assert!(host.contains("attempted=3") && host.contains("renamed=2") && host.contains("visible=2"), "{host}");
        fs::write(dir.join("data/part"), frame::frame(2, 2, 1)).unwrap();
        assert_ne!(run(&dir, &["c2", "file-sync", "--dir", "DIR", "--role", "host", "--count", "3", "--timeout", "2"]), 0);
        let bad = text(&dir, "result-host");
        assert!(bad.contains("verdict=failed") && bad.contains("reason=bad-part"), "{bad}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn second_publish_failure_keeps_attempted_and_renamed() {
        let dir = scratch("c2cut");
        fs::write(dir.join("data/f-2.tmp"), b"blocked").unwrap();
        assert_ne!(run(&dir, &["c2", "file-sync", "--dir", "DIR", "--role", "guest", "--count", "3", "--timeout", "2"]), 0);
        let progress = text(&dir, "evidence/progress");
        assert!(progress.contains("planned=3"), "{progress}");
        assert!(progress.contains("attempted=2"), "{progress}");
        assert!(progress.contains("renamed=1"), "{progress}");
        assert!(progress.contains("visible=1"), "{progress}");
        assert!(!progress.contains("attempted=0") && !progress.contains("attempted=3") && !progress.contains("renamed=0"), "{progress}");
        assert_ne!(run(&dir, &["c2", "file-sync", "--dir", "DIR", "--role", "host", "--count", "3", "--timeout", "2"]), 0);
        let host = text(&dir, "result-host");
        assert!(host.contains("attempted=2") && host.contains("renamed=1") && host.contains("visible=1"), "{host}");
        assert!(!host.contains("verdict=not-ack"), "{host}");
        fs::remove_file(dir.join("evidence/progress")).unwrap();
        assert_ne!(run(&dir, &["c2", "file-sync", "--dir", "DIR", "--role", "host", "--count", "3", "--timeout", "2"]), 0);
        let missing = text(&dir, "result-host");
        assert!(missing.contains("attempted=-") && missing.contains("renamed=-"), "{missing}");
        assert!(missing.contains("visible=1"), "{missing}");
        assert!(!missing.contains("attempted=3"), "{missing}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dir_sync_and_rename_cuts_do_not_claim_ack() {
        for boundary in ["dir-sync", "rename"] {
            let dir = scratch(boundary);
            assert_eq!(run(&dir, &["c2", boundary, "--dir", "DIR", "--role", "guest", "--count", "3", "--timeout", "2"]), 0);
            assert!(dir.join("data/f-3").is_file());
            assert!(!dir.join("data/part").exists());
            let progress = text(&dir, "evidence/progress");
            assert!(progress.contains("renamed=3"), "{progress}");
            assert!(progress.contains("verdict=not-ack"), "{progress}");
            if boundary == "dir-sync" {
                assert!(progress.contains("dir_synced=1"), "{progress}");
            } else {
                assert!(progress.contains("dir_synced=0"), "{progress}");
            }
            assert_eq!(run(&dir, &["c2", boundary, "--dir", "DIR", "--role", "host", "--count", "3", "--timeout", "2"]), 0);
            let host = text(&dir, "result-host");
            assert!(host.contains("verdict=not-ack"), "{host}");
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn one_non_admin_is_single_principal_and_host_does_not_elevate() {
        let users = "root 0\ndaemon 1\n_spotlight 89\neval 501\n";
        let admins = "dsAttrTypeStandard:GroupMembership: admin\n";
        assert_eq!(count_non_admin(users, admins).unwrap(), 1);
        assert_eq!(judge_c8(Some(1)), ("unknown", "single-principal"));
        assert_eq!(judge_c8(Some(2)).1, "mapping-not-measured");
        let dir = scratch("c8");
        let guest = c8_line("guest", "unknown", "single-principal", "1");
        fs::write(dir.join("result-guest"), guest.as_bytes()).unwrap();
        assert_ne!(run(&dir, &["c8", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 0);
        let line = text(&dir, "result-host");
        assert!(line.contains("reason=single-principal"), "{line}");
        assert!(line.contains("ops=0") && line.contains("created=0") && line.contains("elevated=0"), "{line}");
        assert!(line.ends_with('\n'));
        fs::write(dir.join("result-guest"), format!("{guest}\n")).unwrap();
        fs::remove_file(dir.join("result-host")).unwrap();
        assert_ne!(run(&dir, &["c8", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 0);
        assert!(text(&dir, "result-host").contains("reason=single-principal"));
        fs::write(dir.join("result-guest"), b"v=1 op=c8 role=guest verdict=unknown reason=single-principal n=1 ops=0 created=1 elevated=0\n").unwrap();
        fs::remove_file(dir.join("result-host")).unwrap();
        assert_ne!(run(&dir, &["c8", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 0);
        let bad = text(&dir, "result-host");
        assert!(bad.contains("verdict=failed") && bad.contains("reason=subject-policy"), "{bad}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn bounded_out_does_not_block_on_a_held_pipe_or_a_sleep() {
        let mut fds = [0i32; 2];
        assert_eq!(unsafe { pipe(fds.as_mut_ptr()) }, 0);
        let started = Instant::now();
        let drained = drain_fd(fds[0], Instant::now() + Duration::from_millis(200));
        assert!(drained.is_none());
        assert!(started.elapsed() < Duration::from_secs(1));
        unsafe {
            close(fds[0]);
            close(fds[1]);
        }
        let started = Instant::now();
        let echoed = bounded_out(&["/bin/echo", "hi"], Instant::now() + Duration::from_secs(2));
        assert_eq!(echoed.as_deref(), Some("hi\n"));
        assert!(started.elapsed() < Duration::from_secs(2));
        let started = Instant::now();
        let slept = bounded_out(&["/bin/sleep", "30"], Instant::now() + Duration::from_millis(150));
        assert!(slept.is_none());
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}

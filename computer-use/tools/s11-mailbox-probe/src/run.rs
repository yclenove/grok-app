use crate::frame::{self, FRAME_LEN};
use crate::io::{self, Dir, RdErr, Times};
use crate::sha256::hex_digest;
use std::io::Read;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
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
}
impl Clock {
    fn new() -> Self {
        Self { open_us: None, write_us: None, fsync_us: None, rename_us: None, dir_sync_us: None, read_us: None }
    }
    fn note(&mut self, t: &Times) {
        bump(&mut self.open_us, t.open_us);
        bump(&mut self.write_us, t.write_us);
        bump(&mut self.fsync_us, t.fsync_us);
        bump(&mut self.rename_us, t.rename_us);
        if let Some(v) = t.dir_sync_us {
            bump(&mut self.dir_sync_us, v);
        }
    }
    fn note_read(&mut self, open_us: u64, read_us: u64) {
        bump(&mut self.open_us, open_us);
        bump(&mut self.read_us, read_us);
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
fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split(' ').find_map(|tok| tok.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| v))
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
fn peer(role: &str) -> &'static str {
    if role == "host" {
        "guest"
    } else {
        "host"
    }
}
fn c1_line(role: &str, verdict: &str, reason: &str, n: u64, torn: u64, clock: &Clock, extra: (u8, &str)) -> String {
    format!(
        "v=1 op=c1 role={role} verdict={verdict} reason={reason} n={n} torn={torn} lost=0 open_us={} write_us={} fsync_us={} rename_us={} dir_sync_us={} read_us={} peer={} digest={}",
        num(clock.open_us),
        num(clock.write_us),
        num(clock.fsync_us),
        num(clock.rename_us),
        num(clock.dir_sync_us),
        num(clock.read_us),
        extra.0,
        extra.1
    )
}
fn c2_line(role: &str, boundary: &str, verdict: &str, reason: &str, nums: [u64; 4], part: &str, dir_synced: &str) -> String {
    format!("v=1 op=c2 role={role} boundary={boundary} verdict={verdict} reason={reason} planned={} attempted={} renamed={} visible={} part={part} dir_synced={dir_synced}", nums[0], nums[1], nums[2], nums[3])
}
fn c8_line(role: &str, verdict: &str, reason: &str, n: u64) -> String {
    format!("v=1 op=c8 role={role} verdict={verdict} reason={reason} n={n} ops=0 created=0 elevated=0")
}
fn say(line: &str) {
    println!("{line}");
    let _ = std::io::Write::flush(&mut std::io::stdout());
}
fn store(dir: &Dir, stem: &str, line: &str) -> Result<(), RdErr> {
    if line.len() > 4096 || line.as_bytes().contains(&b'\n') {
        return Err(RdErr::Size);
    }
    dir.publish(&format!("{stem}.tmp"), stem, line.as_bytes(), true).map(|_| ())
}
fn finish(dir: &Dir, stem: &str, line: &str, verdict: &str) -> i32 {
    if store(dir, stem, line).is_err() {
        say("v=1 op=none role=none verdict=unknown reason=io n=0 torn=0 lost=0 open_us=- write_us=- fsync_us=- rename_us=- dir_sync_us=- read_us=- peer=0 digest=-");
        return 1;
    }
    say(line);
    if verdict == "ok" || verdict == "not-ack" {
        0
    } else {
        1
    }
}
fn slot(opt: &Opt) -> String {
    format!("result-{}", opt.role)
}
fn bare_unknown(opt: &Opt, reason: &str) -> String {
    match opt.cmd {
        "c2" => c2_line(opt.role, opt.boundary, "unknown", reason, [0, 0, 0, 0], "absent", "-"),
        "c8" => c8_line(opt.role, "unknown", reason, 0),
        _ => c1_line(opt.role, "unknown", reason, 0, 0, &Clock::new(), (0, "-")),
    }
}
fn unk(opt: &Opt, root: &Dir, reason: &str) -> i32 {
    let line = bare_unknown(opt, reason);
    finish(root, &slot(opt), &line, "unknown")
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
    let line = bare_unknown(opt, "deadline");
    if let Ok(dir) = Dir::open_path(&opt.dir) {
        return finish(&dir, &slot(opt), &line, "unknown");
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
fn c1_writer(opt: &Opt) -> i32 {
    let root = match open_root(opt) {
        Ok(dir) => dir,
        Err(code) => return code,
    };
    let sub = match child_dir(&root, leaf(opt.role)) {
        Ok(dir) => dir,
        Err(_) => return unk(opt, &root, "io"),
    };
    let mut clock = Clock::new();
    let mut frames = Vec::with_capacity(opt.count as usize);
    for seq in 1..=opt.count {
        if io::past(opt.end) {
            let line = c1_line(opt.role, "unknown", "deadline", frames.len() as u64, 0, &clock, (0, "-"));
            return finish(&root, &slot(opt), &line, "unknown");
        }
        let bytes = frame::frame(1, direction(opt.role), seq);
        match sub.publish(&format!("f-{seq}.tmp"), &format!("f-{seq}"), &bytes, true) {
            Ok(t) => clock.note(&t),
            Err(_) => return unk(opt, &root, "io"),
        }
        frames.push(bytes);
    }
    let digest = digest_of(&frames);
    let line = c1_line(opt.role, "ok", "-", u64::from(opt.count), 0, &clock, (0, digest.as_str()));
    if store(&root, &format!("summary-{}", opt.role), &line).is_err() {
        return unk(opt, &root, "io");
    }
    finish(&root, &slot(opt), &line, "ok")
}
fn digest_of(frames: &[[u8; FRAME_LEN]]) -> String {
    let mut all = Vec::with_capacity(frames.len() * FRAME_LEN);
    for frame in frames {
        all.extend_from_slice(frame);
    }
    hex_digest(&all)
}
fn integrity_reason(err: RdErr) -> &'static str {
    match err {
        RdErr::Replaced => "replaced",
        RdErr::Symlink | RdErr::NotFile | RdErr::HardLink | RdErr::Size => "nofollow",
        RdErr::Absent | RdErr::Name | RdErr::Io => "nofollow",
    }
}
fn done_c1(root: &Dir, opt: &Opt, verdict: &str, reason: &str, n_torn: (u64, u64), clock: &Clock, extra: (u8, &str)) -> i32 {
    let line = c1_line(opt.role, verdict, reason, n_torn.0, n_torn.1, clock, extra);
    finish(root, &slot(opt), &line, verdict)
}
fn c1_reader(opt: &Opt) -> i32 {
    let root = match open_root(opt) {
        Ok(dir) => dir,
        Err(code) => return code,
    };
    let other = peer(opt.role);
    let sub = match child_dir(&root, leaf(other)) {
        Ok(dir) => dir,
        Err(_) => return unk(opt, &root, "io"),
    };
    let mut clock = Clock::new();
    loop {
        if io::past(opt.end) {
            return unk(opt, &root, "deadline");
        }
        let mut frames = Vec::new();
        let mut missing = false;
        for seq in 1..=opt.count {
            match sub.read_file(&format!("f-{seq}"), FRAME_LEN) {
                Ok((bytes, o, r)) => {
                    clock.note_read(o, r);
                    match frame::classify(&bytes) {
                        Ok((1, dir, s)) if s == seq && dir == direction(other) => {
                            let mut buf = [0u8; FRAME_LEN];
                            buf.copy_from_slice(&bytes);
                            frames.push(buf);
                        }
                        _ => return done_c1(&root, opt, "failed", "torn", (0, 1), &clock, (0, "-")),
                    }
                }
                Err(RdErr::Absent) => missing = true,
                Err(RdErr::Io) | Err(RdErr::Name) => {
                    return done_c1(&root, opt, "unknown", "io", (0, 0), &clock, (0, "-"));
                }
                Err(err) => return done_c1(&root, opt, "failed", integrity_reason(err), (0, 1), &clock, (0, "-")),
            }
        }
        if missing {
            if !wait_more(opt.end) {
                return done_c1(&root, opt, "unknown", "deadline", (frames.len() as u64, 0), &clock, (0, "-"));
            }
            continue;
        }
        let digest = digest_of(&frames);
        match root.read_file(&format!("summary-{other}"), 4096) {
            Err(RdErr::Absent) => {
                if !wait_more(opt.end) {
                    return done_c1(&root, opt, "unknown", "no-peer-summary", (frames.len() as u64, 0), &clock, (0, digest.as_str()));
                }
            }
            Err(RdErr::Io) | Err(RdErr::Name) => {
                return done_c1(&root, opt, "unknown", "io", (frames.len() as u64, 0), &clock, (0, digest.as_str()));
            }
            Err(err) => {
                let reason = integrity_reason(err);
                return done_c1(&root, opt, "failed", reason, (frames.len() as u64, 0), &clock, (0, digest.as_str()));
            }
            Ok((bytes, o, r)) => {
                clock.note_read(o, r);
                return judge_summary(&root, opt, &clock, &frames, &digest, &bytes, other);
            }
        }
    }
}
fn judge_summary(root: &Dir, opt: &Opt, clock: &Clock, frames: &[[u8; FRAME_LEN]], digest: &str, bytes: &[u8], other: &str) -> i32 {
    let text = std::str::from_utf8(bytes).unwrap_or("");
    let digest_ok = field(text, "digest") == Some(digest);
    let shape_ok = field(text, "v") == Some("1") && field(text, "op") == Some("c1") && field(text, "role") == Some(other) && field(text, "verdict") == Some("ok") && field(text, "n") == Some(&opt.count.to_string());
    if shape_ok && digest_ok {
        done_c1(root, opt, "ok", "-", (frames.len() as u64, 0), clock, (1, digest))
    } else {
        done_c1(root, opt, "failed", "bad-record", (frames.len() as u64, 0), clock, (0, digest))
    }
}
fn c2_guest(opt: &Opt) -> i32 {
    let root = match open_root(opt) {
        Ok(dir) => dir,
        Err(code) => return code,
    };
    let data = match child_dir(&root, "data") {
        Ok(dir) => dir,
        Err(_) => return unk(opt, &root, "io"),
    };
    let planned = u64::from(opt.count);
    let mut attempted = 0u64;
    let mut renamed = 0u64;
    for seq in 1..=opt.count {
        if io::past(opt.end) {
            return unk(opt, &root, "deadline");
        }
        let bytes = frame::frame(2, 2, seq);
        let last = seq == opt.count;
        let step = if !last {
            data.publish(&format!("f-{seq}.tmp"), &format!("f-{seq}"), &bytes, true)
        } else if opt.boundary == "file-sync" {
            // File fsync only. No rename and no fsync of `data`.
            data.write_fsync("part", &bytes)
        } else {
            data.publish(&format!("f-{seq}.tmp"), &format!("f-{seq}"), &bytes, opt.boundary == "dir-sync")
        };
        match step {
            Ok(_) => {
                attempted += 1;
                if !last || opt.boundary != "file-sync" {
                    renamed += 1;
                }
            }
            Err(_) => return unk(opt, &root, "io"),
        }
    }
    let dir_synced = if opt.boundary == "dir-sync" { "1" } else { "0" };
    let part = if opt.boundary == "file-sync" { "ok" } else { "absent" };
    let line = c2_line(opt.role, opt.boundary, "not-ack", "-", [planned, attempted, renamed, renamed], part, dir_synced);
    let evidence = match child_dir(&root, "evidence") {
        Ok(dir) => dir,
        Err(_) => return unk(opt, &root, "io"),
    };
    if store(&evidence, "progress", &line).is_err() {
        return unk(opt, &root, "io");
    }
    finish(&root, &slot(opt), &line, "not-ack")
}
fn c2_host(opt: &Opt) -> i32 {
    let root = match open_root(opt) {
        Ok(dir) => dir,
        Err(code) => return code,
    };
    if io::past(opt.end) {
        return unk(opt, &root, "deadline");
    }
    let data = match child_dir(&root, "data") {
        Ok(dir) => dir,
        Err(_) => return unk(opt, &root, "io"),
    };
    let mut visible = 0u64;
    for seq in 1..=opt.count {
        match data.read_file(&format!("f-{seq}"), FRAME_LEN) {
            Ok((bytes, _, _)) => match frame::classify(&bytes) {
                Ok((2, 2, s)) if s == seq => visible += 1,
                _ => return c2_done(&root, opt, "failed", "torn", visible, "absent"),
            },
            Err(RdErr::Absent) => {}
            Err(RdErr::Io) | Err(RdErr::Name) => return c2_done(&root, opt, "unknown", "io", visible, "absent"),
            Err(err) => return c2_done(&root, opt, "failed", integrity_reason(err), visible, "absent"),
        }
    }
    let part_state = match data.read_file("part", FRAME_LEN) {
        Err(RdErr::Absent) => "absent",
        Ok((bytes, _, _)) => match frame::classify(&bytes) {
            Ok((2, 2, s)) if s == opt.count => "ok",
            _ => "bad",
        },
        Err(RdErr::Io) | Err(RdErr::Name) => return c2_done(&root, opt, "unknown", "io", visible, "absent"),
        Err(err) => return c2_done(&root, opt, "failed", integrity_reason(err), visible, "absent"),
    };
    if part_state == "bad" {
        return c2_done(&root, opt, "failed", "bad-part", visible, "bad");
    }
    let evidence = match child_dir(&root, "evidence") {
        Ok(dir) => dir,
        Err(_) => return c2_done(&root, opt, "unknown", "io", visible, part_state),
    };
    let progress = match evidence.read_file("progress", 4096) {
        Ok((bytes, _, _)) => match std::str::from_utf8(&bytes) {
            Ok(text) => text.to_string(),
            Err(_) => return c2_done(&root, opt, "unknown", "bad-record", visible, part_state),
        },
        Err(RdErr::Absent) => return c2_done(&root, opt, "unknown", "no-peer-summary", visible, part_state),
        Err(RdErr::Io) | Err(RdErr::Name) => return c2_done(&root, opt, "unknown", "io", visible, part_state),
        Err(err) => return c2_done(&root, opt, "failed", integrity_reason(err), visible, part_state),
    };
    if !c2_progress_ok(&progress, opt, visible, part_state) {
        return c2_done(&root, opt, "unknown", "bad-record", visible, part_state);
    }
    c2_done(&root, opt, "not-ack", "-", visible, part_state)
}
fn c2_progress_ok(text: &str, opt: &Opt, visible: u64, part_state: &str) -> bool {
    let want_renamed = if opt.boundary == "file-sync" { u64::from(opt.count - 1) } else { u64::from(opt.count) };
    let want_part = if opt.boundary == "file-sync" { "ok" } else { "absent" };
    let want_dir = if opt.boundary == "dir-sync" { "1" } else { "0" };
    field(text, "v") == Some("1")
        && field(text, "op") == Some("c2")
        && field(text, "boundary") == Some(opt.boundary)
        && field(text, "verdict") == Some("not-ack")
        && field(text, "planned") == Some(&opt.count.to_string())
        && field(text, "attempted") == Some(&opt.count.to_string())
        && field(text, "renamed") == Some(&want_renamed.to_string())
        && field(text, "dir_synced") == Some(want_dir)
        && part_state == want_part
        && visible == want_renamed
}
fn c2_done(root: &Dir, opt: &Opt, verdict: &str, reason: &str, visible: u64, part: &str) -> i32 {
    let dir_synced = if verdict != "not-ack" {
        "-"
    } else if opt.boundary == "dir-sync" {
        "1"
    } else {
        "0"
    };
    let line = c2_line("host", opt.boundary, verdict, reason, [u64::from(opt.count), u64::from(opt.count), visible, visible], part, dir_synced);
    finish(root, "result-host", &line, verdict)
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
    let (verdict, reason) = judge_c8(n);
    let line = c8_line("guest", verdict, reason, u64::try_from(n.unwrap_or(0)).unwrap_or(0));
    finish(&root, "result-guest", &line, verdict)
}
fn c8_host(opt: &Opt) -> i32 {
    let root = match open_root(opt) {
        Ok(dir) => dir,
        Err(code) => return code,
    };
    if io::past(opt.end) {
        return unk(opt, &root, "deadline");
    }
    match root.read_file("result-guest", 4096) {
        Err(RdErr::Absent) => unk(opt, &root, "no-peer-summary"),
        Err(RdErr::Io) | Err(RdErr::Name) => unk(opt, &root, "io"),
        Err(_) => finish(&root, "result-host", &c8_line("host", "failed", "nofollow", 0), "failed"),
        Ok((bytes, _, _)) => {
            let text = std::str::from_utf8(&bytes).unwrap_or("");
            let n = field(text, "n").and_then(|s| s.parse().ok()).unwrap_or(0);
            let policy = field(text, "op") != Some("c8") || field(text, "ops") != Some("0") || field(text, "created") != Some("0") || field(text, "elevated") != Some("0") || field(text, "verdict") != Some("unknown");
            if policy {
                return finish(&root, "result-host", &c8_line("host", "failed", "subject-policy", n), "failed");
            }
            let reason = field(text, "reason").unwrap_or("bad-record");
            finish(&root, "result-host", &c8_line("host", "unknown", reason, n), "unknown")
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
fn bounded_out(argv: &[&str], end: Instant) -> Option<String> {
    if io::past(end) {
        return None;
    }
    let mut child = Command::new(argv[0]).args(&argv[1..]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    loop {
        if io::past(end) {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                let mut stdout = child.stdout.take()?;
                let mut out = String::new();
                let mut buf = [0u8; 4096];
                loop {
                    if out.len() > 65536 {
                        return None;
                    }
                    match stdout.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => out.push_str(&String::from_utf8_lossy(&buf[..n])),
                        Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => return None,
                    }
                }
                return Some(out);
            }
            Ok(Some(_)) => return None,
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(_) => return None,
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

    #[test]
    fn deadline_writes_unknown_without_invented_times() {
        let dir = scratch("dead");
        let code = run(&dir, &["writer", "--dir", "DIR", "--role", "guest", "--count", "8", "--timeout", "0"]);
        assert_eq!(code, 1);
        let line = text(&dir, "result-guest");
        assert!(line.contains("verdict=unknown"), "{line}");
        assert!(line.contains("reason=deadline"), "{line}");
        assert!(line.contains("open_us=-"), "{line}");
        assert!(line.contains("write_us=-"), "{line}");
        assert!(!line.contains("verdict=ok"), "{line}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reader_needs_peer_summary_and_torn_bytes_stay_failed() {
        let dir = scratch("c1");
        assert_eq!(run(&dir, &["writer", "--dir", "DIR", "--role", "guest", "--count", "1", "--timeout", "2"]), 0);
        assert_eq!(run(&dir, &["writer", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 0);
        assert_eq!(run(&dir, &["reader", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 0);
        let ok = text(&dir, "result-host");
        assert!(ok.contains("verdict=ok") && ok.contains("peer=1"), "{ok}");
        fs::remove_file(dir.join("summary-guest")).unwrap();
        assert_ne!(run(&dir, &["reader", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "1"]), 0);
        let missing = text(&dir, "result-host");
        assert!(missing.contains("reason=no-peer-summary"), "{missing}");
        assert!(!missing.contains("verdict=ok"), "{missing}");
        let mut bytes = fs::read(dir.join("guest-write/f-1")).unwrap();
        bytes[80] ^= 0xff;
        fs::write(dir.join("guest-write/f-1"), bytes).unwrap();
        fs::write(dir.join("summary-guest"), b"v=1 op=c1 role=guest verdict=ok n=1 digest=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa peer=0").unwrap();
        assert_ne!(run(&dir, &["reader", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 0);
        let torn = text(&dir, "result-host");
        assert!(torn.contains("verdict=failed") && torn.contains("reason=torn"), "{torn}");
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
        assert!(progress.contains("renamed=2"), "{progress}");
        assert!(progress.contains("dir_synced=0"), "{progress}");
        assert!(progress.contains("verdict=not-ack"), "{progress}");
        assert_eq!(run(&dir, &["c2", "file-sync", "--dir", "DIR", "--role", "host", "--count", "3", "--timeout", "2"]), 0);
        let host = text(&dir, "result-host");
        assert!(host.contains("verdict=not-ack"), "{host}");
        fs::write(dir.join("data/part"), frame::frame(2, 2, 1)).unwrap();
        assert_ne!(run(&dir, &["c2", "file-sync", "--dir", "DIR", "--role", "host", "--count", "3", "--timeout", "2"]), 0);
        let bad = text(&dir, "result-host");
        assert!(bad.contains("verdict=failed") && bad.contains("reason=bad-part"), "{bad}");
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
        fs::write(dir.join("result-guest"), b"v=1 op=c8 role=guest verdict=unknown reason=single-principal n=1 ops=0 created=0 elevated=0").unwrap();
        assert_ne!(run(&dir, &["c8", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 0);
        let line = text(&dir, "result-host");
        assert!(line.contains("reason=single-principal"), "{line}");
        assert!(line.contains("ops=0") && line.contains("created=0") && line.contains("elevated=0"), "{line}");
        fs::write(dir.join("result-guest"), b"v=1 op=c8 role=guest verdict=unknown reason=single-principal n=1 ops=0 created=1 elevated=0").unwrap();
        fs::remove_file(dir.join("result-host")).unwrap();
        assert_ne!(run(&dir, &["c8", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 0);
        let bad = text(&dir, "result-host");
        assert!(bad.contains("verdict=failed") && bad.contains("reason=subject-policy"), "{bad}");
        let _ = fs::remove_dir_all(&dir);
    }
}

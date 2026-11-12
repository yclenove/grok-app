use crate::frame::{self, FRAME_LEN};
use crate::io::{self, Dir, RdErr, Times};
use crate::sha256::hex_digest;
use std::io::{self as stdio, ErrorKind};
use std::os::unix::io::AsRawFd;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const C1_KEYS: &[&str] =
    &["v", "op", "role", "verdict", "reason", "n", "torn", "lost", "integrity", "latency", "vis_us", "rtt_us", "slice_us", "w_slice_us", "open_us", "write_us", "fsync_us", "rename_us", "dir_sync_us", "read_us", "peer", "digest", "gate"];
const ROUND_WAIT: Duration = Duration::from_secs(3);
const SLOW_US: u64 = 2_000_000;
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
fn peer_role(role: &str) -> &'static str {
    if role == "host" {
        "guest"
    } else {
        "host"
    }
}
fn c1_line(role: &str, verdict: &str, reason: &str, counts: (u64, u64), marks: (&str, &str, &str, &str, &str, &str), clock: &Clock, tail: (u8, &str)) -> String {
    let (n, torn) = counts;
    let (integrity, latency, w_slice, vis, rtt, gate) = marks;
    let (peer, digest) = tail;
    format!(
        "v=1 op=c1 role={role} verdict={verdict} reason={reason} n={n} torn={torn} lost=- integrity={integrity} latency={latency} vis_us={vis} rtt_us={rtt} slice_us={} w_slice_us={w_slice} open_us={} write_us={} fsync_us={} rename_us={} dir_sync_us={} read_us={} peer={peer} digest={digest} gate={gate}",
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
    c1_line(opt.role, "unknown", reason, (n, torn), (integrity, "not_measured", "-", "-", "hg=n=0;gh=n=0", "-"), clock, (0, "-"))
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
struct Exch {
    clock: Clock,
    samples: Vec<u64>,
    frames: Vec<[u8; FRAME_LEN]>,
}
struct Fail {
    exch: Exch,
    stop: Stop,
}
enum Stop {
    Deadline,
    Timeout,
    Torn,
    Replaced,
    Nofollow,
    BadPong,
    Bad,
    Io,
}
fn rank_of(n: usize, num: u64, den: u64) -> usize {
    let rank = (num * n as u64).div_ceil(den);
    rank.max(1) as usize - 1
}
fn fmt_samples(samples: &[u64]) -> String {
    if samples.is_empty() {
        return "n=0".to_string();
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let n = sorted.len();
    let p50 = sorted[rank_of(n, 1, 2)];
    if n >= 20 {
        format!("n={n},min={},p50={p50},max={},p99={}", sorted[0], sorted[n - 1], sorted[rank_of(n, 99, 100)])
    } else {
        format!("n={n},min={},p50={p50},max={}", sorted[0], sorted[n - 1])
    }
}
fn fmt_rtt(hg: &[u64], gh: &[u64]) -> String {
    format!("hg={};gh={}", fmt_samples(hg), fmt_samples(gh))
}
fn vis_of(hg: &[u64], gh: &[u64]) -> &'static str {
    if hg.is_empty() && gh.is_empty() {
        "-"
    } else {
        "≤rtt"
    }
}
fn gate_of(hg: &[u64], gh: &[u64]) -> &'static str {
    if hg.iter().chain(gh).copied().any(|v| v >= SLOW_US) {
        "exceeds-2s"
    } else {
        "-"
    }
}
fn empty_exch() -> Exch {
    Exch { clock: Clock::new(), samples: Vec::new(), frames: Vec::new() }
}
fn kv_num(tok: Option<&str>, key: &str) -> bool {
    tok.and_then(|t| t.strip_prefix(key)).and_then(|t| t.strip_prefix('=')).is_some_and(numeric)
}
fn part_ok(part: &str, n: u64) -> bool {
    if n == 0 {
        return part == "n=0";
    }
    let expect = format!("n={n}");
    let mut it = part.split(',');
    let head = it.next();
    let min = it.next();
    let p50 = it.next();
    let max = it.next();
    let extra = it.next();
    let tail_ok = if n >= 20 { kv_num(extra, "p99") && it.next().is_none() } else { extra.is_none() };
    head == Some(expect.as_str()) && kv_num(min, "min") && kv_num(p50, "p50") && kv_num(max, "max") && tail_ok
}
fn part_max(part: &str) -> Option<u64> {
    if part == "n=0" {
        return Some(0);
    }
    let max = part.split(',').find_map(|kv| kv.strip_prefix("max="))?;
    if numeric(max) {
        max.parse().ok()
    } else {
        None
    }
}
fn dir_part<'a>(rtt: &'a str, tag: &str) -> Option<&'a str> {
    rtt.split(';').find_map(|part| part.strip_prefix(tag)?.strip_prefix('='))
}
fn side_of(direction: u8, samples: &[u64]) -> (Vec<u64>, Vec<u64>) {
    if direction == 1 {
        (samples.to_vec(), Vec::new())
    } else {
        (Vec::new(), samples.to_vec())
    }
}
fn as_deadline(text: &str) -> String {
    text.split(' ')
        .map(|tok| {
            if tok.starts_with("verdict=") {
                "verdict=unknown"
            } else if tok.starts_with("reason=") {
                "reason=deadline"
            } else if tok.starts_with("latency=") {
                "latency=not_measured"
            } else {
                tok
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
fn merge_clock(dst: &mut Clock, src: &Clock) {
    if let Some(v) = src.open_us {
        bump(&mut dst.open_us, v);
    }
    if let Some(v) = src.write_us {
        bump(&mut dst.write_us, v);
    }
    if let Some(v) = src.fsync_us {
        bump(&mut dst.fsync_us, v);
    }
    if let Some(v) = src.rename_us {
        bump(&mut dst.rename_us, v);
    }
    if let Some(v) = src.dir_sync_us {
        bump(&mut dst.dir_sync_us, v);
    }
    if let Some(v) = src.read_us {
        bump(&mut dst.read_us, v);
    }
    if let Some(v) = src.slice_us {
        bump(&mut dst.slice_us, v);
    }
}
fn absorb_nums(clock: &mut Clock, rec: &[(&str, &str)]) {
    let take = |key| field(rec, key).parse::<u64>().ok().filter(|_| numeric(field(rec, key)));
    if let Some(v) = take("open_us") {
        bump(&mut clock.open_us, v);
    }
    if let Some(v) = take("write_us") {
        bump(&mut clock.write_us, v);
    }
    if let Some(v) = take("fsync_us") {
        bump(&mut clock.fsync_us, v);
    }
    if let Some(v) = take("rename_us") {
        bump(&mut clock.rename_us, v);
    }
    if let Some(v) = take("dir_sync_us") {
        bump(&mut clock.dir_sync_us, v);
    }
    if let Some(v) = take("read_us") {
        bump(&mut clock.read_us, v);
    }
    if let Some(v) = take("slice_us") {
        bump(&mut clock.slice_us, v);
    }
}
fn clocks_ready(clock: &Clock) -> bool {
    clock.open_us.is_some() && clock.write_us.is_some() && clock.fsync_us.is_some() && clock.rename_us.is_some() && clock.dir_sync_us.is_some() && clock.read_us.is_some() && clock.slice_us.is_some()
}
fn stop_of(err: RdErr, round_end: Option<Instant>, end: Instant) -> Stop {
    match err {
        RdErr::Deadline if round_end.is_some_and(|t| Instant::now() >= t) && !io::past(end) => Stop::Timeout,
        RdErr::Deadline => Stop::Deadline,
        RdErr::Replaced => Stop::Replaced,
        RdErr::Symlink | RdErr::NotFile | RdErr::HardLink | RdErr::Size => Stop::Nofollow,
        RdErr::Io | RdErr::Name | RdErr::Absent => Stop::Io,
    }
}
fn wait_bytes(dir: &Dir, name: &str, end: Instant, round: Option<Duration>, clock: &mut Clock, mut good: impl FnMut(&[u8]) -> Result<bool, Stop>) -> Result<Vec<u8>, Stop> {
    let round_end = round.map(|d| Instant::now() + d);
    loop {
        if io::past(end) {
            return Err(Stop::Deadline);
        }
        let limit = round_end.filter(|t| *t < end).unwrap_or(end);
        if Instant::now() >= limit {
            return Err(if round_end.is_some() && !io::past(end) { Stop::Timeout } else { Stop::Deadline });
        }
        match dir.read_file(name, FRAME_LEN, limit) {
            Ok((bytes, open_us, read_us, slice_us)) => {
                clock.note_read(open_us, read_us, slice_us);
                match good(&bytes) {
                    Ok(true) => return Ok(bytes),
                    Ok(false) => {
                        if !wait_more(limit) {
                            return Err(if round_end.is_some() && !io::past(end) { Stop::Timeout } else { Stop::Deadline });
                        }
                    }
                    Err(stop) => return Err(stop),
                }
            }
            Err(RdErr::Absent) => {
                if !wait_more(limit) {
                    return Err(if round_end.is_some() && !io::past(end) { Stop::Timeout } else { Stop::Deadline });
                }
            }
            Err(err) => return Err(stop_of(err, round_end, end)),
        }
    }
}
fn accept_ping(bytes: &[u8], direction: u8, seq: u8) -> Result<bool, Stop> {
    if bytes.len() != FRAME_LEN {
        return Ok(false);
    }
    match frame::classify(bytes) {
        Ok((1, dir, s)) if dir == direction && s == seq => Ok(true),
        Ok(_) => Err(Stop::Bad),
        Err(()) => Err(Stop::Torn),
    }
}
fn accept_pong(bytes: &[u8], seq: u8, ping: &[u8]) -> Result<bool, Stop> {
    if bytes.len() == FRAME_LEN {
        return match frame::classify(bytes) {
            Ok(_) => Ok(false),
            Err(()) => Err(Stop::Torn),
        };
    }
    let Some(text) = rec_text(bytes) else {
        return Ok(false);
    };
    let want = format!("v=1 op=pong seq={seq} digest={}", hex_digest(ping));
    if text == want {
        return Ok(true);
    }
    if text.starts_with("v=1 op=pong seq=") && text.contains(" digest=") {
        Err(Stop::BadPong)
    } else {
        Ok(false)
    }
}
fn pong_bytes(seq: u8, ping: &[u8]) -> Vec<u8> {
    format!("v=1 op=pong seq={seq} digest={}\n", hex_digest(ping)).into_bytes()
}
fn fail_at(exch: Exch, stop: Stop) -> Box<Fail> {
    Box::new(Fail { exch, stop })
}
fn drive(ping_dir: &Dir, pong_dir: &Dir, direction: u8, count: u8, end: Instant) -> Result<Exch, Box<Fail>> {
    let mut exch = empty_exch();
    for seq in 1..=count {
        if io::past(end) {
            return Err(fail_at(exch, Stop::Deadline));
        }
        let ping = frame::frame(1, direction, seq);
        match ping_dir.publish(&format!("f-{seq}.tmp"), &format!("f-{seq}"), &ping, true, end) {
            Ok(t) => {
                exch.clock.note(&t);
                if io::past(end) {
                    return Err(fail_at(exch, Stop::Deadline));
                }
            }
            Err(RdErr::Deadline) => return Err(fail_at(exch, Stop::Deadline)),
            Err(RdErr::Replaced) => return Err(fail_at(exch, Stop::Replaced)),
            Err(_) => return Err(fail_at(exch, Stop::Io)),
        }
        let t0 = Instant::now();
        let waited = wait_bytes(pong_dir, &format!("f-{seq}"), end, Some(ROUND_WAIT), &mut exch.clock, |bytes| accept_pong(bytes, seq, &ping));
        match waited {
            Ok(_) => {
                exch.samples.push(io::us(t0.elapsed()));
                exch.frames.push(ping);
            }
            Err(stop) => return Err(fail_at(exch, stop)),
        }
    }
    Ok(exch)
}
fn respond(ping_dir: &Dir, pong_dir: &Dir, direction: u8, count: u8, end: Instant) -> Result<Exch, Box<Fail>> {
    let mut exch = empty_exch();
    for seq in 1..=count {
        if io::past(end) {
            return Err(fail_at(exch, Stop::Deadline));
        }
        let ping = match wait_bytes(ping_dir, &format!("f-{seq}"), end, None, &mut exch.clock, |bytes| accept_ping(bytes, direction, seq)) {
            Ok(bytes) => bytes,
            Err(stop) => return Err(fail_at(exch, stop)),
        };
        if io::past(end) {
            return Err(fail_at(exch, Stop::Deadline));
        }
        let body = pong_bytes(seq, &ping);
        match pong_dir.publish(&format!("f-{seq}.tmp"), &format!("f-{seq}"), &body, true, end) {
            Ok(t) => {
                exch.clock.note(&t);
                let mut buf = [0u8; FRAME_LEN];
                buf.copy_from_slice(&ping);
                exch.frames.push(buf);
                if io::past(end) {
                    return Err(fail_at(exch, Stop::Deadline));
                }
            }
            Err(RdErr::Deadline) => return Err(fail_at(exch, Stop::Deadline)),
            Err(RdErr::Replaced) => return Err(fail_at(exch, Stop::Replaced)),
            Err(_) => return Err(fail_at(exch, Stop::Io)),
        }
    }
    Ok(exch)
}
fn stop_view(stop: &Stop, n: u64) -> (&'static str, &'static str, u64, &'static str) {
    let integrity = if n > 0 { "ok" } else { "-" };
    match stop {
        Stop::Deadline => ("unknown", "deadline", 0, integrity),
        Stop::Timeout => ("unknown", "timeout", 0, integrity),
        Stop::Torn => ("failed", "torn", 1, "bad"),
        Stop::Replaced => ("failed", "replaced", 1, "bad"),
        Stop::Nofollow => ("failed", "nofollow", 1, "bad"),
        Stop::BadPong => ("failed", "bad-pong", 0, "bad"),
        Stop::Bad => ("failed", "bad-record", 0, "bad"),
        Stop::Io => ("unknown", "io", 0, integrity),
    }
}
#[allow(clippy::too_many_arguments)]
fn emit(root: &Dir, opt: &Opt, verdict: &str, reason: &str, counts: (u64, u64), integrity: &str, latency: &str, clock: &Clock, peer: u8, digest: &str, rtt: &str, vis: &str, gate: &str, w_slice: &str, summary: bool) -> i32 {
    let (n, torn) = counts;
    let line = c1_line(opt.role, verdict, reason, (n, torn), (integrity, latency, w_slice, vis, rtt, gate), clock, (peer, digest));
    let late = as_deadline(&line);
    if summary && verdict == "ok" && (io::past(opt.end) || store(root, &format!("summary-{}", opt.role), &line, Some(opt.end)).is_err() || io::past(opt.end)) {
        return finish(root, &slot(opt), &late, "unknown", opt.end, &late);
    }
    if summary && verdict != "ok" {
        let _ = store(root, &format!("summary-{}", opt.role), &line, None);
    }
    finish(root, &slot(opt), &line, verdict, opt.end, &late)
}
#[allow(clippy::too_many_arguments)]
fn emit_samples(root: &Dir, opt: &Opt, verdict: &str, reason: &str, torn: u64, integrity: &str, latency: &str, clock: &Clock, peer: u8, digest: &str, hg: &[u64], gh: &[u64], w_slice: &str, summary: bool) -> i32 {
    let n = if hg.is_empty() { gh.len() as u64 } else { hg.len() as u64 };
    emit(root, opt, verdict, reason, (n, torn), integrity, latency, clock, peer, digest, &fmt_rtt(hg, gh), vis_of(hg, gh), gate_of(hg, gh), w_slice, summary)
}
fn emit_fail(root: &Dir, opt: &Opt, fail: Fail, direction: u8, summary: bool) -> i32 {
    let (verdict, reason, torn, integrity) = stop_view(&fail.stop, fail.exch.samples.len() as u64);
    let digest = if fail.exch.frames.is_empty() { "-".to_string() } else { digest_of(&fail.exch.frames) };
    let peer = u8::from(!fail.exch.samples.is_empty());
    let (hg, gh) = side_of(direction, &fail.exch.samples);
    emit_samples(root, opt, verdict, reason, torn, integrity, "not_measured", &fail.exch.clock, peer, &digest, &hg, &gh, "-", summary)
}
fn emit_phase_ok(root: &Dir, opt: &Opt, exch: Exch, direction: u8) -> i32 {
    let digest = digest_of(&exch.frames);
    let (hg, gh) = side_of(direction, &exch.samples);
    let n = exch.samples.len() as u64;
    emit(root, opt, "ok", "-", (n, 0), "ok", "not_measured", &exch.clock, 1, &digest, &fmt_rtt(&hg, &gh), vis_of(&hg, &gh), gate_of(&hg, &gh), "-", true)
}
fn phase_ok(rec: &[(&str, &str)], role: &str, count: u8, side: &str) -> bool {
    let Some(part) = dir_part(field(rec, "rtt_us"), side) else {
        return false;
    };
    field(rec, "v") == "1"
        && field(rec, "op") == "c1"
        && field(rec, "role") == role
        && field(rec, "verdict") == "ok"
        && field(rec, "torn") == "0"
        && field(rec, "integrity") == "ok"
        && field(rec, "n") == count.to_string()
        && field(rec, "peer") == "1"
        && part_ok(part, u64::from(count))
        && numeric(field(rec, "slice_us"))
}
fn open_pair(opt: &Opt) -> Result<(Dir, Dir, Dir), i32> {
    let root = open_root(opt)?;
    let mine = match child_dir(&root, leaf(opt.role)) {
        Ok(dir) => dir,
        Err(RdErr::Replaced) => return Err(emit(&root, opt, "failed", "replaced", (0, 1), "bad", "not_measured", &Clock::new(), 0, "-", "hg=n=0;gh=n=0", "-", "-", "-", false)),
        Err(_) => return Err(unk(opt, &root, "io")),
    };
    let theirs = match child_dir(&root, leaf(peer_role(opt.role))) {
        Ok(dir) => dir,
        Err(RdErr::Replaced) => return Err(emit(&root, opt, "failed", "replaced", (0, 1), "bad", "not_measured", &Clock::new(), 0, "-", "hg=n=0;gh=n=0", "-", "-", "-", false)),
        Err(_) => return Err(unk(opt, &root, "io")),
    };
    Ok((root, mine, theirs))
}
fn c1_writer(opt: &Opt) -> i32 {
    let (root, mine, theirs) = match open_pair(opt) {
        Ok(dirs) => dirs,
        Err(code) => return code,
    };
    if opt.role == "host" {
        match drive(&mine, &theirs, 1, opt.count, opt.end) {
            Ok(exch) => emit_phase_ok(&root, opt, exch, 1),
            Err(fail) => emit_fail(&root, opt, *fail, 1, true),
        }
    } else {
        match respond(&theirs, &mine, 1, opt.count, opt.end) {
            Ok(exch) => {
                let digest = digest_of(&exch.frames);
                emit(&root, opt, "ok", "-", (u64::from(opt.count), 0), "ok", "not_measured", &exch.clock, 1, &digest, "hg=n=0;gh=n=0", "-", "-", "-", false)
            }
            Err(fail) => emit_fail(&root, opt, *fail, 1, false),
        }
    }
}
fn load_summary(root: &Dir, name: &str, end: Instant, clock: &mut Clock) -> Result<Vec<u8>, Stop> {
    wait_bytes(root, name, end, None, clock, |_| Ok(true))
}
fn c1_guest_reader(opt: &Opt, root: &Dir, mine: &Dir, theirs: &Dir) -> i32 {
    let mut clock = Clock::new();
    let bytes = match load_summary(root, "summary-host", opt.end, &mut clock) {
        Ok(bytes) => bytes,
        Err(stop) => return emit_fail(root, opt, *fail_at(Exch { clock, ..empty_exch() }, stop), 2, true),
    };
    let Some(text) = rec_text(&bytes) else {
        return emit(root, opt, "failed", "bad-record", (0, 0), "bad", "not_measured", &clock, 0, "-", "hg=n=0;gh=n=0", "-", "-", "-", true);
    };
    let Some(rec) = decode(text, C1_KEYS) else {
        return emit(root, opt, "failed", "bad-record", (0, 0), "bad", "not_measured", &clock, 0, "-", "hg=n=0;gh=n=0", "-", "-", "-", true);
    };
    if !phase_ok(&rec, "host", opt.count, "hg") {
        return emit(root, opt, "unknown", safe_reason(field(&rec, "reason")), (0, 0), "-", "not_measured", &clock, 0, "-", "hg=n=0;gh=n=0", "-", "-", "-", true);
    }
    match drive(mine, theirs, 2, opt.count, opt.end) {
        Ok(exch) => emit_phase_ok(root, opt, exch, 2),
        Err(fail) => emit_fail(root, opt, *fail, 2, true),
    }
}
fn safe_reason(reason: &str) -> &str {
    if !reason.is_empty() && reason.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        reason
    } else {
        "bad-record"
    }
}
fn gate_parts(hg: &str, gh: &str) -> &'static str {
    if part_max(hg).unwrap_or(0).max(part_max(gh).unwrap_or(0)) >= SLOW_US {
        "exceeds-2s"
    } else {
        "-"
    }
}
fn side_part(bytes: &[u8], role: &str, side: &str) -> Option<String> {
    let rec = decode(rec_text(bytes)?, C1_KEYS)?;
    if field(&rec, "op") != "c1" || field(&rec, "role") != role {
        return None;
    }
    let part = dir_part(field(&rec, "rtt_us"), side)?;
    part_max(part).map(|_| part.to_string())
}
fn guest_gh(root: &Dir, end: Instant, clock: &mut Clock) -> String {
    let limit = (Instant::now() + ROUND_WAIT).min(end);
    let ready = if io::past(limit) { None } else { load_summary(root, "summary-guest", limit, clock).ok() };
    let bytes = ready.or_else(|| {
        let grace = Instant::now() + Duration::from_millis(250);
        root.read_file("summary-guest", 4096, grace).ok().map(|item| item.0)
    });
    bytes.as_deref().and_then(|raw| side_part(raw, "guest", "gh")).unwrap_or_else(|| "n=0".to_string())
}
fn republish(root: &Dir, opt: &Opt, text: &str, verdict: &str) -> i32 {
    let late = as_deadline(text);
    finish(root, &slot(opt), text, verdict, opt.end, &late)
}
fn c1_host_reader(opt: &Opt, root: &Dir, mine: &Dir, theirs: &Dir) -> i32 {
    let mut clock = Clock::new();
    let bytes = match load_summary(root, "summary-host", opt.end, &mut clock) {
        Ok(bytes) => bytes,
        Err(stop) => return emit_fail(root, opt, *fail_at(Exch { clock, ..empty_exch() }, stop), 1, false),
    };
    let Some(text) = rec_text(&bytes) else {
        return emit(root, opt, "failed", "bad-record", (0, 0), "bad", "not_measured", &clock, 0, "-", "hg=n=0;gh=n=0", "-", "-", "-", false);
    };
    let Some(rec) = decode(text, C1_KEYS) else {
        return emit(root, opt, "failed", "bad-record", (0, 0), "bad", "not_measured", &clock, 0, "-", "hg=n=0;gh=n=0", "-", "-", "-", false);
    };
    if field(&rec, "verdict") == "failed" {
        return republish(root, opt, text, "failed");
    }
    if !phase_ok(&rec, "host", opt.count, "hg") {
        if field(&rec, "op") == "c1" && field(&rec, "role") == "host" {
            return republish(root, opt, text, "unknown");
        }
        return emit(root, opt, "failed", "bad-record", (0, 0), "bad", "not_measured", &clock, 0, "-", "hg=n=0;gh=n=0", "-", "-", "-", false);
    }
    absorb_nums(&mut clock, &rec);
    let hg_part = dir_part(field(&rec, "rtt_us"), "hg").unwrap_or("n=0").to_string();
    let digest = field(&rec, "digest").to_string();
    match respond(theirs, mine, 2, opt.count, opt.end) {
        Err(fail) => {
            merge_clock(&mut clock, &fail.exch.clock);
            let (verdict, reason, torn, integrity) = stop_view(&fail.stop, u64::from(opt.count));
            let gh_part = guest_gh(root, opt.end, &mut clock);
            let rtt = format!("hg={hg_part};gh={gh_part}");
            return emit(root, opt, verdict, reason, (u64::from(opt.count), torn), integrity, "not_measured", &clock, 1, &digest, &rtt, "≤rtt", gate_parts(&hg_part, &gh_part), "-", false);
        }
        Ok(exch) => merge_clock(&mut clock, &exch.clock),
    }
    let guest = match load_summary(root, "summary-guest", opt.end, &mut clock) {
        Ok(bytes) => bytes,
        Err(_) => {
            return emit(root, opt, "unknown", "no-peer-summary", (u64::from(opt.count), 0), "ok", "not_measured", &clock, 1, &digest, &format!("hg={hg_part};gh=n=0"), "≤rtt", gate_parts(&hg_part, "n=0"), "-", false);
        }
    };
    let Some(gtext) = rec_text(&guest) else {
        return emit(root, opt, "failed", "bad-record", (u64::from(opt.count), 0), "bad", "not_measured", &clock, 1, &digest, &format!("hg={hg_part};gh=n=0"), "≤rtt", gate_parts(&hg_part, "n=0"), "-", false);
    };
    let Some(grec) = decode(gtext, C1_KEYS) else {
        return emit(root, opt, "failed", "bad-record", (u64::from(opt.count), 0), "bad", "not_measured", &clock, 1, &digest, &format!("hg={hg_part};gh=n=0"), "≤rtt", gate_parts(&hg_part, "n=0"), "-", false);
    };
    if !phase_ok(&grec, "guest", opt.count, "gh") {
        let gh_kept = side_part(&guest, "guest", "gh").unwrap_or_else(|| "n=0".to_string());
        let verdict = field(&grec, "verdict");
        let (verdict, reason) = if verdict == "failed" || verdict == "unknown" { (verdict, safe_reason(field(&grec, "reason"))) } else { ("unknown", "bad-record") };
        let torn = u64::from(field(&grec, "torn") != "0");
        let integrity = if verdict != "failed" && field(&grec, "integrity") == "ok" { "ok" } else { "bad" };
        return emit(root, opt, verdict, reason, (u64::from(opt.count), torn), integrity, "not_measured", &clock, 1, &digest, &format!("hg={hg_part};gh={gh_kept}"), "≤rtt", gate_parts(&hg_part, &gh_kept), "-", false);
    }
    let gh_part = dir_part(field(&grec, "rtt_us"), "gh").unwrap_or("n=0");
    let w_slice = field(&grec, "slice_us");
    let gate = gate_parts(&hg_part, gh_part);
    if !numeric(w_slice) || !clocks_ready(&clock) {
        return emit(root, opt, "unknown", "io", (u64::from(opt.count), 0), "ok", "not_measured", &clock, 1, &digest, &format!("hg={hg_part};gh={gh_part}"), "≤rtt", gate, "-", false);
    }
    emit(root, opt, "ok", "-", (u64::from(opt.count), 0), "ok", "measured", &clock, 1, &digest, &format!("hg={hg_part};gh={gh_part}"), "≤rtt", gate, w_slice, false)
}
fn c1_reader(opt: &Opt) -> i32 {
    let (root, mine, theirs) = match open_pair(opt) {
        Ok(dirs) => dirs,
        Err(code) => return code,
    };
    if opt.role == "guest" {
        c1_guest_reader(opt, &root, &mine, &theirs)
    } else {
        c1_host_reader(opt, &root, &mine, &theirs)
    }
}
fn digest_of(frames: &[[u8; FRAME_LEN]]) -> String {
    let mut all = Vec::with_capacity(frames.len() * FRAME_LEN);
    for frame in frames {
        all.extend_from_slice(frame);
    }
    hex_digest(&all)
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
        Some(0) => ("unknown", "no-non-admin-principal"),
        Some(1) => ("unknown", "single-principal"),
        Some(2..) => ("unknown", "mapping-not-measured"),
        None => ("unknown", "principal-unknown"),
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
fn poll_ready(fd: i32, timeout_ms: i32) -> stdio::Result<i16> {
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
            return Ok(0);
        }
        return Err(err);
    }
    if rc == 0 {
        return Ok(0);
    }
    Ok(pfd.revents)
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
        let revents = match poll_ready(fd, wait) {
            Ok(0) => continue,
            Ok(bits) => bits,
            Err(_) => return None,
        };
        // Linux reports POLLHUP alone once the buffer is empty. That is EOF, not a timeout.
        if revents & (0x8 | 0x20) != 0 && revents & 1 == 0 {
            return None;
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

    fn both_sides(dir: &std::path::Path, count: &str, timeout: &str) -> (i32, i32) {
        let guest_dir = dir.to_path_buf();
        let count_g = count.to_string();
        let timeout_g = timeout.to_string();
        let guest = thread::spawn(move || {
            let w = run(&guest_dir, &["writer", "--dir", "DIR", "--role", "guest", "--count", &count_g, "--timeout", &timeout_g]);
            let r = run(&guest_dir, &["reader", "--dir", "DIR", "--role", "guest", "--count", &count_g, "--timeout", &timeout_g]);
            (w, r)
        });
        let hw = run(dir, &["writer", "--dir", "DIR", "--role", "host", "--count", count, "--timeout", timeout]);
        let hr = run(dir, &["reader", "--dir", "DIR", "--role", "host", "--count", count, "--timeout", timeout]);
        let (gw, gr) = guest.join().unwrap();
        assert_eq!((gw, gr), (0, 0), "guest writer/reader");
        (hw, hr)
    }

    #[test]
    fn ping_pong_both_directions_measure_rtt_on_one_clock() {
        let dir = scratch("pong");
        let (hw, hr) = both_sides(&dir, "2", "8");
        assert_eq!((hw, hr), (0, 0));
        let line = text(&dir, "result-host");
        assert!(line.contains("verdict=ok"), "{line}");
        assert!(line.contains("reason=-"), "{line}");
        assert!(line.contains("latency=measured"), "{line}");
        assert!(line.contains("integrity=ok") && line.contains("torn=0") && line.contains("peer=1"), "{line}");
        assert!(line.contains("vis_us=≤rtt"), "{line}");
        assert!(line.contains("rtt_us=hg=n=2,") && line.contains(";gh=n=2,"), "{line}");
        assert!(line.contains("min=") && line.contains("p50=") && line.contains("max="), "{line}");
        assert!(!line.contains("p99="), "{line}");
        assert!(line.contains("gate=-"), "{line}");
        assert!(line.contains("lost=-"), "{line}");
        assert!(decode(line.trim_end_matches('\n'), C1_KEYS).is_some(), "{line}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_pong_times_out_and_keeps_the_sample() {
        let dir = scratch("tmo");
        let peer = dir.clone();
        let helper = thread::spawn(move || {
            let root = Dir::open_path(peer.to_str().unwrap()).unwrap();
            let pings = root.open_child_dir("host-write").unwrap();
            let pongs = root.open_child_dir("guest-write").unwrap();
            let end = Instant::now() + Duration::from_secs(8);
            loop {
                if Instant::now() >= end {
                    return;
                }
                match pings.read_file("f-1", FRAME_LEN, end) {
                    Ok((bytes, _, _, _)) if frame::classify(&bytes) == Ok((1, 1, 1)) => {
                        let _ = pongs.publish("f-1.tmp", "f-1", &pong_bytes(1, &bytes), true, end);
                        return;
                    }
                    Err(RdErr::Absent) => thread::sleep(Duration::from_millis(10)),
                    Ok(_) => thread::sleep(Duration::from_millis(10)),
                    Err(_) => return,
                }
            }
        });
        let started = Instant::now();
        let code = run(&dir, &["writer", "--dir", "DIR", "--role", "host", "--count", "2", "--timeout", "8"]);
        let elapsed = started.elapsed();
        assert_eq!(code, 1);
        assert!(elapsed >= Duration::from_secs(3) && elapsed < Duration::from_secs(7), "{elapsed:?}");
        let line = text(&dir, "result-host");
        assert!(line.contains("verdict=unknown") && line.contains("reason=timeout"), "{line}");
        assert!(line.contains("latency=not_measured"), "{line}");
        assert!(line.contains("rtt_us=hg=n=1,") && line.contains("min=") && line.contains("p50=") && line.contains("max="), "{line}");
        assert!(line.contains(";gh=n=0"), "{line}");
        assert!(line.contains("vis_us=≤rtt"), "{line}");
        assert!(!line.contains("verdict=ok") && !line.contains("p99="), "{line}");
        helper.join().unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn torn_ping_is_failed() {
        let dir = scratch("torn");
        let mut torn = frame::frame(1, 1, 1);
        torn[80] ^= 0xff;
        fs::write(dir.join("host-write/f-1"), torn).unwrap();
        assert_ne!(run(&dir, &["writer", "--dir", "DIR", "--role", "guest", "--count", "1", "--timeout", "2"]), 0);
        let line = text(&dir, "result-guest");
        assert!(line.contains("verdict=failed") && line.contains("reason=torn") && line.contains("integrity=bad"), "{line}");
        assert!(!line.contains("latency=measured"), "{line}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pong_digest_mismatch_is_failed() {
        let dir = scratch("badpong");
        let peer = dir.clone();
        let helper = thread::spawn(move || {
            let root = Dir::open_path(peer.to_str().unwrap()).unwrap();
            let pings = root.open_child_dir("host-write").unwrap();
            let pongs = root.open_child_dir("guest-write").unwrap();
            let end = Instant::now() + Duration::from_secs(4);
            loop {
                if Instant::now() >= end {
                    return;
                }
                match pings.read_file("f-1", FRAME_LEN, end) {
                    Ok((bytes, _, _, _)) if frame::classify(&bytes) == Ok((1, 1, 1)) => {
                        let mut bad = pong_bytes(1, &bytes);
                        let flip = bad.len() - 2;
                        bad[flip] = if bad[flip] == b'a' { b'b' } else { b'a' };
                        let _ = pongs.publish("f-1.tmp", "f-1", &bad, true, end);
                        return;
                    }
                    Err(RdErr::Absent) => thread::sleep(Duration::from_millis(10)),
                    Ok(_) => thread::sleep(Duration::from_millis(10)),
                    Err(_) => return,
                }
            }
        });
        assert_ne!(run(&dir, &["writer", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "4"]), 0);
        let line = text(&dir, "result-host");
        assert!(line.contains("verdict=failed") && line.contains("reason=bad-pong"), "{line}");
        assert!(!line.contains("latency=measured") && !line.contains("verdict=ok"), "{line}");
        helper.join().unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    fn clock_1() -> Clock {
        Clock { open_us: Some(1), write_us: Some(1), fsync_us: Some(1), rename_us: Some(1), dir_sync_us: Some(1), read_us: Some(1), slice_us: Some(4) }
    }

    #[test]
    fn torn_reply_keeps_samples_already_measured() {
        let dir = scratch("keep");
        let clock = clock_1();
        let host = c1_line("host", "ok", "-", (2, 0), ("ok", "not_measured", "-", "≤rtt", &fmt_rtt(&[10, 40], &[]), "-"), &clock, (1, "ab"));
        let guest = c1_line("guest", "unknown", "timeout", (1, 0), ("ok", "not_measured", "-", "≤rtt", &fmt_rtt(&[], &[15]), "-"), &clock, (1, "cd"));
        fs::write(dir.join("summary-host"), format!("{host}\n")).unwrap();
        fs::write(dir.join("summary-guest"), format!("{guest}\n")).unwrap();
        let mut torn = frame::frame(1, 2, 1);
        torn[80] ^= 0xff;
        fs::write(dir.join("guest-write/f-1"), torn).unwrap();
        assert_ne!(run(&dir, &["reader", "--dir", "DIR", "--role", "host", "--count", "2", "--timeout", "2"]), 0);
        let line = text(&dir, "result-host");
        assert!(line.contains("verdict=failed") && line.contains("reason=torn") && line.contains("integrity=bad"), "{line}");
        assert!(line.contains("rtt_us=hg=n=2,min=10,p50=10,max=40;gh=n=1,min=15,p50=15,max=15"), "{line}");
        assert!(line.contains("latency=not_measured") && line.contains("gate=-"), "{line}");
        assert!(!line.contains("latency=measured"), "{line}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn slow_rtt_stays_measured_and_marks_the_gate() {
        let dir = scratch("slow");
        let clock = clock_1();
        let host = c1_line("host", "ok", "-", (1, 0), ("ok", "not_measured", "-", "≤rtt", &fmt_rtt(&[SLOW_US], &[]), "-"), &clock, (1, "ab"));
        let guest = c1_line("guest", "ok", "-", (1, 0), ("ok", "not_measured", "-", "≤rtt", &fmt_rtt(&[], &[12]), "-"), &clock, (1, "cd"));
        fs::write(dir.join("summary-host"), format!("{host}\n")).unwrap();
        fs::write(dir.join("summary-guest"), format!("{guest}\n")).unwrap();
        fs::write(dir.join("guest-write/f-1"), frame::frame(1, 2, 1)).unwrap();
        assert_eq!(run(&dir, &["reader", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 0);
        let line = text(&dir, "result-host");
        assert!(line.contains("verdict=ok") && line.contains("reason=-") && line.contains("latency=measured"), "{line}");
        assert!(line.contains("gate=exceeds-2s") && line.contains("vis_us=≤rtt"), "{line}");
        assert!(line.contains("rtt_us=hg=n=1,min=2000000,p50=2000000,max=2000000;gh=n=1,min=12,p50=12,max=12"), "{line}");
        assert!(decode(line.trim_end_matches('\n'), C1_KEYS).is_some(), "{line}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rtt_stats_omit_p99_below_twenty_and_gate_at_two_seconds() {
        assert_eq!(fmt_samples(&[10, 20, 30, 40]), "n=4,min=10,p50=20,max=40");
        assert_eq!(fmt_samples(&[10, 20, 30, 40, 100]), "n=5,min=10,p50=30,max=100");
        assert_eq!(fmt_samples(&[]), "n=0");
        let small: Vec<u64> = (1..=19).collect();
        assert!(!fmt_samples(&small).contains("p99"));
        let mut full: Vec<u64> = (1..=20).collect();
        assert_eq!(fmt_samples(&full), "n=20,min=1,p50=10,max=20,p99=20");
        full.push(21);
        assert!(fmt_samples(&full).contains("p99="));
        assert!(part_ok("n=4,min=10,p50=20,max=40", 4));
        assert!(!part_ok("n=4,min=10,p50=20,max=40,p99=40", 4));
        assert_eq!(vis_of(&[], &[]), "-");
        assert_eq!(vis_of(&[1], &[]), "≤rtt");
        assert_eq!(gate_of(&[SLOW_US - 1], &[]), "-");
        assert_eq!(gate_of(&[SLOW_US], &[]), "exceeds-2s");
        assert_eq!(gate_of(&[1], &[SLOW_US]), "exceeds-2s");
        assert_eq!(gate_parts("n=1,min=1,p50=1,max=1999999", "n=0"), "-");
        assert_eq!(gate_parts(&fmt_samples(&[SLOW_US]), "n=1,min=1,p50=1,max=1"), "exceeds-2s");
        let line = c1_line("host", "ok", "-", (8, 0), ("ok", "measured", "1", "≤rtt", &fmt_rtt(&[SLOW_US, 10], &[12, 20]), "exceeds-2s"), &Clock::new(), (1, "ab"));
        assert!(line.contains("gate=exceeds-2s") && line.contains("vis_us=≤rtt") && line.contains("latency=measured"), "{line}");
        assert!(line.contains("verdict=ok"), "{line}");
        assert!(!fmt_rtt(&[SLOW_US], &[1]).contains("p99"));
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
    fn late_publish_does_not_become_measured() {
        let _guard = HookGuard;
        let dir = scratch("late-pub");
        io::test_set_hook(Box::new(|| thread::sleep(Duration::from_millis(1100))));
        let started = Instant::now();
        let code = run(&dir, &["writer", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "1"]);
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
        assert_eq!(code, 1);
        let line = text(&dir, "result-host");
        assert!(line.contains("reason=deadline") && line.contains("latency=not_measured"), "{line}");
        assert!(!line.contains("verdict=ok") && !line.contains("latency=measured"), "{line}");
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
        assert_eq!(count_non_admin("root 0\nadmin 501\n", "dsAttrTypeStandard:GroupMembership: admin\n").unwrap(), 0);
        assert_eq!(judge_c8(Some(0)), ("unknown", "no-non-admin-principal"));
        assert_eq!(judge_c8(Some(1)), ("unknown", "single-principal"));
        assert_eq!(judge_c8(Some(2)).1, "mapping-not-measured");
        assert_eq!(judge_c8(None), ("unknown", "principal-unknown"));
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
        let zero = c8_line("guest", "unknown", "no-non-admin-principal", "0");
        fs::write(dir.join("result-guest"), format!("{zero}\n")).unwrap();
        fs::remove_file(dir.join("result-host")).unwrap();
        assert_ne!(run(&dir, &["c8", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 0);
        let none = text(&dir, "result-host");
        assert!(none.contains("reason=no-non-admin-principal") && none.contains("n=0"), "{none}");
        assert!(none.contains("ops=0") && none.contains("created=0") && none.contains("elevated=0"), "{none}");
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

use crate::frame::{self, FRAME_LEN};
use crate::io::{self, Dir, RdErr, Times};
use crate::sha256::hex_digest;
use std::io::{self as stdio, ErrorKind};
use std::os::unix::io::AsRawFd;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const C1_KEYS: &[&str] = &[
    "v",
    "op",
    "role",
    "verdict",
    "reason",
    "n",
    "torn",
    "lost",
    "integrity",
    "latency",
    "vis_us",
    "rtt_us",
    "slice_us",
    "w_slice_us",
    "open_us",
    "write_us",
    "fsync_us",
    "rename_us",
    "dir_sync_us",
    "read_us",
    "peer",
    "digest",
    "retry",
    "gate",
];
const ROUND_WAIT: Duration = Duration::from_secs(3);
const SLOW_US: u64 = 2_000_000;
const C2_KEYS: &[&str] = &["v", "op", "role", "boundary", "verdict", "reason", "planned", "attempted", "renamed", "visible", "part", "dir_synced"];
const C8_KEYS: &[&str] = &["v", "op", "role", "verdict", "reason", "n", "ops", "created", "elevated"];
const CLOCK_KEYS: [&str; 7] = ["open_us", "write_us", "fsync_us", "rename_us", "dir_sync_us", "read_us", "slice_us"];

struct Opt {
    cmd: &'static str,
    boundary: &'static str,
    mode: &'static str,
    dir: String,
    role: &'static str,
    count: u8,
    end: Instant,
}
/// open, write, fsync, rename, dir_sync, read, slice. Unknown and non-numeric fields stay unset.
struct Clock([Option<u64>; 7]);
#[rustfmt::skip]
impl Clock {
    fn new() -> Self { Self([None; 7]) }
    fn take(&mut self, vals: [Option<u64>; 7]) {
        for (slot, v) in self.0.iter_mut().zip(vals) {
            if let Some(v) = v { bump(slot, v); }
        }
    }
    fn note(&mut self, t: &Times) {
        self.take([Some(t.open_us), Some(t.write_us), Some(t.fsync_us), Some(t.rename_us), t.dir_sync_us, None, Some(t.slice_us)]);
    }
    fn note_read(&mut self, open_us: u64, read_us: u64, slice_us: u64) {
        self.take([Some(open_us), None, None, None, None, Some(read_us), Some(slice_us)]);
    }
    fn merge(&mut self, src: &Clock) { self.take(src.0); }
    fn ready(&self) -> bool { self.0.iter().all(|v| v.is_some()) }
    fn from_fields(rec: &[(&str, &str)]) -> Self {
        let mut clock = Self::new();
        for (slot, key) in clock.0.iter_mut().zip(CLOCK_KEYS) {
            let v = field(rec, key);
            if numeric(v) { *slot = v.parse().ok(); }
        }
        clock
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
        "fill" => c9_fill(&opt),
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
        "fill" => ("fill", "-", &args[1..]),
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
    let mut mode = None;
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
            "--mode" => {
                mode = Some(match val.as_str() {
                    "data" => "data",
                    "small" => "small",
                    _ => return Err("usage"),
                });
            }
            "--timeout" => timeout = Some(small(val, 0, 120)?),
            _ => return Err("usage"),
        }
        i += 2;
    }
    let timeout = u64::from(timeout.ok_or("usage")?);
    let role = role.ok_or("usage")?;
    let mode = if cmd == "fill" {
        if role != "guest" {
            return Err("usage");
        }
        mode.ok_or("usage")?
    } else {
        "-"
    };
    let count = if cmd == "fill" { 1 } else { count.ok_or("usage")? };
    Ok(Opt { cmd, boundary, mode, dir: dir.ok_or("usage")?, role, count, end: io::now() + Duration::from_secs(timeout) })
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
    ["guest-write", "host-write"][usize::from(role == "host")]
}
fn peer_role(role: &str) -> &'static str {
    ["host", "guest"][usize::from(role == "host")]
}
/// Transient slot reads. `absent` is ENOENT; `replaced` is an inode that changed under the read. Neither count is a verdict.
#[derive(Clone, Copy)]
struct Retry {
    absent: u64,
    replaced: u64,
}
impl Retry {
    fn zero() -> Self {
        Self { absent: 0, replaced: 0 }
    }
    fn note(&mut self, err: &RdErr) {
        match err {
            RdErr::Absent => self.absent += 1,
            RdErr::Replaced => self.replaced += 1,
            _ => {}
        }
    }
    fn add(&mut self, other: Self) {
        self.absent += other.absent;
        self.replaced += other.replaced;
    }
    fn text(self) -> String {
        format!("absent:{},replaced:{}", self.absent, self.replaced)
    }
}
fn c1_line(role: &str, verdict: &str, reason: &str, counts: (u64, u64), marks: (&str, &str, &str, &str, &str, &str), clock: &Clock, tail: (u8, &str, &Retry)) -> String {
    let (n, torn) = counts;
    let (integrity, latency, w_slice, vis, rtt, gate) = marks;
    let (peer, digest, retry) = tail;
    let retry = retry.text();
    let c = &clock.0;
    format!(
        "v=1 op=c1 role={role} verdict={verdict} reason={reason} n={n} torn={torn} lost=- integrity={integrity} latency={latency} vis_us={vis} rtt_us={rtt} slice_us={} w_slice_us={w_slice} open_us={} write_us={} fsync_us={} rename_us={} dir_sync_us={} read_us={} peer={peer} digest={digest} retry={retry} gate={gate}",
        num(c[6]),
        num(c[0]),
        num(c[1]),
        num(c[2]),
        num(c[3]),
        num(c[4]),
        num(c[5])
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
#[cfg(test)]
thread_local! {
    static STORE_FAILS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}
#[cfg(test)]
pub(crate) fn test_fail_stores(n: u32) {
    STORE_FAILS.with(|slot| slot.set(n));
}
fn store(dir: &Dir, stem: &str, line: &str, end: Option<Instant>) -> Result<(), RdErr> {
    #[cfg(test)]
    if STORE_FAILS.with(|slot| {
        let left = slot.get();
        if left == 0 {
            false
        } else {
            slot.set(left - 1);
            true
        }
    }) {
        return Err(RdErr::Io);
    }
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
    c1_line(opt.role, "unknown", reason, (n, torn), (integrity, "not_measured", "-", "-", "hg=n=0;gh=n=0", "-"), clock, (0, "-", &Retry::zero()))
}
fn c2_unknown(opt: &Opt, reason: &str, planned: &str, attempted: &str, renamed: &str, visible: &str, part: &str) -> String {
    c2_line(opt.role, opt.boundary, "unknown", reason, [planned, attempted, renamed, visible], part, "-")
}
fn bare_unknown(opt: &Opt, reason: &str) -> String {
    match opt.cmd {
        "c2" => c2_unknown(opt, reason, &opt.count.to_string(), "-", "-", "-", "absent"),
        "c8" => c8_line(opt.role, "unknown", reason, "0"),
        "fill" => c9_line(opt, &crate::fill::Report { bytes: 0, files: 0, batches: 0, enospc: false }),
        _ => c1_unknown(opt, reason, 0, 0, "-", &Clock::new()),
    }
}
fn c9_line(opt: &Opt, report: &crate::fill::Report) -> String {
    format!("v=1 op=c9 role=guest mode={} enospc={} bytes={} files={} batches={}", opt.mode, u8::from(report.enospc), report.bytes, report.files, report.batches)
}
fn c9_fill(opt: &Opt) -> i32 {
    let root = match open_root(opt) {
        Ok(dir) => dir,
        Err(code) => return code,
    };
    let Some(plan) = crate::fill::resolve(opt.mode) else {
        return unk(opt, &root, "io");
    };
    let payload = match root.open_child_dir("payload") {
        Ok(dir) => dir,
        Err(_) => {
            let line = c9_line(opt, &crate::fill::Report { bytes: 0, files: 0, batches: 0, enospc: false });
            return finish(&root, "result-guest", &line, "unknown", opt.end, &line);
        }
    };
    let mut report = crate::fill::run(&payload, &root, plan, opt.end);
    let mut line = c9_line(opt, &report);
    if io::past(opt.end) {
        let _ = store(&root, "result-guest", &line, None);
        say(&line);
        return 1;
    }
    if store(&root, "result-guest", &line, Some(opt.end)).is_ok() && !io::past(opt.end) {
        say(&line);
        return 0;
    }
    if report.enospc && report.files > 0 && !io::past(opt.end) && crate::fill::remove_last(&payload, plan, report.files) && !io::past(opt.end) {
        report.files -= 1;
        report.bytes = report.bytes.saturating_sub(plan.chunk);
        line = c9_line(opt, &report);
        if !io::past(opt.end) && store(&root, "result-guest", &line, Some(opt.end)).is_ok() && !io::past(opt.end) {
            say(&line);
            return 0;
        }
    }
    if io::past(opt.end) {
        let _ = store(&root, "result-guest", &line, None);
    }
    say(&line);
    1
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
fn wait_more(limit: Instant) -> bool {
    if io::past(limit) {
        return false;
    }
    #[cfg(test)]
    {
        if io::test_wait_fire() {
            return !io::past(limit);
        }
    }
    let remain = limit.saturating_duration_since(io::now());
    if remain.is_zero() {
        return false;
    }
    thread::sleep(Duration::from_millis(20).min(remain));
    !io::past(limit)
}
struct Exch {
    clock: Clock,
    samples: Vec<u64>,
    frames: Vec<[u8; FRAME_LEN]>,
    retry: Retry,
}
struct Fail {
    exch: Exch,
    stop: Stop,
}
#[derive(Clone, Copy)]
enum Stop {
    Deadline,
    Timeout,
    PeerNotReady,
    Torn,
    Replaced,
    Nofollow,
    BadPong,
    Bad,
    Io,
}
#[rustfmt::skip]
mod c1 {
    use super::{c1_line, child_dir, decode, digest_of, field, finish, leaf, numeric, open_root, peer_role, rec_text, slot, store, unk, wait_more, C1_KEYS, Clock, Exch, Fail, Opt, ROUND_WAIT, Retry, SLOW_US, Stop};
    use crate::frame::{self, FRAME_LEN};
    use crate::io::{self, Dir, RdErr, Times};
    use crate::sha256::hex_digest;
    use std::time::{Duration, Instant};

    fn rank_of(n: usize, num: u64, den: u64) -> usize {
        let rank = (num * n as u64).div_ceil(den);
        rank.max(1) as usize - 1
    }
    pub(super) fn part_of(samples: &[u64]) -> Part {
        if samples.is_empty() { return Part::empty(); }
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        let n = sorted.len();
        let p99 = (n >= 20).then(|| sorted[rank_of(n, 99, 100)]);
        Part { n: n as u64, min: sorted[0], p50: sorted[rank_of(n, 1, 2)], max: sorted[n - 1], p99 }
    }
    #[derive(Clone, Copy)]
    pub(super) struct Part { n: u64, min: u64, p50: u64, max: u64, p99: Option<u64> }
    impl Part {
        fn empty() -> Self { Self { n: 0, min: 0, p50: 0, max: 0, p99: None } }
        fn text(self) -> String {
            if self.n == 0 { return "n=0".to_string(); }
            let body = format!("n={},min={},p50={},max={}", self.n, self.min, self.p50, self.max);
            match self.p99 { Some(p99) => format!("{body},p99={p99}"), None => body }
        }
        fn max_us(self) -> u64 { [self.max, 0][usize::from(self.n == 0)] }
    }
    #[cfg(test)]
    pub(super) fn fmt_samples(samples: &[u64]) -> String { part_of(samples).text() }
    #[cfg(test)]
    pub(super) fn fmt_rtt(hg: &[u64], gh: &[u64]) -> String { format!("hg={};gh={}", fmt_samples(hg), fmt_samples(gh)) }
    #[cfg(test)]
    pub(super) fn vis_of(hg: &[u64], gh: &[u64]) -> &'static str { ["≤rtt", "-"][usize::from(hg.is_empty() && gh.is_empty())] }
    pub(super) fn gate_part(hg: &Part, gh: &Part) -> &'static str { ["-", "exceeds-2s"][usize::from(hg.max_us().max(gh.max_us()) >= SLOW_US)] }
    #[cfg(test)]
    pub(super) fn gate_of(hg: &[u64], gh: &[u64]) -> &'static str { gate_part(&part_of(hg), &part_of(gh)) }
    fn key_num(piece: &str, key: &str) -> Option<u64> {
        let (k, v) = piece.split_once('=')?;
        (k == key && numeric(v)).then(|| v.parse().ok()).flatten()
    }
    pub(super) fn parse_part(s: &str) -> Option<Part> {
        if s == "n=0" { return Some(Part::empty()); }
        let bits: Vec<&str> = s.split(',').collect();
        if bits.len() != 4 && bits.len() != 5 { return None; }
        let n = key_num(bits[0], "n")?;
        let min = key_num(bits[1], "min")?;
        let p50 = key_num(bits[2], "p50")?;
        let max = key_num(bits[3], "max")?;
        if n == 0 || min > p50 || p50 > max { return None; }
        let p99 = if bits.len() == 5 { Some(key_num(bits[4], "p99")?) } else { None };
        if (n >= 20) != p99.is_some() || p99.is_some_and(|p| p < min || p > max) { return None; }
        Some(Part { n, min, p50, max, p99 })
    }
    pub(super) fn parse_rtt(rtt: &str) -> Option<(Part, Part)> {
        let (mut hg, mut gh) = (None, None);
        if rtt.is_empty() { return None; }
        for piece in rtt.split(';') {
            let (tag, rest) = piece.split_once('=')?;
            let slot = match tag { "hg" => &mut hg, "gh" => &mut gh, _ => return None };
            if slot.replace(parse_part(rest)?).is_some() { return None; }
        }
        Some((hg?, gh?))
    }
    fn empty_exch() -> Exch { Exch { clock: Clock::new(), samples: Vec::new(), frames: Vec::new(), retry: Retry::zero() } }
    pub(super) fn retry_ok(v: &str) -> bool {
        let Some(rest) = v.strip_prefix("absent:") else { return false };
        let Some((absent, replaced)) = rest.split_once(",replaced:") else { return false };
        numeric(absent) && numeric(replaced)
    }
    struct Rec { raw: String, vals: Vec<String>, hg: Option<Part>, gh: Option<Part>, clock: Clock }
    impl Rec {
        fn get(&self, key: &str) -> &str { C1_KEYS.iter().position(|k| *k == key).map(|i| self.vals[i].as_str()).unwrap_or("") }
        fn num(&self, key: &str) -> Option<u64> {
            let v = self.get(key);
            numeric(v).then(|| v.parse().ok()).flatten()
        }
        fn complete(&self, role: &str, count: u8, active: &str) -> bool {
            let (hot, cold, dir) = if active == "hg" { (self.hg.as_ref(), self.gh.as_ref(), 1) } else { (self.gh.as_ref(), self.hg.as_ref(), 2) };
            let (Some(hot), Some(cold)) = (hot, cold) else { return false };
            let n = u64::from(count);
            self.get("v") == "1" && self.get("op") == "c1" && self.get("role") == role && self.get("verdict") == "ok" && self.get("torn") == "0" && self.get("integrity") == "ok"
                && self.num("n") == Some(n) && self.get("peer") == "1" && hot.n == n && cold.n == 0 && self.num("slice_us").is_some() && self.get("digest") == expect_digest(dir, count)
        }
    }
    fn decode_c1(bytes: &[u8]) -> Result<Rec, ()> {
        let text = rec_text(bytes).ok_or(())?;
        let pairs = decode(text, C1_KEYS).ok_or(())?;
        if !retry_ok(field(&pairs, "retry")) { return Err(()); }
        let vals = C1_KEYS.iter().map(|k| field(&pairs, k).to_string()).collect();
        let (hg, gh) = parse_rtt(field(&pairs, "rtt_us")).map(|(h, g)| (Some(h), Some(g))).unwrap_or((None, None));
        Ok(Rec { raw: text.to_string(), vals, hg, gh, clock: Clock::from_fields(&pairs) })
    }
    fn as_deadline(text: &str) -> String {
        text.split(' ').map(|tok| match tok.split_once('=').map(|(k, _)| k) {
            Some("verdict") => "verdict=unknown",
            Some("reason") => "reason=deadline",
            Some("latency") => "latency=not_measured",
            _ => tok,
        }).collect::<Vec<_>>().join(" ")
    }
    struct Done { clock: Clock, retry: Retry, verdict: &'static str, reason: String, n: u64, torn: u64, integrity: &'static str, latency: &'static str, peer: u8, digest: String, hg: Part, gh: Part, w_slice: String, summary: bool }
    impl Done {
        fn blank(clock: Clock) -> Self {
            Self { clock, retry: Retry::zero(), verdict: "unknown", reason: "io".to_string(), n: 0, torn: 0, integrity: "-", latency: "not_measured", peer: 0, digest: "-".to_string(), hg: Part::empty(), gh: Part::empty(), w_slice: "-".to_string(), summary: false }
        }
        fn ship(self, root: &Dir, opt: &Opt) -> i32 {
            let vis = if self.hg.n == 0 && self.gh.n == 0 { "-" } else { "≤rtt" };
            let gate = gate_part(&self.hg, &self.gh);
            let rtt = format!("hg={};gh={}", self.hg.text(), self.gh.text());
            let line = c1_line(opt.role, self.verdict, &self.reason, (self.n, self.torn), (self.integrity, self.latency, &self.w_slice, vis, &rtt, gate), &self.clock, (self.peer, &self.digest, &self.retry));
            let late = as_deadline(&line);
            let stem = format!("summary-{}", opt.role);
            if self.summary && self.verdict == "ok" && (io::past(opt.end) || store(root, &stem, &line, Some(opt.end)).is_err() || io::past(opt.end)) {
                return finish(root, &slot(opt), &late, "unknown", opt.end, &late);
            }
            if self.summary && self.verdict != "ok" { let _ = store(root, &stem, &line, None); }
            finish(root, &slot(opt), &line, self.verdict, opt.end, &late)
        }
    }
    fn stopped(got: (Clock, Retry), stop: &Stop, ns: (u64, u64), peer: u8, digest: String, parts: (Part, Part), summary: bool) -> Done {
        let (clock, retry) = got;
        let (view_n, n) = ns;
        let (verdict, reason, torn, integrity) = stop_view(stop, view_n);
        Done { clock, retry, verdict, reason: reason.into(), n, torn, integrity, latency: "not_measured", peer, digest, hg: parts.0, gh: parts.1, w_slice: "-".into(), summary }
    }
    fn fail_done(fail: Fail, direction: u8, summary: bool) -> Done {
        let n = fail.exch.samples.len() as u64;
        let digest = if fail.exch.frames.is_empty() { "-".into() } else { digest_of(&fail.exch.frames) };
        let (h, g) = sides(direction, &fail.exch.samples);
        let stop = fail.stop;
        stopped((fail.exch.clock, fail.exch.retry), &stop, (n, n), u8::from(n > 0), digest, (part_of(h), part_of(g)), summary)
    }
    fn phase_done(exch: Exch, direction: u8) -> Done {
        let n = exch.samples.len() as u64;
        let (h, g) = sides(direction, &exch.samples);
        let (hg, gh) = (part_of(h), part_of(g));
        let digest = digest_of(&exch.frames);
        Done { clock: exch.clock, retry: exch.retry, verdict: "ok", reason: "-".into(), n, torn: 0, integrity: "ok", latency: "not_measured", peer: 1, digest, hg, gh, w_slice: "-".into(), summary: true }
    }
    fn sides(direction: u8, samples: &[u64]) -> (&[u64], &[u64]) {
        if direction == 1 { (samples, &[]) } else { (&[], samples) }
    }
    fn stop_of(err: RdErr, round_end: Option<Instant>, end: Instant) -> Stop {
        match err {
            RdErr::Deadline if round_end.is_some_and(|t| io::now() >= t) && !io::past(end) => Stop::Timeout,
            RdErr::Deadline => Stop::Deadline,
            RdErr::Replaced => Stop::Replaced,
            RdErr::Symlink | RdErr::NotFile | RdErr::HardLink | RdErr::Size => Stop::Nofollow,
            RdErr::Io | RdErr::Name | RdErr::Absent => Stop::Io,
        }
    }
    fn sticky(stop: &Stop) -> bool { matches!(stop, Stop::Replaced | Stop::Nofollow | Stop::Torn | Stop::Bad | Stop::BadPong) }
    fn wait_bytes(dir: &Dir, name: &str, end: Instant, round: Option<Duration>, clock: &mut Clock, retry: &mut Retry, mut good: impl FnMut(&[u8]) -> Result<bool, Stop>) -> Result<Vec<u8>, Stop> {
        let round_end = round.map(|d| io::now() + d);
        let timed = |at: Instant| if round_end.is_some() && !io::past(at) { Stop::Timeout } else { Stop::Deadline };
        loop {
            if io::past(end) { return Err(Stop::Deadline); }
            let limit = round_end.filter(|t| *t < end).unwrap_or(end);
            if io::now() >= limit { return Err(timed(end)); }
            match dir.read_file(name, FRAME_LEN, limit) {
                Ok((bytes, open_us, read_us, slice_us)) => {
                    clock.note_read(open_us, read_us, slice_us);
                    match good(&bytes) {
                        Ok(true) => return Ok(bytes),
                        // Placeholder, or an older seq still in this same slot.
                        Ok(false) => { if !wait_more(limit) { return Err(timed(end)); } }
                        Err(stop) => return Err(stop),
                    }
                }
                // Pre-created slot. ENOENT and a rename-over are the virtiofs window
                // measured on the Mini: discard this read and try again until the
                // round or command end. Bytes are only accepted from the Ok arm above.
                Err(err @ (RdErr::Absent | RdErr::Replaced)) => {
                    retry.note(&err);
                    if !wait_more(limit) { return Err(timed(end)); }
                }
                Err(err) => return Err(stop_of(err, round_end, end)),
            }
        }
    }
    fn dir_tag(direction: u8) -> &'static str { ["gh", "hg"][usize::from(direction == 1)] }
    fn slot_name(direction: u8, kind: &str) -> String { format!("{}-{kind}", dir_tag(direction)) }
    /// Bash plants this in every slot before the VM starts. It is not a ping, pong, or ready mark.
    pub(super) const PLACEHOLDER: &[u8] = b"v=1 op=empty seq=0\n";
    pub(super) fn ready_mark(direction: u8) -> &'static [u8] {
        [b"v=1 op=ready dir=gh\n" as &[u8], b"v=1 op=ready dir=hg\n" as &[u8]][usize::from(direction == 1)]
    }
    const READY_CAP: usize = {
        let mark = b"v=1 op=ready dir=hg\n".len();
        let other = b"v=1 op=ready dir=gh\n".len();
        let empty = b"v=1 op=empty seq=0\n".len();
        let mut n = mark;
        if other > n {
            n = other;
        }
        if empty > n {
            n = empty;
        }
        if n > 64 {
            panic!("ready record exceeds the read cap");
        }
        64
    };
    // Placeholder keeps waiting until the command deadline. Any other body fails.
    fn wait_ready(dir: &Dir, direction: u8, end: Instant, clock: &mut Clock, retry: &mut Retry) -> Result<(), Stop> {
        if io::past(end) {
            return Err(Stop::Deadline);
        }
        let name = slot_name(direction, "ready");
        let want = ready_mark(direction);
        loop {
            if io::past(end) {
                return Err(Stop::PeerNotReady);
            }
            match dir.read_file(&name, READY_CAP, end) {
                Ok((bytes, open_us, read_us, slice_us)) => {
                    clock.note_read(open_us, read_us, slice_us);
                    if bytes == want {
                        return Ok(());
                    }
                    if bytes == PLACEHOLDER {
                        if !wait_more(end) {
                            return Err(Stop::PeerNotReady);
                        }
                    } else {
                        return Err(Stop::Bad);
                    }
                }
                // Same transient window as wait_bytes. A bad body on a later read is still sticky.
                Err(err @ (RdErr::Absent | RdErr::Replaced)) => {
                    retry.note(&err);
                    if !wait_more(end) {
                        return Err(Stop::PeerNotReady);
                    }
                }
                Err(err) => return Err(stop_of(err, None, end)),
            }
        }
    }
    // Same slot every round. Older seq and the placeholder stay; a skip, the wrong direction, or a torn body sticks.
    fn accept_ping(bytes: &[u8], direction: u8, seq: u8) -> Result<bool, Stop> {
        if bytes == PLACEHOLDER {
            return Ok(false);
        }
        match frame::classify(bytes) {
            Ok((1, dir, s)) if dir == direction && s == seq => Ok(true),
            Ok((1, dir, s)) if dir == direction && s < seq => Ok(false),
            Ok(_) => Err(Stop::Bad),
            Err(()) => Err(Stop::Torn),
        }
    }
    fn pong_text(direction: u8, seq: u8, ping: &[u8]) -> String {
        format!("v=1 op=pong dir={} seq={seq} digest={}", dir_tag(direction), hex_digest(ping))
    }
    fn parse_pong(text: &str) -> Option<(&str, u8, &str)> {
        let rest = text.strip_prefix("v=1 op=pong dir=")?;
        let (dir, rest) = rest.split_once(" seq=")?;
        let (seq, digest) = rest.split_once(" digest=")?;
        if !matches!(dir, "hg" | "gh") || !numeric(seq) {
            return None;
        }
        let seq: u8 = seq.parse().ok()?;
        if seq == 0 || digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        Some((dir, seq, digest))
    }
    fn accept_pong(bytes: &[u8], direction: u8, seq: u8, ping: &[u8]) -> Result<bool, Stop> {
        if bytes == PLACEHOLDER {
            return Ok(false);
        }
        if bytes.len() == FRAME_LEN {
            return Err(if frame::classify(bytes).is_ok() { Stop::Bad } else { Stop::Torn });
        }
        let text = match rec_text(bytes) {
            Some(text) => text,
            None => return Err(Stop::Bad),
        };
        if text == pong_text(direction, seq, ping) {
            return Ok(true);
        }
        match parse_pong(text) {
            Some((dir, got, _)) if dir == dir_tag(direction) && got < seq => Ok(false),
            Some((dir, got, _)) if dir == dir_tag(direction) && got == seq => Err(Stop::BadPong),
            Some(_) => Err(Stop::Bad),
            None if text.starts_with("v=1 op=pong") => Err(Stop::BadPong),
            None => Err(Stop::Bad),
        }
    }
    pub(super) fn pong_bytes(direction: u8, seq: u8, ping: &[u8]) -> Vec<u8> {
        let mut body = pong_text(direction, seq, ping).into_bytes();
        body.push(b'\n');
        body
    }
    fn fail_at(exch: Exch, stop: Stop) -> Box<Fail> { Box::new(Fail { exch, stop }) }
    fn note_pub(exch: &mut Exch, res: Result<Times, RdErr>) -> Result<(), Stop> {
        match res {
            Ok(t) => { exch.clock.note(&t); Ok(()) }
            Err(RdErr::Deadline) => Err(Stop::Deadline),
            Err(RdErr::Replaced) => Err(Stop::Replaced),
            Err(_) => Err(Stop::Io),
        }
    }
    fn drive(ping_dir: &Dir, pong_dir: &Dir, direction: u8, count: u8, end: Instant) -> Result<Exch, Box<Fail>> {
        let mut exch = empty_exch();
        // `end` is the command deadline. Ready does not start a round or a new deadline.
        if let Err(stop) = wait_ready(pong_dir, direction, end, &mut exch.clock, &mut exch.retry) {
            return Err(fail_at(exch, stop));
        }
        for seq in 1..=count {
            if io::past(end) { return Err(fail_at(exch, Stop::Deadline)); }
            let ping = frame::frame(1, direction, seq);
            let name = slot_name(direction, "ping");
            if let Err(stop) = note_pub(&mut exch, ping_dir.publish(&format!("{name}.tmp"), &name, &ping, true, end)) { return Err(fail_at(exch, stop)); }
            if io::past(end) { return Err(fail_at(exch, Stop::Deadline)); }
            // Round trip starts when ping publication has returned, after the ready wait.
            let t0 = io::now();
            match wait_bytes(pong_dir, &slot_name(direction, "pong"), end, Some(ROUND_WAIT), &mut exch.clock, &mut exch.retry, |bytes| accept_pong(bytes, direction, seq, &ping)) {
                Ok(_) => { exch.samples.push(io::us(io::now().saturating_duration_since(t0))); exch.frames.push(ping); }
                Err(stop) => return Err(fail_at(exch, stop)),
            }
        }
        Ok(exch)
    }
    fn respond(ping_dir: &Dir, pong_dir: &Dir, direction: u8, count: u8, end: Instant) -> Result<Exch, Box<Fail>> {
        let mut exch = empty_exch();
        if io::past(end) { return Err(fail_at(exch, Stop::Deadline)); }
        let ready = slot_name(direction, "ready");
        if let Err(stop) = note_pub(&mut exch, pong_dir.publish(&format!("{ready}.tmp"), &ready, ready_mark(direction), true, end)) {
            return Err(fail_at(exch, stop));
        }
        for seq in 1..=count {
            if io::past(end) { return Err(fail_at(exch, Stop::Deadline)); }
            let ping = match wait_bytes(ping_dir, &slot_name(direction, "ping"), end, None, &mut exch.clock, &mut exch.retry, |bytes| accept_ping(bytes, direction, seq)) {
                Ok(bytes) => bytes,
                Err(stop) => return Err(fail_at(exch, stop)),
            };
            if io::past(end) { return Err(fail_at(exch, Stop::Deadline)); }
            let name = slot_name(direction, "pong");
            let body = pong_bytes(direction, seq, &ping);
            if let Err(stop) = note_pub(&mut exch, pong_dir.publish(&format!("{name}.tmp"), &name, &body, true, end)) { return Err(fail_at(exch, stop)); }
            let mut buf = [0u8; FRAME_LEN];
            buf.copy_from_slice(&ping);
            exch.frames.push(buf);
            if io::past(end) { return Err(fail_at(exch, Stop::Deadline)); }
        }
        Ok(exch)
    }
    fn stop_view(stop: &Stop, n: u64) -> (&'static str, &'static str, u64, &'static str) {
        let integrity = if n > 0 { "ok" } else { "-" };
        match stop {
            Stop::Deadline => ("unknown", "deadline", 0, integrity),
            Stop::Timeout => ("unknown", "timeout", 0, integrity),
            Stop::PeerNotReady => ("unknown", "peer-not-ready", 0, integrity),
            Stop::Torn => ("failed", "torn", 1, "bad"),
            Stop::Replaced => ("failed", "replaced", 1, "bad"),
            Stop::Nofollow => ("failed", "nofollow", 1, "bad"),
            Stop::BadPong => ("failed", "bad-pong", 0, "bad"),
            Stop::Bad => ("failed", "bad-record", 0, "bad"),
            Stop::Io => ("unknown", "io", 0, integrity),
        }
    }
    fn child(opt: &Opt, root: &Dir, name: &str) -> Result<Dir, i32> {
        match child_dir(root, name) {
            Ok(dir) => Ok(dir),
            Err(RdErr::Replaced) => Err(stopped((Clock::new(), Retry::zero()), &Stop::Replaced, (0, 0), 0, "-".into(), (Part::empty(), Part::empty()), false).ship(root, opt)),
            Err(_) => Err(unk(opt, root, "io")),
        }
    }
    fn open_pair(opt: &Opt) -> Result<(Dir, Dir, Dir), i32> {
        let root = open_root(opt)?;
        let mine = child(opt, &root, leaf(opt.role))?;
        let theirs = child(opt, &root, leaf(peer_role(opt.role)))?;
        Ok((root, mine, theirs))
    }
    pub(super) fn c1_writer(opt: &Opt) -> i32 {
        let (root, mine, theirs) = match open_pair(opt) { Ok(dirs) => dirs, Err(code) => return code };
        if opt.role == "host" {
            return match drive(&mine, &theirs, 1, opt.count, opt.end) {
                Ok(exch) => phase_done(exch, 1).ship(&root, opt),
                Err(fail) => fail_done(*fail, 1, true).ship(&root, opt),
            };
        }
        match respond(&theirs, &mine, 1, opt.count, opt.end) {
            Ok(exch) => {
                let mut done = phase_done(exch, 1);
                done.n = u64::from(opt.count);
                done.summary = false;
                done.ship(&root, opt)
            }
            Err(fail) => fail_done(*fail, 1, false).ship(&root, opt),
        }
    }
    fn summary_body(bytes: &[u8]) -> Result<bool, Stop> {
        if bytes == PLACEHOLDER {
            Ok(false)
        } else if decode_c1(bytes).is_ok() {
            Ok(true)
        } else {
            Err(Stop::Bad)
        }
    }
    fn load_summary(root: &Dir, name: &str, end: Instant, clock: &mut Clock, retry: &mut Retry) -> Result<Vec<u8>, Stop> {
        wait_bytes(root, name, end, None, clock, retry, summary_body)
    }
    fn read_rec(root: &Dir, name: &str, end: Instant, clock: &mut Clock, retry: &mut Retry) -> Result<Rec, Stop> {
        decode_c1(&load_summary(root, name, end, clock, retry)?).map_err(|()| Stop::Bad)
    }
    fn safe_reason(reason: &str) -> &str {
        if !reason.is_empty() && reason.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') { reason } else { "bad-record" }
    }
    fn c1_guest_reader(opt: &Opt, root: &Dir, mine: &Dir, theirs: &Dir) -> i32 {
        let mut clock = Clock::new();
        let mut retry = Retry::zero();
        let host = match read_rec(root, "summary-host", opt.end, &mut clock, &mut retry) {
            Ok(rec) => rec,
            Err(stop) => return stopped((clock, retry), &stop, (0, 0), 0, "-".into(), (Part::empty(), Part::empty()), true).ship(root, opt),
        };
        if !host.complete("host", opt.count, "hg") {
            let mut done = Done::blank(clock);
            done.retry = retry;
            done.reason = safe_reason(host.get("reason")).into();
            done.summary = true;
            return done.ship(root, opt);
        }
        match drive(mine, theirs, 2, opt.count, opt.end) {
            Ok(mut exch) => {
                exch.retry.add(retry);
                phase_done(exch, 2).ship(root, opt)
            }
            Err(mut fail) => {
                fail.exch.retry.add(retry);
                fail_done(*fail, 2, true).ship(root, opt)
            }
        }
    }
    fn gh_of(bytes: &[u8]) -> Part {
        match decode_c1(bytes) {
            Ok(rec) if rec.get("op") == "c1" && rec.get("role") == "guest" => rec.gh.unwrap_or_else(Part::empty),
            _ => Part::empty(),
        }
    }
    // No read past the original end, and a sticky summary error is not opened again.
    fn guest_part(root: &Dir, end: Instant, clock: &mut Clock, retry: &mut Retry) -> Result<Part, Stop> {
        if io::past(end) { return Ok(Part::empty()); }
        let limit = io::now().checked_add(ROUND_WAIT).unwrap_or(end).min(end);
        match load_summary(root, "summary-guest", limit, clock, retry) {
            Ok(bytes) => Ok(gh_of(&bytes)),
            Err(stop) if sticky(&stop) => Err(stop),
            Err(_) => Ok(Part::empty()),
        }
    }
    fn republish(root: &Dir, opt: &Opt, text: &str, verdict: &str) -> i32 { finish(root, &slot(opt), text, verdict, opt.end, &as_deadline(text)) }
    fn kept(clock: Clock, retry: Retry, n: u64, digest: String, parts: (Part, Part), mark: (&'static str, &str, u64, &'static str)) -> Done {
        let (verdict, reason, torn, integrity) = mark;
        Done { clock, retry, verdict, reason: reason.into(), n, torn, integrity, latency: "not_measured", peer: 1, digest, hg: parts.0, gh: parts.1, w_slice: "-".into(), summary: false }
    }
    fn c1_host_reader(opt: &Opt, root: &Dir, mine: &Dir, theirs: &Dir) -> i32 {
        let mut clock = Clock::new();
        let mut retry = Retry::zero();
        let host = match read_rec(root, "summary-host", opt.end, &mut clock, &mut retry) {
            Ok(rec) => rec,
            Err(stop) => return stopped((clock, retry), &stop, (0, 0), 0, "-".into(), (Part::empty(), Part::empty()), false).ship(root, opt),
        };
        if host.get("verdict") == "failed" { return republish(root, opt, &host.raw, "failed"); }
        if !host.complete("host", opt.count, "hg") {
            if host.get("op") == "c1" && host.get("role") == "host" { return republish(root, opt, &host.raw, "unknown"); }
            return stopped((clock, retry), &Stop::Bad, (0, 0), 0, "-".into(), (Part::empty(), Part::empty()), false).ship(root, opt);
        }
        clock.merge(&host.clock);
        let hg = host.hg.unwrap_or_else(Part::empty);
        let digest = host.get("digest").to_string();
        let n = u64::from(opt.count);
        match respond(theirs, mine, 2, opt.count, opt.end) {
            Err(fail) => {
                clock.merge(&fail.exch.clock);
                retry.add(fail.exch.retry);
                let stop = fail.stop;
                let gh = match guest_part(root, opt.end, &mut clock, &mut retry) {
                    Ok(part) => part,
                    Err(stop) => return stopped((clock, retry), &stop, (n, n), 1, digest, (hg, Part::empty()), false).ship(root, opt),
                };
                return stopped((clock, retry), &stop, (n, n), 1, digest, (hg, gh), false).ship(root, opt);
            }
            Ok(exch) => {
                clock.merge(&exch.clock);
                retry.add(exch.retry);
            }
        }
        let guest = match read_rec(root, "summary-guest", opt.end, &mut clock, &mut retry) {
            Ok(rec) => rec,
            Err(stop) if sticky(&stop) => return stopped((clock, retry), &stop, (n, n), 1, digest, (hg, Part::empty()), false).ship(root, opt),
            Err(_) => return kept(clock, retry, n, digest, (hg, Part::empty()), ("unknown", "no-peer-summary", 0, "ok")).ship(root, opt),
        };
        if !guest.complete("guest", opt.count, "gh") {
            let gh = guest.gh.unwrap_or_else(Part::empty);
            let saw = guest.get("verdict");
            let (verdict, reason) = if saw == "failed" || saw == "unknown" { (if saw == "failed" { "failed" } else { "unknown" }, safe_reason(guest.get("reason"))) } else { ("unknown", "bad-record") };
            let integrity = if verdict != "failed" && guest.get("integrity") == "ok" { "ok" } else { "bad" };
            return kept(clock, retry, n, digest, (hg, gh), (verdict, reason, u64::from(guest.get("torn") != "0"), integrity)).ship(root, opt);
        }
        let gh = guest.gh.unwrap_or_else(Part::empty);
        let Some(w_slice) = guest.num("slice_us").filter(|_| clock.ready()) else {
            return kept(clock, retry, n, digest, (hg, gh), ("unknown", "io", 0, "ok")).ship(root, opt);
        };
        Done { clock, retry, verdict: "ok", reason: "-".into(), n, torn: 0, integrity: "ok", latency: "measured", peer: 1, digest, hg, gh, w_slice: w_slice.to_string(), summary: false }.ship(root, opt)
    }
    pub(super) fn c1_reader(opt: &Opt) -> i32 {
        let (root, mine, theirs) = match open_pair(opt) { Ok(dirs) => dirs, Err(code) => return code };
        if opt.role == "guest" { c1_guest_reader(opt, &root, &mine, &theirs) } else { c1_host_reader(opt, &root, &mine, &theirs) }
    }
    pub(super) fn expect_digest(direction: u8, count: u8) -> String {
        let frames: Vec<[u8; FRAME_LEN]> = (1..=count).map(|seq| frame::frame(1, direction, seq)).collect();
        digest_of(&frames)
    }
}
use c1::{c1_reader, c1_writer};
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
        // Signed: macOS lists system accounts such as `nobody -2`; anything below 500 is not a person.
        let uid: i64 = parts.next().ok_or(())?.parse().map_err(|_| ())?;
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
    use super::c1::*;
    use super::*;
    use std::fs;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc};

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
        let dir = fs::canonicalize(&p).unwrap();
        seed_c1(&dir);
        dir
    }
    fn seed_c1(dir: &std::path::Path) {
        for leaf in ["host-write", "guest-write"] {
            for name in ["hg-ping", "hg-pong", "gh-ping", "gh-pong", "hg-ready", "gh-ready"] {
                fs::write(dir.join(leaf).join(name), PLACEHOLDER).unwrap();
            }
        }
        fs::write(dir.join("summary-host"), PLACEHOLDER).unwrap();
        fs::write(dir.join("summary-guest"), PLACEHOLDER).unwrap();
    }
    fn run(dir: &std::path::Path, args: &[&str]) -> i32 {
        let owned: Vec<String> = args.iter().map(|arg| if *arg == "DIR" { dir.to_str().unwrap().to_string() } else { (*arg).to_string() }).collect();
        execute(&owned).unwrap()
    }
    fn text(dir: &std::path::Path, name: &str) -> String {
        fs::read_to_string(dir.join(name)).unwrap()
    }
    struct SeamGuard;
    impl Drop for SeamGuard {
        fn drop(&mut self) {
            io::test_clear_hook();
            io::test_clear_read_hook();
            io::test_clear_clock();
            io::test_clear_wait();
        }
    }
    fn assert_gate_matches(line: &str) {
        let text = line.trim_end_matches('\n');
        let rec = decode(text, C1_KEYS).unwrap_or_else(|| panic!("record {line}"));
        let (hg, gh) = parse_rtt(field(&rec, "rtt_us")).unwrap_or_else(|| panic!("rtt {line}"));
        assert_eq!(field(&rec, "gate"), gate_part(&hg, &gh), "{line}");
    }
    fn phase_summary(role: &str, count: u8, direction: u8, samples: &[u64], clock: &Clock) -> String {
        let (hg, gh): (&[u64], &[u64]) = if direction == 1 { (samples, &[]) } else { (&[], samples) };
        c1_line(role, "ok", "-", (u64::from(count), 0), ("ok", "not_measured", "-", "≤rtt", &fmt_rtt(hg, gh), "-"), clock, (1, &expect_digest(direction, count), &Retry::zero()))
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

    fn leaf_names(dir: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir).unwrap().map(|ent| ent.unwrap().file_name().into_string().unwrap()).collect();
        names.sort();
        names
    }

    #[test]
    fn preseeded_fixed_slots_overwrite_and_measure_rtt_for_eight_rounds() {
        let dir = scratch("pong");
        let (hw, hr) = both_sides(&dir, "8", "30");
        assert_eq!((hw, hr), (0, 0));
        let line = text(&dir, "result-host");
        assert!(line.contains("verdict=ok"), "{line}");
        assert!(line.contains("reason=-"), "{line}");
        assert!(line.contains("latency=measured"), "{line}");
        assert!(line.contains("integrity=ok") && line.contains("torn=0") && line.contains("peer=1"), "{line}");
        assert!(line.contains("vis_us=≤rtt"), "{line}");
        assert!(line.contains("rtt_us=hg=n=8,") && line.contains(";gh=n=8,"), "{line}");
        assert!(line.contains(&format!("digest={}", expect_digest(1, 8))), "{line}");
        assert!(line.contains("min=") && line.contains("p50=") && line.contains("max="), "{line}");
        assert!(!line.contains("p99="), "{line}");
        assert_gate_matches(&line);
        assert!(line.contains("lost=-"), "{line}");
        let retry = line.split_whitespace().find_map(|tok| tok.strip_prefix("retry=")).unwrap_or("");
        assert!(retry_ok(retry), "{line}");
        assert!(decode(line.trim_end_matches('\n'), C1_KEYS).is_some(), "{line}");
        let guest = text(&dir, "summary-guest");
        assert!(guest.contains("gh=n=8,") && guest.contains(&format!("digest={}", expect_digest(2, 8))), "{guest}");
        assert_eq!(fs::read(dir.join("guest-write/hg-ready")).unwrap(), ready_mark(1));
        assert_eq!(fs::read(dir.join("host-write/gh-ready")).unwrap(), ready_mark(2));
        let ping = fs::read(dir.join("host-write/hg-ping")).unwrap();
        assert_eq!(ping.len(), FRAME_LEN);
        assert_eq!(frame::classify(&ping), Ok((1, 1, 8)));
        assert_eq!(fs::read(dir.join("guest-write/hg-pong")).unwrap(), pong_bytes(1, 8, &ping));
        let gping = fs::read(dir.join("guest-write/gh-ping")).unwrap();
        assert_eq!(frame::classify(&gping), Ok((1, 2, 8)));
        assert_eq!(fs::read(dir.join("host-write/gh-pong")).unwrap(), pong_bytes(2, 8, &gping));
        assert_eq!(fs::read(dir.join("host-write/hg-pong")).unwrap(), PLACEHOLDER);
        assert_eq!(fs::read(dir.join("guest-write/hg-ping")).unwrap(), PLACEHOLDER);
        assert_eq!(fs::read(dir.join("host-write/gh-ping")).unwrap(), PLACEHOLDER);
        assert_eq!(fs::read(dir.join("guest-write/gh-pong")).unwrap(), PLACEHOLDER);
        assert_eq!(fs::read(dir.join("host-write/hg-ready")).unwrap(), PLACEHOLDER);
        assert_eq!(fs::read(dir.join("guest-write/gh-ready")).unwrap(), PLACEHOLDER);
        let slots = vec!["gh-ping".to_string(), "gh-pong".to_string(), "gh-ready".to_string(), "hg-ping".to_string(), "hg-pong".to_string(), "hg-ready".to_string()];
        assert_eq!(leaf_names(&dir.join("host-write")), slots);
        assert_eq!(leaf_names(&dir.join("guest-write")), slots);
        assert!(!dir.join("host-write/hg-ping-1").exists());
        assert!(!dir.join("host-write/hg-ping.tmp").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn late_ready_is_outside_the_first_rtt() {
        let _guard = SeamGuard;
        let dir = scratch("late-ready");
        let t0 = Instant::now();
        io::test_set_clock(t0);
        let peer = dir.clone();
        let mut step = 0u8;
        io::test_set_wait(Box::new(move || {
            let root = Dir::open_path(peer.to_str().unwrap()).unwrap();
            let pings = root.open_child_dir("host-write").unwrap();
            let pongs = root.open_child_dir("guest-write").unwrap();
            let end = t0 + Duration::from_secs(30);
            if step == 0 {
                step = 1;
                io::test_set_clock(t0 + Duration::from_secs(5));
                pongs.publish("hg-ready.tmp", "hg-ready", ready_mark(1), true, end).unwrap();
            } else {
                let (bytes, _, _, _) = pings.read_file("hg-ping", FRAME_LEN, end).unwrap();
                pongs.publish("hg-pong.tmp", "hg-pong", &pong_bytes(1, 1, &bytes), true, end).unwrap();
            }
        }));
        assert_eq!(run(&dir, &["writer", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "30"]), 0);
        let line = text(&dir, "result-host");
        assert!(line.contains("verdict=ok") && line.contains("reason=-"), "{line}");
        assert!(line.contains("rtt_us=hg=n=1,min=0,p50=0,max=0;gh=n=0"), "{line}");
        assert!(!line.contains("5000000"), "{line}");
        assert!(!line.contains("reason=timeout") && !line.contains("reason=peer-not-ready"), "{line}");
        assert_gate_matches(&line);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_seq_waits_skip_and_malformed_fail_and_placeholder_is_not_a_pong() {
        let dir = scratch("oldseq");
        fs::write(dir.join("guest-write/hg-ready"), ready_mark(1)).unwrap();
        let ping1 = frame::frame(1, 1, 1);
        fs::write(dir.join("guest-write/hg-pong"), pong_bytes(1, 1, &ping1)).unwrap();
        let peer = dir.clone();
        let guard = SeamGuard;
        io::test_set_wait(Box::new(move || {
            let root = Dir::open_path(peer.to_str().unwrap()).unwrap();
            let pongs = root.open_child_dir("guest-write").unwrap();
            let end = Instant::now() + Duration::from_secs(5);
            let ping2 = frame::frame(1, 1, 2);
            pongs.publish("hg-pong.tmp", "hg-pong", &pong_bytes(1, 2, &ping2), true, end).unwrap();
        }));
        assert_eq!(run(&dir, &["writer", "--dir", "DIR", "--role", "host", "--count", "2", "--timeout", "5"]), 0);
        let line = text(&dir, "result-host");
        assert!(line.contains("verdict=ok") && line.contains("rtt_us=hg=n=2,"), "{line}");
        assert_eq!(fs::read(dir.join("guest-write/hg-pong")).unwrap(), pong_bytes(1, 2, &frame::frame(1, 1, 2)));
        drop(guard);
        let _ = fs::remove_dir_all(&dir);

        let dir = scratch("skip");
        fs::write(dir.join("guest-write/hg-ready"), ready_mark(1)).unwrap();
        fs::write(dir.join("guest-write/hg-pong"), pong_bytes(1, 2, &frame::frame(1, 1, 2))).unwrap();
        assert_ne!(run(&dir, &["writer", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 0);
        let line = text(&dir, "result-host");
        assert!(line.contains("verdict=failed") && line.contains("reason=bad-record"), "{line}");
        assert!(!line.contains("reason=timeout") && !line.contains("verdict=ok"), "{line}");
        let _ = fs::remove_dir_all(&dir);

        let dir = scratch("badshape");
        fs::write(dir.join("guest-write/hg-ready"), ready_mark(1)).unwrap();
        fs::write(dir.join("guest-write/hg-pong"), b"v=1 op=pong seq=1\n").unwrap();
        assert_ne!(run(&dir, &["writer", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 0);
        let line = text(&dir, "result-host");
        assert!(line.contains("verdict=failed") && (line.contains("reason=bad-pong") || line.contains("reason=bad-record")), "{line}");
        assert!(!line.contains("reason=timeout") && !line.contains("verdict=ok"), "{line}");
        let _ = fs::remove_dir_all(&dir);

        let dir = scratch("skip-ping");
        fs::write(dir.join("host-write/hg-ping"), frame::frame(1, 1, 2)).unwrap();
        assert_ne!(run(&dir, &["writer", "--dir", "DIR", "--role", "guest", "--count", "1", "--timeout", "2"]), 0);
        let line = text(&dir, "result-guest");
        assert!(line.contains("verdict=failed") && line.contains("reason=bad-record"), "{line}");
        assert!(!line.contains("reason=timeout"), "{line}");
        fs::write(dir.join("host-write/hg-ping"), b"not-a-frame\n").unwrap();
        assert_ne!(run(&dir, &["writer", "--dir", "DIR", "--role", "guest", "--count", "1", "--timeout", "2"]), 0);
        let line = text(&dir, "result-guest");
        assert!(line.contains("verdict=failed") && line.contains("reason=torn"), "{line}");
        let _ = fs::remove_dir_all(&dir);

        let _guard = SeamGuard;
        let dir = scratch("ph-pong");
        fs::write(dir.join("guest-write/hg-ready"), ready_mark(1)).unwrap();
        let t0 = Instant::now();
        io::test_set_clock(t0);
        io::test_set_wait(Box::new(move || {
            io::test_set_clock(t0 + ROUND_WAIT);
        }));
        assert_ne!(run(&dir, &["writer", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "8"]), 0);
        let line = text(&dir, "result-host");
        assert!(line.contains("verdict=unknown") && line.contains("reason=timeout"), "{line}");
        assert!(!line.contains("reason=bad-pong") && !line.contains("verdict=failed") && !line.contains("verdict=ok"), "{line}");
        assert_eq!(fs::read(dir.join("guest-write/hg-pong")).unwrap(), PLACEHOLDER);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_ready_is_peer_not_ready() {
        let _guard = SeamGuard;
        let dir = scratch("noready");
        let t0 = Instant::now();
        io::test_set_clock(t0);
        io::test_set_wait(Box::new(move || {
            io::test_set_clock(t0 + Duration::from_secs(30));
        }));
        assert_ne!(run(&dir, &["writer", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "8"]), 0);
        let line = text(&dir, "result-host");
        assert!(line.contains("verdict=unknown") && line.contains("reason=peer-not-ready"), "{line}");
        assert!(line.contains("n=0") && line.contains("rtt_us=hg=n=0;gh=n=0"), "{line}");
        assert!(!line.contains("reason=timeout") && !line.contains("reason=deadline"), "{line}");
        assert!(!line.contains("latency=measured") && !line.contains("verdict=ok"), "{line}");
        assert!(decode(line.trim_end_matches('\n'), C1_KEYS).is_some(), "{line}");
        let _ = fs::remove_dir_all(&dir);
        // A pre-created slot that is missing (the transient ENOENT macOS virtiofs shows
        // during a rename-over) is retried until the end, then reported as peer-not-ready.
        let dir = scratch("noready-absent");
        fs::remove_file(dir.join("guest-write/hg-ready")).unwrap();
        let t1 = Instant::now();
        io::test_set_clock(t1);
        io::test_set_wait(Box::new(move || {
            io::test_set_clock(t1 + Duration::from_secs(30));
        }));
        assert_ne!(run(&dir, &["writer", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "8"]), 0);
        let line = text(&dir, "result-host");
        assert!(line.contains("verdict=unknown") && line.contains("reason=peer-not-ready"), "{line}");
        assert!(line.contains("retry=absent:1,replaced:0"), "{line}");
        assert!(!line.contains("verdict=ok") && !line.contains("latency=measured"), "{line}");
        let _ = fs::remove_dir_all(&dir);
        io::test_clear_clock();
        io::test_clear_wait();
    }

    #[test]
    fn wrong_ready_marker_is_failed() {
        for body in [b"v=1 op=other\n".as_slice(), b"v=1 op=ready\n".as_slice(), ready_mark(2)] {
            let dir = scratch("badready");
            fs::write(dir.join("guest-write/hg-ready"), body).unwrap();
            assert_ne!(run(&dir, &["writer", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 0);
            let line = text(&dir, "result-host");
            assert!(line.contains("verdict=failed") && line.contains("reason=bad-record"), "{body:?} {line}");
            assert!(!line.contains("reason=peer-not-ready") && !line.contains("reason=timeout"), "{line}");
            assert!(!line.contains("latency=measured") && !line.contains("verdict=ok"), "{line}");
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn replaced_ready_marker_is_failed() {
        let _guard = SeamGuard;
        let dir = scratch("repready");
        let live = dir.join("guest-write/hg-ready");
        fs::write(&live, ready_mark(1)).unwrap();
        let aside = dir.join("guest-write/hg-ready.aside");
        io::test_set_read_hook(
            "hg-ready",
            Box::new(move || {
                fs::rename(&live, &aside).unwrap();
                fs::write(&live, b"v=1 op=other\n").unwrap();
            }),
        );
        assert_ne!(run(&dir, &["writer", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 0);
        let line = text(&dir, "result-host");
        // The bytes under the old inode were the valid mark. They are discarded.
        // The successor body is malformed, so the failure sticks.
        assert!(line.contains("verdict=failed") && line.contains("reason=bad-record"), "{line}");
        assert!(line.contains("retry=absent:0,replaced:1"), "{line}");
        assert!(!line.contains("reason=replaced") && !line.contains("reason=peer-not-ready") && !line.contains("latency=measured"), "{line}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_pong_times_out_and_keeps_the_sample() {
        let dir = scratch("tmo");
        fs::write(dir.join("guest-write/hg-ready"), ready_mark(1)).unwrap();
        let peer = dir.clone();
        let (ready_tx, ready_rx) = mpsc::channel();
        let helper = thread::spawn(move || {
            let root = Dir::open_path(peer.to_str().unwrap()).unwrap();
            let pings = root.open_child_dir("host-write").unwrap();
            let pongs = root.open_child_dir("guest-write").unwrap();
            let end = Instant::now() + Duration::from_secs(15);
            loop {
                if Instant::now() >= end {
                    return;
                }
                match pings.read_file("hg-ping", FRAME_LEN, end) {
                    Ok((bytes, _, _, _)) if frame::classify(&bytes) == Ok((1, 1, 1)) => {
                        pongs.publish("hg-pong.tmp", "hg-pong", &pong_bytes(1, 1, &bytes), true, end).unwrap();
                        let _ = ready_tx.send(());
                        return;
                    }
                    Err(RdErr::Absent) | Ok(_) => thread::sleep(Duration::from_millis(5)),
                    Err(_) => return,
                }
            }
        });
        let work = dir.clone();
        let (done_tx, done_rx) = mpsc::channel();
        thread::spawn(move || {
            let _guard = SeamGuard;
            let t0 = Instant::now();
            io::test_set_clock(t0);
            let mut n = 0;
            io::test_set_wait(Box::new(move || {
                if n == 0 {
                    n = 1;
                    let _ = ready_rx.recv_timeout(Duration::from_secs(15));
                } else {
                    io::test_set_clock(t0 + ROUND_WAIT);
                }
            }));
            let code = run(&work, &["writer", "--dir", "DIR", "--role", "host", "--count", "2", "--timeout", "8"]);
            let line = fs::read_to_string(work.join("result-host")).unwrap_or_default();
            let _ = done_tx.send((code, line));
        });
        let (code, line) = done_rx.recv_timeout(Duration::from_secs(20)).expect("hang watchdog");
        assert_eq!(code, 1);
        assert!(line.contains("verdict=unknown") && line.contains("reason=timeout"), "{line}");
        assert!(line.contains("latency=not_measured"), "{line}");
        assert!(line.contains("rtt_us=hg=n=1,") && line.contains("min=") && line.contains("p50=") && line.contains("max="), "{line}");
        assert!(line.contains(";gh=n=0"), "{line}");
        assert!(line.contains("vis_us=≤rtt"), "{line}");
        assert!(!line.contains("verdict=ok") && !line.contains("p99="), "{line}");
        assert!(!line.contains("reason=bad-pong") && !line.contains("reason=bad-record"), "{line}");
        assert_gate_matches(&line);
        helper.join().unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn torn_ping_is_failed() {
        let dir = scratch("torn");
        let mut torn = frame::frame(1, 1, 1);
        torn[80] ^= 0xff;
        fs::write(dir.join("host-write/hg-ping"), torn).unwrap();
        assert_ne!(run(&dir, &["writer", "--dir", "DIR", "--role", "guest", "--count", "1", "--timeout", "2"]), 0);
        let line = text(&dir, "result-guest");
        assert!(line.contains("verdict=failed") && line.contains("reason=torn") && line.contains("integrity=bad"), "{line}");
        assert!(!line.contains("latency=measured"), "{line}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pong_digest_mismatch_is_failed() {
        let dir = scratch("badpong");
        fs::write(dir.join("guest-write/hg-ready"), ready_mark(1)).unwrap();
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
                match pings.read_file("hg-ping", FRAME_LEN, end) {
                    Ok((bytes, _, _, _)) if frame::classify(&bytes) == Ok((1, 1, 1)) => {
                        let mut bad = pong_bytes(1, 1, &bytes);
                        let flip = bad.len() - 2;
                        bad[flip] = if bad[flip] == b'a' { b'b' } else { b'a' };
                        let _ = pongs.publish("hg-pong.tmp", "hg-pong", &bad, true, end);
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
        Clock([Some(1), Some(1), Some(1), Some(1), Some(1), Some(1), Some(4)])
    }

    #[test]
    fn torn_reply_keeps_samples_already_measured() {
        let dir = scratch("keep");
        let clock = clock_1();
        let host = phase_summary("host", 2, 1, &[10, 40], &clock);
        let guest = c1_line("guest", "unknown", "timeout", (1, 0), ("ok", "not_measured", "-", "≤rtt", &fmt_rtt(&[], &[15]), "-"), &clock, (1, "cd", &Retry::zero()));
        fs::write(dir.join("summary-host"), format!("{host}\n")).unwrap();
        fs::write(dir.join("summary-guest"), format!("{guest}\n")).unwrap();
        let mut torn = frame::frame(1, 2, 1);
        torn[80] ^= 0xff;
        fs::write(dir.join("guest-write/gh-ping"), torn).unwrap();
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
        let host = phase_summary("host", 1, 1, &[SLOW_US], &clock);
        let guest = phase_summary("guest", 1, 2, &[12], &clock);
        fs::write(dir.join("summary-host"), format!("{host}\n")).unwrap();
        fs::write(dir.join("summary-guest"), format!("{guest}\n")).unwrap();
        fs::write(dir.join("guest-write/gh-ping"), frame::frame(1, 2, 1)).unwrap();
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
        assert!(parse_part("n=4,min=10,p50=20,max=40").is_some());
        assert!(parse_part("n=4,min=10,p50=20,max=40,p99=40").is_none());
        assert!(parse_part("n=20,min=1,p50=10,max=20").is_none());
        assert!(parse_part("n=20,min=1,p50=10,max=20,p99=20").is_some());
        assert!(parse_part("n=20,min=1,p50=10,max=20,p99=21").is_none());
        assert!(parse_part("n=4,min=30,p50=20,max=40").is_none());
        assert!(parse_rtt("hg=n=1,min=2500000,p50=2500000,max=1;gh=n=0").is_none());
        assert!(parse_rtt("hg=n=1,min=1,p50=1,max=1;hg=n=0").is_none());
        assert!(parse_rtt("hg=n=1,min=1,p50=1,max=1").is_none());
        assert!(parse_rtt("gh=n=0;hg=n=1,min=1,p50=1,max=1").is_some());
        assert_eq!(vis_of(&[], &[]), "-");
        assert_eq!(vis_of(&[1], &[]), "≤rtt");
        assert_eq!(gate_of(&[SLOW_US - 1], &[]), "-");
        assert_eq!(gate_of(&[SLOW_US], &[]), "exceeds-2s");
        assert_eq!(gate_of(&[1], &[SLOW_US]), "exceeds-2s");
        assert_eq!(gate_part(&parse_part("n=1,min=1,p50=1,max=1999999").unwrap(), &parse_part("n=0").unwrap()), "-");
        assert_eq!(gate_part(&parse_part(&fmt_samples(&[SLOW_US])).unwrap(), &parse_part("n=1,min=1,p50=1,max=1").unwrap()), "exceeds-2s");
        let line = c1_line("host", "ok", "-", (8, 0), ("ok", "measured", "1", "≤rtt", &fmt_rtt(&[SLOW_US, 10], &[12, 20]), "exceeds-2s"), &Clock::new(), (1, "ab", &Retry::zero()));
        assert!(line.contains("retry=absent:0,replaced:0"), "{line}");
        assert!(retry_ok("absent:0,replaced:0") && !retry_ok("absent:0,replaced:") && !retry_ok("nope") && !retry_ok("absent:0,replaced:1,extra"));
        assert!(decode(&line, C1_KEYS).is_some(), "{line}");
        assert!(decode(&line.replace(" retry=absent:0,replaced:0", ""), C1_KEYS).is_none(), "{line}");
        assert!(line.contains("gate=exceeds-2s") && line.contains("vis_us=≤rtt") && line.contains("latency=measured"), "{line}");
        assert!(line.contains("verdict=ok"), "{line}");
        assert!(!fmt_rtt(&[SLOW_US], &[1]).contains("p99"));
    }

    #[test]
    fn malformed_ping_then_valid_ping_is_not_measured() {
        let _guard = SeamGuard;
        let dir = scratch("badping");
        let clock = clock_1();
        fs::write(dir.join("summary-host"), format!("{}\n", phase_summary("host", 1, 1, &[10], &clock))).unwrap();
        fs::write(dir.join("summary-guest"), format!("{}\n", phase_summary("guest", 1, 2, &[12], &clock))).unwrap();
        let mut short = frame::frame(1, 2, 1).to_vec();
        short.pop();
        fs::write(dir.join("guest-write/gh-ping"), &short).unwrap();
        let retries = Arc::new(AtomicUsize::new(0));
        let flag = retries.clone();
        let peer = dir.clone();
        io::test_set_wait(Box::new(move || {
            flag.fetch_add(1, Ordering::Relaxed);
            let root = Dir::open_path(peer.to_str().unwrap()).unwrap();
            let pings = root.open_child_dir("guest-write").unwrap();
            let end = Instant::now() + Duration::from_secs(2);
            let ping = frame::frame(1, 2, 1);
            let _ = pings.publish("gh-ping.tmp", "gh-ping", &ping, true, end);
        }));
        assert_ne!(run(&dir, &["reader", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 0);
        let line = text(&dir, "result-host");
        assert!(line.contains("verdict=failed") && line.contains("reason=torn") && line.contains("integrity=bad"), "{line}");
        assert!(!line.contains("latency=measured"), "{line}");
        assert!(line.contains("retry=absent:0,replaced:0"), "{line}");
        assert_eq!(retries.load(Ordering::Relaxed), 0, "malformed ping was retried");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_pong_then_valid_pong_is_not_measured() {
        let _guard = SeamGuard;
        let dir = scratch("badpong2");
        let clock = clock_1();
        fs::write(dir.join("summary-host"), format!("{}\n", phase_summary("host", 1, 1, &[10], &clock))).unwrap();
        fs::write(dir.join("host-write/gh-ready"), ready_mark(2)).unwrap();
        let mut torn = frame::frame(1, 2, 1);
        torn[80] ^= 0xff;
        fs::write(dir.join("host-write/gh-pong"), torn).unwrap();
        let retries = Arc::new(AtomicUsize::new(0));
        let flag = retries.clone();
        let peer = dir.clone();
        io::test_set_wait(Box::new(move || {
            flag.fetch_add(1, Ordering::Relaxed);
            let root = Dir::open_path(peer.to_str().unwrap()).unwrap();
            let pongs = root.open_child_dir("host-write").unwrap();
            let end = Instant::now() + Duration::from_secs(2);
            let ping = frame::frame(1, 2, 1);
            let _ = pongs.publish("gh-pong.tmp", "gh-pong", &pong_bytes(2, 1, &ping), true, end);
        }));
        assert_ne!(run(&dir, &["reader", "--dir", "DIR", "--role", "guest", "--count", "1", "--timeout", "2"]), 0);
        let guest = text(&dir, "result-guest");
        assert!(guest.contains("verdict=failed") && guest.contains("reason=torn"), "{guest}");
        assert!(!guest.contains("latency=measured"), "{guest}");
        assert!(guest.contains("retry=absent:0,replaced:0"), "{guest}");
        assert_eq!(retries.load(Ordering::Relaxed), 0, "malformed pong was retried");
        io::test_clear_wait();
        assert_ne!(run(&dir, &["reader", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 0);
        let host = text(&dir, "result-host");
        assert!(!host.contains("latency=measured"), "{host}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_inode_successor_during_verify_is_reread_and_measured() {
        let dir = scratch("race");
        let clock = clock_1();
        fs::write(dir.join("summary-host"), format!("{}\n", phase_summary("host", 1, 1, &[10], &clock))).unwrap();
        fs::write(dir.join("summary-guest"), format!("{}\n", phase_summary("guest", 1, 2, &[12], &clock))).unwrap();
        fs::write(dir.join("guest-write/gh-ping"), frame::frame(1, 2, 1)).unwrap();
        let (hit_tx, hit_rx) = mpsc::channel();
        let (go_tx, go_rx) = mpsc::channel();
        let work = dir.clone();
        let (done_tx, done_rx) = mpsc::channel();
        thread::spawn(move || {
            let _guard = SeamGuard;
            io::test_set_read_hook(
                "gh-ping",
                Box::new(move || {
                    let _ = hit_tx.send(());
                    let _ = go_rx.recv_timeout(Duration::from_secs(15));
                }),
            );
            let code = run(&work, &["reader", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "8"]);
            let line = fs::read_to_string(work.join("result-host")).unwrap_or_default();
            let _ = done_tx.send((code, line));
        });
        assert!(hit_rx.recv_timeout(Duration::from_secs(15)).is_ok(), "read hook did not fire");
        let root = Dir::open_path(dir.to_str().unwrap()).unwrap();
        let pings = root.open_child_dir("guest-write").unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        pings.publish("gh-ping.tmp", "gh-ping", &frame::frame(1, 2, 1), true, end).unwrap();
        let _ = go_tx.send(());
        let (code, line) = done_rx.recv_timeout(Duration::from_secs(20)).expect("hang watchdog");
        assert_eq!(code, 0);
        assert!(line.contains("verdict=ok") && line.contains("latency=measured") && line.contains("integrity=ok"), "{line}");
        assert!(line.contains("retry=absent:0,replaced:1"), "{line}");
        assert!(!line.contains("verdict=failed") && !line.contains("reason=replaced"), "{line}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn slot_absent_then_restored_completes() {
        let _guard = SeamGuard;
        let dir = scratch("absent-back");
        fs::write(dir.join("guest-write/hg-ready"), ready_mark(1)).unwrap();
        fs::remove_file(dir.join("guest-write/hg-pong")).unwrap();
        let peer = dir.clone();
        io::test_set_wait(Box::new(move || {
            let root = Dir::open_path(peer.to_str().unwrap()).unwrap();
            let pings = root.open_child_dir("host-write").unwrap();
            let pongs = root.open_child_dir("guest-write").unwrap();
            let end = Instant::now() + Duration::from_secs(5);
            if let Ok((bytes, _, _, _)) = pings.read_file("hg-ping", FRAME_LEN, end) {
                if frame::classify(&bytes) == Ok((1, 1, 1)) {
                    pongs.publish("hg-pong.tmp", "hg-pong", &pong_bytes(1, 1, &bytes), true, end).unwrap();
                }
            }
        }));
        assert_eq!(run(&dir, &["writer", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "5"]), 0);
        let line = text(&dir, "result-host");
        assert!(line.contains("verdict=ok") && line.contains("rtt_us=hg=n=1,"), "{line}");
        assert!(line.contains("retry=absent:1,replaced:0"), "{line}");
        assert!(!line.contains("verdict=failed") && !line.contains("reason=io") && !line.contains("reason=timeout"), "{line}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn replaced_guest_summary_stays_failed() {
        let dir = scratch("repsum");
        let clock = clock_1();
        fs::write(dir.join("summary-host"), format!("{}\n", phase_summary("host", 1, 1, &[10], &clock))).unwrap();
        fs::write(dir.join("summary-guest"), b"placeholder\n").unwrap();
        fs::write(dir.join("guest-write/gh-ping"), frame::frame(1, 2, 1)).unwrap();
        let (hit_tx, hit_rx) = mpsc::channel();
        let (go_tx, go_rx) = mpsc::channel();
        let work = dir.clone();
        let (done_tx, done_rx) = mpsc::channel();
        thread::spawn(move || {
            let _guard = SeamGuard;
            io::test_set_read_hook(
                "summary-guest",
                Box::new(move || {
                    let _ = hit_tx.send(());
                    let _ = go_rx.recv_timeout(Duration::from_secs(15));
                }),
            );
            let code = run(&work, &["reader", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]);
            let line = fs::read_to_string(work.join("result-host")).unwrap_or_default();
            let _ = done_tx.send((code, line));
        });
        assert!(hit_rx.recv_timeout(Duration::from_secs(15)).is_ok(), "summary read hook did not fire");
        let root = Dir::open_path(dir.to_str().unwrap()).unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        let body = b"v=1 op=c1 role=guest\n";
        root.publish("summary-guest.tmp", "summary-guest", body, true, end).unwrap();
        let _ = go_tx.send(());
        let (code, line) = done_rx.recv_timeout(Duration::from_secs(20)).expect("hang watchdog");
        assert_ne!(code, 0);
        assert!(line.contains("verdict=failed") && line.contains("reason=bad-record"), "{line}");
        assert!(line.contains("retry=absent:0,replaced:1"), "{line}");
        assert!(!line.contains("reason=replaced") && !line.contains("no-peer-summary") && !line.contains("latency=measured"), "{line}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn precreated_summary_overwrite_completes() {
        let dir = scratch("sumover");
        let clock = clock_1();
        fs::write(dir.join("summary-host"), format!("{}\n", phase_summary("host", 1, 1, &[10], &clock))).unwrap();
        fs::write(dir.join("guest-write/gh-ping"), frame::frame(1, 2, 1)).unwrap();
        let summary = format!("{}\n", phase_summary("guest", 1, 2, &[12], &clock));
        let (hit_tx, hit_rx) = mpsc::channel();
        let (go_tx, go_rx) = mpsc::channel();
        let work = dir.clone();
        let (done_tx, done_rx) = mpsc::channel();
        thread::spawn(move || {
            let _guard = SeamGuard;
            io::test_set_read_hook(
                "summary-guest",
                Box::new(move || {
                    let _ = hit_tx.send(());
                    let _ = go_rx.recv_timeout(Duration::from_secs(15));
                }),
            );
            let code = run(&work, &["reader", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "8"]);
            let line = fs::read_to_string(work.join("result-host")).unwrap_or_default();
            let _ = done_tx.send((code, line));
        });
        assert!(hit_rx.recv_timeout(Duration::from_secs(15)).is_ok(), "summary read hook did not fire");
        let root = Dir::open_path(dir.to_str().unwrap()).unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        root.publish("summary-guest.tmp", "summary-guest", summary.as_bytes(), true, end).unwrap();
        let _ = go_tx.send(());
        let (code, line) = done_rx.recv_timeout(Duration::from_secs(20)).expect("hang watchdog");
        assert_eq!(code, 0);
        assert!(line.contains("verdict=ok") && line.contains("latency=measured"), "{line}");
        assert!(line.contains("rtt_us=hg=n=1,min=10,p50=10,max=10;gh=n=1,min=12,p50=12,max=12"), "{line}");
        assert!(line.contains("retry=absent:0,replaced:1"), "{line}");
        assert!(!line.contains("verdict=failed") && !line.contains("reason=replaced"), "{line}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn symlink_guest_summary_is_nofollow() {
        let dir = scratch("sym");
        let clock = clock_1();
        fs::write(dir.join("summary-host"), format!("{}\n", phase_summary("host", 1, 1, &[8], &clock))).unwrap();
        fs::write(dir.join("guest-write/gh-ping"), frame::frame(1, 2, 1)).unwrap();
        fs::remove_file(dir.join("summary-guest")).unwrap();
        symlink("missing-target", dir.join("summary-guest")).unwrap();
        assert_ne!(run(&dir, &["reader", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 0);
        let line = text(&dir, "result-host");
        assert!(line.contains("verdict=failed") && line.contains("reason=nofollow"), "{line}");
        assert!(!line.contains("no-peer-summary") && !line.contains("latency=measured"), "{line}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unordered_guest_stats_are_not_measured() {
        let dir = scratch("order");
        let clock = clock_1();
        fs::write(dir.join("summary-host"), format!("{}\n", phase_summary("host", 1, 1, &[10], &clock))).unwrap();
        let bad = c1_line("guest", "ok", "-", (1, 0), ("ok", "not_measured", "-", "≤rtt", "hg=n=0;gh=n=1,min=2500000,p50=2500000,max=1", "-"), &clock, (1, &expect_digest(2, 1), &Retry::zero()));
        fs::write(dir.join("summary-guest"), format!("{bad}\n")).unwrap();
        fs::write(dir.join("guest-write/gh-ping"), frame::frame(1, 2, 1)).unwrap();
        assert_ne!(run(&dir, &["reader", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 0);
        let line = text(&dir, "result-host");
        assert!(!line.contains("latency=measured") && !line.contains("2500000"), "{line}");
        assert!(line.contains("rtt_us=hg=n=1,min=10,p50=10,max=10;gh=n=0"), "{line}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_guest_summary_is_unknown_not_measured() {
        let _guard = SeamGuard;
        let dir = scratch("nosum");
        let clock = clock_1();
        fs::write(dir.join("summary-host"), format!("{}\n", phase_summary("host", 1, 1, &[9], &clock))).unwrap();
        fs::write(dir.join("guest-write/gh-ping"), frame::frame(1, 2, 1)).unwrap();
        let t0 = Instant::now();
        io::test_set_clock(t0);
        io::test_set_wait(Box::new(move || {
            io::test_set_clock(t0 + Duration::from_secs(5));
        }));
        assert_ne!(run(&dir, &["reader", "--dir", "DIR", "--role", "host", "--count", "1", "--timeout", "2"]), 0);
        let line = text(&dir, "result-host");
        assert!(line.contains("verdict=unknown") && line.contains("reason=no-peer-summary"), "{line}");
        assert!(line.contains("hg=n=1") && line.contains(";gh=n=0"), "{line}");
        assert!(!line.contains("latency=measured"), "{line}");
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
    fn late_publish_does_not_become_measured() {
        let _guard = SeamGuard;
        let dir = scratch("late-pub");
        fs::write(dir.join("guest-write/hg-ready"), ready_mark(1)).unwrap();
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

    #[test]
    fn count_non_admin_handles_negative_system_uids() {
        let users = "_amavisd                 83\ndaemon                   1\neval                     501\nnobody                   -2\nroot                     0\n";
        let admins = "GroupMembership: root _mbsetupuser eval\n";
        assert_eq!(count_non_admin(users, admins), Ok(0));
        let with_user = format!("{users}tester                   502\n");
        assert_eq!(count_non_admin(&with_user, admins), Ok(1));
        assert_eq!(judge_c8(count_non_admin(users, admins).ok()), ("unknown", "no-non-admin-principal"));
    }

    #[test]
    fn fill_quota_reports_enospc_and_stops() {
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                crate::fill::test_clear_plan();
            }
        }
        let _guard = Guard;
        let dir = scratch("c9");
        fs::create_dir(dir.join("payload")).unwrap();
        // batch=2, so the second file publishes once, then the quota stop publishes again.
        crate::fill::test_set_plan(crate::fill::Plan { files: 5, chunk: 100, batch: 2, quota: 250, tag: 's' });
        assert_eq!(run(&dir, &["fill", "--dir", "DIR", "--role", "guest", "--mode", "small", "--timeout", "30"]), 0);
        let line = text(&dir, "result-guest");
        assert!(line.contains("op=c9") && line.contains("mode=small") && line.contains("enospc=1"), "{line}");
        assert!(line.contains("files=2") && line.contains("bytes=200") && line.contains("batches=2"), "{line}");
        assert!(dir.join("payload/s-0000").is_file() && dir.join("payload/s-0001").is_file());
        assert!(!dir.join("payload/s-0002").exists());
        let bad: Vec<String> = ["fill", "--dir", dir.to_str().unwrap(), "--role", "host", "--mode", "data", "--timeout", "1"].into_iter().map(str::to_string).collect();
        assert!(execute(&bad).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn full_volume_fallback_does_not_rewrite_counts_or_claim_delivery() {
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                crate::fill::test_clear_plan();
                crate::fill::test_fail_unlink(false);
                test_fail_stores(0);
            }
        }
        let _guard = Guard;
        let plan = crate::fill::Plan { files: 4, chunk: 100, batch: 10, quota: 250, tag: 's' };
        let run_fill = |dir: &std::path::Path| {
            fs::create_dir(dir.join("payload")).unwrap();
            crate::fill::test_set_plan(plan);
            run(dir, &["fill", "--dir", "DIR", "--role", "guest", "--mode", "small", "--timeout", "30"])
        };
        let dir = scratch("c9un");
        test_fail_stores(1);
        crate::fill::test_fail_unlink(true);
        assert_ne!(run_fill(&dir), 0);
        assert!(dir.join("payload/s-0001").is_file());
        if dir.join("result-guest").exists() {
            let body = fs::read_to_string(dir.join("result-guest")).unwrap();
            assert!(!body.contains("files=1"), "{body}");
        }
        let _ = fs::remove_dir_all(&dir);
        let dir = scratch("c9st");
        test_fail_stores(2);
        crate::fill::test_fail_unlink(false);
        assert_ne!(run_fill(&dir), 0);
        assert!(dir.join("payload/s-0000").is_file());
        assert!(!dir.join("payload/s-0001").exists());
        assert!(!dir.join("result-guest").exists());
        let _ = fs::remove_dir_all(&dir);
        let dir = scratch("c9ok");
        test_fail_stores(1);
        crate::fill::test_fail_unlink(false);
        assert_eq!(run_fill(&dir), 0);
        let line = text(&dir, "result-guest");
        assert!(line.contains("files=1") && line.contains("bytes=100") && line.contains("enospc=1"), "{line}");
        assert!(dir.join("payload/s-0000").is_file());
        assert!(!dir.join("payload/s-0001").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fallback_retry_past_the_deadline_is_not_success() {
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                crate::fill::test_clear_plan();
                crate::fill::test_fail_unlink(false);
                test_fail_stores(0);
                crate::io::test_clear_hook();
                crate::io::test_clear_clock();
            }
        }
        let _guard = Guard;
        let dir = scratch("c9late");
        fs::create_dir(dir.join("payload")).unwrap();
        let t0 = crate::io::now();
        crate::io::test_set_clock(t0);
        let watched = dir.clone();
        fn arm(dir: std::path::PathBuf, t0: std::time::Instant) {
            crate::io::test_set_hook(Box::new(move || {
                if dir.join("result-guest.tmp").exists() {
                    crate::io::test_set_clock(t0 + std::time::Duration::from_secs(3600));
                } else {
                    arm(dir.clone(), t0);
                }
            }));
        }
        arm(watched, t0);
        crate::fill::test_set_plan(crate::fill::Plan { files: 4, chunk: 100, batch: 10, quota: 250, tag: 's' });
        test_fail_stores(1);
        assert_ne!(run(&dir, &["fill", "--dir", "DIR", "--role", "guest", "--mode", "small", "--timeout", "30"]), 0);
        let _ = fs::remove_dir_all(&dir);
    }
}

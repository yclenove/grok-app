//! In-place pwrite/pread visibility (C11).
//!
//! Two pre-created 64-byte files: `ip-hg` (host writes, guest reads) and `ip-gh`
//! (guest writes, host reads). The payload is `v=1 seq=<u32> crc=<8 hex>`, left
//! aligned, space padded, with `\n` at byte 63. `crc` is the first 8 lowercase
//! hex digits of SHA-256 over the decimal ASCII of `seq` (no leading zeros).
//! A bad crc or an unparseable seq is a tear: count it and read again. A seq
//! that goes backwards or jumps ahead is a failure.
//!
//! `fd` holds one fd and `pread`s every 5 ms. `reopen` opens, reads, and closes
//! on each poll. The writer `pwrite`s 64 bytes at offset 0 and then `fsync`s.
//! Before sequence 1, the guest writes canonical `seq=0` on the pre-created
//! `ip-gh` and the host waits for it. That wait is outside the round-trip
//! samples. The host's round-trip starts immediately before `pwrite` and ends
//! when the matching pong is read, so the write and `fsync` are inside the
//! sample, on the host clock. The time from `fsync` returning to that read is
//! a separate residual. A hit counts only while the original round deadline
//! and the command deadline are still ahead; neither is renewed. The guest's
//! gap starts when its pong `fsync` returns and ends when the next ping is
//! read, on the guest clock, and that pong counts only under the same check.
use super::c1::samples_text;
use super::{finish, open_root, slot, Opt};
use crate::io::{self, Dir, Held, RdErr};
use crate::sha256::hex_digest;
use std::thread;
use std::time::{Duration, Instant};

const ROUND: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(5);
const HOST_WRITES: &str = "ip-hg";
const GUEST_WRITES: &str = "ip-gh";

/// SHA-256 of the decimal ASCII seq, truncated to 8 lowercase hex digits.
fn crc_of(seq: u32) -> String {
    let hex = hex_digest(seq.to_string().as_bytes());
    hex[..8].to_string()
}

fn encode(seq: u32) -> [u8; 64] {
    let crc = crc_of(seq);
    let text = format!("v=1 seq={seq} crc={crc}");
    debug_assert!(text.len() < 63, "seq record does not fit");
    let mut buf = [b' '; 64];
    buf[..text.len()].copy_from_slice(text.as_bytes());
    buf[63] = b'\n';
    buf
}

/// `Some(seq)` only for a canonical record. Anything else is a tear, including
/// the all-zero placeholder. `seq=0` is the guest-ready mark, written only on
/// `ip-gh`. While a round waits for seq 1 it is the previous value, not a tear.
fn parse_seq(buf: &[u8; 64]) -> Option<u32> {
    if buf[63] != b'\n' {
        return None;
    }
    let body = &buf[..63];
    let end = body.iter().rposition(|b| *b != b' ').map(|i| i + 1).unwrap_or(0);
    if body[end..].iter().any(|b| *b != b' ') {
        return None;
    }
    let text = std::str::from_utf8(&body[..end]).ok()?;
    let rest = text.strip_prefix("v=1 seq=")?;
    let (seq_s, crc_s) = rest.split_once(" crc=")?;
    if seq_s.is_empty() || seq_s.len() > 10 || !seq_s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if seq_s.len() > 1 && seq_s.starts_with('0') {
        return None;
    }
    if crc_s.len() != 8 || !crc_s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
        return None;
    }
    let seq: u32 = seq_s.parse().ok()?;
    if crc_s != crc_of(seq) {
        return None;
    }
    Some(seq)
}

enum Judge {
    Hit,
    Stale,
    Skip,
    Rollback,
}

/// `seq == want - 1` is the previous value and keeps polling. A larger seq is a
/// skip. Anything older than the previous value is a rollback.
fn judge(seq: u32, want: u32) -> Judge {
    if seq == want {
        Judge::Hit
    } else if want > 0 && seq == want - 1 {
        Judge::Stale
    } else if seq > want {
        Judge::Skip
    } else {
        Judge::Rollback
    }
}

#[derive(Debug)]
enum WaitErr {
    Timeout { torn: u64 },
    Deadline { torn: u64 },
    Rollback { torn: u64 },
    Skip { torn: u64 },
    Io { torn: u64 },
    Nofollow { torn: u64 },
    PeerNotReady { torn: u64 },
}

impl WaitErr {
    fn torn(&self) -> u64 {
        match self {
            WaitErr::Timeout { torn } | WaitErr::Deadline { torn } | WaitErr::Rollback { torn } | WaitErr::Skip { torn } | WaitErr::Io { torn } | WaitErr::Nofollow { torn } | WaitErr::PeerNotReady { torn } => *torn,
        }
    }
    fn outcome(&self) -> (&'static str, &'static str) {
        match self {
            WaitErr::Timeout { .. } => ("unknown", "timeout"),
            WaitErr::Deadline { .. } => ("unknown", "deadline"),
            WaitErr::Rollback { .. } => ("failed", "rollback"),
            WaitErr::Skip { .. } => ("failed", "skip"),
            WaitErr::Io { .. } => ("unknown", "io"),
            WaitErr::Nofollow { .. } => ("failed", "nofollow"),
            WaitErr::PeerNotReady { .. } => ("unknown", "peer-not-ready"),
        }
    }
}

#[derive(Debug)]
struct Seen {
    polls: u64,
    torn: u64,
    at: Instant,
}

enum Got {
    Bytes([u8; 64]),
    Absent,
    Nofollow,
    Io,
}

enum Watch<'a> {
    Fd(Held),
    Reopen(&'a Dir, &'static str),
}

impl Watch<'_> {
    fn poll(&self) -> Result<[u8; 64], RdErr> {
        match self {
            Watch::Fd(held) => held.pread64(),
            Watch::Reopen(dir, name) => dir.read_inplace(name),
        }
    }
}

fn class_open(err: RdErr) -> (&'static str, &'static str) {
    match err {
        RdErr::Symlink | RdErr::NotFile | RdErr::HardLink => ("failed", "nofollow"),
        _ => ("unknown", "io"),
    }
}

fn expired(round_end: Instant, command_end: Instant, torn: u64) -> Option<WaitErr> {
    if io::past(command_end) {
        Some(WaitErr::Deadline { torn })
    } else if io::past(round_end) {
        Some(WaitErr::Timeout { torn })
    } else {
        None
    }
}

/// Sleep 5 ms, or run the test wait hook. `false` means the limit has passed.
fn pause(limit: Instant) -> bool {
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
    thread::sleep(POLL.min(remain));
    !io::past(limit)
}

fn wait_got<F>(mut read: F, want: u32, round_end: Instant, command_end: Instant) -> Result<Seen, WaitErr>
where
    F: FnMut() -> Got,
{
    let mut polls = 0u64;
    let mut torn = 0u64;
    let limit = round_end.min(command_end);
    loop {
        if let Some(err) = expired(round_end, command_end, torn) {
            return Err(err);
        }
        match read() {
            Got::Bytes(buf) => {
                polls += 1;
                match parse_seq(&buf) {
                    Some(seq) => match judge(seq, want) {
                        Judge::Hit => {
                            // The read can return after the deadline that was captured
                            // before it. Do not count that frame, and do not move the end.
                            if let Some(err) = expired(round_end, command_end, torn) {
                                return Err(err);
                            }
                            return Ok(Seen { polls, torn, at: io::now() });
                        }
                        Judge::Stale => {
                            if !pause(limit) {
                                return Err(expired(round_end, command_end, torn).unwrap_or(WaitErr::Timeout { torn }));
                            }
                        }
                        Judge::Skip => return Err(WaitErr::Skip { torn }),
                        Judge::Rollback => return Err(WaitErr::Rollback { torn }),
                    },
                    None => {
                        torn += 1;
                        if !pause(limit) {
                            return Err(expired(round_end, command_end, torn).unwrap_or(WaitErr::Timeout { torn }));
                        }
                    }
                }
            }
            // A missing file is a poll, not a tear. The pre-created names should not hit this.
            Got::Absent => {
                polls += 1;
                if !pause(limit) {
                    return Err(expired(round_end, command_end, torn).unwrap_or(WaitErr::Timeout { torn }));
                }
            }
            Got::Nofollow => return Err(WaitErr::Nofollow { torn }),
            Got::Io => return Err(WaitErr::Io { torn }),
        }
    }
}

fn wait_seq(watch: &Watch, want: u32, round_end: Instant, command_end: Instant) -> Result<Seen, WaitErr> {
    wait_got(
        || match watch.poll() {
            Ok(buf) => Got::Bytes(buf),
            Err(RdErr::Absent) => Got::Absent,
            Err(RdErr::Symlink | RdErr::NotFile | RdErr::HardLink) => Got::Nofollow,
            Err(_) => Got::Io,
        },
        want,
        round_end,
        command_end,
    )
}

/// Guest ready on pre-created `ip-gh`. All-zero placeholders are not tears.
/// Any other unparseable record is. `seq=0` while the command is still open is
/// ready; seeing it after the command end is `deadline`. Any other seq is a skip.
/// Expiry while the file is still a placeholder or torn is `peer-not-ready`.
fn wait_ready(watch: &Watch, command_end: Instant) -> Result<u64, WaitErr> {
    let mut torn = 0u64;
    loop {
        if io::past(command_end) {
            return Err(WaitErr::PeerNotReady { torn });
        }
        match watch.poll() {
            Ok(buf) => match parse_seq(&buf) {
                Some(0) => {
                    if io::past(command_end) {
                        return Err(WaitErr::Deadline { torn });
                    }
                    return Ok(torn);
                }
                Some(_) => return Err(WaitErr::Skip { torn }),
                None => {
                    if buf != [0u8; 64] {
                        torn += 1;
                    }
                    if !pause(command_end) {
                        return Err(WaitErr::PeerNotReady { torn });
                    }
                }
            },
            Err(RdErr::Absent) => {
                if !pause(command_end) {
                    return Err(WaitErr::PeerNotReady { torn });
                }
            }
            Err(RdErr::Symlink | RdErr::NotFile | RdErr::HardLink) => return Err(WaitErr::Nofollow { torn }),
            Err(_) => return Err(WaitErr::Io { torn }),
        }
    }
}

/// `fd` opens once and holds the fd. An absent file is retried until the first
/// round ends. `reopen` opens on every poll.
fn open_watch<'a>(dir: &'a Dir, name: &'static str, mode: &str, round_end: Instant, command_end: Instant) -> Result<Watch<'a>, WaitErr> {
    if mode != "fd" {
        return Ok(Watch::Reopen(dir, name));
    }
    let limit = round_end.min(command_end);
    loop {
        if let Some(err) = expired(round_end, command_end, 0) {
            return Err(err);
        }
        match dir.open_held(name, false) {
            Ok(held) => return Ok(Watch::Fd(held)),
            Err(RdErr::Absent) => {
                if !pause(limit) {
                    return Err(expired(round_end, command_end, 0).unwrap_or(WaitErr::Timeout { torn: 0 }));
                }
            }
            Err(RdErr::Symlink | RdErr::NotFile | RdErr::HardLink) => return Err(WaitErr::Nofollow { torn: 0 }),
            Err(_) => return Err(WaitErr::Io { torn: 0 }),
        }
    }
}

fn polls_text(polls: &[(u32, u64)]) -> String {
    if polls.is_empty() {
        return "-".to_string();
    }
    polls.iter().map(|(seq, n)| format!("{seq}:{n}")).collect::<Vec<_>>().join(",")
}

fn host_line(mode: &str, n: u64, torn: u64, rtt: &[u64], resid: &[u64], verdict: &str, reason: &str) -> String {
    let rtt = samples_text(rtt);
    let resid = samples_text(resid);
    format!("v=1 op=c11 role=host mode={mode} n={n} torn={torn} rtt_us={rtt} resid_us={resid} verdict={verdict} reason={reason}")
}

fn guest_line(mode: &str, n: u64, torn: u64, polls: &[(u32, u64)], gaps: &[u64], verdict: &str, reason: &str) -> String {
    let polls = polls_text(polls);
    let gap = samples_text(gaps);
    format!("v=1 op=c11 role=guest mode={mode} n={n} torn={torn} polls={polls} gap_us={gap} verdict={verdict} reason={reason}")
}

pub(super) fn unknown_line(opt: &Opt, reason: &str) -> String {
    if opt.role == "guest" {
        guest_line(opt.mode, 0, 0, &[], &[], "unknown", reason)
    } else {
        host_line(opt.mode, 0, 0, &[], &[], "unknown", reason)
    }
}

struct Report {
    guest: bool,
    mode: &'static str,
    n: u64,
    torn: u64,
    rtt: Vec<u64>,
    /// Post-fsync residual of each counted round. Same length as `rtt`.
    resid: Vec<u64>,
    polls: Vec<(u32, u64)>,
    gaps: Vec<u64>,
}

impl Report {
    fn host(mode: &'static str) -> Self {
        Self { guest: false, mode, n: 0, torn: 0, rtt: Vec::new(), resid: Vec::new(), polls: Vec::new(), gaps: Vec::new() }
    }
    fn guest(mode: &'static str) -> Self {
        Self { guest: true, mode, n: 0, torn: 0, rtt: Vec::new(), resid: Vec::new(), polls: Vec::new(), gaps: Vec::new() }
    }
    fn line(&self, verdict: &str, reason: &str) -> String {
        if self.guest {
            guest_line(self.mode, self.n, self.torn, &self.polls, &self.gaps, verdict, reason)
        } else {
            host_line(self.mode, self.n, self.torn, &self.rtt, &self.resid, verdict, reason)
        }
    }
    fn emit(&self, root: &Dir, opt: &Opt, verdict: &str, reason: &str) -> i32 {
        let line = self.line(verdict, reason);
        let late = unknown_line(opt, "deadline");
        finish(root, &slot(opt), &line, verdict, opt.end, &late)
    }
    fn fail(&mut self, root: &Dir, opt: &Opt, err: WaitErr) -> i32 {
        self.torn += err.torn();
        let (verdict, reason) = err.outcome();
        self.emit(root, opt, verdict, reason)
    }
}

fn role_host(root: &Dir, opt: &Opt) -> i32 {
    let mut report = Report::host(opt.mode);
    let writer = match root.open_held(HOST_WRITES, true) {
        Ok(held) => held,
        Err(err) => {
            let (verdict, reason) = class_open(err);
            return report.emit(root, opt, verdict, reason);
        }
    };
    // The ready wait uses the command deadline, not a 5s round.
    let watch = match open_watch(root, GUEST_WRITES, opt.mode, opt.end, opt.end) {
        Ok(watch) => watch,
        Err(err) => return report.fail(root, opt, err),
    };
    match wait_ready(&watch, opt.end) {
        Ok(torn) => report.torn += torn,
        Err(err) => return report.fail(root, opt, err),
    }
    for seq in 1..=u32::from(opt.count) {
        let round_end = io::now() + ROUND;
        if let Some(err) = expired(round_end, opt.end, 0) {
            return report.fail(root, opt, err);
        }
        let bytes = encode(seq);
        let t0 = io::now();
        if let Err(err) = writer.pwrite64(&bytes) {
            let (verdict, reason) = class_open(err);
            return report.emit(root, opt, verdict, reason);
        }
        let t_sync = io::now();
        if let Some(err) = expired(round_end, opt.end, 0) {
            return report.fail(root, opt, err);
        }
        match wait_seq(&watch, seq, round_end, opt.end) {
            Ok(seen) => {
                if let Some(err) = expired(round_end, opt.end, seen.torn) {
                    return report.fail(root, opt, err);
                }
                report.torn += seen.torn;
                report.rtt.push(io::us(seen.at.saturating_duration_since(t0)));
                report.resid.push(io::us(seen.at.saturating_duration_since(t_sync)));
                report.n += 1;
            }
            Err(err) => return report.fail(root, opt, err),
        }
    }
    report.emit(root, opt, "ok", "-")
}

fn role_guest(root: &Dir, opt: &Opt) -> i32 {
    let mut report = Report::guest(opt.mode);
    let writer = match root.open_held(GUEST_WRITES, true) {
        Ok(held) => held,
        Err(err) => {
            let (verdict, reason) = class_open(err);
            return report.emit(root, opt, verdict, reason);
        }
    };
    let watch = match open_watch(root, HOST_WRITES, opt.mode, opt.end, opt.end) {
        Ok(watch) => watch,
        Err(err) => return report.fail(root, opt, err),
    };
    // Ready mark before any round clock. seq=0 lives only on ip-gh.
    if let Err(err) = writer.pwrite64(&encode(0)) {
        let (verdict, reason) = class_open(err);
        return report.emit(root, opt, verdict, reason);
    }
    // pwrite64 has returned, fsync included. Only now tell the host over SSH,
    // so a slow readiness fsync stays outside every round the host times.
    {
        use std::io::Write;
        let mut out = std::io::stdout();
        let _ = writeln!(out, "v=1 op=c11 role=guest ready=1 mode={}", opt.mode);
        let _ = out.flush();
    }
    if io::past(opt.end) {
        return report.fail(root, opt, WaitErr::Deadline { torn: 0 });
    }
    let mut last_pong: Option<Instant> = None;
    for seq in 1..=u32::from(opt.count) {
        let round_end = io::now() + ROUND;
        if let Some(err) = expired(round_end, opt.end, 0) {
            return report.fail(root, opt, err);
        }
        match wait_seq(&watch, seq, round_end, opt.end) {
            Ok(seen) => {
                if let Err(err) = writer.pwrite64(&encode(seq)) {
                    report.torn += seen.torn;
                    let (verdict, reason) = class_open(err);
                    return report.emit(root, opt, verdict, reason);
                }
                let pong_at = io::now();
                if let Some(err) = expired(round_end, opt.end, seen.torn) {
                    return report.fail(root, opt, err);
                }
                report.torn += seen.torn;
                if let Some(prev) = last_pong {
                    report.gaps.push(io::us(seen.at.saturating_duration_since(prev)));
                }
                last_pong = Some(pong_at);
                report.polls.push((seq, seen.polls));
                report.n += 1;
            }
            Err(err) => return report.fail(root, opt, err),
        }
    }
    report.emit(root, opt, "ok", "-")
}

pub(super) fn run(opt: &Opt) -> i32 {
    let root = match open_root(opt) {
        Ok(dir) => dir,
        Err(code) => return code,
    };
    match opt.role {
        "guest" => role_guest(&root, opt),
        _ => role_host(&root, opt),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{decode, field};
    use super::{encode, guest_line, host_line, parse_seq, wait_got, Got, WaitErr};
    use crate::io;
    use crate::sha256::hex_digest;
    use std::fs::{self, OpenOptions};
    use std::os::unix::fs::{FileExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    const HOST_KEYS: &[&str] = &["v", "op", "role", "mode", "n", "torn", "rtt_us", "resid_us", "verdict", "reason"];
    const GUEST_KEYS: &[&str] = &["v", "op", "role", "mode", "n", "torn", "polls", "gap_us", "verdict", "reason"];

    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            io::test_clear_hook();
            io::test_clear_read_hook();
            io::test_clear_clock();
            io::test_clear_wait();
        }
    }
    fn guard() -> Guard {
        io::test_clear_hook();
        io::test_clear_read_hook();
        io::test_clear_clock();
        io::test_clear_wait();
        Guard
    }
    fn quiet_waits() {
        io::test_set_wait(Box::new(|| {}));
    }
    fn far() -> Instant {
        io::now() + Duration::from_secs(30)
    }
    fn scratch(tag: &str) -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("s11c11-{tag}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        let dir = fs::canonicalize(&p).unwrap();
        fs::write(dir.join("ip-hg"), [0u8; 64]).unwrap();
        fs::write(dir.join("ip-gh"), [0u8; 64]).unwrap();
        dir
    }
    fn own(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_string()).collect()
    }
    fn exec(dir: &Path, role: &str, mode: &str, count: &str, timeout: &str) -> i32 {
        let path = dir.to_str().unwrap().to_string();
        crate::execute(&own(&["c11", "--dir", &path, "--role", role, "--mode", mode, "--count", count, "--timeout", timeout])).unwrap()
    }
    fn line_of(dir: &Path, name: &str) -> String {
        fs::read_to_string(dir.join(name)).unwrap()
    }
    fn poke(dir: &Path, name: &str, seq: u32) {
        let file = OpenOptions::new().read(true).write(true).open(dir.join(name)).unwrap();
        let buf = encode(seq);
        file.write_at(&buf, 0).unwrap();
        file.sync_all().unwrap();
    }

    #[test]
    fn c11_record_codec_and_crc() {
        assert_eq!(super::crc_of(1), "6b86b273");
        assert!(hex_digest(b"1").starts_with("6b86b273"));
        let buf = encode(1);
        assert_eq!(buf.len(), 64);
        assert_eq!(buf[63], b'\n');
        let text = b"v=1 seq=1 crc=6b86b273";
        assert_eq!(&buf[..text.len()], text);
        assert!(buf[text.len()..63].iter().all(|b| *b == b' '));
        assert_eq!(parse_seq(&buf), Some(1));
        assert_eq!(parse_seq(&encode(8)), Some(8));
        assert_eq!(parse_seq(&encode(u32::MAX)), Some(u32::MAX));
        assert_eq!(parse_seq(&[0u8; 64]), None);
        let mut bad = encode(1);
        bad[text.len() - 1] = b'0';
        assert_eq!(parse_seq(&bad), None);
        let mut upper = encode(1);
        upper[text.len() - 1] = b'B';
        assert_eq!(parse_seq(&upper), None);
        let mut shifted = encode(1);
        shifted[62] = b'\n';
        shifted[63] = b' ';
        assert_eq!(parse_seq(&shifted), None);
        let lead = *b"v=1 seq=01 crc=6b86b273";
        let mut padded = [b' '; 64];
        padded[..lead.len()].copy_from_slice(&lead);
        padded[63] = b'\n';
        assert_eq!(parse_seq(&padded), None);
        let rtt = super::super::c1::samples_text(&[10, 30, 20]);
        assert_eq!(rtt, "n=3,min=10,p50=20,max=30");
        let resid = super::super::c1::samples_text(&[1, 3, 2]);
        assert_eq!(resid, "n=3,min=1,p50=2,max=3");
        let host = host_line("fd", 8, 2, &[10, 30, 20], &[1, 3, 2], "ok", "-");
        assert_eq!(host, format!("v=1 op=c11 role=host mode=fd n=8 torn=2 rtt_us={rtt} resid_us={resid} verdict=ok reason=-"));
        let guest = guest_line("reopen", 0, 0, &[], &[], "unknown", "timeout");
        assert_eq!(guest, "v=1 op=c11 role=guest mode=reopen n=0 torn=0 polls=- gap_us=n=0 verdict=unknown reason=timeout");
        let polls = guest_line("fd", 2, 1, &[(1, 4), (2, 7)], &[15], "ok", "-");
        assert!(polls.contains("polls=1:4,2:7"), "{polls}");
        assert!(polls.contains("gap_us=n=1,min=15,p50=15,max=15"), "{polls}");
    }

    #[test]
    fn c11_torn_reread_is_not_failure() {
        let _g = guard();
        quiet_waits();
        let mut step = 0u32;
        let mut bad = encode(1);
        bad[20] ^= 1;
        let end = far();
        let seen = wait_got(
            || {
                step += 1;
                let buf = match step {
                    1 => [0u8; 64],
                    2 => bad,
                    _ => encode(1),
                };
                Got::Bytes(buf)
            },
            1,
            end,
            end,
        )
        .unwrap();
        assert_eq!(seen.torn, 2);
        assert_eq!(seen.polls, 3);
    }

    #[test]
    fn c11_seq_rollback_fails() {
        let _g = guard();
        quiet_waits();
        let end = far();
        let err = wait_got(|| Got::Bytes(encode(0)), 2, end, end).unwrap_err();
        assert!(matches!(err, WaitErr::Rollback { torn: 0 }), "{err:?}");
        let mut n = 0;
        let seen = wait_got(
            || {
                n += 1;
                Got::Bytes(if n == 1 { encode(0) } else { encode(1) })
            },
            1,
            end,
            end,
        )
        .unwrap();
        assert_eq!(seen.torn, 0, "seq 0 while waiting for 1 is stale, not a tear");
        assert_eq!(seen.polls, 2);
    }

    #[test]
    fn c11_seq_skip_fails() {
        let _g = guard();
        quiet_waits();
        let end = far();
        let err = wait_got(|| Got::Bytes(encode(3)), 1, end, end).unwrap_err();
        assert!(matches!(err, WaitErr::Skip { torn: 0 }), "{err:?}");
    }

    #[test]
    fn c11_round_timeout_is_unknown() {
        let _g = guard();
        let start = Instant::now();
        io::test_set_clock(start);
        io::test_set_wait(Box::new(move || {
            io::test_set_clock(start + Duration::from_secs(6));
        }));
        let err = wait_got(|| Got::Bytes([0u8; 64]), 1, start + super::ROUND, start + Duration::from_secs(30)).unwrap_err();
        assert!(matches!(err, WaitErr::Timeout { torn: 1 }), "{err:?}");
    }

    #[test]
    fn c11_host_rollback_on_fd() {
        let _g = guard();
        let dir = scratch("rollback");
        let peer_dir = dir.clone();
        let peer = thread::spawn(move || {
            poke(&peer_dir, "ip-gh", 0);
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut answered = false;
            while Instant::now() < deadline {
                let bytes = fs::read(peer_dir.join("ip-hg")).unwrap_or_default();
                if bytes.len() == 64 {
                    let buf: [u8; 64] = bytes.as_slice().try_into().unwrap();
                    match parse_seq(&buf) {
                        Some(1) if !answered => {
                            poke(&peer_dir, "ip-gh", 1);
                            answered = true;
                        }
                        Some(2) => {
                            poke(&peer_dir, "ip-gh", 0);
                            return;
                        }
                        _ => {}
                    }
                }
                thread::sleep(Duration::from_millis(1));
            }
        });
        let code = exec(&dir, "host", "fd", "2", "10");
        peer.join().unwrap();
        assert_eq!(code, 1);
        let raw = line_of(&dir, "result-host");
        let rec = decode(raw.trim_end_matches('\n'), HOST_KEYS).unwrap_or_else(|| panic!("{raw}"));
        assert_eq!(field(&rec, "verdict"), "failed");
        assert_eq!(field(&rec, "reason"), "rollback");
        assert_eq!(field(&rec, "n"), "1");
        assert_eq!(field(&rec, "mode"), "fd");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fd_round_trip() {
        exchange("fd");
    }

    #[test]
    fn reopen_round_trip() {
        exchange("reopen");
    }

    fn exchange(mode: &str) {
        let _g = guard();
        let dir = scratch(mode);
        let host_dir = dir.clone();
        let guest_dir = dir.clone();
        let host_mode = mode.to_string();
        let guest_mode = mode.to_string();
        let host = thread::spawn(move || exec(&host_dir, "host", &host_mode, "8", "20"));
        let guest = thread::spawn(move || exec(&guest_dir, "guest", &guest_mode, "8", "20"));
        let host_code = host.join().unwrap();
        let guest_code = guest.join().unwrap();
        let host_raw = line_of(&dir, "result-host");
        let guest_raw = line_of(&dir, "result-guest");
        assert_eq!(host_code, 0, "{host_raw}");
        assert_eq!(guest_code, 0, "{guest_raw}");
        let host_rec = decode(host_raw.trim_end_matches('\n'), HOST_KEYS).unwrap_or_else(|| panic!("{host_raw}"));
        let guest_rec = decode(guest_raw.trim_end_matches('\n'), GUEST_KEYS).unwrap_or_else(|| panic!("{guest_raw}"));
        assert_eq!(field(&host_rec, "op"), "c11");
        assert_eq!(field(&host_rec, "role"), "host");
        assert_eq!(field(&host_rec, "mode"), mode);
        assert_eq!(field(&host_rec, "n"), "8");
        assert_eq!(field(&host_rec, "verdict"), "ok");
        assert_eq!(field(&host_rec, "reason"), "-");
        assert!(field(&host_rec, "rtt_us").starts_with("n=8,min="), "{}", field(&host_rec, "rtt_us"));
        assert!(field(&host_rec, "resid_us").starts_with("n=8,min="), "{}", field(&host_rec, "resid_us"));
        assert_eq!(field(&guest_rec, "role"), "guest");
        assert_eq!(field(&guest_rec, "n"), "8");
        assert_eq!(field(&guest_rec, "mode"), mode);
        let polls = field(&guest_rec, "polls");
        assert!(polls.split(',').any(|p| p.starts_with("1:")), "{polls}");
        assert!(polls.split(',').any(|p| p.starts_with("8:")), "{polls}");
        assert!(field(&guest_rec, "gap_us").starts_with("n=7,"), "{}", field(&guest_rec, "gap_us"));
        for name in ["ip-hg", "ip-gh"] {
            let bytes = fs::read(dir.join(name)).unwrap();
            assert_eq!(bytes.len(), 64);
            let buf: [u8; 64] = bytes.as_slice().try_into().unwrap();
            assert_eq!(parse_seq(&buf), Some(8));
        }
        let _ = fs::remove_dir_all(&dir);
    }

    fn arm_slow_fsync() {
        io::test_set_hook(Box::new(|| {
            arm_slow_fsync();
            thread::sleep(Duration::from_millis(120));
        }));
    }
    fn arm_pong_jump(start: Instant, n: Arc<AtomicU32>) {
        let n2 = Arc::clone(&n);
        io::test_set_hook(Box::new(move || {
            let k = n2.fetch_add(1, Ordering::SeqCst);
            arm_pong_jump(start, Arc::clone(&n2));
            if k == 1 {
                io::test_set_clock(start + Duration::from_secs(6));
            }
        }));
    }
    fn sample_bound(text: &str, key: &str) -> u64 {
        text.split(',').find_map(|part| part.strip_prefix(key).and_then(|v| v.strip_prefix('=')).and_then(|v| v.parse().ok())).unwrap_or_else(|| panic!("{key} in {text}"))
    }

    #[test]
    fn c11_slow_fsync_is_inside_full_rtt() {
        let _g = guard();
        let dir = scratch("slow-fsync");
        let host_dir = dir.clone();
        let guest_dir = dir.clone();
        let host = thread::spawn(move || {
            arm_slow_fsync();
            exec(&host_dir, "host", "fd", "8", "20")
        });
        let guest = thread::spawn(move || exec(&guest_dir, "guest", "fd", "8", "20"));
        let host_code = host.join().unwrap();
        let guest_code = guest.join().unwrap();
        let host_raw = line_of(&dir, "result-host");
        let guest_raw = line_of(&dir, "result-guest");
        assert_eq!(host_code, 0, "{host_raw}");
        assert_eq!(guest_code, 0, "{guest_raw}");
        let host_rec = decode(host_raw.trim_end_matches('\n'), HOST_KEYS).unwrap_or_else(|| panic!("{host_raw}"));
        assert_eq!(field(&host_rec, "n"), "8");
        assert_eq!(field(&host_rec, "verdict"), "ok");
        let rtt_min = sample_bound(field(&host_rec, "rtt_us"), "min");
        let resid_max = sample_bound(field(&host_rec, "resid_us"), "max");
        assert!(rtt_min >= 50_000, "rtt min {rtt_min} dropped the fsync stall: {}", field(&host_rec, "rtt_us"));
        assert!(rtt_min > resid_max, "rtt min {rtt_min} resid max {resid_max}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn c11_valid_read_past_round_is_not_ok() {
        let _g = guard();
        let start = Instant::now();
        io::test_set_clock(start);
        let err = wait_got(
            || {
                io::test_set_clock(start + Duration::from_secs(6));
                Got::Bytes(encode(1))
            },
            1,
            start + super::ROUND,
            start + Duration::from_secs(30),
        )
        .unwrap_err();
        assert!(matches!(err, WaitErr::Timeout { torn: 0 }), "{err:?}");
    }

    #[test]
    fn c11_last_round_valid_read_past_deadline_is_not_ok() {
        let _g = guard();
        let start = Instant::now();
        io::test_set_clock(start);
        let err = wait_got(
            || {
                io::test_set_clock(start + Duration::from_secs(6));
                Got::Bytes(encode(8))
            },
            8,
            start + super::ROUND,
            start + Duration::from_secs(30),
        )
        .unwrap_err();
        assert!(matches!(err, WaitErr::Timeout { torn: 0 }), "{err:?}");
    }

    #[test]
    fn c11_read_past_command_deadline_is_not_ok() {
        let _g = guard();
        let start = Instant::now();
        io::test_set_clock(start);
        let err = wait_got(
            || {
                io::test_set_clock(start + Duration::from_secs(6));
                Got::Bytes(encode(1))
            },
            1,
            start + Duration::from_secs(30),
            start + Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(matches!(err, WaitErr::Deadline { torn: 0 }), "{err:?}");
    }

    #[test]
    fn c11_guest_pong_past_round_is_not_counted() {
        let _g = guard();
        let dir = scratch("pong-late");
        poke(&dir, "ip-hg", 1);
        let guest_dir = dir.clone();
        let code = thread::spawn(move || {
            let start = Instant::now();
            io::test_set_clock(start);
            arm_pong_jump(start, Arc::new(AtomicU32::new(0)));
            exec(&guest_dir, "guest", "fd", "8", "30")
        })
        .join()
        .unwrap();
        assert_eq!(code, 1);
        let raw = line_of(&dir, "result-guest");
        let rec = decode(raw.trim_end_matches('\n'), GUEST_KEYS).unwrap_or_else(|| panic!("{raw}"));
        assert_eq!(field(&rec, "n"), "0");
        assert_eq!(field(&rec, "verdict"), "unknown");
        assert_eq!(field(&rec, "reason"), "timeout");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn c11_ready_delay_stays_outside_rtt() {
        let _g = guard();
        let dir = scratch("ready-delay");
        let peer_dir = dir.clone();
        let peer = thread::spawn(move || {
            thread::sleep(Duration::from_secs(1));
            poke(&peer_dir, "ip-gh", 0);
            let deadline = Instant::now() + Duration::from_secs(15);
            let mut next = 1u32;
            while Instant::now() < deadline && next <= 8 {
                let bytes = fs::read(peer_dir.join("ip-hg")).unwrap_or_default();
                if bytes.len() == 64 {
                    let buf: [u8; 64] = bytes.as_slice().try_into().unwrap();
                    if parse_seq(&buf) == Some(next) {
                        poke(&peer_dir, "ip-gh", next);
                        next += 1;
                    }
                }
                thread::sleep(Duration::from_millis(1));
            }
        });
        let code = exec(&dir, "host", "fd", "8", "20");
        peer.join().unwrap();
        let raw = line_of(&dir, "result-host");
        assert_eq!(code, 0, "{raw}");
        let rec = decode(raw.trim_end_matches('\n'), HOST_KEYS).unwrap_or_else(|| panic!("{raw}"));
        assert_eq!(field(&rec, "n"), "8");
        assert!(field(&rec, "rtt_us").starts_with("n=8,"), "{}", field(&rec, "rtt_us"));
        let rtt_max = sample_bound(field(&rec, "rtt_us"), "max");
        assert!(rtt_max < 400_000, "startup leaked into rtt max {rtt_max}: {}", field(&rec, "rtt_us"));
        let torn: u64 = field(&rec, "torn").parse().unwrap();
        assert!(torn < 8, "ready placeholders counted as torn={torn}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn c11_ready_stays_placeholder_is_peer_not_ready() {
        let _g = guard();
        let start = Instant::now();
        io::test_set_clock(start);
        io::test_set_wait(Box::new(move || {
            io::test_set_clock(start + Duration::from_secs(6));
        }));
        let dir = scratch("ready-zero");
        let root = io::Dir::open_path(dir.to_str().unwrap()).unwrap();
        let watch = super::Watch::Reopen(&root, super::GUEST_WRITES);
        let err = super::wait_ready(&watch, start + Duration::from_secs(5)).unwrap_err();
        assert!(matches!(err, WaitErr::PeerNotReady { torn: 0 }), "{err:?}");
        let mut bad = encode(1);
        bad[20] ^= 1;
        fs::write(dir.join("ip-gh"), bad).unwrap();
        io::test_set_clock(start);
        io::test_set_wait(Box::new(move || {
            io::test_set_clock(start + Duration::from_secs(6));
        }));
        let err = super::wait_ready(&watch, start + Duration::from_secs(5)).unwrap_err();
        assert!(matches!(err, WaitErr::PeerNotReady { torn: 1 }), "{err:?}");
        poke(&dir, "ip-gh", 3);
        io::test_set_clock(start);
        let err = super::wait_ready(&watch, start + Duration::from_secs(30)).unwrap_err();
        assert!(matches!(err, WaitErr::Skip { torn: 0 }), "{err:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn c11_usage_and_mode_scope() {
        assert!(crate::execute(&own(&["c11", "--dir", "/tmp", "--role", "host", "--count", "8", "--timeout", "1"])).is_err());
        assert!(crate::execute(&own(&["c11", "--dir", "/tmp", "--role", "host", "--mode", "data", "--count", "1", "--timeout", "1"])).is_err());
        assert!(crate::execute(&own(&["c11", "--dir", "/tmp", "--role", "host", "--mode", "fd", "--count", "1", "--timeout", "1", "--kind", "file"])).is_err());
        assert!(crate::execute(&own(&["fill", "--dir", "/tmp", "--role", "guest", "--mode", "fd", "--timeout", "1"])).is_err());
        assert!(crate::execute(&own(&["writer", "--dir", "/tmp", "--role", "host", "--mode", "fd", "--count", "1", "--timeout", "1"])).is_err());
        let missing = format!("/tmp/s11-c11-absent-{}-data", std::process::id());
        let code = crate::execute(&own(&["writer", "--dir", &missing, "--role", "host", "--mode", "data", "--count", "1", "--timeout", "1"])).unwrap();
        assert_eq!(code, 1);
    }
}

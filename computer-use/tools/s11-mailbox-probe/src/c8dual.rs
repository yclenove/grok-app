//! C8 dual-principal mapping. Mechanism measurement only: the verdict is
//! `unknown`, never `ok` or `failed`.
//!
//! ADR counts create, chmod, chown and rename toward the 12-op cap. read,
//! write and unlink are recorded and do not spend the cap. chmod and chown
//! run before rename and unlink so they still see the object. A rename that
//! succeeds is renamed back; the undo is not a second counted rename.
use super::{decode, field, finish, numeric, rec_text, say, Opt};
use crate::io::{self, Dir, RdErr};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

const CAP: u32 = 12;
const PHASES: [&str; 3] = ["a1", "b", "a2"];
const GUEST_NAMES: [&str; 4] = ["a-file", "a-shared", "a-dir", "b-file"];
const HOST_NAMES: [&str; 7] = ["result-a1", "result-b", "result-a2", "a-file", "a-shared", "a-dir", "b-file"];
const PLACEHOLDER: &str = "v=1 op=empty seq=0";
const GUEST_KEYS: &[&str] = &["v", "op", "role", "phase", "verdict", "reason", "self", "ops", "cap", "steps", "ids"];
#[cfg(test)]
const HOST_KEYS: &[&str] = &["v", "op", "role", "verdict", "reason", "n", "counted", "a_steps", "b_steps", "guest_id", "host_id"];

#[derive(Clone, Copy)]
struct Act {
    object: &'static str,
    kind: &'static str,
}

const A1: &[Act] = &[Act { object: "a-file", kind: "create" }, Act { object: "a-file", kind: "chmod" }, Act { object: "a-shared", kind: "create" }, Act { object: "a-shared", kind: "chmod" }, Act { object: "a-dir", kind: "create" }];
const B_ONE: &[&str] = &["read", "write", "chmod", "chown", "rename", "unlink"];
fn b_plan() -> Vec<Act> {
    let mut acts = Vec::with_capacity(21);
    for object in ["a-file", "a-shared", "a-dir"] {
        for kind in B_ONE {
            acts.push(Act { object, kind });
        }
    }
    acts.push(Act { object: "b-file", kind: "create" });
    acts.push(Act { object: "b-file", kind: "chmod" });
    acts.push(Act { object: "b-file", kind: "chown501" });
    acts
}
const A2: &[Act] = &[
    Act { object: "b-file", kind: "read" },
    Act { object: "b-file", kind: "write" },
    Act { object: "b-file", kind: "chmod" },
    Act { object: "b-file", kind: "chown" },
    Act { object: "b-file", kind: "rename" },
    Act { object: "b-file", kind: "unlink" },
];

fn plan(phase: &str) -> Vec<Act> {
    match phase {
        "a1" => A1.to_vec(),
        "b" => b_plan(),
        "a2" => A2.to_vec(),
        _ => Vec::new(),
    }
}

fn counts(kind: &str) -> bool {
    matches!(kind, "create" | "chmod" | "chown" | "chown501" | "rename")
}

/// `Ok(already)` when `kind` is uncounted. `Err` when a counted op would be the 13th.
fn admit(already: u32, kind: &str) -> Result<u32, ()> {
    if !counts(kind) {
        return Ok(already);
    }
    if already >= CAP {
        return Err(());
    }
    Ok(already + 1)
}

#[cfg(test)]
fn counted_of(phase: &str) -> u32 {
    plan(phase).iter().filter(|act| counts(act.kind)).count() as u32
}

pub fn class_raw(errno: Option<i32>) -> &'static str {
    match errno {
        Some(13) => "EACCES",
        Some(1) => "EPERM",
        _ => "other",
    }
}

fn class_err(err: &std::io::Error) -> &'static str {
    class_raw(err.raw_os_error())
}

extern "C" {
    fn getuid() -> u32;
    fn getgid() -> u32;
}

fn self_ids() -> (u32, u32) {
    unsafe { (getuid(), getgid()) }
}

fn chmod_mode(phase: &str, object: &str) -> u32 {
    if object == "a-dir" {
        0o700
    } else if phase == "a1" && object == "a-shared" {
        0o644
    } else {
        0o600
    }
}

fn op_create_file(dir: &Path, name: &str) -> &'static str {
    let path = dir.join(name);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => return "other",
        _ => {}
    }
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true).mode(0o600);
    match opts.open(&path) {
        Ok(_) => "ok",
        Err(err) => class_err(&err),
    }
}

fn op_create_dir(dir: &Path, name: &str) -> &'static str {
    let path = dir.join(name);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => return "other",
        _ => {}
    }
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    match builder.create(&path) {
        Ok(()) => "ok",
        Err(err) => class_err(&err),
    }
}

fn op_chmod(dir: &Path, name: &str, mode: u32) -> &'static str {
    let path = dir.join(name);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => "other",
        Ok(_) => match fs::set_permissions(&path, fs::Permissions::from_mode(mode)) {
            Ok(()) => "ok",
            Err(err) => class_err(&err),
        },
        Err(err) => class_err(&err),
    }
}

fn op_chown(dir: &Path, name: &str, uid: u32) -> &'static str {
    let path = dir.join(name);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => return "other",
        Ok(_) => {}
        Err(err) => return class_err(&err),
    }
    match std::os::unix::fs::lchown(&path, Some(uid), None) {
        Ok(()) => "ok",
        Err(err) => class_err(&err),
    }
}

fn op_read(dir: &Path, name: &str) -> &'static str {
    let path = dir.join(name);
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => return "other",
        Ok(meta) => meta,
        Err(err) => return class_err(&err),
    };
    if meta.is_dir() {
        return match fs::read_dir(&path) {
            Ok(_) => "ok",
            Err(err) => class_err(&err),
        };
    }
    match fs::File::open(&path) {
        Ok(mut file) => {
            let mut buf = [0u8; 1];
            match file.read(&mut buf) {
                Ok(n) => {
                    if n > buf.len() {
                        "other"
                    } else {
                        "ok"
                    }
                }
                Err(err) => class_err(&err),
            }
        }
        Err(err) => class_err(&err),
    }
}

fn op_write(dir: &Path, name: &str) -> &'static str {
    let path = dir.join(name);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => return "other",
        Ok(_) => {}
        Err(err) => return class_err(&err),
    }
    match OpenOptions::new().write(true).create(false).truncate(false).open(&path) {
        Ok(mut file) => match file.write(b"x") {
            Ok(n) => {
                if n == 1 {
                    "ok"
                } else {
                    "other"
                }
            }
            Err(err) => class_err(&err),
        },
        Err(err) => class_err(&err),
    }
}

fn op_rename(dir: &Path, name: &str) -> &'static str {
    let src = dir.join(name);
    let dst = dir.join(format!("{name}.ren"));
    match fs::symlink_metadata(&dst) {
        Ok(_) => return "other",
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return class_err(&err),
    }
    match fs::symlink_metadata(&src) {
        Ok(meta) if meta.file_type().is_symlink() => return "other",
        Ok(_) => {}
        Err(err) => return class_err(&err),
    }
    if let Err(err) = fs::rename(&src, &dst) {
        return class_err(&err);
    }
    // The counted rename already happened. A failed undo leaves `name.ren`.
    let _ = fs::rename(&dst, &src);
    "ok"
}

fn op_unlink(dir: &Path, name: &str) -> &'static str {
    let path = dir.join(name);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => "other",
        Ok(meta) if meta.is_dir() => match fs::remove_dir(&path) {
            Ok(()) => "ok",
            Err(err) => class_err(&err),
        },
        Ok(_) => match fs::remove_file(&path) {
            Ok(()) => "ok",
            Err(err) => class_err(&err),
        },
        Err(err) => class_err(&err),
    }
}

fn perform(dir: &Path, phase: &str, act: &Act, uid: u32) -> &'static str {
    match act.kind {
        "create" if act.object == "a-dir" => op_create_dir(dir, act.object),
        "create" => op_create_file(dir, act.object),
        "chmod" => op_chmod(dir, act.object, chmod_mode(phase, act.object)),
        "read" => op_read(dir, act.object),
        "write" => op_write(dir, act.object),
        "chown" => op_chown(dir, act.object, uid),
        "chown501" => op_chown(dir, act.object, 501),
        "rename" => op_rename(dir, act.object),
        "unlink" => op_unlink(dir, act.object),
        _ => "other",
    }
}

fn ident_tail(path: &Path, host: bool) -> String {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => "symlink".to_string(),
        Ok(meta) => {
            let mode = meta.mode() & 0o7777;
            if host {
                format!("{}:{}:{:o}:{}:{}:{}", meta.uid(), meta.gid(), mode, meta.nlink(), meta.dev(), meta.ino())
            } else {
                format!("{}:{}:{:o}:{}", meta.uid(), meta.gid(), mode, meta.nlink())
            }
        }
        Err(_) => "absent".to_string(),
    }
}

fn guest_ids(dir: &Path) -> String {
    GUEST_NAMES.iter().map(|name| format!("{name}:{}", ident_tail(&dir.join(name), false))).collect::<Vec<_>>().join(",")
}

fn host_ids(dir: &Path) -> String {
    HOST_NAMES.iter().map(|name| format!("{name}:{}", ident_tail(&dir.join(name), true))).collect::<Vec<_>>().join(",")
}

fn steps_text(steps: &[String]) -> String {
    if steps.is_empty() {
        "-".to_string()
    } else {
        steps.join(",")
    }
}

fn guest_line(phase: &str, reason: &str, uid: u32, gid: u32, ops: u32, steps: &str, ids: &str) -> String {
    format!("v=1 op=c8d role=guest phase={phase} verdict=unknown reason={reason} self={uid}:{gid} ops={ops} cap=12 steps={steps} ids={ids}")
}

fn host_line(reason: &str, counted: &str, a_steps: &str, b_steps: &str, guest_id: &str, host_id: &str) -> String {
    format!("v=1 op=c8d role=host verdict=unknown reason={reason} n=2 counted={counted} a_steps={a_steps} b_steps={b_steps} guest_id={guest_id} host_id={host_id}")
}

pub fn failure_line(reason: &str) -> String {
    host_line(reason, "-", "-", "-", "-", "-")
}

fn line_ok(line: &str) -> bool {
    !line.is_empty() && line.len() <= 4095 && !line.contains('\n') && !line.contains('"') && !line.contains('\\')
}

fn run_plan(dir: &Path, phase: &str, end: Instant, uid: u32) -> (Vec<String>, u32, &'static str) {
    let mut steps = Vec::new();
    let mut ops = 0u32;
    for act in plan(phase) {
        if io::past(end) {
            return (steps, ops, "deadline");
        }
        if counts(act.kind) {
            match admit(ops, act.kind) {
                Ok(next) => ops = next,
                Err(()) => return (steps, ops, "cap"),
            }
        }
        let code = perform(dir, phase, &act, uid);
        steps.push(format!("{}:{}:{code}", act.object, act.kind));
    }
    (steps, ops, "observed")
}

fn run_guest(opt: &Opt) -> i32 {
    let (uid, gid) = self_ids();
    let absent = "a-file:absent,a-shared:absent,a-dir:absent,b-file:absent";
    let c8 = Path::new(&opt.dir).join("c8");
    let Some(path) = c8.to_str() else {
        say(&guest_line(opt.boundary, "deadline", uid, gid, 0, "-", absent));
        return 1;
    };
    let slot_dir = match Dir::open_path(path) {
        Ok(dir) => dir,
        Err(_) => {
            say(&guest_line(opt.boundary, "deadline", uid, gid, 0, "-", absent));
            return 1;
        }
    };
    let (steps, ops, reason) = run_plan(&c8, opt.boundary, opt.end, uid);
    let ids = guest_ids(&c8);
    let steps = steps_text(&steps);
    let line = guest_line(opt.boundary, reason, uid, gid, ops, &steps, &ids);
    let late = guest_line(opt.boundary, "deadline", uid, gid, ops, &steps, &ids);
    let line = if line_ok(&line) { line } else { guest_line(opt.boundary, "deadline", uid, gid, ops, "-", &ids) };
    finish(&slot_dir, &format!("result-{}", opt.boundary), &line, "unknown", opt.end, &late)
}

#[derive(Clone, Copy, Debug)]
enum Fault {
    Missing,
    Malformed,
    Timeout,
    Cap,
}

struct View {
    steps: String,
    ids: String,
    ops: u32,
}

fn code_ok(code: &str) -> bool {
    matches!(code, "ok" | "EACCES" | "EPERM" | "other")
}

fn parse_steps(text: &str) -> Option<Vec<(&str, &str, &str)>> {
    if text == "-" {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for part in text.split(',') {
        let mut bits = part.split(':');
        let object = bits.next()?;
        let kind = bits.next()?;
        let code = bits.next()?;
        if bits.next().is_some() || object.is_empty() || kind.is_empty() || !code_ok(code) {
            return None;
        }
        out.push((object, kind, code));
    }
    Some(out)
}

fn id_num(text: &str) -> bool {
    !text.is_empty() && text.len() <= 10 && text.bytes().all(|b| b.is_ascii_digit())
}

fn ids_ok(text: &str) -> bool {
    let parts: Vec<&str> = text.split(',').collect();
    if parts.len() != GUEST_NAMES.len() {
        return false;
    }
    for (name, part) in GUEST_NAMES.iter().zip(parts) {
        let Some(rest) = part.strip_prefix(&format!("{name}:")) else {
            return false;
        };
        if rest == "absent" || rest == "symlink" {
            continue;
        }
        let mut bits = rest.split(':');
        let uid = bits.next().unwrap_or("");
        let gid = bits.next().unwrap_or("");
        let mode = bits.next().unwrap_or("");
        let nlink = bits.next().unwrap_or("");
        if bits.next().is_some() || !id_num(uid) || !id_num(gid) || !id_num(nlink) || mode.is_empty() || !mode.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
    }
    true
}

fn is_prefix(steps: &[(&str, &str, &str)], phase: &str) -> bool {
    let expect = plan(phase);
    if steps.len() > expect.len() {
        return false;
    }
    steps.iter().zip(expect.iter()).all(|((object, kind, _), act)| *object == act.object && *kind == act.kind)
}

fn parse_guest(text: &str, phase: &str) -> Result<View, Fault> {
    let Some(rec) = decode(text, GUEST_KEYS) else {
        return Err(Fault::Malformed);
    };
    if field(&rec, "v") != "1" || field(&rec, "op") != "c8d" || field(&rec, "role") != "guest" || field(&rec, "phase") != phase || field(&rec, "verdict") != "unknown" {
        return Err(Fault::Malformed);
    }
    let reason = field(&rec, "reason");
    if !matches!(reason, "observed" | "deadline" | "cap") {
        return Err(Fault::Malformed);
    }
    let Some((uid, gid)) = field(&rec, "self").split_once(':') else {
        return Err(Fault::Malformed);
    };
    if !id_num(uid) || !id_num(gid) || field(&rec, "cap") != "12" || !ids_ok(field(&rec, "ids")) {
        return Err(Fault::Malformed);
    }
    let ops_text = field(&rec, "ops");
    if !numeric(ops_text) {
        return Err(Fault::Malformed);
    }
    let ops: u32 = ops_text.parse().map_err(|_| Fault::Malformed)?;
    if ops > CAP {
        return Err(Fault::Cap);
    }
    let Some(steps) = parse_steps(field(&rec, "steps")) else {
        return Err(Fault::Malformed);
    };
    let counted = steps.iter().filter(|(_, kind, _)| counts(kind)).count() as u32;
    if ops != counted || !is_prefix(&steps, phase) {
        return Err(Fault::Malformed);
    }
    match reason {
        "deadline" => Err(Fault::Timeout),
        "cap" => Err(Fault::Cap),
        "observed" if steps.len() == plan(phase).len() => Ok(View { steps: field(&rec, "steps").to_string(), ids: field(&rec, "ids").to_string(), ops }),
        _ => Err(Fault::Malformed),
    }
}

fn fault_reason(phase: &str, fault: Fault) -> &'static str {
    match (phase, fault) {
        ("a1", Fault::Missing) => "a1-missing",
        ("b", Fault::Missing) => "b-missing",
        ("a2", Fault::Missing) => "a2-missing",
        ("a1", Fault::Malformed) => "a1-malformed",
        ("b", Fault::Malformed) => "b-malformed",
        ("a2", Fault::Malformed) => "a2-malformed",
        ("a1", Fault::Timeout) => "a1-timeout",
        ("b", Fault::Timeout) => "b-timeout",
        ("a2", Fault::Timeout) => "a2-timeout",
        (_, Fault::Cap) => "cap",
        _ => "io",
    }
}

fn over_principal(a1: u32, a2: u32) -> bool {
    a1.saturating_add(a2) > CAP
}

fn judge_views(views: [Result<View, Fault>; 3], host_id: &str) -> String {
    let mut reason = "observed";
    for (phase, view) in PHASES.iter().zip(views.iter()) {
        if let Err(fault) = view {
            reason = fault_reason(phase, *fault);
            break;
        }
    }
    if let (Ok(a1), Ok(a2)) = (&views[0], &views[2]) {
        if reason == "observed" && over_principal(a1.ops, a2.ops) {
            reason = "cap";
        }
    }
    let both_a = views[0].is_ok() && views[2].is_ok();
    let b_ok = views[1].is_ok();
    let a_steps = if both_a { format!("{},{}", views[0].as_ref().unwrap().steps, views[2].as_ref().unwrap().steps) } else { "-".to_string() };
    let b_steps = if b_ok { views[1].as_ref().unwrap().steps.clone() } else { "-".to_string() };
    let counted = if both_a && b_ok {
        let sum = views[0].as_ref().unwrap().ops + views[2].as_ref().unwrap().ops;
        format!("{sum},{}", views[1].as_ref().unwrap().ops)
    } else {
        "-".to_string()
    };
    let mut guest_parts = Vec::new();
    for (phase, view) in PHASES.iter().zip(views.iter()) {
        if let Ok(view) = view {
            guest_parts.push(format!("{phase}={}", view.ids));
        }
    }
    let guest_id = if guest_parts.is_empty() { "-".to_string() } else { guest_parts.join(";") };
    let host_id = if host_id.is_empty() { "-" } else { host_id };
    let line = host_line(reason, &counted, &a_steps, &b_steps, &guest_id, host_id);
    if line_ok(&line) {
        line
    } else {
        failure_line("io")
    }
}

#[cfg(test)]
fn judge_texts(phases: [Option<&str>; 3], host_id: &str) -> String {
    let views = [0, 1, 2].map(|i| match phases[i] {
        None => Err(Fault::Missing),
        Some(text) => parse_guest(text, PHASES[i]),
    });
    judge_views(views, host_id)
}

fn wait_retry(retry_end: Instant, end: Instant) -> bool {
    if io::past(retry_end) || io::past(end) {
        return false;
    }
    let remain = retry_end.saturating_duration_since(io::now());
    if remain.is_zero() {
        return false;
    }
    thread::sleep(Duration::from_millis(50).min(remain));
    !io::past(end)
}

fn read_phase(dir: &Dir, phase: &str, end: Instant) -> Result<Option<String>, Fault> {
    let name = format!("result-{phase}");
    let retry_end = io::now() + Duration::from_secs(2);
    let retry_end = if retry_end < end { retry_end } else { end };
    loop {
        if io::past(end) {
            return Err(Fault::Timeout);
        }
        match dir.read_file(&name, 4096, end) {
            Ok((bytes, _, _, _)) => {
                let Some(text) = rec_text(&bytes) else {
                    return Err(Fault::Malformed);
                };
                if text == PLACEHOLDER {
                    if !wait_retry(retry_end, end) {
                        return if io::past(end) { Err(Fault::Timeout) } else { Ok(None) };
                    }
                    continue;
                }
                return Ok(Some(text.to_string()));
            }
            Err(RdErr::Absent) => {
                if !wait_retry(retry_end, end) {
                    return if io::past(end) { Err(Fault::Timeout) } else { Ok(None) };
                }
            }
            Err(RdErr::Deadline) => return Err(Fault::Timeout),
            Err(_) => return Err(Fault::Malformed),
        }
    }
}

fn run_host(opt: &Opt) -> i32 {
    let root = match Dir::open_path(&opt.dir) {
        Ok(dir) => dir,
        Err(_) => {
            say(&failure_line("io"));
            return 1;
        }
    };
    let c8_path = Path::new(&opt.dir).join("c8");
    let Some(c8_text) = c8_path.to_str() else {
        let line = failure_line("io");
        return finish(&root, "result-host", &line, "unknown", opt.end, &line);
    };
    let c8 = match Dir::open_path(c8_text) {
        Ok(dir) => dir,
        Err(_) => {
            let line = failure_line("io");
            return finish(&root, "result-host", &line, "unknown", opt.end, &line);
        }
    };
    if io::past(opt.end) {
        let line = failure_line("deadline");
        return finish(&root, "result-host", &line, "unknown", opt.end, &line);
    }
    let mut texts: [Option<String>; 3] = [None, None, None];
    let mut faults: [Option<Fault>; 3] = [None, None, None];
    for (i, phase) in PHASES.iter().enumerate() {
        match read_phase(&c8, phase, opt.end) {
            Ok(Some(text)) => texts[i] = Some(text),
            Ok(None) => faults[i] = Some(Fault::Missing),
            Err(fault) => faults[i] = Some(fault),
        }
    }
    let host_id = host_ids(&c8_path);
    let views = [0, 1, 2].map(|i| {
        if let Some(fault) = faults[i] {
            return Err(fault);
        }
        match texts[i].as_deref() {
            Some(text) => parse_guest(text, PHASES[i]),
            None => Err(Fault::Missing),
        }
    });
    let line = judge_views(views, &host_id);
    let late = failure_line("deadline");
    finish(&root, "result-host", &line, "unknown", opt.end, &late)
}

pub fn run(opt: &Opt) -> i32 {
    if opt.boundary == "host" {
        run_host(opt)
    } else {
        run_guest(opt)
    }
}

pub fn deadline(opt: &Opt) -> i32 {
    if opt.boundary == "host" {
        let line = failure_line("deadline");
        if let Ok(dir) = Dir::open_path(&opt.dir) {
            return finish(&dir, "result-host", &line, "unknown", opt.end, &line);
        }
        say(&line);
        return 1;
    }
    let (uid, gid) = self_ids();
    let c8 = Path::new(&opt.dir).join("c8");
    let ids = if c8.is_dir() { guest_ids(&c8) } else { "a-file:absent,a-shared:absent,a-dir:absent,b-file:absent".to_string() };
    let line = guest_line(opt.boundary, "deadline", uid, gid, 0, "-", &ids);
    if let Some(path) = c8.to_str() {
        if let Ok(dir) = Dir::open_path(path) {
            return finish(&dir, &format!("result-{}", opt.boundary), &line, "unknown", opt.end, &line);
        }
    }
    say(&line);
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_ids() -> String {
        "a-file:501:20:600:1,a-shared:501:20:644:1,a-dir:501:20:700:2,b-file:absent".to_string()
    }

    fn synth(phase: &str, reason: &str, ops: u32, steps: &str) -> String {
        guest_line(phase, reason, 501, 20, ops, steps, &sample_ids())
    }

    fn planned(phase: &str) -> (String, u32) {
        let steps = plan(phase).iter().map(|act| format!("{}:{}:ok", act.object, act.kind)).collect::<Vec<_>>().join(",");
        (steps, counted_of(phase))
    }

    #[test]
    fn class_raw_maps_eacces_eperm_and_other() {
        assert_eq!(class_raw(Some(13)), "EACCES");
        assert_eq!(class_raw(Some(1)), "EPERM");
        assert_eq!(class_raw(Some(2)), "other");
        assert_eq!(class_raw(Some(21)), "other");
        assert_eq!(class_raw(None), "other");
    }

    #[test]
    fn counted_plans_and_thirteenth_refused() {
        assert_eq!(counted_of("a1"), 5);
        assert_eq!(counted_of("b"), 12);
        assert_eq!(counted_of("a2"), 3);
        assert_eq!(counted_of("a1") + counted_of("a2"), 8);
        assert!(counted_of("a1") <= CAP && counted_of("b") <= CAP && counted_of("a2") <= CAP);
        let mut n = 0u32;
        for _ in 0..CAP {
            n = admit(n, "create").unwrap();
        }
        assert!(admit(n, "create").is_err());
        assert!(admit(n, "chmod").is_err());
        assert!(admit(n, "chown").is_err());
        assert!(admit(n, "chown501").is_err());
        assert!(admit(n, "rename").is_err());
        assert_eq!(admit(n, "read").unwrap(), n);
        assert_eq!(admit(n, "write").unwrap(), n);
        assert_eq!(admit(n, "unlink").unwrap(), n);
    }

    #[test]
    fn phase_order_is_a1_then_b_then_a2() {
        assert_eq!(PHASES, ["a1", "b", "a2"]);
        assert_eq!(plan("a1")[0].object, "a-file");
        assert_eq!(plan("a1")[0].kind, "create");
        assert_eq!(plan("b")[0].kind, "read");
        assert_eq!(plan("b").last().unwrap().kind, "chown501");
        assert_eq!(plan("a2")[0].object, "b-file");
        assert_eq!(plan("a2")[0].kind, "read");
        let b = plan("b");
        let objects: Vec<&str> = b.iter().take(18).map(|act| act.object).collect();
        assert_eq!(&objects[..6], ["a-file"; 6]);
        assert_eq!(&objects[6..12], ["a-shared"; 6]);
        assert_eq!(&objects[12..18], ["a-dir"; 6]);
        assert_eq!(B_ONE, ["read", "write", "chmod", "chown", "rename", "unlink"]);
    }

    #[test]
    fn records_round_trip_and_reject_tamper() {
        let (steps, ops) = planned("a1");
        let guest = synth("a1", "observed", ops, &steps);
        let rec = decode(&guest, GUEST_KEYS).expect("guest decode");
        assert_eq!(field(&rec, "op"), "c8d");
        assert_eq!(field(&rec, "phase"), "a1");
        assert_eq!(field(&rec, "ops"), "5");
        assert_eq!(field(&rec, "verdict"), "unknown");
        let host = host_line("observed", "8,12", &steps, "b-file:create:ok", &format!("a1={}", sample_ids()), "a-file:1:1:600:1:1:2");
        let rec = decode(&host, HOST_KEYS).expect("host decode");
        assert_eq!(field(&rec, "role"), "host");
        assert_eq!(field(&rec, "n"), "2");
        assert_eq!(field(&rec, "reason"), "observed");
        assert!(decode(&format!("{guest} op=c8d"), GUEST_KEYS).is_none());
        assert!(decode("v=1 op=c8d", GUEST_KEYS).is_none());
        let dup = format!("{guest} ops=5");
        assert!(decode(&dup, GUEST_KEYS).is_none());
        let judged = judge_texts([Some(&dup), None, None], "-");
        assert!(judged.contains("reason=a1-malformed"), "{judged}");
        let bad_code = guest.replace("a-file:create:ok", "a-file:create:NOPE");
        let judged = judge_texts([Some(&bad_code), None, None], "a-file:absent");
        assert!(judged.contains("reason=a1-malformed"), "{judged}");
        let missing_key = guest.replacen(" steps=", " dropped=", 1);
        assert!(decode(&missing_key, GUEST_KEYS).is_none());
        let judged = judge_texts([Some(&missing_key), None, None], "-");
        assert!(judged.contains("reason=a1-malformed"), "{judged}");
    }

    #[test]
    fn judge_observed_missing_malformed_timeout_and_cap() {
        let (a1, a1_ops) = planned("a1");
        let (b, b_ops) = planned("b");
        let (a2, a2_ops) = planned("a2");
        let host_id = "a-file:1000:1000:600:1:1:2,result-a1:1000:1000:600:1:1:9";
        let phases = [synth("a1", "observed", a1_ops, &a1), synth("b", "observed", b_ops, &b), synth("a2", "observed", a2_ops, &a2)];
        let line = judge_texts([Some(&phases[0]), Some(&phases[1]), Some(&phases[2])], host_id);
        assert!(line.contains("verdict=unknown"), "{line}");
        assert!(line.contains("reason=observed"), "{line}");
        assert!(line.contains("n=2"), "{line}");
        assert!(line.contains("counted=8,12"), "{line}");
        assert!(line.contains("a-file:create:ok"), "{line}");
        assert!(line.contains("b-file:chown501:ok"), "{line}");
        assert!(!line.contains("verdict=ok"), "{line}");
        assert!(!line.contains("verdict=failed"), "{line}");
        assert!(decode(line.trim(), HOST_KEYS).is_some(), "{line}");

        let missing = judge_texts([None, Some(&phases[1]), Some(&phases[2])], host_id);
        assert!(missing.contains("reason=a1-missing"), "{missing}");
        assert!(missing.contains("a_steps=-"), "{missing}");
        assert!(missing.contains("verdict=unknown"), "{missing}");

        let malformed = judge_texts([Some("not a record"), Some(&phases[1]), None], host_id);
        assert!(malformed.contains("reason=a1-malformed"), "{malformed}");

        let deadline = synth("a1", "deadline", 0, "-");
        let timed = judge_texts([Some(&deadline), None, None], host_id);
        assert!(timed.contains("reason=a1-timeout"), "{timed}");

        let b_deadline = synth("b", "deadline", 0, "-");
        let timed_b = judge_texts([Some(&phases[0]), Some(&b_deadline), Some(&phases[2])], host_id);
        assert!(timed_b.contains("reason=b-timeout"), "{timed_b}");

        let capped = synth("b", "cap", 12, &b);
        let cap_line = judge_texts([Some(&phases[0]), Some(&capped), Some(&phases[2])], host_id);
        assert!(cap_line.contains("reason=cap"), "{cap_line}");

        let over = synth("a1", "observed", 13, &a1);
        let over_line = judge_texts([Some(&over), Some(&phases[1]), Some(&phases[2])], host_id);
        assert!(over_line.contains("reason=cap"), "{over_line}");
        assert!(over_principal(8, 5));
        assert!(!over_principal(5, 3));
    }

    #[test]
    fn same_uid_run_records_creates_and_denials() {
        let n = std::process::id();
        let base = std::env::temp_dir().join(format!("s11-c8d-{n}"));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let base = fs::canonicalize(&base).unwrap();
        let mbx = base.join("mailbox");
        fs::create_dir(&mbx).unwrap();
        let c8 = mbx.join("c8");
        fs::create_dir(&c8).unwrap();
        fs::set_permissions(&c8, fs::Permissions::from_mode(0o700)).unwrap();
        for name in ["result-a1", "result-b", "result-a2"] {
            fs::write(c8.join(name), b"v=1 op=empty seq=0\n").unwrap();
        }
        let dir = mbx.to_str().unwrap().to_string();
        let run_phase = |phase: &str, role: &str| {
            let args = ["c8dual".to_string(), phase.to_string(), "--dir".to_string(), dir.clone(), "--role".to_string(), role.to_string(), "--timeout".to_string(), "30".to_string()];
            crate::execute(&args).unwrap()
        };
        assert_eq!(run_phase("a1", "guest"), 1);
        assert_eq!(run_phase("b", "guest"), 1);
        assert_eq!(run_phase("a2", "guest"), 1);
        assert_eq!(run_phase("host", "host"), 1);
        let host = fs::read_to_string(mbx.join("result-host")).unwrap();
        assert!(host.contains("verdict=unknown"), "{host}");
        assert!(host.contains("reason=observed"), "{host}");
        assert!(host.contains("counted=8,12"), "{host}");
        assert!(host.contains("a-file:create:ok"), "{host}");
        assert!(host.contains("a-shared:chmod:ok"), "{host}");
        assert!(host.contains("a-dir:create:ok"), "{host}");
        assert!(host.contains("b-file:create:ok"), "{host}");
        assert!(!host.contains("verdict=ok"), "{host}");
        assert!(!host.contains("verdict=failed"), "{host}");
        let uid = self_ids().0;
        if uid != 0 && uid != 501 {
            assert!(host.contains("b-file:chown501:EPERM"), "{host}");
        }
        for phase in ["a1", "b", "a2"] {
            let text = fs::read_to_string(c8.join(format!("result-{phase}"))).unwrap();
            let rec = decode(text.trim_end_matches('\n'), GUEST_KEYS).unwrap_or_else(|| panic!("{text}"));
            assert_eq!(field(&rec, "phase"), phase);
            assert_eq!(field(&rec, "cap"), "12");
            assert_eq!(field(&rec, "verdict"), "unknown");
        }
        let a1 = fs::read_to_string(c8.join("result-a1")).unwrap();
        assert!(a1.contains("ops=5"), "{a1}");
        let b = fs::read_to_string(c8.join("result-b")).unwrap();
        assert!(b.contains("ops=12"), "{b}");
        let a2 = fs::read_to_string(c8.join("result-a2")).unwrap();
        assert!(a2.contains("ops=3"), "{a2}");
        assert!(crate::execute(&["c8dual".into(), "a1".into(), "--dir".into(), dir, "--role".into(), "host".into(), "--timeout".into(), "2".into()]).is_err());
        let _ = fs::remove_dir_all(&base);
    }
}

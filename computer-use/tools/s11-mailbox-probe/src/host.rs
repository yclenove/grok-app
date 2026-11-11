//! Host driver for one case. External commands go through `Runner`.

#![allow(clippy::too_many_arguments)]

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use crate::argv::{self, closed_program};
use crate::budget::{
    Deadline, EVAL_KEY_FILE, GUEST_DIR, MS_CLONE, MS_DELETE, MS_KEYGEN, MS_QUERY, MS_SSH, MS_STOP,
    UPLOAD_CAP,
};
use crate::cases::{self, Case, Row};
use crate::clone::{self, Absent};
use crate::dispatcher::{self, dispatcher_script, production_init};
use crate::identity::{self, Tree};
use crate::osutil::{self, Held};
use crate::runner::{self, host_cmd, CmdOut, Runner};
use crate::sha256::{self, hex_digest, is_hex32};
use crate::ssh;

#[derive(Clone, Debug)]
pub struct RunRequest {
    pub case: Case,
    pub tart: PathBuf,
    pub home: PathBuf,
    pub out: PathBuf,
    pub tart_home: Option<PathBuf>,
    pub dry_run: bool,
    pub pinned: Option<PathBuf>,
    pub probe: Vec<u8>,
    pub attempt: Option<String>,
    pub share: Option<String>,
    pub deadline: Deadline,
}

#[derive(Debug)]
pub struct RunOutcome {
    pub verdict: String,
    pub cleanup_absent: bool,
    pub report: String,
}

pub fn real_path_supported() -> bool {
    cfg!(target_os = "macos")
}

pub fn guest_entry(case: Case) -> i32 {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = case;
        2
    }
    #[cfg(target_os = "macos")]
    {
        match guest_dispatch(case) {
            Ok(()) => 0,
            Err("fault-exit") => 86,
            Err(_) => 1,
        }
    }
}

#[cfg(target_os = "macos")]
fn guest_dispatch(case: Case) -> Result<(), &'static str> {
    let dir = Path::new(GUEST_DIR);
    let share = std::fs::read_to_string(dir.join("share-name")).map_err(|_| "share")?;
    let sub = std::fs::read_to_string(dir.join("subcase")).map_err(|_| "subcase")?;
    let share = share.trim();
    let sub = sub.trim();
    if !is_hex32(share) || !dispatcher::subcase_ok(sub) {
        return Err("token");
    }
    let mailbox = Path::new(crate::budget::SHARE_VOLUME).join(share);
    let mut proc = crate::proc::ProcessRunner::new();
    match case {
        Case::C1 => cases::guest_c1(&mailbox),
        Case::C2 => cases::guest_c2(&mailbox, sub),
        Case::C8 => cases::guest_c8(&mut proc, &mailbox),
    }
}

pub fn run_case(req: &RunRequest, runner: &mut dyn Runner) -> Result<RunOutcome, &'static str> {
    if req.deadline.past() {
        let rows: Vec<Row> = req
            .case
            .subcases()
            .iter()
            .copied()
            .map(Row::not_run)
            .collect();
        return finish(req, "unknown", true, Some("deadline"), &rows, false);
    }
    if !path_ok(&req.out) || closed_program(&[req.tart.display().to_string()]).is_err() {
        return Err("path");
    }
    let parent = req.out.parent().ok_or("path")?;
    let leaf = req.out.file_name().and_then(|s| s.to_str()).ok_or("path")?;
    let parent_held = osutil::verify_private_leaf(parent)?;
    if child_exists(&parent_held.file, leaf)? {
        return Err("out-exists");
    }
    if req.dry_run {
        let out = osutil::mkdir_0700_at(&parent_held.file, leaf).map_err(|_| "mkdir")?;
        drop(out);
        let rows: Vec<Row> = req
            .case
            .subcases()
            .iter()
            .copied()
            .map(Row::not_run)
            .collect();
        return finish(req, "dry_run", true, None, &rows, true);
    }
    if req.probe.is_empty() || req.probe.len() > UPLOAD_CAP {
        return Err("probe");
    }
    let pinned_path = req.pinned.as_deref().ok_or("pinned-host-key")?;
    let pinned = ssh::load_pinned(pinned_path)?;
    let (attempt, share) = ids(req)?;
    let out_file = osutil::mkdir_0700_at(&parent_held.file, leaf).map_err(|_| "mkdir")?;
    let out_held = osutil::held_of(out_file).map_err(|_| "mkdir")?;
    if osutil::verify_private_leaf(&req.out).is_err() {
        return Err("private-root");
    }
    let mut rows = Vec::new();
    let mut stop = false;
    let mut all_absent = true;
    for sub in req.case.subcases() {
        if stop || budget_for(req, true, MS_QUERY).is_err() {
            rows.push(Row::not_run(sub));
            continue;
        }
        let (row, absent) = run_sub(req, runner, &out_held, sub, &attempt, &share, &pinned)?;
        if !absent {
            all_absent = false;
            stop = true;
        }
        if row.verdict == "quarantine" {
            stop = true;
        }
        rows.push(row);
    }
    let folded = cases::fold_verdicts(&rows.iter().map(|r| r.verdict).collect::<Vec<_>>());
    let verdict = if all_absent { folded } else { "quarantine" };
    finish(req, verdict, all_absent, None, &rows, true)
}

fn finish(
    req: &RunRequest,
    verdict: &str,
    cleanup_absent: bool,
    reason: Option<&str>,
    rows: &[Row],
    write: bool,
) -> Result<RunOutcome, &'static str> {
    let bytes = crate::report::render(req.case.as_str(), verdict, cleanup_absent, reason, rows)?;
    let report = String::from_utf8(bytes).map_err(|_| "report")?;
    if write {
        if let Ok(held) = osutil::verify_private_leaf(&req.out) {
            if let Ok(mut file) = osutil::create_0600_at(&held.file, "report.json") {
                let _ = osutil::write_all_fd(&mut file, report.as_bytes());
            }
        }
    }
    Ok(RunOutcome {
        verdict: verdict.to_string(),
        cleanup_absent,
        report,
    })
}

fn run_sub(
    req: &RunRequest,
    runner: &mut dyn Runner,
    out: &Held,
    sub: &'static str,
    attempt: &str,
    share: &str,
    pinned: &str,
) -> Result<(Row, bool), &'static str> {
    let name = clone::clone_name(attempt)?;
    let mut tree = Tree::new();
    let mut owned = false;
    let mut spawn = None;
    let (base_path, base_held) = add_dir(&mut tree, out, &req.out, &name, true)?;
    let (stage, _) = add_dir(&mut tree, &base_held, &base_path, "host-stage", false)?;
    let (mailbox, mb) = add_dir(&mut tree, &base_held, &base_path, "mailbox", true)?;
    let _ = add_dir(&mut tree, &mb, &mailbox, "host-write", true)?;
    let _ = add_dir(&mut tree, &mb, &mailbox, "guest-write", true)?;
    let (ssh_dir, _) = add_dir(&mut tree, &base_held, &base_path, "ssh", false)?;
    let built = (|| -> Result<Row, &'static str> {
        if mb.fsid.is_none() {
            return Ok(cases::unknown_row(sub, "fsid-unknown"));
        }
        let listed = call_list(req, runner)?;
        match clone::precheck(&Ok(listed), &name) {
            clone::Precheck::Ok => {}
            clone::Precheck::Stop { reason } => return Ok(cases::unknown_row(sub, reason)),
        }
        let cloned = call(
            req,
            runner,
            argv::tart_clone(&req.tart, &name),
            MS_CLONE,
            true,
            Vec::new(),
            false,
        )?;
        let end = cmd_end(&cloned);
        let after = call_list(req, runner);
        let outcome = clone::decide_after_clone(end, &after, &name);
        if outcome != clone::CloneOutcome::Owned {
            let reason = match outcome {
                clone::CloneOutcome::NotCreated { reason }
                | clone::CloneOutcome::Ambiguous { reason } => reason,
                clone::CloneOutcome::Owned => "clone",
            };
            return Ok(cases::unknown_row(sub, reason));
        }
        owned = true;
        let left = req.deadline.remaining();
        if left.as_millis() == 0 {
            return Ok(cases::unknown_row(sub, "deadline"));
        }
        let run_cmd = host_cmd(
            argv::tart_run(&req.tart, &name, share, &mailbox),
            left,
            Vec::new(),
            req.tart_home.as_deref(),
            true,
        )?;
        spawn = Some(runner.spawn(&run_cmd)?);
        let ip = wait_ip(req, runner, &name)?;
        let known = ssh::known_hosts_line(&ip, pinned)?;
        write_recorded(&mut tree, &ssh_dir, "known_hosts", known.as_bytes())?;
        let key = ssh_dir.join("id_ed25519");
        let gen = call(
            req,
            runner,
            argv::ssh_keygen(&key),
            MS_KEYGEN,
            true,
            Vec::new(),
            false,
        )?;
        if !gen.clean_exit() {
            return Ok(cases::unknown_row(sub, "keygen"));
        }
        record_name(&mut tree, &ssh_dir, "id_ed25519")?;
        record_name(&mut tree, &ssh_dir, "id_ed25519.pub")?;
        let pub_bytes = read_child(&ssh_dir, "id_ed25519.pub", 4096)?;
        let pub_text = std::str::from_utf8(&pub_bytes).map_err(|_| "keygen")?;
        let body = ssh::parse_pubkey_line(pub_text)?;
        let auth = ssh::auth_line(&body)?;
        let disp = dispatcher_script(GUEST_DIR)?;
        let digest = hex_digest(disp.as_bytes());
        let init = production_init(&digest, &auth, share, sub)?;
        if !dispatcher::is_init_script(&init) || !dispatcher::remote_allowed(&init, &init) {
            return Err("init");
        }
        let eval_key = req.home.join(EVAL_KEY_FILE);
        if !regular_file(&eval_key) {
            return Ok(cases::unknown_row(sub, "eval-key"));
        }
        let init_out = ssh_exec(
            req,
            runner,
            &name,
            &eval_key,
            &ssh_dir.join("known_hosts"),
            &ip,
            &init,
            disp.into_bytes(),
        )?;
        if !init_out.clean_exit() || init_out.stdout != b"init-ok\n" {
            return Ok(cases::unknown_row(sub, "init-failed"));
        }
        let case = req.case.as_str();
        let upload = format!("upload {case}");
        let verify = format!("verify {}", hex_digest(&req.probe));
        let run = format!("run {case}");
        for remote in [&upload, &verify, &run] {
            if !dispatcher::remote_allowed(remote, &init) {
                return Err("remote");
            }
        }
        let up = ssh_exec(
            req,
            runner,
            &name,
            &key,
            &ssh_dir.join("known_hosts"),
            &ip,
            &upload,
            req.probe.clone(),
        )?;
        if !up.clean_exit() || up.stdout != b"upload-ok\n" {
            return Ok(cases::unknown_row(sub, "upload-failed"));
        }
        let ver = ssh_exec(
            req,
            runner,
            &name,
            &key,
            &ssh_dir.join("known_hosts"),
            &ip,
            &verify,
            Vec::new(),
        )?;
        if !ver.clean_exit() || ver.stdout != b"verify-ok\n" {
            return Ok(cases::unknown_row(sub, "verify-failed"));
        }
        run_guest(
            req,
            runner,
            &name,
            &key,
            &ssh_dir.join("known_hosts"),
            &ip,
            &run,
            &mailbox,
            &stage,
            sub,
        )
    })();
    let absent = teardown(req, runner, &name, owned, spawn.take(), &tree);
    let mut row = match built {
        Ok(row) => row,
        Err(err) => cases::unknown_row(sub, err),
    };
    if !absent {
        row.verdict = "quarantine";
        row.unknown_reason = Some("cleanup");
    }
    Ok((row, absent))
}

fn run_guest(
    req: &RunRequest,
    runner: &mut dyn Runner,
    name: &str,
    key: &Path,
    known: &Path,
    ip: &str,
    remote: &str,
    mailbox: &Path,
    stage: &Path,
    sub: &'static str,
) -> Result<Row, &'static str> {
    let listed = call_list(req, runner)?;
    if !clone::sole_running(&listed, name) {
        return Err("not-sole");
    }
    if req.case == Case::C1 {
        let timeout = budget_for(req, true, MS_SSH)?;
        let cmd = host_cmd(
            argv::ssh(key, known, ip, remote),
            timeout,
            Vec::new(),
            None,
            false,
        )?;
        let id = runner.spawn(&cmd)?;
        let row = cases::drive_c1(mailbox, stage);
        let out = runner.finish(id);
        if out.reap_unknown {
            return Ok(cases::unknown_row(sub, "guest-reap"));
        }
        return Ok(row);
    }
    let out = ssh_exec(req, runner, name, key, known, ip, remote, Vec::new())?;
    let status = if out.timed_out || out.reap_unknown || out.io_error {
        None
    } else {
        out.status
    };
    Ok(match req.case {
        Case::C2 => cases::observe_c2(mailbox, sub, status),
        Case::C8 => cases::observe_c8(mailbox),
        Case::C1 => cases::unknown_row(sub, "case"),
    })
}

fn teardown(
    req: &RunRequest,
    runner: &mut dyn Runner,
    name: &str,
    owned: bool,
    spawn: Option<u32>,
    tree: &Tree,
) -> bool {
    if req.deadline.past() {
        if let Some(id) = spawn {
            let _ = runner.finish(id);
        }
        return false;
    }
    let mut vm_absent = !owned;
    if owned && !matches!(clone::decide_delete(true, name), clone::DeletePlan::Delete) {
        vm_absent = false;
    } else if owned {
        let stopped = match budget_for(req, false, MS_STOP) {
            Ok(timeout) => {
                match exec(
                    runner,
                    argv::tart_stop(&req.tart, name),
                    timeout,
                    Vec::new(),
                    req.tart_home.as_deref(),
                    false,
                ) {
                    Ok(out) if !out.reap_unknown && !out.io_error => match call_list(req, runner) {
                        Ok(vms) => clone::confirm_stopped(&Ok(vms), name).is_ok(),
                        Err(_) => false,
                    },
                    _ => false,
                }
            }
            Err(_) => false,
        };
        let reap_ok = if let Some(id) = spawn {
            let out = runner.finish(id);
            out.status.is_some() && !out.reap_unknown
        } else {
            true
        };
        if stopped && reap_ok {
            if let Ok(timeout) = budget_for(req, false, MS_DELETE) {
                let _ = exec(
                    runner,
                    argv::tart_delete(&req.tart, name),
                    timeout,
                    Vec::new(),
                    req.tart_home.as_deref(),
                    false,
                );
                vm_absent = match call_list(req, runner) {
                    Ok(vms) => clone::confirm_absent(&Ok(vms), name) == Absent::Yes,
                    Err(_) => false,
                };
            }
        }
    }
    if req.deadline.past() {
        return false;
    }
    identity::cleanup(tree).is_ok() && vm_absent
}

fn wait_ip(req: &RunRequest, runner: &mut dyn Runner, name: &str) -> Result<String, &'static str> {
    loop {
        if budget_for(req, true, MS_QUERY).is_err() {
            return Err("vm-not-ready");
        }
        if let Ok(vms) = call_list(req, runner) {
            if clone::sole_running(&vms, name) {
                let out = call(
                    req,
                    runner,
                    argv::tart_ip(&req.tart, name),
                    MS_QUERY,
                    true,
                    Vec::new(),
                    false,
                )?;
                if let Ok(text) = runner::trusted_text(&out) {
                    if let Ok(ip) = ssh::parse_single_ipv4(text) {
                        if let Ok(again) = call_list(req, runner) {
                            if clone::sole_running(&again, name) {
                                return Ok(ip);
                            }
                        }
                    }
                }
            }
        }
        if budget_for(req, true, MS_QUERY).is_err() {
            return Err("vm-not-ready");
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn call_list(
    req: &RunRequest,
    runner: &mut dyn Runner,
) -> Result<Vec<crate::inventory::Vm>, &'static str> {
    let timeout = budget_for(req, true, MS_QUERY).or_else(|_| budget_for(req, false, MS_QUERY))?;
    let out = exec(
        runner,
        argv::tart_list(&req.tart),
        timeout,
        Vec::new(),
        req.tart_home.as_deref(),
        false,
    )?;
    if !out.clean_exit() {
        return Err("list-untrusted");
    }
    let text = std::str::from_utf8(&out.stdout).map_err(|_| "list-untrusted")?;
    clone::list_from_end(clone::ToolEnd::Exited(0), text)
}

fn ssh_exec(
    req: &RunRequest,
    runner: &mut dyn Runner,
    name: &str,
    key: &Path,
    known: &Path,
    ip: &str,
    remote: &str,
    stdin: Vec<u8>,
) -> Result<CmdOut, &'static str> {
    if budget_for(req, true, MS_QUERY).is_err() {
        return Err("deadline");
    }
    let vms = call_list(req, runner)?;
    if !clone::sole_running(&vms, name) {
        return Err("not-sole");
    }
    call(
        req,
        runner,
        argv::ssh(key, known, ip, remote),
        MS_SSH,
        true,
        stdin,
        false,
    )
}

fn call(
    req: &RunRequest,
    runner: &mut dyn Runner,
    argv: Vec<String>,
    cap: u64,
    experiment: bool,
    stdin: Vec<u8>,
    null_stdio: bool,
) -> Result<CmdOut, &'static str> {
    let timeout = budget_for(req, experiment, cap)?;
    exec(
        runner,
        argv,
        timeout,
        stdin,
        req.tart_home.as_deref(),
        null_stdio,
    )
}

fn exec(
    runner: &mut dyn Runner,
    argv: Vec<String>,
    timeout: Duration,
    stdin: Vec<u8>,
    tart_home: Option<&Path>,
    null_stdio: bool,
) -> Result<CmdOut, &'static str> {
    let cmd = host_cmd(argv, timeout, stdin, tart_home, null_stdio)?;
    Ok(runner.run(&cmd))
}

fn budget_for(req: &RunRequest, experiment: bool, cap: u64) -> Result<Duration, &'static str> {
    let found = if experiment {
        req.deadline.experiment(cap)
    } else {
        req.deadline.cleanup(cap)
    };
    match found {
        Some(d) if d.as_millis() >= 1 => Ok(d),
        _ => Err("deadline"),
    }
}

fn cmd_end(out: &CmdOut) -> clone::ToolEnd {
    if out.timed_out {
        clone::ToolEnd::Timeout
    } else if out.overflow {
        clone::ToolEnd::Overflow
    } else if out.reap_unknown || out.io_error {
        clone::ToolEnd::Timeout
    } else {
        clone::ToolEnd::Exited(out.status.unwrap_or(1))
    }
}

fn add_dir(
    tree: &mut Tree,
    parent: &Held,
    parent_path: &Path,
    name: &str,
    retain: bool,
) -> Result<(PathBuf, Held), &'static str> {
    let file = osutil::mkdir_0700_at(&parent.file, name).map_err(|_| "mkdir")?;
    tree.record_dir(parent, parent_path, name, &file, retain)?;
    let path = parent_path.join(name);
    let held = osutil::held_of(file).map_err(|_| "mkdir")?;
    Ok((path, held))
}

fn write_recorded(
    tree: &mut Tree,
    dir: &Path,
    name: &str,
    bytes: &[u8],
) -> Result<(), &'static str> {
    let held = osutil::verify_private_leaf(dir)?;
    let mut file = osutil::create_0600_at(&held.file, name).map_err(|_| "write")?;
    osutil::write_all_fd(&mut file, bytes).map_err(|_| "write")?;
    tree.record_file(&held, dir, name, &file)?;
    Ok(())
}

fn record_name(tree: &mut Tree, dir: &Path, name: &str) -> Result<(), &'static str> {
    let held = osutil::verify_private_leaf(dir)?;
    let file = osutil::open_child(&held.file, name).map_err(|_| "record")?;
    tree.record_file(&held, dir, name, &file)?;
    Ok(())
}

fn read_child(dir: &Path, name: &str, cap: usize) -> Result<Vec<u8>, &'static str> {
    let held = osutil::verify_private_leaf(dir)?;
    let mut file = osutil::open_child(&held.file, name).map_err(|_| "read")?;
    osutil::read_fd_capped(&mut file, cap).map_err(|_| "read")
}

fn regular_file(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    let Ok((dir, _)) = osutil::walk_dir(parent) else {
        return false;
    };
    let Ok(file) = osutil::open_child(&dir, name) else {
        return false;
    };
    osutil::id_of(&file)
        .map(|id| id.is_reg && id.nlink == 1)
        .unwrap_or(false)
}

fn child_exists(parent: &File, name: &str) -> Result<bool, &'static str> {
    match osutil::open_child(parent, name) {
        Ok(_) => Ok(true),
        Err(err) if osutil::is_enoent(&err) => Ok(false),
        Err(err) if osutil::is_eloop(&err) => Ok(true),
        Err(_) => Err("out-exists"),
    }
}

fn path_ok(path: &Path) -> bool {
    let Some(text) = path.to_str() else {
        return false;
    };
    path.is_absolute()
        && !text.contains("..")
        && !text
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '\'' | '"' | '\\'))
}

fn ids(req: &RunRequest) -> Result<(String, String), &'static str> {
    match (&req.attempt, &req.share) {
        (Some(a), Some(s)) if is_hex32(a) && is_hex32(s) => Ok((a.clone(), s.clone())),
        (None, None) => Ok((rand_hex()?, rand_hex()?)),
        _ => Err("attempt-id"),
    }
}

fn rand_hex() -> Result<String, &'static str> {
    let mut file = File::open("/dev/urandom").map_err(|_| "random")?;
    let mut buf = [0u8; 16];
    file.read_exact(&mut buf).map_err(|_| "random")?;
    Ok(sha256::hex_encode(&buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch(tag: &str) -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let home = std::env::var("HOME").unwrap();
        let p = PathBuf::from(format!("{home}/.s11m2a-h-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir(&p).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
        p
    }

    fn req(case: Case, parent: &Path, dry: bool) -> RunRequest {
        RunRequest {
            case,
            tart: PathBuf::from("/home/claude/bin/tart"),
            home: parent.join("home"),
            out: parent.join("out"),
            tart_home: None,
            dry_run: dry,
            pinned: None,
            probe: b"probe-bytes".to_vec(),
            attempt: Some("0123456789abcdef0123456789abcdef".into()),
            share: Some("abcdef0123456789abcdef0123456789".into()),
            deadline: Deadline::fresh(),
        }
    }

    #[test]
    fn dry_run_private_root_and_expired_deadline() {
        let parent = scratch("dry");
        let mut fake = runner::Fake::new(vec![]);
        let mut request = req(Case::C1, &parent, true);
        let out = run_case(&request, &mut fake).unwrap();
        assert_eq!(out.verdict, "dry_run");
        assert!(out.report.contains(crate::budget::EXPORT_LABEL));
        assert!(out.report.contains("not_run"));
        assert!(!out.report.contains("p99"));
        assert!(fake.calls.is_empty());
        assert!(parent.join("out/report.json").is_file());

        let mut request2 = req(Case::C1, &parent, false);
        request2.out = PathBuf::from(format!("/tmp/s11-not-private-{}", std::process::id()));
        let mut fake = runner::Fake::new(vec![]);
        assert_eq!(run_case(&request2, &mut fake).unwrap_err(), "private-root");
        assert!(fake.calls.is_empty());

        request.dry_run = false;
        request.deadline = Deadline::expired();
        request.out = parent.join("out2");
        let mut fake = runner::Fake::new(vec![]);
        let out = run_case(&request, &mut fake).unwrap();
        assert_eq!(out.verdict, "unknown");
        assert!(out.cleanup_absent);
        assert!(out.report.contains("deadline"));
        assert!(fake.calls.is_empty());
        let _ = std::fs::remove_file(parent.join("out/report.json"));
        let _ = std::fs::remove_dir(parent.join("out"));
        let _ = std::fs::remove_dir(&parent);
    }

    const NAME: &str = "s11-0c-0123456789abcdef0123456789abcdef";

    fn golden() -> String {
        r#"[{"Name":"macos-s2-eval","State":"stopped"}]"#.into()
    }

    fn listed(state: &str) -> String {
        format!(
            r#"[{{"Name":"macos-s2-eval","State":"stopped"}},{{"Name":"{NAME}","State":"{state}"}}]"#
        )
    }

    fn pin(parent: &Path) -> PathBuf {
        let path = parent.join("golden.pub");
        std::fs::write(&path, format!("ssh-ed25519 {}\n", "A".repeat(64))).unwrap();
        path
    }

    fn eval_key(parent: &Path) {
        let dir = parent.join("home/.ssh");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(parent.join("home"), std::fs::Permissions::from_mode(0o700))
            .unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(dir.join("id_ed25519_tart"), b"eval-key\n").unwrap();
    }

    fn no_stop_or_delete(fake: &runner::Fake) {
        assert!(fake
            .calls
            .iter()
            .all(|c| !c.iter().any(|a| a == "stop" || a == "delete")));
    }

    fn scrub(parent: &Path) {
        let out = parent.join("out");
        let attempt = out.join(NAME);
        let mb = attempt.join("mailbox");
        for name in ["host-write", "guest-write"] {
            let _ = std::fs::remove_dir(mb.join(name));
        }
        let _ = std::fs::remove_dir(&mb);
        let _ = std::fs::remove_dir(attempt.join("host-stage"));
        let ssh = attempt.join("ssh");
        for name in ["known_hosts", "id_ed25519", "id_ed25519.pub"] {
            let _ = std::fs::remove_file(ssh.join(name));
        }
        let _ = std::fs::remove_dir(&ssh);
        let _ = std::fs::remove_dir(&attempt);
        let _ = std::fs::remove_file(out.join("report.json"));
        let _ = std::fs::remove_dir(&out);
        let _ = std::fs::remove_file(parent.join("golden.pub"));
        let _ = std::fs::remove_file(parent.join("home/.ssh/id_ed25519_tart"));
        let _ = std::fs::remove_dir(parent.join("home/.ssh"));
        let _ = std::fs::remove_dir(parent.join("home"));
        let _ = std::fs::remove_dir(parent);
    }

    #[test]
    fn clone_timeout_and_unconfirmed_clone_do_not_delete() {
        let parent = scratch("clone");
        let mut request = req(Case::C1, &parent, false);
        request.pinned = Some(pin(&parent));
        request.deadline = Deadline::within(90_000);
        let mut fake = runner::Fake::new(vec![
            ("list --format", runner::CmdOut::ok_text(&golden())),
            (" clone ", runner::CmdOut::timed_out()),
        ]);
        let out = run_case(&request, &mut fake).unwrap();
        assert_eq!(out.verdict, "unknown");
        assert!(out.report.contains("clone-timeout"));
        assert!(out.cleanup_absent);
        no_stop_or_delete(&fake);

        request.out = parent.join("out2");
        let mut fake = runner::Fake::new(vec![
            ("list --format", runner::CmdOut::ok_text(&golden())),
            (" clone ", runner::CmdOut::ok_text("")),
        ]);
        let out = run_case(&request, &mut fake).unwrap();
        assert_eq!(out.verdict, "unknown");
        assert!(out.report.contains("clone-unconfirmed"));
        no_stop_or_delete(&fake);
        let _ = std::fs::remove_file(parent.join("out2/report.json"));
        let attempt = parent.join("out2").join(NAME);
        let _ = std::fs::remove_dir(attempt.join("host-stage"));
        let _ = std::fs::remove_dir(attempt.join("mailbox/host-write"));
        let _ = std::fs::remove_dir(attempt.join("mailbox/guest-write"));
        let _ = std::fs::remove_dir(attempt.join("mailbox"));
        let _ = std::fs::remove_dir(attempt.join("ssh"));
        let _ = std::fs::remove_dir(&attempt);
        let _ = std::fs::remove_dir(parent.join("out2"));
        scrub(&parent);
    }

    #[test]
    fn init_failure_cleans_the_vm_and_untrusted_delete_quarantines() {
        let parent = scratch("init");
        eval_key(&parent);
        let mut request = req(Case::C1, &parent, false);
        request.pinned = Some(pin(&parent));
        request.deadline = Deadline::within(90_000);
        let lists = vec![
            runner::CmdOut::ok_text(&golden()),
            runner::CmdOut::ok_text(&listed("stopped")),
            runner::CmdOut::ok_text(&listed("running")),
            runner::CmdOut::ok_text(&listed("running")),
            runner::CmdOut::ok_text(&listed("running")),
            runner::CmdOut::ok_text(&listed("stopped")),
            runner::CmdOut::ok_text(&golden()),
        ];
        let mut fake = runner::Fake::sequence(vec![
            ("list --format", lists),
            (" clone ", vec![runner::CmdOut::ok_text("")]),
            (" run ", vec![runner::CmdOut::ok_text("")]),
            (" ip ", vec![runner::CmdOut::ok_text("192.168.64.2\n")]),
            ("ssh-keygen", vec![runner::CmdOut::ok_text("")]),
            (
                "/usr/bin/ssh ",
                vec![runner::CmdOut::status_text(1, "fail-exists\n")],
            ),
            (" stop ", vec![runner::CmdOut::ok_text("")]),
            (" delete ", vec![runner::CmdOut::ok_text("")]),
        ]);
        let out = run_case(&request, &mut fake).unwrap();
        assert_eq!(out.verdict, "unknown", "{}", out.report);
        assert!(out.cleanup_absent);
        assert!(out.report.contains("init-failed"));
        assert!(fake.calls.iter().any(|c| c.iter().any(|a| a == "stop")));
        assert!(fake.calls.iter().any(|c| c.iter().any(|a| a == "delete")));
        assert!(fake
            .calls
            .iter()
            .all(|c| !c.iter().any(|a| a.starts_with("upload "))));
        let ssh: Vec<_> = fake
            .calls
            .iter()
            .filter(|c| c.first().map(String::as_str) == Some("/usr/bin/ssh"))
            .collect();
        assert_eq!(ssh.len(), 1);
        assert!(ssh[0].iter().any(|a| a.ends_with("id_ed25519_tart")));
        scrub(&parent);

        let parent = scratch("quar");
        eval_key(&parent);
        let mut request = req(Case::C1, &parent, false);
        request.pinned = Some(pin(&parent));
        request.deadline = Deadline::within(90_000);
        let lists = vec![
            runner::CmdOut::ok_text(&golden()),
            runner::CmdOut::ok_text(&listed("stopped")),
            runner::CmdOut::ok_text(&listed("running")),
            runner::CmdOut::ok_text(&listed("running")),
            runner::CmdOut::ok_text(&listed("running")),
            runner::CmdOut::ok_text(&listed("stopped")),
            runner::CmdOut::status_text(1, "[]"),
        ];
        let mut fake = runner::Fake::sequence(vec![
            ("list --format", lists),
            (" clone ", vec![runner::CmdOut::ok_text("")]),
            (" run ", vec![runner::CmdOut::ok_text("")]),
            (" ip ", vec![runner::CmdOut::ok_text("192.168.64.2\n")]),
            ("ssh-keygen", vec![runner::CmdOut::ok_text("")]),
            (
                "/usr/bin/ssh ",
                vec![runner::CmdOut::status_text(1, "fail-mkdir\n")],
            ),
            (" stop ", vec![runner::CmdOut::ok_text("")]),
            (" delete ", vec![runner::CmdOut::ok_text("")]),
        ]);
        let out = run_case(&request, &mut fake).unwrap();
        assert_eq!(out.verdict, "quarantine", "{}", out.report);
        assert!(!out.cleanup_absent);
        assert!(fake.calls.iter().any(|c| c.iter().any(|a| a == "delete")));
        assert!(fake
            .calls
            .iter()
            .all(|c| !c.iter().any(|a| a.starts_with("upload "))));
        scrub(&parent);
    }
}

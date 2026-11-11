//! Host driver. Every external command goes through `Runner`. The real macOS
//! process runner is not used by tests.

// `Judgement` is the attempt verdict (samples, notes, errors). Returning it
// directly keeps the sequencer's stop path obvious.
#![allow(clippy::result_large_err)]

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::argv;
use crate::budget::{
    self, MS_ATTACH, MS_CLONE, MS_CREATE, MS_DELETE, MS_DETACH, MS_FORCE, MS_LOAD, MS_QUERY,
    MS_SSH, MS_STOP, MS_TEARDOWN, MS_UNLINK, VISIBILITY_MS,
};
use crate::cases::{self, Case, Judgement, ProbeItem};
use crate::clone::{self, Absent, CloneOutcome, DeletePlan, Precheck, ToolEnd};
use crate::identity::{self, AttachId, Destroy, FileId};
use crate::inventory::{self, Vm};
use crate::ledger::Ledger;
use crate::plist::{self, ImageAttach, Plist};
use crate::report::{self, SubRow};
use crate::runner::{self, Cmd, CmdOut, Runner};
use crate::slot;
use crate::sshdriver;
use crate::volume::{self, FileStat};

pub fn real_path_supported() -> bool {
    cfg!(target_os = "macos")
}

pub struct RunRequest<'a> {
    pub case: Case,
    pub tart: &'a Path,
    pub home: &'a Path,
    pub out: &'a Path,
    pub tart_home: Option<&'a Path>,
    pub dry_run: bool,
    pub exchange: bool,
    pub ids: &'a [(&'a str, &'a str)],
    pub probe: Option<&'a [u8]>,
}

pub struct RunOutcome {
    pub status: &'static str,
    pub report: Vec<u8>,
}

pub fn run_case(
    req: &RunRequest<'_>,
    runner: &mut dyn Runner,
    on_guest: &mut dyn FnMut(&Path),
) -> Result<RunOutcome, &'static str> {
    path_ok(req.out)?;
    path_ok(req.tart)?;
    path_ok(req.home)?;
    if let Some(home) = req.tart_home {
        path_ok(home)?;
    }
    if req.dry_run {
        let rows = dry_rows(req.case);
        return finish_report(req, &rows, true);
    }
    volume::verify_private_leaf(req.out).map_err(|_| "private-root")?;
    for rel in ["private", "ssh", "ledger"] {
        let path = req.out.join(rel);
        if !path.exists() {
            crate::osutil::mkdir_0700(&path).map_err(|_| "mkdir")?;
        }
        volume::verify_private_leaf(&path).map_err(|_| "private-root")?;
    }
    let ledger_path = req.out.join("ledger").join("events.jsonl");
    let mut ledger = Ledger::create(&ledger_path)?;
    let mut rows = Vec::new();
    for (index, sub) in req.case.subcases().iter().copied().enumerate() {
        let (attempt_id, share) = match req.ids.get(index) {
            Some((attempt_id, share)) => ((*attempt_id).to_string(), (*share).to_string()),
            None => (random_hex32()?, random_hex32()?),
        };
        if !crate::sha256::is_hex32(&attempt_id) || !crate::sha256::is_hex32(&share) {
            return Err("attempt-id");
        }
        let row = attempt(req, runner, &mut ledger, on_guest, sub, &attempt_id, &share);
        let stop = row.judgement.stop_next || !row.cleanup_absent;
        rows.push(row);
        if stop {
            break;
        }
    }
    finish_report(req, &rows, false)
}

fn finish_report(
    req: &RunRequest<'_>,
    rows: &[SubRow],
    dry: bool,
) -> Result<RunOutcome, &'static str> {
    let status = report::aggregate_status(rows, dry);
    let bytes = report::render(req.case.as_str(), status, rows)?;
    report::store(&req.out.join("report.json"), &bytes)?;
    Ok(RunOutcome {
        status,
        report: bytes,
    })
}

pub fn dry_rows(case: Case) -> Vec<SubRow> {
    let attempt = "0".repeat(32);
    case.subcases()
        .iter()
        .copied()
        .map(|id| SubRow {
            id,
            attempt: attempt.clone(),
            judgement: judgement("dry_run", "not_run", true),
            cleanup_absent: false,
        })
        .collect()
}

pub fn random_hex32() -> Result<String, &'static str> {
    let mut file = std::fs::File::open("/dev/urandom").map_err(|_| "urandom")?;
    let mut buf = [0u8; 16];
    file.read_exact(&mut buf).map_err(|_| "urandom")?;
    Ok(crate::sha256::hex_encode(&buf))
}

pub fn guest_entry(case: Case) -> i32 {
    if !real_path_supported() {
        return 2;
    }
    guest_main(case)
}

#[cfg(not(target_os = "macos"))]
fn guest_main(_case: Case) -> i32 {
    2
}

#[cfg(target_os = "macos")]
fn guest_main(case: Case) -> i32 {
    let dir = Path::new(budget::GUEST_DIR);
    let share = match read_token(dir, "share-name") {
        Ok(value) if crate::sha256::is_hex32(&value) => value,
        _ => return 1,
    };
    let sub = match read_token(dir, "subcase") {
        Ok(value) if sshdriver::subcase_ok(&value) => value,
        _ => return 1,
    };
    let export = Path::new(budget::SHARE_VOLUME).join(&share);
    match volume::lstat(&export) {
        Ok(st) if st.is_dir && !st.is_link => {}
        _ => return 1,
    }
    match case {
        Case::C1 => match cases::guest_c1(&export, Duration::from_millis(VISIBILITY_MS)) {
            Ok(()) => 0,
            Err(_) => 1,
        },
        Case::C2 => {
            let _ = cases::guest_c2(&export, &sub);
            86
        }
        Case::C4 => 0,
        Case::C8 => guest_c8(dir, &export),
    }
}

#[cfg(target_os = "macos")]
fn guest_c8(dir: &Path, export: &Path) -> i32 {
    let mut runner = crate::proc::ProcessRunner::new();
    let users = runner.run(&Cmd::new(argv::dscl_users(), MS_QUERY));
    let group = runner.run(&Cmd::new(argv::dscl_admin_group(), MS_QUERY));
    if users.status != 0
        || group.status != 0
        || users.timed_out
        || group.timed_out
        || users.overflow
        || group.overflow
    {
        return 1;
    }
    let unique = match String::from_utf8(users.stdout) {
        Ok(text) => text,
        Err(_) => return 1,
    };
    let admin = match String::from_utf8(group.stdout) {
        Ok(text) => text,
        Err(_) => return 1,
    };
    let work = dir.join("work");
    let obs = match cases::observe_c8_text(&unique, &admin, Some(&work)) {
        Ok(obs) => obs,
        Err(_) => return 1,
    };
    let bytes = match cases::c8_slot_json(&obs) {
        Ok(bytes) => bytes,
        Err(_) => return 1,
    };
    match slot::write_result_slot(&export.join("guest-write"), &bytes) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

fn path_ok(path: &Path) -> Result<(), &'static str> {
    let text = path.to_str().ok_or("path")?;
    if !path.is_absolute()
        || text.contains(' ')
        || text.contains('\t')
        || text.contains('\n')
        || text.contains('\'')
        || text.contains('"')
    {
        return Err("path");
    }
    Ok(())
}

fn judgement(verdict: &'static str, reason: &'static str, stop: bool) -> Judgement {
    Judgement {
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

fn failed(error: &'static str) -> Judgement {
    let mut row = judgement("failed", "", true);
    row.errors.push(error);
    row
}

struct Node {
    path: PathBuf,
    id: FileId,
    dir: bool,
    volume: bool,
}

struct Work {
    attempt: String,
    share: String,
    sub: &'static str,
    case: Case,
    tart: PathBuf,
    tart_home: Option<String>,
    home: PathBuf,
    root: PathBuf,
    seed: PathBuf,
    image: PathBuf,
    mount: PathBuf,
    ssh_dir: PathBuf,
    key_path: PathBuf,
    known_path: PathBuf,
    nodes: Vec<Node>,
    image_id: Option<FileId>,
    image_created: bool,
    attach: Option<AttachId>,
    device: String,
    clone_name: String,
    clone_attempted: bool,
    owned: bool,
    vm_started: bool,
    run_id: Option<u64>,
    ssh_id: Option<u64>,
    down_reason: &'static str,
    probe: Vec<u8>,
    exchange: bool,
}

fn attempt(
    req: &RunRequest<'_>,
    runner: &mut dyn Runner,
    ledger: &mut Ledger,
    on_guest: &mut dyn FnMut(&Path),
    sub: &'static str,
    attempt_id: &str,
    share: &str,
) -> SubRow {
    let clone_name = clone::clone_name(attempt_id).unwrap_or_else(|_| "s11-0c-invalid".into());
    let root = req.out.join("private").join(attempt_id);
    let mut work = Work {
        attempt: attempt_id.to_string(),
        share: share.to_string(),
        sub,
        case: req.case,
        tart: req.tart.to_path_buf(),
        tart_home: req.tart_home.map(|p| p.display().to_string()),
        home: req.home.to_path_buf(),
        seed: root.join("seed"),
        image: root.join("mailbox.sparseimage"),
        mount: root.join("mnt"),
        ssh_dir: req.out.join("ssh").join(attempt_id),
        key_path: req.out.join("ssh").join(attempt_id).join("id_ed25519"),
        known_path: req.out.join("ssh").join(attempt_id).join("known_hosts"),
        root,
        nodes: Vec::new(),
        image_id: None,
        image_created: false,
        attach: None,
        device: String::new(),
        clone_name,
        clone_attempted: false,
        owned: false,
        vm_started: false,
        run_id: None,
        ssh_id: None,
        down_reason: "cleanup-incomplete",
        probe: req.probe.unwrap_or(&[]).to_vec(),
        exchange: req.exchange,
    };
    let mut judged = match work.up(runner, ledger, on_guest) {
        Ok(row) => row,
        Err(row) => row,
    };
    let absent = work.down(runner, ledger);
    if !absent && !matches!(judged.verdict, "failed" | "ambiguous") {
        judged.verdict = "quarantine";
        judged.stop_next = true;
        judged.unknown_reason = work.down_reason;
    }
    if !absent {
        judged.stop_next = true;
    }
    SubRow {
        id: sub,
        attempt: attempt_id.to_string(),
        judgement: judged,
        cleanup_absent: absent,
    }
}

impl Work {
    fn tcmd(&self, argv: Vec<String>, timeout_ms: u64) -> Cmd {
        let mut cmd = Cmd::new(argv, timeout_ms);
        if let Some(home) = &self.tart_home {
            cmd.env.push(("TART_HOME".into(), home.clone()));
        }
        cmd
    }

    fn run_raw(
        &self,
        runner: &mut dyn Runner,
        ledger: &mut Ledger,
        step: &'static str,
        resource: &'static str,
        cmd: Cmd,
    ) -> Result<CmdOut, Judgement> {
        if let Err(code) = runner::check_cmd(&cmd) {
            return Err(judgement("quarantine", code, true));
        }
        ledger
            .intent(&self.attempt, step, resource)
            .map_err(|_| judgement("quarantine", "ledger-io", true))?;
        let out = runner.run(&cmd);
        if let Some(reason) = untrusted(&out) {
            ledger
                .quarantine(&self.attempt, step, reason)
                .map_err(|_| judgement("quarantine", "ledger-io", true))?;
        } else if out.status < 0 {
            ledger
                .quarantine(&self.attempt, step, "status")
                .map_err(|_| judgement("quarantine", "ledger-io", true))?;
        } else {
            ledger
                .effect(&self.attempt, step, resource, out.status)
                .map_err(|_| judgement("quarantine", "ledger-io", true))?;
        }
        Ok(out)
    }

    fn record_dir(
        &mut self,
        ledger: &mut Ledger,
        step: &'static str,
        resource: &'static str,
        path: &Path,
        volume: bool,
    ) -> Result<(), Judgement> {
        ledger
            .intent(&self.attempt, step, resource)
            .map_err(|_| judgement("quarantine", "ledger-io", true))?;
        if crate::osutil::mkdir_0700(path).is_err() {
            return Err(judgement("quarantine", "mkdir", true));
        }
        ledger
            .effect(&self.attempt, step, resource, 0)
            .map_err(|_| judgement("quarantine", "ledger-io", true))?;
        self.record_identity(ledger, step, resource, path, true, volume)
    }

    fn note_file(
        &mut self,
        ledger: &mut Ledger,
        step: &'static str,
        resource: &'static str,
        path: &Path,
    ) -> Result<(), Judgement> {
        self.record_identity(ledger, step, resource, path, false, false)
    }

    fn record_identity(
        &mut self,
        ledger: &mut Ledger,
        step: &'static str,
        resource: &'static str,
        path: &Path,
        dir: bool,
        volume: bool,
    ) -> Result<(), Judgement> {
        let st = volume::lstat(path).map_err(|_| judgement("quarantine", "stat", true))?;
        let kind_ok = if dir {
            st.is_dir && !st.is_link
        } else {
            st.is_file && !st.is_link
        };
        if !kind_ok {
            let _ = ledger.quarantine(&self.attempt, step, "mode");
            return Err(judgement("quarantine", "mode", true));
        }
        ledger
            .identity(&self.attempt, step, resource, st.dev, st.ino)
            .map_err(|_| judgement("quarantine", "ledger-io", true))?;
        self.nodes.push(Node {
            path: path.to_path_buf(),
            id: volume::file_id(&st),
            dir,
            volume,
        });
        Ok(())
    }

    fn up(
        &mut self,
        runner: &mut dyn Runner,
        ledger: &mut Ledger,
        on_guest: &mut dyn FnMut(&Path),
    ) -> Result<Judgement, Judgement> {
        self.record_dir(ledger, "mkdir-a", "a", &self.root.clone(), true)?;
        volume::verify_private_leaf(&self.root)
            .map_err(|_| judgement("block", "private-root", true))?;
        let root = self.root.clone();
        let acl = self.run_raw(
            runner,
            ledger,
            "ls-acl",
            "a",
            Cmd::new(argv::ls_acl(&root), MS_QUERY),
        )?;
        self.must_trust(&acl)?;
        let text = acl
            .trusted_text()
            .map_err(|_| judgement("block", "acl-unknown", true))?;
        volume::acl_clean(&text).map_err(|code| judgement("block", code, true))?;
        self.exclude(runner, ledger, &self.root.clone(), "a")?;
        self.record_dir(ledger, "mkdir-seed", "seed", &self.seed.clone(), true)?;
        self.record_dir(ledger, "mkdir-m", "m", &self.mount.clone(), true)?;
        self.record_dir(ledger, "mkdir-ssh", "ssh", &self.ssh_dir.clone(), false)?;
        ledger
            .intent(&self.attempt, "seed-tree", "seed")
            .map_err(|_| judgement("quarantine", "ledger-io", true))?;
        cases::prepare_volume(&self.seed).map_err(|_| judgement("quarantine", "seed", true))?;
        ledger
            .effect(&self.attempt, "seed-tree", "seed", 0)
            .map_err(|_| judgement("quarantine", "ledger-io", true))?;
        for (rel, resource, dir) in [
            ("host-stage", "host-stage", true),
            ("mailbox", "mailbox", true),
            ("mailbox/host-write", "host-write", true),
            ("mailbox/guest-write", "guest-write", true),
            (".fseventsd", "fseventsd", true),
            (".metadata_never_index", "marker", false),
            (".fseventsd/no_log", "nolog", false),
        ] {
            self.record_identity(ledger, "seed-id", resource, &self.seed.join(rel), dir, true)?;
        }
        let seed = self.seed.clone();
        let image = self.image.clone();
        let created = self.run_raw(
            runner,
            ledger,
            "hdiutil-create",
            "i",
            Cmd::new(argv::hdiutil_create(&seed, &image), MS_CREATE),
        )?;
        self.must_trust(&created)?;
        if created.status != 0 {
            return Err(failed("create-failed"));
        }
        let st = volume::lstat(&self.image).map_err(|_| judgement("quarantine", "stat", true))?;
        if !st.is_file || !volume::scratch_within(&st) {
            return Err(failed("oversize"));
        }
        ledger
            .identity(&self.attempt, "hdiutil-create", "i", st.dev, st.ino)
            .map_err(|_| judgement("quarantine", "ledger-io", true))?;
        self.image_id = Some(volume::file_id(&st));
        self.image_created = true;
        let image = self.image.clone();
        let info = self.run_raw(
            runner,
            ledger,
            "hdiutil-imageinfo",
            "i",
            Cmd::new(argv::hdiutil_imageinfo(&image), MS_QUERY),
        )?;
        self.must_trust(&info)?;
        let image_doc =
            plist_doc(&info).map_err(|_| judgement("block", "imageinfo-unknown", true))?;
        let listed = self.run_raw(
            runner,
            ledger,
            "hdiutil-info",
            "i",
            Cmd::new(argv::hdiutil_info(), MS_QUERY),
        )?;
        self.must_trust(&listed)?;
        let images = image_list(&listed)
            .map_err(|_| judgement("quarantine", "readback-incomplete", true))?;
        if !images.is_empty() {
            let _ = ledger.quarantine(&self.attempt, "hdiutil-info", "possibly-attached");
            return Err(judgement("quarantine", "possibly-attached", true));
        }
        self.exclude(runner, ledger, &self.image.clone(), "i")?;
        self.exclude(runner, ledger, &self.mount.clone(), "m")?;
        let boot = matches!(self.case, Case::C1 | Case::C2 | Case::C8);
        let do_attach = boot || self.sub == "attach";
        if !do_attach {
            let scratch_ok = volume::lstat(&self.image)
                .map(|st| volume::scratch_within(&st))
                .unwrap_or(false);
            let excluded = true;
            return Ok(self.judge_create(&image_doc, scratch_ok, excluded));
        }
        let mount = self.mount.clone();
        let image = self.image.clone();
        let attached = self.run_raw(
            runner,
            ledger,
            "hdiutil-attach",
            "v",
            Cmd::new(argv::hdiutil_attach(&mount, &image), MS_ATTACH),
        )?;
        self.must_trust(&attached)?;
        if attached.status != 0 {
            return Err(failed("attach-failed"));
        }
        let attach_text = attached
            .trusted_text()
            .map_err(|_| judgement("quarantine", "readback-incomplete", true))?;
        let (whole, slice, mount_point) = bind_attach(
            &attach_text,
            &self.image.display().to_string(),
            &self.mount.display().to_string(),
        )
        .map_err(|code| judgement("quarantine", code, true))?;
        self.device = slice.clone().unwrap_or_else(|| whole.clone());
        let device = self.device.clone();
        let disk = self.run_raw(
            runner,
            ledger,
            "diskutil-info",
            "v",
            Cmd::new(argv::diskutil_info(&device), MS_QUERY),
        )?;
        self.must_trust(&disk)?;
        let disk_doc =
            plist_doc(&disk).map_err(|_| judgement("quarantine", "readback-incomplete", true))?;
        let mounted = self.run_raw(
            runner,
            ledger,
            "mount",
            "v",
            Cmd::new(argv::mount_readonly(), MS_QUERY),
        )?;
        self.must_trust(&mounted)?;
        let mount_text = mounted
            .trusted_text()
            .map_err(|_| judgement("quarantine", "readback-incomplete", true))?;
        let mount_path = self.mount.display().to_string();
        let flags = identity::mount_flags(&mount_text, &mount_path);
        let md = self.run_raw(
            runner,
            ledger,
            "mdutil",
            "v",
            Cmd::new(argv::mdutil_status(&self.mount.clone()), MS_QUERY),
        )?;
        self.must_trust(&md)?;
        let md_text = md.trusted_text().unwrap_or_default();
        let excluded_mount = self.excluded_path(runner, ledger, &self.mount.clone(), "m")?;
        let fsid = std::fs::File::open(&self.mount)
            .and_then(|file| {
                crate::osutil::fsid_of(&file).map_err(|_| std::io::Error::other("fsid"))
            })
            .ok();
        let uuid = disk_doc
            .get("VolumeUUID")
            .and_then(Plist::string)
            .unwrap_or("")
            .to_string();
        let image_st =
            volume::lstat(&self.image).map_err(|_| judgement("quarantine", "stat", true))?;
        let mount_st =
            volume::lstat(&self.mount).map_err(|_| judgement("quarantine", "stat", true))?;
        let recorded = AttachId {
            image_path: self.image.display().to_string(),
            whole: whole.clone(),
            slice: slice.clone(),
            mount: mount_point,
            uuid: uuid.clone(),
            fsid: fsid.unwrap_or([0; 8]),
            image: volume::file_id(&image_st),
            mount_underlying: volume::file_id(&mount_st),
        };
        self.attach = Some(recorded);
        if self.sub == "attach" && !boot {
            return Ok(judge_attach_case(
                attached.status,
                &disk_doc,
                flags.as_ref(),
                fsid,
                &self.mount,
            ));
        }
        boot_gate(
            &image_doc,
            &disk_doc,
            flags.as_ref(),
            fsid,
            &md_text,
            excluded_mount,
            &image_st,
        )?;
        self.boot(runner, ledger, on_guest)
    }

    fn exclude(
        &mut self,
        runner: &mut dyn Runner,
        ledger: &mut Ledger,
        path: &Path,
        resource: &'static str,
    ) -> Result<(), Judgement> {
        let add = self.run_raw(
            runner,
            ledger,
            "tmutil-add",
            resource,
            Cmd::new(argv::tmutil_addexclusion(path), MS_QUERY),
        )?;
        self.must_trust(&add)?;
        self.elevation(&add)?;
        if add.status != 0 {
            return Err(judgement("block", "exclusion-failed", true));
        }
        if !self.excluded_path(runner, ledger, path, resource)? {
            return Err(judgement("block", "exclusion-unread", true));
        }
        Ok(())
    }

    fn excluded_path(
        &mut self,
        runner: &mut dyn Runner,
        ledger: &mut Ledger,
        path: &Path,
        resource: &'static str,
    ) -> Result<bool, Judgement> {
        let out = self.run_raw(
            runner,
            ledger,
            "tmutil-is",
            resource,
            Cmd::new(argv::tmutil_isexcluded(path), MS_QUERY),
        )?;
        self.must_trust(&out)?;
        self.elevation(&out)?;
        if out.status != 0 {
            return Err(judgement("block", "exclusion-failed", true));
        }
        let text = out
            .trusted_text()
            .map_err(|_| judgement("block", "exclusion-unread", true))?;
        Ok(identity::excluded_exact(&text, &path.display().to_string()))
    }

    fn elevation(&self, out: &CmdOut) -> Result<(), Judgement> {
        let err = String::from_utf8_lossy(&out.stderr);
        if runner::needs_elevation(&err) {
            Err(judgement("quarantine", "tmutil-needs-elevation", true))
        } else {
            Ok(())
        }
    }

    fn must_trust(&self, out: &CmdOut) -> Result<(), Judgement> {
        if let Some(reason) = untrusted(out) {
            Err(judgement("quarantine", reason, true))
        } else {
            Ok(())
        }
    }

    fn judge_create(&self, doc: &Plist, scratch_ok: bool, excluded: bool) -> Judgement {
        let items = vec![
            item("create-exit", "pass"),
            item(
                "sparse-suffix",
                pass_if(self.image.extension().and_then(|s| s.to_str()) == Some("sparseimage")),
            ),
            item(
                "fs-hfsx",
                key_state(doc, "FilesystemName", |v| {
                    v.to_ascii_lowercase().contains("hfs")
                }),
            ),
            item("no-journal", desc_journal(doc)),
            item(
                "type-sparse",
                match doc.get("Format").and_then(Plist::string) {
                    Some("UDSP") => "pass",
                    Some(_) => "fail",
                    None => "unknown",
                },
            ),
            item(
                "layout-none",
                match doc.get("Layout").and_then(Plist::string) {
                    Some("NONE") | Some("none") => "pass",
                    Some(_) => "fail",
                    None => "unknown",
                },
            ),
            item(
                "volname",
                match doc.get("Volume Name").and_then(Plist::string) {
                    Some(budget::VOLNAME) => "pass",
                    Some(_) => "fail",
                    None => "unknown",
                },
            ),
            item("size-64m", size_state(doc)),
            item(
                "marker-seeded",
                if marker_files(&self.seed) {
                    "unknown"
                } else {
                    "fail"
                },
            ),
            item("scratch-bound", pass_if(scratch_ok)),
            item("no-residual-attach", "pass"),
            item("exclusion-a", pass_if(excluded)),
        ];
        let mut judged = cases::judge_c4(&items);
        judged.notes.push("marker-not-pre-mount-proof");
        judged
    }

    fn boot(
        &mut self,
        runner: &mut dyn Runner,
        ledger: &mut Ledger,
        on_guest: &mut dyn FnMut(&Path),
    ) -> Result<Judgement, Judgement> {
        if self.probe.is_empty() {
            self.probe = load_current_exe().map_err(|code| judgement("block", code, true))?;
        }
        if self.probe.is_empty() || self.probe.len() > budget::UPLOAD_CAP {
            return Err(judgement("block", "probe-size", true));
        }
        let digest = crate::sha256::hex_digest(&self.probe);
        let before = self.list_vms(runner, ledger)?;
        match clone::precheck(&Ok(before), &self.clone_name) {
            Precheck::Ok => {}
            Precheck::Stop { reason } => return Err(judgement("quarantine", reason, true)),
        }
        self.clone_attempted = true;
        let clone_cmd = self.tcmd(argv::tart_clone(&self.tart, &self.clone_name), MS_CLONE);
        let cloned = self.run_raw(runner, ledger, "tart-clone", "vm", clone_cmd)?;
        let end = tool_end(&cloned);
        let after = self.list_vms(runner, ledger);
        let after_res = match &after {
            Ok(vms) => Ok(vms.clone()),
            Err(_) => Err("list-untrusted"),
        };
        match clone::decide_after_clone(end, &after_res, &self.clone_name) {
            CloneOutcome::Owned => self.owned = true,
            CloneOutcome::NotCreated { reason } => return Err(failed(reason)),
            CloneOutcome::Ambiguous { reason } => return Err(judgement("ambiguous", reason, true)),
        }
        let key = self.key_path.clone();
        let gen = self.run_raw(
            runner,
            ledger,
            "ssh-keygen",
            "key",
            Cmd::new(argv::ssh_keygen(&key), MS_QUERY),
        )?;
        self.must_trust(&gen)?;
        if gen.status != 0 {
            return Err(judgement("quarantine", "keygen", true));
        }
        let pub_path = self.key_path.with_extension("pub");
        self.note_file(ledger, "ssh-keygen", "key", &self.key_path.clone())?;
        self.note_file(ledger, "ssh-keygen", "pub", &pub_path)?;
        let run = self.tcmd(
            argv::tart_run(
                &self.tart,
                &self.clone_name,
                &self.share,
                &self.mount.join("mailbox"),
            ),
            120_000,
        );
        runner::check_cmd(&run).map_err(|code| judgement("quarantine", code, true))?;
        ledger
            .intent(&self.attempt, "tart-run", "vm")
            .map_err(|_| judgement("quarantine", "ledger-io", true))?;
        let id = runner
            .spawn(&run)
            .map_err(|code| judgement("quarantine", code, true))?;
        ledger
            .effect(&self.attempt, "tart-run", "vm", 0)
            .map_err(|_| judgement("quarantine", "ledger-io", true))?;
        self.run_id = Some(id);
        self.vm_started = true;
        let ip = self.read_ip(runner, ledger)?;
        let scan = self.run_raw(
            runner,
            ledger,
            "ssh-keyscan",
            "key",
            Cmd::new(argv::ssh_keyscan(&ip), MS_QUERY),
        )?;
        self.must_trust(&scan)?;
        let scan_text = scan
            .trusted_text()
            .map_err(|_| judgement("quarantine", "host-key-untrusted", true))?;
        let pinned = sshdriver::parse_keyscan(&scan_text, &ip)
            .map_err(|code| judgement("quarantine", code, true))?;
        write_private(&self.known_path, format!("{pinned}\n").as_bytes())
            .map_err(|_| judgement("quarantine", "known-hosts", true))?;
        self.note_file(ledger, "known-hosts", "known", &self.known_path.clone())?;
        let pub_line = std::fs::read_to_string(self.key_path.with_extension("pub"))
            .map_err(|_| judgement("quarantine", "pubkey", true))?;
        let pubkey = sshdriver::validate_pubkey(pub_line.trim())
            .map_err(|_| judgement("quarantine", "pubkey", true))?;
        let eval = sshdriver::eval_key(&self.home)
            .map_err(|_| judgement("quarantine", "eval-key", true))?;
        let install = sshdriver::install_remote(&self.share, self.sub, &pubkey)
            .map_err(|code| judgement("quarantine", code, true))?;
        let dispatcher = sshdriver::production_dispatcher()
            .map_err(|_| judgement("quarantine", "dispatcher", true))?;
        let install_cmd =
            sshdriver::ssh_cmd(&eval, &self.known_path, &ip, &install, dispatcher, MS_SSH)
                .map_err(|code| judgement("quarantine", code, true))?;
        let installed = self.run_raw(runner, ledger, "ssh-install", "key", install_cmd)?;
        self.must_trust(&installed)?;
        if installed.status != 0 {
            return Err(judgement("quarantine", "ssh-install", true));
        }
        let upload_cmd = sshdriver::ssh_cmd(
            &self.key_path,
            &self.known_path,
            &ip,
            &format!("upload {}", self.case.as_str()),
            self.probe.clone(),
            MS_SSH,
        )
        .map_err(|code| judgement("quarantine", code, true))?;
        let uploaded = self.run_raw(runner, ledger, "ssh-upload", "key", upload_cmd)?;
        self.must_trust(&uploaded)?;
        if uploaded.status != 0 {
            return Err(judgement("quarantine", "ssh-upload", true));
        }
        let verify_cmd = sshdriver::ssh_cmd(
            &self.key_path,
            &self.known_path,
            &ip,
            &format!("verify {digest}"),
            Vec::new(),
            MS_SSH,
        )
        .map_err(|code| judgement("quarantine", code, true))?;
        let verified = self.run_raw(runner, ledger, "ssh-verify", "key", verify_cmd)?;
        self.must_trust(&verified)?;
        if verified.status != 0 {
            return Err(judgement("quarantine", "verify-failed", true));
        }
        let run_ssh = sshdriver::ssh_cmd(
            &self.key_path,
            &self.known_path,
            &ip,
            &format!("run {}", self.case.as_str()),
            Vec::new(),
            MS_SSH,
        )
        .map_err(|code| judgement("quarantine", code, true))?;
        runner::check_cmd(&run_ssh).map_err(|code| judgement("quarantine", code, true))?;
        ledger
            .intent(&self.attempt, "ssh-run", "key")
            .map_err(|_| judgement("quarantine", "ledger-io", true))?;
        let ssh_id = runner
            .spawn(&run_ssh)
            .map_err(|code| judgement("quarantine", code, true))?;
        ledger
            .effect(&self.attempt, "ssh-run", "key", 0)
            .map_err(|_| judgement("quarantine", "ledger-io", true))?;
        self.ssh_id = Some(ssh_id);
        if !self.exchange {
            return Ok(judgement("unknown", "exchange-skipped", true));
        }
        let mailbox = self.mount.join("mailbox");
        let guest_write = mailbox.join("guest-write");
        match self.case {
            Case::C1 => {
                on_guest(&mailbox);
                let (samples, digest) =
                    cases::drive_c1(&self.mount, Duration::from_millis(VISIBILITY_MS))
                        .map_err(|_| judgement("unknown", "publish", true))?;
                let guest = slot::read_result_slot(&guest_write)
                    .ok()
                    .and_then(|bytes| cases::parse_c1_slot(&bytes).ok());
                let guest_digest = guest.as_ref().map(|(_, d)| d.as_str());
                Ok(cases::judge_c1(&samples, guest_digest, &digest))
            }
            Case::C2 => {
                self.finish_ssh(runner);
                let (mixed, complete) = cases::c2_frame_status(&mailbox);
                let acked = slot::read_result_slot(&guest_write).is_ok();
                let publishes = if complete || mixed { 3 } else { 0 };
                Ok(cases::judge_c2(self.sub, publishes, complete, mixed, acked))
            }
            Case::C8 => {
                self.finish_ssh(runner);
                match slot::read_result_slot(&guest_write) {
                    Ok(bytes) => match cases::parse_c8_slot(&bytes) {
                        Ok(obs) => Ok(cases::judge_c8(&obs)),
                        Err(_) => Err(judgement("unknown", "slot-schema", true)),
                    },
                    Err(_) => Err(judgement("unknown", "slot-missing", true)),
                }
            }
            Case::C4 => Err(judgement("quarantine", "case", true)),
        }
    }

    fn read_ip(
        &mut self,
        runner: &mut dyn Runner,
        ledger: &mut Ledger,
    ) -> Result<String, Judgement> {
        for _ in 0..6 {
            let cmd = self.tcmd(argv::tart_ip(&self.tart, &self.clone_name), MS_QUERY);
            let out = self.run_raw(runner, ledger, "tart-ip", "vm", cmd)?;
            if untrusted(&out).is_some() || out.status != 0 {
                continue;
            }
            if let Ok(text) = out.trusted_text() {
                if let Ok(ip) = sshdriver::parse_ipv4(&text) {
                    return Ok(ip);
                }
            }
        }
        Err(judgement("quarantine", "ip-missing", true))
    }

    fn list_vms(
        &mut self,
        runner: &mut dyn Runner,
        ledger: &mut Ledger,
    ) -> Result<Vec<Vm>, Judgement> {
        let cmd = self.tcmd(argv::tart_list(&self.tart), MS_QUERY);
        let out = self.run_raw(runner, ledger, "tart-list", "vm", cmd)?;
        self.must_trust(&out)?;
        let text = out
            .trusted_text()
            .map_err(|_| judgement("quarantine", "list-untrusted", true))?;
        inventory::parse_tart_list(&text)
            .map_err(|_| judgement("quarantine", "list-untrusted", true))
    }

    fn finish_ssh(&mut self, runner: &mut dyn Runner) {
        if let Some(id) = self.ssh_id.take() {
            let _ = runner.finish(id, MS_LOAD);
        }
    }

    fn down(&mut self, runner: &mut dyn Runner, ledger: &mut Ledger) -> bool {
        let started = Instant::now();
        self.finish_ssh(runner);
        let vm_absent = self.settle_vm(runner, ledger);
        let mut volume_absent = true;
        if !vm_absent {
            self.down_reason = "vm-present";
            volume_absent = false;
        } else if let Some(attach) = self.attach.clone() {
            volume_absent = self.detach_and_unlink(runner, ledger, &attach);
        } else if self.image_created {
            volume_absent = self.unlink_unattached(runner, ledger);
        }
        if volume_absent {
            if let Err(reason) = self.remove_nodes(true) {
                self.down_reason = reason;
                volume_absent = false;
            }
        }
        if let Err(reason) = self.remove_nodes(false) {
            if self.down_reason == "cleanup-incomplete" {
                self.down_reason = reason;
            }
            volume_absent = false;
        }
        if started.elapsed() > Duration::from_millis(MS_TEARDOWN) {
            self.down_reason = "teardown-budget";
            return false;
        }
        vm_absent && volume_absent && self.image_gone() && !self.mount_still_listed(runner, ledger)
    }

    fn image_gone(&self) -> bool {
        volume::lstat(&self.image).is_err()
    }

    fn mount_still_listed(&mut self, runner: &mut dyn Runner, ledger: &mut Ledger) -> bool {
        let out = match self.run_raw(
            runner,
            ledger,
            "mount",
            "v",
            Cmd::new(argv::mount_readonly(), MS_QUERY),
        ) {
            Ok(out) => out,
            Err(_) => return true,
        };
        if untrusted(&out).is_some() || out.status != 0 {
            return true;
        }
        match out.trusted_text() {
            Ok(text) => identity::mount_contains(&text, &self.mount.display().to_string()),
            Err(_) => true,
        }
    }

    fn settle_vm(&mut self, runner: &mut dyn Runner, ledger: &mut Ledger) -> bool {
        if !self.clone_attempted {
            return true;
        }
        if self.vm_started {
            let cmd = self.tcmd(argv::tart_stop(&self.tart, &self.clone_name), MS_STOP);
            let _ = self.run_raw(runner, ledger, "tart-stop", "vm", cmd);
            if let Some(id) = self.run_id.take() {
                let _ = runner.finish(id, MS_STOP);
            }
        }
        let list = match self.list_vms(runner, ledger) {
            Ok(vms) => vms,
            Err(_) => return false,
        };
        if !self.owned {
            return clone::confirm_absent(&Ok(list), &self.clone_name) == Absent::Yes;
        }
        let stopped = inventory::find(&list, &self.clone_name)
            .map(|vm| vm.state == "stopped")
            .unwrap_or(true);
        if inventory::find(&list, &self.clone_name).is_none() {
            return true;
        }
        if !stopped {
            self.down_reason = "vm-present";
            return false;
        }
        if !matches!(
            clone::decide_delete(true, &self.clone_name),
            DeletePlan::Delete
        ) {
            self.down_reason = "not-owned";
            return false;
        }
        let cmd = self.tcmd(argv::tart_delete(&self.tart, &self.clone_name), MS_DELETE);
        let deleted = match self.run_raw(runner, ledger, "tart-delete", "vm", cmd) {
            Ok(out) => out,
            Err(_) => return false,
        };
        if untrusted(&deleted).is_some() {
            return false;
        }
        let after = match self.list_vms(runner, ledger) {
            Ok(vms) => vms,
            Err(_) => return false,
        };
        clone::confirm_absent(&Ok(after), &self.clone_name) == Absent::Yes
    }

    fn detach_and_unlink(
        &mut self,
        runner: &mut dyn Runner,
        ledger: &mut Ledger,
        attach: &AttachId,
    ) -> bool {
        let info = self.info_images(runner, ledger);
        let disk = match self.disk_doc(runner, ledger) {
            Ok(doc) => Ok(doc),
            Err(_) => Err("readback-incomplete"),
        };
        let fsid = std::fs::File::open(&self.mount)
            .ok()
            .and_then(|file| crate::osutil::fsid_of(&file).ok());
        match identity::decide_detach(attach, &info, &disk, fsid) {
            Destroy::Proceed => {}
            Destroy::SkipAbsent => return self.unlink_attached(runner, ledger, attach),
            Destroy::Quarantine { reason } => {
                self.down_reason = reason;
                return false;
            }
        }
        let device = attach.whole.clone();
        let detached = match self.run_raw(
            runner,
            ledger,
            "hdiutil-detach",
            "v",
            Cmd::new(argv::hdiutil_detach(&device), MS_DETACH),
        ) {
            Ok(out) => out,
            Err(_) => return false,
        };
        if untrusted(&detached).is_some() {
            self.down_reason = "detach-untrusted";
            return false;
        }
        let still = match self.info_images(runner, ledger) {
            Ok(images) => images
                .iter()
                .any(|img| img.image_path == attach.image_path || img.whole == attach.whole),
            Err(_) => true,
        };
        if detached.status != 0 || still {
            let disk = match self.disk_doc(runner, ledger) {
                Ok(doc) => Ok(doc),
                Err(_) => Err("readback-incomplete"),
            };
            let info = self.info_images(runner, ledger);
            let fsid = std::fs::File::open(&self.mount)
                .ok()
                .and_then(|file| crate::osutil::fsid_of(&file).ok());
            if matches!(
                identity::decide_detach(attach, &info, &disk, fsid),
                Destroy::Proceed
            ) {
                let forced = match self.run_raw(
                    runner,
                    ledger,
                    "hdiutil-detach-force",
                    "v",
                    Cmd::new(argv::hdiutil_detach_force(&device), MS_FORCE),
                ) {
                    Ok(out) => out,
                    Err(_) => return false,
                };
                if untrusted(&forced).is_some() {
                    self.down_reason = "detach-untrusted";
                    return false;
                }
            } else {
                self.down_reason = "identity-mismatch";
                return false;
            }
        }
        self.unlink_attached(runner, ledger, attach)
    }

    fn unlink_attached(
        &mut self,
        runner: &mut dyn Runner,
        ledger: &mut Ledger,
        attach: &AttachId,
    ) -> bool {
        let info = self.info_images(runner, ledger);
        let mount_text = match self.mount_text(runner, ledger) {
            Ok(text) => Ok(text),
            Err(_) => Err("readback-incomplete"),
        };
        let now = volume::lstat(&self.image)
            .ok()
            .map(|st| volume::file_id(&st));
        let missing = now.is_none() && volume::lstat(&self.image).is_err();
        let view = identity::UnlinkView {
            info: &info,
            mount_text: &mount_text,
            image_now: now,
            image_missing: missing,
        };
        match identity::decide_unlink(attach, &view) {
            Destroy::Proceed => self.unlink_image(ledger),
            Destroy::SkipAbsent => true,
            Destroy::Quarantine { reason } => {
                self.down_reason = reason;
                false
            }
        }
    }

    fn unlink_unattached(&mut self, runner: &mut dyn Runner, ledger: &mut Ledger) -> bool {
        let Some(recorded) = self.image_id else {
            self.down_reason = "identity-incomplete";
            return false;
        };
        let info = self.info_images(runner, ledger);
        let now = volume::lstat(&self.image)
            .ok()
            .map(|st| volume::file_id(&st));
        let missing = now.is_none();
        match identity::decide_unlink_unattached(
            &recorded,
            &info,
            &self.image.display().to_string(),
            now,
            missing,
        ) {
            Destroy::Proceed => self.unlink_image(ledger),
            Destroy::SkipAbsent => true,
            Destroy::Quarantine { reason } => {
                self.down_reason = reason;
                false
            }
        }
    }

    fn unlink_image(&mut self, ledger: &mut Ledger) -> bool {
        if ledger.intent(&self.attempt, "unlink", "i").is_err() {
            self.down_reason = "ledger-io";
            return false;
        }
        let unlink_at = Instant::now();
        if std::fs::remove_file(&self.image).is_err() {
            self.down_reason = "unlink";
            return false;
        }
        if unlink_at.elapsed() > Duration::from_millis(MS_UNLINK) {
            self.down_reason = "unlink-budget";
            return false;
        }
        if ledger.effect(&self.attempt, "unlink", "i", 0).is_err() {
            self.down_reason = "ledger-io";
            return false;
        }
        true
    }

    fn info_images(
        &mut self,
        runner: &mut dyn Runner,
        ledger: &mut Ledger,
    ) -> Result<Vec<ImageAttach>, &'static str> {
        let out = self
            .run_raw(
                runner,
                ledger,
                "hdiutil-info",
                "i",
                Cmd::new(argv::hdiutil_info(), MS_QUERY),
            )
            .map_err(|_| "readback-incomplete")?;
        if untrusted(&out).is_some() || out.status != 0 {
            return Err("readback-incomplete");
        }
        image_list(&out)
    }

    fn disk_doc(
        &mut self,
        runner: &mut dyn Runner,
        ledger: &mut Ledger,
    ) -> Result<Plist, &'static str> {
        let device = self.device.clone();
        let out = self
            .run_raw(
                runner,
                ledger,
                "diskutil-info",
                "v",
                Cmd::new(argv::diskutil_info(&device), MS_QUERY),
            )
            .map_err(|_| "readback-incomplete")?;
        if untrusted(&out).is_some() || out.status != 0 {
            return Err("readback-incomplete");
        }
        plist_doc(&out)
    }

    fn mount_text(
        &mut self,
        runner: &mut dyn Runner,
        ledger: &mut Ledger,
    ) -> Result<String, &'static str> {
        let out = self
            .run_raw(
                runner,
                ledger,
                "mount",
                "v",
                Cmd::new(argv::mount_readonly(), MS_QUERY),
            )
            .map_err(|_| "readback-incomplete")?;
        if untrusted(&out).is_some() || out.status != 0 {
            return Err("readback-incomplete");
        }
        out.trusted_text().map_err(|_| "readback-incomplete")
    }

    fn remove_nodes(&mut self, volume: bool) -> Result<(), &'static str> {
        let nodes = std::mem::take(&mut self.nodes);
        let (chosen, keep): (Vec<Node>, Vec<Node>) =
            nodes.into_iter().partition(|node| node.volume == volume);
        self.nodes = keep;
        for node in chosen.into_iter().rev() {
            remove_node(&node)?;
        }
        Ok(())
    }
}

fn remove_node(node: &Node) -> Result<(), &'static str> {
    let now = volume::lstat(&node.path)
        .ok()
        .map(|st| volume::file_id(&st));
    if node.dir {
        let empty = std::fs::read_dir(&node.path)
            .map(|mut it| it.next().is_none())
            .unwrap_or(false);
        match identity::decide_rmdir(node.id, now, empty) {
            Destroy::Proceed => std::fs::remove_dir(&node.path).map_err(|_| "rmdir"),
            Destroy::SkipAbsent => Ok(()),
            Destroy::Quarantine { reason } => Err(reason),
        }
    } else {
        match now {
            None => Ok(()),
            Some(id) if id == node.id => std::fs::remove_file(&node.path).map_err(|_| "unlink"),
            Some(_) => Err("identity-mismatch"),
        }
    }
}

fn boot_gate(
    image: &Plist,
    disk: &Plist,
    flags: Option<&Vec<String>>,
    fsid: Option<[u8; 8]>,
    mdutil_text: &str,
    excluded_mount: bool,
    image_st: &FileStat,
) -> Result<(), Judgement> {
    match image.get("Format").and_then(Plist::string) {
        Some("UDSP") => {}
        Some(_) => return Err(failed("image-format")),
        None => return Err(judgement("block", "image-format-unknown", true)),
    }
    if desc_journal(image) == "fail" {
        return Err(failed("journal"));
    }
    if size_state(image) == "fail" {
        return Err(failed("oversize"));
    }
    if size_state(image) == "unknown" {
        return Err(judgement("block", "size-unknown", true));
    }
    match image.get("Volume Name").and_then(Plist::string) {
        Some(budget::VOLNAME) => {}
        Some(_) => return Err(failed("volname")),
        None => return Err(judgement("block", "volname-unknown", true)),
    }
    let Some(flags) = flags else {
        return Err(judgement("block", "flags-unknown", true));
    };
    if !identity::flags_have(flags, &["rw", "nosuid", "noexec", "nodev"]) {
        return Err(failed("flags"));
    }
    match disk.get("GlobalPermissionsEnabled").and_then(Plist::bool) {
        Some(true) => {}
        Some(false) => return Err(failed("owners")),
        None => return Err(judgement("block", "owners-unknown", true)),
    }
    match disk.get("VolumeUUID").and_then(Plist::string) {
        Some(value) if !value.is_empty() => {}
        _ => return Err(judgement("block", "uuid-unknown", true)),
    }
    match disk.get("FilesystemName").and_then(Plist::string) {
        Some(name) if name.to_ascii_lowercase().contains("journal") => {
            return Err(failed("journal"))
        }
        Some(name) if name.to_ascii_lowercase().contains("hfs") => {}
        Some(_) => return Err(judgement("block", "fs-unknown", true)),
        None => return Err(judgement("block", "fs-unknown", true)),
    }
    if fsid.is_none() {
        return Err(judgement("block", "fsid-unknown", true));
    }
    if !indexing_disabled(mdutil_text) {
        return Err(judgement("block", "index-unknown", true));
    }
    if !excluded_mount {
        return Err(judgement("block", "exclusion-unread", true));
    }
    if !volume::scratch_within(image_st) {
        return Err(failed("oversize"));
    }
    Ok(())
}

fn judge_attach_case(
    status: i32,
    disk: &Plist,
    flags: Option<&Vec<String>>,
    fsid: Option<[u8; 8]>,
    mount: &Path,
) -> Judgement {
    let flag = |name| match flags {
        Some(flags) if identity::flags_have(flags, &[name]) => "pass",
        Some(_) => "fail",
        None => "unknown",
    };
    let items = vec![
        item("attach-exit", pass_if(status == 0)),
        item("device-mapped", "pass"),
        item("flag-rw", flag("rw")),
        item("flag-nosuid", flag("nosuid")),
        item("flag-noexec", flag("noexec")),
        item("flag-nodev", flag("nodev")),
        item(
            "owners-on",
            match disk.get("GlobalPermissionsEnabled").and_then(Plist::bool) {
                Some(true) => "pass",
                Some(false) => "fail",
                None => "unknown",
            },
        ),
        item(
            "uuid-present",
            match disk.get("VolumeUUID").and_then(Plist::string) {
                Some(value) if !value.is_empty() => "pass",
                _ => "unknown",
            },
        ),
        item("fsid-present", pass_if(fsid.is_some())),
        item(
            "capacity-bound",
            match disk.get("VolumeTotalSpace").and_then(Plist::int) {
                Some(n) if n <= 80 * 1024 * 1024 => "pass",
                Some(_) => "fail",
                None => "unknown",
            },
        ),
        item(
            "no-journal",
            match disk.get("FilesystemName").and_then(Plist::string) {
                Some(name) if name.to_ascii_lowercase().contains("journal") => "fail",
                Some(_) => "pass",
                None => "unknown",
            },
        ),
        item("exec-probe", "unknown"),
    ];
    let mut judged = cases::judge_c4(&items);
    if marker_files(mount) {
        judged.notes.push("marker-visible");
    } else {
        judged.notes.push("marker-absent");
    }
    judged
}

fn item(id: &'static str, state: &'static str) -> ProbeItem {
    ProbeItem { id, state }
}

fn pass_if(ok: bool) -> &'static str {
    if ok {
        "pass"
    } else {
        "fail"
    }
}

fn key_state(doc: &Plist, key: &str, pred: impl Fn(&str) -> bool) -> &'static str {
    match doc.get(key).and_then(Plist::string) {
        Some(value) if pred(value) => "pass",
        Some(_) => "fail",
        None => "unknown",
    }
}

fn desc_journal(doc: &Plist) -> &'static str {
    match doc.get("Format Description").and_then(Plist::string) {
        Some(value) if value.to_ascii_lowercase().contains("journal") => "fail",
        Some(_) => "pass",
        None => "unknown",
    }
}

fn size_state(doc: &Plist) -> &'static str {
    match doc.get("Total Bytes").and_then(Plist::int) {
        Some(n) if (1..=80 * 1024 * 1024).contains(&n) => "pass",
        Some(_) => "fail",
        None => "unknown",
    }
}

fn marker_files(root: &Path) -> bool {
    let marker = volume::lstat(&root.join(".metadata_never_index")).ok();
    let nolog = volume::lstat(&root.join(".fseventsd").join("no_log")).ok();
    matches!((marker, nolog), (Some(a), Some(b)) if a.is_file && b.is_file)
}

fn indexing_disabled(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("indexing disabled") && !lower.contains("indexing enabled")
}

fn untrusted(out: &CmdOut) -> Option<&'static str> {
    if out.timed_out {
        Some("timeout")
    } else if out.overflow {
        Some("overflow")
    } else if out.reap_unknown {
        Some("reap-unknown")
    } else {
        None
    }
}

fn tool_end(out: &CmdOut) -> ToolEnd {
    if out.timed_out {
        ToolEnd::Timeout
    } else if out.overflow {
        ToolEnd::Overflow
    } else {
        ToolEnd::Exited(out.status)
    }
}

fn plist_doc(out: &CmdOut) -> Result<Plist, &'static str> {
    let text = out.trusted_text()?;
    plist::parse_plist(&text)
}

fn image_list(out: &CmdOut) -> Result<Vec<ImageAttach>, &'static str> {
    let doc = plist_doc(out)?;
    plist::attached_images(&doc)
}

fn bind_attach(
    stdout: &str,
    image: &str,
    mount: &str,
) -> Result<(String, Option<String>, String), &'static str> {
    let doc = plist::parse_plist(stdout)?;
    let mut images = plist::attached_images(&doc)?;
    if images.len() == 1 && images[0].image_path.is_empty() {
        images[0].image_path = image.to_string();
    }
    if images.len() != 1 {
        return Err("plist-ambiguous");
    }
    let hit = &images[0];
    if hit.image_path != image || !plist::whole_disk(&hit.whole) {
        return Err("identity-mismatch");
    }
    let point = hit.mount.clone().unwrap_or_default();
    if point != mount {
        return Err("identity-mismatch");
    }
    Ok((hit.whole.clone(), hit.slice.clone(), point))
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), &'static str> {
    let mut file = crate::osutil::file_0600(path).map_err(|_| "key-file")?;
    file.write_all(bytes).map_err(|_| "key-file")?;
    file.sync_all().map_err(|_| "key-file")?;
    Ok(())
}

fn read_token(dir: &Path, name: &str) -> Result<String, &'static str> {
    if !crate::osutil::component_ok(name) {
        return Err("token");
    }
    let path = dir.join(name);
    let st = volume::lstat(&path)?;
    if !st.is_file || st.nlink != 1 || st.len == 0 || st.len > 64 {
        return Err("token");
    }
    let text = std::fs::read_to_string(&path).map_err(|_| "token")?;
    if text.len() as u64 != st.len {
        return Err("token");
    }
    Ok(text.trim().to_string())
}

fn load_current_exe() -> Result<Vec<u8>, &'static str> {
    let path = std::env::current_exe().map_err(|_| "probe-exe")?;
    let st = volume::lstat(&path)?;
    if st.is_link || !st.is_file || st.len == 0 || st.len > budget::UPLOAD_CAP as u64 {
        return Err("probe-size");
    }
    std::fs::read(&path).map_err(|_| "probe-read")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::GOLDEN;

    struct Lab {
        mismatch: bool,
        diskutil_n: u32,
        calls: Vec<Vec<String>>,
        spawns: Vec<Vec<String>>,
        ledger_path: PathBuf,
        create_ledgers: Vec<String>,
        attached: bool,
        cloned: bool,
        started: bool,
        stopped: bool,
        deleted: bool,
        clone_name: String,
        image: String,
        mount: String,
        bad_delete: bool,
        owned_visible: bool,
    }

    impl Lab {
        fn new(mismatch: bool, ledger: PathBuf) -> Self {
            Self {
                mismatch,
                diskutil_n: 0,
                calls: Vec::new(),
                spawns: Vec::new(),
                ledger_path: ledger,
                create_ledgers: Vec::new(),
                attached: false,
                cloned: false,
                started: false,
                stopped: false,
                deleted: false,
                clone_name: String::new(),
                image: String::new(),
                mount: String::new(),
                bad_delete: false,
                owned_visible: false,
            }
        }

        fn list_json(&mut self) -> String {
            let mut body = format!("[{{\"Name\":\"{GOLDEN}\",\"State\":\"stopped\"}}");
            if self.cloned && !self.deleted && !self.clone_name.is_empty() {
                let state = if self.started && !self.stopped {
                    "running"
                } else {
                    "stopped"
                };
                body = format!("[{{\"Name\":\"{GOLDEN}\",\"State\":\"stopped\"}},{{\"Name\":\"{}\",\"State\":\"{state}\"}}", self.clone_name);
                self.owned_visible = true;
            }
            body.push(']');
            body
        }
    }

    fn plist_wrap(inner: &str) -> String {
        format!("<?xml version=\"1.0\"?><plist version=\"1.0\">{inner}</plist>")
    }

    impl Runner for Lab {
        fn run(&mut self, cmd: &Cmd) -> CmdOut {
            if let Err(code) = runner::check_cmd(cmd) {
                return CmdOut::code(127, "", code);
            }
            self.calls.push(cmd.argv.clone());
            let argv = &cmd.argv;
            let bin = argv[0].rsplit('/').next().unwrap_or("");
            let sub = argv.get(1).map(String::as_str).unwrap_or("");
            if bin == "hdiutil" && sub == "create" {
                if let Some(path) = argv.last() {
                    let _ = std::fs::File::create(path);
                    self.image = path.clone();
                }
                let text = std::fs::read_to_string(&self.ledger_path).unwrap_or_default();
                self.create_ledgers.push(text);
            }
            if bin == "hdiutil" && sub == "attach" {
                self.attached = true;
                self.mount = argv
                    .get(argv.len().saturating_sub(2))
                    .cloned()
                    .unwrap_or_default();
                self.image = argv.last().cloned().unwrap_or_default();
            }
            if bin == "hdiutil" && sub == "detach" {
                self.attached = false;
            }
            if bin == "tart" && sub == "clone" {
                self.cloned = true;
                self.clone_name = argv.last().cloned().unwrap_or_default();
            }
            if bin == "tart" && sub == "stop" {
                self.stopped = true;
            }
            if bin == "tart" && sub == "delete" {
                let name = argv.last().map(String::as_str).unwrap_or("");
                if name == GOLDEN || !self.owned_visible || name != self.clone_name {
                    self.bad_delete = true;
                }
                self.deleted = true;
                self.owned_visible = false;
            }
            if bin == "ssh-keygen" {
                if let Some(pos) = argv.iter().position(|a| a == "-f") {
                    if let Some(path) = argv.get(pos + 1) {
                        let _ = std::fs::write(path, b"test-key\n");
                        let _ = std::fs::write(
                            format!("{path}.pub"),
                            format!("ssh-ed25519 {} probe\n", "A".repeat(68)),
                        );
                    }
                }
            }
            let stdout = match (bin, sub) {
                ("ls", _) => "drwx------  2 role staff 64 Nov 11 00:00 a\n".into(),
                ("tmutil", "isexcluded") => format!("[Excluded] {}\n", argv.last().map(String::as_str).unwrap_or("")),
                ("tmutil", _) => String::new(),
                ("hdiutil", "imageinfo") => plist_wrap(
                    "<dict><key>Format</key><string>UDSP</string><key>Format Description</key><string>SPARSE</string><key>Layout</key><string>NONE</string><key>Volume Name</key><string>GCU-MAILBOX</string><key>Total Bytes</key><integer>67108864</integer></dict>",
                ),
                ("hdiutil", "info") if self.attached => plist_wrap(&format!(
                    "<dict><key>images</key><array><dict><key>image-path</key><string>{}</string><key>system-entities</key><array><dict><key>dev-entry</key><string>/dev/disk4</string></dict><dict><key>dev-entry</key><string>/dev/disk4s1</string><key>mount-point</key><string>{}</string></dict></array></dict></array></dict>",
                    xml_escape(&self.image),
                    xml_escape(&self.mount)
                )),
                ("hdiutil", "info") => plist_wrap("<dict><key>images</key><array></array></dict>"),
                ("hdiutil", "attach") => plist_wrap(&format!(
                    "<dict><key>image-path</key><string>{}</string><key>system-entities</key><array><dict><key>dev-entry</key><string>/dev/disk4</string></dict><dict><key>dev-entry</key><string>/dev/disk4s1</string><key>mount-point</key><string>{}</string></dict></array></dict>",
                    xml_escape(&self.image),
                    xml_escape(&self.mount)
                )),
                ("hdiutil", _) => String::new(),
                ("diskutil", _) => {
                    self.diskutil_n += 1;
                    let uuid = if self.mismatch && self.diskutil_n >= 2 {
                        "22222222-2222-2222-2222-222222222222"
                    } else {
                        "11111111-1111-1111-1111-111111111111"
                    };
                    plist_wrap(&format!(
                        "<dict><key>VolumeUUID</key><string>{uuid}</string><key>DeviceNode</key><string>/dev/disk4s1</string><key>FilesystemName</key><string>Case-sensitive HFS+</string><key>MountPoint</key><string>{}</string><key>Writable</key><true/><key>GlobalPermissionsEnabled</key><true/><key>VolumeTotalSpace</key><integer>67108864</integer></dict>",
                        xml_escape(&self.mount)
                    ))
                }
                ("mount", _) if self.attached => format!("/dev/disk4s1 on {} (hfs, local, nodev, nosuid, noexec, rw)\n", self.mount),
                ("mount", _) => "/dev/disk1 on / (apfs, local)\n".into(),
                ("mdutil", _) => "Indexing disabled.\n".into(),
                ("tart", "list") => self.list_json(),
                ("tart", "ip") => "203.0.113.10\n".into(),
                ("ssh-keyscan", _) => format!("203.0.113.10 ssh-ed25519 {}\n", "A".repeat(68)),
                _ => String::new(),
            };
            CmdOut::ok(stdout)
        }

        fn spawn(&mut self, cmd: &Cmd) -> Result<u64, &'static str> {
            runner::check_cmd(cmd)?;
            self.spawns.push(cmd.argv.clone());
            if cmd.argv.get(1).map(String::as_str) == Some("run") {
                self.started = true;
            }
            Ok(1)
        }

        fn poll(&mut self, _id: u64) -> crate::runner::Poll {
            crate::runner::Poll::Running
        }

        fn finish(&mut self, _id: u64, _timeout_ms: u64) -> CmdOut {
            CmdOut::ok("")
        }
    }

    fn xml_escape(text: &str) -> String {
        text.replace('&', "&amp;").replace('<', "&lt;")
    }

    fn temp_out(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "s11-host-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&path);
        crate::osutil::mkdir_0700(&path).unwrap();
        path
    }

    fn ids() -> [(&'static str, &'static str); 3] {
        [
            (
                "0123456789abcdef0123456789abcdef",
                "abcdef0123456789abcdef0123456789",
            ),
            (
                "1123456789abcdef0123456789abcdef",
                "bbcdef0123456789abcdef0123456789",
            ),
            (
                "2123456789abcdef0123456789abcdef",
                "cbcdef0123456789abcdef0123456789",
            ),
        ]
    }

    fn step_line(text: &str, kind: &str, step: &str) -> bool {
        text.lines().any(|line| {
            line.contains(&format!("\"kind\":\"{kind}\""))
                && line.contains(&format!("\"step\":\"{step}\""))
        })
    }

    #[test]
    fn dry_run_does_not_call_the_runner() {
        let out = temp_out("dry");
        let mut lab = Lab::new(false, out.join("ledger").join("events.jsonl"));
        let ids = ids();
        let req = RunRequest {
            case: Case::C1,
            tart: Path::new("/Users/eval/bin/tart"),
            home: Path::new("/Users/eval"),
            out: &out,
            tart_home: None,
            dry_run: true,
            exchange: false,
            ids: &ids,
            probe: Some(b"probe-bytes"),
        };
        let outcome = run_case(&req, &mut lab, &mut |_| {}).unwrap();
        assert_eq!(outcome.status, "dry_run");
        assert!(lab.calls.is_empty());
        assert!(lab.spawns.is_empty());
        let report = String::from_utf8(outcome.report).unwrap();
        assert!(report.contains("DEFAULT-network/mechanics-only"));
        assert!(!report.contains("/Users"));
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn intent_is_durable_and_mismatch_does_not_detach() {
        let out = temp_out("mis");
        let ledger_path = out.join("ledger").join("events.jsonl");
        let mut lab = Lab::new(true, ledger_path);
        let ids = ids();
        let probe = b"probe-bytes";
        let req = RunRequest {
            case: Case::C2,
            tart: Path::new("/Users/eval/bin/tart"),
            home: Path::new("/Users/eval"),
            out: &out,
            tart_home: None,
            dry_run: false,
            exchange: false,
            ids: &ids,
            probe: Some(probe),
        };
        let outcome = run_case(&req, &mut lab, &mut |_| {}).unwrap();
        assert!(!lab.create_ledgers.is_empty());
        assert!(step_line(
            &lab.create_ledgers[0],
            "intent",
            "hdiutil-create"
        ));
        assert!(!step_line(
            &lab.create_ledgers[0],
            "effect",
            "hdiutil-create"
        ));
        assert_eq!(
            lab.calls
                .iter()
                .filter(|c| c.get(1).map(String::as_str) == Some("clone"))
                .count(),
            1
        );
        assert!(!lab
            .calls
            .iter()
            .any(|c| c.get(1).map(String::as_str) == Some("detach")));
        assert!(!lab.bad_delete);
        assert!(
            std::path::Path::new(&lab.image).exists(),
            "image must remain"
        );
        let report = String::from_utf8(outcome.report).unwrap();
        assert_eq!(outcome.status, "quarantine");
        assert!(report.contains("identity-mismatch") || report.contains("quarantine"));
        assert!(!report.contains("203.0.113.10"));
        assert!(!report.contains("/Users"));
        assert!(!report.contains("sparseimage"));
        assert!(report.contains("DEFAULT-network/mechanics-only"));
        let installs = lab
            .calls
            .iter()
            .filter(|c| c.last().is_some_and(|s| s.starts_with("umask 077")))
            .count();
        assert_eq!(installs, 1);
        assert!(lab
            .calls
            .iter()
            .any(|c| c.iter().any(|a| a.ends_with("id_ed25519_tart"))));
        let run = lab
            .spawns
            .iter()
            .find(|c| c.get(1).map(String::as_str) == Some("run"))
            .unwrap();
        assert!(run.iter().any(|a| a == "--no-graphics"));
        assert!(run.iter().any(|a| a == "--net-softnet"));
        assert!(!run
            .iter()
            .any(|a| a.contains("softnet-block") || a.contains("softnet-allow")));
        let create = lab
            .calls
            .iter()
            .find(|c| {
                c.first().is_some_and(|b| b.ends_with("/hdiutil"))
                    && c.get(1).map(String::as_str) == Some("create")
            })
            .unwrap();
        let image = Path::new(create.last().unwrap());
        let seed = Path::new(&create[create.len() - 2]);
        assert_eq!(create, &argv::hdiutil_create(seed, image));
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn c4_create_and_attach_use_templates_and_unlink_when_identity_matches() {
        let out = temp_out("c4");
        let mut lab = Lab::new(false, out.join("ledger").join("events.jsonl"));
        let ids = ids();
        let req = RunRequest {
            case: Case::C4,
            tart: Path::new("/Users/eval/bin/tart"),
            home: Path::new("/Users/eval"),
            out: &out,
            tart_home: None,
            dry_run: false,
            exchange: false,
            ids: &ids,
            probe: Some(b"probe-bytes"),
        };
        let outcome = run_case(&req, &mut lab, &mut |_| {}).unwrap();
        assert!(lab.spawns.is_empty());
        assert!(!lab.calls.iter().any(|c| c[0].ends_with("/tart")));
        assert!(!lab.bad_delete);
        let creates: Vec<_> = lab
            .calls
            .iter()
            .filter(|c| c.get(1).map(String::as_str) == Some("create") && c[0].ends_with("hdiutil"))
            .collect();
        assert_eq!(creates.len(), 2);
        for create in &creates {
            let image = Path::new(create.last().unwrap());
            let seed = Path::new(&create[create.len() - 2]);
            assert_eq!(*create, &argv::hdiutil_create(seed, image));
            assert!(!image.exists());
        }
        let attaches: Vec<_> = lab
            .calls
            .iter()
            .filter(|c| c.get(1).map(String::as_str) == Some("attach"))
            .collect();
        assert_eq!(attaches.len(), 1);
        let attach = attaches[0];
        let image = Path::new(attach.last().unwrap());
        let mount = Path::new(&attach[attach.len() - 2]);
        assert_eq!(attach, &argv::hdiutil_attach(mount, image));
        let detaches = lab
            .calls
            .iter()
            .filter(|c| c.get(1).map(String::as_str) == Some("detach"))
            .count();
        assert_eq!(detaches, 1);
        let report = String::from_utf8(outcome.report).unwrap();
        assert!(report.contains("probe-unknown"));
        assert!(report.contains("marker-not-pre-mount-proof"));
        assert!(!report.contains("sparseimage"));
        assert!(report.contains("DEFAULT-network/mechanics-only"));
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn guest_entry_does_not_run_off_macos() {
        if real_path_supported() {
            return;
        }
        assert_eq!(guest_entry(Case::C1), 2);
        assert_eq!(guest_entry(Case::C8), 2);
    }
}

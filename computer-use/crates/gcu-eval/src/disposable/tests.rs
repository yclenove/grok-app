//! Fake evidence only; none of these checks establish a host sandbox or VM.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use std::cell::RefCell;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

fn binding() -> Binding {
    Binding {
        attempt: "attempt-unique-1".into(),
        instance: "fake-vm-1".into(),
        boot: "fake-boot-1".into(),
        desktop_user: "fake-uid:desktop".into(),
        image_digest: "fake-image-digest".into(),
        policy_digest: "fake-policy-digest".into(),
        scope_ack: gcu_host_protocol::scope::Ack {
            run_id: "attempt-unique-1".into(),
            generation: 1,
            scope_digest: "fake-scope-digest".into(),
        },
    }
}

struct FakeVerifier;
impl AttestationVerifier for FakeVerifier {
    fn authentic(&self, proof: &Attestation) -> bool {
        // Test-only trust oracle. This byte string is NOT a signature format.
        proof.proof == b"test-only-pinned-authority"
    }
}

struct FakeProvider {
    prepared: Prepared,
    cleanup: Option<CleanupProof>,
    events: Rc<RefCell<Vec<&'static str>>>,
    root: Option<PathBuf>,
    workers: Vec<Child>,
}

impl FakeProvider {
    fn ready() -> Self {
        let binding = binding();
        Self {
            prepared: Prepared {
                binding: Some(binding.clone()),
                checks: Probe::ALL
                    .into_iter()
                    .map(|probe| (probe, Check::Passed))
                    .collect(),
                attestations: vec![Attestation {
                    binding: binding.clone(),
                    guarantees: Guarantee::ALL.to_vec(),
                    expires_at: 200,
                    proof: b"test-only-pinned-authority".to_vec(),
                }],
                scope_ack: Some(binding.scope_ack.clone()),
            },
            cleanup: Some(CleanupProof {
                binding: Some(binding),
                whole_tree: Check::Passed,
                storage: Check::Passed,
                channels: Check::Passed,
            }),
            events: Rc::default(),
            root: None,
            workers: Vec::new(),
        }
    }
}

impl DisposableProvider for FakeProvider {
    fn prepare(&mut self) -> Prepared {
        self.events.borrow_mut().push("gate");
        self.prepared.clone()
    }
    fn teardown(&mut self, cause: CleanupCause) -> Option<CleanupProof> {
        self.events.borrow_mut().push("cleanup");
        let mut proof = self.cleanup.clone();
        // Whole fake tree is a registry of owned handles, including a workload
        // marked as a detached descendant. No numeric-PID tree inference.
        for child in &mut self.workers {
            let killed = child.kill();
            let reaped = child.wait();
            if killed.is_err() || reaped.is_err() {
                if let Some(proof) = &mut proof {
                    proof.whole_tree = Check::Failed;
                }
            }
        }
        self.workers.clear();
        if let Some(root) = self.root.take() {
            if std::fs::remove_dir_all(root).is_err() {
                if let Some(proof) = &mut proof {
                    proof.storage = Check::Failed;
                }
            }
        }
        assert!(matches!(
            cause,
            CleanupCause::Finished
                | CleanupCause::Refused
                | CleanupCause::Cancelled
                | CleanupCause::ParentDeath
        ));
        proof
    }
}

fn run(fake: &mut FakeProvider) -> (Option<&'static str>, EnvironmentReport) {
    let events = fake.events.clone();
    supervise(fake, Some(&binding()), 100, &FakeVerifier, || {
        events.borrow_mut().push("credentialed_tool_preflight");
        events.borrow_mut().push("task_client");
        "oracle_pass"
    })
}

fn refuses(fake: &mut FakeProvider, reason: Refusal) -> EnvironmentReport {
    let (result, report) = run(fake);
    assert!(result.is_none());
    assert_eq!(report.refusal, Some(reason));
    assert_eq!(fake.events.borrow().as_slice(), ["gate", "cleanup"]);
    assert_eq!(report.guarantees.len(), 5);
    report
}

#[test]
fn gate_precedes_even_credentialed_tool_preflight_and_cleanup_precedes_pass() {
    let mut fake = FakeProvider::ready();
    let (value, report) = run(&mut fake);
    assert_eq!(value, Some("oracle_pass"));
    assert_eq!(report.refusal, None);
    assert!(report.complete());
    assert_eq!(
        fake.events.borrow().as_slice(),
        [
            "gate",
            "credentialed_tool_preflight",
            "task_client",
            "cleanup"
        ]
    );
    for row in report.guarantees {
        assert_eq!(row.machine, EvidenceState::Checked);
        assert_eq!(row.external, EvidenceState::Attested);
    }
    assert!(report.cleanup.unwrap().complete());
}

#[test]
fn every_machine_probe_fails_closed_even_with_all_external_attestations() {
    for probe in Probe::ALL {
        for check in [Check::Failed, Check::Unknown] {
            let mut fake = FakeProvider::ready();
            fake.prepared
                .checks
                .iter_mut()
                .find(|(key, _)| *key == probe)
                .unwrap()
                .1 = check;
            refuses(
                &mut fake,
                if check == Check::Failed {
                    Refusal::MachineCheckFailed
                } else {
                    Refusal::CriticalUnknown
                },
            );
        }
        let mut fake = FakeProvider::ready();
        fake.prepared.checks.retain(|(key, _)| *key != probe);
        refuses(&mut fake, Refusal::CriticalUnknown);
        let mut fake = FakeProvider::ready();
        fake.prepared.checks.push((probe, Check::Passed));
        refuses(&mut fake, Refusal::CriticalUnknown);
    }
}

#[test]
fn every_required_attestation_is_required_even_with_all_machine_checks() {
    for guarantee in Guarantee::ALL {
        let mut fake = FakeProvider::ready();
        fake.prepared.attestations[0]
            .guarantees
            .retain(|claim| *claim != guarantee);
        refuses(&mut fake, Refusal::MissingAttestation);
    }
}

#[test]
fn unsigned_expired_replayed_and_wrong_subject_attestations_are_untrusted() {
    for variant in 0..10 {
        let mut fake = FakeProvider::ready();
        let proof = &mut fake.prepared.attestations[0];
        match variant {
            0 => proof.proof.clear(),
            1 => proof.proof = b"--disposable".to_vec(),
            2 => proof.expires_at = 100,
            3 => proof.binding.attempt.push('x'),
            4 => proof.binding.instance.push('x'),
            5 => proof.binding.boot.push('x'),
            6 => proof.binding.desktop_user.push('x'),
            7 => proof.binding.image_digest.push('x'),
            8 => proof.binding.policy_digest.push('x'),
            _ => proof.binding.scope_ack.generation += 1,
        }
        refuses(&mut fake, Refusal::UntrustedAttestation);
    }
}

#[test]
fn unavailable_incomplete_or_mismatched_environment_is_not_run() {
    let mut fake = FakeProvider::ready();
    fake.prepared.binding = None;
    // Make cleanup explicitly describe the absence of an allocation.
    fake.cleanup.as_mut().unwrap().binding = None;
    refuses(&mut fake, Refusal::ProviderUnavailable);
    for variant in 0..3 {
        let mut fake = FakeProvider::ready();
        match variant {
            0 => fake.prepared.binding.as_mut().unwrap().boot.clear(),
            1 => fake.prepared.binding.as_mut().unwrap().attempt.push('x'),
            _ => fake.prepared.binding.as_mut().unwrap().scope_ack.generation = 0,
        }
        fake.cleanup.as_mut().unwrap().binding = fake.prepared.binding.clone();
        refuses(&mut fake, Refusal::BindingMismatch);
    }
}

#[test]
fn missing_or_mismatched_native_scope_ack_never_starts_preflight() {
    for variant in 0..4 {
        let mut fake = FakeProvider::ready();
        match variant {
            0 => fake.prepared.scope_ack = None,
            1 => fake.prepared.scope_ack.as_mut().unwrap().run_id.push('x'),
            2 => fake.prepared.scope_ack.as_mut().unwrap().generation += 1,
            _ => fake
                .prepared
                .scope_ack
                .as_mut()
                .unwrap()
                .scope_digest
                .push('x'),
        }
        refuses(&mut fake, Refusal::MissingScopeAck);
    }
}

#[test]
fn any_cleanup_failure_missing_receipt_or_wrong_binding_withholds_a_pass() {
    for variant in 0..9 {
        let mut fake = FakeProvider::ready();
        let proof = fake.cleanup.as_mut().unwrap();
        match variant {
            0 => proof.whole_tree = Check::Failed,
            1 => proof.storage = Check::Failed,
            2 => proof.channels = Check::Failed,
            3 => proof.whole_tree = Check::Unknown,
            4 => proof.storage = Check::Unknown,
            5 => proof.channels = Check::Unknown,
            6 => proof.binding.as_mut().unwrap().attempt.push('x'),
            7 => proof.binding = None,
            _ => fake.cleanup = None,
        }
        let (value, report) = run(&mut fake);
        assert!(value.is_none());
        assert_eq!(report.refusal, Some(Refusal::CleanupFailed));
    }
}

#[test]
fn cleanup_still_runs_on_refusal_and_unwind() {
    let mut fake = FakeProvider::ready();
    fake.prepared.scope_ack = None;
    let report = refuses(&mut fake, Refusal::MissingScopeAck);
    assert_eq!(report.cleanup.unwrap().cause, CleanupCause::Refused);
    let mut fake = FakeProvider::ready();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        supervise(&mut fake, Some(&binding()), 100, &FakeVerifier, || {
            panic!("cancel")
        })
    }));
    assert!(result.is_err());
    assert_eq!(fake.events.borrow().as_slice(), ["gate", "cleanup"]);
}

/// Local controlled file broker; safe and leaking configurations share the
/// same existing sentinel. A missing file cannot accidentally count as denial.
fn file_probe(expose: bool) -> Check {
    let root = scratch("file-probe");
    let sentinel = root.join("host-sentinel");
    std::fs::write(&sentinel, b"synthetic-canary").unwrap();
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"synthetic-canary");
    let guest = root.join("guest");
    std::fs::create_dir(&guest).unwrap();
    let allowed_root = if expose { &root } else { &guest };
    // The fake file broker enforces this root. Tests inject the wrong root to
    // prove a real canary read is detected, rather than trusting an absent file.
    let observed = if sentinel.starts_with(allowed_root) {
        std::fs::read(&sentinel)
    } else {
        Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
    };
    std::fs::remove_dir_all(root).unwrap();
    match observed {
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => Check::Passed,
        Ok(_) => Check::Failed,
        Err(_) => Check::Unknown,
    }
}

/// Local gateway simulator. Route classes never become real LAN, metadata,
/// DNS, public, IPv6 or QUIC destinations. Test clients use loopback only.
fn network_probes(leaking_route: Option<usize>) -> Vec<(Probe, Check)> {
    let probes = [
        Probe::HostIpcDenied,
        Probe::LanDenied,
        Probe::MetadataDenied,
        Probe::PublicDenied,
        Probe::DnsDenied,
        Probe::Ipv6Denied,
        Probe::QuicDenied,
    ];
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let gateway = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut count = 0;
        while count < 7 && Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    // BSD/macOS accepted sockets inherit the listener's non-blocking mode.
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    let mut route = [0];
                    stream.read_exact(&mut route).unwrap();
                    stream
                        .write_all(&[u8::from(leaking_route == Some(usize::from(route[0])))])
                        .unwrap();
                    count += 1;
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(err) => panic!("local gateway: {err}"),
            }
        }
        assert_eq!(count, 7);
    });
    let checks = probes
        .into_iter()
        .enumerate()
        .map(|(route, probe)| {
            let mut stream =
                std::net::TcpStream::connect_timeout(&address, Duration::from_secs(1)).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            stream.write_all(&[u8::try_from(route).unwrap()]).unwrap();
            let mut reply = [0];
            stream.read_exact(&mut reply).unwrap();
            (
                probe,
                if reply == [0] {
                    Check::Passed
                } else {
                    Check::Failed
                },
            )
        })
        .collect();
    gateway.join().unwrap();
    checks
}

#[test]
fn controlled_negative_probes_detect_file_and_every_gateway_route_leak() {
    assert_eq!(file_probe(false), Check::Passed);
    let mut fake = FakeProvider::ready();
    fake.prepared
        .checks
        .iter_mut()
        .find(|(probe, _)| *probe == Probe::HostFileDenied)
        .unwrap()
        .1 = file_probe(true);
    refuses(&mut fake, Refusal::MachineCheckFailed);
    assert!(network_probes(None)
        .iter()
        .all(|(_, check)| *check == Check::Passed));
    for route in 0..7 {
        let mut fake = FakeProvider::ready();
        for (probe, check) in network_probes(Some(route)) {
            fake.prepared
                .checks
                .iter_mut()
                .find(|(key, _)| *key == probe)
                .unwrap()
                .1 = check;
        }
        refuses(&mut fake, Refusal::MachineCheckFailed);
    }
}

fn scratch(label: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("gcu-s6-{label}-{}-{sequence}", std::process::id()));
    std::fs::create_dir(&path).unwrap();
    path
}

fn spawn_role(role: &str, root: &std::path::Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "disposable::tests::parent_death_child",
            "--nocapture",
        ])
        .env("GCU_S6_TEST_ROLE", role)
        .env("GCU_S6_TEST_ROOT", root)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn wait_file(path: &std::path::Path) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !path.is_file() {
        assert!(Instant::now() < deadline, "missing {}", path.display());
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Subprocess roles used only by the parent-death test. The external fake
/// supervisor owns handles and storage. Killing the owner bypasses Rust Drop.
#[test]
fn parent_death_child() {
    let Ok(role) = std::env::var("GCU_S6_TEST_ROLE") else {
        return;
    };
    let root = PathBuf::from(std::env::var_os("GCU_S6_TEST_ROOT").unwrap());
    match role.as_str() {
        "workload" => {
            std::thread::sleep(Duration::from_secs(20));
        }
        "owner" => {
            let mut supervisor = spawn_role("supervisor", &root)
                .stdin(Stdio::piped())
                .spawn()
                .unwrap();
            let _control = supervisor.stdin.take().unwrap();
            // `_control` is the only owner endpoint. SIGKILL/TerminateProcess
            // closes it; the supervisor's stdin gets EOF without a Drop hook.
            std::thread::sleep(Duration::from_secs(20));
            let _ = supervisor.kill();
            let _ = supervisor.wait();
        }
        "supervisor" => {
            let mut fake = FakeProvider::ready();
            let tree = root.join("attempt");
            std::fs::create_dir_all(tree.join("detached-descendant")).unwrap();
            std::fs::write(tree.join("auth"), b"synthetic-eval-auth").unwrap();
            for _ in 0..2 {
                fake.workers.push(
                    spawn_role("workload", &root)
                        .stdin(Stdio::null())
                        .spawn()
                        .unwrap(),
                );
            }
            fake.root = Some(tree);
            std::fs::write(root.join("ready"), b"ready").unwrap();
            let mut control = [0];
            assert_eq!(std::io::stdin().read(&mut control).unwrap(), 0);
            let proof = fake.teardown(CleanupCause::ParentDeath).unwrap();
            let receipt = CleanupReceipt {
                cause: CleanupCause::ParentDeath,
                binding_matches: proof.binding == Some(binding()),
                whole_tree: proof.whole_tree,
                storage: proof.storage,
                channels: proof.channels,
            };
            std::fs::write(
                root.join("receipt.tmp"),
                serde_json::to_vec(&receipt).unwrap(),
            )
            .unwrap();
            std::fs::rename(root.join("receipt.tmp"), root.join("receipt.json")).unwrap();
        }
        _ => panic!("unknown test role"),
    }
}

#[test]
fn parent_death_external_owner_reaps_entire_fake_tree_and_writes_receipt() {
    let root = scratch("parent-death");
    let mut owner = spawn_role("owner", &root)
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    wait_file(&root.join("ready"));
    owner.kill().unwrap();
    owner.wait().unwrap();
    wait_file(&root.join("receipt.json"));
    let receipt: CleanupReceipt =
        serde_json::from_slice(&std::fs::read(root.join("receipt.json")).unwrap()).unwrap();
    assert!(receipt.complete());
    assert_eq!(receipt.cause, CleanupCause::ParentDeath);
    assert!(!root.join("attempt").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn json_html_and_ab_reports_render_all_rows_and_sanitized_cleanup() {
    let mut fake = FakeProvider::ready();
    let (_, environment) = run(&mut fake);
    let cfg = crate::RunConfig::default();
    let mut row = crate::runner::blank(
        "<script>task</script>",
        "native",
        &cfg,
        crate::RunStatus::NotRun,
        "offline fake evidence",
    );
    row.assurance.environment = Some(environment);
    let report = crate::SuiteReport {
        model: "fake".into(),
        platform: cfg.platform,
        group: "A".into(),
        summary: crate::summarize(&[row.clone()]),
        tasks: vec![row.clone()],
    };
    let root = scratch("report");
    crate::write_reports(&root, &report).unwrap();
    let json = std::fs::read_to_string(root.join("summary.json")).unwrap();
    let html = crate::render_html(&report);
    for text in [&json, &html] {
        for label in [
            "disposable_desktop",
            "file_ipc_isolation",
            "endpoint_only_egress",
            "no_ambient_credentials",
            "native_control_and_exit",
            "checked",
            "attested",
            "whole_tree",
        ] {
            assert!(text.contains(label), "missing {label}");
        }
        for secret in [
            "test-only-pinned-authority",
            "fake-vm-1",
            "fake-image-digest",
            "fake-policy-digest",
        ] {
            assert!(!text.contains(secret));
        }
    }
    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;"));
    let trial = crate::ab::trial_from_outcome(&row).unwrap();
    assert_eq!(trial.environment_contract, row.assurance.environment);
    let unknown = unavailable_report();
    assert_eq!(unknown.refusal, Some(Refusal::ProviderUnavailable));
    assert!(
        unknown
            .guarantees
            .iter()
            .all(|row| row.machine == EvidenceState::Unknown
                && row.external == EvidenceState::Unknown)
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn critical_unknown_or_incomplete_receipt_never_renders_or_scores_as_pass() {
    let (_, valid) = run(&mut FakeProvider::ready());
    for variant in 0..8 {
        let mut environment = valid.clone();
        match variant {
            0 => environment.guarantees[0].machine = EvidenceState::Unknown,
            1 => environment.guarantees[0].external = EvidenceState::Unknown,
            2 => environment.guarantees[0].checks[0].1 = Check::Unknown,
            3 => {
                environment.guarantees.pop();
            }
            4 => environment.scope_ack = EvidenceState::Unknown,
            5 => environment.cleanup = None,
            6 => environment.cleanup.as_mut().unwrap().whole_tree = Check::Unknown,
            _ => environment.cleanup.as_mut().unwrap().cause = CleanupCause::ParentDeath,
        }
        // A stale or dishonest stored "pass" with no explicit refusal is still
        // not_run, including the derived summary and A/B comparison inputs.
        assert!(!environment.complete());
        let mut row = crate::runner::blank(
            "task",
            "native",
            &crate::RunConfig::default(),
            crate::RunStatus::Pass,
            "",
        );
        row.assurance.environment = Some(environment);
        assert!(!row.scored());
        assert_eq!(crate::summarize(&[row.clone()]).passed, 0);
        assert_eq!(
            crate::ab::trial_from_outcome(&row).unwrap().status,
            crate::RunStatus::NotRun
        );
        let report = crate::SuiteReport {
            model: "fake".into(),
            platform: "linux".into(),
            group: "A".into(),
            summary: crate::summarize(&[row.clone()]),
            tasks: vec![row],
        };
        let html = crate::render_html(&report);
        assert!(html.contains("<td>not_run</td>"));
        assert!(!html.contains("<td>pass</td>"));
        let root = scratch("unknown-report");
        crate::write_reports(&root, &report).unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("summary.json")).unwrap()).unwrap();
        assert_eq!(json["tasks"][0]["status"], "not_run");
        assert_eq!(json["summary"]["passed"], 0);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn runner_gate_stops_the_whole_suite_without_local_fallback_or_credential_copy() {
    let task = crate::load_tasks(&crate::bundled_tasks_dir().unwrap())
        .unwrap()
        .into_iter()
        .find(|task| task.id == "native-form-01")
        .unwrap();
    let mut later = task.clone();
    later.id = "must-not-even-validate-later-oracle".into();
    later.pass_script = PathBuf::from("missing-later-oracle");
    let root = scratch("runner-gate");
    let sentinel = root.join("not-an-executable");
    std::fs::write(&sentinel, b"synthetic-only").unwrap();
    for run_local in [false, true] {
        for attested in [false, true] {
            let cfg = crate::RunConfig {
                real_driver: true,
                run_local,
                attest_disposable_desktop: attested,
                grok_bin: Some(sentinel.clone()),
                gcu_bin: Some(sentinel.clone()),
                fixture_bin: Some(sentinel.clone()),
                auth_seed: Some(root.join("missing-seed")),
                include_desktop: false,
                ..crate::RunConfig::default()
            };
            let report =
                crate::block_on(crate::run_tasks(&[task.clone(), later.clone()], &cfg)).unwrap();
            assert_eq!(report.summary.scored, 0);
            for row in report.tasks {
                assert_eq!(row.status, crate::RunStatus::NotRun);
                assert!(!row.assurance.client_started);
                assert!(row.env_root.is_none());
                assert!(row.home.is_empty() && row.grok_home.is_empty());
                assert_eq!(
                    row.assurance.environment.unwrap().refusal,
                    Some(Refusal::ProviderUnavailable)
                );
            }
        }
    }
    // Fake driver + external client/backend also needs the same gate. Supplying
    // a fake manifest with an arbitrary backend must not bypass it.
    for backend in [false, true] {
        let cfg = crate::RunConfig {
            builtin_fake: backend,
            grok_bin: (!backend).then_some(sentinel.clone()),
            gcu_bin: Some(sentinel.clone()),
            attest_disposable_desktop: true,
            fake_scope: backend.then_some(gcu_host_protocol::scope::Scope {
                engine_instance: "fake".into(),
                desktop_session: "fake".into(),
                control_lease: "fake".into(),
                targets: vec![],
            }),
            ..crate::RunConfig::default()
        };
        assert!(refuse_unsupported_launch(&task, &cfg).is_some());
    }
    let cfg = crate::RunConfig {
        builtin_fake: true,
        builtin_gcu: true,
        ..crate::RunConfig::default()
    };
    assert!(refuse_unsupported_launch(&task, &cfg).is_none());
    let cfg = crate::RunConfig {
        real_driver: true,
        builtin_fake: true,
        ..crate::RunConfig::default()
    };
    let id = crate::ab::ReportId {
        platform: crate::host_platform().into(),
        model: "fake".into(),
        date: "1970-01-01".into(),
        effort: "same".into(),
        machine: "unspecified".into(),
    };
    let ab = crate::block_on(crate::ab::run_ab(&[task], &cfg, &id)).unwrap();
    assert_eq!(ab.provenance, crate::ab::Provenance::NotRun);
    assert_eq!(ab.trials.len(), 6);
    for trial in &ab.trials {
        assert_eq!(trial.status, crate::RunStatus::NotRun);
        assert_eq!(
            trial.environment_contract.as_ref().unwrap().refusal,
            Some(Refusal::ProviderUnavailable)
        );
    }
    let mut forged = ab;
    forged.trials[0].status = crate::RunStatus::Pass;
    let parsed = crate::ab::parse_report(&serde_json::to_vec(&forged).unwrap()).unwrap();
    assert_eq!(parsed.trials[0].status, crate::RunStatus::NotRun);
    std::fs::remove_dir_all(root).unwrap();
}

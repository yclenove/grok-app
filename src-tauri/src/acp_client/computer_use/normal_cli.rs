//! Normal CLI + production ACP/MCP wiring; Host scope is explicitly synthetic.
use super::*;
use grok_computer_use_core::{
    adapter::SurfaceKind, broker::AccessApprovalReady, fake::FakeAdapter,
};
use std::path::Path;

mod cold_chat;
mod events;
const APP_SESSION: &str = "normal-cli-synthetic-consent";

fn note(event: &str, details: Value) {
    println!(
        "{}",
        json!({"event":event,"details":details,"decisionClass":"synthetic-test-data-not-human-or-production-grant"})
    );
}

fn required_path(name: &str) -> Result<PathBuf, String> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .ok_or_else(|| format!("{name} required"))
}

pub(crate) fn run() -> i32 {
    match tauri::async_runtime::block_on(run_inner()) {
        Ok(()) => {
            note("result", json!({"pass":true}));
            0
        }
        Err(error) => {
            note("result", json!({"pass":false,"error":error}));
            1
        }
    }
}

async fn run_inner() -> Result<(), String> {
    let repo = required_path("CU_PROBE_REPO")?
        .canonicalize()
        .map_err(|_| "repo missing")?;
    let evidence = required_path("CU_PROBE_EVIDENCE")?;
    let cli_home = required_path("CU_PROBE_CLI_HOME")?
        .canonicalize()
        .map_err(|_| "configured isolated CLI home missing")?;
    let cli = required_path("CU_PROBE_CLI")?;
    let probe_root = repo
        .join("tools/computer-use-probe/.run")
        .canonicalize()
        .map_err(|_| "probe root missing")?;
    let evidence_parent = evidence
        .parent()
        .ok_or("evidence parent missing")?
        .canonicalize()
        .map_err(|_| "evidence parent missing")?;
    if evidence_parent != probe_root || !cli_home.starts_with(&probe_root) {
        return Err("probe evidence and configured CLI home must be under owned .run root".into());
    }
    std::fs::create_dir(&evidence)
        .map_err(|_| "fresh evidence directory required; prior results are preserved")?;
    let app_home = evidence.join("app-home");
    std::fs::create_dir(&app_home).map_err(|_| "fresh App home failed")?;
    std::env::set_var("GROK_APP_HOME", &app_home);
    let settings = crate::store::AppSettings {
        subagents_enabled: false,
        plan_enabled: false,
        disable_web_search: true,
        official_aux_inject: false,
        ..crate::store::AppSettings::default()
    };
    crate::store::save_settings(&settings).map_err(|_| "isolated settings failed")?;
    note(
        "configuration",
        json!({"permissionMode":"default","model":"grok-4.7","effort":"low","featureEnabled":false,"leader":false,"subagents":false,"cliVersion":crate::cli_probe::read_version_of(&cli)}),
    );
    cold_chat::prepare_runtime(&repo)?;
    let broker = crate::computer_use::ensure_request_endpoint()?;
    let adapter = Arc::new(FakeAdapter::new());
    broker
        .register_surface_adapter(SurfaceKind::Desktop, adapter.clone())
        .map_err(|_| "synthetic executor registration failed")?;
    let cwd = evidence.join("workspace");
    std::fs::create_dir(&cwd).map_err(|_| "fixture workspace failed")?;
    let result = run_cli(cli, cli_home, &cwd, &repo, &evidence, &broker, &adapter).await;
    crate::computer_use::shutdown::fence_all();
    crate::computer_use::shutdown::join_request_probe_workers().await?;
    let local_cleanup = tauri::async_runtime::spawn_blocking(move || {
        for ticket in broker.take_cleanup_tickets() {
            broker
                .finish_stop_cleanup(&ticket)
                .map_err(|_| "original synthetic run cleanup failed")?;
        }
        crate::computer_use::shutdown_product_checked()
    })
    .await
    .map_err(|_| "local cleanup worker failed")?;
    local_cleanup?;
    note("original-host-workers-joined", json!({}));
    result
}

async fn run_cli(
    cli: PathBuf,
    cli_home: PathBuf,
    cwd: &Path,
    repo: &Path,
    evidence: &Path,
    broker: &Arc<crate::computer_use::ComputerUseBroker>,
    adapter: &Arc<FakeAdapter>,
) -> Result<(), String> {
    let (client, receiver) = AcpClient::spawn_with_home(cli, cwd.to_path_buf(), "shared", SpawnOptions {
        grok_home_override: Some(cli_home), model_id: Some("grok-4.7".into()), effort: Some("low".into()),
        permission_policy: Some("ask".into()), no_ask_user: Some(true),
        extra_rules: Some("This is an isolated synthetic Computer Use transport test. Use only the exact injected synthetic MCP tools requested. Never use shell, files, network, other tools or native computer control. When access is pending, stop the turn; do not poll. After the Host commits, use fresh computer_observe with screenshot:false twice and stop. No actions or writes are permitted.".into()),
        ..SpawnOptions::default()
    }).await.map_err(|_| "normal CLI spawn failed")?;
    note("cli-spawned", json!({"pid":client.child_pid()}));
    let result = exercise(&client, receiver, cwd, repo, evidence, broker, adapter).await;
    let closed = events::close(&client).await;
    closed?;
    result
}

async fn exercise(
    client: &Arc<AcpClient>,
    receiver: mpsc::UnboundedReceiver<(Option<String>, AcpEvent)>,
    cwd: &Path,
    repo: &Path,
    evidence: &Path,
    broker: &Arc<crate::computer_use::ComputerUseBroker>,
    adapter: &Arc<FakeAdapter>,
) -> Result<(), String> {
    client
        .request_timeout("initialize", wire_initialize_params(), 45)
        .await
        .map_err(|_| "actual initialize failed")?;
    client
        .request_timeout("authenticate", json!({"methodId":"cached_token"}), 45)
        .await
        .map_err(|_| "configured isolated CLI cached authentication failed")?;
    let (agent, _) = client
        .open_app_session_at(APP_SESSION, None, false, &cwd.to_string_lossy())
        .await
        .map_err(|_| "actual App session/new failed")?;
    let transport = client
        .computer_use_connections
        .lock()
        .get(APP_SESSION)
        .and_then(|slot| slot.transport.clone())
        .ok_or("actual request transport unavailable")?;
    if broker.feature_enabled() || adapter.observe_calls() != 0 || !adapter.executions().is_empty()
    {
        return Err("preconnection started execution".into());
    }
    let node = crate::computer_use::runtime::resolve(crate::computer_use::runtime::JS_RUNTIME)?;
    let other_log = evidence.join("ordinary-effects.jsonl");
    if !cold_chat::enabled() {
        client.update_mcp_servers(&agent, json!([transport.entry.clone(), {
            "name":"consent-fixture", "command":node,"args":[repo.join("tools/computer-use-probe/consent-fixture.mjs"),other_log],"env":[]
        }])).await.map_err(|_| "actual MCP catalog update failed")?;
    }
    let counts = Arc::new(ParkingMutex::new(events::Counts::default()));
    let pump = tokio::spawn(events::pump(
        client.clone(),
        receiver,
        agent.clone(),
        counts.clone(),
    ));
    let result = if cold_chat::enabled() {
        cold_chat::exercise(client, &agent, broker, adapter, &transport, &counts).await
    } else {
        turns(
            client, &agent, broker, adapter, &transport, &counts, &other_log,
        )
        .await
    };
    // Stop the original event task and observe its JoinError if interrupted.
    pump.abort();
    match pump.await {
        Ok(Err(error)) => return Err(error),
        Err(error) if !error.is_cancelled() => return Err("event pump panicked".into()),
        _ => {}
    }
    result
}

async fn turns(
    client: &AcpClient,
    agent: &str,
    broker: &Arc<crate::computer_use::ComputerUseBroker>,
    adapter: &FakeAdapter,
    transport: &RequestTransport,
    counts: &ParkingMutex<events::Counts>,
    other_log: &Path,
) -> Result<(), String> {
    let prompt = "Synthetic transport negative test only. In order: call consent-fixture__consent_fixed_data with sequence 1, then sequence 2 (both require ordinary once-only confirmations). Next attempt grok-computer-use__computer_observe with screenshot:false exactly once while no scope is granted; it must be rejected. On rejection, end this turn. No other tools or actions.";
    tokio::time::timeout(
        Duration::from_secs(180),
        client.prompt_for_turn(agent, prompt, "probe-user-turn"),
    )
    .await
    .map_err(|_| "first normal CLI turn timed out")?
    .map_err(|_| "first normal CLI turn failed")?;
    {
        let tally = counts.lock();
        if tally.ordinary != 2
            || tally.observe_rejected != 1
            || adapter.observe_calls() != 0
            || broker.executable_grant_count(APP_SESSION) != 0
        {
            return Err("normal CLI negative control failed".into());
        }
    }
    // A normal CLI reject-once terminates that prompt. The positive scenario
    // is a separate explicit user turn, never a retry of denied execution.
    tokio::time::timeout(Duration::from_secs(180), client.prompt_for_turn(agent,
        "Synthetic transport positive test. Call grok-computer-use__computer_request_access exactly once with action:request, purpose:'synthetic normal CLI observation test', scopeIntent:'owned fake fixture only', surfaces:['desktop'], capabilities:['observe']. On pending, end your turn immediately. Do not poll. A later Host committed decision means call grok-computer-use__computer_observe with screenshot:false exactly twice, then finish. No other tools or actions.",
        "probe-access-turn"))
        .await.map_err(|_| "normal CLI access turn timed out")?
        .map_err(|_| "normal CLI access turn failed")?;
    let pending = broker
        .submit_access(&transport.token, json!({"action":"status"}))
        .map_err(|_| "pending request status unavailable")?;
    if pending["state"] != "pending"
        || adapter.observe_calls() != 0
        || broker.executable_grant_count(APP_SESSION) != 0
    {
        note("pending-state", pending);
        return Err("request did not remain pending without execution".into());
    }
    let request = pending["requestId"].as_str().ok_or("request id missing")?;
    let revision = pending["revision"]
        .as_u64()
        .ok_or("request revision missing")?;
    broker
        .bind_synthetic_scope(request, &adapter.fixture_id())
        .map_err(|_| "synthetic binding failed")?;
    let AccessApprovalReady::Prepare(mut approval) = broker
        .reserve_access_approval(&transport.token, request, revision)
        .map_err(|_| "synthetic reservation failed")?
    else {
        return Err("unexpected prior synthetic approval".into());
    };
    approval
        .publish_feature()
        .map_err(|_| "synthetic feature publish failed")?;
    approval
        .prepare_execution()
        .map_err(|_| "synthetic execution preparation failed")?;
    approval.commit().map_err(|_| "synthetic commit failed")?;
    note(
        "synthetic-host-commit",
        json!({"grants":broker.executable_grant_count(APP_SESSION),"continuations":broker.continuation_count(APP_SESSION),"observations":adapter.observe_calls()}),
    );
    let delivery = client
        .computer_use_decision(APP_SESSION, agent)
        .ok_or("typed continuation missing")?;
    let original_claim = broker
        .claim_continuation(APP_SESSION)
        .ok_or("original claim missing")?;
    let result = tokio::time::timeout(
        Duration::from_secs(180),
        client.prompt_computer_use(&delivery, "probe-host-turn", &|| Ok(())),
    )
    .await
    .map_err(|_| "continuation turn timed out")?;
    if !matches!(result, DeliveryAttempt::Completed) {
        return Err("typed continuation did not complete".into());
    }
    let ledger_empty = client.computer_use_decision(APP_SESSION, agent).is_none();
    let consumed = broker.claim_continuation(APP_SESSION).is_none()
        && broker.validate_continuation(&original_claim).is_err();
    // Exercise the actual admission path with the same old delivery. This must
    // emit no second session/prompt and must leave the two observations intact.
    let duplicate_not_started = matches!(
        client
            .prompt_computer_use(&delivery, "probe-duplicate-host-turn", &|| Ok(()))
            .await,
        DeliveryAttempt::NotStarted
    );
    let fixed = std::fs::read_to_string(other_log).map_err(|_| "ordinary effects log missing")?;
    let effects = fixed
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "ordinary effects malformed")?;
    let counts = counts.lock();
    let pass = counts.request == 1
        && counts.observe_rejected == 1
        && counts.observe_allowed == 2
        && counts.ordinary == 2
        && counts.nonfixture_cancelled == 0
        && adapter.observe_calls() == 2
        && adapter.executions().is_empty()
        && ledger_empty
        && delivery.was_admitted()
        && consumed
        && duplicate_not_started
        // This counter includes the consumed, bounded receipt; it is not the
        // number of pending/claimable continuations.
        && broker.continuation_count(APP_SESSION) == 1
        && effects.len() == 2
        && effects[0]["sequence"] == 1
        && effects[1]["sequence"] == 2;
    note(
        "oracle",
        json!({"pass":pass,"requestPermissions":counts.request,"observeRejected":counts.observe_rejected,"observeAllowed":counts.observe_allowed,"ordinaryConfirmations":counts.ordinary,"nonfixtureCancelled":counts.nonfixture_cancelled,"observations":adapter.observe_calls(),"actions":adapter.executions().len(),"continuationRecords":broker.continuation_count(APP_SESSION),"consumed":consumed,"duplicateNotStarted":duplicate_not_started,"ledgerEmpty":ledger_empty,"deliveryAdmitted":delivery.was_admitted(),"ordinaryEffects":effects}),
    );
    if pass {
        Ok(())
    } else {
        Err("normal CLI oracle failed; preserve actual evidence".into())
    }
}

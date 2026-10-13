//! Real CLI catalog on first connect, before any explicit MCP catalog update.
use super::*;

pub(super) fn enabled() -> bool {
    std::env::var("CU_PROBE_COLD_CHAT").as_deref() == Ok("1")
}

pub(super) fn prepare_runtime(repo: &Path) -> Result<(), String> {
    match std::env::var("CU_PROBE_COLD_CHAT").as_deref() {
        Ok("1") => {
            let bundle = required_path("CU_PROBE_BUNDLE")?
                .canonicalize()
                .map_err(|_| "verified portable bundle missing")?;
            let packages = repo
                .join("dist-portable")
                .canonicalize()
                .map_err(|_| "task portable directory missing")?;
            if bundle.parent() != Some(packages.as_path())
                || !bundle.join("portable-files.json").is_file()
            {
                return Err("cold-chat probe requires a verified task portable bundle".into());
            }
            crate::computer_use::runtime::register_probe_installation(
                bundle.join("resources"),
                bundle,
            )?;
            if crate::computer_use::runtime::runtime_root().exists() {
                return Err("cold-chat probe must start without a private runtime".into());
            }
            note("cold-chat-before-connect", json!({"runtimePresent":false}));
            Ok(())
        }
        Err(std::env::VarError::NotPresent) => crate::computer_use::runtime::repair_from_install(
            &repo.join("src-tauri/resources"),
            repo,
        )
        .map(|_| ())
        .map_err(|_| "private installer runtime repair failed".into()),
        _ => Err("invalid cold-chat probe mode".into()),
    }
}

pub(super) async fn exercise(
    client: &AcpClient,
    agent: &str,
    broker: &Arc<crate::computer_use::ComputerUseBroker>,
    adapter: &FakeAdapter,
    transport: &RequestTransport,
    counts: &ParkingMutex<events::Counts>,
) -> Result<(), String> {
    let node = crate::computer_use::runtime::resolve(crate::computer_use::runtime::JS_RUNTIME)?;
    let scripts = crate::computer_use::runtime::resolve(crate::computer_use::runtime::MCP_SERVER)?;
    let private_root = crate::computer_use::runtime::runtime_root();
    if !node.starts_with(&private_root) || !scripts.starts_with(&private_root) {
        return Err("cold-chat catalog escaped its private runtime".into());
    }
    note(
        "cold-chat-request-catalog",
        json!({
            "requestEntryPresent":true,"privateNode":true,"privateMcp":true,
            "explicitCatalogUpdates":0,"featureEnabled":broker.feature_enabled()
        }),
    );
    tokio::time::timeout(Duration::from_secs(180), client.prompt_for_turn(agent,
        "请使用 Grok App 的 Computer Use 操作测试窗口，填写中文备注并保存。当前只验证申请步骤：先调用 grok-computer-use__computer_request_access，action=request，purpose=验证首次聊天工具，scopeIntent=owned fake fixture only，surfaces=[desktop]，capabilities=[observe]。pending 时结束本轮，等待 Host 决定。不要读取屏幕，不要执行动作，不要调用其它工具。",
        "probe-cold-chat-access-turn"))
        .await.map_err(|_| "cold first-chat turn timed out")?
        .map_err(|_| "cold first-chat turn failed")?;
    let pending = broker
        .submit_access(&transport.token, json!({"action":"status"}))
        .map_err(|_| "cold first-chat pending status unavailable")?;
    let counts = counts.lock();
    let pass = pending["state"] == "pending"
        && counts.request == 1
        && counts.ordinary == 0
        && counts.nonfixture_cancelled == 0
        && counts.observe_allowed == 0
        && counts.observe_rejected == 0
        && !broker.feature_enabled()
        && broker.executable_grant_count(APP_SESSION) == 0
        && adapter.observe_calls() == 0
        && adapter.executions().is_empty();
    note(
        "cold-chat-oracle",
        json!({
            "pass":pass,"requestPermissions":counts.request,"pending":pending["state"]=="pending",
            "explicitCatalogUpdates":0,"featureEnabled":broker.feature_enabled(),
            "grants":broker.executable_grant_count(APP_SESSION),"observations":adapter.observe_calls(),
            "actions":adapter.executions().len(),"nonfixtureCancelled":counts.nonfixture_cancelled
        }),
    );
    if pass {
        Ok(())
    } else {
        Err("cold first-chat catalog oracle failed; preserve evidence".into())
    }
}

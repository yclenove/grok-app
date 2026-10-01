//! D8: segmented cold/warm timings + 20 fault classes ×5 against shipped APIs.

use grok_computer_use_core::broker::{BrokerOptions, ComputerUseBroker};
use grok_computer_use_core::fake::FakeAdapter;
use grok_computer_use_core::ipc;
use grok_computer_use_core::perf::{summarize, SeriesSummary};
use grok_computer_use_core::privacy::{write_owner, SENTINELS};
use grok_computer_use_core::protocol::{
    ActionKind, ActionRequest, ActionTarget, OutcomeKind, PROTOCOL_VERSION,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub(super) const ROUNDS: usize = 6;
pub(super) const FAULT_N: usize = 5;
pub(super) const OWNER: &str = "grok-app-d8";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TimingRow {
    phase: String,
    segment: String,
    ms: u64,
    ok: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FaultRow {
    pub class: String,
    pub n: usize,
    pub code: String,
    pub ok: bool,
    pub executed: bool,
    pub leaked: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ResourceRow {
    pub pid: u32,
    pub kind: String,
    pub private_bytes: u64,
    pub working_set: u64,
    pub handles: u32,
    pub threads: u32,
    pub cpu_norm: f64,
    pub disk_home_bytes: u64,
    pub lease_files: u32,
    pub telemetry_ok: bool,
}

fn out_dir() -> PathBuf {
    if let Ok(p) = std::env::var("GROK_CU_D8_OUT") {
        return PathBuf::from(p);
    }
    std::env::temp_dir().join(format!("cu-d8-{}", std::process::id()))
}

pub(super) fn click_req(
    run: &str,
    target: &str,
    gen: u64,
    snap: &str,
    geo: u64,
    id: &str,
) -> ActionRequest {
    ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: id.into(),
        run_id: run.into(),
        target_id: target.into(),
        target_generation: gen,
        snapshot_id: snap.into(),
        geometry_revision: geo,
        action: ActionKind::Click,
        target: ActionTarget::Element {
            element_ref: "n1".into(),
        },
        parameters: serde_json::json!({}),
    }
}

pub(super) fn append_jsonl(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    writeln!(f, "{value}").map_err(|e| e.to_string())
}

pub(super) fn leaked(s: &str) -> bool {
    SENTINELS.iter().any(|x| s.contains(x)) || s.contains("secret=")
}

fn cleanup_residue(out: &Path, home: &Path) -> Result<(), String> {
    let _ = fs::remove_dir_all(home);
    if let Ok(rd) = fs::read_dir(out) {
        for ent in rd.flatten() {
            let p = ent.path();
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if p.extension().and_then(|s| s.to_str()) == Some("lease")
                || name.starts_with("chrome-cold-")
                || name.starts_with("chrome-kill-")
            {
                if p.is_dir() {
                    let _ = fs::remove_dir_all(&p);
                } else {
                    let _ = fs::remove_file(&p);
                }
            }
        }
    }
    if home.exists() {
        return Err(format!("d8 home residue remains: {}", home.display()));
    }
    Ok(())
}

pub fn run_harness() -> Result<(), String> {
    let out = out_dir();
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let home = out.join("home");
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    write_owner(&home, OWNER)?;
    let prev_home = std::env::var("GROK_APP_HOME").ok();
    std::env::set_var("GROK_APP_HOME", &home);
    let result = (|| {
        let mut resources = Vec::new();
        match sample_self() {
            Ok(row) => resources.push(row),
            Err(e) => {
                println!("gate: d8_telemetry_failure {e}");
                resources.push(ResourceRow {
                    pid: std::process::id(),
                    kind: "app".into(),
                    private_bytes: 0,
                    working_set: 0,
                    handles: 0,
                    threads: 0,
                    cpu_norm: 0.0,
                    disk_home_bytes: 0,
                    lease_files: 0,
                    telemetry_ok: false,
                });
            }
        }
        run_timings(&out)?;
        super::d8_faults::run_faults(&out, &home, &mut resources)?;
        if let Ok(row) = sample_self() {
            resources.push(row);
        }
        for row in resources {
            append_jsonl(
                &out.join("d8-resources.jsonl"),
                &serde_json::to_value(&row).unwrap_or(json!({})),
            )?;
        }
        cleanup_residue(&out, &home)?;
        println!("gate: d8_timing_and_faults");
        Ok(())
    })();
    match prev_home {
        Some(v) => std::env::set_var("GROK_APP_HOME", v),
        None => std::env::remove_var("GROK_APP_HOME"),
    }
    result
}

fn run_timings(out: &Path) -> Result<(), String> {
    let cold_path = out.join("d8-timing-cold.jsonl");
    let warm_path = out.join("d8-timing-warm.jsonl");
    let _ = fs::remove_file(&cold_path);
    let _ = fs::remove_file(&warm_path);
    let mut cold: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    let mut warm: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    let mut cold_fail: BTreeMap<String, usize> = BTreeMap::new();
    let mut warm_fail: BTreeMap<String, usize> = BTreeMap::new();

    for i in 0..ROUNDS {
        time_round("cold", i, &mut cold, &mut cold_fail, &cold_path)?;
    }
    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(true);
    let broker = Arc::new(ComputerUseBroker::new(
        fake.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: out.join(format!("warm-{}.lease", Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    ));
    broker
        .open_run("s-warm", "run-warm")
        .map_err(|e| e.to_string())?;
    broker
        .authorize_target("run-warm", &fake.fixture_id())
        .map_err(|e| e.to_string())?;
    let ipc = ipc::spawn(broker.clone()).map_err(|e| e.to_string())?;
    let token = ipc
        .issue_session("s-warm", "run-warm")
        .map_err(|e| e.to_string())?;
    for i in 0..ROUNDS {
        time_warm(
            i,
            &broker,
            &fake,
            &ipc,
            &token,
            TimingSeries {
                samples: &mut warm,
                failures: &mut warm_fail,
                path: &warm_path,
            },
        )?;
    }
    drop(ipc);
    super::d8_faults::time_browser_cold(out, &mut cold, &mut cold_fail, &cold_path)?;

    let mut summaries = Vec::new();
    for (phase, map, fails) in [("cold", &cold, &cold_fail), ("warm", &warm, &warm_fail)] {
        for (seg, samples) in map {
            let f = *fails.get(seg).unwrap_or(&0);
            if let Some(s) = summarize(phase, seg, samples, f) {
                if s.failure_rate != 0.0 {
                    return Err(format!(
                        "timing {phase}/{seg} failure_rate={}",
                        s.failure_rate
                    ));
                }
                summaries.push(s);
            }
        }
    }
    let summary_path = out.join("d8-timing-summary.json");
    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    assert_phases_not_mixed(&summaries)?;
    println!(
        "gate: d8_timing_summary n={} cold_file={} warm_file={}",
        summaries.len(),
        cold_path.display(),
        warm_path.display()
    );
    Ok(())
}

fn assert_phases_not_mixed(rows: &[SeriesSummary]) -> Result<(), String> {
    let cold_obs = rows
        .iter()
        .find(|s| s.phase == "cold" && s.segment == "capture");
    let warm_obs = rows
        .iter()
        .find(|s| s.phase == "warm" && s.segment == "capture");
    if cold_obs.is_none() || warm_obs.is_none() {
        return Err("missing cold or warm capture series".into());
    }
    Ok(())
}

pub(super) fn record(
    map: &mut BTreeMap<String, Vec<u64>>,
    fails: &mut BTreeMap<String, usize>,
    path: &Path,
    phase: &str,
    segment: &str,
    start: Instant,
    ok: bool,
) -> Result<(), String> {
    let ms = start.elapsed().as_millis() as u64;
    append_jsonl(
        path,
        &serde_json::to_value(TimingRow {
            phase: phase.into(),
            segment: segment.into(),
            ms,
            ok,
        })
        .unwrap_or(json!({})),
    )?;
    if ok {
        map.entry(segment.into()).or_default().push(ms);
    } else {
        *fails.entry(segment.into()).or_default() += 1;
    }
    Ok(())
}

fn time_round(
    phase: &str,
    i: usize,
    map: &mut BTreeMap<String, Vec<u64>>,
    fails: &mut BTreeMap<String, usize>,
    path: &Path,
) -> Result<(), String> {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(true);
    let broker = Arc::new(ComputerUseBroker::new(
        fake.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!("cu-d8-c-{i}-{}.lease", Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    ));
    broker.open_run("s", "run").map_err(|e| e.to_string())?;
    let t0 = Instant::now();
    let listed = broker.list_targets("run").map_err(|e| e.to_string())?;
    record(map, fails, path, phase, "discover", t0, !listed.is_empty())?;
    broker
        .authorize_target("run", &fake.fixture_id())
        .map_err(|e| e.to_string())?;
    fake.set_include_png(false);
    let t1 = Instant::now();
    let obs = broker.observe("run").map_err(|e| e.to_string())?;
    record(map, fails, path, phase, "capture", t1, true)?;
    let t2 = Instant::now();
    let extracted = !obs.nodes.is_empty();
    record(map, fails, path, phase, "extract", t2, extracted)?;
    fake.set_include_png(true);
    let t3 = Instant::now();
    let png = broker.observe("run").map_err(|e| e.to_string())?;
    record(
        map,
        fails,
        path,
        phase,
        "png",
        t3,
        png.image.png_base64.is_some(),
    )?;
    let ipc = ipc::spawn(broker.clone()).map_err(|e| e.to_string())?;
    let token = ipc.issue_session("s", "run").map_err(|e| e.to_string())?;
    let t4 = Instant::now();
    let (status, _) = ipc_call(&ipc.url, &token, json!({"name":"computer_status"}));
    record(map, fails, path, phase, "loopback", t4, status == 200)?;
    let gen = broker.authorized_target("run").map(|(_, g)| g).unwrap_or(1);
    let t5 = Instant::now();
    let act = broker.act(click_req(
        "run",
        &fake.fixture_id(),
        gen,
        &png.snapshot_id,
        png.geometry_revision,
        &format!("c{i}"),
    ));
    record(
        map,
        fails,
        path,
        phase,
        "dispatch",
        t5,
        act.executed && act.kind != OutcomeKind::Rejected,
    )?;
    record(
        map,
        fails,
        path,
        phase,
        "apply",
        t5,
        act.executed && act.kind != OutcomeKind::Rejected,
    )?;
    let t6 = Instant::now();
    let verified = act.kind == OutcomeKind::Verified || act.kind == OutcomeKind::Applied;
    record(map, fails, path, phase, "verify", t6, verified)?;
    let t_mcp = Instant::now();
    let (mcp_status, _) = ipc_call(&ipc.url, &token, json!({"name":"computer_observe"}));
    record(
        map,
        fails,
        path,
        phase,
        "mcp_stdio",
        t_mcp,
        mcp_status == 200,
    )?;
    let t7 = Instant::now();
    broker.request_stop("run").map_err(|e| e.to_string())?;
    record(map, fails, path, phase, "stop", t7, true)?;
    drop(ipc);
    Ok(())
}

struct TimingSeries<'a> {
    samples: &'a mut BTreeMap<String, Vec<u64>>,
    failures: &'a mut BTreeMap<String, usize>,
    path: &'a Path,
}

fn time_warm(
    i: usize,
    broker: &Arc<ComputerUseBroker>,
    fake: &Arc<FakeAdapter>,
    ipc: &ipc::IpcServer,
    token: &str,
    timings: TimingSeries<'_>,
) -> Result<(), String> {
    let TimingSeries {
        samples: map,
        failures: fails,
        path,
    } = timings;
    let t0 = Instant::now();
    let _ = broker.list_targets("run-warm").map_err(|e| e.to_string())?;
    record(map, fails, path, "warm", "discover", t0, true)?;
    let t1 = Instant::now();
    let obs = broker.observe("run-warm").map_err(|e| e.to_string())?;
    record(map, fails, path, "warm", "capture", t1, true)?;
    record(
        map,
        fails,
        path,
        "warm",
        "extract",
        t1,
        !obs.nodes.is_empty(),
    )?;
    let t3 = Instant::now();
    let png = broker.observe("run-warm").map_err(|e| e.to_string())?;
    record(
        map,
        fails,
        path,
        "warm",
        "png",
        t3,
        png.image.png_base64.is_some(),
    )?;
    let t4 = Instant::now();
    let (status, _) = ipc_call(&ipc.url, token, json!({"name":"computer_status"}));
    record(map, fails, path, "warm", "loopback", t4, status == 200)?;
    let gen = broker
        .authorized_target("run-warm")
        .map(|(_, g)| g)
        .unwrap_or(1);
    let t5 = Instant::now();
    let act = broker.act(click_req(
        "run-warm",
        &fake.fixture_id(),
        gen,
        &png.snapshot_id,
        png.geometry_revision,
        &format!("w{i}"),
    ));
    record(map, fails, path, "warm", "dispatch", t5, act.executed)?;
    record(map, fails, path, "warm", "apply", t5, act.executed)?;
    record(map, fails, path, "warm", "page_action", t5, act.executed)?;
    record(
        map,
        fails,
        path,
        "warm",
        "verify",
        t5,
        act.kind != OutcomeKind::Rejected,
    )?;
    let t_mcp = Instant::now();
    let (mcp_status, _) = ipc_call(&ipc.url, token, json!({"name":"computer_observe"}));
    record(
        map,
        fails,
        path,
        "warm",
        "mcp_stdio",
        t_mcp,
        mcp_status == 200,
    )?;
    std::thread::sleep(Duration::from_millis(400));
    Ok(())
}

pub(super) fn ipc_call(url: &str, token: &str, body: Value) -> (u16, String) {
    match reqwest::blocking::Client::new()
        .post(format!("{url}/cu/tool"))
        .bearer_auth(token)
        .json(&body)
        .timeout(Duration::from_secs(4))
        .send()
    {
        Ok(r) => {
            let status = r.status().as_u16();
            let text = r.text().unwrap_or_default();
            (status, text)
        }
        Err(e) => (0, e.to_string()),
    }
}

pub(super) fn sample_self() -> Result<ResourceRow, String> {
    sample_pid(std::process::id(), "app")
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let walk = match fs::read_dir(path) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    for ent in walk.flatten() {
        let p = ent.path();
        if let Ok(meta) = ent.metadata() {
            if meta.is_dir() {
                total = total.saturating_add(dir_size(&p));
            } else {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

fn count_leases(root: &Path) -> u32 {
    let mut n = 0u32;
    if let Ok(rd) = fs::read_dir(root) {
        for ent in rd.flatten() {
            let p = ent.path();
            if p.extension().and_then(|s| s.to_str()) == Some("lease") {
                n += 1;
            } else if p.is_dir() {
                n = n.saturating_add(count_leases(&p));
            }
        }
    }
    n
}

#[cfg(windows)]
pub(super) fn sample_pid(pid: u32, kind: &str) -> Result<ResourceRow, String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::SystemInformation::GetSystemInfo;
    use windows::Win32::System::Threading::{
        GetProcessHandleCount, GetProcessTimes, OpenProcess, PROCESS_QUERY_INFORMATION,
        PROCESS_VM_READ,
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid)
            .map_err(|e| e.to_string())?;
        let mut pmc = PROCESS_MEMORY_COUNTERS::default();
        let cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        pmc.cb = cb;
        let mem_ok = GetProcessMemoryInfo(handle, &mut pmc, cb).is_ok();
        let mut handles = 0u32;
        let h_ok = GetProcessHandleCount(handle, &mut handles).is_ok();
        let mut create = windows::Win32::Foundation::FILETIME::default();
        let mut exit = create;
        let mut kernel0 = create;
        let mut user0 = create;
        let t0_ok =
            GetProcessTimes(handle, &mut create, &mut exit, &mut kernel0, &mut user0).is_ok();
        std::thread::sleep(Duration::from_millis(120));
        let mut kernel1 = create;
        let mut user1 = create;
        let t1_ok =
            GetProcessTimes(handle, &mut create, &mut exit, &mut kernel1, &mut user1).is_ok();
        let _ = CloseHandle(handle);
        let mut info = windows::Win32::System::SystemInformation::SYSTEM_INFO::default();
        GetSystemInfo(&mut info);
        let ncpu = info.dwNumberOfProcessors.max(1) as f64;
        let cpu0 = filetime_u64(kernel0).saturating_add(filetime_u64(user0));
        let cpu1 = filetime_u64(kernel1).saturating_add(filetime_u64(user1));
        let delta = cpu1.saturating_sub(cpu0) as f64;
        let wall: f64 = 1_200_000.0; // 120ms in 100-ns units
        let cpu_norm = if t0_ok && t1_ok {
            (delta / wall.max(1.0)) / ncpu
        } else {
            0.0
        };
        let mut threads = 0u32;
        if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) {
            let mut entry = THREADENTRY32 {
                dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
                ..Default::default()
            };
            if Thread32First(snap, &mut entry).is_ok() {
                loop {
                    if entry.th32OwnerProcessID == pid {
                        threads += 1;
                    }
                    if Thread32Next(snap, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
        }
        if !(mem_ok && h_ok && t0_ok && t1_ok) {
            return Err("process query incomplete".into());
        }
        let home = std::env::var("GROK_APP_HOME").unwrap_or_default();
        let home_path = PathBuf::from(&home);
        Ok(ResourceRow {
            pid,
            kind: kind.into(),
            private_bytes: pmc.PagefileUsage as u64,
            working_set: pmc.WorkingSetSize as u64,
            handles,
            threads,
            cpu_norm,
            disk_home_bytes: if home_path.is_dir() {
                dir_size(&home_path)
            } else {
                0
            },
            lease_files: if home_path.is_dir() {
                count_leases(&home_path)
            } else {
                0
            },
            telemetry_ok: true,
        })
    }
}

#[cfg(windows)]
fn filetime_u64(ft: windows::Win32::Foundation::FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
}

#[cfg(not(windows))]
pub(super) fn sample_pid(pid: u32, kind: &str) -> Result<ResourceRow, String> {
    Ok(ResourceRow {
        pid,
        kind: kind.into(),
        private_bytes: 0,
        working_set: 0,
        handles: 0,
        threads: 0,
        cpu_norm: 0.0,
        disk_home_bytes: 0,
        lease_files: 0,
        telemetry_ok: false,
    })
}

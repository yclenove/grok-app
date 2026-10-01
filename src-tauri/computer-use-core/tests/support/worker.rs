//! Deterministic pipe peer for process lifecycle tests. Never installed with the App.
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::process::Stdio;

fn main() {
    let mut args = std::env::args();
    let _exe = args.next();
    if args.next().as_deref() == Some("__hang-child") {
        let path = args.next().expect("pid path");
        std::fs::write(&path, std::process::id().to_string()).unwrap();
        loop {
            std::thread::park();
        }
    }
    worker_main();
}

fn worker_main() {
    let generation = std::env::args().nth(3).expect("generation");
    let input = std::io::stdin();
    let mut output = std::io::stdout().lock();
    let mut observe_history = Vec::<Value>::new();
    let mut observe_behavior = json!({});
    let mut fixture_root = None;
    for line in input.lock().lines() {
        let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let mut reply_generation = generation.clone();
        let mut reply_request_id = request["request_id"].clone();
        let result = match request["operation"].as_str().unwrap() {
            "initialize" => {
                // The production worker deliberately sanitizes its environment.
                // Do not assume parent and child share TMPDIR; keep proof files
                // beside this fixture's unique Host-owned manifest instead.
                let manifest = request["arguments"]["configured_driver"]["authorization"]
                    ["compatibility_capability_manifest_path"]
                    .as_str()
                    .expect("fixture manifest");
                fixture_root = Some(
                    std::path::Path::new(manifest)
                        .parent()
                        .expect("fixture root")
                        .to_path_buf(),
                );
                assert_eq!(
                    request["arguments"]["configured_driver"]["authorization"]["allowed_modes"],
                    json!(["bounded"])
                );
                assert!(!request["run_id"].as_str().unwrap_or("").is_empty());
                assert!(request["deadline_ms"].as_u64().is_some());
                assert_eq!(request["cancellation"]["armed"], true);
                json!({"ready": true, "pid": std::process::id(),
                "host_bundle_id": request["arguments"]["host_bundle_id"],
                "protocol_version": request["protocol_version"],
                "build_identity": request["arguments"]["build_identity"],
                "generation": request["generation"],
                "max_response_size": request["arguments"]["max_response_size"],
                "capabilities": {
                    "echo": true, "echo_meta": true, "hang": true,
                    "wrong_generation": true, "oversize": true,
                    "environment": true, "stale_request_id": true,
                    "spawn_hanging_child": true, "flood_stderr": true,
                    "prepare_dispatch": true,
                    "list_windows": true, "observe": true, "act": true, "abort": true,
                    "observe_history": true, "configure_observe": true
                },
                "metadata": {
                    "contract_version": "0.7.0", "tools_list_schema_version": "1",
                    "capability_version": "1", "mcp_protocol_version": "2025-06-18"
                }})
            }
            "bind_session" => json!({"session_handle": "bound-fixture"}),
            "call" => match request["name"].as_str().unwrap() {
                "echo" => request["arguments"].clone(),
                "prepare_dispatch" => json!({
                    "run_id": request["run_id"],
                    "generation": request["generation"],
                }),
                "list_windows" => json!({
                    "executed": true,
                    "pid": std::process::id(),
                    "ids": request["arguments"]["ids"],
                }),
                "observe" => {
                    let target = request["arguments"]["targetId"].as_str().unwrap_or("");
                    observe_history.push(json!({
                        "targetId": target, "runId": request["run_id"],
                        "pid": std::process::id(),
                    }));
                    if let Some(path) = observe_behavior["started"].as_str() {
                        std::fs::write(path, "started").unwrap();
                    }
                    if observe_behavior["hang"] == true {
                        loop {
                            std::thread::park();
                        }
                    }
                    json!({
                        "executed": observe_behavior["executed"].as_bool().unwrap_or(true),
                        "pid": std::process::id(),
                        "targetId": observe_behavior["targetId"].as_str().unwrap_or(target),
                    })
                }
                "observe_history" => json!(observe_history),
                "configure_observe" => {
                    observe_behavior = request["arguments"].clone();
                    json!({"configured": true})
                }
                "act" => {
                    let target = request["arguments"]["targetId"].as_str().unwrap_or("");
                    let action_id = request["arguments"]["actionId"].as_str().unwrap_or("");
                    if target.is_empty() {
                        panic!("act requires targetId");
                    }
                    let marker = fixture_root
                        .as_ref()
                        .expect("initialized fixture")
                        .join(format!("grok-cu-worker-act-{action_id}.txt"));
                    std::fs::write(&marker, std::process::id().to_string()).unwrap();
                    json!({
                        "executed": true,
                        "pid": std::process::id(),
                        "targetId": target,
                        "actionId": action_id,
                    })
                }
                "abort" => json!({
                    "executed": true,
                    "pid": std::process::id(),
                    "run_id": request["run_id"],
                }),
                "echo_meta" => json!({
                    "run_id": request["run_id"],
                    "generation": request["generation"],
                    "request_id": request["request_id"],
                    "deadline_ms": request["deadline_ms"],
                    "cancellation_armed": request["cancellation"]["armed"],
                    "protocol_version": request["protocol_version"],
                }),
                "hang" => {
                    if let Some(path) = request["arguments"]["started"].as_str() {
                        std::fs::write(path, "started").unwrap();
                    }
                    loop {
                        std::thread::park();
                    }
                }
                "wrong_generation" => {
                    reply_generation = "old-generation".into();
                    json!({})
                }
                "oversize" => {
                    output.write_all(&vec![b'x'; 16 * 1024 * 1024 + 1]).unwrap();
                    output.flush().unwrap();
                    continue;
                }
                "stale_request_id" => {
                    reply_request_id = json!(u64::MAX);
                    json!({})
                }
                "environment" => {
                    json!({"secretInherited": std::env::var_os("GROK_CU_TEST_SECRET").is_some()})
                }
                "spawn_hanging_child" => {
                    let path = request["arguments"]["pid_path"].as_str().expect("pid_path");
                    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
                    command
                        .arg("__hang-child")
                        .arg(path)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null());
                    #[cfg(windows)]
                    {
                        use std::os::windows::process::CommandExt;
                        command.creation_flags(0x08000000);
                    }
                    let child = command.spawn().unwrap();
                    let pid = child.id();
                    // Host Job Object / process group owns the tree; waiting here would hang.
                    std::mem::forget(child);
                    json!({"child_pid": pid})
                }
                "flood_stderr" => {
                    let marker = request["arguments"]["marker"].as_str().unwrap_or("MARKER");
                    let secret = request["arguments"]["secret"].as_str().unwrap_or("secret");
                    let chunk = vec![b'y'; 8192];
                    let mut err = std::io::stderr().lock();
                    for _ in 0..128 {
                        let _ = err.write_all(&chunk);
                    }
                    let _ = writeln!(err, "{marker} token={secret}");
                    let _ = err.flush();
                    json!({"flooded": true})
                }
                _ => panic!("unsupported fixture call"),
            },
            _ => panic!("unsupported fixture operation"),
        };
        serde_json::to_writer(
            &mut output,
            &json!({"protocol_version": 1,
            "request_id": reply_request_id, "generation": reply_generation,
            "ok": true, "completion": "completed", "result": result}),
        )
        .unwrap();
        output.write_all(b"\n").unwrap();
        output.flush().unwrap();
    }
}

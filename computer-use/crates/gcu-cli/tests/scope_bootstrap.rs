//! ADR 0017 S3: real child processes, fake desktop, no native OS identity/model.
#![allow(clippy::unwrap_used, clippy::expect_used)]
// Windows private-pipe runtime proof: not_run (ADR 0017 S3)
#[cfg(unix)]
use gcu_host_protocol::{
    codec::{read_frame, write_frame},
    scope::{Ack, Control, Scope, VERSION},
    transport::BoxStream,
};
#[cfg(unix)]
use serde_json::{json, Value};
#[cfg(unix)]
use std::net::SocketAddr;
use std::{process::Stdio, time::Duration};
use tokio::process::Command;
#[cfg(unix)]
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    process::Child,
};
const LIMIT: Duration = Duration::from_secs(8);

fn isolated_command(root: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gcu"));
    command.env_clear();
    #[cfg(windows)]
    for key in [
        "SystemRoot",
        "SystemDrive",
        "windir",
        "PATH",
        "PATHEXT",
        "ComSpec",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    // On Windows the user config directory comes from APPDATA, not HOME.
    #[cfg(windows)]
    command.env("APPDATA", root).env("LOCALAPPDATA", root);
    command
        .env("HOME", root)
        .env("USERPROFILE", root)
        .env("XDG_STATE_HOME", root)
        .env("XDG_CONFIG_HOME", root)
        .env("XDG_RUNTIME_DIR", root)
        .env("TMPDIR", root)
        .env("TEMP", root)
        .env("TMP", root)
        .env("GCU_ALLOW_FAKE", "1")
        .kill_on_drop(true);
    command
}

#[cfg(unix)]
fn empty_scope() -> Scope {
    Scope {
        engine_instance: "fake-engine".into(),
        desktop_session: "fake-desktop".into(),
        control_lease: "private".into(),
        targets: vec![],
    }
}
#[cfg(unix)]
fn install() -> Control {
    let scope = empty_scope();
    Control::Install {
        version: VERSION,
        run_id: "run".into(),
        generation: 1,
        scope_digest: scope.digest("run", 1).unwrap(),
        scope,
    }
}
#[cfg(unix)]
struct Engine {
    child: Child,
    control: Option<BoxStream>,
    stdout: BufReader<tokio::process::ChildStdout>,
    _root: tempfile::TempDir,
}
#[cfg(unix)]
impl Engine {
    async fn spawn(profile: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut command = isolated_command(root.path());
        command
            .args([
                "scoped-engine",
                "--driver",
                "fake",
                "--scene",
                "form",
                "--profile",
                profile,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let control = gcu_host_protocol::scope::launch::prepare(command.as_std_mut())
            .await
            .unwrap();
        let mut child = command.spawn().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            control: Some(control),
            stdout,
            _root: root,
        }
    }
    async fn ack(&mut self, frame: &Control) -> Ack {
        let control = self.control.as_mut().unwrap();
        write_frame(control, frame).await.unwrap();
        read_frame(control).await.unwrap().unwrap()
    }
    async fn address(&mut self) -> SocketAddr {
        let mut line = String::new();
        self.stdout.read_line(&mut line).await.unwrap();
        serde_json::from_str::<Value>(&line).unwrap()["data_address"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    }
    async fn wait(&mut self) {
        tokio::time::timeout(Duration::from_secs(1), self.child.wait())
            .await
            .expect("engine did not exit")
            .unwrap();
    }
}
#[cfg(unix)]
struct Mcp {
    io: BufReader<TcpStream>,
}
#[cfg(unix)]
impl Mcp {
    async fn connect(address: SocketAddr) -> Self {
        let mut m = Self {
            io: BufReader::new(TcpStream::connect(address).await.unwrap()),
        };
        let init=m.request(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"scope-fake","version":"1"}}})).await;
        assert!(init.get("result").is_some(), "{init}");
        m.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await;
        m
    }
    async fn send(&mut self, value: Value) {
        let mut bytes = serde_json::to_vec(&value).unwrap();
        bytes.push(b'\n');
        self.io.get_mut().write_all(&bytes).await.unwrap();
    }
    async fn request(&mut self, value: Value) -> Value {
        let id = value["id"].clone();
        self.send(value).await;
        loop {
            let mut line = String::new();
            assert_ne!(
                self.io.read_line(&mut line).await.unwrap(),
                0,
                "unexpected MCP EOF"
            );
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == id {
                return v;
            }
        }
    }
    async fn closed(&mut self) {
        // Pending protocol notices are allowed, but the socket must reach EOF.
        let mut bytes = Vec::new();
        let result = tokio::time::timeout(Duration::from_secs(1), self.io.read_to_end(&mut bytes))
            .await
            .expect("control failure waited for lease expiry instead of revoking");
        // A reset is as dead as an EOF: the engine tore the connection down.
        if let Err(error) = result {
            assert!(
                matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::UnexpectedEof
                ),
                "unexpected error closing the data plane: {error}"
            );
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn data_plane_before_install_has_no_listener_or_ready_address() {
    tokio::time::timeout(LIMIT, async {
        let mut engine = Engine::spawn("default").await;
        // No stdout address (and no TCP listener) before Install/ACK. A model
        // cannot supply authority on stdin: it is not the data channel.
        let mut byte = [0];
        assert!(
            tokio::time::timeout(Duration::from_millis(150), engine.stdout.read(&mut byte))
                .await
                .is_err()
        );
        #[cfg(target_os = "linux")]
        assert_no_tcp_listener(engine.child.id().unwrap());
        engine.control.take();
        engine.wait().await;
    })
    .await
    .unwrap();
}
#[cfg(unix)]
#[tokio::test]
async fn install_ack_data_and_control_looking_mcp_cannot_change_scope() {
    tokio::time::timeout(LIMIT,async {
        let mut engine=Engine::spawn("default").await;
        let ack=engine.ack(&install()).await;assert_eq!(ack.generation,1);
        assert_eq!(ack.scope_digest,empty_scope().digest("run",1).unwrap());
        let address=engine.address().await;
        let mut mcp=Mcp::connect(address).await;
        let before=mcp.request(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_apps","arguments":{}}})).await;
        for (index, frame) in [install(), Control::Rebind {run_id:"run".into(),generation:99,scope:empty_scope()}, Control::Renew {run_id:"run".into(),generation:1,seq:99}, Control::Revoke {run_id:"run".into()}].into_iter().enumerate() {
            let control_json=serde_json::to_value(frame).unwrap();
            let mut payload=control_json.clone();
            payload["jsonrpc"]=json!("2.0");payload["id"]=json!(10+index);payload["method"]=json!("scope.control");payload["params"]=control_json;
            let answer=mcp.request(payload).await;assert!(answer.get("error").is_some());
        }
        // This would be a replay if data-plane Renew had changed the sequence.
        write_frame(engine.control.as_mut().unwrap(),&Control::Renew {run_id:"run".into(),generation:1,seq:1}).await.unwrap();
        let after=mcp.request(json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"list_apps","arguments":{}}})).await;
        assert_eq!(before["result"],after["result"]);assert!(after.get("result").is_some());
        let text=after["result"].to_string();
        assert!(!text.contains("fake.form"),"empty scope leaked discovery: {text}");
        // A valid later control message still works: data JSON did not revoke.
        let next=engine.ack(&Control::Rebind {run_id:"run".into(),generation:2,scope:empty_scope()}).await;
        assert_eq!(next.generation,2);
        mcp.closed().await;
        let mut current=Mcp::connect(address).await;
        write_frame(engine.control.as_mut().unwrap(),&Control::Revoke {run_id:"run".into()}).await.unwrap();
        current.closed().await;engine.wait().await;
        assert!(TcpStream::connect(address).await.is_err());
    }).await.unwrap();
}
#[cfg(unix)]
#[tokio::test]
async fn tools_list_every_profile_contains_only_data_tools() {
    tokio::time::timeout(Duration::from_secs(20),async {
        let minimal=["list_apps","list_windows","get_app_state","click","perform_secondary_action","set_value","type_text","press_key","scroll"];
        let default=["list_apps","list_windows","launch_app","get_app_state","click","perform_secondary_action","set_value","select_text","type_text","press_key","scroll","drag","wait_for","computer_batch","capabilities"];
        for profile in ["minimal","default","full"] {
            let mut engine=Engine::spawn(profile).await;engine.ack(&install()).await;
            let mut mcp=Mcp::connect(engine.address().await).await;
            let listed=mcp.request(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).await;
            let mut actual:Vec<_>=listed["result"]["tools"].as_array().unwrap().iter().map(|t|t["name"].as_str().unwrap().to_owned()).collect();
            let mut expected:Vec<_>=if profile=="minimal" {minimal.as_slice()} else {default.as_slice()}.iter().map(|s|s.to_string()).collect();
            if profile=="full" && cfg!(all(feature="script",unix)) {expected.push("computer_script".into());}
            actual.sort();expected.sort();assert_eq!(actual,expected,"profile {profile}");
            // Calling a plausible control tool is also rejected by dispatch.
            let denied=mcp.request(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"set_scope","arguments":{"scope":empty_scope()}}})).await;
            assert!(denied.get("error").is_some() || denied["result"]["isError"]==true);
            engine.control.take();mcp.closed().await;engine.wait().await;
        }
    }).await.unwrap();
}
#[cfg(unix)]
#[tokio::test]
async fn malformed_replayed_or_mismatched_control_revokes_and_kills_data() {
    tokio::time::timeout(Duration::from_secs(30),async {
        let frames=vec![
            json!({"type":"Install","version":999,"run_id":"run","generation":2,"scope_digest":"x","scope":empty_scope()}),
            json!({"type":"Renew","run_id":"run","generation":1}),
            json!({"type":"Renew","run_id":"wrong","generation":1,"seq":1}),
            json!({"type":"Renew","run_id":"run","generation":0,"seq":1}),
            json!({"type":"Rebind","run_id":"run","generation":1,"scope":empty_scope()}),
            json!({"type":"Rebind","run_id":"run","generation":0,"scope":empty_scope()}),
            json!({"type":"Unknown"}),json!({"type":"Revoke","run_id":"wrong"}),
            json!({"type":"Renew","run_id":"run","generation":1,"seq":0}),
        ];
        for frame in frames {
            let mut engine=Engine::spawn("minimal").await;engine.ack(&install()).await;
            let address=engine.address().await;let mut mcp=Mcp::connect(address).await;
            write_frame(engine.control.as_mut().unwrap(),&Control::Renew {run_id:"run".into(),generation:1,seq:1}).await.unwrap();
            write_frame(engine.control.as_mut().unwrap(),&frame).await.unwrap();
            mcp.closed().await;engine.wait().await;assert!(TcpStream::connect(address).await.is_err());
        }
        let mut engine=Engine::spawn("minimal").await;engine.ack(&install()).await;
        let mut mcp=Mcp::connect(engine.address().await).await;
        for _ in 0..2 {write_frame(engine.control.as_mut().unwrap(),&Control::Renew {run_id:"run".into(),generation:1,seq:5}).await.unwrap();}
        mcp.closed().await;engine.wait().await;
    }).await.unwrap();
}
#[cfg(unix)]
#[tokio::test]
async fn control_eof_and_partial_frame_revoke_live_data_connections() {
    tokio::time::timeout(Duration::from_secs(15), async {
        for partial in [false, true] {
            let mut engine = Engine::spawn("minimal").await;
            engine.ack(&install()).await;
            let address = engine.address().await;
            let mut mcp = Mcp::connect(address).await;
            if partial {
                engine
                    .control
                    .as_mut()
                    .unwrap()
                    .write_all(&[0, 0, 0, 30, b'{'])
                    .await
                    .unwrap();
            }
            engine.control.take();
            mcp.closed().await;
            engine.wait().await;
            assert!(TcpStream::connect(address).await.is_err());
        }
    })
    .await
    .unwrap();
}
#[cfg(unix)]
#[tokio::test]
async fn missing_initial_fields_or_unknown_version_never_expose_data() {
    tokio::time::timeout(Duration::from_secs(15),async {
        for frame in [json!({"type":"Install","version":999,"run_id":"run","generation":1,"scope_digest":empty_scope().digest("run",1).unwrap(),"scope":empty_scope()}),json!({"type":"Install","version":1,"run_id":"run","generation":1,"scope_digest":"x"})] {
            let mut engine=Engine::spawn("minimal").await;
            write_frame(engine.control.as_mut().unwrap(),&frame).await.unwrap();engine.wait().await;
            let mut stdout=String::new();engine.stdout.read_to_string(&mut stdout).await.unwrap();assert!(stdout.is_empty());
        }
    }).await.unwrap();
}
#[cfg(unix)]
#[tokio::test]
async fn relay_copies_mcp_and_stops_when_control_is_lost() {
    tokio::time::timeout(LIMIT,async {
        let mut engine=Engine::spawn("minimal").await;engine.ack(&install()).await;
        let address=engine.address().await;
        let mut relay=Command::new(env!("CARGO_BIN_EXE_gcu")).args(["scope-relay","--data-address",&address.to_string()])
            .env_clear().stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true).spawn().unwrap();
        let mut input=relay.stdin.take().unwrap();let mut output=BufReader::new(relay.stdout.take().unwrap());
        input.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-03-26\",\"capabilities\":{},\"clientInfo\":{\"name\":\"fake\",\"version\":\"1\"}}}\n").await.unwrap();
        let mut line=String::new();output.read_line(&mut line).await.unwrap();assert!(serde_json::from_str::<Value>(&line).unwrap().get("result").is_some());
        engine.control.take();engine.wait().await;
        assert!(tokio::time::timeout(LIMIT,relay.wait()).await.unwrap().unwrap().success());
    }).await.unwrap();
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn killed_supervisor_revokes_engine_in_another_process() {
    tokio::time::timeout(Duration::from_secs(15), async {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("ready.json");
        let mut supervisor = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "supervisor_process_helper",
                "--nocapture",
            ])
            .env("S3_SUPERVISOR_MARKER", &marker)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let ready = loop {
            if let Ok(bytes) = std::fs::read(&marker) {
                if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                    break value;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        let address = ready["address"].as_str().unwrap().parse().unwrap();
        let mut mcp = Mcp::connect(address).await;
        supervisor.start_kill().unwrap();
        supervisor.wait().await.unwrap();
        mcp.closed().await;
        assert!(TcpStream::connect(address).await.is_err());
        // Grandparent cannot waitpid the orphan. Verify disappearance/zombie
        // with a deadline; the dedicated parent-death test owns its process id.
        let pid = ready["pid"].as_u64().unwrap();
        loop {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"));
            if stat.as_ref().map_or(true, |s| {
                s.rsplit_once(')').unwrap().1.trim_start().starts_with('Z')
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        std::fs::remove_dir_all(ready["root"].as_str().unwrap()).unwrap();
    })
    .await
    .unwrap();
}
#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "subprocess fixture, invoked only by killed_supervisor test"]
async fn supervisor_process_helper() {
    let marker = std::env::var_os("S3_SUPERVISOR_MARKER").unwrap();
    tokio::time::timeout(Duration::from_secs(12), async {
        let mut engine = Engine::spawn("minimal").await;
        engine.ack(&install()).await;
        let address = engine.address().await;
        std::fs::write(
            marker,
            serde_json::to_vec(
                &json!({"address":address,"pid":engine.child.id(),"root":engine._root.path()}),
            )
            .unwrap(),
        )
        .unwrap();
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .unwrap();
}

#[cfg(target_os = "linux")]
fn assert_no_tcp_listener(pid: u32) {
    let sockets: Vec<_> = std::fs::read_dir(format!("/proc/{pid}/fd"))
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|f| std::fs::read_link(f.path()).ok())
        .filter_map(|p| p.to_str().map(str::to_owned))
        .collect();
    for table in ["tcp", "tcp6"] {
        let content = std::fs::read_to_string(format!("/proc/{pid}/net/{table}")).unwrap();
        for row in content.lines().skip(1) {
            let fields: Vec<_> = row.split_whitespace().collect();
            if fields.len() > 9 && fields[3] == "0A" {
                assert!(
                    !sockets.contains(&format!("socket:[{}]", fields[9])),
                    "data listener before ACK"
                );
            }
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn installed_allow_set_is_enforced_and_rebind_can_remove_it() {
    use gcu_host_protocol::scope::{Instance, Target, Window};
    tokio::time::timeout(LIMIT,async {
        let mut scope=empty_scope();
        let instance=Instance {pid:1001,os_start_time:1,boot_identity:"s3-fake-boot".into()};
        scope.targets.push(Target {role:"fixture".into(),instance:instance.clone(),executable_proof:"/fixtures/form".into(),windows:vec![Window {native_id:"form-window".into(),incarnation:1,owner_instance:instance}]});
        let mut engine=Engine::spawn("default").await;
        engine.ack(&Control::Install {version:VERSION,run_id:"run".into(),generation:1,scope_digest:scope.digest("run",1).unwrap(),scope}).await;
        let address=engine.address().await;
        let mut mcp=Mcp::connect(address).await;
        let allowed=mcp.request(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_apps","arguments":{}}})).await;
        assert!(allowed["result"].to_string().contains("fake.form"),"{allowed}");
        engine.ack(&Control::Rebind {run_id:"run".into(),generation:2,scope:empty_scope()}).await;
        mcp.closed().await;
        let mut mcp=Mcp::connect(address).await;
        let denied=mcp.request(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_apps","arguments":{}}})).await;
        assert!(!denied["result"].to_string().contains("fake.form"),"{denied}");
        engine.control.take();mcp.closed().await;engine.wait().await;
    }).await.unwrap();
}

#[tokio::test]
async fn scoped_engine_without_private_control_fails_closed() {
    tokio::time::timeout(LIMIT, async {
        let root = tempfile::tempdir().unwrap();
        // Null stdin uses no private socket/pipe or unproven Windows runtime.
        let output = isolated_command(root.path())
            .args(["scoped-engine", "--driver", "fake", "--scene", "form"])
            .stdin(Stdio::null())
            .output()
            .await
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty(), "scoped engine exposed data");
        #[cfg(windows)]
        {
            assert_eq!(output.status.code(), Some(2));
            assert_eq!(
                String::from_utf8_lossy(&output.stderr).trim(),
                "Scoped engine unsupported on this platform (Windows private-pipe runtime: not_run)"
            );
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn unscoped_fake_call_retains_existing_discovery_without_control() {
    tokio::time::timeout(LIMIT, async {
        let root = tempfile::tempdir().unwrap();
        let mut command = isolated_command(root.path());
        command.args([
            "call",
            "list_apps",
            "--driver",
            "fake",
            "--scene",
            "form",
            "--json",
        ]);
        let output = command.output().await.unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("fake.form"));
    })
    .await
    .unwrap();
}

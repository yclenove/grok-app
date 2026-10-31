//! Harness-free so the same executable can be a silent framed fake child.
//! This tests eval ownership/ACK/renew/exit barriers; gcu-cli's tests exercise
//! the actual scoped engine and MCP server. No model or native driver.
#![allow(clippy::unwrap_used, clippy::expect_used)]
// Windows private-pipe runtime proof: not_run (ADR 0017 S3)
#[cfg(unix)]
mod checks {
    use gcu_eval::{scope::Supervisor, EnvProvider, Environment};
    use gcu_host_protocol::{
        codec::{read_frame, write_frame},
        scope::{Ack, Control, Scope, LEASE},
    };
    use std::{
        io::Write,
        net::SocketAddr,
        time::{Duration, Instant},
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };
    struct TestEnv(Environment);
    impl TestEnv {
        fn new() -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root =
                std::env::temp_dir().join(format!("gcu-s3-test-{}-{nonce}", std::process::id()));
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&root)
                .unwrap();
            let env = Environment {
                home: root.join("home"),
                grok_home: root.join("grok"),
                work: root.join("work"),
                state_path: root.join("state.json"),
                shim_dir: root.join("shims"),
                side_log: root.join("side.log"),
                tool_log: root.join("tools.log"),
                transcript: root.join("grok/transcript.json"),
                root,
            };
            for path in [&env.home, &env.grok_home, &env.work] {
                std::fs::create_dir(path).unwrap();
            }
            Self(env)
        }
        fn events(&self) -> String {
            std::fs::read_to_string(self.0.work.join("events")).unwrap_or_default()
        }
    }
    impl Drop for TestEnv {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0.root);
        }
    }
    fn admitted_engine(binary: &std::path::Path) -> gcu_eval::AdmittedLaunch {
        gcu_eval::mint_scope_lifecycle_test(binary.to_path_buf())
    }
    fn shell_quote(text: &str) -> String {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
    async fn public_path_refuses_before_spawn() {
        use std::os::unix::fs::PermissionsExt;
        let task = gcu_eval::load_tasks(&gcu_eval::bundled_tasks_dir().unwrap())
            .unwrap()
            .into_iter()
            .find(|task| task.id == "native-form-01")
            .unwrap();
        let provision = gcu_eval::LocalTempProvider.provision(&task).unwrap();
        let gcu_eval::Provision::Ready(env) = provision else {
            panic!("local temp provider did not prepare an environment");
        };
        let sentinel = env.work.join("started");
        let wrapper = env.work.join("wrapper.sh");
        let body = format!(
            "#!/bin/sh\nprintf '%s\\n' started > {}\nexit 0\n",
            shell_quote(&sentinel.display().to_string())
        );
        std::fs::write(&wrapper, body).unwrap();
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
        // The wrapper itself can write the sentinel. Admission must not run it.
        let demonstrated = std::process::Command::new(&wrapper).status().unwrap();
        assert!(demonstrated.success());
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"started\n");
        std::fs::remove_file(&sentinel).unwrap();
        let started = Instant::now();
        let opened = Supervisor::open(
            &wrapper,
            &env,
            "form",
            "default",
            "run".into(),
            scope("normal"),
        )
        .await;
        let Err(err) = opened else {
            panic!("public entry spawned an unadmitted backend");
        };
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "admission blocked for {:?}",
            started.elapsed()
        );
        let text = err.to_string();
        assert!(text.contains("provider_unavailable"), "{text}");
        assert!(!sentinel.exists(), "public entry spawned before admission");
        assert!(gcu_eval::LocalTempProvider.destroy(*env).unwrap());
    }
    fn scope(mode: &str) -> Scope {
        Scope {
            engine_instance: mode.into(),
            desktop_session: "fake".into(),
            control_lease: "private".into(),
            targets: vec![],
        }
    }
    async fn request(address: SocketAddr) -> String {
        let mut socket = TcpStream::connect(address).await.unwrap();
        socket.write_all(b"data").await.unwrap();
        let mut reply = String::new();
        socket.read_to_string(&mut reply).await.unwrap();
        reply
    }
    pub(super) async fn run() {
        if std::env::args().nth(1).as_deref() == Some("scoped-engine") {
            fake_engine().await;
            return;
        }
        tokio::time::timeout(Duration::from_secs(20), async {
            public_path_refuses_before_spawn().await;
            let binary = std::env::current_exe().unwrap();
            let env = TestEnv::new();
            let supervisor = Supervisor::start(
                admitted_engine(&binary),
                &env.0,
                "form",
                "default",
                "run".into(),
                scope("normal"),
            )
            .await
            .unwrap();
            let address = supervisor.data_address();
            assert_eq!(request(address).await, "1");
            // Longer than a lease: this goes red if the ownership task stops renewing.
            tokio::time::sleep(LEASE + Duration::from_millis(200)).await;
            assert_eq!(request(address).await, "1");
            let ack = supervisor.rebind(2, scope("normal")).await.unwrap();
            assert_eq!(ack.generation, 2);
            assert_eq!(ack.scope_digest, scope("normal").digest("run", 2).unwrap());
            assert_eq!(request(address).await, "2");
            tokio::time::sleep(Duration::from_millis(1100)).await;
            assert!(
                env.events().contains("renew:2:1"),
                "renew sequence did not rebind: {}",
                env.events()
            );
            let start = Instant::now();
            supervisor.close().await.unwrap();
            assert!(
                start.elapsed() >= Duration::from_millis(150),
                "teardown returned before child exit"
            );
            assert!(env.events().contains("revoked"));
            assert!(env.events().contains("exited"));
            assert!(TcpStream::connect(address).await.is_err());
            let dropped_env = TestEnv::new();
            let dropped = Supervisor::start(
                admitted_engine(&binary),
                &dropped_env.0,
                "form",
                "default",
                "run".into(),
                scope("normal"),
            )
            .await
            .unwrap();
            let dropped_address = dropped.data_address();
            drop(dropped);
            tokio::time::timeout(Duration::from_secs(3), async {
                while !dropped_env.events().contains("exited") {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
            assert!(dropped_env.events().contains("revoked"));
            assert!(TcpStream::connect(dropped_address).await.is_err());
            for mode in ["bad-ack", "bad-run", "bad-digest"] {
                let env = TestEnv::new();
                assert!(Supervisor::start(
                    admitted_engine(&binary),
                    &env.0,
                    "form",
                    "default",
                    "run".into(),
                    scope(mode)
                )
                .await
                .is_err());
                assert!(env.events().contains("exited"));
            }
            let env = TestEnv::new();
            let start = Supervisor::start(
                admitted_engine(&binary),
                &env.0,
                "form",
                "default",
                "run".into(),
                scope("early-address"),
            );
            tokio::pin!(start);
            tokio::select! {
                result = &mut start => panic!("startup completed before ACK: {}", result.is_ok()),
                _ = async {
                    while !env.events().contains("early-address") {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                } => {}
            }
            // stdout already contains a usable loopback address, but the
            // startup future must still withhold it (and thus client startup).
            assert!(tokio::time::timeout(Duration::from_millis(50), &mut start)
                .await
                .is_err());
            assert!(!env.events().contains("client-start"));
            std::fs::write(env.0.work.join("release-ack"), b"release").unwrap();
            if let Ok(supervisor) = start.await {
                std::fs::write(env.0.work.join("client-start"), b"started").unwrap();
                let _ = request(supervisor.data_address()).await;
                panic!("early-address child bypassed ACK verification");
            }
            assert!(!env.0.work.join("client-start").exists());
            assert!(!env.events().contains("client-start"));
            assert!(env.events().contains("exited"));
            // A nonzero engine exit cannot certify child cleanup.
            let env = TestEnv::new();
            let supervisor = Supervisor::start(
                admitted_engine(&binary),
                &env.0,
                "form",
                "default",
                "run".into(),
                scope("bad-exit"),
            )
            .await
            .unwrap();
            assert!(supervisor.close().await.is_err());
        })
        .await
        .unwrap();
    }
    fn event(text: &str) {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("events")
            .unwrap();
        writeln!(file, "{text}").unwrap();
    }
    async fn fake_engine() {
        tokio::time::timeout(Duration::from_secs(15),async {
            let mut control=gcu_host_protocol::scope::launch::inherited().unwrap();
            let frame:Control=read_frame(&mut control).await.unwrap().unwrap();
            let Control::Install {run_id,mut generation,scope_digest,scope,..}=frame else {panic!("missing Install");};
            assert_eq!(scope_digest,scope.digest(&run_id,generation).unwrap());
            let mut ack=Ack {run_id:run_id.clone(),generation,scope_digest};
            match scope.engine_instance.as_str() {
                "bad-ack" => ack.generation += 1,
                "bad-run" | "early-address" => ack.run_id = "wrong-run".into(),
                "bad-digest" => ack.scope_digest = "wrong-digest".into(),
                _ => {}
            }
            let early = scope.engine_instance == "early-address";
            if !early { write_frame(&mut control,&ack).await.unwrap(); }
            let listener=TcpListener::bind((std::net::Ipv4Addr::LOCALHOST,0)).await.unwrap();
            let address=listener.local_addr().unwrap();
            println!("{}",serde_json::json!({"data_address":address}));std::io::stdout().flush().unwrap();
            if early {
                event("early-address");
                while !std::path::Path::new("release-ack").exists() {
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(5)) => {},
                        accepted = listener.accept() => {
                            let _ = accepted.unwrap(); event("client-start");
                        }
                    }
                }
                write_frame(&mut control, &ack).await.unwrap();
            }
            if ["bad-ack", "bad-run", "bad-digest", "early-address"].contains(&scope.engine_instance.as_str()) {
                let _=read_frame::<_,Control>(&mut control).await;event("exited");return;
            }
            let mut seq=0;
            loop {
                // Keep one read future across accepts: cancel/retry of a
                // partially read frame would corrupt this control fixture.
                let frame = {
                    let next=tokio::time::timeout(LEASE,read_frame::<_,Control>(&mut control));
                    tokio::pin!(next);
                    loop {
                    tokio::select! {biased;
                        frame=&mut next=>break frame,
                        client=listener.accept()=>{
                            let (mut socket,_)=client.unwrap();let mut bytes=[0;4];
                            tokio::time::timeout(Duration::from_secs(1),socket.read_exact(&mut bytes)).await.unwrap().unwrap();
                            socket.write_all(generation.to_string().as_bytes()).await.unwrap();
                        }
                    }
                    }
                }; // Drop the pinned read before writing an ACK.
                match frame {
                    Ok(Ok(Some(Control::Renew {run_id:run,generation:next,seq:next_seq}))) if run==run_id && next==generation && next_seq>seq=>{seq=next_seq;event(&format!("renew:{generation}:{seq}"));}
                    Ok(Ok(Some(Control::Rebind {run_id:run,generation:next,scope}))) if run==run_id && next>generation=>{
                        generation=next;seq=0;
                        write_frame(&mut control,&Ack {run_id:run_id.clone(),generation,scope_digest:scope.digest(&run_id,generation).unwrap()}).await.unwrap();
                    }
                    _=>{event("revoked");break;}
                }
            }
            drop(listener);drop(control);
            tokio::time::sleep(Duration::from_millis(200)).await;event("exited");
            if scope.engine_instance == "bad-exit" { std::process::exit(10); }
        }).await.unwrap();
    }
}
fn main() {
    #[cfg(unix)]
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(checks::run());
    #[cfg(not(unix))]
    println!("not_run: Windows private-pipe runtime; production code still compiles");
}

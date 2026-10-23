//! Harness-free so the same executable can be a silent framed fake child.
//! This tests eval ownership/ACK/renew/exit barriers; gcu-cli's tests exercise
//! the actual scoped engine and MCP server. No model or native driver.
#![allow(clippy::unwrap_used, clippy::expect_used)]
// Windows private-pipe runtime proof: not_run (ADR 0017 S3)
#[cfg(unix)]
mod checks {
    use gcu_eval::{scope::Supervisor, Environment};
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
            let binary = std::env::current_exe().unwrap();
            let env = TestEnv::new();
            let supervisor = Supervisor::start(
                &binary,
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
            let env = TestEnv::new();
            assert!(Supervisor::start(
                &binary,
                &env.0,
                "form",
                "default",
                "run".into(),
                scope("bad-ack")
            )
            .await
            .is_err());
            // The failure path must also wait for the child, not just drop a
            // kill-on-drop handle and immediately delete the task directory.
            assert!(env.events().contains("exited"));
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
            if scope.engine_instance=="bad-ack" {ack.generation+=1;}
            write_frame(&mut control,&ack).await.unwrap();
            let listener=TcpListener::bind((std::net::Ipv4Addr::LOCALHOST,0)).await.unwrap();
            let address=listener.local_addr().unwrap();
            println!("{}",serde_json::json!({"data_address":address}));std::io::stdout().flush().unwrap();
            if scope.engine_instance=="bad-ack" {
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

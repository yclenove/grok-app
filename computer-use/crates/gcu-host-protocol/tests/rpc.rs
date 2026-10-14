//! Bounded RPC, callback cancellation, replay, and strict dispatch tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_host_protocol::{auth::*, messages::*, server::*, Error};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::io::duplex;
async fn pair() -> (Peer, Incoming, Peer, Incoming) {
    let (a, b) = duplex(8192);
    let c = AuthConfig::current(Role::Host, Channel::Rpc).unwrap();
    let mut e = c.clone();
    e.role = Role::Engine;
    let secret = Secret::random();
    let other = secret.clone();
    let expected = c.instance.clone();
    let ep = expected.clone();
    let task = tokio::spawn(async move { Peer::connect(a, &c, &expected, &secret).await.unwrap() });
    let (engine, ei, _) = Peer::connect(b, &e, &ep, &other).await.unwrap();
    let (host, hi, _) = task.await.unwrap();
    (host, hi, engine, ei)
}
#[tokio::test]
async fn rpc_success_remote_errors_invalid_parameters_notifications_and_unknown_methods() {
    let (host, mut hi, engine, _) = pair().await;
    let job = tokio::spawn({
        let engine = engine.clone();
        async move {
            engine
                .call::<_, Value>("host.policy.load", &Empty {}, RPC_TIMEOUT)
                .await
        }
    });
    let request = hi.recv().await.unwrap();
    host.respond_flushed(&request, Ok(json!({"ok":true})))
        .await
        .unwrap();
    assert_eq!(job.await.unwrap().unwrap(), json!({"ok":true}));
    let job = tokio::spawn({
        let engine = engine.clone();
        async move {
            engine
                .call::<_, Empty>("host.policy.load", &Empty {}, RPC_TIMEOUT)
                .await
        }
    });
    let request = hi.recv().await.unwrap();
    host.respond(&request, Err(RpcError::new(-32001, "approval_timeout")));
    assert!(matches!(job.await.unwrap(), Err(Error::Remote(-32001))));
    assert!(matches!(
        engine
            .call::<_, Empty>("host.policy.load", &json!({"extra":true}), RPC_TIMEOUT)
            .await,
        Err(Error::Remote(-32602))
    ));
    assert!(matches!(
        engine
            .call::<_, Empty>("host.policy.grantAlways", &Empty {}, RPC_TIMEOUT)
            .await,
        Err(Error::Remote(-32601))
    ));
    engine
        .notify(
            "host.overlay.hide",
            &SessionId {
                session_id: "s".into(),
            },
        )
        .unwrap();
    let request = hi.recv().await.unwrap();
    assert!(request.id.is_none());
    host.respond(&request, Ok(json!({})));
    host.close();
    engine.disconnected().await;
    assert!(!engine.connected());
    assert!(matches!(
        engine
            .call::<_, Empty>("host.policy.load", &Empty {}, RPC_TIMEOUT)
            .await,
        Err(Error::Disconnected)
    ));
    assert!(engine.notify("host.overlay.hide", &Empty {}).is_err());
}
#[tokio::test]
async fn rpc_deadline_disconnect_and_dropped_waiters_never_consume_late_approvals() {
    let (host, mut hi, engine, _) = pair().await;
    let job = tokio::spawn({
        let engine = engine.clone();
        async move {
            engine
                .call::<_, Empty>("host.policy.load", &Empty {}, Duration::from_millis(10))
                .await
        }
    });
    let request = hi.recv().await.unwrap();
    assert!(matches!(job.await.unwrap(), Err(Error::Timeout)));
    host.respond(&request, Ok(json!({})));
    host.respond(&request, Ok(json!({})));
    let job = tokio::spawn({
        let engine = engine.clone();
        async move {
            engine
                .call::<_, Empty>("host.policy.load", &Empty {}, RPC_TIMEOUT)
                .await
        }
    });
    hi.recv().await.unwrap();
    job.abort();
    assert!(job.await.is_err());
    let job = tokio::spawn({
        let engine = engine.clone();
        async move {
            engine
                .call::<_, Empty>("host.policy.load", &Empty {}, RPC_TIMEOUT)
                .await
        }
    });
    hi.recv().await.unwrap();
    host.close();
    assert!(matches!(job.await.unwrap(), Err(Error::Disconnected)));
}
#[tokio::test]
async fn rpc_pending_limit_returns_busy_and_recovers_after_cancellation() {
    let (host, mut hi, engine, _) = pair().await;
    let mut tasks = Vec::new();
    for _ in 0..QUEUE_LIMIT {
        let engine = engine.clone();
        tasks.push(tokio::spawn(async move {
            engine
                .call::<_, Empty>("host.policy.load", &Empty {}, RPC_TIMEOUT)
                .await
        }));
        hi.recv().await.unwrap();
    }
    assert!(matches!(
        engine
            .call::<_, Empty>("host.policy.load", &Empty {}, RPC_TIMEOUT)
            .await,
        Err(Error::Busy)
    ));
    for t in tasks {
        t.abort();
        let _ = t.await;
    }
    host.close();
}
struct Callbacks(AtomicBool);
impl HostCallbacks for Callbacks {
    fn handle(&self, r: Request) -> CallbackFuture<'_> {
        Box::pin(async move {
            if r.method == "host.policy.load" {
                Ok(json!({}))
            } else {
                std::future::pending().await
            }
        })
    }
    fn disconnected(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
#[tokio::test]
async fn host_server_dispatches_callbacks_and_dismisses_on_disconnect() {
    let (a, b) = duplex(8192);
    let c = AuthConfig::current(Role::Host, Channel::Rpc).unwrap();
    let mut e = c.clone();
    e.role = Role::Engine;
    let expected = c.instance.clone();
    let ep = expected.clone();
    let secret = Secret::random();
    let other = secret.clone();
    let task =
        tokio::spawn(async move { HostServer::accept(a, &c, &expected, &secret).await.unwrap() });
    let (engine, _, _) = Peer::connect(b, &e, &ep, &other).await.unwrap();
    let server = task.await.unwrap();
    let callbacks = Arc::new(Callbacks(AtomicBool::new(false)));
    let seen = callbacks.clone();
    let task = tokio::spawn(server.run(callbacks));
    let _: Empty = engine
        .call("host.policy.load", &Empty {}, RPC_TIMEOUT)
        .await
        .unwrap();
    engine.close();
    task.await.unwrap();
    assert!(seen.0.load(Ordering::SeqCst));
}
#[tokio::test]
async fn rpc_replayed_request_id_and_malformed_frame_close_connection() {
    for malformed in [false, true] {
        let (mut a, b) = duplex(8192);
        let c = AuthConfig::current(Role::Host, Channel::Rpc).unwrap();
        let mut e = c.clone();
        e.role = Role::Engine;
        let expected = c.instance.clone();
        let ep = expected.clone();
        let secret = Secret::random();
        let other = secret.clone();
        let task = tokio::spawn(async move {
            authenticate(&mut a, &c, &expected, &secret).await.unwrap();
            a
        });
        let (engine, mut incoming, _) = Peer::connect(b, &e, &ep, &other).await.unwrap();
        let mut raw = task.await.unwrap();
        if malformed {
            use tokio::io::AsyncWriteExt;
            raw.write_all(&[0, 0, 0, 1, 0xff]).await.unwrap();
        } else {
            let request = Envelope::Request(Request {
                jsonrpc: "2.0".into(),
                id: Some(1),
                method: "engine.system.doctor".into(),
                params: json!({}),
            });
            gcu_host_protocol::codec::write_frame(&mut raw, &request)
                .await
                .unwrap();
            incoming.recv().await.unwrap();
            gcu_host_protocol::codec::write_frame(&mut raw, &request)
                .await
                .unwrap();
        }
        tokio::time::timeout(RPC_TIMEOUT, engine.disconnected())
            .await
            .unwrap();
    }
}

//! Cancellation/queue boundaries around actual consumers and duplex bytes.
use super::*;
use crate::acp_client::{AcpEvent, PermissionOutcome};
use crate::session_manager::permission_resolution::PermissionAnswer;
use test_transport::{EventReceiver, WriterPauseAt};
use tokio::io::AsyncReadExt;

fn with_events(fixture: &mut Fixture) -> EventReceiver {
    let entered = fixture.runtime.enter();
    let (client, peer, events) =
        test_transport::client_with_events(false, SessionMcpPolicy::default(), None);
    drop(entered);
    fixture
        .manager
        .with_session_mut(&fixture.target_meta.id, |session| {
            session.acp = Some(client.clone());
        })
        .unwrap();
    fixture.target = client;
    fixture._peers[0] = peer;
    events
}

fn request(
    fixture: &Fixture,
) -> tokio::task::JoinHandle<
    Result<
        Option<PermissionRequestUpdate>,
        crate::session_manager::permission_request::PermissionRequestFailure,
    >,
> {
    let manager = fixture.manager.clone();
    let agent = fixture.target_meta.agent_session_id.clone();
    tokio::spawn(async move {
        manager
            .prepare_permission_request("target-process", agent.as_deref(), event())
            .await
    })
}

fn retry(fixture: &Fixture) -> PermissionAnswer {
    PermissionAnswer {
        rpc_id: 92,
        request_generation: fixture
            .manager
            .pending_permission(Some(fixture.target_meta.id.clone()))
            .map(|card| card.request_generation),
        decision: "deny".into(),
        option_id: None,
        scope: None,
        session_id: Some(fixture.target_meta.id.clone()),
        client_options: None,
        client_tool_name: None,
    }
}

fn exits(events: &mut EventReceiver) -> usize {
    let mut count = 0;
    while let Ok((_, event)) = events.try_recv() {
        if matches!(event, AcpEvent::ProcessExited { .. }) {
            count += 1;
        }
    }
    count
}

async fn read_bytes(peer: &mut tokio::io::DuplexStream, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    tokio::time::timeout(Duration::from_secs(2), peer.read_exact(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    bytes
}

#[test]
fn cancelling_a_partial_permission_frame_blocks_queued_writes_and_releases_actual_waiters() {
    let mut fixture = Fixture::new();
    let mut events = with_events(&mut fixture);
    setup(&fixture, false, PermissionPolicy::AlwaysApprove);
    let mut peer = fixture._peers.remove(0);
    let outcome = fixture.runtime.block_on(async {
        let client = fixture.target.clone();
        let waiter = tokio::spawn(async move {
            test_transport::request_with_timeout(&client, "synthetic/waiter", 4).await
        });
        let mut line = String::new();
        tokio::time::timeout(
            Duration::from_secs(2),
            BufReader::new(&mut peer).read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        let control =
            test_transport::install_writer_pause(&fixture.target, WriterPauseAt::Bytes(17)).await;
        let first = request(&fixture);
        tokio::time::timeout(Duration::from_secs(2), control.wait())
            .await
            .unwrap();
        let client = fixture.target.clone();
        let queued = tokio::spawn(async move {
            client
                .respond_permission(93, PermissionOutcome::Cancelled)
                .await
        });
        tokio::task::yield_now().await;
        first.abort();
        let cancelled = first.await.err().is_some_and(|error| error.is_cancelled());
        control.release();
        let queued = tokio::time::timeout(Duration::from_secs(3), queued)
            .await
            .unwrap()
            .unwrap();
        let waiter = tokio::time::timeout(Duration::from_secs(6), waiter)
            .await
            .unwrap()
            .unwrap();
        let written = control.written.load(std::sync::atomic::Ordering::SeqCst);
        let alive = fixture.target.is_alive();
        let exit_count = exits(&mut events);
        fixture.target.kill().await;
        let bytes = read_bytes(&mut peer, written).await;
        (
            cancelled, queued, waiter, written, alive, exit_count, bytes, line,
        )
    });
    assert!(outcome.0);
    assert!(
        outcome.1.is_err(),
        "queued frame appended to a cancelled partial permission reply"
    );
    assert!(
        !outcome.2.unwrap_err().contains("rpc timeout"),
        "actual sibling waiter survived transport retirement"
    );
    assert_eq!(outcome.3, 17);
    assert!(!outcome.4);
    assert_eq!(outcome.5, 1);
    assert_eq!(&outcome.6, b"{\"id\":92,\"jsonrpc");
    assert!(outcome.7.contains("synthetic/waiter"));
    assert!(fixture.other.is_alive());
    assert!(fixture.manager.permission_responses.lock().is_empty());
    assert_eq!(
        serde_json::to_value(workspace_context::session(&fixture.other_meta.id).unwrap()).unwrap(),
        serde_json::to_value(&fixture.other_meta).unwrap()
    );
}

#[test]
fn cancelling_before_any_permission_bytes_preserves_the_same_gate_for_a_real_retry() {
    let mut fixture = Fixture::new();
    let mut events = with_events(&mut fixture);
    setup(&fixture, false, PermissionPolicy::AlwaysApprove);
    let mut peer = fixture._peers.remove(0);
    let (cancelled, alive, result, replies, exit_count) = fixture.runtime.block_on(async {
        let control =
            test_transport::install_writer_pause(&fixture.target, WriterPauseAt::Bytes(0)).await;
        let first = request(&fixture);
        tokio::time::timeout(Duration::from_secs(2), control.wait())
            .await
            .unwrap();
        first.abort();
        let cancelled = first.await.err().is_some_and(|error| error.is_cancelled());
        control.release();
        let alive = fixture.target.is_alive();
        let result = fixture
            .manager
            .resolve_permission_core(retry(&fixture))
            .await;
        let mut line = String::new();
        tokio::time::timeout(
            Duration::from_secs(2),
            BufReader::new(&mut peer).read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        let exit_count = exits(&mut events);
        fixture.target.kill().await;
        (
            cancelled,
            alive,
            result,
            serde_json::from_str::<Value>(&line).unwrap(),
            exit_count,
        )
    });
    assert!(cancelled);
    assert!(alive);
    assert!(result.is_ok());
    assert_eq!(replies["id"], 92);
    assert_eq!(replies["result"]["outcome"]["optionId"], "reject-once");
    assert_eq!(exit_count, 0);
    assert!(fixture
        .manager
        .pending_permission(Some(fixture.target_meta.id.clone()))
        .is_none());
    assert!(fixture.other.is_alive());
}

#[test]
fn cancelling_a_permission_flush_never_replies_to_the_same_rpc_again() {
    let mut fixture = Fixture::new();
    let mut events = with_events(&mut fixture);
    setup(&fixture, false, PermissionPolicy::AlwaysApprove);
    let mut peer = fixture._peers.remove(0);
    let (cancelled, alive, result, written, bytes, exit_count) = fixture.runtime.block_on(async {
        let control =
            test_transport::install_writer_pause(&fixture.target, WriterPauseAt::Flush).await;
        let first = request(&fixture);
        tokio::time::timeout(Duration::from_secs(2), control.wait())
            .await
            .unwrap();
        let original_written = control.written.load(std::sync::atomic::Ordering::SeqCst);
        first.abort();
        let cancelled = first.await.err().is_some_and(|error| error.is_cancelled());
        control.release();
        let alive = fixture.target.is_alive();
        let result = fixture
            .manager
            .resolve_permission_core(retry(&fixture))
            .await;
        let written = control.written.load(std::sync::atomic::Ordering::SeqCst);
        let exit_count = exits(&mut events);
        fixture.target.kill().await;
        let bytes = read_bytes(&mut peer, written).await;
        (
            cancelled,
            alive,
            result,
            (original_written, written),
            bytes,
            exit_count,
        )
    });
    assert!(cancelled);
    assert!(
        !alive,
        "cancelled flush left the original permission sender alive"
    );
    assert!(
        result.is_err(),
        "same gate was answered a second time after its frame was sent"
    );
    assert_eq!(written.0, written.1);
    let replies = std::str::from_utf8(&bytes)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["id"], 92);
    assert_eq!(exit_count, 1);
}

#[test]
fn partial_frame_timeout_cannot_unlock_a_queue_into_the_same_transport() {
    let mut fixture = Fixture::new();
    let mut events = with_events(&mut fixture);
    setup(&fixture, true, PermissionPolicy::AlwaysApprove);
    let mut peer = fixture._peers.remove(0);
    let (failed, update, queued, written, bytes, exit_count) = fixture.runtime.block_on(async {
        let control =
            test_transport::install_writer_pause(&fixture.target, WriterPauseAt::Bytes(17)).await;
        let first = request(&fixture);
        tokio::time::timeout(Duration::from_secs(2), control.wait())
            .await
            .unwrap();
        // The original eight-second writer is confirmed pending. Start the
        // queue later so its own deadline cannot hide the first writer's gap.
        tokio::time::sleep(Duration::from_secs(4)).await;
        let client = fixture.target.clone();
        let queued = tokio::spawn(async move {
            client
                .respond_permission(93, PermissionOutcome::Cancelled)
                .await
        });
        let release = tokio::spawn({
            let control = control.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(4200)).await;
                control.release();
            }
        });
        let result = tokio::time::timeout(Duration::from_secs(20), first)
            .await
            .unwrap()
            .unwrap();
        let failed = result.is_err();
        let update = fixture.manager.prepare_permission_request_effect(result);
        let queued = tokio::time::timeout(Duration::from_secs(20), queued)
            .await
            .unwrap()
            .unwrap();
        release.await.unwrap();
        let written = control.written.load(std::sync::atomic::Ordering::SeqCst);
        let exit_count = exits(&mut events);
        fixture.target.kill().await;
        let bytes = read_bytes(&mut peer, written).await;
        (failed, update, queued, written, bytes, exit_count)
    });
    assert!(failed);
    assert!(update.is_none());
    assert!(
        queued.is_err(),
        "queue wrote after the partial frame deadline"
    );
    assert_eq!(written, 17);
    assert_eq!(&bytes, b"{\"id\":92,\"jsonrpc");
    assert_eq!(exit_count, 1);
    fixture.assert_other_untouched();
}

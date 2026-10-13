//! Actual Host typed packets/completion receipts; extension outcomes are synthetic.
use super::*;
use crate::browser::extension_action::ActionOutcome;
use crate::browser::extension_action::*;
use crate::protocol::{ActionRequest, Observation};

pub(super) fn start(
    f: &Fixture,
    observation: &Observation,
    id: &str,
) -> (std::thread::JoinHandle<serde_json::Value>, ActionDispatch) {
    f.broker
        .tabs()
        .negotiate_existing_actions(
            &f.origin,
            &f.pairing.session_key,
            ActionNegotiation {
                protocol: 2,
                completion_protocol: 2,
                connection: connection(&f.pairing),
                actions: vec![ActionName::Click],
            },
        )
        .unwrap();
    let request = ActionRequest {
        version: crate::protocol::PROTOCOL_VERSION,
        action_id: id.into(),
        run_id: observation.run_id.clone(),
        target_id: observation.target_id.clone(),
        target_generation: observation.target_generation,
        snapshot_id: observation.snapshot_id.clone(),
        geometry_revision: observation.geometry_revision,
        action: crate::protocol::ActionKind::Click,
        target: crate::protocol::ActionTarget::Element {
            element_ref: observation.nodes[0].node_ref.clone(),
        },
        parameters: json!({}),
    };
    let broker = f.broker.clone();
    let token = f.token.clone();
    let worker = std::thread::spawn(move || {
        crate::tools::dispatch_with_transport(
            &broker,
            &token,
            "computer_act",
            serde_json::to_value(request).unwrap(),
            None,
        )
    });
    let until = Instant::now() + Duration::from_secs(2);
    let dispatch = loop {
        if let Some(dispatch) = f
            .broker
            .tabs()
            .poll_existing_action(&f.origin, &f.pairing.session_key, &connection(&f.pairing))
            .unwrap()
        {
            break dispatch;
        }
        assert!(
            Instant::now() < until,
            "typed action never reached original transport"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(
        dispatch.request.tab_id,
        observation.target_id.strip_prefix("tab:").unwrap()
    );
    f.broker
        .tabs()
        .claim_existing_action(&f.origin, &f.pairing.session_key, &dispatch)
        .unwrap();
    (worker, dispatch)
}

pub(super) fn finish(
    f: &Fixture,
    worker: std::thread::JoinHandle<serde_json::Value>,
    dispatch: ActionDispatch,
) {
    f.broker
        .tabs()
        .settle_existing_completion(&f.origin, &dispatch.proof)
        .unwrap();
    f.broker
        .tabs()
        .complete_existing_action(
            &f.origin,
            &f.pairing.session_key,
            ActionResult {
                request: dispatch.request,
                proof: dispatch.proof,
                outcome: ActionOutcome {
                    status: ActionStatus::Verified,
                    detail: "synthetic_peer_verified".into(),
                },
            },
        )
        .unwrap();
    let result = worker.join().unwrap();
    assert_eq!(result["isError"], false, "{result}");
    assert!(f
        .broker
        .tabs()
        .poll_existing_action(&f.origin, &f.pairing.session_key, &connection(&f.pairing))
        .unwrap()
        .is_none());
    f.no_io();
}

#[test]
fn existing_task_group_unknown_original_action_blocks_borrow_switch_until_physical_settlement() {
    let f = Fixture::new();
    let outcome = approve_group(&f);
    let old = io::observe(&f, &outcome.run_id);
    let (worker, dispatch) = start(&f, &old, "physical-pending");
    assert!(!f.broker.tabs().existing_operations_idle(&outcome.run_id));
    assert!(matches!(
        open(&f, &outcome.run_id, "102"),
        Err(BrokerError::LeaseHeld { .. })
    ));
    assert!(f.broker.tabs().existing_target_info("102").is_none());
    assert_eq!(
        f.broker.tabs().existing_target_info("101").unwrap().run_id,
        outcome.run_id
    );
    assert!(!f.broker.tabs().existing_operations_idle(&outcome.run_id));
    f.broker
        .tabs()
        .settle_existing_completion(&f.origin, &dispatch.proof)
        .unwrap();
    let result = worker.join().unwrap();
    assert_eq!(result["isError"], true);
    assert!(f.broker.tabs().existing_operations_idle(&outcome.run_id));
    // Original active member can settle the interrupted switch too; no new
    // borrow or permission is manufactured to recover a failed destination.
    open(&f, &outcome.run_id, "101").unwrap();
    open(&f, &outcome.run_id, "102").unwrap();
    let fresh = io::observe(&f, &outcome.run_id);
    let (worker, dispatch) = start(&f, &fresh, "fresh-after-settlement");
    finish(&f, worker, dispatch);
    assert!(f.broker.lease.holder().is_none());
}

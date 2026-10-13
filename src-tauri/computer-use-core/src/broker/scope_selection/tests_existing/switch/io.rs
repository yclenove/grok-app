//! Authenticated Host observation transport with a synthetic extension peer.
use super::*;
use crate::browser::extension_protocol::*;
use crate::protocol::Observation;

pub(super) fn poll(f: &Fixture) -> ExtensionRequest {
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(request) = f
            .broker
            .tabs()
            .poll_extension_request(&f.origin, &f.pairing.session_key, &connection(&f.pairing))
            .unwrap()
        {
            return request;
        }
        assert!(
            Instant::now() < until,
            "original observation never reached transport"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub(super) fn complete(f: &Fixture, request: ExtensionRequest) {
    try_complete(f, request).unwrap();
}

pub(super) fn try_complete(f: &Fixture, request: ExtensionRequest) -> Result<(), String> {
    f.broker.tabs().claim_extension_observation(
        &f.origin,
        &f.pairing.session_key,
        &request.completion,
    )?;
    f.broker
        .tabs()
        .settle_extension_observation(&f.origin, &request.completion)?;
    let ExtensionCommand::Observe {
        snapshot_id,
        screenshot,
        ..
    } = &request.command;
    let observation = ExtensionObservation {
        snapshot_id: snapshot_id.clone(),
        document_id: DOCUMENT.into(),
        title: "Shared fixture".into(),
        url: "https://fixture.invalid/".into(),
        text: format!("Visible fixture {}", request.tab_id),
        nodes: vec![ExtensionNode {
            element_ref: format!("{snapshot_id}-1"),
            role: "button".into(),
            name: "Control".into(),
            disabled: false,
        }],
        viewport_width: 800,
        viewport_height: 600,
        truncated: false,
        screenshot: screenshot.then(crate::browser::extension_image::fixture_noisy_image),
    };
    f.broker.tabs().complete_extension_request(
        &f.origin,
        &f.pairing.session_key,
        ExtensionResult {
            request,
            outcome: ExtensionOutcome::Observation { observation },
        },
    )
}

pub(super) fn observe(f: &Fixture, run: &str) -> Observation {
    let broker = f.broker.clone();
    let permit = broker
        .begin_model_call_claiming(&f.token, "computer_observe", Some(run), None)
        .unwrap();
    let worker = std::thread::spawn(move || broker.observe_for_permit(&permit, false));
    let request = poll(f);
    assert_eq!(request.run_id, run);
    complete(f, request);
    worker.join().unwrap().unwrap()
}

pub(super) fn address(f: &Fixture, run: &str) -> crate::broker::HostExecutionAddress {
    let view = f.broker.host_task_view(&f.token).unwrap();
    let execution = view
        .executions
        .into_iter()
        .find(|row| row.run_id == run)
        .unwrap();
    crate::broker::HostExecutionAddress {
        key: execution.key,
        run_id: execution.run_id,
        generation: execution.generation,
    }
}

pub(super) fn stale_action(f: &Fixture, observation: &Observation, action: &str) {
    let mut request = crate::broker::gates::click_req(
        &observation.run_id,
        &observation.target_id,
        observation.target_generation,
        &observation.snapshot_id,
        observation.geometry_revision,
        action,
    );
    request.target = crate::protocol::ActionTarget::Element {
        element_ref: observation.nodes[0].node_ref.clone(),
    };
    let result = crate::tools::dispatch_with_transport(
        &f.broker,
        &f.token,
        "computer_act",
        serde_json::to_value(request).unwrap(),
        None,
    );
    assert_eq!(result["isError"], true, "{result}");
    f.no_io();
}

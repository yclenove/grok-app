//! Original choose/reservation/publication versus App slots and real ACP Arc owners.
use super::*;
mod boundaries;

const CHECKS: &[&str] = &[
    "request-off-no-model-read",
    "real-mcp-first-request-no-authority",
    "original-app-confirm-required",
    "real-unshare-reshare-original-picker",
    "original-owner-replaced-with-zero-authority",
    "publication-owner-replacement-rejects-original-pin",
    "original-task-zero-authority-user-shares-retained",
];

pub(super) fn is_scenario(scenario: &str) -> bool {
    parse(scenario).is_some()
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum ReplacementStage {
    Choose,
    Reservation,
    Publication,
}

#[derive(Clone, Copy)]
enum Identity {
    Process,
    Agent,
    Acp,
    Transport,
}

fn parse(scenario: &str) -> Option<(ReplacementStage, Identity)> {
    use Identity::*;
    use ReplacementStage::*;
    Some(match scenario {
        "choose-app-process-replaced" => (Choose, Process),
        "choose-agent-replaced" => (Choose, Agent),
        "choose-acp-replaced" => (Choose, Acp),
        "choose-transport-replaced" => (Choose, Transport),
        "reservation-app-process-replaced" => (Reservation, Process),
        "reservation-agent-replaced" => (Reservation, Agent),
        "reservation-acp-replaced" => (Reservation, Acp),
        "reservation-transport-replaced" => (Reservation, Transport),
        "publication-app-process-replaced" => (Publication, Process),
        "publication-agent-replaced" => (Publication, Agent),
        "publication-acp-replaced" => (Publication, Acp),
        "publication-transport-replaced" => (Publication, Transport),
        _ => return None,
    })
}

pub(super) fn stage(scenario: &str) -> Option<ReplacementStage> {
    parse(scenario).map(|value| value.0)
}

pub(super) fn rejection(scenario: &str) -> Result<&'static str, String> {
    let (stage, identity) = parse(scenario).ok_or("fixed original owner scenario missing")?;
    Ok(match (stage, identity) {
        (ReplacementStage::Choose, Identity::Agent) => "Computer Use request owner unavailable",
        (ReplacementStage::Choose, _) => "Computer Use original App picker owner changed",
        _ => "Computer Use request owner retired",
    })
}

pub(super) enum ReplacementPoint<'a> {
    Choose(&'a str),
    Reservation(&'a ReservationProbe),
    Publication(&'a mut PublicationProbe),
}

pub(super) struct OriginalOwner<'a> {
    pub(super) manager: &'a Arc<SessionManager>,
    pub(super) fixture: &'a TaskTransportFixture,
    pub(super) key: &'a HostRequestKey,
}

#[derive(Default)]
pub(super) struct OwnerReplacementProbe {
    replaced: bool,
    replacement: Option<TaskTransportFixture>,
    choose_reservation_entries: Option<Arc<AtomicUsize>>,
}

impl OwnerReplacementProbe {
    pub(super) fn replace(
        &mut self,
        scenario: &str,
        runtime: &tokio::runtime::Runtime,
        original: OriginalOwner<'_>,
        point: ReplacementPoint<'_>,
        approvals: &FixtureApprovals,
    ) -> Result<Value, String> {
        if self.replaced || self.replacement.is_some() || !is_scenario(scenario) {
            return Err("original owner replacement repeated or misplaced".into());
        }
        let broker = original.fixture.broker();
        let (stage, identity) = parse(scenario).ok_or("fixed original owner scenario missing")?;
        let ready = match point {
            ReplacementPoint::Publication(publication)
                if stage == ReplacementStage::Publication =>
            {
                publication.ready(original.manager, &broker, approvals)?
            }
            ReplacementPoint::Reservation(reservation)
                if stage == ReplacementStage::Reservation =>
            {
                reservation.ready(original.manager, &broker, approvals)?
            }
            ReplacementPoint::Choose(picker) if stage == ReplacementStage::Choose => {
                // Re-read only the retained ready receipt through the original App.
                // No substitute discovery, selection, reservation or constructor.
                let view = runtime.block_on(original.manager.load_computer_use_task_picker(
                    "view-app",
                    original.key,
                    picker,
                ))?;
                let view = serde_json::to_value(view)
                    .map_err(|_| "original ready receipt serialization failed")?;
                if view["key"] != json!(original.key)
                    || view["pickerId"] != picker
                    || view["candidates"].as_array().map(Vec::len) != Some(3)
                {
                    return Err("original ready receipt changed before replacement".into());
                }
                let entries = Arc::new(AtomicUsize::new(0));
                let entered = entries.clone();
                broker.arm_scope_reservation_hook_for_test(Box::new(move || {
                    entered.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }));
                self.choose_reservation_entries = Some(entries);
                json!({"ready":true,"state":original.fixture.original_host_task_view()?})
            }
            _ => return Err("original owner replacement cut changed".into()),
        };
        if ready["ready"] != true
            || ready["state"]["request"]["key"] != json!(original.key)
            || !original.fixture.original_transport_is_current()
        {
            return Err("original owner was not fixed before replacement".into());
        }
        let before = original.manager.task_picker_endpoint("view-app")?;
        if !Arc::ptr_eq(&before.acp, &original.fixture.client) {
            return Err("original App ACP slot already changed".into());
        }
        // The replacement owns an actual isolated ACP/TCP peer, with no request,
        // grant or execution. Keep both original and replacement owners to finish.
        if matches!(identity, Identity::Acp) {
            self.replacement = Some(runtime.block_on(TaskTransportFixture::new_existing_http()));
        }
        {
            let mut slot = original.manager.inner.lock();
            let current = slot.as_mut().ok_or("original App slot missing")?;
            match identity {
                Identity::Process => {
                    current.process_id = "replacement-process".into();
                }
                Identity::Agent => {
                    current.meta.agent_session_id = Some("replacement-agent".into());
                }
                Identity::Acp => {
                    current.acp = Some(self.replacement.as_ref().unwrap().client.clone());
                }
                Identity::Transport => {
                    // Do not hold an App lock while changing an ACP transport slot.
                }
            }
        }
        if matches!(identity, Identity::Transport) {
            original.fixture.replace_approval_transport_same_identity();
        }
        let after = original.manager.task_picker_endpoint("view-app")?;
        let unchanged = [
            before.process_id == after.process_id,
            before.agent_session_id == after.agent_session_id,
            Arc::ptr_eq(&before.acp, &after.acp),
            original.fixture.original_transport_is_current(),
        ];
        let expected = match identity {
            Identity::Process => [false, true, true, true],
            Identity::Agent => [true, false, true, true],
            Identity::Acp => [true, true, false, true],
            Identity::Transport => [true, true, true, false],
        };
        let state = original.fixture.original_host_task_view()?;
        let publication = stage == ReplacementStage::Publication;
        let reservation = stage == ReplacementStage::Reservation;
        let (stored, remaining, shares) = broker.tabs().stored_pairing_lease_for_test();
        if unchanged != expected
            || !original.fixture.client.is_alive()
            || state.request.as_ref().map(|request| &request.key) != Some(original.key)
            || state.feature_enabled != publication
            || !state.grants.is_empty()
            || state.executions.len() != usize::from(publication)
            || (publication && ready["runId"] != state.executions[0].run_id)
            || approvals.owners() != usize::from(publication)
            || approvals.settings()["computerUseEnabled"] != publication
            || crate::computer_use::task_picker::reservation_owners_for_test()
                != usize::from(reservation)
            || broker.executable_grant_count("view-app") != 0
            || broker.continuation_count("view-app") != 0
            || !stored
            || remaining.is_zero()
            || shares != 3
        {
            return Err(
                "owner replacement changed more than the original publication owner".into(),
            );
        }
        self.replaced = true;
        Ok(
            json!({"replaced":true,"unchanged":unchanged,"runId":ready["runId"],
            "state":state,"ownedWorkers":approvals.owners(),"continuations":0,
            "reservationWorkers":crate::computer_use::task_picker::reservation_owners_for_test(),
            "stored":approvals.settings()["computerUseEnabled"],
            "ownerWitness":self.witness(original.fixture)?}),
        )
    }

    pub(super) fn witness(&self, original: &TaskTransportFixture) -> Result<Value, String> {
        if !self.replaced {
            return Err("original owner replacement missing".into());
        }
        let replacement = self
            .replacement
            .as_ref()
            .map(TaskTransportFixture::original_host_task_view)
            .transpose()?;
        if replacement.as_ref().is_some_and(|view| {
            view.request.is_some() || !view.grants.is_empty() || !view.executions.is_empty()
        }) {
            return Err("replacement ACP fixture acquired authority".into());
        }
        Ok(json!({"originalAcpAlive":original.client.is_alive(),
            "originalTransportCurrent":original.original_transport_is_current(),
            "replacementAcpAlive":self.replacement.as_ref().is_some_and(|f| f.client.is_alive()),
            "replacementRequestExists":replacement.as_ref().is_some_and(|view| view.request.is_some()),
            "replacementGrants":replacement.as_ref().map_or(0, |view| view.grants.len()),
            "replacementExecutions":replacement.as_ref().map_or(0, |view| view.executions.len())}))
    }

    pub(super) async fn finish(&mut self) {
        if let Some(replacement) = self.replacement.take() {
            replacement.finish().await;
            println!("replacement ACP fixture actual kill/finish joined; no request/grant/execution created");
        }
    }
}

#[test]
#[ignore = "manual native gate: explicit Chrome-for-Testing 140+, original source/new TaskGrant pin"]
fn existing_task_joint_publication_owner_process_replacement_rejects_original_pin() {
    scenario("publication-app-process-replaced", CHECKS);
}

#[test]
#[ignore = "manual native gate: explicit Chrome-for-Testing 140+, original source/new TaskGrant pin"]
fn existing_task_joint_publication_owner_agent_replacement_rejects_original_pin() {
    scenario("publication-agent-replaced", CHECKS);
}

#[test]
#[ignore = "manual native gate: explicit Chrome-for-Testing 140+, original source/new TaskGrant pin"]
fn existing_task_joint_publication_owner_acp_replacement_rejects_original_pin() {
    scenario("publication-acp-replaced", CHECKS);
}

#[test]
#[ignore = "manual native gate: explicit Chrome-for-Testing 140+, unchanged original bearer"]
fn existing_task_joint_publication_owner_transport_arc_replacement_rejects_original_pin() {
    scenario("publication-transport-replaced", CHECKS);
}

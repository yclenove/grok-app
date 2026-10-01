use super::*;
use crate::browser::extension_protocol::*;
use crate::browser::SharedTabOffer;
use crate::pairing::{ExtensionPairing, PairingSession, EXTENSION_ID};

pub(super) struct Fixture {
    pub host: ExistingTabHost,
    pub session: PairingSession,
    pub origin: String,
    pub connection: PairingConnection,
}

impl Fixture {
    pub fn new(negotiate: bool) -> Self {
        let host = ExistingTabHost::new();
        let challenge = host.begin_pairing_challenge();
        host.confirm_pairing_app_for(&challenge.nonce).unwrap();
        let session = host
            .complete_pairing_request(&ExtensionPairing::proof_for(
                &challenge,
                &uuid::Uuid::new_v4().to_string(),
            ))
            .unwrap();
        let connection = PairingConnection {
            instance_id: session.instance_id.clone(),
            connection_nonce: session.connection_nonce.clone(),
            generation: session.generation,
        };
        let result = Self {
            host,
            session,
            origin: format!("chrome-extension://{EXTENSION_ID}"),
            connection,
        };
        if negotiate {
            result.negotiate();
        }
        result
    }

    pub fn negotiation(&self) -> ActionNegotiation {
        ActionNegotiation {
            protocol: 2,
            completion_protocol: 2,
            connection: self.connection.clone(),
            actions: vec![
                ActionName::Click,
                ActionName::SetValue,
                ActionName::TypeText,
                ActionName::Scroll,
                ActionName::Wait,
            ],
        }
    }

    pub fn negotiate(&self) {
        self.host
            .negotiate_existing_actions(&self.origin, &self.session.session_key, self.negotiation())
            .unwrap();
    }

    pub fn observe(&self, tab: &str, sequence: u64) -> ExtensionActionCommand {
        self.host
            .offer_connected_tab(
                &self.connection,
                sequence,
                SharedTabOffer {
                    pairing_token: &self.session.session_key,
                    origin: &self.origin,
                    extension_id: Some(EXTENSION_ID),
                    tab_id: tab,
                    title: "fixture",
                    url: "https://fixture.invalid/",
                    browser_id: "chromium",
                    profile_id: "fixture",
                    document_generation: 7,
                    connection_generation: self.connection.generation,
                    focused: true,
                },
            )
            .unwrap();
        let selector = self
            .host
            .list_shared_candidates()
            .into_iter()
            .find(|(id, _, _)| id.starts_with(&format!("{tab}@")))
            .unwrap()
            .0;
        self.host
            .grant_picker_tab("owner", &format!("run-{tab}"), &selector)
            .unwrap();
        let (request, receiver) = self
            .host
            .enqueue_extension_observe("owner", &format!("run-{tab}"), tab, false)
            .unwrap();
        self.host
            .poll_extension_request(&self.origin, &self.session.session_key, &self.connection)
            .unwrap()
            .unwrap();
        let ExtensionCommand::Observe { snapshot_id, .. } = &request.command;
        let element = format!("{snapshot_id}-1");
        let command =
            serde_json::from_value(serde_json::json!({"kind":"act", "snapshotId":snapshot_id,
            "elementRef":element, "action":"click", "parameters":{}}))
            .unwrap();
        let observation = ExtensionObservation {
            snapshot_id: snapshot_id.clone(),
            document_id: "0123456789abcdef0123456789abcdef".into(),
            title: "fixture".into(),
            url: "https://fixture.invalid/".into(),
            text: "fixture".into(),
            nodes: vec![ExtensionNode {
                element_ref: element,
                role: "button".into(),
                name: "click".into(),
                disabled: false,
            }],
            viewport_width: 800,
            viewport_height: 600,
            truncated: false,
            screenshot: None,
        };
        self.host
            .complete_extension_request(
                &self.origin,
                &self.session.session_key,
                ExtensionResult {
                    request,
                    outcome: ExtensionOutcome::Observation { observation },
                },
            )
            .unwrap();
        receiver.recv().unwrap().unwrap();
        command
    }

    pub fn enqueue(
        &self,
        tab: &str,
        command: ExtensionActionCommand,
        cancellation: ActionCancellation,
    ) -> Result<(ExtensionActionRequest, ActionReceiver), BrokerError> {
        let info = self.host.inner.lock().tabs.get(tab).unwrap().info.clone();
        self.host.enqueue_existing_action(ExistingActionInput {
            session: "owner",
            run: &format!("run-{tab}"),
            tab,
            grant_generation: info.generation,
            document_generation: info.document_generation,
            command,
            cancellation,
        })
    }

    pub fn poll(&self) -> Option<ActionDispatch> {
        self.host
            .poll_existing_action(&self.origin, &self.session.session_key, &self.connection)
            .unwrap()
    }

    pub fn claim(&self, dispatch: &ActionDispatch) -> Result<(), String> {
        self.host
            .claim_existing_action(&self.origin, &self.session.session_key, dispatch)
    }

    pub fn result(&self, dispatch: &ActionDispatch, status: ActionStatus) -> Result<(), String> {
        self.host.complete_existing_action(
            &self.origin,
            &self.session.session_key,
            ActionResult {
                request: dispatch.request.clone(),
                proof: dispatch.proof.clone(),
                outcome: ActionOutcome {
                    status,
                    detail: "fixture_result".into(),
                },
            },
        )
    }
}

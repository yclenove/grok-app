//! The production picker constructor with isolated storage, never a second grant API.
use super::*;

impl AcpClient {
    pub(crate) async fn approve_existing_fixture(
        self: &Arc<Self>,
        key: &HostRequestKey,
        picker_id: &str,
        candidates: Vec<String>,
        pin: TaskApprovalPin,
        fixture: Arc<crate::computer_use::test_support::FixtureApprovals>,
    ) -> Result<CommitOutcome, String> {
        self.approve_picker_with(
            "view-app",
            "synthetic-view-agent",
            (key, picker_id),
            candidates,
            pin,
            move |broker, approval, publication| async move {
                let selected = broker.clone();
                fixture
                    .run(broker, approval, publication, move || {
                        crate::computer_use::task_picker::prepare(
                            &selected,
                            SurfaceKind::ExistingTab,
                        )
                    })
                    .await
            },
        )
        .await
    }

    pub(crate) async fn approve_webview_fixture(
        self: &Arc<Self>,
        owner: (&str, &str),
        selection: (&HostRequestKey, &str, Vec<String>),
        pin: TaskApprovalPin,
        fixture: Arc<crate::computer_use::test_support::FixtureApprovals>,
    ) -> Result<CommitOutcome, String> {
        let (session, agent) = owner;
        let (key, picker_id, candidates) = selection;
        self.approve_picker_with(
            session,
            agent,
            (key, picker_id),
            candidates,
            pin,
            move |broker, approval, publication| async move {
                // Discovery registered this exact fixture executor already,
                // matching the product's idempotent second registration.
                fixture.run(broker, approval, publication, || Ok(())).await
            },
        )
        .await
    }
}

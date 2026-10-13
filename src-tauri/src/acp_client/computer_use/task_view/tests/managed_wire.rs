//! One actual local ACP prompt and reply, against the retained synthetic peer.
//! No Grok process, model call or human permission is involved.
use super::*;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

impl TransportFixture {
    pub(crate) async fn queued_continuation_stops_before_wire(
        &mut self,
        delivery: crate::acp_client::computer_use::DecisionDelivery,
        turn: String,
        stop: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let claim = self
            .broker()
            .claim_continuation("view-app")
            .ok_or("original queued continuation claim missing")?;
        if !self.client.pending.lock().is_empty()
            || self.client.prompt_rpc_in_flight("synthetic-view-agent")
            || delivery.app_session() != "view-app"
            || delivery.agent_session() != "synthetic-view-agent"
            || !self.original_transport_is_current()
            || self.broker().continuation_count("view-app") != 1
        {
            return Err("original queued continuation owner was not unique".into());
        }
        // Hold the REAL original stdin admission lock, not a fake delivery flag.
        let owner = self.client.clone();
        let writer = owner.stdin.lock().await;
        let client = self.client.clone();
        let original = delivery.clone();
        let original_turn = turn.clone();
        let mut prompt = tokio::spawn(async move {
            client
                .prompt_computer_use(&original, &turn, &|| Ok(()))
                .await
        });
        let prepared = async {
            tokio::time::timeout(Duration::from_secs(3), async {
                while owner.pending.lock().is_empty() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .map_err(|_| "original continuation never queued at stdin")?;
            if delivery.was_admitted()
                || owner.pending.lock().len() != 1
                || owner.prompt_calls.lock().get("synthetic-view-agent") != Some(&original_turn)
                || self.broker().continuation_count("view-app") != 1
                || self.broker().validate_continuation(&claim).is_err()
            {
                return Err("queued continuation consumed authority before stdin".into());
            }
            self.original_continuation_wire_empty().await?;
            stop()?;
            Ok(())
        }
        .await;
        drop(writer);
        if let Err(error) = prepared {
            prompt.abort();
            let _ = prompt.await;
            return Err(error);
        }
        let completed = match tokio::time::timeout(Duration::from_secs(3), &mut prompt).await {
            Ok(joined) => joined.map_err(|_| "queued original prompt task failed")?,
            Err(_) => {
                prompt.abort();
                let _ = prompt.await;
                return Err("queued original prompt did not join after Stop".into());
            }
        };
        if !matches!(
            completed,
            super::super::super::delivery::DeliveryAttempt::NotStarted
        ) || delivery.was_admitted()
            || !owner.pending.lock().is_empty()
            || self.broker().claim_continuation("view-app").is_some()
            || self.broker().validate_continuation(&claim).is_ok()
        {
            return Err("Stop allowed or abandoned original queued continuation".into());
        }
        self.original_continuation_wire_empty().await?;
        println!("original queued ACP: pending=1, continuation=1 before Stop; original stdin lock; exact NotStarted, zero admission and zero wire; original task joined; original claim retired, no executable continuation");
        Ok(())
    }

    pub(crate) async fn original_continuation_wire_empty(&mut self) -> Result<(), String> {
        // Detect even a partial write, rather than timing out on an incomplete NDJSON line.
        let mut byte = [0_u8; 1];
        if tokio::time::timeout(Duration::from_millis(80), self._peer.read(&mut byte))
            .await
            .is_ok()
        {
            return Err("original ACP peer received bytes or stopped being observable".into());
        }
        Ok(())
    }

    pub(crate) async fn consumed_continuation_cannot_restart_recovery(
        &mut self,
        delivery: &crate::acp_client::computer_use::DecisionDelivery,
        restart_chat: bool,
    ) -> Result<(), String> {
        if !delivery.was_admitted() || !self.original_transport_is_current() {
            return Err("original consumed recovery delivery missing".into());
        }
        for attempt in 0..2 {
            if restart_chat && attempt == 1 {
                self.client.computer_use_chat_started("view-app");
            }
            let result = tokio::time::timeout(
                Duration::from_secs(3),
                self.client.prompt_computer_use(
                    delivery,
                    "synthetic-consumed-recovery-continuation",
                    &|| Ok(()),
                ),
            )
            .await
            .map_err(|_| "original consumed recovery delivery did not settle")?;
            if !matches!(
                result,
                super::super::super::delivery::DeliveryAttempt::NotStarted
            ) || !delivery.was_admitted()
                || !self.client.pending.lock().is_empty()
                || self
                    .client
                    .computer_use_decision("view-app", "synthetic-view-agent")
                    .is_some()
            {
                return Err("consumed delivery revived interrupted recovery".into());
            }
            self.original_continuation_wire_empty().await?;
        }
        Ok(())
    }

    pub(crate) async fn retired_continuation_cannot_restart_chat(
        &mut self,
        delivery: &crate::acp_client::computer_use::DecisionDelivery,
    ) -> Result<(), String> {
        for started in [false, true] {
            if started {
                self.client.computer_use_chat_started("view-app");
            }
            let result = tokio::time::timeout(
                Duration::from_secs(3),
                self.client.prompt_computer_use(
                    delivery,
                    "synthetic-retired-original-continuation",
                    &|| Ok(()),
                ),
            )
            .await
            .map_err(|_| "retired original continuation did not settle")?;
            if !matches!(
                result,
                super::super::super::delivery::DeliveryAttempt::NotStarted
            ) || delivery.was_admitted()
                || !self.client.pending.lock().is_empty()
                || self
                    .client
                    .computer_use_decision("view-app", "synthetic-view-agent")
                    .is_some()
                || !self.original_transport_is_current()
            {
                return Err("old continuation revived after original Chat Stop".into());
            }
            self.original_continuation_wire_empty().await?;
        }
        println!("original Chat Stop replay: two exact NotStarted results, including explicit chat started; zero admission/pending/wire; original transport retained");
        Ok(())
    }

    pub(crate) async fn deliver_managed_continuation_once(&mut self) -> Result<(), String> {
        let delivery = self
            .client
            .computer_use_decision("view-app", "synthetic-view-agent")
            .ok_or("original continuation missing")?;
        if delivery.was_admitted() {
            return Err("continuation was consumed before actual stdin admission".into());
        }
        let client = self.client.clone();
        let original = delivery.clone();
        let mut prompt = tokio::spawn(async move {
            client
                .prompt_computer_use(&original, "synthetic-managed-original-turn", &|| Ok(()))
                .await
        });
        let mut joined = false;
        let result = async {
            let (read, mut write) = self._peer.split();
            let mut lines = BufReader::new(read).lines();
            let line = tokio::time::timeout(Duration::from_secs(3), lines.next_line())
                .await
                .map_err(|_| "original continuation did not reach local ACP wire")?
                .map_err(|_| "local ACP wire read failed")?
                .ok_or("local ACP peer closed before continuation")?;
            let wire: Value = serde_json::from_str(&line).map_err(|_| "invalid ACP prompt wire")?;
            if wire["method"] != "session/prompt"
                || wire["params"]["sessionId"] != "synthetic-view-agent"
                || !delivery.was_admitted()
            {
                return Err("continuation reached a different RPC/agent or was not admitted".into());
            }
            let response = json!({"jsonrpc":"2.0", "id":wire["id"],
                "result":{"stopReason":"end_turn"}})
            .to_string()
                + "\n";
            write
                .write_all(response.as_bytes())
                .await
                .map_err(|_| "local ACP response write failed")?;
            let completed = tokio::time::timeout(Duration::from_secs(3), &mut prompt)
                .await
                .map_err(|_| "original continuation reply did not join")?;
            joined = true;
            let completed = completed.map_err(|_| "original continuation task failed")?;
            if !matches!(completed, super::super::super::delivery::DeliveryAttempt::Completed) {
                return Err("original continuation was not completed".into());
            }
            if !matches!(
                self.client
                    .prompt_computer_use(&delivery, "synthetic-duplicate-turn", &|| Ok(()))
                    .await,
                super::super::super::delivery::DeliveryAttempt::NotStarted
            ) || self
                .client
                .computer_use_decision("view-app", "synthetic-view-agent")
                .is_some()
                || tokio::time::timeout(Duration::from_millis(80), lines.next_line())
                    .await
                    .is_ok()
            {
                return Err("original continuation was duplicated on ACP wire".into());
            }
            println!("managed owned ACP: one actual session/prompt to original agent; reply joined; duplicate rejected before wire");
            Ok(())
        }
        .await;
        if !joined {
            prompt.abort();
            let _ = prompt.await;
        }
        result
    }
}

//! Test harness helper; never replaces model-supplied IDs on the product tool path.
use super::*;

impl ComputerUseBroker {
    /// Host observe → act → observe. Postcondition is never taken from tool ok.
    pub fn observe_act_verify(
        &self,
        req: ActionRequest,
    ) -> Result<(Observation, ActionOutcome, Observation), BrokerError> {
        let t0 = Instant::now();
        let before = self.observe(&req.run_id)?;
        let mut req = req;
        req.snapshot_id = before.snapshot_id.clone();
        req.geometry_revision = before.geometry_revision;
        req.target_generation = before.target_generation;
        let t1 = Instant::now();
        let outcome = self.act(req);
        let t2 = Instant::now();
        let mut last_err: Option<BrokerError> = None;
        let mut after = None;
        for attempt in 0..5 {
            match self.observe(&outcome.run_id) {
                Ok(obs) => {
                    after = Some(obs);
                    break;
                }
                Err(e) => {
                    last_err = Some(e);
                    if attempt + 1 < 5 {
                        std::thread::sleep(Duration::from_millis(40));
                    }
                }
            }
        }
        let t3 = Instant::now();
        {
            let mut g = self.inner.lock();
            if let Some(run) = g.runs.get_mut(&outcome.run_id) {
                run.timings.observe_ms = t1.saturating_duration_since(t0).as_millis() as u64;
                run.timings.act_ms = t2.saturating_duration_since(t1).as_millis() as u64;
                run.timings.verify_ms = t3.saturating_duration_since(t2).as_millis() as u64;
            }
            push_trace(
                &mut g,
                "verify",
                &outcome.run_id,
                &format!("{:?}", outcome.kind),
                TraceAudience::Model,
            );
        }
        match after {
            Some(obs) => Ok((before, outcome, obs)),
            None => Err(BrokerError::Adapter(format!(
                "post-observe failed: {}",
                last_err
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "unknown".into())
            ))),
        }
    }
}

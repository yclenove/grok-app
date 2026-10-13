//! Test lifecycle observation only. Never replay input or lengthen its deadline.
use super::super::ComputerUseBroker;
use grok_computer_use_core::{
    broker::{StopCleanupTicket, StopState},
    error::BrokerError,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Debug, Default)]
pub(crate) struct CleanupEvidence {
    pub(crate) stopped: usize,
    pub(crate) pending_replies: usize,
}

pub(crate) async fn finish_fixture_cleanup(
    broker: Arc<ComputerUseBroker>,
    tickets: Vec<StopCleanupTicket>,
    budget: Duration,
) -> Result<CleanupEvidence, String> {
    // Native HTTP belongs off Tokio's runtime thread. Preserve the captured
    // tickets, even if another original consumer is already physically busy.
    tokio::task::spawn_blocking(move || {
        let deadline = Instant::now() + budget;
        let mut pending = tickets;
        let mut evidence = CleanupEvidence::default();
        while !pending.is_empty() {
            if Instant::now() >= deadline {
                return Err(format!("original fixture cleanup unconfirmed: {} owners, {} pending replies",
                    pending.len(), evidence.pending_replies));
            }
            let mut remaining = Vec::new();
            for ticket in pending {
                match broker.finish_stop_cleanup(&ticket) {
                    Ok(StopState::Stopped) => evidence.stopped += 1,
                    Ok(StopState::StopRequested) => remaining.push(ticket),
                    Err(BrokerError::Adapter(message)) if message.contains("run_cleanup_pending") => {
                        evidence.pending_replies += 1;
                        println!("fixture cleanup: unknown/pending reply retained; no physical success claimed");
                        remaining.push(ticket);
                    }
                    Ok(StopState::Running) => return Err("cleanup ticket regained execution".into()),
                    Err(error) => return Err(format!("original fixture cleanup failed: {error}")),
                }
            }
            pending = remaining;
            if !pending.is_empty() { std::thread::sleep(Duration::from_millis(25)); }
        }
        Ok(evidence)
    }).await.map_err(|_| "fixture cleanup observer did not join".to_string())?
}

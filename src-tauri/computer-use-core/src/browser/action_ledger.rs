//! Run-scoped actionId ledger. Fingerprints are keyed hashes of canonical
//! semantics only; secrets never enter the hash or logs.

use std::collections::HashMap;
use std::sync::OnceLock;

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::error::BrokerError;

const LEDGER_CAP: usize = 256;
type HmacSha256 = Hmac<Sha256>;

fn process_key() -> &'static [u8; 32] {
    static KEY: OnceLock<[u8; 32]> = OnceLock::new();
    KEY.get_or_init(|| {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let mut key = [0u8; 32];
        key[..16].copy_from_slice(a.as_bytes());
        key[16..].copy_from_slice(b.as_bytes());
        key
    })
}

pub fn fingerprint(canonical: &str) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(process_key()).expect("hmac key");
    mac.update(canonical.as_bytes());
    let digest = mac.finalize().into_bytes();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

pub fn canonical_semantics(
    kind: &str,
    page_id: &str,
    page_generation: u64,
    snapshot_id: &str,
    element_ref: Option<&str>,
    params: &serde_json::Value,
) -> String {
    let mut obj = serde_json::Map::new();
    obj.insert("kind".into(), serde_json::json!(kind));
    obj.insert("pageId".into(), serde_json::json!(page_id));
    obj.insert("pageGeneration".into(), serde_json::json!(page_generation));
    obj.insert("snapshotId".into(), serde_json::json!(snapshot_id));
    if let Some(element_ref) = element_ref {
        obj.insert("elementRef".into(), serde_json::json!(element_ref));
    }
    obj.insert("params".into(), params.clone());
    serde_json::Value::Object(obj).to_string()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LedgerOutcome {
    Execute,
    ReplayInFlight,
    ReplayTerminal(LedgerTerminal),
    Conflict,
    Busy,
    Full,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LedgerTerminal {
    Done {
        page_id: String,
        page_generation: u64,
        url: String,
        popup: bool,
    },
    Rejected,
    Unknown,
}

impl LedgerTerminal {
    pub fn done_empty() -> Self {
        Self::Done {
            page_id: String::new(),
            page_generation: 0,
            url: String::new(),
            popup: false,
        }
    }
}

#[derive(Clone, Debug)]
struct Entry {
    fingerprint: [u8; 32],
    terminal: Option<LedgerTerminal>,
}

#[derive(Default)]
pub struct ActionLedger {
    entries: HashMap<String, Entry>,
    in_flight: Option<String>,
}

impl ActionLedger {
    pub fn begin(&mut self, action_id: &str, fingerprint: [u8; 32]) -> LedgerOutcome {
        if let Some(existing) = self.entries.get(action_id) {
            if existing.fingerprint != fingerprint {
                return LedgerOutcome::Conflict;
            }
            return match existing.terminal {
                None if self.in_flight.as_deref() == Some(action_id) => {
                    LedgerOutcome::ReplayInFlight
                }
                None => LedgerOutcome::ReplayInFlight,
                Some(ref terminal) => LedgerOutcome::ReplayTerminal(terminal.clone()),
            };
        }
        if self.entries.len() >= LEDGER_CAP {
            return LedgerOutcome::Full;
        }
        if self.in_flight.is_some() {
            return LedgerOutcome::Busy;
        }
        self.entries.insert(
            action_id.to_string(),
            Entry {
                fingerprint,
                terminal: None,
            },
        );
        self.in_flight = Some(action_id.to_string());
        LedgerOutcome::Execute
    }

    pub fn finish(&mut self, action_id: &str, terminal: LedgerTerminal) {
        if let Some(entry) = self.entries.get_mut(action_id) {
            if entry.terminal == Some(LedgerTerminal::Unknown) {
                // Late completion cannot mutate unknown.
            } else {
                entry.terminal = Some(terminal);
            }
        }
        if self.in_flight.as_deref() == Some(action_id) {
            self.in_flight = None;
        }
    }

    pub fn get(&self, action_id: &str) -> Option<LedgerTerminal> {
        self.entries
            .get(action_id)
            .and_then(|entry| entry.terminal.clone())
    }

    pub fn is_in_flight(&self) -> bool {
        self.in_flight.is_some()
    }
}

pub fn begin_for_run(
    ledgers: &mut HashMap<String, ActionLedger>,
    run_id: &str,
    action_id: &str,
    fingerprint: [u8; 32],
) -> Result<LedgerOutcome, BrokerError> {
    let ledger = ledgers.entry(run_id.to_string()).or_default();
    Ok(ledger.begin(action_id, fingerprint))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Condvar, Mutex};
    use std::thread;

    fn fp(label: &str) -> [u8; 32] {
        fingerprint(label)
    }

    #[test]
    fn same_id_replays_and_conflict_does_not_execute() {
        let mut ledger = ActionLedger::default();
        assert_eq!(ledger.begin("a1", fp("click")), LedgerOutcome::Execute);
        ledger.finish("a1", LedgerTerminal::done_empty());
        assert_eq!(
            ledger.begin("a1", fp("click")),
            LedgerOutcome::ReplayTerminal(LedgerTerminal::done_empty())
        );
        assert_eq!(ledger.begin("a1", fp("type")), LedgerOutcome::Conflict);
        assert_eq!(ledger.get("a1"), Some(LedgerTerminal::done_empty()));
    }

    #[test]
    fn unknown_ignores_late_finish() {
        let mut ledger = ActionLedger::default();
        assert_eq!(ledger.begin("a1", fp("click")), LedgerOutcome::Execute);
        ledger.finish("a1", LedgerTerminal::Unknown);
        ledger.finish("a1", LedgerTerminal::done_empty());
        assert_eq!(ledger.get("a1"), Some(LedgerTerminal::Unknown));
    }

    #[test]
    fn full_ledger_rejects_unseen_but_keeps_known() {
        let mut ledger = ActionLedger::default();
        for i in 0..LEDGER_CAP {
            let id = format!("id-{i}");
            assert_eq!(ledger.begin(&id, fp(&id)), LedgerOutcome::Execute);
            ledger.finish(&id, LedgerTerminal::done_empty());
        }
        assert_eq!(ledger.begin("new", fp("new")), LedgerOutcome::Full);
        assert_eq!(
            ledger.begin("id-0", fp("id-0")),
            LedgerOutcome::ReplayTerminal(LedgerTerminal::done_empty())
        );
    }

    #[test]
    fn concurrent_distinct_ids_are_run_wide_busy_without_sleep() {
        let pair = Arc::new((Mutex::new(ActionLedger::default()), Condvar::new()));
        let started = Arc::new((Mutex::new(false), Condvar::new()));
        let pair_t = pair.clone();
        let started_t = started.clone();
        let first = thread::spawn(move || {
            let (lock, cv) = &*pair_t;
            let mut ledger = lock.lock().unwrap();
            assert_eq!(ledger.begin("one", fp("one")), LedgerOutcome::Execute);
            {
                let (s_lock, s_cv) = &*started_t;
                *s_lock.lock().unwrap() = true;
                s_cv.notify_one();
            }
            while ledger.is_in_flight() {
                ledger = cv.wait(ledger).unwrap();
                if !ledger.is_in_flight() {
                    break;
                }
            }
        });
        {
            let (s_lock, s_cv) = &*started;
            let mut ready = s_lock.lock().unwrap();
            while !*ready {
                ready = s_cv.wait(ready).unwrap();
            }
        }
        {
            let (lock, cv) = &*pair;
            let mut ledger = lock.lock().unwrap();
            assert_eq!(ledger.begin("two", fp("two")), LedgerOutcome::Busy);
            ledger.finish("one", LedgerTerminal::done_empty());
            cv.notify_one();
        }
        first.join().unwrap();
    }
}

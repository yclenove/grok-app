//! Scoped fixture barrier after authentic native completion, before Broker commit.

use super::DownloadResult;
use parking_lot::{Condvar, Mutex};
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

type Key = (String, String);
type Registry = Mutex<BTreeMap<Key, Weak<Gate>>>;

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}

#[derive(Default)]
struct State {
    staged: Option<DownloadResult>,
    arrivals: usize,
    released: bool,
}

#[derive(Default)]
struct Gate {
    state: Mutex<State>,
    changed: Condvar,
}

pub(crate) struct PublicationProbe {
    key: Key,
    gate: Arc<Gate>,
}

impl PublicationProbe {
    pub(crate) fn arm(run_id: &str, native_key: String) -> Result<Self, String> {
        let key = (run_id.to_string(), native_key);
        let mut registry = registry().lock();
        if registry.get(&key).and_then(Weak::upgrade).is_some() {
            return Err("original download publication gate already installed".into());
        }
        let gate = Arc::new(Gate::default());
        registry.insert(key.clone(), Arc::downgrade(&gate));
        Ok(Self { key, gate })
    }

    pub(crate) fn wait_staged(&self) -> Result<DownloadResult, String> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut state = self.gate.state.lock();
        while state.staged.is_none() {
            let now = Instant::now();
            if now >= deadline {
                return Err("original native download missed the publication gate".into());
            }
            self.gate.changed.wait_for(&mut state, deadline - now);
        }
        Ok(state
            .staged
            .clone()
            .expect("observed authentic staged result"))
    }

    pub(crate) fn arrivals(&self) -> usize {
        self.gate.state.lock().arrivals
    }

    pub(crate) fn release(&self) {
        self.gate.state.lock().released = true;
        self.gate.changed.notify_all();
    }
}

impl Drop for PublicationProbe {
    fn drop(&mut self) {
        self.release();
        let mut registry = registry().lock();
        if registry
            .get(&self.key)
            .and_then(Weak::upgrade)
            .is_some_and(|gate| Arc::ptr_eq(&gate, &self.gate))
        {
            registry.remove(&self.key);
        }
    }
}

pub(super) fn before_return(run_id: &str, native_key: &str, result: &DownloadResult) {
    let key = (run_id.to_string(), native_key.to_string());
    let gate = registry().lock().get(&key).and_then(Weak::upgrade);
    let Some(gate) = gate else { return };
    let mut state = gate.state.lock();
    state.arrivals += 1;
    state.staged = Some(result.clone());
    gate.changed.notify_all();
    while !state.released {
        gate.changed.wait(&mut state);
    }
}

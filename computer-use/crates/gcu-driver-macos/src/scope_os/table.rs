//! Window incarnation table.
//!
//! Incarnation moves only when a continuity token says the `CGWindowID` was
//! reused or the owner instance changed. A missing token, a lost observation,
//! or an owner that cannot be re-proved poisons that id. Title and geometry
//! are not inputs.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

use gcu_core::scope::{BootIdentity, OsStartTime, ProcessInstance, WindowBinding};

use super::facts::{is_app_window, OsFacts, OsWindow, Snapshot};

#[derive(Clone, Debug)]
struct Witness {
    start_us: u64,
    boot: String,
}

#[derive(Clone, Debug)]
struct Slot {
    incarnation: u64,
    token: u64,
    owner: ProcessInstance,
}

/// Witnesses and window incarnations for one driver.
///
/// A poisoned id stays unproved for the life of this table. `epoch` is chosen
/// once per table so a new driver does not restart incarnation at 1.
pub(crate) struct IdentityState {
    epoch: u64,
    witnessed: HashMap<u32, Witness>,
    slots: HashMap<u32, Slot>,
    retired: HashMap<u32, u64>,
    poisoned: HashSet<u32>,
    gap: bool,
    /// Rows from the last readable list. Empty after a lost observation.
    pub(crate) rows: Vec<OsWindow>,
}

impl Default for IdentityState {
    fn default() -> Self {
        Self {
            epoch: next_epoch(),
            witnessed: HashMap::new(),
            slots: HashMap::new(),
            retired: HashMap::new(),
            poisoned: HashSet::new(),
            gap: false,
            rows: Vec::new(),
        }
    }
}

fn next_epoch() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    n.saturating_mul(1_u64 << 32)
}

impl IdentityState {
    fn poison(&mut self, id: u32) {
        self.poisoned.insert(id);
        self.slots.remove(&id);
        self.retired.remove(&id);
    }

    fn note_lost(&mut self) {
        let ids: Vec<u32> = self
            .slots
            .keys()
            .copied()
            .chain(self.retired.keys().copied())
            .collect();
        for id in ids {
            self.poisoned.insert(id);
        }
        self.slots.clear();
        self.retired.clear();
        self.rows.clear();
        self.gap = true;
    }
}

/// Re-read one pid. A start or boot change after the first proof is reuse:
/// the replacement is not adopted.
pub(crate) fn prove_pid(
    state: &mut IdentityState,
    facts: &dyn OsFacts,
    pid: u32,
) -> Option<ProcessInstance> {
    if pid == 0 {
        return None;
    }
    let start = facts.process_start_us(pid).filter(|ticks| *ticks != 0)?;
    let boot = facts.boot_identity().filter(|value| !value.is_empty())?;
    remember_owner(state, pid, start, &boot)
}

fn remember_owner(
    state: &mut IdentityState,
    pid: u32,
    start: u64,
    boot: &str,
) -> Option<ProcessInstance> {
    if pid == 0 || start == 0 || boot.is_empty() {
        return None;
    }
    if let Some(seen) = state.witnessed.get(&pid) {
        if seen.start_us != start || seen.boot != boot {
            return None;
        }
    } else {
        state.witnessed.insert(
            pid,
            Witness {
                start_us: start,
                boot: boot.to_owned(),
            },
        );
    }
    ProcessInstance::new(
        pid,
        OsStartTime::new(start).ok()?,
        BootIdentity::new(boot.to_owned()).ok()?,
    )
    .ok()
}

/// Apply one window-list observation. A missing snapshot is a lost observation.
pub(crate) fn sync(state: &mut IdentityState, facts: &dyn OsFacts) {
    if facts.observation_lost() {
        state.note_lost();
        return;
    }
    let Some(snap) = facts.snapshot() else {
        state.note_lost();
        return;
    };
    apply(state, &snap);
}

/// Apply `snap` as one observation. Rows, boot, and starts are not re-read.
pub(crate) fn apply(state: &mut IdentityState, snap: &Snapshot) {
    let rows = &snap.rows;
    if state.gap {
        for row in rows {
            state.poisoned.insert(row.cg_id);
        }
        state.slots.clear();
        state.retired.clear();
        state.rows.clone_from(rows);
        state.gap = false;
        return;
    }
    let present: HashSet<u32> = rows.iter().map(|row| row.cg_id).collect();
    let stale: Vec<u32> = state
        .slots
        .keys()
        .copied()
        .filter(|id| !present.contains(id))
        .collect();
    for id in stale {
        if let Some(slot) = state.slots.remove(&id) {
            state.retired.insert(id, slot.incarnation);
        }
    }
    let boot = snap.boot.as_deref().unwrap_or("");
    for row in rows {
        let start = snap.starts.get(&row.owner_pid).copied().unwrap_or(0);
        consider(state, boot, start, row);
    }
    state.rows.clone_from(rows);
}

fn consider(state: &mut IdentityState, boot: &str, start: u64, row: &OsWindow) {
    if row.cg_id == 0 || state.poisoned.contains(&row.cg_id) {
        return;
    }
    let Some(token) = row.token else {
        state.poison(row.cg_id);
        return;
    };
    let Some(owner) = remember_owner(state, row.owner_pid, start, boot) else {
        state.poison(row.cg_id);
        return;
    };
    let existing = state
        .slots
        .get(&row.cg_id)
        .map(|slot| (slot.incarnation, slot.token, slot.owner.clone()));
    match existing {
        Some((incarnation, seen, previous)) if seen == token && previous == owner => {
            let _ = incarnation;
        }
        Some((incarnation, _, _)) => match incarnation.checked_add(1) {
            Some(next) => {
                state.slots.insert(
                    row.cg_id,
                    Slot {
                        incarnation: next,
                        token,
                        owner,
                    },
                );
            }
            None => state.poison(row.cg_id),
        },
        None => {
            let incarnation = match state.retired.remove(&row.cg_id) {
                Some(previous) => match previous.checked_add(1) {
                    Some(next) => next,
                    None => {
                        state.poison(row.cg_id);
                        return;
                    }
                },
                None => match state.epoch.checked_add(1) {
                    Some(next) => next,
                    None => {
                        state.poison(row.cg_id);
                        return;
                    }
                },
            };
            state.slots.insert(
                row.cg_id,
                Slot {
                    incarnation,
                    token,
                    owner,
                },
            );
        }
    }
}

/// Binding for `cg_id`, or `None` when the id is poisoned or absent.
pub(crate) fn binding_of(state: &IdentityState, cg_id: u32) -> Option<WindowBinding> {
    if state.poisoned.contains(&cg_id) {
        return None;
    }
    let slot = state.slots.get(&cg_id)?;
    WindowBinding::new(cg_id.to_string(), slot.incarnation, slot.owner.clone()).ok()
}

/// Frontmost normal application window. Menu bars and overlays are skipped.
pub(crate) fn front_app(rows: &[OsWindow]) -> Option<&OsWindow> {
    rows.iter().find(|window| is_app_window(window))
}

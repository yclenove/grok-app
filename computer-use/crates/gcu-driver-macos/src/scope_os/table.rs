//! Window incarnation table.
//!
//! Incarnation moves only when a continuity token says the `CGWindowID` was
//! reused or the owner instance changed. A missing token, a lost observation,
//! or an owner that cannot be re-proved poisons that id. Title and geometry
//! are not inputs.

use std::collections::{HashMap, HashSet};

use gcu_core::scope::{BootIdentity, OsStartTime, ProcessInstance, WindowBinding};

use super::facts::{is_app_window, OsFacts, OsWindow};

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
/// A poisoned id stays unproved for the life of this table.
#[derive(Default)]
pub(crate) struct IdentityState {
    witnessed: HashMap<u32, Witness>,
    slots: HashMap<u32, Slot>,
    retired: HashMap<u32, u64>,
    poisoned: HashSet<u32>,
    gap: bool,
    /// Rows from the last readable list. Empty after a lost observation.
    pub(crate) rows: Vec<OsWindow>,
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
    if let Some(seen) = state.witnessed.get(&pid) {
        if seen.start_us != start || seen.boot != boot {
            return None;
        }
    } else {
        state.witnessed.insert(
            pid,
            Witness {
                start_us: start,
                boot: boot.clone(),
            },
        );
    }
    ProcessInstance::new(
        pid,
        OsStartTime::new(start).ok()?,
        BootIdentity::new(boot).ok()?,
    )
    .ok()
}

/// Apply one window-list observation. `None` from the list is a lost observation.
pub(crate) fn sync(state: &mut IdentityState, facts: &dyn OsFacts) {
    if facts.observation_lost() {
        state.note_lost();
        return;
    }
    let Some(rows) = facts.windows() else {
        state.note_lost();
        return;
    };
    if state.gap {
        for row in &rows {
            state.poisoned.insert(row.cg_id);
        }
        state.slots.clear();
        state.retired.clear();
        state.rows = rows;
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
    for row in &rows {
        consider(state, facts, row);
    }
    state.rows = rows;
}

fn consider(state: &mut IdentityState, facts: &dyn OsFacts, row: &OsWindow) {
    if row.cg_id == 0 || state.poisoned.contains(&row.cg_id) {
        return;
    }
    let Some(token) = row.token else {
        state.poison(row.cg_id);
        return;
    };
    let Some(owner) = prove_pid(state, facts, row.owner_pid) else {
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
                None => 1,
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

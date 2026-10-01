//! Reservation and retirement of wrapper ownership, not native permission.
//! Never hold the shared registry across an adapter callback. Pending claim or
//! release callbacks cannot be mistaken for an idle, replaceable owner.
use super::HostOwnedAdapter;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Claiming { released: bool },
    Claimed,
    Releasing,
    Retiring,
}

pub(super) struct Ownership {
    instance: uuid::Uuid,
    backend: &'static str,
    revision: uuid::Uuid,
    claims: HashMap<Option<String>, Phase>,
}

impl HostOwnedAdapter {
    fn owns(&self, owner: &Ownership) -> bool {
        owner.instance == self.instance_id
    }

    pub(super) fn claim_owned(&self, run: Option<&str>, target: &str) -> Result<(), String> {
        let key = run.map(str::to_owned);
        {
            let mut owners = self.owners.lock();
            if let Some(owner) = owners.get_mut(target) {
                if self.owns(owner) && owner.claims.get(&key) == Some(&Phase::Claimed) {
                    return Ok(());
                }
                if !self.owns(owner) || owner.claims.values().any(|p| *p != Phase::Claimed) {
                    return Err(format!("target owned by backend {}; stop and re-authorize before switching to {} (claim or retirement may still be pending)", owner.backend, self.ownership_id));
                }
                // Some native adapters support independent retained snapshots
                // for multiple runs. Forward every new run's claim rather than
                // silently narrowing their capability to one observer.
                owner
                    .claims
                    .insert(key.clone(), Phase::Claiming { released: false });
                owner.revision = uuid::Uuid::new_v4();
            } else {
                owners.insert(
                    target.into(),
                    Ownership {
                        instance: self.instance_id,
                        backend: self.ownership_id,
                        revision: uuid::Uuid::new_v4(),
                        claims: HashMap::from([(key.clone(), Phase::Claiming { released: false })]),
                    },
                );
            }
        }
        let claim = match run {
            Some(run) => self.inner.claim_target_for_run(run, target),
            None => self.inner.claim_target(target),
        };
        let rejected = claim.is_err();
        let released = {
            let mut owners = self.owners.lock();
            // Only this reservation can retire or remove itself. A release
            // during the callback flags it, but does not permit a replacement.
            let owner = owners.get_mut(target).expect("claim reservation retained");
            let phase = owner.claims.get_mut(&key).expect("run claim retained");
            let released = rejected || matches!(phase, Phase::Claiming { released: true });
            *phase = if released {
                Phase::Releasing
            } else {
                Phase::Claimed
            };
            owner.revision = uuid::Uuid::new_v4();
            released
        };
        if released {
            self.finish_release(run, target);
            claim.and(Err("target released while native claim was pending".into()))
        } else {
            Ok(())
        }
    }

    pub(super) fn release_owned(&self, run: Option<&str>, target: &str) {
        let key = run.map(str::to_owned);
        let release = {
            let mut owners = self.owners.lock();
            let Some(owner) = owners.get_mut(target).filter(|o| self.owns(o)) else {
                return;
            };
            let Some(phase) = owner.claims.get_mut(&key) else {
                return;
            };
            match phase {
                Phase::Claiming { .. } => {
                    *phase = Phase::Claiming { released: true };
                    owner.revision = uuid::Uuid::new_v4();
                    false
                }
                Phase::Claimed => {
                    *phase = Phase::Releasing;
                    owner.revision = uuid::Uuid::new_v4();
                    true
                }
                Phase::Releasing | Phase::Retiring => false,
            }
        };
        if release {
            self.finish_release(run, target);
        }
    }

    fn finish_release(&self, run: Option<&str>, target: &str) {
        let key = run.map(str::to_owned);
        match run {
            Some(run) => self.inner.release_target_for_run(run, target),
            None => self.inner.release_target(target),
        }
        {
            let mut owners = self.owners.lock();
            if let Some(owner) = owners.get_mut(target).filter(|o| self.owns(o)) {
                if let Some(phase) = owner.claims.get_mut(&key) {
                    *phase = Phase::Retiring;
                    owner.revision = uuid::Uuid::new_v4();
                }
            }
        }
        // Native release may only have requested Stop. Retain the record
        // until the original native owner proves quiescence, not a timeout.
        self.owner_idle(run);
    }

    pub(super) fn owner_idle(&self, run: Option<&str>) -> bool {
        let key = run.map(str::to_owned);
        let revisions = |owners: &HashMap<String, Ownership>| {
            owners
                .iter()
                .filter(|(_, o)| self.owns(o) && o.claims.contains_key(&key))
                .map(|(target, o)| (target.clone(), o.revision))
                .collect::<HashMap<_, _>>()
        };
        let before = revisions(&self.owners.lock());
        if !self.inner.is_idle(run.unwrap_or("")) {
            return false;
        }
        let mut owners = self.owners.lock();
        // The callback is outside our lock. Its result can only retire the
        // exact reservations observed before it began, not cleanup started
        // during the read or a released/reclaimed same-run/same-target ABA.
        if before != revisions(&owners) {
            return false;
        }
        if owners.values().any(|o| {
            self.owns(o)
                && matches!(
                    o.claims.get(&key),
                    Some(Phase::Claiming { .. } | Phase::Releasing)
                )
        }) {
            return false;
        }
        owners.retain(|_, o| {
            if self.owns(o) && o.claims.get(&key) == Some(&Phase::Retiring) {
                o.claims.remove(&key);
                o.revision = uuid::Uuid::new_v4();
            }
            !o.claims.is_empty()
        });
        true
    }
}

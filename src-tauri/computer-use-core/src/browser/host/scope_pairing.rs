//! Non-destructive metadata preparation. A completed Pair is user device state.
use super::{
    existing::{expire_pairing, invalidate_shared_tabs},
    ExistingTabHost,
};
use crate::pairing::{PairingChallenge, PairingConnection};

#[derive(Clone)]
pub(crate) enum ScopePairing {
    Challenge(PairingChallenge),
    Connection(PairingConnection),
}

impl ExistingTabHost {
    pub(crate) fn scope_pairing_confirmed(&self, challenge: &PairingChallenge) -> bool {
        self.inner
            .lock()
            .pairing
            .app_confirmed_for(&challenge.nonce)
    }

    pub(crate) fn prepare_scope_pairing(&self) -> Result<ScopePairing, String> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        if let Some(connection) = state.pairing.live_connection() {
            return Ok(ScopePairing::Connection(connection));
        }
        if state
            .pairing
            .pending_challenge()
            .is_some_and(|ch| !ch.expired())
        {
            return Err("pairing preparation busy".into());
        }
        invalidate_shared_tabs(&mut state);
        Ok(ScopePairing::Challenge(state.pairing.begin_challenge()))
    }

    pub(crate) fn scope_pairing_current(&self, expected: &ScopePairing) -> bool {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        match expected {
            ScopePairing::Challenge(ch) => {
                state.pairing.pending_challenge().is_some_and(|pending| {
                    !pending.expired()
                        && pending.nonce == ch.nonce
                        && pending.instance_id == ch.instance_id
                        && pending.expires_at_ms == ch.expires_at_ms
                })
            }
            ScopePairing::Connection(connection) => {
                state.pairing.live_connection().as_ref() == Some(connection)
            }
        }
    }

    pub(crate) fn retire_scope_pairing(&self, expected: &ScopePairing) {
        if let ScopePairing::Challenge(ch) = expected {
            // Never revoke a completed device connection or a successor challenge.
            self.inner.lock().pairing.cancel_pending_for(&ch.nonce);
        }
    }
}

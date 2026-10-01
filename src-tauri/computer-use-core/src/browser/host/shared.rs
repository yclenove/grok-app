//! Authenticated candidate updates: identity, sequence and mutation use one lock.
use super::existing::expire_pairing;
use super::{ExistingTabHost, Inner, SharedOffer};
use crate::browser::{SharedTabOffer, TabDisconnect};
use crate::pairing::PairingConnection;

const MAX_CANDIDATES: usize = 64;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_:".contains(&c))
}

pub(super) fn validate_offer(offer: &SharedTabOffer<'_>) -> Result<(), String> {
    let url = reqwest::Url::parse(offer.url).map_err(|_| "invalid shared tab URL")?;
    if !identifier(offer.tab_id)
        || !identifier(offer.browser_id)
        || !identifier(offer.profile_id)
        || offer.title.len() > 1024
        || offer.url.len() > 8192
        || !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || offer.url.chars().any(char::is_control)
        || !(1..=MAX_SAFE_INTEGER).contains(&offer.document_generation)
        || !(1..=MAX_SAFE_INTEGER).contains(&offer.connection_generation)
    {
        return Err("invalid shared tab metadata".into());
    }
    Ok(())
}

fn sequence_ok(state: &Inner, sequence: u64) -> Result<(), String> {
    if sequence <= state.shared_sequence || sequence > MAX_SAFE_INTEGER {
        return Err("stale shared tab sequence".into());
    }
    Ok(())
}

fn invalidate_grant(state: &mut Inner, tab_id: &str) {
    if let Some(record) = state
        .tabs
        .get_mut(tab_id)
        .filter(|record| record.info.user_owned)
    {
        record.info.borrowed = false;
        record.info.generation = record.info.generation.saturating_add(1);
        record.info.preview_generation = 0;
        record.info.disconnect_reason = Some(TabDisconnect::Disconnect.as_str().into());
        record.disconnect = Some(TabDisconnect::Disconnect);
        record.current_observation = None;
        record.pairing_token = None;
    }
    super::completion::sweep(state);
    super::transport::sweep_requests(state);
}

impl ExistingTabHost {
    pub fn offer_connected_tab(
        &self,
        connection: &PairingConnection,
        sequence: u64,
        offer: SharedTabOffer<'_>,
    ) -> Result<(), String> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        state
            .pairing
            .authenticate_connection(offer.origin, offer.pairing_token, connection)?;
        validate_offer(&offer)?;
        sequence_ok(&state, sequence)?;
        if offer.connection_generation != connection.generation
            || offer.extension_id != Some(state.installed_extension_id.as_str())
        {
            return Err("shared tab identity mismatch".into());
        }
        if !state.shared.contains_key(offer.tab_id) && state.shared.len() >= MAX_CANDIDATES {
            return Err("shared tab capacity exceeded".into());
        }
        if state
            .shared
            .get(offer.tab_id)
            .is_some_and(|old| offer.document_generation < old.document_generation)
        {
            return Err("stale shared document".into());
        }
        // Sharing produces only a candidate, never a continued App authorization.
        invalidate_grant(&mut state, offer.tab_id);
        state.shared.insert(
            offer.tab_id.into(),
            SharedOffer {
                picker_token: uuid::Uuid::new_v4().to_string(),
                title: offer.title.into(),
                url: offer.url.into(),
                browser_id: offer.browser_id.into(),
                profile_id: offer.profile_id.into(),
                origin: offer.origin.into(),
                extension_id: offer.extension_id.map(str::to_owned),
                pairing_token: offer.pairing_token.into(),
                document_generation: offer.document_generation,
                connection_generation: connection.generation,
                focused: offer.focused,
            },
        );
        state.shared_sequence = sequence;
        Ok(())
    }

    pub fn unoffer_connected_tab(
        &self,
        origin: &str,
        token: &str,
        connection: &PairingConnection,
        sequence: u64,
        tab_id: &str,
        document: u64,
    ) -> Result<bool, String> {
        let mut state = self.inner.lock();
        expire_pairing(&mut state);
        state
            .pairing
            .authenticate_connection(origin, token, connection)?;
        sequence_ok(&state, sequence)?;
        if !identifier(tab_id) || !(1..=MAX_SAFE_INTEGER).contains(&document) {
            return Err("invalid shared tab metadata".into());
        }
        if state.shared.get(tab_id).is_some_and(|offer| {
            offer.document_generation != document
                || offer.connection_generation != connection.generation
        }) {
            return Err("stale shared document".into());
        }
        let removed = state.shared.remove(tab_id).is_some();
        invalidate_grant(&mut state, tab_id);
        // Retain one high-water mark even for an absent tab: late offers cannot
        // resurrect it, without allocating an unbounded per-tab tombstone map.
        state.shared_sequence = sequence;
        Ok(removed)
    }
}

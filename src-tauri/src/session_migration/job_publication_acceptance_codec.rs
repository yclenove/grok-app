//! Bounded, thread-local memoization of exact member bytes under a loaded key.
//! No SQL reads, membership/body/allocation facts, or writer witnesses live here.
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    mem::size_of,
};

use sha2::{Digest, Sha256};

use super::{InstallationAuthenticator, VerifiedMember, RECORD_BYTES};

const MAX_ENTRIES: usize = 16_384;
const MAX_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub(super) struct Key {
    context: [u8; 32],
    bytes: [u8; 32],
}

impl Key {
    pub(super) fn new(auth: &InstallationAuthenticator, bytes: &[u8]) -> Self {
        Self {
            context: auth.codec_context(),
            bytes: Sha256::digest(bytes).into(),
        }
    }
}

struct Entry {
    encoded: Vec<u8>,
    member: VerifiedMember,
}

impl Entry {
    fn cost(&self) -> usize {
        // Valid members contain fixed digest/UUID strings. Charge three times
        // raw length plus container/key storage for their decoded allocations.
        self.encoded.len().saturating_mul(3) + size_of::<Self>() + 2 * size_of::<Key>()
    }
}

#[derive(Default)]
struct Cache {
    entries: HashMap<Key, Entry>,
    order: VecDeque<Key>,
    bytes: usize,
}

impl Cache {
    fn get(&self, key: &Key, bytes: &[u8]) -> Option<VerifiedMember> {
        self.entries
            .get(key)
            .filter(|entry| entry.encoded == bytes)
            .map(|entry| entry.member.clone())
    }

    fn insert(&mut self, key: Key, bytes: &[u8], member: VerifiedMember) {
        if bytes.len() > RECORD_BYTES {
            return;
        }
        let entry = Entry {
            encoded: bytes.to_vec(),
            member,
        };
        let cost = entry.cost();
        if cost > MAX_BYTES {
            return;
        }
        if let Some(old) = self.entries.remove(&key) {
            self.bytes -= old.cost();
            self.order.retain(|stored| stored != &key);
        }
        while self.entries.len() >= MAX_ENTRIES || self.bytes + cost > MAX_BYTES {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(old) = self.entries.remove(&oldest) {
                self.bytes -= old.cost();
            }
        }
        self.bytes += cost;
        self.order.push_back(key);
        self.entries.insert(key, entry);
    }
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

pub(super) fn get(key: &Key, bytes: &[u8]) -> Option<VerifiedMember> {
    CACHE.with(|cache| cache.borrow().get(key, bytes))
}

pub(super) fn insert(key: Key, bytes: &[u8], member: VerifiedMember) {
    CACHE.with(|cache| cache.borrow_mut().insert(key, bytes, member));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_migration::job_control::publication_acceptance::{Member, MemberFact};

    #[test]
    fn cache_storage_remains_bounded_and_compares_exact_bytes() {
        // Storage-only synthetic data, never an authenticated SQL fixture.
        let member = std::sync::Arc::new(MemberFact {
            body: Member {
                version: 1,
                job_id: "11111111-1111-8111-8111-111111111111".into(),
                header_digest: "1".repeat(64),
                candidate_digest: "2".repeat(64),
                input_digest: "3".repeat(64),
                allocation_digest: "4".repeat(64),
            },
            digest: "5".repeat(64),
            commitment: "6".repeat(64),
        });
        for length in [476, RECORD_BYTES] {
            let mut cache = Cache::default();
            let encoded = vec![b' '; length];
            let first = Key {
                context: [0; 32],
                bytes: [0; 32],
            };
            let mut last = first;
            for index in 0..=MAX_ENTRIES {
                let mut bytes = [0; 32];
                bytes[..8].copy_from_slice(&(index as u64).to_le_bytes());
                last = Key {
                    context: [0; 32],
                    bytes,
                };
                cache.insert(last, &encoded, member.clone());
                assert!(cache.entries.len() <= MAX_ENTRIES);
                assert!(cache.bytes <= MAX_BYTES);
                assert_eq!(cache.order.len(), cache.entries.len());
            }
            assert!(cache.get(&first, &encoded).is_none());
            assert!(cache.get(&last, &encoded).is_some());
            let mut changed = encoded.clone();
            changed[0] = b'x';
            assert!(cache.get(&last, &changed).is_none());
        }
    }
}

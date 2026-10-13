//! Pure deterministic identity hashing. Hashes are process-independent, not cryptographic.

use crate::model::{Fingerprint, RawNode, Rect, Role};

/// Frame buckets are four physical pixels, using floor (also for negative origins).
/// Missing frames differ from zero frames; non-finite values use a sentinel bucket.
/// The hash is FNV-1a over length-delimited UTF-8 identity fields, the role's declared
/// ordinal, the ancestor-path hash, and little-endian quantized x/y/w/h buckets.
/// Value and state deliberately do not affect identity. Changing this constant
/// changes identities and must invalidate snapshots.
pub const FRAME_QUANTUM_PX: f64 = 4.0;
const OFFSET: u64 = 0xcbf29ce484222325;
const PRIME: u64 = 0x100000001b3;

fn bytes(hash: &mut u64, value: &[u8]) {
    for byte in value {
        *hash = (*hash ^ u64::from(*byte)).wrapping_mul(PRIME);
    }
}
fn field(hash: &mut u64, value: Option<&str>) {
    bytes(hash, &[u8::from(value.is_some())]);
    if let Some(value) = value {
        bytes(hash, &(value.len() as u64).to_le_bytes());
        bytes(hash, value.as_bytes());
    }
}
fn identity(
    role: Role,
    name: Option<&str>,
    identifier: Option<&str>,
    ancestor: Fingerprint,
) -> u64 {
    let mut hash = OFFSET;
    bytes(&mut hash, &[role as u8]);
    field(&mut hash, name);
    field(&mut hash, identifier);
    bytes(&mut hash, &ancestor.0.to_le_bytes());
    hash
}

/// Calculate a fingerprint without inspecting a native handle or performing IO.
pub fn fingerprint(
    role: Role,
    name: Option<&str>,
    identifier: Option<&str>,
    ancestor: Fingerprint,
    frame: Option<Rect>,
) -> Fingerprint {
    let mut hash = identity(role, name, identifier, ancestor);
    bytes(&mut hash, &[u8::from(frame.is_some())]);
    if let Some(frame) = frame {
        for value in [frame.x, frame.y, frame.w, frame.h] {
            let bucket = if value.is_finite() {
                (value / FRAME_QUANTUM_PX).floor() as i64
            } else {
                i64::MIN
            };
            bytes(&mut hash, &bucket.to_le_bytes());
        }
    }
    Fingerprint(hash)
}

/// Extend the ancestor path with a parent identity and its native sibling position.
/// Frames are excluded so movement of an ancestor does not rewrite descendant paths.
pub fn ancestor_path(
    parent: Fingerprint,
    role: Role,
    name: Option<&str>,
    identifier: Option<&str>,
    sibling: usize,
) -> Fingerprint {
    let mut hash = identity(role, name, identifier, parent);
    bytes(&mut hash, &(sibling as u64).to_le_bytes());
    Fingerprint(hash)
}

/// Recompute driver fingerprints after a native tree read, preserving all tree data.
pub fn refresh_tree(node: &mut RawNode, ancestor: Fingerprint, sibling: usize) {
    node.fingerprint = fingerprint(
        node.role,
        node.name.as_deref(),
        node.identifier.as_deref(),
        ancestor,
        node.frame,
    );
    let path = ancestor_path(
        ancestor,
        node.role,
        node.name.as_deref(),
        node.identifier.as_deref(),
        sibling,
    );
    for (i, child) in node.children.iter_mut().enumerate() {
        refresh_tree(child, path, i);
    }
}

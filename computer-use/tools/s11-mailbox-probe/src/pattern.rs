//! Both sides build frames from this spec. The host does not send the expected bytes.

use crate::budget::FRAME_LEN;

pub fn frame(case_tag: u8, direction: u8, seq: u8) -> [u8; FRAME_LEN] {
    let mut out = [0u8; FRAME_LEN];
    out[0..6].copy_from_slice(b"S11FR\n");
    out[6] = case_tag;
    out[7] = direction;
    out[8] = seq;
    out[9] = 0;
    let len = (FRAME_LEN as u32).to_be_bytes();
    out[10..14].copy_from_slice(&len);
    for (i, byte) in out.iter_mut().enumerate().skip(14) {
        *byte = seq
            .wrapping_mul(131)
            .wrapping_add(direction.wrapping_mul(17))
            .wrapping_add(i as u8);
    }
    out
}

/// A complete frame from this spec, of either direction. A partial or mixed buffer is not.
pub fn is_spec_frame(bytes: &[u8]) -> bool {
    if bytes.len() != FRAME_LEN || &bytes[0..6] != b"S11FR\n" {
        return false;
    }
    let seq = bytes[8];
    if seq == 0 || seq > crate::budget::C1_ITERS {
        return false;
    }
    bytes == frame(bytes[6], bytes[7], seq)
}

pub fn concat_digest(frames: &[[u8; FRAME_LEN]]) -> String {
    let mut all = Vec::with_capacity(frames.len() * FRAME_LEN);
    for frame in frames {
        all.extend_from_slice(frame);
    }
    crate::sha256::hex_digest(&all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directions_and_seqs_differ_and_match_across_calls() {
        let a = frame(1, 1, 1);
        let b = frame(1, 1, 1);
        let c = frame(1, 2, 1);
        let d = frame(1, 1, 2);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
        assert_eq!(a.len(), 4096);
    }
}

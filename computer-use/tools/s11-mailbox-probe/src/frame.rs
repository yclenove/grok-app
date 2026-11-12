pub const FRAME_LEN: usize = 4096;

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
        *byte = seq.wrapping_mul(131).wrapping_add(direction.wrapping_mul(17)).wrapping_add(i as u8);
    }
    out
}

pub fn classify(bytes: &[u8]) -> Result<(u8, u8, u8), ()> {
    if bytes.len() != FRAME_LEN || &bytes[0..6] != b"S11FR\n" || bytes[8] == 0 {
        return Err(());
    }
    let (tag, dir, seq) = (bytes[6], bytes[7], bytes[8]);
    if bytes == frame(tag, dir, seq) {
        Ok((tag, dir, seq))
    } else {
        Err(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_roundtrip_and_mixed_bytes_are_torn() {
        let a = frame(1, 1, 1);
        let b = frame(1, 2, 1);
        let c = frame(1, 1, 2);
        assert_eq!(a.len(), 4096);
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(classify(&a), Ok((1, 1, 1)));
        let mut mixed = a;
        mixed[100] = mixed[100].wrapping_add(1);
        assert_eq!(classify(&mixed), Err(()));
        assert_eq!(classify(&a[..100]), Err(()));
        let mut swapped = a;
        swapped[200..400].copy_from_slice(&b[200..400]);
        assert_eq!(classify(&swapped), Err(()));
    }
}

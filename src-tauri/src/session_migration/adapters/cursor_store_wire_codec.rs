//! Known Store edge frames and checked protobuf field-4 JSON decoding.
use serde_json::Value;

/// Public Store readers recognize these ordered 34-byte frames. Unknown bytes
/// are not interpreted as JSON or arbitrary hashes.
pub(super) struct References {
    pub edges: Vec<(u8, String)>,
    pub truncated: bool,
}

pub(super) fn references(bytes: &[u8], max: usize) -> Option<References> {
    let mut result = Vec::new();
    let mut offset = 0usize;
    let mut truncated = false;
    while offset < bytes.len() {
        let tag = bytes[offset];
        if matches!(tag, 0x0a | 0x12) && bytes.get(offset + 1) == Some(&0x20) {
            if offset + 34 > bytes.len() {
                truncated = true;
                break;
            }
            if result.len() >= max {
                return None;
            }
            result.push((tag, hex::encode(&bytes[offset + 2..offset + 34])));
            offset += 34;
        } else {
            offset += 1;
        }
    }
    Some(References {
        edges: result,
        truncated,
    })
}

fn json(bytes: &[u8]) -> Option<Value> {
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let value: Value = serde_json::from_slice(bytes).ok()?;
    value.is_object().then_some(value)
}

fn varint(bytes: &[u8], offset: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..=63).step_by(7) {
        let byte = *bytes.get(*offset)?;
        *offset += 1;
        if shift == 63 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

pub(super) fn message(bytes: &[u8]) -> Option<Value> {
    if let Some(value) = json(bytes) {
        return Some(value);
    }
    let mut offset = 0;
    while offset < bytes.len() {
        let tag = varint(bytes, &mut offset)?;
        let field = tag >> 3;
        if field == 0 {
            return None;
        }
        match tag & 7 {
            0 => {
                varint(bytes, &mut offset)?;
            }
            1 | 5 => {
                let size = if tag & 7 == 1 { 8 } else { 4 };
                offset = offset.checked_add(size).filter(|end| *end <= bytes.len())?;
            }
            2 => {
                let length = usize::try_from(varint(bytes, &mut offset)?).ok()?;
                let end = offset
                    .checked_add(length)
                    .filter(|end| *end <= bytes.len())?;
                if field == 4 {
                    if let Some(value) = json(&bytes[offset..end]) {
                        return Some(value);
                    }
                }
                offset = end;
            }
            _ => return None,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checked_field4_skips_supported_fields_and_rejects_bad_lengths() {
        let payload = br#"{"role":"assistant","content":"ok"}"#;
        let mut bytes = vec![8, 1, 17];
        bytes.extend([0; 8]);
        bytes.extend([29, 0, 0, 0, 0, 34, payload.len() as u8]);
        bytes.extend(payload);
        assert_eq!(message(&bytes).unwrap()["content"], "ok");
        for bytes in [
            vec![34, 255],
            vec![34, 127, b'{'],
            vec![0],
            vec![34; 12],
            vec![34, 2, 255, 255],
        ] {
            assert!(message(&bytes).is_none());
        }
        let mut overflow = vec![34];
        overflow.extend([255; 10]);
        assert!(message(&overflow).is_none());
    }
}

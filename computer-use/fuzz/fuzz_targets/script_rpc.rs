#![no_main]
use gcu_script::{decode_payload, encode_payload};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(frame) = decode_payload(data) {
        let encoded = encode_payload(&frame);
        let again = decode_payload(&encoded).expect("roundtrip");
        assert_eq!(frame, again);
    }
});

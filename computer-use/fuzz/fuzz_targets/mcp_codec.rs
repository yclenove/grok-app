#![no_main]
use gcu_mcp::codec::NdjsonCodec;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut whole = NdjsonCodec::default();
    let mut expected = whole.push(data);
    expected.extend(whole.finish());
    let mut streaming = NdjsonCodec::default();
    let mut actual = Vec::new();
    let chunk_size = data.first().map_or(1, |byte| usize::from(*byte) + 1);
    for chunk in data.chunks(chunk_size) {
        actual.extend(streaming.push(chunk));
    }
    actual.extend(streaming.finish());
    assert_eq!(actual, expected);
    assert!(streaming.finish().is_none());
    assert_eq!(
        streaming.push(b"{}\n"),
        NdjsonCodec::default().push(b"{}\n")
    );
});

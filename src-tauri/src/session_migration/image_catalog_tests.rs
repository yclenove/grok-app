use super::*;
use std::io::Cursor;

struct CountReads<'a> {
    input: Cursor<Vec<u8>>,
    calls: &'a mut usize,
}

impl Read for CountReads<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        *self.calls += 1;
        self.input.read(buffer)
    }
}

fn asset(id: &str) -> serde_json::Value {
    serde_json::json!({
        "assetId": id, "digest": null, "mime": "image/png",
        "size": 16, "state": "stored",
    })
}

#[test]
fn imported_images_catalog_batches_io_for_large_embedded_source_references() {
    let mut record = asset("selected");
    record["originalRef"] = format!("data:image/png;base64,{}", "a".repeat(4 * 1024 * 1024)).into();
    let bytes = serde_json::to_vec(&vec![record, asset("later")]).unwrap();
    let mut calls = 0;
    let selected = read(
        CountReads {
            input: Cursor::new(bytes.clone()),
            calls: &mut calls,
        },
        "selected",
    )
    .unwrap();
    assert_eq!(selected.asset_id, "selected");
    // An I/O budget avoids timing-dependent assertions on slower computers.
    // The parser must still read to the end to reject duplicate identities.
    assert!(calls < 2048, "{} bytes required {calls} reads", bytes.len());
}

#[test]
fn imported_images_catalog_checks_the_tail_after_finding_the_selected_asset() {
    let duplicate = serde_json::to_vec(&vec![asset("selected"), asset("selected")]).unwrap();
    assert!(matches!(
        read(Cursor::new(duplicate), "selected"),
        Err(ImageState::Invalid)
    ));
    let mut trailing = serde_json::to_vec(&vec![asset("selected")]).unwrap();
    trailing.extend_from_slice(b" trailing garbage");
    assert!(matches!(
        read(Cursor::new(trailing), "selected"),
        Err(ImageState::Invalid)
    ));
}

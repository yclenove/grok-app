//! The three original loss-path assertions, now using path-scoped transactions.
use super::{
    queue_store::{self, Item},
    QUEUE_CAP, QUEUE_FILE,
};
use std::{fs, path::PathBuf};
pub(super) struct Fixture(pub(super) PathBuf);
impl Fixture {
    pub(super) fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("grok-durable-queue-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    pub(super) fn path(&self) -> PathBuf {
        self.0.join(QUEUE_FILE)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
pub(super) fn item(id: &str) -> Item {
    Item::new("s", id, "prompt", 1)
}
#[test]
fn corrupt_queue_is_preserved_when_enqueue_is_attempted() {
    let home = Fixture::new();
    let original = b"{broken queue";
    fs::write(home.path(), original).unwrap();
    assert!(queue_store::insert(&home.path(), item("new")).is_err());
    assert_eq!(fs::read(home.path()).unwrap(), original);
}
#[test]
fn full_queue_does_not_silently_drop_oldest_accepted_prompt() {
    let home = Fixture::new();
    for i in 0..QUEUE_CAP {
        queue_store::insert(&home.path(), item(&format!("q-{i}"))).unwrap();
    }
    let before = fs::read(home.path()).unwrap();
    assert!(queue_store::insert(&home.path(), item("overflow")).is_err());
    assert_eq!(fs::read(home.path()).unwrap(), before);
}
#[test]
fn claiming_head_retains_payload_until_dispatch_confirmation() {
    let home = Fixture::new();
    let head = item("head");
    queue_store::insert(&home.path(), head.clone()).unwrap();
    assert!(queue_store::claim(&home.path(), &head).unwrap().is_some());
    let items = queue_store::list(&home.path()).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item_id, "head");
    assert_eq!(items[0].prompt, head.prompt);
    assert!(items[0].delivery.is_some());
}

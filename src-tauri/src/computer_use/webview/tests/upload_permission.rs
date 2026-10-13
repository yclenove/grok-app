//! Reuse the typed App adapter with a synthetic retained page, not native consent.
use super::*;

pub(crate) fn fixture() -> (Arc<WebViewAdapter>, impl Fn() -> u64) {
    let view = TestView::named("upload-permission", 0);
    view.upload_field.store(true, Ordering::SeqCst);
    let adapter = Arc::new(WebViewAdapter::new());
    adapter.install_scope_fixture(vec![view.clone()]);
    (adapter, move || view.scripts.load(Ordering::SeqCst))
}

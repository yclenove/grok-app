//! Production Hub revocation on an in-memory channel, without a real browser.

use gcu_core::browser::{BrowserHub, DomChannel, DomEffect, DomFault, DomOperation, Share};
use gcu_core::driver::{Cancel, Driver, TreeOpts};
use gcu_core::model::{RawNode, Role};
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

struct MemoryDom(AtomicUsize);

// Spell out async-trait's boxed futures to avoid adding a dependency for one
// offline channel. The production DomChannel interface is unchanged.
impl DomChannel for MemoryDom {
    fn extension_id(&self) -> &str {
        "eval-extension"
    }

    fn observe<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _share_id: &'life1 str,
    ) -> Pin<Box<dyn Future<Output = Result<Value, DomFault>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(json!({
                "id": "eval-snapshot", "frameSpace": "tab_viewport_physical",
                "geometry": {"width": 600.0, "height": 350.0, "scale": 1.0},
                "incomplete": null, "handoff": false,
                "root": {"id": "root", "role": "document", "native_role": "document",
                    "name": null, "value": null, "states": ["enabled"], "frame": null,
                    "children": [{"id": "dom-button", "role": "button", "native_role": "button",
                        "name": "DOM fixture button", "value": null, "states": ["enabled"],
                        "frame": {"x": 10.0, "y": 10.0, "w": 150.0, "h": 30.0}, "children": []}]}
            }))
        })
    }

    fn perform<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _op: &'life1 DomOperation,
    ) -> Pin<Box<dyn Future<Output = Result<DomEffect, DomFault>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async { Err(DomFault::Unavailable) })
    }
}

fn find_role(node: &RawNode, role: Role) -> Option<&RawNode> {
    if node.role == role {
        return Some(node);
    }
    node.children.iter().find_map(|n| find_role(n, role))
}

fn has_name(node: &RawNode, name: &str) -> bool {
    node.name.as_deref() == Some(name) || node.children.iter().any(|n| has_name(n, name))
}

#[test]
fn revoked_production_hub_returns_l0_without_further_dom_reads() {
    gcu_eval::block_on(async {
        let driver = gcu_driver_fake::FakeDriver::from_scene("webarea").unwrap();
        let window = driver.fixture_window("fake.webarea").await.unwrap();
        let raw = driver
            .snapshot_tree(&window, &TreeOpts::default(), &Cancel::default())
            .await
            .unwrap();
        let web = find_role(raw.root.as_ref().unwrap(), Role::WebArea).unwrap();
        let dom = Arc::new(MemoryDom(AtomicUsize::new(0)));
        let hub = BrowserHub::new(dom.clone());
        hub.bind(Share {
            share_id: "eval-share".into(),
            session_id: "eval-session".into(),
            app_id: window.app.info.id.clone(),
            window_id: window.id.clone(),
            extension_id: "eval-extension".into(),
            web_area_fingerprint: web.fingerprint,
            web_area_frame: web.frame.unwrap(),
            document_id: "eval-document".into(),
        })
        .unwrap();
        let merged = hub.align("eval-session", &window, raw.clone()).await;
        assert!(has_name(
            merged.root.as_ref().unwrap(),
            "DOM fixture button"
        ));
        assert_eq!(dom.0.load(Ordering::SeqCst), 1);
        hub.revoke("eval-session", &window.app.info.id, &window.id);
        let fallback = hub.align("eval-session", &window, raw).await;
        assert!(!has_name(
            fallback.root.as_ref().unwrap(),
            "DOM fixture button"
        ));
        assert!(has_name(fallback.root.as_ref().unwrap(), "Documentation"));
        assert_eq!(dom.0.load(Ordering::SeqCst), 1);
    })
    .unwrap();
}

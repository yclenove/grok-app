use super::*;

#[cfg(feature = "computer-use-probe")]
struct DeadView;

#[cfg(feature = "computer-use-probe")]
impl LiveWebView for DeadView {
    fn label(&self) -> String {
        "resource-browser-dead".into()
    }
    fn tab_id(&self) -> String {
        "tab-0".into()
    }
    fn url(&self) -> Result<String, String> {
        Err("dead".into())
    }
    fn title(&self) -> Result<String, String> {
        Err("dead".into())
    }
    fn is_alive(&self) -> bool {
        false
    }
    fn navigation_generation(&self) -> u64 {
        0
    }
    fn navigate(&self, _url: &str) -> Result<u64, String> {
        Err("dead".into())
    }
}

#[cfg(feature = "computer-use-probe")]
fn eval_and_cookie_rejected(wv: &WebViewAdapter) -> Result<(), String> {
    use super::super::adapter::{ActionScope, DispatchRequest};
    use super::super::protocol::{ActionKind, ActionTarget};
    let eval = wv.act(&DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "r".into(),
        action_id: "eval".into(),
        generation: 1,
        target_id: "wv:any".into(),
        target_generation: 1,
        snapshot_id: "s".into(),
        geometry_revision: 1,
        action: ActionKind::Click,
        target: ActionTarget::Coord { x: 1.0, y: 1.0 },
        parameters: serde_json::json!({"eval": "document.cookie"}),
        scope: ActionScope::Directed,
    });
    match eval {
        Err(e) if e.contains("eval") => {}
        other => return Err(format!("eval must be rejected, got {other:?}")),
    }
    let cookies = wv.act(&DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "r".into(),
        action_id: "ck".into(),
        generation: 1,
        target_id: "wv:any".into(),
        target_generation: 1,
        snapshot_id: "s".into(),
        geometry_revision: 1,
        action: ActionKind::Click,
        target: ActionTarget::Coord { x: 1.0, y: 1.0 },
        parameters: serde_json::json!({"cookies": "sid=1"}),
        scope: ActionScope::Directed,
    });
    match cookies {
        Err(e) if e.contains("cookie") => {}
        other => return Err(format!("cookie reuse must be rejected, got {other:?}")),
    }
    match wv.import_surface_auth("sid=album") {
        Err(e) if e.contains("cookie") || e.contains("auth") => {}
        other => return Err(format!("album auth import must be rejected, got {other:?}")),
    }
    match wv.migrate_auth_to_managed() {
        Err(e) if e.contains("migrate") || e.contains("managed") => {}
        other => return Err(format!("auth migration must be rejected, got {other:?}")),
    }
    Ok(())
}

#[cfg(feature = "computer-use-probe")]
pub fn run_webview_gates() -> Result<(), String> {
    let product = crate::computer_use::product_webview();
    if !product.list_targets()?.is_empty() {
        return Err("product WebView registry must start empty".into());
    }
    let again = crate::computer_use::product_webview();
    if !Arc::ptr_eq(&product, &again) {
        return Err("product WebView adapter must be process-wide".into());
    }
    let wv = WebViewAdapter::new();
    let cap = wv.capabilities();
    if cap.semantic_click || cap.coordinate_click || cap.observe_screenshot {
        return Err("webview must stay honestly unavailable until typed bind".into());
    }
    if !wv.list_targets()?.is_empty() {
        return Err("unbound webview must list no targets".into());
    }
    if wv.target_alive("wv:any") {
        return Err("unbound webview target must be dead".into());
    }
    match wv.observe("wv:any") {
        Err(e) if e.contains("not bound") => {}
        other => return Err(format!("unbound observe must fail closed, got {other:?}")),
    }
    eval_and_cookie_rejected(&wv)?;
    match wv.bind(Arc::new(DeadView), "s91", "run-a", &serde_json::json!({})) {
        Err(e) if e.contains("live") => {}
        other => return Err(format!("dead view bind must fail, got {other:?}")),
    }
    match wv.bind(
        Arc::new(DeadView),
        "s91",
        "run-a",
        &serde_json::json!({"cookies": "sid=1"}),
    ) {
        Err(e) if e.contains("cookie") || e.contains("auth") => {}
        other => return Err(format!("cookie bind must fail, got {other:?}")),
    }
    println!("gate: webview_honest_unavailable");
    #[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
    {
        run_live_bind_gate()?;
        run_typed_act_gate()?;
        run_unsupported_gate()?;
        run_product_bind_rounds(20)?;
        super::probe_isolation::run()?;
        super::probe_dom::run()?;
        super::probe_timeout::run()?;
        super::probe_deadline::run()?;
        super::probe_setup::run()?;
        super::probe_process::run()?;
        super::probe_close::run()?;
    }
    #[cfg(not(all(target_os = "windows", feature = "computer-use-probe")))]
    {
        println!("gate: webview_bind not_run (Host WebView2 probe path is Windows-only)");
    }
    Ok(())
}

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
fn run_live_bind_gate() -> Result<(), String> {
    use super::super::webview_host::HostOwnedWebView;
    use uuid::Uuid;

    let root = std::env::temp_dir().join(format!(
        "grok-cu-s91-{}-{}",
        std::process::id(),
        Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let page_a = root.join("page-a.html");
    let page_b = root.join("page-b.html");
    std::fs::write(
        &page_a,
        "<!doctype html><html lang=\"zh-CN\"><meta charset=\"utf-8\"><title>S91-A</title><body><p id=\"mark\">s91-a</p></body></html>",
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(
        &page_b,
        "<!doctype html><html lang=\"zh-CN\"><meta charset=\"utf-8\"><title>S91-B</title><body><p id=\"mark\">s91-b</p></body></html>",
    )
    .map_err(|e| e.to_string())?;
    let url_a = format!(
        "file:///{}",
        page_a.display().to_string().replace('\\', "/")
    );
    let url_b = format!(
        "file:///{}",
        page_b.display().to_string().replace('\\', "/")
    );
    let live = HostOwnedWebView::spawn("resource-browser-s91", "tab-0", &url_a)?;
    let wv = WebViewAdapter::new();
    if !wv.list_targets()?.is_empty() {
        return live.finish_probe(Err("must stay empty before explicit bind".into()));
    }
    let listed = match wv.bind(live.clone(), "s91-a", "run-a", &serde_json::json!({})) {
        Ok(t) => t,
        Err(e) => return live.finish_probe(Err(e)),
    };
    let fail = |live: &HostOwnedWebView, msg: String| {
        live.finish_probe::<()>(Err(msg))
            .expect_err("primary failure is retained")
    };
    if listed.display_id != "resource-browser-s91" {
        return Err(fail(&live, format!("label missing {listed:?}")));
    }
    if listed.lifecycle_stamp < 1 {
        return Err(fail(&live, format!("generation missing {listed:?}")));
    }
    if !listed.scope_label.contains("s91-a") || !listed.scope_label.contains("run-a") {
        return Err(fail(&live, format!("session/run missing {listed:?}")));
    }
    if !listed.title.contains("S91-A") && !listed.title.contains("page-a") {
        return Err(fail(
            &live,
            format!("title must come from live webview, got {}", listed.title),
        ));
    }
    if !wv.target_alive(&listed.target_id) {
        return Err(fail(&live, "bound target must be alive".into()));
    }
    eval_and_cookie_rejected(&wv).map_err(|e| fail(&live, e))?;
    let old_id = listed.target_id.clone();
    let old_gen = listed.lifecycle_stamp;
    live.navigate(&url_b).map_err(|e| fail(&live, e))?;
    if !wv.list_targets().map_err(|e| fail(&live, e))?.is_empty() {
        return Err(fail(
            &live,
            "navigation must retire the document-scoped binding".into(),
        ));
    }
    if wv.target_alive(&old_id) || wv.observe(&old_id).is_ok() {
        return Err(fail(&live, "old document must remain unauthorized".into()));
    }
    // Navigation cannot silently transfer authority to another document. This
    // dedicated probe explicitly reauthorizes the newly loaded document.
    let after = wv
        .bind(live.clone(), "s91-a", "run-a", &serde_json::json!({}))
        .map_err(|e| fail(&live, e))?;
    if after.lifecycle_stamp <= old_gen {
        return Err(fail(
            &live,
            format!(
                "generation must bump {old_gen} -> {}",
                after.lifecycle_stamp
            ),
        ));
    }
    if wv.target_alive(&old_id) {
        return Err(fail(&live, "stale generation must not stay alive".into()));
    }
    if !wv.target_alive(&after.target_id) {
        return Err(fail(&live, "new generation must be alive".into()));
    }
    if !after.title.contains("S91-B") && !after.title.contains("page-b") {
        return Err(fail(
            &live,
            format!(
                "navigated title must come from live webview, got {}",
                after.title
            ),
        ));
    }
    let url = live.url().map_err(|e| fail(&live, e))?;
    if !url.contains("page-b") {
        return Err(fail(&live, format!("live url after navigate {url}")));
    }
    live.shutdown()?;
    if wv.target_alive(&after.target_id) {
        return Err("closed webview must not stay alive".into());
    }
    println!(
        "gate: webview_bind_live label={} gen={} url={} session=s91-a run=run-a",
        after.display_id, after.lifecycle_stamp, url
    );
    Ok(())
}

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
fn run_typed_act_gate() -> Result<(), String> {
    use super::super::adapter::{ActionScope, DispatchRequest};
    use super::super::protocol::{ActionKind, ActionTarget};
    use super::super::webview_host::HostOwnedWebView;
    use uuid::Uuid;

    let root = std::env::temp_dir().join(format!(
        "grok-cu-s92-{}-{}",
        std::process::id(),
        Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let page_a = root.join("page-a.html");
    let page_b = root.join("page-b.html");
    std::fs::write(
        &page_a,
        r#"<!doctype html><html lang="zh-CN"><meta charset="utf-8"><title>S92-A</title>
<body>
<button id="inc" type="button">+1</button>
<output id="count">0</output>
<input id="name" aria-label="Fixture name" />
<div id="scroller" aria-label="Fixture scroll region" style="height:72px;overflow:auto;border:1px solid #888"><div style="height:400px">scroll-body</div></div>
<a id="go" href="page-b.html">go</a>
<script>
globalThis.fixtureLifecycle = [];
for (const source of [window, window.visualViewport]) for (const type of ['resize', 'scroll', 'blur']) {
  source?.addEventListener(type, event => {
    if (globalThis.fixtureLifecycle.length < 24) globalThis.fixtureLifecycle.push({type, trusted: event.isTrusted, ms: Math.round(performance.now()), width: innerWidth, height: innerHeight});
  }, {capture: true, passive: true});
}
document.getElementById('inc').onclick = () => {
  const n = Number(document.getElementById('count').textContent || '0') + 1;
  document.getElementById('count').textContent = String(n);
};
</script>
</body></html>"#,
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(
        &page_b,
        "<!doctype html><html lang=\"zh-CN\"><meta charset=\"utf-8\"><title>S92-B</title><body><p id=\"mark\">s92-b</p></body></html>",
    )
    .map_err(|e| e.to_string())?;
    let url_a = format!(
        "file:///{}",
        page_a.display().to_string().replace('\\', "/")
    );
    let live = HostOwnedWebView::spawn("resource-browser-s92", "tab-0", &url_a)?;
    let wv = WebViewAdapter::new();
    let listed = match wv.bind(live.clone(), "s92-a", "run-a", &serde_json::json!({})) {
        Ok(t) => t,
        Err(e) => return live.finish_probe(Err(e)),
    };
    let fail = |live: &HostOwnedWebView, msg: String| {
        if let Ok(events) = live.host_script("({events: globalThis.fixtureLifecycle || []})") {
            eprintln!("webview typed fixture lifecycle: {events}");
        }
        live.finish_probe::<()>(Err(msg))
            .expect_err("primary failure is retained")
    };
    eval_and_cookie_rejected(&wv).map_err(|e| fail(&live, e))?;
    let obs = wv.observe(&listed.target_id).map_err(|e| fail(&live, e))?;
    let inc = obs
        .nodes
        .iter()
        .find(|n| n.name == "+1")
        .ok_or_else(|| fail(&live, format!("inc missing {:?}", obs.nodes)))?
        .node_ref
        .clone();
    let click = |action_id: &str, element_ref: &str| DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "run-a".into(),
        action_id: action_id.into(),
        generation: 1,
        target_id: listed.target_id.clone(),
        target_generation: listed.lifecycle_stamp,
        snapshot_id: obs.snapshot_id.clone(),
        geometry_revision: listed.lifecycle_stamp,
        action: ActionKind::Click,
        target: ActionTarget::Element {
            element_ref: element_ref.into(),
        },
        parameters: serde_json::json!({}),
        scope: ActionScope::Directed,
    };
    let clicked = wv
        .act(&click("c1", &inc))
        .map_err(|e| fail(&live, format!("typed first click: {e}")))?;
    if !clicked.applied {
        return Err(fail(&live, "click not applied".into()));
    }
    if wv.act(&click("c1b", &inc)).is_ok() {
        return Err(fail(
            &live,
            "stale elementRef after DOM change must fail".into(),
        ));
    }
    let listed2 = wv
        .list_targets()
        .map_err(|e| fail(&live, e))?
        .into_iter()
        .next()
        .ok_or_else(|| fail(&live, "lost target".into()))?;
    let obs2 = wv.observe(&listed2.target_id).map_err(|e| fail(&live, e))?;
    let name_ref = obs2
        .nodes
        .iter()
        .find(|n| n.name == "Fixture name")
        .ok_or_else(|| fail(&live, "name missing".into()))?
        .node_ref
        .clone();
    let filled = wv
        .act(&DispatchRequest {
            managed_request: None,
            cancellation: Default::default(),
            run_id: "run-a".into(),
            action_id: "fill".into(),
            generation: 1,
            target_id: listed2.target_id.clone(),
            target_generation: listed2.lifecycle_stamp,
            snapshot_id: obs2.snapshot_id.clone(),
            geometry_revision: listed2.lifecycle_stamp,
            action: ActionKind::SetValue,
            target: ActionTarget::Element {
                element_ref: name_ref.clone(),
            },
            parameters: serde_json::json!({ "text": "你好" }),
            scope: ActionScope::Directed,
        })
        .map_err(|e| fail(&live, format!("typed fill: {e}")))?;
    if !filled.applied {
        return Err(fail(&live, format!("fill not applied {filled:?}")));
    }
    let scroll_obs = wv.observe(&listed2.target_id).map_err(|e| fail(&live, e))?;
    let scroll_ref = scroll_obs
        .nodes
        .iter()
        .find(|n| n.name == "Fixture scroll region")
        .ok_or_else(|| fail(&live, "scroll region missing".into()))?
        .node_ref
        .clone();
    let scrolled = wv
        .act(&DispatchRequest {
            managed_request: None,
            cancellation: Default::default(),
            run_id: "run-a".into(),
            action_id: "scroll".into(),
            generation: 1,
            target_id: listed2.target_id.clone(),
            target_generation: listed2.lifecycle_stamp,
            snapshot_id: scroll_obs.snapshot_id,
            geometry_revision: listed2.lifecycle_stamp,
            action: ActionKind::Scroll,
            target: ActionTarget::Element {
                element_ref: scroll_ref,
            },
            parameters: serde_json::json!({ "delta": 120 }),
            scope: ActionScope::Directed,
        })
        .map_err(|e| fail(&live, format!("typed scroll: {e}")))?;
    if !scrolled.applied {
        return Err(fail(&live, "scroll not applied".into()));
    }
    // Independent fixture postconditions, never part of a product observation or trace.
    let fixture_state = "({count: document.getElementById('count').textContent, value: document.getElementById('name').value, scroll: document.getElementById('scroller').scrollTop})";
    let pre = parse_script_json(
        &live
            .host_script(fixture_state)
            .map_err(|e| fail(&live, e))?,
    )
    .map_err(|e| fail(&live, e))?;
    let count = pre
        .get("count")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let name = pre
        .get("value")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if count != "1"
        || name != "你好"
        || pre.get("scroll").and_then(serde_json::Value::as_u64) != Some(120)
    {
        return Err(fail(
            &live,
            format!("live page postcondition count={count} name={name} snap={pre}"),
        ));
    }
    let listed3 = wv
        .list_targets()
        .map_err(|e| fail(&live, e))?
        .into_iter()
        .next()
        .ok_or_else(|| fail(&live, "lost target after fill".into()))?;
    let obs3 = wv.observe(&listed3.target_id).map_err(|e| fail(&live, e))?;
    let go = obs3
        .nodes
        .iter()
        .find(|n| n.name == "go")
        .ok_or_else(|| fail(&live, "go missing".into()))?
        .node_ref
        .clone();
    let navigation_result = wv.act(&DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "run-a".into(),
        action_id: "go".into(),
        generation: 1,
        target_id: listed3.target_id.clone(),
        target_generation: listed3.lifecycle_stamp,
        snapshot_id: obs3.snapshot_id.clone(),
        geometry_revision: listed3.lifecycle_stamp,
        action: ActionKind::Click,
        target: ActionTarget::Element { element_ref: go },
        parameters: serde_json::json!({}),
        scope: ActionScope::Directed,
    });
    match navigation_result {
        Ok(result) if result.applied => {}
        // A click which changes the document can retire its own binding before
        // the reply arrives. Do not require a fabricated verified old result.
        Err(error) if error.contains("outcome unknown after document change") => {}
        other => return Err(fail(&live, format!("navigation click failed: {other:?}"))),
    }
    let start = std::time::Instant::now();
    let mut url = String::new();
    while start.elapsed() < std::time::Duration::from_secs(5) {
        url = live.url().unwrap_or_default();
        if url.contains("page-b") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if !url.contains("page-b") {
        return Err(fail(&live, format!("click go did not navigate {url}")));
    }
    if wv.target_alive(&listed3.target_id)
        || !wv.list_targets().map_err(|e| fail(&live, e))?.is_empty()
    {
        return Err(fail(
            &live,
            "link navigation must retire the bound document".into(),
        ));
    }
    if wv
        .act(&DispatchRequest {
            managed_request: None,
            cancellation: Default::default(),
            run_id: "run-a".into(),
            action_id: "stale".into(),
            generation: 1,
            target_id: listed3.target_id,
            target_generation: listed3.lifecycle_stamp,
            snapshot_id: obs3.snapshot_id,
            geometry_revision: listed3.lifecycle_stamp,
            action: ActionKind::SetValue,
            target: ActionTarget::Element {
                element_ref: name_ref,
            },
            parameters: serde_json::json!({ "text": "x" }),
            scope: ActionScope::Directed,
        })
        .is_ok()
    {
        return Err(fail(&live, "elementRef must die after navigate".into()));
    }
    live.shutdown()?;
    println!("gate: webview_typed_observe_act count={count} name={name} url={url}");
    Ok(())
}

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
fn run_unsupported_gate() -> Result<(), String> {
    use super::super::adapter::{ActionScope, DispatchRequest};
    use super::super::protocol::{ActionKind, ActionTarget};
    use super::super::webview_host::HostOwnedWebView;
    use uuid::Uuid;

    let root = std::env::temp_dir().join(format!(
        "grok-cu-s93-{}-{}",
        std::process::id(),
        Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let page = root.join("cross.html");
    std::fs::write(
        &page,
        r#"<!doctype html><html lang="zh-CN"><meta charset="utf-8"><title>S93</title>
<body data-permission-ui="1">
<iframe id="x" src="https://example.com/"></iframe>
<a id="dl" href="https://example.com/file.bin" download>dl</a>
</body></html>"#,
    )
    .map_err(|e| e.to_string())?;
    let url = format!("file:///{}", page.display().to_string().replace('\\', "/"));
    let live = HostOwnedWebView::spawn("resource-browser-s93", "tab-0", &url)?;
    let wv = WebViewAdapter::new();
    if !wv.list_targets()?.is_empty() {
        return live.finish_probe(Err("unbound must stay empty".into()));
    }
    let listed = match wv.bind(live.clone(), "s93-a", "run-a", &serde_json::json!({})) {
        Ok(t) => t,
        Err(e) => return live.finish_probe(Err(e)),
    };
    let fail = |live: &HostOwnedWebView, msg: String| {
        live.finish_probe::<()>(Err(msg))
            .expect_err("primary failure is retained")
    };
    match wv.observe(&listed.target_id) {
        Err(e) if e.contains("unsupported") && e.contains("cross-origin") => {}
        other => {
            return Err(fail(
                &live,
                format!("cross-origin iframe must be unsupported, got {other:?}"),
            ))
        }
    }
    match wv.act(&DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "run-a".into(),
        action_id: "x".into(),
        generation: 1,
        target_id: listed.target_id,
        target_generation: listed.lifecycle_stamp,
        snapshot_id: "s".into(),
        geometry_revision: listed.lifecycle_stamp,
        action: ActionKind::Click,
        target: ActionTarget::Coord { x: 1.0, y: 1.0 },
        parameters: serde_json::json!({}),
        scope: ActionScope::Directed,
    }) {
        Err(e) if e == "typed WebView action requires an elementRef" => {}
        other => {
            return Err(fail(
                &live,
                format!(
                    "coordinate action without an authorized elementRef must fail, got {other:?}"
                ),
            ))
        }
    }
    eval_and_cookie_rejected(&wv).map_err(|e| fail(&live, e))?;
    live.shutdown()?;
    if !wv.list_targets()?.is_empty() {
        return Err("closed webview must list no targets".into());
    }
    println!("gate: webview_unsupported cross-origin-iframe");
    Ok(())
}

#[cfg(all(target_os = "windows", feature = "computer-use-probe"))]
fn run_product_bind_rounds(rounds: u32) -> Result<(), String> {
    use super::super::webview_host::HostOwnedWebView;
    use uuid::Uuid;
    let root = std::env::temp_dir().join(format!(
        "grok-cu-wv-product-{}-{}",
        std::process::id(),
        Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let page = root.join("page.html");
    std::fs::write(
        &page,
        "<!doctype html><html><meta charset=\"utf-8\"><title>WV-P</title><body><p id=\"mark\">ok</p></body></html>",
    )
    .map_err(|e| e.to_string())?;
    let url = format!("file:///{}", page.display().to_string().replace('\\', "/"));
    let live = HostOwnedWebView::spawn("resource-browser-product", "tab-0", &url)?;
    let wv = crate::computer_use::product_webview();
    let mut err: Option<String> = None;
    for i in 1..=rounds {
        wv.unbind();
        if !wv.list_targets().unwrap_or_default().is_empty() {
            err = Some(format!("product webview not empty before round {i}"));
            break;
        }
        match wv.bind(live.clone(), "s-prod", "run-prod", &serde_json::json!({})) {
            Ok(listed) if listed.kind == "webview" || listed.target_id.starts_with("wv|") => {}
            Ok(listed) => {
                err = Some(format!("round {i} expected webview target, got {listed:?}"));
                break;
            }
            Err(e) => {
                err = Some(e);
                break;
            }
        }
        wv.unbind();
    }
    live.finish_probe(err.map_or(Ok(()), Err))?;
    println!("gate: webview_product_bind_rounds={rounds}");
    Ok(())
}

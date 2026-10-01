//! Model-facing managed-browser tools. Writes go through Broker dispatch.

use serde_json::{json, Value};

use crate::broker::{BrowserOp, BrowserWrite, BrowserWriteResult, ComputerUseBroker};
use crate::browser::{observation::model_observation_view, ManagedLocator};
use crate::ipc::{error, fail, success};

fn require_id<'a>(args: &'a Value, key: &str) -> Result<&'a str, Value> {
    let value = args.get(key).and_then(Value::as_str).unwrap_or("");
    if value.trim().is_empty() {
        return Err(error(format!("{key} required")));
    }
    if value.len() > 256 {
        return Err(error("invalid identity length"));
    }
    Ok(value)
}

fn require_generation(args: &Value) -> Result<u64, Value> {
    let generation = args
        .get("pageGeneration")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if generation == 0 {
        return Err(error("pageGeneration required"));
    }
    Ok(generation)
}

fn model_url(url: &str) -> String {
    crate::browser::model_visible_url(url)
}

pub(crate) fn list_tabs(broker: &ComputerUseBroker, run: &str) -> Value {
    let tabs: Vec<Value> = broker
        .tabs()
        .list_for_run(run)
        .into_iter()
        .map(|tab| {
            json!({
                "tabId": tab.tab_id,
                "title": tab.title,
                "url": model_url(&tab.url),
                "pageGeneration": tab.document_generation.max(1),
                "borrowed": tab.borrowed,
            })
        })
        .collect();
    success(json!({ "tabs": tabs }))
}

pub(crate) fn open_tab(broker: &ComputerUseBroker, run: &str, args: &Value) -> Value {
    let tab_id = match require_id(args, "tabId") {
        Ok(id) => id,
        Err(err) => return err,
    };
    let Some(tab) = broker
        .tabs()
        .list_for_run(run)
        .into_iter()
        .find(|tab| tab.tab_id == tab_id)
    else {
        return error("select an already authorized tab; this cannot create a grant or profile");
    };
    success(json!({
        "tabId": tab.tab_id,
        "pageGeneration": tab.document_generation.max(1),
        "url": model_url(&tab.url),
    }))
}

pub(crate) fn observe_tab(broker: &ComputerUseBroker, run: &str, args: &Value) -> Value {
    let tab_id = match require_id(args, "tabId") {
        Ok(id) => id,
        Err(err) => return err,
    };
    let page_generation = match require_generation(args) {
        Ok(gen) => gen,
        Err(err) => return err,
    };
    match broker.observe_managed_tab(run, tab_id, page_generation) {
        Ok(mut obs) => {
            let png = obs.png_base64.take();
            let mut content = Vec::new();
            if let Some(data) = png.filter(|value| !value.is_empty()) {
                content.push(json!({"type":"image","mimeType":"image/png","data":data}));
            }
            content.push(json!({
                "type": "text",
                "text": model_observation_view(&obs).to_string(),
            }));
            json!({"isError":false,"content":content})
        }
        Err(e) => fail(&e),
    }
}

pub(crate) fn act_tab(broker: &ComputerUseBroker, session: &str, run: &str, args: &Value) -> Value {
    let tab_id = match require_id(args, "tabId") {
        Ok(id) => id,
        Err(err) => return err,
    };
    let action_id = match require_id(args, "actionId") {
        Ok(id) => id,
        Err(err) => return err,
    };
    let kind = match require_id(args, "kind") {
        Ok(id) => id,
        Err(err) => return err,
    };
    let page_generation = match require_generation(args) {
        Ok(gen) => gen,
        Err(err) => return err,
    };
    let snapshot_id = args.get("snapshotId").and_then(Value::as_str).unwrap_or("");
    if snapshot_id.trim().is_empty() {
        return error("snapshotId required");
    }
    let element_ref = args
        .get("elementRef")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let params = args.get("parameters").cloned().unwrap_or_else(|| json!({}));
    if let Some(key) = crate::protocol::forbidden_permission_param(&params) {
        return error(format!("{key} never grants desktop control"));
    }
    match broker.dispatch_browser(BrowserWrite {
        session_id: Some(session),
        run_id: run,
        tab_id,
        action_id,
        page_generation,
        op: BrowserOp::Act {
            kind,
            snapshot_id,
            locator: ManagedLocator { element_ref },
            params,
        },
    }) {
        Ok(BrowserWriteResult::Page(page)) => success(json!({
            "tabId": tab_id,
            "pageGeneration": page.page_generation,
            "url": model_url(&page.url),
        })),
        Ok(_) => error("act returned an unexpected result"),
        Err(e) => fail(&e),
    }
}

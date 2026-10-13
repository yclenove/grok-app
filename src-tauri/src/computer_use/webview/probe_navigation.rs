//! Real WebView2 navigation through the production Broker + TaskGrant tool path.

use super::*;
use crate::computer_use::webview_host::HostOwnedWebView;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::thread::{self, JoinHandle};

struct FixtureServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl FixtureServer {
    fn spawn() -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let address = listener.local_addr().map_err(|error| error.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let join = thread::spawn(move || {
            while !stop_thread.load(AtomicOrdering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => serve(stream),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            address,
            stop,
            join: Some(join),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.address, path)
    }

    fn stop_and_join(&mut self) -> Result<(), String> {
        self.stop.store(true, AtomicOrdering::SeqCst);
        if let Some(join) = self.join.take() {
            join.join()
                .map_err(|_| "WebView navigation fixture server panicked".to_string())?;
        }
        Ok(())
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        let _ = self.stop_and_join();
    }
}

fn serve(mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    let mut request = [0u8; 8192];
    let Ok(read) = stream.read(&mut request) else {
        return;
    };
    let first_line = String::from_utf8_lossy(&request[..read]);
    let path = first_line
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|path| path.split('?').next())
        .unwrap_or("/");
    let (status, body) = match path {
        "/a" => (
            "200 OK",
            "<!doctype html><meta charset=\"utf-8\"><title>Navigation A</title><main data-page=\"a\">A</main>",
        ),
        "/b" => (
            "200 OK",
            "<!doctype html><meta charset=\"utf-8\"><title>Navigation B</title><main data-page=\"b\">B</main>",
        ),
        _ => (
            "404 Not Found",
            "<!doctype html><meta charset=\"utf-8\"><title>Missing</title>",
        ),
    };
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(reply.as_bytes());
}

struct NavigationWorld {
    broker: Arc<crate::computer_use::ComputerUseBroker>,
    owner: String,
}

struct NavigationGrant {
    token: String,
    run_id: String,
}

impl NavigationWorld {
    fn new(live: Arc<HostOwnedWebView>) -> Result<Self, String> {
        let adapter = Arc::new(WebViewAdapter::new());
        adapter.install_scope_fixture(vec![live as Arc<dyn LiveWebView>]);
        let broker = Arc::new(crate::computer_use::ComputerUseBroker::request_endpoint(
            crate::computer_use::BrokerOptions {
                feature_enabled: false,
                lease_path: std::env::temp_dir().join(format!(
                    "cu-webview-navigation-probe-{}.lease",
                    uuid::Uuid::new_v4()
                )),
                ..Default::default()
            },
        ));
        broker
            .register_surface_adapter(crate::computer_use::SurfaceKind::WebView, adapter)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            broker,
            owner: format!("webview-navigation-probe-{}", uuid::Uuid::new_v4()),
        })
    }

    fn grant(&self) -> Result<NavigationGrant, String> {
        let connection = self
            .broker
            .reserve_transport_connection(&self.owner)
            .map_err(|error| error.to_string())?;
        let token = connection.token().to_string();
        self.broker
            .bind_transport_agent(&token, "webview-navigation-probe-agent")
            .map_err(|error| error.to_string())?;
        self.broker
            .submit_access(
                &token,
                serde_json::json!({
                    "action": "request",
                    "purpose": "real WebView2 navigation acceptance",
                    "scopeIntent": "the selected local fixture page only",
                    "surfaces": ["app-webview"],
                    "capabilities": ["observe", "input"]
                }),
            )
            .map_err(|error| error.to_string())?;
        let key = self
            .broker
            .host_task_view(&token)
            .map_err(|error| error.to_string())?
            .request
            .ok_or("WebView navigation access request was not published")?
            .key;
        let picker = self
            .broker
            .begin_task_scope_picker(&token, &key, crate::computer_use::SurfaceKind::WebView)
            .map_err(|error| error.to_string())?
            .discover()
            .map_err(|error| error.to_string())?;
        let candidates = picker.views().map_err(|error| error.to_string())?;
        if candidates.len() != 1 {
            return Err(format!(
                "WebView navigation fixture expected one candidate, got {}",
                candidates.len()
            ));
        }
        let selection = picker
            .select(&[candidates[0].candidate_id.clone()])
            .map_err(|error| error.to_string())?;
        let grok_computer_use_core::broker::AccessApprovalReady::Prepare(mut approval) = self
            .broker
            .reserve_selected_access_approval(&token, selection)
            .map_err(|error| error.to_string())?
        else {
            return Err("fresh WebView navigation grant skipped preparation".into());
        };
        approval
            .publish_feature()
            .map_err(|error| error.to_string())?;
        approval
            .prepare_execution()
            .map_err(|error| error.to_string())?;
        approval
            .prepare_publication()
            .map_err(|error| error.to_string())?;
        let outcome = approval
            .commit_for_transport(&token)
            .map_err(|error| error.to_string())?;
        Ok(NavigationGrant {
            token,
            run_id: outcome.run_id,
        })
    }
}

impl Drop for NavigationWorld {
    fn drop(&mut self) {
        self.broker.fence_task_consent(&self.owner);
        for ticket in self.broker.take_cleanup_tickets() {
            let _ = self.broker.finish_stop_cleanup(&ticket);
        }
    }
}

#[derive(Clone)]
struct Cursor {
    target_id: String,
    target_generation: u64,
}

fn args(
    run_id: &str,
    cursor: &Cursor,
    action_id: &str,
    kind: &str,
    url: Option<&str>,
) -> serde_json::Value {
    let mut value = serde_json::json!({
        "runId": run_id,
        "targetId": cursor.target_id,
        "targetGeneration": cursor.target_generation,
        "actionId": action_id,
        "kind": kind
    });
    if let Some(url) = url {
        value["url"] = serde_json::json!(url);
    }
    value
}

fn dispatch(
    world: &NavigationWorld,
    grant: &NavigationGrant,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    world
        .broker
        .preflight_tool_permission(&grant.token, "computer_navigate", &args)
        .map_err(|error| error.to_string())?;
    Ok(grok_computer_use_core::tools::dispatch_with_transport(
        &world.broker,
        &grant.token,
        "computer_navigate",
        args,
        None,
    ))
}

fn payload(result: &serde_json::Value) -> Result<serde_json::Value, String> {
    if result.get("isError").and_then(serde_json::Value::as_bool) != Some(false) {
        return Err(format!("WebView navigation unexpectedly failed: {result}"));
    }
    serde_json::from_str(
        result
            .pointer("/content/0/text")
            .and_then(serde_json::Value::as_str)
            .ok_or("WebView navigation result text missing")?,
    )
    .map_err(|error| error.to_string())
}

fn cursor(payload: &serde_json::Value) -> Result<Cursor, String> {
    Ok(Cursor {
        target_id: payload
            .get("targetId")
            .and_then(serde_json::Value::as_str)
            .ok_or("WebView navigation successor targetId missing")?
            .to_string(),
        target_generation: payload
            .get("targetGeneration")
            .and_then(serde_json::Value::as_u64)
            .ok_or("WebView navigation successor generation missing")?,
    })
}

fn expect_native_delta(
    live: &HostOwnedWebView,
    before: (u64, u64),
    delta: u64,
    label: &str,
) -> Result<(), String> {
    let after = live.navigation_counts();
    let expected = (before.0 + delta, before.1 + delta);
    if after != expected {
        return Err(format!(
            "{label} native navigation count mismatch: expected {expected:?}, got {after:?}"
        ));
    }
    Ok(())
}

fn expect_url(live: &HostOwnedWebView, expected: &str, label: &str) -> Result<(), String> {
    let actual = live.url()?;
    if actual != expected {
        return Err(format!("{label} landed at {actual}, expected {expected}"));
    }
    Ok(())
}

struct NavigationStep<'a> {
    action_id: &'a str,
    kind: &'a str,
    url: Option<&'a str>,
    expected_url: &'a str,
}

fn navigate_success(
    world: &NavigationWorld,
    grant: &NavigationGrant,
    live: &HostOwnedWebView,
    current: &Cursor,
    step: NavigationStep<'_>,
) -> Result<(Cursor, serde_json::Value), String> {
    let native_before = live.navigation_counts();
    let generation_before = live.navigation_generation();
    let result = dispatch(
        world,
        grant,
        args(&grant.run_id, current, step.action_id, step.kind, step.url),
    )?;
    let body = payload(&result)?;
    let next = cursor(&body)?;
    if next.target_id == current.target_id
        || next.target_generation != current.target_generation + 1
        || live.navigation_generation() != generation_before + 1
    {
        return Err(format!(
            "{} did not publish the exact successor document: {body}",
            step.kind
        ));
    }
    expect_native_delta(live, native_before, 1, step.kind)?;
    expect_url(live, step.expected_url, step.kind)?;
    Ok((next, result))
}

fn check(live: &Arc<HostOwnedWebView>, url_a: &str, url_b: &str) -> Result<(), String> {
    live.fixture_ready()?;
    let initial_counts = live.navigation_counts();
    if initial_counts.0 == 0 || initial_counts.0 != initial_counts.1 {
        return Err(format!(
            "initial native navigation was not balanced: {initial_counts:?}"
        ));
    }
    let world = NavigationWorld::new(live.clone())?;
    let grant = world.grant()?;
    let (target_id, target_generation) = world
        .broker
        .authorized_target(&grant.run_id)
        .map_err(|error| error.to_string())?;
    let initial = Cursor {
        target_id,
        target_generation,
    };

    let first_args = args(
        &grant.run_id,
        &initial,
        "native-url-navigation",
        "url",
        Some(url_b),
    );
    let before_url = live.navigation_counts();
    let native_generation_before_url = live.navigation_generation();
    let first = dispatch(&world, &grant, first_args.clone())?;
    let first_body = payload(&first)?;
    let mut current = cursor(&first_body)?;
    if current.target_id == initial.target_id
        || current.target_generation != initial.target_generation + 1
        || live.navigation_generation() != native_generation_before_url + 1
    {
        return Err(format!(
            "url did not publish the exact successor document: {first_body}"
        ));
    }
    expect_native_delta(live, before_url, 1, "url")?;
    expect_url(live, url_b, "url")?;

    let before_replay = live.navigation_counts();
    let replay = dispatch(&world, &grant, first_args)?;
    if replay != first {
        return Err("successful WebView navigation did not replay its exact result".into());
    }
    expect_native_delta(live, before_replay, 0, "successful replay")?;

    (current, _) = navigate_success(
        &world,
        &grant,
        live,
        &current,
        NavigationStep {
            action_id: "native-reload",
            kind: "reload",
            url: None,
            expected_url: url_b,
        },
    )?;
    (current, _) = navigate_success(
        &world,
        &grant,
        live,
        &current,
        NavigationStep {
            action_id: "native-back",
            kind: "back",
            url: None,
            expected_url: url_a,
        },
    )?;
    (current, _) = navigate_success(
        &world,
        &grant,
        live,
        &current,
        NavigationStep {
            action_id: "native-forward",
            kind: "forward",
            url: None,
            expected_url: url_b,
        },
    )?;

    let no_history_args = args(
        &grant.run_id,
        &current,
        "native-forward-no-history",
        "forward",
        None,
    );
    let no_history_counts = live.navigation_counts();
    let no_history_generation = live.navigation_generation();
    let no_history = dispatch(&world, &grant, no_history_args)?;
    let no_history_text = no_history
        .pointer("/content/0/text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if no_history
        .get("isError")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
        || !no_history_text.contains("history")
    {
        return Err(format!(
            "forward without history was not rejected before dispatch: {no_history}"
        ));
    }
    if live.navigation_generation() != no_history_generation {
        return Err("forward without history retired the native document".into());
    }
    expect_native_delta(live, no_history_counts, 0, "forward without history")?;

    live.make_next_navigation_unknown();
    let unknown_args = args(
        &grant.run_id,
        &current,
        "native-unknown-reload",
        "reload",
        None,
    );
    let unknown_counts = live.navigation_counts();
    let unknown_generation = live.navigation_generation();
    let unknown = dispatch(&world, &grant, unknown_args.clone())?;
    let unknown_text = unknown
        .pointer("/content/0/text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if unknown.get("isError").and_then(serde_json::Value::as_bool) != Some(true)
        || !unknown_text.contains("unknown")
    {
        return Err(format!(
            "native completion uncertainty was not preserved: {unknown}"
        ));
    }
    if live.navigation_generation() != unknown_generation + 1 {
        return Err("unknown navigation did not retire the dispatched document".into());
    }
    expect_native_delta(live, unknown_counts, 1, "unknown navigation")?;

    let before_unknown_replay = live.navigation_counts();
    let generation_before_replay = live.navigation_generation();
    let unknown_replay = dispatch(&world, &grant, unknown_args)?;
    if unknown_replay != unknown {
        return Err("unknown WebView navigation did not replay its exact terminal".into());
    }
    if live.navigation_generation() != generation_before_replay {
        return Err("unknown replay dispatched a second native navigation".into());
    }
    expect_native_delta(live, before_unknown_replay, 0, "unknown replay")?;

    println!(
        "gate: webview_navigation url=1 replay=0 reload=1 back=1 forward=1 no_history=0 unknown=1 unknown_replay=0 native_starts={} native_completions={}",
        live.navigation_counts().0,
        live.navigation_counts().1
    );
    Ok(())
}

pub(crate) fn run() -> Result<(), String> {
    let mut server = FixtureServer::spawn()?;
    let url_a = server.url("/a");
    let url_b = server.url("/b");
    let live = HostOwnedWebView::spawn("resource-browser-navigation", "navigation", &url_a)?;
    let result = check(&live, &url_a, &url_b);
    let result = live.finish_probe(result);
    let server_result = server.stop_and_join();
    result.and(server_result)
}

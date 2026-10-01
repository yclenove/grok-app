//! Bounded loopback HTTP client for the managed Playwright worker.

use std::time::Duration;

use serde_json::Value;

use super::observation::parse_worker_success;
use super::WorkerError;
use crate::execution::ActionCancellation;
use crate::ipc::host_is_loopback;
use crate::protocol::{OBSERVATION_JSON_BYTES, OBSERVATION_PNG_B64_CAP};

pub const WORKER_HTTP_MAX_BYTES: usize = OBSERVATION_PNG_B64_CAP + OBSERVATION_JSON_BYTES + 8192;

pub fn require_loopback_base(base: &str) -> Result<String, WorkerError> {
    let trimmed = base.trim();
    let without_slash = trimmed.trim_end_matches('/');
    let Some(rest) = without_slash
        .strip_prefix("http://")
        .or_else(|| without_slash.strip_prefix("HTTP://"))
    else {
        return Err(WorkerError::transport(
            "managed browser worker base must be loopback http",
        ));
    };
    if rest.contains('@') || rest.contains('?') || rest.contains('#') || rest.contains('/') {
        return Err(WorkerError::transport(
            "managed browser worker base must be loopback http",
        ));
    }
    if !host_is_loopback(rest) {
        return Err(WorkerError::transport(
            "managed browser worker base must be loopback http",
        ));
    }
    Ok(without_slash.to_string())
}

/// Keep the synchronous Host contract, but own the entire async HTTP runtime
/// off any Tokio worker (including its blocking pool). Join before returning;
/// neither cancellation nor timeout may detach an exchange or its runtime.
fn run_off_tokio<T, F>(f: F) -> T
where
    T: Send,
    F: FnOnce() -> T + Send,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        std::thread::scope(|scope| match scope.spawn(f).join() {
            Ok(value) => value,
            Err(payload) => std::panic::resume_unwind(payload),
        })
    } else {
        f()
    }
}

pub fn bounded_loopback_post(
    base: &str,
    token: &str,
    path: &str,
    body: &Value,
    extra_headers: &[(&str, &str)],
    timeout: Duration,
) -> Result<Value, WorkerError> {
    bounded_loopback_post_cancellable(
        base,
        token,
        path,
        body,
        extra_headers,
        timeout,
        &ActionCancellation::default(),
    )
}

/// Cancellation after dispatch is *unknown*, not remote quiescence. A caller
/// must keep its remote cleanup/idle fence until the worker confirms completion.
/// This function only guarantees that its own HTTP I/O has been dropped/joined.
pub fn bounded_loopback_post_cancellable(
    base: &str,
    token: &str,
    path: &str,
    body: &Value,
    extra_headers: &[(&str, &str)],
    timeout: Duration,
    cancellation: &ActionCancellation,
) -> Result<Value, WorkerError> {
    let base = require_loopback_base(base)?;
    if !path.starts_with('/') || path.contains("://") {
        return Err(WorkerError::transport(
            "managed browser worker path is invalid",
        ));
    }
    cancellation
        .check()
        .map_err(|_| cancelled_before_dispatch())?;
    run_off_tokio(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| WorkerError::transport("managed browser worker client failed"))?;
        // Runtime drop cancels/drains connection tasks and closes sockets before
        // the plain thread is joined. Never move this work into a detached task.
        runtime.block_on(async {
            cancellation.check().map_err(|_| cancelled_before_dispatch())?;
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => Err(WorkerError::unknown(
                    409, "worker_cancelled", "managed browser worker request cancelled; remote completion unknown",
                )),
                result = bounded_loopback_exchange(&base, token, path, body, extra_headers, timeout) => result,
            }
        })
    })
}

fn cancelled_before_dispatch() -> WorkerError {
    WorkerError::not_started(
        409,
        "worker_cancelled",
        "managed browser worker request cancelled before dispatch",
    )
}

#[cfg(test)]
#[path = "worker_http_cancel_tests.rs"]
mod cancellation_tests;

async fn bounded_loopback_exchange(
    base: &str,
    token: &str,
    path: &str,
    body: &Value,
    extra_headers: &[(&str, &str)],
    timeout: Duration,
) -> Result<Value, WorkerError> {
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        // Only localhost and literal loopback addresses are accepted above.
        // Static localhost resolution avoids a blocking system-DNS task that
        // could outlive cancellation and delay runtime shutdown.
        .resolve_to_addrs(
            "localhost",
            &[
                std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
                std::net::SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, 0)),
            ],
        )
        .build()
        .map_err(|_| WorkerError::transport("managed browser worker client failed"))?;
    let mut request = client
        .post(format!("{base}{path}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json")
        .json(body);
    for (key, value) in extra_headers {
        request = request.header(*key, *value);
    }
    let mut response = request.send().await.map_err(|error| {
        if error.is_timeout() {
            WorkerError::timeout("managed browser worker request timed out")
        } else if error.is_redirect() {
            WorkerError::invalid_response(error.status().map(|s| s.as_u16()).unwrap_or(0))
        } else {
            WorkerError::transport("managed browser worker transport failed")
        }
    })?;
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let mut bytes = Vec::new();
    if response
        .content_length()
        .is_some_and(|len| len > WORKER_HTTP_MAX_BYTES as u64)
    {
        return Err(WorkerError::invalid_response(status));
    }
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        if error.is_timeout() {
            WorkerError::timeout("managed browser worker response timed out")
        } else {
            WorkerError::invalid_response(status)
        }
    })? {
        if chunk.len() > WORKER_HTTP_MAX_BYTES.saturating_sub(bytes.len()) {
            return Err(WorkerError::invalid_response(status));
        }
        bytes.extend_from_slice(&chunk);
    }
    if !(200..300).contains(&status) {
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        return Err(super::parse_worker_error_response(status, &json));
    }
    parse_worker_success(status, content_type.as_deref(), &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn spawn_http(
        status_line: &str,
        headers: &str,
        body: &[u8],
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("addr");
        let status_line = status_line.to_string();
        let headers = headers.to_string();
        let body = body.to_vec();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let response = format!(
                "{status_line}\r\n{headers}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(&body);
        });
        (format!("http://127.0.0.1:{}", addr.port()), handle)
    }

    fn ok_body() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "ok": true,
            "pageId": "page-1",
            "pageGeneration": 1,
            "snapshotId": "snap-1",
            "nodes": []
        }))
        .unwrap()
    }

    #[test]
    fn require_loopback_base_rejects_non_loopback_and_userinfo() {
        assert!(require_loopback_base("http://127.0.0.1:9").is_ok());
        assert!(require_loopback_base("http://localhost:9/").is_ok());
        assert!(require_loopback_base("http://[::1]:9").is_ok());
        assert!(require_loopback_base("http://example.com:9").is_err());
        assert!(require_loopback_base("https://127.0.0.1:9").is_err());
        assert!(require_loopback_base("http://127.0.0.1:9/secret").is_err());
        assert!(require_loopback_base("http://user:pass@127.0.0.1:9").is_err());
        assert!(require_loopback_base("http://127.0.0.1:9?x=1").is_err());
        assert!(require_loopback_base("http://192.168.0.1:9").is_err());
    }

    #[test]
    fn bounded_client_rejects_non_json_oversize_and_redirects() {
        let (base, join) = spawn_http(
            "HTTP/1.1 200 OK",
            "Content-Type: text/plain",
            b"{\"ok\":true,\"pageId\":\"p\",\"pageGeneration\":1,\"snapshotId\":\"s\",\"nodes\":[]}",
        );
        let err = bounded_loopback_post(
            &base,
            "token",
            "/observe",
            &serde_json::json!({}),
            &[],
            Duration::from_secs(2),
        )
        .expect_err("text/plain");
        assert_eq!(err.code, "invalid_worker_response");
        let _ = join.join();

        let huge = vec![b'a'; WORKER_HTTP_MAX_BYTES + 8];
        let (base, join) = spawn_http("HTTP/1.1 200 OK", "Content-Type: application/json", &huge);
        let err = bounded_loopback_post(
            &base,
            "token",
            "/observe",
            &serde_json::json!({}),
            &[],
            Duration::from_secs(2),
        )
        .expect_err("oversize");
        assert_eq!(err.code, "invalid_worker_response");
        let _ = join.join();

        let trap = ok_body();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let location = format!("http://127.0.0.1:{}/trap", addr.port());
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let request = String::from_utf8_lossy(&buf);
            if request.contains(" /observe ") {
                let body = b"redirect-body";
                let response = format!(
                    "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(body);
            } else {
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    trap.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(&trap);
            }
        });
        let base = format!("http://127.0.0.1:{}", addr.port());
        let err = bounded_loopback_post(
            &base,
            "token",
            "/observe",
            &serde_json::json!({}),
            &[],
            Duration::from_secs(2),
        )
        .expect_err("redirect");
        assert!(
            err.code == "invalid_worker_response" || err.code == "worker_transport",
            "{}",
            err.code
        );
        let _ = handle.join();
    }

    #[test]
    fn bounded_client_ignores_http_proxy_for_loopback() {
        let (base, join) = spawn_http(
            "HTTP/1.1 200 OK",
            "Content-Type: application/json; charset=utf-8",
            &ok_body(),
        );
        let _guard = PROXY_LOCK.lock().expect("proxy lock");
        let previous = std::env::var("HTTP_PROXY").ok();
        std::env::set_var("HTTP_PROXY", "http://127.0.0.1:1");
        std::env::set_var("http_proxy", "http://127.0.0.1:1");
        let result = bounded_loopback_post(
            &base,
            "token",
            "/observe",
            &serde_json::json!({}),
            &[],
            Duration::from_secs(2),
        );
        if let Some(value) = previous {
            std::env::set_var("HTTP_PROXY", value);
        } else {
            std::env::remove_var("HTTP_PROXY");
        }
        std::env::remove_var("http_proxy");
        result.expect("no_proxy must still reach loopback");
        let _ = join.join();
    }

    static PROXY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
}

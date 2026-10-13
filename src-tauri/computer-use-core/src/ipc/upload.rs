//! The response body owns cancellation of its one original upload caller.
//! Heartbeats keep the existing MCP socket-idle budget, not a polling tool.
use super::*;
use crate::broker::{HostUploadCancel, WebViewUploadReady};
use axum::{
    body::{Body, Bytes},
    response::{IntoResponse, Response},
};
use http_body::{Body as HttpBody, Frame};
use std::{
    convert::Infallible,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

const HEARTBEAT: Duration = Duration::from_secs(4);
const WAIT_LIMIT: Duration = Duration::from_secs(600);

#[derive(Default)]
struct LifetimeState {
    closed: bool,
    cancellation: Option<HostUploadCancel>,
}
#[derive(Default)]
pub(super) struct UploadLifetime(Mutex<LifetimeState>);

impl UploadLifetime {
    fn closed(&self) -> bool {
        self.0.lock().closed
    }
    fn attach(&self, cancellation: HostUploadCancel) -> bool {
        let mut state = self.0.lock();
        if state.closed {
            return false;
        }
        state.cancellation = Some(cancellation);
        true
    }
    pub(super) fn close(&self) {
        let cancellation = {
            let mut state = self.0.lock();
            state.closed = true;
            state.cancellation.take()
        };
        if let Some(cancellation) = cancellation {
            cancellation.fence();
        }
    }
}

#[derive(Default)]
struct CallsState {
    closed: bool,
    calls: HashMap<String, Arc<UploadLifetime>>,
}
#[derive(Default)]
pub(super) struct UploadRegistry(Mutex<CallsState>);
pub(super) type UploadCalls = Arc<UploadRegistry>;
impl UploadRegistry {
    #[cfg(test)]
    pub(super) fn waiter_counts(&self) -> (usize, usize) {
        let state = self.0.lock();
        (
            state.calls.len(),
            state.calls.values().filter(|call| call.closed()).count(),
        )
    }
    fn register(self: &Arc<Self>, lifetime: Arc<UploadLifetime>) -> Option<OriginalWorker> {
        let mut state = self.0.lock();
        if state.closed {
            return None;
        }
        let key = uuid::Uuid::new_v4().to_string();
        state.calls.insert(key.clone(), lifetime.clone());
        Some(OriginalWorker {
            key,
            calls: self.clone(),
            lifetime,
        })
    }
    pub(super) fn close_all(&self) {
        let calls = {
            let mut state = self.0.lock();
            state.closed = true;
            std::mem::take(&mut state.calls)
        };
        for lifetime in calls.into_values() {
            lifetime.close();
        }
    }
}
struct OriginalWorker {
    key: String,
    calls: UploadCalls,
    lifetime: Arc<UploadLifetime>,
}
impl Drop for OriginalWorker {
    fn drop(&mut self) {
        self.lifetime.close();
        self.calls.0.lock().calls.remove(&self.key);
    }
}

struct UploadBody {
    result: oneshot::Receiver<Value>,
    heartbeat: tokio::time::Interval,
    deadline: Pin<Box<tokio::time::Sleep>>,
    lifetime: Arc<UploadLifetime>,
    done: bool,
}
impl Drop for UploadBody {
    fn drop(&mut self) {
        self.lifetime.close();
    }
}
impl HttpBody for UploadBody {
    type Data = Bytes;
    type Error = Infallible;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        if self.done {
            return Poll::Ready(None);
        }
        if let Poll::Ready(result) = Pin::new(&mut self.result).poll(cx) {
            self.done = true;
            let value = result.unwrap_or_else(|_| error("upload worker ended; outcome unknown"));
            return Poll::Ready(Some(Ok(Frame::data(Bytes::from(value.to_string())))));
        }
        if self.deadline.as_mut().poll(cx).is_ready() {
            self.lifetime.close();
        }
        if self.heartbeat.poll_tick(cx).is_ready() {
            return Poll::Ready(Some(Ok(Frame::data(Bytes::from_static(b" ")))));
        }
        Poll::Pending
    }
    fn is_end_stream(&self) -> bool {
        self.done
    }
}

pub(super) async fn call(
    State(service): State<Service>,
    headers: HeaderMap,
    Json(call): Json<ToolCall>,
) -> Response {
    if let Err(error) = tool_headers(&headers) {
        return error.into_response();
    }
    let token = bearer_token(&headers).unwrap_or("").to_string();
    let crate::task_consent::BearerClass::Live(ctx) = service.broker.classify_bearer(&token) else {
        forget_retired_rates(&service);
        return (
            StatusCode::UNAUTHORIZED,
            Json(error(
                "upload requires an original task transport credential",
            )),
        )
            .into_response();
    };
    if transport_session_override(&call, &ctx.app_session_id) {
        return (
            StatusCode::FORBIDDEN,
            Json(error("session/run does not match credential")),
        )
            .into_response();
    }
    if call.name != "computer_upload" {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("upload route accepts only computer_upload")),
        )
            .into_response();
    }
    let permit = match admit_tool(&service, &token, &call.name) {
        Ok(permit) => permit,
        Err(error) => return error.into_response(),
    };
    let lifetime = Arc::new(UploadLifetime::default());
    let Some(owner) = service.upload_calls.register(lifetime.clone()) else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error("upload listener is retiring")),
        )
            .into_response();
    };
    let (send, result) = oneshot::channel();
    // Both preparation and waiting stay under this original bounded worker.
    // Dropping an HTTP body fences it; neither a semaphore nor native picker
    // occupancy is released before its actual corresponding owner returns.
    tokio::task::spawn_blocking(move || {
        let _owner = owner;
        let _permit = permit;
        let value = if _owner.lifetime.closed() {
            error("original upload request cancelled")
        } else {
            match crate::tools_upload::prepare(
                &service.broker,
                &token,
                &call.arguments,
                |cancellation| _owner.lifetime.attach(cancellation),
            ) {
                Ok(WebViewUploadReady::Replay(completion)) => {
                    crate::tools_upload::response(completion)
                }
                Ok(WebViewUploadReady::Pending(call)) => match call.wait() {
                    Ok(completion) => crate::tools_upload::response(completion),
                    Err(error) => fail(&error),
                },
                Err(error) => fail(&error),
            }
        };
        forget_retired_rates(&service);
        let _ = send.send(value);
    });
    let body = UploadBody {
        result,
        lifetime,
        heartbeat: tokio::time::interval(HEARTBEAT),
        deadline: Box::pin(tokio::time::sleep(WAIT_LIMIT)),
        done: false,
    };
    let mut response = Body::new(body).into_response();
    response.headers_mut().insert(
        "content-type",
        "application/json".parse().expect("static content type"),
    );
    response.headers_mut().insert(
        "cache-control",
        "no-store".parse().expect("static cache policy"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[test]
    fn listener_registry_rejects_late_registration_after_shutdown() {
        let registry = Arc::new(UploadRegistry::default());
        let original = Arc::new(UploadLifetime::default());
        let owner = registry.register(original.clone()).unwrap();
        registry.close_all();
        assert!(original.closed());
        assert!(registry.register(Arc::default()).is_none());
        drop(owner);
        assert!(registry.0.lock().calls.is_empty());
    }

    #[tokio::test]
    async fn heartbeat_and_original_terminal_result_form_one_json_response() {
        let lifetime = Arc::new(UploadLifetime::default());
        let (send, result) = oneshot::channel();
        let mut body = UploadBody {
            result,
            lifetime: lifetime.clone(),
            heartbeat: tokio::time::interval(HEARTBEAT),
            deadline: Box::pin(tokio::time::sleep(WAIT_LIMIT)),
            done: false,
        };
        let first = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert_eq!(&first[..], b" ");
        assert!(!body.is_end_stream());
        send.send(json!({"original":"result"})).unwrap();
        let terminal = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&terminal).unwrap()["original"],
            "result"
        );
        assert!(body.is_end_stream());
        assert!(body.frame().await.is_none());
        drop(body);
        assert!(lifetime.closed());
    }

    #[tokio::test]
    async fn upload_deadline_retires_only_its_lifetime_without_fabricating_completion() {
        let lifetime = Arc::new(UploadLifetime::default());
        let unrelated = Arc::new(UploadLifetime::default());
        let (_send, result) = oneshot::channel();
        let mut body = UploadBody {
            result,
            lifetime: lifetime.clone(),
            heartbeat: tokio::time::interval(HEARTBEAT),
            deadline: Box::pin(tokio::time::sleep(Duration::from_millis(20))),
            done: false,
        };
        body.frame().await.unwrap().unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(80), body.frame())
                .await
                .is_err()
        );
        assert!(lifetime.closed());
        assert!(!unrelated.closed());
        assert!(!body.is_end_stream());
    }
}

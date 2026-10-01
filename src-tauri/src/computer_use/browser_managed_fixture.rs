//! Probe-only fixture HTTP: receive complete bounded bodies, never acknowledge
//! an incomplete oracle report or recursively serve the form inside its iframe.

use axum::extract::DefaultBodyLimit;
use axum::http::{header, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use std::net::TcpListener;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use tokio::sync::oneshot;

type FixtureThread = JoinHandle<Result<(), String>>;

#[derive(Deserialize)]
struct OracleReport {
    count: u64,
}

pub(super) fn spawn(
    listener: TcpListener,
    html: Vec<u8>,
    frame: Vec<u8>,
    oracle: Arc<AtomicU64>,
) -> Result<(oneshot::Sender<()>, FixtureThread), String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let (stop, stopped) = oneshot::channel();
    let thread = std::thread::spawn(move || {
        runtime.block_on(async move {
            let listener =
                tokio::net::TcpListener::from_std(listener).map_err(|e| e.to_string())?;
            let app = Router::new()
                .route(
                    "/",
                    get(move || {
                        let html = html.clone();
                        async move { ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html) }
                    }),
                )
                .route(
                    "/frame.html",
                    get(move || {
                        let frame = frame.clone();
                        async move { ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], frame) }
                    }),
                )
                .route(
                    "/oracle",
                    post(move |Json(report): Json<OracleReport>| {
                        let oracle = oracle.clone();
                        async move {
                            oracle.store(report.count, Ordering::SeqCst);
                            StatusCode::NO_CONTENT
                        }
                    }),
                )
                .layer(DefaultBodyLimit::max(4096));
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .map_err(|e| e.to_string())
        })
    });
    Ok((stop, thread))
}

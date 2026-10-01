//! Owned HTTP gates hold real navigation, native download and screenshot fonts.
use axum::{
    body::{Body, Bytes},
    http::header,
    routing::get,
    Router,
};
use std::{
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::sync::{oneshot, watch};

pub(super) struct Gate {
    pub entered: AtomicBool,
    pub finished: AtomicBool,
    pub hits: AtomicUsize,
    released: watch::Sender<bool>,
}

impl Gate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            hits: AtomicUsize::new(0),
            released: watch::channel(false).0,
        })
    }
    async fn wait(&self) {
        self.hits.fetch_add(1, Ordering::SeqCst);
        self.entered.store(true, Ordering::SeqCst);
        let _ = self.released.subscribe().wait_for(|ready| *ready).await;
    }
    pub fn release(&self) {
        self.released.send_replace(true);
    }
}

struct StreamClosed(Arc<Gate>);
impl Drop for StreamClosed {
    fn drop(&mut self) {
        self.0.finished.store(true, Ordering::SeqCst);
    }
}

pub(super) struct Fixture {
    pub base: String,
    pub click: Arc<Gate>,
    pub download: Arc<Gate>,
    pub pending_download: Arc<Gate>,
    pub screenshot: Arc<Gate>,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<Result<(), String>>>,
}

impl Fixture {
    pub fn start() -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let base = format!(
            "http://{}",
            listener.local_addr().map_err(|e| e.to_string())?
        );
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let click = Gate::new();
        let download = Gate::new();
        let pending_download = Gate::new();
        let screenshot = Gate::new();
        let slow_click = click.clone();
        let slow_download = download.clone();
        let slow_pending_download = pending_download.clone();
        let slow_font = screenshot.clone();
        let (stop, stopped) = oneshot::channel();
        let thread = std::thread::spawn(move || {
            runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).map_err(|e| e.to_string())?;
            let app = Router::new()
                .route("/click", get(|| async { ([(header::CONTENT_TYPE,"text/html")], "<a href='/landed'>Slow navigation</a>") }))
                .route("/landed", get(move || { let gate = slow_click.clone(); async move {
                    gate.wait().await;
                    gate.finished.store(true, Ordering::SeqCst);
                    ([(header::CONTENT_TYPE,"text/html")], "<button>Recovered page</button>")
                }}))
                .route("/download", get(|| async { ([(header::CONTENT_TYPE,"text/html")], "<a href='/slow.bin' download='native.bin'>Slow download</a>") }))
                .route("/slow.bin", get(move || { let gate = slow_download.clone(); async move {
                    held_download(gate, 2048)
                }}))
                .route("/download-pending", get(|| async { ([(header::CONTENT_TYPE,"text/html")], "<a href='/slow-pending.bin' download='native.bin'>Pending download</a>") }))
                .route("/slow-pending.bin", get(move || { let gate = slow_pending_download.clone(); async move {
                    held_download(gate, 5)
                }}))
                .route("/screenshot", get(|| async { ([(header::CONTENT_TYPE,"text/html")], "<style>@font-face{font-family:held;src:url('/held.woff2')}button{font-family:held}</style><button>Screenshot fixture</button>") }))
                .route("/held.woff2", get(move || { let gate = slow_font.clone(); async move {
                    gate.wait().await;
                    gate.finished.store(true, Ordering::SeqCst);
                    ([(header::CONTENT_TYPE,"font/woff2")], "fixture deliberately invalid font")
                }}));
            axum::serve(listener, app).with_graceful_shutdown(async { let _ = stopped.await; }).await.map_err(|e| e.to_string())
        })
        });
        Ok(Self {
            base,
            click,
            download,
            pending_download,
            screenshot,
            stop: Some(stop),
            thread: Some(thread),
        })
    }
    pub fn close(&mut self) -> Result<(), String> {
        self.click.release();
        self.download.release();
        self.pending_download.release();
        self.screenshot.release();
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().map_err(|_| "native fixture panicked")??;
        }
        Ok(())
    }
}

fn held_download(gate: Arc<Gate>, first_bytes: usize) -> impl axum::response::IntoResponse {
    let body = Body::from_stream(async_stream::stream! {
        let _closed = StreamClosed(gate.clone());
        yield Ok::<_, std::io::Error>(Bytes::from(vec![65; first_bytes]));
        gate.wait().await;
        yield Ok::<_, std::io::Error>(Bytes::from_static(b"end"));
    });
    (
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=native.bin".to_string(),
            ),
            (header::CONTENT_LENGTH, (first_bytes + 3).to_string()),
        ],
        body,
    )
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

//! Hold a real attachment after its first bytes until the test explicitly releases it.

use super::super::{http, write_response, write_status, DOWNLOAD_BYTES};
use parking_lot::{Condvar, Mutex};
use std::io::{self, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const BODY_BYTES: usize = 1024 * 1024;
const FIRST_BYTES: usize = 16 * 1024;

#[derive(Default)]
struct State {
    requested: bool,
    headers_released: bool,
    started: bool,
    released: bool,
    finished: bool,
}

#[derive(Default)]
pub(super) struct Transfer {
    hold_headers: bool,
    state: Mutex<State>,
    changed: Condvar,
}

impl Transfer {
    pub(super) fn wait_requested(&self) -> Result<(), String> {
        self.wait_for(|state| state.requested, "slow attachment original request")
    }

    pub(super) fn wait_started(&self) -> Result<(), String> {
        self.wait_for(|state| state.started, "slow attachment first bytes")
    }

    pub(super) fn wait_finished(&self) -> Result<(), String> {
        self.wait_for(|state| state.finished, "slow attachment worker retirement")
    }

    fn wait_for(&self, predicate: impl Fn(&State) -> bool, label: &str) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut state = self.state.lock();
        while !predicate(&state) {
            let now = Instant::now();
            if now >= deadline {
                return Err(format!("{label} missed its five-second deadline"));
            }
            self.changed.wait_for(&mut state, deadline - now);
        }
        Ok(())
    }

    pub(super) fn is_held(&self) -> bool {
        let state = self.state.lock();
        state.started && !state.released && !state.finished
    }

    pub(super) fn is_before_start_held(&self) -> bool {
        let state = self.state.lock();
        self.hold_headers
            && state.requested
            && !state.headers_released
            && !state.started
            && !state.finished
    }

    pub(super) fn release_headers(&self) {
        self.state.lock().headers_released = true;
        self.changed.notify_all();
    }

    pub(super) fn release(&self) {
        let mut state = self.state.lock();
        state.headers_released = true;
        state.released = true;
        drop(state);
        self.changed.notify_all();
    }

    fn send(&self, stream: &mut TcpStream) -> io::Result<()> {
        {
            let mut state = self.state.lock();
            state.requested = true;
            self.changed.notify_all();
            while self.hold_headers && !state.headers_released {
                self.changed.wait(&mut state);
            }
        }
        stream.write_all(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=slow.bin\r\nContent-Length: {BODY_BYTES}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n"
        ).as_bytes())?;
        stream.write_all(b"x")?;
        stream.write_all(&[b'x'; FIRST_BYTES - 1])?;
        stream.flush()?;
        {
            let mut state = self.state.lock();
            state.started = true;
            self.changed.notify_all();
            while !state.released {
                self.changed.wait(&mut state);
            }
        }
        let chunk = [b'x'; FIRST_BYTES];
        for _ in 1..BODY_BYTES / FIRST_BYTES {
            stream.write_all(&chunk)?;
        }
        Ok(())
    }
}

pub(super) struct Server {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    downloads: Arc<AtomicU64>,
    pub(super) transfer: Arc<Transfer>,
    join: Option<JoinHandle<()>>,
}

impl Server {
    pub(super) fn spawn() -> Result<Self, String> {
        Self::spawn_with(Arc::new(Transfer::default()))
    }

    pub(super) fn spawn_before_start() -> Result<Self, String> {
        Self::spawn_with(Arc::new(Transfer {
            hold_headers: true,
            ..Default::default()
        }))
    }

    fn spawn_with(transfer: Arc<Transfer>) -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
        let address = listener.local_addr().map_err(|error| error.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let downloads = Arc::new(AtomicU64::new(0));
        let stop_thread = stop.clone();
        let downloads_thread = downloads.clone();
        let transfer_thread = transfer.clone();
        let join = thread::spawn(move || {
            let mut workers = Vec::new();
            // A loopback connection wakes blocking accept on shutdown. No polling sleep.
            while let Ok((stream, _)) = listener.accept() {
                if stop_thread.load(Ordering::SeqCst) {
                    break;
                }
                let downloads = downloads_thread.clone();
                let transfer = transfer_thread.clone();
                workers.push(thread::spawn(move || serve(stream, &downloads, &transfer)));
            }
            transfer_thread.release();
            for worker in workers {
                let _ = worker.join();
            }
        });
        Ok(Self {
            address,
            stop,
            downloads,
            transfer,
            join: Some(join),
        })
    }

    pub(super) fn page_url(&self) -> String {
        format!("http://{}/page", self.address)
    }

    pub(super) fn downloads(&self) -> u64 {
        self.downloads.load(Ordering::SeqCst)
    }

    pub(super) fn stop_and_join(&mut self) -> Result<(), String> {
        self.transfer.release();
        if self.join.is_some() {
            self.stop.store(true, Ordering::SeqCst);
            TcpStream::connect_timeout(&self.address, Duration::from_secs(3))
                .map_err(|error| format!("slow fixture accept wake failed: {error}"))?;
            let join = self.join.take().expect("checked original server owner");
            join.join()
                .map_err(|_| "slow download fixture server panicked")?;
        }
        Ok(())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Err(error) = self.stop_and_join() {
            eprintln!("slow fixture cleanup unconfirmed: {error}");
        }
    }
}

fn serve(mut stream: TcpStream, downloads: &AtomicU64, transfer: &Transfer) {
    if stream.set_nonblocking(false).is_err()
        || stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .is_err()
        || stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .is_err()
    {
        return;
    }
    let Ok(path) = http::read_request_path(&mut stream) else {
        return;
    };
    match path.as_str() {
        "/page" => {
            let _ = write_response(
                &mut stream,
                "text/html; charset=utf-8",
                &[],
                concat!(
                    "<!doctype html><html lang=\"zh-CN\"><meta charset=\"utf-8\">",
                    "<title>Slow download lifecycle</title><body>",
                    "<a href=\"/slow.bin\" download=\"slow.bin\">下载慢速文件</a>",
                    "<a href=\"/fixture.bin\" download=\"fixture.bin\">下载测试文件</a>",
                    "</body></html>"
                )
                .as_bytes(),
            );
        }
        "/slow.bin" => {
            downloads.fetch_add(1, Ordering::SeqCst);
            let _ = transfer.send(&mut stream);
            transfer.state.lock().finished = true;
            transfer.changed.notify_all();
        }
        "/fixture.bin" => {
            downloads.fetch_add(1, Ordering::SeqCst);
            let _ = write_response(
                &mut stream,
                "application/octet-stream",
                &["Content-Disposition: attachment; filename=fixture.bin"],
                DOWNLOAD_BYTES,
            );
        }
        _ => {
            let _ = write_status(&mut stream, "404 Not Found");
        }
    }
}

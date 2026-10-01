//! Independent socket oracles: cancellation must close I/O before returning,
//! not abandon a detached blocking thread while it continues the exchange.

use super::*;
use crate::browser::WorkerCompletion;
use crate::execution::ActionCancellation;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

const DEADLINE: Duration = Duration::from_secs(4);
const CANCEL_BOUND: Duration = Duration::from_secs(1);

fn request(base: &str, cancellation: &ActionCancellation) -> Result<Value, WorkerError> {
    bounded_loopback_post_cancellable(
        base,
        "test-secret-do-not-echo",
        "/act",
        &serde_json::json!({}),
        &[],
        DEADLINE,
        cancellation,
    )
}

fn read_request(stream: &mut TcpStream) {
    stream.set_read_timeout(Some(DEADLINE)).unwrap();
    let mut request = Vec::new();
    let mut byte = [0];
    while !request.ends_with(b"\r\n\r\n") {
        stream
            .read_exact(&mut byte)
            .expect("complete request headers");
        request.push(byte[0]);
        assert!(request.len() < 8192);
    }
    let headers = String::from_utf8(request).unwrap();
    let len: usize = headers
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().unwrap())
        })
        .expect("JSON body length");
    assert!(len < 8192);
    stream.read_exact(&mut vec![0; len]).unwrap();
}

fn peer_closed(stream: &mut TcpStream) -> bool {
    stream.set_read_timeout(Some(CANCEL_BOUND)).unwrap();
    match stream.read(&mut [0u8; 1]) {
        Ok(0) => true,
        Err(error) => matches!(
            error.kind(),
            std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
        ),
        _ => false,
    }
}

#[test]
fn pre_cancel_never_opens_a_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let cancellation = ActionCancellation::default();
    cancellation.cancel();
    let started = Instant::now();
    let result = request(&base, &cancellation);
    let accepted = listener.accept().is_ok();
    assert!(!accepted, "cancelled request reached the listener");
    let error = result.expect_err("cancelled before dispatch");
    assert_eq!(error.code, "worker_cancelled");
    assert_eq!(error.completion, WorkerCompletion::NotStarted);
    assert!(started.elapsed() < CANCEL_BOUND);
    assert!(!error.to_string().contains("test-secret"));
}

fn cancel_pending_response(partial_body: bool) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (ready_tx, ready_rx) = mpsc::channel();
    let (closed_tx, closed_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        read_request(&mut stream);
        if partial_body {
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 99\r\n\r\n{\"ok\":").unwrap();
            stream.flush().unwrap();
        }
        ready_tx.send(()).unwrap();
        closed_tx.send(peer_closed(&mut stream)).unwrap();
    });
    let cancellation = ActionCancellation::default();
    let client_token = cancellation.clone();
    let client = thread::spawn(move || request(&base, &client_token));
    ready_rx
        .recv_timeout(DEADLINE)
        .expect("request admitted by server");
    let started = Instant::now();
    cancellation.cancel();
    let error = client.join().unwrap().expect_err("cancelled exchange");
    let closed = closed_rx.recv_timeout(DEADLINE).unwrap();
    server.join().unwrap();
    assert_eq!(error.code, "worker_cancelled");
    assert_eq!(error.completion, WorkerCompletion::Unknown);
    assert!(
        started.elapsed() < CANCEL_BOUND,
        "cancellation waited for request deadline"
    );
    assert!(
        closed,
        "transport returned without closing the outstanding socket"
    );
    assert!(!error.to_string().contains("test-secret"));
}

#[test]
fn cancellation_closes_request_waiting_for_headers() {
    cancel_pending_response(false);
}

#[test]
fn cancellation_closes_request_waiting_for_body() {
    cancel_pending_response(true);
}

fn ok_response(stream: &mut TcpStream) {
    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\n{\"ok\":true}");
}

#[test]
fn completion_racing_cancel_is_success_or_unknown_never_replayed() {
    for _ in 0..24 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let gate = std::sync::Arc::new(std::sync::Barrier::new(2));
        let (ready_tx, ready_rx) = mpsc::channel();
        let server_gate = gate.clone();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_request(&mut stream);
            ready_tx.send(()).unwrap();
            server_gate.wait();
            ok_response(&mut stream);
            assert!(peer_closed(&mut stream));
            listener.set_nonblocking(true).unwrap();
            assert!(
                matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
                "no retry connection"
            );
        });
        let token = ActionCancellation::default();
        let client_token = token.clone();
        let client = thread::spawn(move || request(&base, &client_token));
        ready_rx.recv_timeout(DEADLINE).unwrap();
        gate.wait();
        token.cancel();
        match client.join().unwrap() {
            Ok(value) => assert_eq!(value["ok"], true),
            Err(error) => {
                assert_eq!(error.code, "worker_cancelled");
                assert_eq!(error.completion, WorkerCompletion::Unknown);
            }
        }
        server.join().unwrap();
    }
}

#[test]
fn successful_completion_and_later_cancel_do_not_poison_a_fresh_token() {
    for late_cancel in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://localhost:{}", listener.local_addr().unwrap().port());
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_request(&mut stream);
            ok_response(&mut stream);
            assert!(peer_closed(&mut stream));
        });
        let token = ActionCancellation::default();
        assert_eq!(request(&base, &token).unwrap()["ok"], true);
        if late_cancel {
            token.cancel();
        }
        server.join().unwrap();
    }
}

#[test]
fn stalled_headers_and_body_timeout_are_unknown_and_close_the_socket() {
    for partial_body in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_request(&mut stream);
            if partial_body {
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 99\r\n\r\n{\"ok\":").unwrap();
            }
            assert!(peer_closed(&mut stream));
            listener.set_nonblocking(true).unwrap();
            assert!(
                matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
                "timeout must not retry"
            );
        });
        let error = bounded_loopback_post_cancellable(
            &base,
            "secret",
            "/act",
            &serde_json::json!({}),
            &[],
            Duration::from_millis(150),
            &ActionCancellation::default(),
        )
        .unwrap_err();
        server.join().unwrap();
        assert_eq!(error.code, "worker_timeout");
        assert_eq!(error.completion, WorkerCompletion::Unknown);
    }
}

#[test]
fn streamed_body_is_bounded_even_without_a_content_length() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        read_request(&mut stream);
        stream.set_write_timeout(Some(DEADLINE)).unwrap();
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
        let block = [b' '; 8192];
        for _ in 0..(WORKER_HTTP_MAX_BYTES / block.len() + 2) {
            if stream.write_all(&block).is_err() {
                break;
            }
        }
        assert!(peer_closed(&mut stream));
    });
    let error = request(&base, &ActionCancellation::default()).unwrap_err();
    server.join().unwrap();
    assert_eq!(error.code, "invalid_worker_response");
    assert_eq!(error.completion, WorkerCompletion::Unknown);
}

#[tokio::test(flavor = "current_thread")]
async fn cancellation_from_async_runtime_joins_its_off_runtime_thread() {
    cancel_pending_response(false);
    // Exercise the synchronous entry on this Tokio thread itself, not only a
    // plain thread launched by cancel_pending_response.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let cancellation = ActionCancellation::default();
    let server_token = cancellation.clone();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        read_request(&mut stream);
        server_token.cancel();
        assert!(peer_closed(&mut stream));
    });
    let error = request(&base, &cancellation).unwrap_err();
    server.join().unwrap();
    assert_eq!(error.code, "worker_cancelled");
    assert_eq!(error.completion, WorkerCompletion::Unknown);
}

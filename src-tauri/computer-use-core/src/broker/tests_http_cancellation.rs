//! Contract test, not a real Playwright acceptance: a stopped local socket is
//! never sufficient to declare that remote browser cleanup has completed.
use super::*;
use std::sync::mpsc;

fn consume_request(stream: &mut std::net::TcpStream, route: &str) {
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    let mut headers = Vec::new();
    while !headers.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        headers.push(byte[0]);
        assert!(headers.len() < 8192);
    }
    let headers = String::from_utf8(headers).unwrap();
    assert!(headers.starts_with(&format!("POST {route} ")));
    let len = headers
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap();
    assert!(len < 8192);
    stream.read_exact(&mut vec![0; len]).unwrap();
}

#[test]
fn cancelled_capture_does_not_ack_stop_before_remote_cleanup() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (entered_tx, entered_rx) = mpsc::channel();
    let (closed_tx, closed_rx) = mpsc::channel();
    let (cleanup_tx, cleanup_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut capture, _) = listener.accept().unwrap();
        consume_request(&mut capture, "/observe");
        entered_tx.send(()).unwrap();
        let closed = match capture.read(&mut [0]) {
            Ok(0) => true,
            Err(e) => matches!(
                e.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
            ),
            _ => false,
        };
        closed_tx.send(closed).unwrap();
        let (mut cleanup, _) = listener.accept().unwrap();
        consume_request(&mut cleanup, "/cancel-run");
        cleanup_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(4)).unwrap();
        cleanup.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}").unwrap();
    });
    let broker = Arc::new(ComputerUseBroker::new(
        Arc::new(FakeAdapter::new()),
        enabled_opts(400),
    ));
    broker.register_managed_browser_adapter().unwrap();
    broker.open_run("session", "run").unwrap();
    let root = bind_loopback(&broker, &base);
    let tab = broker
        .tabs()
        .open_managed_profile("session", "run", "p1")
        .unwrap();
    broker.authorize_target("run", &tab.tab_id).unwrap();
    let observe_broker = broker.clone();
    let capture = std::thread::spawn(move || observe_broker.observe("run"));
    entered_rx.recv_timeout(Duration::from_secs(4)).unwrap();
    let ticket = broker.fence_stop("run").unwrap().unwrap();
    assert!(capture.join().unwrap().is_err());
    assert!(
        closed_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        "capture socket closed"
    );
    assert_eq!(broker.stop_state("run").unwrap(), StopState::StopRequested);
    assert!(broker.stop_cleanup_pending("run").unwrap());
    let cleanup_broker = broker.clone();
    let cleanup = std::thread::spawn(move || cleanup_broker.finish_stop_cleanup(&ticket));
    cleanup_rx.recv_timeout(Duration::from_secs(4)).unwrap();
    assert_eq!(broker.stop_state("run").unwrap(), StopState::StopRequested);
    assert!(broker.observe("run").is_err());
    release_tx.send(()).unwrap();
    assert_eq!(cleanup.join().unwrap().unwrap(), StopState::Stopped);
    server.join().unwrap();
    assert!(broker.tabs().is_closed(&tab.tab_id));
    assert!(!broker.stop_cleanup_pending("run").unwrap());
    drop(broker);
    std::fs::remove_dir_all(root).unwrap();
}

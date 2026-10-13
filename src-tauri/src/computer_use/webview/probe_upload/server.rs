//! Independent bounded loopback receiver witnesses actual browser file bytes.

use parking_lot::Mutex;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

const PAGE: &str = r#"<!doctype html><meta charset="utf-8"><title>CU file fixture</title>
<label>上传文件 <input type="file" accept="*/*"></label>
<script>
globalThis.uploadEvents = {input:0,change:0,settled:0,error:null};
globalThis.uploadPage = crypto.randomUUID();
const input = document.querySelector('input');
input.addEventListener('input', () => uploadEvents.input++);
input.addEventListener('change', async () => {
  uploadEvents.change++;
  const file = input.files[0];
  try {
    const response = await fetch('/upload?name='+encodeURIComponent(file.name), {
      method:'POST', headers:{'Content-Type':file.type||'application/octet-stream'}, body:file
    });
    if (!response.ok) throw new Error('receiver rejected');
    uploadEvents.settled++;
  } catch { uploadEvents.error = 'upload failed'; }
});
</script>"#;

#[derive(Clone)]
pub(super) struct Received {
    pub(super) name: String,
    pub(super) mime: String,
    pub(super) bytes: Vec<u8>,
}

pub(super) struct Server {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    received: Arc<Mutex<Vec<Received>>>,
    failures: Arc<Mutex<Vec<String>>>,
    empty_connections: Arc<AtomicU64>,
    join: Option<JoinHandle<()>>,
}

impl Server {
    pub(super) fn spawn() -> Result<Self, String> {
        // Browser-safe range, independent of the machine's ephemeral range.
        let listener = (49152..=65535)
            .find_map(|port| TcpListener::bind(("127.0.0.1", port)).ok())
            .ok_or("upload fixture loopback listener unavailable")?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let address = listener.local_addr().map_err(|error| error.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let received = Arc::new(Mutex::new(Vec::new()));
        let failures = Arc::new(Mutex::new(Vec::new()));
        let empty_connections = Arc::new(AtomicU64::new(0));
        let worker_stop = stop.clone();
        let worker_received = received.clone();
        let worker_failures = failures.clone();
        let worker_empty = empty_connections.clone();
        let join = thread::spawn(move || {
            while !worker_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => match serve(&mut stream, &worker_received) {
                        Ok(false) => {
                            worker_empty.fetch_add(1, Ordering::SeqCst);
                        }
                        Ok(true) => {}
                        Err(error) => worker_failures.lock().push(error.to_string()),
                    },
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => {
                        worker_failures.lock().push(error.to_string());
                        break;
                    }
                }
            }
        });
        Ok(Self {
            address,
            stop,
            received,
            failures,
            empty_connections,
            join: Some(join),
        })
    }

    pub(super) fn url(&self) -> String {
        format!("http://{}/page", self.address)
    }
    pub(super) fn received(&self) -> Vec<Received> {
        self.received.lock().clone()
    }

    pub(super) fn empty_connections(&self) -> u64 {
        self.empty_connections.load(Ordering::SeqCst)
    }

    pub(super) fn finish(&mut self) -> Result<(), String> {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(join) = self.join.take() {
            join.join()
                .map_err(|_| "upload fixture receiver panicked")?;
        }
        if !self.failures.lock().is_empty() {
            return Err(format!(
                "upload fixture receiver failed: {:?}",
                self.failures.lock()
            ));
        }
        Ok(())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

fn invalid(detail: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, detail)
}

fn read_header(reader: &mut impl Read) -> std::io::Result<Option<String>> {
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        if header.len() >= 8192 {
            return Err(invalid("upload header too large"));
        }
        let mut byte = [0];
        match reader.read_exact(&mut byte) {
            Ok(()) => {}
            // Only zero bytes can be a speculative, unused browser socket.
            Err(error)
                if error.kind() == std::io::ErrorKind::UnexpectedEof && header.is_empty() =>
            {
                return Ok(None);
            }
            Err(error) => return Err(error),
        }
        header.push(byte[0]);
    }
    String::from_utf8(header)
        .map(Some)
        .map_err(|_| invalid("upload header invalid"))
}

fn serve(stream: &mut TcpStream, received: &Mutex<Vec<Received>>) -> std::io::Result<bool> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let Some(header) = read_header(stream)? else {
        return Ok(false);
    };
    let mut lines = header.lines();
    let request = lines.next().ok_or_else(|| invalid("request missing"))?;
    let mut parts = request.split_whitespace();
    let method = parts.next().ok_or_else(|| invalid("method missing"))?;
    let path = parts.next().ok_or_else(|| invalid("path missing"))?;
    if parts.next() != Some("HTTP/1.1") || parts.next().is_some() {
        return Err(invalid("request version invalid"));
    }
    let mut length = None;
    let mut mime = None;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| invalid("header invalid"))?;
        if name.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                return Err(invalid("duplicate content length"));
            }
            length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| invalid("length invalid"))?,
            );
        } else if name.eq_ignore_ascii_case("content-type") {
            if mime.is_some() {
                return Err(invalid("duplicate content type"));
            }
            mime = Some(value.trim().to_string());
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(invalid("fixture requires bounded content length"));
        }
    }
    if method == "GET" && path == "/page" {
        return response(
            stream,
            "200 OK",
            "text/html; charset=utf-8",
            PAGE.as_bytes(),
        )
        .map(|()| true);
    }
    if method == "POST" && path.starts_with("/upload?") {
        let length = length.ok_or_else(|| invalid("upload length missing"))?;
        if length > grok_computer_use_core::adapter::MAX_UPLOAD_BYTES || received.lock().len() >= 8
        {
            return Err(invalid("upload receiver bound exceeded"));
        }
        let url = tauri::Url::parse(&format!("http://127.0.0.1{path}"))
            .map_err(|_| invalid("upload URL invalid"))?;
        let parameters: Vec<_> = url.query_pairs().collect();
        if parameters.len() != 1 || parameters[0].0 != "name" {
            return Err(invalid("upload filename missing"));
        }
        let mut bytes = vec![0; length];
        stream.read_exact(&mut bytes)?;
        received.lock().push(Received {
            name: parameters[0].1.to_string(),
            mime: mime.ok_or_else(|| invalid("upload MIME missing"))?,
            bytes,
        });
        return response(stream, "200 OK", "text/plain", b"received").map(|()| true);
    }
    response(stream, "404 Not Found", "text/plain", b"not found").map(|()| true)
}

fn response(stream: &mut TcpStream, status: &str, mime: &str, bytes: &[u8]) -> std::io::Result<()> {
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", bytes.len())?;
    stream.write_all(bytes)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn only_an_empty_connection_is_not_a_truncated_http_request() {
        assert_eq!(read_header(&mut Cursor::new(b"")).unwrap(), None);
        for truncated in [
            b"G".as_slice(),
            b"GET /page HTTP/1.1\r\nHost: localhost\r\n",
        ] {
            assert_eq!(
                read_header(&mut Cursor::new(truncated)).unwrap_err().kind(),
                std::io::ErrorKind::UnexpectedEof
            );
        }
    }

    #[test]
    fn headers_are_bounded_and_do_not_consume_upload_body_bytes() {
        let mut request =
            Cursor::new(b"POST /upload?name=a HTTP/1.1\r\nContent-Length: 3\r\n\r\nabc");
        assert!(read_header(&mut request)
            .unwrap()
            .unwrap()
            .ends_with("\r\n\r\n"));
        let mut body = String::new();
        request.read_to_string(&mut body).unwrap();
        assert_eq!(body, "abc");
        assert_eq!(
            read_header(&mut Cursor::new(vec![b'x'; 8193]))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData
        );
    }
}

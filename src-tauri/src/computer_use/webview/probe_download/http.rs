//! Bounded HTTP header framing for the local download fixture.

use std::io::{self, Read};

const MAX_HEADER_BYTES: usize = 8192;

pub(super) fn read_request_path(reader: &mut impl Read) -> io::Result<String> {
    let mut header = [0; MAX_HEADER_BYTES];
    let mut used = 0;
    loop {
        if used == header.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "header too large",
            ));
        }
        match reader.read(&mut header[used..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "incomplete HTTP header",
                ));
            }
            Ok(bytes) => used += bytes,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
        // TCP reads may split anywhere, including inside the request line or
        // CRLF terminator. Closing with unread headers can reset the response.
        if header[..used].windows(4).any(|part| part == b"\r\n\r\n") {
            break;
        }
    }
    let request = std::str::from_utf8(&header[..used])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid HTTP header"))?;
    let mut parts = request
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace();
    let method = parts.next();
    let path = parts.next();
    let version = parts.next();
    if method != Some("GET")
        || !path.is_some_and(|path| path.starts_with('/'))
        || !matches!(version, Some("HTTP/1.0" | "HTTP/1.1"))
        || parts.next().is_some()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid request line",
        ));
    }
    Ok(path.unwrap().split('?').next().unwrap().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const REQUEST: &[u8] =
        b"GET /oversize.bin?round=1 HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";

    struct Segmented {
        bytes: Cursor<&'static [u8]>,
        chunk: usize,
    }

    impl Read for Segmented {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let len = buffer.len().min(self.chunk);
            self.bytes.read(&mut buffer[..len])
        }
    }

    #[test]
    fn split_headers_are_consumed_before_a_response_can_be_sent() {
        for chunk in 1..=REQUEST.len() {
            let mut reader = Segmented {
                bytes: Cursor::new(REQUEST),
                chunk,
            };
            assert_eq!(read_request_path(&mut reader).unwrap(), "/oversize.bin");
            assert_eq!(reader.bytes.position() as usize, REQUEST.len());
        }
    }

    #[test]
    fn a_complete_request_line_with_truncated_headers_is_rejected() {
        let mut request = Cursor::new(b"GET /page HTTP/1.1\r\nHost: 127.0.0.1\r\n");
        assert_eq!(
            read_request_path(&mut request).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn unterminated_headers_are_bounded() {
        let mut request = Cursor::new(vec![b'x'; MAX_HEADER_BYTES + 1]);
        assert_eq!(
            read_request_path(&mut request).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(request.position() as usize, MAX_HEADER_BYTES);
    }

    #[test]
    fn accepted_socket_sends_the_full_large_body_with_exact_content_length() {
        use std::io::Write;
        use std::net::TcpStream;
        use std::time::Duration;

        let mut server = super::super::FixtureServer::spawn().unwrap();
        let mut client = TcpStream::connect(server.address).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        client
            .write_all(b"GET /oversize.bin HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
            .unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).unwrap();
        server.stop_and_join().unwrap();
        let header_end = response
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .unwrap()
            + 4;
        let headers = std::str::from_utf8(&response[..header_end]).unwrap();
        let expected = super::super::OVERSIZE_BYTES;
        assert!(headers
            .lines()
            .any(|line| line == format!("Content-Length: {expected}")));
        assert_eq!(response.len() - header_end, expected as usize);
        assert!(response[header_end..].iter().all(|byte| *byte == b'x'));
        assert_eq!(server.oversize_stats(), (1, expected, 0));
    }
}

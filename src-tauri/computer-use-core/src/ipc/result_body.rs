//! Bound result buffering and rejection cleanup independently. Dropping a large
//! upload as soon as it crosses the JSON limit can reset HTTP/1 while the peer
//! is still writing, hiding the 413 response. Rejected bytes are never parsed.

use std::time::Duration;

use axum::body::Body;
use axum::http::StatusCode;
use http_body_util::BodyExt;

use crate::browser::extension_protocol::EXTENSION_RESULT_BYTES;

const UPLOAD_BUDGET: Duration = Duration::from_secs(2);
const MAX_FRAMES: usize = 4096;

#[derive(Debug, PartialEq)]
pub(super) struct Rejection {
    pub status: StatusCode,
    pub close: bool,
}

pub(super) async fn read(body: Body) -> Result<Vec<u8>, Rejection> {
    // Exactly one result admission owns both this bounded upload and decoding.
    // A slow or endless sender cannot monopolize it indefinitely.
    tokio::time::timeout(UPLOAD_BUDGET, collect(body, EXTENSION_RESULT_BYTES))
        .await
        .unwrap_or(Err(Rejection {
            status: StatusCode::REQUEST_TIMEOUT,
            close: true,
        }))
}

async fn collect(mut body: Body, limit: usize) -> Result<Vec<u8>, Rejection> {
    let mut buffered = Vec::with_capacity(limit);
    let mut received = 0usize;
    let mut oversized = false;
    // Consume at most one additional payload limit without retaining those
    // bytes. Beyond that cap the connection must close; reliable delivery of a
    // rejection is not promised to arbitrarily large or unending senders.
    let drain_cap = limit.saturating_mul(2);
    for _ in 0..MAX_FRAMES {
        let Some(frame) = body.frame().await else {
            return if oversized {
                Err(Rejection {
                    status: StatusCode::PAYLOAD_TOO_LARGE,
                    close: false,
                })
            } else {
                Ok(buffered)
            };
        };
        let frame = frame.map_err(|_| Rejection {
            status: StatusCode::BAD_REQUEST,
            close: true,
        })?;
        if let Ok(data) = frame.into_data() {
            received = received.saturating_add(data.len());
            if received > drain_cap {
                return Err(Rejection {
                    status: StatusCode::PAYLOAD_TOO_LARGE,
                    close: true,
                });
            }
            if received > limit {
                oversized = true;
                buffered.clear();
            } else if !oversized {
                buffered.extend_from_slice(&data);
            }
        }
    }
    Err(Rejection {
        status: StatusCode::PAYLOAD_TOO_LARGE,
        close: true,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::convert::Infallible;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::task::{Context, Poll};

    use axum::body::Bytes;
    use http_body::{Frame, SizeHint};

    use super::*;

    struct Frames {
        frames: VecDeque<Bytes>,
        polls: Arc<AtomicUsize>,
        pending: bool,
    }

    impl http_body::Body for Frames {
        type Data = Bytes;
        type Error = Infallible;

        fn poll_frame(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
            self.polls.fetch_add(1, Ordering::SeqCst);
            if self.pending {
                Poll::Pending
            } else {
                Poll::Ready(self.frames.pop_front().map(|frame| Ok(Frame::data(frame))))
            }
        }

        fn size_hint(&self) -> SizeHint {
            SizeHint::default() // No Content-Length shortcut: exercise chunked bodies.
        }
    }

    fn frames(chunks: Vec<Bytes>) -> (Body, Arc<AtomicUsize>) {
        let polls = Arc::new(AtomicUsize::new(0));
        let body = Body::new(Frames {
            frames: chunks.into(),
            polls: polls.clone(),
            pending: false,
        });
        (body, polls)
    }

    #[tokio::test]
    async fn exact_limit_is_accepted_without_truncation() {
        let (body, _) = frames(vec![
            Bytes::from_static(b"abcd"),
            Bytes::from_static(b"efgh"),
        ]);
        assert_eq!(collect(body, 8).await.unwrap(), b"abcdefgh");
    }

    #[tokio::test]
    async fn over_limit_is_drained_to_eof_but_never_accepted() {
        let (body, polls) = frames(vec![
            Bytes::from_static(b"abcdefgh"),
            Bytes::from_static(b"ijk"),
        ]);
        assert_eq!(
            collect(body, 8).await.unwrap_err(),
            Rejection {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                close: false
            }
        );
        assert_eq!(polls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn drain_cap_does_not_consume_unbounded_input() {
        let (body, polls) = frames(vec![
            Bytes::from_static(b"abcdefgh"),
            Bytes::from_static(b"ijklmnopq"),
            Bytes::from_static(b"unread"),
        ]);
        assert_eq!(
            collect(body, 8).await.unwrap_err(),
            Rejection {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                close: true
            }
        );
        assert_eq!(polls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn repeated_empty_frames_have_an_independent_bound() {
        let (body, polls) = frames(vec![Bytes::new(); MAX_FRAMES + 1]);
        assert!(collect(body, 8).await.unwrap_err().close);
        assert_eq!(polls.load(Ordering::SeqCst), MAX_FRAMES);
    }

    #[tokio::test]
    async fn stalled_upload_releases_its_reader_after_the_absolute_deadline() {
        let body = Body::new(Frames {
            frames: VecDeque::new(),
            polls: Arc::new(AtomicUsize::new(0)),
            pending: true,
        });
        assert_eq!(
            read(body).await.unwrap_err(),
            Rejection {
                status: StatusCode::REQUEST_TIMEOUT,
                close: true
            }
        );
    }
}

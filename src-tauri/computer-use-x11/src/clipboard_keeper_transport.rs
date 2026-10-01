//! Private, fixed-size metadata channel to the exact running executable.
//! Clipboard bytes travel only over the selection protocol, never argv/files.
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};

pub const FLAG: &str = "--computer-use-x11-clipboard-keeper-v1";
pub const SEED: &[u8; 8] = b"GCUKPR01";
pub const READY: &[u8; 8] = b"READY001";
pub const COMMIT: &[u8; 8] = b"COMMIT01";
pub const OWNED: &[u8; 8] = b"OWNED001";
pub const FAILED: &[u8; 8] = b"FAILED01";
pub type Frame = [u8; 32];

pub fn frame(tag: &[u8; 8], window: u32, time: u32, nonce: &[u8; 16]) -> Frame {
    let mut out = [0; 32];
    out[..8].copy_from_slice(tag);
    out[8..12].copy_from_slice(&window.to_le_bytes());
    out[12..16].copy_from_slice(&time.to_le_bytes());
    out[16..].copy_from_slice(nonce);
    out
}
pub fn number(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes.try_into().expect("fixed frame field"))
}

static LIVE: AtomicUsize = AtomicUsize::new(0);
struct Permit;
impl Permit {
    fn acquire() -> Result<Self, String> {
        LIVE.fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            (n < 8).then_some(n + 1)
        })
        .map_err(|_| "clipboard keeper process capacity is occupied")?;
        Ok(Self)
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Clone)]
pub struct ProcessWitness {
    pub pid: u32,
    finished: Arc<AtomicBool>,
}
impl ProcessWitness {
    pub fn finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }
}

pub struct Transport {
    socket: Option<UnixStream>,
    pending: Option<(Frame, usize)>,
    received: Frame,
    received_len: usize,
    pub process: ProcessWitness,
}
impl Transport {
    pub fn spawn(seed: Frame) -> Result<Self, String> {
        let permit = Permit::acquire()?;
        let (socket, child_socket) =
            UnixStream::pair().map_err(|_| "keeper socket creation failed")?;
        socket
            .set_nonblocking(true)
            .map_err(|_| "keeper socket setup failed")?;
        let stdin: OwnedFd = child_socket
            .try_clone()
            .map_err(|_| "keeper socket duplication failed")?
            .into();
        let stdout: OwnedFd = child_socket.into();
        let finished = Arc::new(AtomicBool::new(false));
        let state = finished.clone();
        let (tx, rx) = mpsc::sync_channel::<Child>(1);
        // Start the reaper before any child can exist. A spawn failure leaves
        // the receiver disconnected, so no fabricated process wait remains.
        std::thread::Builder::new()
            .name("cu-x11-keeper-reaper".into())
            .spawn(move || {
                let _permit = permit;
                if let Ok(mut child) = rx.recv() {
                    // Interrupted waits do not prove exit. Keep the actual handle.
                    loop {
                        match child.wait() {
                            Ok(_) => {
                                state.store(true, Ordering::Release);
                                break;
                            }
                            Err(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
                        }
                    }
                }
            })
            .map_err(|_| "keeper reaper could not start")?;
        let child = Command::new("/proc/self/exe")
            .arg(FLAG)
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map_err(|_| "exact executable keeper could not start")?;
        let pid = child.id();
        if let Err(error) = tx.send(child) {
            // Bootstrap has not been sent; this child cannot own a selection.
            let mut child = error.0;
            let _ = child.kill();
            let _ = child.wait();
            return Err("keeper reaper did not accept process ownership".into());
        }
        Ok(Self {
            socket: Some(socket),
            pending: Some((seed, 0)),
            received: [0; 32],
            received_len: 0,
            process: ProcessWitness { pid, finished },
        })
    }
    pub fn queue(&mut self, frame: Frame) -> Result<(), String> {
        if self.pending.is_some() {
            return Err("keeper control write already pending".into());
        }
        self.pending = Some((frame, 0));
        Ok(())
    }
    // Keep the original frame/offset across observer deadlines. A completed
    // control write is never queued again, including during late-reply recovery.
    pub(super) fn flush_pending(&mut self) -> Result<bool, String> {
        let socket = self
            .socket
            .as_mut()
            .ok_or("keeper control channel closed")?;
        if let Some((frame, offset)) = self.pending.as_mut() {
            match socket.write(&frame[*offset..]) {
                Ok(0) => return Err("keeper control write closed".into()),
                Ok(n) => {
                    *offset += n;
                    if *offset == 32 {
                        self.pending = None;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => (),
                Err(_) => return Err("keeper control write failed".into()),
            }
        }
        Ok(self.pending.is_none())
    }
    pub fn poll(&mut self) -> Result<Option<Frame>, String> {
        self.flush_pending()?;
        self.read_reply()
    }
    /// Recovery reads only the original connection. A partial/unsent COMMIT
    /// must NOT be completed merely because an observer deadline expired.
    pub(super) fn read_reply(&mut self) -> Result<Option<Frame>, String> {
        let socket = self
            .socket
            .as_mut()
            .ok_or("keeper control channel closed")?;
        match socket.read(&mut self.received[self.received_len..]) {
            Ok(0) => Err("keeper control read closed".into()),
            Ok(n) => {
                self.received_len += n;
                if self.received_len == 32 {
                    self.received_len = 0;
                    Ok(Some(self.received))
                } else {
                    Ok(None)
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                Ok(None)
            }
            Err(_) => Err("keeper control read failed".into()),
        }
    }
    pub fn disconnect(&mut self) {
        self.socket.take();
        self.pending.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_frame_has_fixed_layout_without_payload() {
        let f = frame(SEED, 0x10203040, 0x50607080, &[9; 16]);
        assert_eq!(&f[..8], SEED);
        assert_eq!(number(&f[8..12]), 0x10203040);
        assert_eq!(number(&f[12..16]), 0x50607080);
        assert_eq!(&f[16..], &[9; 16]);
    }
    #[test]
    fn fragmented_control_frames_stay_bounded_and_eof_is_not_acknowledgement() {
        let (socket, mut peer) = UnixStream::pair().unwrap();
        socket.set_nonblocking(true).unwrap();
        let mut transport = Transport {
            socket: Some(socket),
            pending: None,
            received: [0; 32],
            received_len: 0,
            process: ProcessWitness {
                pid: 0,
                finished: Arc::new(AtomicBool::new(false)),
            },
        };
        let value = frame(READY, 11, 0, &[7; 16]);
        peer.write_all(&value[..7]).unwrap();
        assert!(transport.poll().unwrap().is_none());
        assert_eq!(transport.received_len, 7);
        peer.write_all(&value[7..]).unwrap();
        assert_eq!(transport.poll().unwrap(), Some(value));
        assert_eq!(transport.received_len, 0);
        transport.queue(frame(COMMIT, 11, 0, &[7; 16])).unwrap();
        assert!(transport.queue(value).is_err());
        assert!(transport.poll().unwrap().is_none());
        let mut received = [0; 32];
        peer.read_exact(&mut received).unwrap();
        assert_eq!(received, frame(COMMIT, 11, 0, &[7; 16]));
        drop(peer);
        assert!(transport.poll().is_err());
        assert!(!transport.process.finished());
    }
    #[test]
    fn late_reply_reads_never_flush_unsent_or_partial_commit() {
        for offset in [0, 11] {
            let (socket, mut peer) = UnixStream::pair().unwrap();
            socket.set_nonblocking(true).unwrap();
            peer.set_nonblocking(true).unwrap();
            let commit = frame(COMMIT, 11, 0, &[7; 16]);
            let mut transport = Transport {
                socket: Some(socket),
                pending: Some((commit, offset)),
                received: [0; 32],
                received_len: 0,
                process: ProcessWitness {
                    pid: 0,
                    finished: Arc::new(AtomicBool::new(false)),
                },
            };
            if offset != 0 {
                transport
                    .socket
                    .as_mut()
                    .unwrap()
                    .write_all(&commit[..offset])
                    .unwrap();
                let mut prefix = [0; 11];
                peer.read_exact(&mut prefix).unwrap();
                assert_eq!(prefix, commit[..offset]);
            }
            let reply = frame(OWNED, 11, 17, &[7; 16]);
            peer.write_all(&reply[..9]).unwrap();
            assert!(transport.read_reply().unwrap().is_none());
            peer.write_all(&reply[9..]).unwrap();
            assert_eq!(transport.read_reply().unwrap(), Some(reply));
            assert_eq!(
                peer.read(&mut [0; 32]).unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock
            );
            assert_eq!(transport.pending, Some((commit, offset)));
            transport.disconnect();
            assert_eq!(peer.read(&mut [0; 32]).unwrap(), 0);
        }
    }

    #[test]
    fn process_admission_counts_live_children_not_control_handles() {
        let mut permits: Vec<_> = (0..8).map(|_| Permit::acquire().unwrap()).collect();
        assert!(Permit::acquire().is_err());
        drop(permits.pop());
        let replacement = Permit::acquire().unwrap();
        assert!(Permit::acquire().is_err());
        drop(replacement);
        drop(permits);
        assert_eq!(LIVE.load(Ordering::Acquire), 0);
    }
}

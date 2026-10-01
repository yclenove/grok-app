//! Child side: materialize first, wait for the parent's exact commit, then
//! atomically claim only the same watched source epoch. EOF never causes a claim.
use super::*;
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;

pub(super) fn run() -> Result<(), String> {
    // Only an inherited private socket is accepted, not a terminal, file or
    // named listener. No sensitive payload is stored in this metadata channel.
    let fd = std::io::stdin()
        .as_fd()
        .try_clone_to_owned()
        .map_err(|_| "keeper control unavailable")?;
    let mut channel = UnixStream::from(fd);
    channel
        .peer_addr()
        .map_err(|_| "keeper control is not a private socket")?;
    channel
        .set_read_timeout(Some(DEADLINE))
        .map_err(|_| "keeper deadline unavailable")?;
    channel
        .set_write_timeout(Some(DEADLINE))
        .map_err(|_| "keeper deadline unavailable")?;
    let mut seed = [0; 32];
    channel
        .read_exact(&mut seed)
        .map_err(|_| "keeper bootstrap incomplete")?;
    let source = number(&seed[8..12]);
    let timestamp = number(&seed[12..16]);
    let nonce: [u8; 16] = seed[16..].try_into().expect("fixed nonce");
    if &seed[..8] != SEED || source == 0 || timestamp == 0 || nonce == [0; 16] {
        return Err("keeper bootstrap invalid".into());
    }
    let mut reader = ClipboardReader::connect()?;
    let (current, epoch) = reader.identity()?;
    if current != source {
        return Err("keeper source changed before snapshot".into());
    }
    require_proof(&reader.conn, source, &nonce)?;
    let time_atom = atom(&reader.conn, b"TIMESTAMP")?;
    let acquired = reader.convert(
        time_atom,
        4,
        Instant::now() + SNAPSHOT_DEADLINE,
        &mut || Ok(()),
    )?;
    if acquired.kind != u32::from(AtomEnum::INTEGER)
        || acquired.format != 32
        || acquired.bytes.len() != 4
        || u32::from_ne_bytes(
            acquired
                .bytes
                .as_slice()
                .try_into()
                .expect("timestamp length"),
        ) != timestamp
    {
        return Err("keeper source timestamp mismatch".into());
    }
    let snapshot = reader.snapshot(|| Ok(()))?;
    if snapshot.owner != source || snapshot.epoch != epoch {
        return Err("keeper source epoch changed during snapshot".into());
    }
    let (mut owner, _lease) = ClipboardOwner::new(reader, snapshot)?;
    write_proof(&owner.reader.conn, owner.window, &nonce)?;
    channel
        .write_all(&frame(READY, owner.window, 0, &nonce))
        .map_err(|_| "keeper preparation reply failed")?;
    let mut commit = [0; 32];
    channel
        .read_exact(&mut commit)
        .map_err(|_| "keeper commit absent; no ownership taken")?;
    if commit != frame(COMMIT, owner.window, 0, &nonce) {
        return Err("keeper commit mismatch".into());
    }
    let result = owner.claim_saved_original();
    let tag = if result.is_ok() { OWNED } else { FAILED };
    // A lost parent/acknowledgement is never permission to drop owned data.
    let _ = channel.write_all(&frame(tag, owner.window, owner.acquired, &nonce));
    drop(channel);
    loop {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| owner.try_close())) {
            Ok(Ok(true)) => return Ok(()),
            Ok(Err(_)) if connection_closed(&owner.reader.conn) => {
                return Err("keeper X connection terminated".into())
            }
            Ok(_) => std::thread::sleep(Duration::from_millis(5)),
            Err(_) => loop {
                std::thread::park_timeout(Duration::from_secs(60));
            },
        }
    }
}

fn terminal_io(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::NotConnected
    )
}
fn connection_closed(conn: &RustConnection) -> bool {
    let result = match conn.get_input_focus() {
        Ok(cookie) => cookie.reply().map(|_| ()),
        Err(error) => Err(x11rb::errors::ReplyError::ConnectionError(error)),
    };
    matches!(result, Err(x11rb::errors::ReplyError::ConnectionError(x11rb::errors::ConnectionError::IoError(error))) if terminal_io(&error))
}

impl ClipboardOwner {
    fn claim_saved_original(&mut self) -> Result<(), String> {
        let timestamp = self.server_time()?;
        self.fenced(|this| {
            let current = this.barrier()?;
            if this.state != ClipboardState::Prepared
                || current != this.saved.owner
                || this.reader.epoch != this.saved.epoch
            {
                return Err("keeper source changed before atomic takeover".into());
            }
            this.install(
                this.window,
                timestamp,
                Source::Saved(this.saved.clone()),
                ClipboardState::Restored,
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_terminal_transport_evidence_allows_disconnected_keeper_exit() {
        use std::io::ErrorKind as K;
        for kind in [
            K::UnexpectedEof,
            K::BrokenPipe,
            K::ConnectionReset,
            K::ConnectionAborted,
            K::NotConnected,
        ] {
            assert!(terminal_io(&std::io::Error::from(kind)));
        }
        for kind in [
            K::Interrupted,
            K::WouldBlock,
            K::TimedOut,
            K::Other,
            K::InvalidData,
        ] {
            assert!(!terminal_io(&std::io::Error::from(kind)));
        }
    }
}

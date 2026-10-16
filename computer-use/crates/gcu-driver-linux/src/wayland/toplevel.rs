//! Read-only `zwlr_foreign_toplevel_manager_v1` inventory (protocol versions 1–3).
//!
//! The protocol carries title/app-id, not peer PID or executable identity. Every
//! result is observation-only. Activation/input never falls back to a title match.

use super::{windows::*, Compositor};
use async_trait::async_trait;
use std::{path::PathBuf, sync::Arc};

/// Protocol transport boundary; deterministic fakes need no Unix socket or desktop.
#[async_trait]
pub trait ForeignToplevelSource: Send + Sync {
    /// Enumerate one coherent inventory. Must bound protocol work and reject missing globals.
    async fn inventory(&self) -> Result<WindowSnapshot, IdentityError>;
}

/// wlroots inventory adapter. The default `activate` implementation refuses mutation.
pub struct ToplevelBackend(Arc<dyn ForeignToplevelSource>);
impl ToplevelBackend {
    /// Construct without opening a connection.
    pub fn new(source: Arc<dyn ForeignToplevelSource>) -> Self {
        Self(source)
    }
}
#[async_trait]
impl WindowBackend for ToplevelBackend {
    fn compositor(&self) -> Compositor {
        Compositor::Wlroots
    }
    async fn snapshot(&self) -> Result<WindowSnapshot, IdentityError> {
        let mut snapshot = tokio::time::timeout(IDENTITY_TIMEOUT, self.0.inventory())
            .await
            .map_err(|_| IdentityError::Timeout {
                maybe_executed: false,
            })??;
        // Neither an injected PID nor app-id from this protocol can mint authority.
        for window in &mut snapshot.windows {
            window.pid = None;
            window.peer_pid = false;
        }
        Ok(snapshot)
    }
}

/// Native read-only Wayland transport. Uses two display sync barriers and then
/// closes the socket; no retained worker, event loop, FD passing or unsafe FFI.
pub struct NativeToplevelSource {
    socket: PathBuf,
}
impl NativeToplevelSource {
    /// Use the session resolver's exact socket path; never discovers a similarly named socket.
    pub fn new(socket: PathBuf) -> Self {
        Self { socket }
    }
}
#[async_trait]
impl ForeignToplevelSource for NativeToplevelSource {
    async fn inventory(&self) -> Result<WindowSnapshot, IdentityError> {
        #[cfg(target_os = "linux")]
        {
            tokio::time::timeout(IDENTITY_TIMEOUT, wire::inventory(&self.socket))
                .await
                .map_err(|_| IdentityError::Timeout {
                    maybe_executed: false,
                })?
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = &self.socket;
            Err(IdentityError::Unsupported)
        }
    }
}

#[cfg(any(target_os = "linux", test))]
mod wire {
    use super::*;
    #[cfg(target_os = "linux")]
    use std::sync::atomic::{AtomicU64, Ordering};
    #[cfg(target_os = "linux")]
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::UnixStream,
    };

    const MANAGER: &str = "zwlr_foreign_toplevel_manager_v1";
    #[cfg(target_os = "linux")]
    static CONNECTION: AtomicU64 = AtomicU64::new(1);

    fn malformed() -> IdentityError {
        IdentityError::Transport {
            maybe_executed: false,
        }
    }

    struct Args<'a>(&'a [u8]);
    impl<'a> Args<'a> {
        fn uint(&mut self) -> Result<u32, IdentityError> {
            let bytes: [u8; 4] = self
                .0
                .get(..4)
                .ok_or_else(malformed)?
                .try_into()
                .map_err(|_| malformed())?;
            self.0 = &self.0[4..];
            Ok(u32::from_ne_bytes(bytes))
        }
        fn string(&mut self) -> Result<String, IdentityError> {
            let size = self.uint()? as usize;
            if size == 0 || size > 16385 {
                return Err(malformed());
            }
            let padded = (size + 3) & !3;
            let bytes = self.0.get(..padded).ok_or_else(malformed)?;
            if bytes[size - 1] != 0 || bytes[..size - 1].contains(&0) {
                return Err(malformed());
            }
            let text = std::str::from_utf8(&bytes[..size - 1])
                .map_err(|_| malformed())?
                .to_owned();
            self.0 = &self.0[padded..];
            Ok(text)
        }
    }

    fn string_arg(text: &str) -> Vec<u8> {
        let mut data = ((text.len() + 1) as u32).to_ne_bytes().to_vec();
        data.extend_from_slice(text.as_bytes());
        data.push(0);
        while !data.len().is_multiple_of(4) {
            data.push(0);
        }
        data
    }

    struct Inventory {
        version: Option<u32>,
        windows: std::collections::BTreeMap<u32, WindowRecord>,
        pending: std::collections::BTreeMap<u32, WindowRecord>,
        next_generation: u64,
    }
    impl Inventory {
        fn new() -> Self {
            Self {
                version: None,
                windows: Default::default(),
                pending: Default::default(),
                next_generation: 1,
            }
        }
        fn event(
            &mut self,
            object: u32,
            opcode: u16,
            payload: &[u8],
        ) -> Result<Option<u32>, IdentityError> {
            let mut args = Args(payload);
            match (object, opcode) {
                (1, 0) => return Err(malformed()), // wl_display.error
                (2, 0) => {
                    let name = args.uint()?;
                    let interface = args.string()?;
                    let version = args.uint()?;
                    if interface == MANAGER {
                        if self.version.is_some() || version == 0 {
                            return Err(malformed());
                        }
                        self.version = Some(version.min(3));
                        return Ok(Some(name));
                    }
                }
                (4, 0) => {
                    let id = args.uint()?;
                    // Server-allocated new_id range, not a client object/callback alias.
                    if id < 0xff00_0000
                        || self.pending.contains_key(&id)
                        || self.pending.len() >= 4096
                    {
                        return Err(malformed());
                    }
                    let generation = self.next_generation;
                    self.next_generation += 1;
                    self.pending.insert(
                        id,
                        WindowRecord {
                            id: id.to_string(),
                            generation,
                            title: String::new(),
                            app_id: String::new(),
                            pid: None,
                            peer_pid: false,
                        },
                    );
                }
                (4, 1) => return Err(IdentityError::Unsupported), // manager finished
                _ => {
                    if let Some(window) = self.pending.get_mut(&object) {
                        match opcode {
                            0 => window.title = args.string()?,
                            1 => window.app_id = args.string()?,
                            5 => {
                                self.windows.insert(object, window.clone());
                            } // done: publish atomically
                            6 => {
                                self.pending.remove(&object);
                                self.windows.remove(&object);
                            } // closed
                            2 | 3 | 4 | 7 => {} // output/state/parent do not prove identity
                            _ => return Err(malformed()),
                        }
                    }
                }
            }
            Ok(None)
        }
    }

    #[cfg(target_os = "linux")]
    async fn request(
        socket: &mut UnixStream,
        object: u32,
        opcode: u16,
        payload: &[u8],
    ) -> Result<(), IdentityError> {
        let size = 8 + payload.len();
        if size > u16::MAX as usize || !size.is_multiple_of(4) {
            return Err(malformed());
        }
        let mut message = object.to_ne_bytes().to_vec();
        message.extend_from_slice(&(((size as u32) << 16) | opcode as u32).to_ne_bytes());
        message.extend_from_slice(payload);
        socket.write_all(&message).await.map_err(|_| malformed())
    }

    #[cfg(target_os = "linux")]
    pub(super) async fn inventory(path: &std::path::Path) -> Result<WindowSnapshot, IdentityError> {
        let mut socket = UnixStream::connect(path).await.map_err(|_| malformed())?;
        let epoch = CONNECTION.fetch_add(1, Ordering::Relaxed);
        if epoch == u64::MAX {
            return Err(IdentityError::Handoff);
        }
        request(&mut socket, 1, 1, &2u32.to_ne_bytes()).await?; // get_registry
        request(&mut socket, 1, 0, &3u32.to_ne_bytes()).await?; // sync registry
        let mut state = Inventory::new();
        let mut barrier = 3;
        let mut bytes_read = 0usize;
        loop {
            let mut header = [0; 8];
            socket
                .read_exact(&mut header)
                .await
                .map_err(|_| malformed())?;
            let object = u32::from_ne_bytes(header[..4].try_into().map_err(|_| malformed())?);
            let word = u32::from_ne_bytes(header[4..].try_into().map_err(|_| malformed())?);
            let size = (word >> 16) as usize;
            let opcode = word as u16;
            if size < 8 || !size.is_multiple_of(4) {
                return Err(malformed());
            }
            bytes_read += size;
            if bytes_read > 8 * 1024 * 1024 {
                return Err(malformed());
            }
            let mut payload = vec![0; size - 8];
            socket
                .read_exact(&mut payload)
                .await
                .map_err(|_| malformed())?;
            if object == barrier && opcode == 0 {
                if barrier == 6 {
                    break;
                }
                if state.version.is_none() {
                    return Err(IdentityError::Unsupported);
                }
                request(&mut socket, 1, 0, &6u32.to_ne_bytes()).await?;
                barrier = 6;
                continue;
            }
            if let Some(name) = state.event(object, opcode, &payload)? {
                let mut args = name.to_ne_bytes().to_vec();
                args.extend(string_arg(MANAGER));
                args.extend_from_slice(&state.version.ok_or_else(malformed)?.to_ne_bytes());
                args.extend_from_slice(&4u32.to_ne_bytes());
                request(&mut socket, 2, 0, &args).await?; // registry.bind
            }
        }
        Ok(WindowSnapshot {
            compositor: Compositor::Wlroots,
            epoch: format!("foreign-toplevel/{epoch}"),
            windows: state.windows.into_values().collect(),
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn protocol_done_and_closed_publish_only_live_coherent_windows() {
            let mut state = Inventory::new();
            let id = 0xff00_0001u32;
            state.event(4, 0, &id.to_ne_bytes()).unwrap();
            state.event(id, 0, &string_arg("Fixture")).unwrap();
            assert!(state.windows.is_empty());
            state.event(id, 5, &[]).unwrap();
            assert_eq!(state.windows[&id].title, "Fixture");
            assert!(!state.windows[&id].peer_pid);
            state.event(id, 6, &[]).unwrap();
            assert!(state.windows.is_empty());
            state.event(4, 0, &id.to_ne_bytes()).unwrap();
            state.event(id, 5, &[]).unwrap();
            assert_eq!(state.windows[&id].generation, 2);
        }

        #[test]
        fn protocol_malformed_strings_and_forged_object_ids_reject() {
            for data in [
                vec![],
                vec![0; 4],
                vec![255; 4],
                vec![2, 0, 0, 0, b'x', b'y', 0, 0],
            ] {
                assert!(Args(&data).string().is_err());
            }
            assert!(Inventory::new().event(4, 0, &1u32.to_ne_bytes()).is_err());
        }

        #[test]
        fn protocol_global_missing_or_finished_never_enables_identity() {
            let mut state = Inventory::new();
            let mut global = 1u32.to_ne_bytes().to_vec();
            global.extend(string_arg(MANAGER));
            global.extend(3u32.to_ne_bytes());
            assert_eq!(state.event(2, 0, &global).unwrap(), Some(1));
            assert_eq!(state.version, Some(3));
            assert_eq!(
                state.event(4, 1, &[]).unwrap_err(),
                IdentityError::Unsupported
            );
        }

        #[cfg(target_os = "linux")]
        #[tokio::test]
        async fn native_socket_inventory_uses_sync_barriers_and_refuses_missing_protocol() {
            // Short isolated sockets; no compositor, account or wall-clock sleeps.
            for supported in [false, true] {
                let number = CONNECTION.fetch_add(1, Ordering::Relaxed);
                let dir = std::env::temp_dir().join(format!("gwi-{}-{number}", std::process::id()));
                std::fs::create_dir(&dir).unwrap();
                let path = dir.join("w");
                let listener = tokio::net::UnixListener::bind(&path).unwrap();
                let server = async {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut initial = [0; 24];
                    socket.read_exact(&mut initial).await.unwrap();
                    assert_eq!(&initial[..4], &1u32.to_ne_bytes());
                    if supported {
                        let mut global = 9u32.to_ne_bytes().to_vec();
                        global.extend(string_arg(MANAGER));
                        global.extend(3u32.to_ne_bytes());
                        request(&mut socket, 2, 0, &global).await.unwrap();
                    }
                    request(&mut socket, 3, 0, &1u32.to_ne_bytes())
                        .await
                        .unwrap();
                    if supported {
                        // Consume registry.bind, then the second display sync.
                        let mut header = [0; 8];
                        socket.read_exact(&mut header).await.unwrap();
                        assert_eq!(&header[..4], &2u32.to_ne_bytes());
                        let word = u32::from_ne_bytes(header[4..].try_into().unwrap());
                        let mut bind = vec![0; (word >> 16) as usize - 8];
                        socket.read_exact(&mut bind).await.unwrap();
                        let mut sync = [0; 12];
                        socket.read_exact(&mut sync).await.unwrap();
                        assert_eq!(&sync[8..], &6u32.to_ne_bytes());
                        let id = 0xff00_0001u32;
                        request(&mut socket, 4, 0, &id.to_ne_bytes()).await.unwrap();
                        request(&mut socket, id, 0, &string_arg("Fixture"))
                            .await
                            .unwrap();
                        request(&mut socket, id, 1, &string_arg("org.fixture"))
                            .await
                            .unwrap();
                        request(&mut socket, id, 5, &[]).await.unwrap();
                        request(&mut socket, 6, 0, &2u32.to_ne_bytes())
                            .await
                            .unwrap();
                    }
                };
                let source = NativeToplevelSource::new(path.clone());
                let (result, ()) = tokio::join!(source.inventory(), server);
                if supported {
                    let snapshot = result.unwrap();
                    assert_eq!(snapshot.windows[0].title, "Fixture");
                    assert_eq!(snapshot.windows[0].pid, None);
                    assert!(!snapshot.windows[0].peer_pid);
                } else {
                    assert_eq!(result.unwrap_err(), IdentityError::Unsupported);
                }
                drop(listener);
                std::fs::remove_file(path).unwrap();
                std::fs::remove_dir(dir).unwrap();
            }
        }
    }
}

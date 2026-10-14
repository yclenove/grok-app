//! Mutual challenge authentication. UID alone never authorizes a peer.
use crate::{
    codec::{read_frame, write_frame},
    messages::{Version, VERSION},
    Error,
};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncWrite};
/// Deadline for the complete handshake, including backpressure.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// One-shot connection secret. Debug/serialization are deliberately absent.
#[derive(Clone)]
pub struct Secret {
    bytes: [u8; 32],
    used: Arc<AtomicU8>,
}
impl Secret {
    /// Generate 256 bits from the OS-seeded generator. No IO waits or cancellation.
    pub fn random() -> Self {
        Self::from_bytes(rand::random())
    }
    /// Import exactly 32 bytes delivered through a private inherited descriptor.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self {
            bytes,
            used: Arc::new(AtomicU8::new(0)),
        }
    }
    /// Encode a session bearer token for the authenticated App result only.
    pub fn hex(&self) -> String {
        self.bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
    /// Constant-time proof comparison for exactly 32 lowercase-hex bytes.
    pub fn verify_hex(&self, text: &str) -> bool {
        if text.len() != 64 {
            return false;
        }
        let mut bytes = [0; 32];
        for (i, chunk) in text.as_bytes().as_chunks::<2>().0.iter().enumerate() {
            let digit = |b: u8| match b {
                b'0'..=b'9' => Some(b - b'0'),
                b'a'..=b'f' => Some(b - b'a' + 10),
                _ => None,
            };
            let (Some(a), Some(b)) = (digit(chunk[0]), digit(chunk[1])) else {
                return false;
            };
            bytes[i] = (a << 4) | b;
        }
        let Ok(mut verifier) = <Hmac<sha2::Sha256> as KeyInit>::new_from_slice(&self.bytes) else {
            return false;
        };
        verifier.update(&bytes);
        let Ok(expected) = mac(self, &self.bytes) else {
            return false;
        };
        verifier.verify_slice(&expected).is_ok()
    }
    /// Derive the independent stop-channel secret with domain separation.
    pub fn stop_secret(&self) -> Result<Self, Error> {
        Ok(Self::from_bytes(mac(self, b"gcu-host-v1/stop-key")?))
    }
}
/// Operating-system process instance. Start time prevents PID reuse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessInstance {
    /// Kernel process ID.
    pub pid: u32,
    /// OS creation timestamp/tick; interpreted only on the local machine.
    pub started: u64,
}
impl ProcessInstance {
    /// Inspect a live process. Brief blocking OS metadata read; callers doing
    /// startup on an async runtime should use spawn_blocking with a deadline.
    pub fn inspect(pid: u32) -> Result<Self, Error> {
        Ok(Self {
            pid,
            started: crate::unsafe_platform::started(pid)?,
        })
    }
    /// Current process instance, with the same IO semantics as inspect.
    pub fn current() -> Result<Self, Error> {
        Self::inspect(std::process::id())
    }
}
/// Authenticated direction; reflected proofs cannot authenticate the other side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Trusted App process.
    Host,
    /// Its specifically launched child.
    Engine,
}
/// Independent transports have different authenticated transcripts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// Ordinary bidirectional RPC.
    Rpc,
    /// User Stop/resume only.
    Stop,
}
/// Local handshake inputs; expected peer comes from launch, never from the wire.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Local direction.
    pub role: Role,
    /// Dedicated transport purpose.
    pub channel: Channel,
    /// Local OS identity.
    pub instance: ProcessInstance,
    /// Negotiated Host protocol version.
    pub version: Version,
    /// Engine package version (Host may send an empty string).
    pub engine_version: String,
    /// Actual engine capabilities, or None in a Host greeting.
    pub capabilities: Option<gcu_protocol::Capabilities>,
    /// Host's persistent Stop mirror. Engine never clears it during startup.
    pub stopped: bool,
}
impl AuthConfig {
    /// Current process defaults; inspect has brief blocking metadata IO.
    pub fn current(role: Role, channel: Channel) -> Result<Self, Error> {
        Ok(Self {
            role,
            channel,
            instance: ProcessInstance::current()?,
            version: VERSION,
            engine_version: env!("CARGO_PKG_VERSION").into(),
            capabilities: None,
            stopped: false,
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Hello {
    host_protocol: Version,
    instance: ProcessInstance,
    role: Role,
    channel: Channel,
    nonce: [u8; 32],
    engine_version: String,
    capabilities: Option<gcu_protocol::Capabilities>,
    stopped: bool,
}
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Proof {
    proof: [u8; 32],
}
/// Peer facts returned only after both proofs succeed.
#[derive(Debug, Clone)]
pub struct Authenticated {
    /// Authenticated peer process.
    pub peer: ProcessInstance,
    /// Negotiated compatible minor version.
    pub version: Version,
    /// Peer-advertised engine version.
    pub engine_version: String,
    /// Authenticated peer driver capabilities.
    pub capabilities: Option<gcu_protocol::Capabilities>,
    /// Persisted Host Stop value, used to initialize the engine mirror.
    pub stopped: bool,
}
fn mac(secret: &Secret, bytes: &[u8]) -> Result<[u8; 32], Error> {
    let mut h = <Hmac<sha2::Sha256> as KeyInit>::new_from_slice(&secret.bytes)
        .map_err(|_| Error::Authentication)?;
    h.update(bytes);
    Ok(h.finalize().into_bytes().into())
}
fn transcript(host: &Hello, engine: &Hello, role: Role) -> Result<Vec<u8>, Error> {
    serde_json::to_vec(&("gcu-host-v1/mutual-proof", host, engine, role))
        .map_err(|_| Error::Invalid)
}
/// Authenticate an already connected private stream. Both sides send fresh
/// nonces, bind version/process/channel to their proofs and verify the expected
/// live process instance. Five-second total deadline; any failure requires
/// closing the stream and consuming the one-shot launch secret. No retries.
pub async fn authenticate<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    config: &AuthConfig,
    expected: &ProcessInstance,
    secret: &Secret,
) -> Result<Authenticated, Error> {
    let bit = match (config.role, config.channel) {
        (Role::Host, Channel::Rpc) => 1,
        (Role::Engine, Channel::Rpc) => 2,
        (Role::Host, Channel::Stop) => 4,
        (Role::Engine, Channel::Stop) => 8,
    };
    if secret.used.fetch_or(bit, Ordering::SeqCst) & bit != 0 {
        return Err(Error::Authentication);
    }
    tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
        let ours = Hello {
            host_protocol: config.version,
            instance: config.instance.clone(),
            role: config.role,
            channel: config.channel,
            nonce: rand::random(),
            engine_version: config.engine_version.clone(),
            capabilities: config.capabilities.clone(),
            stopped: config.stopped,
        };
        write_frame(stream, &ours).await?;
        let peer: Hello = read_frame(stream).await?.ok_or(Error::Disconnected)?;
        if peer.host_protocol.major != config.version.major {
            return Err(Error::Version);
        }
        if peer.instance != *expected
            || peer.role == config.role
            || peer.channel != config.channel
            || peer.nonce == ours.nonce
        {
            return Err(Error::Authentication);
        }
        let pin = expected.clone();
        let live = tokio::task::spawn_blocking(move || ProcessInstance::inspect(pin.pid))
            .await
            .map_err(|_| Error::Authentication)??;
        if live != *expected {
            return Err(Error::Authentication);
        }
        let (host, engine) = if config.role == Role::Host {
            (&ours, &peer)
        } else {
            (&peer, &ours)
        };
        write_frame(
            stream,
            &Proof {
                proof: mac(secret, &transcript(host, engine, config.role)?)?,
            },
        )
        .await?;
        let proof: Proof = read_frame(stream).await?.ok_or(Error::Disconnected)?;
        let mut verifier = <Hmac<sha2::Sha256> as KeyInit>::new_from_slice(&secret.bytes)
            .map_err(|_| Error::Authentication)?;
        verifier.update(&transcript(host, engine, peer.role)?);
        verifier
            .verify_slice(&proof.proof)
            .map_err(|_| Error::Authentication)?;
        let stopped = host.stopped;
        Ok(Authenticated {
            peer: peer.instance,
            version: Version {
                major: config.version.major,
                minor: config.version.minor.min(peer.host_protocol.minor),
            },
            engine_version: peer.engine_version,
            capabilities: peer.capabilities,
            stopped,
        })
    })
    .await
    .map_err(|_| Error::Timeout)?
}
/// Private launch bootstrap. Send through the inherited connection or token fd,
/// never through argv, environment, disk, logs, MCP, or an unauthenticated socket.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Bootstrap {
    /// Host instance supplied by the trusted launcher.
    pub host: ProcessInstance,
    token: [u8; 32],
    /// Pin for the optional fallback endpoint.
    pub endpoint: Option<crate::transport::EndpointStamp>,
}
impl Bootstrap {
    /// Construct one launch bootstrap. Pure and nonblocking.
    pub fn new(host: ProcessInstance, token: &Secret) -> Self {
        Self {
            host,
            token: token.bytes,
            endpoint: None,
        }
    }
    /// Attach a fallback endpoint pin before writing the private bootstrap.
    pub fn with_endpoint(mut self, endpoint: crate::transport::EndpointStamp) -> Self {
        self.endpoint = Some(endpoint);
        self
    }
    /// Read one private bootstrap with a five-second deadline.
    pub async fn read<R: AsyncRead + Unpin>(r: &mut R) -> Result<Self, Error> {
        tokio::time::timeout(HANDSHAKE_TIMEOUT, read_frame(r))
            .await
            .map_err(|_| Error::Timeout)?
            .and_then(|v| v.ok_or(Error::Disconnected))
    }
    /// Borrow the launch secret for authentication only. No IO or cancellation.
    pub fn secret(&self) -> Secret {
        Secret::from_bytes(self.token)
    }
}
/// Generate authentication schemas from the exact strict wire types. Pure.
pub fn schemas() -> serde_json::Value {
    serde_json::json!({"hello":schemars::schema_for!(Hello),"proof":schemars::schema_for!(Proof),"bootstrap":schemars::schema_for!(Bootstrap)})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bearer_secret_wrong_bytes_and_invalid_hex_are_rejected() {
        let secret = Secret::random();
        assert!(secret.verify_hex(&secret.hex()));
        assert!(!secret.verify_hex(&Secret::random().hex()));
        assert!(!secret.verify_hex("bad"));
        assert!(!secret.verify_hex(&"z".repeat(64)));
        assert!(!secret.verify_hex(&"0".repeat(64)));
        assert_ne!(secret.hex(), secret.stop_secret().unwrap().hex());
    }
    #[tokio::test]
    async fn handshake_replayed_hello_and_proof_fail_against_fresh_nonce() {
        let config = AuthConfig::current(Role::Host, Channel::Rpc).unwrap();
        let expected = config.instance.clone();
        let secret = Secret::random();
        let replay_secret = secret.clone();
        let (mut a, mut b) = tokio::io::duplex(8192);
        let engine = Hello {
            host_protocol: config.version,
            instance: expected.clone(),
            role: Role::Engine,
            channel: Channel::Rpc,
            nonce: [2; 32],
            engine_version: "test".into(),
            capabilities: None,
            stopped: false,
        };
        let old_host = Hello {
            role: Role::Host,
            nonce: [1; 32],
            ..engine.clone()
        };
        let task =
            tokio::spawn(async move { authenticate(&mut a, &config, &expected, &secret).await });
        let _: Hello = read_frame(&mut b).await.unwrap().unwrap();
        write_frame(&mut b, &engine).await.unwrap();
        let _: Proof = read_frame(&mut b).await.unwrap().unwrap();
        write_frame(
            &mut b,
            &Proof {
                proof: mac(
                    &replay_secret,
                    &transcript(&old_host, &engine, Role::Engine).unwrap(),
                )
                .unwrap(),
            },
        )
        .await
        .unwrap();
        assert!(matches!(task.await.unwrap(), Err(Error::Authentication)));
    }
    #[tokio::test]
    async fn bootstrap_private_descriptor_roundtrip_and_disconnect_are_bounded() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        let instance = ProcessInstance::current().unwrap();
        let secret = Secret::random();
        let bootstrap = Bootstrap::new(instance.clone(), &secret);
        write_frame(&mut a, &bootstrap).await.unwrap();
        let got = Bootstrap::read(&mut b).await.unwrap();
        assert_eq!(got.host, instance);
        assert_eq!(got.secret().hex(), secret.hex());
        drop(a);
        assert!(Bootstrap::read(&mut b).await.is_err());
    }
}

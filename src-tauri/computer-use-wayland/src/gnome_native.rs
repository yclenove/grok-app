//! Optional, explicitly installed GNOME Shell helper policy. Never auto-install
//! or grant desktop access. Requires the pinned same-session Shell/login1 watch.
//! Signal delivery is asynchronous; this is not an atomic compositor input fence.
use crate::{GnomeSessionWatch, PortalInputPolicy};
use futures_util::{FutureExt, StreamExt};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use zbus::{message::Type, Connection, MatchRule, MessageStream, Proxy};

#[path = "gnome_consent.rs"]
mod consent;
pub use consent::GnomePolicyActivation;

const PATH: &str = "/org/grok/ComputerUse/NativePolicy";
const INTERFACE: &str = "org.grok.ComputerUse.NativePolicy1";
const FRESH_MS: u64 = 500;
type State = (u32, String, u32, bool);
fn error(e: impl std::fmt::Display) -> String {
    format!("GNOME native policy: {e}")
}

struct Guard {
    phase: AtomicU8,
    armed: AtomicBool,
    takeover: AtomicBool,
    origin: Instant,
    fresh_until: AtomicU64,
    session: Arc<dyn PortalInputPolicy>,
}
impl Guard {
    fn now(&self) -> u64 {
        self.origin.elapsed().as_millis().min(u64::MAX as u128) as u64
    }
    fn fresh(&self) -> bool {
        if self.now() >= self.fresh_until.load(Ordering::Acquire) {
            self.phase.store(2, Ordering::Release);
            return false;
        }
        self.phase.load(Ordering::Acquire) == 1
    }
}
impl PortalInputPolicy for Guard {
    fn input_available(&self) -> bool {
        self.armed.load(Ordering::Acquire)
            && self.fresh()
            && self.session.input_available()
            && self.fresh()
    }
    fn user_input_active(&self) -> bool {
        self.takeover.load(Ordering::Acquire) || self.session.user_input_active()
    }
}

/// Retain and poll this original future for the entire portal lifetime. Any
/// helper generation change, disable/reload, fault, timeout, owner loss or stale
/// event loop permanently retires its grant; a new watch and consent are needed.
/// No internally spawned/detached tasks. Host permission remains mandatory.
pub struct GnomeNativePolicyWatch {
    session: GnomeSessionWatch,
    helper: HelperWatch,
}
struct HelperWatch {
    connection: Connection,
    owner: String,
    signals: MessageStream,
    expected: State,
    guard: Arc<Guard>,
    activation: Option<tokio::sync::oneshot::Receiver<consent::ActivationRequest>>,
}
impl GnomeNativePolicyWatch {
    pub async fn connect(host: Arc<dyn PortalInputPolicy>) -> Result<Self, String> {
        tokio::time::timeout(Duration::from_secs(4), async {
            Self::prepare(GnomeSessionWatch::connect(host).await?).await
        })
        .await
        .map_err(|_| error("initialization deadline"))?
    }
    pub(super) async fn prepare(session: GnomeSessionWatch) -> Result<Self, String> {
        let (connection, owner) = session.native_endpoint();
        let signals = MessageStream::for_match_rule(
            MatchRule::builder()
                .msg_type(Type::Signal)
                .sender(owner.as_str())
                .map_err(error)?
                .path(PATH)
                .map_err(error)?
                .interface(INTERFACE)
                .map_err(error)?
                .member("Changed")
                .map_err(error)?
                .build(),
            &connection,
            Some(64),
        )
        .await
        .map_err(error)?;
        let expected = snapshot(&connection, &owner).await?;
        if expected.0 != 1
            || expected.3
            || uuid::Uuid::parse_str(&expected.1)
                .map(|id| id.to_string() != expected.1)
                .unwrap_or(true)
        {
            return Err(error("unsupported, blocked or malformed helper state"));
        }
        let guard = Arc::new(Guard {
            phase: AtomicU8::new(0),
            armed: AtomicBool::new(true),
            takeover: AtomicBool::new(false),
            origin: Instant::now(),
            fresh_until: AtomicU64::new(FRESH_MS),
            session: session.input_policy(),
        });
        Ok(Self {
            session,
            helper: HelperWatch {
                connection,
                owner,
                signals,
                expected,
                guard,
                activation: None,
            },
        })
    }
    pub fn input_policy(&self) -> Arc<dyn PortalInputPolicy> {
        self.helper.guard.clone()
    }
    pub async fn run_until(self, stop: tokio::sync::oneshot::Receiver<()>) -> Result<(), String> {
        let Self { session, helper } = self;
        let (_send, receive) = tokio::sync::oneshot::channel();
        tokio::select! {
            biased;
            _ = stop => Ok(()),
            result = session.run_until(receive) => result,
            result = helper.run() => result,
        }
    }
}
async fn snapshot(connection: &Connection, owner: &str) -> Result<State, String> {
    tokio::time::timeout(Duration::from_millis(250), async {
        Proxy::new(connection, owner, PATH, INTERFACE)
            .await
            .map_err(error)?
            .call("GetState", &())
            .await
            .map_err(error)
    })
    .await
    .map_err(|_| error("helper heartbeat deadline"))?
}
fn unchanged(actual: &State, expected: &State, guard: &Guard) -> Result<(), String> {
    if actual != expected {
        if actual.2 != expected.2 && !actual.3 {
            guard.takeover.store(true, Ordering::Release);
        }
        guard.phase.store(2, Ordering::Release);
        return Err(error("helper changed; new consent required"));
    }
    Ok(())
}
impl HelperWatch {
    async fn run(mut self) -> Result<(), String> {
        if let Some(activation) = self.activation.take() {
            return self.run_consent(activation).await;
        }
        // A startup pulse cannot disappear into a later unlocked snapshot.
        if self.signals.next().now_or_never().is_some() {
            return Err(error("helper changed during initialization"));
        }
        if self.guard.now() >= self.guard.fresh_until.load(Ordering::Acquire)
            || self
                .guard
                .phase
                .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return Err(error("helper snapshot expired before monitoring"));
        }
        let heartbeat = async {
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;
                if !self.guard.fresh() {
                    return Err(error("helper liveness expired"));
                }
                let value = snapshot(&self.connection, &self.owner).await?;
                let refreshed_until = self.guard.now().saturating_add(FRESH_MS);
                // A late reply must not resurrect an expired grant, even if no
                // model action has queried the policy during an event-loop stall.
                if !self.guard.fresh() {
                    return Err(error("late helper heartbeat"));
                }
                unchanged(&value, &self.expected, &self.guard)?;
                self.guard
                    .fresh_until
                    .store(refreshed_until, Ordering::Release);
            }
        };
        let signals = async {
            loop {
                let message = self
                    .signals
                    .next()
                    .await
                    .ok_or_else(|| error("helper bus ended"))?
                    .map_err(error)?;
                let value: State = message.body().deserialize().map_err(error)?;
                unchanged(&value, &self.expected, &self.guard)?;
            }
        };
        tokio::select! { biased; result = heartbeat => result, result = signals => result }
    }
}
impl Drop for HelperWatch {
    fn drop(&mut self) {
        self.guard.phase.store(2, Ordering::Release);
    }
}

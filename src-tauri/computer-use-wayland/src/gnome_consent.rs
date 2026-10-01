//! One-shot consent-to-input barrier on the original helper/session watches.
//! Picker clicks are expected before a grant exists, never after it is armed.
use super::*;
use tokio::sync::oneshot;

pub(super) struct ActivationRequest(oneshot::Sender<Result<(), String>>);

struct ConsentPolicy(Arc<Guard>);
impl PortalInputPolicy for ConsentPolicy {
    fn input_available(&self) -> bool {
        self.0.fresh() && self.0.session.input_available() && self.0.fresh()
    }
    fn user_input_active(&self) -> bool {
        self.0.user_input_active()
    }
}

/// Consumed exactly once, after native portal consent and before publishing any
/// adapter. Dropping/cancelling activation retires the entire original watch.
pub struct GnomePolicyActivation {
    send: Option<oneshot::Sender<ActivationRequest>>,
    guard: Arc<Guard>,
    completed: bool,
}
impl GnomePolicyActivation {
    /// Health of the pending consent, not permission to dispatch desktop input.
    pub fn consent_ready(&self) -> bool {
        ConsentPolicy(self.guard.clone()).input_available()
    }
    pub(crate) fn consent_policy(&self) -> Arc<dyn PortalInputPolicy> {
        Arc::new(ConsentPolicy(self.guard.clone()))
    }
    pub async fn activate(mut self) -> Result<Arc<dyn PortalInputPolicy>, String> {
        let (send, receive) = oneshot::channel();
        self.send
            .take()
            .ok_or_else(|| error("activation already consumed"))?
            .send(ActivationRequest(send))
            .map_err(|_| error("original monitor ended before activation"))?;
        receive
            .await
            .map_err(|_| error("original monitor rejected activation"))??;
        if !self.guard.input_available() || self.guard.user_input_active() {
            return Err(error("policy retired at activation barrier"));
        }
        self.completed = true;
        Ok(self.guard.clone())
    }
}
impl Drop for GnomePolicyActivation {
    fn drop(&mut self) {
        if !self.completed {
            self.guard.phase.store(2, Ordering::Release);
        }
    }
}

impl GnomeNativePolicyWatch {
    /// Keep the returned original watch running during the picker. Its input
    /// policy stays unavailable until activation succeeds; only health is live.
    pub async fn connect_for_consent(
        host: Arc<dyn PortalInputPolicy>,
    ) -> Result<(Self, GnomePolicyActivation), String> {
        tokio::time::timeout(Duration::from_secs(4), async {
            Self::prepare_for_consent(GnomeSessionWatch::connect(host).await?).await
        })
        .await
        .map_err(|_| error("consent initialization deadline"))?
    }
    pub(crate) async fn prepare_for_consent(
        session: GnomeSessionWatch,
    ) -> Result<(Self, GnomePolicyActivation), String> {
        let mut watch = Self::prepare(session).await?;
        watch.helper.guard.armed.store(false, Ordering::Release);
        let (send, receive) = oneshot::channel();
        watch.helper.activation = Some(receive);
        let activation = GnomePolicyActivation {
            send: Some(send),
            guard: watch.helper.guard.clone(),
            completed: false,
        };
        Ok((watch, activation))
    }
}

impl HelperWatch {
    fn accept(&mut self, value: State, pending: bool) -> Result<(), String> {
        if !pending {
            return unchanged(&value, &self.expected, &self.guard);
        }
        // Only physical serial advancement is allowed while there is NO input
        // grant. Never absorb a lock pulse, reload, protocol change or fault.
        if value.0 != self.expected.0 || value.1 != self.expected.1 || value.3 {
            return Err(error("helper changed during consent"));
        }
        // A heartbeat reply can precede an already queued Changed signal. Both
        // share a pinned epoch; max avoids rebasing onto that older snapshot.
        self.expected.2 = self.expected.2.max(value.2);
        Ok(())
    }
    fn queued(&mut self, pending: bool) -> Result<(), String> {
        for _ in 0..64 {
            match self.signals.next().now_or_never() {
                None => return Ok(()),
                Some(Some(Ok(message))) => {
                    self.accept(message.body().deserialize().map_err(error)?, pending)?;
                }
                _ => return Err(error("helper bus ended during consent")),
            }
        }
        Err(error("helper consent signal backlog exceeded"))
    }
    pub(super) async fn run_consent(
        mut self,
        activation: oneshot::Receiver<ActivationRequest>,
    ) -> Result<(), String> {
        self.queued(true)?;
        if self.guard.now() >= self.guard.fresh_until.load(Ordering::Acquire)
            || self
                .guard
                .phase
                .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return Err(error("helper consent snapshot expired before monitoring"));
        }
        tokio::pin!(activation);
        let mut pending = true;
        loop {
            // Own one heartbeat future until it finishes. Signals must not
            // repeatedly restart its timeout or postpone liveness verification.
            let connection = self.connection.clone();
            let owner = self.owner.clone();
            let heartbeat = async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                snapshot(&connection, &owner).await
            };
            tokio::pin!(heartbeat);
            loop {
                if !self.guard.fresh() {
                    return Err(error("helper consent liveness expired"));
                }
                tokio::select! {
                    biased;
                    result = &mut heartbeat => {
                        let until = self.guard.now().saturating_add(FRESH_MS);
                        if !self.guard.fresh() {
                            return Err(error("late consent heartbeat"));
                        }
                        self.accept(result?, pending)?;
                        self.guard.fresh_until.store(until, Ordering::Release);
                        break;
                    }
                    request = &mut activation, if pending => {
                        let request = request.map_err(|_| error("consent activation dropped"))?;
                        // Drain picker events BEFORE fixing the baseline; from
                        // this point onward every physical change retires it.
                        self.queued(true)?;
                        self.arm(request).await?;
                        pending = false;
                        // An in-flight pre-barrier heartbeat is older evidence;
                        // discard it, never use it to refresh the active grant.
                        break;
                    }
                    message = self.signals.next() => {
                        let message = message.ok_or_else(|| error("helper bus ended"))?.map_err(error)?;
                        self.accept(message.body().deserialize().map_err(error)?, pending)?;
                    }
                }
            }
        }
    }
    async fn arm(&mut self, request: ActivationRequest) -> Result<(), String> {
        let connection = self.connection.clone();
        let owner = self.owner.clone();
        let barrier = snapshot(&connection, &owner);
        tokio::pin!(barrier);
        loop {
            tokio::select! {
                biased;
                result = &mut barrier => {
                    let until = self.guard.now().saturating_add(FRESH_MS);
                    self.accept(result?, false)?;
                    self.queued(false)?;
                    if !self.guard.fresh() || !self.guard.session.input_available()
                        || self.guard.session.user_input_active() || !self.guard.fresh()
                    {
                        return Err(error("session retired during activation"));
                    }
                    self.guard.fresh_until.store(until, Ordering::Release);
                    self.guard.armed.store(true, Ordering::Release);
                    request.0.send(Ok(())).map_err(|_| error("activation receiver dropped"))?;
                    return Ok(());
                }
                message = self.signals.next() => {
                    let message = message.ok_or_else(|| error("helper bus ended"))?.map_err(error)?;
                    self.accept(message.body().deserialize().map_err(error)?, false)?;
                }
            }
        }
    }
}

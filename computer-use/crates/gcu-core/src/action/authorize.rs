use super::{gate::Gate, observe::Fresh, router::ActionRouter, ToolCall};
use crate::{
    driver::{AppQuery, Cancel},
    policy::{
        revalidate, ActionIntent, ApprovalBinding, ConfirmKind, PolicyCtx, PolicyDecision,
        PolicyEngine, PolicyWindow, RouteLimit,
    },
    redact::{EncodeOptions, ImageFormat},
    session::Session,
};
use gcu_host_api::{
    AppIdentity, AppInfo, ApprovalDecision, ConfirmationLevel, ForegroundPolicy, HandoffReason,
    ImageRef, PolicyConfig, SignatureState,
};
use gcu_protocol::ErrorCode;
use std::{sync::atomic::Ordering, time::Duration};

pub(crate) struct Approved {
    pub(super) config: PolicyConfig,
    pub(super) binding: ApprovalBinding,
    sensitive: bool,
    foreground: bool,
}
impl ActionRouter {
    pub(crate) async fn authorize(
        &self,
        session: &Session,
        intent: &mut ActionIntent,
        fresh: &Fresh,
    ) -> Result<Approved, ErrorCode> {
        let app = host_app(fresh, self.driver.platform())?;
        let window = policy_window(fresh);
        let config = self.policy.load();
        let binding = self.binding(intent, fresh, &app);
        let mut approved = Approved {
            config,
            binding,
            sensitive: false,
            foreground: false,
        };
        let timeout = Duration::from_secs(approved.config.timeout_secs);
        for _ in 0..4 {
            self.check_boundary(session).await?;
            let decision = self.evaluate(session, intent, fresh, &app, &window, &approved);
            match decision {
                PolicyDecision::Allow { route_limit } => {
                    if intent.foreground && route_limit != RouteLimit::ForegroundAllowed {
                        return Err(ErrorCode::CapabilityUnavailable);
                    }
                    return Ok(approved);
                }
                PolicyDecision::NeedApproval(request) => {
                    let answer = self
                        .wait(
                            session,
                            session.request_app(
                                self.approvals.as_ref(),
                                &request.app,
                                request.reason.as_ref(),
                                request.risk,
                                timeout,
                            ),
                        )
                        .await?;
                    match answer {
                        ApprovalDecision::Deny => return Err(ErrorCode::ApprovalDenied),
                        ApprovalDecision::Timeout => return Err(ErrorCode::ApprovalTimeout),
                        ApprovalDecision::Always => {
                            // Persistent grants are written by the trusted Host only.
                            self.policy.refresh();
                            let current = self.policy.load();
                            if session.grants().covers(&current, &app).is_none() {
                                return Err(ErrorCode::ApprovalDenied);
                            }
                            // A newly persisted Always grant is the expected result
                            // of this request, not permission to change other policy.
                            let mut without_grants = current.clone();
                            without_grants.always = approved.config.always.clone();
                            if without_grants != approved.config {
                                return Err(ErrorCode::StaleElement);
                            }
                            approved.config = current;
                        }
                        ApprovalDecision::Turn | ApprovalDecision::Session => {}
                    }
                }
                PolicyDecision::NeedConfirm(request) => {
                    if intent.foreground
                        && approved.config.allow_foreground == ForegroundPolicy::Never
                    {
                        return Err(ErrorCode::CapabilityUnavailable);
                    }
                    let crop = match approval_crop(fresh, intent) {
                        Ok(crop) => crop,
                        Err(ErrorCode::HandoffRequired) => {
                            self.take_over(
                                session,
                                &app,
                                HandoffReason::Other("sensitive area could not be located".into()),
                            )
                            .await?;
                            return Err(ErrorCode::HandoffRequired);
                        }
                        Err(code) => return Err(code),
                    };
                    let started = tokio::time::Instant::now();
                    let answer = self
                        .wait(
                            session,
                            session.request_confirm(
                                self.approvals.as_ref(),
                                &app,
                                &request.summary,
                                crop.as_ref(),
                                timeout,
                            ),
                        )
                        .await?;
                    if !answer {
                        return Err(if started.elapsed() >= timeout {
                            ErrorCode::ApprovalTimeout
                        } else {
                            ErrorCode::ApprovalDenied
                        });
                    }
                    match request.kind {
                        ConfirmKind::Sensitive => approved.sensitive = true,
                        ConfirmKind::Foreground => approved.foreground = true,
                    }
                }
                PolicyDecision::Refuse(code, _) => return Err(code),
                PolicyDecision::Handoff(reason) => {
                    self.take_over(session, &app, reason).await?;
                    return Err(ErrorCode::HandoffRequired);
                }
            }
        }
        Err(ErrorCode::ApprovalDenied)
    }
    pub(super) fn evaluate(
        &self,
        session: &Session,
        intent: &ActionIntent,
        fresh: &Fresh,
        app: &AppInfo,
        window: &PolicyWindow,
        approved: &Approved,
    ) -> PolicyDecision {
        let mut config = approved.config.clone();
        // Consume only confirmations for this exact binding, without changing
        // Host policy or allowing a later action to inherit them.
        if approved.sensitive {
            config.sensitive_action_confirmation = ConfirmationLevel::Off;
        }
        if approved.foreground {
            config.allow_foreground = ForegroundPolicy::Always;
        }
        PolicyEngine::new().evaluate(
            &PolicyCtx {
                session_id: session.id(),
                app,
                window: Some(window),
                tree: Some(&fresh.tree),
                activity: session.activity(),
                config: &config,
                grants: &session.grants(),
                policy_version: approved.binding.policy_version,
                closed: session.is_closed(),
                browser: None,
            },
            intent,
        )
    }
    pub(crate) fn bind_parameters(
        &self,
        intent: &mut ActionIntent,
        call: &ToolCall,
        fresh: &Fresh,
        gate: &Gate,
    ) -> Result<(), ErrorCode> {
        intent.parameters = serde_json::to_string(&(
            call,
            format!("{:?}", call.pointer_route),
            &fresh.signature,
            &fresh.info,
            gate.element.as_ref().map(|e| e.fingerprint.0),
            gate.hashes.iter().map(|h| h.0).collect::<Vec<_>>(),
        ))
        .map_err(|_| ErrorCode::Internal)?;
        // This digest is an opaque change detector; security compares the full
        // canonical parameters with revalidate, never a truncated hash alone.
        let hash = super::observe::digest(intent.parameters.as_bytes());
        for (i, chunk) in intent
            .action_digest
            .as_chunks_mut::<8>()
            .0
            .iter_mut()
            .enumerate()
        {
            chunk.copy_from_slice(&hash.wrapping_add(i as u64).to_le_bytes());
        }
        Ok(())
    }
    fn binding(&self, intent: &ActionIntent, fresh: &Fresh, app: &AppInfo) -> ApprovalBinding {
        ApprovalBinding {
            app: app.identity.clone(),
            pid: app.pid,
            window_id: fresh.win.id.clone(),
            parameters: intent.parameters.clone(),
            action_digest: intent.action_digest,
            snapshot_id: intent.snapshot_id.clone(),
            policy_version: self.policy_version.load(Ordering::Acquire),
        }
    }
    async fn take_over(
        &self,
        session: &Session,
        app: &AppInfo,
        reason: HandoffReason,
    ) -> Result<(), ErrorCode> {
        self.wait(
            session,
            session.request_handoff(self.approvals.as_ref(), app, reason),
        )
        .await?
    }
    pub(crate) async fn recheck(
        &self,
        session: &Session,
        call: &ToolCall,
        gate: &Gate,
        fresh: &Fresh,
        intent: &ActionIntent,
        approved: &Approved,
    ) -> Result<Fresh, ErrorCode> {
        self.check_boundary(session).await?;
        let cancel = Cancel::default();
        let app = self
            .native(
                session,
                &cancel,
                false,
                self.driver
                    .resolve_app(&AppQuery(fresh.win.app.info.id.clone())),
            )
            .await
            .map_err(|e| e.error_code())?;
        let win = crate::driver::WindowRef {
            app,
            id: fresh.win.id.clone(),
        };
        let current = self
            .read(session, &win, fresh.image.is_some(), &self.tree_opts)
            .await?;
        super::gate::check(gate, &current)?;
        if self.policy.load() != approved.config {
            return Err(ErrorCode::StaleElement);
        }
        if super::lock(&session.actions)
            .snapshots
            .latest(&fresh.key())
            .map(|s| s.id())
            != gate.snapshot_id
        {
            return Err(ErrorCode::StaleElement);
        }
        let mut current_intent = intent.clone();
        self.bind_parameters(&mut current_intent, call, &current, gate)?;
        let app = host_app(&current, self.driver.platform())?;
        revalidate(
            &approved.binding,
            &self.binding(&current_intent, &current, &app),
        )
        .map_err(|_| {
            if gate.points.is_empty() {
                ErrorCode::StaleElement
            } else {
                ErrorCode::StaleScreenshot
            }
        })?;
        match self.evaluate(
            session,
            &current_intent,
            &current,
            &app,
            &policy_window(&current),
            approved,
        ) {
            PolicyDecision::Allow { .. } => Ok(current),
            PolicyDecision::Refuse(code, _) => Err(code),
            PolicyDecision::Handoff(reason) => {
                self.take_over(session, &app, reason).await?;
                Err(ErrorCode::HandoffRequired)
            }
            _ => Err(ErrorCode::ApprovalDenied),
        }
    }
}
pub(crate) fn host_app(
    fresh: &Fresh,
    platform: crate::driver::Platform,
) -> Result<AppInfo, ErrorCode> {
    let info = &fresh.win.app.info;
    let id = info.identity.as_ref().ok_or(ErrorCode::HandoffRequired)?;
    let path = id
        .executable
        .as_ref()
        .filter(|s| !s.is_empty())
        .ok_or(ErrorCode::HandoffRequired)?;
    // The portable discovery type does not encode signature validity. Never
    // invent Valid from a publisher/team string, or manufacture a missing digest.
    let sha256 = id.code_hash.as_deref().and_then(|s| {
        if s.len() != 64 {
            return None;
        }
        let mut bytes = [0; 32];
        for (i, pair) in s.as_bytes().as_chunks::<2>().0.iter().enumerate() {
            bytes[i] =
                (char::from(pair[0]).to_digit(16)? * 16 + char::from(pair[1]).to_digit(16)?) as u8;
        }
        Some(bytes)
    });
    let mut identity = AppIdentity {
        platform: match platform {
            crate::driver::Platform::Macos => gcu_host_api::Platform::Macos,
            crate::driver::Platform::Windows => gcu_host_api::Platform::Windows,
            crate::driver::Platform::Linux => gcu_host_api::Platform::Linux,
        },
        key: id.id.clone(),
        team_id: id.team_id.clone(),
        publisher: id.publisher.clone(),
        sha256,
        signature: SignatureState::Unknown,
        image_path: path.clone(),
        proof: gcu_host_api::GrantProof::default(),
    };
    super::provenance::apply_driver_provenance(&mut identity);
    Ok(AppInfo {
        name: info.name.clone(),
        pid: info.pid,
        icon: None,
        identity,
    })
}
pub(super) fn policy_window(fresh: &Fresh) -> PolicyWindow {
    PolicyWindow {
        id: fresh.win.id.clone(),
        title: fresh.info.title.clone(),
        url: fresh
            .tree
            .nodes()
            .into_iter()
            .find_map(|(_, n)| n.node.url.clone()),
    }
}
fn approval_crop(fresh: &Fresh, intent: &ActionIntent) -> Result<Option<ImageRef>, ErrorCode> {
    let Some(image) = &fresh.image else {
        return Ok(None);
    };
    let rect = intent
        .element_index
        .and_then(|i| fresh.tree.element_index.get(&i))
        .and_then(|e| e.frame)
        .or_else(|| {
            intent.at.map(|(x, y)| crate::model::Rect {
                x: x - 16.,
                y: y - 16.,
                w: 32.,
                h: 32.,
            })
        });
    let Some(rect) = rect else {
        return Ok(None);
    };
    let crop = image
        .approval_crop(rect)
        .map_err(|_| ErrorCode::HandoffRequired)?;
    let encoded = crop
        .encode(EncodeOptions {
            format: ImageFormat::Png,
            ..EncodeOptions::default()
        })
        .map_err(|_| ErrorCode::HandoffRequired)?;
    if !encoded.can_attach() {
        return Ok(None);
    }
    Ok(Some(ImageRef::from_bytes(
        encoded.mime(),
        encoded.width(),
        encoded.height(),
        encoded.bytes().to_vec(),
    )))
}

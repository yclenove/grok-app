//! Run-owned bridge to the existing Host wire types. The App broker must still
//! enforce explicit user intent/permission, lock screen and user takeover.
//! This is not the synchronous ComputerUseAdapter or an enabled App backend.
use crate::{InputAction, PortalSession, PresentedFrame, PresentedImage, SessionState, SourceKind};
use base64::Engine;
use chrono::{DateTime, Utc};
use grok_computer_use_core::{
    adapter::{ActionScope, AdapterActResult, CaptureOptions, DispatchRequest},
    protocol::{
        click_button, click_count, normalize_key, ActionKind, ActionRequest, ActionTarget,
        Observation, ObservationImage, OutcomeKind, ScrollDelta, COORDINATE_SPACE_IMAGE_PIXELS,
        JS_MAX_SAFE_INTEGER, OBSERVATION_PNG_B64_CAP, PROTOCOL_VERSION,
    },
};
use image::ImageEncoder;
use std::time::Instant;

struct Ticket {
    snapshot_id: String,
    geometry_revision: u64,
    picture: PresentedFrame,
}

/// Owns the portal session and its one outstanding model picture. Identity
/// fields from JSON never create native authority. Preview does not issue or
/// refresh a ticket. No raw session escape hatch, automatic retry or fallback.
pub struct PortalHostSession {
    session: PortalSession,
    run_id: String,
    target_id: String,
    generation: u64,
    target_generation: u64,
    ticket: Option<Ticket>,
    clock: (Instant, DateTime<Utc>),
}
impl PortalHostSession {
    pub fn new(
        session: PortalSession,
        generation: u64,
        target_generation: u64,
    ) -> Result<Self, String> {
        Self::from_session_slot(&mut Some(session), generation, target_generation)
    }

    /// Failed handoff leaves the original owner with its granting runtime so
    /// a registry can await cleanup rather than merely dropping a JoinHandle.
    pub(crate) fn from_session_slot(
        slot: &mut Option<PortalSession>,
        generation: u64,
        target_generation: u64,
    ) -> Result<Self, String> {
        let session = slot.as_ref().ok_or("portal session already handed off")?;
        let SessionState::Granted(grant) = session.state() else {
            return Err("Host binding requires an active portal grant".into());
        };
        // Window capture does not constrain session-wide keyboard input. Do not
        // represent that as directed per-window control or widen it silently.
        if grant.stream.source != SourceKind::Monitor {
            return Err("window-scoped Host input is not implemented".into());
        }
        if !(1..=JS_MAX_SAFE_INTEGER).contains(&generation)
            || !(1..=JS_MAX_SAFE_INTEGER).contains(&target_generation)
        {
            return Err("Host generations must be positive safe integers".into());
        }
        Ok(Self {
            session: slot.take().ok_or("portal session already handed off")?,
            run_id: grant.run_id,
            target_id: format!("wayland:{}", uuid::Uuid::new_v4().simple()),
            generation,
            target_generation,
            ticket: None,
            clock: (Instant::now(), Utc::now()),
        })
    }
    pub fn run_id(&self) -> &str {
        &self.run_id
    }
    pub fn target_id(&self) -> &str {
        &self.target_id
    }
    pub(crate) fn control(&self) -> crate::session::SessionControl {
        self.session.control()
    }
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
    /// Only the trusted Broker capture path may advance this binding. A new
    /// generation retires its old ticket; actions never rebind themselves.
    pub(crate) fn bind_generation(&mut self, generation: u64) -> Result<(), String> {
        if generation < self.generation || !(1..=JS_MAX_SAFE_INTEGER).contains(&generation) {
            return Err("invalid or retired Host generation".into());
        }
        if generation != self.generation || generation != self.target_generation {
            self.ticket = None;
            self.generation = generation;
            self.target_generation = generation;
        }
        Ok(())
    }
    pub fn state(&self) -> SessionState {
        self.session.state()
    }
    pub async fn stop(&mut self) -> Result<(), String> {
        self.ticket = None;
        self.session.stop().await
    }
    fn identity(&self, run: &str, target: &str) -> Result<(), String> {
        if run != self.run_id || target != self.target_id {
            return Err("Host run or target does not own this portal session".into());
        }
        if !matches!(self.session.state(), SessionState::Granted(_)) {
            return Err("Host portal session is retired".into());
        }
        Ok(())
    }
    /// The timestamp denotes original local pixel delivery, not PNG encoding or
    /// model observation time. A fixed clock anchor preserves static timestamps.
    fn delivered_at(&self, captured: Instant) -> Result<String, String> {
        let delta = if captured >= self.clock.0 {
            chrono::Duration::from_std(captured.duration_since(self.clock.0))
        } else {
            chrono::Duration::from_std(self.clock.0.duration_since(captured)).map(|d| -d)
        }
        .map_err(|_| "pixel delivery timestamp outside clock range")?;
        self.clock
            .1
            .checked_add_signed(delta)
            .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true))
            .ok_or_else(|| "pixel delivery timestamp overflow".into())
    }
    pub fn capture(
        &mut self,
        run: &str,
        target: &str,
        options: &CaptureOptions,
        max_width: u32,
        max_height: u32,
    ) -> Result<Observation, String> {
        if options.for_model {
            // Failure/cancellation of a replacement capture must not revive the
            // earlier model reply, including failures before pixels are ready.
            self.ticket = None;
        }
        self.identity(run, target)?;
        options.cancellation.check()?;
        if options.managed_request.is_some() || !options.screenshot {
            return Err(
                "native Host observation requires a screenshot, not browser authority".into(),
            );
        }
        let (frame, mut picture) = if options.for_model {
            let observed = self
                .session
                .observe()?
                .ok_or("no granted pixels available")?;
            (
                observed.frame().clone(),
                Some(observed.present(max_width, max_height)?),
            )
        } else {
            (
                self.session.frame()?.ok_or("no granted pixels available")?,
                None,
            )
        };
        let mut preview = if picture.is_none() {
            Some(PresentedImage::render(&frame, max_width, max_height)?)
        } else {
            None
        };
        let (width, height, png) = loop {
            options.cancellation.check()?;
            let image = picture
                .as_ref()
                .map(PresentedFrame::image)
                .or(preview.as_ref())
                .expect("one presentation");
            let mut bytes = Vec::new();
            image::codecs::png::PngEncoder::new(&mut bytes)
                .write_image(
                    image.rgba(),
                    image.width(),
                    image.height(),
                    image::ExtendedColorType::Rgba8,
                )
                .map_err(|e| format!("PNG encoding failed: {e}"))?;
            let png = base64::engine::general_purpose::STANDARD.encode(bytes);
            if png.len() <= OBSERVATION_PNG_B64_CAP {
                break (image.width(), image.height(), png);
            }
            let (w, h) = ((image.width() / 2).max(1), (image.height() / 2).max(1));
            if w == image.width() && h == image.height() {
                return Err("minimum PNG exceeds Host image limit".into());
            }
            if let Some(old) = picture.take() {
                picture = Some(old.resize(w, h)?);
            } else {
                preview = Some(PresentedImage::render(&frame, w, h)?);
            }
        };
        let revision = picture
            .as_ref()
            .map(|p| p.observation().geometry_revision())
            .unwrap_or(0);
        if revision > JS_MAX_SAFE_INTEGER {
            return Err("geometry revision exceeds wire precision".into());
        }
        self.identity(run, target)?;
        options.cancellation.check()?;
        let snapshot_id = format!("wl-snapshot:{}", uuid::Uuid::new_v4().simple());
        let observation = Observation {
            version: PROTOCOL_VERSION,
            run_id: self.run_id.clone(),
            target_id: self.target_id.clone(),
            target_generation: self.target_generation,
            snapshot_id: snapshot_id.clone(),
            captured_at: self.delivered_at(frame.captured_at)?,
            geometry_revision: revision,
            coordinate_space: COORDINATE_SPACE_IMAGE_PIXELS.into(),
            image: ObservationImage {
                width,
                height,
                content_id: format!("wl-image:{}", uuid::Uuid::new_v4().simple()),
                png_base64: Some(png),
            },
            nodes: Vec::new(),
            text: String::new(),
            truncated: false,
            // Published geometry describes the upright content-only image. Raw
            // buffer crop/orientation and EI scale remain in the opaque ticket.
            crop_x: 0,
            crop_y: 0,
            crop_width: width,
            crop_height: height,
            scale: 1.0,
            dpi: 96.0,
            origin_x: 0,
            origin_y: 0,
            topology_revision: revision,
        };
        if let Some(picture) = picture {
            self.ticket = Some(Ticket {
                snapshot_id,
                geometry_revision: revision,
                picture,
            });
        }
        Ok(observation)
    }
    fn prepare(
        &mut self,
        request: &DispatchRequest,
    ) -> Result<(crate::ObservedFrame, Vec<InputAction>), String> {
        self.identity(&request.run_id, &request.target_id)?;
        request.cancellation.check()?;
        if request.scope != ActionScope::Directed
            || request.managed_request.is_some()
            || request.generation != self.generation
            || request.target_generation != self.target_generation
        {
            return Err("Host scope or generation mismatch".into());
        }
        ActionRequest {
            version: PROTOCOL_VERSION,
            action_id: request.action_id.clone(),
            run_id: request.run_id.clone(),
            target_id: request.target_id.clone(),
            target_generation: request.target_generation,
            snapshot_id: request.snapshot_id.clone(),
            geometry_revision: request.geometry_revision,
            action: request.action,
            target: request.target.clone(),
            parameters: request.parameters.clone(),
        }
        .validate_schema()?;
        let ticket = self
            .ticket
            .as_ref()
            .ok_or("no model screenshot authority")?;
        if ticket.snapshot_id != request.snapshot_id
            || ticket.geometry_revision != request.geometry_revision
        {
            return Err("Host snapshot or geometry mismatch".into());
        }
        let ActionTarget::Coord { x, y } = request.target else {
            return Err("native Wayland semantic actions are not implemented".into());
        };
        let (width, height) = (
            ticket.picture.image().width(),
            ticket.picture.image().height(),
        );
        if x >= f64::from(width) || y >= f64::from(height) {
            return Err("coordinate outside the exact Host image".into());
        }
        let mut actions = Vec::new();
        match request.action {
            ActionKind::Click => {
                let code = match click_button(&request.parameters) {
                    "right" => 0x111,
                    "middle" => 0x112,
                    _ => 0x110,
                };
                for _ in 0..click_count(&request.parameters) {
                    actions.push(InputAction::Button {
                        code,
                        pressed: true,
                    });
                    actions.push(InputAction::Button {
                        code,
                        pressed: false,
                    });
                }
            }
            ActionKind::Key => {
                let code =
                    match normalize_key(request.parameters["key"].as_str().ok_or("key required")?)?
                    {
                        "enter" => 28,
                        "tab" => 15,
                        "escape" => 1,
                        "space" => 57,
                        "down" => 108,
                        "up" => 103,
                        "left" => 105,
                        "right" => 106,
                        "backspace" => 14,
                        "delete" => 111,
                        "home" => 102,
                        "end" => 107,
                        "pageup" => 104,
                        "pagedown" => 109,
                        _ => return Err("unmapped navigation key".into()),
                    };
                actions.push(InputAction::Key {
                    code,
                    pressed: true,
                });
                actions.push(InputAction::Key {
                    code,
                    pressed: false,
                });
            }
            ActionKind::Scroll => {
                let delta = ScrollDelta::parse(&request.parameters)?.value();
                if delta != 0 {
                    actions.push(InputAction::Scroll {
                        dx: 0.0,
                        dy: f64::from(delta),
                    });
                    actions.push(InputAction::CancelScroll);
                }
            }
            _ => return Err("native Wayland action is not implemented".into()),
        }
        // Select the containing screenshot pixel; no clamping or global desktop
        // interpretation. Its exact centre is checked again at EI wire precision.
        let picture = self.ticket.take().expect("validated ticket").picture;
        if request.action == ActionKind::Key {
            // Keyboard authority is session-wide and independent of pointer
            // region pairing. Keep the exact one-use observation and all EI
            // keyboard/generation preflight; never synthesize pointer motion or
            // guess a region simply to submit a navigation key.
            return Ok((picture.into_observation(), actions));
        }
        let (observation, motion) = picture.into_pixel_input(x.floor() as u32, y.floor() as u32)?;
        if !actions.is_empty() {
            actions.insert(0, motion);
        }
        Ok((observation, actions))
    }
    pub async fn dispatch(
        &mut self,
        request: &DispatchRequest,
    ) -> Result<AdapterActResult, String> {
        let (observation, actions) = match self.prepare(request) {
            Ok(plan) => plan,
            Err(detail) => {
                return Ok(AdapterActResult {
                    applied: false,
                    outcome: Some(OutcomeKind::Rejected),
                    postcondition_ok: false,
                    verifiable: false,
                    detail,
                })
            }
        };
        if actions.is_empty() {
            let checked = self
                .session
                .consume_observation(observation, &request.cancellation);
            return Ok(match checked {
                Ok(()) => AdapterActResult {
                    applied: false,
                    outcome: None,
                    postcondition_ok: false,
                    verifiable: false,
                    detail: "zero scroll delta; no native event posted".into(),
                },
                Err(detail) => AdapterActResult {
                    applied: false,
                    outcome: Some(OutcomeKind::Rejected),
                    postcondition_ok: false,
                    verifiable: false,
                    detail,
                },
            });
        }
        // From enqueue onward, errors are conservatively unknown. Never replay a
        // partial click or turn a libei receipt into a verified application effect.
        self.session
            .input_sequence_observed(observation, actions, request.cancellation.clone())
            .await?;
        Ok(AdapterActResult {
            applied: true,
            outcome: None,
            postcondition_ok: false,
            verifiable: false,
            detail: "submitted to native EI; application effect unverified".into(),
        })
    }
}

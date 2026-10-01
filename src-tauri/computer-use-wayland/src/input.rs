//! Run-owned EI command port. Coordinates here are EI region-local logical
//! coordinates, NOT screenshot pixels. The App observation adapter is separate.
use crate::StreamGrant;
use grok_computer_use_core::execution::ActionCancellation;
use std::{
    os::fd::OwnedFd,
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{sync::oneshot, task::JoinHandle};

#[derive(Clone, Debug, PartialEq)]
pub struct InputRegion {
    pub mapping_id: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub physical_scale: f64,
}
impl InputRegion {
    /// ei_pointer_absolute.motion_absolute carries float32, although libei's C
    /// API accepts doubles. Validate the encoded position, not just the input:
    /// rounding at a distant origin must not cross this region's boundary.
    pub(crate) fn wire_position(&self, x: f64, y: f64) -> Result<(f64, f64), String> {
        let axis = |offset: f64, origin: u32, size: u32| {
            if !offset.is_finite() || offset < 0.0 || offset >= f64::from(size) {
                return Err("position outside granted EI region".to_string());
            }
            let start = f64::from(origin);
            let encoded = f64::from((start + offset) as f32);
            if encoded < start || encoded >= start + f64::from(size) {
                return Err("EI wire precision would leave the granted region".to_string());
            }
            Ok(encoded)
        };
        Ok((axis(x, self.x, self.width)?, axis(y, self.y, self.height)?))
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct InputCapabilities {
    pub generation: u64,
    pub keyboard: bool,
    pub relative_pointer: bool,
    pub buttons: bool,
    pub scroll: bool,
    pub absolute_region: Option<InputRegion>,
}
#[derive(Clone, Debug, PartialEq, Default)]
pub enum InputState {
    #[default]
    Starting,
    Ready(InputCapabilities),
    Revoked,
}
#[derive(Clone, Debug)]
pub enum InputAction {
    /// evdev scan code, NOT a Unicode character or an XKB keycode (+8).
    Key {
        code: u32,
        pressed: bool,
    },
    Button {
        code: u32,
        pressed: bool,
    },
    /// Relative logical pixels on a virtual device, never physical millimeters.
    Relative {
        dx: f64,
        dy: f64,
    },
    /// Half-open coordinates relative to the uniquely matched EI region.
    Absolute {
        x: f64,
        y: f64,
    },
    /// Smooth logical-pixel scroll on a virtual device.
    Scroll {
        dx: f64,
        dy: f64,
    },
    /// Fractions/multiples of 120, not pixels. Never duplicate as smooth scroll.
    ScrollDiscrete {
        dx: i32,
        dy: i32,
    },
    CancelScroll,
    ReleaseAll,
}
impl InputAction {
    pub(crate) fn validate(&self) -> Result<(), String> {
        let valid = match *self {
            Self::Key { code, .. } => {
                (1..=0x2ff).contains(&code)
                    && !(0x100..=0x15f).contains(&code)
                    && !(0x220..=0x223).contains(&code)
                    && !(0x2c0..=0x2ff).contains(&code)
            }
            Self::Button { code, .. } => (0x110..=0x117).contains(&code),
            Self::Relative { dx, dy } | Self::Scroll { dx, dy } => {
                dx.is_finite() && dy.is_finite() && dx.abs() <= 10000.0 && dy.abs() <= 10000.0
            }
            Self::Absolute { x, y } => x.is_finite() && y.is_finite() && x >= 0.0 && y >= 0.0,
            Self::ScrollDiscrete { dx, dy } => {
                dx.unsigned_abs() <= 120000 && dy.unsigned_abs() <= 120000
            }
            Self::CancelScroll | Self::ReleaseAll => true,
        };
        valid
            .then_some(())
            .ok_or_else(|| "invalid bounded EI action".into())
    }
}
/// Submitted to libei, not proof the compositor/application performed it.
/// No retry on an unknown result; the higher-level adapter must observe effects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputSubmission {
    pub run_id: String,
    pub generation: u64,
    pub sequence: u64,
}
#[derive(Default)]
pub(crate) struct InputGate(pub(crate) Mutex<InputState>);
impl InputGate {
    pub(crate) fn revoke(&self) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = InputState::Revoked;
    }
    pub(crate) fn state(&self) -> InputState {
        self.0
            .lock()
            .map(|s| s.clone())
            .unwrap_or(InputState::Revoked)
    }
}
pub(crate) struct Request {
    pub generation: u64,
    pub action: InputAction,
    pub following: Vec<InputAction>,
    pub cancellation: ActionCancellation,
    pub frame: Option<crate::frame::FramePermit>,
    pub deadline: Instant,
    pub response: oneshot::Sender<Result<InputSubmission, String>>,
}
pub(crate) struct InputPort {
    pub gate: Arc<InputGate>,
    send: mpsc::SyncSender<Request>,
}
impl InputPort {
    pub(crate) fn new() -> (Self, mpsc::Receiver<Request>) {
        let (send, receive) = mpsc::sync_channel(32);
        (
            Self {
                gate: Arc::new(InputGate::default()),
                send,
            },
            receive,
        )
    }
    pub(crate) async fn submit(
        &self,
        generation: u64,
        action: InputAction,
    ) -> Result<InputSubmission, String> {
        self.submit_bound(generation, action, None).await
    }
    pub(crate) async fn submit_bound(
        &self,
        generation: u64,
        action: InputAction,
        frame: Option<crate::frame::FramePermit>,
    ) -> Result<InputSubmission, String> {
        self.submit_actions_bound(
            generation,
            vec![action],
            frame,
            ActionCancellation::default(),
        )
        .await
    }
    pub(crate) async fn submit_actions_bound(
        &self,
        generation: u64,
        actions: Vec<InputAction>,
        frame: Option<crate::frame::FramePermit>,
        cancellation: ActionCancellation,
    ) -> Result<InputSubmission, String> {
        cancellation.check()?;
        if actions.is_empty() || actions.len() > 8 {
            return Err("EI action sequence must contain 1..=8 bounded events".into());
        }
        for action in &actions {
            action.validate()?;
        }
        if actions.len() > 1 {
            crate::sequence::validate_balanced(&actions)?;
        }
        let mut actions = actions.into_iter();
        let action = actions.next().expect("nonempty bounded sequence");
        if !matches!(self.gate.state(), InputState::Ready(ref c) if c.generation == generation) {
            return Err("EI input not ready or device generation changed".into());
        }
        let (response, receive) = oneshot::channel();
        self.send
            .try_send(Request {
                generation,
                action,
                following: actions.collect(),
                cancellation,
                frame,
                deadline: Instant::now() + Duration::from_secs(2),
                response,
            })
            .map_err(|_| "EI command queue unavailable or full")?;
        // Once enqueued, cancelling this future makes execution uncertain. Revoke
        // the run's input instead of replaying it or abandoning a held key.
        struct Cancel<'a>(Option<&'a InputGate>);
        impl Drop for Cancel<'_> {
            fn drop(&mut self) {
                if let Some(g) = self.0 {
                    g.revoke();
                }
            }
        }
        let mut cancel = Cancel(Some(&self.gate));
        let result = tokio::time::timeout(Duration::from_secs(3), receive)
            .await
            .map_err(|_| "EI submission result unknown; input revoked")?
            .map_err(|_| "EI owner retired before submission result")?;
        cancel.0 = None;
        result
    }
}
pub(crate) struct InputWorker {
    gate: Arc<InputGate>,
    owner: Option<JoinHandle<Result<(), String>>>,
}
impl InputWorker {
    pub(crate) fn start(
        fd: OwnedFd,
        stream: StreamGrant,
        run_id: String,
        gate: Arc<InputGate>,
        receive: mpsc::Receiver<Request>,
    ) -> Self {
        let shared = gate.clone();
        let owner = tokio::task::spawn_blocking(move || {
            let result = crate::ei_owner::run(fd, stream, run_id, &shared, receive);
            shared.revoke();
            result
        });
        Self {
            gate,
            owner: Some(owner),
        }
    }
    pub(crate) async fn finished(&mut self) -> Result<(), String> {
        if let Some(owner) = self.owner.as_mut() {
            let result = owner.await.map_err(|e| format!("EI owner failed: {e}"));
            self.owner.take();
            result?
        } else {
            Ok(())
        }
    }
    pub(crate) async fn stop(&mut self) -> Result<(), String> {
        self.gate.revoke();
        self.finished().await
    }
}
impl Drop for InputWorker {
    fn drop(&mut self) {
        self.gate.revoke();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_wire_quantization_cannot_escape_the_paired_region() {
        let mut r = InputRegion {
            mapping_id: "r".into(),
            x: 100,
            y: 200,
            width: 800,
            height: 600,
            physical_scale: 2.0,
        };
        let (x, y) = r.wire_position(400.0 / 28.0, 15.0).unwrap();
        assert_eq!(x, f64::from((100.0 + 400.0 / 28.0) as f32));
        assert_eq!(y, 215.0);
        for (x, y) in [
            (f64::NAN, 0.0),
            (0.0, f64::INFINITY),
            (-1.0, 0.0),
            (800.0, 0.0),
            (0.0, 600.0),
            (799.99999999, 0.0),
        ] {
            assert!(r.wire_position(x, y).is_err());
        }
        r.x = u32::MAX;
        r.width = 1;
        assert!(r.wire_position(0.5, 0.5).is_err());
        r.x = 0;
        r.width = 0;
        assert!(r.wire_position(0.0, 0.0).is_err());
    }
    fn ready(port: &InputPort) {
        *port.gate.0.lock().unwrap() = InputState::Ready(InputCapabilities {
            generation: 7,
            keyboard: true,
            buttons: true,
            scroll: true,
            relative_pointer: true,
            absolute_region: None,
        });
    }
    #[tokio::test]
    async fn invalid_raw_actions_never_enter_the_native_command_queue() {
        let (port, receiver) = InputPort::new();
        ready(&port);
        for action in [
            InputAction::Relative {
                dx: f64::NAN,
                dy: 0.0,
            },
            InputAction::Scroll {
                dx: f64::INFINITY,
                dy: 0.0,
            },
            InputAction::Absolute { x: -1.0, y: 0.0 },
            InputAction::Key {
                code: 0,
                pressed: true,
            },
            InputAction::Key {
                code: 0x110,
                pressed: true,
            },
            InputAction::Button {
                code: 30,
                pressed: true,
            },
            InputAction::ScrollDiscrete {
                dx: i32::MIN,
                dy: 0,
            },
        ] {
            assert!(action.validate().is_err(), "{action:?}");
            assert!(port.submit(7, action).await.is_err());
            assert!(receiver.try_recv().is_err());
            assert!(matches!(port.gate.state(), InputState::Ready(_)));
        }
    }
    #[tokio::test]
    async fn cancelled_and_unbalanced_sequences_never_enter_the_owner_queue() {
        let (port, receiver) = InputPort::new();
        ready(&port);
        let tap = vec![
            InputAction::Key {
                code: 30,
                pressed: true,
            },
            InputAction::Key {
                code: 30,
                pressed: false,
            },
        ];
        let cancellation = ActionCancellation::default();
        cancellation.cancel();
        assert!(port
            .submit_actions_bound(7, tap, None, cancellation)
            .await
            .is_err());
        for actions in [
            Vec::new(),
            vec![InputAction::Relative { dx: 1.0, dy: 0.0 }; 9],
            vec![
                InputAction::Button {
                    code: 0x110,
                    pressed: true,
                },
                InputAction::Absolute { x: 1.0, y: 1.0 },
            ],
        ] {
            assert!(port
                .submit_actions_bound(7, actions, None, ActionCancellation::default())
                .await
                .is_err());
        }
        assert!(receiver.try_recv().is_err());
        assert!(matches!(port.gate.state(), InputState::Ready(_)));
    }
    #[tokio::test]
    async fn cancelling_enqueued_command_revokes_and_marks_response_abandoned() {
        let (port, receiver) = InputPort::new();
        ready(&port);
        let mut pending = Box::pin(port.submit(
            7,
            InputAction::Key {
                code: 30,
                pressed: true,
            },
        ));
        assert!(futures_util::poll!(pending.as_mut()).is_pending());
        let request = receiver.try_recv().unwrap();
        assert!(!request.response.is_closed());
        drop(pending);
        assert_eq!(port.gate.state(), InputState::Revoked);
        assert!(request.response.is_closed());
        assert!(port.submit(7, InputAction::ReleaseAll).await.is_err());
    }
    #[tokio::test]
    async fn stale_generation_and_full_queue_are_known_non_submissions() {
        let (port, receiver) = InputPort::new();
        ready(&port);
        assert!(port.submit(6, InputAction::ReleaseAll).await.is_err());
        assert!(receiver.try_recv().is_err());
        let mut receipts = Vec::new();
        for _ in 0..32 {
            let (response, receive) = oneshot::channel();
            receipts.push(receive);
            port.send
                .try_send(Request {
                    generation: 7,
                    action: InputAction::ReleaseAll,
                    following: Vec::new(),
                    cancellation: ActionCancellation::default(),
                    frame: None,
                    deadline: Instant::now(),
                    response,
                })
                .unwrap();
        }
        assert!(port
            .submit(7, InputAction::ReleaseAll)
            .await
            .unwrap_err()
            .contains("full"));
        assert!(matches!(port.gate.state(), InputState::Ready(_)));
        assert_eq!(receiver.try_iter().count(), 32);
    }
}

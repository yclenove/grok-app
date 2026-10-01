//! In-process screenshot authority. Host permission/target/intent checks remain
//! mandatory; this proves neither unchanged UI contents nor application effects.
use crate::{
    frame::{FramePermit, FrameStore},
    input::InputGate,
    CapturedFrame, InputCapabilities, InputState, PortalGrant,
};
use std::{sync::Arc, time::Instant};

/// One-use, non-serializable frame/capability binding, minted only by its session.
/// Pixels may be copied for display, but a copied/modified CapturedFrame cannot
/// mint input authority. The bounded action deadline starts at observation issue;
/// pixel delivery time remains unchanged. A later observation retires this one.
pub struct ObservedFrame {
    pub(crate) permit: FramePermit,
    gate: Arc<InputGate>,
    capabilities: InputCapabilities,
}

impl ObservedFrame {
    pub fn frame(&self) -> &CapturedFrame {
        &self.permit.frame
    }
    pub fn input_capabilities(&self) -> &InputCapabilities {
        &self.capabilities
    }
    pub fn issued_at(&self) -> Instant {
        self.permit.issued_at
    }
    pub fn expires_at(&self) -> Instant {
        self.permit.expires_at
    }
    pub fn geometry_revision(&self) -> u64 {
        self.permit.geometry_revision()
    }

    pub(crate) fn capture(
        frames: &Arc<FrameStore>,
        gate: &Arc<InputGate>,
        grant: &PortalGrant,
    ) -> Result<Option<Self>, String> {
        // Same lock order as native dispatch, including the frame snapshot.
        let state = gate.0.lock().map_err(|_| "EI authority lock poisoned")?;
        let InputState::Ready(capabilities) = &*state else {
            return Err("EI input is not ready for an observation".into());
        };
        let Some(permit) = frames.observe()? else {
            return Ok(None);
        };
        let frame = &permit.frame;
        if frame.run_id != grant.run_id
            || frame.node_id != grant.stream.node_id
            || grant
                .stream
                .pipewire_serial
                .is_some_and(|serial| serial != frame.pipewire_serial)
            || frame.pipewire_serial == 0
            || frame.format_generation == 0
            || frame.sequence == 0
        {
            return Err("observed frame does not belong to the granted source".into());
        }
        if capabilities.absolute_region.as_ref().is_some_and(|region| {
            grant.stream.mapping_id.as_deref() != Some(region.mapping_id.as_str())
        }) {
            return Err("observed EI region does not match the portal source".into());
        }
        Ok(Some(Self {
            permit,
            gate: gate.clone(),
            capabilities: capabilities.clone(),
        }))
    }

    pub(crate) fn belongs_to(&self, frames: &Arc<FrameStore>, gate: &Arc<InputGate>) -> bool {
        Arc::ptr_eq(&self.permit.store, frames) && Arc::ptr_eq(&self.gate, gate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InputAction, InputRegion, PixelRegion, SourceKind, StreamGrant, VideoTransform};
    use std::time::{Duration, Instant};

    fn setup() -> (Arc<FrameStore>, Arc<InputGate>, PortalGrant) {
        let frames = Arc::new(FrameStore::default());
        frames.publish(CapturedFrame {
            run_id: "run".into(),
            node_id: 3,
            pipewire_serial: 12,
            sequence: 1,
            format_generation: 1,
            region: PixelRegion {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            buffer_width: 1,
            buffer_height: 1,
            transform: VideoTransform::Normal,
            captured_at: Instant::now(),
            rgba: Arc::from([0, 0, 0, 255]),
        });
        let gate = Arc::new(InputGate::default());
        *gate.0.lock().unwrap() = InputState::Ready(InputCapabilities {
            generation: 4,
            keyboard: true,
            relative_pointer: true,
            buttons: true,
            scroll: true,
            absolute_region: Some(InputRegion {
                mapping_id: "region".into(),
                x: 100,
                y: 200,
                width: 800,
                height: 600,
                physical_scale: 2.0,
            }),
        });
        let grant = PortalGrant {
            run_id: "run".into(),
            session_path: "/owned".into(),
            devices: 3,
            remote_desktop_version: 2,
            screen_cast_version: 5,
            stream: StreamGrant {
                node_id: 3,
                pipewire_serial: Some(12),
                mapping_id: Some("region".into()),
                source: SourceKind::Monitor,
                compositor_position: None,
                compositor_size: None,
                logical_size: None,
            },
        };
        (frames, gate, grant)
    }

    #[test]
    fn observation_mints_only_for_the_actual_granted_source_and_ready_input() {
        let (frames, gate, grant) = setup();
        type GrantChange = Box<dyn Fn(&mut PortalGrant)>;
        let changes: Vec<GrantChange> = vec![
            Box::new(|g| g.run_id = "other".into()),
            Box::new(|g| g.stream.node_id += 1),
            Box::new(|g| g.stream.pipewire_serial = Some(13)),
            Box::new(|g| g.stream.mapping_id = Some("other".into())),
            Box::new(|g| g.stream.mapping_id = None),
        ];
        for change in changes {
            let mut wrong = grant.clone();
            change(&mut wrong);
            assert!(ObservedFrame::capture(&frames, &gate, &wrong).is_err());
        }
        let ticket = ObservedFrame::capture(&frames, &gate, &grant)
            .unwrap()
            .unwrap();
        assert_eq!(ticket.input_capabilities().generation, 4);
        *gate.0.lock().unwrap() = InputState::Starting;
        assert!(ObservedFrame::capture(&frames, &gate, &grant).is_err());
        gate.revoke();
        assert!(ObservedFrame::capture(&frames, &gate, &grant).is_err());
    }

    #[test]
    fn identical_strings_or_mutated_pixel_copies_cannot_transfer_owner_authority() {
        let (frames, gate, grant) = setup();
        let (other_frames, other_gate, _) = setup();
        let ticket = ObservedFrame::capture(&frames, &gate, &grant)
            .unwrap()
            .unwrap();
        assert!(ticket.belongs_to(&frames, &gate));
        assert!(!ticket.belongs_to(&other_frames, &gate));
        assert!(!ticket.belongs_to(&frames, &other_gate));
        let mut display_copy = ticket.frame().clone();
        display_copy.run_id = "forged".into();
        display_copy.rgba = Arc::from([9, 9, 9, 9]);
        assert_eq!(ticket.frame().run_id, "run");
        assert_eq!(&*ticket.frame().rgba, &[0, 0, 0, 255]);
    }

    #[test]
    fn static_pixels_are_not_retimed_and_invalid_pixels_cannot_be_reauthorized() {
        let (frames, gate, grant) = setup();
        let mut frame = frames.latest().unwrap().unwrap();
        frame.sequence += 1;
        frame.captured_at -= Duration::from_secs(3);
        frames.publish(frame);
        let observation = ObservedFrame::capture(&frames, &gate, &grant)
            .unwrap()
            .unwrap();
        assert!(observation.frame().captured_at.elapsed() >= Duration::from_secs(3));
        assert_eq!(
            observation.expires_at() - observation.issued_at(),
            Duration::from_secs(60)
        );
        frames.clear();
        assert!(ObservedFrame::capture(&frames, &gate, &grant)
            .unwrap()
            .is_none());
        frames.revoke();
        assert!(ObservedFrame::capture(&frames, &gate, &grant).is_err());
    }

    #[test]
    fn observation_budget_rejects_unbounded_or_zero_lifetime_before_starting() {
        for timeout in [
            Duration::ZERO,
            Duration::from_millis(300_001),
            Duration::MAX,
        ] {
            let mut options = crate::PortalOptions::new("budget".into(), SourceKind::Monitor);
            options.observation_timeout = timeout;
            assert!(crate::PortalSession::start(options)
                .err()
                .unwrap()
                .contains("observation timeout"));
        }
    }

    #[test]
    fn presentation_maps_pixel_centres_not_padding_origin_or_physical_scale() {
        let (frames, gate, mut grant) = setup();
        // None of the portal's compositor-space hints are image pixels.
        grant.stream.compositor_position = Some((-500, 700));
        grant.stream.compositor_size = Some((912, 736));
        grant.stream.logical_size = Some((321, 123));
        for scale in [0.75, 1.0, 1.25, 2.0, 3.0] {
            {
                let InputState::Ready(ref mut c) = *gate.0.lock().unwrap() else {
                    panic!()
                };
                c.absolute_region.as_mut().unwrap().physical_scale = scale;
            }
            let ticket = ObservedFrame::capture(&frames, &gate, &grant)
                .unwrap()
                .unwrap();
            let original_time = ticket.frame().captured_at;
            let original_deadline = ticket.expires_at();
            let presented = ticket.present(100, 100).unwrap();
            assert_eq!(presented.image().rgba(), [0, 0, 0, 255]);
            let (ticket, action) = presented.into_pixel_input(0, 0).unwrap();
            assert!(ticket.belongs_to(&frames, &gate));
            assert_eq!(ticket.frame().captured_at, original_time);
            assert_eq!(ticket.expires_at(), original_deadline);
            assert!(matches!(
                action,
                InputAction::Absolute { x: 400.0, y: 300.0 }
            ));
        }
    }

    #[test]
    fn presentation_cannot_map_outside_image_or_without_unique_region() {
        let (frames, gate, grant) = setup();
        for (x, y) in [(1, 0), (0, 1), (u32::MAX, 0), (0, u32::MAX)] {
            let p = ObservedFrame::capture(&frames, &gate, &grant)
                .unwrap()
                .unwrap()
                .present(10, 10)
                .unwrap();
            assert!(p.into_pixel_input(x, y).is_err());
        }
        {
            let InputState::Ready(ref mut c) = *gate.0.lock().unwrap() else {
                panic!()
            };
            c.absolute_region = None;
        }
        let p = ObservedFrame::capture(&frames, &gate, &grant)
            .unwrap()
            .unwrap()
            .present(10, 10)
            .unwrap();
        assert_eq!(p.image().width(), 1); // Read-only presentation still works.
        assert!(p.into_pixel_input(0, 0).is_err());
    }

    #[test]
    fn presented_pixel_rejects_wire_rounding_into_another_pixel() {
        let (frames, gate, grant) = setup();
        let mut frame = frames.latest().unwrap().unwrap();
        frame.buffer_width = 4;
        frame.region.width = 4;
        frame.rgba = Arc::from([0; 16]);
        frame.sequence += 1;
        frames.publish(frame);
        {
            let InputState::Ready(ref mut c) = *gate.0.lock().unwrap() else {
                panic!()
            };
            let r = c.absolute_region.as_mut().unwrap();
            r.x = 16_777_216;
            r.width = 1;
        }
        let p = ObservedFrame::capture(&frames, &gate, &grant)
            .unwrap()
            .unwrap()
            .present(4, 1)
            .unwrap();
        assert!(p
            .into_pixel_input(3, 0)
            .err()
            .unwrap()
            .contains("precision"));
    }
}

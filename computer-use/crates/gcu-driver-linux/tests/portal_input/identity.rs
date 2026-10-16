//! P7-04 integration: image/window lifetimes are rechecked, never inferred from titles.
#![cfg(unix)]
use super::*;
use async_trait::async_trait;
use gcu_driver_linux::{
    wayland::{
        input_identity::{InputEvidence, VerifiedInputAuthority},
        windows::{IdentityError, ProcessSource, WaylandWindows, WindowBackend, WindowSnapshot},
        Compositor, PortalStream,
    },
    AppIdentity,
};

struct Backend(Mutex<WindowSnapshot>);
#[async_trait]
impl WindowBackend for Backend {
    fn compositor(&self) -> Compositor {
        Compositor::Gnome
    }
    async fn snapshot(&self) -> Result<WindowSnapshot, IdentityError> {
        Ok(self.0.lock().unwrap().clone())
    }
}
struct Processes(Mutex<AppIdentity>);
#[async_trait]
impl ProcessSource for Processes {
    async fn inspect(&self, _pid: u32) -> Result<AppIdentity, IdentityError> {
        Ok(self.0.lock().unwrap().clone())
    }
}
struct Evidence(Mutex<StreamIdentity>);
#[async_trait]
impl InputEvidence for Evidence {
    async fn stream(
        &self,
        _session: &str,
        _stream: &PortalStream,
    ) -> Result<StreamIdentity, DriverError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn confined(&self) -> bool {
        true
    }
}
#[tokio::test(start_paused = true)]
async fn exact_source_proof_rechecks_compositor_window_and_process_image() {
    for case in 0..5 {
        let h = Harness::new();
        let backend = Arc::new(Backend(Mutex::new(WindowSnapshot {
            compositor: Compositor::Gnome,
            epoch: h.binding.epoch.clone(),
            windows: vec![h.binding.record.clone()],
        })));
        let process = Arc::new(Processes(Mutex::new(h.binding.image.clone())));
        let evidence = Arc::new(Evidence(Mutex::new(StreamIdentity {
            session: "/session/1".into(),
            node_id: 42,
            serial: 1,
            source: "source-7".into(),
            window: h.binding.clone(),
        })));
        let authority = Arc::new(VerifiedInputAuthority::new(
            Arc::new(WaylandWindows::new(backend.clone(), process.clone())),
            evidence.clone(),
        ));
        let input = drive(PortalInput::open(
            h.config(),
            &h.portal,
            authority,
            h.stop.clone(),
            &Cancel::default(),
        ))
        .await
        .unwrap();
        match case {
            0 => backend.0.lock().unwrap().epoch = "owner-replaced".into(),
            1 => backend.0.lock().unwrap().windows[0].generation += 1,
            2 => process.0.lock().unwrap().sha256 = "cd".repeat(32),
            3 => process.0.lock().unwrap().start_time += 1,
            _ => evidence.0.lock().unwrap().node_id += 1,
        }
        assert!(input.perform(&h.keys(), &Cancel::default()).await.is_err());
        assert!(h.ei.events().is_empty());
        drive(input.close()).await.unwrap();
    }
}

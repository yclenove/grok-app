//! Policy reads, the no-op overlay, and the recording event sink.
#![cfg(feature = "testing")]

use gcu_host_api::testing::{MemoryPolicyStore, NoopOverlay, RecordingEventSink};
use gcu_host_api::{
    AppIdentity, CursorKind, EventSink, GrantProof, ImageRef, OverlayProvider, Platform,
    PolicyConfig, PolicyStore, ScreenPoint, SessionCtx, SignatureState, StatusText, WindowInfo,
};
use gcu_protocol::result::Outcome;
use gcu_protocol::ToolName;

fn session() -> SessionCtx {
    SessionCtx {
        session_id: "session-7".into(),
    }
}

fn macos_signed() -> AppIdentity {
    AppIdentity {
        platform: Platform::Macos,
        key: "com.example.notes".into(),
        team_id: Some("TEAM123456".into()),
        publisher: Some("Example Publisher".into()),
        sha256: Some([0x11; 32]),
        signature: SignatureState::Valid,
        image_path: "/Applications/Notes.app/Contents/MacOS/Notes".into(),
        proof: GrantProof {
            cdhash: Some("0123456789abcdef0123456789abcdef01234567".into()),
            certificate_sha256: Some("22".repeat(32)),
            product: Some("Notes".into()),
            build: Some("100".into()),
            ..GrantProof::default()
        },
    }
}

fn linux_app() -> AppIdentity {
    AppIdentity {
        platform: Platform::Linux,
        key: "org.example.Notes.desktop".into(),
        team_id: None,
        publisher: None,
        sha256: Some([0x33; 32]),
        signature: SignatureState::Unsigned,
        image_path: "/usr/bin/notes".into(),
        proof: GrantProof {
            file_dev: Some(1),
            file_inode: Some(2),
            ..GrantProof::default()
        },
    }
}

fn engine_view(store: &dyn PolicyStore, id: &AppIdentity) -> (PolicyConfig, bool) {
    let config = store.load();
    store.refresh();
    (config, store.always_allowed(id))
}

#[test]
fn memory_policy_always_allowed_uses_grant_eq() {
    let store = MemoryPolicyStore::new(PolicyConfig::default());
    let granted = macos_signed();
    assert!(!store.always_allowed(&granted));
    store.insert_always(granted.clone());
    store.insert_always(granted.clone());
    assert_eq!(store.load().always.len(), 1);
    assert!(store.always_allowed(&granted));
    let mut moved = granted.clone();
    moved.image_path = "/Applications/Moved.app/Contents/MacOS/Notes".into();
    moved.sha256 = Some([0x55; 32]);
    assert!(!store.always_allowed(&moved));
    assert_eq!(store.load().always[0].image_path, granted.image_path);

    let mut folded = granted.clone();
    folded.proof.cdhash = granted.proof.cdhash.as_deref().map(str::to_ascii_uppercase);
    assert_ne!(granted, folded);
    assert!(store.always_allowed(&folded));
    store.remove_always(&folded);
    assert!(!store.always_allowed(&granted));
    assert!(store.load().always.is_empty());
}

#[test]
fn memory_policy_refresh_swaps_staged_snapshot() {
    let store = MemoryPolicyStore::new(PolicyConfig::default());
    let granted = macos_signed();
    store.insert_always(granted.clone());
    store.refresh();
    assert!(store.always_allowed(&granted));
    assert_eq!(store.load().timeout_secs, 120);

    let staged = PolicyConfig {
        timeout_secs: 10,
        always_allow_enabled: false,
        always: vec![linux_app(), linux_app()],
        extra_denied_apps: vec!["com.example.terminal".into()],
        ..PolicyConfig::default()
    };
    store.stage(staged);
    assert_eq!(store.load().timeout_secs, 120);
    assert!(store.always_allowed(&granted));

    store.refresh();
    let loaded = store.load();
    assert_eq!(loaded.timeout_secs, 10);
    assert!(!loaded.always_allow_enabled);
    assert_eq!(
        loaded.extra_denied_apps,
        vec!["com.example.terminal".to_string()]
    );
    assert_eq!(loaded.always.len(), 1);
    assert!(store.always_allowed(&linux_app()));
    assert!(!store.always_allowed(&granted));
    let (_before, allowed) = engine_view(&store, &linux_app());
    assert!(allowed);
}

#[test]
fn noop_overlay_accepts_calls_and_cannot_exclude_capture() {
    let overlay = NoopOverlay;
    let window = WindowInfo {
        id: "w1".into(),
        title: "Notes".into(),
        frame: gcu_protocol::Rect {
            x: 10.0,
            y: 20.0,
            w: 800.0,
            h: 600.0,
        },
        scale: 2.0,
    };
    let provider: &dyn OverlayProvider = &overlay;
    provider.show_target(
        &session(),
        &window,
        ScreenPoint { x: 40.0, y: 50.0 },
        CursorKind::Pointer,
    );
    provider.status(
        &session(),
        &StatusText {
            message: "Using Notes".into(),
            foreground: true,
        },
    );
    provider.hide(&session());
    assert!(!provider.exclude_from_capture_supported());
}

#[test]
fn recording_event_sink_keeps_thumbnail_bytes() {
    let sink = RecordingEventSink::new();
    let thumbnail = ImageRef::from_bytes("image/png", 1, 1, vec![1, 2, 3]);
    let event_sink: &dyn EventSink = &sink;
    event_sink.emit(
        &session(),
        gcu_host_api::CuEvent {
            tool: ToolName::Click,
            target: "Send".into(),
            outcome: Outcome::Done,
            duration_ms: 12,
            thumbnail: Some(thumbnail.clone()),
        },
    );
    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0.session_id, "session-7");
    assert_eq!(events[0].1.tool, ToolName::Click);
    assert_eq!(events[0].1.outcome, Outcome::Done);
    assert_eq!(events[0].1.duration_ms, 12);
    assert_eq!(
        events[0].1.thumbnail.as_ref().map(ImageRef::as_bytes),
        Some(thumbnail.as_bytes())
    );
    sink.clear();
    assert!(sink.events().is_empty());
}

#[test]
fn host_values_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<gcu_host_api::Cancel>();
    assert_send_sync::<AppIdentity>();
    assert_send_sync::<MemoryPolicyStore>();
    assert_send_sync::<NoopOverlay>();
    assert_send_sync::<RecordingEventSink>();
    assert_send_sync::<gcu_host_api::testing::ScriptedApprovals>();
    assert_send_sync::<gcu_host_api::testing::FakeUserActivity>();
}

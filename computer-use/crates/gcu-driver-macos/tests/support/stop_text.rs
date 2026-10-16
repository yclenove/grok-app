//! Text/clipboard integration with the same Stop latch and virtual clock.
#[path = "text.rs"]
mod text_support;

use super::Fixture;
use gcu_core::driver::{Cancel, Delivery, DriverError, DRIVER_DEADLINE};
use gcu_driver_macos::input::{self, clock::Clock, Call, EventApi};

fn clipboard_allowed() -> input::text::TextOptions {
    input::text::TextOptions {
        allow_clipboard: true,
        sensitive_text: false,
        high_risk_target: false,
    }
}

#[tokio::test]
async fn stop_during_paste_aborts_and_conditionally_restores_without_foreign_overwrite() {
    for foreign in [false, true] {
        let f = Fixture::new();
        let rig = text_support::Rig::rich();
        rig.events.set_guards(f.guards.clone());
        rig.text.hold_observed();
        let original = rig.board.snapshot().content;
        let mut ctx = rig.context();
        ctx.clock = f.clock.clone();
        let target = rig.target.clone();
        let task = tokio::spawn(async move {
            input::text::type_text(
                &ctx,
                &target,
                None,
                &"x".repeat(2001),
                gcu_core::driver::KeyRoute::AllowForeground,
                clipboard_allowed(),
                &Cancel::default(),
                DRIVER_DEADLINE,
            )
            .await
        });
        rig.text.observed.notified().await;
        if foreign {
            rig.board.foreign("external");
        }
        let stopped_at = f.clock.now();
        f.guards.request_stop();
        assert_eq!(
            task.await.unwrap(),
            Err(DriverError::Cancelled {
                maybe_executed: true
            })
        );
        // A completed next transaction proves the stopped worker released its
        // global lock only after finishing conditional native cleanup.
        text_support::Rig::rich()
            .run(&"y".repeat(2001), clipboard_allowed())
            .await
            .unwrap();
        assert!(rig.ledger.is_empty());
        assert!(
            rig.board.snapshot().content
                == if foreign {
                    text_support::content("external")
                } else {
                    original
                }
        );
        assert_eq!(rig.board.writes(), if foreign { 1 } else { 2 });
        assert!(f.clock.now() - stopped_at <= input::watchdog::RELEASE_DEADLINE);
    }
}

#[tokio::test]
async fn stop_during_paste_key_down_sends_up_and_restores_owned_clipboard() {
    let f = Fixture::new();
    let rig = text_support::Rig::rich();
    rig.events.set_guards(f.guards.clone());
    rig.events.hold_posts();
    let original = rig.board.snapshot().content;
    let mut ctx = rig.context();
    ctx.clock = f.clock.clone();
    let target = rig.target.clone();
    let task = tokio::spawn(async move {
        input::text::type_text(
            &ctx,
            &target,
            None,
            &"x".repeat(2001),
            gcu_core::driver::KeyRoute::AllowForeground,
            clipboard_allowed(),
            &Cancel::default(),
            DRIVER_DEADLINE,
        )
        .await
    });
    while rig.events.calls().is_empty() {
        tokio::task::yield_now().await;
    }
    f.guards.request_stop();
    assert_eq!(
        task.await.unwrap(),
        Err(DriverError::Cancelled {
            maybe_executed: true
        })
    );
    text_support::Rig::rich()
        .run(&"y".repeat(2001), clipboard_allowed())
        .await
        .unwrap();
    assert!(rig.ledger.is_empty());
    assert!(rig.board.snapshot().content == original);
    assert!(rig
        .events
        .calls()
        .iter()
        .any(|c| matches!(c, Call::Post(e) if !e.down && e.keycode == Some(0x37))));
    assert!(!rig
        .events
        .calls()
        .iter()
        .any(|c| matches!(c, Call::Post(e) if e.down && e.keycode == Some(0x09))));
}

#[tokio::test]
async fn text_guards_stopped_refuse_all_routes_and_unavailable_foreground_keeps_background() {
    for plain in [false, true] {
        let f = Fixture::new();
        let rig = if plain {
            text_support::Rig::plain("prefix")
        } else {
            text_support::Rig::rich()
        };
        rig.events.set_guards(f.guards.clone());
        f.guards.request_stop();
        assert_eq!(
            rig.run("x", input::text::TextOptions::default()).await,
            Err(DriverError::Cancelled {
                maybe_executed: false
            })
        );
        assert!(rig.events.calls().is_empty());
        assert_eq!(rig.board.writes(), 0);
    }
    for unavailable in 0..3 {
        let f = Fixture::new();
        let rig = text_support::Rig::rich();
        rig.events.set_guards(f.guards.clone());
        match unavailable {
            0 => f.monitor.hotkey_registered(false),
            1 => f.monitor.observe(0),
            _ => f.monitor.connected(false),
        }
        assert_eq!(
            rig.run(&"x".repeat(2001), clipboard_allowed()).await,
            Err(DriverError::CapabilityUnavailable)
        );
        assert_eq!(rig.board.writes(), 0);
        assert_eq!(
            rig.run("x", input::text::TextOptions::default())
                .await
                .unwrap(),
            Delivery::TargetedEvent
        );
        let mut probe = rig.events.probe();
        probe.field_51 = false;
        rig.events.set_probe(probe);
        assert_eq!(
            rig.run("x", input::text::TextOptions::default()).await,
            Err(DriverError::CapabilityUnavailable)
        );
        assert!(!rig
            .events
            .calls()
            .iter()
            .any(|c| matches!(c, Call::Activate(_))));
    }
}

#[tokio::test]
async fn stop_while_waiting_for_clipboard_lock_refuses_before_write() {
    let first = std::sync::Arc::new(text_support::Rig::rich());
    first.text.hold_observed();
    let running = first.clone();
    let task =
        tokio::spawn(async move { running.run(&"x".repeat(2001), clipboard_allowed()).await });
    first.text.observed.notified().await;
    let f = Fixture::new();
    let rig = text_support::Rig::rich();
    rig.events.set_guards(f.guards.clone());
    let payload = "y".repeat(2001);
    let pending = rig.run(&payload, clipboard_allowed());
    tokio::pin!(pending);
    tokio::select! {
        biased;
        result = &mut pending => panic!("paste escaped global lock: {result:?}"),
        _ = tokio::task::yield_now() => {},
    }
    f.guards.request_stop();
    assert_eq!(
        pending.await,
        Err(DriverError::Cancelled {
            maybe_executed: false
        })
    );
    assert_eq!(rig.board.writes(), 0);
    assert!(rig.events.calls().is_empty());
    first.text.resume.notify_one();
    assert_eq!(task.await.unwrap().unwrap(), Delivery::Clipboard);
}

//! Real background selection ownership through the production LinuxAdapter.
//! No global user clipboard, input callback completion or App exit claim.
use super::*;
use crate::clipboard::{ClipboardMonitor, RetentionState, RetentionStatus};
use crate::LinuxAdapter;
use grok_computer_use_core::adapter::ComputerUseAdapter;

fn wait(
    monitor: &ClipboardMonitor,
    condition: impl Fn(&RetentionStatus) -> bool,
) -> Result<RetentionStatus, String> {
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        let status = monitor.status();
        if condition(&status) {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err(format!("clipboard retention deadline: {status:?}"));
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
pub(super) fn closed(monitor: &ClipboardMonitor) -> Result<(), String> {
    wait(monitor, |s| {
        s.state == RetentionState::Closed && s.worker_finished
    })?;
    Ok(())
}
fn read_background(expected: &FormatBytes) -> Result<(), String> {
    let actual = ClipboardReader::connect()?.snapshot(|| Ok(()))?;
    require(
        sort(bytes(&actual)) == sort(expected.clone()),
        "background owner changed saved format bytes",
    )
}
fn notify_background(client: &Requestor) -> Result<SelectionNotifyEvent, String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        while let Some(event) = client.conn.poll_for_event().map_err(err)? {
            if let Event::SelectionNotify(n) = event {
                return Ok(n);
            }
        }
        if Instant::now() >= deadline {
            return Err("background conversion notify deadline".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
pub(super) fn receive_background(client: &Requestor) -> Result<Vec<u8>, String> {
    require(
        client.property(client.property, false)?.type_ == client.incr,
        "background INCR header missing",
    )?;
    client
        .conn
        .delete_property(client.window, client.property)
        .map_err(err)?
        .check()
        .map_err(err)?;
    let mut bytes = Vec::new();
    let mut shape = None;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let p = client.property(client.property, true)?;
        if p.type_ != 0 {
            require(
                shape.is_none_or(|s| s == (p.type_, p.format)),
                "background INCR changed width or type",
            )?;
            shape = Some((p.type_, p.format));
            require(
                p.bytes_after == 0 && bytes.len() + p.value.len() <= MAX_FORMAT_BYTES,
                "background INCR exceeded bound",
            )?;
            if p.value.is_empty() {
                return Ok(bytes);
            }
            bytes.extend(p.value);
        }
        if Instant::now() >= deadline {
            return Err("background INCR terminator deadline".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

pub(super) fn run() -> Result<(), String> {
    let user = Requestor::new()?;
    {
        let adapter = LinuxAdapter::new();
        let original = Owner::start(Mode::Formats)?;
        let (owner, lease) = prepared()?;
        let (mut unused, wrong) = prepared()?;
        unused.close()?;
        let mut pending = Some((owner, lease));
        require(
            adapter.retain_restored_clipboard(&mut pending).is_err() && pending.is_some(),
            "prepared original moved despite retention rejection",
        )?;
        let (owner, lease) = pending.as_mut().ok_or("lost prepared caller asset")?;
        owner.publish(lease, "temporary text", &Default::default())?;
        require(
            adapter.retain_restored_clipboard(&mut pending).is_err() && pending.is_some(),
            "Published task moved into retention",
        )?;
        let (owner, lease) = pending.as_mut().ok_or("lost published caller asset")?;
        owner.restore(lease)?;
        let (owner, correct) = pending.take().ok_or("lost restored caller asset")?;
        let mut wrong_pair = Some((owner, wrong));
        require(
            adapter.retain_restored_clipboard(&mut wrong_pair).is_err() && wrong_pair.is_some(),
            "foreign lease moved original into worker",
        )?;
        let (owner, _) = wrong_pair.take().ok_or("lost foreign-lease caller asset")?;
        pending = Some((owner, correct));
        let handle = adapter.retain_restored_clipboard(&mut pending)?;
        require(
            pending.is_none(),
            "successful retention left two service owners",
        )?;
        let monitor = handle.monitor();
        user.claim(user.window)?;
        closed(&monitor)?;
        require(
            adapter.clipboard_retention_statuses().is_empty(),
            "closed worker not reaped from Host status",
        )?;
        drop(original);
        user.claim(0)?;
        println!("PASS clipboard Host retention rejects un-restored task and foreign lease without consuming caller ownership");
    }
    {
        let adapter = LinuxAdapter::new();
        let (owner, lease, expected) = restored(Mode::LargeFormats)?;
        let mut pending = Some((owner, lease));
        let handle = adapter.retain_restored_clipboard(&mut pending)?;
        let monitor = handle.monitor();
        wait(&monitor, |s| s.state == RetentionState::Serving)?;
        read_background(&expected)?;
        let started = Instant::now();
        adapter.abort("retention-fixture", 2)?;
        adapter.release_target_for_run("retention-fixture", "owned-clipboard-fixture");
        require(
            started.elapsed() < Duration::from_millis(300),
            "Host abort/release blocked on clipboard worker",
        )?;
        read_background(&expected)?;
        require(
            adapter.is_idle("retention-fixture"),
            "retaining original clipboard incorrectly occupies native input",
        )?;
        drop(handle);
        drop(adapter);
        wait(&monitor, |s| s.handoff == HandoffStatus::Unavailable)?;
        require(
            !monitor.status().worker_finished,
            "dropping controls killed retained service without manager",
        )?;
        read_background(&expected)?;
        user.claim(user.window)?;
        closed(&monitor)?;
        require(
            user.selected()? == user.window,
            "orphan retirement restored over new user copy",
        )?;
        user.claim(0)?;
        println!("PASS clipboard background original survives Host abort release and all control drops without a manager");
    }
    {
        let adapter = LinuxAdapter::new();
        let (owner, lease, expected) = restored(Mode::Formats)?;
        let mut pending = Some((owner, lease));
        let handle = adapter.retain_restored_clipboard(&mut pending)?;
        let monitor = handle.monitor();
        adapter.request_clipboard_handoff();
        wait(&monitor, |s| s.handoff == HandoffStatus::Unavailable)?;
        let manager = Manager::start(Behavior::Good)?;
        handle.request_handoff();
        closed(&monitor)?;
        require(
            monitor.status().handoff == HandoffStatus::Acknowledged
                && manager.requests.load(Ordering::Acquire) == 1,
            "late manager did not receive one exact request",
        )?;
        read_background(&expected)?;
        manager.stop()?;
        println!("PASS clipboard retained original hands off once when a manager appears after initial absence");
    }
    {
        let adapter = LinuxAdapter::new();
        let (owner, lease, expected) = restored(Mode::LargeFormats)?;
        let mut pending = Some((owner, lease));
        let handle = adapter.retain_restored_clipboard(&mut pending)?;
        let monitor = handle.monitor();
        let reader = Requestor::new()?;
        reader.begin(reader.utf8, reader.property, 0)?;
        require(
            notify_background(&reader)?.property == reader.property,
            "background INCR request refused",
        )?;
        require(
            reader.property(reader.property, false)?.type_ == reader.incr,
            "background test did not enter INCR",
        )?;
        let manager = Manager::start(Behavior::Good)?;
        drop(handle);
        drop(adapter);
        wait(&monitor, |s| s.handoff == HandoffStatus::Acknowledged)?;
        require(
            !monitor.status().worker_finished && monitor.status().state != RetentionState::Closed,
            "handoff killed another accepted INCR reader",
        )?;
        let task = receive_background(&reader)?;
        require(
            task == expected
                .iter()
                .find(|f| f.0 == reader.utf8)
                .ok_or("missing original UTF8")?
                .3,
            "retained INCR changed saved bytes",
        )?;
        closed(&monitor)?;
        read_background(&expected)?;
        manager.stop()?;
        println!("PASS clipboard Host drop and manager acknowledgement keep accepted INCR alive through final deletion");
    }
    for behavior in [Behavior::Silent, Behavior::EarlyAck] {
        let manager = Manager::start(behavior)?;
        let adapter = LinuxAdapter::new();
        let (owner, lease, _) = restored(Mode::Formats)?;
        let mut pending = Some((owner, lease));
        let handle = adapter.retain_restored_clipboard(&mut pending)?;
        let monitor = handle.monitor();
        drop(handle);
        drop(adapter);
        let wanted = if behavior == Behavior::Silent {
            HandoffStatus::Pending
        } else {
            HandoffStatus::Unknown
        };
        wait(&monitor, |s| s.handoff == wanted)?;
        require(
            !monitor.status().worker_finished && monitor.status().state != RetentionState::Closed,
            "unproven manager reply released orphan service",
        )?;
        user.claim(user.window)?;
        closed(&monitor)?;
        require(
            monitor.status().handoff == HandoffStatus::Interrupted
                && user.selected()? == user.window
                && manager.requests.load(Ordering::Acquire) == 1,
            "orphan handoff replayed or overwrote newer clipboard",
        )?;
        manager.stop()?;
        user.claim(0)?;
        println!("PASS clipboard orphan {behavior:?} handoff stays retained until native ownership change without replay");
    }
    {
        let manager = Manager::start(Behavior::Good)?;
        let adapter = LinuxAdapter::new();
        let cancellation = ActionCancellation::default();
        let mut input = adapter
            .input
            .begin("unknown-native-fixture", 1, &cancellation)?;
        input.retain_until_native_recovery();
        drop(input);
        let (owner, lease, _) = restored(Mode::Formats)?;
        let mut pending = Some((owner, lease));
        let handle = adapter.retain_restored_clipboard(&mut pending)?;
        let monitor = handle.monitor();
        adapter.request_clipboard_handoff();
        closed(&monitor)?;
        adapter.abort("unknown-native-fixture", 2)?;
        require(
            !adapter.is_idle("unknown-native-fixture"),
            "clipboard acknowledgement released unrelated unknown native input",
        )?;
        manager.stop()?;
        println!("PASS clipboard worker retirement cannot confirm or release unknown native input occupancy");
    }
    {
        // An empty original needs no history-manager handoff, but an already
        // accepted task INCR still has to finish after restoration to NONE.
        user.claim(0)?;
        let manager = Manager::start(Behavior::Good)?;
        let adapter = LinuxAdapter::new();
        let (mut owner, lease) = prepared()?;
        let text = "t".repeat(32768);
        owner.publish(&lease, &text, &Default::default())?;
        let reader = Requestor::new()?;
        reader.begin(reader.utf8, reader.property, 0)?;
        require(
            reader.notify(&mut owner)?.property == reader.property
                && reader.property(reader.property, false)?.type_ == reader.incr,
            "empty-original test did not enter accepted task INCR",
        )?;
        owner.restore(&lease)?;
        let mut pending = Some((owner, lease));
        let handle = adapter.retain_restored_clipboard(&mut pending)?;
        let monitor = handle.monitor();
        drop(handle);
        drop(adapter);
        wait(&monitor, |s| s.state == RetentionState::Serving)?;
        // Allow several worker turns without acknowledging the header.
        std::thread::sleep(Duration::from_millis(30));
        let status = monitor.status();
        require(
            status.state == RetentionState::Serving
                && status.handoff == HandoffStatus::NotRequested
                && status.error.is_none()
                && !status.worker_finished,
            "empty original dispatched handoff, faulted or abandoned accepted INCR",
        )?;
        require(
            receive_background(&reader)? == text.as_bytes(),
            "empty-original restoration truncated accepted task bytes",
        )?;
        closed(&monitor)?;
        require(
            user.selected()? == 0 && manager.requests.load(Ordering::Acquire) == 0,
            "empty original was replaced or task was offered to manager",
        )?;
        manager.stop()?;
        println!("PASS clipboard empty original stays NONE while orphan task INCR completes without manager dispatch");
    }
    Ok(())
}

//! Owned-Xvfb subprocess acceptance. The simulated Host really exits or is
//! killed; a third X client then reads the keeper, not a cached test value.
use super::*;
use crate::clipboard::{KeeperProcess, KeeperStatus};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

fn wait_process(process: &KeeperProcess) -> Result<(), String> {
    let until = Instant::now() + Duration::from_secs(4);
    while !process.finished() {
        if Instant::now() >= until {
            return Err("keeper actual process has not exited".into());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Ok(())
}
fn until_keeper(owner: &mut ClipboardOwner, status: KeeperStatus) -> Result<(), String> {
    let until = Instant::now() + Duration::from_secs(6);
    while owner.keeper_status() != status {
        owner.pump()?;
        if Instant::now() >= until {
            return Err(format!(
                "keeper expected {status:?}, got {:?}",
                owner.keeper_status()
            ));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}
fn read_independent(expected: &FormatBytes) -> Result<(), String> {
    require(
        sort(bytes(&ClipboardReader::connect()?.snapshot(|| Ok(()))?)) == sort(expected.clone()),
        "process keeper changed original format bytes",
    )
}
pub fn run_keeper_parent_fixture(mode: &str) -> Result<(), String> {
    if std::env::var("GROK_CU_X11_FIXTURE").as_deref() != Ok("owned-xvfb") {
        return Err("keeper parent fixture requires owned Xvfb".into());
    }
    let selected = match mode {
        "direct" => Mode::Formats,
        "large" | "crash" => Mode::LargeFormats,
        _ => return Err("unknown keeper parent fixture".into()),
    };
    let (mut owner, lease, _) = restored(selected)?;
    owner.begin_keeper(&lease, &Default::default())?;
    until_keeper(&mut owner, KeeperStatus::Transferred)?;
    owner.close()?;
    let process = owner
        .keeper_process()
        .ok_or("keeper process witness missing")?;
    require(
        !process.finished(),
        "keeper exited while it still owns original",
    )?;
    println!("KEEPER_READY {}", process.pid);
    std::io::stdout().flush().map_err(err)?;
    if mode == "crash" {
        loop {
            std::thread::park_timeout(Duration::from_secs(60));
        }
    }
    Ok(())
}

// A start-time witness prevents a reused numeric PID from being considered the
// same helper. This is observation only; no PID-based kill is ever issued.
fn process_identity(pid: u32) -> Result<Option<(String, char)>, String> {
    let stat = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(err(e)),
    };
    let fields: Vec<_> = stat
        .rsplit_once(')')
        .ok_or("bad proc stat")?
        .1
        .split_whitespace()
        .collect();
    Ok(Some((
        fields
            .get(19)
            .ok_or("missing process start time")?
            .to_string(),
        fields[0].chars().next().ok_or("missing process state")?,
    )))
}
fn wait_orphan_exit(pid: u32, start: &str) -> Result<(), String> {
    let until = Instant::now() + Duration::from_secs(4);
    loop {
        match process_identity(pid)? {
            None => return Ok(()),
            Some((s, state)) if s != start || matches!(state, 'Z' | 'X') => return Ok(()),
            _ => (),
        }
        if Instant::now() >= until {
            return Err("orphan keeper did not terminate after new copy".into());
        }
        std::thread::sleep(Duration::from_millis(3));
    }
}

pub(super) fn run() -> Result<(), String> {
    let user = Requestor::new()?;
    {
        let status = Command::new("/proc/self/exe")
            .arg("--computer-use-x11-clipboard-keeper-v1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(err)?;
        require(
            !status.success() && user.selected()? == 0,
            "unbootstrapped keeper touched selection",
        )?;
        println!("PASS clipboard keeper CLI refuses missing private bootstrap without changing selection");
    }
    {
        let original = Owner::start(Mode::Formats)?;
        let (mut owner, lease) = prepared()?;
        let (mut other, foreign) = prepared()?;
        other.close()?;
        require(
            owner.begin_keeper(&lease, &Default::default()).is_err()
                && owner.keeper_process().is_none(),
            "prepared service started keeper",
        )?;
        owner.publish(&lease, "task only", &Default::default())?;
        require(
            owner.begin_keeper(&lease, &Default::default()).is_err()
                && owner.keeper_process().is_none(),
            "published task started keeper",
        )?;
        owner.restore(&lease)?;
        require(
            owner.begin_keeper(&foreign, &Default::default()).is_err()
                && owner.keeper_process().is_none(),
            "foreign lease started keeper",
        )?;
        let cancel = ActionCancellation::default();
        cancel.cancel();
        require(
            owner.begin_keeper(&lease, &cancel).is_err() && owner.keeper_process().is_none(),
            "pre-cancel started keeper",
        )?;
        user.claim(user.window)?;
        owner.close()?;
        drop(original);
        user.claim(0)?;
        println!("PASS clipboard keeper admission rejects prepared task foreign lease and pre-cancel with no child");
    }
    for mode in ["direct", "large", "crash"] {
        let original = Owner::start(if mode == "direct" {
            Mode::Formats
        } else {
            Mode::LargeFormats
        })?;
        let expected = bytes(&ClipboardReader::connect()?.snapshot(|| Ok(()))?);
        drop(original);
        let mut parent = Command::new("/proc/self/exe")
            .args(["--clipboard-keeper-parent-fixture", mode])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(err)?;
        let stdout = parent
            .stdout
            .take()
            .ok_or("missing parent fixture output")?;
        let (tx, rx) = mpsc::sync_channel(1);
        let reader = std::thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(stdout)
                .take(128)
                .read_line(&mut line)
                .map(|_| line);
            let _ = tx.send(result);
        });
        let result = (|| {
            let line = rx
                .recv_timeout(Duration::from_secs(8))
                .map_err(|_| "parent fixture did not acknowledge real keeper")?
                .map_err(err)?;
            let pid = line
                .trim()
                .strip_prefix("KEEPER_READY ")
                .ok_or("bad parent fixture receipt")?
                .parse::<u32>()
                .map_err(err)?;
            let (start, state) =
                process_identity(pid)?.ok_or("keeper absent before parent exit")?;
            require(!matches!(state, 'Z' | 'X'), "keeper already terminated")?;
            if mode == "crash" {
                parent.kill().map_err(err)?;
            }
            let exit = parent.wait().map_err(err)?;
            require(
                if mode == "crash" {
                    !exit.success()
                } else {
                    exit.success()
                },
                "parent fixture exit evidence mismatch",
            )?;
            let current = process_identity(pid)?.ok_or("keeper died with parent")?;
            require(
                current.0 == start && !matches!(current.1, 'Z' | 'X'),
                "parent exit killed or replaced keeper",
            )?;
            read_independent(&expected)?;
            user.claim(user.window)?;
            wait_orphan_exit(pid, &start)?;
            require(
                user.selected()? == user.window,
                "keeper overwrote newer copy while retiring",
            )?;
            Ok::<(), String>(())
        })();
        // Only this owned child handle can be killed. A helper PID is never
        // killed; moving selection to the private user peer lets it retire.
        let _ = user.claim(user.window);
        let _ = parent.kill();
        let _ = parent.wait();
        let _ = reader.join();
        user.claim(0)?;
        result?;
        println!("PASS clipboard keeper {mode} preserves exact 8/16/32-bit originals after actual Host process exit");
    }
    {
        let (mut owner, lease, expected) = restored(Mode::Formats)?;
        owner.begin_keeper(&lease, &Default::default())?;
        let process = owner.keeper_process().ok_or("missing keeper process")?;
        require(
            owner.begin_handoff(&lease, &Default::default()).is_err()
                && owner.begin_keeper(&lease, &Default::default()).is_err(),
            "pending keeper allowed competing manager or duplicate keeper",
        )?;
        until_keeper(&mut owner, KeeperStatus::Transferred)?;
        owner.close()?;
        read_independent(&expected)?;
        user.claim(user.window)?;
        wait_process(&process)?;
        user.claim(0)?;
        println!("PASS clipboard pending keeper rejects competing manager and duplicate process handoffs");
    }
    for label in ["same-owner", "ABA"] {
        let (mut owner, lease, _) = restored(Mode::Formats)?;
        let old = user.selected()?;
        owner.begin_keeper(&lease, &Default::default())?;
        let process = owner
            .keeper_process()
            .ok_or("missing uncommitted keeper process")?;
        // Bootstrap is not flushed until pump, so this precedes the child's
        // snapshot. Only the parent's retained epoch catches this copy.
        if label == "ABA" {
            user.claim(user.window)?;
        }
        user.claim(old)?;
        let until = Instant::now() + Duration::from_secs(6);
        while owner.keeper_status() != KeeperStatus::Failed {
            let _ = owner.pump();
            require(
                user.selected()? == old,
                "prepared keeper replaced same-window or ABA copy",
            )?;
            if Instant::now() >= until {
                return Err("keeper did not reject stale parent epoch".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        wait_process(&process)?;
        require(
            user.selected()? == old,
            "failed keeper changed source ownership",
        )?;
        user.claim(user.window)?;
        owner.close()?;
        user.claim(0)?;
        println!("PASS clipboard keeper rejects {label} copy before child snapshot using retained parent epoch");
    }
    {
        let adapter = crate::LinuxAdapter::new();
        let (mut owner, lease, expected) = restored(Mode::LargeFormats)?;
        let reader = Requestor::new()?;
        reader.begin(reader.utf8, reader.property, 0)?;
        reader.notify(&mut owner)?;
        require(
            reader.property(reader.property, false)?.type_ == reader.incr,
            "keeper test did not start old INCR",
        )?;
        let mut pending = Some((owner, lease));
        let handle = adapter.retain_restored_clipboard(&mut pending)?;
        let monitor = handle.monitor();
        handle.request_process_handoff();
        let until = Instant::now() + Duration::from_secs(5);
        while monitor.status().keeper != KeeperStatus::Transferred {
            if Instant::now() >= until {
                return Err(format!(
                    "background keeper deadline: {:?}",
                    monitor.status()
                ));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let process = monitor
            .keeper_process()
            .ok_or("missing background keeper witness")?;
        require(
            !monitor.status().worker_finished,
            "keeper cut off prior INCR reader",
        )?;
        drop(handle);
        drop(adapter);
        let received = retention_fixture::receive_background(&reader)?;
        require(
            received
                == expected
                    .iter()
                    .find(|f| f.0 == reader.utf8)
                    .ok_or("missing UTF8")?
                    .3,
            "keeper changed accepted original INCR",
        )?;
        retention_fixture::closed(&monitor)?;
        require(
            !process.finished(),
            "worker retirement killed process keeper",
        )?;
        read_independent(&expected)?;
        user.claim(user.window)?;
        wait_process(&process)?;
        user.claim(0)?;
        println!("PASS clipboard background keeper outlives all Host controls without truncating accepted INCR");
    }
    for case in ["late", "superseded", "ABA"] {
        let (mut owner, lease, expected) = restored(Mode::Formats)?;
        owner.begin_keeper(&lease, &Default::default())?;
        until_keeper(&mut owner, KeeperStatus::Committing)?;
        let process = owner
            .keeper_process()
            .ok_or("missing late keeper witness")?;
        let deadline = owner.fixture_flush_keeper_commit()?;
        std::thread::sleep(
            deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(20),
        );
        require(
            owner.pump().is_err() && owner.keeper_status() == KeeperStatus::Unknown,
            "keeper deadline did not preserve unknown native ownership",
        )?;
        read_independent(&expected)?;
        require(
            !process.finished(),
            "unacknowledged keeper exited with owned data",
        )?;
        if case == "superseded" {
            user.claim(user.window)?;
            until_keeper(&mut owner, KeeperStatus::Superseded)?;
            require(
                user.selected()? == user.window,
                "late keeper reply overwrote newer copy",
            )?;
        } else if case == "ABA" {
            let keeper_window = user.selected()?;
            user.conn.grab_server().map_err(err)?.check().map_err(err)?;
            let change = user
                .claim(user.window)
                .and_then(|()| user.claim(keeper_window));
            let cleanup = user.conn.ungrab_server().map_err(err)?.check().map_err(err);
            cleanup?;
            change?;
            require(
                owner.pump().is_err() && owner.keeper_status() == KeeperStatus::Unknown,
                "late keeper acknowledgement accepted ABA ownership as original transfer",
            )?;
            require(
                user.selected()? == keeper_window,
                "late keeper recovery mutated ABA ownership",
            )?;
            user.claim(user.window)?;
            until_keeper(&mut owner, KeeperStatus::Superseded)?;
        } else {
            until_keeper(&mut owner, KeeperStatus::Transferred)?;
            read_independent(&expected)?;
            require(
                owner.keeper_process().is_some_and(|p| p.pid == process.pid),
                "late acknowledgement spawned a replacement",
            )?;
        }
        owner.close()?;
        user.claim(user.window)?;
        wait_process(&process)?;
        user.claim(0)?;
        if case == "superseded" {
            println!(
                "PASS clipboard late keeper acknowledgement cannot supersede a newer user copy"
            );
        } else if case == "ABA" {
            println!("PASS clipboard late keeper acknowledgement rejects ABA despite matching child window");
        } else {
            println!("PASS clipboard keeper recovers original acknowledgement after real deadline without replay");
        }
    }
    Ok(())
}

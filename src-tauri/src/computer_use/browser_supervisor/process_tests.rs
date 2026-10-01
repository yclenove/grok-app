//! Real owned Node children and kernel Jobs; never select a user's process.

use super::*;
use crate::computer_use::browser_supervisor::process_tree::active_job_processes;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{mpsc, Arc};

struct OwnedChild(Option<Child>);

struct OwnedTestJob(Option<windows::Win32::Foundation::HANDLE>);

impl Drop for OwnedTestJob {
    fn drop(&mut self) {
        if let Some(job) = self.0.take() {
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(job);
            }
        }
    }
}

impl std::ops::Deref for OwnedChild {
    type Target = Child;
    fn deref(&self) -> &Child {
        self.0.as_ref().unwrap()
    }
}

impl std::ops::DerefMut for OwnedChild {
    fn deref_mut(&mut self) -> &mut Child {
        self.0.as_mut().unwrap()
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn child(script: &str) -> OwnedChild {
    use std::os::windows::process::CommandExt;
    let node =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/computer-use/seed/bin/node.exe");
    assert!(
        node.is_file(),
        "native supervisor tests require the pinned Node seed"
    );
    let mut command = Command::new(node);
    apply_worker_env(&mut command);
    OwnedChild(Some(
        command
            .args(["-e", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap(),
    ))
}

fn supervisor() -> (BrowserSupervisor, u32) {
    let mut owned = child(
        r#"
      process.stdin.once('data', () => {
        const kid = require('node:child_process').spawn(process.execPath,
          ['-e', 'setInterval(() => {}, 1000)'], {stdio:'ignore', windowsHide:true});
        process.stdout.write(String(kid.pid) + '\n');
      });
      setInterval(() => {}, 1000);
    "#,
    );
    let job = assign_job(&owned).unwrap();
    let mut owned_job = OwnedTestJob(Some(job));
    // The descendant starts only after the root is owned by this kernel Job.
    owned.stdin.take().unwrap().write_all(b"start\n").unwrap();
    let stdout = owned.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(BufReader::new(stdout).lines().next().unwrap().unwrap());
    });
    let pid = owned.id();
    let descendant = rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .parse::<u32>()
        .unwrap();
    assert!(descendant > 0);
    // Move the Child out without suppressing any failure-path cleanup.
    let child = owned.0.take().unwrap();
    (
        BrowserSupervisor {
            child: Mutex::new(child),
            closed: Mutex::new(false),
            shutdown_requested: AtomicBool::new(false),
            pid,
            base: "http://127.0.0.1:0".into(),
            token: "owned-test-token".into(),
            profile_root: PathBuf::new(),
            job: owned_job.0.take().unwrap(),
        },
        descendant,
    )
}

fn job_members(supervisor: &BrowserSupervisor) -> Vec<usize> {
    use windows::Win32::System::JobObjects::{
        JobObjectBasicProcessIdList, QueryInformationJobObject,
    };
    #[repr(C)]
    struct ProcessList {
        assigned: u32,
        returned: u32,
        ids: [usize; 32],
    }
    let mut list = ProcessList {
        assigned: 0,
        returned: 0,
        ids: [0; 32],
    };
    unsafe {
        QueryInformationJobObject(
            Some(supervisor.job),
            JobObjectBasicProcessIdList,
            &mut list as *mut _ as *mut _,
            std::mem::size_of_val(&list) as u32,
            None,
        )
        .unwrap();
    }
    assert!(list.returned <= 32);
    let ids = list.ids[..list.returned as usize].to_vec();
    println!("owned job members={ids:?}");
    ids
}

#[test]
fn owned_job_teardown_does_not_kill_an_unrelated_diagnostic_pid() {
    let (supervisor, descendant) = supervisor();
    let mut decoy = child("setInterval(() => {}, 1000)");
    let members = job_members(&supervisor);
    assert!(members.contains(&(supervisor.pid as usize)));
    assert!(members.contains(&(descendant as usize)));
    assert!(!members.contains(&(decoy.id() as usize)));
    // Simulate an untrusted or reused PID supplied by diagnostics.
    supervisor.shutdown_with(&[decoy.id()]).unwrap();
    assert_eq!(active_job_processes(supervisor.job).unwrap(), 0);
    assert!(supervisor
        .child
        .lock()
        .unwrap()
        .try_wait()
        .unwrap()
        .is_some());
    assert!(
        decoy.try_wait().unwrap().is_none(),
        "unrelated owned decoy must survive"
    );
}

#[test]
fn simultaneous_shutdown_waits_for_the_same_physical_teardown() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let (mut supervisor, _) = supervisor();
    supervisor.base = format!("http://{}", listener.local_addr().unwrap());
    let supervisor = Arc::new(supervisor);
    let (accepted_tx, accepted_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut buf = [0; 1024];
        assert!(socket.read(&mut buf).unwrap() > 0);
        accepted_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
            .unwrap();
    });
    let first = {
        let s = supervisor.clone();
        std::thread::spawn(move || s.shutdown())
    };
    accepted_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let second = {
        let s = supervisor.clone();
        std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            done_tx.send(s.shutdown()).unwrap();
        })
    };
    started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let premature = done_rx.recv_timeout(Duration::from_millis(150));
    release_tx.send(()).unwrap();
    first.join().unwrap().unwrap();
    server.join().unwrap();
    assert!(
        matches!(premature, Err(mpsc::RecvTimeoutError::Timeout)),
        "second shutdown returned before owned teardown"
    );
    done_rx
        .recv_timeout(Duration::from_secs(3))
        .unwrap()
        .unwrap();
    second.join().unwrap();
    assert_eq!(active_job_processes(supervisor.job).unwrap(), 0);
}

#[test]
fn job_failure_is_not_reported_as_closed_and_can_be_rechecked() {
    let (mut supervisor, descendant) = supervisor();
    let job = supervisor.job;
    supervisor.job = windows::Win32::Foundation::HANDLE::default();
    let failed = supervisor.shutdown();
    supervisor.job = job;
    assert!(failed.is_err());
    assert!(!*supervisor.closed.lock().unwrap());
    let members = job_members(&supervisor);
    assert!(members.contains(&(supervisor.pid as usize)));
    assert!(members.contains(&(descendant as usize)));
    supervisor.shutdown().unwrap();
    assert!(*supervisor.closed.lock().unwrap());
}

#[test]
fn product_slot_retains_the_exact_worker_when_shutdown_fails() {
    let (mut supervisor, descendant) = supervisor();
    let expected_pid = supervisor.pid;
    let job = supervisor.job;
    // Keep the real kernel authority alive even when the regression fails.
    // Only this disposable fixture's job is faulted; no PID-based cleanup.
    let mut owned_job = OwnedTestJob(Some(job));
    supervisor.job = windows::Win32::Foundation::HANDLE::default();
    let slot = Mutex::new(Some(supervisor));
    assert!(shutdown_slot(&slot).is_err());
    assert!(ensure_slot(
        &slot,
        || panic!("pending cleanup must not spawn a replacement"),
        |_| panic!("pending cleanup must not bind the old worker"),
    )
    .unwrap_err()
    .contains("cleanup is pending"));
    {
        let mut guard = slot.lock().unwrap();
        if let Some(retained) = guard.as_mut() {
            retained.job = owned_job.0.take().unwrap();
        }
        let retained = guard
            .as_ref()
            .expect("failed cleanup must retain the worker");
        assert_eq!(retained.pid, expected_pid);
        assert!(job_members(retained).contains(&(descendant as usize)));
        assert!(!*retained.closed.lock().unwrap());
    }
    shutdown_slot(&slot).unwrap();
    assert!(slot.lock().unwrap().is_none());
    // Once physically closed, repeated shutdown is a true no-op.
    shutdown_slot(&slot).unwrap();
}

#[test]
fn concurrent_product_initializers_publish_only_one_bound_worker() {
    use std::sync::atomic::AtomicUsize;
    let slot = Arc::new(Mutex::new(None));
    let launches = Arc::new(AtomicUsize::new(0));
    let (bound_tx, bound_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let first = {
        let slot = slot.clone();
        let launches = launches.clone();
        std::thread::spawn(move || {
            ensure_slot(
                &slot,
                || {
                    launches.fetch_add(1, Ordering::SeqCst);
                    Ok(supervisor().0)
                },
                |_| {
                    bound_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                    Ok(())
                },
            )
        })
    };
    bound_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let second = {
        let slot = slot.clone();
        let launches = launches.clone();
        std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            done_tx
                .send(ensure_slot(
                    &slot,
                    || {
                        launches.fetch_add(1, Ordering::SeqCst);
                        Ok(supervisor().0)
                    },
                    |_| Ok(()),
                ))
                .unwrap();
        })
    };
    started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let premature = done_rx.recv_timeout(Duration::from_millis(150));
    release_tx.send(()).unwrap();
    first.join().unwrap().unwrap();
    second.join().unwrap();
    assert!(matches!(premature, Err(mpsc::RecvTimeoutError::Timeout)));
    done_rx
        .recv_timeout(Duration::from_secs(3))
        .unwrap()
        .unwrap();
    assert_eq!(launches.load(Ordering::SeqCst), 1);
    shutdown_slot(&slot).unwrap();
}

#[test]
fn failed_binding_preserves_both_errors_and_the_original_cleanup_authority() {
    let (mut worker, _) = supervisor();
    let expected_pid = worker.pid;
    let mut owned_job = OwnedTestJob(Some(worker.job));
    worker.job = windows::Win32::Foundation::HANDLE::default();
    let slot = Mutex::new(None);
    let result = ensure_slot(&slot, || Ok(worker), |_| Err("fixture bind failure".into()));
    {
        let mut guard = slot.lock().unwrap();
        if let Some(retained) = guard.as_mut() {
            retained.job = owned_job.0.take().unwrap();
        }
        assert_eq!(guard.as_ref().unwrap().pid, expected_pid);
    }
    let error = result.unwrap_err();
    assert!(error.contains("fixture bind failure"));
    assert!(error.contains("owned worker cleanup also failed"));
    assert!(ensure_slot(&slot, || panic!("must not replace"), |_| Ok(())).is_err());
    shutdown_slot(&slot).unwrap();
    assert!(slot.lock().unwrap().is_none());
    ensure_slot(&slot, || Ok(supervisor().0), |_| Ok(())).unwrap();
    shutdown_slot(&slot).unwrap();
}

#[test]
fn failed_binding_with_confirmed_cleanup_allows_a_fresh_worker() {
    let slot = Mutex::new(None);
    let result = ensure_slot(
        &slot,
        || Ok(supervisor().0),
        |_| Err("fixture bind failure".into()),
    );
    assert_eq!(result.unwrap_err(), "fixture bind failure");
    assert!(slot.lock().unwrap().is_none());
    ensure_slot(&slot, || Ok(supervisor().0), |_| Ok(())).unwrap();
    shutdown_slot(&slot).unwrap();
}

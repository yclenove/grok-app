use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Instant;

const CHILD_TEST: &str =
    "session_migration::search::owner_tests::process_tests::index_owner_child_entry";
const CHILD_DIRECTORY: &str = "GROK_TEST_INDEX_OWNER_DIRECTORY";
const CHILD_READY: &str = "GROK_INDEX_OWNER_CHILD_READY";
const CHILD_TIMEOUT: Duration = Duration::from_secs(10);

struct ChildOwner {
    child: Child,
    output: Option<JoinHandle<()>>,
}

impl ChildOwner {
    fn start(dir: &Path) -> Self {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                CHILD_TEST,
                "--test-threads=1",
                "--nocapture",
            ])
            .env(CHILD_DIRECTORY, dir)
            .env("GROK_APP_HOME", dir.join("unused-app-home"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut owner = Self {
            child: command.spawn().expect("spawn isolated index owner child"),
            output: None,
        };
        let stdout = owner.child.stdout.take().unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        owner.output = Some(std::thread::spawn(move || {
            let mut ready = false;
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) if !ready && line == CHILD_READY => {
                        ready = true;
                        let _ = ready_tx.send(Ok(()));
                    }
                    Ok(_) => {}
                    Err(error) => {
                        let _ = ready_tx.send(Err(format!("read child output: {error}")));
                        return;
                    }
                }
            }
            if !ready {
                let _ = ready_tx.send(Err("child exited before owning the index".into()));
            }
        }));
        ready_rx
            .recv_timeout(CHILD_TIMEOUT)
            .expect("child did not signal index ownership before the deadline")
            .expect("child failed before readiness");
        assert!(owner.is_alive(), "child exited after its readiness signal");
        owner
    }

    fn is_alive(&mut self) -> bool {
        self.child.try_wait().unwrap().is_none()
    }

    fn wait_for_exit(&mut self) -> ExitStatus {
        let deadline = Instant::now() + CHILD_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "index owner child did not exit");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn join_output(&mut self) {
        if let Some(output) = self.output.take() {
            output.join().unwrap();
        }
    }

    fn finish(mut self) -> ExitStatus {
        let mut input = self.child.stdin.take().unwrap();
        input.write_all(&[1]).unwrap();
        input.flush().unwrap();
        drop(input);
        let status = self.wait_for_exit();
        self.join_output();
        status
    }

    fn kill_and_wait(mut self) {
        assert!(
            self.is_alive(),
            "test owner must still be alive before termination"
        );
        self.child.kill().unwrap();
        let _ = self.wait_for_exit();
        self.join_output();
    }
}

impl Drop for ChildOwner {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(output) = self.output.take() {
            let _ = output.join();
        }
    }
}

#[test]
#[ignore = "invoked only by the isolated index ownership process tests"]
fn index_owner_child_entry() {
    let dir = PathBuf::from(std::env::var_os(CHILD_DIRECTORY).expect(CHILD_DIRECTORY));
    let name = dir.file_name().and_then(|name| name.to_str()).unwrap();
    Uuid::parse_str(
        name.strip_prefix("idx-owner-")
            .expect("synthetic fixture prefix"),
    )
    .expect("synthetic fixture identity");
    assert_eq!(
        dir.parent().unwrap().canonicalize().unwrap(),
        std::env::temp_dir().canonicalize().unwrap(),
        "child may only use the synthetic temporary fixture"
    );
    let owner = PausedBuilder::start(&dir);
    {
        let mut output = std::io::stdout().lock();
        writeln!(output, "\n{CHILD_READY}").unwrap();
        output.flush().unwrap();
    }
    std::io::stdin().read_exact(&mut [0]).unwrap();
    owner
        .finish()
        .expect("child must publish its own complete temporary index");
}

#[test]
fn live_child_rebuild_blocks_recovery_and_completes_after_release() {
    let fixture = Fixture::new();
    let mut child = ChildOwner::start(&fixture.0);
    let before = fixture.snapshot();
    let started = Instant::now();
    let contender = ensure_index(&fixture.0);
    let elapsed = started.elapsed();
    let after = fixture.snapshot();
    let remained_alive = child.is_alive();
    // Per-revision ownership must not serialize unrelated import histories.
    let other = Fixture::new();
    ensure_index(&other.0).unwrap();
    other.assert_complete();
    let status = child.finish();

    assert!(
        status.success(),
        "live index owner failed after release: {status}"
    );
    assert!(
        remained_alive,
        "the contender was not tested against a live owner"
    );
    assert!(
        elapsed < CHILD_TIMEOUT,
        "index ownership contention was not bounded"
    );
    assert_eq!(
        before, after,
        "parent recovery changed another process's index artifacts"
    );
    let error = contender.expect_err("cross-process ownership must block recovery");
    assert_eq!(error.code, ErrorCode::SourceBusy);
    assert_eq!(error.retry, "retry");
    ensure_index(&fixture.0).unwrap();
    fixture.assert_complete();
}

#[test]
fn killed_child_releases_index_owner_and_recovers_complete_history() {
    let fixture = Fixture::new();
    let child = ChildOwner::start(&fixture.0);
    assert!(!index_path(&fixture.0).exists());
    assert!(fixture.0.join("event-index.sqlite.tmp").is_file());
    assert!(index_backup_path(&index_path(&fixture.0)).is_file());
    child.kill_and_wait();

    ensure_index(&fixture.0).expect("terminated owner must not leave an unrecoverable lock");
    fixture.assert_complete();
    rebuild_index(&fixture.0, &|| false)
        .expect("a recovered index must remain rebuildable after owner termination");
    fixture.assert_complete();
}

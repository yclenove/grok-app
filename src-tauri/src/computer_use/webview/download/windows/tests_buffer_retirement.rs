//! A cancelled long-path buffer belongs to the original joined download,
//! not to the first native terminal callback or the abandoned caller alone.

use super::*;

struct Fixture {
    directories: Vec<PathBuf>,
    expectation: Arc<Expectation>,
    tracker: ScriptTracker,
    run: String,
    private_directory: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let nonce = uuid::Uuid::new_v4().to_string();
        let root = std::env::temp_dir().join(format!("cu-dl-retirement-{nonce}"));
        std::fs::create_dir(&root).unwrap();
        let mut directories = vec![root.clone()];
        let mut parent = root;
        for index in 0..8 {
            parent = parent.join(format!("{index}-{}", "x".repeat(40)));
            std::fs::create_dir(&parent).unwrap();
            directories.push(parent.clone());
        }
        let destination = std::fs::canonicalize(parent).unwrap().join("download.bin");
        let instance = NEXT_EXPECTATION.fetch_add(1, Ordering::SeqCst) + 800_000;
        tests::install_record(instance);
        let tracker = ScriptTracker::default();
        let run = format!("buffer-retirement-{nonce}");
        let operation = tracker
            .begin(&run, &nonce, &Default::default(), &Default::default())
            .unwrap();
        let expectation = register(
            instance,
            "https://fixture.invalid/retirement.bin",
            destination,
            operation,
        )
        .unwrap();
        assert!(expectation.native_destination.buffered());
        let private_directory = expectation
            .native_destination
            .path()
            .parent()
            .unwrap()
            .to_path_buf();
        std::fs::write(expectation.native_destination.path(), b"original buffer").unwrap();
        std::fs::write(&expectation.destination, b"original partial transfer").unwrap();
        expectation.state.lock().native_started = true;
        expectation.state.lock().wry_requested = true;
        Self {
            directories,
            expectation,
            tracker,
            run,
            private_directory,
        }
    }

    fn assert_original_files_retained(&self) {
        assert_eq!(
            std::fs::read(self.expectation.native_destination.path()).unwrap(),
            b"original buffer"
        );
        assert_eq!(
            std::fs::read(&self.expectation.destination).unwrap(),
            b"original partial transfer"
        );
        assert!(!self.tracker.is_idle(&self.run));
        assert!(EXPECTATIONS.lock().contains_key(&self.expectation.id));
    }

    fn finish_wry(&self) {
        assert!(finish_wry(
            self.expectation.instance,
            &self.expectation.url,
            Some(self.expectation.native_destination.path()),
            false,
        ));
    }

    fn assert_retired(&self) {
        assert!(self.tracker.is_idle(&self.run));
        assert!(!EXPECTATIONS.lock().contains_key(&self.expectation.id));
        assert!(!self.private_directory.exists());
        assert!(!self.expectation.destination.exists());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Exact self-created test resources only, including failure paths.
        self.expectation.state.lock().caller_done = true;
        self.expectation.state.lock().abandoned = true;
        self.expectation.state.lock().native_exited = true;
        complete(&self.expectation, NativeTerminal::OwnerExited);
        cleanup_expectation(&self.expectation);
        INSTALLS.lock().remove(&self.expectation.instance);
        for directory in self.directories.iter().rev() {
            let _ = std::fs::remove_dir(directory);
        }
    }
}

#[test]
fn abandoned_buffer_waits_for_original_wry_after_native_terminal() {
    let fixture = Fixture::new();
    abandon(&fixture.expectation);
    complete(&fixture.expectation, NativeTerminal::Interrupted);
    fixture.assert_original_files_retained();
    fixture.finish_wry();
    fixture.assert_retired();
}

#[test]
fn native_terminal_then_abandon_keeps_buffer_until_original_wry() {
    let fixture = Fixture::new();
    complete(&fixture.expectation, NativeTerminal::Interrupted);
    fixture.assert_original_files_retained();
    abandon(&fixture.expectation);
    fixture.assert_original_files_retained();
    fixture.finish_wry();
    fixture.assert_retired();
}

#[test]
fn joined_callbacks_keep_buffer_until_original_caller_finishes() {
    let fixture = Fixture::new();
    complete(&fixture.expectation, NativeTerminal::Interrupted);
    fixture.finish_wry();
    fixture.assert_original_files_retained();
    assert!(finish_caller(&fixture.expectation, NativeTerminal::Interrupted, true).is_err());
    fixture.assert_retired();
}

#[test]
fn exact_native_exit_retires_an_abandoned_buffer_without_faking_wry() {
    let fixture = Fixture::new();
    abandon(&fixture.expectation);
    fixture.assert_original_files_retained();
    fixture.expectation.state.lock().native_exited = true;
    complete(&fixture.expectation, NativeTerminal::OwnerExited);
    fixture.assert_retired();
    assert!(!fixture.expectation.state.lock().wry_finished);
}

#[test]
fn exact_native_exit_keeps_files_until_original_caller_finishes() {
    let fixture = Fixture::new();
    exit::retire_instance(fixture.expectation.instance);
    fixture.assert_original_files_retained();
    assert!(!fixture.expectation.state.lock().wry_finished);
    assert!(finish_caller(&fixture.expectation, NativeTerminal::OwnerExited, true).is_err());
    fixture.assert_retired();
}

#[test]
fn duplicate_old_callbacks_and_drop_do_not_delete_a_replacement_path() {
    let fixture = Fixture::new();
    abandon(&fixture.expectation);
    complete(&fixture.expectation, NativeTerminal::Interrupted);
    fixture.finish_wry();
    fixture.assert_retired();
    std::fs::create_dir(&fixture.private_directory).unwrap();
    let replacement = fixture.private_directory.join("download.bin");
    std::fs::write(&replacement, b"replacement private file").unwrap();
    std::fs::create_dir(fixture.expectation.destination.parent().unwrap()).unwrap();
    std::fs::write(&fixture.expectation.destination, b"replacement broker file").unwrap();
    complete(&fixture.expectation, NativeTerminal::Interrupted);
    abandon(&fixture.expectation);
    fixture.expectation.native_destination.cleanup();
    assert_eq!(
        std::fs::read(&replacement).unwrap(),
        b"replacement private file"
    );
    assert_eq!(
        std::fs::read(&fixture.expectation.destination).unwrap(),
        b"replacement broker file"
    );
    // Retire the test's old object too; its Drop must not revisit the path.
    let original = fixture.expectation.clone();
    drop(fixture);
    drop(original);
    assert_eq!(
        std::fs::read(&replacement).unwrap(),
        b"replacement private file"
    );
    std::fs::remove_file(&replacement).unwrap();
    std::fs::remove_dir(replacement.parent().unwrap()).unwrap();
}

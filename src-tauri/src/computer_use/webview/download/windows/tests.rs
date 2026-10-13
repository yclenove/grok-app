//! Original native/Wry/caller retirement and path/permission regressions.

use super::*;

pub(super) fn install_record(instance: u64) {
    let record = Arc::new(InstallRecord::pending());
    static TEST_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
        let root = std::env::temp_dir().join(format!(
            "cu-download-install-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&root).unwrap();
        root
    });
    *record.owners.lock() =
        Some(journal::Owners::capture(std::process::id(), TEST_ROOT.clone()).unwrap());
    record.finish(true);
    INSTALLS.lock().insert(instance, record);
}

pub(super) fn expectation() -> (Arc<Expectation>, ScriptTracker, u64, String, PathBuf) {
    let tracker = ScriptTracker::default();
    let instance = NEXT_EXPECTATION.fetch_add(1, Ordering::SeqCst) + 10_000;
    install_record(instance);
    let native = format!("download-test-native-{instance}");
    let operation = tracker
        .begin(
            "download-test-run",
            &native,
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
    let url = format!("https://fixture.invalid/{instance}.bin");
    let destination = std::env::temp_dir()
        .join(format!("cu-webview-download-{instance}"))
        .join("report.bin");
    let expectation = register(instance, &url, destination.clone(), operation).unwrap();
    let mut claimed_destination = PathBuf::new();
    assert!(claim_wry_requested(
        instance,
        &url,
        &mut claimed_destination
    ));
    assert_eq!(claimed_destination, destination);
    assert!(claim_native_start(instance, &url).is_some());
    (expectation, tracker, instance, url, destination)
}

fn assert_retired(expectation: &Arc<Expectation>, tracker: &ScriptTracker) {
    assert!(!EXPECTATIONS.lock().contains_key(&expectation.id));
    assert!(tracker.is_idle("download-test-run"));
}

#[test]
fn native_terminal_then_caller_then_wry_retires_expectation() {
    let (expectation, tracker, instance, url, destination) = expectation();

    complete(&expectation, NativeTerminal::Completed);
    assert!(!tracker.is_idle("download-test-run"));
    finish_caller(&expectation, NativeTerminal::Completed, false).unwrap();
    assert!(EXPECTATIONS.lock().contains_key(&expectation.id));
    assert!(!tracker.is_idle("download-test-run"));
    assert!(finish_wry(
        instance,
        &url,
        Some(destination.as_path()),
        true
    ));

    assert_retired(&expectation, &tracker);
}

#[test]
fn wry_terminal_then_caller_allows_a_second_download() {
    let (expectation, tracker, instance, url, destination) = expectation();

    assert!(finish_wry(
        instance,
        &url,
        Some(destination.as_path()),
        true
    ));
    assert!(!tracker.is_idle("download-test-run"));
    finish_caller(&expectation, NativeTerminal::Completed, false).unwrap();
    assert_retired(&expectation, &tracker);

    let second_tracker = ScriptTracker::default();
    let second_operation = second_tracker
        .begin(
            "download-test-run-2",
            &format!("download-test-native-{instance}-2"),
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
    let second = register(
        instance,
        &format!("{url}?second"),
        destination.with_file_name("second.bin"),
        second_operation,
    )
    .unwrap();
    retire_before_dispatch(&second);
    assert!(!EXPECTATIONS.lock().contains_key(&second.id));
    assert!(second_tracker.is_idle("download-test-run-2"));
}

#[test]
fn completion_by_id_releases_registry_lock_before_retirement() {
    let (expectation, tracker, ..) = expectation();
    {
        let mut state = expectation.state.lock();
        state.caller_done = true;
        state.wry_finished = true;
    }

    complete_by_id(expectation.id, NativeTerminal::Completed);

    assert_retired(&expectation, &tracker);
}

#[test]
fn completed_native_operation_is_not_mistaken_for_cancellation() {
    let (expectation, tracker, instance, url, destination) = expectation();
    let control = expectation.operation.lock().as_ref().unwrap().clone();

    complete(&expectation, NativeTerminal::Completed);
    assert!(control.check().is_ok());
    let terminal = { expectation.state.lock().terminal }.unwrap();
    let result = finish_caller(&expectation, terminal, control.is_cancelled()).unwrap();
    assert_eq!(result.path, destination);
    assert!(finish_wry(
        instance,
        &url,
        Some(destination.as_path()),
        true
    ));

    assert_retired(&expectation, &tracker);
}

#[test]
fn webview_download_paths_use_the_native_legacy_format() {
    let drive_path = PathBuf::from(r"\\?\C:\staging\report.bin");
    assert_eq!(windows_path(&drive_path), r"C:\staging\report.bin");

    let unc_path = PathBuf::from(r"\\?\UNC\server\share\report.bin");
    assert_eq!(windows_path(&unc_path), r"\\server\share\report.bin");
}

#[test]
fn automatic_download_permission_is_scoped_to_the_pending_origin() {
    let (expectation, tracker, instance, ..) = expectation();
    assert!(has_pending_expectation(instance));
    assert!(pending_expectation_matches_origin(
        instance,
        "https://fixture.invalid/page"
    ));
    assert!(!pending_expectation_matches_origin(
        instance,
        "https://other.invalid/page"
    ));
    assert!(!pending_expectation_matches_origin(instance, "not a URL"));

    retire_before_dispatch(&expectation);

    assert!(!has_pending_expectation(instance));
    assert!(!pending_expectation_matches_origin(
        instance,
        "https://fixture.invalid/page"
    ));
    assert!(tracker.is_idle("download-test-run"));
}

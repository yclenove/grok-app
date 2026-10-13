//! Admission and durable buffer publication precede every native dispatch.

use super::*;

pub(super) fn register(
    instance: u64,
    url: &str,
    destination: PathBuf,
    operation: ScriptOperation,
) -> Result<Arc<Expectation>, String> {
    // Serialize with close. Passing an earlier document check cannot admit a
    // fresh expectation after this exact original view has retired.
    let installs = INSTALLS.lock();
    let owners = installs
        .get(&instance)
        .filter(|record| matches!(*record.state.lock(), InstallState::Ready))
        .and_then(|record| record.owners.lock().clone());
    let Some(owners) = owners.filter(journal::Owners::live) else {
        operation.finished();
        return Err("WebView native download instance retired or unavailable".into());
    };
    let mut expectations = EXPECTATIONS.lock();
    if expectations
        .values()
        .any(|expectation| expectation.instance == instance)
    {
        operation.finished();
        return Err("WebView native download is already pending".into());
    }
    let id = NEXT_EXPECTATION.fetch_add(1, Ordering::SeqCst);
    if id == 0 {
        operation.finished();
        return Err("WebView download identity exhausted".into());
    }
    let native_destination =
        match destination::NativeDestination::new(&destination).and_then(|mut native| {
            native.bind(&owners, instance, id)?;
            Ok(native)
        }) {
            Ok(native) => native,
            Err(error) => {
                operation.finished();
                return Err(error);
            }
        };
    #[cfg(feature = "computer-use-probe")]
    probe::record_destination(instance, &destination, &native_destination);
    let expectation = Arc::new(Expectation {
        id,
        instance,
        url: url.to_string(),
        destination,
        native_destination,
        operation: Mutex::new(Some(operation)),
        state: Mutex::new(ExpectationState::default()),
        changed: Condvar::new(),
    });
    expectations.insert(id, expectation.clone());
    Ok(expectation)
}

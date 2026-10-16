//! Worker execution boundary. Initialization starts only after the parent frame.
use super::{codec, tree::Tree, wire::*};
use gcu_core::{driver::*, model::NativeHandle};
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    sync::Arc,
};
use tokio::io::{AsyncRead, AsyncWrite};

#[cfg(test)]
mod tests;

#[derive(Default)]
struct Handles {
    elements: BTreeMap<ElementKey, (Element, NativeHandle)>,
    bound_apps: BTreeSet<String>,
    resolved_apps: BTreeMap<String, gcu_protocol::AppInfo>,
    generation: u64,
}
type ElementKey = (u64, Vec<i32>);
impl Handles {
    fn bind(&mut self, handle: &NativeHandle, epoch: u64) -> Result<Element> {
        let native = handle
            .downcast_ref::<crate::uia::UiaElement>()
            .ok_or(DriverError::StaleElement)?;
        if native.runtime_id.is_empty() || native.runtime_id.len() > 256 {
            return Err(DriverError::StaleElement);
        }
        let el = Element {
            window: Window::from(&native.window),
            hwnd: native.hwnd,
            runtime_id: native.runtime_id.clone(),
            generation: self.generation,
            slot: native.node.slot,
            fingerprint: native.fingerprint,
            epoch,
        };
        self.elements.insert(
            (el.hwnd, el.runtime_id.clone()),
            (el.clone(), handle.clone()),
        );
        Ok(el)
    }
    fn resolve(&self, el: Element, epoch: u64) -> Result<NativeHandle> {
        if el.epoch != epoch {
            return Err(DriverError::StaleElement);
        }
        let (current, handle) = self
            .elements
            .get(&(el.hwnd, el.runtime_id))
            .ok_or(DriverError::StaleElement)?;
        if current.window != el.window
            || current.fingerprint != el.fingerprint
            || el.generation == 0
            || el.generation > self.generation
        {
            return Err(DriverError::StaleElement);
        }
        Ok(handle.clone())
    }
}
/// Serve one trusted inherited stdin/stdout connection until EOF or invalid frame.
/// Initialization and native work occur here, never in the supervisor. The caller
/// receives the stable launcher-instance namespace in `initialize`. The caller
/// must be the unelevated launcher; no public socket or model-facing worker RPC
/// exists. Dropping pipes terminates service; the parent watchdog kills stuck FFI.
pub async fn serve<R, W, F, Fut>(mut input: R, mut output: W, initialize: F) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<Arc<dyn Driver>>>,
{
    serve_with_input(&mut input, &mut output, |namespace, _| {
        initialize(namespace)
    })
    .await
}
/// Serve input-capable native adapters with an independent parent ledger barrier.
/// `initialize` must attach the supplied tracker to every synthetic input backend.
/// The tracker is serviced alongside native dispatch; Stop remains parent-owned.
pub async fn serve_with_input<R, W, F, Fut>(
    mut input: R,
    mut output: W,
    initialize: F,
) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    F: FnOnce(String, Arc<dyn crate::input::api::InputTracker>) -> Fut,
    Fut: Future<Output = Result<Arc<dyn Driver>>>,
{
    let Some(first): Option<Call> = codec::read_frame(&mut input).await.map_err(|_| invalid())?
    else {
        return Ok(());
    };
    if first.version != VERSION
        || first.id != 0
        || first.epoch == 0
        || first.namespace.is_empty()
        || first.namespace.len() > 128
        || !first
            .namespace
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'.')
        || !matches!(first.request, Request::Doctor)
    {
        return Err(invalid());
    }
    let (tracker, mut notices) = super::input::channel();
    let initialization = crate::timing::Timing::start("worker_initialize");
    let driver = match initialize(first.namespace.clone(), tracker.clone()).await {
        Ok(driver) => driver,
        Err(error) => {
            codec::write_frame(
                &mut output,
                &Response {
                    version: VERSION,
                    id: 0,
                    epoch: first.epoch,
                    result: Err(Failure(error.clone())),
                },
            )
            .await
            .map_err(|_| invalid())?;
            return Err(error);
        }
    };
    drop(initialization);
    let epoch = first.epoch;
    let namespace = first.namespace.clone();
    let mut handles = Handles::default();
    let mut call = first;
    loop {
        let id = call.id;
        let operation = dispatch(driver.as_ref(), &mut handles, epoch, call.request);
        tokio::pin!(operation);
        let result = loop {
            tokio::select! { biased;
                notice = notices.recv() => {
                    let notice = notice.ok_or_else(invalid)?;
                    super::input::notify(notice, &mut input, &mut output, id, epoch).await?;
                }
                result = &mut operation => break result,
            }
        };
        codec::write_frame(
            &mut output,
            &Response {
                version: VERSION,
                id,
                epoch,
                result: result.map_err(Failure),
            },
        )
        .await
        .map_err(|_| invalid())?;
        let Some(next): Option<Call> =
            codec::read_frame(&mut input).await.map_err(|_| invalid())?
        else {
            break;
        };
        if next.version != VERSION
            || next.epoch != epoch
            || next.namespace != namespace
            || next.id <= id
        {
            return Err(invalid());
        }
        call = next;
    }
    driver.release_all_input().await;
    Ok(())
}
fn invalid() -> DriverError {
    DriverError::Native {
        message: "invalid worker connection".into(),
        maybe_executed: false,
    }
}
async fn dispatch(
    driver: &dyn Driver,
    handles: &mut Handles,
    epoch: u64,
    request: Request,
) -> Result<Reply> {
    let cancel = Cancel::default();
    // A restarted worker has no discovery registry. Rehydrate by exact path and
    // compare the authorized provenance; never substitute a similar name/window.
    let expected = match &request {
        Request::LaunchApp(info) | Request::KeyWindow(info) | Request::Focused(info) => Some(info),
        Request::ListWindows(info) => info.as_ref(),
        Request::Snapshot(w, _) | Request::Capture(w, _) | Request::Geometry(w) => Some(&w.app),
        Request::Perform(a) => Some(&a.window().app),
        _ => None,
    };
    if let Some(expected) = expected {
        // Rehydrate only after restart or a changed binding. Each native method
        // still performs its own live token/image/window validation: this cache
        // never authorizes content or delivery. Re-resolving every request used
        // to hash/verify the executable before doing that same preflight again.
        if handles.resolved_apps.get(&expected.id) != Some(expected) {
            let _timing = crate::timing::Timing::start("worker_rehydrate");
            let fresh = driver.resolve_app(&AppQuery(expected.id.clone())).await?;
            if fresh.info.id != expected.id
                || fresh.info.identity != expected.identity
                || fresh.info.pid != expected.pid
            {
                return Err(DriverError::AppNotFound);
            }
            handles.bound_apps.remove(&expected.id);
            handles
                .resolved_apps
                .insert(expected.id.clone(), expected.clone());
        }
        // Resolution no longer fills the window registry by scanning/hashing
        // every application. Rebind live stamps on the read queue after restart.
        // No activation or mutation; later observation still checks the exact ID.
        // ListWindows/KeyWindow/Focused already enumerate their own HWNDs.
        // Only stamped-window requests need an extra binding pass after restart.
        let needs_stamp = matches!(
            &request,
            Request::Snapshot(..)
                | Request::Capture(..)
                | Request::Geometry(_)
                | Request::Perform(_)
        );
        if needs_stamp && expected.pid.is_some() && !handles.bound_apps.contains(&expected.id) {
            driver
                .list_windows(Some(&AppRef {
                    info: expected.clone(),
                }))
                .await?;
            handles.bound_apps.insert(expected.id.clone());
        }
    }
    Ok(match request {
        Request::Doctor => {
            let report = driver.doctor().await;
            let mut capabilities = report.capabilities;
            // No empty event subscriptions are advertised across worker RPC.
            capabilities.observe.events = false;
            Reply::Doctor {
                capabilities,
                ready: report.ready,
                issues: report.issues,
            }
        }
        Request::ListApps => Reply::Apps {
            apps: driver.list_apps().await?,
            indexing: driver.discovery_status().indexing,
        },
        Request::ResolveApp(query) => {
            let info = driver.resolve_app(&AppQuery(query)).await?.info;
            if handles.resolved_apps.get(&info.id) != Some(&info) {
                handles.bound_apps.remove(&info.id);
            }
            handles.resolved_apps.insert(info.id.clone(), info.clone());
            Reply::App(info)
        }
        Request::LaunchApp(info) => Reply::App(driver.launch_app(&AppRef { info }).await?.info),
        Request::ListWindows(info) => {
            let target = info.as_ref().map(|info| info.id.clone());
            let windows = driver
                .list_windows(info.map(|info| AppRef { info }).as_ref())
                .await?;
            // A successful native enumeration has populated its stamp registry.
            for info in handles.resolved_apps.values().filter(|info| {
                info.pid.is_some() && target.as_ref().is_none_or(|id| id == &info.id)
            }) {
                handles.bound_apps.insert(info.id.clone());
            }
            Reply::Windows(windows)
        }
        Request::KeyWindow(info) => {
            let window = driver.key_window(&AppRef { info: info.clone() }).await?;
            handles.bound_apps.insert(info.id);
            Reply::Window(window)
        }
        Request::Snapshot(window, opts) => {
            let tree = driver
                .snapshot_tree(&window.into(), &opts.into(), &cancel)
                .await?;
            handles.elements.clear();
            handles.generation += 1;
            Reply::Tree(Tree::encode(tree, |h| handles.bind(h, epoch))?)
        }
        Request::Capture(window, opts) => {
            let capture = driver
                .capture(
                    &window.into(),
                    &CaptureOpts {
                        timeout: std::time::Duration::from_millis(opts.timeout_ms)
                            .min(DRIVER_DEADLINE),
                        include_cursor: opts.include_cursor,
                    },
                    &cancel,
                )
                .await?;
            Reply::Capture {
                bytes: capture.bytes,
                width: capture.width,
                height: capture.height,
                mime: capture.mime,
                geometry: capture.geometry,
            }
        }
        Request::Geometry(window) => Reply::Geometry(driver.window_geometry(&window.into()).await?),
        Request::Focused(info) => {
            let focus = driver.focused_element(&AppRef { info }).await?;
            let Some(focus) = focus else {
                return Ok(Reply::Focused(None));
            };
            let native = focus
                .downcast_ref::<crate::uia::UiaElement>()
                .ok_or(DriverError::StaleElement)?;
            // focused_element internally reads a new native generation. Refresh
            // all bindings for that exact window so subsequent core guards cannot
            // leave sibling targets pointing at expired worker-local slots.
            let raw = driver
                .snapshot_tree(
                    &native.window,
                    &TreeOpts {
                        max_tree_nodes: crate::uia::tree::NODE_CAP,
                        ..Default::default()
                    },
                    &cancel,
                )
                .await?;
            fn focused(node: &gcu_core::model::RawNode) -> Option<NativeHandle> {
                if node.states.contains(gcu_core::model::States::FOCUSED) {
                    return Some(node.handle.clone());
                }
                node.children.iter().find_map(focused)
            }
            let focus = raw.root.as_ref().and_then(focused);
            if focus.is_none() && raw.incomplete.is_some() {
                return Err(DriverError::StaleElement);
            }
            handles.elements.clear();
            handles.generation += 1;
            let _tree = Tree::encode(raw, |h| handles.bind(h, epoch))?;
            Reply::Focused(focus.as_ref().map(|h| handles.bind(h, epoch)).transpose()?)
        }
        Request::Perform(action) => Reply::Delivery(
            driver
                .perform(&action.decode(|el| handles.resolve(el, epoch))?, &cancel)
                .await?
                .into(),
        ),
    })
}

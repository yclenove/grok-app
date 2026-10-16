//! Same-run control: bare UIA Invoke, outside the driver, on the exact fixture element.
use super::unsafe_foreground::{current, Foreground};
use crate::harness::progress;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use windows::{
    core::{Interface, HRESULT},
    Win32::{
        Foundation::HWND,
        System::{Com::*, Ole::*, Variant::VT_I4},
        UI::Accessibility::*,
    },
};

#[derive(Debug)]
pub struct Observation {
    pub before: Foreground,
    pub after_dispatch: Foreground,
}
struct Array(*mut SAFEARRAY);
impl Drop for Array {
    fn drop(&mut self) {
        // SAFETY: GetRuntimeId transfers ownership; no array locks are retained.
        let _ = unsafe { SafeArrayDestroy(self.0) };
    }
}
fn runtime(element: &IUIAutomationElement) -> windows::core::Result<Vec<i32>> {
    // SAFETY: live MTA-local element; successful result is our owned SAFEARRAY.
    let array = Array(unsafe { element.GetRuntimeId() }?);
    if array.0.is_null() {
        return Err(windows::core::Error::from_hresult(HRESULT(
            0x80004005u32 as i32,
        )));
    }
    // SAFETY: non-null owned array; shape/type are checked before scalar access.
    let (dim, size, kind, low, high) = unsafe {
        (
            SafeArrayGetDim(array.0),
            SafeArrayGetElemsize(array.0),
            SafeArrayGetVartype(array.0)?,
            SafeArrayGetLBound(array.0, 1)?,
            SafeArrayGetUBound(array.0, 1)?,
        )
    };
    if dim != 1
        || size != 4
        || kind != VT_I4
        || !(1..=256).contains(&(i64::from(high) - i64::from(low) + 1))
    {
        return Err(windows::core::Error::from_hresult(HRESULT(
            0x80004005u32 as i32,
        )));
    }
    let mut values = Vec::new();
    for index in low..=high {
        let mut value = 0i32;
        // SAFETY: verified VT_I4 element, in-bounds index and writable i32 output.
        unsafe { SafeArrayGetElement(array.0, &index, (&mut value as *mut i32).cast()) }?;
        values.push(value);
    }
    Ok(values)
}

fn describe(element: &IUIAutomationElement) -> String {
    // SAFETY: read-only properties of an interface owned by this MTA.
    unsafe {
        format!(
            "Name={:?}; AutomationId={:?}; ControlType={:?}; RuntimeId={:?}",
            element.CurrentName(),
            element.CurrentAutomationId(),
            element.CurrentControlType(),
            runtime(element)
        )
    }
}

fn resolve(
    automation: &IUIAutomation,
    hwnd: u64,
    name: Option<&str>,
    expected_runtime: Option<&[i32]>,
    abandoned: &AtomicBool,
) -> Result<IUIAutomationElement, String> {
    let start = Instant::now();
    let mut attempt = 0;
    loop {
        if abandoned.load(Ordering::Acquire) {
            return Err("direct UIA resolution abandoned; no mutation".into());
        }
        attempt += 1;
        // SAFETY: MTA-local read-only traversal of the exact fixture HWND.
        let read = unsafe {
            (|| -> Result<IUIAutomationElement, String> {
                let root = automation
                    .ElementFromHandle(HWND(hwnd as usize as *mut _))
                    .map_err(|e| format!("ElementFromHandle({hwnd:x}): {e}"))?;
                if expected_runtime.is_none() && name.is_none() {
                    return Ok(root);
                }
                // Enumerate the raw subtree: Name/ControlType filtering can hide
                // virtual AccessKit nodes. RuntimeId is the observed identity;
                // duplicate names must never substitute another element.
                let condition = automation
                    .CreateTrueCondition()
                    .map_err(|e| e.to_string())?;
                let nodes = root
                    .FindAll(TreeScope_Descendants, &condition)
                    .map_err(|e| format!("FindAll; root={}; {e}", describe(&root)))?;
                let length = nodes.Length().map_err(|e| e.to_string())?;
                if length > 5000 {
                    return Err(format!("direct UIA subtree exceeds 5000 nodes: {length}"));
                }
                let mut candidates = Vec::new();
                for i in 0..length {
                    let node = nodes.GetElement(i).map_err(|e| e.to_string())?;
                    let matches = if let Some(expected) = expected_runtime {
                        runtime(&node).map_err(|e| format!("RuntimeId at index {i}: {e}"))?
                            == expected
                    } else {
                        let name = name.unwrap_or_default();
                        node.CurrentName().map_err(|e| e.to_string())? == name
                            || node.CurrentAutomationId().map_err(|e| e.to_string())? == name
                    };
                    if matches {
                        progress(format!("DIRECT UIA RESOLVED: root_hwnd={hwnd:x}; attempt={attempt}; elapsed_ms={}; root={}; element={}; selector_RuntimeId={expected_runtime:?}; selector_Name_or_AutomationId={name:?}", start.elapsed().as_millis(), describe(&root), describe(&node)));
                        return Ok(node);
                    }
                    if candidates.len() < 64 {
                        candidates.push(describe(&node));
                    }
                }
                Err(format!("exact UIA target pending; root_hwnd={hwnd:x}; root={}; nodes={length}; expected_RuntimeId={expected_runtime:?}; Name_or_AutomationId={name:?}; first_nodes={candidates:?}", describe(&root)))
            })()
        };
        match read {
            Ok(element) => return Ok(element),
            Err(last) => {
                progress(format!(
                    "DIRECT UIA READINESS: attempt={attempt}; elapsed_ms={}; {last}",
                    start.elapsed().as_millis()
                ));
                if start.elapsed() >= Duration::from_secs(3) {
                    return Err(format!(
                        "direct UIA resolution timed out; no mutation; {last}"
                    ));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

pub async fn invoke(
    root: u64,
    name: Option<&str>,
    expected: u64,
    expected_runtime: Option<&[i32]>,
) -> Result<Observation, String> {
    let name = name.map(str::to_owned);
    let expected_runtime = expected_runtime.map(<[i32]>::to_vec);
    let closed = Arc::new(AtomicBool::new(false));
    let abandoned = closed.clone();
    let (reply, job) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let result = (|| {
            // SAFETY: dedicated MTA; every interface drops before CoUninitialize.
            unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
                .ok()
                .map_err(|e| e.to_string())?;
            struct Com;
            impl Drop for Com {
                fn drop(&mut self) {
                    // SAFETY: balances this thread's successful initialization.
                    unsafe { CoUninitialize() };
                }
            }
            let _com = Com;
            // SAFETY: initialized COM; only exact disposable fixture HWNDs/owned arguments.
            let result = unsafe {
                (|| -> Result<Observation, String> {
                    let automation: IUIAutomation =
                        CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
                            .map_err(|e| e.to_string())?;
                    let limits: IUIAutomation2 = automation.cast().map_err(|e| e.to_string())?;
                    limits
                        .SetConnectionTimeout(500)
                        .map_err(|e| e.to_string())?;
                    limits
                        .SetTransactionTimeout(500)
                        .map_err(|e| e.to_string())?;
                    let element = resolve(
                        &automation,
                        root,
                        name.as_deref(),
                        expected_runtime.as_deref(),
                        &abandoned,
                    )?;
                    let pattern: IUIAutomationInvokePattern = element
                        .GetCurrentPatternAs(UIA_InvokePatternId)
                        .map_err(|e| {
                            format!("GetCurrentPattern(Invoke); {}; {e}", describe(&element))
                        })?;
                    let before = current();
                    progress(format!("DIRECT UIA: resolved exact element; before_Invoke={before:?}; expected={expected:x}"));
                    if abandoned.load(Ordering::Acquire) || before.hwnd != expected {
                        return Err(format!("foreground changed during resolution or control abandoned; before={before:?}; expected={expected:x}; no mutation"));
                    }
                    pattern.Invoke().map_err(|e| e.to_string())?;
                    Ok(Observation {
                        before,
                        after_dispatch: current(),
                    })
                })()
            };
            result.map_err(|e| format!("direct UIA Invoke: {e}; no retry"))
        })();
        let _ = reply.send(result);
    });
    let result = tokio::time::timeout(Duration::from_secs(5), job).await;
    closed.store(true, Ordering::Release);
    result
        .map_err(|_| "direct UIA control timed out; mutation unknown, no retry".to_string())?
        .map_err(|e| e.to_string())?
}

//! Same-run control: bare UIA Invoke, outside the driver, on the exact fixture element.
use super::unsafe_foreground::{current, Foreground};
use crate::harness::progress;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use windows::{
    core::{Interface, HRESULT},
    Win32::{
        Foundation::HWND,
        System::{
            Com::*,
            Ole::*,
            Variant::{VARIANT, VT_I4},
        },
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
                (|| -> windows::core::Result<Observation> {
                    let automation: IUIAutomation =
                        CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)?;
                    let limits: IUIAutomation2 = automation.cast()?;
                    limits.SetConnectionTimeout(500)?;
                    limits.SetTransactionTimeout(500)?;
                    let root = automation.ElementFromHandle(HWND(root as usize as *mut _))?;
                    let element = if let Some(name) = name {
                        let condition = automation.CreatePropertyCondition(
                            UIA_NamePropertyId,
                            &VARIANT::from(name.as_str()),
                        )?;
                        root.FindFirst(TreeScope_Descendants, &condition)?
                    } else {
                        root
                    };
                    if let Some(expected) = expected_runtime {
                        let actual = runtime(&element)?;
                        if actual != expected {
                            return Err(windows::core::Error::from_hresult(HRESULT(
                                UIA_E_ELEMENTNOTAVAILABLE as i32,
                            )));
                        }
                    }
                    let pattern: IUIAutomationInvokePattern =
                        element.GetCurrentPatternAs(UIA_InvokePatternId)?;
                    let before = current();
                    progress(format!("DIRECT UIA: resolved exact element; before_Invoke={before:?}; expected={expected:x}"));
                    if abandoned.load(Ordering::Acquire) || before.hwnd != expected {
                        return Err(windows::core::Error::from_hresult(HRESULT(
                            0x80004004u32 as i32,
                        )));
                    }
                    pattern.Invoke()?;
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

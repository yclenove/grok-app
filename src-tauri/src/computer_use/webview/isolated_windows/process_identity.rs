//! A held OS handle is exit evidence for one renderer instance, not a PID lookup
//! at cleanup time. Native main-frame identity selects it before evaluation.

use super::*;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use webview2_com::GetProcessExtendedInfosCompletedHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Environment13, ICoreWebView2FrameInfo2,
    ICoreWebView2ProcessExtendedInfoCollection, ICoreWebView2_2, ICoreWebView2_20,
    COREWEBVIEW2_FRAME_KIND, COREWEBVIEW2_FRAME_KIND_MAIN_FRAME, COREWEBVIEW2_PROCESS_KIND,
    COREWEBVIEW2_PROCESS_KIND_RENDERER,
};
use windows::core::{Interface, BOOL};
use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};

struct Witness {
    handle: OwnedHandle,
    #[cfg(feature = "computer-use-probe")]
    diagnostic_pid: u32,
}

impl process::PhysicalExit for Witness {
    fn exited(&self) -> bool {
        // WAIT_FAILED and WAIT_TIMEOUT are deliberately not evidence of exit.
        unsafe { WaitForSingleObject(HANDLE(self.handle.as_raw_handle()), 0) == WAIT_OBJECT_0 }
    }

    #[cfg(feature = "computer-use-probe")]
    fn diagnostic(&self) -> String {
        let handle = HANDLE(self.handle.as_raw_handle());
        // Diagnostics only: settlement always uses the already-held handle.
        let wait = unsafe { WaitForSingleObject(handle, 0) };
        format!(
            "renderer_pid={} wait_status={}",
            self.diagnostic_pid, wait.0
        )
    }
}

pub(super) fn capture(job: &Rc<Job>, context: String) {
    if !job.current() {
        return job.fail("process identity retired before execution");
    }
    if job.setup_deadline.borrow().is_none() {
        let Some(timer) = setup::Timer::start(job) else {
            return job.fail("process identity deadline unavailable");
        };
        *job.setup_deadline.borrow_mut() = Some(timer);
    }
    let native = (|| -> windows::core::Result<_> {
        let mut frame_id = 0;
        unsafe {
            job.webview
                .cast::<ICoreWebView2_20>()?
                .FrameId(&mut frame_id)?
        };
        let environment = unsafe { job.webview.cast::<ICoreWebView2_2>()?.Environment()? };
        Ok((frame_id, environment.cast::<ICoreWebView2Environment13>()?))
    })();
    let Ok((frame_id, environment)) = native else {
        return job.fail("native process identity unavailable before execution");
    };
    let captured = job.clone();
    let handler =
        GetProcessExtendedInfosCompletedHandler::create(Box::new(move |status, infos| {
            if !captured.current() {
                captured.fail("process identity retired before execution");
                return Ok(());
            }
            let witness = status
                .ok()
                .and_then(|()| {
                    infos
                        .as_ref()
                        .and_then(|infos| renderer(infos, frame_id).ok())
                })
                .flatten();
            let Some(witness) = witness else {
                captured.fail("native renderer identity unavailable before execution");
                return Ok(());
            };
            #[cfg(feature = "computer-use-probe")]
            if probe_fault::suppress(&captured, probe_fault::Phase::ProcessIdentityReply) {
                return Ok(());
            }
            if witness.exited() || !captured.world.attach(captured.ticket, witness) {
                captured.fail("native renderer retired before execution");
                return Ok(());
            }
            // The no-script setup timer must be gone before evaluation is queued.
            let timer = captured.setup_deadline.borrow_mut().take();
            drop(timer);
            captured.execute(context);
            Ok(())
        }));
    if unsafe { environment.GetProcessExtendedInfos(&handler) }.is_err() {
        job.fail("native process identity dispatch unavailable");
    }
}

fn renderer(
    infos: &ICoreWebView2ProcessExtendedInfoCollection,
    expected_frame: u32,
) -> windows::core::Result<Option<Arc<dyn process::PhysicalExit>>> {
    let mut count = 0;
    unsafe { infos.Count(&mut count)? };
    if count > 256 {
        return Ok(None);
    }
    let mut found = None;
    for index in 0..count {
        let info = unsafe { infos.GetValueAtIndex(index)? };
        let process = unsafe { info.ProcessInfo()? };
        let mut kind = COREWEBVIEW2_PROCESS_KIND::default();
        unsafe { process.Kind(&mut kind)? };
        if kind != COREWEBVIEW2_PROCESS_KIND_RENDERER {
            continue;
        }
        // WebView2's iterator borrows the collection's backing storage. Keep the
        // collection alive through the whole traversal; chaining these calls
        // drops it before GetCurrent and can crash inside the native runtime.
        let associated_frames = unsafe { info.AssociatedFrameInfos()? };
        let frames = unsafe { associated_frames.GetIterator()? };
        let mut has_current = BOOL::default();
        unsafe { frames.HasCurrent(&mut has_current)? };
        let mut visited = 0;
        while has_current.as_bool() {
            visited += 1;
            if visited > 1024 {
                return Ok(None);
            }
            let frame = unsafe { frames.GetCurrent()? }.cast::<ICoreWebView2FrameInfo2>()?;
            let mut id = 0;
            let mut kind = COREWEBVIEW2_FRAME_KIND::default();
            unsafe {
                frame.FrameId(&mut id)?;
                frame.FrameKind(&mut kind)?
            };
            if id == expected_frame && kind == COREWEBVIEW2_FRAME_KIND_MAIN_FRAME {
                if found.is_some() {
                    return Ok(None); // Ambiguous association never becomes authority.
                }
                let mut pid = 0;
                unsafe { process.ProcessId(&mut pid)? };
                let Ok(pid) = u32::try_from(pid) else {
                    return Ok(None);
                };
                if pid == 0 {
                    return Ok(None);
                }
                let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid)? };
                let owned = unsafe { OwnedHandle::from_raw_handle(handle.0) };
                found = Some(Arc::new(Witness {
                    handle: owned,
                    #[cfg(feature = "computer-use-probe")]
                    diagnostic_pid: pid,
                }) as Arc<dyn process::PhysicalExit>);
            }
            unsafe { frames.MoveNext(&mut has_current)? };
        }
    }
    Ok(found)
}

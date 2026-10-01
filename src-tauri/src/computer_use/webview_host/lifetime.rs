//! STA-local fixture resources. Native teardown must run before COM uninitializes.

use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use webview2_com::{
    BrowserProcessExitedEventHandler,
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2Environment, ICoreWebView2Environment5,
    },
};
use windows::core::Interface;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;

pub(super) struct Apartment(PhantomData<Rc<()>>);

impl Apartment {
    pub(super) fn enter() -> Result<Self, String> {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok() }
            .map_err(|error| error.to_string())?;
        Ok(Self(PhantomData))
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        // Declared before the HWND and every COM interface on this same STA.
        unsafe { CoUninitialize() };
    }
}

pub(super) struct Window(pub(super) HWND);

impl Drop for Window {
    fn drop(&mut self) {
        // Posting WM_DESTROY only delivers a notification; it does not destroy
        // a window. This HWND was created by this fixture on the current STA.
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}

/// Keep the disposable environment and its STA pump alive after controller
/// closure, as required to receive BrowserProcessExited. This fixture witness
/// does not settle scripts, authorize profile deletion, or affect shared views.
pub(super) struct BrowserExit {
    environment: ICoreWebView2Environment5,
    token: i64,
    pub(super) observed: Arc<AtomicBool>,
}

impl BrowserExit {
    pub(super) fn observe(
        environment: &ICoreWebView2Environment,
        webview: &ICoreWebView2,
    ) -> Result<Self, String> {
        let environment = environment
            .cast::<ICoreWebView2Environment5>()
            .map_err(|_| "owned browser exit observer unavailable")?;
        let mut expected = 0;
        unsafe { webview.BrowserProcessId(&mut expected) }
            .map_err(|_| "owned browser identity unavailable")?;
        let observed = Arc::new(AtomicBool::new(false));
        let seen = observed.clone();
        let handler = BrowserProcessExitedEventHandler::create(Box::new(move |_, args| {
            if let Some(args) = args {
                let mut exited = 0;
                unsafe { args.BrowserProcessId(&mut exited)? };
                if exited == expected {
                    seen.store(true, Ordering::SeqCst);
                }
            }
            Ok(())
        }));
        let mut token = 0;
        unsafe { environment.add_BrowserProcessExited(&handler, &mut token) }
            .map_err(|_| "owned browser exit observer failed")?;
        Ok(Self {
            environment,
            token,
            observed,
        })
    }
}

impl Drop for BrowserExit {
    fn drop(&mut self) {
        unsafe {
            let _ = self.environment.remove_BrowserProcessExited(self.token);
        }
    }
}

//! Exact observed UIA recipient/focus resolution for input routes.
use super::super::{patterns::ActionContext, source::NodeId};
use super::*;
impl RealUia {
    /// Resolve an observed UIA handle to its own HWND. Zero means a virtual
    /// element: targeted messages are unavailable, never guessed from a parent.
    pub(crate) async fn input_recipient(&self, node: NodeId, hwnd: u64) -> Result<(u64, String)> {
        self.worker
            .call(move |s, r| {
                let (element, props) = s.validate(node, hwnd, r)?;
                if !props.enabled
                    || props.partial
                    || super::super::sensitive::is_execution_surface(&props)
                {
                    return Err(DriverError::ElementNotActionable);
                }
                r.check()?;
                // SAFETY: exact validated MTA-local UIA interface, no mutation.
                let native = unsafe { element.CurrentNativeWindowHandle() }.map_err(native)?;
                Ok((
                    native.0 as usize as u64,
                    props.class_name.unwrap_or_default(),
                ))
            })
            .await
    }
    /// Focus only an exact validated element after authorized activation. Verify
    /// native UIA focus, preserve uncertainty after SetFocus; never retry.
    pub(crate) async fn focus_input(
        &self,
        node: NodeId,
        hwnd: u64,
        context: ActionContext,
    ) -> Result<()> {
        self.worker
            .call_action(context.clone(), move |s, r| {
                let (element, props) = s.validate(node, hwnd, r)?;
                if !props.enabled
                    || props.partial
                    || super::super::sensitive::is_execution_surface(&props)
                {
                    return Err(DriverError::ElementNotActionable);
                }
                r.check()?;
                // SAFETY: nonblocking probe; never focus after foreground changed.
                if unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }
                    != HWND(hwnd as usize as *mut _)
                {
                    return Err(DriverError::CapabilityUnavailable);
                }
                context.begin()?;
                // SAFETY: exact retained UIA element on this MTA. Calls have native
                // transaction limits plus independently cancellable worker wait.
                unsafe {
                    element.SetFocus().map_err(|e| context.error(native(e)))?;
                    let focused = s
                        .automation
                        .GetFocusedElement()
                        .map_err(|e| context.error(native(e)))?;
                    if !s
                        .automation
                        .CompareElements(&element, &focused)
                        .map_err(|e| context.error(native(e)))?
                        .as_bool()
                    {
                        return Err(context.error(DriverError::ElementNotActionable));
                    }
                }
                Ok(())
            })
            .await
    }
}

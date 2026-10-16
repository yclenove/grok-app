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

impl RealUia {
    pub(crate) async fn clipboard_safe(
        &self,
        el: super::super::UiaElement,
        text: String,
    ) -> Result<bool> {
        self.worker
            .call(move |s, r| {
                let (element, mut props) = s.validate(el.node, el.hwnd, r)?;
                if props.partial || !props.enabled {
                    return Ok(false);
                }
                // Evaluate exact ancestry, not just a safe-looking field label.
                props.focused = true;
                let mut tree = super::super::tree::map_properties(props, el.node, (0., 0.));
                let mut ancestor = element;
                // SAFETY: exact stamped root HWND, queried on the owning MTA.
                let root = unsafe {
                    s.automation
                        .ElementFromHandle(HWND(el.hwnd as usize as *mut _))
                }
                .map_err(native)?;
                for _ in 0..=super::super::tree::DEPTH_CAP {
                    r.check()?;
                    // SAFETY: both interfaces are live on this MTA. Descendants
                    // may report the root HWND too; only UIA identity proves the
                    // ancestor walk reached the root (including web ancestors).
                    let is_root = unsafe { s.automation.CompareElements(&ancestor, &root) }
                        .map_err(native)?;
                    if is_root.as_bool() {
                        return Ok(!gcu_core::policy::clipboard::is_sensitive(
                            &el.window.app.info,
                            gcu_core::model::RawTree {
                                root: Some(tree),
                                incomplete: None,
                            },
                            &text,
                        ));
                    }
                    // SAFETY: exact parent, no search by name or replacement element.
                    ancestor = unsafe { s.walker.GetParentElement(&ancestor) }.map_err(native)?;
                    // SAFETY: refresh this exact ancestor with bounded cached properties.
                    let cached = unsafe { ancestor.BuildUpdatedCache(&s.cache) }.map_err(native)?;
                    let mut props = super::properties::read(&cached);
                    if props.partial {
                        return Ok(false);
                    }
                    props.focused = false;
                    let mut parent = super::super::tree::map_properties(props, el.node, (0., 0.));
                    parent.children.push(tree);
                    tree = parent;
                }
                Ok(false)
            })
            .await
    }
    pub(crate) async fn verify_paste_focus(
        &self,
        el: super::super::UiaElement,
        context: ActionContext,
    ) -> Result<()> {
        self.worker
            .call_action(context.clone(), move |s, r| {
                r.check()?;
                let (element, props) = s.validate(el.node, el.hwnd, r)?;
                if props.partial || props.password || !props.enabled {
                    return Err(DriverError::ElementNotActionable);
                }
                // SAFETY: nonblocking foreground query, followed by exact UIA focus
                // comparison on its owning MTA. No focus is set or input dispatched.
                unsafe {
                    if windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow()
                        != HWND(el.hwnd as usize as *mut _)
                    {
                        return Err(DriverError::CapabilityUnavailable);
                    }
                    let focused = s.automation.GetFocusedElement().map_err(native)?;
                    if !s
                        .automation
                        .CompareElements(&element, &focused)
                        .map_err(native)?
                        .as_bool()
                    {
                        return Err(DriverError::CapabilityUnavailable);
                    }
                }
                context.check()
            })
            .await
    }
}

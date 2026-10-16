//! AppKit UI lives on the helper's main thread (ADR 0006).
use crate::{
    desktop::{parts, Part, UserEvent},
    Error, Rect, Scene,
};
use objc2::{
    define_class, msg_send, rc::Retained, runtime::AnyObject, DefinedClass, MainThreadMarker,
    MainThreadOnly,
};
use objc2_app_kit::*;
use objc2_core_graphics::{CGEvent, CGEventField};
use objc2_foundation::{
    NSDate, NSDefaultRunLoopMode, NSDictionary, NSPoint, NSRect, NSSize, NSString,
};
use std::cell::RefCell;

#[derive(Default)]
struct Visual {
    text: String,
    circle: bool,
    warning: bool,
}
define_class!(
    // SAFETY: NSView subclass with main-thread-only ivars and matching selectors below.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = RefCell<Visual>]
    struct OverlayView;
    impl OverlayView {
        // SAFETY: exact NSResponder selector signature, with no foreign pointers.
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool { false }
        // SAFETY: exact NSAccessibility selector signature, with no foreign pointers.
        #[unsafe(method(isAccessibilityElement))]
        fn accessibility_element(&self) -> bool { false }
        // SAFETY: exact NSView draw selector/NSRect ABI; the callback contains unwinds.
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            // Objective-C must never receive a Rust unwind, including on reentrancy.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.paint()));
        }
    }
);
impl OverlayView {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let allocated = Self::alloc(mtm).set_ivars(RefCell::new(Visual::default()));
        // SAFETY: initializing this subclass once with NSView's designated initializer.
        unsafe {
            msg_send![super(allocated), initWithFrame: NSRect::new(NSPoint::new(0., 0.), NSSize::new(1., 1.))]
        }
    }
    fn paint(&self) {
        let Ok(visual) = self.ivars().try_borrow() else {
            return;
        };
        let bounds = self.bounds();
        let color = if visual.circle || visual.text.is_empty() {
            NSColor::colorWithSRGBRed_green_blue_alpha(0.1, 0.8, 0.85, 0.9)
        } else if visual.warning {
            NSColor::colorWithSRGBRed_green_blue_alpha(0.48, 0.16, 0.09, 1.)
        } else {
            NSColor::colorWithSRGBRed_green_blue_alpha(0.09, 0.1, 0.12, 1.)
        };
        color.setFill();
        if visual.circle {
            NSBezierPath::bezierPathWithOvalInRect(bounds).fill();
        } else {
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                bounds,
                10_f64.min(bounds.size.height / 2.),
                10_f64.min(bounds.size.height / 2.),
            )
            .fill();
        }
        let font = NSFont::systemFontOfSize(14.);
        let white = NSColor::colorWithSRGBRed_green_blue_alpha(0.96, 0.96, 0.97, 1.);
        // SAFETY: AppKit attribute constants and values have the documented types;
        // dictionary contains only NSFont/NSColor, and all objects outlive drawing.
        unsafe {
            let attrs: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(
                &[NSFontAttributeName, NSForegroundColorAttributeName],
                &[&*font as &AnyObject, &*white as &AnyObject],
            );
            let text_bounds = NSRect::new(
                NSPoint::new(8., (bounds.size.height - 20.) / 2.),
                NSSize::new((bounds.size.width - 16.).max(1.), 20.),
            );
            NSString::from_str(&visual.text).drawInRect_withAttributes(text_bounds, Some(&attrs));
        }
    }
}
define_class!(
    // SAFETY: main-thread NSPanel subclass; this is its documented superclass chain.
    #[unsafe(super(NSPanel, NSWindow, NSResponder, objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    struct OverlayPanel;
    impl OverlayPanel {
        // SAFETY: exact NSWindow selector signature, with no foreign pointers.
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool { false }
        // SAFETY: exact NSWindow selector signature, with no foreign pointers.
        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self) -> bool { false }
        // SAFETY: exact NSAccessibility selector signature, with no foreign pointers.
        #[unsafe(method(isAccessibilityElement))]
        fn accessibility_element(&self) -> bool { false }
    }
);
struct Window {
    panel: Retained<OverlayPanel>,
    view: Retained<OverlayView>,
    part: Part,
    down: bool,
}
impl Drop for Window {
    fn drop(&mut self) {
        self.panel.orderOut(None);
        self.panel.close();
    }
}
pub(crate) struct Surface {
    mtm: MainThreadMarker,
    app: Retained<NSApplication>,
    windows: Vec<Window>,
    scale: Option<f64>,
}
impl Surface {
    pub fn open() -> Result<Self, Error> {
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| Error::native("AppKit overlay requires helper main thread"))?;
        // Establish WindowServer connection before driver capture as required by ADR 0006.
        let app = NSApplication::sharedApplication(mtm);
        if NSScreen::screens(mtm).is_empty() {
            return Err(Error::native("interactive AppKit screen unavailable"));
        }
        Ok(Self {
            mtm,
            app,
            windows: Vec::new(),
            scale: None,
        })
    }
    pub fn coordinate_scale(&mut self, scale: Option<f64>) {
        // Driver geometry scales both origin and extent by the target window's
        // backing factor. Trying each monitor's factor makes overlapping pixel
        // spaces ambiguous on a Retina primary plus an external 1x display.
        self.scale = scale;
    }
    fn display(&self, target: Option<Rect>) -> Result<(Retained<NSScreen>, f64), Error> {
        let screens = NSScreen::screens(self.mtm);
        let primary = screens
            .firstObject()
            .ok_or_else(|| Error::native("AppKit primary screen unavailable"))?;
        let top = primary.frame().size.height;
        let scale = self.scale.unwrap_or_else(|| primary.backingScaleFactor());
        let screen = target
            .and_then(|target| {
                screens.iter().find(|s| {
                    let r = s.frame();
                    let x = (target.x + target.w / 2.) / scale;
                    let y = top - (target.y + target.h / 2.) / scale;
                    x >= r.origin.x
                        && x < r.origin.x + r.size.width
                        && y >= r.origin.y
                        && y < r.origin.y + r.size.height
                })
            })
            .unwrap_or(primary);
        Ok((screen, top))
    }
    pub fn monitor(&self, target: Option<Rect>) -> Result<Rect, Error> {
        let (screen, top) = self.display(target)?;
        let r = screen.visibleFrame();
        let scale = self.scale.unwrap_or_else(|| screen.backingScaleFactor());
        Ok(Rect {
            x: r.origin.x * scale,
            y: (top - r.origin.y - r.size.height) * scale,
            w: r.size.width * scale,
            h: r.size.height * scale,
        })
    }
    pub fn events(&mut self) -> Result<Vec<UserEvent>, Error> {
        let mut events = Vec::new();
        for _ in 0..128 {
            // SAFETY: framework-provided run-loop mode is initialized by AppKit.
            let mode = unsafe { NSDefaultRunLoopMode };
            let Some(event) = self.app.nextEventMatchingMask_untilDate_inMode_dequeue(
                NSEventMask::Any,
                Some(&NSDate::distantPast()),
                mode,
                true,
            ) else {
                break;
            };
            if let Some(w) = self
                .windows
                .iter_mut()
                .find(|w| w.panel.windowNumber() == event.windowNumber())
            {
                // NSEvent synthesized by accessibility or a client lacks a HID
                // event. CGEvent posting stamps a nonzero source process ID.
                let physical = event.CGEvent().is_some_and(|cg| {
                    hardware_source(
                        CGEvent::integer_value_field(
                            Some(&cg),
                            CGEventField::EventSourceUnixProcessID,
                        ),
                        CGEvent::integer_value_field(Some(&cg), CGEventField::EventSourceStateID),
                    )
                });
                if event.r#type() == NSEventType::LeftMouseDown {
                    w.down = physical;
                }
                if event.r#type() == NSEventType::LeftMouseUp {
                    let down = std::mem::take(&mut w.down);
                    let p = event.locationInWindow();
                    let bounds = w.view.bounds();
                    if physical
                        && down
                        && p.x >= 0.
                        && p.y >= 0.
                        && p.x < bounds.size.width
                        && p.y < bounds.size.height
                    {
                        if let Some(action) = w.part.action {
                            events.push(UserEvent {
                                action,
                                revision: w.part.revision,
                            });
                        }
                    }
                }
            } else {
                self.app.sendEvent(&event);
            }
        }
        Ok(events)
    }
    pub fn render(&mut self, scene: &Scene) -> Result<(), Error> {
        let next = parts(scene);
        if self.windows.len() != next.len()
            || self
                .windows
                .iter()
                .zip(&next)
                .any(|(w, p)| w.part.action != p.action)
        {
            self.windows.clear();
            for part in next {
                let window = self.create(part)?;
                self.windows.push(window);
            }
        } else {
            for (w, p) in self.windows.iter_mut().zip(next) {
                if w.part.revision != p.revision && p.action != Some(crate::ButtonAction::Stop) {
                    w.down = false;
                }
                w.part = p;
            }
        }
        for w in &self.windows {
            let r = w.part.frame;
            let (display, top) = self.display(Some(r))?;
            let scale = self.scale.unwrap_or_else(|| display.backingScaleFactor());
            w.panel.setFrame_display(
                NSRect::new(
                    NSPoint::new(r.x / scale, top - (r.y + r.h) / scale),
                    NSSize::new(r.w / scale, r.h / scale),
                ),
                false,
            );
            *w.view.ivars().borrow_mut() = Visual {
                text: w.part.text.clone(),
                circle: w.part.circle,
                warning: w.part.warning,
            };
            w.view.setNeedsDisplay(true);
            w.panel.orderFrontRegardless();
            w.view.displayIfNeeded();
        }
        Ok(())
    }
    fn create(&self, part: Part) -> Result<Window, Error> {
        let allocated = OverlayPanel::alloc(self.mtm).set_ivars(());
        // SAFETY: initialize the allocated subclass exactly once with NSPanel's
        // designated initializer; the thread marker proves AppKit main-thread use.
        let panel: Retained<OverlayPanel> = unsafe {
            msg_send![super(allocated),
            initWithContentRect: NSRect::new(NSPoint::new(0.,0.), NSSize::new(1.,1.)),
            styleMask: NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            backing: NSBackingStoreType::Buffered, defer: false]
        };
        // SAFETY: retained owner closes once and releases afterward; AppKit must
        // not autorelease the object on close in addition to the Rust owner.
        unsafe {
            panel.setReleasedWhenClosed(false);
        }
        panel.setSharingType(NSWindowSharingType::None);
        if panel.sharingType() != NSWindowSharingType::None {
            return Err(Error::native("NSPanel screenshot exclusion rejected"));
        }
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setHasShadow(false);
        panel.setLevel(NSFloatingWindowLevel);
        panel.setHidesOnDeactivate(false);
        panel.setBecomesKeyOnlyIfNeeded(true);
        panel.setIgnoresMouseEvents(part.action.is_none());
        let view = OverlayView::new(self.mtm);
        panel.setContentView(Some(&view));
        Ok(Window {
            panel,
            view,
            part,
            down: false,
        })
    }
    pub fn hide(&mut self) {
        self.windows.clear();
    }
}
fn hardware_source(pid: i64, state: i64) -> bool {
    pid == 0 && state == 1
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_posted_or_private_source_cannot_activate_stop_resume_or_approval() {
        assert!(!hardware_source(1234, 1));
        assert!(!hardware_source(0, -1));
        assert!(!hardware_source(0, 0));
        assert!(hardware_source(0, 1));
    }
}

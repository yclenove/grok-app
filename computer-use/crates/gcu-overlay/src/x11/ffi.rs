//! Dynamically loaded system Cairo/Pango: Unicode shaping, no bundled runtime.
use crate::{desktop::Part, Error};
use libc::{c_char, c_int, c_void};
use std::{ffi::CString, ptr::NonNull};
type Ptr = *mut c_void;
struct Library(NonNull<c_void>);
impl Library {
    fn open(name: &std::ffi::CStr) -> Result<Self, Error> {
        // SAFETY: static NUL-terminated SONAME; symbols are used only while owned.
        let pointer = unsafe {
            libc::dlopen(
                name.as_ptr(),
                libc::RTLD_NOW | libc::RTLD_LOCAL | libc::RTLD_NODELETE,
            )
        };
        NonNull::new(pointer).map(Self).ok_or_else(|| {
            Error::native(format!(
                "system library missing: {}",
                name.to_string_lossy()
            ))
        })
    }
    fn symbol(&self, name: &std::ffi::CStr) -> Result<Ptr, Error> {
        // SAFETY: handle is live, name is NUL-terminated; caller supplies exact ABI.
        let pointer = unsafe { libc::dlsym(self.0.as_ptr(), name.as_ptr()) };
        if pointer.is_null() {
            Err(Error::native("Cairo/Pango symbol unavailable"))
        } else {
            Ok(pointer)
        }
    }
}
impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: close this owner's reference once. NODELETE keeps code alive
        // for system Pango/GObject font caches and their process-global workers.
        unsafe {
            libc::dlclose(self.0.as_ptr());
        }
    }
}
macro_rules! symbol {
    ($lib:expr, $name:literal, $ty:ty) => {{
        let pointer = $lib.symbol($name)?;
        // SAFETY: the specified signature is the documented system C symbol ABI;
        // owning Library fields outlive every invocation of the function pointer.
        unsafe { std::mem::transmute::<Ptr, $ty>(pointer) }
    }};
}
pub(super) struct Painter {
    surface_create: unsafe extern "C" fn(c_int, c_int, c_int) -> Ptr,
    surface_destroy: unsafe extern "C" fn(Ptr),
    surface_status: unsafe extern "C" fn(Ptr) -> c_int,
    surface_flush: unsafe extern "C" fn(Ptr),
    surface_data: unsafe extern "C" fn(Ptr) -> *const u8,
    surface_stride: unsafe extern "C" fn(Ptr) -> c_int,
    create: unsafe extern "C" fn(Ptr) -> Ptr,
    destroy: unsafe extern "C" fn(Ptr),
    status: unsafe extern "C" fn(Ptr) -> c_int,
    rgba: unsafe extern "C" fn(Ptr, f64, f64, f64, f64),
    arc: unsafe extern "C" fn(Ptr, f64, f64, f64, f64, f64),
    fill: unsafe extern "C" fn(Ptr),
    move_to: unsafe extern "C" fn(Ptr, f64, f64),
    layout_create: unsafe extern "C" fn(Ptr) -> Ptr,
    layout_text: unsafe extern "C" fn(Ptr, *const c_char, c_int),
    layout_width: unsafe extern "C" fn(Ptr, c_int),
    layout_ellipsize: unsafe extern "C" fn(Ptr, c_int),
    layout_font: unsafe extern "C" fn(Ptr, Ptr),
    font_create: unsafe extern "C" fn(*const c_char) -> Ptr,
    font_free: unsafe extern "C" fn(Ptr),
    show_layout: unsafe extern "C" fn(Ptr, Ptr),
    unref: unsafe extern "C" fn(Ptr),
    _libs: [Library; 4],
}
impl Painter {
    pub fn open() -> Result<Self, Error> {
        let c = Library::open(c"libcairo.so.2")?;
        let pc = Library::open(c"libpangocairo-1.0.so.0")?;
        let p = Library::open(c"libpango-1.0.so.0")?;
        let g = Library::open(c"libgobject-2.0.so.0")?;
        Ok(Self {
            surface_create: symbol!(
                c,
                c"cairo_image_surface_create",
                unsafe extern "C" fn(c_int, c_int, c_int) -> Ptr
            ),
            surface_destroy: symbol!(c, c"cairo_surface_destroy", unsafe extern "C" fn(Ptr)),
            surface_status: symbol!(
                c,
                c"cairo_surface_status",
                unsafe extern "C" fn(Ptr) -> c_int
            ),
            surface_flush: symbol!(c, c"cairo_surface_flush", unsafe extern "C" fn(Ptr)),
            surface_data: symbol!(
                c,
                c"cairo_image_surface_get_data",
                unsafe extern "C" fn(Ptr) -> *const u8
            ),
            surface_stride: symbol!(
                c,
                c"cairo_image_surface_get_stride",
                unsafe extern "C" fn(Ptr) -> c_int
            ),
            create: symbol!(c, c"cairo_create", unsafe extern "C" fn(Ptr) -> Ptr),
            destroy: symbol!(c, c"cairo_destroy", unsafe extern "C" fn(Ptr)),
            status: symbol!(c, c"cairo_status", unsafe extern "C" fn(Ptr) -> c_int),
            rgba: symbol!(
                c,
                c"cairo_set_source_rgba",
                unsafe extern "C" fn(Ptr, f64, f64, f64, f64)
            ),
            arc: symbol!(
                c,
                c"cairo_arc",
                unsafe extern "C" fn(Ptr, f64, f64, f64, f64, f64)
            ),
            fill: symbol!(c, c"cairo_fill", unsafe extern "C" fn(Ptr)),
            move_to: symbol!(c, c"cairo_move_to", unsafe extern "C" fn(Ptr, f64, f64)),
            layout_create: symbol!(
                pc,
                c"pango_cairo_create_layout",
                unsafe extern "C" fn(Ptr) -> Ptr
            ),
            show_layout: symbol!(
                pc,
                c"pango_cairo_show_layout",
                unsafe extern "C" fn(Ptr, Ptr)
            ),
            layout_text: symbol!(
                p,
                c"pango_layout_set_text",
                unsafe extern "C" fn(Ptr, *const c_char, c_int)
            ),
            layout_width: symbol!(
                p,
                c"pango_layout_set_width",
                unsafe extern "C" fn(Ptr, c_int)
            ),
            layout_ellipsize: symbol!(
                p,
                c"pango_layout_set_ellipsize",
                unsafe extern "C" fn(Ptr, c_int)
            ),
            layout_font: symbol!(
                p,
                c"pango_layout_set_font_description",
                unsafe extern "C" fn(Ptr, Ptr)
            ),
            font_create: symbol!(
                p,
                c"pango_font_description_from_string",
                unsafe extern "C" fn(*const c_char) -> Ptr
            ),
            font_free: symbol!(p, c"pango_font_description_free", unsafe extern "C" fn(Ptr)),
            unref: symbol!(g, c"g_object_unref", unsafe extern "C" fn(Ptr)),
            _libs: [c, pc, p, g],
        })
    }
    pub fn paint(&self, part: &Part, width: u16, height: u16) -> Result<Vec<u8>, Error> {
        let text = CString::new(part.text.as_bytes()).map_err(Error::native)?;
        // SAFETY: all pointers are system-library objects used on this thread;
        // dimensions are bounded u16, all allocations/status/stride are checked,
        // and each owned reference is released after the final use below.
        unsafe {
            let surface = (self.surface_create)(0, width.into(), height.into()); // CAIRO_FORMAT_ARGB32
            if surface.is_null() {
                return Err(Error::native("Cairo surface allocation failed"));
            }
            if (self.surface_status)(surface) != 0 {
                (self.surface_destroy)(surface);
                return Err(Error::native("Cairo ARGB surface failed"));
            }
            let cr = (self.create)(surface);
            if cr.is_null() {
                (self.surface_destroy)(surface);
                return Err(Error::native("Cairo context failed"));
            }
            if part.circle {
                (self.rgba)(cr, 0.1, 0.8, 0.85, 0.9);
                (self.arc)(
                    cr,
                    f64::from(width) / 2.,
                    f64::from(height) / 2.,
                    f64::from(width.min(height)) / 2.,
                    0.,
                    std::f64::consts::TAU,
                );
            } else {
                if part.text.is_empty() {
                    (self.rgba)(cr, 0.1, 0.8, 0.85, 1.);
                } else if part.warning {
                    (self.rgba)(cr, 0.48, 0.16, 0.09, 1.);
                } else {
                    (self.rgba)(cr, 0.09, 0.1, 0.12, 1.);
                }
                let w = f64::from(width);
                let h = f64::from(height);
                let r = 10_f64.min(w / 2.).min(h / 2.);
                for (x, y, start) in [
                    (w - r, r, -std::f64::consts::FRAC_PI_2),
                    (w - r, h - r, 0.),
                    (r, h - r, std::f64::consts::FRAC_PI_2),
                    (r, r, std::f64::consts::PI),
                ] {
                    (self.arc)(cr, x, y, r, start, start + std::f64::consts::FRAC_PI_2);
                }
            }
            (self.fill)(cr);
            let layout = (self.layout_create)(cr);
            let font = (self.font_create)(c"sans 11".as_ptr());
            let valid = !layout.is_null() && !font.is_null();
            if valid {
                (self.layout_text)(layout, text.as_ptr(), -1);
                (self.layout_width)(layout, (i32::from(width) - 16).max(1) * 1024);
                (self.layout_ellipsize)(layout, 3);
                (self.layout_font)(layout, font);
                (self.rgba)(cr, 0.96, 0.96, 0.97, 1.);
                (self.move_to)(cr, 8., (f64::from(height) - 20.) / 2.);
                (self.show_layout)(cr, layout);
            }
            if !font.is_null() {
                (self.font_free)(font);
            }
            if !layout.is_null() {
                (self.unref)(layout);
            }
            (self.surface_flush)(surface);
            let data = (self.surface_data)(surface);
            let stride = (self.surface_stride)(surface);
            let pixels = if valid
                && (self.status)(cr) == 0
                && !data.is_null()
                && stride == i32::from(width) * 4
            {
                Some(
                    std::slice::from_raw_parts(data, usize::from(width) * usize::from(height) * 4)
                        .to_vec(),
                )
            } else {
                None
            };
            (self.destroy)(cr);
            (self.surface_destroy)(surface);
            pixels.ok_or_else(|| Error::native("Pango rendering or Cairo pixel buffer unavailable"))
        }
    }
}

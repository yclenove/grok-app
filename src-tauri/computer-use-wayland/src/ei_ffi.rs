//! Small libei ABI surface, linked to verified static libei >= 1.5 by build.rs.
//! Objects never leave the native owner thread.
//! Declarations follow upstream libei.h; opaque objects are reference counted.
use std::ffi::{c_char, c_int, c_void};
pub enum Ei {}
pub enum Seat {}
pub enum Device {}
pub enum Region {}
pub enum Event {}
pub const POINTER: u32 = 1;
pub const ABSOLUTE: u32 = 2;
pub const KEYBOARD: u32 = 4;
pub const SCROLL: u32 = 16;
pub const BUTTON: u32 = 32;
extern "C" {
    pub fn ei_new_sender(data: *mut c_void) -> *mut Ei;
    pub fn ei_unref(ei: *mut Ei) -> *mut Ei;
    pub fn ei_configure_name(ei: *mut Ei, name: *const c_char);
    pub fn ei_setup_backend_fd(ei: *mut Ei, fd: c_int) -> c_int;
    pub fn ei_get_fd(ei: *mut Ei) -> c_int;
    pub fn ei_dispatch(ei: *mut Ei);
    pub fn ei_get_event(ei: *mut Ei) -> *mut Event;
    pub fn ei_event_unref(event: *mut Event) -> *mut Event;
    pub fn ei_event_get_type(event: *mut Event) -> u32;
    pub fn ei_event_get_seat(event: *mut Event) -> *mut Seat;
    pub fn ei_event_get_device(event: *mut Event) -> *mut Device;
    pub fn ei_seat_bind_capabilities(seat: *mut Seat, ...);
    pub fn ei_device_ref(device: *mut Device) -> *mut Device;
    pub fn ei_device_unref(device: *mut Device) -> *mut Device;
    pub fn ei_device_get_type(device: *mut Device) -> u32;
    pub fn ei_device_has_capability(device: *mut Device, capability: u32) -> bool;
    pub fn ei_device_get_region(device: *mut Device, index: usize) -> *mut Region;
    pub fn ei_region_get_mapping_id(region: *mut Region) -> *const c_char;
    pub fn ei_region_get_x(region: *mut Region) -> u32;
    pub fn ei_region_get_y(region: *mut Region) -> u32;
    pub fn ei_region_get_width(region: *mut Region) -> u32;
    pub fn ei_region_get_height(region: *mut Region) -> u32;
    pub fn ei_region_get_physical_scale(region: *mut Region) -> f64;
    pub fn ei_now(ei: *mut Ei) -> u64;
    pub fn ei_device_start_emulating(device: *mut Device, sequence: u32);
    pub fn ei_device_stop_emulating(device: *mut Device);
    pub fn ei_device_frame(device: *mut Device, time: u64);
    pub fn ei_device_pointer_motion(device: *mut Device, x: f64, y: f64);
    pub fn ei_device_pointer_motion_absolute(device: *mut Device, x: f64, y: f64);
    pub fn ei_device_button_button(device: *mut Device, code: u32, pressed: bool);
    pub fn ei_device_keyboard_key(device: *mut Device, code: u32, pressed: bool);
    pub fn ei_device_scroll_delta(device: *mut Device, x: f64, y: f64);
    pub fn ei_device_scroll_discrete(device: *mut Device, x: i32, y: i32);
    pub fn ei_device_scroll_cancel(device: *mut Device, x: bool, y: bool);
}

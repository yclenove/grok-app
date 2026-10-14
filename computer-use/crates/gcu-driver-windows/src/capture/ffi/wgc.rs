//! WGC interop, single-frame ownership and bounded D3D11 readback.
use super::super::{bgra_to_rgba, native, CapturePath, RawPixels, SourceWindow};
use super::{map_error, preflight, Request};
use gcu_core::driver::{DriverError, Result};
use std::{sync::mpsc, time::Duration};
use windows::{
    core::{factory, IInspectable, Interface},
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{
            Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem,
            GraphicsCaptureSession,
        },
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
    },
    Win32::{
        Foundation::{HMODULE, HWND, RECT},
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            Direct3D11::*,
            Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS},
            Dxgi::{IDXGIDevice, DXGI_ERROR_WAS_STILL_DRAWING},
        },
        System::WinRT::{
            Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
            Graphics::Capture::IGraphicsCaptureItemInterop,
        },
    },
};

pub(in crate::capture) fn supported() -> bool {
    GraphicsCaptureSession::IsSupported().unwrap_or(false)
}

struct Stream {
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    token: Option<i64>,
}
impl Drop for Stream {
    fn drop(&mut self) {
        if let Some(token) = self.token {
            if self.pool.RemoveFrameArrived(token).is_err() {
                tracing::debug!("WGC callback cleanup failed");
            }
        }
        if self.session.Close().is_err() {
            tracing::debug!("WGC session cleanup failed");
        }
        if self.pool.Close().is_err() {
            tracing::debug!("WGC frame pool cleanup failed");
        }
    }
}
struct Frame(Direct3D11CaptureFrame);
impl Drop for Frame {
    fn drop(&mut self) {
        if self.0.Close().is_err() {
            tracing::debug!("WGC frame cleanup failed");
        }
    }
}
fn optional_property(result: windows::core::Result<()>) -> Result<()> {
    match result {
        Ok(()) => Ok(()),
        // Runtime interface probing: older Windows releases lack these setters.
        Err(e) if [0x80004002u32 as i32, 0x80004001u32 as i32].contains(&e.code().0) => Ok(()),
        Err(e) => Err(map_error(e)),
    }
}
fn devices() -> Result<(ID3D11Device, ID3D11DeviceContext, IDirect3DDevice)> {
    let mut device = None;
    let mut context = None;
    // SAFETY: owned output slots; device/context stay on this capture MTA thread.
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .map_err(map_error)?;
    }
    let device = device.ok_or_else(|| native("D3D11 device missing"))?;
    let context = context.ok_or_else(|| native("D3D11 context missing"))?;
    let dxgi: IDXGIDevice = device.cast().map_err(map_error)?;
    // SAFETY: wraps the live owned DXGI device; the WinRT reference owns its lease.
    let runtime: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }
        .map_err(map_error)?
        .cast()
        .map_err(map_error)?;
    Ok((device, context, runtime))
}
pub(in crate::capture) fn capture(window: &SourceWindow, request: &Request) -> Result<RawPixels> {
    let hwnd = preflight(window, request, CapturePath::Wgc)?;
    if !supported() {
        return Err(DriverError::CapabilityUnavailable);
    }
    let interop: IGraphicsCaptureItemInterop =
        factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(map_error)?;
    // SAFETY: only the authorized HWND is used; never a monitor or WindowId route.
    let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(hwnd) }.map_err(map_error)?;
    let size = item.Size().map_err(map_error)?;
    if size.Width <= 0
        || size.Height <= 0
        || super::super::source::byte_len(size.Width as u32, size.Height as u32).is_none()
    {
        return Err(native("WGC content dimensions invalid"));
    }
    let content_bounds = extended_bounds(hwnd)?;
    let (device, context, runtime) = devices()?;
    request.check()?;
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &runtime,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        1,
        size,
    )
    .map_err(map_error)?;
    let session = match pool.CreateCaptureSession(&item) {
        Ok(session) => session,
        Err(error) => {
            if pool.Close().is_err() {
                tracing::debug!("WGC unused pool cleanup failed");
            }
            return Err(map_error(error));
        }
    };
    let mut stream = Stream {
        pool,
        session,
        token: None,
    };
    optional_property(stream.session.SetIsBorderRequired(false))?;
    optional_property(stream.session.SetIsCursorCaptureEnabled(false))?;
    let (notify, arrivals) = mpsc::sync_channel(1);
    let handler =
        TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(move |_, _| {
            // Only a bounded wakeup crosses the callback; no COM frame or pixels.
            // Contain unwinding before returning into WinRT.
            std::panic::catch_unwind(|| {
                let _ = notify.try_send(());
            })
            .map_err(|_| {
                windows::core::Error::from_hresult(windows::core::HRESULT(0x80004005u32 as i32))
            })
        });
    stream.token = Some(stream.pool.FrameArrived(&handler).map_err(map_error)?);
    request.check()?;
    stream.session.StartCapture().map_err(map_error)?;
    loop {
        request.check()?;
        let wait = request
            .deadline
            .saturating_duration_since(std::time::Instant::now())
            .min(Duration::from_millis(20));
        match arrivals.recv_timeout(wait) {
            Ok(()) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(native("WGC callback disconnected"))
            }
        }
    }
    request.check()?;
    let frame = Frame(stream.pool.TryGetNextFrame().map_err(map_error)?);
    let actual = frame.0.ContentSize().map_err(map_error)?;
    if actual != size {
        return Err(native("WGC content resized during capture"));
    }
    let pixels = readback(
        &frame.0,
        &device,
        &context,
        actual.Width as u32,
        actual.Height as u32,
        request,
    )?;
    pixels.validate()?;
    preflight(window, request, CapturePath::Wgc)?;
    if extended_bounds(hwnd)? != content_bounds {
        return Err(native("WGC content moved during capture"));
    }
    align(window, content_bounds, pixels)
}
fn extended_bounds(hwnd: HWND) -> Result<gcu_core::model::Rect> {
    let mut rect = RECT::default();
    // SAFETY: trusted HWND and correctly sized, writable RECT output.
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut rect as *mut RECT).cast(),
            std::mem::size_of::<RECT>() as u32,
        )
    }
    .map_err(map_error)?;
    Ok(gcu_core::model::Rect {
        x: f64::from(rect.left),
        y: f64::from(rect.top),
        w: f64::from(rect.right - rect.left),
        h: f64::from(rect.bottom - rect.top),
    })
}
fn align(
    window: &SourceWindow,
    bounds: gcu_core::model::Rect,
    pixels: RawPixels,
) -> Result<RawPixels> {
    // WGC may omit invisible resize borders. Preserve GetWindowRect/UIA's origin
    // by adding opaque padding, never scaling or sampling another window.
    super::super::source::align_pixels(window, bounds, pixels)
}
struct Mapped<'a> {
    context: &'a ID3D11DeviceContext,
    texture: &'a ID3D11Texture2D,
}
impl Drop for Mapped<'_> {
    fn drop(&mut self) {
        // SAFETY: balances the successful Map of this owned staging texture.
        unsafe {
            self.context.Unmap(self.texture, 0);
        }
    }
}
fn readback(
    frame: &Direct3D11CaptureFrame,
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    width: u32,
    height: u32,
    request: &Request,
) -> Result<RawPixels> {
    let access: IDirect3DDxgiInterfaceAccess = frame
        .Surface()
        .map_err(map_error)?
        .cast()
        .map_err(map_error)?;
    // SAFETY: the frame owns a live D3D11 surface until copying is complete.
    let texture: ID3D11Texture2D = unsafe { access.GetInterface() }.map_err(map_error)?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: writable descriptor for the owned source texture.
    unsafe {
        texture.GetDesc(&mut desc);
    }
    if desc.Width < width
        || desc.Height < height
        || super::super::source::byte_len(desc.Width, desc.Height).is_none()
        || desc.Format != windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM
    {
        return Err(native("WGC texture format/dimensions invalid"));
    }
    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    desc.MiscFlags = 0;
    let mut staging = None;
    // SAFETY: source-compatible descriptor and owned output slot; no raw data.
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut staging)) }.map_err(map_error)?;
    let staging = staging.ok_or_else(|| native("D3D11 staging texture missing"))?;
    request.check()?;
    // SAFETY: textures share dimensions/format/device; staging is exclusively owned.
    unsafe {
        context.CopyResource(&staging, &texture);
    }
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    loop {
        request.check()?;
        // SAFETY: live staging resource and writable mapping output. DO_NOT_WAIT
        // prevents GPU stalls from blocking the worker's frame deadline.
        match unsafe {
            context.Map(
                &staging,
                0,
                D3D11_MAP_READ,
                D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
                Some(&mut mapped),
            )
        } {
            Ok(()) => break,
            Err(e) if e.code() == DXGI_ERROR_WAS_STILL_DRAWING => {
                std::thread::sleep(Duration::from_millis(2))
            }
            Err(e) => return Err(map_error(e)),
        }
    }
    let _mapped = Mapped {
        context,
        texture: &staging,
    };
    let stride = mapped.RowPitch as usize;
    let len = stride
        .checked_mul(height as usize)
        .filter(|l| *l <= gcu_core::redact::MAX_CAPTURE_BYTES)
        .ok_or_else(|| native("WGC mapped buffer too large"))?;
    if mapped.pData.is_null() || stride < width as usize * 4 {
        return Err(native("WGC mapped buffer invalid"));
    }
    // SAFETY: Map exposes RowPitch bytes per row for a validated texture height;
    // the owned staging resource stays mapped until after copying this slice.
    let data = unsafe { std::slice::from_raw_parts(mapped.pData.cast::<u8>(), len) };
    let rgba =
        bgra_to_rgba(data, width, height, stride).ok_or_else(|| native("WGC stride invalid"))?;
    request.check()?;
    Ok(RawPixels {
        rgba,
        width,
        height,
    })
}

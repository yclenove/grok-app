// Ignored live measurement of where `MOVE|ABSOLUTE|VIRTUALDESK` actually lands.
// Included from `unsafe_click_probe.rs`. Formulas match `pointer_settle` and are duplicated
// because that module is crate-private.
use std::time::{Duration, Instant};
use windows::Win32::{
    Foundation::{GetLastError, LPARAM},
    Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    },
    UI::{
        HiDpi::{
            AreDpiAwarenessContextsEqual, GetThreadDpiAwarenessContext,
            SetProcessDpiAwarenessContext, SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT,
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            DPI_AWARENESS_CONTEXT_SYSTEM_AWARE, DPI_AWARENESS_CONTEXT_UNAWARE,
            DPI_AWARENESS_CONTEXT_UNAWARE_GDISCALED,
        },
        Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_MOVE,
            MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT,
        },
    },
};

/// Counts the caller asserts. `text` is the histogram, including when this returns `Err`.
pub struct LandingReport {
    pub planned: u32,
    pub sent_ok: u32,
    pub valid_reads: u32,
    pub text: String,
}

struct Metrics {
    origin_x: i32,
    origin_y: i32,
    width: i32,
    height: i32,
}

struct Monitor {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    primary: bool,
}

struct ThreadDpi(*mut core::ffi::c_void);
impl Drop for ThreadDpi {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: restores the context this thread saved from a successful set.
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT(self.0)) };
        }
    }
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.0
    }
}

fn normalize_axis(pixel: i32, origin: i32, extent: i32) -> i32 {
    ((f64::from(pixel) - f64::from(origin)) * 65535. / f64::from(extent - 1)).round() as i32
}
fn denormalize_axis(norm: i32, origin: i32, extent: i32) -> i32 {
    origin + (f64::from(norm) * f64::from(extent - 1) / 65535.).round() as i32
}
fn bin_axis(norm: i32, origin: i32, extent: i32) -> i32 {
    origin + (i64::from(norm) * i64::from(extent) / 65536) as i32
}
fn disagrees(pixel: i32, origin: i32, extent: i32) -> bool {
    let norm = normalize_axis(pixel, origin, extent);
    denormalize_axis(norm, origin, extent) != bin_axis(norm, origin, extent)
}
fn disagreement_offsets(extent: i32) -> Vec<i32> {
    if extent <= 1 {
        return Vec::new();
    }
    (0..extent)
        .filter(|offset| disagrees(*offset, 0, extent))
        .collect()
}
fn on_axis(pixel: i32, origin: i32, extent: i32) -> bool {
    extent > 0 && pixel >= origin && pixel < origin.saturating_add(extent)
}

fn bump(hist: &mut [u32; 17], delta: i32) {
    let index = (delta + 8).clamp(0, 16) as usize;
    hist[index] += 1;
}
fn format_hist(hist: &[u32; 17]) -> String {
    let mut parts = Vec::new();
    for (index, count) in hist.iter().enumerate() {
        let label = match index {
            0 => "<=-8".to_string(),
            16 => ">=8".to_string(),
            n => format!("{}", n as i32 - 8),
        };
        parts.push(format!("{label}:{count}"));
    }
    parts.join(" ")
}

fn dpi_label(context: DPI_AWARENESS_CONTEXT) -> &'static str {
    let pairs = [
        (DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, "PER_MONITOR_AWARE_V2"),
        (DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE, "PER_MONITOR_AWARE"),
        (DPI_AWARENESS_CONTEXT_SYSTEM_AWARE, "SYSTEM_AWARE"),
        (DPI_AWARENESS_CONTEXT_UNAWARE, "UNAWARE"),
        (DPI_AWARENESS_CONTEXT_UNAWARE_GDISCALED, "UNAWARE_GDISCALED"),
    ];
    for (candidate, name) in pairs {
        // SAFETY: compares context handles. It does not change process state.
        if unsafe { AreDpiAwarenessContextsEqual(context, candidate) }.as_bool() {
            return name;
        }
    }
    "OTHER"
}

fn enter_pmv2() -> Result<(String, ThreadDpi), String> {
    // Process first, then the thread, matching the worker's thread context.
    // A process call often fails once awareness is already set; the thread context is the gate.
    let process = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let process_note = match process {
        Ok(()) => "process=PER_MONITOR_AWARE_V2".to_string(),
        Err(error) => {
            let raw = error.code().0 as u32;
            let extra = unsafe { GetLastError() }.0;
            format!("process_set_failed hresult={raw:#x} last_error={extra:#x}")
        }
    };
    // SAFETY: changes only this thread. A null return means the set did not take effect.
    let previous = unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    if previous.0.is_null() {
        return Err(format!("SetThreadDpiAwarenessContext failed; {process_note}"));
    }
    let guard = ThreadDpi(previous.0);
    // SAFETY: reads this thread's context after the set above.
    let current = unsafe { GetThreadDpiAwarenessContext() };
    let label = dpi_label(current);
    if label != "PER_MONITOR_AWARE_V2" {
        return Err(format!("thread DPI is {label}; {process_note}"));
    }
    Ok((format!("{process_note}; thread={label}"), guard))
}

fn virtual_desktop() -> Result<Metrics, String> {
    // SAFETY: scalar metric queries. No output buffer.
    let metrics = unsafe {
        Metrics {
            origin_x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            origin_y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            width: GetSystemMetrics(SM_CXVIRTUALSCREEN),
            height: GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    };
    if metrics.width <= 1 || metrics.height <= 1 {
        return Err(format!(
            "virtual desktop is not usable: {}x{}",
            metrics.width, metrics.height
        ));
    }
    Ok(metrics)
}

unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut windows::Win32::Foundation::RECT,
    data: LPARAM,
) -> windows::core::BOOL {
    if data.0 == 0 {
        return windows::core::BOOL(1);
    }
    // SAFETY: `data` is the Vec<Monitor> `monitors` passes and that outlives this call.
    let list = unsafe { &mut *(data.0 as *mut Vec<Monitor>) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..MONITORINFO::default()
    };
    // SAFETY: writable MONITORINFO with cbSize set. Read-only monitor query.
    if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        list.push(Monitor {
            left: info.rcMonitor.left,
            top: info.rcMonitor.top,
            right: info.rcMonitor.right,
            bottom: info.rcMonitor.bottom,
            primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
        });
    }
    windows::core::BOOL(1)
}

fn monitors() -> Result<Vec<Monitor>, String> {
    let mut list = Vec::new();
    // SAFETY: the callback only appends to `list`, which outlives the enumeration.
    let ok = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect_monitor),
            LPARAM(&mut list as *mut Vec<Monitor> as isize),
        )
    };
    if !ok.as_bool() {
        return Err("EnumDisplayMonitors failed".into());
    }
    if list.is_empty() {
        return Err("EnumDisplayMonitors returned no monitors".into());
    }
    Ok(list)
}

fn push_unique(set: &mut Vec<i32>, pixel: i32, origin: i32, extent: i32) {
    if on_axis(pixel, origin, extent) && !set.contains(&pixel) {
        set.push(pixel);
    }
}

fn axis_points(origin: i32, extent: i32, edges: &[i32]) -> Vec<i32> {
    let mut set = Vec::new();
    for offset in disagreement_offsets(extent) {
        let pixel = origin + offset;
        for delta in -1..=1 {
            push_unique(&mut set, pixel + delta, origin, extent);
        }
    }
    for delta in 0..=2 {
        push_unique(&mut set, origin + delta, origin, extent);
        push_unique(&mut set, origin + extent - 1 - delta, origin, extent);
    }
    for edge in edges {
        for delta in -2..=2 {
            push_unique(&mut set, edge + delta, origin, extent);
        }
    }
    set
}

fn interior(origin: i32, extent: i32, rng: &mut Lcg) -> i32 {
    let span = u64::from(extent.max(1) as u32);
    origin + (rng.next() % span) as i32
}

fn push_point(points: &mut Vec<(i32, i32)>, metrics: &Metrics, x: i32, y: i32) {
    if on_axis(x, metrics.origin_x, metrics.width)
        && on_axis(y, metrics.origin_y, metrics.height)
        && !points.contains(&(x, y))
    {
        points.push((x, y));
    }
}

fn build_targets(metrics: &Metrics, found: &[Monitor]) -> Result<Vec<(i32, i32)>, String> {
    let mut vertical_edges = Vec::new();
    let mut horizontal_edges = Vec::new();
    for monitor in found {
        vertical_edges.push(monitor.left);
        vertical_edges.push(monitor.right - 1);
        horizontal_edges.push(monitor.top);
        horizontal_edges.push(monitor.bottom - 1);
    }
    let xs = axis_points(metrics.origin_x, metrics.width, &vertical_edges);
    let ys = axis_points(metrics.origin_y, metrics.height, &horizontal_edges);
    let mut points = Vec::new();
    let mid_x = metrics.origin_x + metrics.width / 2;
    let mid_y = metrics.origin_y + metrics.height / 2;
    for x in &xs {
        push_point(&mut points, metrics, *x, mid_y);
    }
    for y in &ys {
        push_point(&mut points, metrics, mid_x, *y);
    }
    let mut rng = Lcg(0x00C0_FFEE);
    let mut spins = 0;
    while points.len() < 400 && spins < 8000 {
        spins += 1;
        let x = interior(metrics.origin_x, metrics.width, &mut rng);
        let y = interior(metrics.origin_y, metrics.height, &mut rng);
        push_point(&mut points, metrics, x, y);
    }
    if points.len() < 400 {
        return Err(format!(
            "only {} in-range targets on {}x{}",
            points.len(),
            metrics.width,
            metrics.height
        ));
    }
    for offset in disagreement_offsets(metrics.width) {
        let pixel = metrics.origin_x + offset;
        if !points.iter().any(|point| point.0 == pixel) {
            return Err(format!("disagreement x {pixel} is not in the sample"));
        }
    }
    for offset in disagreement_offsets(metrics.height) {
        let pixel = metrics.origin_y + offset;
        if !points.iter().any(|point| point.1 == pixel) {
            return Err(format!("disagreement y {pixel} is not in the sample"));
        }
    }
    Ok(points)
}

fn sample_cursor(budget: Duration) -> Vec<(Duration, i32, i32)> {
    let start = Instant::now();
    let mut out = Vec::new();
    let mut index = 0u32;
    while index < 100 && start.elapsed() < budget {
        let due = Duration::from_micros(500) * index;
        while start.elapsed() < due {
            std::thread::yield_now();
        }
        if start.elapsed() >= budget {
            break;
        }
        let mut point = windows::Win32::Foundation::POINT::default();
        // SAFETY: writable local POINT. Read-only physical cursor query.
        if unsafe { GetPhysicalCursorPos(&mut point) }.is_ok() {
            out.push((start.elapsed(), point.x, point.y));
        }
        index += 1;
    }
    out
}

fn send_absolute(x: i32, y: i32, metrics: &Metrics) -> Result<(), String> {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: normalize_axis(x, metrics.origin_x, metrics.width),
                dy: normalize_axis(y, metrics.origin_y, metrics.height),
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    // SAFETY: one owned INPUT with exact cbSize. This moves the real cursor.
    let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    if sent == 1 {
        Ok(())
    } else {
        Err(format!("SendInput returned {sent} for ({x},{y})"))
    }
}

fn chebyshev(a: (i32, i32), b: (i32, i32)) -> i32 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs())
}

fn park_corner(target: (i32, i32), metrics: &Metrics) -> Result<(i32, i32), String> {
    let corners = [
        (metrics.origin_x, metrics.origin_y),
        (metrics.origin_x + metrics.width - 1, metrics.origin_y),
        (metrics.origin_x, metrics.origin_y + metrics.height - 1),
        (
            metrics.origin_x + metrics.width - 1,
            metrics.origin_y + metrics.height - 1,
        ),
    ];
    let mut best = corners[0];
    let mut best_d = -1;
    for corner in corners {
        let distance = chebyshev(corner, target);
        if distance > best_d {
            best = corner;
            best_d = distance;
        }
    }
    if best_d <= 200 {
        return Err(format!(
            "desktop {}x{} cannot park >200px from {target:?} (best {best_d})",
            metrics.width, metrics.height
        ));
    }
    Ok(best)
}

fn park(target: (i32, i32), metrics: &Metrics) -> Result<(i32, i32), String> {
    let corner = park_corner(target, metrics)?;
    send_absolute(corner.0, corner.1, metrics)?;
    let samples = sample_cursor(Duration::from_millis(50));
    let Some((_, x, y)) = samples.last().copied() else {
        return Err(format!("park to {corner:?} produced no cursor read"));
    };
    let landed = (x, y);
    if chebyshev(landed, target) <= 200 {
        return Err(format!(
            "park landed at {landed:?}, not >200px from target {target:?}"
        ));
    }
    Ok(landed)
}

fn stable_landing(samples: &[(Duration, i32, i32)], park_at: (i32, i32)) -> Option<(Duration, i32, i32)> {
    for pair in samples.windows(2) {
        let (t0, x0, y0) = pair[0];
        let (_, x1, y1) = pair[1];
        if (x0, y0) != park_at && (x0, y0) == (x1, y1) {
            return Some((t0, x0, y0));
        }
    }
    None
}

fn ms(duration: Duration) -> String {
    format!("{:.3}", duration.as_secs_f64() * 1000.0)
}

fn reference_disagreements() -> String {
    let mut parts = Vec::new();
    for extent in [1920, 2560, 3840, 7680] {
        let offsets = disagreement_offsets(extent);
        parts.push(format!("{extent} ({}): {offsets:?}", offsets.len()));
    }
    parts.join("; ")
}

/// At least 400 physical pixels, including every live-extent quantization disagreement.
/// Returns `Err` unless every planned move was sent and read, so an empty histogram cannot pass.
pub fn landing_histogram() -> Result<LandingReport, String> {
    let (dpi, _thread) = enter_pmv2()?;
    let metrics = virtual_desktop()?;
    let found = monitors()?;
    let targets = build_targets(&metrics, &found)?;
    let planned = targets.len() as u32;
    let mut requested_dx = [0u32; 17];
    let mut requested_dy = [0u32; 17];
    let mut inverse_dx = [0u32; 17];
    let mut inverse_dy = [0u32; 17];
    let mut sent_ok = 0u32;
    let mut send_fail = 0u32;
    let mut unreadable = 0u32;
    let mut max_requested = 0i32;
    let mut max_inverse = 0i32;
    let mut stable_us = Vec::new();
    let mut unstable = 0u32;
    for (x, y) in &targets {
        let target = (*x, *y);
        let parked = park(target, &metrics)?;
        if send_absolute(target.0, target.1, &metrics).is_err() {
            send_fail += 1;
            continue;
        }
        sent_ok += 1;
        let samples = sample_cursor(Duration::from_millis(50));
        if samples.is_empty() {
            unreadable += 1;
            continue;
        }
        let (_when, lx, ly) = if let Some(stable) = stable_landing(&samples, parked) {
            stable_us.push(stable.0.as_micros());
            stable
        } else {
            unstable += 1;
            samples[samples.len() - 1]
        };
        let ix = denormalize_axis(
            normalize_axis(target.0, metrics.origin_x, metrics.width),
            metrics.origin_x,
            metrics.width,
        );
        let iy = denormalize_axis(
            normalize_axis(target.1, metrics.origin_y, metrics.height),
            metrics.origin_y,
            metrics.height,
        );
        bump(&mut requested_dx, lx - target.0);
        bump(&mut requested_dy, ly - target.1);
        bump(&mut inverse_dx, lx - ix);
        bump(&mut inverse_dy, ly - iy);
        max_requested = max_requested.max((lx - target.0).abs()).max((ly - target.1).abs());
        max_inverse = max_inverse.max((lx - ix).abs()).max((ly - iy).abs());
    }
    let valid_reads = sent_ok.saturating_sub(unreadable);
    stable_us.sort_unstable();
    let stable_summary = if stable_us.is_empty() {
        "stable_ms min=none p50=none max=none".to_string()
    } else {
        let at = |index: usize| {
            Duration::from_micros(u64::try_from(stable_us[index]).unwrap_or(u64::MAX))
        };
        format!(
            "stable_ms min={} p50={} max={}",
            ms(at(0)),
            ms(at(stable_us.len() / 2)),
            ms(at(stable_us.len() - 1))
        )
    };
    let mut topology = String::new();
    for (index, monitor) in found.iter().enumerate() {
        topology.push_str(&format!(
            "monitor[{index}] primary={} rc=({},{},{},{})\n",
            monitor.primary, monitor.left, monitor.top, monitor.right, monitor.bottom
        ));
    }
    let origin_x = metrics.origin_x;
    let origin_y = metrics.origin_y;
    let width = metrics.width;
    let height = metrics.height;
    let reference = reference_disagreements();
    let live_x = disagreement_offsets(width);
    let live_y = disagreement_offsets(height);
    let req_dx = format_hist(&requested_dx);
    let req_dy = format_hist(&requested_dy);
    let inv_dx = format_hist(&inverse_dx);
    let inv_dy = format_hist(&inverse_dy);
    let text = format!(
        "landing probe planned={planned} sent_ok={sent_ok} send_fail={send_fail} unreadable={unreadable} valid_reads={valid_reads} unstable={unstable}\n{dpi}\nvirtual=({origin_x},{origin_y},{width},{height})\n{topology}reference disagreements {reference}\nlive disagreement x={live_x:?}\nlive disagreement y={live_y:?}\n{stable_summary}\nvs_requested dx [{req_dx}]\nvs_requested dy [{req_dy}]\nvs_inverse dx [{inv_dx}]\nvs_inverse dy [{inv_dy}]\nmax_abs requested={max_requested} inverse={max_inverse}"
    );
    let report = LandingReport {
        planned,
        sent_ok,
        valid_reads,
        text: text.clone(),
    };
    if sent_ok != planned || valid_reads != planned {
        return Err(text);
    }
    Ok(report)
}

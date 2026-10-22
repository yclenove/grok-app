// Ignored live measurement of where `MOVE|ABSOLUTE|VIRTUALDESK` actually lands.
// Included from `unsafe_click_probe.rs`. Target selection and tail stability live in
// `landing_plan` so they can be tested without a desktop. `GetSystemMetrics`,
// `GetPhysicalCursorPos`, and `MONITORINFOF_PRIMARY` come from that module's glob import.
use std::time::{Duration, Instant};
use super::super::landing_plan::{
    self, CursorSample, Desktop, Inconclusive, MonitorRect, MoveTrace,
};
use windows::Win32::{
    Foundation::LPARAM,
    Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO},
    UI::{
        HiDpi::{
            AreDpiAwarenessContextsEqual, GetDpiForMonitor, GetThreadDpiAwarenessContext,
            SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE,
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, DPI_AWARENESS_CONTEXT_SYSTEM_AWARE,
            DPI_AWARENESS_CONTEXT_UNAWARE, DPI_AWARENESS_CONTEXT_UNAWARE_GDISCALED, MDT_EFFECTIVE_DPI,
        },
        Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_MOVE,
            MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT,
        },
    },
};

const OBSERVE: Duration = Duration::from_millis(50);

/// Counts the caller asserts. `text` is the histogram, including when this returns `Err`.
pub struct LandingReport {
    pub planned: u32,
    pub sent_ok: u32,
    pub conclusive: u32,
    pub screen_small: bool,
    pub text: String,
}
impl LandingReport {
    /// Conclusive landings meet the 95% bar and the 400-target floor (or the actual count).
    pub fn accepted(&self) -> bool {
        landing_plan::acceptance_met(self.planned, self.conclusive, self.screen_small)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LiveMonitor {
    rect: MonitorRect,
    primary: bool,
    dpi_x: u32,
    dpi_y: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Topology {
    desktop: Desktop,
    monitors: Vec<LiveMonitor>,
}

enum ProbeFail {
    Topology(String),
    Other(String),
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
    // Thread context only, matching `DpiContext::new` on the production worker.
    // Do not call `SetProcessDpiAwarenessContext`: that changes the process default
    // for later threads, and the worker does not do it.
    // SAFETY: changes only this thread. A null return means the set did not take effect.
    let previous = unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    if previous.0.is_null() {
        return Err("SetThreadDpiAwarenessContext failed; process DPI was not changed".into());
    }
    let guard = ThreadDpi(previous.0);
    // SAFETY: reads this thread's context after the set above.
    let current = unsafe { GetThreadDpiAwarenessContext() };
    let label = dpi_label(current);
    if label != "PER_MONITOR_AWARE_V2" {
        return Err(format!("thread DPI is {label}; process DPI was not changed"));
    }
    Ok((format!("thread={label}; process_dpi_unchanged"), guard))
}

fn virtual_desktop() -> Result<Desktop, String> {
    // SAFETY: scalar metric queries. No output buffer.
    let desktop = unsafe {
        Desktop {
            origin_x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            origin_y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            width: GetSystemMetrics(SM_CXVIRTUALSCREEN),
            height: GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    };
    if desktop.width <= 1 || desktop.height <= 1 {
        return Err(format!(
            "virtual desktop is not usable: {}x{}",
            desktop.width, desktop.height
        ));
    }
    Ok(desktop)
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
    // SAFETY: `data` is the Vec<LiveMonitor> `monitors` passes and that outlives this call.
    let list = unsafe { &mut *(data.0 as *mut Vec<LiveMonitor>) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..MONITORINFO::default()
    };
    // SAFETY: writable MONITORINFO with cbSize set. Read-only monitor query.
    if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        let mut dpi_x = 0u32;
        let mut dpi_y = 0u32;
        // SAFETY: writable dpi slots. The monitor handle is valid for this callback.
        let dpi_ok = unsafe {
            GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_ok()
        };
        if !dpi_ok {
            dpi_x = 0;
            dpi_y = 0;
        }
        list.push(LiveMonitor {
            rect: MonitorRect {
                left: info.rcMonitor.left,
                top: info.rcMonitor.top,
                right: info.rcMonitor.right,
                bottom: info.rcMonitor.bottom,
            },
            primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
            dpi_x,
            dpi_y,
        });
    }
    windows::core::BOOL(1)
}

fn monitors() -> Result<Vec<LiveMonitor>, String> {
    let mut list: Vec<LiveMonitor> = Vec::new();
    // SAFETY: the callback only appends to `list`, which outlives the enumeration.
    let ok = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect_monitor),
            LPARAM(&mut list as *mut Vec<LiveMonitor> as isize),
        )
    };
    if !ok.as_bool() {
        return Err("EnumDisplayMonitors failed".into());
    }
    if list.is_empty() {
        return Err("EnumDisplayMonitors returned no monitors".into());
    }
    if list.iter().any(|monitor| monitor.dpi_x == 0 || monitor.dpi_y == 0) {
        return Err("GetDpiForMonitor failed".into());
    }
    list.sort_by_key(|monitor| {
        (
            monitor.rect.left,
            monitor.rect.top,
            monitor.rect.right,
            monitor.rect.bottom,
            monitor.dpi_x,
            monitor.dpi_y,
        )
    });
    Ok(list)
}

fn capture_topology() -> Result<Topology, String> {
    Ok(Topology {
        desktop: virtual_desktop()?,
        monitors: monitors()?,
    })
}

fn ensure(baseline: &Topology) -> Result<(), ProbeFail> {
    match capture_topology() {
        Ok(now) if now == *baseline => Ok(()),
        Ok(now) => Err(ProbeFail::Topology(format!("before {baseline:?}\nafter {now:?}"))),
        Err(message) => Err(ProbeFail::Other(message)),
    }
}

fn sample_cursor(budget: Duration) -> Vec<CursorSample> {
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
        let pos = if unsafe { GetPhysicalCursorPos(&mut point) }.is_ok() {
            Some((point.x, point.y))
        } else {
            None
        };
        out.push(CursorSample {
            at: start.elapsed(),
            pos,
        });
        index += 1;
    }
    out
}

fn send_absolute(x: i32, y: i32, desktop: &Desktop) -> Result<(), String> {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: landing_plan::normalize_axis(x, desktop.origin_x, desktop.width),
                dy: landing_plan::normalize_axis(y, desktop.origin_y, desktop.height),
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    // SAFETY: one owned INPUT. This moves the real cursor.
    let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    if sent == 1 {
        Ok(())
    } else {
        Err(format!("SendInput returned {sent} for ({x},{y})"))
    }
}

fn far_enough(a: (i32, i32), b: (i32, i32)) -> bool {
    a.0.abs_diff(b.0).max(a.1.abs_diff(b.1)) > 200
}

fn park(target: (i32, i32), topology: &Topology) -> Result<(i32, i32), String> {
    let rects: Vec<MonitorRect> = topology.monitors.iter().map(|monitor| monitor.rect).collect();
    let corner = landing_plan::park_corner(target, &rects)?;
    send_absolute(corner.0, corner.1, &topology.desktop)?;
    let samples = sample_cursor(OBSERVE);
    let Some(landed) = samples.last().and_then(|sample| sample.pos) else {
        return Err(format!("park to {corner:?} produced no final cursor read"));
    };
    if !far_enough(landed, target) {
        return Err(format!(
            "park landed at {landed:?}, not >200px from target {target:?}"
        ));
    }
    Ok(landed)
}

fn counts(traces: &[MoveTrace]) -> (u32, u32, u32, u32) {
    let mut conclusive = 0u32;
    let mut no_departure = 0u32;
    let mut unstable = 0u32;
    let mut incomplete = 0u32;
    for trace in traces {
        match trace.verdict {
            Ok(()) => conclusive += 1,
            Err(Inconclusive::NoDeparture) => no_departure += 1,
            Err(Inconclusive::Unstable) => unstable += 1,
            Err(Inconclusive::Incomplete) => incomplete += 1,
        }
    }
    (conclusive, no_departure, unstable, incomplete)
}

fn duration_summary(label: &str, samples: &mut [Duration]) -> String {
    if samples.is_empty() {
        return format!("{label} min=none p50=none max=none");
    }
    samples.sort_unstable();
    let at = |index: usize| samples[index];
    format!(
        "{label} min={} p50={} max={}",
        landing_ms(at(0)),
        landing_ms(at(samples.len() / 2)),
        landing_ms(at(samples.len() - 1))
    )
}

fn landing_ms(duration: Duration) -> String {
    format!("{:.3}", duration.as_secs_f64() * 1000.0)
}

fn reference_disagreements() -> String {
    let mut parts = Vec::new();
    for extent in [1920, 2560, 3840, 7680] {
        let offsets = landing_plan::disagreement_offsets(extent);
        parts.push(format!("{extent} ({}): {offsets:?}", offsets.len()));
    }
    parts.join("; ")
}

fn unreachable(desktop: &Desktop, monitors: &[LiveMonitor]) -> (Vec<i32>, Vec<i32>) {
    let rects: Vec<MonitorRect> = monitors.iter().map(|monitor| monitor.rect).collect();
    let x = landing_plan::disagreement_offsets(desktop.width)
        .into_iter()
        .map(|offset| desktop.origin_x + offset)
        .filter(|pixel| {
            !rects.iter().any(|rect| {
                rect.bottom > rect.top && *pixel >= rect.left && *pixel < rect.right
            })
        })
        .collect();
    let y = landing_plan::disagreement_offsets(desktop.height)
        .into_iter()
        .map(|offset| desktop.origin_y + offset)
        .filter(|pixel| {
            !rects
                .iter()
                .any(|rect| rect.right > rect.left && *pixel >= rect.top && *pixel < rect.bottom)
        })
        .collect();
    (x, y)
}

fn report_text(
    dpi: &str,
    topology: &Topology,
    plan_small: bool,
    planned: u32,
    sent_ok: u32,
    send_fail: u32,
    traces: &[MoveTrace],
) -> (String, u32) {
    let (conclusive, no_departure, unstable, incomplete) = counts(traces);
    let hist = landing_plan::histograms(traces, &topology.desktop);
    let mut arrivals = Vec::new();
    for trace in traces {
        if let Some((at, _, _)) = trace.first_within_budget {
            arrivals.push(at);
        }
    }
    let mut topology_text = String::new();
    for (index, monitor) in topology.monitors.iter().enumerate() {
        topology_text.push_str(&format!(
            "monitor[{index}] primary={} dpi={}x{} rc=({},{},{},{})\n",
            monitor.primary,
            monitor.dpi_x,
            monitor.dpi_y,
            monitor.rect.left,
            monitor.rect.top,
            monitor.rect.right,
            monitor.rect.bottom
        ));
    }
    let (gap_x, gap_y) = unreachable(&topology.desktop, &topology.monitors);
    let live_x = landing_plan::disagreement_offsets(topology.desktop.width);
    let live_y = landing_plan::disagreement_offsets(topology.desktop.height);
    let ranked = landing_plan::largest_error_indices(traces, &topology.desktop, 20);
    let mut top = String::new();
    for (rank, index) in ranked.iter().enumerate() {
        top.push_str(&format!(
            "top[{rank}] {}\n",
            landing_plan::format_trace(&traces[*index])
        ));
    }
    let desktop = &topology.desktop;
    let text = format!(
        "landing probe planned={planned} sent_ok={sent_ok} send_fail={send_fail} conclusive={conclusive} inconclusive no_departure={no_departure} unstable={unstable} incomplete={incomplete} screen_small={plan_small}\nacceptance requires conclusive >= 95% of planned and >= 400 (or the actual count when the monitor union is smaller)\n{dpi}\nvirtual=({},{},{},{})\n{topology_text}reference disagreements {}\nlive disagreement x={live_x:?}\nlive disagreement y={live_y:?}\nunreachable disagreement x={gap_x:?}\nunreachable disagreement y={gap_y:?}\n{}\nvs_requested dx [{}]\nvs_requested dy [{}]\nvs_inverse dx [{}]\nvs_inverse dy [{}]\nmax_abs requested={} inverse={}\n{top}",
        desktop.origin_x,
        desktop.origin_y,
        desktop.width,
        desktop.height,
        reference_disagreements(),
        duration_summary("first_15ms", &mut arrivals),
        landing_plan::format_hist(&hist.requested_dx),
        landing_plan::format_hist(&hist.requested_dy),
        landing_plan::format_hist(&hist.inverse_dx),
        landing_plan::format_hist(&hist.inverse_dy),
        hist.max_requested,
        hist.max_inverse,
    );
    (text, conclusive)
}

fn collect(dpi: &str) -> Result<LandingReport, ProbeFail> {
    let baseline = capture_topology().map_err(ProbeFail::Other)?;
    let rects: Vec<MonitorRect> = baseline.monitors.iter().map(|monitor| monitor.rect).collect();
    let plan = landing_plan::build_targets(&baseline.desktop, &rects).map_err(ProbeFail::Other)?;
    let planned = u32::try_from(plan.points.len()).unwrap_or(u32::MAX);
    let mut traces = Vec::new();
    let mut sent_ok = 0u32;
    let mut send_fail = 0u32;
    let probe_start = Instant::now();
    for (index, target) in plan.points.iter().copied().enumerate() {
        if let Err(error) = ensure(&baseline) {
            return Err(tag_move(error, index));
        }
        let parked = park(target, &baseline).map_err(ProbeFail::Other)?;
        if let Err(error) = ensure(&baseline) {
            return Err(tag_move(error, index));
        }
        let normalized = (
            landing_plan::normalize_axis(target.0, baseline.desktop.origin_x, baseline.desktop.width),
            landing_plan::normalize_axis(target.1, baseline.desktop.origin_y, baseline.desktop.height),
        );
        let origin = probe_start.elapsed();
        if send_absolute(target.0, target.1, &baseline.desktop).is_err() {
            send_fail += 1;
            continue;
        }
        sent_ok += 1;
        let samples = sample_cursor(OBSERVE);
        traces.push(MoveTrace::capture(
            target, normalized, parked, origin, samples,
        ));
        if let Err(error) = ensure(&baseline) {
            return Err(tag_move(error, index));
        }
    }
    let (text, conclusive) = report_text(
        dpi,
        &baseline,
        plan.screen_small,
        planned,
        sent_ok,
        send_fail,
        &traces,
    );
    Ok(LandingReport {
        planned,
        sent_ok,
        conclusive,
        screen_small: plan.screen_small,
        text,
    })
}

fn tag_move(error: ProbeFail, index: usize) -> ProbeFail {
    match error {
        ProbeFail::Topology(message) => {
            ProbeFail::Topology(format!("after move {index}\n{message}"))
        }
        other => other,
    }
}

/// At least 400 on-monitor pixels when the union is that large, including every reachable
/// quantization disagreement. Inconclusive traces are printed and excluded from the histogram.
/// Returns `Err` unless the send count matches the plan and the conclusive count meets the bar.
pub fn landing_histogram() -> Result<LandingReport, String> {
    let (dpi, _thread) = enter_pmv2()?;
    let report = match collect(&dpi) {
        Ok(report) => report,
        Err(ProbeFail::Other(message)) => return Err(message),
        Err(ProbeFail::Topology(first)) => match collect(&dpi) {
            Ok(report) => report,
            Err(ProbeFail::Topology(second)) => {
                return Err(format!(
                    "display topology changed during collection; rerun\n{first}\n{second}"
                ));
            }
            Err(ProbeFail::Other(message)) => return Err(message),
        },
    };
    if report.sent_ok != report.planned || !report.accepted() {
        return Err(report.text);
    }
    Ok(report)
}

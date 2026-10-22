//! Pure landing-probe plan: monitor-union targets and tail stability.
//!
//! The live sampler includes this module's decisions. It does not call Win32.
//! Gap pixels are not targets. A gap study needs its own labeled set and must not
//! be passed to [`histograms`].
use std::time::Duration;

/// Production cursor-settle budget (`native::CURSOR_SETTLE`). Not a tolerance.
pub const ARRIVAL_BUDGET: Duration = Duration::from_millis(15);
/// Tail length that must agree with the last sample. Two samples is the end of the
/// observation, not the first equal pair anywhere in the trace.
pub const TAIL_SAMPLES: usize = 2;
/// Acceptance floor when the monitor union has at least this many pixels.
pub const SAMPLE_FLOOR: u32 = 400;
const CONCLUSIVE_PERCENT: u32 = 95;
const FILL_SPINS: u32 = 8_000;

/// Half-open physical monitor rectangle, matching `rcMonitor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonitorRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Virtual desktop used only to normalize `MOUSEEVENTF_VIRTUALDESK`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Desktop {
    pub origin_x: i32,
    pub origin_y: i32,
    pub width: i32,
    pub height: i32,
}

/// One cursor read. `None` is a failed read, kept so the tail stays aligned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorSample {
    pub at: Duration,
    pub pos: Option<(i32, i32)>,
}

/// Why a sent move is not a measured landing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inconclusive {
    /// Every readable sample stayed at the park position.
    NoDeparture,
    /// The cursor left park, but the observation tail does not agree.
    Unstable,
    /// The tail could not be read.
    Incomplete,
}

/// One planned move and everything needed to judge it later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveTrace {
    pub requested: (i32, i32),
    pub normalized: (i32, i32),
    pub park: (i32, i32),
    /// Send instant, as an offset from the start of the probe run.
    pub origin: Duration,
    pub samples: Vec<CursorSample>,
    pub terminal: Option<(i32, i32)>,
    /// First sample that left park inside [`ARRIVAL_BUDGET`]. Not the stable landing.
    pub first_within_budget: Option<(Duration, i32, i32)>,
    /// Tail position when the trace is conclusive.
    pub stable: Option<(i32, i32)>,
    pub verdict: Result<(), Inconclusive>,
}

/// Targets selected from the monitor union.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetPlan {
    pub points: Vec<(i32, i32)>,
    /// The union has fewer than [`SAMPLE_FLOOR`] pixels, so the floor is `points.len()`.
    pub screen_small: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Histograms {
    pub requested_dx: [u32; 17],
    pub requested_dy: [u32; 17],
    pub inverse_dx: [u32; 17],
    pub inverse_dy: [u32; 17],
    pub max_requested: i32,
    pub max_inverse: i32,
    pub conclusive: u32,
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.0
    }
}

/// Floor model matching `pointer_settle::normalize_axis` for an integer pixel.
/// `P` is clamped to `[0, extent - 1]`; `norm = ceil(P * 65536 / extent)`.
pub fn normalize_axis(pixel: i32, origin: i32, extent: i32) -> i32 {
    if extent <= 1 {
        return 0;
    }
    let offset = i64::from(pixel) - i64::from(origin);
    let p = u64::try_from(offset.clamp(0, i64::from(extent) - 1)).unwrap_or(0);
    let extent = u64::try_from(extent).unwrap_or(1);
    let norm = (p * 65536).div_ceil(extent);
    i32::try_from(norm.min(65535)).unwrap_or(0)
}

/// `origin + floor(norm * extent / 65536)`, matching `pointer_settle::denormalize_axis`.
pub fn denormalize_axis(norm: i32, origin: i32, extent: i32) -> i32 {
    if extent <= 0 {
        return origin;
    }
    let norm = i64::from(norm).clamp(0, 65535);
    let landed = i64::from(origin) + norm * i64::from(extent) / 65536;
    i32::try_from(landed).unwrap_or(if landed > 0 { i32::MAX } else { i32::MIN })
}

/// Retired round model, `round(offset * 65535 / (extent - 1))`.
fn legacy_normalize_axis(pixel: i32, origin: i32, extent: i32) -> i32 {
    ((f64::from(pixel) - f64::from(origin)) * 65535. / f64::from(extent - 1)).round() as i32
}

fn legacy_denormalize_axis(norm: i32, origin: i32, extent: i32) -> i32 {
    origin + (f64::from(norm) * f64::from(extent - 1) / 65535.).round() as i32
}

/// Offsets where the retired round model and the floor model do not name the same pixel.
/// Same-bin norm choices are not included: Windows lands on that pixel either way.
pub fn disagreement_offsets(extent: i32) -> Vec<i32> {
    if extent <= 1 {
        return Vec::new();
    }
    (0..extent)
        .filter(|offset| {
            let legacy_norm = legacy_normalize_axis(*offset, 0, extent);
            let floor_norm = normalize_axis(*offset, 0, extent);
            let legacy_pixel = legacy_denormalize_axis(legacy_norm, 0, extent);
            let floor_pixel = denormalize_axis(floor_norm, 0, extent);
            let legacy_windows = denormalize_axis(legacy_norm, 0, extent);
            legacy_pixel != floor_pixel || legacy_windows != floor_pixel
        })
        .collect()
}

/// Conclusive traces whose stable landing is not the requested pixel, in sample order.
pub fn requested_miss_indices(traces: &[MoveTrace], limit: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for (index, trace) in traces.iter().enumerate() {
        if out.len() == limit {
            break;
        }
        if trace.verdict.is_err() {
            continue;
        }
        let Some((x, y)) = trace.stable else {
            continue;
        };
        if x != trace.requested.0 || y != trace.requested.1 {
            out.push(index);
        }
    }
    out
}

pub fn on_monitor(monitors: &[MonitorRect], x: i32, y: i32) -> bool {
    monitors.iter().any(|monitor| {
        monitor.right > monitor.left
            && monitor.bottom > monitor.top
            && x >= monitor.left
            && x < monitor.right
            && y >= monitor.top
            && y < monitor.bottom
    })
}

/// Pixel count of the union. Overlaps are counted once.
pub fn union_pixels(monitors: &[MonitorRect]) -> u64 {
    let mut ys: Vec<i32> = monitors
        .iter()
        .filter(|monitor| monitor.right > monitor.left && monitor.bottom > monitor.top)
        .flat_map(|monitor| [monitor.top, monitor.bottom])
        .collect();
    ys.sort_unstable();
    ys.dedup();
    let mut area = 0u64;
    for pair in ys.windows(2) {
        let (y0, y1) = (pair[0], pair[1]);
        if y1 <= y0 {
            continue;
        }
        let mut spans: Vec<(i32, i32)> = monitors
            .iter()
            .filter(|monitor| {
                monitor.top <= y0 && monitor.bottom >= y1 && monitor.right > monitor.left
            })
            .map(|monitor| (monitor.left, monitor.right))
            .collect();
        spans.sort_unstable();
        let mut covered = 0i64;
        let mut open = false;
        let mut end = 0i32;
        for (left, right) in spans {
            if !open {
                open = true;
                end = right;
                covered += i64::from(right - left);
                continue;
            }
            if left >= end {
                covered += i64::from(right - left);
                end = right;
            } else if right > end {
                covered += i64::from(right - end);
                end = right;
            }
        }
        if covered > 0 {
            area += covered as u64 * u64::from((y1 - y0) as u32);
        }
    }
    area
}

fn usable(monitors: &[MonitorRect]) -> Vec<MonitorRect> {
    let mut out: Vec<_> = monitors
        .iter()
        .copied()
        .filter(|monitor| monitor.right > monitor.left && monitor.bottom > monitor.top)
        .collect();
    out.sort_by_key(|monitor| (monitor.left, monitor.top, monitor.right, monitor.bottom));
    out
}

fn covers_x(monitors: &[MonitorRect], x: i32) -> bool {
    monitors
        .iter()
        .any(|monitor| monitor.bottom > monitor.top && x >= monitor.left && x < monitor.right)
}
fn covers_y(monitors: &[MonitorRect], y: i32) -> bool {
    monitors
        .iter()
        .any(|monitor| monitor.right > monitor.left && y >= monitor.top && y < monitor.bottom)
}

/// Other-axis pixel on a monitor that contains `fixed`. Prefers `prefer` when that point is on a monitor.
fn partner(monitors: &[MonitorRect], fixed: i32, prefer: i32, fixed_is_x: bool) -> Option<i32> {
    let contains = |monitor: &MonitorRect, other: i32| {
        if fixed_is_x {
            on_monitor(monitors, fixed, other) && fixed >= monitor.left && fixed < monitor.right
        } else {
            on_monitor(monitors, other, fixed) && fixed >= monitor.top && fixed < monitor.bottom
        }
    };
    if monitors.iter().any(|monitor| contains(monitor, prefer)) {
        return Some(prefer);
    }
    let mut best: Option<(i32, i32)> = None;
    for monitor in monitors {
        let (start, end) = if fixed_is_x {
            if fixed < monitor.left || fixed >= monitor.right {
                continue;
            }
            (monitor.top, monitor.bottom)
        } else {
            if fixed < monitor.top || fixed >= monitor.bottom {
                continue;
            }
            (monitor.left, monitor.right)
        };
        if end <= start {
            continue;
        }
        let mid = start + (end - start) / 2;
        let other = mid.clamp(start, end - 1);
        if !contains(monitor, other) {
            continue;
        }
        let distance = (other - prefer).abs();
        match best {
            Some((current, _)) if current <= distance => {}
            _ => best = Some((distance, other)),
        }
    }
    best.map(|(_, other)| other)
}

fn push_unique(points: &mut Vec<(i32, i32)>, point: (i32, i32), monitors: &[MonitorRect]) {
    if on_monitor(monitors, point.0, point.1) && !points.contains(&point) {
        points.push(point);
    }
}

fn on_axis(pixel: i32, origin: i32, extent: i32) -> bool {
    extent > 0 && pixel >= origin && pixel < origin.saturating_add(extent)
}

fn axis_candidates(origin: i32, extent: i32, edges: &[i32]) -> Vec<i32> {
    let mut set = Vec::new();
    let mut push = |pixel: i32| {
        if on_axis(pixel, origin, extent) && !set.contains(&pixel) {
            set.push(pixel);
        }
    };
    for offset in disagreement_offsets(extent) {
        for delta in -1..=1 {
            push(origin + offset + delta);
        }
    }
    for delta in 0..=2 {
        push(origin + delta);
        push(origin + extent - 1 - delta);
    }
    for edge in edges {
        for delta in -2..=2 {
            push(edge + delta);
        }
    }
    set
}

/// At least [`SAMPLE_FLOOR`] points on the monitor union, including every reachable
/// quantization disagreement. A smaller union returns every point the fill can find
/// and sets [`TargetPlan::screen_small`].
pub fn build_targets(desktop: &Desktop, monitors: &[MonitorRect]) -> Result<TargetPlan, String> {
    if desktop.width <= 1 || desktop.height <= 1 {
        return Err(format!(
            "virtual desktop is not usable: {}x{}",
            desktop.width, desktop.height
        ));
    }
    let monitors = usable(monitors);
    if monitors.is_empty() {
        return Err("no usable monitors".into());
    }
    let screen_small = union_pixels(&monitors) < u64::from(SAMPLE_FLOOR);
    let mut vertical_edges = Vec::new();
    let mut horizontal_edges = Vec::new();
    for monitor in &monitors {
        vertical_edges.push(monitor.left);
        vertical_edges.push(monitor.right - 1);
        horizontal_edges.push(monitor.top);
        horizontal_edges.push(monitor.bottom - 1);
    }
    let xs = axis_candidates(desktop.origin_x, desktop.width, &vertical_edges);
    let ys = axis_candidates(desktop.origin_y, desktop.height, &horizontal_edges);
    let prefer_y = desktop.origin_y + desktop.height / 2;
    let prefer_x = desktop.origin_x + desktop.width / 2;
    let mut points = Vec::new();
    for x in xs {
        if let Some(y) = partner(&monitors, x, prefer_y, true) {
            push_unique(&mut points, (x, y), &monitors);
        }
    }
    for y in ys {
        if let Some(x) = partner(&monitors, y, prefer_x, false) {
            push_unique(&mut points, (x, y), &monitors);
        }
    }
    let mut rng = Lcg(0x00C0_FFEE);
    let mut spins = 0;
    while points.len() < usize::try_from(SAMPLE_FLOOR).unwrap_or(usize::MAX) && spins < FILL_SPINS {
        spins += 1;
        let monitor = &monitors[usize::try_from(rng.next()).unwrap_or(0) % monitors.len()];
        let width = u64::from((monitor.right - monitor.left) as u32);
        let height = u64::from((monitor.bottom - monitor.top) as u32);
        let x = monitor.left + u64_to_i32(rng.next() % width);
        let y = monitor.top + u64_to_i32(rng.next() % height);
        push_unique(&mut points, (x, y), &monitors);
    }
    if points.is_empty() {
        return Err("no on-monitor targets".into());
    }
    if !screen_small && points.len() < usize::try_from(SAMPLE_FLOOR).unwrap_or(usize::MAX) {
        return Err(format!(
            "only {} on-monitor targets; need {SAMPLE_FLOOR}",
            points.len()
        ));
    }
    for offset in disagreement_offsets(desktop.width) {
        let pixel = desktop.origin_x + offset;
        if covers_x(&monitors, pixel) && !points.iter().any(|point| point.0 == pixel) {
            return Err(format!(
                "reachable disagreement x {pixel} is not in the sample"
            ));
        }
    }
    for offset in disagreement_offsets(desktop.height) {
        let pixel = desktop.origin_y + offset;
        if covers_y(&monitors, pixel) && !points.iter().any(|point| point.1 == pixel) {
            return Err(format!(
                "reachable disagreement y {pixel} is not in the sample"
            ));
        }
    }
    Ok(TargetPlan {
        points,
        screen_small,
    })
}

fn u64_to_i32(value: u64) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// Farthest on-monitor corner whose Chebyshev distance from `target` is greater than 200.
/// Bounding-box corners that lie in a monitor gap are not candidates.
pub fn park_corner(target: (i32, i32), monitors: &[MonitorRect]) -> Result<(i32, i32), String> {
    let monitors = usable(monitors);
    let mut best: Option<((i32, i32), u32)> = None;
    for monitor in &monitors {
        let corners = [
            (monitor.left, monitor.top),
            (monitor.right - 1, monitor.top),
            (monitor.left, monitor.bottom - 1),
            (monitor.right - 1, monitor.bottom - 1),
        ];
        for corner in corners {
            if !on_monitor(&monitors, corner.0, corner.1) {
                continue;
            }
            let distance = target.0.abs_diff(corner.0).max(target.1.abs_diff(corner.1));
            match best {
                Some((_, current)) if current >= distance => {}
                _ => best = Some((corner, distance)),
            }
        }
    }
    match best {
        Some((corner, distance)) if distance > 200 => Ok(corner),
        Some((_, distance)) => Err(format!(
            "no on-monitor park >200px from {target:?} (best {distance})"
        )),
        None => Err("no on-monitor park corner".into()),
    }
}

fn tail_positions(samples: &[CursorSample]) -> Option<Vec<(i32, i32)>> {
    if samples.len() < TAIL_SAMPLES {
        return None;
    }
    let mut out = Vec::with_capacity(TAIL_SAMPLES);
    for sample in &samples[samples.len() - TAIL_SAMPLES..] {
        out.push(sample.pos?);
    }
    Some(out)
}

fn first_within_budget(samples: &[CursorSample], park: (i32, i32)) -> Option<(Duration, i32, i32)> {
    samples.iter().find_map(|sample| {
        let pos = sample.pos?;
        (pos != park && sample.at <= ARRIVAL_BUDGET).then_some((sample.at, pos.0, pos.1))
    })
}

fn classify(
    samples: &[CursorSample],
    park: (i32, i32),
) -> (Result<(), Inconclusive>, Option<(i32, i32)>) {
    let Some(tail) = tail_positions(samples) else {
        return (Err(Inconclusive::Incomplete), None);
    };
    let last = tail[tail.len() - 1];
    if tail.iter().any(|pos| *pos != last) {
        return (Err(Inconclusive::Unstable), None);
    }
    let departed = samples
        .iter()
        .any(|sample| sample.pos.is_some_and(|pos| pos != park));
    if !departed {
        return (Err(Inconclusive::NoDeparture), None);
    }
    (Ok(()), Some(last))
}

impl MoveTrace {
    /// Keep the request, the park, the time origin, and every sample. Classify from the tail.
    pub fn capture(
        requested: (i32, i32),
        normalized: (i32, i32),
        park: (i32, i32),
        origin: Duration,
        samples: Vec<CursorSample>,
    ) -> Self {
        let terminal = samples.iter().rev().find_map(|sample| sample.pos);
        let first_within_budget = first_within_budget(&samples, park);
        let (verdict, stable) = classify(&samples, park);
        Self {
            requested,
            normalized,
            park,
            origin,
            samples,
            terminal,
            first_within_budget,
            stable,
            verdict,
        }
    }

    pub fn conclusive(&self) -> bool {
        self.verdict.is_ok()
    }
}

pub fn inverse_pixel(requested: (i32, i32), desktop: &Desktop) -> (i32, i32) {
    (
        denormalize_axis(
            normalize_axis(requested.0, desktop.origin_x, desktop.width),
            desktop.origin_x,
            desktop.width,
        ),
        denormalize_axis(
            normalize_axis(requested.1, desktop.origin_y, desktop.height),
            desktop.origin_y,
            desktop.height,
        ),
    )
}

fn bump(hist: &mut [u32; 17], delta: i32) {
    let index = usize::try_from((delta + 8).clamp(0, 16)).unwrap_or(0);
    hist[index] += 1;
}

/// Error histogram of conclusive landings only. Nonzero deltas stay in their bins.
pub fn histograms(traces: &[MoveTrace], desktop: &Desktop) -> Histograms {
    let mut out = Histograms {
        requested_dx: [0; 17],
        requested_dy: [0; 17],
        inverse_dx: [0; 17],
        inverse_dy: [0; 17],
        max_requested: 0,
        max_inverse: 0,
        conclusive: 0,
    };
    for trace in traces {
        let Some((landed_x, landed_y)) = trace.stable else {
            continue;
        };
        if trace.verdict.is_err() {
            continue;
        }
        let (inverse_x, inverse_y) = inverse_pixel(trace.requested, desktop);
        let requested_dx = landed_x - trace.requested.0;
        let requested_dy = landed_y - trace.requested.1;
        let inverse_dx = landed_x - inverse_x;
        let inverse_dy = landed_y - inverse_y;
        bump(&mut out.requested_dx, requested_dx);
        bump(&mut out.requested_dy, requested_dy);
        bump(&mut out.inverse_dx, inverse_dx);
        bump(&mut out.inverse_dy, inverse_dy);
        out.max_requested = out
            .max_requested
            .max(requested_dx.abs())
            .max(requested_dy.abs());
        out.max_inverse = out.max_inverse.max(inverse_dx.abs()).max(inverse_dy.abs());
        out.conclusive += 1;
    }
    out
}

fn error_rank(trace: &MoveTrace, desktop: &Desktop) -> Option<(i32, i32)> {
    if trace.verdict.is_err() {
        return None;
    }
    let (landed_x, landed_y) = trace.stable?;
    let (inverse_x, inverse_y) = inverse_pixel(trace.requested, desktop);
    let inverse = (landed_x - inverse_x)
        .abs()
        .max((landed_y - inverse_y).abs());
    let requested = (landed_x - trace.requested.0)
        .abs()
        .max((landed_y - trace.requested.1).abs());
    Some((inverse, requested))
}

/// Largest conclusive inverse errors, then requested errors. Inconclusive traces are omitted.
pub fn largest_error_indices(traces: &[MoveTrace], desktop: &Desktop, limit: usize) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..traces.len())
        .filter(|index| error_rank(&traces[*index], desktop).is_some())
        .collect();
    indices.sort_by(|&left, &right| {
        let left_error = error_rank(&traces[left], desktop).unwrap_or((i32::MIN, i32::MIN));
        let right_error = error_rank(&traces[right], desktop).unwrap_or((i32::MIN, i32::MIN));
        right_error.cmp(&left_error).then(left.cmp(&right))
    });
    indices.truncate(limit);
    indices
}

pub fn format_hist(hist: &[u32; 17]) -> String {
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

fn ms(duration: Duration) -> String {
    format!("{:.3}", duration.as_secs_f64() * 1000.0)
}

fn pos_text(pos: Option<(i32, i32)>) -> String {
    match pos {
        Some((x, y)) => format!("({x},{y})"),
        None => "none".to_string(),
    }
}

/// Full trajectory, including samples that are not the stable landing.
pub fn format_trace(trace: &MoveTrace) -> String {
    let samples: Vec<String> = trace
        .samples
        .iter()
        .map(|sample| match sample.pos {
            Some((x, y)) => format!("{}:({x},{y})", ms(sample.at)),
            None => format!("{}:unread", ms(sample.at)),
        })
        .collect();
    let first = match trace.first_within_budget {
        Some((at, x, y)) => format!("{}:({x},{y})", ms(at)),
        None => "none".to_string(),
    };
    format!(
        "requested=({},{}) normalized=({},{}) park=({},{}) origin_ms={} terminal={} first_15ms={first} stable={} verdict={:?} samples=[{}]",
        trace.requested.0,
        trace.requested.1,
        trace.normalized.0,
        trace.normalized.1,
        trace.park.0,
        trace.park.1,
        ms(trace.origin),
        pos_text(trace.terminal),
        pos_text(trace.stable),
        trace.verdict,
        samples.join(" ")
    )
}

/// `conclusive >= 95%` of `planned`, and `conclusive >= 400` unless the screen is smaller
/// than that floor. A small screen uses the actual planned count as the floor.
pub fn acceptance_met(planned: u32, conclusive: u32, screen_small: bool) -> bool {
    if planned == 0 {
        return false;
    }
    let ratio = u64::from(conclusive) * 100 >= u64::from(planned) * u64::from(CONCLUSIVE_PERCENT);
    let floor = if screen_small { planned } else { SAMPLE_FLOOR };
    ratio && conclusive >= floor
}

fn sample_at(us: u64, pos: (i32, i32)) -> CursorSample {
    CursorSample {
        at: Duration::from_micros(us),
        pos: Some(pos),
    }
}

fn trace_of(park: (i32, i32), samples: Vec<CursorSample>, requested: (i32, i32)) -> MoveTrace {
    MoveTrace::capture(requested, (1, 2), park, Duration::from_millis(4), samples)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dual() -> (Desktop, Vec<MonitorRect>) {
        (
            Desktop {
                origin_x: 0,
                origin_y: 0,
                width: 4480,
                height: 1440,
            },
            vec![
                MonitorRect {
                    left: 0,
                    top: 0,
                    right: 1920,
                    bottom: 1080,
                },
                MonitorRect {
                    left: 1920,
                    top: 0,
                    right: 4480,
                    bottom: 1440,
                },
            ],
        )
    }

    fn in_gap(x: i32, y: i32) -> bool {
        (0..1920).contains(&x) && (1080..1440).contains(&y)
    }

    #[test]
    fn arrival_budget_is_the_production_15ms_window() {
        assert_eq!(ARRIVAL_BUDGET, Duration::from_millis(15));
        assert_eq!(TAIL_SAMPLES, 2);
    }

    #[test]
    fn floor_model_round_trips_and_reports_requested_misses() {
        // win-test 2026-10-23: request -2553 on origin -2560, extent 6000.
        assert_eq!(normalize_axis(-2553, -2560, 6000), 77);
        assert_eq!(denormalize_axis(77, -2560, 6000), -2553);
        assert_eq!(normalize_axis(720, 0, 1440), 32768);
        assert_eq!(denormalize_axis(32768, 0, 1440), 720);
        assert_eq!(disagreement_offsets(1920), vec![1, 2, 3, 8, 9, 16, 23]);
        assert_eq!(disagreement_offsets(2560), vec![2, 4, 7, 9, 12, 17, 22, 27]);
        assert_eq!(disagreement_offsets(1440), vec![2, 4, 6, 8, 10]);
        assert_eq!(disagreement_offsets(1080), vec![2, 3, 6]);
        let live = disagreement_offsets(6000);
        assert_eq!((live.len(), live.first().copied()), (72, Some(7)));
        assert_eq!(disagreement_offsets(7680).len(), 110);
        assert_eq!(disagreement_offsets(3440).len(), 25);
        let pair = |pos: (i32, i32)| vec![sample_at(500, pos), sample_at(49_000, pos)];
        let hit = MoveTrace::capture((10, 10), (1, 1), (0, 0), Duration::ZERO, pair((10, 10)));
        let miss = MoveTrace::capture((10, 10), (1, 1), (0, 0), Duration::ZERO, pair((9, 10)));
        let parked = trace_of((1, 1), pair((1, 1)), (10, 10));
        assert_eq!(
            requested_miss_indices(&[hit, miss, parked.clone()], 20),
            vec![1]
        );
        assert!(requested_miss_indices(&[parked], 20).is_empty());
    }

    #[test]
    fn early_p_then_final_q_lands_on_q_and_keeps_the_trace() {
        let park = (0, 0);
        let early = (10, 10);
        let final_q = (20, 30);
        // The first equal pair is `early`. The tail is `final_q`.
        let trace = trace_of(
            park,
            vec![
                sample_at(500, early),
                sample_at(1_000, early),
                sample_at(20_000, final_q),
                sample_at(49_500, final_q),
            ],
            (20, 30),
        );
        assert_eq!(trace.verdict, Ok(()));
        assert_eq!(trace.stable, Some(final_q));
        assert_ne!(trace.stable, Some(early));
        assert_eq!(
            trace.first_within_budget,
            Some((Duration::from_micros(500), early.0, early.1))
        );
        assert_eq!(trace.terminal, Some(final_q));
        assert_eq!(trace.requested, (20, 30));
        assert_eq!(trace.normalized, (1, 2));
        assert_eq!(trace.park, park);
        assert_eq!(trace.origin, Duration::from_millis(4));
        assert_eq!(trace.samples.len(), 4);
        let text = format_trace(&trace);
        assert!(text.contains("requested=(20,30)"), "{text}");
        assert!(text.contains("park=(0,0)"), "{text}");
        assert!(text.contains("origin_ms="), "{text}");
        assert!(text.contains("(10,10)"), "{text}");
        assert!(text.contains("(20,30)"), "{text}");
        assert!(trace.conclusive());
    }

    #[test]
    fn park_only_sequence_is_inconclusive_and_not_a_landing() {
        let park = (5, 5);
        let trace = trace_of(
            park,
            vec![
                sample_at(500, park),
                sample_at(1_000, park),
                sample_at(20_000, park),
                sample_at(49_500, park),
            ],
            (80, 80),
        );
        assert_eq!(trace.verdict, Err(Inconclusive::NoDeparture));
        assert_eq!(trace.stable, None);
        assert_eq!(trace.first_within_budget, None);
        assert!(!trace.conclusive());
        let desktop = Desktop {
            origin_x: 0,
            origin_y: 0,
            width: 1920,
            height: 1080,
        };
        let hist = histograms(&[trace], &desktop);
        assert_eq!(hist.conclusive, 0);
        assert_eq!(hist.inverse_dx.iter().sum::<u32>(), 0);
        assert_eq!(hist.requested_dx.iter().sum::<u32>(), 0);
    }

    #[test]
    fn nonzero_inverse_error_stays_in_the_histogram_and_outranks_zero() {
        let desktop = Desktop {
            origin_x: 0,
            origin_y: 0,
            width: 1920,
            height: 1080,
        };
        let requested = (100, 100);
        let inverse = inverse_pixel(requested, &desktop);
        let off = (inverse.0 + 3, inverse.1);
        let conclusive = MoveTrace::capture(
            requested,
            (9, 9),
            (0, 0),
            Duration::ZERO,
            vec![sample_at(500, off), sample_at(49_000, off)],
        );
        let exact = MoveTrace::capture(
            requested,
            (9, 9),
            (0, 0),
            Duration::ZERO,
            vec![sample_at(500, inverse), sample_at(49_000, inverse)],
        );
        let parked = trace_of(
            (1, 1),
            vec![sample_at(500, (1, 1)), sample_at(49_000, (1, 1))],
            requested,
        );
        assert!(conclusive.conclusive());
        assert_eq!(parked.verdict, Err(Inconclusive::NoDeparture));
        let hist = histograms(
            &[conclusive.clone(), exact.clone(), parked.clone()],
            &desktop,
        );
        assert_eq!(hist.conclusive, 2);
        assert_eq!(hist.inverse_dx[11], 1, "delta +3 is retained");
        assert_eq!(hist.inverse_dx[8], 1, "the exact landing stays in bin 0");
        assert_eq!(hist.max_inverse, 3);
        let printed = format_hist(&hist.inverse_dx);
        assert!(printed.contains("3:1"), "{printed}");
        assert!(printed.contains("0:1"), "{printed}");
        let ranked = largest_error_indices(&[conclusive, exact, parked], &desktop, 20);
        assert_eq!(ranked, vec![0, 1]);
    }

    #[test]
    fn dual_monitor_targets_all_land_inside_the_monitor_union() {
        let (desktop, monitors) = dual();
        let plan = build_targets(&desktop, &monitors).expect("dual monitors can fill 400");
        assert!(!plan.screen_small);
        assert!(
            plan.points.len() >= usize::try_from(SAMPLE_FLOOR).unwrap_or(usize::MAX),
            "got {}",
            plan.points.len()
        );
        assert!(
            plan.points
                .iter()
                .all(|point| on_monitor(&monitors, point.0, point.1)),
            "a target left the monitor union"
        );
        assert!(
            plan.points.iter().all(|point| !in_gap(point.0, point.1)),
            "gap pixel entered the acceptance set"
        );
        assert!(!plan.points.contains(&(1167, 1252)));
        assert!(!plan.points.contains(&(1259, 1360)));
        assert!(plan
            .points
            .iter()
            .any(|point| point.0 < 1920 && point.1 < 1080));
        assert!(plan.points.iter().any(|point| point.0 >= 1920));
        for target in [(100, 100), (4300, 100), (2000, 1400), (4000, 1300)] {
            let park = park_corner(target, &monitors).expect("park");
            assert!(
                on_monitor(&monitors, park.0, park.1),
                "{target:?} parked at {park:?}"
            );
            assert!(
                !in_gap(park.0, park.1),
                "{target:?} parked in the gap at {park:?}"
            );
            assert!(target.0.abs_diff(park.0).max(target.1.abs_diff(park.1)) > 200);
        }
    }

    #[test]
    fn disagreement_on_a_gap_midline_moves_onto_a_monitor() {
        // Bounding midline y=550 sits in the gap between the two monitors.
        let desktop = Desktop {
            origin_x: 0,
            origin_y: 0,
            width: 1000,
            height: 1100,
        };
        let monitors = vec![
            MonitorRect {
                left: 0,
                top: 0,
                right: 1000,
                bottom: 400,
            },
            MonitorRect {
                left: 0,
                top: 600,
                right: 1000,
                bottom: 1100,
            },
        ];
        let plan = build_targets(&desktop, &monitors).expect("stacked monitors");
        assert!(plan.points.len() >= 400);
        assert!(plan
            .points
            .iter()
            .all(|point| on_monitor(&monitors, point.0, point.1)));
        assert!(plan.points.iter().all(|point| point.1 != 550));
        for x in disagreement_offsets(desktop.width) {
            assert!(
                plan.points
                    .iter()
                    .any(|point| point.0 == x && on_monitor(&monitors, point.0, point.1)),
                "disagreement x {x} missing"
            );
        }
        for y in disagreement_offsets(desktop.height) {
            assert!(
                plan.points.iter().any(|point| point.1 == y),
                "disagreement y {y} missing"
            );
        }
    }

    #[test]
    fn small_screen_uses_the_actual_target_count() {
        let monitors = [MonitorRect {
            left: 0,
            top: 0,
            right: 10,
            bottom: 10,
        }];
        let desktop = Desktop {
            origin_x: 0,
            origin_y: 0,
            width: 10,
            height: 10,
        };
        let plan = build_targets(&desktop, &monitors).expect("tiny monitor");
        assert!(plan.screen_small);
        assert!(!plan.points.is_empty());
        assert!(plan.points.len() <= 100);
        assert!(plan
            .points
            .iter()
            .all(|point| on_monitor(&monitors, point.0, point.1)));
        let planned = u32::try_from(plan.points.len()).unwrap_or(u32::MAX);
        assert!(acceptance_met(planned, planned, true));
        assert!(!acceptance_met(planned, planned.saturating_sub(1), true));
        assert!(
            !acceptance_met(400, 380, false),
            "95% of 400 is still under the floor"
        );
        assert!(acceptance_met(400, 400, false));
        assert!(acceptance_met(1_000, 950, false));
        assert!(!acceptance_met(1_000, 949, false));
    }
}

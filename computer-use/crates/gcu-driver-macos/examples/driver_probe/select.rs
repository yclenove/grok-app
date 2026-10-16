use gcu_protocol::WindowInfo;

/// Select only a currently listed window owned by the resolved fixture PID.
/// A missing key window is normal for a background app. Match the driver's
/// largest-window fallback without fabricating a CG id from an AX array index.
pub fn window(listed: &[WindowInfo], key: Option<&WindowInfo>, pid: u32) -> Option<WindowInfo> {
    let owned = |win: &&WindowInfo| crate_pid(&win.id) == Some(pid);
    key.and_then(|key| listed.iter().filter(owned).find(|win| win.id == key.id))
        .or_else(|| {
            listed.iter().filter(owned).max_by(|a, b| {
                (a.frame.w * a.frame.h)
                    .total_cmp(&(b.frame.w * b.frame.h))
                    .then(b.id.cmp(&a.id))
            })
        })
        .cloned()
}

fn crate_pid(id: &str) -> Option<u32> {
    let mut parts = id.split(':');
    (parts.next()? == "cg").then_some(())?;
    parts.next()?.parse::<u32>().ok().filter(|id| *id > 0)?;
    let pid = parts.next()?.parse().ok()?;
    parts.next().is_none().then_some(pid)
}

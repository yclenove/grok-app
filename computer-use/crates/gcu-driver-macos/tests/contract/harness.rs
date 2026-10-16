//! Minimal serial harness: libtest spawns test threads even with
//! --test-threads=1, but the native event probe needs AppKit on main first.

pub struct Case {
    pub name: &'static str,
    pub ignored: bool,
    pub run: fn(),
}

pub fn run(cases: &[Case]) -> Result<(), String> {
    let mut ignored_only = false;
    let mut include_ignored = false;
    let mut list = false;
    let mut exact = false;
    let mut filters = Vec::new();
    let mut skips = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--ignored" => ignored_only = true,
            "--include-ignored" => include_ignored = true,
            "--list" => list = true,
            "--exact" => exact = true,
            "--skip" => skips.push(args.next().ok_or("--skip requires a filter")?),
            "--nocapture" | "--show-output" | "--test" => {}
            "--test-threads=1" => {}
            "--test-threads" if args.next().as_deref() == Some("1") => {}
            _ if !arg.starts_with('-') => filters.push(arg),
            _ => return Err(format!("unsupported contract harness argument: {arg}")),
        }
    }
    let matches = |name: &str, filter: &str| {
        if exact {
            name == filter
        } else {
            name.contains(filter)
        }
    };
    let mut failed = Vec::new();
    for case in cases.iter().filter(|case| {
        (filters.is_empty() || filters.iter().any(|f| matches(case.name, f)))
            && !skips.iter().any(|s| matches(case.name, s))
            && (!ignored_only || case.ignored)
    }) {
        if list {
            println!("{}: test", case.name);
        } else if case.ignored && !ignored_only && !include_ignored {
            println!("test {} ... ignored (macOS GUI + TCC)", case.name);
        } else if std::panic::catch_unwind(case.run).is_ok() {
            println!("test {} ... ok", case.name);
        } else {
            println!("test {} ... FAILED", case.name);
            failed.push(case.name);
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("failed: {failed:?}"))
    }
}

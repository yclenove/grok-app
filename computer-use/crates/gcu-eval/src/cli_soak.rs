//! Core soak CLI. Native adapters are not supplied by this binary.

use super::{Cli, Stop};
use gcu_eval::host_platform;

pub(crate) fn soak(cli: &Cli, hours: u64) -> Result<u8, Stop> {
    use gcu_eval::soak::{
        not_run_report, parse_report, validate_report, write_report, FileArtifacts, SoakConfig,
        SoakPlatform, Status,
    };
    if cli.suite != "core" {
        return Err(Stop::Usage("soak requires --suite core".into()));
    }
    let platform = match cli.platform.as_str() {
        "host" => match host_platform() {
            "macos" => SoakPlatform::Macos,
            "windows" => SoakPlatform::Windows,
            _ => SoakPlatform::LinuxX11,
        },
        "macos" => SoakPlatform::Macos,
        "windows" => SoakPlatform::Windows,
        "linux" | "linux-x11" => SoakPlatform::LinuxX11,
        _ => return Err(Stop::Usage("unsupported soak platform".into())),
    };
    let config = SoakConfig::core(platform, hours);
    config
        .duration_ms()
        .map_err(|error| Stop::Usage(error.to_string()))?;
    let report = if let Some(path) = &cli.soak_report {
        let bytes =
            std::fs::read(path).map_err(|_| Stop::Usage("could not read soak report".into()))?;
        let report = parse_report(&bytes).map_err(|error| Stop::Run(error.into()))?;
        if report.config.platform != platform {
            return Err(Stop::Usage(
                "soak report platform does not match --platform".into(),
            ));
        }
        report
    } else {
        not_run_report(
            config,
            "matching one-shot desktop soak adapters and native platform artifact unavailable",
        )
    };
    let root = cli
        .soak_report
        .as_deref()
        .and_then(std::path::Path::parent)
        .unwrap_or_else(|| std::path::Path::new("."));
    let checked = validate_report(&report, hours, &FileArtifacts::new(root))
        .map_err(|error| Stop::Run(error.into()))?;
    println!(
        "soak {} resource_gate={} tasks={} crash_restarts={} stop_restarts={}",
        checked.status.as_str(),
        checked.resource_gate.as_str(),
        checked.counts.completed_tasks,
        checked.counts.crash_restarts,
        checked.counts.stop_restarts
    );
    for issue in &checked.issues {
        println!("fail {issue}");
    }
    for reason in &checked.not_run {
        println!("not_run {reason}");
    }
    if let Some(dir) = &cli.out {
        write_report(dir, &report).map_err(|error| Stop::Run(error.into()))?;
    }
    Ok(u8::from(checked.status != Status::Pass))
}

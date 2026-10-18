//! Offline evaluation CLI.
//!
//! A one-shot desktop container is required for a real model. This binary never
//! calls a model API, never reads `~/.grok`, `~/.codex`, `~/.gemini`, or
//! `~/.claude`, and never logs into an account, IM, or mailbox. Isolated
//! `GROK_HOME` is not an environment boundary.

use anyhow::Context;
use clap::Parser;
use gcu_eval::{
    block_on, bundled_tasks_dir, compare_trials, host_platform, judge_release, load_tasks,
    parse_approval, parse_release_evidence, run_tasks, trials_from_summary, write_release_report,
    write_reports, RunConfig, RunStatus, SuiteReport,
};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

#[derive(Debug, Parser)]
#[command(name = "gcu-eval", about = "Run computer-use evaluation tasks offline")]
struct Cli {
    /// Suite name stored on each task. Default `core`.
    #[arg(long, default_value = "core")]
    suite: String,
    /// `linux`, `macos`, `windows`, or `host`.
    #[arg(long, default_value = "host")]
    platform: String,
    /// Model id recorded on the report. `fake` does not call a network API.
    #[arg(long, default_value = "fake")]
    model: String,
    /// Client binary. Absent clients are `not_run`.
    #[arg(long)]
    grok: Option<PathBuf>,
    /// `gcu` binary placed in the isolated MCP config.
    #[arg(long)]
    gcu: Option<PathBuf>,
    /// Task directory. Default is the bundled `eval/tasks` tree.
    #[arg(long)]
    tasks: Option<PathBuf>,
    /// Report directory. Omitted reports are not written into the repo.
    #[arg(long)]
    out: Option<PathBuf>,
    /// `local` uses a temp directory for tests. `desktop` is interface-only.
    #[arg(long, default_value = "local")]
    env: String,
    /// Override each task budget, in milliseconds.
    #[arg(long)]
    timeout_ms: Option<u64>,
    /// A/B group label stored on each row.
    #[arg(long, default_value = "A")]
    group: String,
    /// Compare two summary JSON files and exit.
    #[arg(long, num_args = 2, value_names = ["A", "B"])]
    compare: Option<Vec<PathBuf>>,
    /// Exit 0 when tasks fail. Invalid oracles still fail the process.
    #[arg(long)]
    allow_fail: bool,
    /// Judge a section 8 evidence file and skip the task run.
    #[arg(long)]
    release_gate: Option<PathBuf>,
    /// Opt in to core soak validation for N hours. No implicit long run.
    #[arg(long, conflicts_with_all = ["compare", "release_gate"])]
    soak_hours: Option<u64>,
    /// Validate a raw soak report bundle; native adapters are supplied separately.
    #[arg(long, requires = "soak_hours", conflicts_with_all = ["compare", "release_gate"])]
    soak_report: Option<PathBuf>,
    /// Ask the child for CU auto-approval. Only an eval-autoapprove debug build accepts this.
    #[arg(long)]
    auto_approve: bool,
    /// Scripted client behavior. Hidden; tests and the fake agent use it.
    #[arg(long, hide = true)]
    behavior: Option<String>,
}

fn main() -> ExitCode {
    match dispatch() {
        Ok(code) => ExitCode::from(code),
        Err(stop) => {
            eprintln!("{stop}");
            ExitCode::from(stop.code())
        }
    }
}

fn dispatch() -> Result<u8, Stop> {
    check_approval()?;
    let cli = Cli::parse();
    if cli.auto_approve && !eval_build() {
        return Err(Stop::Approval(
            "auto-approve requires an eval-autoapprove debug build".into(),
        ));
    }
    if let Some(paths) = &cli.compare {
        return compare(paths);
    }
    if let Some(path) = &cli.release_gate {
        return release_gate(path, cli.out.as_deref());
    }
    if let Some(hours) = cli.soak_hours {
        return soak(&cli, hours);
    }
    let run_local = match cli.env.as_str() {
        "local" => true,
        "desktop" => false,
        other => {
            return Err(Stop::Usage(format!(
                "unknown --env {other}; expected local or desktop"
            )));
        }
    };
    let tasks_dir = cli
        .tasks
        .clone()
        .or_else(bundled_tasks_dir)
        .ok_or_else(|| Stop::Usage("task directory not found; pass --tasks".into()))?;
    let mut tasks = load_tasks(&tasks_dir).map_err(|err| Stop::Run(err.into()))?;
    tasks.retain(|task| task.suites.iter().any(|suite| suite == &cli.suite));
    let platform = if cli.platform == "host" {
        host_platform().to_string()
    } else if matches!(cli.platform.as_str(), "linux" | "macos" | "windows") {
        cli.platform.clone()
    } else {
        return Err(Stop::Usage(format!("unknown platform {}", cli.platform)));
    };
    let cfg = RunConfig {
        grok_bin: cli.grok.clone(),
        gcu_bin: cli.gcu.clone(),
        model: cli.model.clone(),
        platform,
        group: cli.group.clone(),
        suite: cli.suite.clone(),
        timeout: cli.timeout_ms.map(Duration::from_millis),
        behavior: cli.behavior.clone(),
        auto_approve: cli.auto_approve,
        include_desktop: true,
        run_local,
        ..RunConfig::default()
    };
    let report = block_on(run_tasks(&tasks, &cfg)).map_err(|err| Stop::Run(err.into()))?;
    for task in &report.tasks {
        print_row(task);
    }
    if let Some(dir) = &cli.out {
        write_reports(dir, &report).map_err(|err| Stop::Run(err.into()))?;
    }
    Ok(exit_code(&report, cli.allow_fail))
}

fn soak(cli: &Cli, hours: u64) -> Result<u8, Stop> {
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

fn release_gate(path: &std::path::Path, out: Option<&std::path::Path>) -> Result<u8, Stop> {
    let bytes =
        std::fs::read(path).map_err(|err| Stop::Usage(format!("read release gate: {err}")))?;
    let evidence = parse_release_evidence(&bytes).map_err(|err| Stop::Run(err.into()))?;
    let decision = judge_release(&evidence);
    println!(
        "release tier={} channel={} promote={} default_formal={}",
        decision.tier.as_str(),
        decision.channel.as_str(),
        decision.promote,
        decision.default_formal
    );
    for reason in &decision.reasons {
        println!("gate {reason}");
    }
    if let Some(dir) = out {
        write_release_report(dir, &decision).map_err(|err| Stop::Run(err.into()))?;
    }
    Ok(0)
}

fn compare(paths: &[PathBuf]) -> Result<u8, Stop> {
    let left = std::fs::read(paths.first().context("compare A").map_err(Stop::run)?)
        .map_err(|err| Stop::Usage(format!("read compare A: {err}")))?;
    let right = std::fs::read(paths.get(1).context("compare B").map_err(Stop::run)?)
        .map_err(|err| Stop::Usage(format!("read compare B: {err}")))?;
    let rows = compare_trials(
        &trials_from_summary(&left).map_err(|err| Stop::Run(err.into()))?,
        &trials_from_summary(&right).map_err(|err| Stop::Run(err.into()))?,
    );
    for row in rows {
        println!(
            "{} pairs={} p={:.4} significant={} diff={:.4}",
            row.task_id, row.pairs, row.p_value, row.significant, row.mean_diff
        );
    }
    Ok(0)
}

fn print_row(task: &gcu_eval::TaskOutcome) {
    if task.reason.is_empty() {
        println!("{} {}", task.status.as_str(), task.id);
    } else {
        println!("{} {}: {}", task.status.as_str(), task.id, task.reason);
    }
}

fn exit_code(report: &SuiteReport, allow_fail: bool) -> u8 {
    let mut invalid = false;
    let mut bad = false;
    for task in &report.tasks {
        match task.status {
            RunStatus::Invalid => invalid = true,
            RunStatus::Fail
            | RunStatus::Partial
            | RunStatus::Timeout
            | RunStatus::Crash
            | RunStatus::SideEffect => bad = true,
            RunStatus::Pass | RunStatus::NotRun => {}
        }
    }
    if invalid || (bad && !allow_fail) {
        1
    } else {
        0
    }
}

fn check_approval() -> Result<(), Stop> {
    match std::env::var("GCU_APPROVAL") {
        Err(std::env::VarError::NotPresent) => Ok(()),
        Err(_) => Err(Stop::Approval("GCU_APPROVAL must be valid text".into())),
        Ok(value) => parse_approval(Some(&value), eval_build())
            .map(|_| ())
            .map_err(|err| Stop::Approval(err.to_string())),
    }
}

const fn eval_build() -> bool {
    cfg!(all(feature = "eval-autoapprove", debug_assertions))
}

enum Stop {
    Approval(String),
    Usage(String),
    Run(anyhow::Error),
}

impl Stop {
    fn run(err: anyhow::Error) -> Self {
        Self::Run(err)
    }

    fn code(&self) -> u8 {
        match self {
            Self::Approval(_) | Self::Usage(_) => 2,
            Self::Run(_) => 1,
        }
    }
}

impl std::fmt::Display for Stop {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Approval(text) | Self::Usage(text) => formatter.write_str(text),
            Self::Run(err) => write!(formatter, "{err}"),
        }
    }
}

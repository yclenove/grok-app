//! Offline evaluation CLI.
//!
//! A one-shot desktop container is required for a real model. This binary never
//! calls a model API, never reads `~/.grok`, `~/.codex`, `~/.gemini`, or
//! `~/.claude`, and never logs into an account, IM, or mailbox. Isolated
//! `GROK_HOME` is not an environment boundary.

use anyhow::Context;
use clap::Parser;
use gcu_eval::{
    block_on, bundled_tasks_dir, compare_trials, host_platform, is_eval_fake_agent, judge_release,
    load_tasks, parse_approval, parse_release_evidence, run_tasks, trials_from_summary,
    write_release_report, write_reports, RunConfig, RunStatus, SuiteReport, ATTEST_REQUIRED,
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
    /// Validate the fixed browser L0/L2 experiment; never run a local fake A/B.
    #[arg(long, conflicts_with_all = ["compare", "release_gate", "soak_hours", "tasks"])]
    ab: bool,
    /// Read a native browser A/B bundle index; absent collectors are not_run.
    #[arg(long, requires = "ab")]
    ab_report: Option<PathBuf>,
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
    /// `fake` (default) uses the fake driver. `real` drives the real `gcu` driver and refuses to
    /// start unless `--attest-disposable-desktop` is set.
    #[arg(long, default_value = "fake")]
    driver: String,
    /// Attest that this process is on a one-shot isolated desktop with egress control.
    ///
    /// Required before a real driver or any client other than `eval-fake-agent`. An environment
    /// variable cannot replace this flag. It is not a confinement proof.
    #[arg(long)]
    attest_disposable_desktop: bool,
    /// Fixture app started for each task when `--driver real`.
    #[arg(long)]
    fixture: Option<PathBuf>,
    /// Directory with a login made for this evaluation only (`--driver real`). Only its `auth.json` is
    /// copied, unread, into each task's temporary `GROK_HOME`.
    #[arg(long)]
    auth_seed: Option<PathBuf>,
    /// Run only these task ids (comma separated).
    #[arg(long, value_delimiter = ',')]
    only: Vec<String>,
}

/// Fields every `RunConfig` takes from the real-driver flags.
struct RealFlags {
    real_driver: bool,
    fixture_bin: Option<PathBuf>,
    auth_seed: Option<PathBuf>,
}

fn real_flags(cli: &Cli) -> Result<RealFlags, Stop> {
    match cli.driver.as_str() {
        "fake" => {
            if let Some(grok) = &cli.grok {
                // A missing path never starts. Refuse only a client that is actually there.
                let present = std::fs::symlink_metadata(grok).is_ok();
                if present && !is_eval_fake_agent(grok) && !cli.attest_disposable_desktop {
                    return Err(Stop::Usage(ATTEST_REQUIRED.into()));
                }
            }
            Ok(RealFlags {
                real_driver: false,
                fixture_bin: None,
                auth_seed: None,
            })
        }
        "real" => {
            if cli.grok.is_none() || cli.gcu.is_none() || cli.fixture.is_none() {
                return Err(Stop::Usage(
                    "--driver real needs --grok, --gcu and --fixture".into(),
                ));
            }
            if cli.env != "local" {
                return Err(Stop::Usage("--driver real needs --env local".into()));
            }
            if !cli.attest_disposable_desktop {
                return Err(Stop::Usage(format!("--driver real: {ATTEST_REQUIRED}")));
            }
            if !cli.auto_approve {
                return Err(Stop::Usage(
                    "--driver real needs --auto-approve (an eval-autoapprove debug build)".into(),
                ));
            }
            if cli.model != "fake" && cli.auth_seed.is_none() {
                return Err(Stop::Usage(
                    "--driver real with a real model needs --auth-seed".into(),
                ));
            }
            Ok(RealFlags {
                real_driver: true,
                fixture_bin: cli.fixture.clone(),
                auth_seed: cli.auth_seed.clone(),
            })
        }
        other => Err(Stop::Usage(format!(
            "unknown --driver {other}; expected fake or real"
        ))),
    }
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
    if cli.ab {
        return match cli.suite.as_str() {
            "browser" => browser_ab(&cli),
            "core" => core_ab(&cli),
            _ => Err(Stop::Usage(
                "--ab supports --suite core or --suite browser".into(),
            )),
        };
    }
    if cli.suite == "browser" {
        return Err(Stop::Usage("browser suite requires --ab".into()));
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
    retain_only(&mut tasks, &cli)?;
    let platform = platform_label(&cli)?;
    let real = real_flags(&cli)?;
    let cfg = RunConfig {
        real_driver: real.real_driver,
        fixture_bin: real.fixture_bin,
        auth_seed: real.auth_seed,
        attest_disposable_desktop: cli.attest_disposable_desktop,
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

fn retain_only(tasks: &mut Vec<gcu_eval::Task>, cli: &Cli) -> Result<(), Stop> {
    if cli.only.is_empty() {
        return Ok(());
    }
    if let Some(unknown) = cli
        .only
        .iter()
        .find(|id| !tasks.iter().any(|task| &task.id == *id))
    {
        return Err(Stop::Usage(format!("--only names unknown task {unknown}")));
    }
    tasks.retain(|task| cli.only.iter().any(|id| id == &task.id));
    Ok(())
}

fn platform_label(cli: &Cli) -> Result<String, Stop> {
    if cli.platform == "host" {
        Ok(host_platform().to_string())
    } else if matches!(cli.platform.as_str(), "linux" | "macos" | "windows") {
        Ok(cli.platform.clone())
    } else {
        Err(Stop::Usage(format!("unknown platform {}", cli.platform)))
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn core_tasks() -> Result<Vec<gcu_eval::Task>, Stop> {
    let dir = bundled_tasks_dir()
        .ok_or_else(|| Stop::Usage("bundled core task directory not found".into()))?;
    let mut tasks = load_tasks(&dir).map_err(|err| Stop::Run(err.into()))?;
    tasks.retain(|task| task.suites.iter().any(|suite| suite == "core"));
    if tasks.is_empty() {
        return Err(Stop::Usage("core suite is empty".into()));
    }
    Ok(tasks)
}

fn core_ab(cli: &Cli) -> Result<u8, Stop> {
    use gcu_eval::ab::{
        decide, format_utc_date, not_run_report, parse_report, run_ab, write_reports, Conclusion,
        ReportId,
    };
    let platform = platform_label(cli)?;
    let id = ReportId {
        platform,
        model: cli.model.clone(),
        date: format_utc_date(unix_now()),
        effort: "unspecified".into(),
        machine: "unspecified".into(),
    };
    let report = if let Some(path) = &cli.ab_report {
        let bytes = std::fs::read(path)
            .map_err(|_| Stop::Usage("could not read core A/B report".into()))?;
        parse_report(&bytes).map_err(|err| Stop::Run(err.into()))?
    } else {
        let run_local = match cli.env.as_str() {
            "local" => true,
            "desktop" => false,
            other => {
                return Err(Stop::Usage(format!(
                    "unknown --env {other}; expected local or desktop"
                )));
            }
        };
        let mut tasks = core_tasks()?;
        retain_only(&mut tasks, cli)?;
        let real = real_flags(cli)?;
        if cli.grok.is_none() || !run_local {
            not_run_report(&tasks, &id)
        } else {
            let cfg = RunConfig {
                real_driver: real.real_driver,
                fixture_bin: real.fixture_bin,
                auth_seed: real.auth_seed,
                attest_disposable_desktop: cli.attest_disposable_desktop,
                grok_bin: cli.grok.clone(),
                gcu_bin: cli.gcu.clone(),
                model: cli.model.clone(),
                platform: id.platform.clone(),
                suite: "core".into(),
                timeout: cli.timeout_ms.map(Duration::from_millis),
                behavior: cli.behavior.clone(),
                auto_approve: cli.auto_approve,
                include_desktop: false,
                run_local: true,
                ..RunConfig::default()
            };
            block_on(run_ab(&tasks, &cfg, &id)).map_err(|err| Stop::Run(err.into()))?
        }
    };
    let decision = decide(&report).map_err(|err| Stop::Run(err.into()))?;
    println!(
        "core_ab {} success={} rounds={} tokens={} red_team={}",
        decision.conclusion.as_str(),
        decision.success.as_str(),
        decision.rounds.as_str(),
        decision.tokens_in.as_str(),
        decision.red_team.as_str()
    );
    for reason in &decision.reasons {
        println!("gate {reason}");
    }
    for trial in &report.trials {
        if trial.status == RunStatus::NotRun {
            println!(
                "not_run {} {} {}",
                trial.task_id,
                trial.arm.as_str(),
                trial.repeat
            );
        }
    }
    if let Some(dir) = &cli.out {
        write_reports(dir, &report).map_err(|err| Stop::Run(err.into()))?;
    }
    Ok(u8::from(decision.conclusion != Conclusion::RecommendB))
}

fn browser_ab(cli: &Cli) -> Result<u8, Stop> {
    use gcu_eval::browser::{
        not_run_report, parse_report, validate_report, write_report, FileArtifacts,
        ValidationStatus,
    };
    if cli.suite != "browser" {
        return Err(Stop::Usage("--ab requires --suite browser".into()));
    }
    let dir = bundled_tasks_dir()
        .ok_or_else(|| Stop::Usage("fixed browser task catalog unavailable".into()))?;
    let tasks = load_tasks(&dir).map_err(|error| Stop::Run(error.into()))?;
    let report = if let Some(path) = &cli.ab_report {
        let bytes = std::fs::read(path)
            .map_err(|_| Stop::Usage("could not read browser A/B report".into()))?;
        parse_report(&bytes).map_err(|error| Stop::Run(error.into()))?
    } else {
        not_run_report()
    };
    let root = cli
        .ab_report
        .as_deref()
        .and_then(std::path::Path::parent)
        .unwrap_or_else(|| std::path::Path::new("."));
    let validation = validate_report(&report, &tasks, &FileArtifacts::new(root))
        .map_err(|error| Stop::Run(error.into()))?;
    println!(
        "browser_ab {} recommend_l2={} default={}",
        validation.status.as_str(),
        validation.recommend_l2,
        validation.default_level.as_str()
    );
    for reason in &validation.reasons {
        println!("gate {reason}");
    }
    for violation in &validation.violations {
        println!("violation {violation}");
    }
    for pair in &validation.pairs {
        println!(
            "{} pairs={} p={:.4} significant={} diff={:.4}",
            pair.task_id, pair.pairs, pair.p_value, pair.significant, pair.mean_diff
        );
    }
    if let Some(dir) = &cli.out {
        write_report(dir, &report, &validation).map_err(|error| Stop::Run(error.into()))?;
    }
    Ok(u8::from(
        validation.status == ValidationStatus::NotRun || !validation.violations.is_empty(),
    ))
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

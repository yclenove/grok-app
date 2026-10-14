//! `gcu install` and `gcu uninstall`. Blocking filesystem work runs on a worker thread.
//!
//! Confirmation is a typed answer on a real terminal, or the `--yes` flag.
//! Pipes are not terminals, and no environment variable is read as consent.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::args::{ClientName, InstallArgs, ScopeName};
use crate::exit::Failure;
use crate::install::{
    installer, ClientBins, ClientId, Confirmer, InstallCtx, InstallError, ProcessExecutor, Scope,
};

/// Run install (`remove` is false) or uninstall. Blocking IO is off the runtime thread.
pub(crate) async fn execute(args: InstallArgs, remove: bool) -> Result<u8, Failure> {
    let joined = tokio::task::spawn_blocking(move || execute_blocking(args, remove)).await;
    match joined {
        Ok(result) => result,
        Err(_) => Err(Failure::internal("install task failed")),
    }
}

fn execute_blocking(args: InstallArgs, remove: bool) -> Result<u8, Failure> {
    let client = selected_client(&args, remove)?;
    let home = resolve_home(args.home.clone())?;
    let project = resolve_project(args.project.clone())?;
    let gcu = resolve_gcu(args.gcu.clone())?;
    let grok_home = match args.grok_home.clone() {
        Some(path) => Some(absolutize(path)?),
        None => None,
    };
    let agy = if client == ClientName::Antigravity {
        resolve_agy(args.agy.clone())?
    } else {
        None
    };
    let confirmer = TtyConfirmer;
    let executor = ProcessExecutor::default();
    let scope = Scope::from(args.scope);
    let ctx = InstallCtx {
        home: &home,
        grok_home: grok_home.as_deref(),
        project_dir: &project,
        gcu_exe: &gcu,
        scope,
        interactive: tty_confirm_available(),
        yes: args.yes,
        backup_stamp: None,
        confirmer: &confirmer,
        executor: &executor,
        bins: ClientBins {
            agy: agy.as_deref(),
        },
    };
    let tool = installer(ClientId::from(client));
    let plan = if remove {
        tool.plan_removal(&ctx)
    } else {
        tool.plan(&ctx)
    }
    .map_err(fail)?;
    let diff = plan.render_diff();
    if args.dry_run {
        emit(
            args.json,
            &report(
                client,
                args.scope,
                remove,
                true,
                plan.is_noop(),
                &diff,
                &[] as &[PathBuf],
            ),
        )?;
        return Ok(0);
    }
    let outcome = if remove {
        tool.uninstall(&ctx)
    } else {
        tool.apply(&ctx)
    }
    .map_err(fail)?;
    emit(
        args.json,
        &report(
            client,
            args.scope,
            remove,
            false,
            outcome.noop,
            &diff,
            &outcome.backups,
        ),
    )?;
    Ok(0)
}

fn selected_client(args: &InstallArgs, remove: bool) -> Result<ClientName, Failure> {
    let verb = if remove { "uninstall" } else { "install" };
    match (args.client_pos, args.client) {
        (Some(left), Some(right)) if left == right => Ok(left),
        (Some(_), Some(_)) => Err(Failure::arguments(
            "positional client and --client disagree",
        )),
        (Some(client), None) | (None, Some(client)) => Ok(client),
        (None, None) => Err(Failure::arguments(format!(
            "client is required: gcu {verb} <client> or gcu {verb} --client <client>"
        ))),
    }
}

fn tty_confirm_available() -> bool {
    // A pipe is not a terminal. Do not read stdin unless both sides are terminals,
    // and never treat an environment variable as consent.
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

struct TtyConfirmer;

impl Confirmer for TtyConfirmer {
    fn confirm(&self, prompt: &str) -> Result<bool, InstallError> {
        let mut out = io::stdout();
        writeln!(out, "{prompt}").map_err(io_write)?;
        out.flush().map_err(io_write)?;
        let mut line = String::new();
        io::stdin().read_line(&mut line).map_err(io_write)?;
        Ok(confirm_line(&line))
    }
}

fn confirm_line(line: &str) -> bool {
    matches!(line.trim(), "y" | "Y" | "yes" | "YES")
}

fn io_write(source: io::Error) -> InstallError {
    InstallError::Write {
        path: PathBuf::from("terminal"),
        source,
    }
}

fn resolve_home(flag: Option<PathBuf>) -> Result<PathBuf, Failure> {
    resolve_dir(flag, "GCU_HOME", user_home)
}

fn resolve_project(flag: Option<PathBuf>) -> Result<PathBuf, Failure> {
    resolve_dir(flag, "GCU_PROJECT", || {
        std::env::current_dir()
            .map_err(|_| Failure::arguments("Cannot resolve the working directory"))
    })
}

fn resolve_dir(
    flag: Option<PathBuf>,
    env_key: &str,
    fallback: impl FnOnce() -> Result<PathBuf, Failure>,
) -> Result<PathBuf, Failure> {
    let raw = if let Some(path) = flag {
        path
    } else if let Some(value) = std::env::var_os(env_key) {
        if value.is_empty() {
            return Err(Failure::arguments(format!("{env_key} is empty")));
        }
        PathBuf::from(value)
    } else {
        fallback()?
    };
    absolutize(raw)
}

fn user_home() -> Result<PathBuf, Failure> {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| Failure::arguments("Cannot resolve home; pass --home or set GCU_HOME"))
}

fn resolve_gcu(flag: Option<PathBuf>) -> Result<PathBuf, Failure> {
    let raw = match flag {
        Some(path) => path,
        None => std::env::current_exe()
            .map_err(|_| Failure::arguments("Cannot resolve gcu; pass --gcu"))?,
    };
    absolutize(raw)
}

fn resolve_agy(flag: Option<PathBuf>) -> Result<Option<PathBuf>, Failure> {
    match flag {
        Some(path) => Ok(Some(absolutize(path)?)),
        None => Ok(find_on_path("agy")),
    }
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let mut candidates = Vec::new();
    if cfg!(windows) {
        candidates.push(format!("{name}.exe"));
        candidates.push(format!("{name}.cmd"));
    }
    candidates.push(name.to_owned());
    for dir in std::env::split_paths(&path) {
        for candidate in &candidates {
            let full = dir.join(candidate);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
}

fn absolutize(path: PathBuf) -> Result<PathBuf, Failure> {
    std::path::absolute(&path)
        .map_err(|_| Failure::arguments(format!("Cannot resolve path {}", path.display())))
}

struct Report {
    client: ClientName,
    scope: ScopeName,
    action: &'static str,
    dry_run: bool,
    noop: bool,
    diff: String,
    backups: Vec<String>,
}

fn report(
    client: ClientName,
    scope: ScopeName,
    remove: bool,
    dry_run: bool,
    noop: bool,
    diff: &str,
    backups: &[impl AsRef<Path>],
) -> Report {
    Report {
        client,
        scope,
        action: if remove { "uninstall" } else { "install" },
        dry_run,
        noop,
        diff: diff.to_owned(),
        backups: backups
            .iter()
            .map(|path| path.as_ref().display().to_string())
            .collect(),
    }
}

fn emit(json: bool, report: &Report) -> Result<(), Failure> {
    let text = if json {
        render_json(report)?
    } else {
        render_text(report)
    };
    print!("{text}");
    io::stdout()
        .flush()
        .map_err(|_| Failure::internal("Cannot write install result"))
}

fn render_text(report: &Report) -> String {
    let mut out = format!(
        "{} {} ({})\n",
        report.client.as_str(),
        report.action,
        report.scope.as_str()
    );
    if report.dry_run {
        out.push_str("dry-run: no files written\n");
    }
    if report.noop {
        out.push_str("no changes\n");
    }
    out.push_str(&report.diff);
    if !report.diff.ends_with('\n') {
        out.push('\n');
    }
    if !report.backups.is_empty() {
        out.push_str("backups:\n");
        for path in &report.backups {
            out.push_str("- ");
            out.push_str(path);
            out.push('\n');
        }
    }
    out
}

fn render_json(report: &Report) -> Result<String, Failure> {
    let value = serde_json::json!({
        "client": report.client.as_str(),
        "action": report.action,
        "scope": report.scope.as_str(),
        "dry_run": report.dry_run,
        "noop": report.noop,
        "backups": report.backups,
        "diff": report.diff,
    });
    let mut text = serde_json::to_string_pretty(&value)
        .map_err(|_| Failure::internal("Cannot format install JSON"))?;
    text.push('\n');
    Ok(text)
}

fn fail(err: InstallError) -> Failure {
    let code = match err {
        InstallError::ConfirmationRequired
        | InstallError::ConfirmationDeclined
        | InstallError::GcuPathNotAbsolute
        | InstallError::PathNotAbsolute { .. }
        | InstallError::PathNotUnicode { .. }
        | InstallError::InvalidBackupStamp
        | InstallError::MissingClientBinary { .. }
        | InstallError::Unparseable { .. }
        | InstallError::UnexpectedShape { .. }
        | InstallError::ForeignFile { .. } => 2,
        InstallError::CommandFailed { .. }
        | InstallError::CommandTimeout { .. }
        | InstallError::ChangedDuringPlan { .. }
        | InstallError::Read { .. }
        | InstallError::Write { .. }
        | InstallError::LockPoisoned => 10,
    };
    Failure {
        code,
        message: err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::confirm_line;

    #[test]
    fn confirm_line_accepts_only_yes() {
        assert!(confirm_line("y\n"));
        assert!(confirm_line("Y\n"));
        assert!(confirm_line("yes\n"));
        assert!(confirm_line("  YES  \n"));
        assert!(!confirm_line("n\n"));
        assert!(!confirm_line("\n"));
        assert!(!confirm_line("yes please\n"));
        assert!(!confirm_line(""));
    }
}

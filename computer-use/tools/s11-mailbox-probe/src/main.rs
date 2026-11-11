//! `s11-mailbox-probe host-run` and `guest`. Off macOS the real path exits
//! before it creates `--out` or touches tart, hdiutil, or ssh.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use s11_mailbox_probe::Case;
#[cfg(target_os = "macos")]
use s11_mailbox_probe::{guest_entry, run_case, RunRequest};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("{}", usage());
        return ExitCode::from(64);
    }
    if args.len() == 1 && matches!(args[0].as_str(), "-h" | "--help" | "help") {
        println!("{}", usage());
        return ExitCode::SUCCESS;
    }
    match args[0].as_str() {
        "host-run" => host(&args[1..]),
        "guest" => guest(&args[1..]),
        _ => {
            eprintln!("{}", usage());
            ExitCode::from(64)
        }
    }
}

fn usage() -> &'static str {
    "s11-mailbox-probe host-run --case C1|C2|C4|C8 --out DIR [--tart PATH] [--tart-home DIR] [--dry-run]\n\
     s11-mailbox-probe guest C1|C2|C4|C8\n\
     macOS only. Requires one owner notice before a real DEFAULT-network run.\n\
     Does not elevate. Report evidence is DEFAULT-network/mechanics-only."
}

struct Parsed {
    case: Case,
    out: PathBuf,
    tart: Option<PathBuf>,
    tart_home: Option<PathBuf>,
    dry_run: bool,
}

fn host(args: &[String]) -> ExitCode {
    let parsed = match parse_host(args) {
        Ok(parsed) => parsed,
        Err(()) => {
            eprintln!("{}", usage());
            return ExitCode::from(64);
        }
    };
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (
            parsed.case,
            parsed.out,
            parsed.tart,
            parsed.tart_home,
            parsed.dry_run,
        );
        eprintln!("unsupported: s11-mailbox-probe real path is macOS-only");
        ExitCode::from(2)
    }
    #[cfg(target_os = "macos")]
    host_macos(parsed)
}

fn guest(args: &[String]) -> ExitCode {
    let Some(case) = args.first().and_then(|s| Case::parse(s)) else {
        eprintln!("{}", usage());
        return ExitCode::from(64);
    };
    if args.len() != 1 {
        eprintln!("{}", usage());
        return ExitCode::from(64);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = case;
        eprintln!("unsupported: s11-mailbox-probe real path is macOS-only");
        ExitCode::from(2)
    }
    #[cfg(target_os = "macos")]
    ExitCode::from(guest_entry(case) as u8)
}

fn parse_host(args: &[String]) -> Result<Parsed, ()> {
    let mut case = None;
    let mut out = None;
    let mut tart = None;
    let mut tart_home = None;
    let mut dry_run = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--case" => {
                i += 1;
                case = Case::parse(args.get(i).ok_or(())?);
                if case.is_none() {
                    return Err(());
                }
            }
            "--out" => {
                i += 1;
                out = Some(PathBuf::from(args.get(i).ok_or(())?));
            }
            "--tart" => {
                i += 1;
                tart = Some(PathBuf::from(args.get(i).ok_or(())?));
            }
            "--tart-home" => {
                i += 1;
                tart_home = Some(PathBuf::from(args.get(i).ok_or(())?));
            }
            "--dry-run" => dry_run = true,
            _ => return Err(()),
        }
        i += 1;
    }
    Ok(Parsed {
        case: case.ok_or(())?,
        out: out.ok_or(())?,
        tart,
        tart_home,
        dry_run,
    })
}

#[cfg(target_os = "macos")]
fn host_macos(parsed: Parsed) -> ExitCode {
    use std::os::unix::fs::PermissionsExt;

    if parsed.out.exists() {
        eprintln!("out-exists");
        return ExitCode::from(64);
    }
    if std::fs::create_dir(&parsed.out).is_err() {
        eprintln!("mkdir");
        return ExitCode::from(1);
    }
    if std::fs::set_permissions(&parsed.out, std::fs::Permissions::from_mode(0o700)).is_err() {
        eprintln!("mkdir");
        return ExitCode::from(1);
    }
    let home = match env::var_os("HOME") {
        Some(home) => PathBuf::from(home),
        None => {
            eprintln!("home");
            return ExitCode::from(1);
        }
    };
    let tart = parsed.tart.unwrap_or_else(|| home.join("bin").join("tart"));
    let mut runner = s11_mailbox_probe::ProcessRunner::new();
    let req = RunRequest {
        case: parsed.case,
        tart: &tart,
        home: &home,
        out: &parsed.out,
        tart_home: parsed.tart_home.as_deref(),
        dry_run: parsed.dry_run,
        exchange: true,
        ids: &[],
        probe: None,
    };
    match run_case(&req, &mut runner, &mut |_| {}) {
        Ok(outcome) => {
            let _ = std::io::Write::write_all(&mut std::io::stdout(), &outcome.report);
            let _ = std::io::Write::write_all(&mut std::io::stdout(), b"\n");
            if outcome.status == "ok" || outcome.status == "dry_run" {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(code) => {
            eprintln!("{code}");
            ExitCode::from(1)
        }
    }
}

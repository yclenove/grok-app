//! `s11-mailbox-probe host-run` and `guest`.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use s11_mailbox_probe::{guest_entry, real_path_supported, run_case, Case, Deadline, RunRequest};

fn main() -> ExitCode {
    set_umask();
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

fn set_umask() {
    unsafe {
        umask(0o077);
    }
}

extern "C" {
    fn umask(mask: u32) -> u32;
}

fn usage() -> &'static str {
    "s11-mailbox-probe host-run --case C1|C2|C8 --out DIR [--tart PATH] [--tart-home DIR] \
[--pinned-host-key FILE] [--dry-run]\n\
s11-mailbox-probe guest C1|C2|C8\n\
macOS only. The owner sends one notice before a real DEFAULT-network run."
}

fn host(args: &[String]) -> ExitCode {
    let parsed = match parse_host(args) {
        Ok(parsed) => parsed,
        Err(()) => {
            eprintln!("{}", usage());
            return ExitCode::from(64);
        }
    };
    if !real_path_supported() {
        eprintln!("unsupported");
        return ExitCode::from(2);
    }
    if !parsed.dry_run && parsed.pinned.is_none() {
        eprintln!("pinned-host-key");
        return ExitCode::from(64);
    }
    let probe = if parsed.dry_run {
        Vec::new()
    } else {
        match read_probe() {
            Ok(bytes) => bytes,
            Err(err) => {
                eprintln!("{err}");
                return ExitCode::from(1);
            }
        }
    };
    let request = RunRequest {
        case: parsed.case,
        tart: parsed.tart,
        home: parsed.home,
        out: parsed.out,
        tart_home: parsed.tart_home,
        dry_run: parsed.dry_run,
        pinned: parsed.pinned,
        probe,
        attempt: None,
        share: None,
        deadline: Deadline::fresh(),
    };
    let mut runner = s11_mailbox_probe::ProcessRunner::new();
    match run_case(&request, &mut runner) {
        Ok(out) => {
            println!("{}", out.report);
            code_for(&out.verdict)
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}

fn code_for(verdict: &str) -> ExitCode {
    match verdict {
        "ok" | "not-ack" | "dry_run" => ExitCode::SUCCESS,
        _ => ExitCode::from(1),
    }
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
    ExitCode::from(guest_entry(case) as u8)
}

fn read_probe() -> Result<Vec<u8>, &'static str> {
    let path = env::current_exe().map_err(|_| "probe")?;
    let bytes = fs::read(&path).map_err(|_| "probe")?;
    if bytes.is_empty() || bytes.len() > 8 * 1024 * 1024 {
        return Err("probe");
    }
    Ok(bytes)
}

struct Parsed {
    case: Case,
    out: PathBuf,
    tart: PathBuf,
    tart_home: Option<PathBuf>,
    dry_run: bool,
    pinned: Option<PathBuf>,
    home: PathBuf,
}

fn parse_host(args: &[String]) -> Result<Parsed, ()> {
    let mut case = None;
    let mut out = None;
    let mut tart = None;
    let mut tart_home = None;
    let mut pinned = None;
    let mut dry = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--case" => {
                case = Case::parse(args.get(i + 1).ok_or(())?);
                if case.is_none() {
                    return Err(());
                }
                i += 2;
            }
            "--out" => {
                out = Some(PathBuf::from(args.get(i + 1).ok_or(())?));
                i += 2;
            }
            "--tart" => {
                tart = Some(PathBuf::from(args.get(i + 1).ok_or(())?));
                i += 2;
            }
            "--tart-home" => {
                tart_home = Some(PathBuf::from(args.get(i + 1).ok_or(())?));
                i += 2;
            }
            "--pinned-host-key" => {
                pinned = Some(PathBuf::from(args.get(i + 1).ok_or(())?));
                i += 2;
            }
            "--dry-run" => {
                dry = true;
                i += 1;
            }
            _ => return Err(()),
        }
    }
    let home = PathBuf::from(env::var_os("HOME").ok_or(())?);
    let tart = tart.unwrap_or_else(|| home.join("bin/tart"));
    Ok(Parsed {
        case: case.ok_or(())?,
        out: out.ok_or(())?,
        tart,
        tart_home,
        dry_run: dry,
        pinned,
        home,
    })
}

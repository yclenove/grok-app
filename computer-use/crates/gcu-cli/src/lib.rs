//! Computer-use command line.
//!
//! [`install`] registers the `grok-computer-use`
//! MCP server with supported clients and removes that registration again.

#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod approval;
mod args;
mod assemble;
mod call;
mod config;
mod doctor;
mod engine;
mod engine_browser;
pub mod exit;
pub mod helper;
mod host;
pub mod install;
mod install_cmd;
pub mod native_host;
#[cfg(all(feature = "native-wayland", target_os = "linux"))]
mod native_wayland;
mod native_windows;
#[cfg(all(feature = "native-x11", target_os = "linux"))]
mod native_x11;
mod private;
mod proxy;
pub mod resume;
mod runtime;
mod scope;
mod script_children;
mod script_worker;
mod turn_ended;
pub mod ui;
mod worker;

/// Process entry used by the `gcu` binary.
///
/// Parses arguments and runs the selected command until completion/stdio EOF.
/// Native work caps at five seconds; interactive resume at 120 seconds. Stop
/// runs separately from action queues. Panics/unknown failures exit with 10,
/// except the nonfatal turn-ended hook. Payloads, credentials and panic details
/// are never printed.
pub fn cli_main() {
    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    // macOS "Quit & Reopen" (after a Screen Recording grant) and a double click start the
    // bundle with no arguments. Inside the helper app that means "run the helper".
    #[cfg(target_os = "macos")]
    if args.is_empty() && helper::auth::own_app_path().is_some() {
        args.push("agent".into());
    }
    if args.first().is_some_and(|arg| arg == "turn-ended") {
        turn_ended::entry(args.into_iter().skip(1).collect());
    }
    // AppKit must exist on the main thread before Tokio. `open --args agent`
    // puts `agent` in argv. A `--scene agent` false positive only initializes
    // AppKit; it does not grant TCC.
    #[cfg(target_os = "macos")]
    if args.iter().any(|arg| arg == "agent") && !gcu_driver_macos::input::ensure_agent_appkit() {
        eprintln!("Cannot start the helper application");
        std::process::exit(10);
    }
    // The wry loop must run on this thread before Tokio. `--driver fake` never
    // opens it. Dismiss falls through and the helper serves. Restart has already
    // spawned the waiter; exiting lets that waiter `exec open`. Other systems
    // return `Serve` and do not open a window, so this call stays in the build.
    if ui::argv_requests_onboarding(&args) {
        match ui::prepare_agent_onboarding() {
            Ok(ui::AgentStart::Serve) => {}
            Ok(ui::AgentStart::Restarted) => std::process::exit(0),
            Err(error) => {
                eprintln!("{}", error.message);
                std::process::exit(i32::from(error.code));
            }
        }
    }
    use clap::Parser;
    std::panic::set_hook(Box::new(|_| {
        eprintln!("Internal panic; operation was not replayed")
    }));
    let result = std::panic::catch_unwind(|| {
        args::approval_mode()?;
        // Chrome/Edge native manifests take an executable path and no arguments.
        // Browsers launch `gcu <origin> [--parent-window=N]`; route that launch to
        // the same parser/validation as explicit `gcu native-host <origin>`.
        let argv0 = std::env::args_os().next().unwrap_or_default();
        let mut argv: Vec<_> = std::iter::once(argv0).chain(args.iter().cloned()).collect();
        if argv
            .get(1)
            .and_then(|a| a.to_str())
            .is_some_and(|a| a.starts_with("chrome-extension://"))
        {
            argv.insert(1, "native-host".into());
        }
        let cli = args::Cli::parse_from(argv);
        let _ = tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_ansi(false)
            .without_time()
            .try_init();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| exit::Failure::internal("Cannot start CLI runtime"))?;
        let result = runtime.block_on(runtime::run(cli));
        runtime.shutdown_timeout(std::time::Duration::from_secs(1));
        result
    });
    let code = match result {
        Ok(Ok(code)) => code,
        Ok(Err(error)) => {
            eprintln!("{}", error.message);
            error.code
        }
        Err(_) => 10,
    };
    std::process::exit(i32::from(code));
}

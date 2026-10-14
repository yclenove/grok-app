//! Computer-use command line.
//!
//! [`install`] registers the `grok-computer-use`
//! MCP server with supported clients and removes that registration again.

#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod args;
mod call;
mod config;
mod doctor;
mod engine;
pub mod exit;
mod host;
pub mod install;
mod private;
mod proxy;
pub mod resume;
mod runtime;

/// Process entry used by the `gcu` binary.
///
/// Parses arguments and runs the selected command until completion/stdio EOF.
/// Native work caps at five seconds; interactive resume at 120 seconds. Stop
/// runs separately from action queues. Panics/unknown failures exit with 10;
/// payloads, credentials and panic details are never printed.
pub fn cli_main() {
    use clap::Parser;
    std::panic::set_hook(Box::new(|_| {
        eprintln!("Internal panic; operation was not replayed")
    }));
    let result = std::panic::catch_unwind(|| {
        args::approval_mode()?;
        let cli = args::Cli::parse();
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

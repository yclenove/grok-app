//! CLI syntax and build/environment gates, independent of driver assembly.
use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "gcu", about = "Grok Computer Use", version)]
pub(crate) struct Cli {
    #[command(flatten)]
    pub driver: DriverArgs,
    #[command(subcommand)]
    pub command: Option<Command>,
}
#[derive(Args)]
pub(crate) struct DriverArgs {
    /// Only the offline fake driver is assembled in this build.
    #[arg(long, global = true, value_enum)]
    pub driver: Option<DriverKind>,
    /// Built-in offline desktop scene.
    #[arg(long, global = true, default_value = "form")]
    pub scene: String,
}
#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum DriverKind {
    Fake,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    /// Serve MCP over stdio or authenticated loopback HTTP; proxy an engine endpoint.
    Mcp {
        #[arg(long, value_enum, default_value = "stdio")]
        transport: Transport,
        #[arg(long, value_enum, default_value = "default")]
        profile: Profile,
        #[arg(long)]
        connect: Option<String>,
        #[arg(long)]
        token_fd: Option<usize>,
        #[arg(long)]
        port: Option<u16>,
        /// Idle fallback for clients without a turn hook (architecture §8: 45s).
        #[arg(long, default_value = "45", value_parser = clap::value_parser!(u64).range(1..))]
        turn_idle_secs: u64,
    },
    /// Run an App-managed engine over private inherited Host and Stop channels.
    Engine {
        #[arg(
            long,
            conflicts_with = "host_socket",
            required_unless_present = "host_socket"
        )]
        host_fd: Option<usize>,
        #[arg(long, requires = "host_token_fd", conflicts_with = "host_fd")]
        host_socket: Option<std::path::PathBuf>,
        #[arg(long, requires = "host_socket")]
        host_token_fd: Option<usize>,
        #[arg(long)]
        stop_fd: usize,
    },
    /// Retire Turn authorization. Hook errors print one line and exit 0.
    #[command(
        long_about = "Retire Turn approvals without clearing Stop. Reads Grok Stop/end_turn JSON from stdin or Codex agent-turn-complete JSON from argv. Uses the existing private resume endpoint. Multiple sessions require --session; never broadcasts. Hook errors print a redacted stderr line and exit 0. --previous-notify forwards the original argv and stdin first, and preserves its exit status. Other clients use notifications/turn-ended or the 45-second idle fallback."
    )]
    TurnEnded {
        /// Existing private control directory; required if several sessions run.
        #[arg(long)]
        session: Option<std::path::PathBuf>,
        /// Previous command and appended JSON, or the notify JSON alone.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        payload: Vec<std::ffi::OsString>,
    },
    /// Execute one tool in process. Images remain in a private temporary session.
    Call {
        tool: String,
        #[arg(long, default_value = "{}")]
        args: String,
        #[arg(long)]
        json: bool,
    },
    /// Show permissions, capabilities, versions and metadata-only diagnostics.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Print the CLI version.
    Version,
    /// Explicitly resume a stopped MCP session from an interactive terminal.
    #[command(
        long_about = "Explicitly resume a stopped MCP session. Both stdin and stdout must be interactive TTYs; manually enter the displayed confirmation. Token authentication and TTY checks are best effort: a model with shell access under the same UID can forge a TTY. Trusted resume requires the overlay button, App panel or hotkey (P4-04/P5). Resume is never an MCP tool."
    )]
    Resume {
        /// Private session directory printed at MCP startup; required if several run.
        #[arg(long)]
        session: Option<std::path::PathBuf>,
    },
}
#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum Transport {
    Stdio,
    Http,
}
#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum Profile {
    Default,
    Minimal,
    Full,
}
impl From<Profile> for gcu_mcp::Profile {
    fn from(p: Profile) -> Self {
        match p {
            Profile::Default => Self::Default,
            Profile::Minimal => Self::Minimal,
            Profile::Full => Self::Full,
        }
    }
}

pub(crate) fn approval_mode() -> Result<Option<String>, crate::exit::Failure> {
    match std::env::var("GCU_APPROVAL") {
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(_) => Err(crate::exit::Failure::arguments(
            "GCU_APPROVAL must be valid text",
        )),
        Ok(value) => {
            if !cfg!(all(feature = "eval-autoapprove", debug_assertions)) {
                return Err(crate::exit::Failure::arguments(
                    "GCU_APPROVAL is forbidden in release or non eval-autoapprove builds",
                ));
            }
            if !matches!(
                value.as_str(),
                "auto-allow-turn" | "auto-allow-session" | "auto-allow-always" | "auto-deny"
            ) {
                return Err(crate::exit::Failure::arguments("Unknown GCU_APPROVAL mode"));
            }
            Ok(Some(value))
        }
    }
}
pub(crate) fn require_fake(args: &DriverArgs) -> Result<(), crate::exit::Failure> {
    if args.driver.is_none() {
        return Err(crate::exit::Failure::arguments("Real driver assembly is not implemented; P4. Explicit --driver fake is required for offline use."));
    }
    if !cfg!(debug_assertions)
        && std::env::var_os("GCU_ALLOW_FAKE").as_deref() != Some(std::ffi::OsStr::new("1"))
    {
        return Err(crate::exit::Failure::arguments(
            "Fake driver requires a debug build or GCU_ALLOW_FAKE=1",
        ));
    }
    Ok(())
}

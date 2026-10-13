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
    /// Serve MCP over stdio; HTTP and proxy assembly arrive in P0-12.
    Mcp {
        #[arg(long, value_enum, default_value = "stdio")]
        transport: Transport,
        #[arg(long, value_enum, default_value = "default")]
        profile: Profile,
        #[arg(long)]
        connect: Option<String>,
        #[arg(long)]
        token_fd: Option<u32>,
        #[arg(long)]
        port: Option<u16>,
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

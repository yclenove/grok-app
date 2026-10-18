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
    /// Chrome/Edge private native messaging: length-prefixed JSON, no MCP tools.
    NativeHost {
        /// Browser-supplied origin, checked against the fixed manifest public key.
        origin: String,
        /// Browser launch metadata; never used as window identity or authorization.
        #[arg(long)]
        parent_window: Option<u64>,
    },
    /// Private native worker; framed stdin/stdout, never an MCP tool.
    #[command(hide = true)]
    Worker,
    /// Sandboxed script worker. Narrow frames on stdin/stdout, never an MCP tool.
    #[command(hide = true, name = "script-worker")]
    ScriptWorker,
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
    /// macOS helper. Launch Services starts it with `open`; it is not an MCP client.
    ///
    /// The process serves the per-user helper socket. Accessibility and screen
    /// recording are attributed to this app, not to the terminal that invoked `open`.
    Agent,
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
    /// List or revoke one persistent approval.
    #[command(
        long_about = "List or revoke one persistent approval. There is no command that grants every application."
    )]
    Approvals(ApprovalArgs),
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
    /// Register grok-computer-use with one MCP client.
    #[command(
        long_about = "Register grok-computer-use. Pass the client positionally (`gcu install grok`) or as `--client grok`. Writing the Grok directory requires a typed TTY confirmation or --yes. A pipe or an environment variable cannot confirm."
    )]
    Install(InstallArgs),
    /// Remove grok-computer-use and hooks this installer owns.
    #[command(
        long_about = "Remove only grok-computer-use and hooks this installer owns. Pass the client positionally or as `--client`. Writing the Grok directory requires a typed TTY confirmation or --yes."
    )]
    Uninstall(InstallArgs),
}

#[derive(Args, Clone)]
pub(crate) struct ApprovalArgs {
    #[command(subcommand)]
    pub action: ApprovalAction,
}

#[derive(Subcommand, Clone)]
pub(crate) enum ApprovalAction {
    /// List persistent approvals.
    List {
        /// Print stored records as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Remove one persistent approval by the key printed by list.
    Revoke {
        /// Storage key from `gcu approvals list`.
        key: String,
    },
}

#[derive(Args, Clone)]
pub(crate) struct InstallArgs {
    /// Client name. Same values as `--client`.
    #[arg(value_enum, value_name = "CLIENT", id = "client_pos")]
    pub client_pos: Option<ClientName>,
    /// Client name. Same values as the positional client.
    #[arg(long = "client", value_enum)]
    pub client: Option<ClientName>,
    /// Where the MCP entry is written.
    #[arg(long, value_enum, default_value = "user")]
    pub scope: ScopeName,
    /// Print the diff and run nothing.
    #[arg(long)]
    pub dry_run: bool,
    /// Modify the Grok directory without a prompt. Environment variables cannot replace this flag.
    #[arg(long)]
    pub yes: bool,
    /// Print a JSON object instead of the text diff.
    #[arg(long)]
    pub json: bool,
    /// Home that contains client config. Overrides `GCU_HOME`.
    #[arg(long)]
    pub home: Option<std::path::PathBuf>,
    /// Project directory for `--scope project`. Overrides `GCU_PROJECT`.
    #[arg(long)]
    pub project: Option<std::path::PathBuf>,
    /// Absolute `gcu` path stored in the client config. Default: this executable.
    #[arg(long)]
    pub gcu: Option<std::path::PathBuf>,
    /// `agy` executable for Antigravity user scope. Default: search `PATH`.
    #[arg(long)]
    pub agy: Option<std::path::PathBuf>,
    /// Grok config directory when it is not `<home>/.grok`.
    #[arg(long)]
    pub grok_home: Option<std::path::PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum ClientName {
    #[value(name = "grok")]
    Grok,
    #[value(name = "codex")]
    Codex,
    #[value(name = "claude")]
    Claude,
    #[value(name = "antigravity")]
    Antigravity,
    #[value(name = "gemini")]
    Gemini,
    #[value(name = "opencode")]
    Opencode,
    #[value(name = "cursor")]
    Cursor,
    #[value(name = "vscode")]
    VsCode,
    #[value(name = "chrome-extension")]
    ChromeExtension,
}

impl ClientName {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Grok => "grok",
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Antigravity => "antigravity",
            Self::Gemini => "gemini",
            Self::Opencode => "opencode",
            Self::Cursor => "cursor",
            Self::VsCode => "vscode",
            Self::ChromeExtension => "chrome-extension",
        }
    }
}

impl TryFrom<ClientName> for crate::install::ClientId {
    type Error = &'static str;
    fn try_from(client: ClientName) -> Result<Self, Self::Error> {
        Ok(match client {
            ClientName::Grok => Self::Grok,
            ClientName::Codex => Self::Codex,
            ClientName::Claude => Self::Claude,
            ClientName::Antigravity => Self::Antigravity,
            ClientName::Gemini => Self::Gemini,
            ClientName::Opencode => Self::Opencode,
            ClientName::Cursor => Self::Cursor,
            ClientName::VsCode => Self::VsCode,
            ClientName::ChromeExtension => {
                return Err("Use the native host installer for chrome-extension")
            }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum ScopeName {
    #[value(name = "user")]
    User,
    #[value(name = "project")]
    Project,
}

impl ScopeName {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
        }
    }
}

impl From<ScopeName> for crate::install::Scope {
    fn from(scope: ScopeName) -> Self {
        match scope {
            ScopeName::User => Self::User,
            ScopeName::Project => Self::Project,
        }
    }
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

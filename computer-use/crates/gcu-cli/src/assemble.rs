//! Single driver selection. Runtime, engine, and helper do not guess.
//!
//! `auto` and `native` fail closed unless the platform route is assembled.
//! Linux X11 is assembled only when `native-x11` is compiled. This build never
//! substitutes fake, never widens the input route, and never elevates. The
//! feature stays off unless a binary enables it.

use crate::args::{DriverArgs, DriverKind};
use crate::exit::Failure;

/// Which command is asking for a driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    /// `gcu agent`.
    Agent,
    /// In-process or proxied `gcu mcp` over stdio.
    McpStdio,
    /// No subcommand.
    Bare,
    /// `call`, `doctor`, `engine`, and HTTP MCP.
    InProcess,
}

/// Host operating system as seen by selection. Tests inject this.
///
/// Each target constructs only its own variant. The others stay for tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum OsKind {
    /// Windows.
    Windows,
    /// macOS.
    Macos,
    /// Linux, classified without treating `DISPLAY` as pure X11.
    Linux {
        /// Session kind.
        session: SessionKind,
    },
}

/// Linux display session. `Wayland` wins whenever `WAYLAND_DISPLAY` is set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum SessionKind {
    /// `DISPLAY` only.
    X11,
    /// Wayland, including a session that also has `DISPLAY`.
    Wayland {
        /// `DISPLAY` is also set. It does not select the X11 route.
        display: bool,
    },
    /// Neither variable is usable.
    Headless,
}

/// Compile-time delivery switches. Production reads `cfg`; tests inject them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Features {
    /// `native-windows`.
    pub native_windows: bool,
    /// `native-x11`.
    pub native_x11: bool,
    /// `native-wayland`.
    pub native_wayland: bool,
}

impl Features {
    /// Flags compiled into this binary. All three default off.
    #[must_use]
    pub(crate) const fn compiled() -> Self {
        Self {
            native_windows: cfg!(feature = "native-windows"),
            native_x11: cfg!(feature = "native-x11"),
            native_wayland: cfg!(feature = "native-wayland"),
        }
    }
}

/// What doctor, capabilities, and instructions report together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Disclosure {
    /// `fake` or `native`.
    pub selected: &'static str,
    /// Why the native route is not running.
    pub native_disabled: String,
}

impl Disclosure {
    /// One sentence shared by doctor and initialize instructions.
    #[must_use]
    pub(crate) fn note(&self) -> String {
        format!(
            "Route: {}. Native disabled: {}.",
            self.selected, self.native_disabled
        )
    }
}

/// Selection result. [`Plan::Fake`] and [`Plan::NativeX11`] may construct a driver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Plan {
    /// Offline driver after the fake gate.
    Fake {
        /// Scene name, default `form`.
        scene: String,
        /// Shared disclosure.
        disclosure: Disclosure,
    },
    /// macOS stdio MCP stays on the existing helper proxy.
    HelperProxy {
        /// Shared disclosure.
        disclosure: Disclosure,
    },
    /// macOS `agent` keeps the existing helper process.
    HelperAgent {
        /// Shared disclosure.
        disclosure: Disclosure,
    },
    /// macOS with no subcommand keeps onboarding.
    Onboarding {
        /// Shared disclosure.
        disclosure: Disclosure,
    },
    /// Argument error, exit 2. No driver is constructed.
    Refuse {
        /// Stable parameter error.
        message: String,
    },
    /// In-process Linux X11 driver. The listener is bound to the Host latch later.
    NativeX11 {
        /// Shared disclosure. Wayland stays disabled.
        disclosure: Disclosure,
    },
}

/// Inputs for [`decide`]. Environment is passed in so tests do not touch it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Query<'a> {
    /// Parsed `--driver`, or auto when omitted.
    pub request: DriverKind,
    /// `Some` only when the user passed `--scene`.
    pub scene: Option<&'a str>,
    /// Command class.
    pub entry: Entry,
    /// Operating system.
    pub os: OsKind,
    /// Compiled features.
    pub features: Features,
    /// `debug_assertions`.
    pub debug: bool,
    /// `GCU_ALLOW_FAKE=1`.
    pub allow_fake: bool,
}

/// Current process gates. Does not read `GCU_DRIVER`.
#[must_use]
pub(crate) fn production(args: &DriverArgs, entry: Entry) -> Query<'_> {
    Query {
        request: args.driver.unwrap_or(DriverKind::Auto),
        scene: args.scene.as_deref(),
        entry,
        os: current_os(),
        features: Features::compiled(),
        debug: cfg!(debug_assertions),
        allow_fake: std::env::var_os("GCU_ALLOW_FAKE").as_deref()
            == Some(std::ffi::OsStr::new("1")),
    }
}

/// Choose a plan. Pure aside from the data in `query`. Never constructs a driver.
#[must_use]
pub(crate) fn decide(query: Query<'_>) -> Plan {
    if query.scene.is_some() && query.request != DriverKind::Fake {
        return Plan::Refuse {
            message: "--scene is valid only with --driver fake".into(),
        };
    }
    if query.request == DriverKind::Fake {
        if !query.debug && !query.allow_fake {
            return Plan::Refuse {
                message: "Fake driver requires a debug build or GCU_ALLOW_FAKE=1".into(),
            };
        }
        return Plan::Fake {
            scene: query.scene.unwrap_or("form").to_owned(),
            disclosure: Disclosure {
                selected: "fake",
                native_disabled: native_disabled(query.os, query.features),
            },
        };
    }
    if query.os == OsKind::Macos {
        let disclosure = Disclosure {
            selected: "native",
            native_disabled: native_disabled(query.os, query.features),
        };
        return match query.entry {
            Entry::McpStdio => Plan::HelperProxy { disclosure },
            Entry::Agent => Plan::HelperAgent { disclosure },
            Entry::Bare => Plan::Onboarding { disclosure },
            Entry::InProcess => Plan::Refuse {
                message: "This entry does not assemble a native driver; use the helper or the App"
                    .into(),
            },
        };
    }
    if matches!(
        query.os,
        OsKind::Linux {
            session: SessionKind::X11
        }
    ) && query.features.native_x11
    {
        if query.entry == Entry::Agent {
            return Plan::Refuse {
                message: "Linux native X11 does not use the macOS helper agent entry".into(),
            };
        }
        return Plan::NativeX11 {
            disclosure: Disclosure {
                selected: "native",
                native_disabled: format!(
                    "selected native-x11; {}",
                    block("native-wayland", query.features.native_wayland)
                ),
            },
        };
    }
    Plan::Refuse {
        message: native_disabled(query.os, query.features),
    }
}

/// Reject `--scene` on auto/native before a command that does not start a driver.
pub(crate) fn reject_unused_scene(args: &DriverArgs) -> Result<(), Failure> {
    let request = args.driver.unwrap_or(DriverKind::Auto);
    if args.scene.is_some() && request != DriverKind::Fake {
        return Err(Failure::arguments(
            "--scene is valid only with --driver fake",
        ));
    }
    Ok(())
}

fn native_disabled(os: OsKind, features: Features) -> String {
    match os {
        OsKind::Windows => block("native-windows", features.native_windows),
        OsKind::Macos => {
            "macOS native assembly is not enabled in this build; use the helper or the App".into()
        }
        OsKind::Linux {
            session: SessionKind::X11,
        } => {
            if features.native_x11 {
                format!(
                    "native-x11 is compiled; this command did not select it. {}",
                    block("native-wayland", features.native_wayland)
                )
            } else {
                block("native-x11", false)
            }
        }
        OsKind::Linux {
            session: SessionKind::Wayland { display: true },
        } => format!(
            "Wayland is active, so the X11 route is not selected; {}",
            block("native-wayland", features.native_wayland)
        ),
        OsKind::Linux {
            session: SessionKind::Wayland { display: false },
        } => block("native-wayland", features.native_wayland),
        OsKind::Linux {
            session: SessionKind::Headless,
        } => "No safe native route is available for this display session".into(),
    }
}

fn block(feature: &str, compiled: bool) -> String {
    if compiled {
        format!("{feature} is compiled but the platform route is not assembled")
    } else {
        format!("{feature} is not compiled into this build")
    }
}

fn current_os() -> OsKind {
    #[cfg(windows)]
    {
        OsKind::Windows
    }
    #[cfg(target_os = "macos")]
    {
        OsKind::Macos
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        OsKind::Linux {
            session: linux_session(),
        }
    }
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn linux_session() -> SessionKind {
    let wayland = present("WAYLAND_DISPLAY");
    let display = present("DISPLAY");
    if wayland {
        SessionKind::Wayland { display }
    } else if display {
        SessionKind::X11
    } else {
        SessionKind::Headless
    }
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn present(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| !value.is_empty())
}

#[cfg(test)]
#[path = "assemble_tests.rs"]
mod tests;

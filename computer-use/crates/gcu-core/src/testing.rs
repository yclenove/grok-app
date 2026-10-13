//! Reusable driver-level contract suite. Factories create a fresh fixture per case.
//! Router snapshot gates, tree budgets, and real OS occlusion are later-layer checks;
//! this suite never claims that a fake fixture proves real OS behavior.

use crate::{driver::*, model::*};
use async_trait::async_trait;
use gcu_protocol::capabilities::Capabilities;

/// Portable hooks implemented by memory and native fixture adapters.
#[async_trait]
pub trait ContractFixture: Send + Sync {
    /// Driver under test.
    fn driver(&self) -> &dyn Driver;
    /// Discoverable fixture app, by display name.
    fn app_query(&self) -> AppQuery;
    /// App containing a 2000-row list.
    fn large_app_query(&self) -> AppQuery;
    /// Read external fixture state, independent of driver observations.
    async fn oracle(&self) -> Result<ContractOracle>;
    /// Inject one reproducible test fault (never against user data).
    async fn inject(&self, fault: ContractFault) -> Result<()>;
}
/// Minimal independent oracle used by the shared suite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractOracle {
    /// Number of successful submit presses.
    pub counter: u64,
    /// Fixture's current text input.
    pub text: String,
    /// Foreground window before/after background delivery.
    pub foreground: Option<String>,
    /// Synthetic keys held by the driver.
    pub held_input: usize,
}
/// Fixture-only injection requests, never exposed through product tools.
#[derive(Debug, Clone)]
pub enum ContractFault {
    /// Timeout once, optionally after execution.
    Timeout {
        /// Whether the fixture should execute before failing.
        maybe_executed: bool,
    },
    /// Crash after one delivered chord, leaving synthetic input held.
    InterruptedInput,
    /// Delete one exact node.
    Delete {
        /// Exact window.
        win: WindowRef,
        /// Fixture node identifier.
        identifier: String,
    },
    /// Close one exact window.
    Close {
        /// Exact window.
        win: WindowRef,
    },
}
/// Required capability used both to select and to validate skips.
#[derive(Debug, Clone, Copy)]
pub enum Requirement {
    /// Discovery is mandatory on every Driver.
    Discovery,
    /// Raw accessibility observations.
    Tree,
    /// Window screenshots.
    Screenshot,
    /// Accessibility actions.
    Accessibility,
    /// Window-targeted pointer delivery.
    TargetedPointer,
    /// Window-targeted key delivery.
    TargetedKeys,
}
impl Requirement {
    /// Match the driver's current declaration; skip decisions use this same predicate.
    pub fn supported(self, caps: &Capabilities) -> bool {
        match self {
            Self::Discovery => true,
            Self::Tree => caps.observe.tree,
            Self::Screenshot => caps.observe.screenshot,
            Self::Accessibility => caps.delivery.accessibility,
            Self::TargetedPointer => caps.delivery.targeted_pointer,
            Self::TargetedKeys => caps.delivery.targeted_keys,
        }
    }
    fn reason(self) -> &'static str {
        match self {
            Self::Discovery => "discovery is mandatory",
            Self::Tree => "capability: observe.tree=false",
            Self::Screenshot => "capability: observe.screenshot=false",
            Self::Accessibility => "capability: delivery.accessibility=false",
            Self::TargetedPointer => "capability: delivery.targeted_pointer=false",
            Self::TargetedKeys => "capability: delivery.targeted_keys=false",
        }
    }
}
/// Contract table row; explicit scope skips cannot be mistaken for capability claims.
#[derive(Debug, Clone, Copy)]
pub struct Case {
    /// Quality-gate ID from 07 §2.
    pub id: &'static str,
    /// Driver-level assertion covered now.
    pub assertion: &'static str,
    /// Required capability.
    pub requires: Requirement,
    /// Explicit skip for semantics outside the Driver boundary.
    pub skip: Option<&'static str>,
}
/// C01–C09, C17–C19, C22–C24 driver-level checks. Native fixture adapters must
/// extend C04 with actual occlusion/grid pixel oracles and C23 with physical input.
pub const CASES: &[Case] = &[
    Case { id: "C01", assertion: "discover and resolve name/ID/path", requires: Requirement::Discovery, skip: None },
    Case { id: "C02", assertion: "launch without foreground activation", requires: Requirement::Discovery, skip: None },
    Case { id: "C03", assertion: "raw fixture controls retain roles", requires: Requirement::Tree, skip: None },
    Case { id: "C04", assertion: "capture matches physical window dimensions", requires: Requirement::Screenshot, skip: None },
    Case { id: "C05", assertion: "press is delivered exactly once", requires: Requirement::Accessibility, skip: None },
    Case { id: "C06", assertion: "window-pixel click hits exact button", requires: Requirement::TargetedPointer, skip: None },
    Case { id: "C07", assertion: "set_value replaces text", requires: Requirement::Accessibility, skip: None },
    Case { id: "C08", assertion: "Unicode/ZWJ/newline input is byte exact", requires: Requirement::TargetedKeys, skip: None },
    Case { id: "C09", assertion: "select all then BackSpace clears text", requires: Requirement::TargetedKeys, skip: None },
    Case { id: "C17", assertion: "old screenshot coordinate gate", requires: Requirement::TargetedPointer, skip: Some("scope: ActionRouter stale_geometry + fresh state (P0-08); Driver has no snapshot ID") },
    Case { id: "C18", assertion: "deleted native handle fails without substitution", requires: Requirement::Accessibility, skip: None },
    Case { id: "C19", assertion: "closed window fails without substitution", requires: Requirement::TargetedKeys, skip: None },
    Case { id: "C22", assertion: "uncertain timeout does not replay", requires: Requirement::Accessibility, skip: None },
    Case { id: "C23", assertion: "release clears interrupted synthetic input ledger", requires: Requirement::TargetedKeys, skip: None },
    Case { id: "C24", assertion: "2000-row get_app_state budget/truncation", requires: Requirement::Tree, skip: Some("scope: TreePipeline + Renderer budget/truncation (P0-06); raw reads must preserve 2000 rows") },
    Case { id: "C04-native", assertion: "real OS occlusion and compositor marker pixels", requires: Requirement::Screenshot, skip: Some("scope: real OS compositor fixture oracle belongs to platform-driver cards") },
    Case { id: "C23-native", assertion: "real OS key release after interrupted shift", requires: Requirement::TargetedKeys, skip: Some("scope: physical input fixture oracle belongs to platform-driver cards") },
];
/// Explicit execution outcome; callers must distinguish skipped from passed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaseOutcome {
    /// Driver-level assertions passed.
    Passed,
    /// Unsupported capability or later-layer semantics, with stable reason.
    Skipped(&'static str),
}
impl Case {
    /// Select a skip from actual capabilities, preserving table scope registration.
    pub fn skip_reason(&self, caps: &Capabilities) -> Option<&'static str> {
        if !self.requires.supported(caps) {
            Some(self.requires.reason())
        } else {
            self.skip
        }
    }
}
fn node<'a>(root: &'a RawNode, id: &str) -> Option<&'a RawNode> {
    if root.identifier.as_deref() == Some(id) {
        Some(root)
    } else {
        root.children.iter().find_map(|n| node(n, id))
    }
}
fn require(condition: bool, message: &str) -> std::result::Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
fn diagnostic(error: DriverError) -> String {
    error.to_string()
}
async fn window(fixture: &dyn ContractFixture) -> std::result::Result<WindowRef, String> {
    let driver = fixture.driver();
    let app = driver
        .resolve_app(&fixture.app_query())
        .await
        .map_err(diagnostic)?;
    let info = driver.key_window(&app).await.map_err(diagnostic)?;
    Ok(WindowRef { app, id: info.id })
}
async fn handle(
    fixture: &dyn ContractFixture,
    win: &WindowRef,
    id: &str,
) -> std::result::Result<NativeHandle, String> {
    let tree = fixture
        .driver()
        .snapshot_tree(win, &TreeOpts::default(), &Cancel::default())
        .await
        .map_err(diagnostic)?;
    tree.root
        .as_ref()
        .and_then(|root| node(root, id))
        .map(|n| n.handle.clone())
        .ok_or_else(|| format!("fixture control missing: {id}"))
}
/// Run one fresh-fixture case. Errors identify the failed assertion without product logging.
/// Reads/actions inherit Driver's deadlines; the harness performs no retry.
pub async fn run_case(
    fixture: &dyn ContractFixture,
    id: &str,
) -> std::result::Result<CaseOutcome, String> {
    let case = CASES
        .iter()
        .find(|case| case.id == id)
        .ok_or("unregistered case")?;
    let driver = fixture.driver();
    if let Some(reason) = case.skip_reason(driver.capabilities()) {
        return Ok(CaseOutcome::Skipped(reason));
    }
    let win = window(fixture).await?;
    let cancel = Cancel::default();
    let before = fixture.oracle().await.map_err(diagnostic)?;
    match id {
        "C01" => {
            require(
                driver
                    .list_apps()
                    .await
                    .map_err(diagnostic)?
                    .contains(&win.app.info),
                "fixture missing from app list",
            )?;
            let identity = win
                .app
                .info
                .identity
                .as_ref()
                .ok_or("fixture identity missing")?;
            for query in [
                win.app.info.name.as_str(),
                win.app.info.id.as_str(),
                identity
                    .executable
                    .as_deref()
                    .ok_or("fixture executable missing")?,
            ] {
                require(
                    driver
                        .resolve_app(&AppQuery::from(query))
                        .await
                        .map_err(diagnostic)?
                        == win.app,
                    "resolution changed app",
                )?;
            }
        }
        "C02" => {
            driver.launch_app(&win.app).await.map_err(diagnostic)?;
            require(
                fixture.oracle().await.map_err(diagnostic)?.foreground == before.foreground,
                "launch activated foreground",
            )?;
        }
        "C03" => {
            let tree = driver
                .snapshot_tree(&win, &TreeOpts::default(), &cancel)
                .await
                .map_err(diagnostic)?;
            let root = tree.root.ok_or("fixture root missing")?;
            require(root.role == Role::Window, "root role incorrect")?;
            for (id, role) in [
                ("text", Role::TextField),
                ("submit", Role::Button),
                ("agree", Role::CheckBox),
                ("slider", Role::Slider),
            ] {
                require(
                    node(&root, id).is_some_and(|n| n.role == role),
                    "fixture role missing/incorrect",
                )?;
            }
        }
        "C04" => {
            let geometry = driver.window_geometry(&win).await.map_err(diagnostic)?;
            let capture = driver
                .capture(&win, &CaptureOpts::default(), &cancel)
                .await
                .map_err(diagnostic)?;
            require(
                f64::from(capture.width) == geometry.frame.w
                    && f64::from(capture.height) == geometry.frame.h
                    && capture.geometry == geometry,
                "capture physical geometry mismatch",
            )?;
            require(
                capture.mime == "image/png" && capture.bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
                "capture is not PNG",
            )?;
        }
        "C05" | "C22" => {
            let el = handle(fixture, &win, "submit").await?;
            if id == "C22" {
                fixture
                    .inject(ContractFault::Timeout {
                        maybe_executed: true,
                    })
                    .await
                    .map_err(diagnostic)?;
            }
            let result = driver.perform(&DriverAction::Press { el }, &cancel).await;
            if id == "C22" {
                require(
                    result
                        == Err(DriverError::Timeout {
                            maybe_executed: true,
                        }),
                    "timeout lost uncertainty",
                )?;
            } else {
                require(
                    result.map_err(diagnostic)? == Delivery::Accessibility,
                    "wrong semantic delivery",
                )?;
            }
            require(
                fixture.oracle().await.map_err(diagnostic)?.counter == before.counter + 1,
                "press was lost or replayed",
            )?;
        }
        "C06" => {
            let tree = driver
                .snapshot_tree(&win, &TreeOpts::default(), &cancel)
                .await
                .map_err(diagnostic)?;
            let frame = tree
                .root
                .as_ref()
                .and_then(|root| node(root, "submit"))
                .and_then(|n| n.frame)
                .ok_or("button frame missing")?;
            let result = driver
                .perform(
                    &DriverAction::PointerClick {
                        win,
                        at: WindowPoint {
                            x: frame.x + frame.w / 2.0,
                            y: frame.y + frame.h / 2.0,
                        },
                        button: Button::Left,
                        count: 1,
                        route: PointerRoute::TargetedOnly,
                    },
                    &cancel,
                )
                .await
                .map_err(diagnostic)?;
            require(result == Delivery::TargetedEvent, "pointer route escalated")?;
            require(
                fixture.oracle().await.map_err(diagnostic)?.counter == before.counter + 1,
                "coordinate hit wrong target",
            )?;
        }
        "C07" | "C08" | "C09" => {
            let el = handle(fixture, &win, "text").await?;
            driver
                .perform(
                    &DriverAction::SetValue {
                        el: el.clone(),
                        value: String::new(),
                    },
                    &cancel,
                )
                .await
                .map_err(diagnostic)?;
            let input = "中文 👩‍👩‍👧‍👦\nline";
            if id == "C07" {
                driver
                    .perform(
                        &DriverAction::SetValue {
                            el,
                            value: input.into(),
                        },
                        &cancel,
                    )
                    .await
                    .map_err(diagnostic)?;
            } else {
                driver
                    .perform(
                        &DriverAction::TypeText {
                            win: win.clone(),
                            focus: Some(el),
                            text: input.into(),
                            route: KeyRoute::TargetedOnly,
                        },
                        &cancel,
                    )
                    .await
                    .map_err(diagnostic)?;
            }
            require(
                fixture.oracle().await.map_err(diagnostic)?.text == input,
                "text bytes changed",
            )?;
            if id == "C09" {
                for key in [
                    if driver.platform() == Platform::Macos {
                        "cmd+a"
                    } else {
                        "ctrl+a"
                    },
                    "BackSpace",
                ] {
                    driver
                        .perform(
                            &DriverAction::PressKeys {
                                win: win.clone(),
                                chord: KeyChord::parse(key).map_err(|e| e.to_string())?,
                                route: KeyRoute::TargetedOnly,
                            },
                            &cancel,
                        )
                        .await
                        .map_err(diagnostic)?;
                }
                require(
                    fixture.oracle().await.map_err(diagnostic)?.text.is_empty(),
                    "select-all deletion failed",
                )?;
            }
        }
        "C18" => {
            let el = handle(fixture, &win, "submit").await?;
            fixture
                .inject(ContractFault::Delete {
                    win,
                    identifier: "submit".into(),
                })
                .await
                .map_err(diagnostic)?;
            require(
                driver.perform(&DriverAction::Press { el }, &cancel).await
                    == Err(DriverError::StaleElement),
                "deleted handle substituted or accepted",
            )?;
            require(
                fixture.oracle().await.map_err(diagnostic)?.counter == before.counter,
                "stale action had effects",
            )?;
        }
        "C19" => {
            fixture
                .inject(ContractFault::Close { win: win.clone() })
                .await
                .map_err(diagnostic)?;
            require(
                driver
                    .perform(
                        &DriverAction::TypeText {
                            win,
                            focus: None,
                            text: "forbidden".into(),
                            route: KeyRoute::TargetedOnly,
                        },
                        &cancel,
                    )
                    .await
                    == Err(DriverError::WindowGone),
                "closed window substituted",
            )?;
            require(
                fixture.oracle().await.map_err(diagnostic)?.counter == before.counter,
                "other window received action",
            )?;
        }
        "C23" => {
            fixture
                .inject(ContractFault::InterruptedInput)
                .await
                .map_err(diagnostic)?;
            let result = driver
                .perform(
                    &DriverAction::PressKeys {
                        win,
                        chord: KeyChord::parse("ctrl+a").map_err(|e| e.to_string())?,
                        route: KeyRoute::TargetedOnly,
                    },
                    &cancel,
                )
                .await;
            require(
                result.is_err_and(|error| error.maybe_executed()),
                "interruption lost uncertainty",
            )?;
            require(
                fixture.oracle().await.map_err(diagnostic)?.held_input > 0,
                "fixture did not hold interrupted input",
            )?;
            driver.release_all_input().await;
            driver.release_all_input().await;
            require(
                fixture.oracle().await.map_err(diagnostic)?.held_input == 0,
                "release left held input",
            )?;
        }
        _ => return Err("registered driver case has no implementation".into()),
    }
    Ok(CaseOutcome::Passed)
}

/// Generate independently named Tokio tests, each with a fresh fixture factory.
/// The calling crate needs tokio's macros/rt features and core's testing feature.
#[macro_export]
macro_rules! contract {
    ($factory:expr) => {
        $crate::contract!(@cases $factory; c01:"C01", c02:"C02", c03:"C03", c04:"C04", c05:"C05", c06:"C06", c07:"C07", c08:"C08", c09:"C09", c18:"C18", c19:"C19", c22:"C22", c23:"C23");
        $crate::contract!(@skip $factory; c17:"C17", c24:"C24", c04_native:"C04-native", c23_native:"C23-native");
    };
    (@skip $factory:expr; $($name:ident:$id:literal),+) => { $(
        #[tokio::test]
        #[ignore = "scope: later core/native fixture checks; see CASES for the exact skip reason"]
        async fn $name() {
            let fixture = $factory.unwrap();
            assert!(matches!($crate::testing::run_case(&fixture, $id).await.unwrap(), $crate::testing::CaseOutcome::Skipped(_)));
        }
    )+ };
    (@cases $factory:expr; $($name:ident:$id:literal),+) => { $(
        #[tokio::test]
        async fn $name() {
            let fixture = $factory.unwrap();
            match $crate::testing::run_case(&fixture, $id).await.unwrap() {
                $crate::testing::CaseOutcome::Passed => {},
                $crate::testing::CaseOutcome::Skipped(reason) => eprintln!("skip(reason={reason:?}): {}", $id),
            }
        }
    )+ };
}

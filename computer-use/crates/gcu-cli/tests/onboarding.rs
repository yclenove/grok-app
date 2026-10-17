//! Onboarding page, permission poll, and helper restart. No WebView.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::ffi::OsString;
use std::path::Path;

use gcu_cli::helper::auth::launch_command;
use gcu_cli::ui::{
    apply_ipc, argv_requests_onboarding, binding_name, embed_document, escape_html, export_module,
    js_embed, navigation_allowed, onboarding_page, onboarding_problems, open_settings_allowed,
    perform_restart, relaunch_waiter, settings_url, should_open_onboarding, status_script,
    CardStatus, IpcEffect, OnboardingModel, PermissionKind, PermissionSnapshot, RestartError,
    RestartHost, SettingsOpener, ACCESSIBILITY_SETTINGS_URL, SCREEN_RECORDING_SETTINGS_URL,
};

const BUNDLE: &str = "ai.grok.computer-use.helper";

fn snap(accessibility: bool, screen_recording: bool) -> PermissionSnapshot {
    PermissionSnapshot {
        accessibility,
        screen_recording,
    }
}

fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

struct Host {
    running: bool,
    stay: bool,
    exit_error: bool,
    schedule_error: Option<RestartError>,
    events: Vec<&'static str>,
    argv: Vec<String>,
}

impl Host {
    fn ready() -> Self {
        Self {
            running: true,
            stay: false,
            exit_error: false,
            schedule_error: None,
            events: Vec::new(),
            argv: Vec::new(),
        }
    }
}

impl RestartHost for Host {
    fn running(&self) -> bool {
        self.running
    }

    fn exit_helper(&mut self) -> Result<(), RestartError> {
        self.events.push("exit");
        if self.exit_error {
            return Err(RestartError::ExitFailed);
        }
        if !self.stay {
            self.running = false;
        }
        Ok(())
    }

    fn cancel_exit(&mut self) {
        self.events.push("cancel");
        self.running = true;
    }

    fn schedule(&mut self, _pid: u32, argv: &[String]) -> Result<(), RestartError> {
        self.events.push("schedule");
        self.argv = argv.to_vec();
        match self.schedule_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

#[derive(Default)]
struct Opened {
    urls: Vec<String>,
    allow: bool,
}

impl SettingsOpener for Opened {
    fn open(&mut self, url: &str) -> bool {
        self.urls.push(url.to_string());
        self.allow
    }
}

fn needing_restart() -> OnboardingModel {
    let mut model = OnboardingModel::new(snap(false, false));
    model.sample(snap(false, true));
    model
}

#[test]
fn embed_document_sources_match_shipped_file() {
    let html = include_str!("../../../ui/onboarding.html");
    let css = include_str!("../../../ui/onboarding.css");
    let rendered = embed_document(html, css).unwrap();
    let shipped = include_str!("../src/ui/onboarding.embed.html");
    assert_eq!(rendered, shipped);
    let lower = shipped.to_ascii_lowercase();
    assert!(!shipped.contains("http:"));
    assert!(!shipped.contains("https:"));
    assert!(!lower.contains("<link"));
    assert!(!lower.contains("<script src"));
    assert!(!shipped.contains("url("));
    assert!(!shipped.contains("{{slot:css}}"));
}

#[test]
fn onboarding_page_default_dismiss_leaves_enter_alone() {
    let page = onboarding_page("en", &OnboardingModel::new(snap(false, false)).view());
    assert!(
        onboarding_problems(&page).is_empty(),
        "{:?}",
        onboarding_problems(&page)
    );
    assert!(page.contains("id=\"deny\""));
    assert!(page.contains("autofocus"));
    assert!(page.contains("Not now"));
    assert!(page.contains("Enter is left untouched"));
    assert!(page.contains("id=\"restart\" data-action=\"restart\" hidden"));
    assert!(!page.contains("{{"));
}

#[test]
fn catalog_locales_match_english_keys_and_audit() {
    let problems = gcu_mcp::audit();
    assert!(problems.is_empty(), "{problems:?}");
    for locale in gcu_mcp::LOCALES {
        let page = onboarding_page(locale, &OnboardingModel::new(snap(false, true)).view());
        assert!(
            onboarding_problems(&page).is_empty(),
            "{locale} {:?}",
            onboarding_problems(&page)
        );
        assert!(page.contains("id=\"deny\""));
        assert!(!page.contains("{{"));
        assert!(page.len() > 400, "{locale}");
    }
}

#[test]
fn poll_screen_grant_after_start_needs_restart() {
    let mut model = OnboardingModel::new(snap(false, false));
    assert_eq!(model.view().screen_recording, CardStatus::Missing);
    model.sample(snap(false, true));
    assert_eq!(model.view().screen_recording, CardStatus::NeedsRestart);
    assert!(model.view().restart_enabled);
    assert!(!model.view().ready_to_close);
}

#[test]
fn poll_accessibility_grant_does_not_enable_restart() {
    let mut model = OnboardingModel::new(snap(false, false));
    model.sample(snap(true, false));
    assert_eq!(model.view().accessibility, CardStatus::Granted);
    assert!(!model.view().restart_enabled);
    assert_eq!(model.view().screen_recording, CardStatus::Missing);
}

#[test]
fn poll_screen_present_at_start_stays_granted() {
    let model = OnboardingModel::new(snap(true, true));
    assert_eq!(model.view().screen_recording, CardStatus::Granted);
    assert!(model.view().ready_to_close);
    assert!(!model.view().restart_enabled);
}

#[test]
fn poll_screen_revoked_after_start_is_missing() {
    let mut model = OnboardingModel::new(snap(true, true));
    model.sample(snap(true, false));
    assert_eq!(model.view().screen_recording, CardStatus::Missing);
    assert!(!model.view().restart_enabled);
}

#[test]
fn sample_repeated_snapshot_settles_and_a_change_resets() {
    let mut model = OnboardingModel::new(snap(false, false));
    assert!(!model.settled());
    model.sample(snap(false, false));
    assert!(model.settled());
    model.sample(snap(true, false));
    assert!(!model.settled());
    model.sample(snap(true, false));
    assert!(model.settled());
}

#[test]
fn sample_permission_change_does_not_open_settings() {
    let mut model = OnboardingModel::new(snap(false, false));
    let mut settings = Opened {
        allow: true,
        ..Opened::default()
    };
    model.sample(snap(true, true));
    assert!(settings.urls.is_empty());
    let mut host = Host::ready();
    let effect = apply_ipc(&mut model, "dismiss", BUNDLE, 7, &mut host, &mut settings);
    assert_eq!(effect, IpcEffect::Dismiss);
    assert!(settings.urls.is_empty());
    assert!(host.events.is_empty());
}

#[test]
fn restart_after_exit_schedules_launch_command() {
    let mut model = needing_restart();
    let mut host = Host::ready();
    let mut settings = Opened::default();
    let effect = apply_ipc(&mut model, "restart", BUNDLE, 42, &mut host, &mut settings);
    assert_eq!(effect, IpcEffect::Restarted);
    assert_eq!(host.events, ["exit", "schedule"]);
    assert_eq!(host.argv, launch_command(BUNDLE).unwrap());
    assert!(settings.urls.is_empty());
    assert!(!host.running);
}

#[test]
fn restart_bad_bundle_does_not_touch_host() {
    let mut host = Host::ready();
    let error = perform_restart("bad id", 7, &mut host).unwrap_err();
    assert_eq!(error, RestartError::BadBundle);
    assert!(host.events.is_empty());
}

#[test]
fn restart_pid_zero_does_not_touch_host() {
    let mut host = Host::ready();
    let error = perform_restart(BUNDLE, 0, &mut host).unwrap_err();
    assert_eq!(error, RestartError::BadPid);
    assert!(host.events.is_empty());
}

#[test]
fn restart_exit_failure_does_not_schedule() {
    let mut model = needing_restart();
    let mut host = Host::ready();
    host.exit_error = true;
    let error = model.restart(BUNDLE, 7, &mut host).unwrap_err();
    assert_eq!(error, RestartError::ExitFailed);
    assert_eq!(host.events, ["exit"]);
}

#[test]
fn restart_still_running_does_not_schedule() {
    let mut model = needing_restart();
    let mut host = Host::ready();
    host.stay = true;
    let error = model.restart(BUNDLE, 7, &mut host).unwrap_err();
    assert_eq!(error, RestartError::StillRunning);
    assert_eq!(host.events, ["exit"]);
    assert!(host.running);
}

#[test]
fn restart_launch_failure_clears_exit() {
    let mut model = needing_restart();
    let mut host = Host::ready();
    host.schedule_error = Some(RestartError::LaunchFailed);
    let error = model.restart(BUNDLE, 7, &mut host).unwrap_err();
    assert_eq!(error, RestartError::LaunchFailed);
    assert_eq!(host.events, ["exit", "schedule", "cancel"]);
    assert!(host.running);
    let page = onboarding_page("en", &model.view());
    assert!(page.contains("The helper did not restart. Try again."));
}

#[test]
fn restart_timeout_clears_exit() {
    let mut model = needing_restart();
    let mut host = Host::ready();
    host.schedule_error = Some(RestartError::TimedOut);
    let error = model.restart(BUNDLE, 7, &mut host).unwrap_err();
    assert_eq!(error, RestartError::TimedOut);
    assert_eq!(host.events, ["exit", "schedule", "cancel"]);
    assert!(host.running);
}

#[test]
fn restart_when_not_needed_does_not_touch_host() {
    let mut model = OnboardingModel::new(snap(false, false));
    let mut host = Host::ready();
    let error = model.restart(BUNDLE, 7, &mut host).unwrap_err();
    assert_eq!(error, RestartError::NotNeeded);
    assert!(host.events.is_empty());
}

#[test]
fn settings_click_opens_only_the_matching_url() {
    let mut model = OnboardingModel::new(snap(false, false));
    let mut host = Host::ready();
    let mut settings = Opened {
        allow: true,
        ..Opened::default()
    };
    let effect = apply_ipc(
        &mut model,
        "settings:accessibility",
        BUNDLE,
        7,
        &mut host,
        &mut settings,
    );
    assert_eq!(effect, IpcEffect::OpenedSettings);
    assert_eq!(settings.urls, vec![ACCESSIBILITY_SETTINGS_URL]);
    assert_eq!(
        settings_url(PermissionKind::Accessibility),
        ACCESSIBILITY_SETTINGS_URL
    );
    let effect = apply_ipc(
        &mut model,
        "settings:screen_recording",
        BUNDLE,
        7,
        &mut host,
        &mut settings,
    );
    assert_eq!(effect, IpcEffect::OpenedSettings);
    assert_eq!(settings.urls[1], SCREEN_RECORDING_SETTINGS_URL);
    assert!(host.events.is_empty());
    assert!(open_settings_allowed(ACCESSIBILITY_SETTINGS_URL));
    assert!(!open_settings_allowed("https://example.invalid"));
}

#[test]
fn ipc_unknown_token_is_ignored() {
    let mut model = needing_restart();
    let mut host = Host::ready();
    let mut settings = Opened {
        allow: true,
        ..Opened::default()
    };
    let effect = apply_ipc(
        &mut model,
        "settings:camera",
        BUNDLE,
        7,
        &mut host,
        &mut settings,
    );
    assert_eq!(effect, IpcEffect::None);
    assert!(settings.urls.is_empty());
    assert!(host.events.is_empty());
    let effect = apply_ipc(&mut model, "allow", BUNDLE, 7, &mut host, &mut settings);
    assert_eq!(effect, IpcEffect::None);
}

#[test]
fn should_open_onboarding_only_when_macos_and_missing() {
    assert!(!should_open_onboarding(false, snap(false, false)));
    assert!(!should_open_onboarding(true, snap(true, true)));
    assert!(should_open_onboarding(true, snap(false, true)));
    assert!(should_open_onboarding(true, snap(true, false)));
}

#[test]
fn escape_html_encodes_markup() {
    assert_eq!(escape_html("</script>\"&"), "&lt;/script&gt;&quot;&amp;");
}

#[test]
fn status_script_cannot_break_out_of_js_string() {
    let embedded = js_embed("</script>\"'\\");
    assert!(!embedded.contains("</script>"));
    assert!(embedded.starts_with('"') && embedded.ends_with('"'));
    assert!(embedded.contains("\\u003c"));
    let page_script = status_script("en", &needing_restart().view());
    assert!(!page_script.contains("</script>"));
    assert!(!page_script.contains("Enter"));
    assert!(page_script.contains("textContent"));
    assert!(page_script.contains("hidden"));
}

#[test]
fn navigation_allowed_refuses_remote_file_and_settings() {
    assert!(!navigation_allowed("https://example.invalid"));
    assert!(!navigation_allowed("http://example.invalid"));
    assert!(!navigation_allowed("file:///etc/passwd"));
    assert!(!navigation_allowed(ACCESSIBILITY_SETTINGS_URL));
    assert!(!navigation_allowed("javascript:alert(1)"));
    assert!(navigation_allowed("about:blank"));
    assert!(navigation_allowed(""));
}

#[test]
fn argv_requests_onboarding_real_agent_returns_true() {
    assert!(argv_requests_onboarding(&os(&["agent"])));
    assert!(argv_requests_onboarding(&os(&["--scene", "form", "agent"])));
}

#[test]
fn argv_requests_onboarding_fake_driver_returns_false() {
    assert!(!argv_requests_onboarding(&os(&[
        "agent", "--driver", "fake"
    ])));
    assert!(!argv_requests_onboarding(&os(&["--driver=fake", "agent"])));
    assert!(!argv_requests_onboarding(&os(&[
        "--driver", "Fake", "agent"
    ])));
}

#[test]
fn argv_requests_onboarding_without_agent_returns_false() {
    assert!(!argv_requests_onboarding(&os(&["doctor"])));
    assert!(!argv_requests_onboarding(&os(&[
        "--scene", "agent", "doctor"
    ])));
    assert!(!argv_requests_onboarding(&os(&["--scene", "agent"])));
}

#[test]
fn relaunch_waiter_execs_open_after_pid_exits() {
    let argv = launch_command(BUNDLE).unwrap();
    let script = relaunch_waiter(42, &argv).unwrap();
    assert!(script.contains("exec open -g -j -b ai.grok.computer-use.helper --args agent"));
    assert!(script.contains("kill -0 42"));
    assert!(script.contains("exit 1"));
    assert_eq!(relaunch_waiter(0, &argv).unwrap_err(), RestartError::BadPid);
    let mut bad = argv.clone();
    bad.push("a;rm".into());
    assert_eq!(
        relaunch_waiter(7, &bad).unwrap_err(),
        RestartError::LaunchFailed
    );
}

#[test]
fn export_module_binding_and_header_match_app_catalog() {
    assert_eq!(binding_name("en"), "enComputerUse");
    assert_eq!(binding_name("pt-BR"), "ptBRComputerUse");
    assert_eq!(binding_name("zh-TW"), "zhTWComputerUse");
    let source = include_str!("../../../ui/locales/en.json");
    let module = export_module("en", source).unwrap();
    assert!(module.starts_with(
        "/** en messages — domain: computer-use. Generated from computer-use/ui/locales/en.json by `cargo xtask ui export-i18n`. Do not edit. */\n"
    ));
    assert!(module.contains("export const enComputerUse = {\n"));
    assert!(module.contains("  \"onboarding.dismiss\": \"Not now\",\n"));
    assert!(module.ends_with("} as const;\n"));
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src/i18n/messages");
    if root.join("en/computer-use.ts").is_file() {
        for locale in gcu_mcp::LOCALES {
            let json = std::fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join(format!("../../ui/locales/{locale}.json")),
            )
            .unwrap();
            let expected = export_module(locale, &json).unwrap();
            let actual =
                std::fs::read_to_string(root.join(locale).join("computer-use.ts")).unwrap();
            assert_eq!(actual, expected, "{locale}");
        }
    }
}

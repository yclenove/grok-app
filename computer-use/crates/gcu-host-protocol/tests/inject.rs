//! Injection decisions, FD fallback, and secret leak scans.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_host_protocol::inject::{
    acp_payload_log, catalog_log, crash_report, crash_report_enabled, extra_env, fd_entry,
    http_entry, leak_sites, omit_local_mcp_catalog, redact, session_kind, should_inject,
    InjectFacts, Issued, Surfaces, TokenBook, SERVER_NAME,
};
use gcu_host_protocol::log_policy::{format_text, payloads_enabled};
use gcu_host_protocol::messages::SessionKind;

fn facts(ssh: bool, scheduled: bool, remote_im: bool, side_channel: bool) -> InjectFacts {
    InjectFacts {
        ssh,
        scheduled,
        remote_im,
        side_channel,
    }
}

fn token(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

#[test]
fn session_kind_ssh_remote_im_and_schedule_refuse_local() {
    assert_eq!(
        session_kind(facts(true, true, true, false)),
        SessionKind::Ssh
    );
    assert_eq!(
        session_kind(facts(false, true, true, false)),
        SessionKind::RemoteIm
    );
    assert_eq!(
        session_kind(facts(false, true, false, false)),
        SessionKind::Automation
    );
    assert_eq!(
        session_kind(facts(false, false, false, true)),
        SessionKind::Local
    );
}

#[test]
fn omit_catalog_matches_ssh_and_side_channel_only() {
    assert!(omit_local_mcp_catalog(facts(true, false, false, false)));
    assert!(omit_local_mcp_catalog(facts(false, false, false, true)));
    assert!(!omit_local_mcp_catalog(facts(false, true, false, false)));
    assert!(!omit_local_mcp_catalog(facts(false, false, true, false)));
    assert!(!omit_local_mcp_catalog(InjectFacts::default()));
}

#[test]
fn should_inject_only_enabled_local_sessions() {
    assert!(should_inject(true, InjectFacts::default()));
    assert!(!should_inject(false, InjectFacts::default()));
    assert!(!should_inject(true, facts(true, false, false, false)));
    assert!(!should_inject(true, facts(false, true, false, false)));
    assert!(!should_inject(true, facts(false, false, true, false)));
    assert!(!should_inject(true, facts(false, false, false, true)));
}

#[test]
fn http_entry_uses_acp_type_and_bearer_without_env() {
    let secret = token(0x11);
    let entry = http_entry("http://127.0.0.1:9/mcp", &secret);
    assert_eq!(entry["type"], "http");
    assert!(entry.get("transport").is_none());
    assert_eq!(entry["name"], SERVER_NAME);
    assert_eq!(entry["headers"][0]["name"], "Authorization");
    assert_eq!(entry["headers"][0]["value"], format!("Bearer {secret}"));
    assert!(entry.get("env").is_none());
    assert!(extra_env().is_empty());
    let text = entry.to_string();
    assert!(!text.contains("GCU_TOKEN"));
}

#[test]
fn fd_entry_passes_descriptor_and_omits_token_and_env() {
    let secret = token(0x22);
    let entry = fd_entry("/opt/gcu", "http://127.0.0.1:9/mcp", 7);
    let text = entry.to_string();
    assert!(!text.contains(&secret));
    assert!(!text.contains("GCU_TOKEN"));
    assert!(entry.get("env").is_none());
    let args: Vec<&str> = entry["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert_eq!(
        args,
        [
            "mcp",
            "--connect",
            "http://127.0.0.1:9/mcp",
            "--token-fd",
            "7"
        ]
    );
    assert!(extra_env().is_empty());
}

#[test]
fn leak_scan_clean_injection_is_empty_and_each_surface_is_named() {
    let secret = token(0x33);
    let entry = http_entry("http://127.0.0.1:9/mcp", &secret);
    let log = redact(&entry.to_string(), std::slice::from_ref(&secret));
    let crash = crash_report(
        "agent exited",
        &format!("stderr Bearer {secret} GCU_TOKEN={secret}"),
    );
    assert!(!log.contains(&secret));
    assert!(!crash.contains(&secret));
    assert!(!crash.contains("GCU_TOKEN="));
    let mixed = redact(&format!("Bearer no then Bearer {secret} end"), &[]);
    assert!(!mixed.contains(&secret));
    assert!(mixed.contains("[redacted]"));
    assert_eq!(
        leak_sites(
            &secret,
            Surfaces {
                process_env: &extra_env(),
                child_env: &extra_env(),
                acp_log: &format!("{} {log}", catalog_log(1)),
                crash_report: &crash,
            }
        ),
        Vec::<&str>::new()
    );
    let planted = secret.clone();
    assert_eq!(
        leak_sites(
            &secret,
            Surfaces {
                process_env: &[("PATH".into(), planted.clone())],
                child_env: &extra_env(),
                acp_log: &catalog_log(1),
                crash_report: "clean",
            }
        ),
        vec!["process_env"]
    );
    assert_eq!(
        leak_sites(
            &secret,
            Surfaces {
                process_env: &[],
                child_env: &[("GCU_TOKEN".into(), planted.clone())],
                acp_log: &catalog_log(0),
                crash_report: "clean",
            }
        ),
        vec!["child_env"]
    );
    assert_eq!(
        leak_sites(
            &secret,
            Surfaces {
                process_env: &[],
                child_env: &[],
                acp_log: &format!("payload {planted}"),
                crash_report: "clean",
            }
        ),
        vec!["acp_log"]
    );
    assert_eq!(
        leak_sites(
            &secret,
            Surfaces {
                process_env: &[],
                child_env: &[],
                acp_log: &catalog_log(0),
                crash_report: &format!("boom {planted}"),
            }
        ),
        vec!["crash_report"]
    );
}

#[test]
fn acp_payload_log_and_crash_report_drop_registered_bearer() {
    let secret = token(0x44);
    gcu_host_protocol::inject::remember_secret(&secret);
    let entry = http_entry("http://127.0.0.1:9/mcp", &secret);
    let body = entry.to_string();
    let log = acp_payload_log(&entry);
    assert_eq!(log, format_text(payloads_enabled(), "acp", &body));
    assert!(!log.contains(&secret));
    let shown = format_text(true, "acp", &body);
    assert!(!shown.contains(&secret));
    assert!(shown.contains("[redacted]"));
    let hidden = format_text(false, "acp", &body);
    assert!(!hidden.contains(&secret));
    assert!(!hidden.contains("127.0.0.1"));
    let stderr = format!("GCU_TOKEN={secret} plain-context-word");
    let crash = crash_report("died", &stderr);
    assert_eq!(
        crash,
        crash_report_enabled(payloads_enabled(), "died", &stderr)
    );
    assert!(!crash.contains(&secret));
    assert!(!crash.contains("GCU_TOKEN="));
    let forced = crash_report_enabled(true, "died", &stderr);
    assert!(!forced.contains(&secret));
    assert!(!forced.contains("GCU_TOKEN="));
    assert!(forced.contains("plain-context-word"));
    let off = crash_report_enabled(false, "died", &stderr);
    assert!(!off.contains(&secret));
    assert!(!off.contains("plain-context-word"));
    assert!(off.contains("category="));
    assert!(off.contains("location="));
}

#[test]
fn token_book_revoke_drops_live_secret_and_rejects_reuse() {
    let mut book = TokenBook::new();
    let first = token(0x01);
    let second = token(0x02);
    book.issue(
        "chat",
        Issued {
            mcp_url: "http://127.0.0.1:1/mcp".into(),
            token: first.clone(),
        },
    );
    book.issue(
        "chat",
        Issued {
            mcp_url: "http://127.0.0.1:2/mcp".into(),
            token: second.clone(),
        },
    );
    assert_eq!(book.current("chat").unwrap().token, second);
    let revoked = book.revoke("chat").unwrap();
    assert_eq!(revoked.token, second);
    assert!(book.current("chat").is_none());
    assert_eq!(book.live_count(), 0);
    let third = token(0x03);
    book.issue(
        "chat",
        Issued {
            mcp_url: "http://127.0.0.1:3/mcp".into(),
            token: third.clone(),
        },
    );
    assert_ne!(book.current("chat").unwrap().token, revoked.token);
    assert_ne!(book.current("chat").unwrap().token, first);
    let scrubbed = redact(
        &format!("old {first} mid {second} new {third}"),
        &[first, second.clone(), third],
    );
    assert!(!scrubbed.contains(&second));
    book.revoke_all();
    assert!(book.live_ids().is_empty());
}

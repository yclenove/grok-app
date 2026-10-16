//! Thin adapter. Classification, headers, and redaction live in `gcu-host-protocol`.

use super::gateway;
use crate::acp_client::AcpClient;
use gcu_host_protocol::inject::InjectFacts;
use tauri::{AppHandle, Manager};

/// Attach or clear the Computer Use MCP entry for this process.
///
/// `ssh_alias` is the same option the ACP spawn already stored. Scheduled and
/// remote-IM chats are refused here; official side jobs never call this.
pub(crate) async fn attach(
    app: &AppHandle,
    client: &AcpClient,
    session_id: &str,
    ssh_alias: Option<&str>,
    scheduled: bool,
) {
    let facts = InjectFacts {
        ssh: ssh_alias.is_some(),
        scheduled,
        remote_im: false,
        side_channel: false,
    };
    let entry = match app.try_state::<super::ComputerUseHost>() {
        Some(host) => gateway::prepare(&host, session_id, facts).await,
        None => None,
    };
    client.set_computer_use_entry(entry);
}

/// Drop the endpoint when the App session's agent is dropped.
pub(crate) async fn revoke_session(app: &AppHandle, session_id: &str) {
    let Some(host) = app.try_state::<super::ComputerUseHost>() else {
        return;
    };
    gateway::revoke(&host, session_id).await;
}

#[cfg(test)]
mod tests {
    use gcu_host_protocol::inject::{
        catalog_log, crash_report, extra_env, fd_entry, http_entry, leak_sites,
        omit_local_mcp_catalog, redact, session_kind, should_inject, InjectFacts, Surfaces,
        SERVER_NAME,
    };
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
    fn ssh_scheduled_and_remote_im_are_not_local() {
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
        assert!(omit_local_mcp_catalog(facts(true, false, false, false)));
        assert!(omit_local_mcp_catalog(facts(false, false, false, true)));
        assert!(!omit_local_mcp_catalog(facts(false, true, false, false)));
        assert!(!should_inject(true, facts(true, false, false, false)));
        assert!(!should_inject(true, facts(false, true, false, false)));
        assert!(!should_inject(true, facts(false, false, true, false)));
        assert!(should_inject(true, InjectFacts::default()));
        assert!(!should_inject(false, InjectFacts::default()));
    }

    #[test]
    fn http_and_fd_entries_omit_environment() {
        let secret = token(0x11);
        let http = http_entry("http://127.0.0.1:9/mcp", &secret);
        assert_eq!(http["type"], "http");
        assert!(http.get("transport").is_none());
        assert!(http.get("env").is_none());
        assert_eq!(http["name"], SERVER_NAME);
        assert_eq!(http["headers"][0]["value"], format!("Bearer {secret}"));
        let fd = fd_entry("/opt/gcu", "http://127.0.0.1:9/mcp", 7);
        let text = fd.to_string();
        assert!(!text.contains(&secret));
        assert!(!text.contains("GCU_TOKEN"));
        assert!(fd.get("env").is_none());
        assert_eq!(fd["args"][4], "7");
        assert!(extra_env().is_empty());
    }

    #[test]
    fn leak_scan_names_each_surface_and_redacts_markers() {
        let secret = token(0x33);
        let log = redact(
            &http_entry("http://127.0.0.1:9/mcp", &secret).to_string(),
            std::slice::from_ref(&secret),
        );
        let crash = crash_report(
            "agent exited",
            &format!("Bearer {secret} GCU_TOKEN={secret}"),
        );
        assert!(!log.contains(&secret));
        assert!(!crash.contains(&secret));
        assert!(!crash.contains("GCU_TOKEN="));
        assert!(leak_sites(
            &secret,
            Surfaces {
                process_env: &extra_env(),
                child_env: &extra_env(),
                acp_log: &format!("{} {log}", catalog_log(1)),
                crash_report: &crash,
            }
        )
        .is_empty());
        assert_eq!(
            leak_sites(
                &secret,
                Surfaces {
                    process_env: &[("PATH".into(), secret.clone())],
                    child_env: &[],
                    acp_log: "",
                    crash_report: "",
                }
            ),
            vec!["process_env"]
        );
        assert_eq!(
            leak_sites(
                &secret,
                Surfaces {
                    process_env: &[],
                    child_env: &[("GCU_TOKEN".into(), "no".into())],
                    acp_log: "",
                    crash_report: "",
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
                    acp_log: &secret,
                    crash_report: "",
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
                    acp_log: "",
                    crash_report: &secret,
                }
            ),
            vec!["crash_report"]
        );
    }
}

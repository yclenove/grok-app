//! Parse one approval decision and forward desk notices to the webview.
//!
//! The desk itself lives in `gcu-host-protocol`. This module does not grant.

use gcu_host_protocol::approval::{
    action_digest_from_hex, Act, Claim, ConfirmAct, ConfirmClaim, Fault, HandoffAct, HandoffClaim,
    ImageBinding, Notice, Office,
};
use gcu_host_protocol::auth::ProcessInstance;
use serde::Deserialize;
use tauri::{AppHandle, Emitter};

/// Body of `cu_approval_decide`. Fields repeat the card the user was shown.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DecisionBody {
    request_id: u64,
    session_id: String,
    image: ImageBinding,
    engine_pid: u32,
    engine_started: u64,
    policy_version: u64,
    act: Act,
}

/// Build a claim. A digest that is not 64 hex characters is an identity fault.
pub(crate) fn claim_from_body(body: DecisionBody) -> Result<Claim, String> {
    if body
        .image
        .sha256_hex
        .as_deref()
        .is_some_and(|hex| !is_sha256_hex(hex))
    {
        return Err(fault_code(Fault::Identity));
    }
    Ok(Claim {
        request_id: body.request_id,
        session_id: body.session_id,
        image: body.image,
        engine: ProcessInstance {
            pid: body.engine_pid,
            started: body.engine_started,
        },
        policy_version: body.policy_version,
        act: body.act,
    })
}

pub(crate) fn fault_code(fault: Fault) -> String {
    match fault {
        Fault::Absent => "absent",
        Fault::Identity => "identity",
        Fault::Replay => "replay",
        Fault::Failed => "failed",
        Fault::Stale => "stale",
    }
    .to_owned()
}

/// Body of `cu_confirm_decide`. Fields repeat the card the user was shown.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConfirmBody {
    request_id: u64,
    session_id: String,
    image: ImageBinding,
    engine_pid: u32,
    engine_started: u64,
    policy_version: u64,
    snapshot_id: String,
    action_digest: String,
    act: ConfirmAct,
}

/// Body of `cu_handoff_decide`. Handling it does not grant or clear Stop.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoffBody {
    request_id: u64,
    session_id: String,
    image: ImageBinding,
    engine_pid: u32,
    engine_started: u64,
    policy_version: u64,
    act: HandoffAct,
}

/// Build a confirm claim. A bad action digest is stale and grants nothing.
pub(crate) fn confirm_from_body(body: ConfirmBody) -> Result<ConfirmClaim, String> {
    if body
        .image
        .sha256_hex
        .as_deref()
        .is_some_and(|hex| !is_sha256_hex(hex))
    {
        return Err(fault_code(Fault::Identity));
    }
    let action_digest = action_digest_from_hex(&body.action_digest).map_err(fault_code)?;
    Ok(ConfirmClaim {
        request_id: body.request_id,
        session_id: body.session_id,
        image: body.image,
        engine: ProcessInstance {
            pid: body.engine_pid,
            started: body.engine_started,
        },
        policy_version: body.policy_version,
        snapshot_id: body.snapshot_id,
        action_digest,
        act: body.act,
    })
}

/// Build a handoff claim. The act cannot approve an action.
pub(crate) fn handoff_from_body(body: HandoffBody) -> Result<HandoffClaim, String> {
    if body
        .image
        .sha256_hex
        .as_deref()
        .is_some_and(|hex| !is_sha256_hex(hex))
    {
        return Err(fault_code(Fault::Identity));
    }
    Ok(HandoffClaim {
        request_id: body.request_id,
        session_id: body.session_id,
        image: body.image,
        engine: ProcessInstance {
            pid: body.engine_pid,
            started: body.engine_started,
        },
        policy_version: body.policy_version,
        act: body.act,
    })
}

/// Subscribe before the engine can ask, then emit one event per notice.
pub(crate) fn forward_notices(app: AppHandle, office: &Office) {
    let mut notes = office.subscribe();
    tauri::async_runtime::spawn(async move {
        while let Some(notice) = notes.recv().await {
            let event = match &notice {
                Notice::Pending { .. } => "cu://approval",
                Notice::Settled { .. } => "cu://approval-settled",
                Notice::ConfirmPending { .. } => "cu://confirm",
                Notice::ConfirmSettled { .. } => "cu://confirm-settled",
                Notice::HandoffPending { .. } => "cu://handoff",
                Notice::HandoffSettled { .. } => "cu://handoff-settled",
            };
            let _ = app.emit(event, &notice);
        }
    });
}

fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gcu_host_protocol::messages::{Platform, Signature};

    fn body(hex: Option<&str>, act: Act) -> DecisionBody {
        DecisionBody {
            request_id: 7,
            session_id: "sess".to_owned(),
            image: ImageBinding {
                platform: Platform::Linux,
                key: "linux:editor".to_owned(),
                team_id: None,
                publisher: Some("Desk".to_owned()),
                sha256_hex: hex.map(str::to_owned),
                signature: Signature::Valid,
                image_path: "/usr/bin/editor".to_owned(),
                pid: Some(77),
            },
            engine_pid: 41,
            engine_started: 9001,
            policy_version: 1,
            act,
        }
    }

    #[test]
    fn claim_from_body_keeps_the_checked_image() {
        let hex = "ab".repeat(32);
        let claim = claim_from_body(body(Some(&hex), Act::Deny)).expect("claim");
        assert_eq!(claim.request_id, 7);
        assert_eq!(claim.session_id, "sess");
        assert_eq!(claim.engine.pid, 41);
        assert_eq!(claim.engine.started, 9001);
        assert_eq!(claim.policy_version, 1);
        assert_eq!(claim.act, Act::Deny);
        assert_eq!(claim.image.image_path, "/usr/bin/editor");
        assert_eq!(claim.image.sha256_hex.as_deref(), Some(hex.as_str()));
    }

    #[test]
    fn claim_from_body_bad_digest_is_identity() {
        assert_eq!(
            claim_from_body(body(Some("zz"), Act::Always)).expect_err("hex"),
            "identity"
        );
        assert_eq!(
            claim_from_body(body(Some(""), Act::Turn)).expect_err("empty"),
            "identity"
        );
    }

    #[test]
    fn claim_from_body_absent_digest_is_allowed() {
        let claim = claim_from_body(body(None, Act::Session)).expect("claim");
        assert!(claim.image.sha256_hex.is_none());
        assert_eq!(claim.act, Act::Session);
    }

    #[test]
    fn fault_code_names_match_the_card() {
        assert_eq!(fault_code(Fault::Absent), "absent");
        assert_eq!(fault_code(Fault::Identity), "identity");
        assert_eq!(fault_code(Fault::Replay), "replay");
        assert_eq!(fault_code(Fault::Failed), "failed");
        assert_eq!(fault_code(Fault::Stale), "stale");
    }

    #[test]
    fn confirm_from_body_keeps_the_digest_and_rejects_bad_hex() {
        let hex = "ab".repeat(32);
        let claim = confirm_from_body(ConfirmBody {
            request_id: 9,
            session_id: "sess".to_owned(),
            image: body(Some(&hex), Act::Deny).image,
            engine_pid: 41,
            engine_started: 9001,
            policy_version: 1,
            snapshot_id: "snap-1".to_owned(),
            action_digest: hex,
            act: ConfirmAct::Allow,
        })
        .expect("claim");
        assert_eq!(claim.snapshot_id, "snap-1");
        assert_eq!(claim.act, ConfirmAct::Allow);
        assert_eq!(claim.action_digest[0], 0xab);
        assert_eq!(
            confirm_from_body(ConfirmBody {
                request_id: 9,
                session_id: "sess".to_owned(),
                image: body(None, Act::Deny).image,
                engine_pid: 41,
                engine_started: 9001,
                policy_version: 1,
                snapshot_id: "snap-1".to_owned(),
                action_digest: "zz".to_owned(),
                act: ConfirmAct::Deny,
            })
            .expect_err("hex"),
            "stale"
        );
    }

    #[test]
    fn handoff_from_body_does_not_become_an_allow() {
        let claim = handoff_from_body(HandoffBody {
            request_id: 4,
            session_id: "sess".to_owned(),
            image: body(None, Act::Deny).image,
            engine_pid: 41,
            engine_started: 9001,
            policy_version: 1,
            act: HandoffAct::Handled,
        })
        .expect("claim");
        assert_eq!(claim.act, HandoffAct::Handled);
    }
}

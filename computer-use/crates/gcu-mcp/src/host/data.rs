//! Explicit in-process/wire conversions; no authorization decisions here.
use gcu_host_api as api;
use gcu_host_protocol::messages as wire;
pub(super) fn identity(id: &api::AppIdentity) -> wire::Identity {
    wire::Identity {
        platform: match id.platform {
            api::Platform::Macos => wire::Platform::Macos,
            api::Platform::Windows => wire::Platform::Windows,
            api::Platform::Linux => wire::Platform::Linux,
        },
        key: id.key.clone(),
        team_id: id.team_id.clone(),
        publisher: id.publisher.clone(),
        sha256: id.sha256,
        signature: match id.signature {
            api::SignatureState::Valid => wire::Signature::Valid,
            api::SignatureState::Invalid => wire::Signature::Invalid,
            api::SignatureState::Unsigned => wire::Signature::Unsigned,
            api::SignatureState::Unknown => wire::Signature::Unknown,
        },
        image_path: id.image_path.clone(),
        proof: wire::GrantProof {
            cdhash: id.proof.cdhash.clone(),
            certificate_sha256: id.proof.certificate_sha256.clone(),
            product: id.proof.product.clone(),
            build: id.proof.build.clone(),
            file_dev: id.proof.file_dev,
            file_inode: id.proof.file_inode,
        },
    }
}
fn api_identity(id: wire::Identity) -> api::AppIdentity {
    api::AppIdentity {
        platform: match id.platform {
            wire::Platform::Macos => api::Platform::Macos,
            wire::Platform::Windows => api::Platform::Windows,
            wire::Platform::Linux => api::Platform::Linux,
        },
        key: id.key,
        team_id: id.team_id,
        publisher: id.publisher,
        sha256: id.sha256,
        signature: match id.signature {
            wire::Signature::Valid => api::SignatureState::Valid,
            wire::Signature::Invalid => api::SignatureState::Invalid,
            wire::Signature::Unsigned => api::SignatureState::Unsigned,
            wire::Signature::Unknown => api::SignatureState::Unknown,
        },
        image_path: id.image_path,
        proof: api::GrantProof {
            cdhash: id.proof.cdhash,
            certificate_sha256: id.proof.certificate_sha256,
            product: id.proof.product,
            build: id.proof.build,
            file_dev: id.proof.file_dev,
            file_inode: id.proof.file_inode,
        },
    }
}
pub(super) fn image(image: &api::ImageRef) -> wire::Image {
    wire::Image {
        mime: image.mime.clone(),
        width: image.width,
        height: image.height,
        bytes: image.bytes.to_vec(),
    }
}
pub(super) fn app(app: &api::AppInfo) -> wire::App {
    wire::App {
        name: app.name.clone(),
        pid: app.pid,
        identity: identity(&app.identity),
        icon: app.icon.as_ref().map(image),
    }
}
pub(super) fn policy(p: wire::Policy) -> api::PolicyConfig {
    api::PolicyConfig {
        always_allow_enabled: p.always_allow_enabled,
        sensitive_action_confirmation: match p.sensitive_action_confirmation {
            wire::ConfirmationLevel::Off => api::ConfirmationLevel::Off,
            wire::ConfirmationLevel::Standard => api::ConfirmationLevel::Standard,
            wire::ConfirmationLevel::Strict => api::ConfirmationLevel::Strict,
        },
        timeout_secs: p.timeout_secs,
        allow_foreground: match p.allow_foreground {
            wire::ForegroundPolicy::Never => api::ForegroundPolicy::Never,
            wire::ForegroundPolicy::Ask => api::ForegroundPolicy::Ask,
            wire::ForegroundPolicy::Always => api::ForegroundPolicy::Always,
        },
        macos_private_spi: p.macos_private_spi,
        user_idle_ms: p.user_idle_ms,
        stop_hotkey: p.stop_hotkey,
        max_tree_nodes: p.max_tree_nodes,
        max_image_edge: p.max_image_edge,
        image_format: p.image_format,
        extra_denied_apps: p.extra_denied_apps,
        always: p.always.into_iter().map(api_identity).collect(),
    }
}

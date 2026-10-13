//! Original App preview and model image use independent observation scopes.
use super::*;
use base64::Engine;
use grok_computer_use_core::broker::HostExecutionAddress;
use sha2::{Digest, Sha256};

pub(in super::super) fn preview(
    runtime: &tokio::runtime::Runtime,
    manager: &SessionManager,
) -> Result<Value, String> {
    let view = manager.computer_use_task_view("view-app")?;
    let [row] = view.executions.as_slice() else {
        return Err("original capture preview execution changed".into());
    };
    let address = HostExecutionAddress {
        key: row.key.clone(),
        run_id: row.run_id.clone(),
        generation: row.generation.clone(),
    };
    let observation =
        runtime.block_on(manager.preview_computer_use_task_execution("view-app", &address))?;
    let png = observation
        .image
        .png_base64
        .as_deref()
        .ok_or("original capture preview image missing")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(png)
        .map_err(|_| "original capture preview PNG invalid")?;
    if bytes.len() < 33 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("original capture preview PNG header missing".into());
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    if width == 0 || height == 0 {
        return Err("original capture preview geometry invalid".into());
    }
    Ok(
        json!({"snapshotId":observation.snapshot_id,"width":width,"height":height,
        "pngBytes":bytes.len(),"pngSha256":hex::encode(Sha256::digest(&bytes)),"address":address}),
    )
}

pub(super) fn validate_evidence(value: &Value) -> Result<(), String> {
    for property in [
        "modelImageSucceeded",
        "previewImageSucceeded",
        "modelRefSurvivedPreview",
        "originalAuthorityRetained",
        "originalDocumentsRetained",
        "threeSharesRetained",
        "originalStopped",
    ] {
        if value[property] != true {
            return Err("original capture success witness missing".into());
        }
    }
    for (property, count) in [
        ("reads", 3),
        ("inputs", 1),
        ("captures", 2),
        ("denials", 0),
        ("settled", 2),
        ("decodes", 2),
        ("results", 3),
        ("pairing", 1),
        ("disconnects", 0),
    ] {
        if value["counts"][property] != count {
            return Err("original successful capture count changed".into());
        }
    }
    println!("original successful capture validated witness: {value}");
    Ok(())
}

#[test]
#[ignore = "manual native gate: pinned CFT154 owned CDP action, original App/MCP image and task preview"]
fn existing_task_joint_capture_success_model_and_preview() {
    let mut checks = CHECKS[..6].to_vec();
    checks.extend([
        "original-grant-fresh-observation-no-input",
        "original-successful-capture-owned-action-grant",
        "original-successful-capture-held-before-physical-settle",
        "original-model-image-published-after-scope-join",
        "original-task-preview-preserves-model-ref",
        "original-successful-capture-stop-keeps-shares",
    ]);
    scenario("active-capture-success", &checks);
}

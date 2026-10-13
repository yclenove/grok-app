//! Real native pages, synthetic Host binding, authority-only final callbacks.
use super::*;
use crate::computer_use::adapter::NativeScopeIdentity;

pub(super) const LABEL: &str = "resource-browser-selection-group";
pub(super) const SESSION: &str = "selection-transfer-fixture";
pub(super) const RUN: &str = "selection-transfer-run";

async fn guard(identities: Vec<NativeScopeIdentity>, accepted: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut commits = 0;
        let result =
            crate::computer_use::product_webview().with_scope_authority(&identities, &mut || {
                // No native query, DOM read, script or input under authority.
                commits += 1;
                Ok(())
            });
        if accepted != result.is_ok() || commits != usize::from(accepted) {
            return Err("native group publication result/commit count differs".into());
        }
        Ok(())
    })
    .await
    .map_err(|_| "native publication worker did not join")?
}

async fn business(run: &str, target: &TargetInfo, text: &str, count: usize) -> Result<(), String> {
    act(
        run,
        target.clone(),
        ActionKind::SetValue,
        "Note",
        json!({"text":text}),
    )
    .await?;
    act(run, target.clone(), ActionKind::Click, "Save", json!({})).await?;
    for _ in 0..count {
        act(
            run,
            target.clone(),
            ActionKind::Click,
            "Increase",
            json!({}),
        )
        .await?;
    }
    Ok(())
}

pub(super) async fn check(app: &tauri::AppHandle, url: &tauri::Url) -> Result<Value, String> {
    let adapter = crate::computer_use::product_webview();
    create(app, LABEL, url).await?;
    let b_initial = ready(app, LABEL).await?;
    let a_initial = state(app, super::LABEL).await?;
    let a = select(app, super::LABEL).await?;
    let b = select(app, LABEL).await?;
    if !adapter.list_targets()?.is_empty()
        || b_initial != json!({"note":"未保存","saves":0,"count":0})
        || state(app, super::LABEL).await? != a_initial
        || state(app, LABEL).await? != b_initial
    {
        return Err("native group selection changed bindings or business values".into());
    }
    let ta = adapter.bind_selected(&a, super::SESSION, super::RUN)?;
    let tb = adapter.bind_selected(&b, super::SESSION, super::RUN)?;
    if adapter.bind_selected(&a, SESSION, RUN).is_ok() {
        return Err("competing task stole a live native page".into());
    }
    let pa = adapter.scope_identity(&ta.target_id)?;
    let pb = adapter.scope_identity(&tb.target_id)?;
    guard(vec![pb.clone(), pa.clone()], true).await?;
    guard(vec![pa.clone(), pa.clone()], false).await?;
    if adapter.list_targets_for_run(super::RUN)?.len() != 2
        || !adapter.list_targets_for_run(RUN)?.is_empty()
    {
        return Err("native group run metadata was not isolated".into());
    }

    adapter.release_target_for_run(super::RUN, &ta.target_id);
    if adapter.target_alive(&ta.target_id) || !adapter.target_alive(&tb.target_id) {
        return Err("exact member release affected the other native page".into());
    }
    guard(vec![pa.clone(), pb.clone()], false).await?;
    guard(vec![pb.clone()], true).await?;

    let transferred = adapter.bind_selected(&a, SESSION, RUN)?;
    let transferred_proof = adapter.scope_identity(&transferred.target_id)?;
    adapter.release_target_for_run(super::RUN, &ta.target_id);
    if adapter
        .scope_target_info(super::RUN, &transferred.target_id)
        .is_ok()
        || !adapter.target_alive(&transferred.target_id)
        || !adapter.target_alive(&tb.target_id)
    {
        return Err("old release or foreign run affected the new owner".into());
    }
    guard(vec![pa], false).await?;
    guard(vec![transferred_proof.clone()], true).await?;
    business(RUN, &transferred, "转交后的页面 你好", 1).await?;
    business(super::RUN, &tb, "独立第二页 中文", 3).await?;
    let a_final = state(app, super::LABEL).await?;
    let b_final = state(app, LABEL).await?;
    if a_final != json!({"note":"转交后的页面 你好","saves":2,"count":2})
        || b_final != json!({"note":"独立第二页 中文","saves":1,"count":3})
    {
        return Err("independent native page postconditions differ".into());
    }

    let handle = app.clone();
    on_main(app, move || {
        crate::side_browser_host::reload(&handle, LABEL.into())
    })
    .await??;
    let b_reloaded = ready(app, LABEL).await?;
    guard(vec![pb, transferred_proof.clone()], false).await?;
    guard(vec![transferred_proof], true).await?;
    if b_reloaded != b_initial || state(app, super::LABEL).await? != a_final {
        return Err("navigation replayed effects or altered the other native page".into());
    }
    adapter.release_target_for_run(super::RUN, &tb.target_id);
    adapter.release_target_for_run(RUN, &transferred.target_id);
    if !adapter.list_targets()?.is_empty() {
        return Err("native group exact cleanup retained a binding".into());
    }
    Ok(json!({
        "pages":2,"metadataOnlySelection":true,"groupCommitExactlyOnce":true,
        "duplicateNativeRejected":true,"competingOwnerRejected":true,
        "exactReleasePreservesOther":true,"transferRejectsOldProof":true,
        "oldReleasePreservesNewOwner":true,"foreignRunMetadataRejected":true,
        "navigationRejectsOldGroup":true,"navigationPreservesOtherOwner":true,
        "typedActions":8,"aFinal":a_final,"bFinal":b_final,"bReloaded":b_reloaded,
        "hostBindingSynthetic":true,"taskGrantPath":false,"realModel":false,
    }))
}

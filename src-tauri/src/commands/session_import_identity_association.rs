/// Read-only review of an explicit old target and fixed preview. The returned
/// digest is neither a grant nor authorization to change/import either history.
#[tauri::command]
pub async fn session_import_identity_association_inspect(
    request: crate::session_migration::identity_association::AssociationRequest,
) -> Result<crate::session_migration::identity_association::AssociationReview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_migration::identity_association::inspect(&request)
            .map_err(|error| error.into_command_err())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Persist a strictly confirmed intent through the existing durable plan engine.
/// Detached callers must provide the complete fixed selection and confirmations.
#[tauri::command]
pub async fn session_import_identity_association_plan(
    request: crate::session_migration::identity_association::AssociationPlanRequest,
) -> Result<crate::session_migration::engine::DurablePlanRef, String> {
    tauri::async_runtime::spawn_blocking(move || {
        request.build()
            .map(|plan| crate::session_migration::engine::DurablePlanRef::from(&plan))
            .map_err(|error| error.into_command_err())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Review one actual prepared member without transferring the whole batch selection.
#[tauri::command]
pub async fn session_import_prepared_association_inspect(
    request: crate::session_migration::prepared_association::PreparedAssociationRequest,
) -> Result<crate::session_migration::prepared_association::PreparedAssociationReview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        request.inspect().map_err(|error| error.into_command_err())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Preserve the prepare origin and receipt when finalizing confirmed associations.
#[tauri::command]
pub async fn session_import_prepared_association_plan(
    request: crate::session_migration::prepared_association::PreparedAssociationPlanRequest,
) -> Result<crate::session_migration::engine::DurablePlanRef, String> {
    tauri::async_runtime::spawn_blocking(move || {
        request
            .build()
            .map(|plan| crate::session_migration::engine::DurablePlanRef::from(&plan))
            .map_err(|error| error.into_command_err())
    })
    .await
    .map_err(|error| error.to_string())?
}

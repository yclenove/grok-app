//! Fail-closed entry for optional fake scope; no fallback without bootstrap.
use crate::{
    runner::{blank, Environment, RunConfig, RunStatus, TaskOutcome},
    tasks::Task,
};

pub(crate) async fn execute(task: &Task, cfg: &RunConfig, env: &Environment) -> TaskOutcome {
    execute_scoped(task, cfg, env, None).await
}

pub(crate) async fn execute_scoped(
    task: &Task,
    cfg: &RunConfig,
    env: &Environment,
    data_address: Option<std::net::SocketAddr>,
) -> TaskOutcome {
    if let Err(error) = crate::scope::validate(cfg) {
        return blank(
            &task.id,
            &task.category,
            cfg,
            RunStatus::NotRun,
            &error.to_string(),
        );
    }
    if cfg.fake_scope.is_some() && data_address.is_none() {
        return blank(
            &task.id,
            &task.category,
            cfg,
            RunStatus::NotRun,
            "scope_bootstrap_required",
        );
    }
    super::execute_ready(task, cfg, env, data_address).await
}

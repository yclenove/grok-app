//! The actual agent argv/environment assembly, without probing or spawning a CLI.
//! Transport wrapping, config preparation, logging and child ownership stay in
//! AcpClient. Do not derive Debug: options and native env may contain secrets.
use super::{spawn_flags::*, SpawnOptions};
use crate::providers::GrokBuildProxySpawn;
use crate::store::AppSettings;
use std::path::Path;
use tokio::process::Command;

pub(super) struct AgentCommandSpec<'a> {
    pub options: &'a SpawnOptions,
    pub settings: &'a AppSettings,
    pub version: Option<&'a str>,
    pub model: &'a str,
    pub sandbox: Option<&'a SandboxSpawnSpec>,
    pub max_turns: Option<&'a MaxTurnsSpawnSpec>,
    pub disallowed_tools: &'a [String],
    pub ssh: bool,
    pub session_data_mode: &'a str,
    pub grok_home: &'a Path,
    pub native: Option<&'a GrokBuildProxySpawn>,
}

impl AgentCommandSpec<'_> {
    /// This is the shared production assembler, not a second test-only CLI.
    /// Top-level options precede `agent`; agent options precede `stdio`.
    pub(super) fn apply_args(&self, cmd: &mut Command) {
        let opts = self.options;
        let settings = self.settings;
        let policy = opts.permission_policy.as_deref().unwrap_or("ask");
        let product_mode = opts.product_mode.as_deref();
        cmd.arg("--no-auto-update");
        cmd.args(folder_trust_spawn_flags(opts.folder_trust));
        apply_compaction_to_command(
            cmd,
            &settings.compaction_mode,
            &settings.compaction_detail,
            cli_supports_compaction_flags(self.version),
        );
        cmd.args(permission_mode_spawn_flags(policy, product_mode));
        if let Some(sandbox) = self.sandbox {
            cmd.args(sandbox.cli_args());
        }
        if let Some(max_turns) = self.max_turns {
            cmd.args(max_turns.cli_args());
        }
        cmd.args(background_wait_spawn_flags_from_settings(
            settings,
            self.version,
        ));
        if let Some(schema) = opts.json_schema.as_deref().map(str::trim) {
            if !schema.is_empty() {
                cmd.args(["--json-schema", schema]);
            }
        }
        cmd.args(extra_rules_spawn_flags(opts.extra_rules.as_deref()));
        cmd.args(system_prompt_override_spawn_flags(
            opts.system_prompt_override.as_deref(),
        ));
        cmd.args(disable_web_search_spawn_flags(settings.disable_web_search));
        cmd.args(no_ask_user_spawn_flags(resolve_no_ask_user(
            opts.no_ask_user,
            settings.no_ask_user,
        )));
        cmd.args(allowed_tools_spawn_flags(&settings.allowed_tools));
        cmd.args(disallowed_tools_spawn_flags(self.disallowed_tools));
        crate::agent_todo_gate::apply_todo_gate_to_command(cmd, settings.todo_gate_enabled);
        cmd.args(no_plan_spawn_flags(settings.plan_enabled));
        if let Some(agent) = AgentSpawnSpec::from_setting(&settings.preferred_agent) {
            cmd.args(agent.cli_args());
        }
        if let Some(agents) = agents_json_spawn_flags(&settings.agents_json) {
            cmd.args(agents);
        }
        crate::agent_subagents::apply_subagents_to_command(cmd, settings.subagents_enabled);
        crate::agent_memory::apply_memory_to_command(cmd, settings.experimental_memory);
        crate::agent_subagent_wt_snap::apply_subagent_wt_snap_to_command(
            cmd,
            settings.subagent_worktree_snapshot_enabled,
            self.version,
        );
        crate::agent_two_pass_compaction::apply_two_pass_compaction_to_command(
            cmd,
            settings.two_pass_compaction_enabled,
            self.version,
        );
        crate::agent_auto_wake::apply_auto_wake_to_command(
            cmd,
            settings.auto_wake_enabled,
            self.session_data_mode,
        );
        cmd.arg("agent");
        cmd.arg(leader_spawn_flag(self.ssh || settings.use_leader));
        // A local profile path must not be forwarded to an SSH remote host.
        if !self.ssh {
            if let Some(profile) =
                crate::agents_catalog::normalize_agent_profile_path(&settings.agent_profile_path)
            {
                cmd.args(["--agent-profile", &profile]);
            }
        }
        if !self.model.is_empty() {
            cmd.args(["--model", self.model]);
        }
        if let Some(effort) = opts.effort.as_deref().map(str::trim) {
            if is_spawnable_reasoning_effort(effort) {
                cmd.args(["--reasoning-effort", effort]);
            }
        }
        if should_pass_always_approve(policy, product_mode) {
            cmd.arg("--always-approve");
        }
        cmd.args(plugin_dir_spawn_flags(&opts.plugin_dirs));
        // fork_session is ACP session/fork, never an unpaired CLI flag.
        cmd.arg("stdio");
    }

    /// Call after transport wrapping: SSH creates a new Command. The capture
    /// contract supplies no native relay for SSH; local GROK_HOME is omitted.
    pub(super) fn apply_env(&self, cmd: &mut Command) {
        if self.options.main_profile.is_some() {
            // A projected home must not inherit another channel's native
            // endpoint/key/role overrides from the desktop process environment.
            for key in [
                "GROK_MODELS_BASE_URL",
                "GROK_MODELS_LIST_URL",
                "GROK_CLI_CHAT_PROXY_BASE_URL",
                "XAI_API_KEY",
                "GROK_WEB_SEARCH_MODEL",
                "GROK_IMAGE_DESCRIPTION_MODEL",
            ] {
                cmd.env_remove(key);
            }
        }
        if !self.ssh {
            cmd.env("GROK_HOME", self.grok_home);
        }
        if let Some(native) = self.native {
            apply_grok_build_proxy_env(cmd, native);
        }
        cmd.env("GROK_CLAUDE_MCPS_ENABLED", "false");
        cmd.env("GROK_CURSOR_MCPS_ENABLED", "false");
        if let Some(sandbox) = self.sandbox {
            let (key, value) = sandbox.env_pair();
            cmd.env(key, value);
        }
    }
}

#[cfg(test)]
#[path = "spawn_command_tests.rs"]
mod tests;

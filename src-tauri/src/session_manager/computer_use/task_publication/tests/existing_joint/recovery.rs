//! Private fixture commands enter the actual original SessionManager controls.
//! Host gestures/session/ACP peer are synthetic, not a model approval surface.
use super::*;
use grok_computer_use_core::broker::HostExecutionAddress;
use grok_computer_use_core::task_consent::CommitOutcome;
mod owners;
mod races;
mod terminals;

pub(super) struct Context<'a> {
    pub(super) scenario: &'a str,
    pub(super) manager: &'a Arc<SessionManager>,
    pub(super) fixture: &'a mut TaskTransportFixture,
    pub(super) approvals: &'a FixtureApprovals,
    pub(super) approved: &'a CommitOutcome,
    pub(super) key: &'a HostRequestKey,
}

pub(super) fn is_scenario(scenario: &str) -> bool {
    owners::is_scenario(scenario)
        || matches!(
            scenario,
            "active-host-resume"
                | "active-host-resume-concurrent"
                | "active-host-resume-pause"
                | "active-host-resume-chat-stop"
        )
}

pub(super) fn is_command(command: &str) -> bool {
    matches!(
        command,
        "host-pause" | "host-resume" | "host-resume-duplicate" | "host-recovery-state"
    ) || races::is_command(command)
        || terminals::is_command(command)
        || owners::is_command(command)
}

#[derive(Default)]
pub(super) struct RecoveryProbe {
    paused: Option<HostExecutionAddress>,
    resumed: bool,
    duplicate_checked: bool,
    race: races::RecoveryRaceProbe,
    terminal: terminals::RecoveryTerminalProbe,
    owner: owners::RecoveryOwnerProbe,
}

impl RecoveryProbe {
    pub(super) fn retain_delivery(&mut self, fixture: &TaskTransportFixture) -> Result<(), String> {
        self.terminal.retain_delivery(fixture)
    }

    pub(super) fn command(
        &mut self,
        command: &str,
        runtime: &tokio::runtime::Runtime,
        mut context: Context<'_>,
    ) -> Result<Value, String> {
        if owners::is_command(command) {
            return self.owner.command(
                command,
                runtime,
                &mut context,
                &mut self.race,
                self.terminal.original_delivery()?,
            );
        }
        if terminals::is_command(command) {
            let reply = self
                .terminal
                .command(command, runtime, &mut context, &mut self.race)?;
            if command == "host-resume-terminal-finish" && reply["interruption"] == "pause" {
                self.paused = Some(terminals::current_address(&context)?);
            }
            return Ok(reply);
        }
        if races::is_command(command) {
            let reply = self
                .race
                .command(command, runtime, &context, self.paused.as_ref())?;
            if command == "host-resume-race-finish" {
                self.resumed = true;
            }
            return Ok(reply);
        }
        let manager = context.manager;
        let broker = context.fixture.broker();
        let approved = context.approved;
        let key = context.key;
        let execution_address = || -> Result<HostExecutionAddress, String> {
            let view = manager.computer_use_task_view("view-app")?;
            let original = view
                .executions
                .iter()
                .find(|row| row.run_id == approved.run_id && &row.key == key)
                .ok_or("original Host recovery execution changed")?;
            Ok(HostExecutionAddress {
                key: original.key.clone(),
                run_id: original.run_id.clone(),
                generation: original.generation.clone(),
            })
        };
        let mut outcome = Value::Null;
        let ok = match command {
            "host-pause" => {
                if self.paused.is_some() {
                    return Err("native fixture repeated original Host pause".into());
                }
                let address = execution_address()?;
                runtime
                    .block_on(manager.pause_computer_use_task_execution("view-app", &address))?;
                self.paused = Some(execution_address()?);
                true
            }
            "host-resume" => {
                if self.resumed {
                    return Err("native fixture repeated original Host recovery".into());
                }
                let address = self.paused.as_ref().ok_or("original Host pause missing")?;
                let result = runtime
                    .block_on(manager.resume_computer_use_task_execution("view-app", address));
                match result {
                    Ok(receipt) => {
                        self.resumed = true;
                        outcome = serde_json::to_value(receipt).unwrap();
                        true
                    }
                    Err(_) => false,
                }
            }
            "host-resume-duplicate" => {
                if !self.resumed || self.duplicate_checked {
                    return Err("native duplicate recovery misplaced".into());
                }
                self.duplicate_checked = true;
                runtime
                    .block_on(manager.resume_computer_use_task_execution(
                        "view-app",
                        self.paused.as_ref().ok_or("original Host pause missing")?,
                    ))
                    .is_err()
            }
            "host-recovery-state" => true,
            _ => return Err("unknown private Host recovery command".into()),
        };
        Ok(json!({"ok":ok,"outcome":outcome,
            "state":manager.computer_use_task_view("view-app")?,
            "executable":broker.executable_grant_count("view-app"),
            "continuations":broker.continuation_count("view-app"),
            "pendingContinuation":broker.claim_continuation("view-app").is_some()}))
    }

    pub(super) fn finish(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        broker: &crate::computer_use::ComputerUseBroker,
    ) -> Result<(), String> {
        self.race.finish(runtime, broker)
    }

    pub(super) fn fence_pending(&self, broker: &crate::computer_use::ComputerUseBroker) {
        self.race.fence_pending(broker);
    }

    pub(super) async fn finish_owners(&mut self) {
        self.owner.finish().await;
    }
}

pub(super) use owners::is_scenario as is_owner_scenario;
pub(super) use owners::validate_evidence as validate_owner_evidence;
pub(super) use races::validate_evidence;
pub(super) use terminals::validate_evidence as validate_terminal_evidence;

#[test]
#[ignore = "manual native gate: CFT140+, original recovery capture/registry/concurrent Continue"]
fn existing_task_joint_host_resume_concurrent_continue_keeps_one_original_native_capture() {
    let mut checks = RECOVERY_CHECKS.to_vec();
    checks.splice(
        8..8,
        [
            "original-recovery-native-return-held",
            "concurrent-continue-retains-one-original-worker",
        ],
    );
    scenario("active-host-resume-concurrent", &checks);
}

//! A process-owned, single-attempt migration grant. No filesystem IO under the
//! runtime/store locks, and no cross-home grant survives an abandoned open.
use super::AcpClient;
use crate::{providers::MigrationWitness, session_execution::ExecutionSource, store::SessionMeta};
use parking_lot::Mutex;
use std::sync::Arc;

enum Intent {
    Load(String),
    Fork(String),
    Fresh(String),
}

enum Phase {
    Ready,
    Opening {
        id: uuid::Uuid,
        intent: Intent,
        allow_origin: bool,
        previous: Option<String>,
    },
    Opened {
        id: uuid::Uuid,
        agent: String,
        allow_origin: bool,
        previous: Option<String>,
    },
    Revoked,
}

pub(super) struct Binding {
    witness: Arc<MigrationWitness>,
    phase: Mutex<Phase>,
}

#[derive(Clone, Copy)]
pub(super) enum Outcome {
    Loaded,
    Forked,
    Fresh,
}

fn unavailable() -> String {
    "execution history migration no longer owns this open attempt".into()
}

pub(super) struct Open {
    binding: Arc<Binding>,
    id: uuid::Uuid,
    completed: bool,
}

impl Drop for Open {
    fn drop(&mut self) {
        if !self.completed {
            let mut phase = self.binding.phase.lock();
            if matches!(&*phase, Phase::Opening { id, .. } | Phase::Opened { id, .. } if *id == self.id)
            {
                *phase = Phase::Revoked;
            }
        }
    }
}

impl Binding {
    pub(super) fn new(witness: MigrationWitness) -> Arc<Self> {
        Arc::new(Self {
            witness: Arc::new(witness),
            phase: Mutex::new(Phase::Ready),
        })
    }

    pub(super) fn revoke(&self) {
        *self.phase.lock() = Phase::Revoked;
    }

    fn validate_meta(
        &self,
        meta: &SessionMeta,
        source: &ExecutionSource,
        agent: Option<&str>,
    ) -> Result<(), String> {
        if self.witness.destination() != source || !self.witness.matches_context(meta) {
            return Err(unavailable());
        }
        let phase = self.phase.lock();
        let valid = match &*phase {
            Phase::Ready => agent.is_none() && self.witness.matches_origin(meta),
            Phase::Opened {
                agent: opened,
                allow_origin,
                previous,
                ..
            } => {
                agent == Some(opened.as_str())
                    && ((*allow_origin && self.witness.matches_origin(meta))
                        || (meta.agent_execution.as_ref().is_some_and(|receipt| {
                            receipt.source == *source
                                && receipt.validate(meta.agent_session_id.as_deref()).is_ok()
                        }) && (meta.agent_session_id.as_deref() == Some(opened.as_str())
                            || meta.agent_session_id.as_deref() == previous.as_deref())))
            }
            Phase::Opening { .. } | Phase::Revoked => false,
        };
        valid.then_some(()).ok_or_else(unavailable)
    }

    pub(super) fn validate_agent(
        &self,
        source: &ExecutionSource,
        agent: &str,
    ) -> Result<(), String> {
        if self.witness.destination() != source
            || !matches!(&*self.phase.lock(), Phase::Opened { agent: opened, .. } if opened == agent)
        {
            return Err(unavailable());
        }
        Ok(())
    }

    fn published(
        &self,
        meta: &SessionMeta,
        source: &ExecutionSource,
        agent: &str,
    ) -> Result<(), String> {
        if meta.agent_session_id.as_deref() != Some(agent)
            || !meta
                .agent_execution
                .as_ref()
                .is_some_and(|receipt| receipt.source == *source)
        {
            return Err(
                "execution migration publication has no matching destination receipt".into(),
            );
        }
        self.validate_meta(meta, source, Some(agent))?;
        let mut phase = self.phase.lock();
        match &mut *phase {
            Phase::Opened {
                agent: opened,
                allow_origin,
                previous,
                ..
            } if opened == agent => {
                *allow_origin = false;
                *previous = None;
                Ok(())
            }
            _ => Err(unavailable()),
        }
    }

    async fn begin(
        self: &Arc<Self>,
        resume: Option<&str>,
        fork: bool,
        cwd: &str,
        source: &ExecutionSource,
    ) -> Result<Open, String> {
        if self.witness.destination() != source || !self.witness.matches_cwd(cwd) {
            return Err(unavailable());
        }
        let id = uuid::Uuid::new_v4();
        let verify = {
            let mut phase = self.phase.lock();
            let (intent, allow_origin, previous, verify) = match &*phase {
                Phase::Ready if self.witness.matches_first_open(resume, fork, cwd) => {
                    let agent = resume.ok_or_else(unavailable)?.to_owned();
                    (
                        if fork {
                            Intent::Fork(agent)
                        } else {
                            Intent::Load(agent)
                        },
                        true,
                        None,
                        true,
                    )
                }
                Phase::Opened {
                    agent,
                    allow_origin,
                    ..
                } if !fork => {
                    let intent = match resume {
                        None => Intent::Fresh(agent.clone()),
                        Some(requested) if requested == agent => Intent::Load(agent.clone()),
                        _ => return Err(unavailable()),
                    };
                    (intent, *allow_origin, Some(agent.clone()), false)
                }
                _ => return Err(unavailable()),
            };
            *phase = Phase::Opening {
                id,
                intent,
                allow_origin,
                previous,
            };
            verify
        };
        let opened = Open {
            binding: Arc::clone(self),
            id,
            completed: false,
        };
        if verify {
            let witness = Arc::clone(&self.witness);
            tokio::task::spawn_blocking(move || witness.verify_before_open())
                .await
                .map_err(|_| {
                    "execution history migration verification worker failed".to_owned()
                })??;
        }
        opened.check()?;
        Ok(opened)
    }
}

impl Open {
    fn valid_response(intent: &Intent, agent: &str, outcome: Outcome) -> bool {
        match (intent, outcome) {
            (Intent::Load(expected), Outcome::Loaded) => agent == expected,
            (Intent::Fork(parent), Outcome::Forked | Outcome::Fresh)
            | (Intent::Load(parent) | Intent::Fresh(parent), Outcome::Fresh) => agent != parent,
            _ => false,
        }
    }

    fn validate_identity(agent: &str) -> Result<(), String> {
        if crate::cli_sessions::validate_agent_session_id(agent)? != agent
            || agent.contains(':')
            || agent.len() > 256
        {
            return Err("opened migration agent identity is invalid".into());
        }
        Ok(())
    }

    fn validate_response(&self, agent: &str, outcome: Outcome) -> Result<(), String> {
        Self::validate_identity(agent)?;
        if !matches!(&*self.binding.phase.lock(), Phase::Opening { id, intent, .. }
            if *id == self.id && Self::valid_response(intent, agent, outcome))
        {
            return Err("opened migration agent does not match its requested history".into());
        }
        Ok(())
    }

    fn check(&self) -> Result<(), String> {
        if !matches!(&*self.binding.phase.lock(), Phase::Opening { id, .. } if *id == self.id) {
            return Err(unavailable());
        }
        Ok(())
    }

    pub(super) fn finish(&mut self, agent: &str, outcome: Outcome) -> Result<(), String> {
        Self::validate_identity(agent)?;
        let mut phase = self.binding.phase.lock();
        let Phase::Opening {
            id,
            intent,
            allow_origin,
            previous,
        } = &*phase
        else {
            return Err(unavailable());
        };
        let valid = *id == self.id && Self::valid_response(intent, agent, outcome);
        if !valid {
            return Err("opened migration agent does not match its requested history".into());
        }
        *phase = Phase::Opened {
            id: self.id,
            agent: agent.into(),
            allow_origin: *allow_origin,
            previous: previous.clone(),
        };
        self.completed = true;
        Ok(())
    }
}

impl AcpClient {
    pub(super) fn validate_main_migration_response(
        &self,
        opened: &Option<Open>,
        agent: &str,
        outcome: Outcome,
    ) -> Result<(), String> {
        if let Some(opened) = opened {
            if !self.is_alive() {
                return Err(unavailable());
            }
            opened.validate_response(agent, outcome)?;
        }
        Ok(())
    }

    pub(crate) fn validate_execution_resume(&self, meta: &SessionMeta) -> Result<(), String> {
        if !self.is_alive() {
            return Err("execution transport ended before resume".into());
        }
        let source = self.execution_source();
        if let Some(binding) = &self.main_migration {
            binding.validate_meta(meta, &source, self.agent_session_id().as_deref())?;
        } else if let Some(receipt) = &meta.agent_execution {
            receipt.validate(meta.agent_session_id.as_deref())?;
            if receipt.source != source || matches!(source, ExecutionSource::Unavailable) {
                return Err(
                    "execution home changed; history migration is required before resume".into(),
                );
            }
        }
        Ok(())
    }

    pub(crate) fn validate_execution_publication(
        &self,
        meta: &SessionMeta,
        agent: &str,
    ) -> Result<(), String> {
        self.validate_execution_resume(meta)?;
        if let Some(binding) = &self.main_migration {
            binding.validate_agent(&self.execution_source(), agent)?;
        }
        Ok(())
    }

    pub(crate) fn note_execution_publication(&self, meta: &SessionMeta) -> Result<(), String> {
        if let Some(binding) = &self.main_migration {
            let agent = self.agent_session_id().ok_or_else(unavailable)?;
            binding.published(meta, &self.execution_source(), &agent)?;
        }
        Ok(())
    }

    pub(super) async fn begin_main_migration(
        &self,
        resume: Option<&str>,
        fork: bool,
        cwd: &str,
    ) -> Result<Option<Open>, String> {
        let Some(binding) = &self.main_migration else {
            return Ok(None);
        };
        let opened = binding
            .begin(resume, fork, cwd, &self.execution_source())
            .await?;
        if !self.is_alive() {
            return Err(unavailable());
        }
        Ok(Some(opened))
    }

    pub(super) fn finish_main_migration(
        &self,
        opened: &mut Option<Open>,
        agent: &str,
        outcome: Outcome,
    ) -> Result<(), String> {
        if let Some(opened) = opened {
            if !self.is_alive() {
                return Err(unavailable());
            }
            opened.finish(agent, outcome)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "main_migration_tests.rs"]
mod tests;

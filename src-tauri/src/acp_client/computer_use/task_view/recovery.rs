//! Final resume publication pins the original ACP pointer, with no native I/O.
use super::*;
use grok_computer_use_core::broker::{
    HostTaskResume, HostTaskResumeCancel, HostTaskResumeReady, HostTaskResumeReceipt,
};
use std::time::Duration;

pub(crate) enum TaskResumeStarted {
    InProgress,
    Prepare(Box<TaskResumeCall>),
}

pub(crate) struct TaskResumeCall {
    execution: TaskExecutionCall,
    resume: HostTaskResume,
}

impl TaskExecutionCall {
    pub(crate) fn reserve_resume(self) -> Result<TaskResumeStarted, String> {
        let ready = self.with_original(|| {
            self.original
                .broker
                .reserve_host_task_resume(&self.original.token, &self.address)
                .map_err(|_| "Computer Use resume unavailable or execution changed".into())
        })?;
        Ok(match ready {
            HostTaskResumeReady::InProgress => TaskResumeStarted::InProgress,
            HostTaskResumeReady::Prepare(resume) => {
                TaskResumeStarted::Prepare(Box::new(TaskResumeCall {
                    execution: self,
                    resume: *resume,
                }))
            }
        })
    }

    fn with_original<R>(&self, operation: impl FnOnce() -> Result<R, String>) -> Result<R, String> {
        let client = self
            .client
            .upgrade()
            .ok_or("Computer Use request owner retired")?;
        let slots = client.computer_use_connections.lock();
        if !slots
            .get(&self.app_session)
            .and_then(|slot| slot.transport.as_ref())
            .is_some_and(|current| {
                Arc::ptr_eq(current, &self.original)
                    && client.is_alive()
                    && current.bound_to(&self.agent)
            })
        {
            return Err("Computer Use request owner retired".into());
        }
        operation()
    }
}

impl TaskResumeCall {
    pub(crate) fn cancel_handle(&self) -> HostTaskResumeCancel {
        self.resume.cancel_handle()
    }

    pub(crate) fn prepare_native(&mut self, timeout: Duration) -> Result<(), String> {
        self.execution.check_original()?;
        self.resume
            .prepare_native(timeout)
            .map_err(|_| "Computer Use original scope or recovery observation unavailable")?;
        self.execution.check_original()
    }

    /// Borrowing leaves failure cleanup to the owned worker after all App locks
    /// are released. SessionManager pins its own endpoint around this call.
    pub(crate) fn commit(&mut self) -> Result<HostTaskResumeReceipt, String> {
        let resume = &mut self.resume;
        self.execution.with_original(|| {
            resume
                .commit()
                .map_err(|_| "Computer Use resume retired before publication".into())
        })
    }
}

//! Fixed Host scripts in a native WebView2 isolated world. No remote-debug port,
//! page-world fallback or numeric execution-context fallback is exposed.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{mpsc::Sender, Arc};

use serde_json::{json, Value};
use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2DevToolsProtocolEventReceiver,
};
use windows::core::PCWSTR;

mod native_world;
#[cfg(feature = "computer-use-probe")]
pub(crate) mod probe_fault;
pub(crate) mod process;
mod process_identity;
pub(crate) use native_world::NativeWorld;
mod setup;

use super::ScriptOperation;

const MAX_REPLY: usize = 1024 * 1024;
const MAX_WORLD_ATTEMPTS: usize = 4;
// A renderer-side deadline belongs to this exact evaluation. A business timeout
// or Stop must never issue Runtime.terminateExecution: that unscoped command can
// terminate the next script after the owned one has already completed. This
// backstop does not replace immediate, operation-specific Stop cancellation.
const NATIVE_SCRIPT_BUDGET_MS: u64 = 12_000;

struct Subscription {
    receiver: ICoreWebView2DevToolsProtocolEventReceiver,
    token: i64,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        // Releasing a diagnostic subscription never releases script occupancy.
        unsafe {
            let _ = self
                .receiver
                .remove_DevToolsProtocolEventReceived(self.token);
        }
    }
}

struct Job {
    ticket: uuid::Uuid,
    webview: ICoreWebView2,
    world: Arc<NativeWorld>,
    generation: u64,
    operation: ScriptOperation,
    current: Box<dyn Fn() -> bool>,
    script: String,
    reply: RefCell<Option<Sender<Result<String, String>>>>,
    setup: RefCell<Option<setup::Watch>>,
    setup_deadline: RefCell<Option<setup::Timer>>,
    #[cfg(feature = "computer-use-probe")]
    fault: RefCell<Option<probe_fault::Fault>>,
}

/// Must be called on the owning native thread. The completion chain, not its
/// waiting business caller, owns the native ticket. No callback means not idle.
pub(crate) fn dispatch(
    webview: ICoreWebView2,
    world: Arc<NativeWorld>,
    generation: u64,
    operation: ScriptOperation,
    current: impl Fn() -> bool + 'static,
    script: String,
    reply: Sender<Result<String, String>>,
) {
    Job::new(
        webview, world, generation, operation, current, script, reply,
    )
    .begin();
}

impl Job {
    fn new(
        webview: ICoreWebView2,
        world: Arc<NativeWorld>,
        generation: u64,
        operation: ScriptOperation,
        current: impl Fn() -> bool + 'static,
        script: String,
        reply: Sender<Result<String, String>>,
    ) -> Rc<Self> {
        Rc::new(Self {
            ticket: uuid::Uuid::new_v4(),
            webview,
            world,
            generation,
            operation,
            current: Box::new(current),
            script,
            reply: RefCell::new(Some(reply)),
            setup: RefCell::new(None),
            setup_deadline: RefCell::new(None),
            #[cfg(feature = "computer-use-probe")]
            fault: RefCell::new(None),
        })
    }

    fn begin(self: &Rc<Self>) {
        if !self.current() {
            self.fail("dispatch retired before execution");
            return;
        }
        let reply = self
            .reply
            .borrow()
            .as_ref()
            .expect("current job has a reply")
            .clone();
        if let Err(error) =
            self.world
                .admit(self.ticket, self.generation, self.operation.clone(), reply)
        {
            self.finish(Err(error));
            return;
        }
        match self.world.reserve(self.generation) {
            Ok(Some(context)) => self.evaluate(context),
            Ok(None) => setup::begin(self),
            Err(error) => self.finish(Err(error)),
        }
    }

    fn current(&self) -> bool {
        self.reply.borrow().is_some() && (self.current)() && self.operation.check().is_ok()
    }

    fn finish(&self, result: Result<String, String>) {
        let reply = self.reply.borrow_mut().take();
        if let Some(reply) = reply {
            self.world.completed(self.ticket);
            let deadline = self.setup_deadline.borrow_mut().take();
            drop(deadline);
            let setup = self.setup.borrow_mut().take();
            drop(setup);
            self.operation.finished();
            let _ = reply.send(result);
        }
    }

    fn fail(&self, stage: &str) {
        self.finish(Err(format!("WebView isolated script {stage}")));
    }

    fn call(
        self: &Rc<Self>,
        method: &'static str,
        parameters: Value,
        next: impl FnOnce(Rc<Self>, Value) + 'static,
    ) {
        if !self.current() {
            self.fail("dispatch retired before execution");
            return;
        }
        let completed = self.clone();
        let handler =
            CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |status, raw| {
                if completed.reply.borrow().is_none() {
                    return Ok(());
                }
                if status.is_err() || raw.len() > MAX_REPLY {
                    completed.fail("outcome unknown after native error");
                } else if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                    if value.get("error").is_some() {
                        completed.fail("outcome unknown after protocol rejection");
                    } else {
                        next(completed, value);
                    }
                } else {
                    completed.fail("outcome unknown after invalid native reply");
                }
                Ok(())
            }));
        let method = wide(method);
        let parameters = wide(&parameters.to_string());
        if unsafe {
            self.webview.CallDevToolsProtocolMethod(
                PCWSTR(method.as_ptr()),
                PCWSTR(parameters.as_ptr()),
                &handler,
            )
        }
        .is_err()
        {
            // The native API rejected this command before it was dispatched.
            self.fail("native dispatch unavailable");
        }
    }

    fn evaluate(self: &Rc<Self>, context: String) {
        process_identity::capture(self, context);
    }

    fn execute(self: &Rc<Self>, context: String) {
        self.call(
            "Runtime.evaluate",
            evaluation_parameters(&self.script, &context),
            |job, evaluated| {
                #[cfg(feature = "computer-use-probe")]
                if probe_fault::suppress(&job, probe_fault::Phase::EvaluationReply) {
                    return;
                }
                if evaluated.get("exceptionDetails").is_some() {
                    job.fail("outcome unknown after execution exception");
                    return;
                }
                match evaluated.pointer("/result/value") {
                    Some(value) if value.is_object() => job.finish(Ok(value.to_string())),
                    _ => job.fail("outcome unknown after invalid result"),
                }
            },
        );
    }
}

fn evaluation_parameters(script: &str, context: &str) -> Value {
    json!({
        "expression": script,
        "uniqueContextId": context,
        "returnByValue": true,
        "awaitPromise": false,
        "timeout": NATIVE_SCRIPT_BUDGET_MS
    })
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn context_identity(value: &Value, world: &str, frame: &str) -> Option<(i64, String)> {
    let context = value.get("context")?;
    if context.get("name")?.as_str()? != world
        || context.pointer("/auxData/frameId")?.as_str()? != frame
        || context.pointer("/auxData/isDefault")?.as_bool()?
        || context.pointer("/auxData/type")?.as_str()? != "isolated"
    {
        return None;
    }
    let unique = context.get("uniqueId")?.as_str()?;
    let id = context.get("id")?.as_i64()?;
    (id > 0 && !unique.is_empty() && unique.len() <= 256).then(|| (id, unique.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadline_is_native_and_scoped_to_the_exact_unique_context_evaluation() {
        let parameters = evaluation_parameters("({fixture: true})", "native-document-1");
        assert_eq!(parameters["timeout"], json!(12_000));
        assert_eq!(parameters["uniqueContextId"], json!("native-document-1"));
        assert_eq!(parameters["expression"], json!("({fixture: true})"));
        assert_eq!(parameters["awaitPromise"], json!(false));
        assert!(parameters.get("contextId").is_none());
    }

    #[test]
    fn context_requires_native_unique_identity_exact_frame_and_isolated_world() {
        let good = json!({"context": {"id": 7, "uniqueId": "native-unique", "name": "owned",
            "auxData": {"frameId": "main", "isDefault": false, "type": "isolated"}}});
        assert_eq!(
            context_identity(&good, "owned", "main"),
            Some((7, "native-unique".into()))
        );
        for (pointer, wrong) in [
            ("/context/uniqueId", json!("")),
            ("/context/name", json!("page")),
            ("/context/auxData/frameId", json!("other")),
            ("/context/auxData/isDefault", json!(true)),
            ("/context/auxData/type", json!("default")),
        ] {
            let mut bad = good.clone();
            *bad.pointer_mut(pointer).unwrap() = wrong;
            assert!(context_identity(&bad, "owned", "main").is_none());
        }
        let mut numeric_only = good;
        numeric_only["context"]
            .as_object_mut()
            .unwrap()
            .remove("uniqueId");
        assert!(context_identity(&numeric_only, "owned", "main").is_none());
    }

    #[test]
    fn native_world_reuses_only_current_document_and_bounds_failed_setup() {
        let world = NativeWorld::default();
        for _ in 0..MAX_WORLD_ATTEMPTS {
            assert_eq!(world.reserve(1).unwrap(), None);
        }
        assert!(world.reserve(1).is_err());
        assert_eq!(world.reserve(2).unwrap(), None);
        assert!(!world.publish(1, "stale".into()));
        assert!(world.publish(2, "current".into()));
        assert_eq!(world.reserve(2).unwrap().as_deref(), Some("current"));
        assert!(world.reserve(1).is_err());
    }
}

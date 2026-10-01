# ADR: synchronous worker HTTP owns its runtime off Tokio workers

Date: 2026-09-13; revised 2026-09-20
Status: accepted
Context: F1 App-shell revoke panic

## Decision

Keep the managed Playwright worker client as a **synchronous** loopback POST
contract. The exchange now uses async reqwest inside a request-owned,
current-thread Tokio runtime. **Create, send, and drop** the client and runtime
only on a plain OS thread outside any existing Tokio runtime.

When `tokio::runtime::Handle::try_current()` is `Ok`, `bounded_loopback_post`
runs the exchange inside `std::thread::scope`. When no Tokio runtime is
present (unit tests, probe threads), it runs inline. The plain thread is joined
before returning. No detached HTTP thread, shared background runtime, or
`shutdown_background` is permitted.

`bounded_loopback_post_cancellable` selects the complete send/header/body
exchange against the admitted cancellation token. Cancellation is sticky and
wakes every clone; the runtime and its socket tasks are dropped before the
synchronous call returns. Literal loopback addresses and a static dual-stack
localhost override avoid an uncancellable system-DNS task. Redirects and proxy
inheritance remain disabled. Content-Length and streamed bodies are bounded.

Pre-cancel is `not_started`; cancellation after entering the exchange and all
timeouts are `unknown`. No automatic retry is introduced. A body timeout now
has explicit `worker_timeout` diagnostics rather than the old generic
`invalid_worker_response`, with the same unknown completion classification.

The non-cancellable entry delegates to the same transport using a fresh token.
Product managed calls now carry the Broker-admitted token, owner, revision and
worker instance through a request-local Host/client. Capability health requests
also use that token. Independent lifecycle traffic remains non-cancellable by
the business token. Remote pause/status/resume now gates recovery; socket exit
alone never permits resume. See the [request binding checkpoint](2026-09-20-computer-use-managed-request-checkpoint.md)
for current evidence and the remaining native-action/UI acceptance gaps.

Session grant `revoke` / `fail` release the grants mutex **before**
`ComputerUseBroker::request_stop`, so Broker/session locks are not held across
the blocking HTTP.

App-shell async harness additionally `spawn_blocking`s `sessions::revoke` so
the Tokio worker is not stalled for the duration of stop. That is defense in
depth; the HTTP boundary is the required invariant.

## Why not convert the entire Host contract to async

The Broker, `ManagedBrowserWorker`, and `ExistingTabHost::cancel_run` are
synchronous contracts used from Tauri commands, probes, and tests. Converting
the whole stack to async would hold locks across `.await` unless every call
site is rewritten. A dedicated off-Tokio runtime preserves the sync API while
making the HTTP exchange cancellable without abandoning blocking I/O.

## Why not only `tokio::task::spawn_blocking`

Tokio blocking-pool threads still report `Handle::try_current() == Ok`.
A nested runtime cannot safely be blocked/dropped in that context. The original
`reqwest::blocking` version produced `Cannot drop a runtime in a context where
blocking is not allowed`; the new explicitly owned runtime must obey the same
boundary. A plain `std::thread` does not see a current Tokio handle.

## Cancellation integration gate

Closing an HTTP socket proves only local I/O termination. It neither retracts
an already dispatched browser input nor confirms that Playwright has stopped.
The Broker must retain `stop_cleanup_pending` until authenticated remote
cleanup succeeds. A gated HTTP/Broker test now verifies that distinction.

Pause cancels the admitted token and invokes versioned worker pause outside
Broker/Host mutexes. Managed `is_idle` checks authenticated remote state before
resume. A resume admission captures the exact local Pause epoch; a later Pause
revokes it even if the worker revision has not yet advanced. A lost or superseded
resume acknowledgement remains fenced, without automatic retry. Terminal Stop
uses the pinned worker and independent cancel-run, including after an unknown
resume. A successful cancel-run plus settled local profile reservations is the
cleanup proof; an idle response alone never proves browser contexts closed.

The worker side now has revision-bound pause/status/resume and physical request
accounting; see the [worker quiescence checkpoint](2026-09-20-computer-use-worker-quiescence-checkpoint.md)
for the exact version 1 contract and initial evidence. At that checkpoint the
Host integration was still outstanding; the newer request-binding checkpoint
records actual App Broker integration. Worker-only fixtures still do not prove
the complete App UI or native-action cancellation matrix.

The [native cancellation checkpoint](2026-09-20-computer-use-native-cancellation-checkpoint.md)
adds actual Broker/source/seed cases for download, navigation click and slow
screenshot. A native download without a Download handle can outlive its event
waiter. The owned context must close before physical admission is released in
that case; the on-disk profile is retained, but its tabs require reopening. An
identified Download uses native cancel and keeps its page. Neither local socket
EOF nor expiration of the event waiter proves the browser transfer ended.

The subsequent [Host profile checkpoint](2026-09-20-computer-use-host-profile-lifecycle-checkpoint.md)
adds strict Rust lifecycle client methods and fixes Host opening/Stop/clear
ownership races, including uncertain open replies. Interrupted request bodies
are now contained by the worker's route error boundary. These are prerequisites:
the adapter/Broker control transaction and admitted business revision/token
propagation were not yet complete at that stage. A subsequent Broker resume transaction
reserves admission, queries the adapter outside the Broker lock, and rechecks
generation before publication; later Pause/Stop invalidates the transaction.
The request-binding batch connects the remote lifecycle controls to that
transaction. Keep all lifecycle HTTP outside Broker/Host locks.

## Non-goals

- Do not swallow the panic with a hook.
- Do not equate cancellation with remote quiescence or weaken
  `unknown`/`stop_requested` semantics.
- Do not auto-replay actions after timeout.

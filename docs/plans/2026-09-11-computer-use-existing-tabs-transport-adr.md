# ADR: Existing Tabs v1 transport

Date: 2026-09-11  
Status: pairing/share and typed text-observation transport implemented; v1 incomplete  
Workspace: `H:\aicoding\grok-app-computer-use`

## Decision

v1 uses **authenticated loopback**, not Chrome Native Messaging. The Host registry
and MV3 pairing/lease slices exist, as do authenticated candidate-update endpoints.
The user Share candidate flow is implemented, with automatic browser evidence;
native toolbar activeTab acceptance is still pending. The bounded poll/result
transport now implements main-frame text observation. Screenshot validation,
action execution and the production ExistingTab adapter remain unfinished.
For the implemented proof protocol and its limitations, the 2026-09-20 revision
of [the pairing ADR](2026-09-13-computer-use-existing-tabs-pairing-adr.md) takes precedence.

## Why loopback for v1

| Criterion | Native Messaging | Authenticated loopback |
| --- | --- | --- |
| Windows/macOS/Linux install | Extra native host manifest per browser + per OS path | App already binds loopback for Computer Use MCP |
| Chrome/Edge MV3 | Supported, but host registration differs per browser | Same extension code; browser only loads MV3 |
| Secret exposure | stdin JSON, not URL | Must **not** put pairing secret in URL query/fragment/DOM/log |
| Extension id bind | Native host can check sender id | Host allowlists expected extension id + Origin |
| App instance/session/run/tab/document/connection generation | Possible | Registry fields exist; real transport binding is still required |
| Revoke/rotate | Kill native host | Drop challenge + pairing token; rotate |
| Browser restart / App restart | Native host relaunch | Re-pair; grants do not survive |
| Enterprise policy | Native host allowlists | Extension admin policy still required for force-install |
| Testability | Hard in CU probe | Chrome for Testing already has a fixture |
| Min permission | Native host is a separate binary | MV3: `activeTab` + fixed `scripting`; no `<all_urls>`, even after Share |

Native Messaging is a valid later transport (especially if enterprise blocks loopback). It is not v1: it adds a second install surface and does not reuse the existing Host pairing store.

## v1 rules

- Pairing secret never in URL query, fragment, ordinary web-page DOM, or logs;
  the one-time code is intentionally shown in trusted App UI and entered in the
  trusted extension popup. Session keys never enter either UI's DOM.
- Bind only loopback + expected extension id.
- Challenge is one-shot, short TTL, burned after use.
- App UI and extension UI both confirm.
- Replay / wrong instance / wrong origin / wrong extension / expired challenge → reject.
- Host service is process-wide; probes must not `ExistingTabHost::new()` and stuff responses as the product path.

## Edge

Edge isolation-profile E3 is a follow-on. If a safe isolated Edge binary/profile is missing, that row is `blocked_external`. Chrome for Testing remains the v1 E3 browser.

## C2 text-observation protocol (2026-09-20)

- `POST /cu/extension-poll`: Bearer + full PairingConnection in a strict body.
  One active long poll, at most two seconds; authenticate again every 50ms so
  revoke/feature-off can interrupt it. Poll is not a heartbeat and cannot extend
  a dead connection's lease. No public page/control endpoint can enqueue work.
- `POST /cu/extension-result`: strict ExtensionResult, route-specific 128KiB
  limit. Ordinary IPC retains its 64KiB limit. Poll/result have their own bounded
  rate bucket (40 requests/800ms), separate from user pairing's 8/800ms bucket.
- Host queue: at most eight requests total, one per tab, one delivered request
  per connection. Ten-second monotonic lifetime, one delivery, no redelivery.
  A caller must own a live borrowed session/run/tab grant before enqueueing.
- Each request binds protocol, UUID requestId, monotonic sequence, wall-clock
  deadlineMs (for the extension), full App/connection identity, session, runId,
  tabId, documentGeneration, grantGeneration and a typed command. Current command
  is only `observe {snapshotId}`. No arbitrary JS, expression or generic params.
- A result echoes the complete request and is either a typed observation or a
  small rejection enum. The Host compares all fields under the grant registry
  lock, rechecks the live grant and monotonic deadline, then consumes once.
  Wrong request/session/tab/connection/document, duplicate or undelivered results
  cannot complete an observation. Grants cancelled during an await cannot return
  a previously valid result to the caller.
- Worker polling starts only with a fresh pairing; epoch changes abort the old
  loop. Poll and result HTTP calls have a five-second deadline and bounded replies.
  A short inter-poll delay also bounds a malfunctioning local server. Result
  submission failure is never retried; next poll revalidates the connection.
- The observer requires an explicitly shared current visible HTTP(S) main frame.
  It validates actual Chrome documentId before/after fixed isolated-world code.
  No frame traversal, field values, passwords, hidden descendants or raw HTML.
  Visible text, semantic control refs, title/URL and viewport are bounded.
  Up to 64 weak element refs are retained for one snapshot in the isolated world.
- The App's private probe control can grant a fixture candidate and request a
  real observation. This is an integration oracle, not production App-picker
  authorization, model/MCP acceptance, or evidence of action execution.

Before C2 is complete, add correct-target screenshot capture with pre/post active
tab/document checks, real navigation/timeout/result-race HTTP tests, and integration
with the production adapter's observation commit/cancellation path. C3 must add
typed actions and revocation before side effects; observation-only queue safety
does not establish action-at-most-once semantics under all browser lifecycle races.

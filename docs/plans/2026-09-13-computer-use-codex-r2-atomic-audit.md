# Computer Use R2 atomic lifecycle audit

Date: 2026-09-13

Workspace: `H:\aicoding\grok-app-computer-use`

Branch: `feat/computer-use-implementation`

HEAD: `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`

Pre-document code fingerprint: `26406035517b6856e1deb6a9e7b3e2103b48391bcfa4e72215d02d6766b34fd5`

Verdict: **partial - not releasable**

## Audit boundary

This audit inspected the current dirty worktree after Grok reported the previous
task complete. It preserved all tracked, untracked, ignored, and evidence files.
It did not reset, restore, clean, stash, commit, push, open a PR, merge, tag, or
release.

The fingerprint above was recomputed immediately before these documents were
created and matched the fingerprint stored by the current App-shell evidence
run. Adding this audit and its companion plans changes the worktree, so it must
not be reused as the next implementation fingerprint.

## Findings

### P0. ACP receives a Computer Use MCP server but no lifecycle path removes it

The current authorization command performs:

```text
begin -> provision/bind -> Broker authorize -> activate session
      -> SessionManager::attach_computer_use -> complete
```

`SessionManager::attach_computer_use` rebuilds the base MCP catalog, appends
`grok-computer-use`, and calls ACP `_x.ai/session/update_mcp_servers`.

There is no matching `detach_computer_use`, desired-state reconciler, or record
of what Computer Use entry is currently installed in the live ACP session.
Every stop/revoke/failure path only revokes Broker state, the loopback token,
and the process-local session flag.

Evidence:

- `src-tauri/src/commands/computer_use.rs:287-377` owns the authorization
  transaction; attach occurs at line 346, after Broker activation.
- `src-tauri/src/session_manager/computer_use.rs:6-54` implements attach only.
- `src-tauri/src/computer_use/sessions.rs:55-59` clears the IPC session and
  enabled flag, but cannot update the live ACP catalog.
- `src-tauri/src/commands/computer_use.rs:448-461` handles UI stop without the
  `SessionManager` and therefore cannot perform an async ACP reconciliation.
- Feature-off, stop, session switching, deletion, soft respawn, logout, and
  App shutdown ultimately use process-local revoke functions. Context
  invalidation is weaker: it invalidates the Broker target but does not clear
  the session flag, IPC token, or ACP catalog entry.

The most important race is:

```text
authorize A                    user Stop
-----------                    ---------
issue IPC token
start ACP update     ------->  revoke Broker/token/flag
ACP update returns with CU entry installed
complete A fails because ticket is stale
fail_checked sees stale ticket and does nothing
```

The leaked MCP token is revoked, so the stale tool should fail closed at IPC.
That is useful defense in depth, but it is not a completed lifecycle: the tool
can remain visible in the agent, spawn a dead MCP child, and survive until a
later catalog update or process restart.

Required fix: make the ACP MCP catalog desired-state driven and serialized per
App session. Revocation must fence authority synchronously, then converge the
live catalog to a state without `grok-computer-use`. A late attach must observe
that it lost ownership and reconcile again before it may return.

### P0. `attempt_id` and `selector_revision` are validated and echoed, not owned by Host

The UI now sends a useful attempt ID and selector revision. The Host validates
their syntax, but does not store either value in an authoritative registry.
They are copied back into the DTO unchanged.

`SessionGrants` has its own private nonce and a `pending` bit, which protects
some publish/fail callbacks. It does not model the external attempt, selector
revision, current phase, cancellation, MCP catalog generation, or compensation
state. Frontend `commandRevision` prevents some stale React updates, but it
cannot cancel Host work after component unmount, chat switch, App shutdown, or
stop followed by a new authorization.

Evidence:

- `src-tauri/src/commands/computer_use.rs:95-115` performs format checks only.
- `src-tauri/src/commands/computer_use.rs:354-355` returns the caller's values.
- `src-tauri/computer-use-core/src/session_grants.rs:7-18` stores only session,
  run, private nonce, and pending.
- `src/components/computer-use/ComputerPanel.tsx:151-178` keeps the latest
  command revision inside one mounted React component.

Required fix: add a Host-owned, monotonically generated attempt registry. The
UI attempt ID remains an opaque correlation value; the Host generation is the
authority. Every step and cleanup callback must prove it still owns the same
session, run, attempt generation, and target binding before publishing or
removing state.

### P0. Authorization compensation ends at Broker state and cannot prove ACP convergence

The new single Tauri authorization command is a real improvement, but it is not
yet an atomic transaction across all owners. Its error arm unbinds a matching
WebView and calls `fail_checked`; it does not know whether ACP accepted the MCP
catalog update. There is no durable `mcp_attach_started`, `mcp_attached`, or
`detach_pending` state.

Consequences:

- a timeout can mean the ACP update applied even when the caller saw an error;
- repeated stop cannot retry an MCP detach because no detach operation exists;
- cleanup success can be reported while ACP still advertises the tool;
- an old cleanup added later could remove a newer run's MCP entry unless it is
  generation fenced;
- generic extension MCP updates can race Computer Use updates because there is
  no shared catalog reconciler.

Required fix: one coordinator must own attempt state, Broker grant, IPC token,
surface resource, and desired MCP catalog state. Cleanup is not complete until
all required owners converge or a visible `cleanup_pending` state is retained.

### P1. The new `20+20` evidence passes while two UI slash scenarios fail

The current App-shell harness collects both slash failures in `errors`, then
explicitly filters `missing-hook` and `slash_` errors out of the blocking set at
`src-tauri/src/computer_use/app_shell.rs:567-573`.

The observed failures are:

```text
slash_desktop: missing-hook:chrome-error://chromewebdata/:complete:undefined
slash_managed_browser: missing-hook:chrome-error://chromewebdata/:complete:undefined
```

The run had no frontend dev/build server, so this does not establish a product
slash regression. It does mean the run cannot be cited as slash/UI entry-point
evidence. The manifest currently records one combined F3 pass instead of
separate Managed, WebView, lifecycle, and slash scenarios. `state.json` remains
at phase F0 and is not derived from the newly appended F3 records.

Required fix: split scenario records, make required UI rows actually blocking,
serve a real built frontend for slash checks, and generate state from manifest
rather than hand-maintaining aggregate status.

### P1. Existing Tabs remains a pairing skeleton, not a product surface

The extension service worker only stores, reads, and clears an in-memory key.
It has no user Share action, tab offer, authenticated transport, observation,
typed action, heartbeat, reconnect, navigation generation, cancellation, or
return-on-stop protocol.

Evidence:

- `tools/computer-use-extension/sw.js:1-21` is the entire worker behavior.
- `tools/computer-use-extension/manifest.json:6-17` has no stable signing key,
  popup/side panel, storage, `activeTab`, scripting, or optional origin flow.
- `src-tauri/computer-use-core/src/browser/host/mod.rs:101` still hardcodes
  `pw-ext-installed`.

There is no production Existing Tabs executor registered with the Broker. It
must remain unavailable rather than borrowing the Managed Browser path or
falling back to Desktop.

### P1. Pairing still allows a local-process race

The core object contains a secret-based response helper, but the HTTP flow does
not use it:

- no `Origin` is accepted by `src-tauri/computer-use-core/src/ipc.rs:337`;
- `PairingConfirm.response` is discarded at line 490;
- `ext` is optional;
- `/cu/pairing-session` needs only public nonce/instance values after two
  booleans are set;
- `complete_dual` mints the session key without a possession proof.

Origin, Host, and CORS cannot authenticate a native local process because it
can forge browser-shaped headers. Pairing needs a one-time secret transferred
through an explicit user gesture and a constant-time MAC bound to nonce,
instance, browser-specific extension identity, and connection generation.

### P1. Packaging and cross-platform v1 are still incomplete

- Generic Tauri resources remain empty; the Windows override bundles only the
  Windows runtime seed and omits the extension.
- `pack-targets.json` marks macOS arm64, macOS x64, and Linux x64 `not_run`.
- macOS still rejects key, scroll, and drag.
- Linux X11 still rejects type/set-value/key/scroll/drag.
- GNOME Wayland keeps `native_wayland=false`.
- No clean installed lifecycle has established independence from repository
  files, PATH Node, or development fixtures.

The user selected Windows, macOS, Linux X11, and GNOME Wayland for v1. These
rows must remain independent and honest.

### P2. Quality and maintainability debt remains material

Current large production/test-support files include:

| lines | file |
| ---: | --- |
| 2202 | `src-tauri/computer-use-core/src/runtime.rs` |
| 1707 | `src-tauri/src/computer_use/app_shell.rs` |
| 1292 | `src-tauri/computer-use-core/src/broker.rs` |
| 1222 | `src-tauri/src/computer_use/webview.rs` |
| 1073 | `src-tauri/src/computer_use/windows_adapter.rs` |

`cargo check -p grok-app --lib --locked --offline` passes but emits 36
dead-code warnings. The pairing frontend test remains primarily a source
string-presence check. Line-ending policy is still implicit: the worktree has
no `.gitattributes` while local Git uses `core.autocrlf=true`.

## What Grok actually completed

The previous completion claim is too broad, but the implementation work is not
throwaway. These are real advances:

1. `SurfaceExecutorRegistry` now exists and fails closed for an unregistered
   non-Desktop surface.
2. Authorized bindings carry surface and executor generation; generic Broker
   observe, act, alive, capabilities, preview, pause, stop, and release resolve
   the authorized executor.
3. Managed Browser has a real adapter over the App-owned worker and does not
   acquire the Desktop lease.
4. App WebView is registered as a product executor and the scripted App-shell
   exercises `computer_observe` and `computer_act` through MCP and Broker.
5. The UI no longer composes separate provision/grant and authorize calls for
   Managed/WebView selection; it calls one typed Host authorization command.
6. Surface-specific cleanup is substantially better: Broker release calls the
   matching adapter, and WebView release is scoped by session and run.
7. Core tests increased to 302 and the new Managed/product surface contracts
   pass.

This corrects the earlier post-finalization audit: Managed and WebView generic
execution are no longer Desktop-only, and the current WebView happy path no
longer directly calls `wv.observe/wv.act`. The remaining direct WebView call in
the fail-closed iframe probe is test support, not the happy-path product claim.

## Current verification

The following checks were run against the exact pre-document fingerprint:

| check | result | interpretation |
| --- | --- | --- |
| Rust core | 302/302 passed | unit/integration contracts, not every product lifecycle |
| driver tests | 12/12 passed | shared driver contract |
| focused frontend authorization tests | 16/16 passed | current component/API behavior |
| TypeScript | `pnpm exec tsc --noEmit` passed | compile only |
| Rust fmt | passed | formatting |
| core Clippy `-D warnings` | passed | core crate clean |
| App check | passed with 36 warnings | App compiles; warning debt remains |
| `git diff --check` | no whitespace errors | LF/CRLF conversion warnings remain |
| short App-shell | Managed 2/2, WebView 2/2 | scripted App-shell E3 |
| repeated App-shell | Managed 20/20, WebView 20/20 | scripted App-shell E3 |

Repeated-run evidence:

- run: `tools/computer-use-probe/.run/finalization/20260912T231647Z-seven`
- report: `logs/F3F4.appshell-07-report.json`
- log: `logs/F3F4.appshell-07.log`
- elapsed: 715,752 ms
- App exit: 0
- Tokio runtime-drop panic: false
- WebView fail/reclaim: true
- isolated Desktop lease after exit: absent
- matching App/Node/Chromium process after exit: none found
- log bytes/SHA-256: matched manifest exactly
- slash rows: both failed and are not part of this pass

The harness uses an isolated home and an ACP stub, then spawns its own MCP
client from the generated entry. It proves the branch-built Host/MCP route and
fixture postconditions. It does not prove that a real Grok model selected and
used the tools, an installed package works, Existing Tabs works, or another OS
works.

## Corrected status matrix

| area | status | highest defensible evidence |
| --- | --- | --- |
| Broker surface execution | implemented for Desktop/Managed/WebView | E1/E2 plus scripted E3 |
| Managed Browser on Windows | happy path locally validated | scripted App-shell E3, 20/20 |
| App WebView on Windows | happy path locally validated | scripted App-shell E3, 20/20 |
| Desktop on Windows | prior native fixture valid; current combined run only 2 rounds | E3 fixture |
| Atomic Host authorization | partial | single command exists; latest-attempt and ACP compensation absent |
| MCP attach/detach | failed/incomplete | attach works; detach/reconcile absent |
| UI slash entry | not established in current run | two non-blocking failures |
| Existing Tabs | incomplete | skeleton/unit only |
| Pairing security | security-blocked | local-process possession proof absent |
| Windows installed package | not run | source/runtime seed only |
| macOS arm64/x64 | incomplete/not run | source prototype |
| Linux X11 | incomplete/not run | source prototype |
| GNOME Wayland | blocked/incomplete | `native_wayland=false` |
| Real model | not run | requires explicit user approval |
| Release freeze/soak | not run | current state is not scenario-derived |

## Required next order

Do not start by adding more happy-path rounds. The next implementation tranche
must complete these in order:

1. Host-owned AttemptRegistry and cancellation transaction.
2. Serialized desired-state MCP catalog reconcile with attach/detach symmetry.
3. Unified revoke/context/feature/session/exit lifecycle and compensation.
4. Phase-specific fault injection and negative behavior tests.
5. Fresh fingerprint, 2-round smoke, 20-round lifecycle matrix, and active
   fault soak.
6. Only after the lifecycle gate passes: secure Existing Tabs pairing and MV3
   transport.

The companion execution document and prompt define those gates. Until they
pass, the only accurate overall result is **partial - not releasable**.

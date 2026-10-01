# Computer Use post-finalization audit

Date: 2026-09-13

Workspace: `H:\aicoding\grok-app-computer-use`

Branch: `feat/computer-use-implementation`

HEAD: `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`

Pre-document audit fingerprint: `70e0cced20d5f7162f11894160a31a07e65ffd8995bc7503ae3d0edfda502b71`

Verdict: **partial, not releasable**

## Audit boundary

This audit inspected the current dirty worktree and ran focused verification. It did not reset, restore, clean, stash, commit, push, create a PR, merge, or alter another worktree. The three resident `grok.exe` processes were not stopped.

During the audit, external source writes were observed at 04:43, 04:49, and 04:57-04:58. Evidence state was updated again at 05:09-05:10. The source then remained stable through the pre-document snapshot at 05:12. Therefore the old F0 fingerprint `4bd5c921...` is not current and none of its freeze claims can be reused. The fingerprint above is an audit snapshot, not an F13 freeze; these audit/plan documents change the worktree again.

## Executive assessment

Grok produced meaningful implementation work. The Broker protocol, permissions, session MCP, Windows native adapter, managed Playwright worker, runtime integrity model, frontend surface, and many safety tests are real. The latest local Windows fixture and the managed-browser scripted MCP path provide useful E3 evidence.

The work is nevertheless not complete. The product currently has four named surfaces but not one end-to-end surface execution path. Desktop is the only surface used by the Broker's generic observe/act/status/preview lifecycle. Managed Browser works through separate browser tools. WebView is exercised by calling its adapter directly. Existing Tabs has a registry and pairing skeleton, but the extension has no automation transport. Packaging is Windows-only, and the required macOS/Linux native acceptance has not run.

The prior report correctly ends with `partial - not releasable`, but several individual `passed` rows are stronger than their evidence permits.

## Release-blocking findings

### P0. The Broker does not route execution by authorized surface

`SurfaceRouter` currently classifies target IDs and performs authorization only. It does not select the backend for observe, act, liveness, preview, abort, release, or idle checks.

Evidence:

- `src-tauri/computer-use-core/src/broker/surface_router.rs:27-61` only implements authorization.
- `src-tauri/computer-use-core/src/broker.rs:216-249` stores one `adapter` plus a tab registry.
- `src-tauri/computer-use-core/src/broker.rs:600-633` performs every generic capture through that one adapter.
- `src-tauri/computer-use-core/src/broker/actions.rs:18-29` checks only that adapter's capabilities, and `src-tauri/computer-use-core/src/broker/actions.rs:282-291` dispatches every generic action to it.
- `src-tauri/computer-use-core/src/broker.rs:794-799` checks every authorized target through the desktop adapter.
- `src-tauri/computer-use-core/src/broker/lifecycle.rs:30-35`, `123-125`, `140-149`, and `218-233` use the same adapter for stop, pause, resume, and preview.
- `src-tauri/src/computer_use/mod.rs:103-111` keeps WebView as a separate process-wide global rather than a Broker-owned backend.

Impact:

- A WebView can be bound and marked authorized, but model `computer_observe` and `computer_act` cannot use it.
- `targetAlive`, backend reporting, UI preview, pause/resume, stop, and cleanup are wrong or incomplete for non-desktop surfaces.
- Managed Browser has a working side path through `browser_*`, but the common Computer Use state is not authoritative.
- A typed surface enum exists, but the architecture promised by F2 does not yet exist.

Required disposition: F2 is **partial**, not passed. Build one Broker-owned surface executor before adding more product behavior.

### P0. Existing Tabs has no production observe/act transport

The MV3 extension is a pairing proof of concept, not a Computer Use extension.

Evidence:

- `tools/computer-use-extension/sw.js:1-22` only stores, reads, and clears an in-memory session key.
- `tools/computer-use-extension/manifest.json:1-18` has no `activeTab`, `scripting`, `storage`, navigation, or transport capability.
- There is no tab-share action, picker offer, command queue, heartbeat, reconnect, navigation generation, close event, observation, typed action, result, cancel, return, or disconnect protocol in the extension.
- Production code never calls `offer_shared_tab`; the only callers outside its implementation are tests in `browser/tests_gates.rs`.
- `src-tauri/computer-use-core/src/tools_browser.rs:73-85` sends every `browser_observe` to `observe_managed_tab`.
- `src-tauri/computer-use-core/src/browser/host/managed.rs:569-580` explicitly rejects a user-owned existing tab because it requires an extension transport.

Impact:

- No real shared tab can enter the App picker.
- Even a synthetic grant cannot be observed or acted through the extension.
- F6 and the final report's `Existing Tabs passed` row are false as product claims.

Required disposition: F6 is **failed/incomplete**. Existing Tabs must remain unavailable in release UI until the complete extension transport passes real Chrome and Edge flows.

### P0. WebView E3 bypasses the Broker and session MCP

The WebView adapter itself performs typed DOM observation/actions, but the App-shell evidence calls it directly.

Evidence:

- `src-tauri/src/computer_use/app_shell.rs:714-810` binds through a product command, then calls `wv.observe` and `wv.act` directly.
- `src-tauri/src/computer_use/app_shell.rs:843-931` tests fail/reclaim through the same global adapter.
- `src-tauri/src/commands/computer_use.rs:337-345` sends UI observe through `Broker::observe_preview`, which is currently the desktop adapter path.
- `src-tauri/computer-use-core/src/tools.rs:85-105` sends model `computer_observe` and `computer_act` through the Broker's generic desktop path.
- The WebView test only checks that `computer_observe` is listed in MCP; it does not call that tool for the WebView.

Impact:

- F4 proves that the adapter can drive a fixture WebView, not that a model can drive the authorized App WebView.
- The current `20/20` evidence is not an end-to-end product loop.

Required disposition: F4 is **adapter E3, product route incomplete**.

### P0. Pairing can be raced by another local process

The code correctly keeps the pairing secret out of GET and URLs, but the implemented dual-confirm path does not prove possession of the extension-side secret.

Evidence:

- `src-tauri/computer-use-core/src/ipc.rs:322-339` accepts pairing requests with no `Origin`.
- `src-tauri/computer-use-core/src/ipc.rs:470-524` accepts an optional `response` and discards it.
- `src-tauri/computer-use-core/src/ipc.rs:527-570` mints and returns a session key from public nonce/instance values after two booleans are set.
- `src-tauri/computer-use-core/src/pairing.rs:161-174` implements `complete_dual` without MAC proof.
- `src-tauri/computer-use-core/src/browser/host/mod.rs:94-107` hardcodes `pw-ext-installed`.
- `tools/computer-use-extension/manifest.json` has no stable signing `key`; an unpacked extension therefore cannot be assumed to have that ID.
- `src/components/computer-use/ComputerPanel.tsx:219-227` starts pairing and immediately confirms the App. It neither opens a pairing page nor waits for an explicit identity/fingerprint decision.

Impact:

A local process can watch the public loopback challenge, race extension confirmation, and request the session key. Origin checking alone is not authentication because a non-browser process can forge an Origin header.

Required disposition: F5 is **partial and security-blocked**. Use a user-transferred one-time secret or an OS/browser authenticated channel, verify a constant-time proof on both confirm and session issuance, and bind it to a stable extension identity.

### P1. Target authorization is split and can publish the wrong selection

Managed Browser and Existing Tabs use two independent Host commands from the UI: first provision/grant, then authorize. `SessionGrants` fences individual tickets, but it cannot make two separate commands atomic.

Evidence:

- `src/components/computer-use/ComputerPanel.tsx:138-171` calls `openManagedProfile` then `computerAuthorize`, or `shareAndGrant` then `computerAuthorize`.
- `src-tauri/src/commands/computer_use.rs:260-293` opens a managed profile under one completed ticket.
- `src-tauri/src/commands/computer_use.rs:218-257` authorizes and attaches MCP under a second ticket.
- The surface and target selectors remain enabled while `busy`; see `src/components/computer-use/ComputerPanel.tsx:323-351`.
- `commandRevision` suppresses stale React updates but does not cancel the first Host operation.

Impact:

If the user selects B while A is between its two commands, A can still authorize after B. Failures can also leave provisioned tabs/profiles that were never authorized.

Required disposition: use one typed Host authorization command per complete transaction, with one attempt ID, one ticket, compensation cleanup, and generation fencing.

### P1. Stop/revoke does not own WebView cleanup

The separate global WebView is unbound only by explicit unbind or a bind failure.

Evidence:

- `src-tauri/src/computer_use/sessions.rs:66-71` revokes the Broker run but does not unbind WebView.
- `src-tauri/src/commands/computer_use.rs:628-640` unbinds only on bind failure or explicit command.
- `src-tauri/src/computer_use/mod.rs:131-135` shuts down the managed browser only.
- `src-tauri/src/lib.rs:1870-1880` calls that managed-browser-only shutdown on App exit.

Impact:

Stop, feature-off, session change, model change, and App exit can leave the global WebView binding outside the authoritative run lifecycle.

Required disposition: surface cleanup must be owned by the Broker and be idempotent for stop, revoke, context invalidation, feature-off, and process exit.

### P1. Packaging is Windows-only and omits the extension

Evidence:

- `src-tauri/tauri.conf.json` has an empty generic resource list.
- `src-tauri/tauri.windows.conf.json` includes only the Windows runtime seed.
- `scripts/tauri-before-build.mjs:41-79` prepares only `x86_64-pc-windows-msvc`; other operating systems skip runtime preparation.
- `src-tauri/resources/computer-use/pack-targets.json` marks both macOS architectures and Linux `not_run`.
- The seed manifest contains Windows Node and `chrome-win` only.
- No Tauri bundle resource references `tools/computer-use-extension`.
- `tools/computer-use-extension/INSTALL.md:5-8` tells users to load the repository source directory.

Impact:

Installed builds on macOS/Linux cannot acquire the required runtime pack. Even Windows users cannot install Existing Tabs without a source checkout.

Required disposition: F11 stays **blocked/incomplete** until an isolated installed build works without repository files, PATH Node, or online latest resolution.

### P1. Cross-platform v1 is not implemented or accepted

The user selected Windows, macOS, Linux X11, and GNOME Wayland as v1 targets.

Evidence:

- `src-tauri/src/computer_use/macos_adapter.rs:378-410` lacks key, scroll, and drag.
- `src-tauri/src/computer_use/linux_adapter.rs:398-437` implements only click; text, key, scroll, and drag return not implemented even though some capability flags suggest scroll/drag support.
- `src-tauri/src/computer_use/linux_adapter.rs:187-228` keeps `native_wayland = false`.
- The final report marks macOS and Linux `not_run` and Wayland `blocked_external`.

Impact:

F9 cannot be globally passed. Windows E3 is valid evidence for Windows only.

Required disposition: keep per-OS rows independent. Cross-compilation, browser success, or Windows fixtures must never promote another OS row.

### P1. The final evidence report is internally inconsistent

Evidence:

- The final report marks F7 passed while Edge is `blocked_external`.
- It marks F9 passed while macOS/Linux are `not_run` and GNOME Wayland is still unavailable.
- It marks Existing Tabs passed even though the extension has no automation transport.
- It marks F12 passed while its own `audit_prod` sub-row and manifest record are failed.
- The current manifest has 43 records: F0=6, F1=17, F2=2, F3=4, F5=1, F8=1, F9=4, F10=1, and F12=7. It has no records keyed to F4, F6, F7, F11, F13, or F14.
- The manifest contains 13 failed and 30 passed historical attempts. Its referenced files currently have zero missing, size-mismatched, or hash-mismatched entries, but the report still does not derive each aggregate status from required scenario records.
- `state.json` still says phase `F0` while later rows are manually marked passed.
- `state.json` records the current audit-time fingerprint, but F13 is still `not_run`; no release freeze exists.
- F13 and F14 correctly remain `not_run`; no 12-hour active soak exists.

Required disposition: evidence state must be generated from scenario-keyed manifest records. Aggregate rows cannot be manually promoted beyond their worst required sub-row.

### P2. Quality debt is material

Current production/test-support files over 1000 lines:

| lines | file |
| ---: | --- |
| 2202 | `src-tauri/computer-use-core/src/runtime.rs` |
| 1576 | `src-tauri/src/computer_use/app_shell.rs` |
| 1329 | `src-tauri/computer-use-core/src/runtime_prepare.rs` |
| 1194 | `src-tauri/src/computer_use/webview.rs` |
| 1151 | `src-tauri/computer-use-core/src/broker.rs` |
| 1150 | `src-tauri/computer-use-core/src/broker/gates.rs` |
| 1073 | `src-tauri/src/computer_use/windows_adapter.rs` |

Additional evidence:

- `src-tauri/src/computer_use/mod.rs:6` suppresses unused imports for the whole module.
- Current `cargo check -p grok-app --lib` succeeds with 36 warnings.
- `src/lib/api/computerUse.pairing.test.ts` is a source string-presence test, not a behavioral contract.
- The worktree has no `.gitattributes`, local `core.autocrlf=true`, and `git diff --check` prints many LF-to-CRLF warnings. It has no whitespace error, but line-ending policy is not deterministic.

Required disposition: F12 is **gates mostly green, quality cleanup incomplete**.

## What is currently validated

The following commands were rerun against the latest inspected production tree:

| verification | result | interpretation |
| --- | --- | --- |
| `cargo test -p grok-computer-use-core --all-features --locked --offline` | core 294/294, driver 12/12 | Shared Rust contracts pass; this is not product E3 for every surface. |
| Computer Use/frontend integration Vitest set | 131/131 across 12 files | Current components, protocol, settings, side-workbench, and slash tests pass. |
| `pnpm typecheck` | pass | TypeScript compiles. |
| `pnpm lint` | pass | Frontend lint passes. |
| `cargo fmt --all -- --check` | pass | Rust formatting passes. |
| core Clippy with `-D warnings` | pass | Core crate is warning-clean. |
| `cargo check -p grok-app --lib --locked --offline` | pass with 36 warnings | App compiles, but is not warning-clean. |
| extension `node --check` | pass | Syntax only; no behavior is established. |
| current `cu_probe windows-native` | pass | Windows UIA, geometry, p8, identity, clipboard, focus, takeover, cancel, Win32/WPF/WebView2 fixture paths pass. Electron is `not_run`. |
| `pnpm check:computer-use` | pass for `x86_64-windows` seed | Confirms only the current Windows seed; macOS/Linux packs and installed lifecycle remain unproved. |

Not run or not established in this audit:

- current-fingerprint App-shell repeated end-to-end test;
- real Existing Tabs Chrome/Edge share, observe, act, navigation, reconnect, stop, and return;
- model-driven WebView observe/act through MCP and Broker;
- installed NSIS clean install, upgrade, rollback, uninstall;
- macOS arm64/x64, Linux X11, or GNOME Wayland native E5;
- real-model E4, which still requires explicit user approval;
- F13 freeze and F14 12-hour active soak.

## Corrected status matrix

| area | current status | highest defensible evidence |
| --- | --- | --- |
| Desktop on Windows | locally validated | E3 fixture/probe |
| Managed Browser on Windows | locally validated with limitations | E3 scripted App/MCP path; generic status/preview is not authoritative |
| App WebView adapter | adapter works; product route incomplete | E3 direct adapter, not product MCP |
| Existing Tabs | incomplete | pairing/registry unit and CFT pairing fixture only |
| Broker surface routing | incomplete | E1/E2 structure and tests |
| Pairing security | incomplete | negative unit tests do not cover the local-process race |
| Windows packaging | partial | seed bundle exists; isolated installer lifecycle not run |
| macOS packaging/native | not run/incomplete | source and contract only |
| Linux X11 packaging/native | not run/incomplete | source partial; no real fixture |
| GNOME Wayland native | blocked/incomplete | `native_wayland=false` |
| Real model | not run | requires approval |
| 12-hour soak | not run | no scenario-keyed active wall time |

## Release predicate

Do not call Computer Use complete until all of the following are true on one exact candidate fingerprint:

1. Every surface uses one Broker-owned execution/lifecycle route.
2. Existing Tabs has a real packaged extension transport with authenticated pairing, explicit tab share, typed observe/act, navigation/close/reconnect handling, cancellation, and return-on-stop.
3. WebView is driven through session MCP and Broker, not direct adapter calls.
4. Managed/Desktop/WebView/Existing Tabs authorization is atomic and race-tested.
5. Installed packages do not depend on repository paths, PATH Node, test fixtures, or online latest artifacts.
6. Windows, macOS arm64/x64, Linux X11, and GNOME Wayland have honest per-platform evidence.
7. Required installer lifecycle, privacy, fault, and scenario tests pass with zero unauthorized writes, wrong-target actions, post-stop dispatches, or credential leakage.
8. F13 freezes the exact fingerprint and F14 records at least 12 hours of scenario-keyed active soak without source/test/runtime drift.

The next execution tranche is defined in `docs/plans/2026-09-13-computer-use-grok-next-48h-execution.md`.

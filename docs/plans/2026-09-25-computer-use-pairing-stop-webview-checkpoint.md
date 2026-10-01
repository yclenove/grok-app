# Computer Use — pairing evidence, Stop priority and WebView result contract

Status: worktree implementation; full Computer Use release goal remains active and incomplete.
Local date: 2026-09-25 (+08:00), corresponding to 2026-09-24 UTC.
Branch: `feat/computer-use-implementation`; base HEAD: `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`.
No commit, push or PR performed. Earlier dirty/untracked work is preserved.

## Pairing diagnostics no longer obscure the first failure

The old exception handler awaited a worker evaluation before recording its primary error.
It also awaited worker and popup diagnostics without a deadline. A stalled diagnostic could
turn a specific assertion failure into the parent's generic 180-second timeout.

`tools/computer-use-probe/pairing-diagnostics.mjs` now records the primary stage first, then
collects all diagnostic reads concurrently within one one-second window. Results distinguish
unavailable, failed and timed-out reads. A timed-out CDP read is neither proof of worker death
nor proof of physical cancellation; native process-tree cleanup remains with the owner.

Only a unique exact production worker may be inspected. Active-tab visibility injection is
restricted to the exact synthetic fixture URL, not a matching title or the first worker.
Route/status/reason and popup-state allowlists discard payloads, headers, entered credentials
and raw exception messages. Late promise rejections are consumed. The response ring is bounded.

Nine Node regressions passed, including never-resolving worker/popup reads and redaction.
Log: `tools/computer-use-probe/.run/mcp-transport-20260925T020240/pairing-bounded-diagnostics-tests.log`.

### Native pairing acceptance

The fresh evidence directory initially lacked the probe home, its owner marker, and then its
private runtime. The three preflight failures are retained in `pairing.log`,
`pairing-native-1.log` and `pairing-native-2.log`. None reached browser acceptance. No guard
was bypassed, no system Node fallback introduced, and no daily App settings were changed.

The actual runs used the already-verified isolated App home under
`tools/computer-use-probe/.run/mcp-transport-20260925T020240/app-home`, with a separately owned
disposable browser profile on each run. **Three consecutive full runs passed all 36 checks**,
each with terminal exit 0. Stop acknowledgement/observation cancellation measured 6, 6 and 5 ms;
the borrowed tab remained open. Click, duplicate-result retrieval, session isolation, heartbeat,
navigation invalidation and the observation-race matrix all used real Host/Broker/IPC/MV3 paths.

Artifacts under `tools/computer-use-probe/.run/pairing-bounded-20260925T060516-e98510c7d71b4e1783ae8034d474fd5c/`:

- `pairing-native-owned-home.log`
- `pairing-repeat-2.log`
- `pairing-repeat-3.log`

Probe SHA-256 for these runs:
`89c12010466d170e89ec08f14e40ceed72570ad47aa23824886eb1fa07b9152d`.
This binary precedes the WebView changes below. Three passes do not establish the root cause
of every historical intermittent failure; retain the earlier failure evidence and run the frozen soak.
This remains private fixture consent, not installed-App, toolbar-granted screenshot, signed-extension
or real-model acceptance.

## UI Stop is not held by ancillary cancellation

The redesigned UI still used `Promise.all` for Host Stop and attempt-specific authorization
cancellation. A successful Stop acknowledgement could leave the UI's Stop latch held forever
if the ancillary cancellation reply never arrived. A rejected Stop already escaped that wait.

The Host's exact-session Stop fences pending authorizations before acknowledging. The UI now
dispatches the attempt-specific cancellation but ties its latch only to Stop itself. It does not
equate the acknowledgement with completed native cleanup. `stop_requested` remains visible,
stale state blocks new control, and late authorization replies cannot publish a new run.

The new regression first reproduced the successful-acknowledgement hang. Its first red run also
contained one test-ordering error (counting reads before a second Stop's own refresh); that
assertion was corrected by flushing the second request before checking late replies.
Current result: **30/30** across the controller's three new tests and the existing 27 panel tests.
Logs: `ui-stop-ancillary-red.log` and `ui-stop-ancillary-green.log` in the MCP evidence directory.

## WebView contract corrections — not completion of W2

- Removed the hidden `parameters.href` navigation path. A click cannot bypass its element
  identity by supplying another URL. Clicking a real observed link still navigates normally.
- Validate supported parameter names and types before any native script. Missing/ambiguous
  fill values no longer silently clear fields; explicit empty-string fills remain valid.
- Bounded JSON reply parsing to 512 KiB and object/one native string envelope. Malformed,
  oversized and rejected replies never include raw page content in errors or model traces.
- Script completion is `applied`, not independently `verified`. Result details no longer
  echo input values or arbitrary page-provided fields.
- Removed fixture `name`/`mark`/`count` value extraction from the product snapshot. The owned
  native fixture checks its own postconditions separately in probe-only code.

The compiled App harness passed **27/27** WebView tests after its required post-link Windows
manifest was embedded. Logs: `webview-result-contract-build.log` and
`webview-result-contract-tests.log`.

The rebuilt `cu_probe webview` also passed with terminal exit 0: actual WebView2 bind,
click exactly once, Chinese fill, link navigation/document retirement, unsupported-origin
rejection, and 20 product-adapter bind rounds. The fixture's separate page-state read checked
the click/fill effect; the product action still reports only `applied`.
Log: `webview-result-native.log`; build: `webview-result-native-build.log`.
Probe SHA-256: `3be2d5febb60a8f612ecb04862937383aa2cbe5889634a29ea8ef8b1c41dcce4`.
This is the owned native WebView2 probe, not an installed App or another operating system.

Still open in W2: generic DOM references, native isolated worlds, truthful in-flight ownership,
and physical cancellation. These contract repairs do not claim to deliver those mechanisms.

## Combined regression checks

Under `tools/computer-use-probe/.run/mcp-transport-20260925T020240/`:

| Scope | Result | Artifact |
| --- | --- | --- |
| Extension + MCP + existing-MCP/pairing diagnostic Node tests | 183/183, no skips, exit 0 | `pairing-stop-webview-node.log` |
| CU UI/domain/API and all-locale catalog tests | 120/120 across 15 files, exit 0 | `pairing-stop-webview-ui.log` |
| Full frontend TypeScript build check | exit 0 | `pairing-stop-webview-typecheck.log` |
| Changed UI files, ESLint with zero warnings | exit 0 | `pairing-stop-webview-lint.log` |
| Changed WebView files, rustfmt check | exit 0 | terminal check |
| Whole frontend | 7,138/7,138 across 600 files, exit 0 | `pairing-stop-webview-full-ui.log` |
| Whole frontend lint | exit 0, zero warnings | `webview-ownership-full-lint.log` |
| Production UI build | exit 0, 25.34s Vite phase | `webview-ownership-ui-build.log` |

`git diff --check` reported no whitespace errors. Git still emitted existing worktree
LF-to-CRLF conversion warnings for a number of tracked files; this batch did not rewrite
repository-wide line endings or Git settings. No matching owned pairing/fixture processes
remained after native runs. The separate whole-frontend run completed in 154.13 seconds.
This verifies frontend regressions, not all native platforms or the full release matrix.
The UI build retained Vite's large-chunk warning; this was not a build failure and was not
hidden by raising its threshold. Native execution-ownership changes made afterward have
their own follow-up validation and do not invalidate or expand this frontend-only scope.

## Full-goal outstanding gates

- Complete WebView W2 and remaining macOS/X11 actions; native GNOME Wayland implementation.
- Mac signed archive/resource delivery and native all-platform install/update/rollback.
- Installed-App UI, OS scaling, real permission/Stop flow and real Grok acceptance.
- Signed Chrome/Edge/platform extension matrix and frozen-version 12-hour active soak.
- Historical pairing intermittence remains an acceptance risk until stronger evidence closes it.

The UI redesign's 33-case browser evidence still uses mocked Host IPC. Preserve that distinction
from native adapter, installed-App and real-model evidence. Do not mark the overall goal complete.

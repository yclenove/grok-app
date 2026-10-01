# Computer Use UX redesign — 2026-09-25

## Scope and invariants

Redesign the Computer Use workspace, chat task card, extension pairing and settings as one product experience. Keep the existing Grok App tokens, controls and native overlay rules; do not introduce a parallel theme or grow App/AppWorkbench. This is an additional workstream, not a declaration that the backend or final release is complete.

Selecting a candidate is not consent. Authorization remains an explicit, exact-target, per-session Host operation. Preserve attempt IDs, selector revisions, cancellation, late-response fencing and Stop priority. Never fall back to desktop, infer pairing success, or claim an action is executing merely because control is authorized.

## U1 — Workspace and authorization

- Extract the controller from the monolithic panel; keep side effects and authorization identity tests.
- Fixed compact header, one scrollable body, always-visible Stop footer.
- Source-specific target selector, candidate summary and explicit authorization CTA; one WebView selector only.
- Separate candidate and authorized target identity. Preview belongs only to the exact authorized session/run/target.
- Shared pure status mapping. Distinguish status unknown, paused, reauthorization, stopping, stopped and tool cleanup; do not equate `recovery=user` with login.
- Bounded, uncropped preview; hidden, waiting and stale states are distinct. Put technical traces and IDs inside diagnostics.

## U2 — Supporting surfaces

- Compact chat card: actual target, honest state, open workspace, bounded actions; preserve last known same-session identity and Stop when polling fails.
- Pairing: connect/confirm/share steps, expiration feedback, no fabricated connected indicator. Sharing still requires separate target authorization.
- Settings: enablement, runtime health and collapsed maintenance. Show pending/failure/export results and operation-specific destructive confirmations.
- All copy in all 15 catalogs; keyboard, focus, reduced motion and light/dark compatibility.

## U3 — Acceptance and evidence

- Unit/component: selection without capture/authorization; exact consent; cancellation on Stop/session switch; late replies; preview identity; status loss; repeated commands; cleanup and stop priority.
- Typecheck, targeted lint, locale parity and existing domain tests.
- Real browser layout fixtures at widths 320/400/640/960 and heights 480/800; dark/light, long translations, portrait/landscape previews and keyboard operation. Fixtures are not installed-App or native permission evidence.
- Native App checks remain required for WebView overlays, OS scaling and actual permission prompts.
- Record every failed gate and outstanding release requirement in the resumed checkpoint. No commit/push/PR without user direction.

## Backend evidence retained before this batch

Native WebView probe passed, including 20 bind rounds. Two full extension-pairing rounds passed (36 checks each), but another failed at explicit share; the latest diagnostic build failed the observation deadline under observed high CPU. Pairing is still intermittent. Extension/MCP/audit Node suites passed 190 tests. Windows seed hygiene audit passed; that audit does not prove signatures or full runtime integrity. Details and logs belong in the resumed checkpoint.

## Implementation checkpoint

U1 and U2 are implemented in the worktree. U3 automated acceptance passed; installed-App acceptance remains open. Nothing in this checkpoint closes the full Computer Use release goal.

- `ComputerPanel.tsx` is the view; `useComputerController.ts` owns exact-session commands, authorization attempts, polling freshness and cancellation. `ComputerDiagnostics.tsx` keeps technical detail out of the main workflow.
- Running sessions lead with the preview; changing the candidate is a separate action. Candidate selection performs neither authorization nor capture. Keyboard consent moves focus to the preview only if the user has not moved focus elsewhere.
- `taskCard.ts` provides shared truthful states. A stale status is unknown, not a green operating state. Hidden or paused previews retain only an exact-identity old frame, clearly labelled as not live.
- Opening a task card preserves an existing Computer tab's surface. It does not grant control or authorize a new target.
- Pairing confirmation exposes the challenge and instructions, not an invented connected status. The expiry timer uses locale-aware number formatting.
- Runtime maintenance uses operation-specific in-app confirmations; failures and actual export paths remain visible. No cleanup runs before confirmation.
- New copy is present in all 15 catalogs. The final sweep also translated previously English control, pairing and consent labels. A catalog regression covers these high-traffic keys; this is not a claim of human review of every legacy translation.

### Stop remains accessible during status loss

Two regressions first failed in `ui-stop-readback-red.log`: an initial status failure removed Stop for a supplied run, and a hanging read-after-Stop kept the command latch held forever. Both passed after repair in `ui-stop-readback-green.log`.

The command latch now belongs to the Stop request, not the subsequent read-only status refresh. Unknown state disables new actions and leaves explicit Stop available. If the first read fails before the UI knows a run ID, Stop uses the existing Host's session-scoped stop contract; an explicitly supplied run ID is preserved. Neither path authorizes, captures, automatically retries actions, or touches another chat. A Stop acknowledgement is not presented as proof that asynchronous native/tool cleanup has finished.

### Visual fixtures and evidence boundary

`ui-fixture.tsx` imports the same token, skin, Tailwind and app stylesheet order as the application. `ui-acceptance.mjs` starts an owned loopback Vite server and isolated bundled Chromium; external requests are blocked. Screenshot directories use a unique timestamp, preserving previous rounds. The fixture renders real product components against mocked Host IPC and never captures a private application window.

The latest completed unit run is **117/117 across 14 files** (`ui-redesign-r5-unit.log`); TypeScript build and scoped ESLint also passed (`ui-redesign-r5-typecheck.log`, `ui-redesign-r5-lint.log`). The final browser process exited 0 with **33/33 cases** (`ui-redesign-r5-browser.log`). Logs are under `tools/computer-use-probe/.run/mcp-transport-20260925T020240/`.

Screenshots and detailed layout metrics are preserved in `tools/computer-use-probe/.run/ui-redesign-2026-09-24T21-07-00-816Z/`. This is September 25 in the local +08:00 timezone. Reviewed the 320px running panel, light settings and task card, initial-disconnection emergency Stop, and 200% Tamil text screenshots. Footer controls remain inside the viewport; only the body scrolls. The earlier 30/31-case rounds and both red regressions are retained, not overwritten or reclassified.

Still required before release: installed Windows App overlays and native permission/Stop behavior, OS display scaling, corresponding macOS/Linux UI acceptance, and the backend/runtime/real-model/soak gates in the resumed checkpoint. No commit, push or PR was performed.

### R6 — prevent UI probes from polluting the runtime

The following runtime regression exposed a harness defect: R5 launched the seed executable
directly and left a 372-byte Chromium `debug.log` beside it. Its layout results remain valid,
but isolated profiles alone did not preserve the immutable package. All 84 pinned Chromium
files were compared with the verified vendor archive and still matched; the extra diagnostic
was moved into the evidence directory. No lock/hash was relaxed.

R6 launches an independent writable browser copy and checks the whole seed fingerprint
before/after cleanup. It passed **33/33**, exit 0, and both fingerprints were
`d1f9a056d0382982d4ac43de0638e38982ddbd257ff254d08741a384e4f1b6e1`.
Log: `ui-redesign-r6-immutable-seed.log`. Screenshots and the integrity receipt:
`tools/computer-use-probe/.run/ui-redesign-2026-09-24T21-45-28-170Z/`.
See [runtime follow-up](2026-09-25-computer-use-runtime-import-checkpoint.md) for the retained
first failures and precise evidence boundary. This does not upgrade mocked Host UI evidence
to native App acceptance.

### R7 — Stop acknowledgement is independent of auxiliary cancellation

The controller no longer waits for an attempt-specific authorization cancellation reply before
releasing the Stop command latch. Host Stop already fences the exact session's pending grants.
A lost auxiliary reply must not leave the button permanently at “Stopping”; the subsequent
Host state still distinguishes `stop_requested`, `stopped` and unknown. New control stays disabled
while state is unknown, and late authorization results remain fenced.

Controller/panel regression: 30/30. Complete CU UI/domain/API/i18n batch: 120/120 across 15 files.
TypeScript and changed-file ESLint passed. This changes interaction logic, not the R6 layout or
its evidence boundary. See [current checkpoint](2026-09-25-computer-use-pairing-stop-webview-checkpoint.md).

### R8 — authoritative Stop readback and cleanup-first layout

September 25, 2026 (+08:00). The existing session/run component keys already isolate
replacement runs. A new regression confirms that a pending old Stop cannot disable
the new card or surface its late error; no unsupported run-switch bug is claimed.

Two real UI state defects were reproduced before repair:

- The card could display Host `stopped` while its button remained at “Stopping…”
  indefinitely; a late transport error then produced a false failure alert.
- The workspace kept its command latch set after a fresh exact-run Host stop,
  preventing an explicit MCP cleanup retry. A late reply could also invalidate
  newer readback or interfere with a subsequent command.

Fresh **exact-run** native `stopped` readback now retires the corresponding command
reply. Card state and physical-stop confirmation are separate: pending tool cleanup
still says “Removing Computer Use tools…”, never “Stopped”. Unknown state,
`stop_requested`, another run, a read begun before the request, and empty idle
status do not prove that request completed. Stop acknowledgements alone are not
native-completion evidence. There is no automatic retry or action replay; existing
Host failure details remain visible. Late retired replies cannot release a newer
explicit retry or overwrite cleanup state.

During stopping/tool cleanup, the workspace hides the unusable target setup form
and change-target action. It retains the exact target, last preview with stale
labelling, diagnostics and fixed controls. The chooser returns after cleanup;
there is still no automatic authorization. Tool cleanup alone is not described as
“local control is stopped” unless the Host actually reports native `stopped`.

Evidence directory: `tools/computer-use-probe/.run/linux-native-20260925/`
(the name does not identify the platform of these Windows/frontend checks).

- `ui-task-stop-readback-red.log`: 3 failed / 3 passed; repaired card batch
  `ui-task-stop-readback-green.log`: 15/15.
- `ui-controller-stop-readback-red.log`: 3 failed / 4 passed; repaired controller
  and panel batch `ui-controller-stop-readback-green.log`: 37/37.
- `ui-settling-layout-red.log`: 3 failed / 25 passed; the first repair run retained
  two test-readiness failures because querying a status role returned the initial
  “Checking components” content. Assertions now wait for the required state,
  without weakening the expected content or introducing a sleep.
  `ui-settling-layout-green-r2.log`: 28/28.
- Final CU UI/domain/API/i18n run `ui-readback-final-domain.log`: **136/136 across
  17 files**. This includes exact-run, stale-read, late-error and explicit-retry races.
- Final TypeScript and scoped ESLint gates passed (`ui-readback-final-typecheck.log`
  and `ui-readback-final-lint.log`). `ui-readback-final-build.log` completed with
  exit 0; Vite reports 36.60s. The existing large-chunk warning remains visible.
- Final Chromium acceptance `ui-readback-final-browser.log`: **38/38**, terminal
  exit 0. This includes native-still-running/tool-cleanup, lost Stop replies,
  late rejection and the cleanup-first layout at narrow widths.
- Latest screenshots and layout metrics:
  `tools/computer-use-probe/.run/ui-redesign-2026-09-25T04-27-16-292Z/`.
  Visually reviewed the 320px light cleanup panel, 320px dark running workspace
  and 200% Tamil dark layout. The cleanup panel no longer shows an unusable form;
  the last frame is clearly stale and footer actions remain visible. This is
  a real Chromium rendering of product components with mocked Host IPC.
  The seed receipt confirms the same before/after fingerprint:
  `d1f9a056d0382982d4ac43de0638e38982ddbd257ff254d08741a384e4f1b6e1`.

Native installed-App overlays, OS scaling/permissions, macOS/Linux UI, real model
and all original backend/release/soak gates remain open. This batch does not close
the full objective or authorize publication.

### R9 — bounded preview freshness and exact identity

September 25, 2026 (+08:00). Review of the running workspace found that a lost
capture reply could leave the last image looking current indefinitely. An
initially slow reply was also marked current on arrival, even though receipt
time does not establish when its pixels were captured.

- Preview freshness now has a **3-second presentation budget**, measured using
  the request's monotonic start. A hung refresh expires the preceding image; a
  late image remains explicitly historical. A timely subsequent frame restores
  the normal display. This does not change native timeouts, authorization,
  physical Stop, action retries or model observations.
- Visibility changes mark the frame stale before waiting for the Host's
  acknowledgement. Cleanup clears the freshness timer; old identities and late
  responses cannot alter a replacement's freshness. No timeout launches an
  overlapping replacement capture.
- The preview key is an encoded `[sessionId, runId, targetId]` tuple, avoiding
  delimiter collisions. An absent target cannot start capture/enable preview.
  This is a UI isolation fix, not a claim that a production Host authorization
  bypass was reproduced.

Evidence in `tools/computer-use-probe/.run/linux-native-20260925/`:

- `ui-preview-freshness-red.log`: **5 failures / 6 passes** before the fix.
- `ui-preview-freshness-green.log`: **11/11** after the fix; the final domain run
  includes two additional wall-clock/replacement regressions (**13 preview tests**).
- `ui-preview-final-domain.log`: **144/144**, 17 files, 18.19s. Typecheck and scoped
  ESLint passed (`ui-preview-final-typecheck.log`, `ui-preview-final-lint.log`).
- `ui-preview-final-browser.log`: **39/39**, terminal exit 0. The new actual
  Chromium case holds a capture reply, verifies stale copy while Stop remains
  accessible, accepts the late frame only as historical, and verifies recovery
  on the next timely capture. The 38 prior interaction/layout cases still pass.
- `ui-preview-final-build.log`: production UI build exit 0, Vite 30.07s. Existing
  large-chunk warnings remain visible; this is not an all-repository lint result.
- Latest artifacts: `tools/computer-use-probe/.run/ui-redesign-2026-09-25T04-42-21-911Z/`.
  `preview-expired-320-light.png` was visually reviewed: the old image is labelled,
  and Pause/Take Over/Stop remain inside the viewport. Its seed receipt matches
  the unchanged R8 before/after fingerprint.

Evidence remains real browser rendering with mocked Host IPC, not installed
App/native acceptance. The full backend, all-platform and release/soak scope
remains active and unchanged; no publication was performed.

### R10 — current native UI review and concise runtime recovery

September 25, 2026 (+08:00). Built the current frontend into an isolated native
Windows debug App (`tauri build --debug --no-bundle --ci`), not a Vite preview or
the September 22 installed test binary. Its separate identifier is
`com.grokapp.desktop.cu-review-20260925`; App/agent/WebView homes are beneath
`tools/computer-use-probe/.run/ui-native-20260925/app-home`. Computer Use remains
off. No credentials, target grants, real-model calls, permission changes, runtime
repair, trace deletion or modifications to other Grok processes were performed.

The real Host returned six `missing_file` component issues in this fresh home.
The active pack contains only embedded `mcp-protocol` and `mcp-server`; the six
browser/runtime components have not been materialized there. This is not evidence
of a network failure or a corrupted signed installation. The old settings UI
repeated the same recovery instruction six times, obscuring the useful controls.

`ComputerRuntimeHealth` now groups recovery copy by issue code. Actual component
names and raw issue codes remain in initially collapsed diagnostics. Distinct or
unknown issues are preserved; no failing state is disguised as ready. Diagnostic
identifiers wrap at narrow widths. Existing translated strings are reused, so
all 15 catalogs keep their current keys. Repair, rollback, enable and consent
behavior is unchanged.

Evidence under `tools/computer-use-probe/.run/ui-native-20260925/`:

- `runtime-summary-red.log`: 2 regressions failed / 4 passed before the fix;
  `runtime-summary-green.log`: 6/6 after the fix.
- `runtime-summary-domain.log`: **146/146**, 17 files, 14.20s; TypeScript and
  scoped ESLint also pass. These are frontend/domain/API/i18n tests.
- `runtime-summary-browser.log`: **42/42**, including three new missing-runtime
  cases at 320px in both themes and 200% Tamil text. Six diagnostic records,
  keyboard expansion, default-off and zero automatic repair/grant calls are
  asserted. Artifacts are in
  `tools/computer-use-probe/.run/ui-redesign-2026-09-25T12-12-17-300Z/`.
  Seed before/after fingerprint remains
  `bf695fefb65bbcf0a7a9dd2f7f653832f47a3c1d1f561713904342757e562a0b`.
- `runtime-summary-native-build.log`: terminal exit 0; UI build 29.24s and Cargo
  46.15s. Existing chunk-size and linker-stdout warnings are retained. The
  reviewed executable SHA256 was
  `1BF012ED820D39E3AFD9A4975CE52EA8903D74DB61093D19323884088F9E2CBA`.
- Actual WebView2 review at 1202×802 verified one runtime warning, expansion into
  all six real Host component records, collapsed maintenance and an off switch.
  Returning to chat and opening the Computer workbench displayed the localized
  no-session state without an authorization action. Maintenance confirmation was
  inspected on the preceding native build and dismissed with Escape; no deletion
  occurred. This does not establish native running-session controls or consent.
- The Windows helper returned `unknown screenshotId`, stale-window and missing
  coordinate-geometry errors during some attempts. State was refreshed rather
  than blindly replaying actions. A separator drag did not visibly resize the
  panel; native narrow-width acceptance is therefore not claimed. The real
  browser fixtures, not this attempt, provide narrow-layout evidence.

The native review also exposed a globe icon shared by Browser and Computer in
both the picker and tab strip. Both now use the existing `IconDeviceDesktop`,
matching the workspace and task card. The inactive icon-only Computer tab is
therefore distinguishable. New rendered-component tests preserve Browser's globe
and assert explicit selection/activation without auto-selection or tab closure.
The first two icon-test attempts used an incorrect dependency CSS class and are
retained as harness failures, not red product evidence. After checking the actual
installed icon markup, `workbench-icon-green-r2.log` passes **33/33** (two entry
tests plus 31 existing workbench tests), with TypeScript and scoped ESLint passing.
The icon change follows the native binary above. Its separate rebuild
`workbench-icon-native-build.log` exited 0 (UI 28.60s, Cargo 45.96s), producing
SHA256 `8B25E6D8D1BD886BEF9E83ED91BE87A835A66605143553EB38B44FE8413F6947`.
That binary was launched in the same isolated home; actual WebView2 screenshots
confirmed the distinct desktop icon in both picker and active tab, matching the
no-session panel. Both owned review processes exited after normal Alt+F4; the
process table confirmed their absence. Other Grok windows were untouched.

U3 remains partial: current native debug rendering is now evidenced, but installed
signed App, native authorization/control/overlays, OS scaling, other platforms,
real Grok E4 and the frozen 12-hour active soak remain open. No commit/push/PR.

### R11 — exact session identity and independent surface availability

September 25, 2026 (+08:00). The panel remount key now encodes the exact
`[sessionId, runId, surface]` tuple with `JSON.stringify`. Colon concatenation
collided for `a:b / run` and `a / b:run`, keeping an earlier controller and its
pending authorization alive. The regressions verify cancellation of the original
attempt, refusal of its late reply, and status/Stop calls for the new exact tuple.
These are rendered-component/mock IPC regressions, not evidence of a native
authorization bypass.

The initial Host status reports Desktop capability when no run is authorized.
That status no longer hides the entire panel or blocks the independent browser
surfaces. The surface chooser remains available; the Desktop-specific warning
and Settings link appear only when Desktop is selected. Browser selection still
requires explicit target selection and consent. No automatic capture, grant,
fallback, or permission changes were introduced. Existing 15-locale copy and
shared Select controls are reused.

Evidence under `tools/computer-use-probe/.run/ui-native-20260925/`:

- `panel-identity-red-r2.log`: two genuine identity failures before the fix.
  The preceding `panel-identity-red.log` also contained an incorrect test-only
  `previewOnly` expectation; that first attempt is not entirely product evidence.
- `surface-availability-red.log`: four regressions failed before the surface fix.
- `panel-identity-domain-green.log` initially retained one test readiness race:
  initial emergency Stop was present before Pause/status loaded. The test now
  waits for the actual loaded-state Pause control before asserting that state.
- `panel-identity-surfaces-green.log`: **153/153 across 18 files** (18.08s).
  TypeScript and scoped ESLint pass in the corresponding typecheck/lint logs.
- `panel-identity-surfaces-browser.log`: first navigation timed out before any
  case passed. Vite observed an unrelated source edit during the run; this is
  preserved, not asserted to be the only cause. Acceptance now disables the
  Vite file watcher and saves bounded failure screenshot/metadata.
- `panel-identity-surfaces-browser-r2.log`: **50/50**, terminal exit 0, with source
  frozen during the run. Eight new 320x480 cases cover all four surfaces in both
  themes with no Desktop backend. They check chooser visibility, menu keyboard
  dismissal, explicit consent, no automatic capture/grant, and viewport bounds.
- Artifacts: `tools/computer-use-probe/.run/ui-redesign-2026-09-25T14-34-26-750Z/`.
  The light Desktop warning, light Managed Browser setup and dark running preview
  screenshots were visually inspected. The source chooser, warning/Settings,
  and persistent control footer fit; browser setup does not show the Desktop
  warning. Seed before/after is unchanged:
  `bf695fefb65bbcf0a7a9dd2f7f653832f47a3c1d1f561713904342757e562a0b`.

This is real Chromium layout and interaction with mocked Host IPC. R10's native
debug binary predates R11; installed/native authorization, running controls,
overlays, OS scaling and all-platform UI acceptance are still open. The full
Computer Use goal remains active, with no commit, push, PR or release.

### R11 native readback — September 26, 2026 (+08:00)

The current R11 source was rebuilt with the same isolated review identifier and
home. `panel-identity-native-build.log` first failed during runtime preparation
with Windows `Access denied (os error 5)`, before the frontend build. This failure
is preserved. Preparation now annotates its major phases and preserves both the
primary failure and an owned-staging cleanup failure; four focused Rust tests
pass in `prepare-phase-error-tests.log` (under the linux-native evidence folder,
but run on Windows). This improves diagnosis, not proof that access denial is fixed.

`panel-identity-prepare-diagnostic.log` subsequently passed the real pinned module
import. `panel-identity-native-build-r2.log` completed with exit 0: UI 28.26s,
Cargo 1m28s, existing linker-stdout warning retained. Executable SHA256:
`1921F99979778A26A395F3D0AA4E5A4CEAE991C7B61B9642DEE7952749FC296D`.

The supported Computer Use skill inspected this actual WebView2 App at 1202x802:
the Computer picker/tab uses its distinct desktop icon; the no-session panel
renders correctly; settings retain one concise missing-runtime warning, closed
diagnostics/maintenance and the off switch. No target was authorized, no feature
or privacy setting changed, no repair/install/update action executed, and no
real-model request submitted. A separator drag again produced no visible width
change, so native narrow-width acceptance remains unproven. The owned review
process (62052) exited normally after Alt+F4; process lookup confirmed absence.

R11's exact-run and independent-surface behavior still has component/mock-Host
evidence, not an authorized native session. Signed installed UI, native running
controls/overlays, OS scaling and the full other-platform matrix remain open.

### R12 — accessible split resizing and stale-fit fencing

Workstation-local September 26, 2026 (+08:00). This extends the redesigned
workspace's interaction and acceptance coverage; it does not close U3 or the
full Computer Use release goal.

#### Product changes

- The shared side-pane separator now supports keyboard focus, current/minimum/
  maximum accessible values, Left/Right 10px steps, Shift+Left/Right 50px steps,
  and Home/End. Tab and application modifiers are not intercepted. Its label
  describes the side pane rather than only Files in all 15 existing catalogs.
- Focus has a visible token-based line/band and a forced-colors outline. Closed,
  phone, overlay and expanded panes do not expose an invisible resize tab stop.
  Existing chrome-safe width and chat-space bounds are preserved.
- Pointer cancellation, window blur and unmount release resize listeners and
  temporary input styling. Closing during a drag cannot be undone by its late
  pointer-up. Live accessible values follow the actual painted width.
- A subsequent regression found that a delayed native window fit could replace
  a user's newer width (574px became 458px). Both pointer and keyboard cases
  failed before repair. Explicit resizing now retires the pending pane-fit
  generations; a late fit cannot overwrite the newer geometry. Reopening the
  sidebar has a separate regression for the same race.
- State remains in `useWorkbenchLayout`; `AsideResizeHandle` and `asideResize`
  hold the reusable view and key mapping. AppWorkbench only rewires the existing
  control prop. The whole worktree AppWorkbench diff remains 4 added/5 removed
  lines; App.tsx is unchanged. No authorization, capture or Stop policy changes.

#### Retained tests and browser evidence

Logs below are in `tools/computer-use-probe/.run/ui-native-20260925/`:

- `r12-resize-red.log`: missing keyboard focus reproduced. The initial green
  batch passed 70 tests; `r12-resize-close-red.log` separately reproduced the
  late-release reopen defect (4 passed / 1 failed).
- `r12-domain-green.log`: 256/256 across 27 files before the late-fit regressions.
  `r12-fit-race-red.log`: both genuine late-fit regressions failed / five passed.
  `r12-fit-race-green.log`: 21/21 in four focused files after repair, including
  the additional sidebar-fit regression.
- `r12-final-domain.log`: **259/259 across 27 files**, 30.63s. These are product
  component/hook/domain/API/locale tests, not native platform acceptance.
- Two dev-server rounds (`r12-browser.log`, `r12-browser-network.log`) timed out
  before assertions. Bounded network diagnostics showed `/src/styles/app.css`
  still pending with no JavaScript page error; the cause of that stalled dev
  transform is not established. Both original failures remain preserved.
- The harness now builds the fixture with the actual Vite product pipeline into
  a unique owned site directory, then serves the compiled fixture on loopback.
  It keeps all product styles and readiness checks, blocks external requests,
  does not overwrite production dist, and retains bounded failure diagnostics.
  `r12-browser-built.log` passed the original **50/50** scenarios.
- `ui-resize-fixture.tsx` uses the real hook, handle and Computer panel inside an
  explicitly probe-only shell. Four cases add real browser keyboard/pointer
  input at 1202/1440px in both themes: value/paint agreement, limits, focus,
  forced colors, persistence, Tab navigation and closing while the mouse remains
  down. They assert zero authorization/enable/Stop calls caused by resizing.
- `r12-resize-browser.log` first failed because the new harness assumed an open
  pane on cold start. The product intentionally restores width but starts closed.
  The harness was corrected to open explicitly, not to change that policy.
  `r12-resize-browser-r2.log`: **54/54**, terminal exit 0. Dark 1202px and light
  1440px focus screenshots were visually inspected. Artifacts:
  `tools/computer-use-probe/.run/ui-redesign-2026-09-25T19-40-11-469Z/`.
  Seed before/after remains
  `bf695fefb65bbcf0a7a9dd2f7f653832f47a3c1d1f561713904342757e562a0b`.
- `tsconfig.ui.json` now includes both visual fixture TSX files as well as
  production src in type checking. The first expanded check caught a nullable
  locale argument and a missing required runId in the fixture. Both were fixed;
  the original `r12-final-typecheck.log` is retained. The expanded rerun passed
  (`r12-final-typecheck-r2.log`), as did scoped ESLint (`r12-final-lint.log`) and
  both harness Node syntax checks. `r12-resize-browser-final.log` rebuilt the
  corrected source and again passed **54/54**, terminal exit 0, with the same
  unchanged runtime seed. Final artifacts:
  `tools/computer-use-probe/.run/ui-redesign-2026-09-25T19-52-37-938Z/`.

The resize fixture proves the product hook/handle's browser interaction, not
the complete AppWorkbench chrome, native permissions or installed App behavior.
The original 50 cases still use mocked Host IPC. Browser forced-colors emulation
is not proof of all native OS display scaling or accessibility configurations.

#### Native review and interference boundary

On the preceding R11 executable, an owned review window at 1202x802 was dragged
left from x744 to x638: the pane grew from 458px to 564px. Dragging right returned
to its valid 458px minimum. The earlier no-change attempts had started at that
minimum; there is no evidence that pointer resizing itself was broken. The
458px floor includes Windows chrome and preserves the chat column. That owned
process exited normally and its absence was confirmed.

`r12-native-build.log` then completed with exit 0: Vite 33.36s, Cargo 2m51s;
existing large-chunk and linker-stdout warnings remain. Executable SHA256:
`E5E90812C36DE3B55D02D612F45CAC65480CC8D23D87E415F22CC5442268EFCF`.
It predates the later stale-fit fencing change. The actual WebView2 accessibility
tree exposes the new localized separator and its value 458.

The owned launch was PID 75508, using the existing isolated review identifier
and home. After a stale/minimized-window error and one exact-target recovery,
the same window contained a new conversation and control state not created by
this agent's actions. Further native input stopped to avoid interfering with
other activity. No inference is made about who performed those actions. The
process was left running rather than forcibly closed. Native keyboard/consent/
running-control acceptance is **not** claimed from this interrupted attempt.
Do not relaunch or terminate it based solely on this recorded PID; revalidate
the exact executable and current ownership first.

U3, signed install/update/rollback, all native OS backends, real Grok E4 and the
final frozen active soak remain open. Goal stays active; no commit/push/PR.

### R13 — guided pairing, exact deadlines and keyboard continuity

Local September 26 (+08:00); screenshot directory timestamps below are September
25 UTC. This continues U2/U3, not a change to the full final-release scope.

- Pairing now shows **Start pairing → Confirm in App → Share a tab**. A Host
  challenge advances only the local workflow, not a fabricated connected/shared
  status. The last step remains current until the separate extension workflow;
  pairing/copying still never authorizes a control target.
- After explicit App confirmation, address/code are labelled readonly fields
  with individual copy buttons. Copy happens only on a user gesture and copies
  only the requested field, never the Host nonce or instance ID. A denied/missing
  clipboard API produces a localized manual-copy fallback, not the raw error.
- Pending copy clicks are deduplicated; fields remain manually selectable.
  Replacement/unmount/expiration fences late success and failure feedback. This
  does not claim that a previously dispatched OS clipboard write is cancellable.
- Confirmation and copy recheck the actual challenge deadline before dispatch.
  The one-second countdown is presentation, not authority between timer ticks.
- Keyboard confirmation moves focus from the removed button to the address
  field, then normal Tab order reaches both copy controls. An unrelated focused
  control is not displaced by a delayed confirmation response.
- All new labels are translated in the 15 catalogs. Narrow panes and enlarged
  text stack complete steps vertically; normal wider panes retain three columns.
  Step circles scale with text. Existing tokens, controls and fixed Stop footer
  remain; no parallel theme or App/AppWorkbench state was added.

#### Retained red/green evidence

Logs are in `tools/computer-use-probe/.run/ui-native-20260925/`:

- `r13-pairing-red.log`: two failed / four passed, before the real-deadline guard
  and copy controls. Initial focused green runs passed 48 and then 58 tests.
- `r13-domain-final.log`: one failed / 185 passed. The existing panel assertion
  still expected a paragraph; it now asserts the labelled readonly input's exact
  value. The separate confirmation and revoke checks remain. No product safety
  check was removed to obtain green results.
- `r13-pairing-focus-red.log`: one failed / seven passed; actual DOM focus fell
  to the body after confirmation. `r13-pairing-focus-green.log`: 18/18 across
  pairing/fields after the focus repair, including non-stealing late replies.
- `r13-domain-keyboard-final.log`: **273/273 across 28 files**, exit 0, 16.74s.
  Covers CU UI/domain/API, all locale catalogs and associated pane/resize tests;
  it is not the entire application's test suite or native platform acceptance.
- `r13-typecheck-final.log`, `r13-probe-typecheck-final.log` and
  `r13-lint-final.log`: product build typecheck, fixture-inclusive typecheck and
  scoped ESLint all exit 0. The helper also passes Node syntax checking.
- Four compiled browser rounds each passed 60/60. The first two preceded focus
  repair; the third verified natural keyboard order but preceded responsive-step
  styling. Those results are retained and are not relabelled as final-source
  acceptance. A visual review found that the third-column layout fragmented
  200% Tamil labels despite passing overflow tests; the responsive change fixes
  that, with an added stack-or-columns geometry assertion.
- **Final** `r13-pairing-browser-responsive.log`: **60/60**, terminal exit 0.
  Six new cases cover English 320×480, Chinese 400×480 and Tamil 200% text
  400×800, each in light/dark themes. They exercise real browser Enter/Tab input,
  local confirmation, exact copy fields, failure/manual selection, responsive
  step geometry and zero enable/authorize/capture calls from pairing/copying.

Final artifacts:
`tools/computer-use-probe/.run/ui-redesign-2026-09-25T22-08-37-048Z/`.
The 320px dark and 200% Tamil light step screenshots were visually re-inspected;
labels now form readable steps rather than word fragments. Page width and fixed
footer bounds pass in all cases. The runtime seed fingerprint before/after is
unchanged: `bf695fefb65bbcf0a7a9dd2f7f653832f47a3c1d1f561713904342757e562a0b`.

Host IPC and the clipboard are deliberately mocked **only inside the owned
browser fixture**. These tests never write the workstation clipboard or approve
a native permission prompt, and do not prove native clipboard/extension pairing
or installed-App behavior. PID 75508 was revalidated by executable path and
creation time and left untouched; its running executable predates this batch.

Remaining U3: current native App keyboard/overlay/scaling/consent acceptance,
real extension installation/pairing, and the Windows/macOS/Linux matrix. The
backend's pending native cancellation/recovery, browser exit/publication,
signed runtime install/update/rollback, real Grok E4 and frozen 12-hour active
soak gates remain open. Goal remains active. No commit, push or PR.

#### Fresh current-source native review — September 26, 08:55 +08:00

The current R13 frontend and current backend were built into a **new** isolated
Windows debug App at `src-tauri/target-cu-review/debug/grok-app.exe`. This does
not overwrite the still-running `target-cu` App or the installed September 22
test binary. Identifier: `com.grokapp.desktop.cu-review-r13-20260926`. App/agent
homes are beneath `.run/ui-native-20260925/app-home-r13-20260926`.

This layout-review home is explicitly initialized with independent sessions,
Chinese/dark presentation, the default ask policy and deferred account setup.
The welcome-tour state is still new. This is fixture setup, not evidence that
first-run setup, authentication or permission approval passed. Readback confirms
`computerUseEnabled=false`. No credentials, existing accounts or target grants
are copied into the fixture, and no prompt is sent to a model.

Build/packaging evidence in `.run/ui-native-20260925`:

- `r13-isolated-current-native-build.log`: the first CLI invocation rejected
  argument forwarding of `--offline` before compilation. Retained, not hidden.
- `r13-isolated-current-native-build-r2.log`: Cargo offline mode was supplied
  through its environment instead. UI build **45.34s**, native build **2m58s**;
  Tauri reports the built application. Existing chunk/linker warnings remain.
- The enclosing verification script then exits 1 because its no-seed-change
  assertion spans the intentional **prepare** step. That is not reported as an
  all-green build command. `r13-isolated-seed-before.json` is
  `bf695fefb65bbcf0a7a9dd2f7f653832f47a3c1d1f561713904342757e562a0b`;
  `r13-isolated-seed-after.json` is
  `c4430eb59a12744c3a7e8bea7334bc9a4a70bef40d99e4f9ed65fa9384bc88b2`.
  Both hygiene audits report zero hits. Do not reuse the old fingerprint as the
  current package's receipt or confuse prepare-time copying with UI mutation.
- `runtime_prepare.rs::pack_worker` maps production `server.mjs` to packaged
  `worker.mjs`. `r13-isolated-source-seed-match-r2.json` verifies that mapping,
  the manifest digest and all **18** sibling module hashes against current
  source. The first comparison used a nonexistent source `worker.mjs`; its
  incomplete receipt is retained and does not demonstrate a product mismatch.

Executable SHA-256:
`550F253CEF70F2DCD5D019ED37FC27EE2B4B3600C0BA9216FED89FC649B1A4FD`.
`r13-current-native-launch.json` records original PID **72600**, creation around
08:55:55 local, the exact executable, independent home and instance identity.
Revalidate identity before future use; a historical PID alone is not authority.

The returned native window was selected by its exact executable, not its shared
`Grok` title. WebView2 rendered the real dark workbench and welcome-tour overlay,
with Chinese accessibility labels and a readable, bounded modal. The native
input helper then returned `call get_window_state before using this window` on
the tour-dismiss action. A fresh accessibility observation followed by the one
permitted retry returned the same error. Input stopped; no keyboard, resize,
pairing, consent or running-control success is claimed. No alternate UI input
path was used to bypass that boundary. The independent window is left open at
the tour for a later verified review; pre-existing Apps remain untouched.

This proves current-binary launch and initial native rendering only, not signed
installation or the remainder of U3. The new prepared worker's regression run
is recorded separately; its result must be read from the actual terminal log.

The first post-prepare worker run is now terminal: `r13-repacked-worker-tests.log`,
**162 passed / 10 failed / 172 total**, exit 1. It omitted the two explicit
isolated-Chromium environment variables and used the test runner's default
parallelism. Five failures require that missing browser setup; the others
include native process discovery, profile removal and immediate-200 cleanup
assertions. They remain failures, not inferred passes. The intended follow-on
`check:computer-use` command did not run because the script stopped here.

Next validation uses a fresh writable copy of the pinned browser, explicit
`GROK_CU_CHROME` and `GROK_CU_TEST_CHROME`, and file concurrency one. Native
cleanup must still be confirmed independently; a bounded `pending/unknown`
reply is not success. This corrected setup cannot erase the original results
or establish that the known native-close long tail is fixed.

## R14 — Truthful target discovery and recoverable loading

Target discovery errors are now separate from authorization/action errors.
A failed refresh retires the old list and candidate, disables consent until a
successful discovery plus explicit selection, and never appears as a successful
empty list. Existing-run Pause and Stop remain independently available.

Screenshot review caught a defect even after the first 68 compiled-browser
cases passed: generic action-failure copy and an irrelevant selection hint.
The dedicated recovery message ships in all 15 catalogs. The next frozen
browser fixture passed **74/74**, including dark/light narrow Chinese and
German views and Tamil text at 200%. Chinese error screenshots show the clear
message and fixed control footer without the redundant hint; large text uses
the existing scrollable body rather than covering the controls.

A separate red regression then exposed an indefinitely pending target request.
Discovery now has a **10-second read-only UI deadline** and ignores expired
success/failure replies, including replies arriving during a new manual retry.
This is not cancellation of native input, proof of quiescence, automatic action
replay, or a substitute for backend timeout/cancellation gates. Unmount retires
its own deadline; successful retry does not erase a separate authorization error.

- Copy regression: 5 failed / 34 passed, then 189/189 after the fix.
- Discovery deadline regression: 7 failed / 1 passed, then 59/59 in the focused suite.
- Current UI/domain/catalog/adjacent suite: **197/197 in 23 files**.
- Final typecheck, fixture typecheck and scoped lint: **exit 0**.
- The 74-case artifact predates the discovery deadline. Its replacement passed
  **79/79, exit 0**, with five real-time pending/late-response cases across all
  four surfaces plus Chinese. Product timers were not shortened or mocked.
  Frozen artifact: `ui-redesign-2026-09-26T02-18-08-886Z`; no page errors.
- The current Chinese timeout screenshot was visually rechecked. Runtime seed
  before/after stayed `c4430eb59a12744c3a7e8bea7334bc9a4a70bef40d99e4f9ed65fa9384bc88b2`.
  `ui-r14-source-receipt.json` records the targeted source/catalog hashes.
- The isolated generated runtime separately passed the final offline
  `check:computer-use` import/integrity check and zero-hit hygiene audit,
  without changing its content. This does not close the earlier 157-second
  native exit failure or prove signed installation.

Full receipts and retained native failures live in
`2026-09-26-computer-use-late-cleanup-checkpoint.md`. The UI fixture mocks Host
IPC and is not evidence of installed-App permissions or actual desktop control.
No user App or running App seed has been replaced. U3 native acceptance and the
complete cross-platform Computer Use goal remain open; no publication occurs.

## R15 — Native settings review and integration fixes

The isolated native App now has successful real WebView2 input evidence: tour
dismissal, settings navigation, maintenance confirmation, Tab containment and
Escape focus return, narrow resizing, and light-theme navigation. It stayed
disabled; no deletion, repair, permission grant or model request was executed.
This supersedes R13's input-helper limitation, not its other acceptance gaps.

Native review exposed nested settings cards in the real ExtensionsPanel and
mixed-language activity-history copy. Both are fixed with actual-parent tests
and all 15 catalogs. Read-only settings loading is bounded to 10 seconds, with
explicit retry and late-reply fencing; mutation ownership is not timed out.
Visual review additionally replaced the maintenance confirmation's translucent
background with the existing opaque elevated surface.

Receipts, retained failures, current test scope and the important old-native-exe
versus new-source boundary live in
`2026-09-26-computer-use-settings-native-checkpoint.md`. Full U3, installed-App,
cross-platform and final-product acceptance remain open.

Final R15 source checks: **321/321 unit/component/domain/catalog tests**, both
typechecks and scoped lint exit 0, **85/85 compiled-browser cases** with terminal
exit 0. Six cases render the real extensions shell; two include the full real-time
read deadline and late-reply/manual-retry sequence. The runtime seed is unchanged.
The corrected opaque confirmation and timeout recovery were visually rechecked.
The earlier native executable remains unchanged and does not contain these fixes.

## R16 — Isolated settings reads and rebuilt native evidence

The actual Computer Use extensions route no longer starts unrelated CLI
skills/MCP/plugin inspections. Already-settled and late inspection errors remain
visible on their own tabs without contaminating Computer Use; returning to a CLI
tab refreshes it. Three new red cases failed first, then the focused settings and
integration batch passed 35/35. Typecheck/scoped lint and 85/85 compiled-browser
cases pass; six actual-shell cases now assert zero unrelated inspection calls.

A new 3464-file frozen snapshot built successfully and started in WebView2 with
no credentials or CU grants. The wrapper stderr failure and first fixture's BOM
encoding error are retained separately. After a corrected BOM-free test home,
native startup and product-tour dismissal were observed, but the input helper
rejected a freshly observed settings control even after one full rebind/retry.
Input was stopped; no alternative UI driver or permission bypass was used.

Updated native settings, confirmation/focus, narrow/light and installed/permission
acceptance remain **not_run**, not passed based on mock-browser or old R15 evidence.
Exact build/launch receipts, hashes, failures and the still-open final scope are
in `2026-09-26-computer-use-r16-native-ui-checkpoint.md`. Goal remains active.

## R17 — Independent mutation/readback, honest health and readable consent

Runtime repair/rollback no longer keeps the mutation latch hostage to its following
status read. Native mutation settlement alone releases that latch; independent
readback has the existing 10-second display deadline, explicit read-only retry,
and stale-reply fencing. No action timeout, replay, grant, or physical idle claim.
Old green health disappears while the mutation is pending; real action errors stay
visible during independent readback, and unmount prevents a late new read.

Visual review then caught an unknown-versus-unavailable defect. All 15 catalogs
now have a distinct unknown runtime state. The consent hint uses the existing
secondary text token instead of low-contrast tertiary text. In 15 compiled settings
scenarios, measured hint contrast is 4.742 (light) / 6.568 (dark), independent of
disabled controls. This measurement is scoped to that hint, not all UI elements.

226/226 component/domain/catalog/adjacent regressions, both TypeScript checks and
scoped lint pass. Final compiled-browser 89/89 passes with real-time readback cases,
zero page errors and unchanged seed; final dark/light screenshots were reviewed.
Host IPC is mocked. R17 also built and launched an isolated current-source native
App: dark/light settings, disclosure, solid confirmation, visible Tab focus and
Escape cancellation were checked in real WebView2. CU stayed off; no repair,
rollback, grant or deletion was performed. Edge resizing was not verified after
helper errors, so native narrow-window acceptance stays open. Receipts and scope are
tracked separately in `2026-09-26-computer-use-r17-settings-readback-checkpoint.md`.
The previous 89-case run predates the visual fix; both runs and red cases remain.

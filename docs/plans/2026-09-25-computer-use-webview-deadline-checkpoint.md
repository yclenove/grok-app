# WebView native execution deadlines and setup ordering — 2026-09-25

Local evidence date: 2026-09-25, UTC+08:00. Overall goal remains **active / partial — not releasable**.
Branch `feat/computer-use-implementation`, HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`.
Existing dirty/untracked work is preserved; no commit, push or PR.
This follows the [generic DOM checkpoint](2026-09-25-computer-use-webview-dom-checkpoint.md).

## Production changes

- Windows fixed Host scripts now carry a **12,000ms renderer-side execution deadline** on
  their exact `Runtime.evaluate` request and native `uniqueContextId`. This is not a Host
  response timeout, a global termination request or an automatic action retry.
- The CDP protocol specifies that unscoped `Runtime.terminateExecution` can terminate the
  **current or next** execution. That is not safe to dispatch merely because an old Host
  caller timed out or Stop was requested. The new bound is an execution backstop, **not
  immediate operation-specific user Stop**. That remaining requirement is not redefined.
- Script occupancy still survives logical cancellation, unbinding and a business timeout.
  A dispatched evaluation settles only on its actual native completion/rejection. Partial
  effects retain an unknown outcome and are not replayed.
- Native isolated-world setup joins two distinct messages: `Page.createIsolatedWorld`'s
  numeric response and `Runtime.executionContextCreated`'s exact unique identity. Either
  may arrive first. Both must match; duplicate, conflicting or numeric-only identities
  cannot authorize execution. The frame, random Host world name, document and binding
  checks remain in force.
- The entire pre-evaluation setup, starting before `Page.getFrameTree`, has a **5,000ms
  monotonic deadline** on the owning native message thread. It can release only work that
  has not dispatched an evaluation. The job becomes terminal before releasing its ticket;
  delayed callbacks are ignored. This deadline is removed before evaluation, not reused
  as a timer that could falsely declare a running operation idle.
- Native timer ID reuse is guarded by an ownership nonce and the replacement's actual
  monotonic deadline. Queued old timer messages and old lease drops cannot prematurely
  retire a replacement. A weak COM observer avoids a Job/WebView reference cycle; a
  bounded setup registry retains the waiting job until completion or setup expiry.

Protocol semantics were read from the official `ChromeDevTools/devtools-protocol`
`json/js_protocol.json` on this local date. No credentials or page content were logged.

## Evidence retained so far

Evidence directory: `tools/computer-use-probe/.run/mcp-transport-20260925T020240/`.

- `webview-native-deadline-first.log`: actual **failure**, stale snapshot/reference in the
  existing typed action fixture before the new deadline gate. The first failure is not
  overwritten or classified as a product fix. Stage-labelled and bounded fixture lifecycle
  diagnostics were added; the cause remains unproven pending reproduction.
- `webview-native-deadline-diagnostic.log`: actual native WebView2 **pass**. The new gate
  returned unknown at **8,022ms**, retained occupancy through Stop/unbind, and received
  native termination at **12,034ms**. Independent page-world counters were **entered=1,
  after=0, next=1**. The runaway loop really ended, the partial effect was not replayed,
  an old Stop did not affect the explicitly rebound next run, and duplicate actions failed.
  The original 10-second late-completion gate still passed at **8,025 / 10,039ms** with its
  original effect=1 contract. All other native gates in that run passed too.
- That diagnostic run used SHA-256
  `49ef2f021de752de2c565d4800bfd24e1988f4f1b147aa5bff117c96563b853e` and preceded the new
  setup join. It temporarily ran the deadline fixture earlier in the sequence to isolate
  that gate; the standard typed-action-first sequence has since been restored.
- `webview-native-deadline-app-tests.log`: **81/81**, no skips, 64.28s, including six new
  Windows deadline/setup tests. It precedes moving deadline arming ahead of the initial
  frame-tree call; final-source rebuild/revalidation is recorded below when complete.
- `webview-native-deadline-node-tests.log`: **193/193**, no skips, 3.94s. This is the selected
  extension/MCP/DOM suite, not every Node test in the repository.
- `webview-native-deadline-clippy.log`: strict probe/test App Clippy passed, 36.12s, also
  before the final setup-deadline arming move. Native builds preserve the existing linker
  stdout warning; it has not been suppressed or called a clean all-platform release.

## Final-source verification

Three consecutive native runs using the restored standard gate order all terminated with
exit 0. Binary SHA-256:
`58d46deedaa56abfa37a5869bbfa6c4fbbeccb1a3d267e3a609156a26fe1176d`.

| Final-source gate | Result | Artifact |
| --- | --- | --- |
| Native WebView2, round 1 | normal late callback 8,020 / 10,042ms; runaway 8,019 / 12,031ms | `webview-native-setup-acceptance-1.log` |
| Native WebView2, round 2 | normal late callback 8,030 / 10,040ms; runaway 8,027 / 12,041ms | `webview-native-setup-acceptance-2.log` |
| Native WebView2, round 3 | normal late callback 8,018 / 10,034ms; runaway 8,018 / 12,031ms | `webview-native-setup-acceptance-3.log` |
| Rebuilt App harness, all matching CU tests | 81/81, no skips, 73.10s | `webview-native-final-app-tests.log` |
| Strict probe/test App Clippy | exit 0, 45.47s | `webview-native-final-clippy.log` |
| Default product lib Clippy | exit 0, 1m01s | `webview-native-final-default-clippy.log` |

Every native round also passes the existing live binding, Chinese fill, stale/duplicate
reference rejection, navigation retirement, unsupported-origin rejection, 20 binding
rounds, page-world separation and generic DOM gates. Runaway counters remain exactly
**entered=1, after=0, next=1**. The DOM gate still observes **[1,0,1,1]** effects and
**[0,120]** scroll positions. The original 10-second action still finishes exactly once
after its 8-second business timeout. No deadline, Stop, or setup cleanup is misreported
as having undone a partial effect.

These are actual **owned Windows WebView2 fixtures** using the production adapter and
native dispatch, not installed App acceptance, a real model, another OS or the frozen
12-hour soak. The setup ordering, mismatch, timer reuse and late-tick tests are unit
evidence; native reordered/missing-event fault injection remains open. Three later passes
do not establish the cause or resolution of the first stale-reference failure.

The final 81 App tests contain 51 WebView tests (not 81 plus 51). Changed Rust formatting,
tracked diff whitespace and the explicitly checked new Rust/checkpoint files passed.
No cu_probe process remains. All owned execution handles were polled to terminal; no
ordinary browser or unrelated Cargo process was stopped. The goal remains active.

## Setup cancellation and native fault injection — local September 26

The setup-only native STA timer now checks cancellation and exact native-identity
retirement as well as its five-second setup deadline. It retires only work that
has not reached evaluation. The timer is still removed before `Runtime.evaluate`.
An already-running script continues to retain its execution ticket until its real
callback or existing native deadline; **immediate running-JS Stop is not implemented
by this change**.

Feature-gated fault injection suppresses a callback only after the actual native
frame-tree reply, execution-context event, or create-world reply has arrived.
For each phase, a real owned WebView2 fixture waits for that native fault point,
checks occupancy, cancels the exact old run, and independently observes old effect
zero and next-run effect one. A stale old Stop cannot cancel the next operation.
These hooks and the fixed fixture script are not a model-visible production API.

The first new native run failed earlier at typed fill after a trusted initial
resize. `webview-setup-cancel-native.log` remains preserved. The probe Host now
checks its actual native controller bounds against the complete, visible fixture
viewport and waits for two stable animation frames, with a four-second deadline.
It does not disable product resize retirement, retry model actions, or use a fixed
startup sleep. This readiness helper is probe-only.

Final probe SHA-256:
`078543EA18477436CC79A3C0947A0EE94FA79FFFA4518393F4996FDD9CE43BAF`.
All following runs finished with exit 0 in the normal full-gate order:

| Artifact in `.run/linux-native-20260925/` | Frame / context / create cancellation | Old / next effect |
| --- | --- | --- |
| `webview-setup-readiness-native.log` | 100 / 83 / 77 ms | 0 / 1 in every phase |
| `webview-setup-readiness-native-2.log` | 108 / 77 / 83 ms | 0 / 1 in every phase |
| `webview-setup-readiness-native-3.log` | 94 / 93 / 76 ms | 0 / 1 in every phase |

Despite its directory name, this evidence was produced on Windows. All previous
native isolation, generic DOM, Chinese fill, stale/duplicate ref, navigation,
20-binding-round, eight-second caller timeout and 12-second runaway gates still
run. Late normal actions finish once; runaway effects remain entered=1, after=0,
next=1. Those longer timings have not been re-labelled as prompt cancellation.

`setup-cancel-app-tests-build.log` built
`grok_app_lib-16074bb80c6f4177.exe`; the required Windows test manifest was embedded
after linking. `setup-cancel-app-tests.log` then passed **104 / 0 failed / 1 existing
ignored**, 66.41s. The ignored test is the explicitly manual native-desktop
`fixture_window_is_enumerable`, not a newly skipped regression. Strict probe/test
App Clippy (1m02s) and normal product-library Clippy (11.96s) both pass in
`setup-cancel-app-clippy.log` and `setup-cancel-default-clippy.log`.

## Remaining full-version gates — scope unchanged

1. Immediate, operation-specific WebView cancellation; native destruction/crash recovery;
   remaining setup-order permutations and broader flaky-fixture diagnosis. Three native
   missing-message cancellation phases are now tested, but do not by themselves prove
   every crash, reordered callback or already-running evaluation lifecycle requirement.
2. Native WebView isolation on macOS/Linux, complete macOS AX and X11 actions/lifecycle,
   and GNOME native Wayland Portal/PipeWire/input integration.
3. Mac signed runtime archive delivery and every platform's installed native
   install/update/rollback/signature/permission matrix.
4. Installed-App acceptance of the redesigned UI/UX: actual display scaling, overlays,
   authorization, Stop/Pause/Take Over and reconnect. The existing layout evidence uses
   mocked Host; it is not replaced or upgraded by these native fixture windows.
5. Real Grok E4 with planned explicit user intervention, signed Chrome/Edge matrix and
   outstanding pairing intermittence; then a frozen **12-hour active soak** after the
   last candidate-affecting edit. Never reuse canceled-task credentials.

No final release or goal completion is implied by this checkpoint.

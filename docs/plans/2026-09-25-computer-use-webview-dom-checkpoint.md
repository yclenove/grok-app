# WebView generic DOM references — 2026-09-25

Local evidence date: 2026-09-25, UTC+08:00. Overall goal remains **active / partial — not releasable**.
Branch `feat/computer-use-implementation`, HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`.
Existing dirty worktree preserved. No commit, push or PR.
This follows the [native isolation checkpoint](2026-09-25-computer-use-webview-isolation-checkpoint.md).

## Production changes

- `webview/dom.js` is the fixed Host kernel, called by `webview/dom.rs` with JSON data only.
  It runs through the existing owned native script path; Windows uses its real isolated world.
  There is no model-supplied eval, selector, cookie or authentication export.
- Observations mint fresh Host snapshot IDs and opaque per-snapshot references. The kernel
  retains actual DOM objects using weak references, not `#id` selectors. No-id controls,
  duplicate ids and open shadow roots are supported within the bounded main-document scan.
- Each node advertises only its supported typed actions. Scroll is semantic and targets the
  selected real overflow container/document scroller; no hardcoded `#scroller` and no claim
  of coordinate support without a screenshot. Observation now returns visible text, actual
  viewport dimensions and node geometry instead of a synthetic 1x1 image description.
- Hidden/password/input values, textarea contents and editable contents are not extracted.
  Names, text, references, nesting, mutation work, scripts and native replies remain bounded.
  The product observation budget stays 200ms, with at most 128 references / 32,000 text units.
- Exact owner/snapshot/reference/action validation happens in Rust and again in the native
  kernel. Removal/reinsertion, same-id replacement, identity/name changes away and back,
  shadow-host replacement, movement, disablement and lifecycle changes retire old authority.
  Both relevant label nodes and composed ancestors participate in mutation checks.
- UI preview honors `CaptureOptions.for_model=false`: it neither replaces native model
  handles nor publishes preview handles as model authority. Only a current successful model
  observation publishes references. Superseded and canceled replies cannot restore them.
- Action snapshots are consumed before side effects. Duplicate or unknown-outcome actions
  are not replayed. Existing run/binding/native-ticket cancellation fences remain in force.
  A returned script still means `applied`, not an independently `verified` business effect.
- Root CI now runs the DOM/extension/MCP Node regression command. This does not replace
  native gates, installed-App acceptance or the all-platform release matrix.

## Red-to-green evidence retained

Evidence directory: `tools/computer-use-probe/.run/mcp-transport-20260925T020240/`.

1. `webview-dom-initial-tests.log`: **16 pass / 2 fail**. An old reference could act after
   its shadow host was removed/reinserted, or its external label was changed/restored.
   Composed-ancestor and label mutation tracking repaired those real kernel defects.
   `webview-dom-fixed-tests.log`: **18/18**.
2. `webview-dom-node-regression.log`: combined parallel suite **188 pass / 4 fail**.
   Existing jsdom functional fixtures relied on real CPU scheduling: their real observation
   time budget expired, returning partial/empty results while tests required complete controls.
   The product did not crash and its deadline was not increased or disabled.
3. `dom-fixture-clock.mjs` now supplies a scoped deterministic clock **only during synchronous
   unit-fixture observations**. It restores the original clock before actions and on throws.
   Dedicated deadline tests bypass it and advance their own monotonic clock. Native tests use
   real clocks throughout. No assertion was deleted and no automatic action retry was added.
   `webview-dom-node-clock-fixed.log`: **192/192**; after adding clock-restoration coverage,
   `webview-dom-node-final.log`: **193/193**, including **19** new DOM/fixture tests, no skips.

## Real native WebView2 acceptance

Two complete runs, both terminal exit 0, against the same actual `cu_probe.exe`:

`af3dd4a96249626292de747601b825f2ba2929cb73bb8b9f6ab78795949f49b0` (SHA-256).

- `webview-dom-native.log`
- `webview-dom-native-repeat.log`

Both runs exercise the production adapter and shared native isolated-world dispatch using
owned fixture WebView2 windows. Independent page-world inspection confirms:

- No-id button operates once; preview handles are rejected while the original model action
  remains usable. A repeated action does not run again.
- With duplicate DOM ids, only the explicitly observed second button runs.
- A replaced DOM object rejects its old reference; a fresh observation permits the new object.
- Four fixture effect counters are exactly **[1, 0, 1, 1]**.
- Two independent scroll regions end at exactly **[0, 120]**; the untargeted one does not move.
- Existing Chinese fill, navigation, unsupported-origin rejection and 20 binding rounds pass.
- Page-world prototype interception counters stay zero; isolated context reuse and same-URL
  reload retirement still pass.
- Native timeout/callback timings are **8,032 / 10,046ms** and **8,019 / 10,037ms**. Occupancy
  survives timeout/cancel/unbind until the real callback. Late effect is exactly once.

These timeout results prove ownership retention, **not forced interruption of running JS**.
These are real Windows native probes, **not installed Tauri App, real Grok, signed browser
extension, macOS or Linux acceptance**. Both owned cu_probe processes exited.

## Regression and quality

| Check | Actual result | Artifact |
| --- | --- | --- |
| App check, lib/tests + probe | exit 0 | `webview-dom-check.log` |
| App harness, WebView filter | 45/45, no skips | `webview-dom-app-tests.log` |
| Final rebuilt App harness, all matching CU including session-manager tests | 75/75, no skips, 62.34s | `webview-dom-full-app-cu-tests.log` |
| Probe/test App Clippy, `-D warnings` | exit 0, 40.82s | `webview-dom-clippy.log` |
| Default product lib Clippy, `-D warnings` | exit 0, 29.04s | `webview-dom-default-clippy.log` |
| Selected extension/MCP/DOM Node suite | 193/193, no skips | `webview-dom-node-final.log` |
| Changed Rust formatting, fixed JS syntax, tracked diff whitespace | exit 0 | terminal checks |

The 45 WebView tests are a subset of the 75, not an additional 45. Node command includes
`tools/computer-use-extension/*.test.mjs`, `tools/computer-use-mcp/*.test.mjs`,
`tools/computer-use-probe/existing-mcp-live.test.mjs`, and `webview-dom.test.mjs`.
It is not every Node test in the repository. Harness manifests were embedded after linking.
The native build retains the pre-existing linker stdout warning. Existing Git LF-to-CRLF
warnings were not “fixed” with global configuration changes or unrelated newline rewrites.

## Remaining final-version gates — scope unchanged

1. Finish WebView native cancellation/destruction recovery and isolation setup race coverage.
   Implement real native isolated worlds on macOS/Linux; those paths were not fixed by this
   Windows-tested kernel. No downgrade to page-world eval is acceptable as final completion.
2. Complete native macOS AX actions, X11 actions and lifecycle, and GNOME native Wayland
   Portal/PipeWire/input integration. Keep unsupported capabilities honest until validated.
3. Finish Mac signed archive/runtime delivery, all-platform native install/update/rollback,
   signing and permission lifecycle. Tests/source locks alone do not establish delivery.
4. Validate redesigned UI/UX in the installed App: actual scaling, overlays, permissions,
   Stop/Pause/Take Over and reconnect. Existing UI layout screenshots use mocked Host.
5. Real-model E4, signed Chrome/Edge extension matrix and remaining pairing intermittence.
   Use the planned explicit user intervention; never repurpose canceled-task credentials.
6. Freeze the final candidate and run the required 12-hour active soak after the last
   candidate-affecting edit. This short native batch is not that soak.

No goal completion or publication is implied by this checkpoint.

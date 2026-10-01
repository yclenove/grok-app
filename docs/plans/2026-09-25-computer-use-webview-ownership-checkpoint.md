# WebView native execution ownership — 2026-09-25

Status: implemented and verified on the owned Windows WebView2 probe; the full
Computer Use goal remains **partial / not releasable**. No commit, push or PR.

## Product changes

- Native script tickets outlive business timeouts, cancellation, unbind and rebind.
  Only an actual native completion or a known pre-dispatch rejection releases them.
- Pending work is bounded to 64 entries, at most one per exact native instance.
  Lost callbacks cannot expire into a false idle state.
- Abort fences queued work using the exact run and execution epoch; epoch exhaustion
  retires authority instead of wrapping. Native dispatch checks document identity,
  caller cancellation and binding cancellation again on the native thread.
- Observation, capture, target list/claim/release now enforce exact run identity.
  Old completion cannot release a replacement, and rebind cannot bypass old occupancy.
- The product uses the captured native handle. Probe-only raw script/navigation
  helpers are not a fallback for product-owned script execution.

The real Broker test remains `stop_requested` after release until the late native
completion arrives, then reaches `stopped`; no Desktop fallback is involved.

## Verified evidence

Artifacts are under `tools/computer-use-probe/.run/mcp-transport-20260925T020240/`.

| Scope | Result | Artifact |
| --- | --- | --- |
| App Clippy, library + tests + probe feature, `-D warnings` | exit 0, 43.65s | `webview-ownership-probe-clippy-2.log` |
| App harness, WebView filter | 38/38, no skips | `webview-ownership-final-tests-2.log` |
| App test build | exit 0; required Windows manifest embedded after linking | `webview-ownership-final-build-2.log` |
| Rebuilt native probe | exit 0 | `webview-ownership-final-native-build.log` |
| Actual owned WebView2 | bind, click once, Chinese fill, document replacement, unsupported origin, 20 bind rounds, timeout gate all passed | `webview-ownership-final-native.log` |

Verified probe SHA-256:
`09f3ddf2d8e7ab38eef41f4f31880fc9444c94a545a633d7ef814c0b7897482c`.

The native timeout gate returned unknown at **8,012 ms**, remained occupied after
abort/unbind, and released only at the real callback at **10,025 ms**. The independently
read page effect was exactly 1; completion did not restore revoked authority; a new
explicit bind then worked. Use the final log and binary hash above, not timings
from older binaries, for this checkpoint.

Important: the page effect completed **after** cancellation. This is evidence of
truthful ownership and no replay, **not forced physical interruption**. This is an
isolated native fixture, not installed-App, real-model or all-platform acceptance.

## Retained first failures

- Initial channel result type inference failed compilation; fixed by typing the channel.
- Default product Clippy found raw navigation unused; it is now explicitly test/probe-only.
- Probe/tests Clippy found three identity `map_err` calls in a Windows fixture and one
  manual `contains` in a pairing assertion; repaired without warning suppression.
- Historical build/test logs remain preserved. No failing test was removed or ignored.

## UI and remaining full goal

The redesigned UI's full frontend regression passed 7,138/7,138 across 600 files;
whole frontend lint and production UI build passed. Vite's existing large-chunk
warnings remain visible. Layout evidence remains real Chromium with mocked Host,
not installed-App native permission, overlay or scaling acceptance.

Subsequent Windows isolation work is recorded in the
[native isolation checkpoint](2026-09-25-computer-use-webview-isolation-checkpoint.md).
At the ownership-only checkpoint, next W2 work was native isolated worlds,
generic opaque DOM references, physical
interruption and actual-destruction recovery. macOS/X11/Wayland functionality,
Mac signed runtime delivery, native all-platform packaging/update/rollback,
installed-App UI, real Grok and the frozen 12-hour active soak remain required.
Do not mark the overall goal complete on the strength of this batch.

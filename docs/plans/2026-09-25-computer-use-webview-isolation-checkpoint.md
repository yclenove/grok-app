# Windows WebView native isolation — 2026-09-25

Status: Windows production path implemented; owned native WebView2 acceptance passed.
The overall goal remains **active, partial / not releasable**. No commit, push or PR.
This follows the [native ownership checkpoint](2026-09-25-computer-use-webview-ownership-checkpoint.md).

## What changed

`webview/isolated_windows.rs` is shared by the real side-browser adapter and the
owned native probe. The product enters through Tauri's captured `with_webview`
handle and WebView2's native `CallDevToolsProtocolMethod`, not an external debugging
port. It obtains the main-frame identity, creates a native isolated world, and
accepts only the corresponding non-default isolated execution-context event.

Every evaluation uses the native **uniqueContextId**. Missing/mismatched identity
fails closed; there is no fallback to the page world or a reusable numeric context
ID. Model-provided eval/script parameters remain rejected by the typed adapter.
The bridge runs fixed Host scripts only; it does not export authentication data.

The context cache belongs to the exact native view and document generation, not
the current binding label or caller. It can be reused within a document, cannot
be published by an older generation, and bounds unsuccessful context setup.
Navigation and explicit authorization remain separate. The listener is removed
after setup; removing it never substitutes for script completion.

The asynchronous native command chain owns the existing execution ticket. Each
dispatch rechecks document and cancellation; timeout does not release occupancy.
Native exceptions and malformed/oversized replies are reported without page
content. A missing native callback is still not completion.

## Actual native acceptance

Both complete runs terminated with exit 0 against the same binary:

`d3cf378c7eeb32fc129dae1ab3dcaddd69d2055dbc5aff4540b29bd50bbe08db` (SHA-256).

Artifacts under `tools/computer-use-probe/.run/mcp-transport-20260925T020240/`:

- `webview-isolated-native.log`
- `webview-isolated-native-repeat.log`
- `webview-isolated-native-build.log`

The owned page deliberately replaces page-world `Document.querySelectorAll` and
`HTMLElement.click`. The production adapter still observes the real control and
clicks it exactly once: both interception counters stay 0; the independent raw
page read sees effect 1. A fixed probe-only marker remains invisible to the page,
persists across five Host calls, and disappears after an actual same-URL reload.
The old binding remains invalid; a new explicit binding succeeds.

Each full run also passes live binding/document replacement, Chinese fill, link
navigation, unsupported-origin rejection, 20 bind rounds and native timeout
ownership. Timeout/callback timings were **8,020 / 10,028 ms** and **8,023 / 10,037 ms**.
The late effect is exactly once, and its completion does not restore authority.
These timings still prove retention, **not forced interruption of running JS**.

This is real Windows WebView2 using the shared production dispatch module. It is
not installed Tauri App, real Grok, Chrome/Edge extension or macOS/Linux evidence.

## Regression and quality checks

| Check | Result | Artifact |
| --- | --- | --- |
| App check, probe feature | exit 0 | `webview-isolated-check.log` |
| App Clippy, lib + tests + probe feature, `-D warnings` | exit 0, 29.48s | `webview-isolated-clippy.log` |
| App harness, WebView filter | 40/40, no skips | `webview-isolated-tests.log` |
| App harness, all matching Computer Use tests including session-manager wiring | 70/70, no skips, 62.49s | `webview-isolated-app-cu-tests.log` |
| Default product lib Clippy, `-D warnings` | exit 0, 24.65s | `webview-isolated-default-clippy.log` |
| Changed Rust modules, rustfmt check | exit 0 | terminal check |
| Worktree tracked diff whitespace check | exit 0 | terminal check |

The Windows manifest was embedded after linking the App test harness. Native
build retains the existing linker-stdout warning. Git still reports existing
LF-to-CRLF conversion warnings; no global Git setting or repository-wide newline
rewrite was performed. Both owned cu_probe processes exited; none remained in
the final exact-name process check.

The preceding UI redesign evidence remains 7,138 frontend tests / whole frontend
lint / production UI build passed, plus the 33-case immutable-browser layout
matrix. Those fixtures use mocked Host. This native batch does not upgrade them
to installed-App, OS-scaling or native-permission acceptance.

## Still required — original full scope preserved

1. Finish WebView W2: generic opaque DOM handles instead of id selectors, correct
   scroll targeting, native isolation on macOS/Linux, physical cancellation and
   safe recovery after actual native destruction. The non-Windows script path is
   unchanged and still lacks native isolated worlds.
2. Harden isolation setup callback ordering/failure recovery and exercise it in
   the installed Tauri App, including close/navigation/cancel during each setup
   step. The current path rejects missing identity and never relaxes isolation;
   two successful native rounds are not a long-run availability guarantee.
3. Complete macOS AX actions, X11 actions/lifecycle, GNOME native Wayland portal,
   PipeWire and input integration. Keep native Wayland false until real acceptance.
4. Finish Mac signed archive/runtime delivery and native all-platform
   install/update/rollback, signatures and permission lifecycle.
5. Installed-App UI/UX/scaling/overlays/permissions and real-model acceptance;
   signed Chrome/Edge matrix; remaining pairing intermittence; frozen 12-hour
   active soak after the final candidate-affecting edit.

No partial suite, mock, source check or this Windows slice closes the final goal.

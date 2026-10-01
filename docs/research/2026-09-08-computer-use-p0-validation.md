# Computer Use P0 validation

Date: 2026-09-08. Host: Windows 11 x64 (`win32-x64`, `10.0.26200`).
Branch: `feat/computer-use-implementation` @ `30757366` + local uncommitted implementation.
Evidence dir: `{SCRATCH}/p0/` and `tools/computer-use-probe/.run/p0/`.

Status vocabulary: `passed` / `failed` / `not_run`. Cross-compile is not native.

## Environment

| Item | Value | Evidence class |
| --- | --- | --- |
| OS / arch | Windows 11 x64 | native |
| Grok CLI | `grok 1.0.13 (5e9a58528b76)` | native |
| Login | grok.com (not printed) | native |
| Default model | grok-4.6 | native |
| Available models | grok-4.6, grok-4.5 | native |
| Effort used in probes | not passed (CLI default) | native |
| Chrome | `C:\Program Files\Google\Chrome\Application\chrome.exe` | native |
| Edge | installed | native |
| Cua Driver CLI | not on PATH; private debug build at `tools/computer-use-cua/libs/cua-driver/rust/target/debug/cua-driver.exe` | native (local build, not installed) |
| Playwright worker | `playwright-core` 1.63.0 in `tools/computer-use-browser/` (not root runtime) | native |

## P0.1 CLI / tools visibility

Command: `grok --version` (exit 0, 53 ms); `grok models` (exit 0, 2654 ms).

ACP inject: Computer Use MCP is appended in `build_session_mcp_servers_with_opts` **only when** `AppSettings.computer_use_enabled` is true (default **false**). Independent of `official_aux_with_user_mcp`.

`tools/list` of the synthetic probe MCP returns `computer_observe` only. Product MCP (`tools/computer-use-mcp/server.mjs`) lists the typed computer_* / browser_* tools and forwards to Host.

## P0.2 Synthetic image MCP (twice, same fixture)

Command: `node tools/computer-use-probe/run-p0.mjs`

Seed 42, 400×300 PNG, red box + 4-digit code. Oracle written to `oracle.json` beside the process.

| Pass | hasImage | imageBytes | oracle in tool text | tools/list |
| --- | --- | --- | --- | --- |
| 1 | true | 2381 | false | computer_observe |
| 2 | true | 2381 | false | computer_observe |

Consistent content: **yes**. Model-visible text is snapshot/geometry JSON only.

Live vision model: **passed** (bounded). Isolated `GROK_HOME` (copy of `auth.json` only, no rewrite of `~/.grok`) + `grok -p --model grok-4.6 --effort low`. Stdout ended with the oracle digits `6011`. Tool text JSON does not contain `6011`. Evidence: `tools/computer-use-probe/.run/p0/p8-model.json`. This is observe-and-read, not a full observe→act→verify desktop loop (P8 still open).

## P0.3 Output contract

Synthetic MCP returns MCP `image` + short text JSON. No oracle code, no `oracle` key. Coordinate space declared `image_pixels`.

## P0.4 Cua target bind + cancel

Cua Driver is not on PATH. Built from the gitignored sparse clone (`c5a15f3`) after disabling the optional `rego` feature (upstream `regorus` needs Spectre-mitigated MSVC libs that this machine does not have). Binary: `tools/computer-use-cua/libs/cua-driver/rust/target/debug/cua-driver.exe`.

Cua native target/cancel: **passed**. Probe `node tools/computer-use-probe/p0-cua-bind.mjs` started a private named pipe, spawned `GrokCuFixture-cua-<pid>`, bound that window (`get_window_state` saw the Count button), then `cua-driver stop`. Daemon status after stop: not running. Did not bind or click ChatGPT/微信.

Windows Win32 adapter on a self-built `GrokCuFixture-p8-<pid>` window: **passed** via `cargo run --bin cu_probe`. Observe 404×201; directed child click; independent file postcondition clicks≥1 (not “tool returned ok”). `grok_app_lib` unit-test exe still fails to start (`STATUS_ENTRYPOINT_NOT_FOUND`); gates also run through `cu_probe`.

## P0.5 Existing-tab routes

Compared Playwright Chrome extension (docs: tab groups, `PLAYWRIGHT_MCP_EXTENSION_TOKEN`, profile dir) vs Tencent BrowserSkill (research cache plugin; plugin-level session ownership).

**Default chosen: Playwright extension / CDP for existing Chrome/Edge.** Reasons:

1. Managed browser is already Playwright; one controller per tab.
2. BrowserSkill plugin session ≠ Grok `appSessionId` / multi-chat authorization.
3. Pairing must check real extension identity, not Origin shape.

Borrow/return and “do not close user tab on cancel” remain P7 implementation requirements on that path.

Managed Chromium form fixture through the **shipped worker** (`tools/computer-use-browser/server.mjs`) twice with isolated persistent profiles: **passed**. Postcondition `{name:"fixture","count":2}` both passes; second owner on a live profile returns 409; localStorage marker does not leak across profiles. Evidence: `tools/computer-use-browser/.run/playwright-form.json` and `{SCRATCH}/browser/playwright-form.json`.

Existing-tab extension pairing / two-session isolation: **not_run** (Playwright extension not installed in this Goal).

## P0.6 Four-target matrix

| Target | Evidence class | Result |
| --- | --- | --- |
| Windows x64 | native host | this machine; adapter present; install package **not_run** |
| macOS arm64 | not_run | no Apple Silicon |
| macOS x64 | not_run | no Intel Mac |
| Linux x64 GNOME Wayland | not_run | no Ubuntu; must not substitute browser/XWayland |
| Linux x64 X11 | not_run | same |

macOS adapter enumerates via CGWindowList and posts CGEvent; Screen Recording / Accessibility are probed at runtime. Linux adapter uses libX11 + XTEST on real X11 and **refuses** to treat XWayland or a browser as GNOME Wayland. Native fixture rows stay **not_run** on this Windows host (`cu_probe` prints that).

## P0.7 Decision

- Go for shared Host Broker + FakeAdapter contract (P1–P2 code landed).
- Go for Playwright as managed browser and existing-tab default.
- No-go for claiming Cua, macOS, or GNOME Wayland native until those rows have native evidence.
- Feature flag remains **default off**.

## Commands to reproduce

```
node tools/computer-use-probe/run-p0.mjs
pnpm test -- src/lib/computer-use/protocol.test.ts src/i18n/messages.test.ts src/lib/settingsCatalog.test.ts
```

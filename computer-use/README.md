# Grok Computer Use

[English](README.md) · [中文](README_ZH.md)

`gcu` is the command-line MCP server for Grok Computer Use. The MCP server name is `grok-computer-use`. The license is MIT. A release is one native binary per platform. It does not bundle a browser, Node, Python, or .NET. The npm package only needs Node 18 or newer to start the binary that is already installed beside it.

This page matches the `gcu` command and the release packager in this tree. A clean virtual-machine install on macOS, Windows, and Linux was not run (see [STATUS P4-07](../docs/plans/computer-use-v2/STATUS.md)). **未验证** means that behavior was not demonstrated on that machine. GitHub archives and the `.deb` / `.rpm` packages bundle [packages/README.md](packages/README.md), not this file.

## What this build runs

| Command | What it does in this build |
|---|---|
| `gcu version`, `gcu install`, `gcu uninstall`, `gcu approvals` | Work without a desktop driver. |
| `gcu mcp` on macOS, without `--driver` | Forwards stdio to `Grok Computer Use.app`. The helper shares the Host Stop latch. A live desktop, helper TCC, the physical hotkey, the App subprocess chain, and a real `gcu resume` are **未验证**. |
| `gcu`, with no subcommand, on macOS | Asks Launch Services to start the helper (`open -g -j -b <bundle id> --args agent`). |
| `gcu doctor`, `gcu call`, and `gcu mcp` on Linux and Windows | Omitting `--driver` selects `auto`. `auto` and `native` exit 2 when that route is not compiled, and they do not fall back to fake. `--driver fake` is the explicit offline driver. |
| `gcu` with no subcommand, on Linux and Windows | Exits 2 with that same message. It does not print doctor. |
| `gcu doctor` and `gcu call` on macOS | Same rule. They do not talk to the helper. |
| `gcu engine` | Private channels for the Grok App host. Not the way to attach a CLI client. |
| `gcu agent` | The helper process. Launch Services must start it. A shell does not receive the helper's permission grant. |

`--driver` is `auto` (the default), `native`, or `fake`. They do not fall back to fake, widen input, or elevate. On Linux and Windows, `auto` and `native` exit 2. On macOS, `gcu mcp` forwards to the helper, `gcu engine` assembles a driver only inside an authenticated App subprocess and only claims that TCC chain for macOS 27 arm64, and `gcu call`, `gcu doctor`, and HTTP MCP exit 2 and point at the helper or the App. `--scene` is valid only with `--driver fake`. Omitted fake uses `form`. The fake desktop does not send operating-system input. A release binary also needs `GCU_ALLOW_FAKE=1`. A debug build does not. Scenes: `form`, `chat`, `list`, `modal`, `webarea`, `canvas`, `ide`, `login`, `terminal`, `web_terminal`, `submit_unknown`.

There is no `gcu batch`, `gcu script`, or `gcu approvals revoke-all`.

## Install

Supported targets in the release workflow: macOS 14+ arm64 and x64, Windows 10 1903+ x64, Linux glibc on x64 and arm64. Linux binaries are built on Ubuntu 22.04. musl and Alpine are not targets. Windows arm64 is not a release target.

Pick the archive that matches the machine:

| Machine | Archive |
|---|---|
| macOS Apple silicon | `gcu-<version>-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `gcu-<version>-x86_64-apple-darwin.tar.gz` |
| Windows x64 | `gcu-<version>-x86_64-pc-windows-msvc.zip` |
| Linux x64 | `gcu-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| Linux arm64 | `gcu-<version>-aarch64-unknown-linux-gnu.tar.gz` |

Tags are `cu-v<version>`. A manual workflow run builds artifacts and does not publish a release. Homebrew and winget manifests are produced for a person to submit. This workflow does not open those pull requests.

### Check the download

A checksum detects corruption. It does not prove who published the file. Tag releases can also include `SHA256SUMS.sigstore.json`. Manual runs skip that bundle.

```sh
sha256sum --check --ignore-missing SHA256SUMS
```

On macOS use `shasum -a 256 -c SHA256SUMS`. On Windows compare `Get-FileHash <archive> -Algorithm SHA256` with `SHA256SUMS`.

When the sigstore bundle is present, verify it with `cosign verify-blob`. The certificate identity is this repository's `.github/workflows/release-computer-use.yml` at the tag, and the OIDC issuer is `https://token.actions.githubusercontent.com`. The exact command is in [packages/README.md](packages/README.md).

### GitHub Release

Extract the archive into a new directory and add that directory to `PATH`. On macOS keep `Grok Computer Use.app` beside `gcu`. Register the helper before first use, with the absolute path:

```sh
open -Ra '/absolute/path/Grok Computer Use.app'
gcu version
```

The release app name is `Grok Computer Use`. Its bundle id is `ai.grok.computer-use.helper`. A debug build uses `Grok Computer Use (Dev).app` and `ai.grok.computer-use.helper.dev`. The file inside the app and the `gcu` beside it must be the same bytes. The helper exits after 10 minutes with no connection.

### npm

Requires a published release and Node 18 or newer. The launcher does not download a binary. It runs the optional package that npm already installed.

```sh
npm install -g gcu@<version>
```

If optional dependencies were omitted, the launcher prints `gcu: missing @grok-computer-use/<platform>; reinstall with optional dependencies enabled` and exits 1. `<platform>` is `darwin-arm64`, `darwin-x64`, `win32-x64`, `linux-x64`, or `linux-arm64`. Any other pair prints `gcu: unsupported platform <platform>` and exits 1. If the installed file cannot be started, it prints `gcu: cannot execute the installed native binary` and exits 1.

On macOS, register the helper that sits next to the binary:

```sh
open -Ra "$(npm root -g)/@grok-computer-use/darwin-arm64/bin/Grok Computer Use.app"
```

Use `darwin-x64` on Intel.

### Homebrew

The workflow writes a formula. It does not publish a tap. From the generated formula file:

```sh
brew install --formula ./gcu.rb
```

The formula's caveats print the `open -Ra` path. It is `$(brew --prefix gcu)/libexec/Grok Computer Use.app`. Remove with `brew uninstall gcu`.

### winget

The workflow writes three manifests for `RongleCat.GrokComputerUse`. Validate them with `winget validate --manifest <directory>` before anyone submits them. After winget lists the package:

```sh
winget install RongleCat.GrokComputerUse
winget uninstall RongleCat.GrokComputerUse
```

### Debian and RPM

Package name `grok-computer-use`. The binary is `/usr/bin/gcu`. Shared libraries come from the package manager. The desktop's accessibility service is still required. These packages do not contain the macOS helper.

```sh
sudo apt install ./gcu-<version>-<target>.deb
sudo apt remove grok-computer-use
sudo dnf install ./gcu-<version>-<target>.rpm
sudo dnf remove grok-computer-use
```

### Unsigned packages

If the Apple signing secrets are absent, the helper keeps an identifier-only ad-hoc signature. It is not Developer ID signed and it is not notarized. Gatekeeper can block it. Do not turn Gatekeeper off. After each update of an ad-hoc build, check the helper again in Privacy & Security. An ad-hoc signature is not a publisher identity.

To open that one app:

1. In Finder, Control-click `Grok Computer Use.app` and choose Open, then Open.
2. Or open System Settings → Privacy & Security and choose Open Anyway for that app.

If quarantine metadata is the blocker, clear it on the helper only, then open it:

```sh
xattr -cr '/absolute/path/Grok Computer Use.app'
open '/absolute/path/Grok Computer Use.app'
```

If the Windows certificate is absent, `gcu.exe` is unsigned. SmartScreen can say the publisher is unknown. Choose More info → Run anyway. Check `SHA256SUMS` first. An unsigned file can also trip heuristic antivirus. The checksum shows the bytes were not altered. It does not make the publisher known. Signing failures abort the build. They do not fall back to an unsigned file when a certificate was configured.

## Permissions

Grant access to the helper or to the desktop session. Do not grant screen capture to the terminal.

### macOS

Accessibility and Screen Recording belong to the helper app, not to Terminal, iTerm, or the editor that started `gcu mcp`.

Register the app with `open -Ra`, then start it through Launch Services. `gcu` with no subcommand does that on macOS. `gcu mcp` does it when the helper is not already running. Do not type `gcu agent` in a terminal to obtain the grant.

The release macOS build includes an onboarding window (`macos-webview`). A build without that feature does not open the window. The window opens only when Accessibility or Screen Recording is missing. It does not grant either permission and it does not click the system dialog.

The window title is "Allow Computer Use". It has two cards:

| Card | What the window says | When it takes effect |
|---|---|---|
| Accessibility | Lets the helper read the focused app. | As soon as you allow it. |
| Screen Recording | Lets the helper see the screen. | Only after the helper restarts. |

"Not now" is focused. Escape closes the window. Enter does not open Settings and does not restart the helper. "Open Settings" opens the matching Privacy & Security pane. After you allow Screen Recording, the card says "Allowed — restart the helper". Use "Restart helper". If the restart fails, the window stays open and says the helper did not restart.

Clicking through this window on a Mac is **未验证**. Whether the system prompt names the helper, rather than the terminal, is **未验证**.

`list_apps` does not need those two permissions. Window listing, launch, and actions do. A missing grant returns `permission_missing` (exit 3 from `gcu call`). A visible security prompt owned by SecurityAgent, CoreAuthenticationUI, or UserNotificationCenter is refused.

### Windows

There is no TCC prompt. The driver reads UI Automation and refuses to raise its own integrity.

A target at a higher integrity level, including an elevated installer or a Settings window at High integrity while `gcu` is Medium, returns `elevation_required`: "The target has a higher integrity level." The driver does not elevate and does not request UIAccess. UIPI is not bypassed. The secure desktop (UAC, sign-in, lock) is refused.

On the GitHub `windows-2022` runner the harness used a restricted token. Foreground mouse delivery was not proven there, so foreground stays unavailable. A normal interactive desktop with a physical mouse is **未验证**.

### Linux

Pure X11 uses AT-SPI for the tree and actions, and the X server for windows and screenshots. An occluded window is captured only when a composite manager can provide that window's pixels. Otherwise the driver refuses the capture. It does not return another window's contents.

The full Wayland guide is [docs/wayland.md](docs/wayland.md) ([中文](docs/wayland.md#zh)). Wayland uses xdg-desktop-portal ScreenCast and RemoteDesktop. You approve the portal dialog. Only the source you selected is captured. An unknown source or an unknown compositor produces no image and no silent full-desktop fallback. A restore token is stored owner-only, expires after 24 hours, and is rejected when its source changes. Native Wayland has no global overlay. XWayland is not treated as a place where a global Stop hotkey can be proved, so that path does not enable the overlay.

A real GNOME, KDE, or wlroots desktop session is **未验证**. The portable Wayland tests passed. The headless GNOME job was not run.

## Connect a client

`gcu install` and `gcu uninstall` edit a client config. They do not start a driver. The server name they write is `grok-computer-use` (`computer-use` is reserved by Claude Code). The command is the absolute path of this `gcu`, with argument `mcp`.

```sh
gcu install grok --dry-run
gcu install --client codex --scope user --json
gcu uninstall --client claude --yes
```

Clients: `grok`, `codex`, `claude`, `antigravity`, `gemini`, `opencode`, `cursor`, `vscode`. `--scope` is `user` (default) or `project`. Pass the client as the positional argument or as `--client`. `--dry-run` prints a diff and writes nothing. `--json` prints one object. Repeating install does not duplicate the entry. Uninstall removes only `grok-computer-use` and hooks this installer owns.

On Linux and Windows the installed command is `gcu mcp` with no `--driver`, which selects `auto` and exits 2 while the native route is not compiled. It does not fall back to fake. The installer does not add `--driver fake`. On macOS the same command is the helper proxy described above.

Any real change under the Grok directory asks on a terminal:

```text
gcu will modify files under ~/.grok (MCP config and the Stop hook). Continue? [y/N]
```

Answer `y` or `yes`. Anything else cancels. A non-terminal needs `--yes`. A pipe or `GCU_YES` is not confirmation. The same prompt applies when a project-scoped Grok install writes the Stop hook under the Grok home. `--grok-home` replaces `~/.grok`.

| Client | User scope | Project scope |
|---|---|---|
| `grok` | `<grok-home>/config.toml`, default `~/.grok/config.toml`. Stop hook `~/.grok/hooks/computer-use-turn-ended.sh` (`.cmd` on Windows). The hook calls `gcu turn-ended` only when `reason` is `end_turn`. | `<project>/.grok/config.toml`. The Stop hook still lives in the Grok home. |
| `codex` | `~/.codex/config.toml`, including `notify = ["<gcu>", "turn-ended"]`. | `<project>/.codex/config.toml` for MCP. `notify` stays in the user file. |
| `claude` | `~/.claude.json` | `<project>/.mcp.json` |
| `antigravity` | `agy mcp add` / `agy mcp remove`. `--agy` selects the binary. | `<project>/.agents/mcp_config.json` |
| `gemini` | `~/.gemini/settings.json` | `<project>/.gemini/settings.json` |
| `opencode` | `~/.config/opencode/opencode.json` | `<project>/opencode.json` |
| `cursor` | `~/.cursor/mcp.json` | `<project>/.cursor/mcp.json` |
| `vscode` | User `Code/User/mcp.json` under Application Support (macOS), `%APPDATA%` (Windows), or `~/.config` (Linux). | `<project>/.vscode/mcp.json` |

`--home` overrides `GCU_HOME`, which overrides `HOME` (`USERPROFILE` on Windows). `--project` overrides `GCU_PROJECT`, which overrides the working directory. `--gcu` is the absolute executable written into the config.

Before a real write, the original file is copied to `<name>.gcu-backup-<unix-time>` beside it. Backups are owner-only (`0600` on Unix, an owner-only ACL on Windows). A file this command will not parse, including OpenCode JSONC it cannot read, is left unchanged (exit 2). An `agy` command failure is exit 10. A missing `agy` binary is exit 2.

An image is attached for the model only when its short edge is at least 256. A shorter image is enlarged once inside the already-redacted frame. If it still cannot be attached, the result keeps the screenshot file path and the note `[image attachment omitted: short edge below 256; use screenshot file]`. See [ADR 0001](docs/adr/0001-grok-build-mcp-capabilities.md).

## Approvals

An app is allowed at one of four levels. Timeout is not a fifth level. The wait is 120 seconds, then the request is denied.

| Level | Lasts | Stored? |
|---|---|---|
| This turn (`turn`) | Until the turn ends. Grok's Stop hook sends `end_turn`. Codex uses `notify`. With no hook, an idle 45 seconds ends the turn. | No. |
| This session (`session`) | Until the MCP session closes. | No. |
| Always allow (`always`) | Until you revoke it. | Yes, in `approvals.json`, mode `0600`. |
| Deny (`deny`) | This request only. | No. |

The client is asked first, inside the tool call, when it offers form elicitation. The choices are `turn`, `session`, `always`, and `deny`. An `always` answer from elicitation is not stored. Cancel, including a headless client that cancels immediately, is not approval. The local dialog then asks. That dialog is what writes `approvals.json` after you pick Always.

This build has no WebKit approval window. A desktop falls back to zenity, then kdialog, on Linux; `osascript` on macOS; PowerShell on Windows. The first choice is Deny. Enter in the chat box does not approve. With no display, in CI, or when none of those tools exist, the request is denied. A release build rejects every `GCU_APPROVAL` value and exits 2. That variable exists only in a debug build compiled with `eval-autoapprove`.

The dialog shows an operating-system check of signature, publisher, and path. The app name and icon are labels, not the identity. A model reason is plain text marked "Model-provided". A signature does not cover plugins loaded into the app.

A sensitive action is a separate confirmation (allow or deny), not one of the four grants. Deny, timeout, and cancel run nothing. If the screen changes while the dialog is open, the confirmation is void. If the sensitive area cannot be located, the unredacted screen is not shown.

There is no command that grants every app.

```sh
gcu approvals list
gcu approvals list --json
gcu approvals revoke KEY
```

`KEY` is a line printed by `list`. A missing key prints a message and exits 2. An unreadable approvals file exits 2.

The file is:

| System | Path |
|---|---|
| macOS | `~/Library/Application Support/grok-computer-use/state/approvals.json` |
| Windows | `%LOCALAPPDATA%\grok-computer-use\state\approvals.json` |
| Linux | `${XDG_STATE_HOME:-~/.local/state}/grok-computer-use/approvals.json` |

`gcu` reads only `[observe]`, `[policy]`, and `[guard]` from `config.toml`. It does not rewrite that file to store grants.

| System | `config.toml` |
|---|---|
| macOS | `~/Library/Application Support/grok-computer-use/config.toml` (beside the `state` directory) |
| Windows | `%APPDATA%\grok-computer-use\config.toml` |
| Linux | `${XDG_CONFIG_HOME:-~/.config}/grok-computer-use/config.toml` |

A present file with an empty `stop_hotkey`, a budget or image format outside the accepted set, or an unknown key inside those three tables exits 2. Other top-level tables are ignored. A missing file uses built-in defaults. The hotkey the drivers register is the built-in chord below. This helper does not pass the config string into the macOS driver.

## Stop and resume

Stop is latched. The next model turn, a reconnect, and a new process do not clear it. The marker is `stopped` in the state directory above. Resume is not an MCP tool.

Built-in Stop chords, when that driver is actually running:

| System | Chord | Demonstrated |
|---|---|---|
| macOS | Control+Option+Command+. | Physical key **未验证**. |
| Windows | Ctrl+Alt+Shift+Backspace | The hosted runner skipped the hotkey. Physical key **未验证**. |
| Linux X11 | Ctrl+Alt+Shift+Backspace | Xvfb grab test passed. Physical key **未验证**. |
| Native Wayland | No proved global hotkey | Dependent input stays off. Real session **未验证**. |

For an in-process server, including `--driver fake`, SIGINT and SIGTERM latch Stop and release held input. `gcu mcp` on macOS without `--driver` only forwards stdio. This guide does not claim that Ctrl+C in that proxy latches the helper.

The macOS helper shares the Host Stop latch. It does not start a private already-stopped latch. Live helper actions, helper-only TCC, the App subprocess chain, the physical hotkey, and a real `gcu resume` are **未验证**. macOS 14, macOS 15, and Intel are not claimed. The configured hotkey string is still not passed into the driver.

`gcu-overlay` draws a Stop control and can exclude itself from capture on macOS and Windows. It is not linked into this `gcu` binary. Its real GUI is **未验证**. `GCU_OVERLAY=0` would hide that UI only. It is not a switch in this binary.

To resume an in-process session, both the terminal input and output must be TTYs:

```sh
gcu resume
gcu resume --session /path/to/gcu-control-directory
```

The process prints `You are resuming stopped model operations. Type yes <challenge> to confirm:`. Type `yes` and the eight hex characters, within 120 seconds. If several control directories exist, pass `--session`. They are named `gcu-control-*` under `$XDG_RUNTIME_DIR` or the system temp directory. A wrong confirmation leaves the session stopped.

This check is best effort. A program running as the same user can forge a TTY. See [ADR 0003](docs/adr/0003-resume-trust.md). `gcu turn-ended` retires a turn grant. It does not clear Stop. Hook failures print one redacted line and exit 0.

## Platform differences

Driver libraries and the `gcu` command are not the same thing. The table is what tests and [STATUS](../docs/plans/computer-use-v2/STATUS.md) showed. "In `gcu`" is what this binary wires up.

| Capability | macOS | Windows | Linux X11 | Linux Wayland |
|---|---|---|---|---|
| Real driver in `gcu mcp` | Helper proxy. The helper shares the Host Stop latch. Live use, TCC, and `gcu resume` **未验证**. | Not assembled. `auto` and `native` exit 2 and do not fall back to fake. | Not linked into `gcu`. `auto` and `native` exit 2 and do not fall back to fake. | Same as X11 for the CLI. |
| Accessibility tree | AX. Mini, macOS 27, arm64: live contract 26 passed, 4 skipped, 0 failed. First read can be a short incomplete tree. macOS 14/15 and Intel **未验证**. | UIA on the `windows-2022` runner: contract 15 passed, 15 skipped, 0 failed. First read can be incomplete. Physical desktop **未验证**. | AT-SPI on Xvfb CI: 23 passed, 6 skipped, 0 failed. Physical desktop **未验证**. | Detection and portal code exist. Real compositor **未验证**. |
| Window screenshot | ScreenCaptureKit, including a covered window, on that Mini for the driver tests. Packaged helper **未验证**. | WGC, then PrintWindow. Occluded windows and 150% DPI **未验证**. | XComposite on Xvfb passed. No composite and covered: refused. | Portal ScreenCast. Real portal **未验证**. |
| Background actions | Partial. AppKit can receive a background click on macOS 27. Chromium does not ([ADR 0006](docs/adr/0006-macos-tcc-helper.md)). | Partial. UIA Invoke can activate the window. A native button uses a non-activating click. | AT-SPI where the interface exists. XTEST is not used when Stop cannot be grabbed. | Portal input only with a reliable Stop. No full-desktop fallback. **未验证** live. |
| Foreground typing and pointing | Policy-gated. Physical idle detection **未验证**. | Unavailable on the hosted runner: synthetic mouse never reached the hook. Physical mouse **未验证**. | XTEST on Xvfb when the hotkey grab works. Physical devices **未验证**. | **未验证**. |
| Stop hotkey | See the Stop section. | See the Stop section. | Xvfb test passed. | Not claimed. |
| Overlay excluded from capture | Implemented in `gcu-overlay`, not in this binary. The live macOS contract skipped this case. Real GUI **未验证**. | Implemented, not in this binary. Real GUI **未验证**. | The X11 driver hides overlay windows during a capture lease. Real GUI **未验证**. | Declared unavailable. |

More detail: [macOS driver notes](crates/gcu-driver-macos/README.md), [Windows driver notes](crates/gcu-driver-windows/README.md), [Windows runner diagnostics](crates/gcu-driver-windows/diagnostics.md), [overlay notes](crates/gcu-overlay/README.md).

## Privacy

`get_app_state.include_screenshot` accepts `true`, `false`, or `"auto"` (see
[ADR 0011](docs/adr/0011-auto-screenshot-policy.md)). The default remains `true`;
existing boolean calls keep their behavior and serialized shape. Auto evaluates
usable interactive nodes, controls inside popups, and the raw canvas role share.
`gcu_core::observe::QualityThresholds.min_interactive_nodes` and
`canvas_role_ratio` are **待评测校准**: both default to `None`, with no invented
production cutoff. Evaluation can inject boundaries through
`ActionRouter::with_auto_capture_policy`; these are not new `config.toml` keys.
Without an interactive-node boundary, auto conservatively keeps the image.
After calibration, sufficient trees may omit it; canvas/sparse trees and an empty
rendered diff with changed safe pixels still request an image. Action results
also attach for coordinate actions or an explicit image request.

Auto still captures and redacts before deciding delivery, so it can detect pixel
changes absent from the text tree. Omitted captures never replace the last image
delivered to the model. Incomplete native trees, unknown sensitive regions, and
tree/capture or geometry mismatches retain the existing refusal path. This does
not claim a capture latency improvement. The default may change only after an
evaluation on the same tasks proves that success rate does not decrease.

Screenshots are redacted before they are shown to a model, a dialog, or `gcu call`. Sensitive regions are painted opaque gray and expanded by 4 pixels. If a sensitive region cannot be located, or the tree is incomplete, no image is returned. The text tree can still be returned. The unredacted frame is not written out for the model.

`gcu call` without `--json` copies those safe images into a private `gcu-call-*` directory under `$XDG_RUNTIME_DIR` or the temp directory. The directory is mode `0700` and the files are mode `0600` (an owner-only ACL on Windows). The next `gcu` process deletes directories older than one hour. You can delete them sooner. `--json` prints the MCP result. Files owned by an MCP session, including overflow text and screenshots, are removed when the connection closes.

Logs go to standard error. There is no log file. `doctor` keeps at most 20 metadata rows from the last completed process: tool name, outcome, duration, and timestamp. No typed text, target labels, or image bytes. Rows older than seven days are dropped. The path is `recent-calls.json` in the state directory.

Session secrets are not placed in the environment or in `argv`. Do not scrape child environments. A panic prints `Internal panic; operation was not replayed` and does not include the payload.

## Troubleshooting

| What you see | What it means |
|---|---|
| `native-x11 is not compiled into this build` (or `native-windows` / `native-wayland`) | Omitting `--driver` selects `auto`. `auto` and `native` exit 2 when that route is not compiled. They do not fall back to fake. `--driver fake` is the explicit offline driver. A release binary also needs `GCU_ALLOW_FAKE=1`. `--scene` is valid only with `--driver fake`. |
| `Fake driver requires a debug build or GCU_ALLOW_FAKE=1` | The binary is a release build. |
| `GCU_APPROVAL is forbidden in release or non eval-autoapprove builds` | Unset `GCU_APPROVAL`. Release builds have no auto-approve switch. |
| Gatekeeper says the app is damaged or from an unidentified developer | Ad-hoc build. Use the unsigned-package steps. Do not disable Gatekeeper. |
| SmartScreen says unknown publisher | Unsigned Windows build. More info → Run anyway, after the checksum matches. |
| `permission_missing` | The helper is missing Accessibility or Screen Recording, or the Linux session is missing AT-SPI / a portal grant. |
| Screen Recording still fails after you allow it | Restart the helper. macOS applies that grant only to processes started afterward. Accessibility applies immediately. |
| The first look at an egui / AccessKit window is a short tree, and there is no screenshot | The tree is published lazily and marked incomplete. On the macOS 27 Mini the fixture grew from 7 nodes to 54 after about 588 ms. An incomplete tree does not produce an image. A later read can be complete. The same lazy first read was seen on Windows UIA. |
| `SetValue` returns not settable | AccessKit text fields on Linux do not implement editable text, and the egui field on macOS can report success while the text is unchanged. The driver reads the value back. Use `type_text`. On Linux that needs a foreground policy. Real GTK and Qt apps are **未验证**. |
| `elevation_required` | The Windows target is higher integrity than `gcu`, or it is UIAccess. `gcu` will not elevate. Run the target unelevated, or do the step yourself. |
| Foreground actions unavailable on a Windows runner | The hosted runner's restricted token never delivered a synthetic mouse to the low-level hook. That is an environment result, not a promise about a PC with a physical mouse. |
| Chromium ignores a background click on macOS | Measured on macOS 27. The driver does not switch to a foreground click to hide that. |
| `Computer-use helper rejected the connection` | The CLI and the helper are not the same signed bytes, or the socket is not the one this user owns. Register the app that contains this `gcu`. |
| `Launch Services did not start the computer-use helper` | `open -Ra` the absolute `.app` path first. `open -b` cannot start an app Launch Services does not know. |
| `The computer-use helper runs only on macOS` | `gcu agent` was started on another system. |
| Approvals file cannot be read | The file is missing its owner-only mode, is a symlink, or is corrupt. Fix or delete that one file. Do not chmod your home directory. |
| The model keeps receiving `stopped_by_user` | Stop is still latched. `gcu turn-ended` will not clear it. Resume from a TTY with `gcu resume`. A real macOS helper resume is **未验证**. |
| `gcu install` says it will edit `~/.grok` | It will. A project-scoped Grok install still writes the Stop hook there. Pass `--yes` only when you mean that. |

## Uninstall

1. Remove the client entry while `gcu` is still on `PATH`. Use the same client and scope you installed. Add `--yes` when the Grok directory would be edited and stdin is not a terminal.

   ```sh
   gcu uninstall grok
   gcu uninstall codex --scope project
   ```

   This deletes the `grok-computer-use` entry and hooks this installer owns. It leaves `*.gcu-backup-*` files in place.

2. Revoke persistent grants, or delete `approvals.json` after the binary is gone:

   ```sh
   gcu approvals list
   gcu approvals revoke KEY
   ```

3. Remove the package or the extracted directory:

   ```sh
   npm uninstall -g gcu
   brew uninstall gcu
   winget uninstall RongleCat.GrokComputerUse
   sudo apt remove grok-computer-use
   sudo dnf remove grok-computer-use
   ```

4. Remove operating-system grants yourself. `gcu` does not revoke them.

   - macOS: System Settings → Privacy & Security → Accessibility and Screen Recording. Remove `Grok Computer Use` or `Grok Computer Use (Dev)`.
   - Linux portal: remove the ScreenCast / RemoteDesktop permission in the desktop's settings. A Wayland driver that stores restore tokens uses `${XDG_DATA_HOME:-~/.local/share}/grok-computer-use/wayland-restore` (directory mode `0700`, files mode `0600`). This `gcu` binary does not link the Linux driver, so it does not create that directory.

5. Leftover files, if you want them gone: `config.toml`, `approvals.json`, `stopped`, `recent-calls.json`, the Wayland `wayland-restore` directory above, and the `gcu-call-*` / `gcu-control-*` directories. Paths are in Approvals and in Stop and resume.

## Commands

Global flags, accepted by the subcommands that clap parses: `--driver auto|native|fake` (default `auto`), `--scene <name>` (only with `fake`; omitted fake uses `form`).

```sh
gcu version
gcu doctor --driver fake --json
gcu call list_apps --driver fake --scene chat
gcu call list_apps --driver fake --args '{}' --json
gcu mcp --driver fake --scene form --profile minimal --transport stdio
gcu mcp --transport http --token-fd <fd> --port <port> --profile default
gcu mcp --connect <url> --token-fd <fd>
gcu install grok --scope user --dry-run --json
gcu uninstall vscode --scope project
gcu approvals list --json
gcu approvals revoke KEY
gcu resume --session <gcu-control-directory>
gcu turn-ended --help
```

`gcu mcp` defaults are `--transport stdio`, `--profile default` (`default`, `minimal`, or `full`), and `--turn-idle-secs 45` (at least 1). HTTP requires `--token-fd`. `--port` defaults to an ephemeral port. `--connect` is a stdio proxy and also needs `--token-fd`. [docs/cli.md](docs/cli.md) still says HTTP is unimplemented. The binary accepts the flags above.

`gcu turn-ended` is handled before ordinary parsing. It reads Grok Stop JSON on stdin, or one Codex notify JSON argument. `--session` selects one control directory. `--previous-notify` runs the previous notify command first and keeps its exit status. Other errors print a redacted line and exit 0.

`gcu engine` is in `--help`. It needs `--host-fd` or `--host-socket` plus `--host-token-fd`, and `--stop-fd`. Those channels are supplied by the App host.

Exit codes: 0 success; 2 bad arguments, a refused confirmation, `ambiguous_app`, `invalid_key`, or `out_of_bounds`; 3 `permission_missing`; 4 any other tool error, including `approval_denied`, `elevation_required`, and `stopped_by_user`; 10 an internal failure, a panic, or an install IO / `agy` failure.

## For developers

This directory is its own Cargo workspace, pinned to Rust 1.99.0. Among these crates, the desktop App may depend only on `gcu-protocol` and `gcu-host-protocol`. From this directory:

```sh
cargo fmt --all --check
bash scripts/verify.sh
CROSS=1 bash scripts/verify.sh
cargo xtask check-deps
cargo xtask golden --check
cargo xtask budget
cargo deny check
```

`verify.sh` runs fmt, clippy, test, check-deps, golden, budget, and deny, and stops at the first failure. On a small machine, compile with `rcargo` so the build runs on the shared server. `cargo xtask golden --check` compares protocol JSON with `crates/gcu-protocol/golden/`. `cargo xtask budget` enforces the 14,000-byte tool-schema ceiling. The license allowlist rejects GPL, AGPL, and LGPL. WebKitGTK, GTK, libpipewire, and libxkbcommon are system libraries and are outside that graph.

Layout: `crates/` (protocol, host API, host protocol, core, drivers, fake driver, MCP, overlay, CLI), `apps/fixture`, `xtask`, `packages`, `docs/adr`, `ui/locales`. Progress for the plan lives in [STATUS.md](../docs/plans/computer-use-v2/STATUS.md). Disposable install checks are described in [scripts/install-smoke/README.md](scripts/install-smoke/README.md).

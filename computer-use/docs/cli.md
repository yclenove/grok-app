# Fake CLI

The CLI currently assembles only the offline fake driver. Omitting `--driver`
returns exit 2 and identifies real driver assembly as P4. A release binary also
requires `GCU_ALLOW_FAKE=1` for fake use. No operating-system input is delivered.

```sh
gcu version
gcu doctor --driver fake --json
gcu call list_apps --driver fake --json
gcu mcp --driver fake --scene form --profile minimal --transport stdio
```

`mcp` accepts default/minimal/full profiles. HTTP, `--connect`, `--token-fd` and
`--port` are parsed but return an explicit P0-12 not-implemented error (exit 2).

## install / uninstall

`gcu install` and `gcu uninstall` register or remove the `grok-computer-use` MCP
server. They do not start a driver. The client is positional or `--client`:

```sh
gcu install grok --dry-run
gcu install --client codex --scope user --json
gcu uninstall --client claude --yes
```

Clients: `grok`, `codex`, `claude`, `antigravity`, `gemini`, `opencode`, `cursor`,
`vscode`. `--scope` is `user` (default) or `project`. `--dry-run` prints the diff
and writes nothing. `--json` prints one object (`client`, `action`, `scope`,
`dry_run`, `noop`, `backups`, `diff`) instead of the text diff.

Repeating install does not duplicate an entry. Uninstall removes only
`grok-computer-use` and hooks this installer owns. Existing files are copied to
`<name>.gcu-backup-<unix-time>` beside the original before a real change. Backup
files are owner-only (`0600` on Unix, an owner-only ACL on Windows). Replacing a
config keeps that file's previous mode. A failed write removes its temporary file.

Paths: `--home` overrides `GCU_HOME`, which overrides `HOME` (or `USERPROFILE` on
Windows). `--project` overrides `GCU_PROJECT`, which overrides the working
directory. `--grok-home` is the Grok directory when it is not `<home>/.grok`.
`--gcu` is the absolute executable written into the client config. Antigravity
user scope runs `agy mcp add` / `agy mcp remove` (`--agy` overrides `PATH`).

Writing the Grok directory asks on a terminal (`y` or `yes`). Anything else
cancels. A non-terminal needs `--yes`. A pipe and environment variables such as
`GCU_YES` are not confirmation. The Stop hook is `computer-use-turn-ended.sh`
under `<home>/.grok/hooks` on Unix, and `computer-use-turn-ended.cmd` on Windows
(both follow `--grok-home` when that flag is set). It calls `gcu turn-ended` only
when the payload reason is `end_turn`.

Exit 2 covers a missing client, a refused confirmation, and a config file this
command will not parse. Exit 10 covers an IO or `agy` failure. A refused command
leaves the config unchanged.

App observations and actions ask this trusted Host. When the client advertises
form elicitation, the Host tries that inside `tools/call` first. An `always`
answer from elicitation is not stored; only this Host writes `approvals.json`
(mode 0600) after the user picks always. This build has no WebKit view. A desktop
falls back to zenity, then kdialog, on Linux; macOS uses `osascript`; Windows uses
a fixed PowerShell prompt. CI, a missing display, and `GCU_ALLOW_FAKE` deny
instead of opening a window.

`gcu approvals list` prints persistent grants. `gcu approvals list --json` prints
the stored records. `gcu approvals revoke KEY` removes the key printed by list.
There is no command that grants every app.

Offline evaluation may use a debug build with `--features eval-autoapprove` and
`GCU_APPROVAL=auto-allow-turn|auto-allow-session|auto-allow-always|auto-deny`.
`auto-allow-always` stays inside the session and does not write `approvals.json`.
All release builds and builds without that feature reject any `GCU_APPROVAL`
value with exit 2.

`call` executes the existing MCP dispatcher in process. Actions first obtain an
authorized fresh observation to establish indices and screenshot geometry.
It does not retry any action. Human output includes text and safe image files
in a 0700 session directory, with 0600 files (owner-only ACLs on Windows).
These inspection files remain after exit; the next CLI invocation removes
leases older than one hour, and the user can delete their directory sooner.
`--json` emits the original MCP CallToolResult, including image blocks; MCP-owned
overflow and screenshot paths are removed on connection close.

`doctor` labels fake permissions/private APIs as not applicable. It reports the
driver capabilities, CLI version, config path, stderr logging (no log file) and
up to 20 metadata-only action summaries saved by the last completed process.
The private summary file retains at most seven days of metadata; doctor reports
its path and scope, including an empty history before the first action.
No text arguments, target labels or image payloads enter the summaries/logs.

The CLI reads only `[observe]`, `[policy]`, `[guard]` from the platform
`grok-computer-use/config.toml` location. Missing files use defaults; invalid
types/keys/budgets return exit 2. Policy is in memory and never rewrites config.

Stop control runs outside the action/approval queues. SIGINT/SIGTERM and the
private control channel latch Stop and release input. The user-state marker
survives process restart. To explicitly resume, run `gcu resume` in a terminal,
or select the private startup directory with `--session`. Both input and output
must be TTYs; manually enter `yes <challenge>`. Credentials rotate after use.
This is **best effort**: a model with shell access under the same UID can forge
a TTY and read the token. Trusted recovery belongs to the overlay button,
App panel or hotkey in P4-04/P5; see [ADR 0003](adr/0003-resume-trust.md).

Exit codes: 0 success; 2 arguments/ambiguous_app/invalid_key/out_of_bounds;
3 permission_missing; 4 other business errors; 10 internal/panic/unknown failure.

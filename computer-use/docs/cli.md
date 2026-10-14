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
Install/uninstall command wiring belongs to P4-05; the existing install library
is unchanged.

App observations/actions are denied with a P4 approval-window message until the
real approval UI lands; MCP client elicitation cannot override this CLI Host.
Offline evaluation may use a debug build with `--features eval-autoapprove` and
`GCU_APPROVAL=auto-allow-turn|auto-allow-session|auto-allow-always|auto-deny`.
The minimal in-memory Host bounds auto-allow-always to the session; it creates no
persistent grant. All release builds and builds without that feature reject any
`GCU_APPROVAL` value with exit 2.

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

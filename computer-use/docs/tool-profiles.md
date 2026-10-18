# MCP tool profiles

Profiles follow [architecture §11.1](../../docs/plans/computer-use-v2/03-architecture.md#111-工具目录16-个默认面--1-个条件注册p0p6-逐步上线).
They only filter executable tools; they do not grant authorization, change
parameters, or bypass the ActionRouter, approvals, Stop, or native-call limits.

| Profile | Tools in protocol order |
| --- | --- |
| `minimal` (9) | `list_apps`, `list_windows`, `get_app_state`, `click`, `perform_secondary_action`, `set_value`, `type_text`, `press_key`, `scroll` |
| `default` (15) | `list_apps`, `list_windows`, `launch_app`, `get_app_state`, `click`, `perform_secondary_action`, `set_value`, `select_text`, `type_text`, `press_key`, `scroll`, `drag`, `wait_for`, `computer_batch`, `capabilities` |
| `full` (15 or 16) | `default`, plus `computer_script` before `capabilities` only on Unix with feature `script` **and** an assembled runner |

The nine minimal tools are the architecture's explicit set, not a new selection:
discovery finds a running app/window, observation supplies current indices and
safe screenshots, and the six actions complete the observe/operate loop.
`perform_secondary_action` preserves advertised actions such as opening menus
and expanding controls. `launch_app` is intentionally outside this fixed set;
minimal operates existing windows. Text selection, dragging, capability queries,
waiting, batching, and scripting remain on the larger surfaces. An allowlist
keeps future implementations from silently expanding minimal or default.

`browser_navigate` is unimplemented and absent from **every** current profile,
regardless of visible window titles or web areas. Once implemented under
[ADR 0010](adr/0010-builtin-browser-window-protocol.md#3-精确的最小-host-扩展),
it is conditional on the built-in browser being open with a verified, live
binding owned by this local session, negotiated Host support, the runtime
switch, and validated platform support. Closing/revoking it must unregister
the tool and reject cached calls. Minimal stays fixed at nine; ADR 0010 permits
conditional exposure on default/full only after that separate implementation.
No browser-window or Host protocol implementation is delivered by this catalog.

Instructions use the actual registered catalog to mention waiting, batching,
and scripting. Absent tools are never recommended. Script instructions identify
the full/feature/Unix/runner gates and the existing
[`cu.d.ts`](../crates/gcu-script/cu.d.ts) API declaration. L0 checks cover gcu's
own inputs; they do not promise to block an external browser's redirects.
The catalog does not promise built-in browsing, DOM connectivity, or native
Wayland support. Driver selection/reporting belongs to
[ADR 0012's assembly work](adr/0012-real-driver-assembly.md).

## Golden files and byte gate

The pure projection in `gcu-mcp/src/catalog/profiles.rs` is shared by serving
code and xtask. `gcu-protocol/golden/profiles/{minimal,default,full}.json` stores
the complete tool objects (descriptions, annotations, and input schemas).
The full golden records the maximal implemented configuration: Unix, script
feature enabled, runner assembled, no built-in browser executor. This is
independent of the platform/features compiling xtask. Live full catalogs omit
the script when any gate is missing; MCP integration tests compare that
filtered golden with the actual `tools/list` response in both feature builds.

All three compact UTF-8 tool arrays must fit the same **14,000-byte** ceiling.
`tools.json` and tool descriptions remain unchanged: no evaluation evidence
supports rewriting them. Full already fits, so no description compression is
needed. Pretty-printed golden file sizes are not the wire budget.

From `computer-use/`, on the authorized build machine:

```sh
rcargo run -q -p xtask -- golden --update
rcargo run -q -p xtask -- golden --check
rcargo run -q -p xtask -- budget
rcargo test -p gcu-mcp --test catalog
rcargo test -p gcu-mcp --features script --test catalog
rcargo test -p gcu-mcp --test mcp_e2e
rcargo test -p xtask
```

`--check` fails on missing or changed profile files without rewriting them.
The feature matrix also asserts that an injected runner cannot enable scripts
in minimal/default, a build without the feature, or a non-Unix build. A fake
window called `cu-browser` cannot register unimplemented navigation.

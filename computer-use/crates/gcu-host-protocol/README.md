# gcu-host-protocol v1.0

External Hosts depend on this crate and `gcu-protocol` only. The engine's
`gcu-mcp::host::RemoteHost` implements the in-process traits; this crate does not
link `gcu-host-api`, core, MCP, drivers, or Tauri. `golden/messages.json` and `golden/auth.json` contain
the generated schemas for every parameter/result and the JSON-RPC envelope.
`cargo run -p xtask -- golden --check` compares them in the existing CI gate.

## Launch and authentication

Prefer App-created private duplex connections, inherited by precisely its child:

```text
gcu engine --host-fd <descriptor> --stop-fd <descriptor> --driver fake
```

Unix uses connected socketpairs. Windows uses an already connected overlapped
local pipe client handle; its name is never given to the child. The launcher
must close its copies of child endpoints immediately after spawn and prevent
other descriptors/handles from leaking into children. Fake has the P0-10 debug
or `GCU_ALLOW_FAKE=1` gate; it never substitutes for a real platform driver.
Importing a connection/token fd marks both the inherited original and its owned
duplicate as non-inheritable, so later engine subprocesses cannot receive them.

The Host generates a new 32-byte `Secret` for each launch. Before RPC handshake,
it writes one length-prefixed `auth::Bootstrap` on the private inherited Host
connection. The bootstrap carries `host: {pid, started}`, `token: [32 bytes]`,
and `endpoint: null`. This secret is a capability of the inherited connection;
it is never sent over a publicly named endpoint. On Unix the child independently
checks its parent PID/start time. HostServer receives the actual spawned child's
PID/start time from the launcher, never from an untrusted greeting.

Fallback launch still requires an independent inherited Stop connection:

```text
gcu engine --host-socket <path> --host-token-fd <descriptor> --stop-fd <descriptor> --driver fake
```

`PrivateListener::bind` refuses an existing unknown endpoint, including a socket;
it never unlinks/reclaims one. Unix requires an existing current-user 0700
parent, creates a 0600 socket, and pins both directory and socket device/inode.
The launcher sends `Bootstrap::with_endpoint(listener.stamp())` through the
secret fd. Both connect/accept recheck inode/owner/mode before and after connection;
kernel peer PID (where supplied by the OS), UID and live creation time are checked.
Windows interprocess listeners set `FILE_FLAG_FIRST_PIPE_INSTANCE`, default to
`PIPE_REJECT_REMOTE_CLIENTS`, and install a protected owner-only DACL. They do
not enable overwrite. PID/creation-time and challenge proofs remain mandatory.

Both streams run the same mutual handshake. Each side sends `{host_protocol:
{major,minor}, instance:{pid,started}, role, channel, nonce:[32 random bytes],
engine_version, capabilities:(driver capabilities or null for Host), stopped}` and then `{proof:[32 bytes]}`.
The HMAC-SHA256 transcript is JSON serialization of
`["gcu-host-v1/mutual-proof", hostHello, engineHello, provingRole]`, in that order.
It binds both fresh nonces, versions, instances, roles, channel and Host Stop.
Stop derives its own key as HMAC-SHA256(token, `gcu-host-v1/stop-key`). Reflection,
wrong token/instance/channel, reused transcript, and major mismatch fail closed.
A compatible minor negotiates the lesser value. Unknown fields in this version's strict types are refused. Future minor
extensions need explicit schemas/defaults, not arbitrary unknown-field acceptance.
Each launch secret is consumed once: never reconnect/retry a failed launch with it.

Only launch/authentication material uses these greeting frames; it is not a
callable RPC method. Authentication lasts at most five seconds. Any malformed,
truncated, oversized, replayed or invalid frame closes the connection without
executing an operation. Same UID/file access alone never proves identity.
Same-user process-memory access or modification of trusted App/engine binaries
is outside this boundary. The App's product/build-signature verification is the
P5 launcher responsibility; arbitrary external engines cannot inherit production
trust merely by completing this protocol.

## Framing, deadlines and errors

Each frame is a four-byte **big-endian** unsigned byte length followed by one
UTF-8 JSON value. Length must be 1 through 16,777,216 inclusive, checked before
allocation. There is no NDJSON, Content-Length guessing or JSON-RPC batching on
these connections. RPC uses `jsonrpc: "2.0"`; IDs are positive monotonic `u64`s
per direction. Notifications omit ID. Success includes `result`, including null;
failure includes `error:{code,message}`. Unknown envelope/parameter/result fields
are refused. Duplicate/nonmonotonic incoming IDs close the connection; consumed,
cancelled, timed-out and late response IDs cannot approve another request.

At most 32 queued frames, pending requests and concurrent Host callbacks are
admitted. Async writes await transport backpressure, with a five-second write
ceiling; full callback/request queues reject immediately. Dropping a pending
call removes its waiter; a full response queue closes rather than leaving an
approval waiting forever. Notifications may be dropped when full. Stop has its
own transport/tasks and never shares approval/action queues.

Ordinary RPC has a five-second deadline; approval callbacks default to 120
seconds (RemoteHost caps the configured policy timeout at 120). Handoff dismissal
also has a 120-second transport ceiling and never grants authority. Heartbeats
arrive every ten seconds. Native operations use the core's five-second ceiling;
Stop latches within 100ms and issues input release within 500ms. Host owns its
persistent latch across engine restarts and supplies it in both handshakes. If
any per-session latch is retained at restart, Host should start globally stopped.
App manages the architecture's 60-second idle engine shutdown.

Errors: `-32601` method/direction/channel unavailable, `-32602` invalid parameters,
`-32603` internal serialization, `-32000` handoff required/disconnected,
`-32001` approval timeout, `-32002` busy, `-32003` untrusted/replayed resume,
`-32004` unknown session or unowned image, `-32005` nonlocal session.
Diagnostic errors omit secrets/payloads. Disconnect denies every pending approval,
refuses new calls, preserves Stop, revokes session endpoints and exits the engine.
`HostCallbacks::disconnected` must dismiss UI and preserve the App's stored latch.

## Method fields

All exact names are in `messages::METHODS`; schemas also define nested fields.

| Direction | Method | Parameters → result |
|---|---|---|
| Engine → Host request | `host.approval.requestApp` | session_id, verified app (identity/name/pid/icon), model reason?, risk → decision: turn/session/always/deny |
| Engine → Host request | `host.approval.confirmAction` | session_id, app, summary(text/snapshot_id/action_digest), redacted crop? → confirmed |
| Engine → Host request | `host.approval.handoff` | session_id, app, engine reason → {} |
| Engine → Host request | `host.policy.load` | {} → version, policy knobs, Host-persisted always identities |
| Engine → Host notification | `host.overlay.show` | session_id, window_id/title/frame/scale, physical screen x/y, cursor kind |
| Engine → Host notification | `host.overlay.status` | session_id, message, foreground |
| Engine → Host notification | `host.overlay.hide` | session_id |
| Engine → Host notification | `host.event.emit` | session_id, tool, target, outcome, duration_ms, redacted thumbnail? |
| Engine → Host notification | `host.engine.health` | uptime_ms, sessions |
| Host → Engine | `engine.session.open` | session_id, kind(local/ssh/remote_im/automation) → mcp_url, token; only local accepted |
| Host → Engine | `engine.session.turnEnded` | session_id → {}; clears turn grants, releases input, forces fresh state, hides overlay |
| Host → Engine | `engine.session.close` | session_id → {}; cancels approvals, revokes endpoint/token/resources |
| Host → Engine **Stop channel only** | `engine.session.stop` | session_id? → {}; absent means all, including future sessions |
| Host → Engine **Stop channel only** | `engine.session.resume` | session_id?, increasing sequence, fresh nonzero user_event[32] → {} |
| Host → Engine | `engine.policy.changed` | increasing version → {}; await read-only reload before replying |
| Host → Engine | `engine.system.doctor` | {} → capabilities, ready, issues (existing core doctor projection) |
| Host → Engine | `engine.screenshot.get` | session_id, id → mime, width, height, redacted bytes[]; session ownership required |
| Host → Engine | `engine.system.shutdown` | {} → {}; close engine gracefully |

There is **no `host.policy.grantAlways`**. Host persists Always only while consuming
its actual UI decision once, bound to RPC ID/session/App identity/engine instance/
policy revision, then sends `engine.policy.changed`. Engine requests alone cannot
write policy. `RemoteHost` never persists grants or changes policy via a tool.
Resume is an authenticated trusted user gesture, never an MCP tool. Ordinary RPC,
old sequence/event, unauthenticated streams and model tools cannot clear Stop.
App invalidates its local pending approval registry on Stop/turnEnded/close before
sending the lifecycle message, so a late click cannot persist an obsolete grant.
Only App's trusted UI may create `user_event`; do not bridge a model-controlled
IPC or CLI directly to the stop connection (ADR 0003 explains TTY limits).

## MCP endpoints and proxy

Each App session gets its own `127.0.0.1:<random>/mcp` endpoint and independent
32-byte lowercase-hex bearer token. Only `Authorization: Bearer <token>` works,
including initialization. Host must occur exactly once and exactly equal
`127.0.0.1:<bound port>`. If Origin is present it must occur once and exactly equal
`http://127.0.0.1:<bound port>`. No wildcard, hostname/DNS, alternate port, null,
comma list, evil Origin or duplicate Host is allowed. This uses rmcp's streamable
HTTP server and the same dispatcher/router/results as stdio. Session close or
App disconnect revokes the endpoint and credentials; old tokens cannot reopen it.

`gcu mcp --connect <exact-url> --token-fd <fd>` bridges NDJSON stdio to the endpoint.
The fd supplies exactly 32 raw token bytes (decode the open result's hex first).
Proxy disables redirects, environmental proxies, nonliteral loopback destinations
and automatic session recovery/replay. It never reads a token from argv/env/logs.
`gcu mcp --transport http --driver fake --token-fd <fd> [--port <port>]` uses a
launcher-supplied 32-byte token and prints only `{mcp_url}` to stdout. Standalone
approval behavior is the same as P0-10; it cannot obtain App approvals.

## 协议边界

物理输入活动不在 Host 协议内。macOS listen-only event tap、Windows 低级钩子和
X11 XInput2 是引擎内平台驱动守卫（P1-08/P2-07/P3-04）的职责，App 无法也不应
提供物理活动。RemoteHost 组合可注入的 `PhysicalActivity`，默认返回 None；Stop
镜像来自 Host 的独立可信控制通道。没有物理信号时按 03 §7.7 放行 user_active
守卫，doctor 的 capabilities.user_activity_guard=false 且 issues 明确报告
`user_activity_guard: unavailable`。默认 foreground 仍为 ask/never，不会因降级放行。

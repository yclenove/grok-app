# 0005 Windows native worker frames and lifetime

状态：接受
日期：2026-10-13

## 背景（事实 + 证据链接）

[架构 §5.2、§6、§7.7](../../../docs/plans/computer-use-v2/03-architecture.md)
requires killable native execution, uncertain mutation outcomes and independent
Stop delivery. P2-04 names bincode, but the reviewed dependency baseline already
includes serde_json and the Host transport uses bounded length-prefixed JSON.
`cargo deny --locked check` passed before adding dependencies; this change adds
no external codec or dependency version, only existing thiserror/tokio facilities
and the CLI-to-Windows-driver workspace edge.

## 决定

Use private inherited stdin/stdout, a four-byte big-endian length prefix and
serde_json. Reject empty, truncated, malformed, uncorrelated and >32 MiB frames.
Flatten trees into parent-indexed nodes to support native depth 64 without
raising serde_json's recursion limit. Bound outbound serialization allocation;
move parent-side JSON CPU work off the async watchdog thread. Version and epoch
bind every response to the current process and request. No public socket or
secret environment variable is introduced.

`ExecMode::Worker` is the default; explicit `InProcess` retains the native adapter
for diagnostics. The child executes discovery, UIA, capture and every resolved
action. Element references contain actual cached UIA RuntimeId + HWND, stamped
window/app provenance, snapshot generation, slot and process epoch. The HWND
generation-property namespace is bound to the Windows parent's PID + OS creation
time + supervisor counter, sent over the private startup frame and stable across
worker restarts; it must not use the child's changing PID. Worker-local
handle maps prevent cross-process COM pointers. Rereading for core screenshot/
action guards resolves only the same RuntimeId/HWND with an unchanged fingerprint
to the newest native slot; changed identity or process epoch is stale.

A ten-second per-attempt watchdog kills/reaps a failed worker. The Driver adapter
keeps the existing shorter caller deadline. Only read-only requests retry once;
launch, restoring a key window and all actions never replay. Mutation transport
loss preserves `maybe_executed=true`, which core maps to `unknown`. A fresh process is started after returning uncertainty; read retries restart
immediately. Failed bootstrap is retried by the next call. Restart re-resolves
exact app provenance before using retained window stamps. Stop and shutdown use
independent signals, invalidate queued calls and interrupt startup/IO/FFI waits.
Windows attaches a noninherited `KILL_ON_JOB_CLOSE` Job Object before sending the
first frame; the child does no native work until that frame. Rust Command limits
inheritance to its explicit standard handles. Parent death closes the Job; before
assignment, pipe EOF terminates the idle child. Unix uses ordinary kill-on-drop
children for portable tests. Events are honestly unavailable across worker RPC;
core's polling fallback applies. Input-ledger releases remain the independent
parent-side responsibility of P2-06, which first enables synthetic input.

## 备选与为什么不选

- bincode: an additional, unreviewed codec/version is unnecessary for this private
  protocol; do not add or exempt a dependency merely to match the task wording.
- postcard: also adds a codec; binary size/performance gains are unproven here.
- Thread-only timeouts: abandon the caller while provider FFI continues running.

## 影响（对接口、测试、用户）

No architecture Driver or public model protocol changes. Existing raw capture
redaction and security pre/postflight remain inside the native adapter/core.
P2-05 can implement native `perform` without changing the worker action transport.
Fault executables require `test-worker-faults` and debug assertions; release builds
contain no fault injection. Portable process tests cover failures, retry limits,
Stop during restart, abandoned calls, stale handles and process reaping. Native
Job/desktop behavior and ignored nightly soak require their matching runner.

## 退出条件

Revisit the codec only if measured IPC/encoding costs miss P9 budgets; preserve
bounded framing, certainty, exact identities and independent Stop in any replacement.

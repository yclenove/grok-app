# U03 checkpoint: exact task controls and shared action controller

Status: **foundation implemented; product card/panel wiring still open**.
This is the historical foundation checkpoint. Product wiring and its retained
pending assertion are now verified in the [UI checkpoint](2026-10-03-computer-use-task-consent-u03-ui-checkpoint.md);
the original logs, failure and source/artifact identities below are preserved.
Single Codex writer. No Grok dispatch, commit, push, PR, release or real consent.
Full U00–U09 / D00–D31 and Windows/macOS/Linux scope remains active.

## Implementation

- Host Pause and Preview address the original connection/task/request/revision,
  run and canonical decimal generation. Core checks identity under admission and
  publication locks; App pins the original ACP transport across native work.
- Preview requires an existing observe grant, does not enumerate targets or
  alter model snapshot/ref/geometry, and does not charge the model observe budget.
- Task commands require the invoking `main` WebView. Session routing never
  bootstraps a legacy/current run or follows a replacement endpoint.
- Bounded owned control workers retain their original handles through waiter
  cancellation. Automatic reaping and shutdown join retain failures.
- Host execution metadata retains typed business failures through Stop and
  trace eviction. Picker descriptors indicate compiled producers, not readiness
  or authorization; currently only Windows/Desktop has a task picker producer.
- `ComputerTaskActions` implements explicit picker, bounded selection, approval,
  deny/cancel, exact Pause and priority Stop with late-reply isolation. Approval
  receipts never establish grants or a running state. It is not yet mounted.
- The model-tools frontend catalog now includes `computer_request_access`,
  matching the real Core/MCP/golden contract.

## Evidence

Directory: `tools/computer-use-probe/.run/codex-u03-task-ui-20261003/`.
Original failed attempts remain alongside corrected runs.

| Check | Actual result |
| --- | --- |
| Core execution / full | 8 / 962 pass (742 unit + 220 integration), zero ignored |
| Core strict Clippy | exit 0 |
| Default App build | `app-build-6.jsonl`, features `[]`, exit 0 |
| App task view / controls / owned workers | 11 / 3 / 4 pass |
| App CU / ACP / routing filters | 202 / 153 / 38 pass; overlapping, not additive; live handshake skipped |
| App strict Clippy | exit 0 |
| Action controller | 17 pass |
| CU/API/domain frontend | 257 pass in 25 files; pending product UI red explicitly excluded |
| TypeScript | `typecheck-5.txt`, exit 0 |
| Probe strict Clippy | `probe-clippy-2.txt`, correct `src-tauri/cu-probe/Cargo.toml`, exit 0 |
| Real pending component test | `ui-pending-red-1.txt`: 0 pass / 1 fail; not fixed by the foundation |

Lint/UI build/quality passed before the final controller increments and require
final verification after product wiring. No installed App, real OS permission,
real Grok, other-platform or signing evidence is claimed here.

824 source files were frozen before the next UI edits in `source-files-1.sha256`;
manifest SHA256 `b67276243a4610c4edabc0062c4d8c4027a28b533e9d2abda9dfe8b3264b1dd9`.
Default App test executable was independently rehashed as
`c3bb1e05b182cc1de1b04044e9217a30568946eeecd0af135d815fb293df6ff9`
and matches `app-artifact-2.json`. The existing Windows test manifest was embedded
in the test executable only; product linker settings were not changed.

## Next work

Mount one session action controller in the pending card and panel, retire their
duplicate status polling and implicit target enumeration, and turn the retained
pending UI assertion green. Preserve original exact Stop, replacement, native
completion, stale, failure and preview contracts using the task API. Complete
Host presentation facts without native calls in metadata readback. U05 Resume
must exist before exposing Continue. All remaining U04–U09, installed/signing,
real Grok E4 and same-final-candidate 12h gates remain open.

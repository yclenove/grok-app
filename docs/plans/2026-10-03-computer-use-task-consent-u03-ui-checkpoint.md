# U03 checkpoint: task card, shared workspace and retained presentation facts

Status: **product UI wired; synthetic and isolated browser checks passed; full
U03/native delivery and overall CU remain partial — not releasable**.

Codex is the single writer in `feat/computer-use-implementation`, based on
`8d8b71c262ab222748bca32412eea884d4b892e0`. Existing dirty/untracked work is
preserved. No Grok dispatch, subagent, commit, push, PR, release or real consent.
The full U00–U09 / D00–D31 and Windows/macOS/Linux scope remains active.

## Product behavior

- The chat card displays a real Host pending request before any execution exists.
  Card and panel consume the same session source and action transaction; neither
  uses legacy status polling or enumerates targets on open.
- Allow explicitly opens the Host picker. Only its original candidate references
  can be selected. Feature-off first use uses the existing enable-and-authorize
  transaction. An approval receipt does not establish a grant or running state.
- Preparing/approval can be cancelled. Stop has priority over those transactions.
  Double clicks, late replies, original native completion, stale streams and
  replacement sessions retain their tested isolation and exact Stop behavior.
- Preview pins task/request/revision/run/generation/Host instance. Hidden,
  paused, closed, unapproved and stale states do not capture. An outstanding old
  request must settle before another starts; its late frame cannot replace a new
  generation. UI preview does not expose or update model snapshot/ref identity.
- The selected group retains every explicit picker's bounded display name,
  including inactive members. Metadata readback performs no native title,
  liveness or discovery call. Title changes do not change authority equality;
  target/surface/native proof changes still invalidate a preparation.
- An actual admitted native dead-target result records `target_closed` for the
  exact original running generation. It clears fresh-image status. Late closed
  witnesses cannot overwrite a paused/replaced generation. Executor backend
  and typed business failure survive Stop and trace eviction.
- Permissions and technical details are collapsed. Request scope is in the
  panel body; actions and Stop remain in the footer. Existing UI tokens and
  GlassModal are reused. Chromium's disabled-trigger blur required retaining the
  actual initiating button to restore focus after Escape.
- Twenty-five consent keys and the old forced-settings copy were updated in all
  fifteen catalogs. Continue is not exposed before U05 has a real implementation.
  Currently only the Windows/Desktop task picker producer is compiled; other
  producers remain explicitly unavailable.

## Verification and source identity

Logs: `tools/computer-use-probe/.run/codex-u03-task-ui-20261003/`.
All original failed attempts are preserved, including the pending-card red,
Chromium Escape-focus failure, native retained-backend red and scope-name red.

| Check | Final actual result |
| --- | --- |
| Frontend affected CU/domain/API/locales | `frontend-wiring-4.txt`: 27 files, 298 pass; panel 38, live card 15 plus pending-without-run 1 |
| Core full, with test-support, serial | `core-full-2.txt`: 982 pass (746 unit + 16 driver + 220 platform contracts); zero ignored/filtered |
| Core strict Clippy | `core-clippy-2.txt`, exit 0 |
| Default-feature App build | `app-build-7.jsonl`, features `[]`, exit 0 |
| App task view / controls / owned registry | 11 / 3 / 4 pass |
| App CU / ACP / routing filters | 202 / 153 / 38 pass; overlapping, not additive; live handshake excluded |
| App / probe strict Clippy | `app-clippy-3.txt` / `probe-clippy-3.txt`, exit 0 |
| Rust workspace format | `fmt-4.txt`, exit 0 |
| TypeScript / lint / quality / diff | `typecheck-8.txt`, `lint-6.txt`, `quality-3.txt`, `diff-3.txt`, exit 0 |
| Fixture TypeScript | `typecheck-ui-fixture-4.txt`, exit 0 |
| Product UI build | `build-ui-4.txt`, exit 0; existing large-chunk warning retained |
| Final isolated Chromium UI | `ui-acceptance-4.txt`, 37 checks, exit 0 |

The final UI runner uses real product components, Tauri mockIPC/Channel and an
owned Chromium copy with loopback-only fixture traffic. Cases cover 300/320/900
widths, light/dark, en/zh/de/ru, DPR 1/1.25/1.5/2, pending/preparing/running/
paused/closed/stopping/stopped/unknown/expired/unsupported, and keyboard
picker/Escape/focus return/approval/Stop. These are **synthetic decisions, not
actual App/ACP/native authorization or installed-platform evidence**.

Final report and 37 original PNGs:
`tools/computer-use-probe/.run/task-consent-ui-2026-10-03T14-22-01-089Z/`.
The previous run's seven contact sheets were all visually inspected, along with
the full running Chinese screenshot. Thirty-five final PNGs have identical
SHA256 to those reviewed images; the changed unsupported and keyboard PNGs were
then inspected at original resolution. No clipping, transparent modal or hidden
Stop was observed. The unsupported fixture originally represented an approved
request incorrectly; final run corrects it to pending and asserts disabled Allow,
enabled Deny and the explicit unsupported explanation. Older reports are retained.

Before final checks, 2,979 relevant source files were frozen in
`source-files-ui-final.sha256` (SHA256
`1025ca23148cc4fe3b6c5544c3f2d3d71ad68760f5d9b525755e1d5f83202c68`).
The final UI-only fixture/runner correction is recorded separately in
`source-files-ui-final-2.sha256` (SHA256
`11ecce7cb7c7fce8c8e72e868aee2a28f94e924c4b2b009f851ec80c05bba192`).
Comparison found exactly those two changed tool files, with product/Rust source
unchanged. The earlier 824-file foundation manifest is historical evidence only.

Default App test executable: `app-artifact-3.json`, SHA256
`7321dea3384bd22497dc7791985e2f56ac00433637da5c34b626fcd39038b883`.
The existing Windows test manifest was embedded only in that test harness;
product linker settings were not changed. Old artifact records are historical.

## Build cleanup

User preference: finish each batch by cleaning disposable builds, duplicate
browser copies and obsolete packaging directories, after their owners exit.
Keep source, original dirty work, reports/screenshots/failures, needed runtime
seeds and current verified executables. Resolve exact paths first; do not delete
a workspace, entire target tree, another worktree, profile or active owner.

This batch removed 45 old UI `site`/`browser-runtime` leaves, 9,953,372,486 bytes.
All 1,331 preserved evidence files were rehashed unchanged. Runtime logs/stamps
and removed-file hashes remain in `.run/disk-cleanup-20261003-ui/`.
The new task UI runner automatically inventories and removes its two disposable
leaves only after browser/server close. Its final two runs removed 372,637,375 and
372,637,399 bytes; reports, PNGs, runtime logs and artifact hashes remain.
After all Rust checks ended, nine inspected obsolete incremental-cache leaves
were removed, 13,853,691,213 bytes, with a record in `rust-cache-cleanup.json`.
Current compiler caches, executable artifacts, dependencies and immutable seed
remain. These deleted generated files can be rebuilt, rather than restored from
trash. Total disposable bytes removed: **24,552,338,473 (22.87 GiB)**.

## Remaining work

1. U04: all four surface producers and Windows/macOS/Linux native membership,
   whole-group production use, related native windows, cross-surface switching,
   incremental expansion and immediate shrinking. Raw identity-only selection
   has no retained picker title and correctly falls back to a surface label.
2. U05: exact user Resume transaction, original owner cleanup, scope/permissions
   revalidation, fresh model observation and one original-session continuation.
3. U06: complete lifecycle convergence and other resource owners. Native stopped
   and transport/catalog cleanup must remain distinct.
4. Migrate/remove unreferenced legacy card/controller/preview helpers and the old
   broad UI fixture while preserving their meaningful regression contracts.
   The new fixed task request catalog does not use old per-run tool removal as
   a Stop-success condition. A retained closed witness replaces the old UI's
   passive liveness polling, not its no-replacement/no-extra-capture guarantees.
5. U07–U09 and D00–D31: real App/native/OS gestures, supported production platform
   integration, signed installed workflows, real Grok E4, same-final-candidate
   12-hour gates, and the separately reviewed development package. Opening a
   fixture or passing mock tests does not close these gates.

Continue with ready scope/Resume work as a single Codex writer. Do not restart
Grok, automatically approve real permissions, or mark the overall goal complete.

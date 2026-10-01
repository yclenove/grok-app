# Computer Use — fork publication checkpoint

Date: 2026-10-01. Status: **development checkpoint; not releasable**.

## Publication scope

The user requested committing and pushing the accumulated Computer Use work to
the existing fork, without opening a pull request. The destination is
`origin` (`yclenove/grok-app`), branch `feat/computer-use-implementation`.
This is not authorization to merge into upstream/main, publish a release, or claim the
full Computer Use objective complete.

Source upstream is `RongleCat/grok-app`. The fetched upstream main on this date
is `6f931681d90336dd5e55a24e423c134e20f90dbb` (v0.2.38), 156 commits ahead
of the original development base `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`.
Preserve the development checkpoint before integrating that upstream history;
do not use a destructive reset, force push, or a blanket ours/theirs resolution.

The source checkpoint includes native host/runtime implementations, browser and
MCP adapters, platform fixtures and regression tests, workspace and settings UX,
15-locale copy, runtime packaging, patched dependency source with its licenses,
CI definitions, design decisions and historical acceptance reports. Computer
Use remains default-off; fail-closed authorization and no-desktop-fallback rules
must survive source integration.

Excluded from publication: local profiles and credentials, VM images, raw core
dumps, screenshots and execution logs, `.run` evidence, generated runtime seeds,
Rust build outputs, Python bytecode, and live process/lock owner records. They
are retained locally, not deleted. Historical absolute paths in reports identify
the original test environment; they are not portable installation instructions.

## Verified progress and explicit limits

The most recent sealed native checkpoint is
[same-process grant recovery](2026-10-01-computer-use-grant-recovery-checkpoint.md).
Its source freeze and tests predate upstream integration. They must not be
presented as tests of the merged source or as full App acceptance.

Pending-consent GNOME portal investigation subsequently matched the original
core to the exact distribution binary and debug symbols. Evidence supports a
second completion of an already-consumed Start invocation following dialog
cancellation and Request.Close. Request/dialog/session teardown and late ready
callbacks also require review. **No backend repair has been applied or accepted.**
A guarded direct-backend regression driver exists only in local `.run` scratch;
it has not run in the guest and is not product code.

The owned acceptance VM attempt21 was cleanly powered down for this publication:
original QEMU PID126668 exited 0 and its original launcher joined. Its stock
portal executable was unchanged. No user desktop, proxy, or network service was
stopped. Four development packages were added only to that disposable VM.

## Remaining final-delivery gates

- Production App/ACP/MCP authorization, dispatch, stop and explicit recovery.
- Supported Linux compositor integration and the pending-consent crash repair.
- Complete input, IME, clipboard, takeover, lock, focus, topology and owner-loss
  recovery across Windows, macOS arm64/Intel, X11 and supported Wayland systems.
- Installed-App UX, native narrow windows, display scaling and platform permissions.
- Signed install, update, repair, rollback and uninstall on every supported target.
- Real Grok end-to-end acceptance and 12 hours of active soak on one frozen final
  candidate, followed by result review.

These remain mandatory. Source backup and successful push do not complete them.

## Publication verification

Pre-merge and post-merge check results are recorded as they finish below. Tests
not executed for this source are unverified, not implicitly inherited from old
green summaries. No pull request, tag, release or upstream push is part of this
checkpoint.

### Published checkpoints and upstream integration

The following development commits were pushed to the fork feature branch and
read back from its remote ref before the upstream merge:

- `ce7c77e2`: native hosts, session-bound runtime, source-only resources and vendors.
- `64908d2f`: redesigned workspace, interaction controls and localized copy.
- `e35da15e`: packaging, regression/acceptance tools, CI and publication hygiene.
- `70ded136`: accumulated plans, scoped evidence and explicit delivery gaps.

The integration candidate merges upstream `6f931681` without rewriting these
commits. Thirteen conflicted files were resolved by preserving both product
changes and Computer Use boundaries, not by taking either entire tree:

- Keep upstream per-session/provider routing, workspace metadata, account
  extraction, rules/commands/settings composition and 461 command registrations;
  retain all 26 Computer Use commands (487 unique commands in total).
- Retain one WebView navigation callback: CU document fencing and upstream
  Google handoff policy both execute in their required order.
- Revoke and detach CU before retiring a respawn owner; leave failed cleanup
  pending, with the original MCP endpoint indexed. Busy sessions retain their
  stamped agent id until the ongoing turn finishes.
- Compare slot/process/agent/ACP identity across asynchronous cleanup. Perform
  persisted resume compare-and-clear under the owning slot lock, using the
  sessions-index transaction; reject newer persisted resume identities. The
  no-owner path preserves one-map-lock-at-a-time discipline and uses the same
  durable comparison without mutating an in-memory owner.
- Keep pending spawn-flag invalidations separate from ordinary deferred respawn;
  recheck the actual target after journal awaits and reload metadata after flush.
- Mechanically split the CU debug App harness and session-manager CU module into
  files below 1,000 lines. Preserve all 21 session-manager CU tests; do not raise
  the repository's 80-file size budget or change its gate scope.

### Validation of the integration candidate

Local logs are under ignored `.run/publication-20261001/`; they are not committed
as portable acceptance evidence. These checks cover the scope explicitly named:

- Frozen pnpm install and locked/offline Cargo metadata: pass.
- Frontend typecheck, ESLint and production UI build: pass. UI build retains
  existing large-chunk warnings; these are not silently suppressed.
- Full frontend suite at four workers: **691 files / 7,918 tests passed**,
  with the original timeout thresholds. A preceding high-concurrency run had
  two timeouts; both isolated tests and the complete bounded rerun passed.
- Deterministic Node adapters/tooling suite: **276 passed**, none skipped.
- Windows native core library: **590 passed**, none ignored or filtered.
- Windows App `cargo check --locked --offline --all-targets`: pass.
- Windows App strict all-target Clippy after the final source edits: pass.
  An earlier run rejected four unnecessary test clones; those were corrected
  using `std::slice::from_ref`, not lint exemptions.
- Formatting, staged whitespace and final code-quality gates: pass. The count
  of files at least 1,000 lines is 80, with the original limit unchanged.
- Windows App regression **execution is still pending at this checkpoint**.
  Its fresh test executable is being compiled; use the final follow-up entry
  for actual assertion results. The initial launcher attempt failed because
  Windows PowerShell treated native stderr as a terminating error; the runner
  now captures the child process directly. That attempt did not enter tests.

These are development integration checks, not installed-App, real Grok, signed
package, cross-platform or final-soak acceptance. The full objective remains
active and the remaining final-delivery gates above are unchanged.

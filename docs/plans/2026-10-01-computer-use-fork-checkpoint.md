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

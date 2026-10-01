# Computer Use — native module import and immutable UI probe checkpoint

Date: 2026-09-25, local Asia/Shanghai (+08:00); corresponding logs use 2026-09-24 UTC timestamps.
Branch: `feat/computer-use-implementation`. Full goal remains **active / not releasable**.
No commit, push or PR. Preserve the dirty worktree and earlier failed artifacts.

## Runtime verification now loads the module

`runtime_prepare` previously read `package.json` through Node and called that an import pass.
It could accept a correct version string even when the actual entry or a transitive dependency
was absent. The child also had no deadline.

The extracted `runtime_import.rs` now:

- Verifies the pinned Node and Playwright versions, imports the exact package `index.mjs`,
  and checks the Chromium APIs used by the worker (`name`, `launch`,
  `launchPersistentContext`, `connectOverCDP`). It does **not** launch a browser.
- Uses the explicitly resolved packaged Node; clears inherited environment, sets a private
  empty PATH, and retains SystemRoot only when present. No system Node fallback.
- Requires a per-invocation completion receipt. A module that exits with code 0 before the
  checks does not count as a successful import.
- Gives the subprocess a 15-second deadline, terminates/reaps the owned child on timeout,
  and discards child-controlled stdout/stderr rather than accumulating or logging them.
- Checks manifest integrity and unexpected files before executing the import probe.
- Reuses the same bounded check for materialized-package acceptance and the App's isolated
  repair probe. The latter no longer unwraps a Windows-only environment variable or runs an
  unbounded `node --version` command.

The public helper requires callers to verify the pack's integrity/ownership first. It is not
a sandbox for arbitrary module paths, a browser-launch probe, or an installed-App certificate.

## Native-target test fixtures

The deterministic prepare/tamper test now selects the native target's actual source lock and
packaged Node. Windows uses its ZIP; macOS/Linux use the Node tar layout. Chromium fixture
paths follow the selected target. macOS fixtures include all five exact pinned framework links
and real destinations; no broad symlink exception or platform skip was added.

A separate archive-inspection test checks all four target layouts on this Windows host. That
proves fixture structure, **not** macOS/Linux execution. Native CI prepares its own target seed
before Cargo tests; both macOS architectures and Linux still require their actual runs.

## Failure found in the preceding UI acceptance

The first expanded runtime run was **57 passed / 2 failed**. Both failed at Chromium integrity,
before reaching their intended assertions. They are retained in `runtime-portable-matrix-tests.log`.

Read-only comparison with the SHA-256-verified cached Chromium archive established:

- All 84 original files matched their pinned archive bytes.
- The only difference was a 372-byte `debug.log`, written at 2026-09-25 05:07:01 +08:00.
- The preceding UI runner directly launched the immutable seed's `chrome.exe`. Chromium
  writes module-adjacent diagnostics even when its browser profile is isolated.

The exact extra log was moved, not deleted, to
`tools/computer-use-probe/.run/mcp-transport-20260925T020240/seed-chromium-debug-20260925T050701.log`.
No lock, expected hash, official executable, daily browser, or unrelated process was changed.

`ui-acceptance.mjs` now uses the existing production `pinnedChromeLaunchPath` helper to launch
an independent writable copy under its own result directory. It verifies the whole seed's
content fingerprint before and after the run, including failure cleanup; original and cleanup
errors are retained together. This runner currently remains Windows acceptance, not a claim
that native UI acceptance on other systems is finished.

## Verified evidence

Logs below are under `tools/computer-use-probe/.run/mcp-transport-20260925T020240/`.

| Scope | Result | Artifact |
| --- | --- | --- |
| Runtime-filtered core suite after exact seed repair | 59/59, exit 0 | `runtime-portable-matrix-repaired-seed.log` |
| Module acceptance (included in core suite) | 11/11 | `runtime-import-native-tests.log` |
| Official Playwright materialization/import | 3/3, exit 0 | `runtime-materialize-native-import.log` |
| Actual prepared Windows seed `--check` | exit 0, `importProbe=passed` | `runtime-real-seed-module-check.log` |
| Existing immutable-browser copy helper | 4/4, no skips | `ui-runtime-copy-tests.log` |
| UI browser regression and immutable seed fence | 33/33, exit 0 | `ui-redesign-r6-immutable-seed.log` |
| App runtime harness, post-link Windows manifest | 2/2, exit 0 | `runtime-import-app-tests.log` |
| Rebuilt native isolated repair + actual module import | exit 0, `module_import=passed` | `runtime-import-isolated-repair.log` |
| Complete core suite after the import changes | 493/493, no ignored tests, exit 0 | `runtime-import-full-core.log` |
| Core all-targets Clippy with `-D warnings` | exit 0 | `runtime-import-core-clippy-green.log` |

The first Clippy run found one unnecessary lazy `Option` fallback in `broker/actions.rs`.
Its three boolean reads and enum selection now use `unwrap_or` rather than `unwrap_or_else`;
the initial failure remains in `runtime-import-core-clippy.log`. The full 493-test run includes
that change and the large-body IPC test that had failed intermittently in an earlier batch.
One green run does not erase that historical failure. The native repair binary below was
built before this equivalent fallback cleanup; do not treat its fingerprint as every subsequent
source revision or an all-platform release candidate.

The UI fixture remains real React components + real Chromium + **mocked Host IPC**.
R6 screenshots and `seed-integrity.json` are in
`tools/computer-use-probe/.run/ui-redesign-2026-09-24T21-45-28-170Z/`.
The 320px running screenshot was visually rechecked. No product UI styling changed in this
runtime follow-up; the earlier 117 component tests belong to the preceding UI batch.

Verified hashes remained the original pins:

- Seed hygiene fingerprint before/after UI: `d1f9a056d0382982d4ac43de0638e38982ddbd257ff254d08741a384e4f1b6e1`.
- Manifest: `6ae07e971e16fa9dd0d45729b758fabc68aee944acb78cff856b59f09d7f99ab`.
- Seed tree: `f2bec98e39763cba243b5dce85827d76c8ae0a8ed1dd2eb3f487e6311f6e3afc`.
- Chromium tree: `63c6075faf6d986bc8e9f11996f5d2e784e7f4d9471366b7a2ad6eebaca245e3`.

The current `cu_probe isolated-runtime-repair` was rebuilt and completed in the fresh
`tools/computer-use-probe/.run/runtime-import-repair-20260925T055135-138b765e512940f4bc1a4194819d292c/`
App home. It repaired from the real seed, resolved Node and Playwright only under that home,
and completed the new bounded module import. This is a native Host probe, not the graphical
installed App or a real-model test. No probe Node processes matching its workspace remained.
Probe SHA-256: `42965c77a1674f54c955b7589147c3bfefd827cbbb032a42f788e19581d1281b`.
The build retained one linker stdout warning; it was not a compilation/test failure.

## Full-goal work still open

- Mac signed/resource-archive delivery and native all-platform install/update/rollback.
- WebView W2: general DOM references, native isolated execution, physical cancellation.
- Complete macOS/X11 native actions and GNOME Wayland portal/PipeWire/libei support.
- Remaining intermittent extension pairing/observation failures and bounded failure diagnostics.
- Installed-App UI/permissions/OS scaling, real Grok acceptance, signed extension matrix, and
  the final frozen-version active soak. None is closed by this batch.

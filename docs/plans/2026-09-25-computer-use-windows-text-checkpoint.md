# Windows text effects and cross-process readback

Recorded 2026-09-25 (+08:00). Full Computer Use goal remains **active / not
releasable**. This batch changes Windows text helpers and their evidence, not
the feature default, authorization policy, release status or platform scope.
No commit, push or PR was performed.

Follow-up: [resolved-control identity fences](2026-09-25-computer-use-windows-child-identity-checkpoint.md)
adds native child/root retirement and directed cleanup checks. Counts below remain
the historical text batch; consult that checkpoint for the newer executable and evidence.

## Reproduced problems

The original native helper reported success if the field merely contained the
requested substring. A rejected append or paste could therefore pass using old
text. It also used a fixed 512-unit `GetWindowTextW` buffer, ignored `WM_SETTEXT`
failure, and used `EM_SETSEL(-1, -1)` as if it selected the end of the document.
That message actually deselects the existing selection. In the separate UIA
path, `type_text` used the same replacement operation as `set_value`.

The first five native regression groups all failed against those original
semantics. See `windows-text-red-native.log`; the red evidence is retained.

## Product changes

- Separate the native text implementation from mouse/key handling. Every new
  mutation still invokes the original request's input/admission/cancellation
  check. No new model-facing endpoint or fallback surface was added.
- Read the actual control value with synchronous `WM_GETTEXTLENGTH` / `WM_GETTEXT`.
  `GetWindowTextW` is not a cross-process EDIT-value API. The reader validates the
  copied count, terminator and UTF-16, permits documented length overestimates,
  and refuses incomplete, malformed or oversized readback.
- Bound standard EDIT readback at 1,048,576 UTF-16 units and Rich Edit at 65,535
  units for its `WM_GETTEXT` contract. The action request still has its existing
  4,000-character / no-NUL contract. Unsupported native control classes are not
  silently treated as editable window titles.
- Append at the explicit UTF-16 end, check the selected range and unchanged
  pre-value, and compare the complete resulting value. Empty append sends no
  input. Set-value checks native acknowledgement and exact readback.
- Refuse read-only/password native controls. UIA property-read errors now fail
  closed; read-only/password Value patterns cannot fall through to a blind write.
- UIA Value-pattern type-text preserves the old value and appends; set-value
  remains replacement. Both require exact provider readback. Failed writes or
  readbacks are not replayed or sent through a second input route.
- Preserve native occupancy until synchronous calls actually return. No timeout
  frees a buffer still accessible by another native window procedure, and this
  batch does not pretend that cancellation physically interrupts that procedure.

`AdapterActResult` is still not advertised as universally verified: a local text
comparison is not proof of an arbitrary application's semantic postcondition.
Field contents are not included in failure diagnostics.

## Evidence and boundaries

Logs live under `tools/computer-use-probe/.run/linux-native-20260925/`. Despite
that historical directory name, these are Windows processes.

- `windows-text-red-native.log`: terminal exit 1; all five original regressions
  fail (long text, caret position, ignored insertion, ignored paste, ignored set).
- `windows-text-green-native-{1,2,3}.log`: three terminal exit-0 rounds, 12 groups
  each. Adds long Unicode append/paste, empty input, cancellation before mutation,
  read-only/invalid input, partial edits, and malformed native readback.
- `windows-text-peer-native-1.log`: first cross-process attempt fails because a
  new child does **not** inherit the private station automatically. Its guard
  refuses the non-probe station before creating the EDIT fixture. No guard was
  disabled and the user's desktop was not used as a fallback.
- `windows-text-peer-bound-{1,2,3}.log`: three terminal exit-0 rounds, **13 groups
  each**, after explicitly opening the parent's exact private station/desktop.
  The child refuses a visible station, proves its explicitly selected binding, and the
  parent verifies the HWND belongs to the exact spawned PID. A different process
  receives direct append and clipboard paste; a local `GetWindowTextW` oracle in
  that child independently checks the complete final value.
- Probe SHA-256 for the final 13-group rounds:
  `82c823a7ab119ff8c960cc13fd1d1278042f71bb1f00069235bca7865f3bdb64`.
- `windows-text-app-tests.log`: **92 passed, 1 explicitly ignored**, exit 0,
  63.77 seconds. Includes seven new UIA policy tests using controlled providers;
  these are not actual third-party UIA provider acceptance. The ignored native
  desktop fixture remains separately gated; it is not counted as a pass.
- `windows-text-final-app-tests.log`: the final post-peer-source rerun also
  reports **92 passed, 1 explicitly ignored**, exit 0, 62.78 seconds. Its harness
  was rebuilt successfully and received the existing Windows test manifest
  post-link with `mt.exe`; no second linker `MANIFESTINPUT` was introduced.
- `windows-text-final-clippy.log`: strict App all-target checks with the probe
  feature pass (terminal exit 0, 21.12 s).
- `windows-text-production-clippy.log`: normal production-library strict checks
  pass (terminal exit 0, 11.60 s).
- Scoped `rustfmt --check` passes for all nine touched Rust files. Tracked
  `git diff --check` passes; it does not inspect untracked files. The five
  extracted/new input and provider-policy files were separately confirmed LF.
  No repository-wide line-ending or unrelated formatting rewrite was performed.

All commands started for this batch are terminal. The final comment-only wording
correction in the peer module does not alter its tested executable behavior.

The native probe invokes the production low-level text functions with a probe
admission closure; it does not claim to exercise the complete Broker/ACP/model
authorization chain. All clipboard mutations run in a new noninteractive window
station. The child is owned, has bounded handshake/exit waits, exits on parent
stdin closure, and is killed/waited only through its exact spawned handle on
failure. No ordinary browser, application, clipboard or unrelated process is
stopped or seeded by these tests.

## Remaining work

- Real installed-App / third-party UIA controls, IME and multiline normalization,
  Rich Edit beyond this bounded contract, and unsupported custom controls still
  require native coverage. Cross-process EDIT success is not coverage of all
  native text providers.
- Audit child-HWND identity/replacement across queued effects, the old named Drop
  fallback, and bounded recovery for a hung synchronous provider. Do not release
  input ownership merely because a caller deadline expires.
- Windows S4.5 foreground setup remains separately unresolved; no foreground
  stealing, privilege escalation or relaxed authorization was added here.
- Redesigned UI installed-App acceptance, macOS, GNOME Wayland, WebView immediate
  Stop/other-platform isolated worlds, signed install/update/rollback, real Grok
  E4 and final frozen 12-hour active soak are still required for the full goal.

## Primary API references inspected

- Microsoft Learn: `GetWindowTextW` — cross-process EDIT limitation and direct
  `WM_GETTEXT` guidance.
- Microsoft Learn: `WM_GETTEXT` / `WM_GETTEXTLENGTH` — copied counts, UTF conversion
  overestimation and Rich Edit limit.
- Microsoft Learn: `EM_SETSEL` — negative start deselects rather than selecting
  the document end.
- Installed `windows` 0.61.3 bindings — private station/desktop handle and
  `GetUserObjectInformationW` contracts used by the owned peer probe.

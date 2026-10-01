# Computer Use — explicit same-process portal recovery checkpoint

Date: 2026-10-01. Goal **active / partial — not releasable**.
Original objective: **接手grok的工作 完成computer use所有功能开发，推进到最终版**.
This is owned native acceptance, not a reduced definition of final delivery.

## Implemented and exercised

The opt-in installed GTK example now keeps the original process, GTK history,
PortalRegistry, broker and SessionGrants across two explicit consent rounds.
Physical takeover retires the first selection; original owners must close and
join and the original Host attempt must be fenced before a native GTK recovery
button is armed. A real click, not a timer or automatic retry, starts a distinct
run/attempt/authorization generation and a second real portal consent.

Before and after fresh input it rejects the old ticket, selection, target,
capture/claim and original keyboard/pointer DispatchRequests and snapshots,
while requiring the fresh target to remain alive. The actual GTK counter/history
continues 0→1→2, without resetting. Both rounds exercise actual PNG-targeted click,
native Enter, separate physical takeover and original-owner retirement.
Consent failure still closes/joins owners and fences its Host attempt.

The pointer oracle supports this cumulative history; a strict recovery oracle
and 18 synthetic regression cases are added to CI. Synthetic receipts do not
prove native execution or provenance. No production capability, unique-mapping,
snapshot, cancellation or no-fallback policy was weakened.

## Actual evidence and boundaries

Evidence: `tools/computer-use-probe/.run/gnome-grant-recovery-20261001/`.
Owned VM attempt19, UUID `812400f8-a6c6-4c38-a735-2b8d0ef8d3e2`, QEMU100528.
No host input, shares, passthrough or LAN bridge. Original experimental Shell2439,
owner`:1.23`, session5, exact previously patched Mutter DSO50318139…8eccb.

- `recovery-default-011`: same probe PID4892; two fresh native portal connections
  `:1.154`/`:1.157`; actual explicit GTK intent between two real pickers. Click
  count 0→1→2, distinct targets/four snapshots, old requests rejected on both
  sides of fresh input. Same helper epoch, generations15→16 and26→27 at takeover.
  Original process exits0/joins.
- `recovery-denied-015`: same default binary, PID5060. First round succeeds;
  explicit recovery is requested, then the second real picker is cancelled.
  Actual exit1/joins with `Start: user cancelled portal consent`; second Host
  attempt is fenced and owners joined. No second input or automatic third attempt.
- `formatted-instrumented-001` and `formatted-default-008` actually retest the
  prior checkpoint's exact formatted binaries b3843fe6…a28/fa5f6367…e9c62.
  This closes that prior native-retest gap without changing its old receipt.

Current exercised default recovery binary SHA256:
`c339a5cfad3d47e833b4aa2cdfe9947eab70162b03fc56a8f3dc2e92cdd4f64c`.
The current recovery code has **not** been native-tested in the diagnostics build;
all-feature Clippy is not a substitute for that run.

These runs use production Registry and SessionGrants fencing, but **do not commit
App Host authorization or exercise App/ACP/MCP**. Receipts explicitly retain
`hostAuthorizationCommitted:false`, `appAcpMcpVerified:false`,
`stockGnomeSupported:false` and `atomicStopVerified:false`.
The two-round success is takeover recovery, **not lock/rebind/topology recovery**.

## New open failure — do not hide behind successful retries

During earlier pending-consent lock/cancellation, the distro
`xdg-desktop-portal-gnome` PID2988 crashed with SIGSEGV; Shell2439 remained alive.
Installed backend46.2-0ubuntu1 and frontend1.18.4-1ubuntu2.24.04.3.
Backend SHA256 cf6839a8003098a7a4d710d520b8fcd9afbba6bbd21931bc0e420ac724b79bc7.
Original apport report SHA256
3797ca67ce3a6fe66b15d8c8123a3e4c7ea764cf916efe65c885e34914586beb.
The original report (including compressed core), journal and stripped backtrace
are retained. Root cause and repair are **not yet proven**.

`formatted-default-002` times out at90s; `003` closes on policy revocation during
consent; `004` encounters transient missing RemoteDesktop/CreateSession failure
during system-service reactivation. The backend automatically reactivates as
PID4115; no script forced a restart. All original failures remain evidence.
Later owned tests temporarily use idle-delay0 to control that separate variable;
the original uint32 300 is restored. This is not a production workaround.

## Regression, freeze and cleanup

- Native Wayland library suite **199 serial +199 four-thread**, zero failed,
  ignored or filtered. Real private PW/EIS fixtures, not an installed App suite.
- Python **101 total** (91 discovery+10 overlap;18 new), Node33, Linux runner22;
  strict Wayland all-target/all-feature and Linux App preview/default Clippy,
  rustfmt, YAML/AST, quality and diff checks pass. App resource overlay is
  check-only, not packaging/install acceptance.
- 587 sources frozen. Earlier238 Rust hashes remain unchanged across native runs.
  Locked/offline **cached**, not clean, replay matches exercised default and
  native test binary700b72a5…7252 exactly.
- Corrected `suite-024-1/4` archives contain272 files each, with member hashes and
  original-child cleanup. Earlier empty `suite-014-1/4` archives are retained as
  invalid evidence, not used for acceptance. Wrong Node glob, initial Clippy
  needless-borrow and first retirement-namespace assertion failures are retained.
- Independent readback finds all7 original probe PIDs and9 original portal
  clients absent;39 observed objects absent and18 original namespaces empty.
  The original frontend portal owner remains`:1.74` during that readback.
- Experimental endpoint/override/private frames removed; stock Shell5788,
  owner`:1.21`, session32, system-only maps/packages and disabled unchanged
  production helper verified. Original private compositor source/DSO restored.
  VM attempt19 original launcher6283 exits0/joins; PID100528 absent and SSH refused.

## Remaining final-delivery requirements

First investigate and repair the pending-consent backend crash with original
request/session lifetime evidence; then prove supported compositor integration
and actual App/ACP/MCP authorization, dispatch, stop and fresh recovery.
No implicit regrant, authority downgrade, guessed device or desktop fallback.

Windows x64; macOS arm64/Intel; Linux X11, Ubuntu24 native Wayland and Ubuntu22;
desktop, managed browsers, existing Chrome/Edge and App WebView; all input/IME/
clipboard; lock/focus/topology/device/owner/cancel recovery; signed install,
update, repair, rollback and uninstall; UX/DPI; real Grok E4 and one frozen final
candidate's 12h active soak all retain their original scope. This checkpoint
does not prove those remaining gates. No commit/push/tag/release.

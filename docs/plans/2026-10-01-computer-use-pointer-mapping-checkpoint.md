# Computer Use — installed portal pointer mapping checkpoint

Date: 2026-10-01. Goal **active / partial — not releasable**.
Original objective remains: **接手grok的工作 完成computer use所有功能开发，推进到最终版**.
This closes an experimental mapping diagnosis and native-click evidence gap,
not the complete Wayland implementation or the final cross-platform product.

## Root cause and scoped implementation

Actual installed GNOME portal consent returned a nonempty stream mapping ID.
Read-only libei diagnostics found **two resumed virtual absolute-pointer devices
with the identical ID and 1280×800 region**, not missing portal metadata.
Pinned Mutter 46.2 calls `update_viewports()` before seat bind, then unconditionally
adds another absolute device set on `EIS_EVENT_SEAT_BIND`.

The exact-version experimental patch delays creation until absolute capability
binding and replaces viewports only when that binding changes. Existing
client-side unique region, original snapshot, target generation, geometry and
cancellation checks are unchanged. No first-device choice, duplicate collapse,
desktop coordinate guess or input fallback was introduced. The patch does not
repair upstream relative-pointer/keyboard rebind behavior or establish supported
compositor distribution. Production helper code remains unchanged and disabled.

New `owned-acceptance-diagnostics` is compile-time **off by default** and also
requires an explicit environment opt-in. It emits bounded read-only metadata on
the original EI owner thread, not actions, native pointers or input content.
The explicitly owned GTK acceptance example locates a target in the actual
portal PNG, sends a production Registry click using that original observation,
and requires the real GTK counter 0→1 plus exact native left down/up. A strict
receipt oracle and 12 synthetic regression tests are wired into CI; synthetic
oracle tests alone are not native acceptance.

## Actual installed before/after evidence

Evidence: `tools/computer-use-probe/.run/gnome-pointer-mapping-20261001/`.
Owned VM attempt18 / UUID `812400f8-a6c6-4c38-a735-2b8d0ef8d3e2`, QEMU PID91858.
No host desktop input, host shares, input passthrough or LAN bridge.

- `mapping-001`: real consent/keyboard Enter, two matching regions identified.
- `pointer-before-003`: original experimental counter DSO, Shell2372/session4;
  exactly two matching regions, original click rejected as ambiguous, original
  owners closed, expected process exit1. No claimed click effect.
- `pointer-after-004`: patched experimental DSO, actual Shell4673 / `:1.24` /
  session23. The **same client binary** now sees one matching region. Actual
  640×400 portal pixels locate `[192,132]` inside `[113,117,271,147]`; GTK receives
  one left down/up and increments the counter. Real Enter also succeeds.
  Physical QMP Shift advances the same helper epoch 7→8, automatically retires
  the original grant and rejects its old target without an external stop.
- `pointer-default-005`: separate default-feature binary, diagnostics absent;
  same click/Enter effects and automatic takeover retirement, generation15→16.
  These are two isolated processes, **not same-process fresh recovery**.

Same instrumented before/after SHA256:
`138a12bcb2ab2a0e7b52f8f0360cb7602df6a09eca948c4a77e4fb14d80c6fa2`.
Exercised default client:
`fb239ede0fe2638c017cfe88634b0697345d0c2e99af85fcf638af57d6e7c3b5`.
Patched experimental Mutter DSO:
`50318139c57b7d680c2a3a3c49ffd49c69d8cd19ec0b9e251fefce4b08f8eccb`.
The unpatched counter DSO is **also experimental**, not a stock-GNOME baseline.

Three PNGs were independently decoded and target pixels recomputed; the two
passing observations were visually inspected. They show the owned test window
explicitly titled **NOT Grok App**, not App UX acceptance. Native effect proof
comes from real GTK events/counter; the images are pre-click observations.

## Regression and exact-candidate limits

Full native Wayland tests: **199×2**, serial and four-thread, zero failed,
ignored or filtered; private native PW/EIS fixtures and original-child cleanup
archived. This is not an installed GNOME/App suite. Node33, Python83 total
(73 discovery plus 10 overlap; 12 are new), Linux runner22, quality/YAML/diff
checks pass. Final rustfmt and strict Wayland all-target/all-feature Clippy pass;
Linux App preview/default all-target strict Clippy is recorded separately.

584 current source files are frozen. Locked/offline same-cache build replay
matches the **executed native test binary** exactly. Final rustfmt reordered the
acceptance example's module declarations; both replayed example binary hashes
changed. Original formatting failure and pre-format source are retained.
**The formatted example binaries are build-checked, not native-retested.**
Do not claim all current frozen binaries equal the installed exercised artifacts
or that the final candidate is accepted. This specific revalidation remains open.

Other retained failures: missing SDK rustfmt, initial diagnostics macro syntax,
Option<String> image decode compilation error and associated incremental rustc
ICE (subsequent successful build without cache deletion), idle-lock rejection
before the baseline pointer test, and a mistyped prior-hash assertion during
freeze preparation. No failed observation or prior receipt was overwritten.

## Original resource retirement and restoration

Original probes3358/4293/5401/5858 absent; no running probe units. Against the same
portal owner `:1.71`, original clients `:1.95` and `:1.115` and all ten original
portal request/session objects were independently read back absent. Helper
generation17 remained in the same epoch after release.

Experimental extension/override/private frames helper removed. Actual new stock
Shell6457 / `:1.20` / session33, `/usr/bin/gnome-shell`, system Mutter maps,
package verification, input settings, absent endpoint and disabled production
helper were checked. Private experimental source and DSO were restored to their
exact prior hashes; patched copies are archived. Ninja cache equivalence is not
claimed. QEMU's original launcher handle90079 returned **exit0 / joined**;
PID91858 absent and SSH42791 connection refused were verified. No commit, push,
tag, installation into system packages, or release.

## Remaining full objective — unchanged

1. Revalidate the formatted probe candidate, then native binding/unbinding,
   region/topology changes and source lifecycle; all remaining input/IME/clipboard.
2. Supported compositor integration and actual App/ACP/MCP consent/actions;
   original-grant lock, focus, topology, cancellation and fresh-grant recovery.
3. Windows x64, macOS arm64/Intel, Linux X11, Ubuntu24 native Wayland and Ubuntu22;
   Desktop, managed browsers, existing Chrome/Edge and App WebView.
4. Signed install/update/repair/rollback/uninstall; native UX and DPI acceptance.
5. Real Grok E4 and **one same frozen final candidate** 12h active soak.

No full-goal completion or release status is inferred from this scoped checkpoint.

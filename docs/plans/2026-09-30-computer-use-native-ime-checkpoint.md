# Native compositor IME edges and source provenance — 2026-09-30

Status: **active / partial — not releasable**. The full goal is unchanged:
接手 grok 的工作，完成 Computer Use 所有功能开发并推进到最终版。
This is real implementation/verification progress, not a verified wait or completion.
The local evidence-directory tag `20261001` is not the document's authoritative date.

## Production repair and remaining P0

The existing observer reaches ordinary native clients, but Mutter 46.2 routes
Wayland text-input filtering before its core idle-monitor callback. The official
Shell 46 input method processes keys asynchronously. Consumed down edges have no
later original event for either current observer. Forwarded up edges reconstruct
an event using `clutter_event_get_device`, not the originating source device.
Mutter's core keyboard is a native **LOGICAL aggregate** created by
`meta_input_device_native_new_virtual`; its device-node is null.

The old classifier treated any native null node as virtual/EI. This silently
ignored aggregate/unknown-source events too. Production now requires both an
actual direct-device `Clutter.InputMode.PHYSICAL` value and a null device-node
before virtual exclusion. The enum comes from GI, not a hardcoded number. Missing,
malformed, throwing or non-direct mode information is unknown loss, never proof
of virtual provenance. Direct kernel nodes still advance; direct native virtual
sources remain excluded in contracts. No input node is opened; no Shell method
is replaced and no event is stolen. Source-device mode is not, by itself, used
as the physical-versus-EI discriminator.

**This repairs forwarded release provenance only.** Do not reinterpret it as
complete IME interception or EI acceptance. A virtual event transformed by IME
may also lose provenance and conservatively revoke; actual native EI/IME
exclusion remains unverified. The three early-consumed down edges still fail.
Do not satisfy the remaining gate by ignoring IME, using device names or idle
polling, suppressing user input, accepting preedit/commit as physical provenance,
or narrowing the required input/platform surfaces.

## Actual installed GNOME acceptance

New permanent artifacts:
- `tools/computer-use-gnome-shell/ime-input-probe.py`: guarded owned-VM GTK3 native
  Wayland entry plus real installed IBus libpinyin. Only counters/booleans and
  identities leave the fixture; no typed content, key codes, coordinates or images.
- `ime_acceptance.py` / `ime_acceptance_test.py`: strict six-edge oracle and 13
  contracts, wired into CI. Native module/engine, same owner/epoch/PID, focus,
  unblocked state, exact release receipt, real composition/commit and generation
  advancement on every edge are mandatory. All full-input/grant claims stay false.
- `IME-ACCEPTANCE.md`: executable scope, lifecycle, source routing and residual gates.

Original same-helper runs `ime-delivery-004/005.json` each deliver all six edges,
compose Chinese and commit the expected owned literal, but **0/6** advance helper
state. The ordinary native-client control between them is **7/7**. This is a real
coverage defect, not a disconnected helper or unavailable IME.

After the source-mode repair, the original Host's embedded Repair path publishes
the candidate, rejects enabling the stale Shell cache, and a real owned GDM
restart replaces Shell PID **1332 -> 2608**. Host enable/disable/readback passes.
`fixed/ime-delivery-001/002.json` each produce **3/6**: all three forwarded releases
now advance by one, while all three consumed presses remain zero. The oracle and
GTK fixture are unchanged from the six-edge red baseline. The ordinary control
remains **7/7** on the repaired candidate. Both full six-edge results deliberately
remain **passed=false**, not filtered/skipped successes. The required next repair
is observation before IME/capture/mapped-pad consumption, preserving provenance.

## Checks and artifact identity

Frozen selection: **427 source records**, including current helper/probes/oracles,
CI and the unchanged Rust model. New original-Host preview test executable:
`/var/tmp/grok-cu-ubuntu-gnome-vm-20261001/ime-mode-bin/app-preview-tests`
SHA-256 `c0073e995cfd6afdf9dcd5f5c47bf2e016c62487c25108564f22b3fa28c9ba2f`.

- Policy contracts: the current tests against the archived old policy give
  **18 pass / 3 fail**; the repaired policy gives **21 pass / 0 fail**.
- Python oracles: **21 pass** (8 ordinary + 13 new IME).
- Rebuilt preview App: helper **17 pass / 10 manual ignored**; commands **15**,
  portal **6**, lifecycle **1**. Two relevant manual Host tests each run explicitly
  with **1 pass / 0 ignored**. Other ignored manual tests are not claimed complete.
- Private GJS/GVariant/GI probes **14 and 16**: each 7 read methods, 5 content-free
  signals. These are private protocol/API checks, not actual input acceptance.
  The private bus's inotify permission warning is retained, not suppressed.
- Full preview App + Wayland all-target strict Clippy `-D warnings`, code-quality
  gates, scoped syntax/whitespace and diff checks pass.

The previous full Wayland **188**, Windows/macOS/UI/Grok results are **historical**;
none is claimed rerun by this phase. No signed build, installer or final release
candidate is created here. No publication, tag, push or PR is performed.

## Ownership, failures and cleanup

All testing stays in the existing expendable, UUID-guarded Ubuntu/GDM VM; no user
login desktop, host input device permissions, host sharing or input group changes.
Official guest apt installs only libpinyin and five dependencies (zero upgrades).
A running IBus initially lacks the new engine, so the owned GDM is explicitly
restarted after verifying disabled helper state. The first blocked/engine-missing
attempt, the unsupported loginctl JSON option, an early fixed-sleep receipt race
and the initial three-stage oracle/driver are retained. The final parent uses a
joined persistent read-only SSH RPC child and bounded actual receipt waits, not
fresh SSH setup latency or a fixed sleep as proof.

Guest input-source and MRU settings are saved/restored for every fixture. Final
readback confirms original `[('xkb', 'us')]`, empty MRU, active xkb engine, helper
disabled/state 2, endpoint absent, directory 0700/files 0600 and matching hashes;
no fixture sockets or processes remain. All original fixture and RPC children
join. QEMU attempt **10 / PID19561 / handle98700** and attempt **11 / PID21461 /
handle68296** both receive explicit poweroff and their original handles return
**exit 0 / originalChildJoined=true**. No App/portal grant is created.

The VM still uses the previously documented GA Mesa comparison; updated Mesa
25.2.8/virtio Shell startup compatibility is not solved or silently reclassified.
All previous sealed receipts/artifacts are retained. This phase's receipt and
independent verifier record current source/binary/log hashes plus the previous
archival source/documents, rather than demanding changed current files match an
older candidate.

## Full end-state remains open

Windows x64, macOS arm64/Intel, Linux X11 and installed Ubuntu24.04 native GNOME;
Desktop, managed browser, existing Chrome/Edge and App WebView through App/ACP/MCP;
all input/Chinese IME/clipboard, OS consent, takeover/lock/focus/topology and joined
cancellation/recovery; Ubuntu22.04 baseline and AppImage/deb/rpm; signed install,
update, repair, rollback and uninstall; native UX/DPI; actual Grok E4; **12-hour
active soak on one final frozen candidate** remain the unchanged completion scope.
No completion/blocked/paused goal transition is warranted by this partial repair.

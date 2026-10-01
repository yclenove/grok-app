# Native EIS / physical IME paired gate — 2026-09-30

**Goal remains active / partial — not releasable.** Full Computer Use development
and final release are not complete. This phase adds actual native EIS acceptance
and exposes a second P0; it does not claim to repair the production observer.
The `20261001` artifact directory is an existing run label, not a new release date.

## Current-state findings

Previous native IME work was real progress: logical-source handling changed
physical six-edge acceptance from 0/6 to 3/6. That alone did not establish EI
exclusion. This phase retains the physical oracle unchanged and adds the opposing
native-EIS contract, actual libei sender and original-session retirement checks.

On the same installed GNOME46/Mutter46.2 Shell owner `:1.28` / PID1328, helper
source and owned VM attempt12/PID22148, both physical and native EI inputs produce
generation deltas **`[0,1,0,1,0,1]`** through actual native Wayland/libpinyin:

| Contract | Expected | Observed | Whole gate |
| --- | --- | --- | --- |
| Physical composition/commit down/up | Six delivered edges each advance loss generation | Three consumed downs missed; three releases observed | **false, 3/6** |
| Native EIS composition/commit down/up | Six delivered edges never advance physical-loss generation | Three forwarded releases falsely advance generation | **false, 3/6** |
| Paired source discrimination | Both contracts hold | Neither does | **false** |

Final receipts are `ei-ime-delivery-006/007.json` paired with
`physical-ime-control-003/004.json`. Each run proves real preedit, correct Chinese
commit, all six ordered edges, stable owner/fixture/epoch, focus and unlocked
state, monotonic counters and no inter-edge drift. Earlier complete `004/005`
native-EIS runs and `001/002` physical controls reproduce the same result.
The original physical gate was not weakened, skipped or replaced with idle
activity. No arbitrary key/text content is exported by the probe.

## Implementation and regression coverage

- Added `tools/computer-use-gnome-shell/ei-ime-peer.c`: native libei sender with
  only six fixed ordered fixture edges, keyboard-only binding, bounded lifetime,
  explicit ownership checks and original peer stop/join. No arbitrary input API.
- Added `ei-ime-launcher.py`: same-Shell D-Bus UID/PID pinning, explicit owned
  `CreateSession` / `Start` / `ConnectToEIS`, inherited original socket, focused
  fixture checks before each edge; original session `Stop` + `Closed` + absent
  endpoint, then connection close. Receipts include process/owner/session identity
  and peer/launcher SHA256, not just success booleans.
- Added `ei_ime_acceptance.py` / `ei_ime_acceptance_test.py`: unchanged physical
  delivery/identity checks, opposing virtual-exclusion contract and mandatory
  paired gate. CI invokes the new eleven-case oracle suite. Eleven EIS/pair,
  thirteen existing physical IME and eight ordinary oracle tests pass: **32**.
  Existing policy contracts pass **21/21**. These are local tests, not a CI run.
- Added `EI-IME-ACCEPTANCE.md` and corrected the helper README's stale 0/6-only
  account. Production `extension.js`, `policy.js`, metadata, Rust and App code
  are unchanged in this phase; no newly built or newly tested App is claimed.
- Strict C build uses `-std=c11 -Wall -Wextra -Werror -O2` and the pinned static
  libei1.5 SDK. Final peer: 179952 bytes, SHA256
  `a5b66c0057bb4aa77aae6e4bf03433770c6c54f3da4355225591d84eb4fe3786`.
  Final launcher SHA256:
  `e64617deee1950522be044b8c7e1f1fd9b83d307b856845fc0b51cb23fbc643a`.
  The recorded GLIBC requirement is not an Ubuntu22.04/package acceptance claim.

## Failures retained, ownership and environment

The initial build failed on CRLF shell line endings; the original log is retained.
Native-EIS attempts001/002 failed before creating a session because the owned VM
marker is exactly 36 ASCII bytes, not a newline-terminated UUID. The guard now
matches that exact immutable marker; UID, owner, non-writability and identity
checks remain strict. Failed startup stderr capture was corrected and retained.
Attempt003 correctly rejected a locked/unfocused fixture. Only the explicitly
identified owned local VM session was unlocked; no user login desktop changed.
All these failures remain in the evidence and are not counted as input passes.

The original VM process handle **11515** returned exit0 with
`originalChildJoined=true`; QEMU PID22148 was not restarted on observation expiry.
Before shutdown, the original final EIS peer PIDs4476/4927 were absent, original
bus owners `:1.171` / `:1.188` were absent, and original session endpoints `u3/u4`
were absent. All native sessions/peers reported clean stop/join. GTK/RPC/SSH
children joined; helper was disabled with endpoint absent; sources/MRU restored
to US xkb/empty MRU; actual engine restored to `xkb:us::eng`; fixture sockets and
owned EIS processes were absent. Original peer/launcher/helper hashes were read
back. No `/dev/input` open, host device/group/permission change, Shell monkeypatch,
interception/reinjection or product permission grant occurred.

The environment remains the owned installed Ubuntu24.04 daily-image VM with the
previously documented GA Mesa24.0.5/LLVM17 comparison. Updated Mesa compatibility
is still unresolved; no general Ubuntu24.04 release certification is asserted.
Mutter's private RemoteDesktop API is used only for this fixed-version owned
acceptance. It proves actual EIS test transport, **not portal consent, an App
grant, authorized product input or a supported production fallback**.

## Evidence and next required development

Phase: `tools/computer-use-probe/.run/gnome-ei-ime-20261001/`.
The final source freeze contains **432** records; the initial/fixed snapshots,
original failed logs, official version-pinned source records, final paired rows,
original lifecycle and prior document copies are retained. `seal.py` validates
the raw rows, hashes, immutable prior archive and cleanup, and the independent
Node verifier recalculates delivery/generations without importing either Python
oracle. The previous receipt SHA256 is
`edbb935d0324a8cfb785f574df4ae17330926b6f8df88d4f8ea7669e959c073c`.
Earlier 7/7 ordinary input, App/helper tests, full Wayland188 and Clippy runs
remain historical evidence, not reruns of this phase.

**Next repair constraint:** lossless source-bound observation must occur before
Mutter's early IME consumption/provenance loss. Excluding every logical/null-node
event regresses the physical releases; accepting them all produces the newly
confirmed virtual false positives. Muting the observer while EI exists, timing
heuristics, raw device access or modifying Shell input behavior do not meet the
required end state. Preserve both six-edge gates when repairing that path.

Original final scope stays open: every input family (IME/capture/pad/touch/tablet/
grabs/clipboard), lock/focus/topology/cancellation/recovery and real human takeover;
App/ACP/MCP actual portal consent and autonomous grant revocation; Windows x64,
macOS arm64+Intel, Linux X11 and installed GNOME Wayland across Desktop, managed
browser, existing Chrome/Edge and App WebView; Ubuntu22.04 baseline, AppImage/deb/
rpm, signed install/update/repair/rollback/uninstall; native UX/DPI; real Grok E4;
and **12h active soak on one final frozen candidate**. No commit, push, tag,
signing, publication or full-goal status change occurs in this phase.

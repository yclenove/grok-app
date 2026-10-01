# Native pointer interleave and original-source retirement

This is an **owned-VM acceptance fixture**, not a product input path, portal
fallback, authorization source or complete input qualification. The production
helper remains unchanged and disabled. The experimental counter still requires
a supported compositor integration before it can be a product provider.

## Artifacts

- `pointer_acceptance.py`: fifteen ordered, content-free native GTK snapshots.
  Requires continuous original Shell owner, client PID and helper epoch; active
  native Wayland focus; no blocked, saturated, regressed or unaccounted state.
  Unexpected event categories or duplicate non-scroll delivery fail. Physical
  overlap must advance the counter even when client delivery is suppressed;
  every EI edge and source retirement must leave the counter unchanged.
- `pointer_acceptance_test.py`: fifteen regression tests including missing and
  reordered cases, extra input, identity/epoch loss, malformed counters, virtual
  false positives, lost physical input, and a missing source-retirement release.
- `ei-pointer-peer.c`: exact-owned-VM, non-root, read-only-marker and Unix-FD
  guarded libei sender. It accepts only six fixed pointer commands in order,
  never arbitrary input or code. The sequence is motion, left down, drag, wheel,
  left up, right down. The final right button remains held until source removal.
  Normal closure does **not** explicitly inject a compensating button release.
- `ei-pointer-launcher.py`: binds the private test session and EI FD to the
  actual authenticated native Shell owner and the foreground GTK client. It
  closes the original peer/session/bus with bounded waits. `--retirement kill`
  kills only its original still-live child after all six acknowledged commands;
  requires actual exit `-SIGKILL`, no fabricated peer-cleanup message, and joins
  that child before completing session cleanup.
- `client-input-probe.py`: unchanged native GTK event-category counter fixture;
  records no typed text, key contents, pointer coordinates or application data.

The private Mutter test transport is **not portal consent or an App grant**.
These probes cannot enable the product or turn a policy failure into readiness.

## Ordered native experiment

Keep one EI source connected while alternating EI/owned USB motions, EI left
press, overlapping USB left press/release, dragging, and both wheel sources.
The USB overlap edges are seat-suppressed at GTK but must remain observable at
native ingress. Then release EI left, press EI right, and retire its source.
Verify exactly one client release without a physical-generation change, followed
by delivered physical motion and right down/up with advancing generations.

Run **both** deliberate disconnect and original-child SIGKILL variants against
the same installed native GNOME candidate. Restore the owned desktop between
fixtures; do not change the event oracle to hide focus/readiness failures.
After each run, independently verify original peer/client PIDs, original bus
owner, original remote-session path and client sockets are absent. Remove only
the exact owned extension/override and verify actual stock Shell maps and package
integrity before powering off/joining the original VM process.

Compile the peer with C11, `-Wall -Wextra -Werror` and `pkg-config libei-1.0`.
Create new build artifacts under `umask 077`: the launcher rejects group/world
writable peers. Do not relax that check or chmod an unrelated SDK tree.
Run this command (also wired into CI):

```sh
python3 -m unittest discover -s tools/computer-use-gnome-shell -p pointer_acceptance_test.py -v
```

The 2026-10-01 native run records live under
`tools/computer-use-probe/.run/gnome-pointer-interleave-20261001/`; full source
snapshots, original failures, executable hashes, raw before/after rows and
original-process lifecycle records are retained. Consult the matching dated
checkpoint for actual results, not this methodology as evidence of passing.

## Remaining scope

These tests do not establish human input provenance, installed stock-GNOME
support, compositor-side atomic stop, authorized App/ACP/MCP revocation or fresh
consent recovery. Native capture, touch, tablet-tool/pad, other grab families,
physical device removal and Shell-owner loss remain separate gates. QMP's USB
tablet routes ordinary pointer events here; it is not a drawing-tablet test.
All original platforms, target surfaces, input/IME/clipboard, packaging/signing,
UX/DPI, real Grok E4 and same-final-candidate 12h active soak remain required.

# Native client input acceptance

This is a **narrow native-client acceptance gate**, not complete input acceptance.
The former GNOME46 late-filter implementation failed all seven cases. The new
synchronous `NativeEventWatch` passes two complete runs on the same owned installed
Ubuntu/GDM/Wayland VM. **Early IME/capture/pad paths and App grant revocation remain
unverified/unfixed**; do not enable real automation on these seven cases alone.
The IME path now has a separate deterministic fixture and a confirmed six-edge
red gate; see [IME-ACCEPTANCE.md](IME-ACCEPTANCE.md). Its requirements supplement,
not replace, the seven ordinary-client cases here.

## Deterministic oracle (CI)

```sh
python3 -m unittest discover -s tools/computer-use-gnome-shell -p input_acceptance_test.py -v
```

`input_acceptance.evaluate` requires seven ordered cases: consecutive pointer
moves, button down/up, scroll, key down/up. Each contains before/after native GTK
client counters and helper snapshots. It rejects missing/filtered/duplicate cases,
wrong input categories, lost foreground status, X11, owner/epoch/PID replacement,
counter rollback, blocked snapshots, malformed types, and content-bearing fields.
Client receipt without helper advancement is **failure**, as is advancement without
client receipt. Positive unit fixtures do not certify a real compositor.

## Explicit owned-VM probe

`client-input-probe.py` is run **inside an isolated, expendable VM**, never the
user's login desktop. It requires all of:

- `--owned-vm-uuid` matching a non-writable, root-owned regular
  `/etc/cu-owned-vm-id`, and a non-root guest user;
- the exact user's `/run/user/UID`, local owned Wayland socket, and
  `GDK_BACKEND=wayland`; the actual GDK backend is checked again;
- a random 32-hex `--token` and a parent that owns the original foreground SSH
  process, bounds its lifetime, and explicitly enables/restores the helper;
- parent-side live QEMU UUID, PID/start-time/boot-ID, exact disk and loopback SSH
  checks before each QMP/SSH operation. A stale owner file is not evidence of life.

The probe opens only its own GTK window, D-Bus read method, and private 0600 Unix
control socket. Same-UID peers with the token may request `snapshot` or `quit`;
there is no command evaluation, capture/grant API, key injection, or `/dev/input`
access. It retains event **category counts only**, not key codes, text, coordinates,
device paths or screenshots. The parent must send only its own fixture input,
release its held keys/buttons on every path, disable the helper, verify endpoint
absence, and join the original SSH/fixture process before stopping the VM.

The parent arms a focused client and reads a baseline before each QMP USB action,
then checks both actual GTK receipt and helper generation in the same owner/epoch.
Two repeated pointer moves deliberately prevent a last-device-changed signal from
being mistaken for a per-input observer. Both down and up edges are separate cases.
Do not reinterpret a failed case as skipped or a protocol-only success.

## Current evidence and remaining scope

The original red adapter and logs are retained unchanged under
`tools/computer-use-probe/.run/gnome-input-source-20261001/`:
`run-client-conformance.py`, `client-delivery-001.json`, `client-delivery-002.json`.
Both runs delivered every case to the actual GTK client; all seven helper
generation deltas were zero. The second run also fixes a fixture-only duplicate
GTK main-loop quit warning; the original warning remains archived.

The repair evidence is separate in
`tools/computer-use-probe/.run/gnome-input-observer-20261001/`:
`client-delivery-001.json` and `client-delivery-002.json` both pass **7/7**.
Each GTK category counter advances by one; helper generation advances by one
(wheel action: two). The helper was published by the rebuilt original Host's
explicit Repair path, then loaded by a genuinely new Shell process, not copied
into a live GJS module cache. Both fixture processes join, and the helper is
disabled with its endpoint absent after each run. Oracle, fixture and seven-case
requirements are unchanged from the red runs.

This demonstrates a repaired ordinary guest-kernel USB-to-native-client path.
It is **not** human physical-input acceptance, IME/touch/tablet/grab coverage,
native EI exclusion, App/ACP/MCP grant revocation, signed installation, or
final-candidate soak. Those remain required. Extend coverage rather than
substituting Shell-panel tests, idle polling, device-name heuristics, or fewer
supported surfaces. Both green JSON receipts intentionally keep
`humanPhysicalInputVerified`, `appGrantRevocationVerified` and
`fullInputCoverageVerified` false.

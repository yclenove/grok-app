# Paired native EIS / physical IME acceptance — release gate remains closed

This is an additional acceptance gate, not an alternative source classifier or
product input route. Keep the physical six-edge gate in `IME-ACCEPTANCE.md`.
Both arms must pass on the same helper source candidate and Shell incarnation:

| Arm | Required actual delivery | Required policy generation |
| --- | --- | --- |
| Owned QMP USB physical-source control | Every composition/commit down/up edge | Advances for every edge |
| Owned native EIS keyboard control | The same six IME edges | Unchanged for every edge |

`ei_ime_acceptance.evaluate_pair` recalculates both arms, rejects missing/reordered
edges and different compositor owners, and never substitutes one arm for the
other. It uses the **unchanged** `ime_acceptance` identity, focus, native Wayland,
real libpinyin/preedit/commit, pristine-fixture, counter and continuity checks.
Rows alone cannot certify actual transport, binary provenance or retirement;
scope flags for those wider claims deliberately remain false.

## Native sender and ownership

- `ei-ime-peer.c` links the pinned libei sender library and accepts only the fixed
  six fixture edges, in order. It cannot take arbitrary keys/text/coordinates.
  Each actual edge is framed through the original EIS keyboard. Readiness/edge
  acknowledgements are **not** delivery receipts: the GTK fixture independently
  proves composition and the expected Chinese commit without exporting content.
- `ei-ime-launcher.py` pins the Mutter RemoteDesktop unique owner to the same
  authenticated GNOME Shell owner, UID and process as the focused fixture. It
  creates one owned native session, starts it, asks for keyboard-only
  `ConnectToEIS`, and passes the original Unix socket FD to the original peer.
  It never calls `NotifyKeyboard*` or a fallback input API.
- The original peer is stopped and joined; its synthetic pressed keys alone are
  released. The original Mutter session is stopped, its `Closed` signal observed,
  and the endpoint read back as absent before the original bus closes. Receipts
  bind peer/launcher hashes, peer PID/exit, Shell PID/owner and session path.
- Hard guards require the exact owned UUID, immutable root-owned marker, UID
  1000, private runtime/control endpoint, focused native fixture and hashed peer.
  These prevent accidental use; they are **not** a sandbox against a malicious
  same-UID process. Use only the dedicated installed acceptance VM.

Mutter 46.2's `org.gnome.Mutter.RemoteDesktop` XML explicitly describes a private,
version-specific API. Using it here proves **native EIS test transport only**.
It does not establish portal consent, authorized product input, App/ACP/MCP grant
revocation, human takeover, supported production API compatibility or release
readiness. Never turn this fixture into a portal bypass or product fallback.

## Reproduced failure and next repair constraint

The current helper produces generation deltas `[0,1,0,1,0,1]` for both actual
physical and EIS input through compositor-native libpinyin. Each arm is 3/6;
the paired gate is **false**. Real preedit and Chinese commit are present, so
silence/no input, client-only IBus and synthetic counter fixtures cannot explain
away the failure. Same-source repeated runs are retained in the native EI/IME
checkpoint, including failed setup attempts and their cleanup evidence.

Mutter's existing early IME return precedes the current observer; forwarded
releases carry logical provenance. A repair must recover lossless, source-bound
observation before that information is lost. Do not suppress logical events,
ignore all input while an EI source exists, infer provenance from idle time,
names or timing, intercept/reinject user keys, open `/dev/input`, or monkeypatch
Shell input methods merely to make either arm green. Both gates stay mandatory.

## Local deterministic tests

```sh
python3 -m unittest discover -s tools/computer-use-gnome-shell -p ei_ime_acceptance_test.py -v
python3 -m unittest discover -s tools/computer-use-gnome-shell -p ime_acceptance_test.py -v
node --test tools/computer-use-gnome-shell/policy.test.js
```

The eleven EIS/pair unit cases cover unchanged generation, every false positive,
delivery versus silence, swapped/reused physical rows, missing/reordered cases,
identity/lock/focus/counter faults, inter-edge changes, paired owner changes and
the reproduced symmetric failure. Passing these tests only validates the oracle;
it must never turn the real red installed-GNOME gate into a pass or skip.

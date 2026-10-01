# Native compositor IME acceptance

**Required gate, currently red. Not a release or full input acceptance claim.**
`ime-input-probe.py` uses an actual native GTK3 entry, the `wayland` IM module and
the installed IBus `libpinyin` engine, inside the explicitly owned expendable VM.
It does not implement an input method, mock Shell callbacks, patch compositor
methods, open input nodes, or create an App/portal grant.

## Content-free fixture and oracle

The fixture requires the same root-owned VM UUID marker, non-root guest user,
owned local runtime/Wayland socket, explicit backend, random control token and
same-UID Unix peers as the ordinary-client fixture. Its only RPCs are snapshot
and quit. It exports counters, booleans and process/helper identity, never input
text, key codes, coordinates, device paths or screenshots. The entry compares
the owned literal test commit internally and exports only `expectedCommit`.

The parent generates its own QMP USB keyboard sequence: `n`, `i`, Space. These
are six separate down/up edges. It waits for each actual client receipt before
sampling and retains the original fixture SSH and persistent read-only RPC child
until joined. A fixed sleep alone is not delivery evidence. The final oracle:

- Requires six distinct ordered cases, one unchanged Shell owner/helper epoch/
  fixture PID, an unblocked helper, native Wayland, entry focus and actual
  `libpinyin` engine at every snapshot.
- Requires preedit on the first two down edges and one expected Chinese commit
  on the final down edge; none may reach GTK as a raw key press.
- Requires exactly one forwarded GTK release and no new composition/commit on
  each up edge. IBus legitimately forwards releases: an all-raw-keys-zero oracle
  would confuse successful IME consumption with failure to deliver a release.
- Requires helper generation advancement on **every** down and up edge, with no
  identity/counter rollback, extra content fields, unaccounted intermediate
  activity, lost focus or synthetic fixture success.

Run deterministic CI contracts (positive fixtures are not compositor proof):

```sh
python3 -m unittest discover -s tools/computer-use-gnome-shell -p ime_acceptance_test.py -v
```

## Original installed-VM result

`tools/computer-use-probe/.run/gnome-ime-input-20261001/ime-delivery-004.json`
and `ime-delivery-005.json` both contain all six valid deliveries, successful
composition/commit and **zero generation advancement on all six edges**.
The ordinary seven-case control passes between them. The helper is not merely
offline: its unique owner/epoch, unblocked snapshots and ordinary observation
are verified. The original three-stage and early fixed-sleep attempts are
retained separately, including their failures; they are not counted as six-edge
acceptance. No fake green or filtered subset substitutes for this gate.

The current classifier also repairs an independent provenance error: a LOGICAL
aggregate keyboard's null device-node is not evidence of a direct virtual/EI
source. Mode validation must remain conservative, but changing classification
alone cannot observe events consumed before either observer runs. A fresh
embedded-Host installation and new Shell process are required to test changes;
on-disk edits do not update an already cached GJS module.

## Lifecycle and remaining requirements

Enable only the owned helper explicitly; save, restore and read back the owned
input-source/MRU settings and active engine. Release only this fixture's held
keys, quit and join the original children, verify helper disabled/endpoint absent
and no fixture sockets, then power off and join the original QEMU process.
Observe actual session lock state; do not change idle/lock settings or borrow
the user's desktop. An explicitly recorded owned-VM unlock is not App consent.

Six edges of this fixed Chinese composition are not all IME behavior, actual
human takeover, native EI exclusion, input-capture/mapped-pad/touch/tablet
coverage, App/ACP/MCP grant revocation or final-candidate soak. The full original
project requirements remain open; early-event observation still needs repair.

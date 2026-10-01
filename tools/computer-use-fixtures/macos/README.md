# Owned macOS native input gate

This is an **opt-in adapter acceptance fixture**, not a replacement backend, a
capability selftest, a permission manager, or final-product acceptance. The probe
includes the production `macos_adapter.rs` directly; no Quartz/AX doubles are
linked on macOS. Non-macOS execution writes `not_run` and exits **2**, never PASS.

## Run on each supported native architecture

On an interactive **arm64 Mac** and an interactive **x86_64 Mac**, with Xcode
command-line tools, Rust and existing Screen Recording + Accessibility consent:

```sh
bash tools/computer-use-fixtures/macos/run-owned.sh --owned-cocoa
```

Review the source first. This deliberately opens and focuses **one new owned
Cocoa window**. Keep it exposed while the gate runs. The script neither requests
nor grants TCC permissions, and never installs/replaces the user's Grok App.
If macOS denies activation or any consent is missing, preserve the failure; do
not fabricate a passing result, expand to desktop input, or loosen the checks.
Building a fresh binary may require the operator to configure consent before a
subsequent deliberate run. Compilation on one CPU or Rosetta is not proof of
native execution on the other CPU. CI only typechecks/builds; it doesn't claim
an interactive/TCC test passed.

Both the probe and Cocoa fixture check their **own** `sysctl.proc_translated`
before native work, rather than inheriting the shell's result. Replies must
identify the same architecture as the probe and explicitly be untranslated;
unknown status, mixed architectures and Rosetta cannot count as native proof.

The output directory is newly allocated and retained even on failure. It records
the native exit code/log, OS/architecture/toolchains, HEAD and dirty state, exact
binary hashes, before/after source hashes, selected-window PNG/observation,
per-gate independent application state, and final `result.json`. The source
hashes must match. Only exit **0** and the **complete ordered 29-gate result**,
with clean owned-process shutdown, are a passing *adapter* run. Exit **1** means
failure, exit **2** means not_run. The fixture has a 90-second watchdog and exits
on its private input pipe's EOF; the probe only cleans up its retained child.

## Evidence covered

- Bind exactly the spawned PID, native window number, nonce title and production
  window-instance identity; never pick the first window with a similar title.
- Decode a real selected-window PNG and check dimensions; exclude the synthetic
  secure control/value from the AX tree.
- Semantic and screenshot-coordinate Unicode append preserve the initial text.
- Coordinate Left + semantic Right each produce one real matching key-down/up
  pair **and** the expected UTF-16 selection change in the owned `NSTextView`.
- Observe unchanged application state after rejecting preview, stale, imageless,
  unknown-reference, cancelled and moved-window authority. A fresh observation
  after the move restores the allowed path without replaying earlier actions.
- Semantic/coordinate button presses increment the owned application once each;
  abort retires the old observation; a newly observed generation fails after the
  fixture exits; final adapter occupancy is idle.
- Left double-click and right/middle click reach the independent pointer view
  with the correct button/count, paired native down/up, image-derived position
  and neutral modifier flags. Extra events or rewritten history fail the gate.
- Positive/negative pixel scrolling must deliver the correctly signed wheel
  amount **and move the rendered ruler's actual viewport state**. Zero delta
  must leave all observed input and controls unchanged.
- Forward/backward drag must move the blue box, not just return adapter success:
  check its unchanged dimensions, actual final position, neutral left-button
  events, ordered bounded path and owned release. Intermediate native drag
  events may coalesce; at least one real final drag event is still required.
  Motion does not have an AppKit click count (recorded as 0); a delayed drag
  release may also have count 0. Click gates still require their exact counts.
  Fresh selected-window PNGs and observations are saved after wheel/drag gates.
- Three read-only retained-name Wait gates match the known owned input label,
  reject an absent label at the deadline, retain the original reference after
  timeout, and reject it after a new model observation. The fixture's text,
  selection, keys, clicks, pointer state and event history must stay unchanged.
  No private pipe command injects a successful wait condition. These gates do
  not substitute for native delayed-label/Stop/replacement race acceptance.

The private JSONL control channel only supports **state / focus / move / quit**.
It cannot set expected text, synthesize keys/pointers, move the blue box, change
the ruler offset, or increment the click counter;
the actual production adapter must cause the checked effects. JSON version,
request sequence, nonce, PID, window and payload bounds are checked on every
reply. Protocol **v2** requires pointer state and rejects old v1 replies; bounded
event overflow, unknown event kinds, invalid coordinates and history loss cannot
be mistaken for unchanged input. The last independently reported state is saved
on failure before owned cleanup. Rejected-operation checks sample the actual
application for 200ms; this
is a bounded observation window, **not** a proof of atomic input isolation.

`pipe-contract-child.mjs` is only a malicious-output/lifecycle double used by
`macos_native_fixture_contract`. Those tests validate the verifier, not AppKit.
They must never be substituted for the compiled Swift executable or included in
native success counts.

## Still required for the full goal

Actual per-architecture runs and screenshot review; full NSTextField/ComboBox/
WebKit/self-drawn/AX-missing coverage; selection replacement, additional
drag/scroll applications and concurrent/held-input cases,
clipboard, IME, permission changes, concurrent native queues and complete
recovery; App/ACP/MCP integration; signed install/update/rollback; UI matrix;
real Grok E4; and the frozen-source 12-hour active soak. None is waived by this
fixture or its cross-host contract tests.

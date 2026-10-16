# GNOME window inventory

Optional GNOME Shell 46–49 extension. Runtime acceptance is **not_run** without
a matching desktop runner; metadata versions are compatibility candidates.
It lists windows and offers exact-object activation; it never captures pixels,
injects input, calls Shell Eval, changes settings over D-Bus or grants portal access.

Install manually as `computer-use-windows@grok.app` using GNOME's extension tools;
compile `schemas/` with `glib-compile-schemas`, then explicitly enable the extension.
Disabling/unloading unexports the endpoint. No automatic installation is performed.

The endpoint is `org.gnome.Shell` at `/org/grok/ComputerUse/Windows`, interface
`org.grok.ComputerUse.Windows1`. `ListWindows` returns protocol version 1, a random
enable-lifetime epoch and `(id, generation, title, appId, pid, peerPid)` records.
Only native Wayland windows set `peerPid`; XWayland PIDs remain untrusted metadata.
Titles and app IDs are never identity or source-selection evidence.

Activation defaults to disabled. Trusted user setup may set schema
`org.grok.computer-use.windows` key `activation-owner` to the current engine's
**unique** D-Bus owner (for example `:1.42`). An engine restart needs a new binding.
Do not bind a well-known name or expose this setting to model tools. `Activate`
also requires a Router-approved foreground flag and the exact epoch/ID/generation/PID;
Shell rechecks the object and PID immediately before activation. Session blocking
or any ambiguity refuses. A lost activation reply is unknown and must not be replayed.

The Rust caller pins Shell's bus PID to an independently trusted compositor process
anchor and rechecks canonical image path, dev/inode, SHA-256 and process start time.
No title/size heuristic can prove a portal stream belongs to a listed window;
unproven stream scope remains handoff in the portal adapter.

Port source: `archive/cu-v1-2026-10-13:tools/computer-use-gnome-shell/extension.js`
(existing Shell connection, unexport cleanup, fail-closed session check) and
`src-tauri/computer-use-wayland/src/gnome_control.rs` (unique-owner/process fence).
The archive extension has no window inventory; the new narrow inventory is tested
independently rather than copying v1's unrelated native-input policy machinery.

# KWin window inventory

Optional KWin 6 script. Native KWin runtime acceptance is **not_run** without a
matching desktop runner. Install this directory explicitly with KPackage tools;
configure `engineBusName` to the trusted running engine's unique D-Bus owner,
then enable the script. Do not use a well-known name or model-provided destination.
No script is loaded or installed automatically by the Rust backend.

The engine exports `org.grok.ComputerUse.Windows1.Publish` on its existing unique
connection and accepts reports only from its independently pinned KWin bus owner.
Reports include an enable-lifetime marker and exact `Window.internalId`/object
generation. A 500 ms script timer refreshes inventory; the engine's injectable
monotonic clock expires reports at two seconds. Disable the script and close the
receiver when the engine session ends. A lost engine connection cannot redirect
reports to a replacement well-known service.

The public script interface exposes `Window.pid` without a documented proof that
it is a native Wayland peer credential rather than an X11 property. Consequently
all records are **observation-only**: process binding hands off, activation/input
are unsupported. Titles, desktop IDs and geometry never establish trusted identity.
The script cannot grant policy, activate windows, capture pixels or inject input.

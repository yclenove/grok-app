"""Owned-VM-only native Wayland IME fixture. Never exports typed content.

The parent owns helper enable/disable and the original foreground SSH process.
No shell evaluation, input injection, /dev/input access, or grant API lives here.
"""
import argparse
import json
import os
import pathlib
import re
import socket
import stat
import struct


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--owned-vm-uuid", required=True)
    parser.add_argument("--token", required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"[a-f0-9-]{36}", args.owned_vm_uuid) or not re.fullmatch(r"[a-f0-9]{32}", args.token):
        raise ValueError("explicit owned VM identity and token required")
    marker = pathlib.Path("/etc/cu-owned-vm-id")
    info = marker.lstat()
    if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022
            or marker.read_text().strip() != args.owned_vm_uuid or os.getuid() == 0):
        raise ValueError("not the exact non-root owned guest")
    runtime = pathlib.Path(f"/run/user/{os.getuid()}")
    if os.environ.get("XDG_RUNTIME_DIR") != str(runtime):
        raise ValueError("foreign runtime directory")
    display_name = os.environ.get("WAYLAND_DISPLAY", "")
    if not re.fullmatch(r"wayland-[0-9]+", display_name):
        raise ValueError("explicit local Wayland socket required")
    for path in (runtime, runtime / display_name):
        info = path.lstat()
        if info.st_uid != os.getuid() or stat.S_ISLNK(info.st_mode):
            raise ValueError("foreign or redirected runtime")
    if not stat.S_ISSOCK((runtime / display_name).lstat().st_mode):
        raise ValueError("Wayland display is not a socket")
    if os.environ.get("GDK_BACKEND") != "wayland":
        raise ValueError("no X11 fallback permitted")

    import gi
    gi.require_version("Gtk", "3.0")
    gi.require_version("Gdk", "3.0")
    gi.require_version("IBus", "1.0")
    from gi.repository import Gdk, Gio, GLib, Gtk, IBus

    if Gdk.Display.get_default().__class__.__name__ != "GdkWaylandDisplay":
        raise ValueError("actual GDK backend is not native Wayland")
    IBus.init()
    ibus = IBus.Bus.new()
    if not ibus.is_connected():
        raise ValueError("actual IBus connection required")
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)

    def call(dest, path, interface, method, values=None):
        return bus.call_sync(dest, path, interface, method, values, None,
                             Gio.DBusCallFlags.NONE, 1500, None).unpack()

    owner = call("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
                 "GetNameOwner", GLib.Variant("(s)", ("org.gnome.Shell",)))[0]
    counts = dict.fromkeys(("preedit", "commit", "key_press", "key_release"), 0)
    flags = {"preeditActive": False}
    window = Gtk.Window(title="Owned native IME acceptance — no content retained")
    window.set_default_size(1000, 650)
    entry = Gtk.Entry()
    entry.set_property("im-module", "wayland")
    entry.set_placeholder_text("Owned fixture only")
    box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=20)
    box.set_border_width(80)
    box.pack_start(Gtk.Label(label="Native IME acceptance: category counters only"), False, False, 0)
    box.pack_start(entry, False, False, 0)

    def preedit_changed(_entry, preedit):
        counts["preedit"] += 1
        flags["preeditActive"] = bool(preedit)

    def committed(_entry):
        counts["commit"] += 1

    def key_received(_entry, event):
        kind = "key_press" if event.type == Gdk.EventType.KEY_PRESS else "key_release"
        counts[kind] += 1
        return False

    entry.connect("preedit-changed", preedit_changed)
    entry.connect("changed", committed)
    entry.connect("key-press-event", key_received)
    entry.connect("key-release-event", key_received)
    window.add(box)
    window.connect("destroy", lambda _window: Gtk.main_quit() if Gtk.main_level() > 0 else None)
    window.maximize()
    window.show_all()
    entry.grab_focus()

    directory = runtime / ("grok-cu-ime-" + args.token)
    directory.mkdir(mode=0o700)
    endpoint = directory / "control.sock"
    server = socket.socket(socket.AF_UNIX)
    server.bind(str(endpoint))
    os.chmod(endpoint, 0o600)
    server.listen(2)
    server.setblocking(False)

    def serve(_source, condition):
        if condition & (GLib.IOCondition.ERR | GLib.IOCondition.HUP):
            Gtk.main_quit()
            return GLib.SOURCE_REMOVE
        connection, _ = server.accept()
        with connection:
            connection.settimeout(0.1)
            try:
                _, uid, _ = struct.unpack("3i", connection.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
                if uid != os.getuid():
                    raise ValueError("foreign control peer")
                payload = connection.recv(1025)
                if len(payload) > 1024:
                    raise ValueError("oversize control request")
                request = json.loads(payload)
                if set(request) != {"token", "method"} or request["token"] != args.token:
                    raise ValueError("wrong control token")
                if request["method"] == "snapshot":
                    policy = call(owner, "/org/grok/ComputerUse/NativePolicy",
                                  "org.grok.ComputerUse.NativePolicy1", "GetState")
                    engine = ibus.get_global_engine()
                    response = {"owner": owner, "policy": policy, "counts": dict(counts),
                                "nativeWayland": True, "windowActive": window.is_active(), "pid": os.getpid(),
                                "entryFocused": entry.has_focus(), "imeModule": entry.get_property("im-module"),
                                "engine": engine.get_name() if engine else None,
                                "preeditActive": flags["preeditActive"],
                                "expectedCommit": entry.get_text() == "\u4f60"}
                elif request["method"] == "quit":
                    response = {"stopping": True}
                    GLib.idle_add(lambda: (Gtk.main_quit(), GLib.SOURCE_REMOVE)[1])
                else:
                    raise ValueError("unsupported read-only control request")
            except Exception as error:
                response = {"error": str(error)}
            connection.sendall(json.dumps(response).encode() + b"\n")
        return GLib.SOURCE_CONTINUE

    source = GLib.io_add_watch(server.fileno(), GLib.IOCondition.IN | GLib.IOCondition.ERR | GLib.IOCondition.HUP, serve)
    print(json.dumps({"ready": True, "pid": os.getpid(), "socket": str(endpoint), "owner": owner}), flush=True)
    try:
        Gtk.main()
    finally:
        GLib.source_remove(source)
        server.close()
        endpoint.unlink()
        directory.rmdir()
        window.destroy()
        print(json.dumps({"fixtureStopped": True, "pid": os.getpid()}), flush=True)


if __name__ == "__main__":
    main()

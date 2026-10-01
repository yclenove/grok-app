#!/usr/bin/env python3
"""Owned GTK fixture. Run only inside the probe's private Xvfb / D-Bus session."""
import json
import os
import select
import sys
import time

if os.environ.get("GROK_CU_X11_FIXTURE") != "owned-xvfb":
    raise SystemExit("private Xvfb fixture opt-in required")

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gdk, GLib, Gtk

window = Gtk.Window(title="CU owned accessibility fixture")
window.set_default_size(420, 260)
window.move(90, 80)
box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
box.set_border_width(20)
entry = Gtk.Entry()
entry.get_accessible().set_name("CU editable field")
button = Gtk.Button(label="CU increment")
counter = [0]
edit_events = {"delete": 0, "insert": 0}
after_delete = None
after_insert = None
other_window = None
button.connect("clicked", lambda *_: counter.__setitem__(0, counter[0] + 1))
protected = Gtk.Entry()
protected.set_visibility(False)
protected.set_text("FIXTURE-PROTECTED-TEXT")
protected.get_accessible().set_name("CU protected field")
status = Gtk.Label(label="CU wait pending")
status.set_width_chars(32)
status.set_size_request(-1, 40)
status.set_single_line_mode(True)
status_events = [0]
box.pack_start(entry, False, False, 0)
box.pack_start(button, False, False, 0)
box.pack_start(protected, False, False, 0)
box.pack_start(status, False, False, 0)
window.add(box)
window.connect("destroy", Gtk.main_quit)
window.show_all()
window.present()
entry.grab_focus()
window.get_window().focus(Gdk.CURRENT_TIME)
Gdk.Display.get_default().sync()


def report(**extra):
    print(json.dumps({"text": entry.get_text(), "caret": entry.get_position(), "selection": list(entry.get_selection_bounds()), "deleteEvents": edit_events["delete"], "insertEvents": edit_events["insert"], "clicks": counter[0], "status": status.get_text(), "statusEvents": status_events[0], **extra}, ensure_ascii=False), flush=True)


def deleted(*_):
    global after_delete
    edit_events["delete"] += 1
    mode, after_delete = after_delete, None
    if mode == "tamper":
        entry.set_text("USER EDIT SURVIVES")
    elif mode == "disable":
        entry.set_sensitive(False)
    elif mode == "stop":
        report(deleteBoundary=True)
        ready, _, _ = select.select([sys.stdin], [], [], 4)
        if not ready or sys.stdin.readline().strip() != "resume-delete":
            raise RuntimeError("owned delete boundary was not resumed")
        report(deleteResumed=True)
    elif mode == "lost-reply":
        # Deliberately outlive the production one-second method timeout.
        # Recovery must correlate the late native reply, not this delay.
        time.sleep(1.3)


def inserted(*_):
    global after_insert
    edit_events["insert"] += 1
    mode, after_insert = after_insert, None
    if mode == "stop":
        report(insertBoundary=True)
        ready, _, _ = select.select([sys.stdin], [], [], 4)
        if not ready or sys.stdin.readline().strip() != "resume-insert":
            raise RuntimeError("owned insert boundary was not resumed")
        report(insertResumed=True)


def wire_entry():
    entry.connect_after("delete-text", deleted)
    # GtkEditable::insert-text has an in/out int pointer which GI cannot safely
    # marshal for a Python observer. EntryBuffer's post-insert signal does not.
    entry.get_buffer().connect("inserted-text", inserted)


wire_entry()


def finish_wait(replace=False):
    global status
    if replace:
        status.destroy()
        status = Gtk.Label(label="CU wait 完成🙂")
        status.set_width_chars(32)
        status.set_size_request(-1, 40)
        status.set_single_line_mode(True)
        box.pack_start(status, False, False, 0)
        status.show()
    else:
        status.set_text("CU wait 完成🙂")
    status_events[0] += 1
    return False


def command(_channel, condition):
    global entry, other_window, after_delete, after_insert
    if condition & GLib.IO_HUP:
        Gtk.main_quit()
        return False
    line = sys.stdin.readline().strip()
    if line == "state":
        report()
    elif line == "wait-start":
        GLib.timeout_add(250, finish_wait)
        report()
    elif line == "wait-replace":
        GLib.timeout_add(250, finish_wait, True)
        report()
    elif line == "wait-reset":
        status.set_text("CU wait pending")
        status.set_sensitive(True)
        status.show()
        report()
    elif line == "wait-disable":
        status.set_sensitive(False)
        report()
    elif line == "wait-hide":
        status.hide()
        report()
    elif line == "disable":
        entry.set_sensitive(False)
        report()
    elif line == "enable":
        entry.set_sensitive(True)
        report()
    elif line == "replace":
        entry.destroy()
        entry = Gtk.Entry()
        wire_entry()
        entry.get_accessible().set_name("CU editable field")
        entry.set_text("replacement")
        box.pack_start(entry, False, False, 0)
        box.reorder_child(entry, 0)
        entry.show()
        entry.grab_focus()
        Gdk.Display.get_default().sync()
        report()
    elif line == "caret-middle":
        entry.select_region(2, 2)
        entry.set_position(2)
        report()
    elif line == "select-middle":
        entry.select_region(2, 6)
        report()
    elif line == "select-reverse":
        entry.select_region(6, 2)
        report()
    elif line == "select-all":
        entry.select_region(0, -1)
        report()
    elif line.startswith("after-delete-"):
        mode = line.removeprefix("after-delete-")
        if mode not in ("tamper", "disable", "stop", "lost-reply"):
            raise RuntimeError("unknown owned delete test")
        after_delete = mode
        report()
    elif line == "after-insert-stop":
        after_insert = "stop"
        report()
    elif line == "focus-away":
        other_window = Gtk.Window(title="CU owned other focus")
        other_window.set_default_size(140, 100)
        other_window.move(700, 80)
        other_window.add(Gtk.Entry())
        other_window.show_all()
        other_window.get_window().focus(Gdk.CURRENT_TIME)
        Gdk.Display.get_default().sync()
        report()
    elif line == "focus-back":
        window.get_window().focus(Gdk.CURRENT_TIME)
        Gdk.Display.get_default().sync()
        report()
    elif line == "quit":
        Gtk.main_quit()
        return False
    return True


GLib.io_add_watch(sys.stdin, GLib.IO_IN | GLib.IO_HUP, command)
GLib.idle_add(lambda: (report(ready=True, pid=os.getpid()), False)[1])
Gtk.main()

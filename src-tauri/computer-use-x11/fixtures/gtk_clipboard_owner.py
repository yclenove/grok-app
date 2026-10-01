"""Private Xvfb clipboard owner; never run against an ordinary desktop."""
import os
import sys
import gi

if os.environ.get("GROK_CU_X11_FIXTURE") != "owned-xvfb":
    raise SystemExit("owned Xvfb required")
gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gdk, GLib, Gtk

clipboard = Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD)
clipboard.set_text("CU clipboard fixture 中文🙂", -1)
print("ready", flush=True)
loop = GLib.MainLoop()


def command(_source, _condition):
    line = sys.stdin.readline()
    if not line or line.strip() == "quit":
        loop.quit()
        return False
    return True


GLib.io_add_watch(sys.stdin, GLib.IO_IN | GLib.IO_HUP, command)
loop.run()

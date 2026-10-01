"""Independent consumer only on a private, explicitly owned Xvfb display."""
import os
import gi

if os.environ.get("GROK_CU_X11_FIXTURE") != "owned-xvfb":
    raise SystemExit("owned Xvfb required")

gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gdk, Gtk

text = Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD).wait_for_text()
if text != "task 中文🙂":
    raise SystemExit("independent GTK selection content mismatch")

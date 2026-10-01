import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Meta from 'gi://Meta';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {NativeEventWatch, PolicyState, PATH, XML, screenBlocked} from './policy.js';

// A read-only compositor observer. Does not synthesize/suppress input, capture
// pixels, call Lock/Unlock, grant portal permission or modify Shell methods.
export default class NativePolicy extends Extension {
  enable() {
    this._signals = [];
    this._filter = 0;
    this._nativeWatch = null;
    this._state = new PolicyState(GLib.uuid_string_random(), state => {
      this._exported?.emit_signal('Changed', new GLib.Variant('(usub)', state));
    }, Clutter.InputMode.PHYSICAL);
    try {
      if (!Meta.is_wayland_compositor()) throw new Error('Native Wayland required');
      const shield = Main.screenShield;
      if (!shield?.actor || !Main.sessionMode) throw new Error('ScreenShield unavailable');
      const refresh = () => {
        try {
          this._state.setBlocked(screenBlocked(shield, Main.sessionMode));
        } catch { this._state.fail(); }
      };
      // actor.show()/sessionMode.pushMode happen before the delayed public
      // ActiveChanged signal. Observe both, and the regular locked state.
      for (const [object, name] of [[shield.actor, 'notify::visible'],
        [shield, 'active-changed'], [shield, 'locked-changed'], [Main.sessionMode, 'updated']]) {
        this._signals.push([object, object.connect(name, refresh)]);
      }
      const names = ['KEY_PRESS', 'KEY_RELEASE', 'BUTTON_PRESS', 'BUTTON_RELEASE',
        'MOTION', 'SCROLL', 'TOUCH_BEGIN', 'TOUCH_UPDATE', 'TOUCH_END', 'TOUCH_CANCEL',
        'TOUCHPAD_PINCH', 'TOUCHPAD_SWIPE', 'TOUCHPAD_HOLD', 'PROXIMITY_IN', 'PROXIMITY_OUT',
        'PAD_BUTTON_PRESS', 'PAD_BUTTON_RELEASE', 'PAD_STRIP', 'PAD_RING'];
      const inputTypes = new Set(names.map(name => Clutter.EventType[name]));
      if (inputTypes.has(undefined)) throw new Error('Unsupported event API');
      const observe = event => {
        try {
          if (inputTypes.has(event.type())) {
            const device = event.get_source_device();
            // Nested/X11/unknown devices cannot silently impersonate native
            // virtual devices simply by omitting their device-node property.
            if (!device || GObject.type_name_from_instance(device) !== 'MetaInputDeviceNative')
              this._state.fail();
            else
              this._state.input(device);
          }
        } catch { this._state.fail(); }
      };
      this._nativeWatch = new NativeEventWatch(
        global.backend.get_core_idle_monitor(), () => Clutter.get_current_event(),
        observe, () => this._state.fail());
      this._nativeWatch.start();
      // Supplementary only: events reaching Shell can be seen by both sources.
      // Duplicate loss generations are conservative; neither source grants input.
      this._filter = Clutter.Event.add_filter(null, event => {
        observe(event);
        return Clutter.EVENT_PROPAGATE; // NEVER steal the physical user's input.
      });
      if (!this._filter) throw new Error('Input observer unavailable');
      // Export only after subscriptions exist. Use Shell's existing connection:
      // the Host pins its unique owner, not a separately claimable helper name.
      this._exported = Gio.DBusExportedObject.wrapJSObject(XML, {
        GetState: () => { refresh(); return this._state.snapshot(); },
      });
      this._exported.export(Gio.DBus.session, PATH);
      refresh();
    } catch (error) {
      this.disable();
      throw error;
    }
  }

  disable() {
    // The bus may already be gone. Still remove every original subscription;
    // a failed final broadcast cannot leave a physical-input observer attached.
    try { this._state?.fail(); } catch { /* Host heartbeat will expire. */ }
    try { this._nativeWatch?.stop(); } catch (error) { console.error(error); }
    this._nativeWatch = null;
    if (this._filter) {
      try { Clutter.Event.remove_filter(this._filter); } catch (error) { console.error(error); }
    }
    this._filter = 0;
    for (const [object, id] of this._signals ?? []) {
      try { object.disconnect(id); } catch (error) { console.error(error); }
    }
    this._signals = [];
    try { this._exported?.unexport(); } catch (error) { console.error(error); }
    this._exported = null;
    this._state = null;
  }
}

// Owned installed-VM acceptance only. This is NOT the production helper.
// policy.js and physical-input-watch.js are copied byte-for-byte by the explicit
// probe installer; no user Shell monkeypatch, event filter or input injection.
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Meta from 'gi://Meta';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {PolicyState, PATH, XML, screenBlocked} from './policy.js';
import {PhysicalInputWatch} from './physical-input-watch.js';

const OWNED_VM = '812400f8-a6c6-4c38-a735-2b8d0ef8d3e2';

function requireOwnedExperiment() {
  if (GLib.getenv('GROK_CU_INSTALLED_COUNTER_PROBE') !== OWNED_VM)
    throw new Error('Explicit owned-VM compositor experiment required');
  const file = Gio.File.new_for_path('/etc/cu-owned-vm-id');
  const info = file.query_info('unix::uid,unix::mode,standard::type',
    Gio.FileQueryInfoFlags.NOFOLLOW_SYMLINKS, null);
  if (info.get_file_type() !== Gio.FileType.REGULAR ||
      info.get_attribute_uint32('unix::uid') !== 0 ||
      (info.get_attribute_uint32('unix::mode') & 0o7777) !== 0o444 ||
      new TextDecoder().decode(file.load_contents(null)[1]) !== OWNED_VM)
    throw new Error('Wrong owned VM identity');
  if (!Meta.is_wayland_compositor() || global.backend.is_headless())
    throw new Error('Installed native Wayland session required; no headless fallback');
}

export default class CounterAcceptanceProbe extends Extension {
  enable() {
    requireOwnedExperiment();
    this._signals = [];
    this._watch = null;
    this._state = new PolicyState(GLib.uuid_string_random(), state =>
      this._exported?.emit_signal('Changed', new GLib.Variant('(usub)', state)),
    Clutter.InputMode.PHYSICAL);
    try {
      const shield = Main.screenShield;
      if (!shield?.actor || !Main.sessionMode) throw new Error('ScreenShield unavailable');
      const refresh = () => {
        try { this._state.setBlocked(screenBlocked(shield, Main.sessionMode)); }
        catch { this._state.fail(); }
      };
      for (const [object, signal] of [[shield.actor, 'notify::visible'],
        [shield, 'active-changed'], [shield, 'locked-changed'], [Main.sessionMode, 'updated']])
        this._signals.push([object, object.connect(signal, refresh)]);
      const seat = Clutter.get_default_backend().get_default_seat();
      if (GObject.type_name_from_instance(seat) !== 'MetaSeatNative')
        throw new Error('Native seat required');
      this._watch = new PhysicalInputWatch({
        read: () => ({version: seat.grok_physical_input_version,
                      generation: seat.grok_physical_input_generation}),
        subscribe: callback => seat.connect('notify::grok-physical-input-generation', callback),
        disconnect: id => seat.disconnect(id),
      }, () => {
        // The public serial is a loss token, not an event count. One observed
        // native delta advances it once, even when compositor notify coalesces.
        if (!this._state.failed) this._state.advance();
      }, () => this._state.fail());
      this._watch.start();
      this._exported = Gio.DBusExportedObject.wrapJSObject(XML, {
        GetState: () => { this._watch.poll(); refresh(); return this._state.snapshot(); },
      });
      this._exported.export(Gio.DBus.session, PATH);
      refresh();
    } catch (error) {
      this.disable();
      throw error;
    }
  }

  disable() {
    try { this._state?.fail(); } catch { /* Any client heartbeat fails closed. */ }
    try { this._watch?.stop(); } catch (error) { console.error(error); }
    this._watch = null;
    for (const [object, id] of this._signals ?? []) {
      try { object.disconnect(id); } catch (error) { console.error(error); }
    }
    this._signals = [];
    try { this._exported?.unexport(); } catch (error) { console.error(error); }
    this._exported = null;
    this._state = null;
  }
}

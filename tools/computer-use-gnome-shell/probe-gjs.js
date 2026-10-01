// Execute only under a fresh dbus-run-session; never point at a login bus.
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import {PolicyState, PATH, INTERFACE, XML} from './policy.js';

if (GLib.getenv('GROK_CU_PRIVATE_HELPER_TEST') !== '1') throw Error('Private test opt-in required');
const address = GLib.getenv('DBUS_SESSION_BUS_ADDRESS');
const connect = () => Gio.DBusConnection.new_for_address_sync(address,
  Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION, null, null);
const server = connect(), client = connect();
const loop = new GLib.MainLoop(null, false);
let exported, id, timer, failed = false;
const received = [];
const state = new PolicyState(GLib.uuid_string_random(), s =>
  exported.emit_signal('Changed', new GLib.Variant('(usub)', s)), 47);
function check(value, message) { if (!value) throw Error(message); }
function read() {
  return new Promise((resolve, reject) => {
    client.call(server.get_unique_name(), PATH, INTERFACE, 'GetState', null,
      new GLib.VariantType('(usub)'), Gio.DBusCallFlags.NONE, 1000, null, (connection, result) => {
        try { resolve(connection.call_finish(result).deep_unpack()); } catch (error) { reject(error); }
      });
  });
}
async function run() {
  exported = Gio.DBusExportedObject.wrapJSObject(XML, {GetState: () => state.snapshot()});
  exported.export(server, PATH);
  id = client.signal_subscribe(server.get_unique_name(), INTERFACE, 'Changed', PATH,
    null, Gio.DBusSignalFlags.NONE, (_c, _s, _p, _i, _m, value) => received.push(value.deep_unpack()));
  check((await read())[3] === true, 'initial blocked state');
  state.setBlocked(false); check((await read())[2] === 1, 'ready readback');
  state.input({get_device_node: () => '/dev/input/event9', get_device_mode: () => 47});
  check((await read())[2] === 2, 'physical generation');
  state.input({get_device_node: () => null, get_device_mode: () => 47});
  check((await read())[2] === 2, 'virtual input must not be physical');
  state.setBlocked(true); state.setBlocked(false);
  check((await read())[2] === 4, 'lock pulse retained');
  state.fail(); state.setBlocked(false);
  const last = await read(); check(last[2] === 5 && last[3], 'fault remains closed');
  check(received.length === 5, `signals: ${received.length}`);
  check(received.every(s => s.length === 4 && s[1] === state.epoch), 'content-free real GVariant');
  exported.unexport(); exported = null;
  let absent = false; try { await read(); } catch { absent = true; }
  check(absent, 'disabled interface remained available');
  const version = GLib.getenv('GROK_CU_TEST_MUTTER_API');
  imports.gi.versions.Clutter = version;
  const Clutter = imports.gi.Clutter;
  check(typeof Clutter.Event.add_filter === 'function', 'native filter GI');
  check(typeof Clutter.Event.remove_filter === 'function', 'native filter cleanup GI');
  check(typeof Clutter.Event.prototype.get_source_device === 'function', 'native source GI');
  check(typeof Clutter.InputDevice.prototype.get_device_node === 'function', 'native device node GI');
  check(typeof Clutter.InputDevice.prototype.get_device_mode === 'function', 'native device mode GI');
  check(Number.isInteger(Clutter.InputMode.PHYSICAL) && Clutter.InputMode.PHYSICAL !== Clutter.InputMode.LOGICAL,
    'direct and aggregate device modes must differ');
  check(typeof GObject.type_name_from_instance === 'function', 'native type identity GI');
  for (const name of ['KEY_PRESS', 'KEY_RELEASE', 'BUTTON_PRESS', 'BUTTON_RELEASE', 'MOTION',
    'SCROLL', 'TOUCH_BEGIN', 'TOUCH_UPDATE', 'TOUCH_END', 'TOUCH_CANCEL', 'TOUCHPAD_PINCH',
    'TOUCHPAD_SWIPE', 'TOUCHPAD_HOLD', 'PROXIMITY_IN', 'PROXIMITY_OUT', 'PAD_BUTTON_PRESS',
    'PAD_BUTTON_RELEASE', 'PAD_STRIP', 'PAD_RING']) check(typeof Clutter.EventType[name] === 'number', name);
  print(JSON.stringify({gjsDbusVerified: true, methods: 7, signals: received.length,
    nativeGiApi: version, physicalDeviceFixture: true, actualGnomeSession: false,
    actualPhysicalInput: false, extensionEnabled: false}));
}
timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 5000, () => {
  printerr('probe deadline'); failed = true; loop.quit(); return GLib.SOURCE_REMOVE;
});
run().catch(error => {printerr(error.stack); failed = true;}).finally(() => {
  GLib.source_remove(timer);
  if (id) client.signal_unsubscribe(id);
  exported?.unexport(); client.close_sync(null); server.close_sync(null); loop.quit();
});
loop.run();
if (failed) throw Error('GJS policy probe failed');

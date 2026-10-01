// Explicit owned-VM experiment only. No physical input or IME acceptance claim.
// Run with the pinned build's GI_TYPELIB_PATH and LD_LIBRARY_PATH, isolated
// HOME/runtime/session bus and in-memory settings. Never in the user's Shell.
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Clutter from 'gi://Clutter?version=14';
import Meta from 'gi://Meta?version=14';
import System from 'system';
import {PhysicalInputWatch} from './physical-input-watch.js';

function assert(condition, message) {
  if (!condition) throw new Error(message);
}
function readText(path) {
  return new TextDecoder().decode(Gio.File.new_for_path(path).load_contents(null)[1]);
}
const uuid = '812400f8-a6c6-4c38-a735-2b8d0ef8d3e2';
assert(GLib.getenv('GROK_CU_OWNED_VM_COUNTER_ABI') === uuid, 'Explicit owned-VM gate');
const marker = Gio.File.new_for_path('/etc/cu-owned-vm-id');
const info = marker.query_info('unix::uid,unix::mode', Gio.FileQueryInfoFlags.NOFOLLOW_SYMLINKS, null);
assert(info.get_attribute_uint32('unix::uid') === 0 &&
       (info.get_attribute_uint32('unix::mode') & 0o7777) === 0o444, 'Owned marker permissions');
assert(new TextDecoder().decode(marker.load_contents(null)[1]) === uuid, 'Owned marker identity');
assert(ARGV.length === 1 && GLib.path_is_absolute(ARGV[0]), 'Explicit built plugin path');
assert(GLib.getenv('GSETTINGS_BACKEND') === 'memory', 'No persistent settings');
const buildRoot = GLib.getenv('GROK_CU_ABI_BUILD_ROOT');
assert(buildRoot !== null && GLib.path_is_absolute(buildRoot), 'Explicit built library root');

const context = Meta.create_context('Computer Use isolated ABI probe');
let watch = null;
let seat = null;
let signalId = null;
let result = null;
try {
  context.set_plugin_name(ARGV[0]);
  context.configure(['counter-gi', '--headless', '--wayland', '--virtual-monitor', '800x600']);
  assert(context.setup(), 'Context setup failed');
  // GI imports are lazy; inspect mappings only after calling the actual API.
  const maps = readText('/proc/self/maps');
  for (const library of ['src/libmutter-14.so.0.0.0',
                         'clutter/clutter/libmutter-clutter-14.so.0.0.0'])
    assert(maps.includes(`${buildRoot}/${library}`), 'Must load actual pinned build: ' + library);
  seat = Clutter.get_default_backend().get_default_seat();
  assert(GObject.type_name(seat.constructor.$gtype) === 'MetaSeatNative', 'Real native seat required');
  const read = () => ({version: seat.grok_physical_input_version,
                       generation: seat.grok_physical_input_generation});
  const initial = read();
  assert(initial.version === 1 && initial.generation === 0, 'Native GI properties unavailable');
  let notifications = 0;
  let advances = 0;
  let faults = 0;
  let disconnected = 0;
  watch = new PhysicalInputWatch({
    read,
    subscribe(callback) {
      signalId = seat.connect('notify::grok-physical-input-generation', () => {
        notifications++;
        callback();
      });
      return signalId;
    },
    disconnect(id) {
      assert(id === signalId, 'Disconnect original signal');
      seat.disconnect(id);
      signalId = null;
      disconnected++;
    },
  }, () => advances++, () => faults++);
  watch.start();
  // Deliberately synthetic notify tests only GI transport. The authoritative
  // native counter must remain unchanged; no false takeover is permitted.
  seat.notify('grok-physical-input-generation');
  watch.poll();
  assert(notifications === 1 && advances === 0 && faults === 0, 'Notify readback contract');
  assert(read().generation === 0, 'Notify is not physical input');
  watch.stop();
  watch.stop();
  seat.notify('grok-physical-input-generation');
  assert(notifications === 1 && disconnected === 1, 'Original subscription retired');
  result = {actualNativeSeat: true, giReadVerified: true, syntheticNotifyOnly: true,
            generation: 0, notifications, advances, faults, disconnected,
            physicalInputVerified: false, imeVerified: false, shellIntegrationVerified: false};
} finally {
  if (watch) watch.stop();
  if (signalId !== null && seat) seat.disconnect(signalId);
  // All GJS wrappers of Clutter objects must release their refs while the
  // Clutter context still exists. Late GC after backend disposal can otherwise
  // finalize a seat/device against a destroyed global Clutter context.
  watch = null;
  seat = null;
  System.gc();
  // Dispose joins Mutter's input thread. The GJS wrapper still owns its ref:
  // meta_context_destroy() also unrefs despite transfer-none GI metadata, so
  // invoking it from GJS would leave a wrapper owning a finalized C object.
  context.run_dispose();
}
assert(result !== null, 'No completed ABI result');
result.contextDisposed = true;
print(JSON.stringify(result));

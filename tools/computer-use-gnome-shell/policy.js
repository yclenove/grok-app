// Shared by the GNOME Shell helper and its deterministic contract tests.
// No keys, text, coordinates, device names or device paths leave this module.
export const VERSION = 1;
export const PATH = '/org/grok/ComputerUse/NativePolicy';
export const INTERFACE = 'org.grok.ComputerUse.NativePolicy1';
export const XML = `<node><interface name="${INTERFACE}">
  <method name="GetState"><arg direction="out" type="u"/><arg direction="out" type="s"/>
    <arg direction="out" type="u"/><arg direction="out" type="b"/></method>
  <signal name="Changed"><arg type="u"/><arg type="s"/><arg type="u"/><arg type="b"/></signal>
</interface></node>`;

export function screenBlocked(shield, mode) {
  const values = [shield?.actor?.visible, shield?.active, shield?.locked,
    mode?.isLocked, mode?.isGreeter, mode?.hasWindows];
  // Ubuntu's normal session inherits 'user' but is named 'ubuntu'. Check its
  // resolved capabilities, not a distro-specific string equality.
  return values.some(v => typeof v !== 'boolean') || values.slice(0, 5).some(Boolean) ||
    !mode.hasWindows;
}

// Mutter 46 native EIS devices ALSO have InputMode.PHYSICAL. The discriminator
// is the compositor-owned device-node property on a direct native device:
// libinput sets /dev/input/eventN; direct native virtual devices leave it null.
// LOGICAL aggregate devices ALSO have a null node, but cannot establish virtual
// provenance (e.g. IME-forwarded keys). Missing/other modes remain unknown loss.
// Never read/open that path, never trust
// a device name, and never interpret an EventFlags.SYNTHETIC bit as libei proof.
export function classifySource(device, physicalMode) {
  if (!Number.isInteger(physicalMode) || physicalMode < 0 || !device ||
      typeof device.get_device_mode !== 'function' ||
      typeof device.get_device_node !== 'function') return 'unknown';
  try {
    if (device.get_device_mode() !== physicalMode) return 'unknown';
    const node = device.get_device_node();
    if (node === null) return 'virtual';
    return typeof node === 'string' && /^\/dev\/input\/event\d+$/.test(node)
      ? 'physical' : 'unknown';
  } catch {
    return 'unknown';
  }
}

// Mutter 46 invokes the core user-active watch synchronously from its event
// filter, while Clutter.get_current_event() still holds the originating event.
// This is NOT idle-time polling: no event means a fault, never physical input.
// Re-arm the one-shot before observing; Mutter snapshots the old watch IDs and
// removes only the fired ID afterwards. The late Clutter filter alone misses
// events consumed by native clients. Earlier capture/IME/pad paths still need
// separate acceptance; this watch is not a claim of complete input coverage.
export class NativeEventWatch {
  constructor(monitor, currentEvent, observe, fault) {
    this._monitor = monitor;
    this._currentEvent = currentEvent;
    this._observe = observe;
    this._fault = fault;
    this._id = null;
    this._token = null;
    this._running = false;
    this._arming = false;
    this._started = false;
  }

  start() {
    if (this._started) throw new Error('Observer already started');
    this._started = true;
    this._running = true;
    try {
      if (typeof this._monitor?.add_user_active_watch !== 'function' ||
          typeof this._monitor?.remove_watch !== 'function')
        throw new Error('Native activity watch unavailable');
      this._arm();
    } catch (error) {
      this._abort();
      throw error;
    }
  }

  _arm() {
    const token = {};
    this._token = token;
    this._arming = true;
    let id;
    try {
      id = this._monitor.add_user_active_watch(() => this._fired(token));
    } finally { this._arming = false; }
    if (!Number.isInteger(id) || id <= 0 || id > 0xffffffff ||
        !this._running || this._token !== token) {
      // Even a wrapped zero ID can have registered a native watch. Remove it.
      if (Number.isInteger(id) && id >= 0 && id <= 0xffffffff)
        this._monitor.remove_watch(id);
      throw new Error('Native activity watch registration failed');
    }
    this._id = id;
  }

  _fired(token) {
    if (!this._running || this._token !== token) return;
    if (this._arming) { this._abort(); return; }
    // The native caller owns removal of this fired one-shot, including faults.
    this._id = null;
    this._token = null;
    try {
      this._arm();
      const event = this._currentEvent();
      if (!event) throw new Error('Native event context unavailable');
      this._observe(event);
    } catch { this._abort(); } // Never throw into Mutter or suppress user input.
  }

  _abort() {
    try { this.stop(); } catch { /* Fault remains sticky even if removal fails. */ }
    try { this._fault(); } catch { /* A dead bus cannot escape into Mutter. */ }
  }

  stop() {
    this._running = false;
    this._token = null;
    const id = this._id;
    this._id = null;
    if (id !== null) this._monitor.remove_watch(id);
  }
}

export class PolicyState {
  constructor(epoch, publish, physicalMode) {
    if (!/^[a-f0-9-]{36}$/.test(epoch)) throw new Error('Invalid helper epoch');
    this.epoch = epoch;
    this.serial = 0;
    this.blocked = true;
    this.failed = false;
    this.publish = publish;
    this.physicalMode = physicalMode;
  }

  snapshot() { return [VERSION, this.epoch, this.serial, this.blocked]; }

  advance() {
    if (this.serial === 0xffffffff) {
      this.failed = true;
      this.blocked = true;
    } else {
      this.serial++;
    }
    try {
      this.publish(this.snapshot());
    } catch {
      // A dead bus must not escape the event filter and interfere with the
      // physical user's event. Readback also fails closed after publish loss.
      this.failed = true;
      this.blocked = true;
    }
  }

  setBlocked(blocked) {
    if (this.failed) return;
    if (typeof blocked !== 'boolean') { this.fail(); return; }
    if (this.blocked === blocked) return;
    this.blocked = blocked;
    this.advance();
  }

  input(device) {
    if (!this.failed && classifySource(device, this.physicalMode) !== 'virtual') this.advance();
  }

  fail() {
    if (this.failed) return;
    this.failed = true;
    this.blocked = true;
    this.advance();
  }
}

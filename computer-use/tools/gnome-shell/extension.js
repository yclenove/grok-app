import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {WindowInventory, screenBlocked} from './inventory.mjs';

const PATH = '/org/grok/ComputerUse/Windows';
const XML = `<node><interface name="org.grok.ComputerUse.Windows1">
  <method name="ListWindows">
    <arg name="version" type="u" direction="out"/>
    <arg name="epoch" type="s" direction="out"/>
    <arg name="windows" type="a(stssub)" direction="out"/>
  </method>
  <method name="Activate">
    <arg name="epoch" type="s" direction="in"/>
    <arg name="id" type="s" direction="in"/>
    <arg name="generation" type="t" direction="in"/>
    <arg name="pid" type="u" direction="in"/>
    <arg name="policyAllowed" type="b" direction="in"/>
    <arg name="accepted" type="b" direction="out"/>
  </method>
</interface></node>`;

// v1's existing Shell bus connection and fail-closed session-blocking check.
// This extension has no capture, input, Eval, grant or settings-mutation RPC.
export default class ComputerUseWindows extends Extension {
    enable() {
        try {
            if (!Meta.is_wayland_compositor()) throw new Error('Native Wayland required');
            this._settings = this.getSettings();
            const blocked = () => screenBlocked(Main.screenShield, Main.sessionMode);
            const inventory = new WindowInventory(GLib.uuid_string_random(),
                () => global.get_window_actors().map(actor => actor.meta_window).filter(Boolean),
                window => ({
                    id: String(window.get_stable_sequence()),
                    title: window.get_title() ?? '',
                    appId: window.get_gtk_application_id() ?? window.get_wm_class() ?? '',
                    pid: Math.max(0, window.get_pid()),
                    peerPid: window.get_client_type() === Meta.WindowClientType.WAYLAND,
                }), window => window.activate(global.get_current_time()), blocked);
            this._exported = Gio.DBusExportedObject.wrapJSObject(XML, {
                ListWindows: () => [1, inventory.epoch, inventory.list()],
                ActivateAsync: (args, invocation) => {
                    try {
                        const accepted = inventory.activate(...args, invocation.get_sender(),
                            this._settings.get_string('activation-owner'));
                        invocation.return_value(new GLib.Variant('(b)', [accepted]));
                    } catch {
                        // Could include a native activation failure: never assert zero effects.
                        invocation.return_dbus_error('org.grok.ComputerUse.Failed', 'Activation unconfirmed');
                    }
                },
            });
            // No separately claimable helper name; clients pin org.gnome.Shell's unique owner.
            this._exported.export(Gio.DBus.session, PATH);
        } catch (error) {
            this.disable();
            throw error;
        }
    }

    disable() {
        try { this._exported?.unexport(); } finally {
            this._exported = null;
            this._settings = null;
        }
    }
}

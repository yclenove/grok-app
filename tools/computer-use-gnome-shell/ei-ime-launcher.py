"""Owned-VM-only native EIS acceptance, never a product authorization route.

Mutter's private test interface is pinned to the actual Shell owner. It is used
only to create one test-owned keyboard source, not as a portal fallback. The
original child, session and bus are all retired before this process returns.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import select
import signal
import socket
import stat
import subprocess
import sys
import time

from gi.repository import Gio, GLib


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--owned-vm-uuid", required=True)
    parser.add_argument("--token", required=True)
    parser.add_argument("--peer", required=True)
    parser.add_argument("--peer-sha256", required=True)
    args = parser.parse_args()
    uuid = "812400f8-a6c6-4c38-a735-2b8d0ef8d3e2"
    assert args.owned_vm_uuid == uuid and os.getuid() == os.geteuid() == 1000
    marker = Path("/etc/cu-owned-vm-id")
    info = marker.lstat()
    assert stat.S_ISREG(info.st_mode) and info.st_uid == 0 and not info.st_mode & 0o222
    # The owned image writes the exact ASCII UUID without a trailing newline.
    assert marker.read_bytes() == uuid.encode("ascii")
    assert re.fullmatch(r"[a-f0-9]{32}", args.token)
    assert re.fullmatch(r"[a-f0-9]{64}", args.peer_sha256)
    runtime = Path("/run/user/1000")
    info = runtime.lstat()
    assert stat.S_ISDIR(info.st_mode) and info.st_uid == 1000 and not info.st_mode & 0o077
    assert os.environ["XDG_RUNTIME_DIR"] == str(runtime)
    assert os.environ["DBUS_SESSION_BUS_ADDRESS"] == "unix:path=/run/user/1000/bus"
    peer = Path(args.peer)
    info = peer.lstat()
    assert peer.is_absolute() and peer.name == "ei-ime-peer"
    assert stat.S_ISREG(info.st_mode) and info.st_uid == 1000 and not info.st_mode & 0o022
    assert hashlib.sha256(peer.read_bytes()).hexdigest() == args.peer_sha256
    endpoint = runtime / ("grok-cu-ime-" + args.token) / "control.sock"
    info = endpoint.lstat()
    assert stat.S_ISSOCK(info.st_mode) and info.st_uid == 1000 and not info.st_mode & 0o077

    def focused():
        with socket.socket(socket.AF_UNIX) as connection:
            connection.settimeout(2)
            connection.connect(str(endpoint))
            connection.sendall(json.dumps({"token": args.token, "method": "snapshot"}).encode())
            with connection.makefile("r") as stream:
                state = json.loads(stream.readline(10001))
        assert all(state[k] is True for k in ("nativeWayland", "windowActive", "entryFocused"))
        assert state["engine"] == "libpinyin" and state["imeModule"] == "wayland"
        assert state["policy"][3] is False
        return state["owner"], state["pid"]

    identity = focused()
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    remote = "org.gnome.Mutter.RemoteDesktop"
    iface = remote + ".Session"
    owner = None
    shell_pid = None
    session = None
    child = None
    raw_fd = None
    closed = []
    sub = None
    sent = 0
    stop_requested = False
    error = None
    cleanup_errors = []
    joined = False
    session_closed = False
    endpoint_absent = False
    child_summary = None
    child_code = None
    connection_owner = bus.get_unique_name()

    def call(destination, path, interface, method, parameters=None):
        return bus.call_sync(destination, path, interface, method, parameters, None,
                             Gio.DBusCallFlags.NONE, 3000, None).unpack()

    def bus_call(method, value):
        return call("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
                    method, GLib.Variant("(s)", (value,)))[0]

    def reply(timeout=5):
        assert select.select([child.stdout], [], [], timeout)[0], "original EI peer response timeout"
        line = child.stdout.readline(10001)
        assert line.endswith("\n") and len(line) <= 10000, "invalid bounded EI peer response"
        return json.loads(line)

    def cancel(signum, frame):
        del signum, frame
        raise InterruptedError("owned EI launcher cancelled")

    signal.signal(signal.SIGTERM, cancel)
    signal.signal(signal.SIGINT, cancel)
    try:
        owner = bus_call("GetNameOwner", remote)
        assert owner == bus_call("GetNameOwner", "org.gnome.Shell") == identity[0]
        assert bus_call("GetConnectionUnixUser", owner) == 1000
        shell_pid = bus_call("GetConnectionUnixProcessID", owner)
        assert Path(f"/proc/{shell_pid}/comm").read_text().strip() == "gnome-shell"
        session = call(owner, "/org/gnome/Mutter/RemoteDesktop", remote, "CreateSession")[0]
        assert re.fullmatch(r"/org/gnome/Mutter/RemoteDesktop/Session/u[0-9]+", session)
        sub = bus.signal_subscribe(owner, iface, "Closed", session, None, Gio.DBusSignalFlags.NONE,
                                   lambda *unused: closed.append(True))
        call(owner, session, iface, "Start")
        response, fd_list = bus.call_with_unix_fd_list_sync(owner, session, iface, "ConnectToEIS",
            GLib.Variant("(a{sv})", ({"device-types": GLib.Variant("u", 1)},)),
            GLib.VariantType.new("(h)"), Gio.DBusCallFlags.NONE, 3000, None, None)
        assert fd_list.get_length() == 1 and response.unpack() == (0,)
        raw_fd = fd_list.get(0)
        child = subprocess.Popen([str(peer), "--owned-vm-uuid", uuid, "--fd", str(raw_fd)],
                                 pass_fds=(raw_fd,), stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                 stderr=sys.stderr, text=True, bufsize=1)
        os.close(raw_fd)
        raw_fd = None
        assert reply(12) == {"eiReady": True, "keyboardResumed": True}
        assert focused() == identity
        print(json.dumps({"nativeEisReady": True, "sameShellOwner": True,
                          "keyboardOnly": True, "portalConsentVerified": False}), flush=True)
        deadline = time.monotonic() + 75
        while not stop_requested:
            assert time.monotonic() < deadline, "native EIS command deadline"
            if not select.select([sys.stdin], [], [], 1)[0]:
                assert child.poll() is None, "original native EI peer exited"
                continue
            line = sys.stdin.readline(10001)
            assert line.endswith("\n") and len(line) <= 10000, "invalid bounded EIS command"
            request = json.loads(line)
            if request == {"stop": True}:
                stop_requested = True
                break
            assert isinstance(request, dict) and set(request) == {"edge"}
            assert type(request["edge"]) is int and request["edge"] == sent and sent < 6
            assert focused() == identity
            child.stdin.write(str(sent) + "\n")
            child.stdin.flush()
            assert reply() == {"edgeSent": sent}
            print(json.dumps({"edgeSent": sent}), flush=True)
            sent += 1
    except BaseException as failure:
        error = type(failure).__name__ + ": " + str(failure)
    finally:
        if raw_fd is not None:
            os.close(raw_fd)
        if child is not None:
            try:
                if child.poll() is None:
                    child.stdin.write("stop\n")
                    child.stdin.flush()
                    child_summary = reply()
                child.stdin.close()
                code = child.wait(timeout=5)
                child_code = code
                joined = True
                assert code == 0 and child_summary == {"eiStopped": True,
                    "allSixEdgesSent": sent == 6, "exitCode": 0}
            except BaseException as failure:
                cleanup_errors.append("peer: " + repr(failure))
                if child.poll() is None:
                    child.terminate()
                    try:
                        child.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.wait(timeout=5)
                joined = child.poll() is not None
                child_code = child.returncode
        if session is not None:
            try:
                call(owner, session, iface, "Stop")
                deadline = time.monotonic() + 3
                context = GLib.MainContext.default()
                while not closed and time.monotonic() < deadline:
                    while context.pending():
                        context.iteration(False)
                    time.sleep(.01)
                session_closed = bool(closed)
                try:
                    call(owner, session, "org.freedesktop.DBus.Properties", "Get",
                         GLib.Variant("(ss)", (iface, "SessionId")))
                except GLib.Error as failure:
                    endpoint_absent = Gio.DBusError.get_remote_error(failure) in (
                        "org.freedesktop.DBus.Error.UnknownMethod", "org.freedesktop.DBus.Error.UnknownObject")
                assert session_closed and endpoint_absent
            except BaseException as failure:
                cleanup_errors.append("session: " + repr(failure))
        if sub is not None:
            bus.signal_unsubscribe(sub)
        bus.close_sync(None)  # Original unique owner vanishes even on failed Stop.
    output = {"nativeEisFinished": True, "originalPeerJoined": joined,
              "nativeSessionClosedSignal": session_closed, "nativeSessionEndpointAbsent": endpoint_absent,
              "sixEdgesSent": sent == 6, "stopRequested": stop_requested,
              "error": error, "cleanupErrors": cleanup_errors,
              "shellOwner": owner, "shellPid": shell_pid, "sessionPath": session,
              "connectionOwner": connection_owner,
              "peerPid": child.pid if child is not None else None,
              "peerExitCode": child_code, "peerStopped": child_summary,
              "peerSha256": args.peer_sha256,
              "launcherSha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              "portalConsentVerified": False, "appGrantRevocationVerified": False}
    print(json.dumps(output), flush=True)
    return 0 if not error and not cleanup_errors and joined and session_closed and endpoint_absent else 1


if __name__ == "__main__":
    raise SystemExit(main())

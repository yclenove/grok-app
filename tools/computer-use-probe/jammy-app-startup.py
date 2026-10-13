#!/usr/bin/env python3
"""Actual release App/window/private-library/normal-close check, not portal/CU acceptance."""
import ctypes
import hashlib
import json
import os
from pathlib import Path
import select
import shutil
import subprocess
import sys
import time


EXPECTED_SHA = "ef76f1d9996b6ed9e59e9bf1de65e6c8932931d1342dc3dbc32edf89bb1aa0e2"


def identity(pid):
    raw = Path(f"/proc/{pid}/stat").read_text()
    fields = raw[raw.rfind(")") + 2:].split()
    return int(fields[1]), fields[19]


def descendants(parent):
    records = {}
    for item in Path("/proc").iterdir():
        if not item.name.isdigit():
            continue
        try:
            records[int(item.name)] = identity(int(item.name))
        except (FileNotFoundError, ProcessLookupError):
            pass
    owned = {parent}
    while True:
        found = {pid for pid, (ppid, _) in records.items() if ppid in owned}
        if found.issubset(owned):
            break
        owned |= found
    return {pid: records[pid][1] for pid in owned if pid in records}


def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage: jammy-app-startup.py /work/product-app-03 /work/new-startup-evidence")
    source, evidence = map(Path, sys.argv[1:])
    if str(source) != "/work/product-app-03" or source.is_symlink() or source.resolve() != source:
        raise SystemExit("unexpected frozen source")
    if evidence.parent != Path("/work") or not evidence.name.startswith("app-startup-"):
        raise SystemExit("unexpected evidence root")
    if evidence.exists() or evidence.is_symlink():
        raise SystemExit("refusing to overwrite evidence")
    exe = Path("/work/product-app-01/src-tauri/target/release/grok-app")
    if exe.is_symlink() or hashlib.sha256(exe.read_bytes()).hexdigest() != EXPECTED_SHA:
        raise SystemExit("release App differs from the verified candidate")
    os_release = Path("/etc/os-release").read_text()
    if 'VERSION_ID="22.04"' not in os_release:
        raise SystemExit("Jammy required")
    if ctypes.CDLL(None).prctl(36, 1, 0, 0, 0) != 0:
        raise SystemExit("cannot own/reap App descendants")
    evidence.mkdir(mode=0o700)
    settings = evidence / "app-home"
    settings.mkdir()
    probe = Path(__file__).resolve().parent
    shutil.copyfile(probe / "jammy-app-startup-settings.json", settings / "settings.json")
    helper = evidence / "app-window"
    flags = subprocess.check_output(["pkg-config", "--cflags", "--libs", "gtk+-3.0", "x11"], text=True).split()
    subprocess.run(["cc", "-Wall", "-Wextra", "-Werror", "-o", str(helper),
                    str(probe / "jammy-app-window.c"), *flags], check=True)
    environment = dict(os.environ, GROK_APP_HOME=str(settings),
                       GROK_APP_INSTANCE_ID="com.grokapp.desktop.jammy-source03-startup",
                       XDG_CONFIG_HOME=str(evidence / "config"), XDG_CACHE_HOME=str(evidence / "cache"),
                       XDG_DATA_HOME=str(evidence / "data"), GDK_BACKEND="x11")
    for variable in ("GROK_APP_ACP", "GROK_CU_PIPEWIRE_RUNTIME", "SPA_PLUGIN_DIR", "PIPEWIRE_MODULE_DIR",
                     "PIPEWIRE_CONFIG_DIR", "PIPEWIRE_CONFIG_PREFIX", "PIPEWIRE_CONFIG_NAME",
                     "WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS"):
        environment.pop(variable, None)
    # Challenge the actual loader with the system directory ahead of other paths.
    environment["LD_LIBRARY_PATH"] = "/usr/lib/x86_64-linux-gnu"
    report = {"scope": "same release App Xvfb startup/normal close; not installed/portal/Grok acceptance",
              "appSha256": EXPECTED_SHA, "sandboxDisabled": False, "modelTaskSent": False,
              "passed": False, "owners": []}
    held = {}
    app = None

    def observe():
        for pid, stamp in descendants(app.pid).items():
            if pid in held:
                continue
            try:
                descriptor = os.pidfd_open(pid)
                if identity(pid)[1] != stamp:
                    os.close(descriptor)
                    raise RuntimeError("process identity changed during enrollment")
                try:
                    executable = os.readlink(f"/proc/{pid}/exe")
                except FileNotFoundError:
                    executable = "retiring"
                held[pid] = (descriptor, stamp, executable)
            except ProcessLookupError:
                pass

    def collect_for(seconds):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            observe()
            if app.poll() is not None:
                raise RuntimeError(f"App exited before normal close: {app.returncode}")
            time.sleep(0.1)

    try:
        with (evidence / "app.stderr.log").open("wb") as log:
            app = subprocess.Popen([str(exe)], cwd=source, env=environment, stdout=log, stderr=subprocess.STDOUT)
            report["pid"] = app.pid
            report["creationTicks"] = identity(app.pid)[1]
            collect_for(12)
            maps = Path(f"/proc/{app.pid}/maps").read_text()
            (evidence / "app.maps.txt").write_text(maps)
            private = exe.parent / "resources/computer-use/native-pipewire/runtime/lib/libpipewire-0.3.so.0"
            paths = {line.split(maxsplit=5)[5] for line in maps.splitlines()
                     if len(line.split(maxsplit=5)) == 6 and "libpipewire-0.3.so" in line}
            if paths != {str(private)}:
                raise RuntimeError(f"actual PipeWire mapping differs: {paths}")
            report["actualPipeWirePath"] = str(private)
            subprocess.run([str(helper), "capture", str(app.pid), str(evidence / "window.png")],
                           env=environment, check=True)
            collect_for(1)
            close_started = time.monotonic()
            subprocess.run([str(helper), "close", str(app.pid), "unused"], env=environment, check=True)
            deadline = close_started + 30
            while app.poll() is None and time.monotonic() < deadline:
                observe()
                time.sleep(0.1)
            if app.poll() is None:
                raise RuntimeError("normal close exceeded 30s; exact owners retained, no kill")
            report["exitCode"] = app.wait()
            report["normalCloseMs"] = round((time.monotonic() - close_started) * 1000)
            if report["exitCode"] != 0:
                raise RuntimeError(f"normal close exit code {report['exitCode']}")
            while time.monotonic() < deadline:
                for pid, (descriptor, _, _) in held.items():
                    if pid != app.pid and not select.select([descriptor], [], [], 0)[0]:
                        break
                else:
                    break
                time.sleep(0.1)
            all_retired = True
            for pid, (descriptor, stamp, executable) in held.items():
                retired = bool(select.select([descriptor], [], [], 0)[0])
                all_retired &= retired
                report["owners"].append({"pid": pid, "creationTicks": stamp,
                                         "executable": executable, "originalPidfdSignalled": retired})
            if not all_retired:
                raise RuntimeError("original App descendant still live after normal close")
            app_log = (evidence / "app.stderr.log").read_text(errors="replace")
            report["frontendQuitTimeoutFallback"] = "frontend did not complete quit" in app_log
            if report["frontendQuitTimeoutFallback"]:
                raise RuntimeError("host timeout, not frontend normal quit")
            report["passed"] = True
    except Exception as error:
        report["error"] = str(error)
        if app is not None:
            report["appStillLive"] = app.poll() is None
        raise
    finally:
        (evidence / "results.json").write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report, indent=2), flush=True)
        if app is not None and app.poll() is None:
            # Keep the private X/DBus parents alive for investigation; no force quit.
            print("Retained original App and private display owners after failed gate.", flush=True)
            app.wait()
        for descriptor, _, _ in held.values():
            os.close(descriptor)
        while True:
            try:
                if os.waitpid(-1, os.WNOHANG)[0] == 0:
                    break
            except ChildProcessError:
                break


if __name__ == "__main__":
    main()

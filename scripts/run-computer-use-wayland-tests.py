#!/usr/bin/env python3
"""Run the complete native Wayland suite against an owned headless compositor.

Use the adjacent shell entry point. This is a private protocol/GTK fixture,
not an installed App, GNOME permission dialog or end-user input-effect test.
All direct children are retained and joined; a timeout never becomes a pass.
"""
import argparse
import contextlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import tempfile
import time


PARENT_TEST = (
    "tests::native::host::parent::"
    "gtk_parent_registry_joins_original_ui_and_native_input_owners"
)


def executable(value):
    found = shutil.which(value)
    if not found:
        raise ValueError("Required executable missing: " + value)
    return str(Path(found).resolve(strict=True))


def fixture_environment(runtime, xwayland):
    env = dict(os.environ)
    for key in ("DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET",
                "DBUS_SESSION_BUS_ADDRESS", "AT_SPI_BUS_ADDRESS",
                "SESSION_MANAGER", "XAUTHORITY"):
        env.pop(key, None)
    env.update(
        XDG_RUNTIME_DIR=str(runtime), HOME=str(runtime / "home"),
        XDG_CONFIG_HOME=str(runtime / "config"),
        XDG_CONFIG_DIRS=str(runtime / "config"),
        XDG_CACHE_HOME=str(runtime / "cache"), XDG_DATA_HOME=str(runtime / "data"),
        WLR_XWAYLAND=xwayland, WLR_BACKENDS="headless", WLR_RENDERER="pixman",
        WLR_LIBINPUT_NO_DEVICES="1", WLR_HEADLESS_OUTPUTS="1",
        NO_AT_BRIDGE="1", GTK_A11Y="none", GDK_BACKEND="wayland",
        GROK_CU_PARENT_FIXTURE="owned-labwc", G_DEBUG="fatal-criticals",
        WAYLAND_DEBUG="client", TMPDIR="/var/tmp",
    )
    return env


def verify_suite(output, diagnostics=""):
    # Reject filtered/ignored/zero-test greens and require the GTK integration.
    results = re.findall(
        r"test result: ok\. (\d+) passed; 0 failed; 0 ignored; "
        r"0 measured; 0 filtered out", output)
    if len(results) != 1 or int(results[0]) == 0:
        raise RuntimeError("Expected a complete, non-empty, unfiltered native suite")
    if not re.search(r"test " + re.escape(PARENT_TEST) + r" \.\.\. ok", output):
        raise RuntimeError("Original UI-owner integration did not pass")
    # libtest can report green even if a detached executor thread panics.
    # Its diagnostics are on stderr, alongside WAYLAND_DEBUG, not stdout.
    if re.search(r"FAILED|panicked at|SIGSEGV", output + "\n" + diagnostics):
        raise RuntimeError("Native suite contains a failure")
    return int(results[0])


def join_child(child):
    if child.poll() is None:
        os.killpg(child.pid, signal.SIGTERM)
    try:
        child.wait(timeout=5)
    except subprocess.TimeoutExpired:
        os.killpg(child.pid, signal.SIGKILL)
        child.wait(timeout=5)


def reap_orphans():
    # Called only as PID 1 after retained direct children have been joined.
    reaped = []
    while True:
        try:
            pid, status = os.waitpid(-1, os.WNOHANG)
        except ChildProcessError:
            break
        if pid == 0:
            break
        reaped.append({"pid": pid, "waitStatus": status})
    return reaped


def interrupted(signum, _frame):
    raise InterruptedError("Fixture interrupted by signal " + str(signum))


def archive_fixtures(out):
    text = "\n".join(p.read_text(errors="replace") for p in
                     (out / "wayland-tests.log", out / "wayland-wire.log") if p.exists())
    paths = sorted(set(re.findall(r"/var/tmp/grok-cu-(?:pw|eis)-[0-9a-f]{32}", text)))
    archive = out / "fixtures"
    archive.mkdir()
    names = ("owned.conf", "source.stdout", "source.stderr", "daemon.stdout",
             "daemon.stderr", "events.jsonl", "stderr.log", "host-observations.json",
             "parent-registry.json", "registry-broker.json", "authorization-broker.json",
             "adapter-broker.json", "wrapped-broker.json", "retirement-first.json",
             "retirement-second.json", "retirement-audit.json", "policy-revocation.json",
             "gnome-owner-revocation.json", "keyboard-without-pointer.json")
    missing = []
    for path in paths:
        folder = Path(path)
        if folder.is_symlink() or not folder.is_dir():
            missing.append(path)
            continue
        destination = archive / folder.name
        destination.mkdir()
        for name in names:
            source = folder / name
            if source.is_symlink():
                raise RuntimeError("Refusing symlink in owned fixture: " + str(source))
            if source.is_file():
                shutil.copyfile(source, destination / name)
    audit = {"archived": len(paths) - len(missing), "paths": paths, "missing": missing}
    (out / "fixture-archive.json").write_text(json.dumps(audit, indent=2) + "\n")
    if missing:
        raise RuntimeError("Missing native fixture directories: " + repr(missing))
    return audit


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--test-binary", required=True)
    parser.add_argument("--evidence", required=True, type=Path)
    parser.add_argument("--labwc", default="labwc")
    parser.add_argument("--xwayland", default="Xwayland")
    parser.add_argument("--test-threads", type=int, choices=range(1, 17), default=1)
    parser.add_argument("--timeout", type=float, default=180)
    args = parser.parse_args()
    if os.getpid() != 1 or not Path("/proc/1").exists():
        parser.error("Use run-computer-use-wayland-tests.sh; a private PID namespace is required")
    if not 0 < args.timeout <= 600:
        parser.error("timeout must be within (0, 600] seconds")
    binary, labwc, xwayland = map(executable, (args.test_binary, args.labwc, args.xwayland))
    for key in ("GROK_CU_PW_SOURCE", "GROK_CU_EIS_SERVER"):
        executable(os.environ.get(key, ""))
    out = args.evidence.resolve()
    out.mkdir(parents=True, exist_ok=False)
    runtime = Path(tempfile.mkdtemp(prefix="grok-cu-parent-runtime.", dir="/var/tmp"))
    runtime.chmod(0o700)
    for folder in ("home", "config", "cache", "data"):
        (runtime / folder).mkdir()
    for filename in ("autostart", "environment"):
        (runtime / "config" / filename).write_text("")
    (runtime / "config" / "rc.xml").write_text(
        "<labwc_config><core><decoration>server</decoration></core></labwc_config>")
    env = fixture_environment(runtime, xwayland)
    children = []
    passed = None
    failure = None
    for signum in (signal.SIGTERM, signal.SIGINT):
        signal.signal(signum, interrupted)
    with contextlib.ExitStack() as files:
        def log(name):
            return files.enter_context((out / name).open("w"))

        try:
            compositor = subprocess.Popen(
                [labwc, "-C", str(runtime / "config")], env=env,
                stdin=subprocess.DEVNULL, stdout=log("labwc.stdout"),
                stderr=log("labwc.stderr"), start_new_session=True)
            children.append(compositor)
            deadline = time.monotonic() + 10
            while True:
                sockets = [p for p in runtime.glob("wayland-*") if p.is_socket()]
                if compositor.poll() is not None:
                    raise RuntimeError("Owned compositor exited before readiness")
                if sockets:
                    if len(sockets) != 1:
                        raise RuntimeError("Ambiguous owned compositor socket")
                    break
                if time.monotonic() >= deadline:
                    raise TimeoutError("Owned compositor startup deadline")
                time.sleep(.02)
            env["WAYLAND_DISPLAY"] = sockets[0].name
            test = subprocess.Popen(
                [binary, "--include-ignored", "--test-threads=" + str(args.test_threads), "--nocapture"],
                env=env, stdin=subprocess.DEVNULL, stdout=log("wayland-tests.log"),
                stderr=log("wayland-wire.log"), start_new_session=True)
            children.append(test)
            code = test.wait(timeout=args.timeout)
            if code != 0:
                raise RuntimeError("Native suite exited with " + str(code))
            if compositor.poll() is not None:
                raise RuntimeError("Owned compositor died during tests")
            passed = verify_suite((out / "wayland-tests.log").read_text(),
                                  (out / "wayland-wire.log").read_text())
        except BaseException as error:
            failure = type(error).__name__ + ": " + str(error)
        finally:
            # A second cancellation must not skip joining the original owners.
            for signum in (signal.SIGTERM, signal.SIGINT):
                signal.signal(signum, signal.SIG_IGN)
            cleanup_errors = []
            for child in reversed(children):
                try:
                    join_child(child)
                except BaseException as error:
                    cleanup_errors.append(str(error))
            reaped = reap_orphans()
            # /proc is private to this PID namespace. Never inspect/signal a
            # logged-in user's processes. Any remaining descendant is a fail;
            # the kernel reclaims it when this namespace's PID 1 exits.
            live = sorted(int(p.name) for p in Path("/proc").iterdir()
                          if p.name.isdigit() and int(p.name) != 1)
            cleanup = {
                "runtime": str(runtime), "privatePidNamespace": True,
                "ownedChildrenJoined": all(c.returncode is not None for c in children),
                "children": [{"pid": c.pid, "command": c.args, "returncode": c.returncode}
                             for c in children],
                "reapedOrphans": reaped, "live": live, "errors": cleanup_errors,
            }
            (out / "cleanup.json").write_text(json.dumps(cleanup, indent=2) + "\n")
            if live or cleanup_errors:
                failure = (failure or "") + "; original descendants not fully joined"
    try:
        archive_fixtures(out)
    except BaseException as error:
        failure = (failure or "") + "; fixture archive: " + str(error)
    result = {"passed": passed, "failure": failure, "fullGoalComplete": False,
              "actualGnome": False, "installedApp": False, "evidence": str(out)}
    (out / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result))
    return 1 if failure else 0


if __name__ == "__main__":
    raise SystemExit(main())

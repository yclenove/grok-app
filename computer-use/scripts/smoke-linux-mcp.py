#!/usr/bin/env python3
"""End-to-end Linux smoke: drive gcu-fixture through `gcu mcp` (X11 route).

Run inside a desktop session (CI: dbus-run-session + xvfb-run + openbox).
Starts the AT-SPI bus and the fixture, then lists apps, observes, clicks a
button and a checkbox, sets and types text, and checks the fixture state.
Text fields are required: type_text focuses egui (AccessKit) fields and uses the keyboard.
Needs `gcu` built with `native-x11,eval-autoapprove` (debug) and gcu-fixture.
"""
import json, os, re, shutil, subprocess, sys, time

target = os.environ.get("CARGO_TARGET_DIR", os.path.join(os.path.dirname(__file__), "..", "target"))
binaries = os.path.join(target, "debug")
state = "/tmp/mcp_fixture_state.json"
env = dict(os.environ, GCU_FIXTURE_STATE=state, GCU_APPROVAL="auto-allow-session")
if os.path.exists(state):
    os.remove(state)
if shutil.which("openbox") and not subprocess.run(["pgrep", "-x", "openbox"], capture_output=True).stdout:
    subprocess.Popen(["openbox"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
for launcher in ["/usr/libexec/at-spi-bus-launcher", "/usr/lib/at-spi2-core/at-spi-bus-launcher"]:
    if os.path.exists(launcher):
        subprocess.Popen([launcher, "--launch-immediately", "--a11y=1", "--screen-reader=1"], env=env)
        break
time.sleep(2)
fixture = subprocess.Popen([os.path.join(binaries, "gcu-fixture")], env=env,
                           stdout=open("/tmp/mcp_fixture.log", "w"), stderr=subprocess.STDOUT)
for _ in range(60):
    if os.path.exists(state):
        break
    time.sleep(0.5)
gcu = subprocess.Popen([os.path.join(binaries, "gcu"), "mcp"], env=env, text=True,
                       stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=open("/tmp/mcp_gcu.err", "w"))
next_id = [0]


def send(message):
    gcu.stdin.write(json.dumps(message) + "\n")
    gcu.stdin.flush()


def wait(request_id):
    while True:
        line = gcu.stdout.readline()
        if not line:
            return None
        reply = json.loads(line)
        if reply.get("id") == request_id:
            return reply


def call(name, arguments):
    next_id[0] += 1
    send({"jsonrpc": "2.0", "id": next_id[0], "method": "tools/call",
          "params": {"name": name, "arguments": arguments}})
    result = (wait(next_id[0]) or {}).get("result", {})
    text = " ".join(c.get("text", "") for c in result.get("content", []) if c.get("type") == "text")
    print(f"{name}: error={result.get('isError')} :: {text[:200]!r}")
    return result.get("isError"), text


send({"jsonrpc": "2.0", "id": 0, "method": "initialize",
      "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "smoke", "version": "0"}}})
if wait(0) is None:
    sys.exit("gcu mcp did not answer initialize")
send({"jsonrpc": "2.0", "method": "notifications/initialized"})
call("list_apps", {})
error, tree = call("get_app_state", {"app": "gcu-fixture", "include_screenshot": True})
if error:
    sys.exit("get_app_state failed")


def index(name):
    for line in tree.split("\n"):
        match = re.match(r"\s*(\d+) .*\b" + name + r"\b", line)
        if match:
            return int(match.group(1))
    sys.exit(f"{name} not found in the tree")


call("click", {"app": "gcu-fixture", "element_index": index("counter_button")})
call("click", {"app": "gcu-fixture", "element_index": index("agree_checkbox")})
# AccessKit (egui) has no AT-SPI EditableText: set_value is not_settable there, and
# type_text focuses the field over AT-SPI and types with the keyboard instead.
call("set_value", {"app": "gcu-fixture", "element_index": index("name_field"), "value": "Zhang"})
call("type_text", {"app": "gcu-fixture", "element_index": index("name_field"), "text": "Zhang"})
call("type_text", {"app": "gcu-fixture", "element_index": index("multiline_field"), "text": "hello"})
time.sleep(2)
final = json.load(open(state))
gcu.stdin.close()
time.sleep(1)
gcu.kill()
fixture.kill()
required = {"counter": 1, "checked": True, "text": "Zhang", "multiline": "hello"}
got = {key: final.get(key) for key in ["counter", "checked", "text", "multiline"]}
print("fixture state:", got)
if any(got[key] != value for key, value in required.items()):
    print(open("/tmp/mcp_gcu.err").read()[-2000:])
    sys.exit(f"fixture state mismatch: expected {required}")
print("Linux MCP smoke passed (click, toggle, type into egui fields, observe)")

#!/usr/bin/env bash
# Linux stub tests for s11-0-probe.sh. No real tart, softnet, or VM.
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
PROBE="$SCRIPT_DIR/s11-0-probe.sh"
ROOT=$(mktemp -d)
STUBS="$ROOT/stubs"
PASS=0
FAIL=0
KEEP_PIDS="$ROOT/pids"

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  FAIL=$((FAIL + 1))
}

ok() {
  printf 'ok: %s\n' "$1"
  PASS=$((PASS + 1))
}

cleanup_suite() {
  local pid=""
  if [[ -f "$KEEP_PIDS" ]]; then
    while read -r pid; do
      [[ -n "$pid" ]] || continue
      kill -KILL "$pid" 2>/dev/null || true
    done < "$KEEP_PIDS"
  fi
  rm -rf "$ROOT"
}
trap cleanup_suite EXIT

mkdir -p "$STUBS"
cat > "$STUBS/tart" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
log=${S11_TART_LOG:-/dev/null}
printf '%s\n' "$*" >> "$log"
cmd=${1:-}
shift || true
case "$cmd" in
  --version)
    printf '%s\n' "2.41.1"
    exit 0
    ;;
  --help)
    printf '%s\n' "commands: clone create delete exec get ip list login logout prune pull push rename run set stop suspend version"
    printf '%s\n' "softnet agent"
    exit 0
    ;;
  list)
    printf '%s %s\n' "${S11_GOLDEN_NAME:-macos-s2-eval}" "${S11_GOLDEN_STATE:-stopped}"
    exit 0
    ;;
  clone)
    printf '%s\n' "${2:-}" >> "${S11_CLONED:-/dev/null}"
    exit 0
    ;;
  run)
    if [[ "${1:-}" == "--help" ]]; then
      printf '%s\n' "run softnet"
      exit 0
    fi
    if [[ "${S11_TART_RUN_EXIT:-0}" == 1 ]]; then
      exit 1
    fi
    printf '%s\n' "$$" >> "${S11_RUN_PIDS:-/dev/null}"
    exec sleep "${S11_RUN_SLEEP:-300}"
    ;;
  ip)
    if [[ "${S11_TART_IP_FAIL:-0}" == 1 ]]; then
      printf '%s\n' "unavailable" >&2
      exit 1
    fi
    printf '%s\n' "203.0.113.50"
    exit 0
    ;;
  stop|delete)
    name=${1:-}
    case "$name" in
      s11-0-probe-*) ;;
      *)
        printf 'BAD %s %s\n' "$cmd" "$name" >> "$log"
        exit 9
        ;;
    esac
    if [[ "$cmd" == "delete" ]]; then
      printf '%s\n' "$name" >> "${S11_DELETED:-/dev/null}"
    fi
    exit 0
    ;;
  exec)
    if [[ "${1:-}" == "--help" ]]; then
      printf '%s\n' "exec agent"
      exit 0
    fi
    exit 0
    ;;
  *)
    exit 0
    ;;
esac
EOF

cat > "$STUBS/ssh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${S11_SSH_FAIL:-0}" == 1 ]]; then
  exit 1
fi
args=()
take=0
for a in "$@"; do
  if [[ "$take" -eq 1 ]]; then
    args+=("$a")
  elif [[ "$a" == "--guest" ]]; then
    take=1
    args+=("--guest")
  fi
done
if [[ "$take" -eq 1 ]]; then
  exec bash -s -- "${args[@]}"
fi
exit 0
EOF

cat > "$STUBS/nc" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ -n "${S11_NC_LOG:-}" ]]; then
  printf '%s\n' "$*" >> "$S11_NC_LOG"
fi
cat >/dev/null || true
udp=0
v6=0
port=""
for a in "$@"; do
  case "$a" in
    -u) udp=1 ;;
    -6) v6=1 ;;
    ''|*[!0-9]*) ;;
    *) port=$a ;;
  esac
done
if [[ "$v6" -eq 1 ]]; then
  printf '%s\n' "No route to host" >&2
  exit 1
fi
if [[ "$udp" -eq 1 && "$port" == "443" ]]; then
  printf '%s\n' "reply"
  exit 0
fi
if [[ "$udp" -eq 1 ]]; then
  printf '%s\n' "Connection refused" >&2
  exit 1
fi
exit 1
EOF

cat > "$STUBS/ping" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF

cat > "$STUBS/route" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "gateway: 203.0.113.77"
exit 0
EOF

cat > "$STUBS/netstat" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "default 203.0.113.77 UGSc en0"
exit 0
EOF

cat > "$STUBS/ifconfig" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "lo0: flags=8049"
printf '%s\n' "inet6 ::1 prefixlen 128"
printf '%s\n' "inet6 fe80::1%lo0 prefixlen 64"
exit 0
EOF

cat > "$STUBS/ioreg" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "IOPlatformSerialNumber NOTAREALSERIAL123"
printf '%s\n' "AppleVirtIO vsock"
exit 0
EOF

cat > "$STUBS/system_profiler" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "Serial Number: NOTAREALSERIAL456"
printf '%s\n' "virtio-net"
exit 0
EOF

cat > "$STUBS/ls" <<'EOF'
#!/usr/bin/env bash
if [[ "${S11_SOFTNET_SUID:-0}" == 1 ]]; then
  for a in "$@"; do
    case "$a" in
      *softnet*)
        printf '%s\n' "-rwsr-xr-x 1 root wheel 123 Jan 1 00:00 softnet"
        exit 0
        ;;
    esac
  done
fi
exec /bin/ls "$@"
EOF

cat > "$STUBS/df" <<'EOF'
#!/usr/bin/env bash
avail=${S11_DF_AVAIL:-99999999}
printf '%s\n' "Filesystem 1024-blocks Used Available Capacity Mounted on"
printf '%s\n' "stub 1 1 ${avail} 1% /stub"
exit 0
EOF

cat > "$STUBS/id" <<'EOF'
#!/usr/bin/env bash
if [[ -n "${S11_FAKE_EUID:-}" && "${1:-}" == "-u" ]]; then
  printf '%s\n' "$S11_FAKE_EUID"
  exit 0
fi
exec /usr/bin/id "$@"
EOF

chmod 755 "$STUBS/"*

note_pids() {
  local f=$1 p=""
  [[ -f "$f" ]] || return 0
  while read -r p; do
    [[ -n "$p" ]] || continue
    printf '%s\n' "$p" >> "$KEEP_PIDS"
  done < "$f"
}

reap() {
  local f=$1 p=""
  [[ -f "$f" ]] || return 0
  while read -r p; do
    [[ -n "$p" ]] || continue
    if kill -0 "$p" 2>/dev/null; then
      kill -KILL "$p" 2>/dev/null || true
      return 1
    fi
  done < "$f"
  return 0
}

assert_no_leak() {
  local f=$1
  python3 -I - "$f" <<'PY'
import re, sys
path = sys.argv[1]
raw = open(path, encoding="utf-8").read()
low = raw.lower()
banned = (
    "/users/",
    "password",
    "passwd",
    "secret",
    "token",
    "private key",
    "id_ed25519",
    ".ssh",
    "169.254",
    "192.0.2",
    "1.1.1.1",
    "203.0.113",
    "198.51.100",
    "2606:",
    "notarealserial",
    "fe80:",
    "ip-redacted",
)
for s in banned:
    if s in low or s in raw:
        raise SystemExit("leak %s in %s" % (s, path))
scrub = raw.replace("0.0.0.0/0", "")
if re.search(r"\d{1,3}(?:\.\d{1,3}){3}", scrub):
    raise SystemExit("ipv4 leak in %s" % path)
PY
}

assert_schema() {
  local f=$1 status=$2
  python3 -I -m json.tool "$f" >/dev/null
  python3 -I - "$f" "$status" <<'PY'
import json, re, sys
path, status = sys.argv[1], sys.argv[2]
doc = json.loads(open(path, encoding="utf-8").read())
if doc.get("schema") != "s11-0-probe/v1":
    raise SystemExit("schema")
if doc.get("status") != status:
    raise SystemExit("status %s" % doc.get("status"))
if doc.get("result_legend") != "timeout-is-error-not-blocked":
    raise SystemExit("legend")
if doc.get("endpoint_cidr") != "/32":
    raise SystemExit("cidr")
if doc.get("stage_b_block") != "0.0.0.0/0":
    raise SystemExit("block")
if doc.get("stage_b_allow") != "endpoint/32":
    raise SystemExit("allow")
need = ("probe_id", "stage", "target", "result", "elapsed_ms", "detail")
targets = {"host", "guest", "gateway", "public", "endpoint", "lan-redacted", "metadata"}
results = {"reachable", "blocked", "error"}
for p in doc["probes"]:
    for k in need:
        if k not in p:
            raise SystemExit("missing " + k)
    if p["result"] not in results:
        raise SystemExit("result")
    if p["target"] not in targets:
        raise SystemExit("target " + p["target"])
    if not isinstance(p["elapsed_ms"], int) or p["elapsed_ms"] < 0:
        raise SystemExit("elapsed")
    blob = p["detail"] + p["target"] + p["probe_id"]
    if re.search(r"\d{1,3}(?:\.\d{1,3}){3}", blob):
        raise SystemExit("probe ip")
for name in doc["clone_names"]:
    if not str(name).startswith("s11-0-probe-"):
        raise SystemExit("clone " + str(name))
print("schema-ok")
PY
}

probe_result() {
  local f=$1 id=$2 stage=$3
  python3 -I - "$f" "$id" "$stage" <<'PY'
import json, sys
doc = json.loads(open(sys.argv[1], encoding="utf-8").read())
want_id, want_stage = sys.argv[2], sys.argv[3]
for p in doc["probes"]:
    if p["probe_id"] == want_id and p["stage"] == want_stage:
        print("%s %s" % (p["result"], p["detail"]))
        raise SystemExit(0)
raise SystemExit(1)
PY
}

# Prepare one isolated case. Caller sets mode-specific env after this.
new_case() {
  local name=$1
  CASE="$ROOT/cases/$name"
  rm -rf "$CASE"
  mkdir -p "$CASE/home" "$CASE/tart-home" "$CASE/out"
  : > "$CASE/tart.log"
  : > "$CASE/cloned"
  : > "$CASE/deleted"
  : > "$CASE/run-pids"
  : > "$CASE/nc.log"
  printf '%s\n' "keep-other-vm" > "$CASE/preserve"
  touch "$CASE/softnet" "$CASE/key"
  chmod 600 "$CASE/key"
  export HOME="$CASE/home"
  export PATH="$STUBS:/usr/bin:/bin"
  export S11_TART_LOG="$CASE/tart.log"
  export S11_CLONED="$CASE/cloned"
  export S11_DELETED="$CASE/deleted"
  export S11_RUN_PIDS="$CASE/run-pids"
  export S11_NC_LOG="$CASE/nc.log"
  export S11_SOFTNET_SUID=1
  unset S11_TART_IP_FAIL S11_TART_RUN_EXIT S11_SSH_FAIL S11_FAKE_EUID S11_DF_AVAIL S11_GOLDEN_STATE S11_GOLDEN_NAME
  TH="$CASE/tart-home"
  SN="$CASE/softnet"
  KEY="$CASE/key"
  OUT="$CASE/out"
  hash -r
}

base_args() {
  BASE_ARGS=(
    --golden macos-s2-eval
    --tart-home "$TH"
    --softnet "$SN"
    --ssh-key "$KEY"
    --out "$OUT"
    --min-free-gb 1
    --overall-timeout 90
    --boot-timeout 8
    --clone-timeout 8
    --ssh-wait 5
    --probe-timeout 25
    --stop-timeout 8
    --delete-timeout 8
    --lan-ip 203.0.113.9
  )
}

run_limited() {
  local limit=$1
  shift
  set +e
  timeout "$limit" bash "$PROBE" "$@" >"$CASE/stdout" 2>"$CASE/stderr"
  RC=$?
  set -e
  note_pids "$CASE/run-pids"
}

assert_deleted_only_prefix() {
  local line=""
  if grep -q '^BAD ' "$CASE/tart.log"; then
    fail "$1 deleted a non-prefix vm"
    return 1
  fi
  if grep -q 'macos-s2-eval' "$CASE/deleted"; then
    fail "$1 deleted golden"
    return 1
  fi
  if grep -q 'keep-other-vm' "$CASE/tart.log"; then
    fail "$1 touched preserved name"
    return 1
  fi
  while read -r line; do
    [[ -n "$line" ]] || continue
    case "$line" in
      s11-0-probe-*) ;;
      *)
        fail "$1 deleted $line"
        return 1
        ;;
    esac
  done < "$CASE/deleted"
  return 0
}

assert_no_clone() {
  if grep -q '^clone ' "$CASE/tart.log"; then
    fail "$1 cloned a vm"
    return 1
  fi
  if [[ -s "$CASE/deleted" ]]; then
    fail "$1 deleted something"
    return 1
  fi
  return 0
}

# --- static -----------------------------------------------------------------

if bash -n "$PROBE"; then
  ok "bash -n probe"
else
  fail "bash -n probe"
fi
if bash -n "$0"; then
  ok "bash -n test"
else
  fail "bash -n test"
fi
if command -v shellcheck >/dev/null 2>&1; then
  if shellcheck "$PROBE"; then
    ok "shellcheck"
  else
    fail "shellcheck"
  fi
else
  ok "shellcheck absent, bash -n used"
fi
if grep -E -n 'sudo|pkill|pfctl|iptables|socketfilterfw|NumberOfPasswordPrompts' "$PROBE"; then
  fail "forbidden command in probe"
else
  ok "no sudo pkill firewall edits"
fi

# --- help / dry-run ----------------------------------------------------------

new_case dry-a
base_args
run_limited 10 --dry-run --stage A --clone-name s11-0-probe-drya --golden macos-s2-eval \
  --tart-home "$CASE/no-tart-home" --ssh-key "$KEY" --out "$CASE/no-out" --min-free-gb 1
if [[ "$RC" -eq 0 ]] && [[ ! -s "$CASE/tart.log" ]] && [[ ! -e "$CASE/no-out" ]] \
  && [[ ! -e "$CASE/no-tart-home" ]]; then
  if grep -q 'tart clone macos-s2-eval s11-0-probe-drya' "$CASE/stdout" \
    && grep -q 'tart run s11-0-probe-drya --no-graphics --net-softnet' "$CASE/stdout" \
    && ! grep -q 'net-softnet-block' "$CASE/stdout" \
    && grep -q 'RFC5737' "$CASE/stdout"; then
    ok "dry-run stage A has no side effects"
  else
    fail "dry-run stage A command list"
  fi
else
  fail "dry-run stage A rc=$RC"
fi

new_case dry-all
base_args
MISSING="$CASE/missing-tart-home"
run_limited 10 --dry-run --stage all --tart-home "$MISSING" --clone-name s11-0-probe-dryall \
  --golden macos-s2-eval --ssh-key "$KEY" --out "$CASE/no-out" --min-free-gb 1
if [[ "$RC" -eq 0 ]] && [[ ! -d "$MISSING" ]] && [[ ! -e "$CASE/no-out" ]] && [[ ! -s "$CASE/tart.log" ]]; then
  if grep -q -- '--net-softnet-block=0.0.0.0/0' "$CASE/stdout" \
    && grep -q 'tart delete s11-0-probe-dryall' "$CASE/stdout"; then
    ok "dry-run stage all prints block and creates nothing"
  else
    fail "dry-run stage all command list"
  fi
else
  fail "dry-run stage all rc=$RC"
fi

new_case dry-endpoint
base_args
run_limited 10 --dry-run --stage B --endpoint-ip 198.51.100.8 --clone-name s11-0-probe-dryb \
  --golden macos-s2-eval --tart-home "$CASE/no-tart-home" --ssh-key "$KEY" \
  --out "$CASE/no-out" --min-free-gb 1
if [[ "$RC" -eq 0 ]] && grep -q 'endpoint source: argument' "$CASE/stdout" \
  && grep -q -- '--net-softnet-allow=198.51.100.8/32' "$CASE/stdout" \
  && [[ ! -e "$CASE/no-out" ]] && [[ ! -s "$CASE/tart.log" ]]; then
  ok "dry-run shows allow /32 and does not write a report"
else
  fail "dry-run endpoint"
fi

new_case help
run_limited 10 --help
if [[ "$RC" -eq 0 ]]; then
  ok "help"
else
  fail "help rc=$RC"
fi

# --- usage rejects -----------------------------------------------------------

new_case same-name
base_args
run_limited 10 "${BASE_ARGS[@]}" --golden s11-0-probe-same --clone-name s11-0-probe-same
if [[ "$RC" -eq 1 ]] && [[ ! -e "$CASE/out/report.json" ]] && [[ ! -s "$CASE/tart.log" ]] \
  && grep -q 'same name' "$CASE/stderr"; then
  ok "golden same name rejected"
else
  fail "same name rc=$RC"
fi

new_case golden-prefix
base_args
run_limited 10 "${BASE_ARGS[@]}" --golden s11-0-probe-gold --clone-name s11-0-probe-other
if [[ "$RC" -eq 1 ]] && [[ ! -e "$CASE/out/report.json" ]] && [[ ! -s "$CASE/tart.log" ]]; then
  ok "golden prefix rejected"
else
  fail "golden prefix rc=$RC"
fi

new_case bad-endpoint
base_args
bad_ok=1
for ip in 10.1.2.3 192.168.1.1 127.0.0.1 169.254.1.1 100.64.0.1 0.0.0.0; do
  run_limited 10 "${BASE_ARGS[@]}" --endpoint-ip "$ip" --clone-name s11-0-probe-badep
  if [[ "$RC" -ne 1 || -e "$CASE/out/report.json" || -s "$CASE/tart.log" ]]; then
    bad_ok=0
    fail "endpoint $ip rc=$RC"
  fi
done
if [[ "$bad_ok" -eq 1 ]]; then
  ok "private and non-public endpoints rejected"
fi

new_case root
base_args
export S11_FAKE_EUID=0
run_limited 10 "${BASE_ARGS[@]}" --dry-run --stage all --clone-name s11-0-probe-root
unset S11_FAKE_EUID
if [[ "$RC" -eq 2 ]] && [[ ! -e "$CASE/out/report.json" ]] && [[ ! -s "$CASE/tart.log" ]] \
  && grep -q 'refusing to run as root' "$CASE/stderr"; then
  ok "root refused before any change"
else
  fail "root rc=$RC"
fi

# --- precheck ----------------------------------------------------------------

new_case no-tart
mkdir -p "$CASE/home" "$CASE/tart-home"
export HOME="$CASE/home"
export PATH="/usr/bin:/bin"
export S11_TART_LOG="$CASE/tart.log"
run_limited 15 --golden macos-s2-eval --tart-home "$TH" --softnet "$SN" --ssh-key "$KEY" \
  --out "$OUT" --min-free-gb 0 --clone-name s11-0-probe-notart --overall-timeout 60
export PATH="$STUBS:/usr/bin:/bin"
if [[ "$RC" -eq 2 ]] && assert_no_clone "no-tart"; then
  assert_no_leak "$OUT/report.json"
  assert_no_leak "$OUT/report.md"
  assert_schema "$OUT/report.json" "precheck-failed"
  if probe_result "$OUT/report.json" precheck-tart precheck | grep -q '^error '; then
    ok "missing tart exits 2 with a partial precheck report"
  else
    fail "missing tart probe row"
  fi
else
  fail "missing tart rc=$RC"
fi

new_case not-suid
base_args
export S11_SOFTNET_SUID=0
run_limited 15 "${BASE_ARGS[@]}" --clone-name s11-0-probe-nosuid
export S11_SOFTNET_SUID=1
if [[ "$RC" -eq 2 ]] && assert_no_clone "not-suid"; then
  assert_schema "$OUT/report.json" "precheck-failed"
  assert_no_leak "$OUT/report.json"
  if probe_result "$OUT/report.json" precheck-softnet precheck | grep -q 'not-suid'; then
    ok "softnet without suid exits 2"
  else
    fail "softnet suid row"
  fi
else
  fail "not-suid rc=$RC"
fi

new_case golden-running
base_args
export S11_GOLDEN_STATE=running
run_limited 15 "${BASE_ARGS[@]}" --clone-name s11-0-probe-runG
unset S11_GOLDEN_STATE
if [[ "$RC" -eq 2 ]] && assert_no_clone "golden-running"; then
  assert_schema "$OUT/report.json" "precheck-failed"
  if probe_result "$OUT/report.json" precheck-golden precheck | grep -q 'running'; then
    ok "running golden exits 2 without clone"
  else
    fail "golden running row"
  fi
else
  fail "golden running rc=$RC"
fi

new_case disk-low
base_args
export S11_DF_AVAIL=100
run_limited 15 --clone-name s11-0-probe-disk --min-free-gb 1 --golden macos-s2-eval \
  --tart-home "$TH" --softnet "$SN" --ssh-key "$KEY" --out "$OUT" --overall-timeout 60
unset S11_DF_AVAIL
if [[ "$RC" -eq 2 ]] && assert_no_clone "disk-low"; then
  assert_schema "$OUT/report.json" "precheck-failed"
  if probe_result "$OUT/report.json" precheck-disk precheck | grep -q 'low'; then
    ok "low disk exits 2"
  else
    fail "disk row"
  fi
else
  fail "disk rc=$RC"
fi

new_case endpoint-redact
base_args
export S11_SOFTNET_SUID=0
run_limited 15 "${BASE_ARGS[@]}" --endpoint-ip 198.51.100.8 --clone-name s11-0-probe-redact
export S11_SOFTNET_SUID=1
if [[ "$RC" -eq 2 ]]; then
  assert_no_leak "$OUT/report.json"
  assert_no_leak "$OUT/report.md"
  python3 -I - "$OUT/report.json" <<'PY'
import json, sys
doc = json.loads(open(sys.argv[1], encoding="utf-8").read())
if doc.get("endpoint_source") != "argument":
    raise SystemExit("source")
if doc.get("placeholder_note") not in ("", None):
    raise SystemExit("note")
PY
  ok "argument endpoint is redacted in the report"
else
  fail "endpoint redact rc=$RC"
fi

# --- runtime -----------------------------------------------------------------

new_case success
base_args
run_limited 45 "${BASE_ARGS[@]}" --stage all --clone-name s11-0-probe-ok --overall-timeout 400
if ! reap "$CASE/run-pids"; then
  fail "success left a tart run process"
fi
if [[ "$RC" -eq 0 ]] && assert_deleted_only_prefix "success"; then
  assert_no_leak "$OUT/report.json"
  assert_no_leak "$OUT/report.md"
  assert_schema "$OUT/report.json" "complete"
  python3 -I - "$OUT/report.json" "$CASE/deleted" <<'PY'
import json, sys
doc = json.loads(open(sys.argv[1], encoding="utf-8").read())
deleted = [ln.strip() for ln in open(sys.argv[2], encoding="utf-8") if ln.strip()]
if doc.get("endpoint_source") != "placeholder":
    raise SystemExit("source")
if "rfc5737" not in doc.get("placeholder_note", ""):
    raise SystemExit("placeholder")
if doc.get("stage_b_ran") is not True or doc.get("interrupted") is not False:
    raise SystemExit("flags")
if doc.get("cleanup", {}).get("delete") != "ok":
    raise SystemExit("cleanup")
names = doc.get("clone_names")
if names != ["s11-0-probe-ok", "s11-0-probe-okb"]:
    raise SystemExit("names %s" % names)
if sorted(deleted) != sorted(names):
    raise SystemExit("deleted %s" % deleted)
want = {
    ("gateway-icmp", "A"): "reachable",
    ("gateway-tcp-22", "A"): "error",
    ("gateway-tcp-443", "A"): "error",
    ("dns-udp-gateway", "A"): "blocked",
    ("dns-udp-public", "B"): "blocked",
    ("ipv6-addr", "A"): "blocked",
    ("ipv6-tcp-public", "B"): "blocked",
    ("endpoint-tcp-443", "B"): "error",
    ("endpoint-udp-443", "A"): "reachable",
    ("endpoint-udp-443", "B"): "reachable",
    ("lan-icmp", "A"): "reachable",
    ("lan-tcp-443", "B"): "error",
    ("metadata-icmp", "A"): "reachable",
    ("metadata-tcp-80", "B"): "error",
    ("host-tart-ip", "B"): "reachable",
    ("host-ssh-channel", "B"): "reachable",
    ("tart-exec-true", "C"): "reachable",
    ("guest-vsock", "C"): "reachable",
    ("guest-virtio", "C"): "reachable",
}
by = {(p["probe_id"], p["stage"]): p for p in doc["probes"]}
for key, result in want.items():
    got = by.get(key)
    if not got or got["result"] != result:
        raise SystemExit("want %s %s got %s" % (key, result, got))
if by[("lan-icmp", "A")]["target"] != "lan-redacted":
    raise SystemExit("lan target")
if by[("metadata-tcp-80", "B")]["target"] != "metadata":
    raise SystemExit("metadata target")
if by[("ipv6-addr", "A")]["detail"] != "only-link-local":
    raise SystemExit("v6 detail")
PY
  if grep -q '^run s11-0-probe-ok --no-graphics --net-softnet$' "$CASE/tart.log" \
    && grep -q -- '--net-softnet-block=0.0.0.0/0 --net-softnet-allow=192.0.2.10/32' "$CASE/tart.log" \
    && ! grep -q '192.0.2.10' "$OUT/report.json"; then
    ok "stage all success, redacted report, prefix-only delete"
  else
    fail "success command trace"
  fi
else
  fail "success rc=$RC"
  sed -n '1,80p' "$CASE/stderr" >&2 || true
fi

new_case ip-timeout
base_args
export S11_TART_IP_FAIL=1
run_limited 20 "${BASE_ARGS[@]}" --stage A --boot-timeout 2 --clone-name s11-0-probe-ipfail
unset S11_TART_IP_FAIL
if ! reap "$CASE/run-pids"; then
  fail "ip-timeout left a process"
fi
if [[ "$RC" -eq 4 ]] && assert_deleted_only_prefix "ip-timeout"; then
  assert_schema "$OUT/report.json" "partial"
  assert_no_leak "$OUT/report.json"
  assert_no_leak "$OUT/report.md"
  if probe_result "$OUT/report.json" host-tart-ip A | grep -q 'ip-unavailable' \
    && grep -qx 's11-0-probe-ipfail' "$CASE/deleted"; then
    ok "ip timeout still deletes and writes a partial report"
  else
    fail "ip timeout rows"
  fi
else
  fail "ip timeout rc=$RC"
fi

new_case run-exit
base_args
export S11_TART_RUN_EXIT=1
run_limited 20 "${BASE_ARGS[@]}" --stage A --clone-name s11-0-probe-runexit
unset S11_TART_RUN_EXIT
if [[ "$RC" -eq 4 ]] && assert_deleted_only_prefix "run-exit"; then
  assert_schema "$OUT/report.json" "partial"
  assert_no_leak "$OUT/report.json"
  if probe_result "$OUT/report.json" host-tart-ip A | grep -q 'tart-run-exited' \
    && grep -qx 's11-0-probe-runexit' "$CASE/deleted"; then
    ok "tart run exit still deletes and writes a partial report"
  else
    fail "run exit rows"
  fi
else
  fail "run exit rc=$RC"
fi

new_case ssh-fail
base_args
export S11_SSH_FAIL=1
run_limited 20 "${BASE_ARGS[@]}" --stage B --ssh-wait 2 --clone-name s11-0-probe-ssh
unset S11_SSH_FAIL
if ! reap "$CASE/run-pids"; then
  fail "ssh-fail left a process"
fi
if [[ "$RC" -eq 4 ]] && assert_deleted_only_prefix "ssh-fail"; then
  assert_schema "$OUT/report.json" "partial"
  assert_no_leak "$OUT/report.json"
  assert_no_leak "$OUT/report.md"
  if probe_result "$OUT/report.json" host-ssh-channel B | grep -q 'ssh-session-failed' \
    && probe_result "$OUT/report.json" gateway-icmp B | grep -q 'no-observation' \
    && grep -qx 's11-0-probe-sshb' "$CASE/deleted" \
    && grep -q -- '--net-softnet-block=0.0.0.0/0' "$CASE/tart.log"; then
    ok "ssh failure still deletes and writes a partial report"
  else
    fail "ssh failure rows"
  fi
else
  fail "ssh failure rc=$RC"
fi

new_case term
base_args
export S11_TART_IP_FAIL=1
bash "$PROBE" --stage A --boot-timeout 30 --ssh-wait 5 --overall-timeout 60 \
  --clone-name s11-0-probe-term --golden macos-s2-eval --tart-home "$TH" \
  --softnet "$SN" --ssh-key "$KEY" --out "$OUT" --min-free-gb 1 \
  --lan-ip 203.0.113.9 >"$CASE/stdout" 2>"$CASE/stderr" &
probe_pid=$!
printf '%s\n' "$probe_pid" >> "$KEEP_PIDS"
seen=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
  if grep -q '^run ' "$CASE/tart.log"; then
    seen=1
    break
  fi
  sleep 0.25
done
kill -TERM "$probe_pid" 2>/dev/null || true
set +e
wait "$probe_pid"
RC=$?
set -e
unset S11_TART_IP_FAIL
note_pids "$CASE/run-pids"
if ! reap "$CASE/run-pids"; then
  fail "term left a process"
fi
if [[ "$seen" -eq 1 && "$RC" -eq 130 ]] && assert_deleted_only_prefix "term"; then
  assert_schema "$OUT/report.json" "partial"
  assert_no_leak "$OUT/report.json"
  python3 -I - "$OUT/report.json" <<'PY'
import json, sys
doc = json.loads(open(sys.argv[1], encoding="utf-8").read())
if doc.get("interrupted") is not True:
    raise SystemExit("interrupted")
if doc.get("cleanup", {}).get("delete") != "ok":
    raise SystemExit("delete")
PY
  if grep -qx 's11-0-probe-term' "$CASE/deleted"; then
    ok "TERM cleans only the prefixed clone and writes a report"
  else
    fail "term delete name"
  fi
else
  fail "term seen=$seen rc=$RC"
  sed -n '1,40p' "$CASE/stderr" >&2 || true
fi

# --- guest -------------------------------------------------------------------

new_case guest
export PATH="$STUBS:/usr/bin:/bin"
export S11_NC_LOG="$CASE/nc.log"
guest_out="$CASE/guest.out"
bash "$PROBE" --guest --stage A --ep 192.0.2.10 --pub 1.1.1.1 --ports 22,53,80,443 \
  --lan 203.0.113.9 >"$guest_out"
lines=$(grep -c '^PROBE ' "$guest_out" || true)
nc_lines=$(wc -l < "$CASE/nc.log" | tr -d ' ')
if [[ "$lines" -eq 16 && "$nc_lines" -le 30 ]]; then
  assert_no_leak "$guest_out"
  if grep -q '^PROBE gateway-icmp reachable ' "$guest_out" \
    && grep -q '^PROBE endpoint-udp-443 reachable ' "$guest_out" \
    && grep -q '^PROBE ipv6-addr blocked ' "$guest_out" \
    && grep -q '^PROBE metadata-tcp-443 error ' "$guest_out" \
    && ! grep -q '169.254.169.254' "$guest_out"; then
    ok "guest probes emit roles without addresses"
  else
    fail "guest probe lines"
  fi
else
  fail "guest lines=$lines nc=$nc_lines"
fi

bash "$PROBE" --guest --inventory >"$CASE/inventory.out"
if grep -q '^PROBE guest-vsock reachable ' "$CASE/inventory.out" \
  && ! grep -q 'NOTAREALSERIAL' "$CASE/inventory.out"; then
  ok "guest inventory keeps hardware identifiers out of the probe lines"
else
  fail "guest inventory"
fi

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
if [[ "$FAIL" -ne 0 ]]; then
  exit 1
fi
exit 0

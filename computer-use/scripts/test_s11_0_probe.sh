#!/bin/bash
# Manual stub suite for s11-0-probe.sh. Not invoked by scripts/verify.sh.
# Bounds each probe with a background sleep and kill. No external timer program.
set -euo pipefail
export LC_ALL=C

HERE=$(cd "$(dirname "$0")" && pwd)
PROBE=$HERE/s11-0-probe.sh
VERIFY=$HERE/verify.sh
RUNBOOK=$HERE/../docs/s11-0-probe-runbook.md
BASE=$(mktemp -d)
SYS=$BASE/sys
PYTHON=$(PATH=/usr/bin:/bin:/usr/local/bin command -v python3 || true)
PROBE_PID=""
WATCHER=""
CASE=""
ROOT=""
ARGS=()

cleanup_base() {
  local st=$?
  if [[ -n "${WATCHER:-}" ]]; then
    kill -KILL -- -"$WATCHER" 2>/dev/null || true
    kill -KILL "$WATCHER" 2>/dev/null || true
  fi
  if [[ -n "${PROBE_PID:-}" ]]; then
    kill -KILL -- -"$PROBE_PID" 2>/dev/null || true
    kill -KILL "$PROBE_PID" 2>/dev/null || true
  fi
  rm -rf "$BASE"
  exit "$st"
}
trap cleanup_base EXIT

die() {
  echo "FAIL: $*" >&2
  echo "---- stdout ----" >&2
  head -c 4000 "$CASE/stdout" >&2 2>/dev/null || true
  echo >&2
  echo "---- stderr ----" >&2
  head -c 4000 "$CASE/stderr" >&2 2>/dev/null || true
  echo >&2
  echo "---- tart.log ----" >&2
  head -c 4000 "$ROOT/tart.log" >&2 2>/dev/null || true
  echo >&2
  echo "---- ssh.log ----" >&2
  head -c 2000 "$ROOT/ssh.log" >&2 2>/dev/null || true
  echo >&2
  echo "---- nc.log ----" >&2
  head -c 2000 "$ROOT/nc.log" >&2 2>/dev/null || true
  echo >&2
  echo "---- report ----" >&2
  head -c 8000 "$CASE/out/report.json" >&2 2>/dev/null || true
  echo >&2
  echo "---- inventory ----" >&2
  cat "$ROOT/inventory" >&2 2>/dev/null || true
  echo "---- ec ----" >&2
  cat "$CASE/ec" >&2 2>/dev/null || true
  echo >&2
  if [[ -n "${WATCHER:-}" ]]; then
    kill -KILL -- -"$WATCHER" 2>/dev/null || true
    kill -KILL "$WATCHER" 2>/dev/null || true
  fi
  if [[ -n "${PROBE_PID:-}" ]]; then
    kill -KILL -- -"$PROBE_PID" 2>/dev/null || true
    kill -KILL "$PROBE_PID" 2>/dev/null || true
  fi
  exit 1
}

link_sys() {
  local c p
  mkdir -p "$SYS"
  for c in bash ps awk sed grep mktemp head tr cut od readlink dirname id wc sleep \
    cat rm mkdir mv cp ln chmod touch kill cmp find; do
    p=$(PATH=/usr/bin:/bin command -v "$c" || true)
    [[ -n "$p" ]] || die "missing system tool $c"
    ln -sf "$p" "$SYS/$c"
  done
}

write_stub() {
  local name=$1
  shift
  mkdir -p "$BASE/tpl"
  printf '%s\n' "#!/bin/bash" "$@" >"$BASE/tpl/$name"
  chmod +x "$BASE/tpl/$name"
}

install_templates() {
  write_stub tart \
    'ROOT=${S11_STUB_ROOT:?}' \
    'INV=$ROOT/inventory' \
    'printf "%s\n" "$*" >>"$ROOT/tart.log"' \
    'echo "LEAK-not-for-report" >&2' \
    'hang() { trap "exit 143" TERM INT HUP; while true; do sleep 0.2; done; }' \
    'has() { awk -v s="$1" '"'"'$1==s{f=1} END{exit !f}'"'"' "$INV"; }' \
    'cmd=${1:-}; shift || true' \
    'case "$cmd" in' \
    '  --version) [[ -f "$ROOT/hang_version" ]] && hang; echo "stub-tart 0"; exit 0 ;;' \
    '  --help) printf "%s\n" "commands: clone list run stop delete ip exec"; exit 0 ;;' \
    '  list)' \
    '    [[ -f "$ROOT/hang_list" ]] && hang' \
    '    [[ -f "$ROOT/hang_list_after_delete" && -s "$ROOT/delete.args" ]] && hang' \
    '    printf "%s\n" "Name Status"; cat "$INV"; exit 0 ;;' \
    '  clone)' \
    '    src=$1; dst=$2' \
    '    [[ -f "$ROOT/fail_clone" ]] && exit 1' \
    '    if [[ -f "$ROOT/partial_clone" ]]; then printf "%s stopped\n" "$dst" >>"$INV"; exit 1; fi' \
    '    [[ -f "$ROOT/omit_clone" ]] && exit 0' \
    '    has "$src" || exit 1' \
    '    has "$dst" && exit 1' \
    '    printf "%s stopped\n" "$dst" >>"$INV"; exit 0 ;;' \
    '  run)' \
    '    has "$1" || exit 1' \
    '    [[ -f "$ROOT/fail_run" ]] && exit 1' \
    '    trap "exit 0" TERM INT HUP' \
    '    while true; do sleep 0.2; done ;;' \
    '  stop)' \
    '    printf "%s\n" "$1" >>"$ROOT/stop.args"' \
    '    touch "$ROOT/stop.entered"' \
    '    [[ -f "$ROOT/slow_stop" ]] && sleep 2' \
    '    [[ -f "$ROOT/fail_stop" ]] && exit 1' \
    '    awk -v n="$1" '"'"'$1==n{$2="stopped"} {print}'"'"' "$INV" >"$INV.tmp" && mv "$INV.tmp" "$INV"' \
    '    exit 0 ;;' \
    '  delete)' \
    '    printf "%s\n" "$1" >>"$ROOT/delete.args"' \
    '    [[ -f "$ROOT/fail_delete" ]] && exit 1' \
    '    awk -v n="$1" '"'"'$1!=n{print}'"'"' "$INV" >"$INV.tmp" && mv "$INV.tmp" "$INV"' \
    '    exit 0 ;;' \
    '  ip) [[ -f "$ROOT/hang_ip" ]] && hang; has "$1" || exit 1; echo "192.0.2.20"; exit 0 ;;' \
    '  exec)' \
    '    if [[ "${1:-}" == "--help" ]]; then printf "%s\n" "--username" "--no-interactive"; exit 0; fi' \
    '    has "$1" || exit 1; exit 0 ;;' \
    '  *) echo "bad tart $cmd" >&2; exit 2 ;;' \
    'esac'
  write_stub ssh \
    'ROOT=${S11_STUB_ROOT:?}' \
    'printf "%s\n" "$*" >>"$ROOT/ssh.log"' \
    '[[ -f "$ROOT/guest_fail" ]] && exit 9' \
    'if [[ -f "$ROOT/guest_hang" ]]; then trap "exit 143" TERM INT HUP; while true; do sleep 0.2; done; fi' \
    'last=""' \
    'for last in "$@"; do :; done' \
    'ep=$(printf "%s\n" "$last" | sed -n "s/.*S11_EP=\\([^ ]*\\).*/\\1/p")' \
    'lan=$(printf "%s\n" "$last" | sed -n "s/.*S11_LAN=\\([^ ]*\\).*/\\1/p")' \
    'export S11_EP="$ep" S11_LAN="$lan"' \
    'f=$(mktemp "${TMPDIR:-/tmp}/guestin.XXXXXX")' \
    'cat >"$f"' \
    'if [[ -f "$ROOT/guest_nonzero" ]]; then bash "$f"; rm -f "$f"; exit 7; fi' \
    'bash "$f"; st=$?; rm -f "$f"; exit "$st"'
  write_stub nc \
    'ROOT=${S11_STUB_ROOT:?}' \
    'args=()' \
    'while [[ $# -gt 0 ]]; do' \
    '  case "$1" in' \
    '    -z) shift ;;' \
    '    -G|-w) shift 2 ;;' \
    '    *) args+=("$1"); shift ;;' \
    '  esac' \
    'done' \
    'host=${args[$((${#args[@]}-2))]}' \
    'port=${args[$((${#args[@]}-1))]}' \
    'printf "%s %s\n" "$host" "$port" >>"$ROOT/nc.log"' \
    'if [[ "$host" == "203.0.113.10" && "$port" == "443" ]]; then exit 0; fi' \
    'echo "Connection refused" >&2' \
    'exit 1'
  write_stub dig \
    'ROOT=${S11_STUB_ROOT:?}' \
    'printf "%s\n" "$*" >>"$ROOT/dig.log"' \
    'for a in "$@"; do case "$a" in @*) printf "%s\n" "${a#@}" >>"$ROOT/dig.servers" ;; esac; done' \
    'exit 0'
  write_stub host \
    'ROOT=${S11_STUB_ROOT:?}' \
    'printf "%s\n" "$*" >>"$ROOT/host.log"' \
    'exit 127'
  write_stub nslookup \
    'ROOT=${S11_STUB_ROOT:?}' \
    'printf "%s\n" "$*" >>"$ROOT/nslookup.log"' \
    'exit 127'
  write_stub route \
    'printf "%s\n" "gateway: 198.51.100.1"'
  write_stub ifconfig \
    'printf "%s\n" "inet6 fe80::1%en0 prefixlen 64" "inet6 2606:4700:4700::10 prefixlen 64"'
  write_stub stat \
    'ROOT=${S11_STUB_ROOT:?}' \
    '[[ $# -eq 3 && "$1" == -f && "$2" == "%Su %Sp" ]] || exit 2' \
    'path=$3' \
    'printf "%s\n" "$path" >>"$ROOT/stat.log"' \
    'if [[ -L "$path" ]]; then printf "%s\n" "root -rwxr-xr-x"; exit 0; fi' \
    'if [[ -f "$path.statout" ]]; then cat "$path.statout"; exit 0; fi' \
    'printf "%s\n" "root -rwxr-xr-x"' \
    'exit 0'
}

setup() {
  ROOT=$CASE/stub
  mkdir -p "$ROOT/bin" "$ROOT/mid" "$ROOT/real" "$CASE/thome" "$CASE/tmp" "$CASE/out"
  cp "$BASE/tpl/"* "$ROOT/bin/"
  [[ -x "$ROOT/bin/tart" && -x "$ROOT/bin/stat" ]] || die "stubs not installed"
  printf '%s\n' 'macos-s2-eval stopped' 's11-0-probe-keep stopped' 's11-0-probe-A-1-deadbee1 stopped' >"$ROOT/inventory"
  cp "$ROOT/inventory" "$ROOT/inventory.orig"
  printf 'key\n' >"$CASE/id_key"
  chmod 600 "$CASE/id_key"
  printf '%s\n' '#!/bin/bash' 'exit 0' >"$ROOT/real/softnet"
  chmod +x "$ROOT/real/softnet"
  printf '%s\n' 'root -rwsr-xr-x' >"$ROOT/real/softnet.statout"
  ln -s ../real/softnet "$ROOT/mid/link"
  ln -s ../mid/link "$ROOT/bin/softnet"
  export S11_STUB_ROOT=$ROOT
  export PATH="$ROOT/bin:$SYS"
  export TMPDIR=$CASE/tmp
  export S11_START_SEC=0
  unset S11_BOUND_SEC S11_FORCE_NAME S11_PROBE_BUDGET S11_CLEANUP_BUDGET
  ARGS=(--ssh-key "$CASE/id_key" --tart-home "$CASE/thome" --out "$CASE/out")
}

start_probe() {
  local limit=$1
  shift
  PROBE_PID=""
  WATCHER=""
  set -m
  set +b
  bash "$PROBE" "$@" >"$CASE/stdout" 2>"$CASE/stderr" &
  PROBE_PID=$!
  ( sleep "$limit"; kill -KILL -- -"$PROBE_PID" 2>/dev/null || true ) &
  WATCHER=$!
}

finish_probe() {
  local st=0
  wait "$PROBE_PID" || st=$?
  PROBE_PID=""
  kill -KILL -- -"$WATCHER" 2>/dev/null || true
  kill -KILL "$WATCHER" 2>/dev/null || true
  wait "$WATCHER" 2>/dev/null || true
  set +m
  printf '%s\n' "$st" >"$CASE/ec"
}

run_probe() {
  local limit=$1
  shift
  start_probe "$limit" "$@"
  finish_probe
}

need_ec() {
  local got
  got=$(cat "$CASE/ec")
  [[ "$got" == "$1" ]] || die "exit $got want $1"
}

report() { cat "$CASE/out/report.json"; }

need_reason() {
  grep -F "\"$1\"" "$CASE/out/report.json" >/dev/null || die "missing reason $1"
}

assert_has() {
  awk -v s="$1" '$1==s {f=1} END{exit !f}' "$ROOT/inventory" || die "lost vm $1"
}

assert_owned_only() {
  local f n
  for f in stop.args delete.args; do
    [[ -f "$ROOT/$f" ]] || continue
    while IFS= read -r n; do
      [[ -n "$n" ]] || continue
      grep -E "^clone [^ ]+ ${n}$" "$ROOT/tart.log" >/dev/null || die "$f $n was not cloned by this run"
      [[ "$n" == "macos-s2-eval" || "$n" == "s11-0-probe-keep" || "$n" == "s11-0-probe-A-1-deadbee1" ]] && die "$f touched $n"
    done <"$ROOT/$f"
  done
  [[ -f "$ROOT/tart.log" ]] || return 0
  while IFS= read -r n; do
    case "$n" in
      clone\ macos-s2-eval\ *) ;;
      *macos-s2-eval*) die "golden in tart argv: $n" ;;
    esac
  done <"$ROOT/tart.log"
}

ban_file() {
  local where=$1 s
  [[ -f "$where" ]] || return 0
  for s in '198.51.100' '192.0.2' '2606:' '1.1.1.1' 'fe80' '169.254.169.254' \
    'blocked' 'reachable' 'LEAK-not-for-report' 'id_key' 'thome' '/tmp/s11-0'; do
    if grep -F -q -- "$s" "$where"; then die "banned [$s] in $where"; fi
  done
}

common_end() {
  local keep_golden=${1:-yes}
  [[ -f "$CASE/out/report.json" ]] || die "no report"
  [[ "$keep_golden" == yes ]] && assert_has macos-s2-eval
  assert_has s11-0-probe-keep
  assert_has s11-0-probe-A-1-deadbee1
  assert_owned_only
  ban_file "$CASE/out/report.json"
  ban_file "$CASE/stdout"
  ban_file "$CASE/stderr"
  if [[ -f "$ROOT/stat.log" ]]; then
    local p
    while IFS= read -r p; do
      [[ -n "$p" ]] || continue
      [[ -L "$p" ]] && die "stat called on symlink $p"
    done <"$ROOT/stat.log"
  fi
  if find "$CASE/tmp" -maxdepth 1 -name 's11-0.*' -print | grep -q .; then
    die "workdir left behind"
  fi
  grep -F -q '"h1_independent_of_ip":"unknown"' "$CASE/out/report.json" || die "h1 not unknown"
}

no_clone_ops() {
  [[ -f "$ROOT/tart.log" ]] || return 0
  grep -E '^(clone|run|stop|delete|ip) ' "$ROOT/tart.log" && die "unexpected tart mutation" || true
  [[ -f "$ROOT/stop.args" || -f "$ROOT/delete.args" ]] && die "stop or delete happened" || true
}

count_eq() {
  local file=$1 text=$2 want=$3 n=0
  [[ -f "$file" ]] && n=$(grep -Fxc -- "$text" "$file" || true)
  [[ "$n" == "$want" ]] || die "count [$text] = $n want $want"
}

static_checks() {
  bash -n "$PROBE"
  bash -n "$HERE/test_s11_0_probe.sh"
  local n
  n=$(wc -l <"$PROBE" | tr -d ' ')
  [[ "$n" -le 500 ]] || die "probe has $n lines"
  [[ "$(head -n 1 "$PROBE")" == '#!/bin/bash' ]] || die "shebang"
  local pat
  for pat in 'mapfile' 'declare -A' 'readarray' 'readlink -f' 'ioreg' 'system_profiler' 'wait -n'; do
    if grep -n -F -- "$pat" "$PROBE"; then die "probe contains $pat"; fi
  done
  if grep -n -E '(^|[^[:alnum:]_])reachable([^[:alnum:]_]|$)' "$PROBE"; then die "probe contains reachable"; fi
  if grep -n -E '(^|[^[:alnum:]_])blocked([^[:alnum:]_]|$)' "$PROBE"; then die "probe contains blocked"; fi
  if grep -n -E '\$\{[A-Za-z_][A-Za-z0-9_]*,,' "$PROBE"; then die "lower-case expansion"; fi
  if grep -n -E '(^|[[:space:]])timeout[[:space:]]+[0-9]' "$PROBE"; then die "external timer in probe"; fi
  if grep -n -E '(^|[^[:alnum:]_])hostname([^[:alnum:]_]|$)' "$PROBE"; then die "hostname in probe"; fi
  n=$(grep -c 'net-softnet-allow' "$PROBE" || true)
  [[ "$n" == 1 ]] || die "allow builder count $n"
  n=$(grep -c 'net-softnet-block' "$PROBE" || true)
  [[ "$n" == 1 ]] || die "block builder count $n"
  n=$(grep -F -c -- '--no-graphics' "$PROBE" || true)
  [[ "$n" == 1 ]] || die "no-graphics count $n"
  if grep -F -q 'test_s11_0_probe' "$VERIFY"; then die "verify.sh still calls the stub suite"; fi
  grep -F -q '26 分 30 秒' "$RUNBOOK"
  grep -F -q '1590' "$RUNBOOK"
  grep -F -q 'test_s11_0_probe.sh' "$RUNBOOK"
  grep -F -q 'verify.sh' "$RUNBOOK"
  grep -F -q '不构成运行授权' "$RUNBOOK"
  grep -F -q '必须先发' "$RUNBOOK"
  grep -F -q '示例，替换为获批地址' "$RUNBOOK"
  grep -F -q 'endpoint-placeholder' "$RUNBOOK"
  grep -F -q 'h1_independent_of_ip' "$RUNBOOK" || grep -F -q 'unknown' "$RUNBOOK"
  if grep -F -q 'reachable' "$RUNBOOK"; then die "runbook reachable"; fi
  if grep -F -q 'blocked' "$RUNBOOK"; then die "runbook blocked"; fi
  if grep -F -q '/Volumes/' "$RUNBOOK"; then die "runbook volume"; fi
  if grep -F -q '2400' "$RUNBOOK"; then die "runbook old budget"; fi
}

test_happy() {
  setup
  run_probe 12 --stage all --endpoint-ip 203.0.113.10 --lan-ip 198.51.100.50 "${ARGS[@]}"
  need_ec 0
  common_end
  local nameA nameB
  nameA=$(sed -n 's/^run \(s11-0-probe-A-[0-9][0-9]*-[0-9a-f][0-9a-f]*\) --no-graphics --net-softnet$/\1/p' "$ROOT/tart.log")
  nameB=$(sed -n 's/^run \(s11-0-probe-B-[0-9][0-9]*-[0-9a-f][0-9a-f]*\) --no-graphics --net-softnet --net-softnet-block=0\.0\.0\.0\/0 --net-softnet-allow=203\.0\.113\.10\/32$/\1/p' "$ROOT/tart.log")
  [[ -n "$nameA" && -n "$nameB" ]] || die "argv lines missing"
  grep -F "\"argv\":[\"tart\",\"run\",\"$nameA\",\"--no-graphics\",\"--net-softnet\"]" "$CASE/out/report.json" >/dev/null || die "A json argv"
  grep -F "\"argv\":[\"tart\",\"run\",\"$nameB\",\"--no-graphics\",\"--net-softnet\",\"--net-softnet-block=0.0.0.0/0\",\"--net-softnet-allow=203.0.113.10/32\"]" "$CASE/out/report.json" >/dev/null || die "B json argv"
  grep -F "argv: tart run $nameA --no-graphics --net-softnet" "$CASE/stdout" >/dev/null || die "A stdout argv"
  grep -F "argv: tart run $nameB --no-graphics --net-softnet --net-softnet-block=0.0.0.0/0 --net-softnet-allow=203.0.113.10/32" "$CASE/stdout" >/dev/null || die "B stdout argv"
  local opt
  for opt in BatchMode=yes ConnectTimeout=8 ServerAliveInterval=5 ServerAliveCountMax=2 IdentitiesOnly=yes StrictHostKeyChecking=accept-new UserKnownHostsFile= mktemp; do
    grep -F -q -- "$opt" "$ROOT/ssh.log" || die "ssh missing $opt"
  done
  if grep -F -q 'bash -s' "$ROOT/ssh.log"; then die "ssh used bash -s"; fi
  count_eq "$ROOT/nc.log" '203.0.113.10 443' 1
  count_eq "$ROOT/nc.log" '198.51.100.1 22' 2
  count_eq "$ROOT/nc.log" '198.51.100.1 53' 2
  count_eq "$ROOT/nc.log" '198.51.100.1 80' 2
  count_eq "$ROOT/nc.log" '198.51.100.1 443' 2
  count_eq "$ROOT/nc.log" '198.51.100.50 443' 2
  count_eq "$ROOT/nc.log" '169.254.169.254 80' 2
  count_eq "$ROOT/nc.log" '2606:4700:4700::1111 443' 2
  if grep -F -q '127.0.0.1' "$ROOT/nc.log" "$ROOT/dig.servers" "$ROOT/ssh.log"; then die "probed 127.0.0.1"; fi
  count_eq "$ROOT/dig.servers" '198.51.100.1' 2
  count_eq "$ROOT/dig.servers" '1.1.1.1' 2
  [[ ! -s "$ROOT/host.log" ]] || die "host used while dig exists"
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "inventory not restored"
  grep -E '^exec s11-0-probe-A-' "$ROOT/tart.log" >/dev/null || die "no exec A"
  grep -E '^exec s11-0-probe-B-' "$ROOT/tart.log" >/dev/null || die "no exec B"
  if [[ -n "$PYTHON" ]]; then
    "$PYTHON" - "$CASE/out/report.json" <<'PY' || die "json shape"
import json, sys
r = json.load(open(sys.argv[1]))
assert r["schema"] == "s11-0-probe/v2"
assert r["status"] == "complete"
assert r["endpoint_source"] == "argument"
assert r["h1_independent_of_ip"] == "unknown"
assert r["probe_budget_sec"] == 1500
assert r["cleanup_budget_sec"] == 90
assert r["reasons"] == []
assert "quic" in r["unmeasured"] and "quic" not in r["measured"]
assert "udp-443" in r["unmeasured"]
assert "tcp-endpoint-443" in r["measured"]
assert "tcp-lan-443" in r["measured"]
assert "lan" not in r["unmeasured"]
by = {}
for s in r["stages"]:
    by[s["stage"]] = s
    assert s["collection"] == "ok"
    assert s["ssh"] == "connected"
    assert s["tart_exec"] == "ok"
    assert s["launch"] == "started"
    assert s["created"] == "yes"
    assert s["ipv6_global"] == "yes"
    assert s["cleanup"] == {"stop": "ok", "delete": "ok", "absent": "yes"}
    ids = {}
    for o in s["observations"]:
        ids[o["id"]] = o["result"]
    for i in ("tcp-gateway-22", "tcp-gateway-53", "tcp-gateway-80", "tcp-gateway-443", "tcp-metadata-80", "tcp-ipv6-public", "tcp-lan-443"):
        assert ids[i] == "refused", (s["stage"], i, ids.get(i))
    assert ids["dns-udp-gateway"] == "connected"
    assert ids["dns-udp-public"] == "connected"
    assert ids["ipv6-global-address"] == "yes"
assert "tcp-endpoint-443" not in dict((o["id"], o["result"]) for o in by["A"]["observations"])
b = dict((o["id"], o["result"]) for o in by["B"]["observations"])
assert b["tcp-endpoint-443"] == "connected"
assert r["tart_help"] == "ok" and "clone" in r["tart_help_subcommands"] and "exec" in r["tart_help_subcommands"]
assert r["tart_exec_help"] == "ok" and "--username" in r["tart_exec_options"] and "--no-interactive" in r["tart_exec_options"]
blob = open(sys.argv[1]).read()
assert "203.0.113.10/32" in blob
PY
  fi
}

test_need_endpoint() {
  setup
  run_probe 6 --stage B "${ARGS[@]}"
  need_ec 2
  common_end
  need_reason endpoint-required
  [[ ! -f "$ROOT/tart.log" ]] || die "tart ran without endpoint"
  rm -rf "$CASE/out" "$ROOT/tart.log"
  run_probe 6 --stage all "${ARGS[@]}"
  need_ec 2
  need_reason endpoint-required
  [[ ! -f "$ROOT/tart.log" ]] || die "tart ran for all without endpoint"
}

test_placeholder() {
  setup
  run_probe 12 --stage B --endpoint-placeholder "${ARGS[@]}"
  need_ec 0
  common_end
  grep -F -q '"endpoint_source":"placeholder"' "$CASE/out/report.json" || die "source"
  grep -F -q '203.0.113.10/32' "$CASE/out/report.json" || die "placeholder argv"
  grep -E '^run s11-0-probe-B-[0-9]+-[0-9a-f]+ --no-graphics --net-softnet --net-softnet-block=0\.0\.0\.0/0 --net-softnet-allow=203\.0\.113\.10/32$' "$ROOT/tart.log" >/dev/null || die "B argv"
  count_eq "$ROOT/nc.log" '203.0.113.10 443' 1
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "inventory"
}

test_both_flags() {
  setup
  run_probe 6 --stage B --endpoint-ip 198.51.100.20 --endpoint-placeholder "${ARGS[@]}"
  need_ec 2
  common_end
  need_reason usage
  grep -F -q '"endpoint_source":"absent"' "$CASE/out/report.json" || die "source leaked"
  if grep -F -q 'net-softnet-allow' "$CASE/out/report.json"; then die "allow leaked"; fi
  [[ ! -f "$ROOT/tart.log" ]] || die "tart ran"
}

test_bad_ips() {
  setup
  local ip
  for ip in 10.1.1.1 127.0.0.1 192.168.1.9 169.254.1.1 172.16.5.5 100.64.1.1 0.1.2.3 224.0.0.1 1.2.3.04; do
    rm -rf "$CASE/out"
    run_probe 6 --stage B --endpoint-ip "$ip" "${ARGS[@]}"
    need_ec 2
    [[ -f "$CASE/out/report.json" ]] || die "no report for $ip"
    if grep -F -q -- "$ip" "$CASE/out/report.json"; then die "ip leaked $ip"; fi
    need_reason endpoint-invalid
    [[ ! -f "$ROOT/tart.log" ]] || die "tart ran for $ip"
  done
  common_end
}

test_unknown_flags() {
  setup
  local flag
  for flag in --golden --clone-name --guest-user --hostname --softnet --ports; do
    rm -rf "$CASE/out" "$ROOT/tart.log"
    run_probe 6 "${ARGS[@]}" "$flag" nope
    need_ec 2
    need_reason usage
    [[ ! -f "$ROOT/tart.log" ]] || die "tart ran for $flag"
  done
  common_end
}

test_dry_run() {
  setup
  run_probe 8 --dry-run --stage all --endpoint-ip 203.0.113.10 --lan-ip 198.51.100.50 "${ARGS[@]}"
  need_ec 0
  common_end
  grep -F -q '"status":"not-run"' "$CASE/out/report.json" || die "status"
  grep -E '^argv: tart run s11-0-probe-A-[0-9]+-[0-9a-f]+ --no-graphics --net-softnet$' "$CASE/stdout" >/dev/null || die "dry A"
  grep -E '^argv: tart run s11-0-probe-B-[0-9]+-[0-9a-f]+ --no-graphics --net-softnet --net-softnet-block=0\.0\.0\.0/0 --net-softnet-allow=203\.0\.113\.10/32$' "$CASE/stdout" >/dev/null || die "dry B"
  grep -F -q '203.0.113.10/32' "$CASE/out/report.json" || die "dry report argv"
  if grep -E '^(clone|run|stop|delete) ' "$ROOT/tart.log"; then die "dry mutated"; fi
  grep -F -q -- '--version' "$ROOT/tart.log" || die "dry skipped version"
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "inventory"
}

test_softnet_mode() {
  setup
  printf '%s\n' 'root -rwxr-xr-x' >"$ROOT/real/softnet.statout"
  run_probe 6 --stage A "${ARGS[@]}"
  need_ec 2
  common_end
  need_reason softnet-not-suid
  no_clone_ops
  printf '%s\n' 'nobody -rwsr-xr-x' >"$ROOT/real/softnet.statout"
  rm -rf "$CASE/out" "$ROOT/tart.log" "$ROOT/stat.log"
  run_probe 6 --stage A "${ARGS[@]}"
  need_ec 2
  need_reason softnet-not-root
  no_clone_ops
}

test_golden_state() {
  setup
  awk '$1=="macos-s2-eval"{$2="running"} {print}' "$ROOT/inventory" >"$ROOT/inventory.tmp"
  mv "$ROOT/inventory.tmp" "$ROOT/inventory"
  run_probe 6 --stage A "${ARGS[@]}"
  need_ec 2
  common_end
  need_reason golden-not-stopped
  no_clone_ops
  awk '$1!="macos-s2-eval"{print}' "$ROOT/inventory.orig" >"$ROOT/inventory"
  rm -rf "$CASE/out" "$ROOT/tart.log"
  run_probe 6 --stage A "${ARGS[@]}"
  need_ec 2
  common_end no
  need_reason golden-missing
  no_clone_ops
}

test_name_in_use() {
  setup
  printf '%s\n' 's11-0-probe-A-99-abc stopped' >>"$ROOT/inventory"
  cp "$ROOT/inventory" "$ROOT/inventory.snap"
  export S11_FORCE_NAME=s11-0-probe-A-99-abc
  run_probe 8 --stage A "${ARGS[@]}"
  need_ec 3
  common_end
  need_reason name-in-use
  grep -F -q '"created":"no"' "$CASE/out/report.json" || die "created"
  if grep -E '^clone ' "$ROOT/tart.log"; then die "cloned despite name in use"; fi
  [[ ! -f "$ROOT/stop.args" && ! -f "$ROOT/delete.args" ]] || die "deleted in-use name"
  cmp -s "$ROOT/inventory" "$ROOT/inventory.snap" || die "inventory changed"
}

test_bad_force_name() {
  setup
  export S11_FORCE_NAME=macos-s2-eval
  run_probe 6 --stage A "${ARGS[@]}"
  need_ec 2
  common_end
  need_reason usage
  [[ ! -f "$ROOT/tart.log" ]] || die "tart ran"
  export S11_FORCE_NAME=s11-0-probe-keep
  rm -rf "$CASE/out"
  run_probe 6 --stage A "${ARGS[@]}"
  need_ec 2
  need_reason usage
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "inventory"
}

test_fail_clone() {
  setup
  touch "$ROOT/fail_clone"
  run_probe 8 --stage A "${ARGS[@]}"
  need_ec 3
  common_end
  need_reason clone-failed
  [[ ! -f "$ROOT/stop.args" && ! -f "$ROOT/delete.args" ]] || die "deleted after failed clone"
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "inventory"
}

test_omit_clone() {
  setup
  touch "$ROOT/omit_clone"
  run_probe 8 --stage A "${ARGS[@]}"
  need_ec 3
  common_end
  need_reason clone-unconfirmed
  [[ ! -f "$ROOT/stop.args" && ! -f "$ROOT/delete.args" ]] || die "deleted unconfirmed clone"
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "inventory"
}

test_partial_clone() {
  setup
  touch "$ROOT/partial_clone"
  run_probe 8 --stage A "${ARGS[@]}"
  need_ec 4
  common_end
  need_reason clone-failed
  grep -F -q '"created":"yes"' "$CASE/out/report.json" || die "not owned"
  grep -F -q '"collection":"failed"' "$CASE/out/report.json" || die "collection"
  if grep -F -q '"status":"complete"' "$CASE/out/report.json"; then die "complete"; fi
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "partial clone left behind"
}

test_hang_version() {
  setup
  touch "$ROOT/hang_version"
  export S11_BOUND_SEC=2
  run_probe 8 --stage A "${ARGS[@]}"
  need_ec 2
  common_end
  need_reason tart-timeout
  no_clone_ops
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "inventory"
}

test_hang_list() {
  setup
  touch "$ROOT/hang_list"
  export S11_BOUND_SEC=2
  run_probe 8 --stage A "${ARGS[@]}"
  need_ec 2
  common_end
  need_reason list-timeout
  no_clone_ops
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "inventory"
}

test_hang_ip() {
  setup
  touch "$ROOT/hang_ip"
  export S11_BOUND_SEC=2
  run_probe 10 --stage A "${ARGS[@]}"
  need_ec 4
  common_end
  need_reason collection-failed
  grep -F -q '"ssh":"timeout"' "$CASE/out/report.json" || die "ssh result"
  if grep -F -q '"status":"complete"' "$CASE/out/report.json"; then die "complete"; fi
  grep -E '^exec s11-0-probe-A-' "$ROOT/tart.log" >/dev/null || die "exec skipped"
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "clone not cleaned"
}

test_hang_list_after_delete() {
  setup
  touch "$ROOT/hang_list_after_delete"
  export S11_BOUND_SEC=2
  run_probe 12 --stage A "${ARGS[@]}"
  need_ec 5
  common_end
  need_reason list-timeout
  need_reason absent-unknown
  grep -F -q '"collection":"ok"' "$CASE/out/report.json" || die "collection"
  if grep -F -q '"status":"complete"' "$CASE/out/report.json"; then die "complete"; fi
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "delete did not happen before hung list"
}

test_fail_stop_delete() {
  setup
  touch "$ROOT/fail_stop" "$ROOT/fail_delete"
  run_probe 12 --stage A "${ARGS[@]}"
  need_ec 5
  common_end
  need_reason stop-failed
  need_reason delete-failed
  need_reason still-present
  grep -F -q '"collection":"ok"' "$CASE/out/report.json" || die "collection"
  grep -F -q '"absent":"no"' "$CASE/out/report.json" || die "absent"
  if grep -F -q '"status":"complete"' "$CASE/out/report.json"; then die "complete"; fi
  local n
  n=$(sed -n 's/^clone [^ ]* \(s11-0-probe-A-[^ ]*\)$/\1/p' "$ROOT/tart.log")
  [[ -n "$n" ]] || die "no clone"
  assert_has "$n"
}

test_fail_stop_only() {
  setup
  touch "$ROOT/fail_stop"
  run_probe 12 --stage A "${ARGS[@]}"
  need_ec 5
  common_end
  need_reason stop-failed
  grep -F -q '"absent":"yes"' "$CASE/out/report.json" || die "absent"
  grep -F -q '"collection":"ok"' "$CASE/out/report.json" || die "collection"
  if grep -F -q '"status":"complete"' "$CASE/out/report.json"; then die "complete after stop failure"; fi
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "vm left after delete"
}

test_signals() {
  setup
  touch "$ROOT/slow_stop"
  start_probe 15 --stage A "${ARGS[@]}"
  local i=0
  while [[ ! -f "$ROOT/stop.entered" && "$i" -lt 100 ]]; do
    sleep 0.1
    i=$((i + 1))
  done
  [[ -f "$ROOT/stop.entered" ]] || die "cleanup did not start"
  kill -TERM "$PROBE_PID"
  kill -TERM "$PROBE_PID" || true
  kill -INT "$PROBE_PID" || true
  kill -HUP "$PROBE_PID" || true
  sleep 0.3
  kill -0 "$PROBE_PID" || die "signals aborted cleanup"
  finish_probe
  need_ec 0
  common_end
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "cleanup incomplete"
  grep -F -q '"status":"complete"' "$CASE/out/report.json" || die "status"
}

test_guest_nonzero() {
  setup
  touch "$ROOT/guest_nonzero"
  run_probe 12 --stage A "${ARGS[@]}"
  need_ec 4
  common_end
  grep -F -q '"collection":"failed"' "$CASE/out/report.json" || die "collection"
  grep -F -q '"ssh":"connected"' "$CASE/out/report.json" || die "ssh"
  if grep -F -q '"status":"complete"' "$CASE/out/report.json"; then die "complete"; fi
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "not cleaned"
}

test_guest_hang() {
  setup
  touch "$ROOT/guest_hang"
  export S11_BOUND_SEC=2
  run_probe 10 --stage A "${ARGS[@]}"
  need_ec 4
  common_end
  grep -F -q '"ssh":"timeout"' "$CASE/out/report.json" || die "ssh"
  grep -F -q '"collection":"failed"' "$CASE/out/report.json" || die "collection"
  if grep -F -q '"status":"complete"' "$CASE/out/report.json"; then die "complete"; fi
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "not cleaned"
}

test_guest_fail() {
  setup
  touch "$ROOT/guest_fail"
  run_probe 12 --stage A "${ARGS[@]}"
  need_ec 4
  common_end
  grep -F -q '"ssh":"tool-error"' "$CASE/out/report.json" || die "ssh"
  grep -F -q '"collection":"failed"' "$CASE/out/report.json" || die "collection"
  grep -E '^exec s11-0-probe-A-' "$ROOT/tart.log" >/dev/null || die "exec skipped"
  if grep -F -q '"status":"complete"' "$CASE/out/report.json"; then die "complete"; fi
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "not cleaned"
}

test_fail_run() {
  setup
  touch "$ROOT/fail_run"
  run_probe 10 --stage A "${ARGS[@]}"
  need_ec 4
  common_end
  grep -F -q '"launch":"tool-error"' "$CASE/out/report.json" || die "launch"
  grep -F -q '"tart_exec":"ok"' "$CASE/out/report.json" || die "exec after launch failure"
  grep -F -q '"collection":"failed"' "$CASE/out/report.json" || die "collection"
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "not cleaned"
}

test_no_dig() {
  setup
  rm -f "$ROOT/bin/dig"
  run_probe 12 --stage A "${ARGS[@]}"
  need_ec 0
  common_end
  [[ -s "$ROOT/host.log" ]] || die "host not used"
  if grep -F -q '127.0.0.1' "$ROOT/host.log"; then die "host target"; fi
  grep -F -q '198.51.100.1' "$ROOT/host.log" || die "gateway dns target"
  grep -F -q '1.1.1.1' "$ROOT/host.log" || die "public dns target"
  [[ ! -s "$ROOT/nslookup.log" ]] || die "nslookup used"
  grep -F -q '"id":"dns-udp-gateway","result":"tool-error"' "$CASE/out/report.json" || die "dns gw"
  grep -F -q '"id":"dns-udp-public","result":"tool-error"' "$CASE/out/report.json" || die "dns pub"
  grep -F -q '"collection":"ok"' "$CASE/out/report.json" || die "collection"
  grep -F -q '"status":"complete"' "$CASE/out/report.json" || die "status"
  if [[ -n "$PYTHON" ]]; then
    "$PYTHON" - "$CASE/out/report.json" <<'PY' || die "stage A coverage"
import json, sys
r = json.load(open(sys.argv[1]))
assert "tcp-endpoint-443" in r["unmeasured"]
assert "tcp-endpoint-443" not in r["measured"]
assert "lan" in r["unmeasured"]
assert "quic" in r["unmeasured"]
PY
  fi
}

test_stage_c() {
  setup
  run_probe 8 --stage C "${ARGS[@]}"
  need_ec 0
  common_end
  grep -F -q '"status":"complete"' "$CASE/out/report.json" || die "status"
  if grep -E '^(clone|run|stop|delete|ip) ' "$ROOT/tart.log"; then die "stage C mutated"; fi
  if grep -E '^exec s11' "$ROOT/tart.log"; then die "stage C exec vm"; fi
  grep -F -q 'exec --help' "$ROOT/tart.log" || die "no exec help"
  if [[ -n "$PYTHON" ]]; then
    "$PYTHON" - "$CASE/out/report.json" <<'PY' || die "stage C json"
import json, sys
r = json.load(open(sys.argv[1]))
assert r["stages"] == []
assert "tart-help" in r["measured"] and "tart-exec-help" in r["measured"]
assert "quic" in r["unmeasured"]
assert r["endpoint_source"] == "absent"
PY
  fi
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "inventory"
}

test_help() {
  setup
  run_probe 6 "${ARGS[@]}" --help
  need_ec 2
  common_end
  need_reason help
  grep -F -q 's11-0-probe: usage:' "$CASE/stderr" || die "usage text"
  [[ ! -f "$ROOT/tart.log" ]] || die "tart ran"
}

run_case() {
  local name=$1
  shift
  CASE=$(mktemp -d "$BASE/case.XXXXXX")
  local start=$SECONDS
  if ( trap - EXIT; "$@" ); then
    echo "ok $name $((SECONDS - start))s"
    rm -rf "$CASE"
  else
    echo "FAILED $name" >&2
    exit 1
  fi
}

link_sys
install_templates
static_checks
echo "static ok"
run_case happy test_happy
run_case need-endpoint test_need_endpoint
run_case placeholder test_placeholder
run_case both-flags test_both_flags
run_case bad-ips test_bad_ips
run_case unknown-flags test_unknown_flags
run_case dry-run test_dry_run
run_case softnet test_softnet_mode
run_case golden test_golden_state
run_case name-in-use test_name_in_use
run_case bad-force test_bad_force_name
run_case fail-clone test_fail_clone
run_case omit-clone test_omit_clone
run_case partial-clone test_partial_clone
run_case hang-version test_hang_version
run_case hang-list test_hang_list
run_case hang-ip test_hang_ip
run_case hang-list-after-delete test_hang_list_after_delete
run_case fail-stop-delete test_fail_stop_delete
run_case fail-stop-only test_fail_stop_only
run_case signals test_signals
run_case guest-nonzero test_guest_nonzero
run_case guest-hang test_guest_hang
run_case guest-fail test_guest_fail
run_case fail-run test_fail_run
run_case no-dig test_no_dig
run_case stage-c test_stage_c
run_case help test_help
echo "ALL_OK ${SECONDS}s"
[[ "$SECONDS" -lt 30 ]] || { echo "suite took ${SECONDS}s" >&2; exit 1; }

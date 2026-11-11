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
  local st=$? pid=""
  if [[ -n "${CASE:-}" && -f "$CASE/stub/target.pid" ]]; then
    pid=$(cat "$CASE/stub/target.pid" 2>/dev/null || true)
    [[ -n "$pid" ]] && kill -KILL "$pid" 2>/dev/null || true
  fi
  if [[ -n "${CASE:-}" && -f "$CASE/stub/bg.pid" ]]; then
    pid=$(cat "$CASE/stub/bg.pid" 2>/dev/null || true)
    [[ -n "$pid" ]] && kill -KILL "$pid" 2>/dev/null || true
  fi
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

kill_target() {
  local pid=""
  [[ -n "${ROOT:-}" && -f "$ROOT/target.pid" ]] || return 0
  pid=$(cat "$ROOT/target.pid" 2>/dev/null || true)
  [[ -n "$pid" ]] || return 0
  kill -KILL "$pid" 2>/dev/null || true
}

die() {
  kill_target
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
  for c in bash awk sed grep mktemp head tr cut od readlink dirname id wc sleep \
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
    'ignore_term() { echo $$ >"$ROOT/target.pid"; trap '"'"'echo TERM >>"$ROOT/term.log"'"'"' TERM; trap "" PIPE; while true; do sleep 0.2; done; }' \
    'overflow_ignore() { echo $$ >"$ROOT/target.pid"; trap '"'"'echo TERM >>"$ROOT/term.log"'"'"' TERM; trap "" XFSZ PIPE; i=0; while [[ $i -lt 1500 ]]; do printf "%s\n" "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"; i=$((i+1)); done; while true; do sleep 0.2; done; }' \
    'has() { awk -v s="$1" '"'"'$1==s{f=1} END{exit !f}'"'"' "$INV"; }' \
    'emit_vm() { local run=false; case "$2" in running|Running|RUNNING) run=true ;; esac; printf "  {\n    \"Size\" : 35,\n    \"Disk\" : 50,\n    \"Source\" : \"local\",\n    \"State\" : \"%s\",\n    \"Name\" : \"%s\",\n    \"Running\" : %s,\n    \"Accessed\" : \"2026-10-31T17:13:29Z\"\n" "$2" "$1" "$run"; }' \
    'show_list() { : >"$ROOT/names.rows"; while read -r n st; do [[ -n "$n" ]] || continue; printf "%s %s\n" "$n" "$st" >>"$ROOT/names.rows"; done <"$INV"; [[ -f "$ROOT/list_collide" ]] && printf "%s %s\n" "s11-0-probe-A-${PPID}-deadbeef" stopped >>"$ROOT/names.rows"; count=$(wc -l <"$ROOT/names.rows" | tr -d " "); if [[ "$count" -eq 0 ]]; then printf "[]\n"; return 0; fi; printf "[\n"; i=0; while read -r n st; do i=$((i+1)); emit_vm "$n" "$st"; if [[ "$i" -lt "$count" ]]; then printf "  },\n"; else printf "  }\n"; fi; done <"$ROOT/names.rows"; printf "]\n"; }' \
    'cmd=${1:-}; shift || true' \
    'case "$cmd" in' \
    '  --version) [[ -f "$ROOT/bg_hold" ]] && { sleep 30 & echo $! >"$ROOT/bg.pid"; echo "stub-tart 0"; exit 0; }; [[ -f "$ROOT/hang_version" ]] && hang; echo "stub-tart 0"; exit 0 ;;' \
    '  --help) [[ -f "$ROOT/fail_help" ]] && exit 1; printf "%s\n" "commands: clone list run stop delete ip exec"; exit 0 ;;' \
    '  list)' \
    '    [[ "${1:-}" == "--format" && "${2:-}" == json ]] || exit 2' \
    '    shift 2' \
    '    [[ -f "$ROOT/hang_list" ]] && hang' \
    '    [[ -f "$ROOT/hang_list_ignore" ]] && ignore_term' \
    '    [[ -f "$ROOT/overflow_list" ]] && overflow_ignore' \
    '    [[ -f "$ROOT/hang_list_after_delete" && -s "$ROOT/delete.args" && ! -f "$ROOT/list_hung" ]] && { touch "$ROOT/list_hung"; hang; }' \
    '    [[ -f "$ROOT/list_void" ]] && exit 0' \
    '    [[ -f "$ROOT/list_trunc" ]] && { printf "%s\n" "[" "  {" "    \"Name\" : \"macos-s2-eval\"," "    \"State\" : \"stopped\"" "  }"; exit 0; }' \
    '    [[ -f "$ROOT/list_no_state" ]] && { printf "%s\n" "[" "  {" "    \"Name\" : \"macos-s2-eval\"," "    \"Running\" : false" "  }" "]"; exit 0; }' \
    '    [[ -f "$ROOT/list_dup_name" ]] && { printf "%s\n" "[" "  {" "    \"Name\" : \"macos-s2-eval\"," "    \"Name\" : \"other.alias\"," "    \"State\" : \"stopped\"" "  }" "]"; exit 0; }' \
    '    [[ -f "$ROOT/list_dup_vm" ]] && { printf "%s\n" "[" "  {" "    \"Name\" : \"macos-s2-eval\"," "    \"State\" : \"stopped\"" "  }," "  {" "    \"Name\" : \"macos-s2-eval\"," "    \"State\" : \"running\"" "  }" "]"; exit 0; }' \
    '    [[ -f "$ROOT/list_bad_state" ]] && { printf "%s\n" "[" "  {" "    \"Name\" : \"macos-s2-eval\"," "    \"State\" : \"paused\"" "  }" "]"; exit 0; }' \
    '    show_list; exit 0 ;;' \
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
    '    [[ -f "$ROOT/run_exit" ]] && { echo $$ >"$ROOT/target.pid"; sleep 0.12; exit 0; }' \
    '    awk -v n="$1" '"'"'$1==n{$2="running"} {print}'"'"' "$INV" >"$INV.tmp" && mv "$INV.tmp" "$INV"' \
    '    trap "exit 0" TERM INT HUP' \
    '    while true; do sleep 0.2; done ;;' \
    '  stop)' \
    '    printf "%s\n" "$1" >>"$ROOT/stop.args"' \
    '    touch "$ROOT/stop.entered"' \
    '    [[ -f "$ROOT/slow_stop" ]] && sleep 0.45' \
    '    [[ -f "$ROOT/fail_stop" ]] && exit 1' \
    '    awk -v n="$1" '"'"'$1==n{$2="stopped"} {print}'"'"' "$INV" >"$INV.tmp" && mv "$INV.tmp" "$INV"' \
    '    exit 0 ;;' \
    '  delete)' \
    '    printf "%s\n" "$1" >>"$ROOT/delete.args"' \
    '    [[ -f "$ROOT/fail_delete" ]] && exit 1' \
    '    awk -v n="$1" '"'"'$1!=n{print}'"'"' "$INV" >"$INV.tmp" && mv "$INV.tmp" "$INV"' \
    '    exit 0 ;;' \
    '  ip) [[ -f "$ROOT/hang_ip" ]] && hang; has "$1" || exit 1' \
    '    if [[ -f "$ROOT/ip_fail_n" ]]; then n=0; [[ -f "$ROOT/ip.count" ]] && n=$(cat "$ROOT/ip.count"); n=$((n+1)); echo "$n" >"$ROOT/ip.count"; fn=$(cat "$ROOT/ip_fail_n"); [[ "$n" -le "$fn" ]] && { echo "no address" >&2; exit 1; }; fi' \
    '    echo "192.0.2.20"; exit 0 ;;' \
    '  exec)' \
    '    if [[ "${1:-}" == "--help" ]]; then [[ -f "$ROOT/fail_exec_help" ]] && exit 1; printf "%s\n" "--username" "--no-interactive"; exit 0; fi' \
    '    has "$1" || exit 1; exit 0 ;;' \
    '  *) echo "bad tart $cmd" >&2; exit 2 ;;' \
    'esac'
  write_stub ssh \
    'ROOT=${S11_STUB_ROOT:?}' \
    'printf "%s\n" "$*" >>"$ROOT/ssh.log"' \
    'kh=""; for a in "$@"; do case "$a" in UserKnownHostsFile=*) kh=${a#UserKnownHostsFile=} ;; esac; done' \
    'if [[ -n "$kh" && -f "$kh" ]]; then cp "$kh" "$ROOT/known_hosts.copy"; fi' \
    'ready=0; for a in "$@"; do [[ "$a" == "/usr/bin/true" ]] && ready=1; done' \
    'if [[ "$ready" == 1 ]]; then' \
    '  if [[ -f "$ROOT/ssh_fail_n" ]]; then n=0; [[ -f "$ROOT/ssh.count" ]] && n=$(cat "$ROOT/ssh.count"); n=$((n+1)); echo "$n" >"$ROOT/ssh.count"; fn=$(cat "$ROOT/ssh_fail_n"); [[ "$n" -le "$fn" ]] && exit 255; fi' \
    '  exit 0; fi' \
    'case " $* " in *s11m.XXXXXX*) cat >"$ROOT/init.body"; printf "init-ok\n"; exit 0 ;; esac' \
    'remote=""; for remote in "$@"; do :; done' \
    'case "$remote" in upload|verify\ *|run\ C1|run\ C2|run\ C8) cat >/dev/null; exit 0 ;; esac' \
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
    'printf "%s\n" "$*" >>"$ROOT/nc.argv"' \
    'z=0; G=0; w=0; host=""; port=""' \
    'while [[ $# -gt 0 ]]; do' \
    '  case "$1" in' \
    '    -z) z=1; shift ;;' \
    '    -G) [[ "${2:-}" == 2 ]] || exit 2; G=1; shift 2 ;;' \
    '    -w) [[ "${2:-}" == 2 ]] || exit 2; w=1; shift 2 ;;' \
    '    *) if [[ -z "$host" ]]; then host=$1; else port=$1; fi; shift ;;' \
    '  esac' \
    'done' \
    '[[ "$z" == 1 && "$G" == 1 && "$w" == 1 && -n "$host" && -n "$port" ]] || exit 2' \
    'printf "%s %s\n" "$host" "$port" >>"$ROOT/nc.log"' \
    'if [[ "$host" == "203.0.113.10" && "$port" == "443" ]]; then exit 0; fi' \
    'echo "Connection refused" >&2' \
    'exit 1'
  write_stub dig \
    'ROOT=${S11_STUB_ROOT:?}' \
    'printf "%s\n" "$*" >>"$ROOT/dig.log"' \
    'got=" $* "' \
    'for o in -r +notcp +ignore +time=2 +tries=1; do case "$got" in *" $o "*) ;; *) exit 2 ;; esac; done' \
    'case "$got" in *" +tcp "*|*" +vc "*) exit 2 ;; esac' \
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
    'ROOT=${S11_STUB_ROOT:?}' \
    '[[ -f "$ROOT/fail_route" ]] && exit 1' \
    'printf "%s\n" "gateway: 198.51.100.1"'
  write_stub ifconfig \
    'ROOT=${S11_STUB_ROOT:?}' \
    '[[ -f "$ROOT/fail_ifconfig" ]] && exit 1' \
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
  mkdir -p "$ROOT/bin" "$ROOT/mid" "$ROOT/real" "$CASE/thome" "$CASE/tmp"
  cp "$BASE/tpl/"* "$ROOT/bin/"
  [[ -x "$ROOT/bin/tart" && -x "$ROOT/bin/stat" ]] || die "stubs not installed"
  printf '%s\n' 'macos-s2-eval stopped' 'macos-s2-eval.alt stopped' 'vm.dotted-name stopped' 's11-0-probe-keep stopped' 's11-0-probe-A-1-deadbee1 stopped' >"$ROOT/inventory"
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
  unset BASH_ENV S11_XTRACE S11_BOUND_SEC S11_BOUND_POLLS S11_FORCE_NAME S11_PROBE_BUDGET S11_CLEANUP_BUDGET S11_REAP_POLLS S11_FAIL_ROW_N S11_FAIL_STAGE S11_PROBE_MODE
  ARGS=(--ssh-key "$CASE/id_key" --tart-home "$CASE/thome" --out "$CASE/out")
}

start_probe() {
  local limit=$1
  shift
  PROBE_PID=""
  WATCHER=""
  set -m
  set +b
  if [[ "${S11_XTRACE:-}" == 1 ]]; then
    BASH_XTRACEFD=9 bash -x "$PROBE" "$@" >"$CASE/stdout" 2>"$CASE/stderr" 9>"$CASE/xtrace" &
  else
    bash "$PROBE" "$@" >"$CASE/stdout" 2>"$CASE/stderr" &
  fi
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
      [[ "$n" == "macos-s2-eval" || "$n" == "macos-s2-eval.alt" || "$n" == "vm.dotted-name" || "$n" == "s11-0-probe-keep" || "$n" == "s11-0-probe-A-1-deadbee1" ]] && die "$f touched $n"
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
  assert_has macos-s2-eval.alt
  assert_has vm.dotted-name
  assert_has s11-0-probe-keep
  assert_has s11-0-probe-A-1-deadbee1
  if [[ -f "$ROOT/tart.log" ]] && grep -F -q 'list ' "$ROOT/tart.log"; then
    grep -F -q 'list --format json' "$ROOT/tart.log" || die "list format"
  fi
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
  [[ "$n" -lt 1470 ]] || die "probe has $n lines"
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
  for pat in S11_FORCE_NAME CMD_LIVE CMD_PID nslookup 'run.out' mkfifo proc_snap same_proc our_child drop_readers 'ps -p'; do
    if grep -n -F -- "$pat" "$PROBE"; then die "probe contains $pat"; fi
  done
  grep -F -q 'dig -r +notcp +ignore +time=2 +tries=1' "$PROBE"
  grep -F -q 'ambiguous-destination' "$PROBE"
  grep -F -q 'nc -z -G 2 -w 2' "$PROBE"
  grep -F -q '累计清理额度' "$RUNBOOK"
  grep -F -q '中途清理计入探测计时' "$RUNBOOK"
  grep -F -q '不承诺精确的总硬上限' "$RUNBOOK"
  grep -F -q '可能没有最终报告' "$RUNBOOK"
  grep -F -q '拒绝已存在' "$RUNBOOK"
  grep -F -q '不得按前缀' "$RUNBOOK"
  grep -F -q 'ambiguous-destination' "$RUNBOOK"
  if grep -F -q '26 分 30 秒' "$RUNBOOK"; then die "runbook hard cap"; fi
  if grep -F -q '1590' "$RUNBOOK"; then die "runbook 1590"; fi
  n=$(grep -F -c 'tart list --format json' "$PROBE" || true)
  [[ "$n" -ge 1 ]] || die "json list"
  n2=$(grep -F -o 'tart list' "$PROBE" | wc -l | tr -d ' ')
  [[ "$n" == "$n2" ]] || die "bare tart list $n2 vs $n"
  if grep -F -q 'Disk Size' "$PROBE" "$RUNBOOK"; then die "column offsets left"; fi
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
  grep -F -q -- '-r' "$ROOT/dig.log" || die "dig -r"
  grep -F -q -- '+notcp' "$ROOT/dig.log" || die "dig +notcp"
  grep -F -q -- '+ignore' "$ROOT/dig.log" || die "dig +ignore"
  grep -F -q -- '+time=2' "$ROOT/dig.log" || die "dig time"
  grep -F -q -- '+tries=1' "$ROOT/dig.log" || die "dig tries"
  if grep -F -q -- '+tcp' "$ROOT/dig.log" || grep -F -q -- '+vc' "$ROOT/dig.log"; then die "dig tcp fallback"; fi
  grep -F -q -- '-z -G 2 -w 2' "$ROOT/nc.argv" || die "nc options"
  [[ ! -s "$ROOT/host.log" ]] || die "host used while dig exists"
  [[ ! -s "$ROOT/nslookup.log" ]] || die "nslookup used"
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
assert r["inventory_columns"] == ["json", "Size", "Disk", "Source", "State", "Name", "Running", "Accessed"]
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
assert "2026-10-31T17:13:29Z" not in blob
assert "Disk Size" not in blob
PY
  fi
}

test_need_endpoint() {
  setup
  run_probe 6 --stage B "${ARGS[@]}"
  need_ec 2
  common_end
  need_reason endpoint-required
  grep -F -q '"measured":[]' "$CASE/out/report.json" || die "measured"
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
  grep -F -q '"measured":[]' "$CASE/out/report.json" || die "dry measured"
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "inventory"
}

test_softnet_mode() {
  setup
  printf '%s\n' 'root -rwxr-xr-x' >"$ROOT/real/softnet.statout"
  run_probe 6 --stage A "${ARGS[@]}"
  need_ec 2
  common_end
  need_reason softnet-not-suid
  grep -F -q '"measured":[]' "$CASE/out/report.json" || die "measured"
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
  if grep -F -q 'list-untrusted' "$CASE/out/report.json"; then die "running untrusted"; fi
  grep -F -q '"measured":[]' "$CASE/out/report.json" || die "measured"
  no_clone_ops
  awk '$1!="macos-s2-eval"{print}' "$ROOT/inventory.orig" >"$ROOT/inventory"
  rm -rf "$CASE/out" "$ROOT/tart.log"
  run_probe 6 --stage A "${ARGS[@]}"
  need_ec 2
  common_end no
  need_reason golden-missing
  if grep -F -q 'list-untrusted' "$CASE/out/report.json"; then die "alias untrusted"; fi
  assert_has macos-s2-eval.alt
  no_clone_ops
  : >"$ROOT/inventory"
  rm -rf "$CASE/out" "$ROOT/tart.log"
  run_probe 6 --stage A "${ARGS[@]}"
  need_ec 2
  need_reason golden-missing
  if grep -F -q 'list-untrusted' "$CASE/out/report.json"; then die "empty array untrusted"; fi
  grep -F -q '"inventory_columns":["json"]' "$CASE/out/report.json" || die "empty columns"
  no_clone_ops
  cp "$ROOT/inventory.orig" "$ROOT/inventory"
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
  need_ec 5
  common_end
  need_reason ambiguous-destination
  grep -F -q 'ambiguous-destination' "$CASE/stderr" || die "stderr hint"
  grep -F -q '"created":"no"' "$CASE/out/report.json" || die "created"
  [[ ! -f "$ROOT/stop.args" && ! -f "$ROOT/delete.args" ]] || die "deleted unconfirmed clone"
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "inventory"
}

test_partial_clone() {
  setup
  touch "$ROOT/partial_clone"
  run_probe 8 --stage all --endpoint-ip 203.0.113.10 "${ARGS[@]}"
  need_ec 5
  common_end
  need_reason ambiguous-destination
  [[ ! -f "$ROOT/stop.args" && ! -f "$ROOT/delete.args" ]] || die "deleted ambiguous clone"
  local n
  n=$(sed -n 's/^clone [^ ]* \(s11-0-probe-A-[^ ]*\)$/\1/p' "$ROOT/tart.log")
  [[ -n "$n" ]] || die "no clone attempt"
  assert_has "$n"
  if grep -E '^clone .* s11-0-probe-B-' "$ROOT/tart.log" || grep -E '^run s11-0-probe-B-' "$ROOT/tart.log"; then
    die "entered stage B"
  fi
  grep -F -q '"created":"no"' "$CASE/out/report.json" || die "created"
  if grep -F -q '"status":"complete"' "$CASE/out/report.json"; then die "complete"; fi
  grep -F -q 'ambiguous-destination' "$CASE/stderr" || die "stderr hint"
}

test_hang_version() {
  setup
  touch "$ROOT/hang_version"
  export S11_BOUND_POLLS=8
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
  export S11_BOUND_POLLS=8
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
  export S11_BOUND_POLLS=8
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
  export S11_BOUND_POLLS=8
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
  sleep 0.15
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
  export S11_BOUND_POLLS=8
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
  local m
  m=$(sed -n 's/.*"measured":\[\([^]]*\)\].*/\1/p' "$CASE/out/report.json")
  [[ "$m" != *tcp-gateway-22* && "$m" != *dns-udp-gateway* ]] || die "ssh failure listed measured"
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
  [[ ! -s "$ROOT/host.log" ]] || die "host fallback"
  [[ ! -s "$ROOT/nslookup.log" ]] || die "nslookup fallback"
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
assert "dns-udp-gateway" in r["unmeasured"] and "dns-udp-gateway" not in r["measured"]
assert "dns-udp-public" in r["unmeasured"] and "dns-udp-public" not in r["measured"]
assert "lan" in r["unmeasured"]
assert "quic" in r["unmeasured"]
PY
  fi
}

child_dead() {
  local pid
  [[ -s "$ROOT/target.pid" ]] || die "no target pid"
  pid=$(cat "$ROOT/target.pid")
  if kill -0 "$pid" 2>/dev/null; then die "child $pid still alive"; fi
}

test_kill_timeout() {
  setup
  touch "$ROOT/hang_list_ignore"
  export S11_BOUND_POLLS=8
  run_probe 8 --stage A "${ARGS[@]}"
  need_ec 2
  child_dead
  grep -q TERM "$ROOT/term.log" || die "no TERM before kill"
  common_end
  need_reason list-timeout
  no_clone_ops
}

test_kill_cancel() {
  setup
  touch "$ROOT/hang_list_ignore"
  start_probe 8 --stage A "${ARGS[@]}"
  local i=0
  while [[ ! -s "$ROOT/target.pid" && "$i" -lt 80 ]]; do sleep 0.05; i=$((i + 1)); done
  [[ -s "$ROOT/target.pid" ]] || die "no target"
  kill -TERM "$PROBE_PID"
  finish_probe
  need_ec 2
  child_dead
  grep -q TERM "$ROOT/term.log" || die "no TERM before kill"
  common_end
  need_reason list-aborted
  no_clone_ops
}

test_kill_overflow() {
  setup
  touch "$ROOT/overflow_list"
  run_probe 8 --stage A "${ARGS[@]}"
  need_ec 2
  child_dead
  grep -q TERM "$ROOT/term.log" || die "no TERM before kill"
  common_end
  need_reason capture-overflow
  need_reason list-untrusted
  if grep -F -q 'golden-missing' "$CASE/out/report.json"; then die "overflow looked like absence"; fi
  no_clone_ops
}

test_bg_hold() {
  setup
  touch "$ROOT/bg_hold"
  export S11_BOUND_SEC=1
  run_probe 4 --dry-run --stage A "${ARGS[@]}"
  need_ec 0
  common_end
  grep -F -q '"status":"not-run"' "$CASE/out/report.json" || die "status"
  [[ -s "$ROOT/bg.pid" ]] || die "no descendant"
  kill -KILL "$(cat "$ROOT/bg.pid")" 2>/dev/null || true
}

test_no_kill_after_exit() {
  setup
  touch "$ROOT/run_exit" "$ROOT/guest_hang"
  export S11_BOUND_POLLS=4 S11_XTRACE=1
  run_probe 6 --stage A "${ARGS[@]}"
  need_ec 4
  common_end
  local pid
  [[ -s "$ROOT/target.pid" ]] || die "run did not start"
  [[ -s "$CASE/xtrace" ]] || die "no xtrace"
  pid=$(cat "$ROOT/target.pid")
  if kill -0 "$pid" 2>/dev/null; then die "run still alive"; fi
  if grep -E -q "kill -(TERM|KILL)( --)? ${pid}( |$)" "$CASE/xtrace"; then
    die "signaled finished job $pid"
  fi
  grep -F -q '"collection":"failed"' "$CASE/out/report.json" || die "collection"
}

test_dry_precheck() {
  setup
  printf '%s\n' 'root -rwxr-xr-x' >"$ROOT/real/softnet.statout"
  run_probe 6 --dry-run --stage A "${ARGS[@]}"
  need_ec 2
  common_end
  need_reason softnet-not-suid
  grep -F -q '"status":"precheck-failed"' "$CASE/out/report.json" || die "status"
  grep -F -q '"measured":[]' "$CASE/out/report.json" || die "measured"
  no_clone_ops
}

test_list_untrusted() {
  setup
  local mode
  for mode in list_void list_trunc list_no_state list_dup_name list_dup_vm list_bad_state; do
    rm -rf "$CASE/out" "$ROOT/tart.log"
    rm -f "$ROOT/list_void" "$ROOT/list_trunc" "$ROOT/list_no_state" "$ROOT/list_dup_name" "$ROOT/list_dup_vm" "$ROOT/list_bad_state"
    touch "$ROOT/$mode"
    run_probe 6 --stage A "${ARGS[@]}"
    need_ec 2
    need_reason list-untrusted
    if grep -F -q 'golden-missing' "$CASE/out/report.json"; then die "untrusted as missing ($mode)"; fi
    if grep -F -q 'golden-unknown' "$CASE/out/report.json"; then die "untrusted as unknown ($mode)"; fi
    if grep -F -q 'golden-not-stopped' "$CASE/out/report.json"; then die "untrusted as running ($mode)"; fi
    if [[ "$mode" == list_no_state ]]; then
      grep -F -q '"inventory_columns":["json","Name","Running"]' "$CASE/out/report.json" || die "columns"
    fi
    if [[ "$mode" == list_void ]]; then
      grep -F -q '"inventory_columns":[]' "$CASE/out/report.json" || die "void columns"
    fi
    if [[ "$mode" == list_bad_state ]] && grep -F -q 'paused' "$CASE/out/report.json"; then die "state value recorded"; fi
    no_clone_ops
  done
  common_end
}

test_name_in_use() {
  setup
  printf '%s\n' '#!/bin/bash' 'printf " deadbeef\n"' >"$ROOT/bin/od"
  chmod +x "$ROOT/bin/od"
  touch "$ROOT/list_collide"
  run_probe 6 --stage A "${ARGS[@]}"
  need_ec 3
  common_end
  need_reason name-in-use
  if grep -F -q 'list-untrusted' "$CASE/out/report.json"; then die "collide untrusted"; fi
  if grep -F -q 'golden-missing' "$CASE/out/report.json"; then die "collide missing"; fi
  no_clone_ops
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "inventory"
}

test_boot_ready() {
  setup
  echo 1 >"$ROOT/ip_fail_n"
  echo 1 >"$ROOT/ssh_fail_n"
  run_probe 12 --stage A "${ARGS[@]}"
  need_ec 0
  common_end
  local n
  n=$(cat "$ROOT/ip.count"); [[ "$n" -ge 2 ]] || die "ip retries $n"
  n=$(cat "$ROOT/ssh.count"); [[ "$n" -ge 2 ]] || die "ssh retries $n"
  grep -F -q '"collection":"ok"' "$CASE/out/report.json" || die "collection"
  grep -F -q '"ssh":"connected"' "$CASE/out/report.json" || die "ssh"
  grep -E '^exec s11-0-probe-A-' "$ROOT/tart.log" >/dev/null || die "exec"
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "inventory"
}

test_cleanup_blocks_b() {
  setup
  touch "$ROOT/fail_delete"
  run_probe 12 --stage all --endpoint-ip 203.0.113.10 "${ARGS[@]}"
  need_ec 5
  common_end
  need_reason delete-failed
  need_reason still-present
  local n clones
  clones=$(grep -c '^clone ' "$ROOT/tart.log" || true)
  [[ "$clones" == 1 ]] || die "clone count $clones"
  if grep -E 's11-0-probe-B-' "$ROOT/tart.log"; then die "entered B"; fi
  n=$(sed -n 's/^clone [^ ]* \(s11-0-probe-A-[^ ]*\)$/\1/p' "$ROOT/tart.log")
  [[ -n "$n" ]] || die "no clone"
  assert_has "$n"
  grep -F -q '"created":"yes"' "$CASE/out/report.json" || die "created"
  grep -F -q '"absent":"no"' "$CASE/out/report.json" || die "absent"
  grep -F -q '"collection":"ok"' "$CASE/out/report.json" || die "collection"
  if grep -F -q '"status":"complete"' "$CASE/out/report.json"; then die "complete"; fi
  if [[ -n "$PYTHON" ]]; then
    "$PYTHON" - "$CASE/out/report.json" <<'PY' || die "stage overwrite"
import json, sys
r = json.load(open(sys.argv[1]))
assert len(r["stages"]) == 1 and r["stages"][0]["stage"] == "A"
assert r["stages"][0]["created"] == "yes"
assert r["stages"][0]["cleanup"]["absent"] == "no"
PY
  fi
}

test_help_fail() {
  setup
  touch "$ROOT/fail_help"
  run_probe 6 --stage C "${ARGS[@]}"
  need_ec 4
  common_end
  grep -F -q '"tart_help":"tool-error"' "$CASE/out/report.json" || die "help"
  if grep -F -q '"status":"complete"' "$CASE/out/report.json"; then die "complete"; fi
  local m
  m=$(sed -n 's/.*"measured":\[\([^]]*\)\].*/\1/p' "$CASE/out/report.json")
  [[ "$m" != *tart-help* ]] || die "failed help measured"
  no_clone_ops
}

test_output_exists() {
  setup
  mkdir -p "$CASE/out"
  printf '%s\n' '{"stale":true}' >"$CASE/out/report.json"
  run_probe 6 --stage A "${ARGS[@]}"
  need_ec 2
  grep -F -q 'output-exists' "$CASE/stderr" || die "no refusal"
  grep -F -q '{"stale":true}' "$CASE/out/report.json" || die "stale report replaced"
  [[ ! -f "$ROOT/tart.log" ]] || die "ran tart"
  rm -rf "$CASE/out"
  ln -s "$CASE/thome" "$CASE/out"
  run_probe 6 --stage A "${ARGS[@]}"
  need_ec 2
  grep -F -q 'output-exists' "$CASE/stderr" || die "symlink accepted"
  [[ ! -f "$ROOT/tart.log" ]] || die "ran tart via symlink"
}

test_route_fail() {
  setup
  touch "$ROOT/fail_route"
  run_probe 12 --stage A "${ARGS[@]}"
  need_ec 0
  common_end
  grep -F -q '"id":"tcp-gateway-22","result":"tool-error"' "$CASE/out/report.json" || die "route tool-error"
  if grep -F -q '"id":"tcp-gateway-22","result":"no-address"' "$CASE/out/report.json"; then die "route became no-address"; fi
  grep -F -q '"collection":"ok"' "$CASE/out/report.json" || die "collection"
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
    if [[ -f "$CASE/stub/target.pid" ]]; then
      local pid
      pid=$(cat "$CASE/stub/target.pid" 2>/dev/null || true)
      [[ -n "$pid" ]] && kill -KILL "$pid" 2>/dev/null || true
    fi
    if [[ -f "$CASE/stub/bg.pid" ]]; then
      local pid
      pid=$(cat "$CASE/stub/bg.pid" 2>/dev/null || true)
      [[ -n "$pid" ]] && kill -KILL "$pid" 2>/dev/null || true
    fi
    rm -rf "$CASE"
  else
    echo "FAILED $name" >&2
    exit 1
  fi
}

write_pinned() {
  printf '%s\n' 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFakeKeyForStageMStubOnly stub' >"$CASE/pinned"
  chmod 600 "$CASE/pinned"
}

write_fake_probe() {
  cat >"$ROOT/bin/s11probe" <<'EOF'
#!/bin/bash
cmd=$1
shift
boundary=-
if [[ "$cmd" == c2 ]]; then boundary=$1; shift; fi
dir=""
role=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --dir) dir=$2; shift 2 ;;
    --role) role=$2; shift 2 ;;
    *) shift ;;
  esac
done
mode=${S11_PROBE_MODE:-}
peer=1
[[ "$cmd" == writer ]] && peer=0
[[ "$mode" == peer0 && "$cmd" == reader ]] && peer=0
digest=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
latency=measured
[[ "$mode" == unmeasured ]] && latency=not_measured
line=""
case "$cmd" in
  writer|reader)
    line="v=1 op=c1 role=${role} verdict=ok reason=- n=8 torn=0 lost=0 integrity=ok latency=${latency} vis_us=1 rtt_us=1 slice_us=1 w_slice_us=1 open_us=1 write_us=1 fsync_us=1 rename_us=1 dir_sync_us=1 read_us=1 peer=${peer} digest=${digest}"
    ;;
  c2)
    line="v=1 op=c2 role=${role} boundary=${boundary} verdict=not-ack reason=- planned=3 attempted=3 renamed=2 visible=2 part=ok dir_synced=0"
    ;;
  c8)
    line="v=1 op=c8 role=${role} verdict=unknown reason=single-principal n=1 ops=0 created=0 elevated=0"
    ;;
esac
if [[ "$mode" == nonewline ]]; then
  printf '%s' "$line" >"$dir/result-$role"
else
  printf '%s\n' "$line" >"$dir/result-$role"
  [[ "$mode" == dup ]] && printf '%s\n' "$line" >>"$dir/result-$role"
fi
if [[ -f "${S11_STUB_ROOT:-}/plant_unknown" ]]; then printf 'x\n' >"$dir/pwned"; fi
if [[ -f "${S11_STUB_ROOT:-}/plant_nl" ]]; then
  nlname=$(printf 'result-host\nX')
  nlname=${nlname%X}
  printf 'x' >"$dir/$nlname"
fi
if [[ "$mode" == hold && "$cmd" == writer ]]; then
  echo $$ >"${S11_STUB_ROOT:?}/target.pid"
  trap '' TERM
  while true; do sleep 0.2; done
fi
exit 0
EOF
  chmod 700 "$ROOT/bin/s11probe"
}

extract_dispatcher() {
  awk '
    index($0, "cat <<'\''END_DISPATCHER'\''") {p=1; next}
    $0=="END_DISPATCHER" && p {exit}
    p {print}
  ' "$PROBE" >"$CASE/dispatcher.sh"
  [[ -s "$CASE/dispatcher.sh" ]] || die "no dispatcher"
}

expect_reject() {
  local cmd=$1 out st
  SSH_ORIGINAL_COMMAND=$cmd bash "$CASE/dispatcher.sh" >"$CASE/disp.out" 2>"$CASE/disp.err"
  st=$?
  out=$(cat "$CASE/disp.out")
  [[ "$st" == 2 ]] || die "reject ec $st for [$cmd]"
  [[ "$out" == reject ]] || die "reject text for [$cmd]: $out"
  if grep -E -q 'uid=|reject-ran' "$CASE/disp.out" "$CASE/disp.err"; then die "reject executed [$cmd]"; fi
}

test_dispatcher_literal() {
  setup
  extract_dispatcher
  bash -n "$CASE/dispatcher.sh" || die "dispatcher syntax"
  expect_reject 'run C1; id'
  expect_reject $'upload\nid'
  expect_reject 'run $(id)'
  expect_reject 'run `id`'
  expect_reject 'run C9'
  expect_reject "$("$PYTHON" -c 'print("z"*80)')"
  SSH_ORIGINAL_COMMAND=upload bash "$CASE/dispatcher.sh" >"$CASE/disp.out"
  [[ "$?" == 3 ]] || die "upload ec"
  grep -F -q fail-dir "$CASE/disp.out" || die "upload fail-dir"
  SSH_ORIGINAL_COMMAND=verify\ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bash "$CASE/dispatcher.sh" >"$CASE/disp.out"
  [[ "$?" == 5 ]] || die "verify ec"
  grep -F -q fail-probe "$CASE/disp.out" || die "verify fail-probe"
  SSH_ORIGINAL_COMMAND='run C1' bash "$CASE/dispatcher.sh" >"$CASE/disp.out"
  [[ "$?" == 6 ]] || die "run ec"
  grep -F -q fail-run "$CASE/disp.out" || die "run fail-run"
  if grep -F -q 'eval ' "$CASE/dispatcher.sh"; then die "dispatcher uses eval"; fi
}

test_stage_m_c1() {
  setup
  write_pinned
  write_fake_probe
  run_probe 20 --stage M --case C1 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 0
  common_end
  grep -E '^run s11-0-probe-M-[0-9]+-[0-9a-f]+ --no-graphics --net-softnet --dir s11mbx:.*/mailbox$' "$ROOT/tart.log" >/dev/null || die "M argv"
  local opt
  for opt in '-F /dev/null' BatchMode=yes StrictHostKeyChecking=yes GlobalKnownHostsFile=/dev/null IdentitiesOnly=yes IdentityAgent=none ClearAllForwardings=yes RequestTTY=no; do
    grep -F -q -- "$opt" "$ROOT/ssh.log" || die "ssh missing $opt"
  done
  if grep -F -q 'StrictHostKeyChecking=accept-new' "$ROOT/ssh.log"; then die "stage M used accept-new"; fi
  grep -F -q 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFakeKeyForStageMStubOnly' "$ROOT/known_hosts.copy" || die "pinned key"
  [[ "$(grep -c 'id_key' "$ROOT/ssh.log")" == 2 ]] || die "eval key count"
  grep -E -q '/once\.[A-Za-z0-9]+/key ' "$ROOT/ssh.log" || die "once key"
  "$PYTHON" - "$ROOT/ssh.log" "$ROOT/init.body" "$PROBE" <<'PY' || die "init order"
import hashlib, sys
log, init_path, probe = sys.argv[1:]
lines = open(log).read().splitlines()
kinds = []
for line in lines:
    if "/usr/bin/true" in line and "id_key" in line:
        kinds.append("ready")
    elif "s11m.XXXXXX" in line and "id_key" in line:
        kinds.append("init")
    elif " upload" in line or line.endswith(" upload"):
        kinds.append("upload")
    elif " verify " in line:
        kinds.append("verify")
    elif " run C1" in line:
        kinds.append("run")
assert kinds[:5] == ["ready", "init", "upload", "verify", "run"], kinds
body = open(init_path).read()
marker = "<<'END_DISPATCHER'\n"
start = body.find(marker)
assert start > 0
rest = body[start + len(marker):]
end = rest.find("\nEND_DISPATCHER\n")
disp = rest[:end] + "\n"
digest = hashlib.sha256(disp.encode()).hexdigest()
assert digest in body
end_at = body.find("\nEND_DISPATCHER\n")
assert body.find("mkdir -m 0700") < start
assert end_at < body.find("/usr/bin/shasum -a 256", end_at)
assert body.find("/usr/bin/shasum -a 256", end_at) < body.find("restrict,command=")
text = open(probe).read().split("cat <<'END_DISPATCHER'\n", 1)[1].split("\nEND_DISPATCHER\n", 1)[0] + "\n"
assert text == disp
PY
  grep -F -q '"evidence":"DEFAULT-network/mechanics-only"' "$CASE/out/report.json" || die "evidence"
  grep -F -q '"export":"host-private-directory (not the capped HFSX volume)"' "$CASE/out/report.json" || die "export"
  grep -F -q '"verdict":"ok"' "$CASE/out/report.json" || die "verdict"
  grep -F -q '"export_state":"retained"' "$CASE/out/report.json" || die "retained"
  find "$CASE/out" -name result-host -print | grep -q . || die "result removed"
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "inventory"
}

test_stage_m_c2() {
  setup
  write_pinned
  write_fake_probe
  run_probe 25 --stage M --case C2 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 0
  common_end
  local n
  n=$(grep -E -c '^run s11-0-probe-M-[0-9]+-[0-9a-f]+ --no-graphics --net-softnet --dir s11mbx:.*/mailbox$' "$ROOT/tart.log" || true)
  [[ "$n" == 3 ]] || die "C2 argv count $n"
  grep -F -q '"verdict":"not-ack"' "$CASE/out/report.json" || die "aggregate"
  if [[ -n "$PYTHON" ]]; then
    "$PYTHON" - "$CASE/out/report.json" <<'PY' || die "C2 rows"
import json, sys
r = json.load(open(sys.argv[1]))
stage = [s for s in r["stages"] if s["stage"] == "M"][0]
assert stage["verdict"] == "not-ack"
bounds = [c["boundary"] for c in stage["cases"]]
assert bounds == ["file-sync", "dir-sync", "rename"], bounds
assert all(c["verdict"] == "not-ack" for c in stage["cases"])
assert all("--dir" in c["argv"] and any(x.startswith("s11mbx:") for x in c["argv"]) for c in stage["cases"])
PY
  fi
}

test_stage_m_quarantine() {
  setup
  write_pinned
  write_fake_probe
  touch "$ROOT/plant_unknown"
  run_probe 20 --stage M --case C1 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 5
  common_end
  need_reason quarantine
  grep -F -q '"export_state":"quarantine"' "$CASE/out/report.json" || die "state"
  find "$CASE/out" -name pwned -print | grep -q . || die "unknown removed"
  find "$CASE/out" -name host-write -print | grep -q . || die "known dir removed"
}

test_stage_m_not_sole() {
  setup
  write_pinned
  write_fake_probe
  printf '%s\n' 'other-vm running' >>"$ROOT/inventory"
  run_probe 15 --stage M --case C1 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 4
  common_end
  need_reason not-sole
  assert_has other-vm
  awk '$1=="other-vm" && $2=="running" {f=1} END{exit !f}' "$ROOT/inventory" || die "other vm changed"
  if [[ -f "$ROOT/stop.args" ]] && grep -F -x -q other-vm "$ROOT/stop.args"; then die "stopped other"; fi
  if [[ -f "$ROOT/delete.args" ]] && grep -F -x -q other-vm "$ROOT/delete.args"; then die "deleted other"; fi
  if [[ -f "$ROOT/ssh.log" ]] && grep -F -q 's11m.XXXXXX' "$ROOT/ssh.log"; then die "ssh before sole check"; fi
}

test_stage_m_report_write() {
  setup
  write_pinned
  cat >"$ROOT/bin/mv" <<EOF
#!/bin/bash
if [[ "\${2:-}" == */report.json ]]; then
  echo "mv-report-fail" >&2
  exit 1
fi
exec $SYS/mv "\$@"
EOF
  chmod 700 "$ROOT/bin/mv"
  run_probe 12 --dry-run --stage M --case C1 --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 2
  grep -F -q 'report-write-failed' "$CASE/stderr" || die "no report-write-failed"
  [[ ! -f "$CASE/out/report.json" ]] || die "report published"
}

m_stage() {
  [[ -n "$PYTHON" ]] || die "need python"
  "$PYTHON" - "$CASE/out/report.json" "$@" <<'PY'
import json, sys
path, expr = sys.argv[1], sys.argv[2]
r = json.load(open(path))
stage = [s for s in r["stages"] if s["stage"] == "M"][0]
ns = {"stage": stage, "report": r, "cases": stage["cases"], "len": len, "any": any, "all": all}
if not eval(expr, {"__builtins__": {}}, ns):
    raise SystemExit("fail " + expr + " " + json.dumps(stage)[:500])
PY
}

test_record_eof_rejected() {
  setup
  write_pinned
  write_fake_probe
  export S11_PROBE_MODE=nonewline
  run_probe 20 --stage M --case C1 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 4
  common_end
  m_stage 'cases[0]["verdict"] != "ok"'
}

test_record_dup_rejected() {
  setup
  write_pinned
  write_fake_probe
  export S11_PROBE_MODE=dup
  run_probe 20 --stage M --case C1 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 4
  common_end
  m_stage 'cases[0]["verdict"] != "ok"'
}

test_peer0_is_not_reader_ok() {
  setup
  write_pinned
  write_fake_probe
  export S11_PROBE_MODE=peer0
  run_probe 20 --stage M --case C1 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 4
  common_end
  m_stage 'cases[0]["verdict"] != "ok" and cases[0]["reason"] != "-"'
}

test_killed_after_ok_is_not_success() {
  setup
  write_pinned
  write_fake_probe
  export S11_PROBE_MODE=hold S11_BOUND_POLLS=30
  run_probe 20 --stage M --case C1 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 4
  common_end
  m_stage 'cases[0]["verdict"] == "unknown" and cases[0]["reason"] == "deadline" and stage["verdict"] != "ok"'
}

test_reap_abandon() {
  setup
  touch "$ROOT/hang_list_ignore"
  export S11_REAP_POLLS=0 S11_BOUND_POLLS=4
  run_probe 8 --stage A "${ARGS[@]}"
  need_ec 2
  common_end
  need_reason list-timeout
  no_clone_ops
}

test_c8_newline_name_quarantine_exit_5() {
  setup
  write_pinned
  write_fake_probe
  touch "$ROOT/plant_nl"
  run_probe 20 --stage M --case C8 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 5
  common_end
  need_reason quarantine
  need_reason single-principal
  m_stage 'cases[0]["verdict"] == "unknown" and cases[0]["reason"] == "single-principal" and cases[0]["export_state"] == "quarantine"'
  "$PYTHON" - "$CASE/out" <<'PY' || die "newline name"
import os, sys
found = False
for root, dirs, files in os.walk(sys.argv[1]):
    for name in files:
        if "\n" in name:
            found = True
if not found:
    raise SystemExit("newline name missing")
PY
  cmp -s "$ROOT/inventory" "$ROOT/inventory.orig" || die "inventory"
}

test_c2_row_loss_is_not_success() {
  setup
  write_pinned
  write_fake_probe
  export S11_FAIL_ROW_N=2
  run_probe 25 --stage M --case C2 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 4
  common_end
  m_stage 'stage["verdict"] != "not-ack" or len(cases) != 3 or any(c["verdict"] != "not-ack" for c in cases)'
  [[ -f "$CASE/out/report.json" ]] || die "report missing"
}

test_stage_write_fails_nonzero() {
  setup
  write_pinned
  export S11_FAIL_STAGE=1
  run_probe 12 --dry-run --stage M --case C1 --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 2
  grep -F -q 'report-write-failed' "$CASE/stderr" || die "no report-write-failed"
  [[ ! -f "$CASE/out/report.json" ]] || die "report published"
}

test_share_path_with_space() {
  setup
  write_pinned
  rm -rf "$CASE/out"
  run_probe 8 --dry-run --stage M --case C1 --pinned-host-key "$CASE/pinned" \
    --ssh-key "$CASE/id_key" --tart-home "$CASE/thome" --out "$CASE/lab results"
  need_ec 0
  [[ -f "$CASE/lab results/report.json" ]] || die "no report"
  "$PYTHON" - "$CASE/lab results/report.json" <<'PY' || die "share dropped"
import json, sys
r = json.load(open(sys.argv[1]))
stage = [s for s in r["stages"] if s["stage"] == "M"][0]
argv = stage["cases"][0]["argv"]
hits = [a for a in argv if a.startswith("s11mbx:") and "lab results" in a and "mailbox" in a]
if len(hits) != 1:
    raise SystemExit(repr(argv))
PY
}

test_stop_unproven_quarantine() {
  setup
  write_pinned
  write_fake_probe
  touch "$ROOT/fail_stop"
  run_probe 20 --stage M --case C1 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 5
  common_end
  need_reason quarantine
  [[ ! -f "$ROOT/delete.args" ]] || die "deleted unproven vm"
  m_stage 'cases[0]["verdict"] == "ok" and cases[0]["export_state"] == "quarantine" and cases[0]["cleanup"]["absent"] == "unknown"'
  find "$CASE/out" -name result-host -print | grep -q . || die "result removed"
}

test_stage_m_usage() {
  setup
  run_probe 6 --stage M "${ARGS[@]}"
  need_ec 2
  common_end
  need_reason usage
  rm -rf "$CASE/out"
  run_probe 6 --stage A --case C1 "${ARGS[@]}"
  need_ec 2
  need_reason usage
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
run_case kill-timeout test_kill_timeout
run_case kill-cancel test_kill_cancel
run_case kill-overflow test_kill_overflow
run_case bg-hold test_bg_hold
run_case no-kill-after-exit test_no_kill_after_exit
run_case dry-precheck test_dry_precheck
run_case list-untrusted test_list_untrusted
run_case boot-ready test_boot_ready
run_case cleanup-blocks-b test_cleanup_blocks_b
run_case help-fail test_help_fail
run_case output-exists test_output_exists
run_case route-fail test_route_fail
run_case dispatcher test_dispatcher_literal
run_case stage-m-c1 test_stage_m_c1
run_case stage-m-c2 test_stage_m_c2
run_case stage-m-quarantine test_stage_m_quarantine
run_case stage-m-not-sole test_stage_m_not_sole
run_case stage-m-report test_stage_m_report_write
run_case stage-m-usage test_stage_m_usage
run_case record-eof test_record_eof_rejected
run_case record-dup test_record_dup_rejected
run_case peer0 test_peer0_is_not_reader_ok
run_case killed-after-ok test_killed_after_ok_is_not_success
run_case reap-abandon test_reap_abandon
run_case c8-newline-quarantine test_c8_newline_name_quarantine_exit_5
run_case c2-row-loss test_c2_row_loss_is_not_success
run_case stage-write-fail test_stage_write_fails_nonzero
run_case share-space test_share_path_with_space
run_case stop-unproven test_stop_unproven_quarantine
echo "ALL_OK ${SECONDS}s"
[[ "$SECONDS" -lt 60 ]] || { echo "suite took ${SECONDS}s" >&2; exit 1; }

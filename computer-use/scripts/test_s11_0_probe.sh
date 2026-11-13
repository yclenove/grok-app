#!/bin/bash
# Manual stub suite for s11-0-probe.sh. Not invoked by scripts/verify.sh.
# Linux with gawk and GNU userland: the stubs and poll timings assume them, so on
# macOS the timing and stage V cases fail, and on mawk the inventory parser segfaults.
# S11_STUB_ONLY=<case> runs one case.
# Bounds each probe with a background sleep and kill. No external timer program.
set -euo pipefail
export LC_ALL=C

HERE=$(cd "$(dirname "$0")" && pwd)
PROBE=$HERE/s11-0-probe.sh
VERIFY=$HERE/verify.sh
RUNBOOK=$HERE/../docs/s11-0-probe-runbook.md
# Not under /tmp: the probe refuses out dirs with world-writable ancestors.
BASE=$(mktemp -d "${S11_STUB_ROOT:-$HOME}/s11-stub.XXXXXX")
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
  for c in bash awk sed grep mktemp head tail tr cut od readlink dirname id wc sleep \
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
    'hang() { trap "exit 143" TERM INT HUP; pp=$PPID; while kill -0 "$pp" 2>/dev/null; do sleep 0.2; done; }' \
    'ignore_term() { echo $$ >"$ROOT/target.pid"; trap '"'"'echo TERM >>"$ROOT/term.log"'"'"' TERM; trap "" PIPE; while true; do sleep 0.2; done; }' \
    'overflow_ignore() { echo $$ >"$ROOT/target.pid"; trap '"'"'echo TERM >>"$ROOT/term.log"'"'"' TERM; trap "" XFSZ PIPE; i=0; while [[ $i -lt 1500 ]]; do printf "%s\n" "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"; i=$((i+1)); done; while true; do sleep 0.2; done; }' \
    'has() { awk -v s="$1" '"'"'$1==s{f=1} END{exit !f}'"'"' "$INV"; }' \
    'inv_lock() { local i=0 p; while ! mkdir "$ROOT/inv.lock" 2>/dev/null; do p=""; [[ -f "$ROOT/inv.lock/pid" ]] && p=$(cat "$ROOT/inv.lock/pid" 2>/dev/null || true); if [[ -n "$p" ]] && ! kill -0 "$p" 2>/dev/null; then rm -rf "$ROOT/inv.lock"; continue; fi; i=$((i+1)); [[ "$i" -gt 400 ]] && return 1; sleep 0.01; done; printf "%s\n" "$$" >"$ROOT/inv.lock/pid"; }' \
    'inv_unlock() { local p; [[ -f "$ROOT/inv.lock/pid" ]] || return 0; p=$(cat "$ROOT/inv.lock/pid" 2>/dev/null || true); [[ "$p" == "$$" ]] || return 0; rm -rf "$ROOT/inv.lock"; }' \
    'wait_run() { local i=0 p n; [[ -f "$ROOT/run.pending" ]] || return 0; read -r p n <"$ROOT/run.pending" || true; while [[ -f "$ROOT/run.pending" && "$i" -lt 100 ]]; do if [[ -n "$p" ]] && ! kill -0 "$p" 2>/dev/null; then rm -f "$ROOT/run.pending"; return 0; fi; i=$((i+1)); sleep 0.01; done; }' \
    'emit_vm() { local run=false; case "$2" in running|Running|RUNNING) run=true ;; esac; printf "  {\n    \"Size\" : 35,\n    \"Disk\" : 50,\n    \"Source\" : \"local\",\n    \"State\" : \"%s\",\n    \"Name\" : \"%s\",\n    \"Running\" : %s,\n    \"Accessed\" : \"2026-10-31T17:13:29Z\"\n" "$2" "$1" "$run"; }' \
    'show_list() { wait_run; : >"$ROOT/names.rows"; while read -r n st; do [[ -n "$n" ]] || continue; printf "%s %s\n" "$n" "$st" >>"$ROOT/names.rows"; done <"$INV"; [[ -f "$ROOT/list_collide" ]] && printf "%s %s\n" "s11-0-probe-A-${PPID}-deadbeef" stopped >>"$ROOT/names.rows"; count=$(wc -l <"$ROOT/names.rows" | tr -d " "); if [[ "$count" -eq 0 ]]; then printf "[]\n"; return 0; fi; printf "[\n"; i=0; while read -r n st; do i=$((i+1)); emit_vm "$n" "$st"; if [[ "$i" -lt "$count" ]]; then printf "  },\n"; else printf "  }\n"; fi; done <"$ROOT/names.rows"; printf "]\n"; }' \
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
    '    inv_lock || exit 1' \
    '    if [[ -f "$ROOT/partial_clone" ]]; then printf "%s stopped\n" "$dst" >>"$INV"; inv_unlock; exit 1; fi' \
    '    if [[ -f "$ROOT/omit_clone" ]]; then inv_unlock; exit 0; fi' \
    '    has "$src" || { inv_unlock; exit 1; }' \
    '    has "$dst" && { inv_unlock; exit 1; }' \
    '    printf "%s stopped\n" "$dst" >>"$INV"; inv_unlock; exit 0 ;;' \
    '  run)' \
    '    printf "%s %s\n" "$$" "$1" >"$ROOT/run.pending.$$"; mv "$ROOT/run.pending.$$" "$ROOT/run.pending"' \
    '    trap '"'"'rm -f "$ROOT/run.pending"; inv_unlock'"'"' EXIT' \
    '    inv_lock || exit 1' \
    '    has "$1" || exit 1' \
    '    [[ -f "$ROOT/fail_run" ]] && exit 1' \
    '    if [[ -f "$ROOT/run_exit" ]]; then echo $$ >"$ROOT/target.pid"; inv_unlock; rm -f "$ROOT/run.pending"; sleep 0.12; exit 0; fi' \
    '    awk -v n="$1" '"'"'$1==n{$2="running"} {print}'"'"' "$INV" >"$INV.$$" && mv "$INV.$$" "$INV" || exit 1' \
    '    inv_unlock; rm -f "$ROOT/run.pending"' \
    '    pp=$PPID; trap "exit 0" TERM INT HUP' \
    '    while kill -0 "$pp" 2>/dev/null; do sleep 0.2; done ;;' \
    '  stop)' \
    '    printf "%s\n" "$1" >>"$ROOT/stop.args"' \
    '    touch "$ROOT/stop.entered"' \
    '    [[ -f "$ROOT/slow_stop" ]] && sleep 0.45' \
    '    [[ -f "$ROOT/fail_stop" ]] && exit 1' \
    '    inv_lock || exit 1' \
    '    awk -v n="$1" '"'"'$1==n{$2="stopped"} {print}'"'"' "$INV" >"$INV.$$" && mv "$INV.$$" "$INV" || { inv_unlock; exit 1; }' \
    '    inv_unlock; exit 0 ;;' \
    '  delete)' \
    '    printf "%s\n" "$1" >>"$ROOT/delete.args"' \
    '    [[ -f "$ROOT/fail_delete" ]] && exit 1' \
    '    inv_lock || exit 1' \
    '    awk -v n="$1" '"'"'$1!=n{print}'"'"' "$INV" >"$INV.$$" && mv "$INV.$$" "$INV" || { inv_unlock; exit 1; }' \
    '    inv_unlock; exit 0 ;;' \
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
    'case "$remote" in upload|verify\ *|run\ C2|run\ C8) cat >/dev/null; exit 0 ;; "run C1"|"run C7") "$ROOT/bin/c7guest" "$remote"; exit $? ;; "run C9") cat >/dev/null; mode=""; mbx=$(awk '"'"'match($0,/--dir s11mbx:/){p=substr($0,RSTART+13)} END{print p}'"'"' "$ROOT/tart.log"); [[ -n "$mbx" && -f "$mbx/fill-mode" ]] && mode=$(tr -d "\r\n" <"$mbx/fill-mode"); if [[ -f "$ROOT/c9_hold_after" && "$mode" == small ]]; then "$ROOT/bin/c9write" & cpid=$!; trap "exit 0" TERM INT HUP; wait "$cpid"; exit $?; fi; "$ROOT/bin/c9write"; exit $? ;; "run C6 file"|"run C6 dir") kind=file; [[ "$remote" == "run C6 dir" ]] && kind=dir; [[ -f "$ROOT/c6_nohold" ]] && exit 0; printf "v=1 op=c6 role=guest kind=%s held=1\n" "$kind"; pp=$PPID; trap "exit 0" TERM INT HUP; while kill -0 "$pp" 2>/dev/null; do sleep 0.2; done ;; esac' \
    '[[ -f "$ROOT/guest_fail" ]] && exit 9' \
    'if [[ -f "$ROOT/guest_hang" ]]; then pp=$PPID; trap "exit 143" TERM INT HUP; while kill -0 "$pp" 2>/dev/null; do sleep 0.2; done; exit 143; fi' \
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
  write_volume_stubs
}

write_volume_stubs() {
  local realstat
  realstat=$(PATH=/usr/bin:/bin command -v stat)
  [[ -n "$realstat" && -x "$realstat" ]] || die "no stat"
  cat >"$BASE/tpl/hdiutil" <<'EOF'
#!/bin/bash
ROOT=${S11_STUB_ROOT:?}
printf '%s\n' "$*" >>"$ROOT/hdiutil.log"
STATE=$ROOT/vol.state
get() { awk -F= -v k="$1" '$1==k{print substr($0, index($0, "=")+1); exit}' "$STATE"; }
put() {
  local k=$1 v=$2 tmp=$STATE.tmp
  if [[ -f "$STATE" ]]; then
    awk -F= -v k="$k" -v v="$v" 'BEGIN{found=0} $1==k{print k"="v; found=1; next} {print} END{if(!found) print k"="v}' "$STATE" >"$tmp"
  else
    printf '%s=%s\n' "$k" "$v" >"$tmp"
  fi
  mv "$tmp" "$STATE"
}
cmd=${1:-}; shift || true
case "$cmd" in
  create)
    [[ $# -eq 11 ]] || exit 2
    [[ "$1" == -size && "$2" == 64m && "$3" == -fs && "$4" == HFSX && "$5" == -type && "$6" == SPARSE && "$7" == -layout && "$8" == NONE && "$9" == -volname && "${10}" == GCU-MAILBOX ]] || exit 2
    img=${11}
    [[ "$img" == */mailbox.sparseimage ]] || exit 2
    [[ -e "$img" || -L "$img" ]] && exit 1
    : >"$img" || exit 1
    chmod 600 "$img" || exit 1
    put image "$img"
    put disk /dev/disk9
    put dev 11
    put ino 22
    put size 4096
    put blocks 8
    put attached 0
    put mount ""
    warn="'hdiutil create ...' is deprecated. Please use 'diskutil image create ...'"
    printf '%s\n' "$warn" >>"$ROOT/create.err"
    printf '%s\n' "$warn" >&2
    exit 0 ;;
  attach)
    printf '%s\n' 'hdiutil: attach: unknown option "-mountOptions"' >&2
    exit 1 ;;
  info)
    [[ $# -eq 1 && "$1" == -plist ]] || exit 2
    n=0
    [[ -f "$ROOT/info.count" ]] && n=$(cat "$ROOT/info.count")
    n=$((n + 1))
    printf '%s\n' "$n" >"$ROOT/info.count"
    if [[ -f "$ROOT/hang_info" && "$n" == 1 ]]; then while true; do sleep 30; done; fi
    att=$(get attached)
    mpt=$(get mount)
    dev_entry=$(get disk)
    img=$(get image)
    [[ -f "$ROOT/info_bare" ]] && dev_entry=${dev_entry#/dev/}
    [[ -f "$ROOT/info_dev" ]] && dev_entry=$(tr -d '\r\n' <"$ROOT/info_dev")
    if [[ -f "$ROOT/mismatch_on" && "$(cat "$ROOT/mismatch_on")" == "$n" ]]; then mpt=/volume-mismatch; fi
    [[ -f "$ROOT/info_retain" ]] && att=1
    if [[ -f "$ROOT/info_hole" ]]; then
      printf '%s\n' '#s11plist' \
        'images.n=3' \
        'images.0.image-path=/var/empty/unrelated.sparseimage' \
        'images.0.system-entities.n=1' \
        'images.0.system-entities.0.dev-entry=/dev/disk3' \
        'images.0.system-entities.0.mount-point=/var/empty/unrelated' \
        "images.2.image-path=$img" \
        'images.2.system-entities.n=1' \
        "images.2.system-entities.0.dev-entry=$dev_entry" \
        "images.2.system-entities.0.mount-point=$mpt"
      exit 0
    fi
    if [[ -f "$ROOT/info_dollar" ]]; then
      if [[ "$att" == 1 ]]; then
        printf '%s\n' '#s11plist' \
          'images.n=2' \
          'images.0.image-path=/var/empty/fee$bar`tick' \
          'images.0.system-entities.n=1' \
          'images.0.system-entities.0.dev-entry=/dev/disk3' \
          'images.0.system-entities.0.mount-point=/var/empty/fee' \
          "images.1.image-path=$img" \
          'images.1.system-entities.n=1' \
          "images.1.system-entities.0.dev-entry=$dev_entry" \
          "images.1.system-entities.0.mount-point=$mpt"
      else
        printf '%s\n' '#s11plist' \
          'images.n=1' \
          'images.0.image-path=/var/empty/fee$bar`tick' \
          'images.0.system-entities.n=1' \
          'images.0.system-entities.0.dev-entry=/dev/disk3' \
          'images.0.system-entities.0.mount-point=/var/empty/fee'
      fi
      exit 0
    fi
    if [[ "$att" == 1 ]]; then
      printf '%s\n' "info dev-entry=$dev_entry" >>"$ROOT/dev-shape.log"
      printf '%s\n' '#s11plist' \
        'images.n=1' \
        "images.0.image-path=$img" \
        'images.0.owner-uid=501' \
        'images.0.image-type=sparse' \
        'images.0.writeable=true' \
        'images.0.system-entities.n=1' \
        "images.0.system-entities.0.dev-entry=$dev_entry" \
        "images.0.system-entities.0.mount-point=$mpt" \
        'images.0.system-entities.0.content-hint=Apple_HFS'
    else
      printf '%s\n' '#s11plist' 'images.n=0'
    fi
    exit 0 ;;
  detach)
    force=0; arg=
    if [[ "${1:-}" == -force ]]; then
      [[ $# -eq 2 ]] || exit 2
      force=1; arg=$2
      printf '%s\n' force >>"$ROOT/force.count"
      if [[ -f "$ROOT/hang_force" ]]; then
        trap 'exit 0' TERM INT HUP
        while true; do sleep 0.2; done
      fi
      [[ -f "$ROOT/fail_force" ]] && exit 1
    else
      [[ $# -eq 1 ]] || exit 2
      arg=$1
      if [[ -f "$ROOT/fail_detach_once" && ! -f "$ROOT/fail_detach_once.hit" ]]; then
        : >"$ROOT/fail_detach_once.hit"
        printf 'hdiutil: couldn'"'"'t unmount "%s" - 资源忙\n' "$(get disk | sed 's#^/dev/##')" >&2
        exit 16
      fi
      if [[ -f "$ROOT/fail_detach" ]]; then
        # Opt-in: the next image-inode stat disagrees, so a later force must not run.
        [[ -f "$ROOT/shift_after_detach" ]] && touch "$ROOT/stat_shift"
        # Real Mini (zh_CN): rc 16 and a localized message; the probe must use the rc.
        if [[ -f "$ROOT/busy_err" ]]; then
          printf 'hdiutil: couldn'"'"'t unmount "%s" - 资源忙\n' "$(get disk | sed 's#^/dev/##')" >&2
          exit 16
        fi
        printf '%s\n' 'Resource busy' >&2
        exit 1
      fi
      [[ -f "$ROOT/slow_detach" ]] && sleep 6
      [[ -f "$ROOT/detach_pause" ]] && sleep 3
    fi
    mp=$(get mount)
    [[ "$arg" == "$mp" ]] || exit 1
    [[ "$(get attached)" == 1 ]] || exit 1
    case "$mp" in */mnt.*) ;; *) exit 2 ;; esac
    [[ -d "$mp.under" && ! -L "$mp" ]] || exit 1
    rm -rf "$mp"
    mv "$mp.under" "$mp" || exit 1
    put attached 0
    if [[ -f "$ROOT/replace_mount" ]]; then
      rm -rf "$mp"
      # Hold the freed inode so the replacement directory cannot reuse it.
      mkdir -m 0700 "$mp.burn" || exit 1
      mkdir -m 0700 "$mp" || exit 1
      rm -rf "$mp.burn"
      printf '%s\n' 1 >"$ROOT/replaced"
    fi
    dev=$(get disk)
    printf '"%s" ejected.\n' "${dev#/dev/}" >>"$ROOT/detach.out"
    printf '"%s" ejected.\n' "${dev#/dev/}"
    exit 0 ;;
  *) exit 2 ;;
esac
EOF
  cat >"$BASE/tpl/diskutil" <<'EOF'
#!/bin/bash
ROOT=${S11_STUB_ROOT:?}
printf '%s\n' "$*" >>"$ROOT/diskutil.log"
STATE=$ROOT/vol.state
get() { awk -F= -v k="$1" '$1==k{print substr($0, index($0, "=")+1); exit}' "$STATE"; }
put() {
  local k=$1 v=$2 tmp=$STATE.tmp
  if [[ -f "$STATE" ]]; then
    awk -F= -v k="$k" -v v="$v" 'BEGIN{found=0} $1==k{print k"="v; found=1; next} {print} END{if(!found) print k"="v}' "$STATE" >"$tmp"
  else
    printf '%s=%s\n' "$k" "$v" >"$tmp"
  fi
  mv "$tmp" "$STATE"
}
if [[ "${1:-}" == image && "${2:-}" == info ]]; then
  printf '%s\n' 'Operation not supported' >&2
  exit 1
fi
[[ "${1:-}" == image && "${2:-}" == attach ]] || exit 2
shift 2
[[ $# -eq 7 ]] || exit 2
[[ "$1" == --plist && "$2" == --nobrowse && "$3" == --mountPoint && "$5" == --mountOptions && "$6" == nosuid,noexec,nodev,owners ]] || exit 2
mp=$4; img=$7
[[ "$img" == "$(get image)" ]] || exit 1
[[ -d "$mp" && ! -L "$mp" ]] || exit 1
set -- "$mp"/*
[[ -e "${1:-}" ]] && exit 1
case "$mp" in */mnt.*) ;; *) exit 2 ;; esac
mv "$mp" "$mp.under" || exit 1
mkdir -m 0700 "$mp" || exit 1
# Measured shape: Spotlight's directory appears on the live mount, mode 0700, and leaves with the overlay.
mkdir -m 0700 "$mp/.fseventsd" || exit 1
put mount "$mp"
put attached 1
ent=disk9
[[ -f "$ROOT/attach_prefixed" ]] && ent=$(get disk)
printf '%s\n' "attach dev-entry=$ent" >>"$ROOT/dev-shape.log"
if [[ -f "$ROOT/attach_fail_live" ]]; then exit 1; fi
if [[ -f "$ROOT/bad_plist" ]]; then
  printf '%s\n' '#s11plist' 'system-entities.0.dev-entry=disk9'
  exit 0
fi
printf '%s\n' '#s11plist' \
  'system-entities.n=1' \
  "system-entities.0.dev-entry=$ent" \
  'system-entities.0.filesystem-name=Case-sensitive HFS+' \
  'system-entities.0.filesystem-type=hfs' \
  "system-entities.0.mount-point=$mp" \
  'system-entities.0.size=67108864' \
  'system-entities.0.volume-name=GCU-MAILBOX' \
  'system-entities.0.content-hint=Apple_HFS'
exit 0
EOF
  cat >"$BASE/tpl/plutil" <<'EOF'
#!/bin/bash
ROOT=${S11_STUB_ROOT:?}
printf '%s\n' "$*" >>"$ROOT/plutil.log"
cmd=${1:-}; shift || true
case "$cmd" in
  -lint)
    [[ $# -eq 1 && -f "$1" ]] || exit 1
    IFS= read -r line <"$1" || exit 1
    [[ "$line" == '#s11plist' ]] || exit 1
    printf '%s\n' "$1: OK"
    exit 0 ;;
  -type)
    [[ $# -eq 2 && -f "$2" ]] || exit 1
    key=$1; file=$2
    if awk -F= -v k="$key.n" '$1==k{found=1; exit} END{exit !found}' "$file"; then
      printf '%s\n' array
      exit 0
    fi
    if awk -F= -v k="$key" '$1==k{found=1; exit} END{exit !found}' "$file"; then
      printf '%s\n' string
      exit 0
    fi
    printf '%s\n' "Could not extract value, error: $key" >&2
    exit 1 ;;
  -extract)
    [[ $# -eq 5 && "$2" == raw || $# -eq 5 && "$2" == json ]] || exit 2
    [[ "$3" == -o && "$4" == - && -f "$5" ]] || exit 2
    key=$1; form=$2; file=$5
    if [[ "$key" == images && "$form" == json ]]; then
      if awk -F= '$1=="images.n"{found=1} END{exit !found}' "$file"; then
        printf '%s\n' '[{"image-path":"present"}]'
      else
        printf '%s\n' '[]'
      fi
      exit 0
    fi
    [[ "$form" == raw ]] || exit 2
    if awk -F= -v k="$key.n" '$1==k{print substr($0, index($0, "=")+1); found=1; exit} END{exit !found}' "$file"; then
      exit 0
    fi
    awk -F= -v k="$key" '$1==k{print substr($0, index($0, "=")+1); found=1; exit} END{exit !found}' "$file" || {
      printf '%s\n' "Could not extract value, error: $key" >&2
      exit 1
    }
    exit 0 ;;
  *) exit 2 ;;
esac
EOF
  cat >"$BASE/tpl/mount" <<'EOF'
#!/bin/bash
ROOT=${S11_STUB_ROOT:?}
printf '%s\n' "$*" >>"$ROOT/mount.log"
[[ $# -eq 0 ]] || exit 2
STATE=$ROOT/vol.state
get() { awk -F= -v k="$1" '$1==k{print substr($0, index($0, "=")+1); exit}' "$STATE"; }
[[ "$(get attached)" == 1 ]] || exit 0
opts='hfs, local, nodev, noexec, nosuid, nobrowse, mounted by stub'
[[ -f "$ROOT/drop_nosuid" ]] && opts='hfs, local, nodev, noexec, nobrowse, mounted by stub'
[[ -f "$ROOT/show_noowners" ]] && opts='hfs, local, nodev, noexec, nosuid, nobrowse, noowners, mounted by stub'
line="$(get disk) on $(get mount) ($opts)"
printf '%s\n' "$line" >>"$ROOT/mount.out"
printf '%s\n' "$line"
exit 0
EOF
  cat >"$BASE/tpl/mdutil" <<'EOF'
#!/bin/bash
ROOT=${S11_STUB_ROOT:?}
printf '%s\n' "$*" >>"$ROOT/mdutil.log"
[[ "$1" == -s && $# -eq 2 ]] || exit 2
[[ -f "$ROOT/mdutil_fail" ]] && exit 1
printf '%s\n' 'Indexing disabled.'
exit 0
EOF
  cat >"$BASE/tpl/tmutil" <<'EOF'
#!/bin/bash
ROOT=${S11_STUB_ROOT:?}
printf '%s\n' "$*" >>"$ROOT/tmutil.log"
if [[ "$1" == addexclusion ]]; then
  printf '%s\n' 'Operation not permitted' >&2
  exit 1
fi
if [[ "$1" == startbackup ]]; then
  exit 9
fi
if [[ "$1" == destinationinfo && $# -eq 1 ]]; then
  if [[ -f "$ROOT/tmutil_empty" ]]; then exit 0; fi
  if [[ -f "$ROOT/tmutil_sep" ]]; then
    printf '%s\n' '===================================================='
    exit 0
  fi
  if [[ -f "$ROOT/tmutil_kind" ]]; then
    printf '%s\n' 'Kind          : Local'
    exit 0
  fi
  if [[ -f "$ROOT/tmutil_trunc" ]]; then
    printf '%s\n' 'Name          :'
    exit 0
  fi
  if [[ -f "$ROOT/tmutil_mix" ]]; then
    printf '%s\n' 'tmutil: No destinations configured.' \
      'Name          : S11SecretDestName' \
      'Kind          : Local' \
      'URL           : file:///var/empty/secret' \
      'ID            : ABC'
    exit 0
  fi
  if [[ -f "$ROOT/tmutil_nodest" ]]; then
    printf '%s\n' 'tmutil: No destinations configured.'
    exit 0
  fi
  if [[ -f "$ROOT/tmutil_localonly" ]]; then
    printf '%s\n' '====================================================' \
      'Name          : S11SecretDestName' \
      'Kind          : Local' \
      'Mount Point   : /var/empty/secret' \
      'ID            : ABC'
    exit 0
  fi
  if [[ -f "$ROOT/tmutil_dest_fail" ]]; then
    printf '%s\n' 'Operation not permitted' >&2
    exit 1
  fi
  printf '%s\n' '====================================================' \
    'Name          : S11SecretDestName' \
    'Kind          : Local' \
    'URL           : file:///var/empty/secret' \
    'Mount Point   : /var/empty/secret' \
    'ID            : ABC'
  exit 0
fi
[[ "$1" == isexcluded && $# -eq 2 ]] || exit 2
if [[ -f "$ROOT/tmutil_fail" ]]; then
  printf '%s\n' 'Operation not permitted' >&2
  exit 1
fi
printf '%s\n' "[Included]  $2"
exit 0
EOF
  cat >"$BASE/tpl/statv" <<EOF
#!/bin/bash
ROOT=\${S11_STUB_ROOT:?}
REALSTAT=$realstat
printf '%s\\n' "\$*" >>"\$ROOT/statv.log"
[[ \$# -eq 3 && "\$1" == -f ]] || exit 2
fmt=\$2
path=\$3
[[ -e "\$path" || -L "\$path" ]] || exit 1
if [[ -L "\$path" ]]; then printf '%s\\n' link; exit 0; fi
case "\$fmt" in
  '%z %b %d %i')
    STATE=\$ROOT/vol.state
    get() { awk -F= -v k="\$1" '\$1==k{print substr(\$0, index(\$0, "=")+1); exit}' "\$STATE"; }
    [[ "\$path" == "\$(get image)" ]] || exit 1
    size=\$(get size); blocks=\$(get blocks)
    ids=\$("\$REALSTAT" -c '%d %i' "\$path") || exit 1
    if [[ -f "\$ROOT/stat_shift" ]]; then
      sdev=\${ids% *}
      sino=\${ids##* }
      sino=\$((sino + 1))
      ids="\$sdev \$sino"
    fi
    if [[ -f "\$ROOT/stat_big" ]]; then size=90000000; blocks=200000; fi
    if [[ -f "\$ROOT/stat_big_arm" ]]; then size=90000000; blocks=200000; rm -f "\$ROOT/stat_big_arm"; fi
    if [[ -f "\$ROOT/alloc_arm" ]]; then size=25165824; blocks=49152; rm -f "\$ROOT/alloc_arm"
    elif [[ -f "\$ROOT/alloc_base" ]]; then size=16777216; blocks=32768; fi
    printf '%s %s %s\\n' "\$size" "\$blocks" "\$ids"
    ;;
  '%d %i')
    "\$REALSTAT" -c '%d %i' "\$path" || exit 1
    ;;
  '%u %OLp')
    case "\$path" in
      */.fseventsd) printf '%s\\n' '0 0700'; exit 0 ;;
    esac
    "\$REALSTAT" -c '%u %a' "\$path" || exit 1
    ;;
  *) exit 2 ;;
esac
exit 0
EOF
  cat >"$BASE/tpl/df" <<'EOF'
#!/bin/bash
ROOT=${S11_STUB_ROOT:?}
printf '%s\n' "$*" >>"$ROOT/df.log"
[[ $# -eq 3 && "$1" == -k && "$2" == -P ]] || exit 2
STATE=$ROOT/vol.state
get() { [[ -f "$STATE" ]] || return 0; awk -F= -v k="$1" '$1==k{print substr($0, index($0, "=")+1); exit}' "$STATE"; }
path=$3
hostk=4000000
mount=$(get mount)
if [[ -f "$ROOT/df_low" && "$path" != "$mount" ]]; then hostk=100000; fi
if [[ -f "$ROOT/df_drop_arm" && "$path" != "$mount" ]]; then
  rm -f "$ROOT/df_drop_arm"
  hostk=$((hostk - 200000))
fi
if [[ -f "$ROOT/df_growth_arm" && "$path" != "$mount" ]]; then
  rm -f "$ROOT/df_growth_arm"
  hostk=$((hostk - 20480))
fi
if [[ -f "$ROOT/df_prefill_low" && "$path" != "$mount" ]]; then
  n=0
  [[ -f "$ROOT/df.hostn" ]] && n=$(tr -d '[:space:]' <"$ROOT/df.hostn")
  n=$((n + 1))
  printf '%s\n' "$n" >"$ROOT/df.hostn"
  [[ "$n" == 2 ]] && hostk=8192
fi
printf '%s\n' 'Filesystem 1024-blocks Used Available Capacity Mounted on'
if [[ -n "$mount" && "$path" == "$mount" ]]; then
  avail=1024
  [[ -f "$ROOT/df_vol_room" ]] && avail=49152
  printf '%s %s %s %s %s %s\n' /dev/disk9 65536 1000 "$avail" 50% "$path"
else
  printf '%s %s %s %s %s %s\n' hostfs 8000000 1000 "$hostk" 50% "$path"
fi
exit 0
EOF
  cat >"$BASE/tpl/c9write" <<'EOF'
#!/bin/bash
ROOT=${S11_STUB_ROOT:?}
mbx=$(awk 'match($0,/--dir s11mbx:/){p=substr($0,RSTART+13)} END{print p}' "$ROOT/tart.log")
[[ -n "$mbx" && -d "$mbx" && ! -L "$mbx" ]] || exit 9
mode=$(tr -d '\r\n' <"$mbx/fill-mode")
en=1; bytes=4096; files=4
[[ -f "$ROOT/c9_no_enospc" ]] && en=0
printf 'v=1 seq=1 bytes=%s files=%s enospc=%s\n' "$bytes" "$files" "$en" >"$mbx/progress"
if [[ -f "$ROOT/arm_growth" && "$mode" == data ]]; then
  touch "$ROOT/alloc_arm" "$ROOT/df_growth_arm"
fi
if [[ -f "$ROOT/c9_pause" ]]; then
  [[ "$mode" == data && -f "$ROOT/arm_stat" ]] && touch "$ROOT/stat_big_arm"
  [[ "$mode" == small && -f "$ROOT/df_drop" ]] && touch "$ROOT/df_drop_arm"
  sleep 1
fi
if [[ -f "$ROOT/c9_dup" && "$mode" == data ]]; then
  printf 'v=1 op=c9 role=guest mode=%s enospc=%s enospc=1 bytes=%s files=%s batches=1\n' "$mode" "$en" "$bytes" "$files" >"$mbx/result-guest"
elif [[ -f "$ROOT/c9_trail" && "$mode" == small ]]; then
  printf 'v=1 op=c9 role=guest mode=%s enospc=%s bytes=%s files=%s batches=1\nTRAIL' "$mode" "$en" "$bytes" "$files" >"$mbx/result-guest"
elif [[ -f "$ROOT/c9_dup" ]]; then
  printf 'v=1 op=c9 role=guest mode=%s enospc=%s enospc=1 bytes=%s files=%s batches=1\n' "$mode" "$en" "$bytes" "$files" >"$mbx/result-guest"
elif [[ -f "$ROOT/c9_trail" ]]; then
  printf 'v=1 op=c9 role=guest mode=%s enospc=%s bytes=%s files=%s batches=1\nTRAIL' "$mode" "$en" "$bytes" "$files" >"$mbx/result-guest"
else
  printf 'v=1 op=c9 role=guest mode=%s enospc=%s bytes=%s files=%s batches=1\n' "$mode" "$en" "$bytes" "$files" >"$mbx/result-guest"
fi
if [[ -f "$ROOT/c9_flood" && "$mode" == data ]]; then
  wrote=0
  chunk='diag................................................................................................'
  trap 'printf "%s\n" "$wrote" >"$ROOT/capture.wrote"; exit 0' PIPE
  while [[ "$wrote" -lt 200000 ]]; do
    printf '%s\n' "$chunk" || break
    wrote=$((wrote + ${#chunk} + 1))
  done
  printf '%s\n' "$wrote" >"$ROOT/capture.wrote"
  exit 0
fi
if [[ -f "$ROOT/c9_hold_after" && "$mode" == small ]]; then
  printf '%s\n' x >"$ROOT/c9_published"
  printf '%s\n' "$$" >"$ROOT/target.pid"
  pp=$PPID
  while kill -0 "$pp" 2>/dev/null; do sleep 0.2; done
  exit 0
fi
[[ -f "$ROOT/c9_ssh_fail" ]] && exit 9
exit 0
EOF
  cat >"$BASE/tpl/mdfind" <<'EOF'
#!/bin/bash
ROOT=${S11_STUB_ROOT:?}
printf '%s\n' "$*" >>"$ROOT/mdfind.log"
# Locale noise on stderr. Hit counting reads stdout only.
printf '%s\n' 'mdfind: locale environment is C' >&2
[[ "$1" == -onlyin && $# -eq 3 ]] || exit 2
if [[ -f "$ROOT/mdfind_fail" ]]; then exit 1; fi
if [[ -f "$ROOT/mdfind_hits" ]]; then
  n=$(tr -d '[:space:]' <"$ROOT/mdfind_hits")
  i=0
  while [[ "$i" -lt "$n" ]]; do
    printf '%s\n' "hit-$i"
    i=$((i + 1))
  done
fi
exit 0
EOF
  cat >"$BASE/tpl/c7guest" <<'EOF'
#!/bin/bash
ROOT=${S11_STUB_ROOT:?}
remote=${1:-}
mbx() { awk 'match($0,/--dir s11mbx:/){p=substr($0,RSTART+13)} END{print p}' "$ROOT/tart.log"; }
if [[ "$remote" == "run C1" ]]; then
  if [[ ! -f "$ROOT/c7_no_ready" ]]; then
    box=$(mbx)
    if [[ -n "$box" && -d "$box/guest-write" && ! -L "$box/guest-write" ]]; then
      printf '%s\n' 'v=1 op=ready dir=hg' >"$box/guest-write/hg-ready"
    fi
  fi
  exit 0
fi
if [[ "$remote" == "run C7" ]]; then
  STATE=$ROOT/vol.state
  get() { awk -F= -v k="$1" '$1==k{print substr($0, index($0, "=")+1); exit}' "$STATE"; }
  if [[ -f "$ROOT/plant_m0" ]]; then
    mp=$(get mount)
    printf '%s\n' landed >"$mp/s11-landed" || exit 1
  fi
  [[ -f "$ROOT/c7_reattach" ]] && touch "$ROOT/info_retain"
  if [[ -f "$ROOT/c7_tart_dead" ]]; then
    awk '$2=="running"{$2="stopped"} {print}' "$ROOT/inventory" >"$ROOT/inventory.tmp" && mv "$ROOT/inventory.tmp" "$ROOT/inventory"
  fi
  [[ -f "$ROOT/c7_ssh_fail" ]] && exit 9
  if [[ -f "$ROOT/c7_hang" ]]; then
    pp=$PPID
    trap 'exit 0' TERM INT HUP
    while kill -0 "$pp" 2>/dev/null; do sleep 0.2; done
    exit 0
  fi
  if [[ -f "$ROOT/c7_bad" ]]; then
    printf '%s\n' 'not-a-record'
    exit 0
  fi
  line="v=1 op=c7 role=guest ops=8 ok=8 enoent=0 eio=0 other=0 hang=0"
  [[ -f "$ROOT/c7_enoent" ]] && line="v=1 op=c7 role=guest ops=8 ok=7 enoent=1 eio=0 other=0 hang=0"
  # Real c7ops shape when the first operation hangs: ops counts attempts.
  [[ -f "$ROOT/c7_hang1" ]] && line="v=1 op=c7 role=guest ops=1 ok=0 enoent=0 eio=0 other=0 hang=1"
  [[ -f "$ROOT/c7_zero" ]] && line="v=1 op=c7 role=guest ops=0 ok=0 enoent=0 eio=0 other=0 hang=0"
  [[ -f "$ROOT/c7_short" ]] && line="v=1 op=c7 role=guest ops=3 ok=3 enoent=0 eio=0 other=0 hang=0"
  printf '%s\n' "$line"
  exit 0
fi
exit 2
EOF
  chmod 700 "$BASE/tpl/hdiutil" "$BASE/tpl/diskutil" "$BASE/tpl/plutil" "$BASE/tpl/mount" "$BASE/tpl/mdutil" "$BASE/tpl/tmutil" "$BASE/tpl/mdfind" "$BASE/tpl/statv" "$BASE/tpl/df" "$BASE/tpl/c9write" "$BASE/tpl/c7guest"
  cat >"$BASE/tpl/lsacl" <<'EOF'
#!/bin/bash
ROOT=${S11_STUB_ROOT:?}
[[ $# -eq 2 && "$1" == -lde ]] || exit 2
path=$2
[[ -e "$path" || -L "$path" ]] || exit 1
printf '%s\n' "$*" >>"$ROOT/ls.log"
emit() { printf '%s\n' "$1"; printf '%s\n' "$1" >>"$ROOT/ls.log"; }
emit "drwx------  2 stub staff  64 Nov 12 00:00 $path"
emit ' 0: group:everyone deny delete'
if [[ -f "$ROOT/acl_grant" ]]; then
  emit ' 1: user:aclwriter allow add_file,delete,add_subdirectory,delete_child'
fi
if [[ -f "$ROOT/acl_writesecurity" ]]; then
  emit ' 1: user:aclwriter inherited allow search,readsecurity,writesecurity'
fi
if [[ -f "$ROOT/acl_readonly" ]]; then
  emit ' 1: user:reader allow list,search,readattr,readextattr,readsecurity'
fi
if [[ -f "$ROOT/acl_on_attempt" ]]; then
  case "$path" in
    */mnt.*) ;;
    */s11v.*) emit ' 1: user:aclwriter inherited allow add_file,delete' ;;
  esac
fi
exit 0
EOF
  chmod 700 "$BASE/tpl/lsacl"
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
  rm -f "$ROOT/mid/link" "$ROOT/bin/softnet"
  ln -s ../real/softnet "$ROOT/mid/link"
  ln -s ../mid/link "$ROOT/bin/softnet"
  export S11_STUB_ROOT=$ROOT
  export PATH="$ROOT/bin:$SYS"
  export TMPDIR=$CASE/tmp
  export S11_START_SEC=0
  unset BASH_ENV S11_XTRACE S11_BOUND_SEC S11_BOUND_POLLS S11_FORCE_NAME S11_PROBE_BUDGET S11_CLEANUP_BUDGET S11_REAP_POLLS S11_FAIL_ROW_N S11_FAIL_STAGE S11_PROBE_MODE
  unset S11_HDIUTIL S11_DISKUTIL S11_PLUTIL S11_MOUNT S11_MDUTIL S11_TMUTIL S11_MDFIND S11_STAT S11_DF S11_VOLIO S11_LS S11_TEST_MODE S11_C10_WAIT  S11_R_BUDGET S11_RECEIPT_STALL S11_READY_SEC S11_C7_BUDGET
  export S11_TEST_MODE=1
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

need_reason_outr() {
  grep -F "\"$1\"" "$CASE/outr/report.json" >/dev/null || die "missing reason $1"
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
  local report_dir=${2:-$CASE/out}
  [[ -f "$report_dir/report.json" ]] || die "no report"
  [[ "$keep_golden" == yes ]] && assert_has macos-s2-eval
  assert_has macos-s2-eval.alt
  assert_has vm.dotted-name
  assert_has s11-0-probe-keep
  assert_has s11-0-probe-A-1-deadbee1
  if [[ -f "$ROOT/tart.log" ]] && grep -F -q 'list ' "$ROOT/tart.log"; then
    grep -F -q 'list --format json' "$ROOT/tart.log" || die "list format"
  fi
  assert_owned_only
  ban_file "$report_dir/report.json"
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
  grep -F -q '"h1_independent_of_ip":"unknown"' "$report_dir/report.json" || die "h1 not unknown"
  grep -F -q '"exclusive_window":"assumed"' "$report_dir/report.json" || die "exclusive window"
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
  [[ "$n" -lt 4600 ]] || die "probe has $n lines"
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
  # The post-delete list hangs. 40 polls lets the guest ssh finish first.
  export S11_BOUND_POLLS=40
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
  [[ -z "${S11_STUB_ONLY:-}" || "$name" == "$S11_STUB_ONLY" ]] || return 0
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
gate=-
[[ "$mode" == gate ]] && gate=exceeds-2s
c8_reason=single-principal
c8_n=1
if [[ "$mode" == noadmin ]]; then c8_reason=no-non-admin-principal; c8_n=0; fi
rtt='hg=n=8,min=10,p50=20,max=30;gh=n=8,min=11,p50=21,max=40'
line=""
case "$cmd" in
  writer|reader)
    line="v=1 op=c1 role=${role} verdict=ok reason=- n=8 torn=0 lost=- integrity=ok latency=${latency} vis_us=≤rtt rtt_us=${rtt} slice_us=1 w_slice_us=1 open_us=1 write_us=1 fsync_us=1 rename_us=1 dir_sync_us=1 read_us=1 peer=${peer} digest=${digest} retry=absent:0,replaced:0 gate=${gate}"
    ;;
  c2)
    line="v=1 op=c2 role=${role} boundary=${boundary} verdict=not-ack reason=- planned=3 attempted=3 renamed=2 visible=2 part=ok dir_synced=0"
    ;;
  c8)
    line="v=1 op=c8 role=${role} verdict=unknown reason=${c8_reason} n=${c8_n} ops=0 created=0 elevated=0"
    ;;
esac
if [[ "$mode" == nonewline ]]; then
  printf '%s' "$line" >"$dir/result-$role"
else
  printf '%s\n' "$line" >"$dir/result-$role"
  [[ "$mode" == dup ]] && printf '%s\n' "$line" >>"$dir/result-$role"
fi
if [[ "$cmd" == writer || "$cmd" == reader ]]; then
  printf '%s\n' 'v=1 op=ready' >"$dir/guest-write/hg-ready"
  printf '%s\n' 'v=1 op=ready' >"$dir/guest-write/hg-ready.tmp"
  printf '%s\n' 'v=1 op=ready' >"$dir/host-write/gh-ready"
  printf '%s\n' 'v=1 op=ready' >"$dir/host-write/gh-ready.tmp"
fi
if [[ "$mode" == oldname ]]; then printf 'x\n' >"$dir/guest-write/hg-ping-1"; fi
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
  expect_reject 'run C9 data'
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
  SSH_ORIGINAL_COMMAND='run C9' bash "$CASE/dispatcher.sh" >"$CASE/disp.out"
  [[ "$?" == 6 ]] || die "run C9 ec"
  grep -F -q fail-run "$CASE/disp.out" || die "run C9 fail-run"
  if grep -F -q 'eval ' "$CASE/dispatcher.sh"; then die "dispatcher uses eval"; fi
}

test_dispatcher_c6_c7_literals() {
  setup
  extract_dispatcher
  expect_reject 'run C6'
  expect_reject 'run C6 files'
  expect_reject 'run C6 file extra'
  expect_reject 'run C7 now'
  expect_reject 'run C10'
  local cmd
  for cmd in 'run C6 file' 'run C6 dir' 'run C7'; do
    SSH_ORIGINAL_COMMAND="$cmd" bash "$CASE/dispatcher.sh" >"$CASE/disp.out"
    [[ "$?" == 6 ]] || die "accept ec for [$cmd]"
    grep -F -q fail-run "$CASE/disp.out" || die "accept text for [$cmd]"
  done
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
  find "$CASE/out" -name hg-ready -print | grep -q . || die "hg-ready removed"
  find "$CASE/out" -name gh-ready -print | grep -q . || die "gh-ready removed"
  find "$CASE/out" -name hg-ready.tmp -print | grep -q . || die "hg-ready.tmp removed"
  find "$CASE/out" -name gh-ready.tmp -print | grep -q . || die "gh-ready.tmp removed"
  local mbx leaf name
  mbx=$(find "$CASE/out" -type d -name mailbox -print)
  mbx=${mbx%%$'\n'*}
  [[ -n "$mbx" && ! -L "$mbx" ]] || die "no mailbox"
  for leaf in host-write guest-write; do
    for name in hg-ping hg-pong gh-ping gh-pong hg-ready gh-ready; do
      [[ -f "$mbx/$leaf/$name" && ! -L "$mbx/$leaf/$name" ]] || die "missing slot $leaf/$name"
    done
  done
  [[ "$(cat "$mbx/guest-write/hg-ping")" == 'v=1 op=empty seq=0' ]] || die "guest ping placeholder"
  [[ "$(cat "$mbx/host-write/hg-pong")" == 'v=1 op=empty seq=0' ]] || die "host pong placeholder"
  [[ "$(cat "$mbx/host-write/hg-ready")" == 'v=1 op=empty seq=0' ]] || die "host ready placeholder"
  [[ "$(cat "$mbx/guest-write/gh-ready")" == 'v=1 op=empty seq=0' ]] || die "guest gh-ready placeholder"
  [[ "$(cat "$mbx/summary-host")" == 'v=1 op=empty seq=0' ]] || die "summary-host placeholder"
  [[ "$(cat "$mbx/summary-guest")" == 'v=1 op=empty seq=0' ]] || die "summary-guest placeholder"
  if find "$mbx" \( -name 'hg-ping-[0-9]*' -o -name 'hg-pong-[0-9]*' -o -name 'gh-ping-[0-9]*' -o -name 'gh-pong-[0-9]*' \) -print | grep -q .; then
    die "numbered slot name"
  fi
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

test_c1_old_name_quarantine() {
  setup
  write_pinned
  write_fake_probe
  export S11_PROBE_MODE=oldname
  run_probe 20 --stage M --case C1 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 5
  common_end
  need_reason quarantine
  find "$CASE/out" -name hg-ping-1 -print | grep -q . || die "numbered name removed"
  find "$CASE/out" -name hg-ping -print | grep -q . || die "fixed slot removed"
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

test_c1_rtt_fields_pass_through() {
  setup
  write_pinned
  write_fake_probe
  run_probe 20 --stage M --case C1 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 0
  common_end
  m_stage 'cases[0]["verdict"] == "ok" and "vis_us=≤rtt" in cases[0]["record"] and "rtt_us=hg=n=8,min=10,p50=20,max=30;gh=n=8,min=11,p50=21,max=40" in cases[0]["record"] and "gate=-" in cases[0]["record"] and "retry=absent:0,replaced:0" in cases[0]["record"] and "p99" not in cases[0]["record"]'
}

test_c1_gate_keeps_verdict() {
  setup
  write_pinned
  write_fake_probe
  export S11_PROBE_MODE=gate
  run_probe 20 --stage M --case C1 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 0
  common_end
  m_stage 'cases[0]["verdict"] == "ok" and cases[0]["reason"] == "-" and "gate=exceeds-2s" in cases[0]["record"] and "latency=measured" in cases[0]["record"]'
}

test_c8_no_non_admin_principal() {
  setup
  write_pinned
  write_fake_probe
  export S11_PROBE_MODE=noadmin
  run_probe 20 --stage M --case C8 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 4
  common_end
  m_stage 'cases[0]["verdict"] == "unknown" and cases[0]["reason"] == "no-non-admin-principal" and "n=0" in cases[0]["record"] and "created=0" in cases[0]["record"]'
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
run_case dispatcher-c6 test_dispatcher_c6_c7_literals
run_case stage-m-c1 test_stage_m_c1
run_case c1-old-name test_c1_old_name_quarantine
run_case stage-m-c2 test_stage_m_c2
run_case stage-m-quarantine test_stage_m_quarantine
run_case stage-m-not-sole test_stage_m_not_sole
run_case stage-m-report test_stage_m_report_write
run_case stage-m-usage test_stage_m_usage
run_case record-eof test_record_eof_rejected
run_case record-dup test_record_dup_rejected
run_case c1-rtt-fields test_c1_rtt_fields_pass_through
run_case c1-gate test_c1_gate_keeps_verdict
run_case c8-zero test_c8_no_non_admin_principal
run_case peer0 test_peer0_is_not_reader_ok
run_case killed-after-ok test_killed_after_ok_is_not_success
run_case reap-abandon test_reap_abandon
run_case c8-newline-quarantine test_c8_newline_name_quarantine_exit_5
run_case c2-row-loss test_c2_row_loss_is_not_success
run_case stage-write-fail test_stage_write_fails_nonzero
run_case share-space test_share_path_with_space
enable_volume() {
  export S11_TEST_MODE=1
  export S11_HDIUTIL=$ROOT/bin/hdiutil
  export S11_DISKUTIL=$ROOT/bin/diskutil
  export S11_PLUTIL=$ROOT/bin/plutil
  export S11_MOUNT=$ROOT/bin/mount
  export S11_MDUTIL=$ROOT/bin/mdutil
  export S11_TMUTIL=$ROOT/bin/tmutil
  export S11_MDFIND=$ROOT/bin/mdfind
  export S11_STAT=$ROOT/bin/statv
  export S11_DF=$ROOT/bin/df
  export S11_LS=$ROOT/bin/lsacl
}


mount_point() {
  sed -n 's/.*--mountPoint \(.*\) --mountOptions.*/\1/p' "$ROOT/diskutil.log" | head -n 1
}
assert_detach_count() {
  local want=$1 mp n
  mp=$(mount_point)
  [[ -n "$mp" ]] || die "no mount point"
  n=$(grep -F -x -c -- "detach $mp" "$ROOT/hdiutil.log" || true)
  [[ "$n" == "$want" ]] || die "detach count $n want $want"
}
assert_force_count() {
  local mp f
  mp=$(mount_point)
  [[ -n "$mp" ]] || die "no mount point"
  f=$(grep -F -x -c -- "detach -force $mp" "$ROOT/hdiutil.log" || true)
  [[ "$f" == "$1" ]] || die "force count $f"
}

image_path() {
  sed -n 's/^create .* \(.*\/mailbox\.sparseimage\)$/\1/p' "$ROOT/hdiutil.log"
}

assert_image_gone() {
  local img
  img=$(image_path)
  [[ -n "$img" ]] || die "no image path"
  [[ ! -e "$img" && ! -L "$img" ]] || die "image remains $img"
  [[ ! -d "$(dirname "$img")" ]] || die "attempt dir remains"
}

assert_image_kept() {
  local img
  img=$(image_path)
  [[ -n "$img" && -f "$img" && ! -L "$img" ]] || die "image not kept"
  if grep -E '^detach( |$)' "$ROOT/hdiutil.log"; then die "detached"; fi
}

assert_create_argv() {
  grep -E '^create -size 64m -fs HFSX -type SPARSE -layout NONE -volname GCU-MAILBOX .*/mailbox\.sparseimage$' "$ROOT/hdiutil.log" >/dev/null || die "create argv"
  if grep -E '^attach( |$)' "$ROOT/hdiutil.log"; then die "hdiutil attach used"; fi
  grep -E '^image attach --plist --nobrowse --mountPoint .+ --mountOptions nosuid,noexec,nodev,owners .+/mailbox\.sparseimage$' "$ROOT/diskutil.log" >/dev/null || die "attach argv"
  grep -F -q -- '-extract system-entities.0.dev-entry raw -o -' "$ROOT/plutil.log" || die "plutil extract"
  grep -F -q -- '-extract system-entities.0.filesystem-type raw -o -' "$ROOT/plutil.log" || die "plutil fstype"
  grep -F -q -- '-extract system-entities.0.size raw -o -' "$ROOT/plutil.log" || die "plutil size"
  grep -F -q -- '-extract system-entities.0.content-hint raw -o -' "$ROOT/plutil.log" || die "plutil hint"
  grep -F -q -- '-extract images.0.image-path raw -o -' "$ROOT/plutil.log" || die "plutil info"
  grep -F -q -- '-extract images.0.system-entities.0.dev-entry raw -o -' "$ROOT/plutil.log" || die "plutil info dev"
  if grep -F -q -- 'volume-kind' "$ROOT/plutil.log"; then die "old volume-kind"; fi
  if grep -F -q -- 'size-bytes' "$ROOT/plutil.log"; then die "old size-bytes"; fi
  if grep -F -q -- 'image info' "$ROOT/diskutil.log"; then die "diskutil image info used"; fi
}

test_stage_v_c4_argv_and_cleanup() {
  setup
  enable_volume
  grep -F -q 'HDIUTIL=/usr/bin/hdiutil' "$PROBE" || die "default hdiutil"
  grep -F -q 'DISKUTIL=/usr/sbin/diskutil' "$PROBE" || die "default diskutil"
  grep -F -q 'S11_TEST_MODE' "$PROBE" || die "test mode gate"
  grep -F -q -- '-size 64m -fs HFSX -type SPARSE -layout NONE -volname GCU-MAILBOX' "$PROBE" || die "create literal"
  grep -F -q -- 'image attach --plist --nobrowse --mountPoint' "$PROBE" || die "attach literal"
  grep -F -q -- '--mountOptions nosuid,noexec,nodev,owners' "$PROBE" || die "mount options"
  if grep -F -q -- 'hdiutil attach' "$PROBE"; then die "hdiutil attach remains"; fi
  if grep -F -q -- '-noautoopen' "$PROBE"; then die "old noautoopen"; fi
  if grep -F -q -- '-owners on' "$PROBE"; then die "old owners flag"; fi
  if grep -F -q -- '-mountpoint' "$PROBE"; then die "old mountpoint"; fi
  if grep -F -q -- 'image info' "$PROBE"; then die "diskutil image info in probe"; fi
  run_probe 12 --stage V --case C4 "${ARGS[@]}"
  need_ec 0
  common_end
  assert_create_argv
  assert_image_gone
  grep -F -q 'attach dev-entry=disk9' "$ROOT/dev-shape.log" || die "attach bare dev"
  grep -F -q 'info dev-entry=/dev/disk9' "$ROOT/dev-shape.log" || die "info prefixed dev"
  grep -E '^/dev/disk9 on .+ \(hfs, local, nodev, noexec, nosuid, nobrowse, mounted by stub\)$' "$ROOT/mount.out" >/dev/null || die "mount line"
  if grep -F -q noowners "$ROOT/mount.out"; then die "owners dropped"; fi
  grep -F -q '"disk9" ejected.' "$ROOT/detach.out" || die "eject text"
  grep -F -q -- "'hdiutil create ...' is deprecated. Please use 'diskutil image create ...'" "$ROOT/create.err" || die "create warning"
  grep -F -q '"export":"capped HFSX volume"' "$CASE/out/report.json" || die "export label"
  grep -F -q '"evidence":"volume-only"' "$CASE/out/report.json" || die "c4 evidence"
  grep -F -q '"fstype":"hfs"' "$CASE/out/report.json" || die "fstype"
  grep -F -q '"nosuid":"yes"' "$CASE/out/report.json" || die "nosuid"
  grep -F -q '"noexec":"yes"' "$CASE/out/report.json" || die "noexec"
  grep -F -q '"nodev":"yes"' "$CASE/out/report.json" || die "nodev"
  grep -F -q '"nobrowse":"yes"' "$CASE/out/report.json" || die "nobrowse"
  grep -F -q '"owners":"yes"' "$CASE/out/report.json" || die "owners"
  grep -F -q '"mdutil":"disabled"' "$CASE/out/report.json" || die "mdutil"
  grep -F -q '"tmutil":"included"' "$CASE/out/report.json" || die "tmutil"
  grep -F -q '"create_warn":"deprecated"' "$CASE/out/report.json" || die "create warn"
  grep -F -q '"capacity":"67108864"' "$CASE/out/report.json" || die "capacity"
  grep -F -q '"verdict":"ok"' "$CASE/out/report.json" || die "c4 ok"
  local img
  img=$(image_path)
  grep -F -q -- "isexcluded $img" "$ROOT/tmutil.log" || die "tmutil target"
  if grep -F -q addexclusion "$ROOT/tmutil.log"; then die "addexclusion called"; fi
  assert_detach_count 1
  if grep -F -q -- '-force' "$ROOT/hdiutil.log"; then die "unexpected force"; fi
  grep -F -q '"fsname":"case-sensitive-hfs"' "$CASE/out/report.json" || die "fsname"
  grep -F -q '"image_bytes":"4096"' "$CASE/out/report.json" || die "image bytes"
  grep -F -q '"image_alloc":"4096"' "$CASE/out/report.json" || die "image alloc"
  grep -F -q '"host_drop":"unknown"' "$CASE/out/report.json" || die "host drop zeroed"
  grep -F -q '"enospc":"unknown"' "$CASE/out/report.json" || die "enospc zeroed"
  grep -F -q '"host_margin":"1048576"' "$CASE/out/report.json" || die "host margin"
  grep -E -q '^phase=gone$' "$CASE/out/volume-receipt" || die "receipt phase"
  grep -E -q '^image_dev=[0-9]+$' "$CASE/out/volume-receipt" || die "receipt dev"
  grep -E -q '^mount_ino=[0-9]+$' "$CASE/out/volume-receipt" || die "receipt mount"
  if grep -E '^(clone|run) ' "$ROOT/tart.log"; then die "c4 started a vm"; fi
}

test_stage_v_mismatch_quarantine() {
  setup
  enable_volume
  printf '%s\n' 2 >"$ROOT/mismatch_on"
  run_probe 12 --stage V --case C4 "${ARGS[@]}"
  need_ec 5
  common_end
  need_reason quarantine
  assert_image_kept
  grep -F -q '"export_state":"quarantine"' "$CASE/out/report.json" || die "export state"
}

test_stage_v_detach_force_once() {
  setup
  enable_volume
  touch "$ROOT/fail_detach"
  run_probe 12 --stage V --case C4 "${ARGS[@]}"
  need_ec 0
  common_end
  assert_image_gone
  assert_detach_count 1
  assert_force_count 1
  grep -F -q '"force":"1"' "$CASE/out/report.json" || die "force flag"
}

# A normal detach past the 5s design target must finish, not be killed into
# an unmounted-but-attached image (seen once on the Mini).
test_stage_v_slow_detach_completes() {
  setup
  enable_volume
  touch "$ROOT/slow_detach"
  run_probe 20 --stage V --case C4 "${ARGS[@]}"
  need_ec 0
  common_end
  assert_image_gone
  assert_detach_count 1
  if grep -F -q -- '-force' "$ROOT/hdiutil.log"; then die "slow detach forced"; fi
  grep -E -q '"detach_sec":"([6-9]|1[0-9])"' "$CASE/out/report.json" || die "detach sec"
  grep -F -q '"verdict":"ok"' "$CASE/out/report.json" || die "slow detach verdict"
}

test_stage_v_no_force_if_vm_not_stopped() {
  setup
  enable_volume
  write_pinned
  write_fake_probe
  touch "$ROOT/fail_stop"
  run_probe 20 --stage V --case C1 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 5
  common_end
  need_reason quarantine
  assert_image_kept
  if grep -F -q -- '-force' "$ROOT/hdiutil.log"; then die "forced while vm up"; fi
}

test_stage_v_timeout_quarantine() {
  setup
  enable_volume
  touch "$ROOT/hang_info"
  # 40 polls still kills the info hang, and leaves room for list/stat under load.
  export S11_BOUND_POLLS=40
  run_probe 12 --stage V --case C4 "${ARGS[@]}"
  need_ec 5
  common_end
  need_reason quarantine
  assert_image_kept
}

test_stage_v_incomplete_quarantine() {
  setup
  enable_volume
  touch "$ROOT/bad_plist"
  run_probe 12 --stage V --case C4 "${ARGS[@]}"
  need_ec 5
  common_end
  need_reason quarantine
  assert_image_kept
}

test_stage_v_option_rejected() {
  setup
  enable_volume
  touch "$ROOT/drop_nosuid"
  run_probe 12 --stage V --case C4 "${ARGS[@]}"
  need_ec 4
  common_end
  assert_image_gone
  grep -F -q '"reason":"option-rejected"' "$CASE/out/report.json" || die "option reason"
  grep -F -q '"export_state":"removed"' "$CASE/out/report.json" || die "option cleaned"
  grep -F -q '"nosuid":"no"' "$CASE/out/report.json" || die "nosuid recorded"
  if grep -E '^clone ' "$ROOT/tart.log"; then die "option started vm"; fi
}

test_stage_v_c1_on_volume() {
  setup
  enable_volume
  write_pinned
  write_fake_probe
  run_probe 20 --stage V --case C1 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 0
  common_end
  assert_create_argv
  assert_image_gone
  grep -F -q '"export":"capped HFSX volume"' "$CASE/out/report.json" || die "c1 export"
  grep -F -q 'DEFAULT-network/mechanics-only' "$CASE/out/report.json" || die "c1 evidence"
  grep -E '^run s11-0-probe-V-[0-9]+-[0-9a-f]+ --no-graphics --net-softnet --dir s11mbx:.*/mailbox$' "$ROOT/tart.log" >/dev/null || die "V argv"
  grep -F -q ' run C1' "$ROOT/ssh.log" || die "ran C1"
}

test_stage_v_c9_ok() {
  setup
  enable_volume
  write_pinned
  write_fake_probe
  run_probe 25 --stage V --case C9 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 0
  common_end
  assert_image_gone
  local n
  n=$(grep -c ' run C9$' "$ROOT/ssh.log" || true)
  [[ "$n" == 2 ]] || die "c9 runs $n"
  grep -F -q '"boundary":"data"' "$CASE/out/report.json" || die "data mode"
  grep -F -q '"boundary":"small"' "$CASE/out/report.json" || die "small mode"
  grep -F -q '"enospc":"1"' "$CASE/out/report.json" || die "enospc"
  grep -F -q '"verdict":"ok"' "$CASE/out/report.json" || die "c9 ok"
  grep -F -q '"host_margin":"1048576"' "$CASE/out/report.json" || die "c9 margin"
  "$PYTHON" - "$CASE/out/report.json" <<'PY' || die "cap"
import json,sys
rep=json.load(open(sys.argv[1]))
cases=rep["stages"][0]["cases"]
assert len(cases)==2
for c in cases:
    vol=c["volume"]
    assert int(vol["image_bytes"])<=83886080
    assert int(vol["image_alloc"])<=83886080
    assert c["verdict"]=="ok"
PY
}

test_stage_v_usage() {
  setup
  run_probe 6 --stage V "${ARGS[@]}"
  need_ec 2
  common_end
  need_reason usage
  [[ ! -f "$ROOT/hdiutil.log" ]] || die "usage called hdiutil"
  rm -rf "$CASE/out"
  run_probe 6 --stage V --case C8 "${ARGS[@]}"
  need_ec 2
  need_reason usage
  rm -rf "$CASE/out"
  run_probe 6 --stage M --case C4 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 2
  need_reason usage
  rm -rf "$CASE/out"
  run_probe 8 --dry-run --stage V --case C4 "${ARGS[@]}"
  need_ec 0
  common_end
  grep -F -q '/usr/bin/hdiutil create -size 64m -fs HFSX -type SPARSE -layout NONE -volname GCU-MAILBOX' "$CASE/stdout" || die "dry create"
  grep -F -q '/usr/sbin/diskutil image attach --plist --nobrowse --mountPoint' "$CASE/stdout" || die "dry attach"
  grep -F -q -- '--mountOptions nosuid,noexec,nodev,owners' "$CASE/stdout" || die "dry options"
  if grep -F -q 'hdiutil attach' "$CASE/stdout"; then die "dry hdiutil attach"; fi
  [[ ! -f "$ROOT/hdiutil.log" ]] || die "dry called hdiutil"
  [[ ! -f "$ROOT/diskutil.log" ]] || die "dry called diskutil"
}

test_stage_v_dev_prefix_normalized() {
  setup
  enable_volume
  touch "$ROOT/info_bare" "$ROOT/attach_prefixed"
  run_probe 12 --stage V --case C4 "${ARGS[@]}"
  need_ec 0
  common_end
  assert_image_gone
  grep -F -q 'attach dev-entry=/dev/disk9' "$ROOT/dev-shape.log" || die "prefixed attach"
  grep -F -q 'info dev-entry=disk9' "$ROOT/dev-shape.log" || die "bare info"
  grep -F -q '"owners":"yes"' "$CASE/out/report.json" || die "normalized owners"
  assert_detach_count 1
  rm -rf "$CASE/out" "$ROOT/hdiutil.log" "$ROOT/diskutil.log" "$ROOT/dev-shape.log" "$ROOT/vol.state" "$ROOT/info.count" "$ROOT/detach.out" "$ROOT/mount.out" "$ROOT/create.err" "$ROOT/plutil.log" "$ROOT/tmutil.log"
  rm -f "$ROOT/info_bare" "$ROOT/attach_prefixed"
  printf '%s\n' '/dev/disk8' >"$ROOT/info_dev"
  run_probe 12 --stage V --case C4 "${ARGS[@]}"
  need_ec 5
  common_end
  need_reason quarantine
  assert_image_kept
  grep -F -q 'attach dev-entry=disk9' "$ROOT/dev-shape.log" || die "mismatch attach shape"
  grep -F -q 'info dev-entry=/dev/disk8' "$ROOT/dev-shape.log" || die "mismatch info shape"
}

test_stage_v_noowners_unknown() {
  setup
  enable_volume
  touch "$ROOT/show_noowners"
  run_probe 12 --stage V --case C4 "${ARGS[@]}"
  need_ec 0
  common_end
  assert_image_gone
  grep -F -q 'noowners' "$ROOT/mount.out" || die "noowners missing"
  grep -F -q '"owners":"unknown"' "$CASE/out/report.json" || die "owners unknown"
  grep -F -q '"nosuid":"yes"' "$CASE/out/report.json" || die "nosuid kept"
  grep -F -q '"export_state":"removed"' "$CASE/out/report.json" || die "noowners cleaned"
  grep -F -q '"verdict":"ok"' "$CASE/out/report.json" || die "noowners verdict"
  if grep -F -q '"export_state":"quarantine"' "$CASE/out/report.json"; then die "noowners quarantined"; fi
  assert_detach_count 1
}

test_stage_v_plist_hole_quarantine() {
  setup
  enable_volume
  touch "$ROOT/attach_fail_live" "$ROOT/info_hole"
  run_probe 12 --stage V --case C4 "${ARGS[@]}"
  need_ec 5
  common_end
  need_reason quarantine
  assert_image_kept
  if grep -E '^detach( |$)' "$ROOT/hdiutil.log"; then die "hole detached"; fi
  grep -F -q -- '-extract images.1.image-path raw -o -' "$ROOT/plutil.log" || die "hole did not read middle"
  grep -E -q '^phase=quarantine$' "$CASE/out/volume-receipt" || die "hole receipt"
  grep -F -q -- "image=$(image_path)" "$CASE/out/volume-receipt" || die "hole receipt image"
}

test_stage_v_plist_dollar_path() {
  setup
  enable_volume
  touch "$ROOT/info_dollar"
  run_probe 12 --stage V --case C4 "${ARGS[@]}"
  need_ec 0
  common_end
  assert_image_gone
  assert_detach_count 1
  grep -F -q '"verdict":"ok"' "$CASE/out/report.json" || die "dollar not ok"
}

test_stage_v_replaced_dir_quarantine() {
  setup
  enable_volume
  touch "$ROOT/replace_mount"
  run_probe 12 --stage V --case C4 "${ARGS[@]}"
  need_ec 5
  common_end
  need_reason quarantine
  [[ -f "$ROOT/replaced" ]] || die "mount was not replaced"
  local mp
  mp=$(mount_point)
  [[ -n "$mp" && -d "$mp" && ! -L "$mp" ]] || die "replaced mount removed"
  if grep -F -q -- '-force' "$ROOT/hdiutil.log"; then die "replace forced"; fi
}

test_stage_v_volio_hang_quarantine() {
  setup
  enable_volume
  printf '%s\n' '#!/bin/bash' 'while true; do sleep 30; done' >"$ROOT/bin/volio-hang"
  chmod 700 "$ROOT/bin/volio-hang"
  export S11_VOLIO=$ROOT/bin/volio-hang
  # 40 polls still kills the volio hang, and leaves room for list/stat under load.
  export S11_BOUND_POLLS=40
  run_probe 12 --stage V --case C4 "${ARGS[@]}"
  need_ec 5
  common_end
  need_reason quarantine
  assert_image_kept
}

test_stage_v_host_space_unknown() {
  setup
  enable_volume
  touch "$ROOT/df_low"
  run_probe 12 --stage V --case C4 "${ARGS[@]}"
  need_ec 4
  common_end
  need_reason host-space
  grep -F -q '"verdict":"unknown"' "$CASE/out/report.json" || die "host space verdict"
  [[ ! -f "$ROOT/hdiutil.log" ]] || die "host space created an image"
}

test_stage_v_over_cap_before_attach() {
  setup
  enable_volume
  touch "$ROOT/stat_big"
  run_probe 12 --stage V --case C4 "${ARGS[@]}"
  need_ec 4
  common_end
  grep -F -q '"verdict":"failed"' "$CASE/out/report.json" || die "over cap verdict"
  grep -F -q '"reason":"over-cap"' "$CASE/out/report.json" || die "over cap reason"
  [[ ! -f "$ROOT/diskutil.log" ]] || die "over cap attached"
}

test_stage_v_still_attached_quarantine() {
  setup
  enable_volume
  touch "$ROOT/info_retain"
  run_probe 12 --stage V --case C4 "${ARGS[@]}"
  need_ec 5
  common_end
  need_reason quarantine
  local img
  img=$(image_path)
  [[ -n "$img" && -f "$img" && ! -L "$img" ]] || die "image unlinked while still attached"
  assert_detach_count 1
}

test_stage_v_c9_peak_and_margin() {
  setup
  enable_volume
  write_pinned
  write_fake_probe
  touch "$ROOT/c9_pause" "$ROOT/arm_stat" "$ROOT/df_drop"
  run_probe 25 --stage V --case C9 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 4
  common_end
  "$PYTHON" - "$CASE/out/report.json" <<'PY' || die "peak judge"
import json,sys
rep=json.load(open(sys.argv[1]))
cases=rep["stages"][0]["cases"]
assert len(cases)==2, len(cases)
data, small = cases
assert data["boundary"]=="data" and small["boundary"]=="small"
vol=data["volume"]
assert data["verdict"]=="failed" and data["reason"]=="over-cap", (data["verdict"], data["reason"])
assert vol["image_bytes"]=="4096", vol["image_bytes"]
assert int(vol["image_bytes_peak"])==90000000, vol["image_bytes_peak"]
assert int(vol["image_alloc_peak"])>83886080, vol["image_alloc_peak"]
assert vol["host_margin"]=="1048576", vol["host_margin"]
assert int(vol["samples"])>=4, vol["samples"]
vol=small["volume"]
assert small["verdict"]=="failed" and small["reason"]=="host-grew", (small["verdict"], small["reason"])
assert int(vol["host_drop"])==200000*1024, vol["host_drop"]
assert int(vol["image_alloc_peak"])<=83886080, vol["image_alloc_peak"]
assert vol["host_margin"]=="1048576"
assert int(vol["samples"])>=4, vol["samples"]
PY
}

test_stage_v_c9_bad_record() {
  setup
  enable_volume
  write_pinned
  write_fake_probe
  touch "$ROOT/c9_dup" "$ROOT/c9_trail"
  run_probe 25 --stage V --case C9 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 4
  common_end
  "$PYTHON" - "$CASE/out/report.json" <<'PY' || die "bad record"
import json,sys
rep=json.load(open(sys.argv[1]))
cases=rep["stages"][0]["cases"]
assert [c["boundary"] for c in cases]==["data","small"], [c["boundary"] for c in cases]
for c in cases:
    assert c["verdict"]!="ok", c["boundary"]
    assert c["reason"]=="probe-failed", (c["boundary"], c["reason"])
PY
}

test_stage_v_c9_guest_fail() {
  setup
  enable_volume
  write_pinned
  write_fake_probe
  touch "$ROOT/c9_ssh_fail"
  run_probe 25 --stage V --case C9 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 4
  common_end
  if grep -F -q '"verdict":"ok"' "$CASE/out/report.json"; then die "guest fail ok"; fi
  grep -F -q '"reason":"probe-failed"' "$CASE/out/report.json" || die "guest fail reason"
  grep -F -q 'op=c9' "$CASE/out/report.json" || die "guest record dropped"
}

test_stage_v_tool_override_refused() {
  setup
  unset S11_TEST_MODE
  export S11_HDIUTIL=$ROOT/bin/hdiutil
  run_probe 8 --stage V --case C4 "${ARGS[@]}"
  need_ec 2
  common_end
  need_reason tool-override
  [[ ! -f "$ROOT/hdiutil.log" ]] || die "override invoked hdiutil"
  rm -rf "$CASE/out"
  unset S11_HDIUTIL
  export S11_LS=$ROOT/bin/lsacl
  run_probe 8 --stage V --case C4 "${ARGS[@]}"
  need_ec 2
  common_end
  need_reason tool-override
  [[ ! -f "$ROOT/ls.log" ]] || die "override invoked ls"
  rm -rf "$CASE/out"
  unset S11_LS
  export S11_MDFIND=$ROOT/bin/mdfind
  run_probe 8 --stage V --case C4 "${ARGS[@]}"
  need_ec 2
  common_end
  need_reason tool-override
  [[ ! -f "$ROOT/mdfind.log" ]] || die "override invoked mdfind"
}

test_stage_v_c1_slot_stat_stall() {
  setup
  enable_volume
  write_pinned
  write_fake_probe
  cat >"$ROOT/bin/volio-slot" <<EOF
#!/bin/bash
if [[ "\${1:-}" == slot ]]; then
  printf '%s\n' "\$\$" >"$ROOT/target.pid"
  while true; do sleep 30; done
fi
exec bash "$PROBE" __volio "\$@"
EOF
  chmod 700 "$ROOT/bin/volio-slot"
  export S11_VOLIO=$ROOT/bin/volio-slot
  # 40 polls still kills the slot hang, and leaves the C1 boot commands time to finish.
  export S11_BOUND_POLLS=40
  run_probe 20 --stage V --case C1 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 5
  common_end
  need_reason quarantine
  assert_image_kept
}

test_stage_v_acl_grant_rejected() {
  setup
  enable_volume
  touch "$ROOT/acl_grant"
  run_probe 12 --stage V --case C4 "${ARGS[@]}"
  need_ec 4
  common_end
  need_reason out-unsafe
  [[ ! -f "$ROOT/hdiutil.log" ]] || die "acl grant created an image"
  grep -F -q '0: group:everyone deny delete' "$ROOT/ls.log" || die "deny entry not observed"
  rm -rf "$CASE/out"
  rm -f "$ROOT/ls.log" "$ROOT/acl_grant" "$ROOT/hdiutil.log"
  touch "$ROOT/acl_on_attempt"
  run_probe 12 --stage V --case C4 "${ARGS[@]}"
  need_ec 5
  common_end
  need_reason quarantine
  grep -F -q '"export_state":"quarantine"' "$CASE/out/report.json" || die "attempt acl not quarantined"
  [[ ! -f "$ROOT/hdiutil.log" ]] || die "attempt acl created an image"
  grep -F -q '0: group:everyone deny delete' "$ROOT/ls.log" || die "deny entry dropped"
  grep -F -q '1: user:aclwriter inherited allow add_file,delete' "$ROOT/ls.log" || die "attempt grant not observed"
  find "$CASE/out" -maxdepth 1 -type d -name 's11v.*' | grep -q . || die "attempt dir missing"
  # Authority to change permissions is refused even without direct write rights.
  rm -rf "$CASE/out"
  rm -f "$ROOT/ls.log" "$ROOT/acl_on_attempt" "$ROOT/hdiutil.log"
  touch "$ROOT/acl_writesecurity"
  run_probe 12 --stage V --case C4 "${ARGS[@]}"
  need_ec 4
  common_end
  need_reason out-unsafe
  [[ ! -f "$ROOT/hdiutil.log" ]] || die "writesecurity grant created an image"
  rm -f "$ROOT/acl_writesecurity"
}

test_stage_v_c9_capture_bound() {
  setup
  enable_volume
  write_pinned
  write_fake_probe
  touch "$ROOT/c9_flood" "$ROOT/c9_hold_after"
  start_probe 30 --stage V --case C9 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  local end=$((SECONDS + 25))
  while [[ "$SECONDS" -lt "$end" ]]; do
    [[ -f "$ROOT/capture.wrote" && -f "$ROOT/c9_published" ]] && break
    sleep 0.05
  done
  [[ -f "$ROOT/capture.wrote" && -f "$ROOT/c9_published" ]] || die "capture not ready"
  local wrote
  wrote=$(tr -d '[:space:]' <"$ROOT/capture.wrote")
  # Direct file capture: a fast burst can land before the size poll, so the contract
  # is "overflow is detected and the case is not ok", checked in the report below.
  kill -TERM "$PROBE_PID"
  finish_probe
  need_ec 4
  common_end
  "$PYTHON" - "$CASE/out/report.json" "$wrote" <<'PY' || die "capture bound"
import json,sys
rep=json.load(open(sys.argv[1]))
wrote=int(sys.argv[2])
cases=rep["stages"][0]["cases"]
assert [c["boundary"] for c in cases]==["data","small"], [c["boundary"] for c in cases]
data, small = cases
assert "op=c9" in data["record"] and "mode=data" in data["record"], data["record"]
assert data["verdict"]!="ok" and data["reason"]=="probe-failed", (data["verdict"], data["reason"])
assert small["verdict"]!="ok" and small["reason"]=="probe-failed", (small["verdict"], small["reason"])
assert small["export_state"]!="quarantine", small["export_state"]
PY
  kill_target
}

test_stage_v_c9_prefill_and_origin() {
  setup
  enable_volume
  write_pinned
  write_fake_probe
  touch "$ROOT/df_prefill_low" "$ROOT/df_vol_room"
  run_probe 25 --stage V --case C9 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 4
  common_end
  local n
  n=$(grep -c ' run C9$' "$ROOT/ssh.log" || true)
  [[ "$n" == 1 ]] || die "c9 runs $n"
  "$PYTHON" - "$CASE/out/report.json" <<'PY' || die "prefill origin"
import json,sys
rep=json.load(open(sys.argv[1]))
cases=rep["stages"][0]["cases"]
assert [c["boundary"] for c in cases]==["data","small"], [c["boundary"] for c in cases]
data, small = cases
assert data["verdict"]=="unknown" and data["reason"]=="host-space", (data["verdict"], data["reason"])
assert data["volume"]["host_pre"]=="4096000000", data["volume"]["host_pre"]
assert data["volume"]["host_fill"]=="8388608", data["volume"]["host_fill"]
assert small["verdict"]=="unknown" and small["reason"]=="enospc-origin", (small["verdict"], small["reason"])
assert small["volume"]["vol_free"]=="50331648", small["volume"]["vol_free"]
assert int(small["volume"]["host_pre"])>=268435456, small["volume"]["host_pre"]
assert int(small["volume"]["host_fill"])>=268435456, small["volume"]["host_fill"]
PY
}

test_stage_v_c9_alloc_growth() {
  setup
  enable_volume
  write_pinned
  write_fake_probe
  touch "$ROOT/alloc_base" "$ROOT/arm_growth"
  run_probe 25 --stage V --case C9 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 4
  common_end
  "$PYTHON" - "$CASE/out/report.json" <<'PY' || die "alloc growth"
import json,sys
rep=json.load(open(sys.argv[1]))
cases=rep["stages"][0]["cases"]
assert [c["boundary"] for c in cases]==["data","small"], [c["boundary"] for c in cases]
data, small = cases
vol=data["volume"]
assert data["verdict"]=="failed" and data["reason"]=="host-grew", (data["verdict"], data["reason"])
assert vol["image_alloc_peak"]=="25165824", vol["image_alloc_peak"]
assert vol["host_drop"]=="20971520", vol["host_drop"]
assert int(vol["image_bytes_peak"])<=83886080
assert small["verdict"]=="ok", (small["verdict"], small["reason"])
PY
}

run_case stop-unproven test_stop_unproven_quarantine
run_case stage-v-c4 test_stage_v_c4_argv_and_cleanup
run_case stage-v-prefix test_stage_v_dev_prefix_normalized
run_case stage-v-noowners test_stage_v_noowners_unknown
run_case stage-v-mismatch test_stage_v_mismatch_quarantine
run_case stage-v-force test_stage_v_detach_force_once
run_case stage-v-slow-detach test_stage_v_slow_detach_completes
run_case stage-v-no-force test_stage_v_no_force_if_vm_not_stopped
run_case stage-v-timeout test_stage_v_timeout_quarantine
run_case stage-v-incomplete test_stage_v_incomplete_quarantine
run_case stage-v-option test_stage_v_option_rejected
run_case stage-v-c1 test_stage_v_c1_on_volume
run_case stage-v-c9 test_stage_v_c9_ok
run_case stage-v-usage test_stage_v_usage
run_case stage-v-hole test_stage_v_plist_hole_quarantine
run_case stage-v-dollar test_stage_v_plist_dollar_path
run_case stage-v-replaced test_stage_v_replaced_dir_quarantine
run_case stage-v-volio-hang test_stage_v_volio_hang_quarantine
run_case stage-v-host-space test_stage_v_host_space_unknown
run_case stage-v-over-cap test_stage_v_over_cap_before_attach
run_case stage-v-still test_stage_v_still_attached_quarantine
run_case stage-v-c9-peak test_stage_v_c9_peak_and_margin
run_case stage-v-c9-record test_stage_v_c9_bad_record
run_case stage-v-c9-guest test_stage_v_c9_guest_fail
run_case stage-v-override test_stage_v_tool_override_refused
run_case stage-v-c1-slot-stall test_stage_v_c1_slot_stat_stall
run_case stage-v-acl test_stage_v_acl_grant_rejected
run_case stage-v-c9-capture test_stage_v_c9_capture_bound
run_case stage-v-c9-origin test_stage_v_c9_prefill_and_origin
run_case stage-v-c9-growth test_stage_v_c9_alloc_growth

detach_lines() {
  local n=0
  if [[ -f "$ROOT/hdiutil.log" ]]; then
    n=$(grep -E -c '^detach( |$)' "$ROOT/hdiutil.log" || true)
  fi
  printf '%s\n' "$n"
}
force_lines() {
  local n=0
  if [[ -f "$ROOT/hdiutil.log" ]]; then
    n=$(grep -E -c '^detach -force ' "$ROOT/hdiutil.log" || true)
  fi
  printf '%s\n' "$n"
}
receipt_field() {
  awk -F= -v k="$1" '$1==k { print substr($0, index($0, "=")+1); exit }' "$RCPT"
}
c5_crash() {
  local fault=$1
  rm -rf "$CASE/out" "$CASE/outr"
  rm -f "$ROOT/hdiutil.log" "$ROOT/diskutil.log" "$ROOT/vol.state" "$ROOT/info.count" "$ROOT/detach.out" "$ROOT/plutil.log"
  run_probe 15 --stage V --case C5 --fault "$fault" "${ARGS[@]}"
  need_ec 137
  [[ ! -f "$CASE/out/report.json" ]] || die "killed C5 wrote a report"
  RCPT=$CASE/out/volume-receipt
  [[ -f "$RCPT" ]] || die "C5 left no receipt"
  find "$CASE/tmp" -mindepth 1 -maxdepth 1 -name 's11-0.*' -exec rm -rf {} +
  ATTEMPT=$(receipt_field attempt)
  IMAGE=$(receipt_field image)
  MOUNT=$(receipt_field mount)
  [[ -n "$ATTEMPT" && -n "$IMAGE" && -n "$MOUNT" ]] || die "receipt fields"
  DETACH_BEFORE=$(detach_lines)
  FORCE_BEFORE=$(force_lines)
}
c5_recover() {
  run_probe 15 --stage R --receipt "$RCPT" "${ARGS[@]}" --out "$CASE/outr"
}
assert_stage_r() {
  "$PYTHON" - "$CASE/outr/report.json" "$1" "$2" "$3" "$4" "$5" <<'PY' || die "stage R shape"
import json, sys
path, verdict, reason, image, mount, attempt = sys.argv[1:]
rep = json.load(open(path))
assert rep["schema"] == "s11-0-probe/v2", rep["schema"]
stages = rep["stages"]
assert len(stages) == 1, stages
st = stages[0]
assert st["stage"] == "R", st
assert st["verdict"] == verdict, st
assert st["reason"] == reason, st
assert (st["image"], st["mount"], st["attempt"]) == (image, mount, attempt), st
if verdict == "quarantine":
    assert "quarantine" in rep["reasons"], rep["reasons"]
    assert reason in rep["reasons"], rep["reasons"]
PY
}
c5_expect_clean() {
  local want_detach=$1 after force_after left
  need_ec 0
  common_end yes "$CASE/outr"
  grep -F -x -q 'recovery=noted' "$RCPT" || die "recovery not noted"
  [[ -d "$ATTEMPT" && ! -L "$ATTEMPT" ]] || die "attempt not kept"
  [[ ! -e "$IMAGE" && ! -L "$IMAGE" ]] || die "image remains"
  [[ ! -e "$MOUNT" && ! -L "$MOUNT" ]] || die "mount remains"
  left=$(find "$ATTEMPT" -mindepth 1 -print | wc -l | tr -d ' ')
  [[ "$left" == 0 ]] || die "attempt not empty ($left)"
  after=$(detach_lines)
  force_after=$(force_lines)
  [[ "$after" == "$want_detach" ]] || die "detach $DETACH_BEFORE -> $after want $want_detach"
  [[ "$force_after" == "$FORCE_BEFORE" ]] || die "recovery forced"
  assert_stage_r ok recovered absent absent kept
}

test_stage_v_c10_observed() {
  setup
  enable_volume
  export S11_C10_WAIT=0
  run_probe 20 --stage V --case C10 "${ARGS[@]}"
  need_ec 4
  common_end
  need_reason observed
  assert_image_gone
  no_clone_ops
  local f tok
  for f in "$CASE/out/report.json" "$CASE/out/volume-receipt" "$CASE/stdout" "$CASE/stderr"; do
    if grep -F -q S11SecretDestName "$f"; then die "secret in $f"; fi
  done
  [[ -f "$ROOT/mdfind.log" ]] || die "mdfind not called"
  grep -F -q -- '-onlyin' "$ROOT/mdfind.log" || die "mdfind argv"
  tok=$(awk 'NR==1 { print $NF; exit }' "$ROOT/mdfind.log")
  [[ ${#tok} -eq 32 ]] || die "token len ${#tok}"
  if grep -F -q -- "$tok" "$CASE/out/report.json" "$CASE/stdout" "$CASE/stderr"; then die "token leaked"; fi
  if grep -E -q 'addexclusion|startbackup' "$ROOT/tmutil.log"; then die "tmutil mutated"; fi
  grep -F -q destinationinfo "$ROOT/tmutil.log" || die "no destinationinfo"
  "$PYTHON" - "$CASE/out/report.json" <<'PY' || die "c10 fields"
import json, sys
rep = json.load(open(sys.argv[1]))
st = rep["stages"][0]
assert st["evidence"] == "volume-only", st["evidence"]
cases = st["cases"]
assert len(cases) == 1, cases
c = cases[0]
assert c["verdict"] == "unknown" and c["reason"] == "observed", (c["verdict"], c["reason"])
fields = dict(tok.split("=", 1) for tok in c["record"].split())
assert fields["op"] == "c10"
assert fields["mdfind_hits"].isdigit(), fields["mdfind_hits"]
assert fields["mdutil"] == "disabled", fields["mdutil"]
assert fields["fsevents"] == "present", fields["fsevents"]
assert fields["fsevents_uid"] == "0", fields["fsevents_uid"]
assert fields["fsevents_mode"] == "0700", fields["fsevents_mode"]
assert fields["dest_count"] == "1", fields["dest_count"]
assert fields["excluded_i"] == "included", fields["excluded_i"]
assert fields["excluded_m"] == "included", fields["excluded_m"]
assert "S11SecretDestName" not in c["record"]
PY
}

# Real Mini output when no backup destination exists.
test_stage_v_c10_nodest() {
  local flag want
  for flag in tmutil_nodest tmutil_localonly; do
    rm -rf "$CASE/out"
    setup
    enable_volume
    export S11_C10_WAIT=0
    rm -f "$ROOT/tmutil_nodest" "$ROOT/tmutil_localonly"
    touch "$ROOT/$flag"
    want=0; [[ "$flag" == tmutil_localonly ]] && want=1
    run_probe 12 --stage V --case C10 "${ARGS[@]}"
    need_ec 4
    common_end
    assert_image_gone
    if grep -F -q S11SecretDestName "$CASE/out/report.json"; then die "dest name leaked"; fi
    "$PYTHON" - "$CASE/out/report.json" "$want" <<'PY' || die "c10 $flag"
import json, sys
c = json.load(open(sys.argv[1]))["stages"][0]["cases"][0]
fields = dict(tok.split("=", 1) for tok in c["record"].split())
assert fields["dest_count"] == sys.argv[2], fields["dest_count"]
PY
  done
}

# Guest holds an FD until the VM stops; the host then tears down the ordinary way.
test_stage_v_c6_guest_hold() {
  setup
  enable_volume
  write_pinned
  write_fake_probe
  run_probe 50 --stage V --case C6 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 0
  common_end
  local n=0 img nd
  sed -n 's/^create .* \(.*\/mailbox\.sparseimage\)$/\1/p' "$ROOT/hdiutil.log" >"$CASE/images.list"
  while IFS= read -r img; do
    [[ -n "$img" ]] || continue
    [[ ! -e "$img" && ! -L "$img" ]] || die "image remains $img"
    n=$((n + 1))
  done <"$CASE/images.list"
  [[ "$n" == 2 ]] || die "c6 image count $n"
  nd=$(grep -E -c '^detach [^ -]' "$ROOT/hdiutil.log" || true)
  [[ "$nd" == 2 ]] || die "c6 normal detach $nd"
  if grep -F -q -- '-force' "$ROOT/hdiutil.log"; then die "c6 forced"; fi
  grep -F -q 'run C6 file' "$ROOT/ssh.log" || die "ssh file"
  grep -F -q 'run C6 dir' "$ROOT/ssh.log" || die "ssh dir"
  "$PYTHON" - "$CASE/out/report.json" <<'PY' || die "c6 records"
import json, sys
st = json.load(open(sys.argv[1]))["stages"][0]
assert st["verdict"] == "ok", st["verdict"]
cases = st["cases"]
assert [c["boundary"] for c in cases] == ["file", "dir"], cases
for c in cases:
    assert c["verdict"] == "ok" and c["reason"] == "measured", (c["boundary"], c["verdict"], c["reason"])
    f = dict(tok.split("=", 1) for tok in c["record"].split())
    assert f["guest_held"] == "1" and f["detach_rc"] == "0" and f["detach_class"] == "none" and f["force"] == "0", c["record"]
PY
}

test_stage_v_c6_not_held() {
  setup
  enable_volume
  write_pinned
  write_fake_probe
  touch "$ROOT/c6_nohold"
  run_probe 50 --stage V --case C6 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 4
  common_end
  assert_image_gone
  "$PYTHON" - "$CASE/out/report.json" <<'PY' || die "c6 not held"
import json, sys
c = json.load(open(sys.argv[1]))["stages"][0]["cases"][0]
assert c["verdict"] == "unknown" and c["reason"] == "guest-not-held", (c["verdict"], c["reason"])
PY
}

test_stage_v_c6_no_force_while_vm_up() {
  setup
  enable_volume
  write_pinned
  write_fake_probe
  touch "$ROOT/fail_stop"
  run_probe 30 --stage V --case C6 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 5
  common_end
  need_reason quarantine
  assert_image_kept
  if grep -E '^detach( |$)' "$ROOT/hdiutil.log"; then die "detached while vm up"; fi
  if grep -F -q -- '-force' "$ROOT/hdiutil.log"; then die "forced while vm up"; fi
  "$PYTHON" - "$CASE/out/report.json" <<'PY' || die "c6 vm up"
import json, sys
rep = json.load(open(sys.argv[1]))
cases = rep["stages"][0]["cases"]
assert [c["boundary"] for c in cases] == ["file", "dir"], cases
assert cases[0]["verdict"] == "quarantine", cases[0]
fields = dict(tok.split("=", 1) for tok in cases[0]["record"].split())
assert fields["guest_held"] == "1", cases[0]["record"]
assert fields["force"] == "0", cases[0]["record"]
assert cases[1]["verdict"] == "not_run", cases[1]
PY
}

test_stage_v_c7_m0_empty_ok() {
  setup
  enable_volume
  write_pinned
  write_fake_probe
  run_probe 30 --stage V --case C7 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 0
  common_end
  assert_image_gone
  assert_detach_count 1
  assert_force_count 0
  grep -F -q 'run C7' "$ROOT/ssh.log" || die "ssh C7"
  "$PYTHON" - "$CASE/out/report.json" <<'PY' || die "c7 ok"
import json, sys
rep = json.load(open(sys.argv[1]))
c = rep["stages"][0]["cases"][0]
assert c["verdict"] == "ok" and c["reason"] == "measured", (c["verdict"], c["reason"])
fields = dict(tok.split("=", 1) for tok in c["record"].split())
assert fields["detach_class"] == "none", c["record"]
assert fields["tart_alive"] == "yes", c["record"]
assert fields["m0_entries"] == "0", c["record"]
assert fields["guest"] == "ok:8,enoent:0,eio:0,other:0,hang:0", c["record"]
PY
}

test_stage_v_c7_m0_entries_failed() {
  setup
  enable_volume
  write_pinned
  write_fake_probe
  touch "$ROOT/plant_m0"
  run_probe 30 --stage V --case C7 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 5
  common_end
  need_reason m0-write
  local mp img
  mp=$(mount_point)
  [[ -f "$mp/s11-landed" ]] || die "landed missing"
  img=$(image_path)
  [[ -n "$img" && -f "$img" && ! -L "$img" ]] || die "m0 image removed"
  if grep -F -q -- '-force' "$ROOT/hdiutil.log"; then die "m0 forced"; fi
  "$PYTHON" - "$CASE/out/report.json" <<'PY' || die "c7 m0"
import json, sys
rep = json.load(open(sys.argv[1]))
c = rep["stages"][0]["cases"][0]
assert c["verdict"] == "failed" and c["reason"] == "m0-write", (c["verdict"], c["reason"])
fields = dict(tok.split("=", 1) for tok in c["record"].split())
assert fields["m0_entries"] == "1", c["record"]
assert fields["tart_alive"] == "yes", c["record"]
assert fields["detach_rc"] == "0", c["record"]
PY
}

test_stage_v_c7_detach_fail_no_force() {
  setup
  enable_volume
  write_pinned
  write_fake_probe
  touch "$ROOT/fail_detach_once"
  run_probe 30 --stage V --case C7 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 0
  common_end
  assert_image_gone
  local nd nf
  nd=$(grep -E -c '^detach [^ -]' "$ROOT/hdiutil.log" || true)
  nf=$(force_lines)
  [[ "$nd" == 2 && "$nf" == 0 ]] || die "c7 fail detach normal=$nd force=$nf"
  grep -F -q 'run C7' "$ROOT/ssh.log" || die "ssh C7 after busy"
  "$PYTHON" - "$CASE/out/report.json" <<'PY' || die "c7 busy"
import json, sys
rep = json.load(open(sys.argv[1]))
c = rep["stages"][0]["cases"][0]
assert c["verdict"] == "ok", (c["verdict"], c["reason"])
fields = dict(tok.split("=", 1) for tok in c["record"].split())
assert fields["detach_class"] == "busy", c["record"]
assert fields["detach_rc"] != "0", c["record"]
assert fields["guest"] == "ok:8,enoent:0,eio:0,other:0,hang:0", c["record"]
assert fields["m0_entries"] == "unknown", c["record"]
PY
}

test_stage_v_c5_f1_recovery() {
  setup
  enable_volume
  c5_crash F1
  c5_recover
  c5_expect_clean "$DETACH_BEFORE"
}
test_stage_v_c5_f3_recovery() {
  setup
  enable_volume
  c5_crash F3
  c5_recover
  c5_expect_clean "$((DETACH_BEFORE + 1))"
}
test_stage_v_c5_f4_recovery() {
  setup
  enable_volume
  c5_crash F4
  c5_recover
  c5_expect_clean "$DETACH_BEFORE"
}
test_stage_v_c5_f5_recovery() {
  setup
  enable_volume
  c5_crash F5
  c5_recover
  c5_expect_clean "$DETACH_BEFORE"
}
test_stage_v_c5_f2_quarantine() {
  setup
  enable_volume
  c5_crash F2
  c5_recover
  need_ec 5
  common_end yes "$CASE/outr"
  grep -F -x -q 'recovery=noted' "$RCPT" || die "F2 not noted"
  [[ -f "$IMAGE" && ! -L "$IMAGE" ]] || die "F2 image not kept"
  local after
  after=$(detach_lines)
  [[ "$after" == "$DETACH_BEFORE" ]] || die "F2 detached"
  assert_stage_r quarantine image-unmatched kept kept kept
}

test_stage_r_receipt_tamper() {
  setup
  enable_volume
  mkdir -p "$CASE/attempt"
  printf 'keep\n' >"$CASE/sentinel"
  printf 'keep\n' >"$CASE/attempt/keepme"
  local kind body attempt image mount
  attempt=$CASE/attempt
  image=$CASE/attempt/mailbox.sparseimage
  mount=$CASE/attempt/mnt
  for kind in extra dup escape; do
    rm -rf "$CASE/out"
    rm -f "$ROOT/hdiutil.log" "$CASE/bad-receipt"
    case "$kind" in
      extra)
        printf '%s\n' 'phase=reserved' "image=$image" 'image_dev=' 'image_ino=' 'device=' "mount=$mount" 'mount_dev=11' 'mount_ino=22' "attempt=$attempt" 'attempt_dev=11' 'attempt_ino=22' 'case=C5' 'boundary=-' 'evil=1' >"$CASE/bad-receipt"
        ;;
      dup)
        printf '%s\n' 'phase=reserved' 'phase=created' "image=$image" 'image_dev=' 'image_ino=' 'device=' "mount=$mount" 'mount_dev=11' 'mount_ino=22' "attempt=$attempt" 'attempt_dev=11' 'attempt_ino=22' 'case=C5' 'boundary=-' >"$CASE/bad-receipt"
        ;;
      escape)
        printf '%s\n' 'phase=reserved' "image=$image" 'image_dev=' 'image_ino=' 'device=' "mount=$mount" 'mount_dev=11' 'mount_ino=22' "attempt=$CASE/../escape" 'attempt_dev=11' 'attempt_ino=22' 'case=C5' 'boundary=-' >"$CASE/bad-receipt"
        ;;
    esac
    run_probe 8 --stage R --receipt "$CASE/bad-receipt" "${ARGS[@]}"
    need_ec 2
    common_end
    need_reason receipt-bad
    [[ ! -f "$ROOT/hdiutil.log" ]] || die "tamper $kind called hdiutil"
    if grep -F -x -q 'recovery=noted' "$CASE/bad-receipt"; then die "tamper $kind noted"; fi
    [[ -f "$CASE/sentinel" && -f "$CASE/attempt/keepme" ]] || die "tamper $kind deleted sentinel"
  done
}

assert_untouched() {
  local why=$1
  [[ "$(detach_lines)" == "$DETACH_BEFORE" ]] || die "$why detach $(detach_lines)"
  [[ "$(force_lines)" == "$FORCE_BEFORE" ]] || die "$why force"
  [[ -f "$IMAGE" && ! -L "$IMAGE" ]] || die "$why image"
  [[ -d "$MOUNT" && ! -L "$MOUNT" ]] || die "$why mount"
  if grep -F -x -q 'recovery=noted' "$RCPT"; then die "$why noted"; fi
}

assert_r_at() {
  "$PYTHON" - "$1" "$2" "$3" "$4" "$5" "$6" <<'PY' || die "stage R shape"
import json, sys
path, verdict, reason, image, mount, attempt = sys.argv[1:]
rep = json.load(open(path))
assert rep["schema"] == "s11-0-probe/v2", rep["schema"]
assert rep["exclusive_window"] == "assumed", rep["exclusive_window"]
st = rep["stages"][0]
assert st["stage"] == "R", st
assert st["verdict"] == verdict, st
assert st["reason"] == reason, st
assert (st["image"], st["mount"], st["attempt"]) == (image, mount, attempt), st
PY
}

set_receipt_case() {
  local tmp=$RCPT.tmp
  awk -F= -v c="$1" 'BEGIN{OFS="="} $1=="case"{$2=c} {print}' "$RCPT" >"$tmp"
  mv "$tmp" "$RCPT"
  chmod 600 "$RCPT"
}

# A parseable C7 receipt is refused before detach, unlink, or rmdir.
test_stage_r_c7_refused() {
  setup
  enable_volume
  c5_crash F3
  set_receipt_case C7
  [[ -f "$IMAGE" && -d "$MOUNT" ]] || die "F3 leftovers"
  grep -F -q 'attached=1' "$ROOT/vol.state" || die "not attached"
  c5_recover
  need_ec 2
  common_end yes "$CASE/outr"
  need_reason_outr recovery-unsupported
  assert_untouched c7
}

# Normal detach fails and the image inode moves: do not issue force.
test_stage_r_shift_skips_force() {
  setup
  enable_volume
  c5_crash F3
  touch "$ROOT/fail_detach" "$ROOT/shift_after_detach"
  c5_recover
  need_ec 5
  common_end yes "$CASE/outr"
  assert_stage_r quarantine mapping-changed kept kept kept
  [[ "$(detach_lines)" == $((DETACH_BEFORE + 1)) ]] || die "normal detach count"
  [[ "$(force_lines)" == "$FORCE_BEFORE" ]] || die "force after shift"
  [[ -f "$IMAGE" && -d "$MOUNT" ]] || die "shift removed volume"
}

test_stage_r_force_once() {
  local f1 i
  setup
  enable_volume
  c5_crash F3
  touch "$ROOT/fail_detach" "$ROOT/fail_force"
  c5_recover
  need_ec 5
  common_end yes "$CASE/outr"
  assert_stage_r quarantine detach-failed kept kept kept
  grep -F -x -q 'force=intent' "$RCPT" || die "intent not recorded"
  [[ "$(force_lines)" == $((FORCE_BEFORE + 1)) ]] || die "first force count"
  [[ -f "$IMAGE" ]] || die "first image gone"
  f1=$(force_lines)
  rm -rf "$CASE/outr2"
  run_probe 12 --stage R --receipt "$RCPT" "${ARGS[@]}" --out "$CASE/outr2"
  need_ec 5
  common_end yes "$CASE/outr2"
  assert_r_at "$CASE/outr2/report.json" quarantine force-spent kept kept kept
  [[ "$(force_lines)" == "$f1" ]] || die "second run forced"
  [[ -f "$IMAGE" && -d "$MOUNT" ]] || die "second run removed volume"

  c5_crash F3
  touch "$ROOT/fail_detach" "$ROOT/hang_force"
  rm -f "$ROOT/force.count"
  start_probe 20 --stage R --receipt "$RCPT" "${ARGS[@]}" --out "$CASE/outr"
  i=0
  while [[ "$i" -lt 50 ]]; do
    [[ -s "$ROOT/force.count" ]] && grep -F -x -q 'force=intent' "$RCPT" && break
    sleep 0.1
    i=$((i + 1))
  done
  [[ -s "$ROOT/force.count" ]] || die "force never started"
  grep -F -x -q 'force=intent' "$RCPT" || die "intent missing at hang"
  kill -KILL -- -"$PROBE_PID" 2>/dev/null || true
  finish_probe
  find "$CASE/tmp" -mindepth 1 -maxdepth 1 -name 's11-0.*' -exec rm -rf {} +
  f1=$(force_lines)
  rm -rf "$CASE/outr2"
  run_probe 12 --stage R --receipt "$RCPT" "${ARGS[@]}" --out "$CASE/outr2"
  need_ec 5
  common_end yes "$CASE/outr2"
  assert_r_at "$CASE/outr2/report.json" quarantine force-spent kept kept kept
  [[ "$(force_lines)" == "$f1" ]] || die "force after crash"
  [[ -f "$IMAGE" && -d "$MOUNT" ]] || die "crash recovery removed volume"
}

test_stage_r_trust_retarget() {
  local decoy dev ino tmp
  setup
  enable_volume
  c5_crash F4
  decoy=$ATTEMPT/not-mailbox.sparseimage
  cp "$IMAGE" "$decoy"
  chmod 600 "$decoy"
  /usr/bin/stat -c '%d %i' "$decoy" >"$CASE/id.tmp"
  read -r dev ino <"$CASE/id.tmp"
  tmp=$RCPT.tmp
  awk -F= -v p="$decoy" -v d="$dev" -v i="$ino" 'BEGIN{OFS="="}
    $1=="image"{$0="image=" p}
    $1=="image_dev"{$0="image_dev=" d}
    $1=="image_ino"{$0="image_ino=" i}
    {print}' "$RCPT" >"$tmp"
  mv "$tmp" "$RCPT"
  chmod 600 "$RCPT"
  c5_recover
  need_ec 2
  common_end yes "$CASE/outr"
  need_reason_outr receipt-untrusted
  assert_untouched retarget
  [[ -f "$decoy" ]] || die "decoy removed"

  c5_crash F4
  mkdir -m 0700 "$CASE/out/s11v.sibling"
  c5_recover
  need_ec 2
  common_end yes "$CASE/outr"
  need_reason_outr receipt-untrusted
  assert_untouched sibling
  [[ -d "$CASE/out/s11v.sibling" ]] || die "sibling removed"

  c5_crash F4
  mv "$CASE" "$CASE.real"
  ln -s "$CASE.real" "$CASE"
  c5_recover
  need_ec 2
  common_end yes "$CASE/outr"
  need_reason_outr receipt-untrusted
  assert_untouched symlink
}

test_stage_r_preflight_keeps_image() {
  setup
  enable_volume
  c5_crash F4
  /usr/bin/rmdir "$MOUNT" || die "mount not empty"
  mkdir -m 0700 "$MOUNT"
  c5_recover
  need_ec 5
  common_end yes "$CASE/outr"
  assert_stage_r quarantine identity-mismatch kept kept kept
  [[ "$(detach_lines)" == "$DETACH_BEFORE" ]] || die "replaced mount detached"
  [[ -f "$IMAGE" ]] || die "replaced mount unlinked image"

  c5_crash F4
  printf x >"$MOUNT/extra"
  c5_recover
  need_ec 5
  common_end yes "$CASE/outr"
  assert_stage_r quarantine mount-dirty kept kept kept
  [[ "$(detach_lines)" == "$DETACH_BEFORE" ]] || die "dirty mount detached"
  [[ -f "$IMAGE" && -f "$MOUNT/extra" ]] || die "dirty mount removed"

  c5_crash F4
  printf x >"$ATTEMPT/stray"
  c5_recover
  need_ec 5
  common_end yes "$CASE/outr"
  assert_stage_r quarantine attempt-dirty kept kept kept
  [[ "$(detach_lines)" == "$DETACH_BEFORE" ]] || die "dirty attempt detached"
  [[ -f "$IMAGE" && -f "$ATTEMPT/stray" ]] || die "dirty attempt removed"

  # Line-breaking names must not hide from the attempt audit.
  local nl
  for nl in $'\n' $'mailbox.sparseimage\n'; do
    c5_crash F4
    printf x >"$ATTEMPT/$nl"
    c5_recover
    need_ec 5
    common_end yes "$CASE/outr"
    [[ "$(detach_lines)" == "$DETACH_BEFORE" ]] || die "newline name detached"
    [[ -f "$IMAGE" && -f "$ATTEMPT/$nl" ]] || die "newline name removed image"
    grep -F -q '"verdict":"quarantine"' "$CASE/outr/report.json" || die "newline name not quarantined"
  done
}

test_stage_r_receipt_bound() {
  local t0 n
  setup
  enable_volume
  c5_crash F4
  export S11_RECEIPT_STALL=30 S11_R_BUDGET=2
  t0=$SECONDS
  run_probe 8 --stage R --receipt "$RCPT" "${ARGS[@]}" --out "$CASE/outr"
  need_ec 2
  common_end yes "$CASE/outr"
  need_reason_outr receipt-bad
  assert_untouched stall
  [[ $((SECONDS - t0)) -lt 5 ]] || die "stall blocked the probe"

  c5_crash F4
  unset S11_RECEIPT_STALL S11_R_BUDGET
  /usr/bin/dd if=/dev/zero bs=1000 count=9 >>"$RCPT" 2>/dev/null
  chmod 600 "$RCPT"
  n=$(wc -c <"$RCPT" | tr -d ' ')
  [[ "$n" -gt 8192 ]] || die "receipt not oversized ($n)"
  t0=$SECONDS
  run_probe 8 --stage R --receipt "$RCPT" "${ARGS[@]}" --out "$CASE/outr"
  need_ec 2
  common_end yes "$CASE/outr"
  need_reason_outr receipt-bad
  assert_untouched oversized
  [[ $((SECONDS - t0)) -lt 5 ]] || die "oversized read blocked"
}

test_stage_v_c7_guest_failed() {
  local kind reason
  for kind in fail hang bad zero short; do
    rm -rf "$CASE/out"
    setup
    enable_volume
    write_pinned
    write_fake_probe
    rm -f "$ROOT/c7_ssh_fail" "$ROOT/c7_hang" "$ROOT/c7_bad" "$ROOT/c7_zero" "$ROOT/c7_short"
    unset S11_C7_BUDGET
    reason=guest-failed
    case "$kind" in
      fail) touch "$ROOT/c7_ssh_fail" ;;
      hang) touch "$ROOT/c7_hang"; export S11_C7_BUDGET=2 ;;
      bad) touch "$ROOT/c7_bad"; reason=guest-malformed ;;
      zero) touch "$ROOT/c7_zero"; reason=guest-malformed ;;
      short) touch "$ROOT/c7_short"; reason=guest-malformed ;;
    esac
    run_probe 25 --stage V --case C7 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
    need_ec 4
    common_end
    need_reason "$reason"
    "$PYTHON" - "$CASE/out/report.json" "$reason" <<'PY' || die "c7 $kind"
import json, sys
c = json.load(open(sys.argv[1]))["stages"][0]["cases"][0]
assert c["verdict"] == "unknown" and c["reason"] == sys.argv[2], (c["verdict"], c["reason"])
PY
  done
}

# A measured first-operation hang is a result, not a malformed record.
test_stage_v_c7_hang_measured() {
  setup
  enable_volume
  write_pinned
  write_fake_probe
  touch "$ROOT/c7_hang1"
  run_probe 25 --stage V --case C7 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 0
  common_end
  "$PYTHON" - "$CASE/out/report.json" <<'PY' || die "c7 hang measured"
import json, sys
c = json.load(open(sys.argv[1]))["stages"][0]["cases"][0]
assert c["verdict"] == "ok" and c["reason"] == "measured", (c["verdict"], c["reason"])
fields = dict(tok.split("=", 1) for tok in c["record"].split())
assert fields["guest"] == "ok:0,enoent:0,eio:0,other:0,hang:1", c["record"]
PY
}

test_stage_v_c7_tart_dead() {
  setup
  enable_volume
  write_pinned
  write_fake_probe
  touch "$ROOT/c7_tart_dead"
  run_probe 25 --stage V --case C7 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 4
  common_end
  need_reason tart-dead
  "$PYTHON" - "$CASE/out/report.json" <<'PY' || die "c7 tart dead"
import json, sys
c = json.load(open(sys.argv[1]))["stages"][0]["cases"][0]
assert c["verdict"] == "unknown" and c["reason"] == "tart-dead", (c["verdict"], c["reason"])
fields = dict(tok.split("=", 1) for tok in c["record"].split())
assert fields["guest"].startswith("ok:"), c["record"]
assert fields["tart_alive"] != "yes", c["record"]
PY
}

test_stage_v_c7_not_ready() {
  setup
  enable_volume
  write_pinned
  write_fake_probe
  touch "$ROOT/c7_no_ready"
  export S11_READY_SEC=1
  run_probe 25 --stage V --case C7 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 4
  common_end
  need_reason not-ready
  if grep -F -q 'run C7' "$ROOT/ssh.log"; then die "ran C7 before ready"; fi
  grep -F -q 'run C1' "$ROOT/ssh.log" || die "no ready handshake"
  "$PYTHON" - "$CASE/out/report.json" <<'PY' || die "c7 not ready"
import json, sys
c = json.load(open(sys.argv[1]))["stages"][0]["cases"][0]
assert c["verdict"] == "unknown" and c["reason"] == "not-ready", (c["verdict"], c["reason"])
PY
}

test_stage_v_c7_reattach_keeps_image() {
  local img
  setup
  enable_volume
  write_pinned
  write_fake_probe
  touch "$ROOT/c7_reattach"
  run_probe 25 --stage V --case C7 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  need_ec 5
  common_end
  img=$(image_path)
  [[ -n "$img" && -f "$img" && ! -L "$img" ]] || die "reattach removed image"
  [[ "$(detach_lines)" -ge 1 ]] || die "early detach missing"
  "$PYTHON" - "$CASE/out/report.json" <<'PY' || die "c7 reattach"
import json, sys
c = json.load(open(sys.argv[1]))["stages"][0]["cases"][0]
assert c["verdict"] != "ok" and c["reason"] != "measured", (c["verdict"], c["reason"])
PY
}

test_stage_v_c10_dest_unknown() {
  local flag
  for flag in tmutil_empty tmutil_sep tmutil_kind tmutil_trunc tmutil_mix; do
    rm -rf "$CASE/out"
    setup
    enable_volume
    export S11_C10_WAIT=0
    rm -f "$ROOT/tmutil_empty" "$ROOT/tmutil_sep" "$ROOT/tmutil_kind" "$ROOT/tmutil_trunc" "$ROOT/tmutil_mix" \
      "$ROOT/hdiutil.log" "$ROOT/diskutil.log" "$ROOT/vol.state" "$ROOT/tmutil.log"
    touch "$ROOT/$flag"
    run_probe 12 --stage V --case C10 "${ARGS[@]}"
    need_ec 4
    common_end
    assert_image_gone
    if grep -F -q S11SecretDestName "$CASE/out/report.json"; then die "secret in $flag"; fi
    "$PYTHON" - "$CASE/out/report.json" <<'PY' || die "c10 $flag"
import json, sys
c = json.load(open(sys.argv[1]))["stages"][0]["cases"][0]
fields = dict(tok.split("=", 1) for tok in c["record"].split())
assert fields["dest_count"] == "unknown", fields["dest_count"]
PY
  done
}

test_stage_v_new_usage() {
  setup
  write_pinned
  write_fake_probe
  try_usage() {
    rm -rf "$CASE/out"
    rm -f "$ROOT/hdiutil.log" "$ROOT/diskutil.log"
    run_probe 8 "$@"
    need_ec 2
    common_end
    need_reason usage
    [[ ! -f "$ROOT/hdiutil.log" ]] || die "usage called hdiutil"
  }
  try_usage --stage V --case C4 --fault F1 "${ARGS[@]}"
  try_usage --stage V --case C10 --fault F1 "${ARGS[@]}"
  try_usage --stage M --case C1 --fault F1 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  try_usage --stage V --case C5 "${ARGS[@]}"
  try_usage --stage V --case C6 --fault F1 --probe "$ROOT/bin/s11probe" --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  try_usage --stage R --fault F1 --receipt "$CASE/pinned" "${ARGS[@]}"
  try_usage --stage R "${ARGS[@]}"
  try_usage --dry-run --stage R --receipt "$CASE/pinned" "${ARGS[@]}"
  try_usage --stage V --case C10 --probe "$ROOT/bin/s11probe" "${ARGS[@]}"
  try_usage --stage V --case C10 --pinned-host-key "$CASE/pinned" "${ARGS[@]}"
  try_usage --stage all --fault F1 --endpoint-placeholder "${ARGS[@]}"
}

run_case stage-v-c10 test_stage_v_c10_observed
run_case stage-v-c10-nodest test_stage_v_c10_nodest
run_case stage-v-c6 test_stage_v_c6_guest_hold
run_case stage-v-c6-notheld test_stage_v_c6_not_held
run_case stage-v-c6-noforce test_stage_v_c6_no_force_while_vm_up
run_case stage-v-c7-hang1 test_stage_v_c7_hang_measured
run_case stage-v-c7-ok test_stage_v_c7_m0_empty_ok
run_case stage-v-c7-m0 test_stage_v_c7_m0_entries_failed
run_case stage-v-c7-busy test_stage_v_c7_detach_fail_no_force
run_case stage-v-c5-f1 test_stage_v_c5_f1_recovery
run_case stage-v-c5-f3 test_stage_v_c5_f3_recovery
run_case stage-v-c5-f4 test_stage_v_c5_f4_recovery
run_case stage-v-c5-f5 test_stage_v_c5_f5_recovery
run_case stage-v-c5-f2 test_stage_v_c5_f2_quarantine
run_case stage-r-tamper test_stage_r_receipt_tamper
run_case stage-v-new-usage test_stage_v_new_usage
run_case stage-r-c7 test_stage_r_c7_refused
run_case stage-r-shift test_stage_r_shift_skips_force
run_case stage-r-force-once test_stage_r_force_once
run_case stage-r-trust test_stage_r_trust_retarget
run_case stage-r-preflight test_stage_r_preflight_keeps_image
run_case stage-r-bound test_stage_r_receipt_bound
run_case stage-v-c7-guest test_stage_v_c7_guest_failed
run_case stage-v-c7-tart test_stage_v_c7_tart_dead
run_case stage-v-c7-ready test_stage_v_c7_not_ready
run_case stage-v-c7-reattach test_stage_v_c7_reattach_keeps_image
run_case stage-v-c10-unknown test_stage_v_c10_dest_unknown
echo "ALL_OK ${SECONDS}s"
[[ "$SECONDS" -lt 240 ]] || { echo "suite took ${SECONDS}s" >&2; exit 1; }

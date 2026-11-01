#!/bin/bash
# S11-0 fact probe, ADR 0018 §10. Bash 3.2. Own a clone only after tart list shows it. Signal that direct child only. h1_independent_of_ip=unknown.
set -u; set -f; umask 077; export LC_ALL=C
GOLDEN=macos-s2-eval; GUEST_USER=eval; PLACEHOLDER_IP=203.0.113.10
PROBE_BUDGET=1500; CLEANUP_BUDGET=90; CAP=65536; GRACE=1; START_CONFIRM=1
FINAL_EC=1; STATUS=partial; DRY=0; STAGE=all; OUT=./s11-0-out; TART_HOME_ARG=""
SSH_KEY="${HOME}/.ssh/id_ed25519_tart"; ENDPOINT=""; ENDPOINT_SOURCE=absent; LAN_IP=""
USAGE_REASON=""; PRECHECK_FAILED=0; ANY_COLLECTION_FAILED=0; ANY_CLEANUP_FAILED=0; ANY_CREATE_FAILED=0
REQUEST_CLEANUP=0; IN_CLEANUP=0; FINALIZING=0; PROBE_DEADLINE=0; CLEANUP_DEADLINE=0; WORKDIR=""
REASONS_FILE=""; OWN_NAME=""; OWN_OK=0; RUN_PID=""; RUN_LIVE=0; RUN_COMM=""
CMD_PID=""; CMD_LIVE=0; CMD_COMM=""; RB_STATUS=0; RB_TIMED_OUT=0; RB_ABORTED=0; REAP_STATUS=0
S_ARGV_OK=0; HELP_STATE=not-run; HELP_SUBS=""; EXEC_HELP=not-run; EXEC_OPTS=""; EXEC_RESULT=not-run
MEASURED=""; UNMEASURED=""; RUN_ARGV=()
S_STAGE=""; S_NAME=""; S_CREATED=no; S_LAUNCH=not-run; S_COLL=not-run
S_SSH=not-run; S_EXEC=not-run; S_V6=not-run

say() { printf 's11-0-probe: %s\n' "$*" >&2; }
add_reason() {
  [[ -n "$REASONS_FILE" ]] || return 0
  grep -Fqx -- "$1" "$REASONS_FILE" 2>/dev/null || printf '%s\n' "$1" >>"$REASONS_FILE"
}
cap_secs() {
  local n="$1" b="${S11_BOUND_SEC:-}"
  case "$b" in ""|*[!0-9]*) printf '%s\n' "$n"; return ;; esac
  if [[ "$b" -gt 0 && "$b" -lt "$n" ]]; then printf '%s\n' "$b"; else printf '%s\n' "$n"; fi
}
cap_file() {
  [[ -f "$1" ]] || return 0
  head -c "$CAP" "$1" >"$1.cap" 2>/dev/null || true
  mv "$1.cap" "$1" 2>/dev/null || true
}
proc_snap() { ps -p "$1" -o ppid= -o state= -o comm= 2>/dev/null | awk 'NR==1{print $1,$2,$3}'; }
same_proc() {
  local snap pp st got
  [[ -n "${1:-}" && -n "${2:-}" ]] || return 1
  snap=$(proc_snap "$1"); case "$snap" in *" "*" "*) ;; *) return 1 ;; esac
  pp=${snap%% *}; snap=${snap#* }; st=${snap%% *}; got=${snap#* }; [[ "$st" == Z* ]] && return 1
  [[ "$pp" == "$$" && "$got" == "$2" ]]
}
signal_owned() {
  [[ "$4" == 1 ]] || return 0
  same_proc "$1" "$3" || return 0
  kill "-$2" "$1" 2>/dev/null || true
}
reap_owned() {
  local n=0; REAP_STATUS=0
  while [[ "$n" -lt 8 ]]; do
    n=$((n + 1)); wait "$1"; REAP_STATUS=$?
    same_proc "$1" "$2" || return 0
    kill -KILL "$1" 2>/dev/null || true
  done
}
grace_kill() {
  local pid="$1" comm="$2" g=$SECONDS
  signal_owned "$pid" TERM "$comm" 1
  while [[ "$SECONDS" -lt $((g + GRACE)) ]]; do
    same_proc "$pid" "$comm" || return 0
    sleep 0.05
  done
  same_proc "$pid" "$comm" && signal_owned "$pid" KILL "$comm" 1
}
run_bounded() {
  local secs="$1" out="$2" err="$3" infile="$4" start sz ez pid comm snap left
  shift 4
  [[ "${1:-}" == -- ]] && shift
  secs=$(cap_secs "$secs"); : >"$out"; : >"$err"
  [[ "$infile" == - ]] && infile=/dev/null
  RB_TIMED_OUT=0; RB_ABORTED=0; RB_STATUS=0
  if [[ "$IN_CLEANUP" == 1 ]]; then
    [[ "$SECONDS" -ge "$CLEANUP_DEADLINE" ]] && { RB_TIMED_OUT=1; RB_STATUS=124; return 124; }
    left=$((CLEANUP_DEADLINE - SECONDS)); [[ "$left" -lt 1 ]] && left=1
    [[ "$secs" -gt "$left" ]] && secs=$left
  elif [[ "$SECONDS" -ge "$PROBE_DEADLINE" ]]; then
    RB_TIMED_OUT=1; RB_STATUS=124; REQUEST_CLEANUP=1; return 124
  fi
  "$@" <"$infile" >"$out" 2>"$err" &
  CMD_PID=$!; CMD_LIVE=1; snap=$(proc_snap "$CMD_PID"); CMD_COMM=${snap##* }; start=$SECONDS
  while same_proc "$CMD_PID" "$CMD_COMM"; do
    if [[ "$SECONDS" -ge $((start + secs)) ]]; then RB_TIMED_OUT=1; grace_kill "$CMD_PID" "$CMD_COMM"; break; fi
    if [[ "$IN_CLEANUP" != 1 && "$REQUEST_CLEANUP" == 1 ]]; then RB_ABORTED=1; signal_owned "$CMD_PID" TERM "$CMD_COMM" 1; break; fi
    sz=$(wc -c <"$out" 2>/dev/null | tr -d ' '); ez=$(wc -c <"$err" 2>/dev/null | tr -d ' ')
    [[ "${sz:-0}" -gt "$CAP" || "${ez:-0}" -gt "$CAP" ]] && { signal_owned "$CMD_PID" TERM "$CMD_COMM" 1; break; }
    sleep 0.05
  done
  pid=$CMD_PID; comm=$CMD_COMM; CMD_LIVE=0; CMD_PID=""; CMD_COMM=""
  reap_owned "$pid" "$comm"
  cap_file "$out"; cap_file "$err"
  if [[ "$RB_TIMED_OUT" == 1 ]]; then RB_STATUS=124
  elif [[ "$RB_ABORTED" == 1 ]]; then RB_STATUS=130
  else RB_STATUS=$REAP_STATUS; fi
  return "$RB_STATUS"
}
start_run() {
  local out="$1" err="$2" pid snap comm confirm start
  shift 2
  : >"$out"; : >"$err"
  "$@" </dev/null >"$out" 2>"$err" &
  pid=$!; snap=$(proc_snap "$pid"); comm=${snap##* }
  confirm=$(cap_secs "$START_CONFIRM")
  case "${S11_START_SEC:-}" in ""|*[!0-9]*) ;; *) [[ "$S11_START_SEC" -lt "$confirm" ]] && confirm=$S11_START_SEC ;; esac
  start=$SECONDS
  while true; do
    if ! same_proc "$pid" "$comm"; then reap_owned "$pid" "$comm"; cap_file "$out"; cap_file "$err"; return 1; fi
    [[ "$SECONDS" -ge $((start + confirm)) ]] && { RUN_PID=$pid; RUN_LIVE=1; RUN_COMM=$comm; return 0; }
    sleep 0.05
  done
}
stop_run_proc() {
  [[ "$RUN_LIVE" == 1 && -n "$RUN_PID" ]] || return 0
  local pid=$RUN_PID comm=$RUN_COMM
  same_proc "$pid" "$comm" && grace_kill "$pid" "$comm"
  RUN_LIVE=0; RUN_PID=""; RUN_COMM=""
  reap_owned "$pid" "$comm" || true
  cap_file "$WORKDIR/run.out"; cap_file "$WORKDIR/run.err"
}
name_is_safe() {
  local n="$1" tail pid rand
  [[ "$n" != "$GOLDEN" ]] || return 1
  case "$n" in s11-0-probe-A-*) tail=${n#s11-0-probe-A-} ;; s11-0-probe-B-*) tail=${n#s11-0-probe-B-} ;; *) return 1 ;; esac
  pid=${tail%%-*}; rand=${tail#*-}
  case "$pid" in ""|*[!0-9]*) return 1 ;; esac
  case "$rand" in ""|*[!0-9a-f]*|*-*) return 1 ;; esac
  [[ "$pid" != "$tail" ]]
}
make_name() {
  local stage="$1" rand
  if [[ -n "${S11_FORCE_NAME:-}" ]]; then
    case "$S11_FORCE_NAME" in s11-0-probe-"$stage"-*) name_is_safe "$S11_FORCE_NAME" && { printf '%s\n' "$S11_FORCE_NAME"; return 0; }; return 1 ;; esac
  fi
  rand=$(od -An -N4 -tx1 /dev/urandom 2>/dev/null | tr -d ' \n' | tr 'ABCDEF' 'abcdef')
  case "$rand" in [0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]) ;; *) return 1 ;; esac
  printf 's11-0-probe-%s-%s-%s\n' "$stage" "$$" "$rand"
}
is_ipv4() {
  local ip="$1" rest a b c d o
  case "$ip" in *.*.*.*) ;; *) return 1 ;; esac
  a=${ip%%.*}; rest=${ip#*.}; b=${rest%%.*}; rest=${rest#*.}; c=${rest%%.*}; d=${rest#*.}
  [[ "${a}.${b}.${c}.${d}" == "$ip" ]] || return 1
  for o in "$a" "$b" "$c" "$d"; do case "$o" in ""|*[!0-9]*|0?*) return 1 ;; esac; [[ "$((10#$o))" -le 255 ]] || return 1; done
}
is_public_ip() {
  local ip="$1" a b rest
  is_ipv4 "$ip" || return 1
  a=${ip%%.*}; rest=${ip#*.}; b=${rest%%.*}; a=$((10#$a)); b=$((10#$b))
  [[ "$a" -eq 0 || "$a" -eq 10 || "$a" -eq 127 || "$a" -ge 224 ]] && return 1
  [[ "$a" -eq 169 && "$b" -eq 254 ]] && return 1; [[ "$a" -eq 192 && "$b" -eq 168 ]] && return 1
  [[ "$a" -eq 172 && "$b" -ge 16 && "$b" -le 31 ]] && return 1; [[ "$a" -eq 100 && "$b" -ge 64 && "$b" -le 127 ]] && return 1
  return 0
}
check_softnet() {
  local p t i=0 owner mode bit bad=0
  p=$(command -v softnet 2>/dev/null || true)
  [[ -n "$p" ]] || { add_reason softnet-missing; return 1; }
  while [[ -L "$p" ]]; do
    i=$((i + 1)); [[ "$i" -le 16 ]] || { add_reason softnet-symlink-loop; return 1; }
    t=$(readlink "$p" 2>/dev/null) || { add_reason softnet-missing; return 1; }
    case "$t" in /*) p=$t ;; *) p=$(dirname "$p")/$t ;; esac
  done
  [[ -e "$p" ]] || { add_reason softnet-missing; return 1; }
  run_bounded 10 "$WORKDIR/stat.out" "$WORKDIR/stat.err" - -- stat -f '%Su %Sp' "$p" || true
  [[ "$RB_TIMED_OUT" == 1 || "$RB_STATUS" -ne 0 ]] && { add_reason softnet-stat-failed; return 1; }
  read -r owner mode <"$WORKDIR/stat.out" || { add_reason softnet-stat-failed; return 1; }
  case "$mode" in ??????????) ;; *) add_reason softnet-stat-failed; return 1 ;; esac
  bit=$(printf '%s' "$mode" | cut -c 4); [[ "$owner" == root ]] || { add_reason softnet-not-root; bad=1; }
  case "$bit" in s|S) ;; *) add_reason softnet-not-suid; bad=1 ;; esac; [[ "$bad" -eq 0 ]]
}
refresh_list() {
  run_bounded 10 "$WORKDIR/list.out" "$WORKDIR/list.err" - -- tart list || true
  cp "$WORKDIR/list.out" "$WORKDIR/list.last" 2>/dev/null || true
  [[ "$RB_TIMED_OUT" == 1 ]] && { add_reason list-timeout; return 1; }
  [[ "$RB_STATUS" -eq 0 ]] || { add_reason list-failed; return 1; }
}
vm_status() {
  [[ -f "$WORKDIR/list.last" ]] || return 1
  awk -v n="$1" '$1==n {print $2; found=1; exit} END{exit !found}' "$WORKDIR/list.last"
}
build_argv() {
  RUN_ARGV=(tart run "$1" --no-graphics --net-softnet); S_ARGV_OK=1
  [[ "$2" == B ]] && RUN_ARGV+=(--net-softnet-block=0.0.0.0/0 "--net-softnet-allow=${3}/32")
}
print_argv() { local x line=argv:; for x in "${RUN_ARGV[@]}"; do line="$line $x"; done; printf '%s\n' "$line"; }
write_guest() {
  cat >"$WORKDIR/guest.sh" <<'EOF'
set -u; export LC_ALL=C
obs(){ printf 'obs %s %s %s\n' "$1" "$2" "$3"; }
classify(){ local e; [[ "$1" -eq 127 ]] && { printf tool-error; return; }
  e=$(head -c 300 "$2" 2>/dev/null || true)
  case "$e" in
    *[Cc]onnection\ refused*) printf refused;;
    *[Nn]o\ route*|*[Nn]etwork\ is\ unreachable*|*[Hh]ost\ is\ unreachable*) printf no-route;;
    *[Oo]peration\ timed\ out*|*[Tt]imed\ out*|*[Tt]imeout*) printf timeout;;
    *) printf tool-error;; esac; }
run_tool(){ local ef st t=$SECONDS
  ef=$(mktemp "${TMPDIR:-/tmp}/s11e.XXXXXX") || { obs "$1" tool-error 0; return; }
  if [[ "$2" == tcp ]]; then command -v nc >/dev/null 2>&1 || { rm -f "$ef"; obs "$1" tool-error 0; return; }
    nc -z -G 2 -w 2 "$3" "$4" 2>"$ef"; st=$?
  elif command -v dig >/dev/null 2>&1; then dig +time=2 +tries=1 @"$3" example.com A >"$ef" 2>&1; st=$?
  elif command -v host >/dev/null 2>&1; then host -W 2 example.com "$3" >"$ef" 2>&1; st=$?
  elif command -v nslookup >/dev/null 2>&1; then nslookup -timeout=2 example.com "$3" >"$ef" 2>&1; st=$?
  else rm -f "$ef"; obs "$1" tool-error 0; return; fi
  if [[ "$st" -eq 0 ]]; then obs "$1" connected $((SECONDS - t))
  elif [[ "$2" != tcp && "$st" -eq 9 ]]; then obs "$1" timeout $((SECONDS - t))
  else obs "$1" "$(classify "$st" "$ef")" $((SECONDS - t)); fi; rm -f "$ef"; }
gw=""; command -v route >/dev/null 2>&1 && gw=$(route -n get default 2>/dev/null | awk '$1=="gateway:"{print $2; exit}')
case "$gw" in [0-9]*.[0-9]*.[0-9]*.[0-9]*) ;; *) gw="";; esac
if [[ -z "$gw" ]]; then obs gw22 no-address 0; obs gw53 no-address 0; obs gw80 no-address 0; obs gw443 no-address 0; obs dns-gw no-address 0
else run_tool gw22 tcp "$gw" 22; run_tool gw53 tcp "$gw" 53; run_tool gw80 tcp "$gw" 80; run_tool gw443 tcp "$gw" 443; run_tool dns-gw dns "$gw"; fi
run_tool dns-pub dns 1.1.1.1; run_tool meta80 tcp 169.254.169.254 80
if command -v ifconfig >/dev/null 2>&1; then
  v6=$(ifconfig 2>/dev/null | awk 'BEGIN{f="no"} $1=="inet6"{a=$2; sub(/%.*/,"",a); if(a~/^fe80:/||a=="::1"||a~/^fc/||a~/^fd/||a~/^ff/) next; if(a~/:/) f="yes"} END{print f}')
  if [[ "$v6" == yes ]]; then obs v6addr yes 0; else obs v6addr no-address 0; fi
else obs v6addr tool-error 0; fi
run_tool v6tcp tcp 2606:4700:4700::1111 443
[[ -n "${S11_LAN:-}" ]] && run_tool lan443 tcp "$S11_LAN" 443
[[ -n "${S11_EP:-}" ]] && run_tool ep443 tcp "$S11_EP" 443
exit 0
EOF
}
role_id() {
  case "$1" in
    gw22) printf tcp-gateway-22;; gw53) printf tcp-gateway-53;; gw80) printf tcp-gateway-80;; gw443) printf tcp-gateway-443;;
    dns-gw) printf dns-udp-gateway;; dns-pub) printf dns-udp-public;; meta80) printf tcp-metadata-80;;
    v6addr) printf ipv6-global-address;; v6tcp) printf tcp-ipv6-public;; lan443) printf tcp-lan-443;; ep443) printf tcp-endpoint-443;;
    *) return 1;;
  esac
}
take_obs() {
  local line tag role result sec id src=$1 dst=$2
  : >"$dst"
  while IFS= read -r line; do
    set -- $line; tag=${1:-}; role=${2:-}; result=${3:-}; sec=${4:-}
    [[ "$tag" == obs ]] || continue
    case "$result" in connected|refused|no-route|no-address|timeout|tool-error|yes) ;; *) continue ;; esac
    [[ "$result" == yes && "$role" != v6addr ]] && continue
    id=$(role_id "$role") || continue; case "$sec" in ""|*[!0-9]*) sec=0 ;; esac
    printf '%s %s %s\n' "$id" "$result" "$sec" >>"$dst"
  done <"$src"
}
ssh_guest() {
  local kh=$WORKDIR/known_hosts inner remote
  : >"$kh"
  inner='f=$(mktemp "${TMPDIR:-/tmp}/s11g.XXXXXX") || exit 127; cat >"$f" || exit 127; bash "$f"; st=$?; rm -f "$f"; exit $st'
  remote="S11_EP=${2} S11_LAN=${3} bash -c '${inner}'"
  run_bounded 50 "$WORKDIR/ssh.out" "$WORKDIR/ssh.err" "$WORKDIR/guest.sh" -- \
    ssh -o BatchMode=yes -o ConnectTimeout=8 -o ServerAliveInterval=5 -o ServerAliveCountMax=2 \
    -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new -o "UserKnownHostsFile=${kh}" \
    -i "$SSH_KEY" "${GUEST_USER}@${1}" "$remote" || true
}
cleanup_owned() {
  [[ "$IN_CLEANUP" == 1 ]] && return 0
  IN_CLEANUP=1; trap '' INT TERM HUP
  CLEANUP_DEADLINE=$((SECONDS + CLEANUP_BUDGET))
  case "${S11_CLEANUP_BUDGET:-}" in
    ""|*[!0-9]*) ;;
    *) [[ "$S11_CLEANUP_BUDGET" -gt 0 && "$S11_CLEANUP_BUDGET" -lt "$CLEANUP_BUDGET" ]] && CLEANUP_DEADLINE=$((SECONDS + S11_CLEANUP_BUDGET)) ;;
  esac
  local stop=not-run del=not-run absent=not-run
  if [[ "$OWN_OK" == 1 ]] && name_is_safe "$OWN_NAME"; then
    run_bounded 30 "$WORKDIR/c.out" "$WORKDIR/c.err" - -- tart stop "$OWN_NAME" || true
    if [[ "$RB_TIMED_OUT" == 1 ]]; then stop=timeout; add_reason stop-timeout; ANY_CLEANUP_FAILED=1
    elif [[ "$RB_STATUS" -ne 0 ]]; then stop=fail; add_reason stop-failed; ANY_CLEANUP_FAILED=1; else stop=ok; fi
    stop_run_proc
    run_bounded 30 "$WORKDIR/c.out" "$WORKDIR/c.err" - -- tart delete "$OWN_NAME" || true
    if [[ "$RB_TIMED_OUT" == 1 ]]; then del=timeout; add_reason delete-timeout; ANY_CLEANUP_FAILED=1
    elif [[ "$RB_STATUS" -ne 0 ]]; then del=fail; add_reason delete-failed; ANY_CLEANUP_FAILED=1; else del=ok; fi
    if refresh_list; then
      if vm_status "$OWN_NAME" >/dev/null; then absent=no; add_reason still-present; ANY_CLEANUP_FAILED=1; else absent=yes; OWN_OK=0; OWN_NAME=""; fi
    else absent=unknown; add_reason absent-unknown; ANY_CLEANUP_FAILED=1; fi
    printf '%s %s %s\n' "$stop" "$del" "$absent" >"$WORKDIR/cleanup.last"
  else stop_run_proc; fi
  IN_CLEANUP=0
  [[ "$FINALIZING" == 1 ]] || trap 'REQUEST_CLEANUP=1' INT TERM HUP
}
note_help() {
  if [[ "$RB_TIMED_OUT" == 1 ]]; then HELP_NOTE=timeout; elif [[ "$RB_STATUS" -ne 0 ]]; then HELP_NOTE=tool-error; else HELP_NOTE=ok; fi
}
collect_help() {
  local w tok n=0
  run_bounded 10 "$WORKDIR/help.out" "$WORKDIR/help.err" - -- tart --help || true
  note_help; HELP_STATE=$HELP_NOTE; HELP_SUBS=""
  for w in clone list run stop delete ip exec; do
    grep -w -q -- "$w" "$WORKDIR/help.out" 2>/dev/null && HELP_SUBS="$HELP_SUBS $w"
  done
  run_bounded 10 "$WORKDIR/help.out" "$WORKDIR/help.err" - -- tart exec --help || true
  note_help; EXEC_HELP=$HELP_NOTE; EXEC_OPTS=""
  while IFS= read -r w; do
    for tok in $w; do
      case "$tok" in --[A-Za-z]*) tok=${tok%%[!A-Za-z0-9-]*} ;; *) continue ;; esac
      case "$tok" in --[A-Za-z0-9-]*) ;; *) continue ;; esac
      case " $EXEC_OPTS " in *" $tok "*) ;; *) EXEC_OPTS="$EXEC_OPTS $tok"; n=$((n + 1)) ;; esac
      [[ "$n" -ge 12 ]] && break
    done
    [[ "$n" -ge 12 ]] && break
  done <"$WORKDIR/help.out"
}
do_exec() {
  run_bounded 20 "$WORKDIR/exec.out" "$WORKDIR/exec.err" - -- tart exec "$1" /usr/bin/true || true
  if [[ "$RB_TIMED_OUT" == 1 ]]; then EXEC_RESULT=timeout; elif [[ "$RB_STATUS" -eq 0 ]]; then EXEC_RESULT=ok; else EXEC_RESULT=fail; fi
}
begin_stage() {
  S_STAGE=$1; S_NAME=""; S_CREATED=no; S_LAUNCH=not-run; S_COLL=not-run; S_SSH=not-run; S_EXEC=not-run; S_V6=not-run; S_ARGV_OK=0
  : >"$WORKDIR/obs.$1"; printf 'not-run not-run not-run\n' >"$WORKDIR/cleanup.last"
}
run_stage() {
  local stage=$1 name cst cto ep="" lan=$LAN_IP boot guest_ip nobs
  begin_stage "$stage"
  [[ "$stage" == B ]] && ep=$ENDPOINT
  name=$(make_name "$stage") || { add_reason name-failed; ANY_CREATE_FAILED=1; return 1; }
  S_NAME=$name; refresh_list || { ANY_CREATE_FAILED=1; return 1; }
  if vm_status "$name" >/dev/null; then add_reason name-in-use; ANY_CREATE_FAILED=1; return 1; fi
  run_bounded 180 "$WORKDIR/clone.out" "$WORKDIR/clone.err" - -- tart clone "$GOLDEN" "$name" || true
  cst=$RB_STATUS; cto=$RB_TIMED_OUT
  if ! refresh_list; then add_reason clone-unconfirmed; ANY_CREATE_FAILED=1; return 1; fi
  if ! vm_status "$name" >/dev/null; then
    if [[ "$cto" == 1 || "$cst" -ne 0 ]]; then add_reason clone-failed; else add_reason clone-unconfirmed; fi
    ANY_CREATE_FAILED=1; return 1
  fi
  OWN_NAME=$name; OWN_OK=1; S_CREATED=yes
  if [[ "$cto" == 1 || "$cst" -ne 0 ]]; then
    add_reason clone-failed; ANY_COLLECTION_FAILED=1; S_COLL=failed; cleanup_owned; return 1
  fi
  build_argv "$name" "$stage" "$ep"; print_argv
  if ! start_run "$WORKDIR/run.out" "$WORKDIR/run.err" "${RUN_ARGV[@]}"; then
    S_LAUNCH=tool-error; add_reason launch-failed; ANY_COLLECTION_FAILED=1; S_COLL=failed
    do_exec "$name"; S_EXEC=$EXEC_RESULT; cleanup_owned; return 1
  fi
  S_LAUNCH=started; do_exec "$name"; S_EXEC=$EXEC_RESULT
  boot=$((SECONDS + $(cap_secs 240))); S_SSH=no-address
  while [[ "$SECONDS" -lt "$boot" && "$REQUEST_CLEANUP" != 1 ]]; do
    run_bounded 10 "$WORKDIR/ip.out" "$WORKDIR/ip.err" - -- tart ip "$name" || true
    [[ "$RB_TIMED_OUT" == 1 ]] && { S_SSH=timeout; break; }
    guest_ip=$(awk '{for(i=1;i<=NF;i++) if($i ~ /^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$/){print $i; exit}}' "$WORKDIR/ip.out")
    is_ipv4 "$guest_ip" && { S_SSH=pending; break; }
    [[ "$RB_STATUS" -ne 0 ]] && { S_SSH=tool-error; break; }
    S_SSH=no-address; sleep 1
  done
  if [[ "$S_SSH" == pending && "$REQUEST_CLEANUP" != 1 ]]; then
    ssh_guest "$guest_ip" "$ep" "$lan"
    take_obs "$WORKDIR/ssh.out" "$WORKDIR/obs.$stage"
    nobs=$(wc -l <"$WORKDIR/obs.$stage" | tr -d ' ')
    if [[ "$RB_TIMED_OUT" == 1 ]]; then S_SSH=timeout; S_COLL=failed
    elif [[ "$RB_STATUS" -eq 0 && "${nobs:-0}" -gt 0 ]]; then S_SSH=connected; S_COLL=ok
    else S_COLL=failed; if [[ "${nobs:-0}" -gt 0 ]]; then S_SSH=connected; else S_SSH=tool-error; fi; fi
  else S_COLL=failed; fi
  [[ "$S_COLL" == ok ]] || { ANY_COLLECTION_FAILED=1; add_reason collection-failed; }
  S_V6=$(awk '$1=="ipv6-global-address"{print $2; exit}' "$WORKDIR/obs.$stage" 2>/dev/null || true)
  [[ -n "$S_V6" ]] || S_V6=not-run
  cleanup_owned
}
json_list() {
  local first=1 w
  printf '['
  for w in "$@"; do
    case "$w" in ""|*[!A-Za-z0-9_./=-]*) continue ;; esac
    [[ "$first" -eq 1 ]] && first=0 || printf ','
    printf '"%s"' "$w"
  done
  printf ']'
}
save_stage() {
  [[ -n "$S_STAGE" ]] || return 0
  local cu stop del abs id result sec first=1
  cu=$(cat "$WORKDIR/cleanup.last" 2>/dev/null || echo "not-run not-run not-run")
  set -- $cu; stop=${1:-not-run}; del=${2:-not-run}; abs=${3:-not-run}
  {
    printf '{"stage":"%s","clone":"%s","created":"%s","argv":' "$S_STAGE" "$S_NAME" "$S_CREATED"
    if [[ "$S_ARGV_OK" == 1 ]]; then json_list "${RUN_ARGV[@]}"; else printf '[]'; fi
    printf ',"launch":"%s","collection":"%s","ssh":"%s","tart_exec":"%s","ipv6_global":"%s","observations":[' \
      "$S_LAUNCH" "$S_COLL" "$S_SSH" "$S_EXEC" "$S_V6"
    if [[ -f "$WORKDIR/obs.$S_STAGE" ]]; then
      while read -r id result sec; do
        [[ -n "$id" ]] || continue
        [[ "$first" -eq 1 ]] && first=0 || printf ','
        printf '{"id":"%s","result":"%s","elapsed_sec":%s}' "$id" "$result" "${sec:-0}"
      done <"$WORKDIR/obs.$S_STAGE"
    fi
    printf '],"cleanup":{"stop":"%s","delete":"%s","absent":"%s"}}\n' "$stop" "$del" "$abs"
  } >"$WORKDIR/stage-$S_STAGE.json"
}
write_report() {
  local tmp=$OUT/report.json.tmp rs="" r first=1 st f
  mkdir -p "$OUT" 2>/dev/null || return 1
  [[ -f "$REASONS_FILE" ]] && while IFS= read -r r; do rs="$rs $r"; done <"$REASONS_FILE"
  {
    printf '{"schema":"s11-0-probe/v2","status":"%s","endpoint_source":"%s","h1_independent_of_ip":"unknown",' "$STATUS" "$ENDPOINT_SOURCE"
    printf '"probe_budget_sec":%s,"cleanup_budget_sec":%s,"elapsed_sec":%s,' "$PROBE_BUDGET" "$CLEANUP_BUDGET" "$SECONDS"
    printf '"measured":%s,"unmeasured":%s,"reasons":%s,' "$(json_list $MEASURED)" "$(json_list $UNMEASURED)" "$(json_list $rs)"
    printf '"tart_help":"%s","tart_help_subcommands":%s,"tart_exec_help":"%s","tart_exec_options":%s,"stages":[' \
      "$HELP_STATE" "$(json_list $HELP_SUBS)" "$EXEC_HELP" "$(json_list $EXEC_OPTS)"
    for st in A B C; do
      f=$WORKDIR/stage-$st.json; [[ -f "$f" ]] || continue
      [[ "$first" -eq 1 ]] && first=0 || printf ','; cat "$f"
    done
    printf ']}\n'
  } >"$tmp" && mv "$tmp" "$OUT/report.json"
}
set_coverage() {
  local base="udp-443 quic ipv6-gateway ipv6-ula ipv6-link-local ipv6-udp dot-853 doh tcp-53-public"
  local net="tcp-gateway-22 tcp-gateway-53 tcp-gateway-80 tcp-gateway-443 dns-udp-gateway dns-udp-public tcp-metadata-80 ipv6-global-address tcp-ipv6-public tart-exec"
  case "$STAGE" in
    A) MEASURED=$net; UNMEASURED="$base tcp-endpoint-443" ;;
    B|all) MEASURED="$net tcp-endpoint-443"; UNMEASURED=$base ;;
    *) MEASURED="tart-help tart-exec-help"; UNMEASURED="$base $net tcp-endpoint-443 lan"; return ;;
  esac
  if [[ -n "$LAN_IP" ]]; then MEASURED="$MEASURED tcp-lan-443"; else UNMEASURED="$UNMEASURED lan"; fi
}
need_arg() { [[ $# -ge 2 ]] || { USAGE_REASON=usage; return 1; }; }
parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --stage) need_arg "$@" || return; case "$2" in A|B|C|all) STAGE=$2 ;; *) USAGE_REASON=usage; return ;; esac; shift 2 ;;
      --out) need_arg "$@" || return; OUT=$2; shift 2 ;;
      --tart-home) need_arg "$@" || return; TART_HOME_ARG=$2; shift 2 ;;
      --ssh-key) need_arg "$@" || return; SSH_KEY=$2; shift 2 ;;
      --endpoint-ip) need_arg "$@" || return; ENDPOINT=$2; shift 2 ;;
      --endpoint-placeholder) ENDPOINT_SOURCE=placeholder; shift ;;
      --lan-ip) need_arg "$@" || return; LAN_IP=$2; shift 2 ;;
      --dry-run) DRY=1; shift ;;
      -h|--help) USAGE_REASON=help; return ;;
      *) USAGE_REASON=usage; return ;;
    esac
  done
  [[ -n "$ENDPOINT" && "$ENDPOINT_SOURCE" == placeholder ]] && { USAGE_REASON=usage; ENDPOINT=""; ENDPOINT_SOURCE=absent; return; }
  if [[ -n "$ENDPOINT" ]]; then is_public_ip "$ENDPOINT" && ENDPOINT_SOURCE=argument || { USAGE_REASON=endpoint-invalid; ENDPOINT=""; return; }; fi
  [[ "$ENDPOINT_SOURCE" == placeholder ]] && ENDPOINT=$PLACEHOLDER_IP
  case "$STAGE" in B|all) [[ -n "$ENDPOINT" ]] || USAGE_REASON=endpoint-required ;; esac
  if [[ -n "$LAN_IP" ]] && ! is_ipv4 "$LAN_IP"; then USAGE_REASON=usage; LAN_IP=""; fi
  [[ -n "${S11_FORCE_NAME:-}" ]] && { name_is_safe "$S11_FORCE_NAME" || USAGE_REASON=usage; }
}
precheck() {
  local gst
  [[ "$(id -u)" == 0 ]] && { add_reason root; return 1; }
  check_softnet || return 1
  [[ -f "$SSH_KEY" && -r "$SSH_KEY" ]] || { add_reason ssh-key-unreadable; return 1; }
  if [[ -n "$TART_HOME_ARG" ]]; then [[ -d "$TART_HOME_ARG" ]] || { add_reason tart-home-missing; return 1; }; export TART_HOME="$TART_HOME_ARG"; fi
  command -v tart >/dev/null 2>&1 || { add_reason tart-missing; return 1; }
  run_bounded 10 "$WORKDIR/ver.out" "$WORKDIR/ver.err" - -- tart --version || true
  [[ "$RB_TIMED_OUT" == 1 ]] && { add_reason tart-timeout; return 1; }
  [[ "$RB_STATUS" -eq 0 ]] || { add_reason tart-missing; return 1; }
  refresh_list || return 1
  gst=$(vm_status "$GOLDEN") || { add_reason golden-missing; return 1; }
  case "$gst" in
    stopped|Stopped|STOPPED) ;;
    running|Running|RUNNING) add_reason golden-not-stopped; return 1 ;;
    *) add_reason golden-unknown; return 1 ;;
  esac
}
decide() {
  if [[ "$DRY" == 1 ]]; then STATUS=not-run; FINAL_EC=0; return; fi
  if [[ "$PRECHECK_FAILED" == 1 ]]; then STATUS=precheck-failed; FINAL_EC=2; return; fi
  if [[ "$ANY_COLLECTION_FAILED" == 1 ]]; then STATUS=partial; FINAL_EC=4; return; fi
  if [[ "$ANY_CLEANUP_FAILED" == 1 ]]; then STATUS=partial; FINAL_EC=5; return; fi
  if [[ "$ANY_CREATE_FAILED" == 1 ]]; then STATUS=partial; FINAL_EC=3; return; fi
  STATUS=complete; FINAL_EC=0
}
on_exit() {
  trap '' INT TERM HUP EXIT
  FINALIZING=1; cleanup_owned || true; decide
  write_report || { say report-write-failed; [[ "$FINAL_EC" -eq 0 ]] && FINAL_EC=2; }
  printf 'status=%s\nreport=%s/report.json\n' "$STATUS" "$OUT"
  case "$WORKDIR" in /tmp/s11-0.*|/private/tmp/s11-0.*|/var/folders/*|*/s11-0.*) [[ ! -L "$WORKDIR" && -d "$WORKDIR" ]] && rm -rf "$WORKDIR" ;; esac
  exit "$FINAL_EC"
}
dry_stages() {
  local st nm ep
  for st in A B; do
    case "$STAGE" in all|"$st") ;; *) continue ;; esac
    nm=$(make_name "$st") || nm="s11-0-probe-${st}-dry-00000000"
    ep=""; [[ "$st" == B ]] && ep=$ENDPOINT
    begin_stage "$st"; S_NAME=$nm; build_argv "$nm" "$st" "$ep"; print_argv; save_stage
  done
}
main() {
  parse_args "$@"
  case "${S11_PROBE_BUDGET:-}" in ""|*[!0-9]*) ;; *) [[ "$S11_PROBE_BUDGET" -gt 0 && "$S11_PROBE_BUDGET" -lt "$PROBE_BUDGET" ]] && PROBE_BUDGET=$S11_PROBE_BUDGET ;; esac
  case "${S11_CLEANUP_BUDGET:-}" in ""|*[!0-9]*) ;; *) [[ "$S11_CLEANUP_BUDGET" -gt 0 && "$S11_CLEANUP_BUDGET" -lt "$CLEANUP_BUDGET" ]] && CLEANUP_BUDGET=$S11_CLEANUP_BUDGET ;; esac
  PROBE_DEADLINE=$((SECONDS + PROBE_BUDGET))
  WORKDIR=$(mktemp -d "${TMPDIR:-/tmp}/s11-0.XXXXXX") || { say report-dir; exit 2; }
  REASONS_FILE=$WORKDIR/reasons; : >"$REASONS_FILE"
  printf 'not-run not-run not-run\n' >"$WORKDIR/cleanup.last"
  trap on_exit EXIT
  trap 'REQUEST_CLEANUP=1' INT TERM HUP
  set_coverage
  if [[ -n "$USAGE_REASON" ]]; then
    [[ "$USAGE_REASON" == help ]] && say 'usage: s11-0-probe.sh [--dry-run] [--stage A|B|C|all] [--endpoint-ip A.B.C.D|--endpoint-placeholder] [--lan-ip A.B.C.D] [--ssh-key FILE] [--tart-home DIR] [--out DIR]'
    add_reason "$USAGE_REASON"; PRECHECK_FAILED=1; exit 2
  fi
  mkdir -p "$OUT" || { add_reason report-dir; PRECHECK_FAILED=1; exit 2; }
  precheck || { PRECHECK_FAILED=1; exit 2; }
  write_guest
  if [[ "$DRY" == 1 ]]; then dry_stages; exit 0; fi
  collect_help
  case "$STAGE" in A|all) run_stage A || true; save_stage ;; esac
  if [[ "$REQUEST_CLEANUP" != 1 ]]; then case "$STAGE" in B|all) run_stage B || true; save_stage ;; esac; fi
  exit 0
}
main "$@"

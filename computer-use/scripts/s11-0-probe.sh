#!/bin/bash
# S11-0 fact probe, ADR 0018 §10. Bash 3.2.
# Own a clone only when tart clone exits 0 and a trusted tart list then shows that name.
# Header-driven inventory: Name and State. Offsets, not whitespace (Disk Size contains spaces).
# Header labels that pass the whitelist are reported as inventory_columns, even if State is absent.
# No SIGKILL handler: a killed run may leave no report. Output directory must not already exist.
# Signal a background job only while the shell job table still lists it. h1_independent_of_ip=unknown.
set -u; set -f; umask 077; export LC_ALL=C
GOLDEN=macos-s2-eval; GUEST_USER=eval; PLACEHOLDER_IP=203.0.113.10
PROBE_BUDGET=1500; CLEANUP_BUDGET=90; CAP=65536; GRACE=1; START_CONFIRM=1
FINAL_EC=1; STATUS=partial; DRY=0; STAGE=all; OUT=./s11-0-out; TART_HOME_ARG=""
SSH_KEY="${HOME}/.ssh/id_ed25519_tart"; ENDPOINT=""; ENDPOINT_SOURCE=absent; LAN_IP=""
USAGE_REASON=""; PRECHECK_FAILED=0; ANY_COLLECTION_FAILED=0; ANY_CLEANUP_FAILED=0; ANY_CREATE_FAILED=0
ANY_AMBIGUOUS=0; OUT_READY=0; DIG_MISSING=0
REQUEST_CLEANUP=0; IN_CLEANUP=0; FINALIZING=0; PROBE_DEADLINE=0; CLEANUP_LEFT=90; WORKDIR=""
REASONS_FILE=""; OWN_NAME=""; OWN_OK=0; RUN_PID=""; RUN_LIVE=0
RB_STATUS=0; RB_TIMED_OUT=0; RB_ABORTED=0; RB_OVERFLOW=0; REAP_STATUS=0
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
charge() {
  [[ "$IN_CLEANUP" == 1 ]] || return 0
  CLEANUP_LEFT=$((CLEANUP_LEFT - $1))
  [[ "$CLEANUP_LEFT" -lt 0 ]] && CLEANUP_LEFT=0
}
# jobs must run in this shell. A pipeline or $(jobs) is a subshell and sees no jobs.
job_running() {
  local pid="${1:-}" line
  [[ -n "$pid" ]] || return 1
  jobs -r -p >"$WORKDIR/jobs.p" 2>/dev/null || true
  while IFS= read -r line; do
    [[ "$line" == "$pid" ]] && return 0
  done <"$WORKDIR/jobs.p"
  return 1
}
term_kill_reap() {
  local pid="$1" i=0
  [[ -n "$pid" ]] || return 0
  if job_running "$pid"; then
    kill -TERM "$pid" 2>/dev/null || true
    # GRACE stays reserved in the cleanup budget. This poll is the whole grace.
    while [[ "$i" -lt 8 ]]; do
      job_running "$pid" || break
      sleep 0.05
      i=$((i + 1))
    done
    if job_running "$pid"; then kill -KILL "$pid" 2>/dev/null || true; fi
  fi
  # Dead job: wait returns at once. Live job: SIGKILL was just sent, so wait returns.
  wait "$pid" 2>/dev/null
  REAP_STATUS=$?
}
capture_over() {
  local n
  # One wc for both files. Arithmetic accepts wc's leading spaces.
  for n in $(wc -c <"$1" 2>/dev/null; wc -c <"$2" 2>/dev/null); do
    [[ $((n)) -ge "$CAP" ]] && return 0
  done
  return 1
}
run_bounded() {
  local secs="$1" out="$2" err="$3" infile="$4" t0 pid reaped=0 left polls=0 pb=""
  shift 4
  [[ "${1:-}" == -- ]] && shift
  secs=$(cap_secs "$secs"); : >"$out"; : >"$err"
  case "${S11_BOUND_POLLS:-}" in ""|*[!0-9]*) ;; *) pb=$S11_BOUND_POLLS ;; esac
  [[ "$infile" == - ]] && infile=/dev/null
  RB_TIMED_OUT=0; RB_ABORTED=0; RB_OVERFLOW=0; RB_STATUS=0; REAP_STATUS=0
  t0=$SECONDS
  if [[ "$IN_CLEANUP" == 1 ]]; then
    if [[ "$CLEANUP_LEFT" -le "$GRACE" ]]; then RB_TIMED_OUT=1; RB_STATUS=124; charge 0; return 124; fi
    left=$((CLEANUP_LEFT - GRACE))
  else
    if [[ "$SECONDS" -ge "$PROBE_DEADLINE" ]]; then
      RB_TIMED_OUT=1; RB_STATUS=124; REQUEST_CLEANUP=1; return 124
    fi
    left=$((PROBE_DEADLINE - SECONDS))
  fi
  [[ "$secs" -gt "$left" ]] && secs=$left
  if [[ "$secs" -lt 1 ]]; then RB_TIMED_OUT=1; RB_STATUS=124; charge $((SECONDS - t0)); return 124; fi
  # Direct child only. Descendants inherit the files; nothing waits on them.
  # ulimit -f on bash 3.2/macOS is 512-byte blocks: 64KiB = 128.
  (
    ulimit -c 0 2>/dev/null || true
    ulimit -f $((CAP / 512)) || exit 127
    exec "$@" <"$infile" >"$out" 2>"$err"
  ) &
  pid=$!
  while :; do
    if ! job_running "$pid"; then break; fi
    polls=$((polls + 1))
    if [[ -n "$pb" && "$polls" -ge "$pb" || "$SECONDS" -ge $((t0 + secs)) ]]; then
      RB_TIMED_OUT=1; term_kill_reap "$pid"; reaped=1; break
    fi
    if [[ "$IN_CLEANUP" != 1 && "$REQUEST_CLEANUP" == 1 ]]; then
      RB_ABORTED=1; term_kill_reap "$pid"; reaped=1; break
    fi
    if capture_over "$out" "$err"; then RB_OVERFLOW=1; term_kill_reap "$pid"; reaped=1; break; fi
    sleep 0.02
  done
  if [[ "$reaped" != 1 ]]; then wait "$pid" 2>/dev/null; REAP_STATUS=$?; fi
  # 153 = 128+SIGXFSZ. A full file is overflow even when the unit of ulimit -f differs.
  [[ "$REAP_STATUS" -eq 153 ]] && RB_OVERFLOW=1
  capture_over "$out" "$err" && RB_OVERFLOW=1
  if [[ "$RB_OVERFLOW" == 1 ]]; then RB_STATUS=125
  elif [[ "$RB_TIMED_OUT" == 1 ]]; then RB_STATUS=124
  elif [[ "$RB_ABORTED" == 1 ]]; then RB_STATUS=130
  else RB_STATUS=$REAP_STATUS; fi
  charge $((SECONDS - t0))
  return "$RB_STATUS"
}
start_run() {
  local pid t0 confirm
  "$@" </dev/null >/dev/null 2>&1 &
  pid=$!; RUN_PID=$pid; RUN_LIVE=1; confirm=$(cap_secs "$START_CONFIRM"); t0=$SECONDS
  case "${S11_START_SEC:-}" in ""|*[!0-9]*) ;; *) [[ "$S11_START_SEC" -lt "$confirm" ]] && confirm=$S11_START_SEC ;; esac
  while :; do
    if ! job_running "$pid"; then
      RUN_LIVE=0; RUN_PID=""
      wait "$pid" 2>/dev/null || true
      return 1
    fi
    if [[ "$SECONDS" -ge $((t0 + confirm)) ]]; then
      # A zero confirm still waits one beat, so an immediate exit is not "started".
      [[ "$confirm" -eq 0 ]] && sleep 0.1
      if job_running "$pid"; then return 0; fi
      RUN_LIVE=0; RUN_PID=""
      wait "$pid" 2>/dev/null || true
      return 1
    fi
    sleep 0.05
  done
}
stop_run_proc() {
  [[ "$RUN_LIVE" == 1 && -n "$RUN_PID" ]] || return 0
  local pid=$RUN_PID t0=$SECONDS
  term_kill_reap "$pid"
  RUN_LIVE=0; RUN_PID=""
  charge $((SECONDS - t0))
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
  [[ "$RB_OVERFLOW" == 1 ]] && { add_reason capture-overflow; add_reason softnet-stat-failed; return 1; }
  [[ "$RB_TIMED_OUT" == 1 || "$RB_STATUS" -ne 0 ]] && { add_reason softnet-stat-failed; return 1; }
  read -r owner mode <"$WORKDIR/stat.out" || { add_reason softnet-stat-failed; return 1; }
  case "$mode" in ??????????) ;; *) add_reason softnet-stat-failed; return 1 ;; esac
  bit=$(printf '%s' "$mode" | cut -c 4); [[ "$owner" == root ]] || { add_reason softnet-not-root; bad=1; }
  case "$bit" in s|S) ;; *) add_reason softnet-not-suid; bad=1 ;; esac; [[ "$bad" -eq 0 ]]
}
parse_list() {
  local tmp=$WORKDIR/names.tmp cols=$WORKDIR/list.cols
  rm -f "$tmp" "$cols"
  awk -v cols="$cols" '
    function ncols(line, s, nm,   i,len,c,run,name,st,n) {
      n=0; len=length(line); i=1
      while (i<=len && substr(line,i,1)==" ") i++
      while (i<=len) {
        st=i; name=""
        while (i<=len) {
          c=substr(line,i,1)
          if (c==" ") {
            run=0
            while (i+run<=len && substr(line,i+run,1)==" ") run++
            if (run>=2 || i+run>len) break
            name=name " "; i++; continue
          }
          name=name c; i++
        }
        sub(/ +$/, "", name)
        if (name != "") { n++; s[n]=st; nm[n]=name }
        while (i<=len && substr(line,i,1)==" ") i++
      }
      return n
    }
    function field(line, s, k, n,   a,b,t) {
      a=s[k]; if (k<n) b=s[k+1]-1; else b=length(line)
      if (a>length(line)) return ""
      if (b>length(line)) b=length(line)
      if (b<a) return ""
      t=substr(line,a,b-a+1); gsub(/^[ ]+|[ ]+$/, "", t); return t
    }
    BEGIN { hdr=0 }
    {
      line=$0
      if (line ~ /^[ ]*$/) next
      if (!hdr) {
        n=ncols(line, starts, names); ni=0; si=0
        for (i=1;i<=n;i++) {
          if (names[i]=="Name") ni=i
          if (names[i]=="State") si=i
        }
        for (i=1;i<=n;i++) if (names[i] ~ /^[A-Za-z][A-Za-z0-9 -]*$/ && length(names[i])<=40) print names[i] > cols
        if (ni==0 || si==0) exit 2
        hdr=1; next
      }
      if (length(line) < starts[si]) exit 3
      name=field(line, starts, ni, n); stt=field(line, starts, si, n)
      if (name=="" || stt=="" || index(name, "\t")>0) exit 3
      printf "%s\t%s\n", name, stt
    }
    END { if (!hdr) exit 2 }
  ' "$WORKDIR/list.last" >"$tmp" || { rm -f "$tmp" "$WORKDIR/names.tab"; return 1; }
  mv "$tmp" "$WORKDIR/names.tab"
}
refresh_list() {
  run_bounded 10 "$WORKDIR/list.out" "$WORKDIR/list.err" - -- tart list || true
  rm -f "$WORKDIR/names.tab"
  if [[ "$RB_OVERFLOW" == 1 ]]; then add_reason capture-overflow; add_reason list-untrusted; return 1; fi
  if [[ "$RB_TIMED_OUT" == 1 ]]; then add_reason list-timeout; return 1; fi
  if [[ "$RB_ABORTED" == 1 ]]; then add_reason list-aborted; return 1; fi
  if [[ "$RB_STATUS" -ne 0 ]]; then add_reason list-failed; return 1; fi
  cp "$WORKDIR/list.out" "$WORKDIR/list.last" || { add_reason list-failed; return 1; }
  parse_list || { add_reason list-untrusted; return 1; }
}
vm_status() {
  [[ -f "$WORKDIR/names.tab" ]] || return 1
  awk -F '\t' -v n="$1" '$1==n {print $2; found=1; exit} END{exit !found}' "$WORKDIR/names.tab"
}
build_argv() {
  RUN_ARGV=(tart run "$1" --no-graphics --net-softnet); S_ARGV_OK=1
  [[ "$2" == B ]] && RUN_ARGV+=(--net-softnet-block=0.0.0.0/0 "--net-softnet-allow=${3}/32")
}
print_argv() { local x line=argv:; for x in "${RUN_ARGV[@]}"; do line="$line $x"; done; printf '%s\n' "$line"; }
write_guest() {
  cat >"$WORKDIR/guest.sh" <<'EOF'
set -u; set -o pipefail; export LC_ALL=C
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
  elif [[ "$2" == dns ]]; then command -v dig >/dev/null 2>&1 || { rm -f "$ef"; printf 'note dig-missing\n'; obs "$1" tool-error 0; return; }
    dig -r +notcp +ignore +time=2 +tries=1 @"$3" example.com A >"$ef" 2>&1; st=$?
  else rm -f "$ef"; obs "$1" tool-error 0; return; fi
  if [[ "$st" -eq 0 ]]; then obs "$1" connected $((SECONDS - t))
  elif [[ "$2" == dns && "$st" -eq 9 ]]; then obs "$1" timeout $((SECONDS - t))
  else obs "$1" "$(classify "$st" "$ef")" $((SECONDS - t)); fi; rm -f "$ef"; }
gw=""; gerr=0
if ! command -v route >/dev/null 2>&1; then gerr=1
else gw=$(route -n get default 2>/dev/null | awk '$1=="gateway:"{print $2; exit}') || gerr=1; fi
if [[ "$gerr" -ne 0 ]]; then
  obs gw22 tool-error 0; obs gw53 tool-error 0; obs gw80 tool-error 0; obs gw443 tool-error 0; obs dns-gw tool-error 0
else
  case "$gw" in [0-9]*.[0-9]*.[0-9]*.[0-9]*) ;; *) gw="";; esac
  if [[ -z "$gw" ]]; then obs gw22 no-address 0; obs gw53 no-address 0; obs gw80 no-address 0; obs gw443 no-address 0; obs dns-gw no-address 0
  else run_tool gw22 tcp "$gw" 22; run_tool gw53 tcp "$gw" 53; run_tool gw80 tcp "$gw" 80; run_tool gw443 tcp "$gw" 443; run_tool dns-gw dns "$gw"; fi
fi
if ! command -v dig >/dev/null 2>&1; then printf 'note dig-missing\n'; obs dns-pub tool-error 0; else run_tool dns-pub dns 1.1.1.1; fi
run_tool meta80 tcp 169.254.169.254 80
if ! command -v ifconfig >/dev/null 2>&1; then obs v6addr tool-error 0
else
  v6=$(ifconfig 2>/dev/null | awk 'BEGIN{f="no"} $1=="inet6"{a=$2; sub(/%.*/,"",a); if(a~/^fe80:/||a=="::1"||a~/^fc/||a~/^fd/||a~/^ff/) next; if(a~/:/) f="yes"} END{print f}') || v6=""
  if [[ -z "$v6" ]]; then obs v6addr tool-error 0
  elif [[ "$v6" == yes ]]; then obs v6addr yes 0; else obs v6addr no-address 0; fi
fi
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
  grep -Fqx 'note dig-missing' "$src" 2>/dev/null && DIG_MISSING=1
  while IFS= read -r line; do
    set -- $line; tag=${1:-}; role=${2:-}; result=${3:-}; sec=${4:-}
    [[ "$tag" == obs ]] || continue
    case "$result" in connected|refused|no-route|no-address|timeout|tool-error|yes) ;; *) continue ;; esac
    [[ "$result" == yes && "$role" != v6addr ]] && continue
    id=$(role_id "$role") || continue; case "$sec" in ""|*[!0-9]*) sec=0 ;; esac
    printf '%s %s %s\n' "$id" "$result" "$sec" >>"$dst"
  done <"$src"
}
expect_roles() {
  local st=$1 f
  f=$WORKDIR/expect.$st
  : >"$f"
  printf '%s\n' tcp-gateway-22 tcp-gateway-53 tcp-gateway-80 tcp-gateway-443 dns-udp-gateway dns-udp-public \
    tcp-metadata-80 ipv6-global-address tcp-ipv6-public >>"$f"
  [[ -n "$LAN_IP" ]] && printf '%s\n' tcp-lan-443 >>"$f"
  [[ "$st" == B ]] && printf '%s\n' tcp-endpoint-443 >>"$f"
}
roles_unique() {
  awk 'NR==FNR { e[$1]=1; next } $1!="" { c[$1]++ } END {
    for (k in e) if (c[k] != 1) exit 1
    for (k in c) if (!(k in e)) exit 1
  }' "$1" "$2"
}
ssh_run() {
  local secs=$1 infile=$2 ip=$3 remote=$4 kh=$WORKDIR/known_hosts
  : >"$kh"
  run_bounded "$secs" "$WORKDIR/ssh.out" "$WORKDIR/ssh.err" "$infile" -- \
    ssh -o BatchMode=yes -o ConnectTimeout=8 -o ServerAliveInterval=5 -o ServerAliveCountMax=2 \
    -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new -o "UserKnownHostsFile=${kh}" \
    -i "$SSH_KEY" "${GUEST_USER}@${ip}" "$remote" || true
}
try_exec() {
  [[ "${1:-}" == 1 ]] && return 0
  run_bounded 20 "$WORKDIR/exec.out" "$WORKDIR/exec.err" - -- tart exec "$2" /usr/bin/true || true
  if [[ "$RB_TIMED_OUT" == 1 ]]; then EXEC_RESULT=timeout; return 0
  elif [[ "$RB_OVERFLOW" == 1 ]]; then EXEC_RESULT=tool-error; return 0
  elif [[ "$RB_STATUS" -eq 0 ]]; then EXEC_RESULT=ok; return 0
  else EXEC_RESULT=fail; return 1; fi
}
boot_wait() {
  local name=$1 ep=$2 lan=$3 do_net=$4 exec_done=0 guest_ip="" boot_for left boot_until exec_until
  boot_for=$(cap_secs 240)
  if [[ "$SECONDS" -lt "$PROBE_DEADLINE" ]]; then
    left=$((PROBE_DEADLINE - SECONDS)); [[ "$boot_for" -gt "$left" ]] && boot_for=$left
  fi
  [[ "$boot_for" -lt 1 ]] && boot_for=1
  boot_until=$((SECONDS + boot_for)); exec_until=$((SECONDS + $(cap_secs 30)))
  [[ "$do_net" == 1 ]] && S_SSH=no-address
  while [[ "$SECONDS" -lt "$boot_until" && "$REQUEST_CLEANUP" != 1 ]]; do
    try_exec "$exec_done" "$name" && exec_done=1
    if [[ "$do_net" == 1 && "$S_SSH" != pending && "$S_SSH" != connected ]]; then
      run_bounded 10 "$WORKDIR/ip.out" "$WORKDIR/ip.err" - -- tart ip "$name" || true
      if [[ "$RB_TIMED_OUT" == 1 || "$RB_OVERFLOW" == 1 ]]; then S_SSH=timeout; break; fi
      if [[ "$RB_STATUS" -eq 127 ]]; then S_SSH=tool-error; break; fi
      guest_ip=$(awk '{for(i=1;i<=NF;i++) if($i ~ /^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$/){print $i; exit}}' "$WORKDIR/ip.out")
      if is_ipv4 "$guest_ip"; then S_SSH=pending; else S_SSH=no-address; fi
    fi
    if [[ "$do_net" == 1 && "$S_SSH" == pending ]]; then
      ssh_run 20 /dev/null "$guest_ip" /usr/bin/true
      if [[ "$RB_OVERFLOW" == 1 ]]; then S_SSH=timeout; break; fi
      if [[ "$RB_STATUS" -eq 0 ]]; then S_SSH=connected
      elif [[ "$RB_TIMED_OUT" == 1 ]]; then S_SSH=pending
      else S_SSH=pending; fi
    fi
    if [[ "$do_net" != 1 && "$exec_done" == 1 ]]; then break; fi
    if [[ "$do_net" == 1 && "$S_SSH" == connected && "$exec_done" == 1 ]]; then break; fi
    if [[ "$do_net" == 1 && "$S_SSH" == connected && "$SECONDS" -ge "$exec_until" ]]; then break; fi
    [[ "$SECONDS" -ge "$boot_until" ]] && break
    sleep 0.2
  done
  S_EXEC=$EXEC_RESULT
  if [[ "$do_net" != 1 ]]; then return 0; fi
  if [[ "$S_SSH" != connected ]]; then
    [[ "$S_SSH" == pending || "$S_SSH" == no-address ]] && S_SSH=timeout
    S_COLL=failed; return 1
  fi
  [[ "$REQUEST_CLEANUP" == 1 ]] && { S_COLL=failed; S_SSH=tool-error; return 1; }
  ssh_run 50 "$WORKDIR/guest.sh" "$guest_ip" "S11_EP=${ep} S11_LAN=${lan} bash -c 'f=\$(mktemp \"\${TMPDIR:-/tmp}/s11g.XXXXXX\") || exit 127; cat >\"\$f\" || exit 127; bash \"\$f\"; st=\$?; rm -f \"\$f\"; exit \$st'"
  take_obs "$WORKDIR/ssh.out" "$WORKDIR/obs.$S_STAGE"
  expect_roles "$S_STAGE"
  if [[ "$RB_TIMED_OUT" == 1 || "$RB_OVERFLOW" == 1 ]]; then S_SSH=timeout; S_COLL=failed; return 1; fi
  if [[ "$RB_STATUS" -eq 0 ]] && roles_unique "$WORKDIR/expect.$S_STAGE" "$WORKDIR/obs.$S_STAGE"; then
    S_SSH=connected; S_COLL=ok; return 0
  fi
  S_COLL=failed
  if [[ ! -s "$WORKDIR/obs.$S_STAGE" ]]; then S_SSH=tool-error; fi
  return 1
}
mark_ambiguous() {
  add_reason ambiguous-destination; ANY_AMBIGUOUS=1; S_COLL=failed
  say "ambiguous-destination: ${1:-clone} not confirmed; not deleting; owner must check tart list"
}
cleanup_owned() {
  [[ "$IN_CLEANUP" == 1 ]] && return 0
  IN_CLEANUP=1; trap '' INT TERM HUP
  local stop=not-run del=not-run absent=not-run
  if [[ "$OWN_OK" == 1 ]] && name_is_safe "$OWN_NAME"; then
    run_bounded 30 "$WORKDIR/c.out" "$WORKDIR/c.err" - -- tart stop "$OWN_NAME" || true
    if [[ "$RB_TIMED_OUT" == 1 || "$RB_OVERFLOW" == 1 ]]; then stop=timeout; add_reason stop-timeout; ANY_CLEANUP_FAILED=1
    elif [[ "$RB_STATUS" -ne 0 ]]; then stop=fail; add_reason stop-failed; ANY_CLEANUP_FAILED=1; else stop=ok; fi
    stop_run_proc
    run_bounded 30 "$WORKDIR/c.out" "$WORKDIR/c.err" - -- tart delete "$OWN_NAME" || true
    if [[ "$RB_TIMED_OUT" == 1 || "$RB_OVERFLOW" == 1 ]]; then del=timeout; add_reason delete-timeout; ANY_CLEANUP_FAILED=1
    elif [[ "$RB_STATUS" -ne 0 ]]; then del=fail; add_reason delete-failed; ANY_CLEANUP_FAILED=1; else del=ok; fi
    if refresh_list; then
      if vm_status "$OWN_NAME" >/dev/null; then absent=no; add_reason still-present; ANY_CLEANUP_FAILED=1
      else absent=yes; OWN_OK=0; OWN_NAME=""; fi
    else absent=unknown; add_reason absent-unknown; ANY_CLEANUP_FAILED=1; fi
    printf '%s %s %s\n' "$stop" "$del" "$absent" >"$WORKDIR/cleanup.last"
  else stop_run_proc; fi
  IN_CLEANUP=0
  [[ "$FINALIZING" == 1 ]] || trap 'REQUEST_CLEANUP=1' INT TERM HUP
}
note_help() {
  if [[ "$RB_TIMED_OUT" == 1 || "$RB_OVERFLOW" == 1 ]]; then HELP_NOTE=timeout
  elif [[ "$RB_STATUS" -ne 0 ]]; then HELP_NOTE=tool-error; else HELP_NOTE=ok; fi
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
  if [[ "$STAGE" == C && ( "$HELP_STATE" != ok || "$EXEC_HELP" != ok ) ]]; then ANY_COLLECTION_FAILED=1; fi
}
stage_main() {
  local stage=$1 name cst cto ep="" lan=$LAN_IP
  [[ "$stage" == B ]] && ep=$ENDPOINT
  name=$(make_name "$stage") || { add_reason name-failed; ANY_CREATE_FAILED=1; S_COLL=failed; return 1; }
  S_NAME=$name
  refresh_list || { ANY_CREATE_FAILED=1; S_COLL=failed; return 1; }
  if vm_status "$name" >/dev/null; then add_reason name-in-use; ANY_CREATE_FAILED=1; S_COLL=failed; return 1; fi
  run_bounded 180 "$WORKDIR/clone.out" "$WORKDIR/clone.err" - -- tart clone "$GOLDEN" "$name" || true
  cst=$RB_STATUS; cto=$RB_TIMED_OUT
  if [[ "$RB_OVERFLOW" == 1 || "$cto" == 1 ]]; then mark_ambiguous "clone $name"; return 1; fi
  if [[ "$cst" -ne 0 ]]; then
    if ! refresh_list || vm_status "$name" >/dev/null; then mark_ambiguous "clone $name"; return 1; fi
    add_reason clone-failed; ANY_CREATE_FAILED=1; S_COLL=failed; return 1
  fi
  if ! refresh_list || ! vm_status "$name" >/dev/null; then mark_ambiguous "clone $name"; return 1; fi
  OWN_NAME=$name; OWN_OK=1; S_CREATED=yes
  build_argv "$name" "$stage" "$ep"; print_argv
  if ! start_run "${RUN_ARGV[@]}"; then
    S_LAUNCH=tool-error; add_reason launch-failed; ANY_COLLECTION_FAILED=1; S_COLL=failed
    boot_wait "$name" "$ep" "$lan" 0; return 1
  fi
  S_LAUNCH=started
  boot_wait "$name" "$ep" "$lan" 1 || { ANY_COLLECTION_FAILED=1; add_reason collection-failed; return 1; }
  return 0
}
run_stage() {
  begin_stage "$1"
  stage_main "$1" || true
  printf '%s\n' "$S_COLL" >"$WORKDIR/coll.$1"
  printf '%s\n' "$S_SSH" >"$WORKDIR/ssh.$1"
  S_V6=$(awk '$1=="ipv6-global-address"{print $2; exit}' "$WORKDIR/obs.$1" 2>/dev/null || true)
  [[ -n "$S_V6" ]] || S_V6=not-run
  cleanup_owned
}
begin_stage() {
  S_STAGE=$1; S_NAME=""; S_CREATED=no; S_LAUNCH=not-run; S_COLL=not-run; S_SSH=not-run; S_EXEC=not-run; S_V6=not-run; S_ARGV_OK=0
  EXEC_RESULT=not-run
  : >"$WORKDIR/obs.$1"; printf 'not-run not-run not-run\n' >"$WORKDIR/cleanup.last"
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
json_lines() {
  local first=1 line
  printf '['
  if [[ -f "$1" ]]; then
    while IFS= read -r line; do
      case "$line" in ""|*[!A-Za-z0-9\ -]*) continue ;; esac
      [[ "$first" -eq 1 ]] && first=0 || printf ','
      printf '"%s"' "$line"
    done <"$1"
  fi
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
add_u() { case " $UNMEASURED " in *" $1 "*) ;; *) UNMEASURED="$UNMEASURED $1" ;; esac; }
add_m() { case " $MEASURED " in *" $1 "*) ;; *) MEASURED="$MEASURED $1" ;; esac; }
derive_coverage() {
  local never="udp-443 quic ipv6-gateway ipv6-ula ipv6-link-local ipv6-udp dot-853 doh tcp-53-public"
  local net="tcp-gateway-22 tcp-gateway-53 tcp-gateway-80 tcp-gateway-443 dns-udp-gateway dns-udp-public tcp-metadata-80 ipv6-global-address tcp-ipv6-public"
  local st coll ssh id
  MEASURED=""; UNMEASURED=$never
  if [[ "$DRY" == 1 || "$PRECHECK_FAILED" == 1 ]]; then
    for id in $net tcp-endpoint-443 lan tart-help tart-exec-help; do add_u "$id"; done
    [[ -n "$LAN_IP" ]] && add_u tcp-lan-443
    return
  fi
  for st in A B; do
    [[ -f "$WORKDIR/coll.$st" ]] || continue
    coll=$(cat "$WORKDIR/coll.$st"); ssh=$(cat "$WORKDIR/ssh.$st")
    [[ "$coll" == ok && "$ssh" == connected ]] || continue
    while read -r id _ _; do
      [[ -n "$id" ]] || continue
      if [[ "$DIG_MISSING" == 1 && ( "$id" == dns-udp-gateway || "$id" == dns-udp-public ) ]]; then continue; fi
      add_m "$id"
    done <"$WORKDIR/obs.$st"
  done
  [[ "$HELP_STATE" == ok ]] && add_m tart-help || add_u tart-help
  [[ "$EXEC_HELP" == ok ]] && add_m tart-exec-help || add_u tart-exec-help
  for id in $net tcp-endpoint-443; do case " $MEASURED " in *" $id "*) ;; *) add_u "$id" ;; esac; done
  if [[ -n "$LAN_IP" ]]; then case " $MEASURED " in *\ tcp-lan-443\ *) ;; *) add_u tcp-lan-443 ;; esac
  else add_u lan; fi
}
write_report() {
  [[ "$OUT_READY" == 1 ]] || return 1
  derive_coverage
  local tmp=$OUT/report.json.tmp rs="" r first=1 st f
  [[ -f "$REASONS_FILE" ]] && while IFS= read -r r; do rs="$rs $r"; done <"$REASONS_FILE"
  {
    printf '{"schema":"s11-0-probe/v2","status":"%s","endpoint_source":"%s","h1_independent_of_ip":"unknown",' "$STATUS" "$ENDPOINT_SOURCE"
    printf '"probe_budget_sec":%s,"cleanup_budget_sec":%s,"elapsed_sec":%s,' "$PROBE_BUDGET" "$CLEANUP_BUDGET" "$SECONDS"
    printf '"inventory_columns":%s,' "$(json_lines "$WORKDIR/list.cols")"
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
}
precheck() {
  local gst
  [[ "$(id -u)" == 0 ]] && { add_reason root; return 1; }
  check_softnet || return 1
  [[ -f "$SSH_KEY" && -r "$SSH_KEY" ]] || { add_reason ssh-key-unreadable; return 1; }
  if [[ -n "$TART_HOME_ARG" ]]; then [[ -d "$TART_HOME_ARG" ]] || { add_reason tart-home-missing; return 1; }; export TART_HOME="$TART_HOME_ARG"; fi
  command -v tart >/dev/null 2>&1 || { add_reason tart-missing; return 1; }
  run_bounded 10 "$WORKDIR/ver.out" "$WORKDIR/ver.err" - -- tart --version || true
  [[ "$RB_OVERFLOW" == 1 ]] && { add_reason capture-overflow; add_reason tart-missing; return 1; }
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
  if [[ "$PRECHECK_FAILED" == 1 ]]; then STATUS=precheck-failed; FINAL_EC=2; return; fi
  if [[ "$DRY" == 1 ]]; then STATUS=not-run; FINAL_EC=0; return; fi
  if [[ "$ANY_AMBIGUOUS" == 1 ]]; then STATUS=partial; FINAL_EC=5; return; fi
  if [[ "$ANY_COLLECTION_FAILED" == 1 ]]; then STATUS=partial; FINAL_EC=4; return; fi
  if [[ "$ANY_CLEANUP_FAILED" == 1 ]]; then STATUS=partial; FINAL_EC=5; return; fi
  if [[ "$ANY_CREATE_FAILED" == 1 ]]; then STATUS=partial; FINAL_EC=3; return; fi
  STATUS=complete; FINAL_EC=0
}
on_exit() {
  trap '' INT TERM HUP EXIT
  FINALIZING=1; cleanup_owned || true; decide
  write_report || { [[ "$OUT_READY" == 1 ]] && say report-write-failed; [[ "$FINAL_EC" -eq 0 ]] && FINAL_EC=2; }
  [[ "$OUT_READY" == 1 ]] && printf 'status=%s\nreport=%s/report.json\n' "$STATUS" "$OUT"
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
can_advance() {
  [[ "$REQUEST_CLEANUP" == 1 ]] && return 1
  [[ "$ANY_CLEANUP_FAILED" == 1 ]] && return 1
  [[ "$ANY_AMBIGUOUS" == 1 ]] && return 1
  [[ "$OWN_OK" == 1 ]] && return 1
  return 0
}
prepare_out() {
  if [[ -e "$OUT" || -L "$OUT" ]]; then
    say "output-exists: refusing existing $OUT; a killed run may leave no report, and an old report is not ownership"
    add_reason output-exists; PRECHECK_FAILED=1; OUT_READY=0; return 1
  fi
  mkdir "$OUT" || { add_reason report-dir; PRECHECK_FAILED=1; return 1; }
  OUT_READY=1
}
main() {
  parse_args "$@"
  PROBE_DEADLINE=$((SECONDS + PROBE_BUDGET)); CLEANUP_LEFT=$CLEANUP_BUDGET
  WORKDIR=$(mktemp -d "${TMPDIR:-/tmp}/s11-0.XXXXXX") || { say report-dir; exit 2; }
  REASONS_FILE=$WORKDIR/reasons; : >"$REASONS_FILE"
  printf 'not-run not-run not-run\n' >"$WORKDIR/cleanup.last"
  trap on_exit EXIT
  trap 'REQUEST_CLEANUP=1' INT TERM HUP
  if [[ -n "$USAGE_REASON" ]]; then
    [[ "$USAGE_REASON" == help ]] && say 'usage: s11-0-probe.sh [--dry-run] [--stage A|B|C|all] [--endpoint-ip A.B.C.D|--endpoint-placeholder] [--lan-ip A.B.C.D] [--ssh-key FILE] [--tart-home DIR] [--out DIR]'
    add_reason "$USAGE_REASON"; PRECHECK_FAILED=1
    prepare_out || exit 2
    exit 2
  fi
  prepare_out || exit 2
  precheck || { PRECHECK_FAILED=1; exit 2; }
  write_guest
  if [[ "$DRY" == 1 ]]; then dry_stages; exit 0; fi
  collect_help
  case "$STAGE" in A|all) run_stage A || true; save_stage ;; esac
  if can_advance; then case "$STAGE" in B|all) run_stage B || true; save_stage ;; esac; fi
  exit 0
}
main "$@"

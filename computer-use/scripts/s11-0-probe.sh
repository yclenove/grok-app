#!/bin/bash
# S11-0 fact probe, ADR 0018 §10. Bash 3.2.
# Own a clone only when tart clone exits 0 and a trusted tart list --format json then shows that name.
# One JSON array. Each object has Name and State once. inventory_columns is json plus whitelist keys, never values.
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
HALT=0; REAP_UNKNOWN=0; M_QUARANTINE=0; IO_FAIL=0; M_LOAD_END=0; NO_DELETE=0
M_RECORD=""; SHAPE_OK=0; ROW_N=0; M_OP=""; M_ROLE_REC=""; M_PEER=""; M_LATENCY=""; M_INTEGRITY=""; M_N=""
H1_ST=0; H1_TO=0; H1_AB=0; H1_OV=0; H1_RU=0
H2_ST=0; H2_TO=0; H2_AB=0; H2_OV=0; H2_RU=0
G_ST=0; G_TO=0; G_AB=0; G_OV=0; G_RU=0
S_ARGV_OK=0; HELP_STATE=not-run; HELP_SUBS=""; EXEC_HELP=not-run; EXEC_OPTS=""; EXEC_RESULT=not-run
MEASURED=""; UNMEASURED=""; RUN_ARGV=()
S_STAGE=""; S_NAME=""; S_CREATED=no; S_LAUNCH=not-run; S_COLL=not-run
S_SSH=not-run; S_EXEC=not-run; S_V6=not-run
M_CASE=""; M_PROBE=""; PINNED_KEY=""; EXPORT_DIR=""; ONCE_KEY=""; ONCE_PUB=""; GUEST_IP=""
M_VERDICT=unknown; M_REASON=skipped; EXPORT_STATE=absent; M_BOUNDARY=-
EXPORT_DONE=0; SSH_BG_PID=""; SSH_BG_OV=0; SSH_BG_AB=0
# Fixed tool paths. Overrides apply only in the stub suite (S11_TEST_MODE=1).
HDIUTIL=/usr/bin/hdiutil
DISKUTIL=/usr/sbin/diskutil
PLUTIL=/usr/bin/plutil
MOUNTBIN=/sbin/mount
MDUTIL=/usr/bin/mdutil
TMUTIL=/usr/bin/tmutil
STATBIN=/usr/bin/stat
DFBIN=/bin/df
LSBIN=/bin/ls
TOOL_OVERRIDE=0
if [[ "${S11_TEST_MODE:-}" == 1 ]]; then
  [[ -n "${S11_HDIUTIL:-}" ]] && HDIUTIL=$S11_HDIUTIL
  [[ -n "${S11_DISKUTIL:-}" ]] && DISKUTIL=$S11_DISKUTIL
  [[ -n "${S11_PLUTIL:-}" ]] && PLUTIL=$S11_PLUTIL
  [[ -n "${S11_MOUNT:-}" ]] && MOUNTBIN=$S11_MOUNT
  [[ -n "${S11_MDUTIL:-}" ]] && MDUTIL=$S11_MDUTIL
  [[ -n "${S11_TMUTIL:-}" ]] && TMUTIL=$S11_TMUTIL
  [[ -n "${S11_STAT:-}" ]] && STATBIN=$S11_STAT
  [[ -n "${S11_DF:-}" ]] && DFBIN=$S11_DF
  [[ -n "${S11_LS:-}" ]] && LSBIN=$S11_LS
elif [[ -n "${S11_HDIUTIL:-}${S11_DISKUTIL:-}${S11_PLUTIL:-}${S11_MOUNT:-}${S11_MDUTIL:-}${S11_TMUTIL:-}${S11_STAT:-}${S11_DF:-}${S11_VOLIO:-}${S11_LS:-}" ]]; then
  TOOL_OVERRIDE=1
fi
VOL_PREP_BUDGET=40; HOST_RESERVE=268435456; HOST_MARGIN=1048576; IMAGE_CAP=83886080
# ENOSPC is the volume only when HFS still reports at most this much free.
VOL_FULL_MAX=8388608
PL_CAP_IMAGES=32; PL_CAP_ENTS=8
VOL_A=""; VOL_I=""; VOL_M=""; VOL_DEV=""; VOL_INO=""; VOL_DISK=""
VOL_A_DEV=""; VOL_A_INO=""; VOL_M_DEV=""; VOL_M_INO=""; VOL_OUT_DEV=""; VOL_OUT_INO=""
VOL_PHASE=none; VOL_MAYBE=0; VOL_ID_BROKEN=0; VOL_FORCE=0; VOL_OPT_REJECT=0; VOL_OVER=0
VOL_IN_PREP=0; VOL_PREP_END=0; CLEANUP_DEADLINE=0
VOL_CREATE_SEC=-1; VOL_ATTACH_SEC=-1; VOL_DETACH_SEC=-1
VOL_FSTYPE=unknown; VOL_FSNAME=unknown; VOL_HINT=unknown; VOL_CAPACITY=unknown
VOL_NOSUID=unknown; VOL_NOEXEC=unknown; VOL_NODEV=unknown; VOL_NOBROWSE=unknown
VOL_OWNERS=unknown; VOL_CREATE_WARN=absent
VOL_MDUTIL=unknown; VOL_TMUTIL=unknown
VOL_IMAGE=unknown; VOL_ALLOC=unknown; VOL_HOST_DROP=unknown; VOL_ENOSPC=unknown; VOL_VOLFREE=unknown
VOL_IMAGE_PEAK=unknown; VOL_ALLOC_PEAK=unknown; VOL_HOST_DROP_PEAK=unknown; VOL_SAMPLES=0
VOL_BASE_ALLOC=0; VOL_BASE_HOST=0; VOL_HAVE_BASE=0
VOL_HOST_PRE=unknown; VOL_HOST_FILL=unknown
PL_VAL=""; PL_TYPE=""; PL_COUNT=0; ST_SIZE=0; ST_BLOCKS=0; ST_DEV=0; ST_INO=0
DF_AVAIL=0; DF_TOTAL=0; C9_ENOSPC=""; PROG_SEQ=0

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
# Running and stopped both count: wait on a stopped job blocks.
job_running() {
  local pid="${1:-}" line
  [[ -n "$pid" ]] || return 1
  jobs -r -p >"$WORKDIR/jobs.p" 2>/dev/null || true
  jobs -s -p >>"$WORKDIR/jobs.p" 2>/dev/null || true
  while IFS= read -r line; do
    [[ "$line" == "$pid" ]] && return 0
  done <"$WORKDIR/jobs.p"
  return 1
}
reap_cap() {
  local cap="${S11_REAP_POLLS:-20}"
  case "$cap" in ""|*[!0-9]*) cap=20 ;; esac
  printf '%s\n' "$cap"
}
mark_reap_unknown() {
  REAP_UNKNOWN=1; REAP_STATUS=124; HALT=1
}
cleanup_past() {
  [[ "$IN_CLEANUP" == 1 && "$CLEANUP_DEADLINE" -gt 0 && "$SECONDS" -ge "$CLEANUP_DEADLINE" ]]
}
term_kill_reap() {
  local pid="$1" i=0 cap
  [[ -n "$pid" ]] || return 0
  REAP_UNKNOWN=0
  if job_running "$pid"; then
    kill -TERM "$pid" 2>/dev/null || true
    while [[ "$i" -lt 8 ]]; do
      job_running "$pid" || break
      cleanup_past && { mark_reap_unknown; return 0; }
      sleep 0.05
      i=$((i + 1))
    done
    if job_running "$pid"; then
      cap=$(reap_cap)
      if [[ "$cap" -eq 0 ]]; then disown "$pid" 2>/dev/null || true; mark_reap_unknown; return 0; fi
      cleanup_past && { mark_reap_unknown; return 0; }
      kill -KILL "$pid" 2>/dev/null || true
      i=0
      while [[ "$i" -lt "$cap" ]]; do
        job_running "$pid" || break
        cleanup_past && { mark_reap_unknown; return 0; }
        sleep 0.05
        i=$((i + 1))
      done
      if job_running "$pid"; then disown "$pid" 2>/dev/null || true; mark_reap_unknown; return 0; fi
    fi
  fi
  if job_running "$pid"; then mark_reap_unknown; return 0; fi
  wait "$pid" 2>/dev/null
  REAP_STATUS=$?
}
capture_over() {
  local n rest
  # One wc. Skip its total line so two half-full streams are not an overflow.
  while read -r n rest; do
    [[ "$rest" == total ]] && continue
    case "$n" in ''|*[!0-9]*) continue ;; esac
    [[ "$n" -ge "$CAP" ]] && return 0
  done < <(wc -c "$1" "$2" 2>/dev/null)
  return 1
}
run_bounded() {
  local secs="$1" out="$2" err="$3" infile="$4" t0 pid reaped=0 left polls=0 pb="" hit=0 spins=0
  shift 4
  [[ "${1:-}" == -- ]] && shift
  secs=$(cap_secs "$secs"); : >"$out"; : >"$err"
  case "${S11_BOUND_POLLS:-}" in ""|*[!0-9]*) ;; *) pb=$S11_BOUND_POLLS ;; esac
  [[ "$infile" == - ]] && infile=/dev/null
  RB_TIMED_OUT=0; RB_ABORTED=0; RB_OVERFLOW=0; RB_STATUS=0; REAP_STATUS=0; REAP_UNKNOWN=0
  t0=$SECONDS
  if [[ "$IN_CLEANUP" == 1 ]]; then
    local remain=0 reserve=2
    if [[ "$CLEANUP_DEADLINE" -gt 0 ]]; then remain=$((CLEANUP_DEADLINE - SECONDS)); else remain=$CLEANUP_LEFT; fi
    if [[ "$remain" -le "$reserve" ]]; then RB_TIMED_OUT=1; RB_STATUS=124; charge 0; return 124; fi
    left=$((remain - reserve))
  else
    if [[ "$SECONDS" -ge "$PROBE_DEADLINE" ]]; then
      RB_TIMED_OUT=1; RB_STATUS=124; REQUEST_CLEANUP=1; return 124
    fi
    left=$((PROBE_DEADLINE - SECONDS))
  fi
  [[ "$secs" -gt "$left" ]] && secs=$left
  if [[ "$secs" -lt 1 ]]; then RB_TIMED_OUT=1; RB_STATUS=124; charge $((SECONDS - t0)); return 124; fi
  # Direct child only. Descendants inherit the files; nothing waits on them.
  # No RLIMIT_FSIZE: it applies to every file the command writes, and on the
  # real Mini it killed `tart ip` at once (SIGXFSZ). The poll below checks the
  # capture files every 20ms and the post-exit check catches the rest.
  (
    ulimit -c 0 2>/dev/null || true
    exec "$@" <"$infile" >"$out" 2>"$err"
  ) &
  pid=$!
  # Stub runs have no poll cap: spin instead of paying /bin/sleep after every fast child.
  # A set poll cap keeps the timed sleep so a count of polls still means time.
  if [[ "${S11_TEST_MODE:-}" == 1 && -z "$pb" ]]; then
    spins=0
    while job_running "$pid"; do
      spins=$((spins + 1))
      hit=0
      [[ "$SECONDS" -ge $((t0 + secs)) ]] && hit=1
      cleanup_past && hit=1
      if [[ "$hit" == 1 ]]; then RB_TIMED_OUT=1; term_kill_reap "$pid"; reaped=1; break; fi
      if [[ "$IN_CLEANUP" != 1 && "$REQUEST_CLEANUP" == 1 ]]; then RB_ABORTED=1; term_kill_reap "$pid"; reaped=1; break; fi
      if [[ $((spins % 16)) -eq 0 ]] && capture_over "$out" "$err"; then RB_OVERFLOW=1; term_kill_reap "$pid"; reaped=1; break; fi
    done
  else
    while :; do
      if ! job_running "$pid"; then break; fi
      polls=$((polls + 1))
      hit=0
      [[ -n "$pb" && "$polls" -ge "$pb" ]] && hit=1
      [[ "$SECONDS" -ge $((t0 + secs)) ]] && hit=1
      cleanup_past && hit=1
      if [[ "$hit" == 1 ]]; then
        RB_TIMED_OUT=1; term_kill_reap "$pid"; reaped=1; break
      fi
      if [[ "$IN_CLEANUP" != 1 && "$REQUEST_CLEANUP" == 1 ]]; then
        RB_ABORTED=1; term_kill_reap "$pid"; reaped=1; break
      fi
      if capture_over "$out" "$err"; then RB_OVERFLOW=1; term_kill_reap "$pid"; reaped=1; break; fi
      if [[ "${S11_TEST_MODE:-}" == 1 ]]; then sleep 0.01; else sleep 0.02; fi
    done
  fi
  if [[ "$reaped" != 1 ]]; then
    if job_running "$pid"; then disown "$pid" 2>/dev/null || true; mark_reap_unknown
    else wait "$pid" 2>/dev/null; REAP_STATUS=$?; fi
  fi
  capture_over "$out" "$err" && RB_OVERFLOW=1
  if [[ "$REAP_UNKNOWN" == 1 ]]; then RB_TIMED_OUT=1; RB_STATUS=124; HALT=1
  elif [[ "$RB_OVERFLOW" == 1 ]]; then RB_STATUS=125
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
  case "$n" in
    s11-0-probe-A-*) tail=${n#s11-0-probe-A-} ;;
    s11-0-probe-B-*) tail=${n#s11-0-probe-B-} ;;
    s11-0-probe-M-*) tail=${n#s11-0-probe-M-} ;;
    s11-0-probe-V-*) tail=${n#s11-0-probe-V-} ;;
    *) return 1 ;;
  esac
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
  # Line state machine for Tart's pretty JSON. Partial names are discarded with the exit status.
  awk -v cols="$cols" '
    function fail() { bad=1; exit 2 }
    function sval(v) { return v=="stopped"||v=="Stopped"||v=="STOPPED"||v=="running"||v=="Running"||v=="RUNNING"||v=="suspended"||v=="Suspended"||v=="SUSPENDED" }
    function note(k) {
      if (k!="Name"&&k!="State"&&k!="Source"&&k!="Disk"&&k!="Size"&&k!="Running"&&k!="Accessed") fail()
      if (index(obj," " k " ")>0) fail()
      obj=obj k " "; if (index(seen," " k " ")==0) { print k > cols; seen=seen k " " }
    }
    function endobj() {
      if (depth!=1 || nseen!=1 || sseen!=1 || name=="" || !sval(stt) || (name in have)) fail()
      have[name]=1; depth=0; nobj++; printf "%s\t%s\n", name, stt
    }
    BEGIN { phase=0; depth=0; nobj=0; pend=0; bad=0; seen=" "; obj=" " }
    {
      line=$0; sub(/^[ \t]+/, "", line); sub(/[ \t]+$/, "", line); if (line=="") next
      if (phase==0) {
        if (line=="[]") { print "json" > cols; phase=3; next }
        if (line!="[") fail()
        print "json" > cols; phase=1; next
      }
      if (phase!=1) fail()
      if (line=="]") { if (depth!=0 || pend==1) fail(); phase=2; next }
      if (line=="{") { if (depth!=0 || (nobj>0 && pend!=1)) fail(); depth=1; nseen=0; sseen=0; name=""; stt=""; obj=" "; pend=0; next }
      if (line=="}" || line=="},") { endobj(); pend=(line=="},"); next }
      if (depth!=1 || substr(line,1,1)!="\"") fail()
      q=index(substr(line,2), "\""); if (q<1) fail()
      k=substr(line,2,q-1); rest=substr(line,q+2); if (substr(rest,1,3)!=" : ") fail()
      v=substr(rest,4); if (substr(v,length(v),1)==",") v=substr(v,1,length(v)-1)
      note(k)
      if (k=="Name" || k=="State" || k=="Source" || k=="Accessed") {
        if (substr(v,1,1)!="\"" || substr(v,length(v),1)!="\"" || index(v,"\\")>0 || index(substr(v,2,length(v)-2),"\"")>0) fail()
        v=substr(v,2,length(v)-2)
      } else if (k=="Running") { if (v!="true" && v!="false") fail() }
      else if (v !~ /^[0-9]+$/) fail()
      if (k=="Name") { if (v=="" || index(v,"\t")>0) fail(); name=v; nseen++ }
      if (k=="State") { stt=v; sseen++ }
    }
    END { if (bad || (phase!=2 && phase!=3)) exit 2 }
  ' "$WORKDIR/list.last" >"$tmp" || { rm -f "$tmp" "$WORKDIR/names.tab"; return 1; }
  mv "$tmp" "$WORKDIR/names.tab"
}
refresh_list() {
  run_bounded 10 "$WORKDIR/list.out" "$WORKDIR/list.err" - -- tart list --format json || true
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
  if [[ "$2" == B ]]; then RUN_ARGV+=(--net-softnet-block=0.0.0.0/0 "--net-softnet-allow=${3}/32")
  elif [[ "$2" == M || "$2" == V ]]; then RUN_ARGV+=(--dir "s11mbx:${EXPORT_DIR}/mailbox"); fi
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
  say "ambiguous-destination: ${1:-clone} not confirmed; not deleting; owner must check tart list --format json"
}
reap_ssh_bg() {
  [[ -n "${SSH_BG_PID:-}" ]] || return 0
  if job_running "$SSH_BG_PID"; then term_kill_reap "$SSH_BG_PID"
  else wait "$SSH_BG_PID" 2>/dev/null || true; fi
  SSH_BG_PID=""
}
cleanup_owned() {
  [[ "$IN_CLEANUP" == 1 ]] && return 0
  IN_CLEANUP=1; trap '' INT TERM HUP
  local stop=not-run del=not-run absent=not-run skip=0
  if [[ "$S_STAGE" == M || "$S_STAGE" == V ]]; then
    [[ "$CLEANUP_LEFT" -gt 60 ]] && CLEANUP_LEFT=60
  fi
  CLEANUP_DEADLINE=$((SECONDS + CLEANUP_LEFT))
  reap_ssh_bg
  if [[ "$NO_DELETE" == 1 ]]; then
    stop_run_proc
    if [[ "$VOL_PHASE" == mounted || "$VOL_PHASE" == created || "$VOL_MAYBE" == 1 ]]; then vol_quarantine; fi
    IN_CLEANUP=0; CLEANUP_DEADLINE=0
    [[ "$FINALIZING" == 1 ]] || trap 'REQUEST_CLEANUP=1' INT TERM HUP
    return 0
  fi
  if [[ "$OWN_OK" == 1 ]] && name_is_safe "$OWN_NAME"; then
    [[ "$HALT" == 1 || "$REAP_UNKNOWN" == 1 ]] && skip=1
    run_bounded 30 "$WORKDIR/c.out" "$WORKDIR/c.err" - -- tart stop "$OWN_NAME" || true
    if [[ "$RB_TIMED_OUT" == 1 || "$RB_OVERFLOW" == 1 || "$REAP_UNKNOWN" == 1 ]]; then stop=timeout; add_reason stop-timeout; ANY_CLEANUP_FAILED=1
    elif [[ "$RB_STATUS" -ne 0 ]]; then stop=fail; add_reason stop-failed; ANY_CLEANUP_FAILED=1; else stop=ok; fi
    [[ "$REAP_UNKNOWN" == 1 ]] && skip=1
    [[ ( "$S_STAGE" == M || "$S_STAGE" == V ) && "$stop" != ok ]] && skip=1
    stop_run_proc
    [[ "$REAP_UNKNOWN" == 1 ]] && skip=1
    if [[ "$skip" == 1 ]]; then
      NO_DELETE=1; HALT=1; del=not-run; absent=unknown
      add_reason absent-unknown; ANY_CLEANUP_FAILED=1
      [[ "$S_STAGE" == M || "$S_STAGE" == V ]] && M_QUARANTINE=1
      printf '%s %s %s\n' "$stop" "$del" "$absent" >"$WORKDIR/cleanup.last"
    else
      run_bounded 30 "$WORKDIR/c.out" "$WORKDIR/c.err" - -- tart delete "$OWN_NAME" || true
      if [[ "$RB_TIMED_OUT" == 1 || "$RB_OVERFLOW" == 1 || "$REAP_UNKNOWN" == 1 ]]; then del=timeout; add_reason delete-timeout; ANY_CLEANUP_FAILED=1
      elif [[ "$RB_STATUS" -ne 0 ]]; then del=fail; add_reason delete-failed; ANY_CLEANUP_FAILED=1; else del=ok; fi
      if [[ "$REAP_UNKNOWN" == 1 ]]; then
        NO_DELETE=1; HALT=1; absent=unknown; add_reason absent-unknown; ANY_CLEANUP_FAILED=1
        [[ "$S_STAGE" == M || "$S_STAGE" == V ]] && M_QUARANTINE=1
      elif refresh_list; then
        if vm_status "$OWN_NAME" >/dev/null; then absent=no; add_reason still-present; ANY_CLEANUP_FAILED=1
        else absent=yes; OWN_OK=0; OWN_NAME=""; fi
      else absent=unknown; add_reason absent-unknown; ANY_CLEANUP_FAILED=1; fi
      printf '%s %s %s\n' "$stop" "$del" "$absent" >"$WORKDIR/cleanup.last"
    fi
  else stop_run_proc; fi
  volume_down
  IN_CLEANUP=0; CLEANUP_DEADLINE=0
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
  local clone_cap=180
  [[ "$stage" == M || "$stage" == V ]] && clone_cap=60
  run_bounded "$clone_cap" "$WORKDIR/clone.out" "$WORKDIR/clone.err" - -- tart clone "$GOLDEN" "$name" || true
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
  if [[ "$stage" == M || "$stage" == V ]]; then
    boot_ready_m "$name" || { ANY_COLLECTION_FAILED=1; add_reason collection-failed; return 1; }
    return 0
  fi
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
  local first=1 w e
  for w in "$@"; do
    case "$w" in *$'\n'*|*$'\r'*|*$'\t'*) return 1 ;; esac
  done
  printf '['
  for w in "$@"; do
    e=${w//\\/\\\\}
    e=${e//\"/\\\"}
    [[ "$first" -eq 1 ]] && first=0 || printf ','
    printf '"%s"' "$e"
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
  if [[ "$STAGE" == M && "$PRECHECK_FAILED" != 1 && ! -s "$WORKDIR/stage-M.json" ]]; then return 1; fi
  if [[ "$STAGE" == V && "$PRECHECK_FAILED" != 1 && "$DRY" != 1 && ! -s "$WORKDIR/stage-V.json" ]]; then return 1; fi
  local tmp=$OUT/report.json.tmp rs="" r cols measured unmeasured reasons help_subs exec_opts extra="" st f
  [[ -f "$REASONS_FILE" ]] && while IFS= read -r r; do rs="$rs $r"; done <"$REASONS_FILE"
  cols=$(json_lines "$WORKDIR/list.cols") || return 1
  measured=$(json_list $MEASURED) || return 1
  unmeasured=$(json_list $UNMEASURED) || return 1
  reasons=$(json_list $rs) || return 1
  help_subs=$(json_list $HELP_SUBS) || return 1
  exec_opts=$(json_list $EXEC_OPTS) || return 1
  if [[ -f "$WORKDIR/stage-V.json" ]]; then
    local vev
    vev=$(cat "$WORKDIR/v.evidence" 2>/dev/null || echo volume-only)
    case "$vev" in volume-only|DEFAULT-network/mechanics-only) ;; *) vev=volume-only ;; esac
    extra='"evidence":"'"$vev"'","export":"capped HFSX volume",'
  elif [[ -f "$WORKDIR/stage-M.json" ]]; then
    extra='"evidence":"DEFAULT-network/mechanics-only","export":"host-private-directory (not the capped HFSX volume)",'
  fi
  (
    set -e
    first=1
    printf '{"schema":"s11-0-probe/v2","status":"%s","endpoint_source":"%s","h1_independent_of_ip":"unknown",' "$STATUS" "$ENDPOINT_SOURCE"
    printf '"probe_budget_sec":%s,"cleanup_budget_sec":%s,"elapsed_sec":%s,' "$PROBE_BUDGET" "$CLEANUP_BUDGET" "$SECONDS"
    printf '"inventory_columns":%s,' "$cols"
    printf '"measured":%s,"unmeasured":%s,"reasons":%s,' "$measured" "$unmeasured" "$reasons"
    printf '"tart_help":"%s","tart_help_subcommands":%s,"tart_exec_help":"%s","tart_exec_options":%s,%s"stages":[' \
      "$HELP_STATE" "$help_subs" "$EXEC_HELP" "$exec_opts" "$extra"
    for st in A B C M V; do
      f=$WORKDIR/stage-$st.json
      [[ -f "$f" ]] || continue
      [[ -s "$f" ]] || exit 1
      if [[ "$first" -eq 1 ]]; then first=0; else printf ','; fi
      cat "$f"
    done
    printf ']}\n'
  ) >"$tmp" || { rm -f "$tmp"; return 1; }
  [[ -s "$tmp" ]] || { rm -f "$tmp"; return 1; }
  mv "$tmp" "$OUT/report.json"
}
has_meta() {
  case "$1" in *"'"*) return 0 ;; esac
  case "$1" in *'"'*) return 0 ;; esac
  case "$1" in *'`'*) return 0 ;; esac
  case "$1" in *'$'*) return 0 ;; esac
  case "$1" in *';'*) return 0 ;; esac
  return 1
}
host_sha256_file() {
  local f=$1 out=""
  if [[ -x /usr/bin/sha256sum ]]; then
    run_bounded 5 "$WORKDIR/sha.out" "$WORKDIR/sha.err" - -- /usr/bin/sha256sum "$f" || true
  elif [[ -x /usr/bin/shasum ]]; then
    run_bounded 5 "$WORKDIR/sha.out" "$WORKDIR/sha.err" - -- /usr/bin/shasum -a 256 "$f" || true
  elif [[ -x /usr/bin/python3 ]]; then
    run_bounded 5 "$WORKDIR/sha.out" "$WORKDIR/sha.err" - -- /usr/bin/python3 -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "$f" || true
  else
    return 1
  fi
  [[ "$RB_STATUS" -eq 0 && "$RB_TIMED_OUT" != 1 && "$RB_OVERFLOW" != 1 && "$RB_ABORTED" != 1 && "$REAP_UNKNOWN" != 1 ]] || return 1
  out=$(awk '{print $1; exit}' "$WORKDIR/sha.out" 2>/dev/null || true)
  case "$out" in *[!0-9a-f]*|"") return 1 ;; esac
  [[ ${#out} -eq 64 ]] || return 1
  printf '%s\n' "$out"
}
# BEGIN DISPATCHER
write_dispatcher() {
cat <<'END_DISPATCHER'
set -eu
set -f
umask 077
MBX='/Volumes/My Shared Files/s11mbx'
WORK='/Users/eval/.s11m/work'
cmd=${SSH_ORIGINAL_COMMAND-}
case "$cmd" in
  *$'\n'*|*$'\r'*|*'$('*|*'`'*|*[';|&><']*) printf 'reject\n'; exit 2 ;;
esac
[[ ${#cmd} -le 71 ]] || { printf 'reject\n'; exit 2; }
case "$cmd" in
  upload)
    [[ -d "$MBX" && ! -L "$MBX" ]] || { printf 'fail-dir\n'; exit 3; }
    [[ -d "$WORK" && ! -L "$WORK" ]] || { printf 'fail-dir\n'; exit 3; }
    rm -f "$WORK/probe.upload"
    dd if=/dev/stdin of="$WORK/probe.upload" bs=4096 count=2049 >/dev/null 2>&1 || { printf 'fail-dir\n'; exit 3; }
    sz=$(wc -c <"$WORK/probe.upload" | tr -d ' ')
    [[ "$sz" -le 8388608 && "$sz" -gt 0 ]] || { rm -f "$WORK/probe.upload"; printf 'fail-dir\n'; exit 3; }
    mv "$WORK/probe.upload" "$WORK/probe"
    chmod 0700 "$WORK/probe"
    exit 0
    ;;
  verify\ *)
    hex=${cmd#verify }
    case "$hex" in *[!0-9a-f]*|'') printf 'reject\n'; exit 2 ;; esac
    [[ ${#hex} -eq 64 ]] || { printf 'reject\n'; exit 2; }
    [[ -f "$WORK/probe" && ! -L "$WORK/probe" ]] || { printf 'fail-probe\n'; exit 5; }
    got=$(/usr/bin/shasum -a 256 "$WORK/probe" | awk '{print $1}')
    [[ "$got" == "$hex" ]] || { printf 'fail-probe\n'; exit 5; }
    printf '%s\n' "$hex" >"$WORK/probe.sha256"
    chmod 0600 "$WORK/probe.sha256"
    exit 0
    ;;
  'run C1'|'run C2'|'run C8'|'run C9')
    [[ -f "$WORK/probe" && ! -L "$WORK/probe" && -f "$WORK/probe.sha256" && ! -L "$WORK/probe.sha256" ]] || { printf 'fail-run\n'; exit 6; }
    want=$(awk '{print $1; exit}' "$WORK/probe.sha256")
    got=$(/usr/bin/shasum -a 256 "$WORK/probe" | awk '{print $1}')
    [[ "$got" == "$want" && ${#want} -eq 64 ]] || { printf 'fail-run\n'; exit 6; }
    case "$cmd" in
      'run C1')
        w=0; r=0
        "$WORK/probe" writer --dir "$MBX" --role guest --count 8 --timeout 50 || w=$?
        "$WORK/probe" reader --dir "$MBX" --role guest --count 8 --timeout 50 || r=$?
        [[ "$w" -eq 0 && "$r" -eq 0 ]] || exit 6
        ;;
      'run C2')
        raw=$(dd if="$MBX/boundary" bs=32 count=1 2>/dev/null || true)
        b=$(printf '%s' "$raw" | tr -d '\r\n')
        case "$b" in file-sync|dir-sync|rename) ;; *) printf 'reject\n'; exit 2 ;; esac
        "$WORK/probe" c2 "$b" --dir "$MBX" --role guest --count 3 --timeout 50 || exit 6
        ;;
      'run C8')
        "$WORK/probe" c8 --dir "$MBX" --role guest --count 1 --timeout 50 || exit 6
        ;;
      'run C9')
        raw=$(dd if="$MBX/fill-mode" bs=16 count=1 2>/dev/null || true)
        b=$(printf '%s' "$raw" | tr -d '\r\n')
        case "$b" in data|small) ;; *) printf 'reject\n'; exit 2 ;; esac
        "$WORK/probe" fill --dir "$MBX" --role guest --mode "$b" --timeout 50 || exit 6
        ;;
    esac
    exit 0
    ;;
  *) printf 'reject\n'; exit 2 ;;
esac
END_DISPATCHER
}
# END DISPATCHER
emit_init() {
  local digest=$1
  printf '%s\n' 'set -eu' 'umask 077' 'd=/Users/eval/.s11m/work' \
    'if [[ -e "$d" || -L "$d" ]]; then printf "fail-exists\n"; exit 1; fi' \
    'mkdir -m 0700 -p /Users/eval/.s11m' 'mkdir -m 0700 "$d"' \
    "cat > \"\$d/dispatcher\" <<'END_DISPATCHER'"
  write_dispatcher
  printf '%s\n' 'END_DISPATCHER' 'chmod 0700 "$d/dispatcher"' \
    'got=$(/usr/bin/shasum -a 256 "$d/dispatcher" | awk '"'"'{print $1}'"'"')' \
    "[[ \"\$got\" == \"$digest\" ]] || { printf 'fail-digest\\n'; exit 1; }" \
    'ak=$HOME/.ssh/authorized_keys' \
    'if [[ -L "$ak" || ! -f "$ak" ]]; then printf "fail-ak\n"; exit 1; fi' \
    "printf '%s\\n' 'restrict,command=\"/Users/eval/.s11m/work/dispatcher\" ${ONCE_PUB}' >> \"\$ak\"" \
    "printf 'init-ok\\n'"
}
sole_running() {
  local got
  refresh_list || { add_reason list-untrusted; return 1; }
  got=$(awk -F '\t' 'BEGIN{n=0; name=""} $2=="running"||$2=="Running"||$2=="RUNNING"{n++; name=$1} END{if(n==1) print name; else print "bad"}' "$WORKDIR/names.tab")
  [[ "$got" == "$OWN_NAME" ]] || { add_reason not-sole; return 1; }
}
write_known() {
  local kh=$1 line
  line=$(awk '$1=="ssh-ed25519" && $2 ~ /^[A-Za-z0-9+\/=]+$/ {print $1" "$2; exit}' "$PINNED_KEY")
  [[ -n "$line" ]] || return 1
  has_meta "$line" && return 1
  printf '%s %s\n' "$GUEST_IP" "$line" >"$kh"
}
fill_ssh() {
  local key=$1 remote=$2 kh=$3
  SSH_ARGV=(ssh -F /dev/null -T
    -o BatchMode=yes -o ConnectTimeout=5 -o StrictHostKeyChecking=yes
    -o "UserKnownHostsFile=${kh}" -o GlobalKnownHostsFile=/dev/null
    -o IdentitiesOnly=yes -o IdentityAgent=none -o ClearAllForwardings=yes
    -o RequestTTY=no -o PreferredAuthentications=publickey
    -o PasswordAuthentication=no -o KbdInteractiveAuthentication=no
    -o PubkeyAuthentication=yes -o NumberOfPasswordPrompts=0
    -i "$key" "${GUEST_USER}@${GUEST_IP}" "$remote")
}
ssh_lab() {
  local secs=$1 infile=$2 key=$3 remote=$4 kh=$WORKDIR/known_hosts
  sole_running || { RB_STATUS=1; RB_TIMED_OUT=0; RB_OVERFLOW=0; return 1; }
  write_known "$kh" || { add_reason pinned-host-key; RB_STATUS=1; return 1; }
  fill_ssh "$key" "$remote" "$kh"
  run_bounded "$secs" "$WORKDIR/ssh.out" "$WORKDIR/ssh.err" "$infile" -- "${SSH_ARGV[@]}" || true
}
ssh_bg() {
  local key=$1 remote=$2 kh=$WORKDIR/known_hosts
  sole_running || return 1
  write_known "$kh" || return 1
  fill_ssh "$key" "$remote" "$kh"
  SSH_BG_OV=0; SSH_BG_AB=0
  : >"$WORKDIR/ssh.bg.out"; : >"$WORKDIR/ssh.bg.err"
  # Direct file capture: no asynchronous reader can finish after ssh exits, so the
  # post-exit size check is exact. note_ssh_bg polls the size and kills ssh at CAP.
  "${SSH_ARGV[@]}" </dev/null >"$WORKDIR/ssh.bg.out" 2>"$WORKDIR/ssh.bg.err" &
  SSH_BG_PID=$!
}
boot_ready_m() {
  local name=$1 exec_done=0 guest_ip="" boot_until
  boot_until=$((SECONDS + $(cap_secs 60)))
  S_SSH=no-address
  while [[ "$SECONDS" -lt "$boot_until" && "$REQUEST_CLEANUP" != 1 ]]; do
    try_exec "$exec_done" "$name" && exec_done=1
    if [[ "$S_SSH" != pending && "$S_SSH" != connected ]]; then
      run_bounded 10 "$WORKDIR/ip.out" "$WORKDIR/ip.err" - -- tart ip "$name" || true
      if [[ "$RB_TIMED_OUT" == 1 || "$RB_OVERFLOW" == 1 || "$HALT" == 1 ]]; then S_SSH=timeout; break; fi
      if [[ "$RB_STATUS" -eq 127 ]]; then S_SSH=tool-error; break; fi
      guest_ip=$(awk '{for(i=1;i<=NF;i++) if($i ~ /^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$/){print $i; exit}}' "$WORKDIR/ip.out")
      if is_ipv4 "$guest_ip"; then S_SSH=pending; GUEST_IP=$guest_ip; else S_SSH=no-address; fi
    fi
    if [[ "$S_SSH" == pending ]]; then
      if ! sole_running; then S_SSH=not-sole; break; fi
      ssh_lab 15 /dev/null "$SSH_KEY" /usr/bin/true
      if [[ "$RB_TIMED_OUT" == 1 || "$RB_OVERFLOW" == 1 || "$HALT" == 1 ]]; then S_SSH=timeout; break; fi
      if [[ "$RB_STATUS" -eq 0 ]]; then S_SSH=connected; fi
    fi
    if [[ "$S_SSH" == connected && "$exec_done" == 1 ]]; then S_COLL=ok; S_EXEC=$EXEC_RESULT; return 0; fi
    [[ "$SECONDS" -ge "$boot_until" ]] && break
    sleep 0.2
  done
  S_EXEC=$EXEC_RESULT
  [[ "$S_SSH" == pending || "$S_SSH" == no-address ]] && S_SSH=timeout
  S_COLL=failed
  return 1
}
make_once_key() {
  local dir
  dir=$(mktemp -d "$WORKDIR/once.XXXXXX") || return 1
  chmod 0700 "$dir" || return 1
  run_bounded 15 "$WORKDIR/kg.out" "$WORKDIR/kg.err" - -- /usr/bin/ssh-keygen -q -t ed25519 -N "" -C s11once -f "$dir/key" || true
  [[ "$RB_STATUS" -eq 0 && -f "$dir/key" && ! -L "$dir/key" && -f "$dir/key.pub" ]] || return 1
  ONCE_KEY=$dir/key
  ONCE_PUB=$(awk '$1=="ssh-ed25519" && $2 ~ /^[A-Za-z0-9+\/=]+$/ {print $1" "$2; exit}' "$dir/key.pub")
  [[ -n "$ONCE_PUB" ]] || return 1
  if has_meta "$ONCE_PUB"; then return 1; fi
}
# Every name the guest reads is planted before the VM. A lookup of a missing name sticks in virtiofs.
# Guest reads host-write/hg-ping, host-write/gh-ready, host-write/gh-pong, and summary-host.
# The same loops also plant the slots only the host reads. write_c1_names must list this set.
seed_c1_slots() {
  local mbx=$1 leaf name
  for leaf in host-write guest-write; do
    for name in hg-ping hg-pong gh-ping gh-pong hg-ready gh-ready; do
      printf '%s\n' 'v=1 op=empty seq=0' >"$mbx/$leaf/$name" || return 1
    done
  done
  printf '%s\n' 'v=1 op=empty seq=0' >"$mbx/summary-host" || return 1
  printf '%s\n' 'v=1 op=empty seq=0' >"$mbx/summary-guest" || return 1
}
prepare_mailbox() {
  local root=$1
  mkdir -m 0700 "$root/mailbox" || return 1
  case "$M_CASE" in
    C1)
      mkdir -m 0700 "$root/mailbox/host-write" "$root/mailbox/guest-write" || return 1
      seed_c1_slots "$root/mailbox" || return 1
      ;;
    C2)
      mkdir -m 0700 "$root/mailbox/data" "$root/mailbox/evidence" || return 1
      printf '%s\n' "$M_BOUNDARY" >"$root/mailbox/boundary" || return 1
      ;;
    C9)
      mkdir -m 0700 "$root/mailbox/payload" || return 1
      printf '%s\n' "$M_BOUNDARY" >"$root/mailbox/fill-mode" || return 1
      printf '%s\n' 'v=1 seq=0 bytes=0 files=0 enospc=0' >"$root/mailbox/progress" || return 1
      printf '%s\n' 'v=1 op=empty seq=0' >"$root/mailbox/result-guest" || return 1
      ;;
  esac
}
prepare_export() {
  local st
  EXPORT_DONE=0; EXPORT_STATE=absent
  if [[ "$S_STAGE" == V ]]; then
    EXPORT_DIR=$VOL_M
    vol_io 10 prepare "$EXPORT_DIR" "$M_CASE" "$M_BOUNDARY"
    st=$?
    if [[ "$st" -eq 2 ]]; then VOL_MAYBE=1; vol_quarantine; return 1; fi
    [[ "$st" -eq 0 ]] || return 1
    return 0
  fi
  EXPORT_DIR=$(mktemp -d "$OUT/s11m.XXXXXX") || return 1
  chmod 0700 "$EXPORT_DIR" || return 1
  prepare_mailbox "$EXPORT_DIR" || return 1
}
arm_m_load() {
  M_LOAD_END=$((SECONDS + 60))
  [[ "$M_LOAD_END" -gt "$PROBE_DEADLINE" ]] && M_LOAD_END=$PROBE_DEADLINE
}
m_left() {
  local n=$((M_LOAD_END - SECONDS))
  [[ "$n" -lt 0 ]] && n=0
  [[ "$n" -gt 60 ]] && n=60
  printf '%s\n' "$n"
}
clear_snaps() {
  H1_ST=0; H1_TO=0; H1_AB=0; H1_OV=0; H1_RU=0
  H2_ST=0; H2_TO=0; H2_AB=0; H2_OV=0; H2_RU=0
  G_ST=0; G_TO=0; G_AB=0; G_OV=0; G_RU=0
}
snap1() { H1_ST=$RB_STATUS; H1_TO=$RB_TIMED_OUT; H1_AB=$RB_ABORTED; H1_OV=$RB_OVERFLOW; H1_RU=$REAP_UNKNOWN; }
snap2() { H2_ST=$RB_STATUS; H2_TO=$RB_TIMED_OUT; H2_AB=$RB_ABORTED; H2_OV=$RB_OVERFLOW; H2_RU=$REAP_UNKNOWN; }
snapg() { G_ST=$RB_STATUS; G_TO=$RB_TIMED_OUT; G_AB=$RB_ABORTED; G_OV=$RB_OVERFLOW; G_RU=$REAP_UNKNOWN; }
note_ssh_bg() {
  local gpid=$1
  if [[ "$REQUEST_CLEANUP" == 1 ]]; then SSH_BG_AB=1; term_kill_reap "$gpid"; return 0; fi
  if capture_over "$WORKDIR/ssh.bg.out" "$WORKDIR/ssh.bg.err"; then SSH_BG_OV=1; term_kill_reap "$gpid"; return 0; fi
  return 1
}
wait_guest() {
  local gpid=$1 st
  while [[ "$SECONDS" -lt "$M_LOAD_END" ]] && job_running "$gpid"; do
    note_ssh_bg "$gpid" && break
    sleep 0.05
  done
  if job_running "$gpid"; then
    term_kill_reap "$gpid"
    G_ST=$REAP_STATUS; G_TO=1; G_RU=$REAP_UNKNOWN
  else
    wait "$gpid" 2>/dev/null
    st=$?
    G_ST=$st; G_TO=0; G_RU=0
  fi
  capture_over "$WORKDIR/ssh.bg.out" "$WORKDIR/ssh.bg.err" && SSH_BG_OV=1
  G_OV=$SSH_BG_OV; G_AB=$SSH_BG_AB
  if [[ "$SSH_BG_AB" == 1 && "$G_TO" != 1 ]]; then G_ST=130; fi
  if [[ "$SSH_BG_OV" == 1 && "$G_TO" != 1 && "$SSH_BG_AB" != 1 ]]; then G_ST=125; fi
  SSH_BG_PID=""
}
c1_retry_ok() {
  case "$1" in
    absent:*,replaced:*) ;;
    *) return 1 ;;
  esac
  local body=${1#absent:}
  local a=${body%%,replaced:*}
  local b=${body#*,replaced:}
  [[ "$1" == "absent:${a},replaced:${b}" ]] || return 1
  [[ -n "$a" && ${#a} -le 7 && "$a" != *[!0-9]* ]] || return 1
  [[ -n "$b" && ${#b} -le 7 && "$b" != *[!0-9]* ]] || return 1
}
parse_fields() {
  local line=$1 tok key val seen=" " nseen=0
  local v= op= role= verdict= reason= n= torn= lost= integrity= latency=
  local vis= rtt= slice= wslice= openus= writeus= fsyncus= renameus= dsync= readus= peer= digest= gate= retry=
  local boundary= planned= attempted= renamed= visible= part= dirsync= ops= created= elevated=
  M_RECORD=$line; SHAPE_OK=0
  for tok in $line; do
    case "$tok" in
      *=*) key=${tok%%=*}; val=${tok#*=} ;;
      *) return 1 ;;
    esac
    [[ -n "$key" && -n "$val" ]] || return 1
    case "$seen" in *" $key "*) return 1 ;; esac
    seen="$seen$key "; nseen=$((nseen + 1))
    case "$key" in
      v) v=$val ;; op) op=$val ;; role) role=$val ;; verdict) verdict=$val ;; reason) reason=$val ;;
      n) n=$val ;; torn) torn=$val ;; lost) lost=$val ;; integrity) integrity=$val ;; latency) latency=$val ;;
      vis_us) vis=$val ;; rtt_us) rtt=$val ;; slice_us) slice=$val ;; w_slice_us) wslice=$val ;;
      open_us) openus=$val ;; write_us) writeus=$val ;; fsync_us) fsyncus=$val ;; rename_us) renameus=$val ;;
      dir_sync_us) dsync=$val ;; read_us) readus=$val ;; peer) peer=$val ;; digest) digest=$val ;; gate) gate=$val ;;
      retry) retry=$val ;;
      boundary) boundary=$val ;; planned) planned=$val ;; attempted) attempted=$val ;; renamed) renamed=$val ;;
      visible) visible=$val ;; part) part=$val ;; dir_synced) dirsync=$val ;;
      ops) ops=$val ;; created) created=$val ;; elevated) elevated=$val ;;
      *) return 1 ;;
    esac
  done
  [[ "$v" == 1 ]] || return 1
  case "$reason" in
    -|deadline|timeout|peer-not-ready|no-peer-summary|single-principal|no-non-admin-principal|principal-unknown|mapping-not-measured|torn|bad-pong|bad-part|bad-record|not-sole|init-failed|upload-failed|verify-failed|run-failed|probe-failed|skipped|quarantine|subject-policy|io|nofollow|replaced|partial-coverage) ;;
    *) return 1 ;;
  esac
  case "$verdict" in ok|failed|unknown|not-ack) ;; *) return 1 ;; esac
  case "$op" in
    c1)
      [[ "$nseen" -eq 24 && -n "$digest" && -n "$peer" && -n "$latency" && -n "$integrity" && -n "$vis" && -n "$rtt" && -n "$gate" && -n "$retry" ]] || return 1
      c1_retry_ok "$retry" || return 1
      case "$gate" in -|exceeds-2s) ;; *) return 1 ;; esac
      if [[ "$verdict" == ok ]]; then
        [[ "$torn" == 0 && "$integrity" == ok && "$latency" == measured && "$peer" == 1 && "$role" == host && "$n" == 8 && "$vis" == "≤rtt" ]] || return 1
        case "$lost" in -) ;; *) case "$lost" in *[!0-9]*) return 1 ;; esac ;; esac
        case "$slice$wslice$openus$writeus$fsyncus$renameus$readus" in *[!0-9]*|"") return 1 ;; esac
      fi
      ;;
    c2)
      [[ "$nseen" -eq 12 && -n "$boundary" && -n "$planned" && -n "$attempted" && -n "$renamed" && -n "$visible" && -n "$part" && -n "$dirsync" ]] || return 1
      if [[ "$verdict" == not-ack ]]; then
        case "$planned$attempted$renamed$visible" in *[!0-9]*) return 1 ;; esac
        [[ "$role" == host && "$boundary" == "$M_BOUNDARY" ]] || return 1
      fi
      ;;
    c8)
      [[ "$nseen" -eq 9 && "$ops" == 0 && "$created" == 0 && "$elevated" == 0 && "$role" == host ]] || return 1
      case "$n" in ""|*[!0-9]*) return 1 ;; esac
      ;;
    *) return 1 ;;
  esac
  M_VERDICT=$verdict; M_REASON=$reason; M_OP=$op; M_ROLE_REC=$role; M_PEER=$peer; M_LATENCY=$latency; M_INTEGRITY=$integrity; M_N=$n
  SHAPE_OK=1
}
read_slot_verdict() {
  local f=$1 raw=$WORKDIR/slot.raw line n st
  SHAPE_OK=0; M_RECORD=""
  if [[ "$S_STAGE" == V ]]; then
    vol_io 5 slot "$f"
    st=$?
    if [[ "$st" -eq 2 ]]; then
      VOL_MAYBE=1; vol_quarantine
      M_VERDICT=unknown; M_REASON=quarantine; ANY_COLLECTION_FAILED=1
      return 2
    fi
    [[ "$st" -eq 0 ]] || return 1
    cp "$WORKDIR/v.out" "$raw" || return 1
  else
    [[ -f "$f" && ! -L "$f" ]] || return 1
    run_bounded 5 "$raw" "$WORKDIR/slot.err" - -- head -c 4097 -- "$f" || true
    [[ "$RB_STATUS" -eq 0 && "$RB_TIMED_OUT" != 1 && "$RB_OVERFLOW" != 1 && "$RB_ABORTED" != 1 && "$REAP_UNKNOWN" != 1 ]] || return 1
  fi
  [[ -s "$raw" ]] || return 1
  n=$(wc -l <"$raw" | tr -d ' ')
  [[ "$n" == 1 ]] || return 1
  IFS= read -r line <"$raw" || return 1
  printf '%s\n' "$line" >"$WORKDIR/slot.one" || { IO_FAIL=1; return 1; }
  cmp -s "$WORKDIR/slot.one" "$raw" || return 1
  parse_fields "$line" || { M_VERDICT=unknown; M_REASON=bad-record; SHAPE_OK=0; return 1; }
}
settle_host() {
  local kind=$1
  if [[ "$M_VERDICT" == failed && "$SHAPE_OK" == 1 ]]; then return 0; fi
  if [[ "$H1_TO" == 1 || "$H2_TO" == 1 || "$G_TO" == 1 || "$H1_RU" == 1 || "$H2_RU" == 1 || "$G_RU" == 1 ]]; then
    M_VERDICT=unknown; M_REASON=deadline; SHAPE_OK=0; return 1
  fi
  if [[ "$H1_OV" == 1 || "$H2_OV" == 1 || "$G_OV" == 1 || "$H1_AB" == 1 || "$H2_AB" == 1 || "$G_AB" == 1 || "$SHAPE_OK" != 1 ]]; then
    M_VERDICT=unknown; M_REASON=probe-failed; SHAPE_OK=0; return 1
  fi
  if [[ "$M_VERDICT" == ok || "$M_VERDICT" == not-ack ]]; then
    [[ "$H1_ST" == 0 && "$G_ST" == 0 ]] || { M_VERDICT=unknown; M_REASON=probe-failed; return 1; }
    if [[ "$kind" == c1 ]]; then
      [[ "$H2_ST" == 0 && "$M_PEER" == 1 && "$M_LATENCY" == measured && "$M_INTEGRITY" == ok && "$M_N" == 8 && "$M_OP" == c1 && "$M_ROLE_REC" == host ]] || { M_VERDICT=unknown; M_REASON=probe-failed; return 1; }
    fi
    if [[ "$kind" == c2 && ( "$M_OP" != c2 || "$M_ROLE_REC" != host ) ]]; then M_VERDICT=unknown; M_REASON=probe-failed; return 1; fi
    return 0
  fi
  [[ "$M_VERDICT" == unknown ]] && return 0
  M_VERDICT=unknown; M_REASON=probe-failed
  return 1
}
m_run_c1() {
  local gpid left
  clear_snaps; arm_m_load
  ssh_bg "$ONCE_KEY" "run C1" || { M_VERDICT=unknown; M_REASON=not-sole; SHAPE_OK=0; return 1; }
  gpid=$SSH_BG_PID
  left=$(m_left)
  run_bounded "$left" "$WORKDIR/pw.out" "$WORKDIR/pw.err" - -- "$M_PROBE" writer --dir "$EXPORT_DIR/mailbox" --role host --count 8 --timeout "$left" || true
  snap1
  left=$(m_left)
  run_bounded "$left" "$WORKDIR/pr.out" "$WORKDIR/pr.err" - -- "$M_PROBE" reader --dir "$EXPORT_DIR/mailbox" --role host --count 8 --timeout "$left" || true
  snap2
  wait_guest "$gpid"
  read_slot_verdict "$EXPORT_DIR/mailbox/result-host" || true
  [[ "$S_STAGE" == V && "$VOL_PHASE" == quarantine ]] && return 1
  settle_host c1 || true
}
m_run_c2() {
  local left
  clear_snaps; arm_m_load
  left=$(m_left)
  ssh_lab "$left" /dev/null "$ONCE_KEY" "run C2"
  snapg
  left=$(m_left)
  run_bounded "$left" "$WORKDIR/ph.out" "$WORKDIR/ph.err" - -- "$M_PROBE" c2 "$M_BOUNDARY" --dir "$EXPORT_DIR/mailbox" --role host --count 3 --timeout "$left" || true
  snap1
  read_slot_verdict "$EXPORT_DIR/mailbox/result-host" || true
  [[ "$S_STAGE" == V && "$VOL_PHASE" == quarantine ]] && return 1
  settle_host c2 || true
}
m_run_c8() {
  local left
  clear_snaps; arm_m_load
  left=$(m_left)
  ssh_lab "$left" /dev/null "$ONCE_KEY" "run C8"
  snapg
  left=$(m_left)
  run_bounded "$left" "$WORKDIR/p8.out" "$WORKDIR/p8.err" - -- "$M_PROBE" c8 --dir "$EXPORT_DIR/mailbox" --role host --count 1 --timeout "$left" || true
  snap1
  read_slot_verdict "$EXPORT_DIR/mailbox/result-host" || true
  settle_host c8 || true
}
m_measure() {
  local digest sz
  make_once_key || { M_VERDICT=unknown; M_REASON=init-failed; return 1; }
  write_dispatcher >"$WORKDIR/dispatcher.body" || { IO_FAIL=1; M_VERDICT=unknown; M_REASON=init-failed; return 1; }
  digest=$(host_sha256_file "$WORKDIR/dispatcher.body") || { M_VERDICT=unknown; M_REASON=init-failed; return 1; }
  emit_init "$digest" >"$WORKDIR/init.body" || { M_VERDICT=unknown; M_REASON=init-failed; return 1; }
  ssh_lab 30 "$WORKDIR/init.body" "$SSH_KEY" 'true s11m.XXXXXX; cat >"$HOME/.s11m.init" && sh "$HOME/.s11m.init"'
  [[ "$RB_STATUS" -eq 0 ]] || { M_VERDICT=unknown; M_REASON=init-failed; return 1; }
  run_bounded 5 "$WORKDIR/sz.out" "$WORKDIR/sz.err" - -- wc -c "$M_PROBE" || true
  [[ "$RB_STATUS" -eq 0 && "$RB_TIMED_OUT" != 1 && "$RB_OVERFLOW" != 1 && "$REAP_UNKNOWN" != 1 ]] || { M_VERDICT=unknown; M_REASON=upload-failed; return 1; }
  sz=$(awk '{print $1; exit}' "$WORKDIR/sz.out")
  [[ "$sz" -le 8388608 && "$sz" -gt 0 ]] || { M_VERDICT=unknown; M_REASON=upload-failed; return 1; }
  ssh_lab 30 "$M_PROBE" "$ONCE_KEY" upload
  [[ "$RB_STATUS" -eq 0 ]] || { M_VERDICT=unknown; M_REASON=upload-failed; return 1; }
  digest=$(host_sha256_file "$M_PROBE") || { M_VERDICT=unknown; M_REASON=verify-failed; return 1; }
  ssh_lab 20 /dev/null "$ONCE_KEY" "verify $digest"
  [[ "$RB_STATUS" -eq 0 ]] || { M_VERDICT=unknown; M_REASON=verify-failed; return 1; }
  case "$M_CASE" in
    C1) m_run_c1 ;;
    C2) m_run_c2 ;;
    C8) m_run_c8 ;;
    C9) m_run_c9 ;;
    *) M_VERDICT=unknown; M_REASON=probe-failed; return 1 ;;
  esac
}
write_names() {
  local f=$1; shift
  : >"$f"
  local n
  for n in "$@"; do printf '%s\n' "$n" >>"$f"; done
}
write_frames() {
  local f=$1 max=$2 i=1
  : >"$f"
  while [[ "$i" -le "$max" ]]; do
    printf '%s\n' "f-$i" "f-$i.tmp" >>"$f"
    i=$((i + 1))
  done
}
# Same slot names as seed_c1_slots, plus the staging file publish renames over them.
write_c1_names() {
  local f=$1 flow kind
  : >"$f"
  for flow in hg gh; do
    for kind in ping pong ready; do
      printf '%s\n' "${flow}-${kind}" "${flow}-${kind}.tmp" >>"$f"
    done
  done
}
name_allowed() { grep -F -x -q -- "$1" "$2"; }
scan_one() {
  local dir=$1 allow=$2 dirs=$3 raw=$WORKDIR/scan.raw path name count=0
  [[ -d "$dir" && ! -L "$dir" ]] || return 1
  # Stop at whitelist+1. NUL records keep a newline inside one name. Nonzero, timeout, or overflow is quarantine.
  run_bounded 10 "$raw" "$WORKDIR/scan.err" - -- bash -c 'set -o pipefail; set -f; n=0; find "$1" -mindepth 1 -maxdepth 1 -print0 | while IFS= read -r -d "" p; do n=$((n+1)); printf "%s\0" "$p"; if [[ "$n" -ge 33 ]]; then exit 1; fi; done; exit $?' _ "$dir" || true
  [[ "$RB_STATUS" -eq 0 && "$RB_TIMED_OUT" != 1 && "$RB_OVERFLOW" != 1 && "$RB_ABORTED" != 1 && "$REAP_UNKNOWN" != 1 ]] || return 1
  while IFS= read -r -d '' path; do
    count=$((count + 1))
    [[ "$count" -le 32 ]] || return 1
    name=${path##*/}
    case "$name" in ''|*$'\n'*|*'/'*) return 1 ;; esac
    [[ -L "$path" ]] && return 1
    name_allowed "$name" "$allow" || return 1
    if name_allowed "$name" "$dirs"; then
      [[ -d "$path" ]] || return 1
    else
      [[ -f "$path" ]] || return 1
    fi
  done <"$raw"
}
audit_export() {
  local mbx=$EXPORT_DIR/mailbox
  [[ -d "$EXPORT_DIR" && ! -L "$EXPORT_DIR" && -d "$mbx" && ! -L "$mbx" ]] || return 1
  : >"$WORKDIR/allow.none"
  case "$M_CASE" in
    C1)
      write_names "$WORKDIR/allow.root" host-write guest-write summary-host summary-guest result-host result-guest
      write_names "$WORKDIR/allow.dirs" host-write guest-write
      write_c1_names "$WORKDIR/allow.child"
      ;;
    C2)
      write_names "$WORKDIR/allow.root" boundary data evidence result-host result-guest summary-host summary-guest
      write_names "$WORKDIR/allow.dirs" data evidence
      write_frames "$WORKDIR/allow.child" 3
      printf '%s\n' part part.tmp >>"$WORKDIR/allow.child"
      write_names "$WORKDIR/allow.evidence" progress progress.tmp
      ;;
    C8)
      write_names "$WORKDIR/allow.root" result-guest result-host
      : >"$WORKDIR/allow.dirs"
      ;;
    *) return 1 ;;
  esac
  scan_one "$mbx" "$WORKDIR/allow.root" "$WORKDIR/allow.dirs" || return 1
  if [[ "$M_CASE" == C1 ]]; then
    [[ -d "$mbx/host-write" ]] && { scan_one "$mbx/host-write" "$WORKDIR/allow.child" "$WORKDIR/allow.none" || return 1; }
    [[ -d "$mbx/guest-write" ]] && { scan_one "$mbx/guest-write" "$WORKDIR/allow.child" "$WORKDIR/allow.none" || return 1; }
  elif [[ "$M_CASE" == C2 ]]; then
    [[ -d "$mbx/data" ]] && { scan_one "$mbx/data" "$WORKDIR/allow.child" "$WORKDIR/allow.none" || return 1; }
    [[ -d "$mbx/evidence" ]] && { scan_one "$mbx/evidence" "$WORKDIR/allow.evidence" "$WORKDIR/allow.none" || return 1; }
  fi
}
cleanup_export() {
  local d abs
  if [[ "$STAGE" == V || "$S_STAGE" == V ]]; then EXPORT_DONE=1; return 0; fi
  [[ "$EXPORT_DONE" == 1 ]] && return 0
  [[ -n "$EXPORT_DIR" ]] || return 0
  if [[ -L "$EXPORT_DIR" || ! -d "$EXPORT_DIR" ]]; then
    EXPORT_STATE=quarantine; ANY_CLEANUP_FAILED=1; add_reason quarantine; EXPORT_DONE=1
    [[ "$S_STAGE" == M ]] && M_QUARANTINE=1
    return 0
  fi
  if [[ "$S_CREATED" == yes ]]; then
    abs=$(awk '{print $3; exit}' "$WORKDIR/cleanup.last" 2>/dev/null || true)
    if [[ "$abs" != yes || "$HALT" == 1 || "$REAP_UNKNOWN" == 1 || "$M_QUARANTINE" == 1 ]]; then
      EXPORT_STATE=quarantine; ANY_CLEANUP_FAILED=1; add_reason quarantine; EXPORT_DONE=1; M_QUARANTINE=1
      return 0
    fi
  fi
  if ! audit_export; then
    EXPORT_STATE=quarantine; ANY_CLEANUP_FAILED=1; add_reason quarantine; EXPORT_DONE=1
    [[ "$S_STAGE" == M ]] && M_QUARANTINE=1
    return 0
  fi
  if [[ "$M_CASE" == C2 && -f "$EXPORT_DIR/mailbox/boundary" && ! -L "$EXPORT_DIR/mailbox/boundary" ]]; then
    rm -f "$EXPORT_DIR/mailbox/boundary" || true
  fi
  for d in host-write guest-write data evidence; do
    if [[ -d "$EXPORT_DIR/mailbox/$d" && ! -L "$EXPORT_DIR/mailbox/$d" ]]; then
      rmdir "$EXPORT_DIR/mailbox/$d" 2>/dev/null || true
    fi
  done
  rmdir "$EXPORT_DIR/mailbox" 2>/dev/null || true
  rmdir "$EXPORT_DIR" 2>/dev/null || true
  if [[ -d "$EXPORT_DIR" ]]; then EXPORT_STATE=retained; else EXPORT_STATE=removed; fi
  EXPORT_DONE=1
}
record_m_row() {
  local argv_f=$WORKDIR/argv.$M_CASE.$M_BOUNDARY cu_f=$WORKDIR/cleanup.$M_CASE.$M_BOUNDARY rec_f=$WORKDIR/record.$M_CASE.$M_BOUNDARY
  ROW_N=$((ROW_N + 1))
  if [[ "${S11_FAIL_ROW_N:-0}" == "$ROW_N" ]]; then IO_FAIL=1; return 0; fi
  if [[ "$S_ARGV_OK" == 1 ]]; then
    json_list "${RUN_ARGV[@]}" >"$argv_f" || { IO_FAIL=1; return 0; }
  else
    printf '[]\n' >"$argv_f" || { IO_FAIL=1; return 0; }
  fi
  if ! cp "$WORKDIR/cleanup.last" "$cu_f" 2>/dev/null; then
    printf 'not-run not-run not-run\n' >"$cu_f" || { IO_FAIL=1; return 0; }
  fi
  printf '%s\n' "$M_RECORD" >"$rec_f" || { IO_FAIL=1; return 0; }
  printf '%s %s %s %s %s %s %s %s %s\n' "$M_CASE" "$M_BOUNDARY" "$M_VERDICT" "$M_REASON" "$EXPORT_STATE" \
    "${S_NAME:--}" "$S_CREATED" "$S_LAUNCH" "$S_SSH" >>"$WORKDIR/m.rows" || IO_FAIL=1
}
run_m_one() {
  local st=0
  M_VERDICT=unknown; M_REASON=skipped; EXPORT_STATE=absent; EXPORT_DIR=""
  begin_stage M
  if ! prepare_export; then
    M_VERDICT=unknown; M_REASON=io; ANY_COLLECTION_FAILED=1
    record_m_row
    return
  fi
  stage_main M || st=$?
  if [[ "$st" -eq 0 ]]; then
    m_measure || true
    case "$M_VERDICT" in ok|not-ack) ;; *) ANY_COLLECTION_FAILED=1 ;; esac
  else
    M_VERDICT=unknown
    [[ "$M_REASON" == skipped ]] && M_REASON=launch-failed
    [[ "$S_SSH" == not-sole ]] && M_REASON=not-sole
  fi
  cleanup_owned
  cleanup_export
  record_m_row
}
fold_m() {
  local v b any_f=0 any_u=0 any_a=0 any_o=0 agg=unknown n=0 fs=0 ds=0 rn=0 dn=0 sn=0 need=1
  [[ "$M_CASE" == C2 ]] && need=3
  [[ "$M_CASE" == C9 ]] && need=2
  if [[ "$IO_FAIL" == 1 || ! -s "$WORKDIR/m.rows" ]]; then any_u=1; n=-1; fi
  if [[ -f "$WORKDIR/m.rows" ]]; then
    while read -r _ b v _; do
      [[ -n "$v" ]] || continue
      n=$((n + 1))
      case "$b" in file-sync) fs=$((fs + 1)) ;; dir-sync) ds=$((ds + 1)) ;; rename) rn=$((rn + 1)) ;; data) dn=$((dn + 1)) ;; small) sn=$((sn + 1)) ;; esac
      case "$v" in
        failed) any_f=1 ;;
        unknown|not_run) any_u=1 ;;
        not-ack) any_a=1 ;;
        ok) any_o=1 ;;
        *) any_u=1 ;;
      esac
    done <"$WORKDIR/m.rows"
  fi
  [[ "$n" -eq "$need" ]] || any_u=1
  if [[ "$M_CASE" == C2 && ( "$fs" -ne 1 || "$ds" -ne 1 || "$rn" -ne 1 ) ]]; then any_u=1; fi
  if [[ "$M_CASE" == C9 && ( "$dn" -ne 1 || "$sn" -ne 1 ) ]]; then any_u=1; fi
  if [[ "$any_f" == 1 ]]; then agg=failed
  elif [[ "$any_u" == 1 ]]; then agg=unknown
  elif [[ "$any_o" == 1 && "$any_a" == 1 ]]; then agg=unknown
  elif [[ "$any_a" == 1 ]]; then agg=not-ack
  elif [[ "$any_o" == 1 ]]; then agg=ok
  fi
  printf '%s\n' "$agg" >"$WORKDIR/m.agg" || IO_FAIL=1
  case "$agg" in ok|not-ack) ;; *) ANY_COLLECTION_FAILED=1 ;; esac
}
save_stage_m() {
  local agg caseb boundary verd reason estate name created launch ssh argv cu stop del abs first=1 rec tmp
  if [[ "${S11_FAIL_STAGE:-0}" == 1 ]]; then IO_FAIL=1; rm -f "$WORKDIR/stage-M.json"; return 1; fi
  agg=$(cat "$WORKDIR/m.agg" 2>/dev/null || echo unknown)
  tmp=$WORKDIR/stage-M.json.tmp
  {
    printf '{"stage":"M","verdict":"%s","evidence":"DEFAULT-network/mechanics-only","export":"host-private-directory (not the capped HFSX volume)","cases":[' "$agg"
    while read -r caseb boundary verd reason estate name created launch ssh; do
      [[ -n "$caseb" ]] || continue
      [[ "$first" == 1 ]] && first=0 || printf ','
      argv=$(cat "$WORKDIR/argv.$caseb.$boundary" 2>/dev/null || echo '[]')
      cu=$(cat "$WORKDIR/cleanup.$caseb.$boundary" 2>/dev/null || echo "not-run not-run not-run")
      rec=$(cat "$WORKDIR/record.$caseb.$boundary" 2>/dev/null || true)
      case "$rec" in *\"*|*\\*|*$'\n'*) rec="" ;; esac
      set -- $cu; stop=${1:-not-run}; del=${2:-not-run}; abs=${3:-not-run}
      printf '{"case":"%s","boundary":"%s","verdict":"%s","reason":"%s","export_state":"%s","clone":"%s","created":"%s","launch":"%s","ssh":"%s","argv":%s,"record":"%s","cleanup":{"stop":"%s","delete":"%s","absent":"%s"}}' \
        "$caseb" "$boundary" "$verd" "$reason" "$estate" "$name" "$created" "$launch" "$ssh" "$argv" "$rec" "$stop" "$del" "$abs"
    done <"$WORKDIR/m.rows"
    printf ']}\n'
  } >"$tmp" || { IO_FAIL=1; rm -f "$tmp" "$WORKDIR/stage-M.json"; return 1; }
  [[ -s "$tmp" ]] || { IO_FAIL=1; rm -f "$tmp" "$WORKDIR/stage-M.json"; return 1; }
  mv "$tmp" "$WORKDIR/stage-M.json" || { IO_FAIL=1; rm -f "$WORKDIR/stage-M.json"; return 1; }
}
vol_bin_ok() { [[ -n "$1" && -x "$1" && ! -L "$1" ]]; }
vol_tok() {
  case "$1" in ""|*[!A-Za-z0-9._-]*) printf 'unknown' ;; *) printf '%s' "$1" ;; esac
}
vol_num() {
  case "$1" in ""|*[!0-9]*) return 1 ;; esac
  [[ ${#1} -le 12 ]] || return 1
  [[ "$1" == 0 || "$1" != 0* ]] || return 1
}
vol_quarantine() {
  EXPORT_STATE=quarantine; M_QUARANTINE=1; ANY_CLEANUP_FAILED=1; VOL_PHASE=quarantine
  add_reason quarantine
  [[ "$M_REASON" == skipped ]] && M_REASON=quarantine
  [[ -n "$VOL_A" ]] && write_receipt || true
}
vol_run() {
  local secs=$1 left
  shift
  if [[ "$VOL_IN_PREP" == 1 && "$IN_CLEANUP" != 1 && "$VOL_PREP_END" -gt 0 ]]; then
    left=$((VOL_PREP_END - SECONDS))
    if [[ "$left" -lt 1 ]]; then RB_TIMED_OUT=1; RB_STATUS=124; return 2; fi
    [[ "$secs" -gt "$left" ]] && secs=$left
  fi
  run_bounded "$secs" "$WORKDIR/v.out" "$WORKDIR/v.err" - -- "$@" || true
  if [[ "$RB_TIMED_OUT" == 1 || "$RB_OVERFLOW" == 1 || "$RB_ABORTED" == 1 || "$REAP_UNKNOWN" == 1 ]]; then return 2; fi
  [[ "$RB_STATUS" -eq 0 ]]
}
vol_io() {
  local secs=$1
  shift
  if [[ "${S11_TEST_MODE:-}" == 1 && -n "${S11_VOLIO:-}" ]]; then
    vol_run "$secs" "$S11_VOLIO" "$@"
  else
    vol_run "$secs" bash "$0" __volio "$@"
  fi
}
pl_one() {
  local n
  n=$(wc -l <"$WORKDIR/v.out" | tr -d ' ')
  [[ "$n" == 1 ]] || return 1
  IFS= read -r PL_VAL <"$WORKDIR/v.out" || return 1
  [[ -n "$PL_VAL" ]] || return 1
  case "$PL_VAL" in *$'\r'*) return 1 ;; esac
}
pl_get() {
  local st
  PL_VAL=
  vol_run 5 "$PLUTIL" -extract "$1" raw -o - "$2"
  st=$?
  [[ "$st" -eq 0 ]] || return "$st"
  pl_one
}
pl_type() {
  local st
  PL_TYPE=
  vol_run 5 "$PLUTIL" -type "$1" "$2"
  st=$?
  [[ "$st" -eq 0 ]] || return "$st"
  pl_one || return 1
  PL_TYPE=$PL_VAL
}
pl_lint() {
  local st
  vol_run 5 "$PLUTIL" -lint "$1"
  st=$?
  [[ "$st" -eq 0 ]] || return "$st"
  grep -F -q ': OK' "$WORKDIR/v.out"
}
pl_count() {
  local st
  pl_type "$1" "$2"
  st=$?
  [[ "$st" -eq 0 ]] || return "$st"
  [[ "$PL_TYPE" == array ]] || return 1
  pl_get "$1" "$2"
  st=$?
  [[ "$st" -eq 0 ]] || return "$st"
  vol_num "$PL_VAL" || return 1
  [[ "$PL_VAL" -le "$3" ]] || return 1
  PL_COUNT=$PL_VAL
}
pl_maybe() {
  local st
  pl_get "$1" "$2"
  st=$?
  [[ "$st" -eq 2 ]] && return 2
  [[ "$st" -eq 0 ]] && return 0
  PL_VAL=
  return 1
}
norm_disk() {
  local rest
  case "$1" in
    /dev/disk*) rest=${1#/dev/disk} ;;
    disk*) rest=${1#disk} ;;
    *) return 1 ;;
  esac
  vol_num "$rest" || return 1
  printf '%s\n' "/dev/disk$rest"
}
stat_image() {
  local a b c d n st
  vol_run 5 "$STATBIN" -f '%z %b %d %i' "$VOL_I"
  st=$?
  [[ "$st" -eq 0 ]] || return "$st"
  n=$(wc -l <"$WORKDIR/v.out" | tr -d ' ')
  [[ "$n" == 1 ]] || return 1
  read -r a b c d <"$WORKDIR/v.out" || return 1
  vol_num "$a" && vol_num "$b" && vol_num "$c" && vol_num "$d" || return 1
  ST_SIZE=$a; ST_BLOCKS=$b; ST_DEV=$c; ST_INO=$d
}
stat_id() {
  local a b st
  [[ ! -L "$1" ]] || return 1
  vol_run 5 "$STATBIN" -f '%d %i' "$1"
  st=$?
  [[ "$st" -eq 0 ]] || return "$st"
  read -r a b <"$WORKDIR/v.out" || return 1
  vol_num "$a" && vol_num "$b" || return 1
  ST_DEV=$a; ST_INO=$b
}
note_size() {
  local alloc=$1
  VOL_IMAGE=$ST_SIZE; VOL_ALLOC=$alloc
  if [[ "$VOL_IMAGE_PEAK" == unknown || "$ST_SIZE" -gt "$VOL_IMAGE_PEAK" ]]; then VOL_IMAGE_PEAK=$ST_SIZE; fi
  if [[ "$VOL_ALLOC_PEAK" == unknown || "$alloc" -gt "$VOL_ALLOC_PEAK" ]]; then VOL_ALLOC_PEAK=$alloc; fi
  [[ "$ST_SIZE" -gt "$IMAGE_CAP" || "$alloc" -gt "$IMAGE_CAP" ]] && VOL_OVER=1
  VOL_SAMPLES=$((VOL_SAMPLES + 1))
}
df_avail() {
  local tot av st
  DF_AVAIL=0; DF_TOTAL=0
  vol_run 5 "$DFBIN" -k -P "$1"
  st=$?
  [[ "$st" -eq 0 ]] || return "$st"
  awk 'NR==2 {print $2, $4; exit}' "$WORKDIR/v.out" >"$WORKDIR/df.nums" || return 1
  read -r tot av <"$WORKDIR/df.nums" || return 1
  vol_num "$tot" && vol_num "$av" || return 1
  DF_TOTAL=$((tot * 1024)); DF_AVAIL=$((av * 1024))
}
vm_proven_stopped() {
  local abs
  [[ "$HALT" == 1 || "$REAP_UNKNOWN" == 1 ]] && return 1
  [[ "$S_CREATED" != yes ]] && return 0
  abs=$(awk '{print $3; exit}' "$WORKDIR/cleanup.last" 2>/dev/null || true)
  [[ "$abs" == yes ]]
}
mode_writes() {
  local m=$1 g o len
  [[ -z "$m" || "$m" == *[!0-9]* ]] && return 0
  len=${#m}
  [[ "$len" -lt 3 ]] && return 0
  g=${m:$((len - 2)):1}
  o=${m:$((len - 1)):1}
  case "$g" in 2|3|6|7) return 0 ;; esac
  case "$o" in 2|3|6|7) return 0 ;; esac
  return 1
}
# 0 = block. deny entries are kept. allow of namespace mutation is only for owner and root.
acl_blocks() {
  local path=$1 st line first=1 rest qual action perms tok me
  me=$(id -un)
  vol_run 5 "$LSBIN" -lde "$path"
  st=$?
  [[ "$st" -eq 0 && -s "$WORKDIR/v.out" ]] || return 0
  while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ "$first" == 1 ]]; then first=0; continue; fi
    line=${line#"${line%%[![:space:]]*}"}
    [[ -z "$line" ]] && continue
    case "$line" in
      [0-9]:*|[0-9][0-9]:*|[0-9][0-9][0-9]:*) ;;
      *) return 0 ;;
    esac
    rest=${line#*:}
    rest=${rest#"${rest%%[![:space:]]*}"}
    qual=${rest%%[[:space:]]*}
    rest=${rest#"$qual"}
    rest=${rest#"${rest%%[![:space:]]*}"}
    [[ -n "$qual" && -n "$rest" ]] || return 0
    action=""; perms=""
    for tok in $rest; do
      case "$tok" in
        allow|deny)
          [[ -n "$action" ]] && return 0
          action=$tok ;;
        *)
          if [[ -n "$action" ]]; then
            [[ -n "$perms" ]] && return 0
            perms=$tok
          else
            case "$tok" in
              inherited|file_inherit|directory_inherit|limit_inherit|only_inherit) ;;
              *) return 0 ;;
            esac
          fi ;;
      esac
    done
    [[ -n "$action" && -n "$perms" ]] || return 0
    [[ "$action" == deny ]] && continue
    case "$qual" in
      owner|user:root|"user:${me}") continue ;;
    esac
    # Allowlist: another principal may hold only understood read-only rights.
    # Anything else (writesecurity, chown, add/delete, unknown words) blocks.
    local perm rest=$perms
    while [[ -n "$rest" ]]; do
      perm=${rest%%,*}
      [[ "$rest" == *,* ]] && rest=${rest#*,} || rest=
      case "$perm" in
        list|search|read|readattr|readextattr|readsecurity|execute) ;;
        *) return 0 ;;
      esac
    done
  done <"$WORKDIR/v.out"
  return 1
}
out_ancestors_ok() {
  local p owner mode st n=0 uid
  uid=$(id -u)
  p=$OUT
  while [[ "$n" -lt 64 ]]; do
    [[ -L "$p" ]] && return 1
    vol_run 5 "$STATBIN" -f '%u %OLp' "$p"
    st=$?
    [[ "$st" -eq 0 ]] || return "$st"
    read -r owner mode <"$WORKDIR/v.out" || return 1
    [[ "$owner" == "$uid" || "$owner" == 0 ]] || return 1
    mode_writes "$mode" && return 1
    acl_blocks "$p" && return 1
    [[ "$p" == "/" ]] && return 0
    p=$(dirname -- "$p")
    n=$((n + 1))
  done
  return 1
}
write_receipt() {
  local tmp=$OUT/volume-receipt.tmp
  [[ -n "$OUT" && -d "$OUT" && ! -L "$OUT" ]] || return 1
  {
    printf 'phase=%s\n' "$VOL_PHASE"
    printf 'image=%s\n' "$VOL_I"
    printf 'image_dev=%s\n' "$VOL_DEV"
    printf 'image_ino=%s\n' "$VOL_INO"
    printf 'device=%s\n' "$VOL_DISK"
    printf 'mount=%s\n' "$VOL_M"
    printf 'mount_dev=%s\n' "$VOL_M_DEV"
    printf 'mount_ino=%s\n' "$VOL_M_INO"
    printf 'attempt=%s\n' "$VOL_A"
    printf 'attempt_dev=%s\n' "$VOL_A_DEV"
    printf 'attempt_ino=%s\n' "$VOL_A_INO"
    printf 'case=%s\n' "$M_CASE"
    printf 'boundary=%s\n' "$M_BOUNDARY"
  } >"$tmp" || return 1
  mv "$tmp" "$OUT/volume-receipt" || return 1
}
info_copy() {
  vol_run 5 "$HDIUTIL" info -plist || return 2
  cp "$WORKDIR/v.out" "$WORKDIR/info.plist" || return 2
}
info_inventory() {
  local i=0
  INFO_N=0
  info_copy || return
  pl_lint "$WORKDIR/info.plist" || return
  pl_count images "$WORKDIR/info.plist" "$PL_CAP_IMAGES" || return
  INFO_N=$PL_COUNT
  : >"$WORKDIR/info.paths"
  while [[ "$i" -lt "$INFO_N" ]]; do
    pl_get "images.$i.image-path" "$WORKDIR/info.plist" || return
    printf '%s\n' "$PL_VAL" >>"$WORKDIR/info.paths" || return 1
    i=$((i + 1))
  done
}
info_absent() {
  local st
  info_inventory
  st=$?
  [[ "$st" -eq 0 ]] || return "$st"
  [[ "$INFO_N" -eq 0 ]] && return 0
  grep -F -x -q -- "$VOL_I" "$WORKDIR/info.paths" && return 1
  return 0
}
info_holds() {
  local i=0 n=0 idx=0 path j ents dev mp found=0
  info_inventory || return
  while IFS= read -r path; do
    if [[ "$path" == "$VOL_I" ]]; then n=$((n + 1)); idx=$i; fi
    i=$((i + 1))
  done <"$WORKDIR/info.paths"
  [[ "$n" -eq 1 ]] || return 1
  stat_image || return
  [[ "$ST_DEV" == "$VOL_DEV" && "$ST_INO" == "$VOL_INO" ]] || return 1
  pl_count "images.$idx.system-entities" "$WORKDIR/info.plist" "$PL_CAP_ENTS" || return
  ents=$PL_COUNT
  [[ "$ents" -ge 1 ]] || return 1
  j=0
  while [[ "$j" -lt "$ents" ]]; do
    pl_get "images.$idx.system-entities.$j.dev-entry" "$WORKDIR/info.plist" || return
    dev=$(norm_disk "$PL_VAL") || return 1
    pl_get "images.$idx.system-entities.$j.mount-point" "$WORKDIR/info.plist" || return
    mp=$PL_VAL
    if [[ "$mp" == "$VOL_M" ]]; then
      [[ "$dev" == "$VOL_DISK" ]] || return 1
      [[ "$found" -eq 1 ]] && return 1
      found=1
    fi
    j=$((j + 1))
  done
  [[ "$found" -eq 1 ]]
}
map_fsname() {
  case "$1" in
    "Case-sensitive HFS+") VOL_FSNAME=case-sensitive-hfs ;;
    *) VOL_FSNAME=unknown ;;
  esac
}
parse_attach() {
  local dev st
  pl_lint "$WORKDIR/attach.plist" || return 1
  pl_count system-entities "$WORKDIR/attach.plist" "$PL_CAP_ENTS" || return 1
  [[ "$PL_COUNT" -eq 1 ]] || return 1
  pl_get "system-entities.0.dev-entry" "$WORKDIR/attach.plist" || return 1
  dev=$(norm_disk "$PL_VAL") || return 1
  pl_get "system-entities.0.mount-point" "$WORKDIR/attach.plist" || return 1
  [[ "$PL_VAL" == "$VOL_M" ]] || return 1
  VOL_DISK=$dev
  pl_maybe "system-entities.0.content-hint" "$WORKDIR/attach.plist"; st=$?
  [[ "$st" -eq 2 ]] && return 1
  [[ "$st" -eq 0 ]] && VOL_HINT=$(vol_tok "$PL_VAL")
  pl_maybe "system-entities.0.filesystem-type" "$WORKDIR/attach.plist"; st=$?
  [[ "$st" -eq 2 ]] && return 1
  [[ "$st" -eq 0 ]] && VOL_FSTYPE=$(vol_tok "$PL_VAL")
  pl_maybe "system-entities.0.filesystem-name" "$WORKDIR/attach.plist"; st=$?
  [[ "$st" -eq 2 ]] && return 1
  if [[ "$st" -eq 0 ]]; then map_fsname "$PL_VAL"; else VOL_FSNAME=unknown; fi
  pl_maybe "system-entities.0.size" "$WORKDIR/attach.plist"; st=$?
  [[ "$st" -eq 2 ]] && return 1
  if [[ "$st" -eq 0 ]] && vol_num "$PL_VAL"; then VOL_CAPACITY=$PL_VAL; fi
}
flag_in() {
  case ",${1// /}," in *",$2,"*) return 0 ;; esac
  return 1
}
read_mount() {
  local line dev rest opts fst hit=0 st
  vol_run 5 "$MOUNTBIN"
  st=$?
  [[ "$st" -eq 0 ]] || return 2
  VOL_NOSUID=no; VOL_NOEXEC=no; VOL_NODEV=no; VOL_NOBROWSE=no; VOL_OWNERS=unknown
  while IFS= read -r line; do
    dev=${line%% on *}
    rest=${line#* on }
    case "$rest" in
      "$VOL_M ("*) ;;
      *) continue ;;
    esac
    dev=$(norm_disk "$dev") || return 1
    [[ "$dev" == "$VOL_DISK" ]] || return 1
    opts=${rest#"$VOL_M ("}
    opts=${opts%)}
    fst=${opts%%,*}
    VOL_FSTYPE=$(vol_tok "$fst")
    VOL_OWNERS=yes
    flag_in "$opts" noowners && VOL_OWNERS=unknown
    flag_in "$opts" nosuid && VOL_NOSUID=yes
    flag_in "$opts" noexec && VOL_NOEXEC=yes
    flag_in "$opts" nodev && VOL_NODEV=yes
    flag_in "$opts" nobrowse && VOL_NOBROWSE=yes
    hit=1
  done <"$WORKDIR/v.out"
  [[ "$hit" == 1 ]] || return 2
  if [[ "$VOL_NOSUID" != yes || "$VOL_NOEXEC" != yes || "$VOL_NODEV" != yes || "$VOL_NOBROWSE" != yes ]]; then
    return 3
  fi
}
note_mdutil() {
  local line word seen=""
  VOL_MDUTIL=unknown
  vol_bin_ok "$MDUTIL" || return 0
  vol_run 5 "$MDUTIL" -s "$VOL_M" || return 0
  while IFS= read -r line; do
    line=${line#"${line%%[![:space:]]*}"}
    case "$line" in
      "Indexing disabled.") word=disabled ;;
      "Indexing enabled.") word=enabled ;;
      *) continue ;;
    esac
    if [[ -n "$seen" && "$seen" != "$word" ]]; then VOL_MDUTIL=unknown; return 0; fi
    seen=$word
  done <"$WORKDIR/v.out"
  [[ -n "$seen" ]] && VOL_MDUTIL=$seen
}
note_tmutil() {
  local line status path n
  VOL_TMUTIL=unknown
  vol_bin_ok "$TMUTIL" || return 0
  vol_run 5 "$TMUTIL" isexcluded "$VOL_I" || return 0
  n=$(wc -l <"$WORKDIR/v.out" | tr -d ' ')
  [[ "$n" == 1 ]] || return 0
  IFS= read -r line <"$WORKDIR/v.out" || return 0
  case "$line" in
    "[Excluded]"*) status=excluded ;;
    "[Included]"*) status=included ;;
    *) return 0 ;;
  esac
  path=${line#*]}
  path=${path#"${path%%[![:space:]]*}"}
  [[ "$path" == "$VOL_I" ]] || return 0
  VOL_TMUTIL=$status
}
note_create_warn() {
  local mark
  VOL_CREATE_WARN=absent
  [[ -s "$WORKDIR/v.err" ]] || return 0
  mark="'hdiutil create ...' is deprecated. Please use 'diskutil image create ...'"
  if grep -F -q -- "$mark" "$WORKDIR/v.err"; then VOL_CREATE_WARN=deprecated; return 0; fi
  VOL_CREATE_WARN=unknown
}
id_remove() {
  local st
  vol_io 5 remove "$@"
  st=$?
  if [[ "$st" -eq 2 ]]; then VOL_MAYBE=1; return 2; fi
  [[ "$st" -eq 0 ]]
}
drop_empty() {
  if [[ -n "$VOL_M_DEV" ]]; then
    id_remove dir "$VOL_A" "$VOL_A_DEV" "$VOL_A_INO" "$VOL_M" "$VOL_M_DEV" "$VOL_M_INO" || return $?
  elif [[ -n "$VOL_M" ]]; then
    vol_quarantine
    return 1
  fi
  if [[ -n "$VOL_A_DEV" ]]; then
    id_remove dir "$OUT" "$VOL_OUT_DEV" "$VOL_OUT_INO" "$VOL_A" "$VOL_A_DEV" "$VOL_A_INO" || return $?
  elif [[ -n "$VOL_A" ]]; then
    vol_quarantine
    return 1
  fi
  VOL_A=""; VOL_M=""; VOL_PHASE=none
}
unlink_image() {
  id_remove file "$VOL_A" "$VOL_A_DEV" "$VOL_A_INO" "$VOL_I" "$VOL_DEV" "$VOL_INO" || return $?
  id_remove dir "$VOL_A" "$VOL_A_DEV" "$VOL_A_INO" "$VOL_M" "$VOL_M_DEV" "$VOL_M_INO" || return $?
  id_remove dir "$OUT" "$VOL_OUT_DEV" "$VOL_OUT_INO" "$VOL_A" "$VOL_A_DEV" "$VOL_A_INO" || return $?
  EXPORT_STATE=removed; VOL_PHASE=gone
  write_receipt || true
}
reset_vol() {
  VOL_A=""; VOL_I=""; VOL_M=""; VOL_DEV=""; VOL_INO=""; VOL_DISK=""
  VOL_A_DEV=""; VOL_A_INO=""; VOL_M_DEV=""; VOL_M_INO=""
  VOL_PHASE=none; VOL_MAYBE=0; VOL_ID_BROKEN=0; VOL_FORCE=0; VOL_OPT_REJECT=0; VOL_OVER=0
  VOL_IN_PREP=0; VOL_PREP_END=0
  VOL_CREATE_SEC=-1; VOL_ATTACH_SEC=-1; VOL_DETACH_SEC=-1
  VOL_FSTYPE=unknown; VOL_FSNAME=unknown; VOL_HINT=unknown; VOL_CAPACITY=unknown
  VOL_NOSUID=unknown; VOL_NOEXEC=unknown; VOL_NODEV=unknown; VOL_NOBROWSE=unknown
  VOL_OWNERS=unknown; VOL_CREATE_WARN=absent
  VOL_MDUTIL=unknown; VOL_TMUTIL=unknown
  VOL_IMAGE=unknown; VOL_ALLOC=unknown; VOL_HOST_DROP=unknown; VOL_ENOSPC=unknown; VOL_VOLFREE=unknown
  VOL_IMAGE_PEAK=unknown; VOL_ALLOC_PEAK=unknown; VOL_HOST_DROP_PEAK=unknown; VOL_SAMPLES=0
  VOL_BASE_ALLOC=0; VOL_BASE_HOST=0; VOL_HAVE_BASE=0
  VOL_HOST_PRE=unknown; VOL_HOST_FILL=unknown
  EXPORT_DIR=""; EXPORT_STATE=absent; EXPORT_DONE=0
}
volume_up() {
  local t0 st alloc ist=0
  if ! vol_bin_ok "$HDIUTIL" || ! vol_bin_ok "$DISKUTIL" || ! vol_bin_ok "$PLUTIL" || ! vol_bin_ok "$MOUNTBIN" || ! vol_bin_ok "$STATBIN" || ! vol_bin_ok "$DFBIN" || ! vol_bin_ok "$LSBIN"; then
    M_VERDICT=unknown; M_REASON=tool-missing; ANY_COLLECTION_FAILED=1
    return 1
  fi
  VOL_IN_PREP=1
  VOL_PREP_END=$((SECONDS + VOL_PREP_BUDGET))
  [[ "$PROBE_DEADLINE" -gt 0 && "$VOL_PREP_END" -gt "$PROBE_DEADLINE" ]] && VOL_PREP_END=$PROBE_DEADLINE
  if ! out_ancestors_ok; then
    M_VERDICT=unknown; M_REASON=out-unsafe; ANY_COLLECTION_FAILED=1; VOL_IN_PREP=0
    return 1
  fi
  if ! df_avail "$OUT"; then
    M_VERDICT=unknown; M_REASON=host-space; ANY_COLLECTION_FAILED=1; VOL_IN_PREP=0
    return 1
  fi
  VOL_HOST_PRE=$DF_AVAIL
  if [[ "$DF_AVAIL" -lt "$HOST_RESERVE" ]]; then
    M_VERDICT=unknown; M_REASON=host-space; ANY_COLLECTION_FAILED=1; VOL_IN_PREP=0
    return 1
  fi
  stat_id "$OUT" || { M_VERDICT=unknown; M_REASON=out-unsafe; ANY_COLLECTION_FAILED=1; VOL_IN_PREP=0; return 1; }
  VOL_OUT_DEV=$ST_DEV; VOL_OUT_INO=$ST_INO
  VOL_A=$(mktemp -d "$OUT/s11v.XXXXXX") || { M_VERDICT=unknown; M_REASON=io; ANY_COLLECTION_FAILED=1; return 1; }
  chmod 0700 "$VOL_A" || { vol_quarantine; return 4; }
  if acl_blocks "$VOL_A"; then M_REASON=out-unsafe; vol_quarantine; return 4; fi
  stat_id "$VOL_A" || { vol_quarantine; return 4; }
  VOL_A_DEV=$ST_DEV; VOL_A_INO=$ST_INO
  VOL_M=$(mktemp -d "$VOL_A/mnt.XXXXXX") || { vol_quarantine; return 4; }
  chmod 0700 "$VOL_M" || { vol_quarantine; return 4; }
  if acl_blocks "$VOL_M"; then M_REASON=out-unsafe; vol_quarantine; return 4; fi
  stat_id "$VOL_M" || { vol_quarantine; return 4; }
  VOL_M_DEV=$ST_DEV; VOL_M_INO=$ST_INO
  VOL_I=$VOL_A/mailbox.sparseimage
  VOL_PHASE=reserved
  write_receipt || { vol_quarantine; return 4; }
  t0=$SECONDS
  vol_run 20 "$HDIUTIL" create -size 64m -fs HFSX -type SPARSE -layout NONE -volname GCU-MAILBOX "$VOL_I"
  st=$?
  VOL_CREATE_SEC=$((SECONDS - t0))
  if [[ "$st" -eq 2 ]]; then VOL_MAYBE=1; vol_quarantine; return 4; fi
  if [[ "$st" -ne 0 ]]; then
    if [[ -e "$VOL_I" || -L "$VOL_I" ]]; then vol_quarantine; return 4; fi
    M_VERDICT=unknown; M_REASON=create-failed; ANY_CREATE_FAILED=1
    if ! drop_empty; then vol_quarantine; return 4; fi
    return 1
  fi
  note_create_warn
  if [[ -L "$VOL_I" || ! -f "$VOL_I" ]]; then vol_quarantine; return 4; fi
  if ! stat_image; then vol_quarantine; return 4; fi
  VOL_DEV=$ST_DEV; VOL_INO=$ST_INO; VOL_PHASE=created
  alloc=$((ST_BLOCKS * 512))
  note_size "$alloc"
  write_receipt || { vol_quarantine; return 4; }
  if [[ "$VOL_OVER" == 1 ]]; then
    M_VERDICT=failed; M_REASON=over-cap; ANY_COLLECTION_FAILED=1
    return 3
  fi
  t0=$SECONDS
  vol_run 10 "$DISKUTIL" image attach --plist --nobrowse --mountPoint "$VOL_M" --mountOptions nosuid,noexec,nodev,owners "$VOL_I"
  st=$?
  VOL_ATTACH_SEC=$((SECONDS - t0))
  if [[ "$st" -eq 2 ]]; then VOL_MAYBE=1; vol_quarantine; return 4; fi
  if [[ "$st" -ne 0 ]]; then
    info_absent || ist=$?
    if [[ "$ist" -eq 0 ]] && unlink_image; then
      M_VERDICT=unknown; M_REASON=attach-failed; ANY_COLLECTION_FAILED=1
      return 1
    fi
    [[ "$ist" -eq 2 ]] && VOL_MAYBE=1
    vol_quarantine
    return 4
  fi
  cp "$WORKDIR/v.out" "$WORKDIR/attach.plist" || { VOL_MAYBE=1; vol_quarantine; return 4; }
  if ! parse_attach; then VOL_MAYBE=1; vol_quarantine; return 4; fi
  if ! info_holds; then VOL_MAYBE=1; vol_quarantine; return 4; fi
  read_mount
  st=$?
  if [[ "$st" -eq 3 ]]; then VOL_OPT_REJECT=1
  elif [[ "$st" -ne 0 ]]; then VOL_MAYBE=1; vol_quarantine; return 4; fi
  note_mdutil; note_tmutil
  vol_io 5 write "$VOL_M/.metadata_never_index"
  st=$?
  if [[ "$st" -eq 2 ]]; then VOL_MAYBE=1; vol_quarantine; return 4; fi
  [[ "$st" -eq 0 ]] || VOL_OPT_REJECT=1
  VOL_PHASE=mounted
  write_receipt || true
  VOL_IN_PREP=0
  if [[ "$VOL_OPT_REJECT" == 1 ]]; then
    M_VERDICT=unknown; M_REASON=option-rejected
    return 3
  fi
}
volume_down() {
  local st t0
  VOL_IN_PREP=0
  if [[ "$VOL_PHASE" == gone ]]; then return 0; fi
  if [[ "$VOL_PHASE" == none ]]; then
    if [[ -n "$VOL_A" ]]; then drop_empty || true; fi
    return 0
  fi
  if [[ "$VOL_PHASE" == quarantine || "$VOL_MAYBE" == 1 || "$VOL_ID_BROKEN" == 1 ]]; then
    vol_quarantine
    return 0
  fi
  if [[ "$VOL_PHASE" == mounted || "$VOL_PHASE" == reserved ]]; then
    if [[ "$VOL_PHASE" == reserved ]]; then
      drop_empty || vol_quarantine
      return 0
    fi
    if ! vm_proven_stopped; then vol_quarantine; return 0; fi
    if ! info_holds; then vol_quarantine; return 0; fi
    t0=$SECONDS
    # Bind detach to this attempt's mount directory, not a reusable /dev/diskN.
    vol_run 5 "$HDIUTIL" detach "$VOL_M"
    st=$?
    VOL_DETACH_SEC=$((SECONDS - t0))
    if [[ "$st" -eq 2 ]]; then vol_quarantine; return 0; fi
    if [[ "$st" -ne 0 ]]; then
      if vm_proven_stopped && info_holds; then
        vol_run 5 "$HDIUTIL" detach -force "$VOL_M"
        st=$?
        if [[ "$st" -eq 2 ]]; then VOL_MAYBE=1; vol_quarantine; return 0; fi
        if [[ "$st" -ne 0 ]]; then vol_quarantine; return 0; fi
        VOL_FORCE=1
      else
        vol_quarantine
        return 0
      fi
    fi
    if ! info_absent; then vol_quarantine; return 0; fi
    VOL_PHASE=created
    write_receipt || true
  fi
  if [[ "$VOL_PHASE" == created ]]; then
    if [[ "$VOL_ID_BROKEN" == 1 ]]; then vol_quarantine; return 0; fi
    unlink_image || vol_quarantine
  fi
}
measure_now() {
  local host alloc now
  stat_image || return 1
  [[ "$ST_DEV" == "$VOL_DEV" && "$ST_INO" == "$VOL_INO" ]] || { VOL_ID_BROKEN=1; return 1; }
  alloc=$((ST_BLOCKS * 512))
  note_size "$alloc"
  df_avail "$VOL_M" || return 1
  VOL_VOLFREE=$DF_AVAIL
  [[ "$VOL_CAPACITY" == unknown ]] && VOL_CAPACITY=$DF_TOTAL
  df_avail "$VOL_A" || return 1
  host=$DF_AVAIL
  if [[ "$1" == base ]]; then
    VOL_BASE_ALLOC=$alloc; VOL_BASE_HOST=$host; VOL_HAVE_BASE=1; VOL_HOST_FILL=$host
    VOL_HOST_DROP_PEAK=0; VOL_HOST_DROP=0
    return 0
  fi
  if [[ "$VOL_HAVE_BASE" == 1 && "$host" -le "$VOL_BASE_HOST" ]]; then
    now=$((VOL_BASE_HOST - host))
    if [[ "$VOL_HOST_DROP_PEAK" == unknown || "$now" -gt "$VOL_HOST_DROP_PEAK" ]]; then VOL_HOST_DROP_PEAK=$now; fi
  fi
  [[ "$VOL_HOST_DROP_PEAK" != unknown ]] && VOL_HOST_DROP=$VOL_HOST_DROP_PEAK
}
bounded_read() {
  local secs=$1 file=$2 cap=$3 left
  if [[ "$M_LOAD_END" -gt "$SECONDS" ]]; then
    left=$((M_LOAD_END - SECONDS))
    [[ "$secs" -gt "$left" ]] && secs=$left
  fi
  run_bounded "$secs" "$WORKDIR/v.out" "$WORKDIR/v.err" - -- bash -c 'set -f; set -u; f=$1; cap=$2; if [[ -L "$f" || ! -f "$f" ]]; then exit 3; fi; head -c "$cap" -- "$f"' bash "$file" "$cap" || true
  if [[ "$RB_TIMED_OUT" == 1 || "$RB_OVERFLOW" == 1 || "$RB_ABORTED" == 1 || "$REAP_UNKNOWN" == 1 ]]; then return 2; fi
  [[ "$RB_STATUS" -eq 0 ]]
}
read_progress() {
  local f=$EXPORT_DIR/mailbox/progress line n tok key val st
  PROG_SEQ=0
  bounded_read 5 "$f" 512
  st=$?
  [[ "$st" -eq 0 ]] || return "$st"
  n=$(wc -l <"$WORKDIR/v.out" | tr -d ' ')
  [[ "$n" == 1 ]] || return 1
  IFS= read -r line <"$WORKDIR/v.out" || return 1
  for tok in $line; do
    key=${tok%%=*}; val=${tok#*=}
    [[ "$key" == seq ]] && PROG_SEQ=$val
  done
  vol_num "$PROG_SEQ"
}
decode_c9() {
  local line=$1 tok key val seen=" " nseen=0
  local v= op= role= mode= en= bytes= files= batches=
  for tok in $line; do
    case "$tok" in
      *=*) key=${tok%%=*}; val=${tok#*=} ;;
      *) return 1 ;;
    esac
    [[ -n "$key" && -n "$val" ]] || return 1
    case "$seen" in *" $key "*) return 1 ;; esac
    seen="$seen$key "
    nseen=$((nseen + 1))
    case "$key" in
      v) v=$val ;; op) op=$val ;; role) role=$val ;; mode) mode=$val ;;
      enospc) en=$val ;; bytes) bytes=$val ;; files) files=$val ;; batches) batches=$val ;;
      *) return 1 ;;
    esac
  done
  [[ "$nseen" -eq 8 ]] || return 1
  [[ "$v" == 1 && "$op" == c9 && "$role" == guest && "$mode" == "$M_BOUNDARY" ]] || return 1
  [[ "$en" == 0 || "$en" == 1 ]] || return 1
  vol_num "$bytes" && vol_num "$files" && vol_num "$batches" || return 1
  C9_ENOSPC=$en; M_RECORD=$line
}
read_c9() {
  local f=$EXPORT_DIR/mailbox/result-guest line bytes n
  C9_ENOSPC=
  bounded_read 5 "$f" 4097 || return $?
  bytes=$(wc -c <"$WORKDIR/v.out" | tr -d ' ')
  [[ "$bytes" -ge 1 && "$bytes" -le 4096 ]] || return 1
  n=$(wc -l <"$WORKDIR/v.out" | tr -d ' ')
  [[ "$n" == 1 ]] || return 1
  # Command substitution drops a trailing newline, so keep a marker byte after it.
  marker=$(tail -c 1 "$WORKDIR/v.out"; printf x)
  [[ "$marker" == $'\nx' ]] || return 1
  IFS= read -r line <"$WORKDIR/v.out" || return 1
  printf '%s\n' "$line" >"$WORKDIR/c9.one" || return 1
  cmp -s "$WORKDIR/c9.one" "$WORKDIR/v.out" || return 1
  decode_c9 "$line"
}
judge_c9() {
  local limit
  if [[ "$VOL_ID_BROKEN" == 1 ]] || ! measure_now final; then
    if [[ "$VOL_OVER" == 1 ]]; then
      M_VERDICT=failed; M_REASON=over-cap; ANY_COLLECTION_FAILED=1
      return 0
    fi
    M_VERDICT=unknown; M_REASON=incomplete; ANY_COLLECTION_FAILED=1
    return 1
  fi
  if ! read_c9; then
    M_VERDICT=unknown; M_REASON=probe-failed; ANY_COLLECTION_FAILED=1
    return 1
  fi
  if [[ "$VOL_OVER" == 1 || "$VOL_IMAGE_PEAK" == unknown || "$VOL_ALLOC_PEAK" == unknown || "$VOL_IMAGE_PEAK" -gt "$IMAGE_CAP" || "$VOL_ALLOC_PEAK" -gt "$IMAGE_CAP" ]]; then
    M_VERDICT=failed; M_REASON=over-cap; ANY_COLLECTION_FAILED=1
    return 0
  fi
  if [[ "$C9_ENOSPC" != 1 ]]; then
    M_VERDICT=unknown; M_REASON=enospc-absent; VOL_ENOSPC=0; ANY_COLLECTION_FAILED=1
    return 0
  fi
  VOL_ENOSPC=1
  if [[ "$VOL_VOLFREE" == unknown || "$VOL_VOLFREE" -gt "$VOL_FULL_MAX" ]]; then
    M_VERDICT=unknown; M_REASON=enospc-origin; ANY_COLLECTION_FAILED=1
    return 0
  fi
  if [[ "$VOL_ALLOC_PEAK" -ge "$VOL_BASE_ALLOC" ]]; then limit=$((VOL_ALLOC_PEAK - VOL_BASE_ALLOC + HOST_MARGIN))
  else limit=$HOST_MARGIN; fi
  if [[ "$VOL_HOST_DROP_PEAK" == unknown || "$VOL_HOST_DROP_PEAK" -gt "$limit" ]]; then
    M_VERDICT=failed; M_REASON=host-grew; ANY_COLLECTION_FAILED=1
    return 0
  fi
  M_VERDICT=ok; M_REASON=-
}
m_run_c9() {
  local gpid last=0 st
  clear_snaps; arm_m_load
  if ! measure_now base; then M_VERDICT=unknown; M_REASON=incomplete; ANY_COLLECTION_FAILED=1; return 1; fi
  if [[ "$VOL_BASE_HOST" -lt "$HOST_RESERVE" ]]; then
    M_VERDICT=unknown; M_REASON=host-space; ANY_COLLECTION_FAILED=1
    return 1
  fi
  ssh_bg "$ONCE_KEY" "run C9" || { M_VERDICT=unknown; M_REASON=not-sole; return 1; }
  gpid=$SSH_BG_PID
  while [[ "$SECONDS" -lt "$M_LOAD_END" ]] && job_running "$gpid"; do
    note_ssh_bg "$gpid" && break
    read_progress
    st=$?
    if [[ "$st" -eq 2 ]]; then
      # Controller cancel aborted this read. That is not an uncertain volume syscall.
      if [[ "$REQUEST_CLEANUP" == 1 ]]; then SSH_BG_AB=1; break; fi
      VOL_MAYBE=1; vol_quarantine; break
    fi
    if [[ "$st" -eq 0 && "$PROG_SEQ" -gt "$last" ]]; then
      measure_now sample || VOL_ID_BROKEN=1
      last=$PROG_SEQ
    fi
    [[ "$VOL_ID_BROKEN" == 1 ]] && break
    sleep 0.05
  done
  wait_guest "$gpid"
  if [[ "$VOL_ID_BROKEN" == 1 || "$VOL_PHASE" == quarantine || "$VOL_MAYBE" == 1 ]]; then
    VOL_MAYBE=1; vol_quarantine
    M_VERDICT=unknown; M_REASON=quarantine; ANY_COLLECTION_FAILED=1
    return 1
  fi
  st=0
  read_c9 || st=$?
  if [[ "${st:-0}" -eq 2 && "$G_AB" != 1 && "$G_OV" != 1 && "$REQUEST_CLEANUP" != 1 ]]; then
    VOL_MAYBE=1; vol_quarantine
    M_VERDICT=unknown; M_REASON=quarantine; ANY_COLLECTION_FAILED=1
    return 1
  fi
  if [[ "$G_TO" == 1 || "$G_RU" == 1 ]]; then
    M_VERDICT=unknown; M_REASON=deadline; ANY_COLLECTION_FAILED=1
    return 1
  fi
  if [[ "$G_AB" == 1 || "$G_OV" == 1 || "$G_ST" != 0 || "$REQUEST_CLEANUP" == 1 ]]; then
    M_VERDICT=unknown; M_REASON=probe-failed; ANY_COLLECTION_FAILED=1
    return 1
  fi
  judge_c9 || true
}
write_vol_meta() {
  local f=$WORKDIR/vol.$M_CASE.$M_BOUNDARY
  printf '%s %s %s %s %s %s %s %s %s %s %s %s %s %s %s %s %s %s %s %s %s %s %s %s %s %s %s\n' \
    "$(vol_tok "$VOL_FSTYPE")" "$(vol_tok "$VOL_HINT")" "$(vol_tok "$VOL_CAPACITY")" \
    "$(vol_tok "$VOL_NOSUID")" "$(vol_tok "$VOL_NOEXEC")" "$(vol_tok "$VOL_NODEV")" "$(vol_tok "$VOL_NOBROWSE")" \
    "$(vol_tok "$VOL_MDUTIL")" "$(vol_tok "$VOL_TMUTIL")" \
    "$(vol_tok "$VOL_CREATE_SEC")" "$(vol_tok "$VOL_ATTACH_SEC")" "$(vol_tok "$VOL_DETACH_SEC")" \
    "$(vol_tok "$VOL_FORCE")" "$(vol_tok "$VOL_IMAGE")" "$(vol_tok "$VOL_ALLOC")" "$(vol_tok "$VOL_HOST_DROP")" \
    "$(vol_tok "$VOL_ENOSPC")" "$(vol_tok "$VOL_VOLFREE")" \
    "$(vol_tok "$VOL_OWNERS")" "$(vol_tok "$VOL_CREATE_WARN")" \
    "$(vol_tok "$VOL_FSNAME")" "$(vol_tok "$VOL_IMAGE_PEAK")" "$(vol_tok "$VOL_ALLOC_PEAK")" \
    "$(vol_tok "$HOST_MARGIN")" "$(vol_tok "$VOL_SAMPLES")" \
    "$(vol_tok "$VOL_HOST_PRE")" "$(vol_tok "$VOL_HOST_FILL")" >"$f" || IO_FAIL=1
}
finish_c4() {
  [[ "$M_CASE" == C4 ]] || return 0
  if [[ "$M_QUARANTINE" == 1 || "$EXPORT_STATE" == quarantine ]]; then
    M_VERDICT=unknown; M_REASON=quarantine
  elif [[ "$VOL_OVER" == 1 ]]; then
    M_VERDICT=failed; M_REASON=over-cap; ANY_COLLECTION_FAILED=1
  elif [[ "$VOL_OPT_REJECT" == 1 && "$EXPORT_STATE" == removed ]]; then
    M_VERDICT=unknown; M_REASON=option-rejected; ANY_COLLECTION_FAILED=1
  elif [[ "$EXPORT_STATE" == removed && "$VOL_PHASE" == gone ]]; then
    if [[ "$VOL_FSNAME" != case-sensitive-hfs || "$VOL_CAPACITY" == unknown || "$VOL_IMAGE" == unknown || "$VOL_ALLOC" == unknown ]]; then
      M_VERDICT=unknown; M_REASON=incomplete; ANY_COLLECTION_FAILED=1
    elif [[ "$VOL_IMAGE" -gt "$IMAGE_CAP" || "$VOL_ALLOC" -gt "$IMAGE_CAP" ]]; then
      M_VERDICT=failed; M_REASON=over-cap; ANY_COLLECTION_FAILED=1
    else
      M_VERDICT=ok; M_REASON=volume-ok
    fi
  elif [[ "$M_REASON" == create-failed || "$M_REASON" == tool-missing || "$M_REASON" == attach-failed || "$M_REASON" == host-space || "$M_REASON" == out-unsafe ]]; then
    M_VERDICT=unknown
  elif [[ "$M_REASON" == skipped ]]; then
    M_VERDICT=unknown; M_REASON=incomplete; ANY_COLLECTION_FAILED=1
  fi
  M_RECORD="v=1 op=c4 fstype=${VOL_FSTYPE} fsname=${VOL_FSNAME} hint=${VOL_HINT} capacity=${VOL_CAPACITY} nosuid=${VOL_NOSUID} noexec=${VOL_NOEXEC} nodev=${VOL_NODEV} nobrowse=${VOL_NOBROWSE} owners=${VOL_OWNERS} mdutil=${VOL_MDUTIL} tmutil=${VOL_TMUTIL} create=${VOL_CREATE_WARN} force=${VOL_FORCE}"
}
run_v_one() {
  local st=0 vrc=0
  M_VERDICT=unknown; M_REASON=skipped; EXPORT_STATE=absent; M_RECORD=""
  begin_stage V
  reset_vol
  volume_up || vrc=$?
  VOL_IN_PREP=0
  if [[ "$vrc" -eq 0 && "$M_CASE" != C4 ]]; then
    if prepare_export; then
      stage_main V || st=$?
      if [[ "$st" -eq 0 ]]; then
        m_measure || true
        case "$M_VERDICT" in ok|not-ack) ;; *) ANY_COLLECTION_FAILED=1 ;; esac
      else
        M_VERDICT=unknown
        [[ "$M_REASON" == skipped ]] && M_REASON=launch-failed
        [[ "$S_SSH" == not-sole ]] && M_REASON=not-sole
        ANY_COLLECTION_FAILED=1
      fi
    else
      [[ "$M_QUARANTINE" == 1 ]] || { M_VERDICT=unknown; M_REASON=io; ANY_COLLECTION_FAILED=1; }
    fi
  fi
  cleanup_owned
  cleanup_export
  finish_c4
  write_vol_meta
  record_m_row
}
run_volume() {
  : >"$WORKDIR/m.rows"
  if [[ "$M_CASE" == C4 ]]; then printf '%s\n' volume-only >"$WORKDIR/v.evidence"
  else printf '%s\n' 'DEFAULT-network/mechanics-only' >"$WORKDIR/v.evidence"; fi
  case "$M_CASE" in
    C2)
      for M_BOUNDARY in file-sync dir-sync rename; do
        if ! can_advance; then
          begin_stage V; reset_vol
          M_VERDICT=not_run; M_REASON=skipped; EXPORT_STATE=absent; S_ARGV_OK=0; M_RECORD=""
          record_m_row
          continue
        fi
        run_v_one
      done
      ;;
    C9)
      for M_BOUNDARY in data small; do
        if ! can_advance; then
          begin_stage V; reset_vol
          M_VERDICT=not_run; M_REASON=skipped; EXPORT_STATE=absent; S_ARGV_OK=0; M_RECORD=""
          record_m_row
          continue
        fi
        run_v_one
      done
      ;;
    *)
      M_BOUNDARY=-
      run_v_one
      ;;
  esac
  fold_m
  save_stage_v
}
save_stage_v() {
  local agg caseb boundary verd reason estate name created launch ssh argv cu stop del abs first=1 rec tmp
  local fstype hint cap nosuid noexec nodev nobrowse mdutil tmutil csec asec dsec force ibytes ialloc hdrop enospc vfree owners cwarn ev
  local fsname ibpeak iapeak hmargin samples host_pre host_fill
  if [[ "${S11_FAIL_STAGE:-0}" == 1 ]]; then IO_FAIL=1; rm -f "$WORKDIR/stage-V.json"; return 1; fi
  agg=$(cat "$WORKDIR/m.agg" 2>/dev/null || echo unknown)
  ev=$(cat "$WORKDIR/v.evidence" 2>/dev/null || echo volume-only)
  case "$ev" in volume-only|DEFAULT-network/mechanics-only) ;; *) ev=volume-only ;; esac
  tmp=$WORKDIR/stage-V.json.tmp
  {
    printf '{"stage":"V","verdict":"%s","evidence":"%s","export":"capped HFSX volume","cases":[' "$agg" "$ev"
    while read -r caseb boundary verd reason estate name created launch ssh; do
      [[ -n "$caseb" ]] || continue
      [[ "$first" == 1 ]] && first=0 || printf ','
      argv=$(cat "$WORKDIR/argv.$caseb.$boundary" 2>/dev/null || echo '[]')
      cu=$(cat "$WORKDIR/cleanup.$caseb.$boundary" 2>/dev/null || echo "not-run not-run not-run")
      rec=$(cat "$WORKDIR/record.$caseb.$boundary" 2>/dev/null || true)
      case "$rec" in *\"*|*\\*|*$'\n'*) rec="" ;; esac
      set -- $cu; stop=${1:-not-run}; del=${2:-not-run}; abs=${3:-not-run}
      fstype=unknown; hint=unknown; cap=unknown; nosuid=unknown; noexec=unknown; nodev=unknown; nobrowse=unknown
      mdutil=unknown; tmutil=unknown; csec=-1; asec=-1; dsec=-1; force=0
      ibytes=unknown; ialloc=unknown; hdrop=unknown; enospc=unknown; vfree=unknown; owners=unknown; cwarn=absent
      fsname=unknown; ibpeak=unknown; iapeak=unknown; hmargin=$HOST_MARGIN; samples=0; host_pre=unknown; host_fill=unknown
      if [[ -f "$WORKDIR/vol.$caseb.$boundary" ]]; then
        read -r fstype hint cap nosuid noexec nodev nobrowse mdutil tmutil csec asec dsec force ibytes ialloc hdrop enospc vfree owners cwarn fsname ibpeak iapeak hmargin samples host_pre host_fill <"$WORKDIR/vol.$caseb.$boundary"
      fi
      printf '{"case":"%s","boundary":"%s","verdict":"%s","reason":"%s","export_state":"%s","clone":"%s","created":"%s","launch":"%s","ssh":"%s","argv":%s,"record":"%s","cleanup":{"stop":"%s","delete":"%s","absent":"%s"},"volume":{"fstype":"%s","fsname":"%s","hint":"%s","capacity":"%s","nosuid":"%s","noexec":"%s","nodev":"%s","nobrowse":"%s","owners":"%s","mdutil":"%s","tmutil":"%s","create_warn":"%s","create_sec":"%s","attach_sec":"%s","detach_sec":"%s","force":"%s","image_bytes":"%s","image_alloc":"%s","image_bytes_peak":"%s","image_alloc_peak":"%s","host_drop":"%s","host_margin":"%s","samples":"%s","enospc":"%s","vol_free":"%s","host_pre":"%s","host_fill":"%s"}}' \
        "$caseb" "$boundary" "$verd" "$reason" "$estate" "$name" "$created" "$launch" "$ssh" "$argv" "$rec" "$stop" "$del" "$abs" \
        "$fstype" "$fsname" "$hint" "$cap" "$nosuid" "$noexec" "$nodev" "$nobrowse" "$owners" "$mdutil" "$tmutil" "$cwarn" "$csec" "$asec" "$dsec" "$force" "$ibytes" "$ialloc" "$ibpeak" "$iapeak" "$hdrop" "$hmargin" "$samples" "$enospc" "$vfree" "$host_pre" "$host_fill"
    done <"$WORKDIR/m.rows"
    printf ']}\n'
  } >"$tmp" || { IO_FAIL=1; rm -f "$tmp" "$WORKDIR/stage-V.json"; return 1; }
  [[ -s "$tmp" ]] || { IO_FAIL=1; rm -f "$tmp" "$WORKDIR/stage-V.json"; return 1; }
  mv "$tmp" "$WORKDIR/stage-V.json" || { IO_FAIL=1; rm -f "$WORKDIR/stage-V.json"; return 1; }
}
volio_remove() {
  local kind=$1 parent=$2 pdev=$3 pino=$4 target=$5 tdev=$6 tino=$7 got
  [[ "$kind" == file || "$kind" == dir ]] || exit 2
  [[ ! -L "$parent" && -d "$parent" ]] || exit 3
  got=$("$STATBIN" -f '%d %i' "$parent" 2>/dev/null) || exit 1
  [[ "$got" == "$pdev $pino" ]] || exit 3
  [[ -L "$target" ]] && exit 3
  got=$("$STATBIN" -f '%d %i' "$target" 2>/dev/null) || exit 1
  [[ "$got" == "$tdev $tino" ]] || exit 3
  if [[ "$kind" == file ]]; then
    [[ -f "$target" ]] || exit 3
    /bin/rm -f -- "$target" || exit 1
  else
    [[ -d "$target" && ! -L "$target" ]] || exit 3
    /bin/rmdir -- "$target" || exit 1
  fi
  if [[ -e "$target" || -L "$target" ]]; then exit 1; fi
}
volio_main() {
  local cmd=${1:-} f d got
  shift || exit 2
  case "$cmd" in
    prepare)
      [[ $# -eq 3 ]] || exit 2
      M_CASE=$2; M_BOUNDARY=$3
      prepare_mailbox "$1" || exit 1
      ;;
    write)
      [[ $# -eq 1 ]] || exit 2
      f=$1
      d=$(dirname -- "$f")
      [[ ! -L "$d" && -d "$d" ]] || exit 1
      [[ -e "$f" || -L "$f" ]] && exit 1
      printf '\n' >"$f" || exit 1
      ;;
    remove)
      [[ $# -eq 7 ]] || exit 2
      volio_remove "$@"
      ;;
    slot)
      [[ $# -eq 1 ]] || exit 2
      f=$1
      got=$("$STATBIN" -f '%d %i' "$f" 2>/dev/null) || exit 3
      [[ "$got" == link ]] && exit 3
      head -c 4097 -- "$f" || exit 3
      ;;
    *) exit 2 ;;
  esac
}
dry_stage_v() {
  local a m img
  a=$(mktemp -d "$OUT/s11v.XXXXXX") || { add_reason io; PRECHECK_FAILED=1; return; }
  chmod 0700 "$a" || return
  m=$(mktemp -d "$a/mnt.XXXXXX") || return
  img=$a/mailbox.sparseimage
  printf '%s\n' "$HDIUTIL create -size 64m -fs HFSX -type SPARSE -layout NONE -volname GCU-MAILBOX $img"
  printf '%s\n' "$DISKUTIL image attach --plist --nobrowse --mountPoint $m --mountOptions nosuid,noexec,nodev,owners $img"
  /bin/rmdir -- "$m" || true
  /bin/rmdir -- "$a" || true
  M_BOUNDARY=-
  M_VERDICT=not_run; M_REASON=skipped; EXPORT_STATE=absent; S_ARGV_OK=0; M_RECORD=""
  begin_stage V
  : >"$WORKDIR/m.rows"
  printf '%s\n' volume-only >"$WORKDIR/v.evidence"
  record_m_row
  printf '%s\n' not_run >"$WORKDIR/m.agg"
  save_stage_v
}
run_mailbox() {
  : >"$WORKDIR/m.rows"
  if [[ "$M_CASE" == C2 ]]; then
    for M_BOUNDARY in file-sync dir-sync rename; do
      if ! can_advance; then
        begin_stage M
        M_VERDICT=not_run; M_REASON=skipped; EXPORT_STATE=absent; S_ARGV_OK=0
        record_m_row
        continue
      fi
      run_m_one
    done
  else
    M_BOUNDARY=-
    run_m_one
  fi
  fold_m
  save_stage_m
}
dry_stage_m() {
  M_BOUNDARY=-
  [[ "$M_CASE" == C2 ]] && M_BOUNDARY=file-sync
  begin_stage M
  prepare_export || { add_reason io; PRECHECK_FAILED=1; return; }
  S_NAME=$(make_name M) || S_NAME="s11-0-probe-M-dry-00000000"
  build_argv "$S_NAME" M ""
  print_argv
  M_VERDICT=not_run; M_REASON=skipped
  cleanup_export
  : >"$WORKDIR/m.rows"
  record_m_row
  printf '%s\n' not_run >"$WORKDIR/m.agg"
  save_stage_m
}

need_arg() { [[ $# -ge 2 ]] || { USAGE_REASON=usage; return 1; }; }
parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --stage) need_arg "$@" || return; case "$2" in A|B|C|M|V|all) STAGE=$2 ;; *) USAGE_REASON=usage; return ;; esac; shift 2 ;;
      --case) need_arg "$@" || return; case "$2" in C1|C2|C8|C4|C9) M_CASE=$2 ;; *) USAGE_REASON=usage; return ;; esac; shift 2 ;;
      --probe) need_arg "$@" || return; M_PROBE=$2; shift 2 ;;
      --pinned-host-key) need_arg "$@" || return; PINNED_KEY=$2; shift 2 ;;
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
  case "$OUT" in
    /*) ;;
    ./*) OUT=$(pwd)/${OUT#./} ;;
    *) OUT=$(pwd)/$OUT ;;
  esac
  case "$OUT" in *$'\n'*|*$'\r'*|*$'\t'*) USAGE_REASON=usage; OUT=""; return ;; esac
  case "$OUT" in *"/../"*|*/..) USAGE_REASON=usage; return ;; esac
  [[ -n "$ENDPOINT" && "$ENDPOINT_SOURCE" == placeholder ]] && { USAGE_REASON=usage; ENDPOINT=""; ENDPOINT_SOURCE=absent; return; }
  if [[ -n "$ENDPOINT" ]]; then is_public_ip "$ENDPOINT" && ENDPOINT_SOURCE=argument || { USAGE_REASON=endpoint-invalid; ENDPOINT=""; return; }; fi
  [[ "$ENDPOINT_SOURCE" == placeholder ]] && ENDPOINT=$PLACEHOLDER_IP
  case "$STAGE" in B|all) [[ -n "$ENDPOINT" ]] || USAGE_REASON=endpoint-required ;; esac
  if [[ -n "$LAN_IP" ]] && ! is_ipv4 "$LAN_IP"; then USAGE_REASON=usage; LAN_IP=""; fi
  if [[ "$STAGE" == M ]]; then
    case "$M_CASE" in C1|C2|C8) ;; *) USAGE_REASON=usage ;; esac
    [[ -n "$PINNED_KEY" ]] || USAGE_REASON=usage
    [[ "$DRY" == 1 || -n "$M_PROBE" ]] || USAGE_REASON=usage
  elif [[ "$STAGE" == V ]]; then
    case "$M_CASE" in C4|C1|C2|C9) ;; *) USAGE_REASON=usage ;; esac
    if [[ "$M_CASE" == C4 ]]; then
      [[ -z "$M_PROBE" && -z "$PINNED_KEY" ]] || USAGE_REASON=usage
    else
      [[ -n "$PINNED_KEY" ]] || USAGE_REASON=usage
      [[ "$DRY" == 1 || -n "$M_PROBE" ]] || USAGE_REASON=usage
    fi
  else
    [[ -z "$M_CASE" && -z "$PINNED_KEY" && -z "$M_PROBE" ]] || USAGE_REASON=usage
  fi
}
precheck() {
  local gst
  [[ "$TOOL_OVERRIDE" == 1 ]] && { add_reason tool-override; return 1; }
  [[ "$(id -u)" == 0 ]] && { add_reason root; return 1; }
  check_softnet || return 1
  [[ -f "$SSH_KEY" && -r "$SSH_KEY" ]] || { add_reason ssh-key-unreadable; return 1; }
  if [[ "$STAGE" == M || ( "$STAGE" == V && "$M_CASE" != C4 ) ]]; then
    [[ -f "$PINNED_KEY" && -r "$PINNED_KEY" && ! -L "$PINNED_KEY" ]] || { add_reason pinned-host-key; return 1; }
    awk '$1=="ssh-ed25519" && $2 ~ /^[A-Za-z0-9+\/=]+$/ {found=1; exit} END{exit !found}' "$PINNED_KEY" || { add_reason pinned-host-key; return 1; }
    if [[ "$DRY" != 1 ]]; then
      [[ -f "$M_PROBE" && -r "$M_PROBE" && ! -L "$M_PROBE" ]] || { add_reason probe-unreadable; return 1; }
    fi
  fi
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
  if [[ "$PRECHECK_FAILED" == 1 ]]; then STATUS=precheck-failed; FINAL_EC=2
  elif [[ "$DRY" == 1 ]]; then STATUS=not-run; FINAL_EC=0
  elif [[ "$ANY_AMBIGUOUS" == 1 ]]; then STATUS=partial; FINAL_EC=5
  elif [[ ( "$STAGE" == M || "$STAGE" == V ) && "$M_QUARANTINE" == 1 ]]; then STATUS=partial; FINAL_EC=5
  elif [[ "$ANY_COLLECTION_FAILED" == 1 ]]; then STATUS=partial; FINAL_EC=4
  elif [[ "$ANY_CLEANUP_FAILED" == 1 ]]; then STATUS=partial; FINAL_EC=5
  elif [[ "$ANY_CREATE_FAILED" == 1 ]]; then STATUS=partial; FINAL_EC=3
  else STATUS=complete; FINAL_EC=0
  fi
  if [[ "$IO_FAIL" == 1 && "$FINAL_EC" -eq 0 ]]; then STATUS=partial; FINAL_EC=2; fi
}
on_exit() {
  trap '' INT TERM HUP EXIT
  FINALIZING=1; cleanup_owned || true; cleanup_export || true; decide
  write_report || { [[ "$OUT_READY" == 1 ]] && say report-write-failed; [[ "$FINAL_EC" -eq 0 ]] && FINAL_EC=2; }
  if [[ "$IO_FAIL" == 1 && "$FINAL_EC" -eq 0 ]]; then FINAL_EC=2; fi
  [[ "$OUT_READY" == 1 ]] && printf 'status=%s\nreport=%s/report.json\n' "$STATUS" "$OUT"
  case "$WORKDIR" in /tmp/s11-0.*|/private/tmp/s11-0.*|/var/folders/*|*/s11-0.*) [[ ! -L "$WORKDIR" && -d "$WORKDIR" ]] && rm -rf "$WORKDIR" ;; esac
  exit "$FINAL_EC"
}
dry_stages() {
  local st nm ep
  if [[ "$STAGE" == M ]]; then dry_stage_m; return; fi
  if [[ "$STAGE" == V ]]; then dry_stage_v; return; fi
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
  [[ "$HALT" == 1 || "$M_QUARANTINE" == 1 ]] && return 1
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
    [[ "$USAGE_REASON" == help ]] && say 'usage: s11-0-probe.sh [--dry-run] [--stage A|B|C|M|V|all] [--case C1|C2|C8|C4|C9] [--probe FILE] [--pinned-host-key FILE] [--endpoint-ip A.B.C.D|--endpoint-placeholder] [--lan-ip A.B.C.D] [--ssh-key FILE] [--tart-home DIR] [--out DIR]'
    add_reason "$USAGE_REASON"; PRECHECK_FAILED=1
    [[ -z "$OUT" ]] && exit 2
    prepare_out || exit 2
    exit 2
  fi
  prepare_out || exit 2
  precheck || { PRECHECK_FAILED=1; exit 2; }
  write_guest
  if [[ "$DRY" == 1 ]]; then dry_stages; exit 0; fi
  if [[ "$STAGE" == M ]]; then run_mailbox; exit 0; fi
  if [[ "$STAGE" == V ]]; then run_volume; exit 0; fi
  collect_help
  case "$STAGE" in A|all) run_stage A || true; save_stage ;; esac
  if can_advance; then case "$STAGE" in B|all) run_stage B || true; save_stage ;; esac; fi
  exit 0
}
if [[ "${1:-}" == __volio ]]; then
  volio_main "${@:2}"
  exit $?
fi
main "$@"

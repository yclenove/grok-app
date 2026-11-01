#!/usr/bin/env bash
# S11-0 model-free Softnet/Tart probe (ADR 0018 §10).
# Run on the owner Mac mini. Never starts, modifies, or deletes the golden VM.
# Deletes only a clone this process created whose name starts with s11-0-probe-.
# Never prompts for or stores a passphrase. Does not change the packet filter,
# routes, or resolver settings. Addresses in the report are role names.
#
# Internal entry (piped to the guest): bash s11-0-probe.sh --guest ...
# Shell notes: keep this file compatible with bash 3.2 (macOS /bin/bash).

set -euo pipefail

CLONE_PREFIX="s11-0-probe-"
PLACEHOLDER_ENDPOINT="192.0.2.10" # RFC 5737 TEST-NET-1, not an approved endpoint
DEFAULT_TART_HOME="/Volumes/Mini扩展/VMs/tart"
V6_PUBLIC="2606:4700:4700::1111"

# --- shared helpers ----------------------------------------------------------

now_ms() {
  local ms=""
  if command -v perl >/dev/null 2>&1; then
    ms=$(perl -MTime::HiRes=time -e 'printf "%d\n", int(time()*1000)' 2>/dev/null || true)
  fi
  if [[ -z "$ms" ]]; then
    ms=$(( $(date +%s) * 1000 ))
  fi
  printf '%s\n' "$ms"
}

is_num_0_255() {
  local n=$1
  case "$n" in
    ''|*[!0-9]*) return 1 ;;
  esac
  if [[ ${#n} -gt 1 && ${n:0:1} == 0 ]]; then
    return 1
  fi
  if [[ "$n" -gt 255 ]]; then
    return 1
  fi
  return 0
}

is_ipv4_syntax() {
  local ip=$1 a="" b="" c="" d=""
  case "$ip" in
    *.*.*.*) ;;
    *) return 1 ;;
  esac
  case "$ip" in
    *.*.*.*.*) return 1 ;;
  esac
  IFS=. read -r a b c d <<< "$ip"
  is_num_0_255 "$a" || return 1
  is_num_0_255 "$b" || return 1
  is_num_0_255 "$c" || return 1
  is_num_0_255 "$d" || return 1
  return 0
}

is_public_endpoint() {
  local ip=$1 a="" b="" c="" d=""
  is_ipv4_syntax "$ip" || return 1
  IFS=. read -r a b c d <<< "$ip"
  if [[ "$a" -eq 0 ]]; then return 1; fi
  if [[ "$a" -eq 127 ]]; then return 1; fi
  if [[ "$a" -ge 224 ]]; then return 1; fi
  if [[ "$a" -eq 169 && "$b" -eq 254 ]]; then return 1; fi
  if [[ "$a" -eq 10 ]]; then return 1; fi
  if [[ "$a" -eq 192 && "$b" -eq 168 ]]; then return 1; fi
  if [[ "$a" -eq 172 && "$b" -ge 16 && "$b" -le 31 ]]; then return 1; fi
  if [[ "$a" -eq 100 && "$b" -ge 64 && "$b" -le 127 ]]; then return 1; fi
  return 0
}

is_ipv6_syntax() {
  local ip=$1 rest=""
  [[ -n "$ip" && ${#ip} -le 64 ]] || return 1
  case "$ip" in
    *[!0-9A-Fa-f:]*) return 1 ;;
    *:*) ;;
    *) return 1 ;;
  esac
  rest=${ip#*::}
  case "$rest" in
    *::*) return 1 ;;
  esac
  return 0
}

ensure_sbin_path() {
  case ":${PATH}:" in
    *:/sbin:*) ;;
    *) PATH="${PATH}:/sbin:/usr/sbin" ;;
  esac
  export PATH
}

die() {
  local code=$1
  shift
  printf 's11-0-probe: %s\n' "$*" >&2
  exit "$code"
}

# --- guest (no address printed) ----------------------------------------------

emit_probe() {
  local id=$1 result=$2 start_ms=$3 detail=$4
  local end_ms="" elapsed=0
  end_ms=$(now_ms)
  elapsed=$(( end_ms - start_ms ))
  if [[ "$elapsed" -lt 0 ]]; then
    elapsed=0
  fi
  case "$detail" in
    ''|*[!A-Za-z0-9_-]*) detail="redacted" ;;
  esac
  printf 'PROBE %s %s %s %s\n' "$id" "$result" "$elapsed" "$detail"
}

emit_now() {
  local id=$1 result=$2 detail=$3
  local start=""
  start=$(now_ms)
  emit_probe "$id" "$result" "$start" "$detail"
}

run_class_probe() {
  local id=$1
  shift
  local start="" line="" result="" detail=""
  start=$(now_ms)
  set +e
  line=$("$@")
  set -e
  result=${line%% *}
  detail=${line#* }
  detail=${detail%% *}
  case "$result" in
    reachable|blocked|error) ;;
    *) result="error"; detail="bad-class" ;;
  esac
  [[ -n "$detail" ]] || detail="none"
  emit_probe "$id" "$result" "$start" "$detail"
}

discover_gateway() {
  local g=""
  set +e
  g=$(route -n get default 2>/dev/null | awk '/gateway:/{print $2; exit}')
  set -e
  g=$(printf '%s' "$g" | tr -d '[:space:]')
  if ! is_ipv4_syntax "$g"; then
    set +e
    g=$(netstat -rn 2>/dev/null | awk '$1=="default"{print $2; exit}')
    set -e
    g=$(printf '%s' "$g" | tr -d '[:space:]')
  fi
  if ! is_ipv4_syntax "$g"; then
    g=""
  fi
  printf '%s' "$g"
}

tcp_class() {
  local host=$1 port=$2 err="" rc=0
  if ! command -v nc >/dev/null 2>&1; then
    printf 'error nc-missing\n'
    return 0
  fi
  if ! is_ipv4_syntax "$host"; then
    printf 'error bad-target\n'
    return 0
  fi
  set +e
  err=$(nc -z -w 2 "$host" "$port" 2>&1)
  rc=$?
  set -e
  if [[ "$rc" -eq 0 ]]; then
    printf 'reachable connect\n'
    return 0
  fi
  case "$err" in
    *[Rr]efused*|*[Uu]nreachable*|*[Pp]rohibited*|*[Nn]o\ route*)
      printf 'blocked explicit-reject\n'
      ;;
    *)
      printf 'error timeout\n'
      ;;
  esac
  return 0
}

tcp6_class() {
  local host=$1 port=$2 err="" rc=0
  if ! command -v nc >/dev/null 2>&1; then
    printf 'error nc-missing\n'
    return 0
  fi
  if ! is_ipv6_syntax "$host"; then
    printf 'error bad-target\n'
    return 0
  fi
  set +e
  err=$(nc -6 -z -w 2 "$host" "$port" 2>&1)
  rc=$?
  set -e
  if [[ "$rc" -eq 0 ]]; then
    printf 'reachable connect\n'
    return 0
  fi
  case "$err" in
    *[Rr]efused*|*[Uu]nreachable*|*[Pp]rohibited*|*[Nn]o\ route*)
      printf 'blocked explicit-reject\n'
      ;;
    *)
      printf 'error timeout\n'
      ;;
  esac
  return 0
}

udp_class() {
  local host=$1 port=$2 mode=$3
  local out="" err="" rc=0
  if ! command -v nc >/dev/null 2>&1; then
    printf 'error nc-missing\n'
    return 0
  fi
  if ! is_ipv4_syntax "$host"; then
    printf 'error bad-target\n'
    return 0
  fi
  out=$(mktemp 2>/dev/null || true)
  if [[ -z "$out" ]]; then
    printf 'error mktemp-failed\n'
    return 0
  fi
  set +e
  if [[ "$mode" == "dns" ]]; then
    err=$(printf '\022\064\001\000\000\001\000\000\000\000\000\000\007example\003com\000\000\001\000\001' \
      | nc -u -w 2 "$host" "$port" 2>&1 >"$out")
  else
    err=$(printf 'probe' | nc -u -w 2 "$host" "$port" 2>&1 >"$out")
  fi
  rc=$?
  set -e
  if [[ -s "$out" ]]; then
    rm -f "$out"
    printf 'reachable udp-reply\n'
    return 0
  fi
  rm -f "$out"
  case "$err" in
    *[Rr]efused*|*[Uu]nreachable*|*[Pp]rohibited*)
      printf 'blocked explicit-reject\n'
      ;;
    *)
      printf 'error timeout-no-icmp\n'
      ;;
  esac
  return 0
}

icmp_class() {
  local host=$1 out="" rc=0
  if ! command -v ping >/dev/null 2>&1; then
    printf 'error ping-missing\n'
    return 0
  fi
  if ! is_ipv4_syntax "$host"; then
    printf 'error bad-target\n'
    return 0
  fi
  set +e
  # macOS ping -W is milliseconds.
  out=$(ping -c 1 -W 2000 "$host" 2>&1)
  rc=$?
  set -e
  if [[ "$rc" -eq 0 ]]; then
    printf 'reachable echo-reply\n'
    return 0
  fi
  case "$out" in
    *[Uu]nreachable*|*[Pp]rohibited*)
      printf 'blocked explicit-reject\n'
      ;;
    *)
      printf 'error timeout\n'
      ;;
  esac
  return 0
}

ipv6_addr_class() {
  local out="" kind=""
  if ! command -v ifconfig >/dev/null 2>&1; then
    printf 'error ifconfig-failed\n'
    return 0
  fi
  set +e
  out=$(ifconfig 2>/dev/null)
  set -e
  if [[ -z "$out" ]]; then
    printf 'error ifconfig-failed\n'
    return 0
  fi
  kind=$(printf '%s\n' "$out" | awk '
    BEGIN { g=0; l=0 }
    {
      for (i=1; i<=NF; i++) {
        if ($i=="inet6" && (i+1)<=NF) {
          a=$(i+1)
          if (a ~ /^fe80:/) l=1
          else if (a != "::1" && a !~ /^::1\//) g=1
        }
      }
    }
    END {
      if (g) print "global"
      else if (l) print "linklocal"
      else print "none"
    }')
  case "$kind" in
    global) printf 'reachable address-present\n' ;;
    linklocal) printf 'blocked only-link-local\n' ;;
    *) printf 'blocked no-ipv6-address\n' ;;
  esac
  return 0
}

guest_capture_bounded() {
  local secs=$1 dest=$2
  shift 2
  local pid="" i=0
  if command -v setsid >/dev/null 2>&1; then
    setsid "$@" >"$dest" 2>/dev/null &
  else
    "$@" >"$dest" 2>/dev/null &
  fi
  pid=$!
  while [[ "$i" -lt "$secs" ]]; do
    if ! kill -0 "$pid" 2>/dev/null; then
      wait "$pid" 2>/dev/null || true
      return 0
    fi
    sleep 1
    i=$((i + 1))
  done
  kill -TERM "$pid" 2>/dev/null || true
  sleep 0.2
  kill -KILL "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  return 1
}

scan_virt_flags() {
  local file=$1
  if [[ ! -s "$file" ]]; then
    return 0
  fi
  if awk '{ l=tolower($0); if (l ~ /vsock/) f=1 } END { exit !f }' "$file"; then
    GUEST_VSOCK="present"
  fi
  if awk '{ l=tolower($0); if (l ~ /virtio/) f=1 } END { exit !f }' "$file"; then
    GUEST_VIRTIO="present"
  fi
}

guest_inventory() {
  local tmp="" devhit=0
  GUEST_VSOCK="absent"
  GUEST_VIRTIO="absent"
  GUEST_SERIAL="absent"
  umask 077
  if [[ -d /dev ]]; then
    if ls -1 /dev 2>/dev/null | awk '{ l=tolower($0); if (l ~ /vsock/) f=1 } END { exit !f }'; then
      GUEST_VSOCK="present"
    fi
    if ls -1 /dev 2>/dev/null | awk '{ l=tolower($0); if (l ~ /^(cu\.|tty\.|ttys|ttyS|ttyAMA)/) f=1 } END { exit !f }'; then
      GUEST_SERIAL="present"
    fi
    devhit=1
  fi
  # ioreg/system_profiler can contain hardware identifiers. Scan in a temp file
  # and delete it. Never print the command output.
  if command -v ioreg >/dev/null 2>&1; then
    tmp=$(mktemp 2>/dev/null || true)
    if [[ -n "$tmp" ]]; then
      guest_capture_bounded 15 "$tmp" ioreg -l -w 0 || true
      scan_virt_flags "$tmp"
      rm -f "$tmp"
    fi
  fi
  if command -v system_profiler >/dev/null 2>&1; then
    tmp=$(mktemp 2>/dev/null || true)
    if [[ -n "$tmp" ]]; then
      guest_capture_bounded 15 "$tmp" system_profiler SPPCIDataType || true
      scan_virt_flags "$tmp"
      rm -f "$tmp"
    fi
  fi
  if [[ "$GUEST_VSOCK" == "present" ]]; then
    emit_now "guest-vsock" "reachable" "present"
  elif [[ "$devhit" -eq 1 ]]; then
    emit_now "guest-vsock" "blocked" "absent"
  else
    emit_now "guest-vsock" "error" "unchecked"
  fi
  if [[ "$GUEST_VIRTIO" == "present" ]]; then
    emit_now "guest-virtio" "reachable" "present"
  else
    emit_now "guest-virtio" "blocked" "absent"
  fi
  if [[ "$GUEST_SERIAL" == "present" ]]; then
    emit_now "guest-serial" "reachable" "present"
  elif [[ "$devhit" -eq 1 ]]; then
    emit_now "guest-serial" "blocked" "absent"
  else
    emit_now "guest-serial" "error" "unchecked"
  fi
  return 0
}

guest_ports_loop() {
  local gw=$1 ports=$2
  local oldifs="" p=""
  oldifs=$IFS
  IFS=','
  # shellcheck disable=SC2086
  set -- $ports
  IFS=$oldifs
  for p in "$@"; do
    if is_ipv4_syntax "$gw"; then
      run_class_probe "gateway-tcp-${p}" tcp_class "$gw" "$p"
    else
      emit_now "gateway-tcp-${p}" "error" "gateway-unknown"
    fi
  done
}

guest_net() {
  local ep=$1 pub=$2 lan=$3 ports=$4
  local gw=""
  gw=$(discover_gateway)
  if is_ipv4_syntax "$gw"; then
    run_class_probe "gateway-icmp" icmp_class "$gw"
  else
    emit_now "gateway-icmp" "error" "gateway-unknown"
  fi
  guest_ports_loop "$gw" "$ports"
  if is_ipv4_syntax "$gw"; then
    run_class_probe "dns-udp-gateway" udp_class "$gw" 53 dns
  else
    emit_now "dns-udp-gateway" "error" "gateway-unknown"
  fi
  if is_public_endpoint "$pub"; then
    run_class_probe "dns-udp-public" udp_class "$pub" 53 dns
  else
    emit_now "dns-udp-public" "error" "bad-target"
  fi
  run_class_probe "ipv6-addr" ipv6_addr_class
  run_class_probe "ipv6-tcp-public" tcp6_class "$V6_PUBLIC" 443
  if is_public_endpoint "$ep"; then
    run_class_probe "endpoint-tcp-443" tcp_class "$ep" 443
    run_class_probe "endpoint-udp-443" udp_class "$ep" 443 quic
  else
    emit_now "endpoint-tcp-443" "error" "bad-target"
    emit_now "endpoint-udp-443" "error" "bad-target"
  fi
  if is_ipv4_syntax "$lan"; then
    run_class_probe "lan-icmp" icmp_class "$lan"
    run_class_probe "lan-tcp-443" tcp_class "$lan" 443
  else
    emit_now "lan-icmp" "error" "lan-unavailable"
    emit_now "lan-tcp-443" "error" "lan-unavailable"
  fi
  run_class_probe "metadata-icmp" icmp_class "169.254.169.254"
  run_class_probe "metadata-tcp-80" tcp_class "169.254.169.254" 80
  run_class_probe "metadata-tcp-443" tcp_class "169.254.169.254" 443
  return 0
}

guest_main() {
  local stage="A" ep="" pub="" lan="" ports="22,53,80,443" inventory=0
  shift
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --stage)
        [[ $# -ge 2 ]] || return 1
        stage=$2
        shift 2
        ;;
      --ep)
        [[ $# -ge 2 ]] || return 1
        ep=$2
        shift 2
        ;;
      --pub)
        [[ $# -ge 2 ]] || return 1
        pub=$2
        shift 2
        ;;
      --lan)
        [[ $# -ge 2 ]] || return 1
        lan=$2
        shift 2
        ;;
      --ports)
        [[ $# -ge 2 ]] || return 1
        ports=$2
        shift 2
        ;;
      --inventory)
        inventory=1
        shift
        ;;
      *)
        shift
        ;;
    esac
  done
  ensure_sbin_path
  if [[ "$inventory" -eq 1 ]]; then
    guest_inventory
    return 0
  fi
  guest_net "$ep" "$pub" "$lan" "$ports"
  return 0
}

# --- host --------------------------------------------------------------------

usage() {
  cat <<'EOF'
s11-0-probe.sh — model-free Softnet/Tart egress probe (ADR 0018 §10)

Runs on the owner Mac mini. Clones the golden VM, probes, then stop+delete.
Only a clone named s11-0-probe-<suffix> created by this process is deleted.
The golden VM is never started, modified, or deleted.

Options:
  --dry-run                 Print the command list and exit
  --stage A|B|C|all         Default: all
  --endpoint-ip <ipv4>      Public /32 to allow in stage B
  --ports <csv>             Gateway TCP ports (default 22,53,80,443)
  --out <dir>               Report directory (default ./s11-0-<utc>/)
  --golden <name>           Default: macos-s2-eval
  --tart-home <dir>         Default: external volume used by the owner
  --softnet <path>          softnet binary (default: PATH, then common prefixes)
  --guest-user <name>       Default: eval
  --ssh-key <path>          Default: ~/.ssh/id_ed25519_tart
  --clone-name <name>       Must start with s11-0-probe- (tests / reruns)
  --lan-ip <ipv4>           Single host-LAN address to probe (role lan-redacted)
  --public-resolver <ipv4>  Default: 1.1.1.1
  --min-free-gb <n>         Default: 10
  --overall-timeout <sec>   Default: 2400
  --boot-timeout <sec>      Default: 300
  --clone-timeout <sec>     Default: 600
  --ssh-wait <sec>          Default: 90
  --probe-timeout <sec>     Default: 180
  --stop-timeout <sec>      Default: 60
  --delete-timeout <sec>    Default: 120
  --help

Exit codes: 0 finished, 1 usage, 2 precheck, 4 partial probes, 5 cleanup failed.
Timeouts are recorded as error, not blocked.
EOF
}

sanitize() {
  local s=$1
  s=$(printf '%s' "$s" | tr $'\t\r\n' '   ')
  s=$(printf '%s' "$s" | sed -E 's/[0-9]{1,3}(\.[0-9]{1,3}){3}/ip-redacted/g')
  # Hyphen is last so tr treats it as a literal, not a range.
  s=$(printf '%s' "$s" | tr -c 'A-Za-z0-9._,:=+ -' '_')
  printf '%.160s' "$s"
}

record_probe() {
  local id=$1 stage=$2 target=$3 result=$4 elapsed=$5 detail=$6
  case "$id" in
    ''|*[!A-Za-z0-9_-]*) id="bad-id" ;;
  esac
  case "$stage" in
    ''|*[!A-Za-z0-9_-]*) stage="bad" ;;
  esac
  case "$target" in
    ''|*[!A-Za-z0-9_-]*) target="host" ;;
  esac
  case "$result" in
    reachable|blocked|error) ;;
    *) result="error"; detail="bad-result" ;;
  esac
  case "$elapsed" in
    ''|*[!0-9]*) elapsed=0 ;;
  esac
  detail=$(sanitize "$detail")
  [[ -n "$detail" ]] || detail="none"
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$id" "$stage" "$target" "$result" "$elapsed" "$detail" >> "$PROBE_TSV"
}

target_for() {
  case "$1" in
    gateway-*|dns-udp-gateway) printf 'gateway' ;;
    dns-udp-public|ipv6-*) printf 'public' ;;
    endpoint-*) printf 'endpoint' ;;
    lan-*) printf 'lan-redacted' ;;
    metadata-*) printf 'metadata' ;;
    guest-*) printf 'guest' ;;
    *) printf 'host' ;;
  esac
}

have_probe() {
  local id=$1 stage=$2
  [[ -f "$PROBE_TSV" ]] || return 1
  awk -F '\t' -v id="$id" -v st="$stage" '
    $1==id && $2==st { found=1 }
    END { exit !found }
  ' "$PROBE_TSV"
}

note() {
  printf 's11-0-probe: %s\n' "$*" >&2
}

require_int() {
  local n=$1
  case "$n" in
    ''|*[!0-9]*) return 1 ;;
  esac
  return 0
}

normalize_ports() {
  local raw=$1
  local IFS=','
  local p="" out="" count=0
  local -a parts=()
  read -r -a parts <<< "$raw"
  if [[ ${#parts[@]} -eq 0 || ${#parts[@]} -gt 16 ]]; then
    return 1
  fi
  for p in "${parts[@]}"; do
    case "$p" in
      ''|*[!0-9]*) return 1 ;;
    esac
    if [[ ${#p} -gt 1 && ${p:0:1} == 0 ]]; then
      return 1
    fi
    if [[ "$p" -lt 1 || "$p" -gt 65535 ]]; then
      return 1
    fi
    count=$((count + 1))
    if [[ -z "$out" ]]; then
      out=$p
    else
      out="${out},${p}"
    fi
  done
  if [[ "$count" -lt 1 ]]; then
    return 1
  fi
  PORTS_CSV=$out
  return 0
}

valid_clone_name() {
  local n=$1 suf=""
  case "$n" in
    ${CLONE_PREFIX}*) ;;
    *) return 1 ;;
  esac
  suf=${n#"$CLONE_PREFIX"}
  [[ -n "$suf" ]] || return 1
  case "$suf" in
    *[!A-Za-z0-9_-]*) return 1 ;;
  esac
  if [[ ${#suf} -gt 40 ]]; then
    return 1
  fi
  return 0
}

make_clone_name() {
  local rand=""
  rand=$(od -An -N4 -tx1 /dev/urandom 2>/dev/null || true)
  rand=$(printf '%s' "$rand" | tr -d '[:space:]')
  case "$rand" in
    *[!0-9a-f]*|'') return 1 ;;
  esac
  printf '%s%s\n' "$CLONE_PREFIX" "$rand"
}

disposable_ok() {
  local name=$1
  valid_clone_name "$name" || return 1
  if [[ "$name" == "$GOLDEN" ]]; then
    return 1
  fi
  return 0
}

kill_recorded() {
  local pid=${1:-}
  local pgid="" i=0
  case "$pid" in
    ''|*[!0-9]*) return 0 ;;
  esac
  if [[ "$pid" -le 1 ]]; then
    return 0
  fi
  if ! kill -0 "$pid" 2>/dev/null; then
    return 0
  fi
  pgid=$(ps -o pgid= -p "$pid" 2>/dev/null | tr -d '[:space:]' || true)
  if [[ -n "$pgid" && "$pgid" == "$pid" ]]; then
    kill -TERM -- "-$pid" 2>/dev/null || true
  else
    kill -TERM -- "$pid" 2>/dev/null || true
  fi
  while [[ "$i" -lt 25 ]]; do
    if ! kill -0 "$pid" 2>/dev/null; then
      return 0
    fi
    sleep 0.2
    i=$((i + 1))
  done
  if [[ -n "$pgid" && "$pgid" == "$pid" ]]; then
    kill -KILL -- "-$pid" 2>/dev/null || true
  else
    kill -KILL -- "$pid" 2>/dev/null || true
  fi
  return 0
}

restore_errexit() {
  if [[ "${1:-0}" -eq 1 ]]; then
    set -e
  else
    set +e
  fi
}

run_bounded() {
  local secs=$1
  shift
  local pid="" start=0 now=0 rc=0 errexit=0
  case $- in
    *e*) errexit=1 ;;
  esac
  set +e
  if command -v setsid >/dev/null 2>&1; then
    setsid "$@" &
  else
    perl -MPOSIX=setsid -e 'setsid(); exec @ARGV or die $!' -- "$@" &
  fi
  pid=$!
  CURRENT_CHILD_PID=$pid
  start=$(date +%s)
  while kill -0 "$pid" 2>/dev/null; do
    now=$(date +%s)
    if [[ $((now - start)) -ge "$secs" || "$now" -ge "$DEADLINE" ]]; then
      kill_recorded "$pid"
      CURRENT_CHILD_PID=""
      restore_errexit "$errexit"
      return 124
    fi
    sleep 0.2
  done
  wait "$pid"
  rc=$?
  CURRENT_CHILD_PID=""
  restore_errexit "$errexit"
  return "$rc"
}

# Explicit stdin/stdout on the child. A background job would otherwise read
# /dev/null, which drops the guest script passed to ssh.
run_bounded_io() {
  local secs=$1 infile=$2 outfile=$3
  shift 3
  local pid="" start=0 now=0 rc=0 errexit=0
  case $- in
    *e*) errexit=1 ;;
  esac
  set +e
  if command -v setsid >/dev/null 2>&1; then
    setsid "$@" <"$infile" >"$outfile" 2>/dev/null &
  else
    perl -MPOSIX=setsid -e 'setsid(); open(STDIN, "<", shift @ARGV) or die $!; my $out = shift @ARGV; open(STDOUT, ">", $out) or die $!; open(STDERR, ">", "/dev/null"); exec @ARGV or die $!' -- "$infile" "$outfile" "$@" &
  fi
  pid=$!
  CURRENT_CHILD_PID=$pid
  start=$(date +%s)
  while kill -0 "$pid" 2>/dev/null; do
    now=$(date +%s)
    if [[ $((now - start)) -ge "$secs" || "$now" -ge "$DEADLINE" ]]; then
      kill_recorded "$pid"
      CURRENT_CHILD_PID=""
      restore_errexit "$errexit"
      return 124
    fi
    sleep 0.2
  done
  wait "$pid"
  rc=$?
  CURRENT_CHILD_PID=""
  restore_errexit "$errexit"
  return "$rc"
}

capture_bounded() {
  local secs=$1 outfile=$2
  shift 2
  local pid="" start=0 now=0 rc=0 errexit=0
  case $- in
    *e*) errexit=1 ;;
  esac
  set +e
  if command -v setsid >/dev/null 2>&1; then
    setsid "$@" >"$outfile" 2>&1 &
  else
    perl -MPOSIX=setsid -e 'setsid(); my $out = shift @ARGV; open(STDOUT, ">", $out) or die $!; open(STDERR, ">&STDOUT"); exec @ARGV or die $!' -- "$outfile" "$@" &
  fi
  pid=$!
  CURRENT_CHILD_PID=$pid
  start=$(date +%s)
  while kill -0 "$pid" 2>/dev/null; do
    now=$(date +%s)
    if [[ $((now - start)) -ge "$secs" || "$now" -ge "$DEADLINE" ]]; then
      kill_recorded "$pid"
      CURRENT_CHILD_PID=""
      restore_errexit "$errexit"
      return 124
    fi
    sleep 0.2
  done
  wait "$pid"
  rc=$?
  CURRENT_CHILD_PID=""
  restore_errexit "$errexit"
  return "$rc"
}

run_bounded_simple() {
  local secs=$1
  shift
  local pid="" start=0 now=0 rc=0 errexit=0
  case $- in
    *e*) errexit=1 ;;
  esac
  set +e
  if command -v setsid >/dev/null 2>&1; then
    setsid "$@" >/dev/null 2>&1 &
  else
    perl -MPOSIX=setsid -e 'setsid(); exec @ARGV or die $!' -- "$@" >/dev/null 2>&1 &
  fi
  pid=$!
  CURRENT_CHILD_PID=$pid
  start=$(date +%s)
  while kill -0 "$pid" 2>/dev/null; do
    now=$(date +%s)
    if [[ $((now - start)) -ge "$secs" ]]; then
      kill_recorded "$pid"
      CURRENT_CHILD_PID=""
      restore_errexit "$errexit"
      return 124
    fi
    sleep 0.2
  done
  wait "$pid"
  rc=$?
  CURRENT_CHILD_PID=""
  restore_errexit "$errexit"
  return "$rc"
}

spawn_tart_run() {
  local log=""
  log=$(mktemp)
  TART_LOG=$log
  if command -v setsid >/dev/null 2>&1; then
    setsid "$@" >"$log" 2>&1 &
  else
    perl -MPOSIX=setsid -e 'setsid(); exec @ARGV or die $!' -- "$@" >"$log" 2>&1 &
  fi
  TART_RUN_PID=$!
}

destroy_owned() {
  local name="" pid=""
  name=${OWNED_CLONE:-}
  pid=${TART_RUN_PID:-}
  if [[ -z "$name" && -z "$pid" ]]; then
    return 0
  fi
  if [[ -n "$pid" ]]; then
    kill_recorded "$pid"
    TART_RUN_PID=""
  fi
  if [[ -n "${TART_LOG:-}" ]]; then
    rm -f "$TART_LOG"
    TART_LOG=""
  fi
  if [[ -z "$name" ]]; then
    return 0
  fi
  if ! disposable_ok "$name"; then
    PREFIX_OK=false
    CLEANUP_DELETE="refused"
    OWNED_CLONE=""
    note "refusing to mutate a VM outside the disposable prefix"
    return 1
  fi
  PREFIX_OK=true
  # Re-check the local name immediately before stop and delete.
  if ! disposable_ok "$name"; then
    PREFIX_OK=false
    CLEANUP_DELETE="refused"
    OWNED_CLONE=""
    return 1
  fi
  if run_bounded_simple "$STOP_TIMEOUT" tart stop "$name"; then
    CLEANUP_STOP="ok"
  else
    CLEANUP_STOP="failed"
  fi
  if ! disposable_ok "$name"; then
    PREFIX_OK=false
    CLEANUP_DELETE="refused"
    OWNED_CLONE=""
    return 1
  fi
  if run_bounded_simple "$DELETE_TIMEOUT" tart delete "$name"; then
    CLEANUP_DELETE="ok"
    OWNED_CLONE=""
    return 0
  fi
  CLEANUP_DELETE="failed"
  return 1
}

json_bool() {
  if [[ "$1" == "true" ]]; then
    printf 'true'
  else
    printf 'false'
  fi
}

write_reports() {
  local tmp="" first=1 id="" stage="" target="" result="" elapsed="" detail=""
  local ra=0 rb=0 re=0 ba=0 bb=0 be=0 ea=0 eb=0 ee=0
  local line="" safe_name=""
  [[ "${DO_REPORT:-0}" -eq 1 ]] || return 0
  [[ -n "${OUT:-}" ]] || return 0
  mkdir -p "$OUT" 2>/dev/null || return 0
  FINISHED_UTC=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  tmp=$(mktemp)
  {
    printf '{\n'
    printf '  "schema": "s11-0-probe/v1",\n'
    printf '  "status": "%s",\n' "$(sanitize "$STATUS")"
    printf '  "started_utc": "%s",\n' "$(sanitize "$STARTED_UTC")"
    printf '  "finished_utc": "%s",\n' "$(sanitize "$FINISHED_UTC")"
    printf '  "host_name": "%s",\n' "$(sanitize "$HOST_NAME")"
    printf '  "tart_version": "%s",\n' "$(sanitize "$TART_VERSION")"
    printf '  "stage_requested": "%s",\n' "$(sanitize "$STAGE")"
    printf '  "stages_run": "%s",\n' "$(sanitize "$STAGES_RUN")"
    printf '  "endpoint_source": "%s",\n' "$(sanitize "$ENDPOINT_SOURCE")"
    printf '  "endpoint_cidr": "/32",\n'
    if [[ "$ENDPOINT_SOURCE" == "placeholder" ]]; then
      printf '  "placeholder_note": "rfc5737-documentation-not-an-approved-model-endpoint",\n'
    else
      printf '  "placeholder_note": "",\n'
    fi
    printf '  "stage_a_policy": "softnet-default",\n'
    printf '  "stage_b_block": "0.0.0.0/0",\n'
    printf '  "stage_b_allow": "endpoint/32",\n'
    printf '  "stage_b_ran": %s,\n' "$(json_bool "$STAGE_B_RAN")"
    printf '  "interrupted": %s,\n' "$(json_bool "$INTERRUPTED")"
    printf '  "result_legend": "timeout-is-error-not-blocked",\n'
    printf '  "clone_names": ['
    first=1
    for safe_name in $CLONE_NAMES; do
      case "$safe_name" in
        ${CLONE_PREFIX}*) ;;
        *) continue ;;
      esac
      if [[ "$first" -eq 0 ]]; then
        printf ', '
      fi
      first=0
      printf '"%s"' "$(sanitize "$safe_name")"
    done
    printf '],\n'
    printf '  "probes": [\n'
    first=1
    if [[ -f "$PROBE_TSV" ]]; then
      while IFS=$'\t' read -r id stage target result elapsed detail; do
        [[ -n "$id" ]] || continue
        if [[ "$first" -eq 0 ]]; then
          printf ',\n'
        fi
        first=0
        printf '    {"probe_id":"%s","stage":"%s","target":"%s","result":"%s","elapsed_ms":%s,"detail":"%s"}' \
          "$(sanitize "$id")" "$(sanitize "$stage")" "$(sanitize "$target")" \
          "$(sanitize "$result")" "${elapsed:-0}" "$(sanitize "$detail")"
      done < "$PROBE_TSV"
      if [[ "$first" -eq 0 ]]; then
        printf '\n'
      fi
    fi
    printf '  ],\n'
    printf '  "cleanup": {"clone_prefix_checked": %s, "stop": "%s", "delete": "%s"}\n' \
      "$(json_bool "$PREFIX_OK")" "$(sanitize "$CLEANUP_STOP")" "$(sanitize "$CLEANUP_DELETE")"
    printf '}\n'
  } > "$tmp"
  mv -f "$tmp" "$OUT/report.json"

  ra=0; rb=0; re=0; ba=0; bb=0; be=0; ea=0; eb=0; ee=0
  if [[ -f "$PROBE_TSV" ]]; then
    while IFS=$'\t' read -r id stage target result elapsed detail; do
      case "$result:$stage" in
        reachable:A) ra=$((ra + 1)) ;;
        blocked:A) ba=$((ba + 1)) ;;
        error:A) ea=$((ea + 1)) ;;
        reachable:B) rb=$((rb + 1)) ;;
        blocked:B) bb=$((bb + 1)) ;;
        error:B) eb=$((eb + 1)) ;;
        reachable:C) re=$((re + 1)) ;;
        blocked:C) be=$((be + 1)) ;;
        error:C) ee=$((ee + 1)) ;;
      esac
    done < "$PROBE_TSV"
  fi
  {
    printf '# S11-0 无模型探针\n\n'
    printf -- '- status: %s\n' "$(sanitize "$STATUS")"
    printf -- '- started_utc: %s\n' "$(sanitize "$STARTED_UTC")"
    printf -- '- finished_utc: %s\n' "$(sanitize "$FINISHED_UTC")"
    printf -- '- host_name: %s\n' "$(sanitize "$HOST_NAME")"
    printf -- '- tart_version: %s\n' "$(sanitize "$TART_VERSION")"
    printf -- '- stage_requested: %s\n' "$(sanitize "$STAGE")"
    printf -- '- stages_run: %s\n' "$(sanitize "$STAGES_RUN")"
    printf -- '- endpoint: role=endpoint cidr=/32 source=%s\n' "$(sanitize "$ENDPOINT_SOURCE")"
    if [[ "$ENDPOINT_SOURCE" == "placeholder" ]]; then
      printf -- '- placeholder_note: rfc5737-documentation-not-an-approved-model-endpoint\n'
    fi
    printf -- '- stage_a_policy: softnet-default\n'
    printf -- '- stage_b_block: 0.0.0.0/0\n'
    printf -- '- stage_b_allow: endpoint/32\n'
    printf -- '- result_legend: reachable=明确连通或明确出现; blocked=明确拒绝或明确缺失; error=超时、工具失败或无法观察。超时不是 blocked。\n\n'
    printf '## 计数\n\n'
    printf '| stage | reachable | blocked | error |\n| --- | --- | --- | --- |\n'
    printf '| A | %s | %s | %s |\n' "$ra" "$ba" "$ea"
    printf '| B | %s | %s | %s |\n' "$rb" "$bb" "$eb"
    printf '| C | %s | %s | %s |\n\n' "$re" "$be" "$ee"
    printf '## 探针\n\n'
    printf '| probe_id | stage | target | result | elapsed_ms | detail |\n'
    printf '| --- | --- | --- | --- | --- | --- |\n'
    if [[ -f "$PROBE_TSV" ]]; then
      while IFS=$'\t' read -r id stage target result elapsed detail; do
        [[ -n "$id" ]] || continue
        printf '| %s | %s | %s | %s | %s | %s |\n' \
          "$(sanitize "$id")" "$(sanitize "$stage")" "$(sanitize "$target")" \
          "$(sanitize "$result")" "${elapsed:-0}" "$(sanitize "$detail")"
      done < "$PROBE_TSV"
    fi
    printf '\n## 清理\n\n'
    printf -- '- clone_prefix_checked: %s\n' "$PREFIX_OK"
    printf -- '- stop: %s\n' "$(sanitize "$CLEANUP_STOP")"
    printf -- '- delete: %s\n\n' "$(sanitize "$CLEANUP_DELETE")"
    printf '本文件只列观测。地址只写角色名（gateway / lan-redacted / metadata / public / endpoint）和策略 CIDR。是否满足 ADR 0018 §5 的拒绝类别，由人按 ADR 0018 §10 判断。\n'
  } > "$OUT/report.md"
  note "report: ${OUT}/report.json"
}

on_term() {
  INTERRUPTED=true
  exit 130
}

cleanup() {
  local rc=$?
  set +e
  trap - EXIT INT TERM
  kill_recorded "${WATCHDOG_PID:-}"
  WATCHDOG_PID=""
  kill_recorded "${CURRENT_CHILD_PID:-}"
  CURRENT_CHILD_PID=""
  destroy_owned || {
    rc=5
    if [[ "$STATUS" == "complete" ]]; then
      STATUS="partial"
    fi
  }
  if [[ "$INTERRUPTED" == "true" && "$rc" -eq 0 ]]; then
    rc=130
  fi
  write_reports
  if [[ -n "${KNOWN_HOSTS:-}" ]]; then
    rm -f "$KNOWN_HOSTS"
  fi
  if [[ -n "${TART_LOG:-}" ]]; then
    rm -f "$TART_LOG"
  fi
  exit "$rc"
}

append_stage() {
  local s=$1
  if [[ -z "$STAGES_RUN" ]]; then
    STAGES_RUN=$s
  else
    case ",${STAGES_RUN}," in
      *",${s},"*) ;;
      *) STAGES_RUN="${STAGES_RUN},${s}" ;;
    esac
  fi
}

remember_clone() {
  local n=$1
  case " ${CLONE_NAMES} " in
    *" ${n} "*) ;;
    *) CLONE_NAMES="${CLONE_NAMES} ${n}" ;;
  esac
}

find_softnet() {
  local c="" cand=""
  if [[ -n "$SOFTNET_PATH" ]]; then
    printf '%s\n' "$SOFTNET_PATH"
    return 0
  fi
  c=$(command -v softnet 2>/dev/null || true)
  if [[ -n "$c" ]]; then
    printf '%s\n' "$c"
    return 0
  fi
  for cand in /opt/homebrew/bin/softnet /usr/local/bin/softnet /usr/local/sbin/softnet; do
    if [[ -e "$cand" ]]; then
      printf '%s\n' "$cand"
      return 0
    fi
  done
  return 1
}

softnet_suid_ok() {
  local path=$1 line="" mode="" owner="" flag=""
  line=$(ls -l "$path" 2>/dev/null | awk 'NR==1 {print}') || return 1
  [[ -n "$line" ]] || return 1
  mode=$(printf '%s\n' "$line" | awk '{print $1}')
  owner=$(printf '%s\n' "$line" | awk '{print $3}')
  flag=${mode:3:1}
  [[ "$owner" == "root" ]] || return 1
  [[ "$flag" == "s" || "$flag" == "S" ]] || return 1
  return 0
}

golden_state_from_list() {
  local list=$1 name=$2
  printf '%s\n' "$list" | tr ',{}[]":' ' ' | awk -v name="$name" '
    BEGIN { state="missing" }
    {
      hit=0
      st=""
      for (i=1; i<=NF; i++) {
        if ($i==name) hit=1
        if ($i=="stopped" || $i=="Stopped" || $i=="stop") st="stopped"
        if ($i=="running" || $i=="Running") st="running"
      }
      if (hit) {
        if (st=="") state="unknown"
        else state=st
      }
    }
    END { printf "%s", state }
  '
}

tart_version_only() {
  local raw=$1 v=""
  v=$(printf '%s\n' "$raw" | awk 'NF{print; exit}' | tr -c '0-9.' ' ' | awk '{print $1}')
  case "$v" in
    [0-9]*) printf '%s' "$v" ;;
    *) printf 'unknown' ;;
  esac
}

precheck() {
  local ok=1 ver_raw="" ver_rc=0 sn="" list_raw="" list_rc=0 st="" avail="" df_raw=""
  local euid=""
  euid=$(id -u 2>/dev/null || echo unknown)
  if [[ "$euid" == "0" ]]; then
    record_probe "precheck-root" "precheck" "host" "error" 0 "euid-0"
    ok=0
  else
    record_probe "precheck-root" "precheck" "host" "reachable" 0 "not-root"
  fi
  if ! command -v tart >/dev/null 2>&1; then
    if [[ -d "${HOME}/bin" ]]; then
      PATH="${HOME}/bin:${PATH}"
      export PATH
    fi
  fi
  set +e
  ver_raw=$(tart --version 2>&1)
  ver_rc=$?
  set -e
  if [[ "$ver_rc" -eq 0 ]]; then
    TART_VERSION=$(tart_version_only "$ver_raw")
    record_probe "precheck-tart" "precheck" "host" "reachable" 0 "version-ok"
  else
    record_probe "precheck-tart" "precheck" "host" "error" 0 "not-found"
    ok=0
  fi
  sn=$(find_softnet || true)
  if [[ -z "$sn" || ! -e "$sn" ]]; then
    record_probe "precheck-softnet" "precheck" "host" "error" 0 "not-found"
    ok=0
  elif softnet_suid_ok "$sn"; then
    record_probe "precheck-softnet" "precheck" "host" "reachable" 0 "suid-root"
  else
    record_probe "precheck-softnet" "precheck" "host" "error" 0 "not-suid"
    ok=0
  fi
  if [[ "$ver_rc" -ne 0 ]]; then
    record_probe "precheck-golden" "precheck" "host" "error" 0 "list-failed"
    ok=0
  else
    set +e
    list_raw=$(tart list 2>&1)
    list_rc=$?
    set -e
    if [[ "$list_rc" -ne 0 ]]; then
      record_probe "precheck-golden" "precheck" "host" "error" 0 "list-failed"
      ok=0
    else
      st=$(golden_state_from_list "$list_raw" "$GOLDEN")
      case "$st" in
        stopped)
          record_probe "precheck-golden" "precheck" "host" "reachable" 0 "stopped"
          ;;
        running)
          record_probe "precheck-golden" "precheck" "host" "error" 0 "running"
          ok=0
          ;;
        missing)
          record_probe "precheck-golden" "precheck" "host" "error" 0 "missing"
          ok=0
          ;;
        *)
          record_probe "precheck-golden" "precheck" "host" "error" 0 "unknown"
          ok=0
          ;;
      esac
    fi
  fi
  if [[ ! -d "$TART_HOME" ]]; then
    record_probe "precheck-disk" "precheck" "host" "error" 0 "tart-home-missing"
    ok=0
  else
    set +e
    df_raw=$(df -kP "$TART_HOME" 2>/dev/null)
    set -e
    avail=$(printf '%s\n' "$df_raw" | awk 'NR==2 {print $4}')
    case "$avail" in
      ''|*[!0-9]*)
        record_probe "precheck-disk" "precheck" "host" "error" 0 "disk-unparsed"
        ok=0
        ;;
      *)
        if [[ "$avail" -lt $((MIN_FREE_GB * 1024 * 1024)) ]]; then
          record_probe "precheck-disk" "precheck" "host" "error" 0 "low"
          ok=0
        else
          record_probe "precheck-disk" "precheck" "host" "reachable" 0 "enough"
        fi
        ;;
    esac
  fi
  if [[ ! -f "$SSH_KEY" || ! -r "$SSH_KEY" ]]; then
    record_probe "precheck-ssh-key" "precheck" "host" "error" 0 "unreadable"
    ok=0
  else
    record_probe "precheck-ssh-key" "precheck" "host" "reachable" 0 "readable"
  fi
  if [[ "$ok" -eq 1 ]]; then
    return 0
  fi
  return 1
}

wait_for_ip() {
  local deadline=0 now=0 ip="" rc=0
  disposable_ok "$OWNED_CLONE" || return 1
  deadline=$(( $(date +%s) + BOOT_TIMEOUT ))
  while true; do
    now=$(date +%s)
    if [[ "$now" -ge "$deadline" || "$now" -ge "$DEADLINE" ]]; then
      return 124
    fi
    if [[ -n "$TART_RUN_PID" ]] && ! kill -0 "$TART_RUN_PID" 2>/dev/null; then
      return 125
    fi
    set +e
    ip=$(tart ip "$OWNED_CLONE" 2>/dev/null | awk 'NF{print; exit}')
    rc=$?
    set -e
    ip=$(printf '%s' "$ip" | tr -d '[:space:]')
    if [[ "$rc" -eq 0 ]] && is_ipv4_syntax "$ip"; then
      GUEST_IP=$ip
      return 0
    fi
    sleep 1
  done
}

ssh_opts_init() {
  KNOWN_HOSTS=$(mktemp)
  SSH_OPTS=(
    -i "$SSH_KEY"
    -o BatchMode=yes
    -o IdentitiesOnly=yes
    -o PreferredAuthentications=publickey
    -o StrictHostKeyChecking=accept-new
    -o "UserKnownHostsFile=${KNOWN_HOSTS}"
    -o GlobalKnownHostsFile=/dev/null
    -o ConnectTimeout=8
    -o LogLevel=ERROR
    -o CheckHostIP=no
  )
}

ssh_up() {
  local deadline=0 now=0 rc=0
  deadline=$(( $(date +%s) + SSH_WAIT ))
  while true; do
    now=$(date +%s)
    if [[ "$now" -ge "$deadline" || "$now" -ge "$DEADLINE" ]]; then
      return 1
    fi
    set +e
    ssh "${SSH_OPTS[@]}" "${GUEST_USER}@${GUEST_IP}" -- /usr/bin/true </dev/null >/dev/null 2>&1
    rc=$?
    set -e
    if [[ "$rc" -eq 0 ]]; then
      return 0
    fi
    sleep 1
  done
}

guest_ids_for_stage() {
  local p="" oldifs=""
  printf '%s\n' "gateway-icmp"
  oldifs=$IFS
  IFS=','
  # shellcheck disable=SC2086
  set -- $PORTS_CSV
  IFS=$oldifs
  for p in "$@"; do
    printf 'gateway-tcp-%s\n' "$p"
  done
  printf '%s\n' \
    "dns-udp-gateway" \
    "dns-udp-public" \
    "ipv6-addr" \
    "ipv6-tcp-public" \
    "endpoint-tcp-443" \
    "endpoint-udp-443" \
    "lan-icmp" \
    "lan-tcp-443" \
    "metadata-icmp" \
    "metadata-tcp-80" \
    "metadata-tcp-443"
}

ingest_guest_output() {
  local file=$1 stage=$2
  local line="" id="" result="" elapsed="" detail=""
  while IFS= read -r line; do
    case "$line" in
      PROBE\ *) ;;
      *) continue ;;
    esac
    set -f
    # shellcheck disable=SC2086
    set -- $line
    set +f
    id=${2:-}
    result=${3:-}
    elapsed=${4:-0}
    detail=${5:-none}
    case "$id" in
      *[!A-Za-z0-9_-]*|'') continue ;;
    esac
    case "$result" in
      reachable|blocked|error) ;;
      *) continue ;;
    esac
    record_probe "$id" "$stage" "$(target_for "$id")" "$result" "$elapsed" "$detail"
  done < "$file"
}

fill_missing_guest() {
  local stage=$1 id=""
  while IFS= read -r id; do
    [[ -n "$id" ]] || continue
    if ! have_probe "$id" "$stage"; then
      record_probe "$id" "$stage" "$(target_for "$id")" "error" 0 "no-observation"
    fi
  done <<EOF
$(guest_ids_for_stage)
EOF
}

run_guest_net() {
  local stage=$1
  local out="" rc=0 args=()
  out=$(mktemp)
  args=(--guest --stage "$stage" --ep "$ENDPOINT_IP" --pub "$PUBLIC_RESOLVER" --ports "$PORTS_CSV")
  if [[ -n "$LAN_IP" ]]; then
    args+=(--lan "$LAN_IP")
  fi
  set +e
  run_bounded_io "$PROBE_TIMEOUT" "$SCRIPT_SELF" "$out" \
    ssh "${SSH_OPTS[@]}" "${GUEST_USER}@${GUEST_IP}" -- bash -s -- "${args[@]}"
  rc=$?
  set -e
  ingest_guest_output "$out" "$stage"
  rm -f "$out"
  fill_missing_guest "$stage"
  return "$rc"
}

begin_clone() {
  local name=$1
  disposable_ok "$name" || return 1
  OWNED_CLONE=$name
  remember_clone "$name"
  if ! run_bounded "$CLONE_TIMEOUT" tart clone "$GOLDEN" "$name"; then
    return 1
  fi
  return 0
}

detect_lan() {
  local iface="" ip=""
  if [[ -n "$LAN_IP_ARG" ]]; then
    LAN_IP=$LAN_IP_ARG
    return 0
  fi
  LAN_IP=""
  if [[ "$(uname -s)" != "Darwin" ]]; then
    return 0
  fi
  set +e
  iface=$(route -n get default 2>/dev/null | awk '/interface:/{print $2; exit}')
  if [[ -n "$iface" ]]; then
    ip=$(ipconfig getifaddr "$iface" 2>/dev/null || true)
  fi
  set -e
  ip=$(printf '%s' "$ip" | tr -d '[:space:]')
  if is_ipv4_syntax "$ip"; then
    LAN_IP=$ip
  fi
}

run_net_stage() {
  local stage=$1 name=$2 policy=$3
  local start_ms="" rc=0
  append_stage "$stage"
  if [[ "$stage" == "B" ]]; then
    STAGE_B_RAN=true
  fi
  note "stage ${stage}"
  if ! begin_clone "$name"; then
    record_probe "clone" "$stage" "host" "error" 0 "clone-failed"
    record_probe "host-tart-ip" "$stage" "host" "error" 0 "not-attempted"
    record_probe "host-ssh-channel" "$stage" "host" "error" 0 "not-attempted"
    fill_missing_guest "$stage"
    return 1
  fi
  if [[ "$policy" == "block" ]]; then
    spawn_tart_run tart run "$name" --no-graphics --net-softnet \
      --net-softnet-block=0.0.0.0/0 \
      --net-softnet-allow="${ENDPOINT_IP}/32"
  else
    spawn_tart_run tart run "$name" --no-graphics --net-softnet
  fi
  start_ms=$(now_ms)
  # `if ! cmd` resets $? to 0, so keep the raw wait status.
  rc=0
  wait_for_ip || rc=$?
  if [[ "$rc" -ne 0 ]]; then
    if [[ "$rc" -eq 125 ]]; then
      record_probe "host-tart-ip" "$stage" "host" "error" 0 "tart-run-exited"
    else
      record_probe "host-tart-ip" "$stage" "host" "error" 0 "ip-unavailable"
    fi
    record_probe "host-ssh-channel" "$stage" "host" "error" 0 "not-attempted"
    fill_missing_guest "$stage"
    return 1
  fi
  record_probe "host-tart-ip" "$stage" "host" "reachable" "$(( $(now_ms) - start_ms ))" "ipv4-assigned"
  start_ms=$(now_ms)
  if ! ssh_up; then
    record_probe "host-ssh-channel" "$stage" "host" "error" "$(( $(now_ms) - start_ms ))" "ssh-session-failed"
    fill_missing_guest "$stage"
    return 1
  fi
  record_probe "host-ssh-channel" "$stage" "host" "reachable" "$(( $(now_ms) - start_ms ))" "ssh-session-exit-0"
  run_guest_net "$stage" || true
  return 0
}

help_keywords() {
  local file=$1 word="" found=""
  for word in exec softnet vsock serial agent; do
    if grep -q -w "$word" "$file" 2>/dev/null; then
      found="${found}${found:+,}${word}=yes"
    else
      found="${found}${found:+,}${word}=no"
    fi
  done
  printf '%s' "$found"
}

help_subcommands() {
  local file=$1 n="" found=""
  for n in clone create delete exec get ip list login logout prune pull push rename run set stop suspend version; do
    if grep -q -w "$n" "$file" 2>/dev/null; then
      found="${found}${found:+,}${n}"
    fi
  done
  printf '%s' "$found"
}

boot_for_exec() {
  local name=$1 start_ms="" rc=0
  if ! begin_clone "$name"; then
    record_probe "clone" "C" "host" "error" 0 "clone-failed"
    record_probe "host-tart-ip" "C" "host" "error" 0 "not-attempted"
    record_probe "host-ssh-channel" "C" "host" "error" 0 "not-attempted"
    return 1
  fi
  spawn_tart_run tart run "$name" --no-graphics --net-softnet
  start_ms=$(now_ms)
  rc=0
  wait_for_ip || rc=$?
  if [[ "$rc" -ne 0 ]]; then
    if [[ "$rc" -eq 125 ]]; then
      record_probe "host-tart-ip" "C" "host" "error" 0 "tart-run-exited"
    else
      record_probe "host-tart-ip" "C" "host" "error" 0 "ip-unavailable"
    fi
    record_probe "host-ssh-channel" "C" "host" "error" 0 "not-attempted"
    return 1
  fi
  record_probe "host-tart-ip" "C" "host" "reachable" "$(( $(now_ms) - start_ms ))" "ipv4-assigned"
  start_ms=$(now_ms)
  if ! ssh_up; then
    record_probe "host-ssh-channel" "C" "host" "error" "$(( $(now_ms) - start_ms ))" "ssh-session-failed"
    return 1
  fi
  record_probe "host-ssh-channel" "C" "host" "reachable" "$(( $(now_ms) - start_ms ))" "ssh-session-exit-0"
  return 0
}

run_stage_c() {
  local help_main="" help_exec="" help_run="" combined="" rc=0 subs="" keys=""
  local out=""
  append_stage "C"
  note "stage C"
  help_main=$(mktemp)
  help_exec=$(mktemp)
  help_run=$(mktemp)
  combined=$(mktemp)
  set +e
  capture_bounded 30 "$help_main" tart --help
  rc=$?
  set -e
  if [[ "$rc" -ne 0 ]]; then
    record_probe "tart-help-subcommands" "C" "host" "error" 0 "exit-${rc}"
  else
    subs=$(help_subcommands "$help_main")
    [[ -n "$subs" ]] || subs="none"
    record_probe "tart-help-subcommands" "C" "host" "reachable" 0 "subcommands=${subs}"
  fi
  set +e
  capture_bounded 30 "$help_exec" tart exec --help
  rc=$?
  set -e
  if [[ "$rc" -eq 0 ]]; then
    record_probe "tart-exec-help" "C" "host" "reachable" 0 "exit-0"
  elif [[ "$rc" -eq 124 ]]; then
    record_probe "tart-exec-help" "C" "host" "error" 0 "timeout"
  else
    record_probe "tart-exec-help" "C" "host" "error" 0 "exit-${rc}"
  fi
  set +e
  capture_bounded 30 "$help_run" tart run --help
  set -e
  cat "$help_main" "$help_exec" "$help_run" > "$combined" 2>/dev/null || true
  keys=$(help_keywords "$combined")
  [[ -n "$keys" ]] || keys="none"
  record_probe "tart-help-keywords" "C" "host" "reachable" 0 "${keys}"
  rm -f "$help_main" "$help_exec" "$help_run" "$combined"
  if [[ -z "${OWNED_CLONE:-}" ]] || ! disposable_ok "$OWNED_CLONE"; then
    record_probe "tart-exec-true" "C" "host" "error" 0 "no-clone"
    record_probe "guest-vsock" "C" "guest" "error" 0 "no-clone"
    record_probe "guest-virtio" "C" "guest" "error" 0 "no-clone"
    record_probe "guest-serial" "C" "guest" "error" 0 "no-clone"
    return 1
  fi
  set +e
  run_bounded 30 tart exec "$OWNED_CLONE" /usr/bin/true >/dev/null 2>&1
  rc=$?
  set -e
  if [[ "$rc" -eq 0 ]]; then
    record_probe "tart-exec-true" "C" "host" "reachable" 0 "exit-0"
  elif [[ "$rc" -eq 124 ]]; then
    record_probe "tart-exec-true" "C" "host" "error" 0 "timeout"
  else
    record_probe "tart-exec-true" "C" "host" "error" 0 "exit-${rc}"
  fi
  if [[ -z "${GUEST_IP:-}" ]]; then
    record_probe "guest-vsock" "C" "guest" "error" 0 "no-ssh"
    record_probe "guest-virtio" "C" "guest" "error" 0 "no-ssh"
    record_probe "guest-serial" "C" "guest" "error" 0 "no-ssh"
    return 1
  fi
  out=$(mktemp)
  set +e
  run_bounded_io "$PROBE_TIMEOUT" "$SCRIPT_SELF" "$out" \
    ssh "${SSH_OPTS[@]}" "${GUEST_USER}@${GUEST_IP}" -- bash -s -- --guest --inventory
  rc=$?
  set -e
  ingest_guest_output "$out" "C"
  rm -f "$out"
  if ! have_probe "guest-vsock" "C"; then
    record_probe "guest-vsock" "C" "guest" "error" 0 "no-observation"
  fi
  if ! have_probe "guest-virtio" "C"; then
    record_probe "guest-virtio" "C" "guest" "error" 0 "no-observation"
  fi
  if ! have_probe "guest-serial" "C"; then
    record_probe "guest-serial" "C" "guest" "error" 0 "no-observation"
  fi
  if [[ "$rc" -ne 0 && "$rc" -ne 124 ]]; then
    return 1
  fi
  if [[ "$rc" -eq 124 ]]; then
    return 1
  fi
  return 0
}

print_dry_run() {
  local name_a=$1 name_b=$2
  printf 's11-0-probe: dry-run; no command will be executed\n'
  printf '# endpoint source: %s\n' "$ENDPOINT_SOURCE"
  if [[ "$ENDPOINT_SOURCE" == "placeholder" ]]; then
    printf '# placeholder is RFC5737 documentation space, not an approved model endpoint\n'
  fi
  printf '# delete is limited to the s11-0-probe- clone this process creates\n'
  printf 'tart --version\n'
  printf 'ls -l <softnet>\n'
  printf 'tart list\n'
  printf 'df -kP <tart-home>\n'
  if [[ "$STAGE" == "A" || "$STAGE" == "all" ]]; then
    printf 'tart clone %s %s\n' "$GOLDEN" "$name_a"
    printf 'tart run %s --no-graphics --net-softnet\n' "$name_a"
    printf 'tart ip %s\n' "$name_a"
    printf 'ssh -i <ssh-key> -o BatchMode=yes -o PreferredAuthentications=publickey %s@<guest-ip> -- /usr/bin/true\n' "$GUEST_USER"
    printf 'ssh -i <ssh-key> -o BatchMode=yes %s@<guest-ip> -- bash -s -- --guest --stage A\n' "$GUEST_USER"
    printf 'tart stop %s\n' "$name_a"
    printf 'tart delete %s\n' "$name_a"
  fi
  if [[ "$STAGE" == "B" || "$STAGE" == "all" ]]; then
    printf 'tart clone %s %s\n' "$GOLDEN" "$name_b"
    printf 'tart run %s --no-graphics --net-softnet --net-softnet-block=0.0.0.0/0 --net-softnet-allow=%s/32\n' \
      "$name_b" "$ENDPOINT_IP"
    printf 'tart ip %s\n' "$name_b"
    printf 'ssh -i <ssh-key> -o BatchMode=yes %s@<guest-ip> -- bash -s -- --guest --stage B\n' "$GUEST_USER"
  fi
  if [[ "$STAGE" == "C" || "$STAGE" == "all" ]]; then
    if [[ "$STAGE" == "C" ]]; then
      printf 'tart clone %s %s\n' "$GOLDEN" "$name_a"
      printf 'tart run %s --no-graphics --net-softnet\n' "$name_a"
    fi
    printf 'tart --help\n'
    printf 'tart exec --help\n'
    printf 'tart run --help\n'
    if [[ "$STAGE" == "C" ]]; then
      printf 'tart exec %s /usr/bin/true\n' "$name_a"
    else
      printf 'tart exec %s /usr/bin/true\n' "$name_b"
    fi
  fi
  if [[ "$STAGE" == "B" || "$STAGE" == "all" ]]; then
    printf 'tart stop %s\n' "$name_b"
    printf 'tart delete %s\n' "$name_b"
  elif [[ "$STAGE" == "C" ]]; then
    printf 'tart stop %s\n' "$name_a"
    printf 'tart delete %s\n' "$name_a"
  fi
}

seconds_left() {
  local now=0
  now=$(date +%s)
  printf '%s\n' $((DEADLINE - now))
}

host_main() {
  local euid="" name_a="" name_b="" hn=""
  DRY_RUN=0
  STAGE="all"
  ENDPOINT_IP=""
  ENDPOINT_SOURCE="placeholder"
  PORTS_CSV="22,53,80,443"
  OUT=""
  GOLDEN="macos-s2-eval"
  TART_HOME="$DEFAULT_TART_HOME"
  SOFTNET_PATH=""
  GUEST_USER="eval"
  SSH_KEY="${HOME}/.ssh/id_ed25519_tart"
  CLONE_NAME_ARG=""
  MIN_FREE_GB=10
  BOOT_TIMEOUT=300
  OVERALL_TIMEOUT=2400
  CLONE_TIMEOUT=600
  SSH_WAIT=90
  PROBE_TIMEOUT=180
  STOP_TIMEOUT=60
  DELETE_TIMEOUT=120
  PUBLIC_RESOLVER="1.1.1.1"
  LAN_IP_ARG=""
  LAN_IP=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --help|-h) usage; exit 0 ;;
      --dry-run) DRY_RUN=1; shift ;;
      --stage)
        [[ $# -ge 2 ]] || die 1 "missing value"
        STAGE=$2; shift 2 ;;
      --endpoint-ip)
        [[ $# -ge 2 ]] || die 1 "missing value"
        ENDPOINT_IP=$2; shift 2 ;;
      --ports)
        [[ $# -ge 2 ]] || die 1 "missing value"
        PORTS_CSV=$2; shift 2 ;;
      --out)
        [[ $# -ge 2 ]] || die 1 "missing value"
        OUT=$2; shift 2 ;;
      --golden)
        [[ $# -ge 2 ]] || die 1 "missing value"
        GOLDEN=$2; shift 2 ;;
      --tart-home)
        [[ $# -ge 2 ]] || die 1 "missing value"
        TART_HOME=$2; shift 2 ;;
      --softnet)
        [[ $# -ge 2 ]] || die 1 "missing value"
        SOFTNET_PATH=$2; shift 2 ;;
      --guest-user)
        [[ $# -ge 2 ]] || die 1 "missing value"
        GUEST_USER=$2; shift 2 ;;
      --ssh-key)
        [[ $# -ge 2 ]] || die 1 "missing value"
        SSH_KEY=$2; shift 2 ;;
      --clone-name)
        [[ $# -ge 2 ]] || die 1 "missing value"
        CLONE_NAME_ARG=$2; shift 2 ;;
      --lan-ip)
        [[ $# -ge 2 ]] || die 1 "missing value"
        LAN_IP_ARG=$2; shift 2 ;;
      --public-resolver)
        [[ $# -ge 2 ]] || die 1 "missing value"
        PUBLIC_RESOLVER=$2; shift 2 ;;
      --min-free-gb)
        [[ $# -ge 2 ]] || die 1 "missing value"
        MIN_FREE_GB=$2; shift 2 ;;
      --overall-timeout)
        [[ $# -ge 2 ]] || die 1 "missing value"
        OVERALL_TIMEOUT=$2; shift 2 ;;
      --boot-timeout)
        [[ $# -ge 2 ]] || die 1 "missing value"
        BOOT_TIMEOUT=$2; shift 2 ;;
      --clone-timeout)
        [[ $# -ge 2 ]] || die 1 "missing value"
        CLONE_TIMEOUT=$2; shift 2 ;;
      --ssh-wait)
        [[ $# -ge 2 ]] || die 1 "missing value"
        SSH_WAIT=$2; shift 2 ;;
      --probe-timeout)
        [[ $# -ge 2 ]] || die 1 "missing value"
        PROBE_TIMEOUT=$2; shift 2 ;;
      --stop-timeout)
        [[ $# -ge 2 ]] || die 1 "missing value"
        STOP_TIMEOUT=$2; shift 2 ;;
      --delete-timeout)
        [[ $# -ge 2 ]] || die 1 "missing value"
        DELETE_TIMEOUT=$2; shift 2 ;;
      *)
        die 1 "unknown argument"
        ;;
    esac
  done

  case "$STAGE" in
    A|B|C|all) ;;
    *) die 1 "bad stage" ;;
  esac
  case "$GOLDEN" in
    ''|*[!A-Za-z0-9._-]*) die 1 "bad golden name" ;;
  esac
  case "$GUEST_USER" in
    ''|*[!A-Za-z0-9._-]*) die 1 "bad guest user" ;;
  esac
  if ! normalize_ports "$PORTS_CSV"; then
    die 1 "bad ports"
  fi
  require_int "$MIN_FREE_GB" || die 1 "bad integer"
  require_int "$OVERALL_TIMEOUT" || die 1 "bad integer"
  require_int "$BOOT_TIMEOUT" || die 1 "bad integer"
  require_int "$CLONE_TIMEOUT" || die 1 "bad integer"
  require_int "$SSH_WAIT" || die 1 "bad integer"
  require_int "$PROBE_TIMEOUT" || die 1 "bad integer"
  require_int "$STOP_TIMEOUT" || die 1 "bad integer"
  require_int "$DELETE_TIMEOUT" || die 1 "bad integer"
  if [[ "$OVERALL_TIMEOUT" -lt 30 || "$BOOT_TIMEOUT" -lt 1 ]]; then
    die 1 "bad integer"
  fi
  if [[ -z "$ENDPOINT_IP" ]]; then
    ENDPOINT_IP=$PLACEHOLDER_ENDPOINT
    ENDPOINT_SOURCE="placeholder"
  else
    ENDPOINT_SOURCE="argument"
  fi
  if ! is_public_endpoint "$ENDPOINT_IP"; then
    die 1 "endpoint ip is not a public ipv4"
  fi
  if ! is_public_endpoint "$PUBLIC_RESOLVER"; then
    die 1 "public resolver is not a public ipv4"
  fi
  if [[ -n "$LAN_IP_ARG" ]] && ! is_ipv4_syntax "$LAN_IP_ARG"; then
    die 1 "lan ip is not an ipv4 address"
  fi
  if [[ -n "$CLONE_NAME_ARG" ]]; then
    if ! valid_clone_name "$CLONE_NAME_ARG"; then
      die 1 "clone name must use the disposable prefix"
    fi
    name_a=$CLONE_NAME_ARG
    name_b="${CLONE_NAME_ARG}b"
    if ! valid_clone_name "$name_b"; then
      die 1 "clone name must use the disposable prefix"
    fi
  else
    name_a=$(make_clone_name) || die 1 "could not mint a clone name"
    name_b=$(make_clone_name) || die 1 "could not mint a clone name"
  fi
  if [[ "$GOLDEN" == "$name_a" || "$GOLDEN" == "$name_b" ]]; then
    die 1 "refusing golden and clone with the same name"
  fi
  case "$GOLDEN" in
    ${CLONE_PREFIX}*)
      die 1 "refusing golden name that uses the disposable clone prefix"
      ;;
  esac
  if [[ "$name_a" == "$name_b" ]]; then
    die 1 "refusing golden and clone with the same name"
  fi

  euid=$(id -u 2>/dev/null || echo unknown)
  if [[ "$euid" == "0" ]]; then
    printf 's11-0-probe: refusing to run as root\n' >&2
    exit 2
  fi
  if [[ "$DRY_RUN" -eq 1 ]]; then
    print_dry_run "$name_a" "$name_b"
    exit 0
  fi

  export TART_HOME
  ensure_sbin_path
  if ! command -v tart >/dev/null 2>&1; then
    PATH="${HOME}/bin:${PATH}"
    export PATH
  fi

  STATUS="partial"
  DO_REPORT=0
  PREFIX_OK=true
  INTERRUPTED=false
  STAGE_B_RAN=false
  STAGES_RUN=""
  CLONE_NAMES=""
  OWNED_CLONE=""
  TART_RUN_PID=""
  CURRENT_CHILD_PID=""
  WATCHDOG_PID=""
  TART_LOG=""
  KNOWN_HOSTS=""
  GUEST_IP=""
  CLEANUP_STOP="skipped"
  CLEANUP_DELETE="skipped"
  TART_VERSION="unknown"
  HOST_NAME="unknown"
  STARTED_UTC=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  FINISHED_UTC=""
  START_EPOCH=$(date +%s)
  DEADLINE=$((START_EPOCH + OVERALL_TIMEOUT))
  PROBE_TSV=$(mktemp)
  SCRIPT_SELF=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")
  if [[ -z "$OUT" ]]; then
    OUT="./s11-0-$(date -u +%Y%m%dT%H%M%SZ)"
  fi
  hn=$(hostname -s 2>/dev/null || hostname 2>/dev/null || true)
  hn=$(printf '%s' "$hn" | tr -c 'A-Za-z0-9._-' '_')
  hn=$(printf '%.40s' "$hn")
  [[ -n "$hn" ]] && HOST_NAME=$hn

  trap on_term INT TERM
  trap cleanup EXIT
  umask 077
  mkdir -p "$OUT"
  chmod 700 "$OUT" 2>/dev/null || true
  DO_REPORT=1
  ssh_opts_init
  if command -v setsid >/dev/null 2>&1; then
    setsid bash -c 'sleep "$1"; kill -TERM "$2"' _ "$OVERALL_TIMEOUT" "$$" >/dev/null 2>&1 &
  else
    bash -c 'sleep "$1"; kill -TERM "$2"' _ "$OVERALL_TIMEOUT" "$$" >/dev/null 2>&1 &
  fi
  WATCHDOG_PID=$!

  if ! precheck; then
    STATUS="precheck-failed"
    note "precheck failed"
    exit 2
  fi
  note "precheck ok"
  detect_lan

  case "$STAGE" in
    A)
      if ! run_net_stage "A" "$name_a" "default"; then
        exit 4
      fi
      ;;
    B)
      if ! run_net_stage "B" "$name_b" "block"; then
        exit 4
      fi
      ;;
    C)
      # Boot a disposable clone so tart exec and guest device reads have a
      # target. Do not run the egress sweep in this stage.
      boot_for_exec "$name_a" || true
      run_stage_c || exit 4
      ;;
    all)
      if ! run_net_stage "A" "$name_a" "default"; then
        exit 4
      fi
      if ! destroy_owned; then
        exit 5
      fi
      if [[ "$(seconds_left)" -lt 120 ]]; then
        exit 4
      fi
      if ! run_net_stage "B" "$name_b" "block"; then
        exit 4
      fi
      if ! run_stage_c; then
        exit 4
      fi
      ;;
  esac
  STATUS="complete"
  exit 0
}

if [[ "${1:-}" == "--guest" ]]; then
  guest_main "$@"
else
  host_main "$@"
fi

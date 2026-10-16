#!/usr/bin/env bash
# Runs Wayland driver contract test suite against gcu-fixture in a headless GNOME Wayland session.
# If required components (mutter, xdg-desktop-portal-gnome, pipewire) are missing,
# truthfully outputs not_run and exits with agreed code (0) so CI does not turn red.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

NOT_RUN_EXIT_CODE="${NOT_RUN_EXIT_CODE:-0}"

# 1. Environment detection: mutter, xdg-desktop-portal (or backend), pipewire
MISSING_DEPS=()
if ! command -v mutter >/dev/null 2>&1; then
  MISSING_DEPS+=("mutter")
fi

PORTAL_FOUND=0
for p in /usr/libexec/xdg-desktop-portal-gnome /usr/lib/xdg-desktop-portal-gnome /usr/libexec/xdg-desktop-portal /usr/lib/xdg-desktop-portal; do
  if [ -x "$p" ]; then
    PORTAL_FOUND=1
    break
  fi
done
if [ "$PORTAL_FOUND" -eq 0 ] && ! command -v xdg-desktop-portal >/dev/null 2>&1; then
  MISSING_DEPS+=("xdg-desktop-portal (or xdg-desktop-portal-gnome)")
fi

if ! command -v pipewire >/dev/null 2>&1; then
  MISSING_DEPS+=("pipewire")
fi

if [ "${#MISSING_DEPS[@]}" -gt 0 ]; then
  echo "not_run: missing required Wayland environment: ${MISSING_DEPS[*]}"
  echo "======================================================================"
  echo "SUMMARY (WAYLAND-CI): 0 passed, 0 skipped, 1 not_run, 0 failed"
  echo "======================================================================"
  exit "$NOT_RUN_EXIT_CODE"
fi

# 2. Setup headless GNOME Wayland session with mutter
echo "Starting PipeWire daemon if not active..."
if ! pgrep -x pipewire >/dev/null 2>&1; then
  pipewire &
  PIPEWIRE_PID=$!
else
  PIPEWIRE_PID=""
fi

export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/tmp/gcu-wayland-runtime-$$}"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 0700 "$XDG_RUNTIME_DIR"

echo "Starting mutter in headless virtual-monitor mode..."
mutter --headless --virtual-monitor 1280x1024 --wayland &
MUTTER_PID=$!

cleanup() {
  echo "Cleaning up Wayland CI background processes..."
  if [ -n "${FIXTURE_PID:-}" ]; then
    kill "$FIXTURE_PID" 2>/dev/null || true
  fi
  if [ -n "${MUTTER_PID:-}" ]; then
    kill "$MUTTER_PID" 2>/dev/null || true
  fi
  if [ -n "${PIPEWIRE_PID:-}" ]; then
    kill "$PIPEWIRE_PID" 2>/dev/null || true
  fi
  if [ -n "${ATSPI_PID:-}" ]; then
    kill "$ATSPI_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

# Wait for Wayland display socket
for i in $(seq 1 40); do
  if ls "$XDG_RUNTIME_DIR"/wayland-* >/dev/null 2>&1; then
    WAYLAND_SOCK=$(ls "$XDG_RUNTIME_DIR"/wayland-* | head -n 1)
    export WAYLAND_DISPLAY=$(basename "$WAYLAND_SOCK")
    echo "Wayland display socket ready: $WAYLAND_DISPLAY"
    break
  fi
  sleep 0.2
done

if [ -z "${WAYLAND_DISPLAY:-}" ]; then
  echo "not_run: mutter failed to create WAYLAND_DISPLAY socket"
  echo "SUMMARY (WAYLAND-CI): 0 passed, 0 skipped, 1 not_run, 0 failed"
  exit "$NOT_RUN_EXIT_CODE"
fi

export XDG_SESSION_TYPE=wayland
export LIBGL_ALWAYS_SOFTWARE=1
export GALLIUM_DRIVER=llvmpipe
export GCU_FIXTURE_STATE="${GCU_FIXTURE_STATE:-/tmp/fixture_state.json}"

# Start fixture if not running
if ! pgrep -f "target/debug/gcu-fixture" >/dev/null 2>&1; then
  echo "Launching gcu-fixture in background..."
  rm -f "$GCU_FIXTURE_STATE" /tmp/fixture.log
  ./target/debug/gcu-fixture > /tmp/fixture.log 2>&1 &
  FIXTURE_PID=$!
else
  FIXTURE_PID=""
fi

for i in $(seq 1 50); do
  if [ -f "$GCU_FIXTURE_STATE" ]; then
    echo "gcu-fixture state ready at $GCU_FIXTURE_STATE"
    break
  fi
  sleep 0.2
done

echo "Executing Wayland contract test suite..."
cargo test --locked -p gcu-driver-linux --test wayland_contract -- --ignored --test-threads=1 --nocapture

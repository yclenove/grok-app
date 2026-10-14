#!/usr/bin/env bash
# Runs Linux real driver smoke test against gcu-fixture in an active desktop session.
# As documented in gcu-cli runtime, CLI currently only wires the fake driver.
# Therefore, smoke runs the full contract suite against the live Linux driver:
#   cargo test -p gcu-driver-linux --test contract -- --ignored --test-threads=1
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

# Ensure openbox is running if DISPLAY is set
if [ -n "${DISPLAY:-}" ] && ! pgrep -x openbox >/dev/null 2>&1; then
  echo "Starting openbox..."
  openbox &
  OPENBOX_PID=$!
else
  OPENBOX_PID=""
fi

# Ensure at-spi-bus-launcher is active if D-Bus is present
if ! busctl --user status org.a11y.Bus >/dev/null 2>&1; then
  ATSPI_LAUNCHER=$(which at-spi-bus-launcher 2>/dev/null || find /usr/lib* /usr/libexec -name at-spi-bus-launcher 2>/dev/null | head -n 1)
  if [ -n "$ATSPI_LAUNCHER" ]; then
    echo "Starting at-spi-bus-launcher: $ATSPI_LAUNCHER"
    "$ATSPI_LAUNCHER" --launch-immediately --a11y=1 --screen-reader=1 &
    ATSPI_PID=$!
    for i in $(seq 1 40); do
      if busctl --user status org.a11y.Bus >/dev/null 2>&1; then
        echo "AT-SPI bus ready"
        break
      fi
      sleep 0.2
    done
  else
    echo "WARNING: at-spi-bus-launcher not found" >&2
    ATSPI_PID=""
  fi
else
  ATSPI_PID=""
fi

# Accessibility settings
gsettings set org.gnome.desktop.a11y.applications screen-reader-enabled true 2>/dev/null || true
gsettings set org.gnome.desktop.interface toolkit-accessibility true 2>/dev/null || true
busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true 2>/dev/null || true
busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status ScreenReaderEnabled b true 2>/dev/null || true

if [ -z "${AT_SPI_BUS_ADDRESS:-}" ]; then
  BUS_RAW=$(busctl --user call org.a11y.Bus /org/a11y/bus org.a11y.Bus GetAddress 2>/dev/null || true)
  ADDR=$(echo "$BUS_RAW" | awk '{print $2}' | tr -d '"')
  if [ -n "$ADDR" ]; then
    export AT_SPI_BUS_ADDRESS="$ADDR"
    echo "Exported AT_SPI_BUS_ADDRESS=$AT_SPI_BUS_ADDRESS"
  fi
fi

export LIBGL_ALWAYS_SOFTWARE=1
export GALLIUM_DRIVER=llvmpipe
export GTK_MODULES="gail:atk-bridge"
export QT_ACCESSIBILITY=1
export ACCESSIBILITY_ENABLED=1
export GCU_FIXTURE_STATE="${GCU_FIXTURE_STATE:-/tmp/fixture_state.json}"

# Start fixture if not already running
if ! pgrep -f "target/debug/gcu-fixture" >/dev/null 2>&1; then
  echo "Launching gcu-fixture in background..."
  rm -f "$GCU_FIXTURE_STATE" /tmp/fixture.log
  ./target/debug/gcu-fixture > /tmp/fixture.log 2>&1 &
  FIXTURE_PID=$!
else
  FIXTURE_PID=""
fi

cleanup() {
  echo "Cleaning up smoke background processes..."
  if [ -n "$FIXTURE_PID" ]; then
    kill "$FIXTURE_PID" 2>/dev/null || true
  fi
  if [ -n "$ATSPI_PID" ]; then
    kill "$ATSPI_PID" 2>/dev/null || true
  fi
  if [ -n "$OPENBOX_PID" ]; then
    kill "$OPENBOX_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

echo "Waiting for gcu-fixture to initialize..."
for i in $(seq 1 50); do
  if [ -f "$GCU_FIXTURE_STATE" ]; then
    echo "gcu-fixture state ready at $GCU_FIXTURE_STATE"
    break
  fi
  sleep 0.2
done

echo "Executing Linux driver contract test suite..."
cargo test --locked -p gcu-driver-linux --test contract -- --ignored --test-threads=1 --nocapture

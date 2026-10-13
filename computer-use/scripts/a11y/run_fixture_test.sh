#!/usr/bin/env bash
# Runs gcu-fixture inside an AT-SPI and Xvfb session and verifies its accessibility tree.
set -euo pipefail

openbox &
OPENBOX_PID=$!

ATSPI_LAUNCHER=$(which at-spi-bus-launcher 2>/dev/null || find /usr/lib* /usr/libexec -name at-spi-bus-launcher 2>/dev/null | head -n 1)
if [ -z "$ATSPI_LAUNCHER" ]; then
  echo "ERROR: at-spi-bus-launcher not found" >&2
  exit 1
fi
echo "Using at-spi-bus-launcher: $ATSPI_LAUNCHER"
"$ATSPI_LAUNCHER" --launch-immediately --a11y=1 --screen-reader=1 &
ATSPI_PID=$!

echo "Waiting for org.a11y.Bus..."
for i in $(seq 1 40); do
  if busctl --user status org.a11y.Bus >/dev/null 2>&1; then
    echo "AT-SPI bus ready"
    break
  fi
  sleep 0.2
done

# Ensure GNOME accessibility and screen reader properties are enabled
gsettings set org.gnome.desktop.a11y.applications screen-reader-enabled true 2>/dev/null || true
gsettings set org.gnome.desktop.interface toolkit-accessibility true 2>/dev/null || true
busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true 2>/dev/null || true
busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status ScreenReaderEnabled b true 2>/dev/null || true

# Extract AT-SPI address and export for direct zbus/accesskit connection
BUS_RAW=$(busctl --user call org.a11y.Bus /org/a11y/bus org.a11y.Bus GetAddress 2>/dev/null || true)
ADDR=$(echo "$BUS_RAW" | awk '{print $2}' | tr -d '"')
if [ -n "$ADDR" ]; then
  export AT_SPI_BUS_ADDRESS="$ADDR"
  echo "Exported AT_SPI_BUS_ADDRESS=$AT_SPI_BUS_ADDRESS"
fi

export LIBGL_ALWAYS_SOFTWARE=1
export GALLIUM_DRIVER=llvmpipe
export GTK_MODULES="gail:atk-bridge"
export QT_ACCESSIBILITY=1
export ACCESSIBILITY_ENABLED=1
export GCU_FIXTURE_STATE=/tmp/fixture_state.json

echo "Launching gcu-fixture in background..."
./target/debug/gcu-fixture > /tmp/fixture.log 2>&1 &
FIXTURE_PID=$!

cleanup() {
  echo "Cleaning up background processes..."
  kill $FIXTURE_PID 2>/dev/null || true
  kill $ATSPI_PID 2>/dev/null || true
  kill $OPENBOX_PID 2>/dev/null || true
}
trap cleanup EXIT

echo "Dumping AT-SPI tree via python3 scripts/a11y/dump_atspi.py..."
if ! /usr/bin/python3 scripts/a11y/dump_atspi.py --output /tmp/fixture_tree.json --timeout 30; then
  echo "=== FIXTURE LOG ON DUMP FAILURE ==="
  cat /tmp/fixture.log 2>/dev/null || true
  exit 1
fi

echo "Checking fixture tree conformance via python3 scripts/a11y/check_fixture_tree.py..."
if ! /usr/bin/python3 scripts/a11y/check_fixture_tree.py --input /tmp/fixture_tree.json; then
  echo "=== FIXTURE LOG ==="
  cat /tmp/fixture.log 2>/dev/null || true
  exit 1
fi

echo "Running live driver AT-SPI test..."
if ! cargo test --locked -p gcu-driver-linux --test atspi_tree -- --ignored; then
  echo "=== FIXTURE LOG ON LIVE DRIVER TEST FAILURE ==="
  cat /tmp/fixture.log 2>/dev/null || true
  exit 1
fi

echo "Running live driver X11 capture test..."
if ! cargo test --locked -p gcu-driver-linux --test capture -- --ignored; then
  echo "=== FIXTURE LOG ON LIVE DRIVER CAPTURE TEST FAILURE ==="
  cat /tmp/fixture.log 2>/dev/null || true
  exit 1
fi

echo "Running live driver X11 actions test..."
if ! cargo test --locked -p gcu-driver-linux --test actions -- --ignored --test-threads=1; then
  echo "=== FIXTURE LOG ON LIVE DRIVER ACTIONS TEST FAILURE ==="
  cat /tmp/fixture.log 2>/dev/null || true
  exit 1
fi



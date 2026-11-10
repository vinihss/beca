#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="$PREFIX/bin"
CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
SOCK="$RUNTIME_DIR/beca/beca.sock"

MODE="${1:-install}" # install | start-daemon | stop-daemon | status

need_cmd() { command -v "$1" >/dev/null 2>&1 || { echo "missing: $1" >&2; exit 1; }; }

build_bins() {
  need_cmd cargo
  echo "[beca-install] building release binaries..."
  (cd "$ROOT" && cargo build --release -p beca -p beca-ui --bin beca --bin beca-voice)
}

install_bins() {
  mkdir -p "$BIN_DIR"
  install -m 0755 "$ROOT/target/release/beca" "$BIN_DIR/beca"
  install -m 0755 "$ROOT/target/release/beca-voice" "$BIN_DIR/beca-voice"
  echo "[beca-install] installed: $BIN_DIR/beca, $BIN_DIR/beca-voice"
}

install_assets() {
  # Seed XDG dirs and copy reference/default assets (UI/slint seeded at runtime by beca-ui loader,
  # but we also install reference files for convenience)
  mkdir -p "$CONFIG_HOME/beca/scripts" "$CONFIG_HOME/beca/themes" "$CONFIG_HOME/beca/ui"
  # reference scripts
  if [ -f "$ROOT/scripts/launcher.lua" ]; then
    cp -n "$ROOT/scripts/launcher.lua" "$CONFIG_HOME/beca/scripts/launcher.lua" 2>/dev/null || true
  fi
  if [ -f "$ROOT/crates/beca-ui/scripts/voice_behavior.luau" ]; then
    cp -n "$ROOT/crates/beca-ui/scripts/voice_behavior.luau" "$CONFIG_HOME/beca/scripts/voice_behavior.luau" 2>/dev/null || true
  fi
  # schema examples as reference (do not overwrite user config)
  mkdir -p "$CONFIG_HOME/beca/schema"
  cp -f "$ROOT/docs/schema/"*.toml "$CONFIG_HOME/beca/schema/" 2>/dev/null || true
  echo "[beca-install] assets prepared in $CONFIG_HOME/beca"
}

start_daemon() {
  if [ -x "$BIN_DIR/beca" ]; then
    BECA="$BIN_DIR/beca"
  elif [ -x "$ROOT/target/release/beca" ]; then
    BECA="$ROOT/target/release/beca"
  else
    BECA="beca"
  fi
  mkdir -p "$RUNTIME_DIR/beca"
  if [ -S "$SOCK" ] && "$BECA" ping >/dev/null 2>&1; then
    echo "[beca-install] daemon already running"
    return 0
  fi
  rm -f "$SOCK" 2>/dev/null || true
  nohup "$BECA" daemon >"$RUNTIME_DIR/beca/daemon.log" 2>&1 &
  echo $! > "$RUNTIME_DIR/beca/daemon.pid"
  # wait for socket
  for i in $(seq 1 50); do
    if [ -S "$SOCK" ]; then break; fi
    sleep 0.1
  done
  if [ -S "$SOCK" ]; then
    echo "[beca-install] daemon started (pid $(cat "$RUNTIME_DIR/beca/daemon.pid" 2>/dev/null || echo '?'))"
  else
    echo "[beca-install] daemon did not create socket yet; check $RUNTIME_DIR/beca/daemon.log" >&2
    return 1
  fi
}

stop_daemon() {
  if [ -f "$RUNTIME_DIR/beca/daemon.pid" ]; then
    kill "$(cat "$RUNTIME_DIR/beca/daemon.pid")" 2>/dev/null || true
    rm -f "$RUNTIME_DIR/beca/daemon.pid"
  fi
  rm -f "$SOCK" 2>/dev/null || true
  echo "[beca-install] daemon stopped"
}

status() {
  if [ -x "$BIN_DIR/beca" ]; then BECA="$BIN_DIR/beca"; else BECA="beca"; fi
  echo "[beca-install] bin: $(command -v beca 2>/dev/null || echo "not in PATH (expected $BIN_DIR/beca)")"
  if "$BECA" ping 2>/dev/null; then
    echo "[beca-install] daemon: reachable ($SOCK)"
  else
    echo "[beca-install] daemon: not reachable"
  fi
}

case "$MODE" in
  install)
    build_bins
    install_bins
    install_assets
    start_daemon
    status
    echo "[beca-install] done. Ensure $BIN_DIR is in PATH."
    ;;
  start-daemon) start_daemon; status ;;
  stop-daemon)  stop_daemon ;;
  status)       status ;;
  build)        build_bins ;;
  *) echo "usage: $0 [install|build|start-daemon|stop-daemon|status]" >&2; exit 1 ;;
esac

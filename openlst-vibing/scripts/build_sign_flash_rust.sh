#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TOOLS_DIR="$ROOT_DIR/../openlst-firmware/open-lst/tools"

DEVICE="/dev/ttyUSB0"
RADIO_ID="2dec"
SIGNING_KEY=""
OUT_BASENAME="openlst-radio"
TARGET="cc1110-none-elf"
PROFILE="release"
FEATURES="cc1110-lowlevel,cc1110-real-mmio"

RX_SOCKET="ipc:///tmp/radiomux1_rx"
TX_SOCKET="ipc:///tmp/radiomux1_tx"
ECHO_SOCKET="ipc:///tmp/radiomux1_echo"
MUX_MODE="777"

KEEP_MUX=0
OPEN_TERMINAL=0

usage() {
  cat <<EOF
Usage: $(basename "$0") --signing-key <hex> [options]

Build Rust firmware, run radio_mux in the background, sign, flash, and optionally open radio_terminal.

Required:
  --signing-key <hex>          16-byte AES signing key in hex (32 chars)

Options:
  --device <path>              Serial device (default: $DEVICE)
  --radio-id <id>              bootload_radio radio id (default: $RADIO_ID)
  --target <triple>            Cargo target (default: $TARGET)
  --profile <debug|release>    Build profile (default: $PROFILE)
  --features <list>            Cargo features (default: $FEATURES)
  --out-name <name>            Output basename for .hex/.sig (default: $OUT_BASENAME)
  --keep-mux                   Keep radio_mux running after script exits
  --terminal                   Launch radio_terminal after flash (blocks until exit)
  --help                       Show this help

Examples:
  $(basename "$0") --signing-key 1546b4ec69f6266fb034b1958b830843
  $(basename "$0") --signing-key <key> --device /dev/ttyUSB0 --terminal
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --signing-key) SIGNING_KEY="$2"; shift 2 ;;
    --device) DEVICE="$2"; shift 2 ;;
    --radio-id) RADIO_ID="$2"; shift 2 ;;
    --target) TARGET="$2"; shift 2 ;;
    --profile) PROFILE="$2"; shift 2 ;;
    --features) FEATURES="$2"; shift 2 ;;
    --out-name) OUT_BASENAME="$2"; shift 2 ;;
    --keep-mux) KEEP_MUX=1; shift ;;
    --terminal) OPEN_TERMINAL=1; shift ;;
    --help|-h) usage; exit 0 ;;
    *) echo "Unknown argument: $1" >&2; usage; exit 1 ;;
  esac
done

if [[ -z "$SIGNING_KEY" ]]; then
  echo "--signing-key is required" >&2
  usage
  exit 1
fi

if ! [[ "$SIGNING_KEY" =~ ^[0-9a-fA-F]{32}$ ]]; then
  echo "--signing-key must be 32 hex characters" >&2
  exit 1
fi

if [[ "$PROFILE" != "debug" && "$PROFILE" != "release" ]]; then
  echo "--profile must be 'debug' or 'release'" >&2
  exit 1
fi

find_objcopy() {
  if command -v llvm-objcopy >/dev/null 2>&1; then
    echo "llvm-objcopy"
  elif command -v rust-objcopy >/dev/null 2>&1; then
    echo "rust-objcopy"
  elif command -v objcopy >/dev/null 2>&1; then
    echo "objcopy"
  else
    return 1
  fi
}

tool_cmd() {
  local name="$1"
  if command -v "$name" >/dev/null 2>&1; then
    echo "$name"
  else
    echo "python3 -m openlst_tools.$2"
  fi
}

OBJCOPY="$(find_objcopy || true)"
if [[ -z "$OBJCOPY" ]]; then
  echo "Could not find objcopy tool (llvm-objcopy/rust-objcopy/objcopy)" >&2
  exit 1
fi

if [[ ! -d "$TOOLS_DIR/openlst_tools" ]]; then
  echo "OpenLST tools directory not found at: $TOOLS_DIR" >&2
  exit 1
fi

RADIO_MUX_CMD="$(tool_cmd radio_mux radio_mux)"
SIGN_RADIO_CMD="$(tool_cmd sign_radio sign_radio)"
BOOTLOAD_RADIO_CMD="$(tool_cmd bootload_radio bootload_radio)"
RADIO_TERMINAL_CMD="$(tool_cmd radio_terminal terminal)"

export PYTHONPATH="$TOOLS_DIR${PYTHONPATH:+:$PYTHONPATH}"

PROFILE_DIR="$PROFILE"
ELF_PATH="$ROOT_DIR/target/$TARGET/$PROFILE_DIR/openlst-radio"
HEX_PATH="$ROOT_DIR/$OUT_BASENAME.hex"
SIG_PATH="$ROOT_DIR/$OUT_BASENAME.sig"

MUX_PID=""
cleanup() {
  if [[ -n "$MUX_PID" && "$KEEP_MUX" -eq 0 ]]; then
    kill "$MUX_PID" >/dev/null 2>&1 || true
    wait "$MUX_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

echo "[1/5] Building Rust firmware..."
pushd "$ROOT_DIR" >/dev/null
if [[ "$PROFILE" == "release" ]]; then
  cargo build -p openlst-radio --target "$TARGET" --features "$FEATURES" --release
else
  cargo build -p openlst-radio --target "$TARGET" --features "$FEATURES"
fi

echo "[2/5] Converting ELF to HEX..."
"$OBJCOPY" -O ihex "$ELF_PATH" "$HEX_PATH"

echo "[3/5] Starting radio_mux in background..."
eval "$RADIO_MUX_CMD --rx-socket $RX_SOCKET --tx-socket $TX_SOCKET --echo-socket $ECHO_SOCKET --mode $MUX_MODE $DEVICE" &
MUX_PID=$!
sleep 1

echo "[4/5] Signing firmware image..."
eval "$SIGN_RADIO_CMD --signing-key $SIGNING_KEY $HEX_PATH $SIG_PATH"

echo "[5/5] Flashing firmware..."
eval "$BOOTLOAD_RADIO_CMD --signature-file $SIG_PATH -i $RADIO_ID $HEX_PATH"

if [[ "$OPEN_TERMINAL" -eq 1 ]]; then
  echo "Opening radio_terminal (radio_mux stays running)..."
  eval "$RADIO_TERMINAL_CMD"
fi

if [[ "$KEEP_MUX" -eq 1 ]]; then
  echo "radio_mux is still running (PID: $MUX_PID)."
  echo "Stop it later with: kill $MUX_PID"
  MUX_PID=""
fi

popd >/dev/null
echo "Done. HEX: $HEX_PATH  SIG: $SIG_PATH"

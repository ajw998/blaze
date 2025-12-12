#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

ROOT="${1:-$HOME}"
WARMUP_RUNS="${WARMUP_RUNS:-3}"
MEASURE_RUNS="${MEASURE_RUNS:-10}"

BLAZE_BIN="${BLAZE_BIN:-$REPO_ROOT/target/release/blaze}"
BLAZE_CLI_CMD="${BLAZE_CLI_CMD:-$BLAZE_BIN query %q}"
BLAZE_DAEMON_CMD="${BLAZE_DAEMON_CMD:-$BLAZE_BIN query --daemon %q}"

# Sanity checks (required tools)
for cmd in hyperfine fdfind find plocate; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "error: '$cmd' not found on PATH" >&2
    exit 1
  fi
done

if [ ! -x "$BLAZE_BIN" ]; then
  echo "error: blaze binary not found or not executable at: $BLAZE_BIN" >&2
  echo "hint: run 'cargo build --release' in the repo root, or set BLAZE_BIN explicitly." >&2
  exit 1
fi

if [ ! -d "$ROOT" ]; then
  echo "error: ROOT directory '$ROOT' does not exist" >&2
  exit 1
fi

echo "Benchmarking on ROOT: $ROOT"
echo "Using blaze binary:        $BLAZE_BIN"
echo "Blaze CLI template:        $BLAZE_CLI_CMD"
echo "Blaze daemon template:     $BLAZE_DAEMON_CMD"
echo "Warmup runs:               $WARMUP_RUNS, measured runs: $MEASURE_RUNS"
echo

QUERIES=(
  "rs"
  "Cargo.toml" # rare-ish filename
  "config"     # common dev term
  "src"        # path substring that hits many paths
)

expand_cmd() {
  local template="$1"
  local query="$2"
  printf '%s' "${template//%q/$query}"
}

# Run benchmarks
for q in "${QUERIES[@]}"; do
  echo "============================================================"
  echo "Query: '$q'"
  echo "============================================================"

  CMD_BLAZE_CLI="$(expand_cmd "$BLAZE_CLI_CMD" "$q")"
  CMD_BLAZE_DAEMON="$(expand_cmd "$BLAZE_DAEMON_CMD" "$q")"

  # Competitors
  CMD_FDFIND="fdfind \"$q\" \"$ROOT\""
  CMD_FIND="find \"$ROOT\" -iname \"*$q*\""
  CMD_PLOCATE="plocate \"$q\""

  echo "blaze (CLI):    $CMD_BLAZE_CLI"
  echo "blaze (daemon): $CMD_BLAZE_DAEMON"
  echo "fdfind:         $CMD_FDFIND"
  echo "find:           $CMD_FIND"
  echo "plocate:        $CMD_PLOCATE"
  echo

  # Build the hyperfine command list dynamically
  CMDS=(
    "$CMD_BLAZE_CLI"
    "$CMD_BLAZE_DAEMON"
    "$CMD_FDFIND"
  )

  CMDS+=(
    "$CMD_FIND"
    "$CMD_PLOCATE"
  )

  hyperfine \
    --warmup "$WARMUP_RUNS" \
    --runs "$MEASURE_RUNS" \
    "${CMDS[@]}"

  echo
done

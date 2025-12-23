#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

ROOT="${1:-$HOME}"
WARMUP_RUNS="${WARMUP_RUNS:-3}"
MEASURE_RUNS="${MEASURE_RUNS:-10}"
TODAY_EPOCH="$(date -d 'today 00:00' +%s)"
RESULTS_DIR="${RESULTS_DIR:-$REPO_ROOT/benchmark_results}"

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

mkdir -p "$RESULTS_DIR"

echo "Benchmarking on ROOT: $ROOT"
echo "Using blaze binary:        $BLAZE_BIN"
echo "Blaze CLI template:        $BLAZE_CLI_CMD"
echo "Blaze daemon template:     $BLAZE_DAEMON_CMD"
echo "Warmup runs:               $WARMUP_RUNS, measured runs: $MEASURE_RUNS"
echo

expand_cmd() {
  local template="$1"
  local query="$2"
  printf '%s' "${template//%q/$query}"
}

queries=(ext_rs cargo_toml config src modified_today)

# Run benchmarks per class
for name in "${queries[@]}"; do
  case "$name" in
    ext_rs)
      blaze_query="ext:rs"
      CMD_FDFIND="fdfind --extension rs --type f . \"$ROOT\""
      CMD_FIND="find \"$ROOT\" -type f -name '*.rs'"
      CMD_PLOCATE="plocate --basename --regex '\\.rs$'"
      ;;
    cargo_toml)
      blaze_query="Cargo.toml"
      CMD_FDFIND="fdfind Cargo.toml \"$ROOT\""
      CMD_FIND="find \"$ROOT\" -type f -name 'Cargo.toml'"
      CMD_PLOCATE="plocate --basename Cargo.toml"
      ;;
    config)
      blaze_query="config"
      CMD_FDFIND="fdfind config \"$ROOT\""
      CMD_FIND="find \"$ROOT\" -type f -iname '*config*'"
      CMD_PLOCATE="plocate config"
      ;;
    src)
      blaze_query="src"
      CMD_FDFIND="fdfind src \"$ROOT\""
      CMD_FIND="find \"$ROOT\" -path '*src*' -type f"
      CMD_PLOCATE="plocate src"
      ;;
    modified_today)
      blaze_query="modified:today"
      CMD_FDFIND="fdfind --type f --changed-within 1day . \"$ROOT\""
      CMD_FIND="find \"$ROOT\" -type f -daystart -mtime 0"
      CMD_PLOCATE="plocate \"$ROOT\" | xargs -r -d '\n' stat -c '%Y %n' | awk -v today=$TODAY_EPOCH '\$1 >= today {print \$2}'"
      ;;
  esac

  echo "============================================================"
  echo "Query class: '$name' => blaze:'$blaze_query'"
  echo "============================================================"

  CMD_BLAZE_CLI="$(expand_cmd "$BLAZE_CLI_CMD" "$blaze_query")"
  CMD_BLAZE_DAEMON="$(expand_cmd "$BLAZE_DAEMON_CMD" "$blaze_query")"

  CMDS=(
    "$CMD_BLAZE_CLI"
    "$CMD_BLAZE_DAEMON"
    "$CMD_FDFIND"
    "$CMD_FIND"
    "$CMD_PLOCATE"
  )

  echo "blaze (CLI):    $CMD_BLAZE_CLI"
  echo "blaze (daemon): $CMD_BLAZE_DAEMON"
  echo "fdfind:         $CMD_FDFIND"
  echo "find:           $CMD_FIND"
  echo "plocate:        $CMD_PLOCATE"
  echo

  RESULT_JSON="$RESULTS_DIR/${name}.json"

  hyperfine \
    --warmup "$WARMUP_RUNS" \
    --runs "$MEASURE_RUNS" \
    --export-json "$RESULT_JSON" \
    "${CMDS[@]}"

  echo "Saved results to: $RESULT_JSON"
  echo
done

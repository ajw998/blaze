#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

ROOT="${1:-$HOME}"
WARMUP_RUNS="${WARMUP_RUNS:-3}"
MEASURE_RUNS="${MEASURE_RUNS:-10}"
EXPORT_MD="${EXPORT_MD:-}"
BLAZE_BIN="${BLAZE_BIN:-$REPO_ROOT/target/release/blaze}"

# Sanity checks
for cmd in hyperfine fdfind find; do
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
echo "Using blaze binary:   $BLAZE_BIN"
echo "Warmup runs:          $WARMUP_RUNS, measured runs: $MEASURE_RUNS"
echo

# Queries to benchmark
QUERIES=(
  "Cargo.toml" # rare-ish filename
  "config" # common dev term
  "src" # path substring that hits many paths
)

# Run benchmarks
for q in "${QUERIES[@]}"; do
  echo "============================================================"
  echo "Query: '$q'"
  echo "============================================================"

  CMD_BLAZE="$BLAZE_BIN query \"$q\""
  CMD_FD="fdfind $q $ROOT"
  CMD_FIND="find $ROOT -iname \"*$q*\""

  echo "blaze: $CMD_BLAZE"
  echo "fd:    $CMD_FD"
  echo "find:  $CMD_FIND"
  echo

  if [ -n "$EXPORT_MD" ]; then
    TMP_MD="$(mktemp)"
    hyperfine \
      --warmup "$WARMUP_RUNS" \
      --runs "$MEASURE_RUNS" \
      "$CMD_BLAZE" \
      "$CMD_FD" \
      "$CMD_FIND" \
      --export-markdown "$TMP_MD"

    {
      echo
      echo "### Query: \`$q\`"
      echo
      cat "$TMP_MD"
      echo
    } >> "$EXPORT_MD"
    rm -f "$TMP_MD"
  else
    hyperfine \
      --warmup "$WARMUP_RUNS" \
      --runs "$MEASURE_RUNS" \
      "$CMD_BLAZE" \
      "$CMD_FD" \
      "$CMD_FIND"
  fi

  echo
done

if [ -n "$EXPORT_MD" ]; then
  echo "Markdown results written to: $EXPORT_MD"
fi


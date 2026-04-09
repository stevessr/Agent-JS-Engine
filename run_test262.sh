#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: ./run_test262.sh [options]

Fetch test262 with sparse checkout if needed, then run the full core-profile suite
with summary-only output for agent-friendly runs.

Options:
  -h, --help               Show this help message.
  -j, --jobs N            Run up to N chunks in parallel.
      --chunk-size N      Set the number of cases per chunk.
      --filter TEXT       Run only cases whose path contains TEXT.
      --max-cases N       Limit the number of discovered cases.
      --offset N          Skip the first N discovered cases.
      --test262-dir PATH  Override the local test262 checkout directory.

Environment variables:
  TEST262_DIR              Override the local test262 checkout directory.
  TEST262_FILTER           Run only cases whose path contains this substring.
  TEST262_MAX_CASES        Limit the number of discovered cases.
  TEST262_OFFSET           Skip the first N discovered cases.
  TEST262_CHUNK_SIZE       Number of cases per chunk in full runs.
  TEST262_PARALLEL_CHUNKS  Number of chunks to run in parallel.

Examples:
  ./run_test262.sh
  ./run_test262.sh -j 4
  ./run_test262.sh --filter Temporal --max-cases 200
  ./run_test262.sh -j 2 --chunk-size 250 --filter import-defer
EOF
}

require_value() {
  local flag="$1"
  local value="${2:-}"
  if [ -z "$value" ]; then
    printf 'Missing value for %s\n\n' "$flag" >&2
    usage >&2
    exit 1
  fi
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    -h|--help)
      usage
      exit 0
      ;;
    -j|--jobs)
      require_value "$1" "${2:-}"
      export TEST262_PARALLEL_CHUNKS="$2"
      shift 2
      ;;
    --chunk-size)
      require_value "$1" "${2:-}"
      export TEST262_CHUNK_SIZE="$2"
      shift 2
      ;;
    --filter)
      require_value "$1" "${2:-}"
      export TEST262_FILTER="$2"
      shift 2
      ;;
    --max-cases)
      require_value "$1" "${2:-}"
      export TEST262_MAX_CASES="$2"
      shift 2
      ;;
    --offset)
      require_value "$1" "${2:-}"
      export TEST262_OFFSET="$2"
      shift 2
      ;;
    --test262-dir)
      require_value "$1" "${2:-}"
      export TEST262_DIR="$2"
      shift 2
      ;;
    *)
      printf 'Unknown argument: %s\n\n' "$1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SUITE_DIR="${TEST262_DIR:-$ROOT_DIR/test262}"

if [ ! -d "$SUITE_DIR/.git" ]; then
  git clone --depth 1 --filter=blob:none --sparse https://github.com/tc39/test262.git "$SUITE_DIR"
fi

git -C "$SUITE_DIR" sparse-checkout set test harness

# Run with lower priority (higher nice value) to avoid interfering with system tasks
exec nice -n 10 env \
  TEST262_DIR="$SUITE_DIR" \
  TEST262_FULL=1 \
  TEST262_QUIET=1 \
  cargo test --release --test test262_runner test262_core_profile -- --nocapture

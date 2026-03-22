#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SUITE_DIR="${TEST262_DIR:-$ROOT_DIR/test262}"

if [ ! -d "$SUITE_DIR/.git" ]; then
  git clone --depth 1 --filter=blob:none --sparse https://github.com/tc39/test262.git "$SUITE_DIR"
fi

git -C "$SUITE_DIR" sparse-checkout set test harness

TEST262_DIR="$SUITE_DIR" cargo test --test test262_runner test262_core_profile -- --ignored --nocapture

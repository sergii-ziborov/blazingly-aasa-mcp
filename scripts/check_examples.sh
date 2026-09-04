#!/usr/bin/env bash
# The CLI prints what the README says it prints.
#
#   ./scripts/check_examples.sh          verify
#   BLESS=1 ./scripts/check_examples.sh  re-record after an intentional change
#
# Offline commands only: nothing here touches the network, so CI can run it anywhere.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release --quiet
BIN=./target/release/blazingly-aasa
status=0

run() { # name, then argv
  local name="$1"; shift
  local expected="examples/expected/$name.txt"
  local actual; actual="$("$BIN" "$@" 2>&1 || true)"
  local code; "$BIN" "$@" > /dev/null 2>&1 && code=0 || code=$?
  actual="$actual
exit=$code"
  if [ "${BLESS:-}" = "1" ]; then
    printf '%s\n' "$actual" > "$expected"; echo "recorded $expected"; return
  fi
  if ! diff -u "$expected" <(printf '%s\n' "$actual") > /dev/null; then
    echo "CHANGED  $name" >&2
    diff -u "$expected" <(printf '%s\n' "$actual") | sed 's/^/  /' >&2
    status=1
  else
    echo "ok       $name"
  fi
}

run validate validate examples/broken.json
run explain explain examples/demo.json example.com \
  "https://example.com/help/1?articleNumber=481" --app ABCDE12345.com.example.app

exit $status

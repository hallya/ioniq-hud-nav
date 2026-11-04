#!/usr/bin/env bash
# End-to-end test on Linux: the Rust core, through its UniFFI bridge (Python
# bindings), runs the whole Bluelink chain against the mock server.
# Same command in CI and locally, from the repo root:
#   scripts/e2e-linux.sh
set -euo pipefail
cd "$(dirname "$0")/.."

OUT=build/e2e
PORT="${HUDNAV_MOCK_PORT:-8765}"
rm -rf "$OUT/py" && mkdir -p "$OUT/py"

cargo build --quiet --manifest-path ffi/Cargo.toml
LIB=ffi/target/debug/libhudnav_ffi.so
test -f "$LIB" || { echo "e2e: $LIB not built" >&2; exit 1; }
cargo run --quiet --manifest-path ffi/Cargo.toml --bin uniffi-bindgen -- generate \
  --library "$LIB" --language python --out-dir "$OUT/py" 2>/dev/null
cp "$LIB" "$OUT/py/"
test -f "$OUT/py/hudnav_ffi.py" || { echo "e2e: Python bindings not generated" >&2; exit 1; }

python3 -I scripts/e2e/mock_bluelink.py --port "$PORT" --constants-out "$OUT/constants.json" \
  2>"$OUT/mock.log" &
MOCK=$!
trap 'kill "$MOCK" 2>/dev/null || true' EXIT
for _ in $(seq 1 100); do
  curl -fsS --noproxy 127.0.0.1 "http://127.0.0.1:$PORT/__checks" >/dev/null 2>&1 && break
  kill -0 "$MOCK" 2>/dev/null || { echo "e2e: mock server exited" >&2; cat "$OUT/mock.log" >&2; exit 1; }
  sleep 0.1
done

python3 -I scripts/e2e/linux_shell.py --bindings "$OUT/py" \
  --constants "$OUT/constants.json" --mock "http://127.0.0.1:$PORT"

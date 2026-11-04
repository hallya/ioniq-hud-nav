#!/usr/bin/env bash
# End-to-end test on the iOS Simulator, on macOS with Xcode (CI and the Mac).
# Builds the core for the Simulator, bundles the mock server's constants,
# checks the core is linked statically, then runs the UI test: account entry,
# "Check connection only", "Send test destination", all against
# scripts/e2e/mock_bluelink.py. From the repo root:
#   scripts/ios-sim-test.sh
# Full output goes to build/ios-sim-test.log. The bundled constants point at
# the mock: scripts/ios-build.sh writes the real ones again before an iPhone build.
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$PWD"
LOG="$ROOT/build/ios-sim-test.log"
OUT="$ROOT/build/e2e"
GEN="$ROOT/ios/Generated"
PORT=8765
mkdir -p "$OUT" "$GEN"
: > "$LOG"

stage() { printf '%-28s' "$1"; }
# In GitHub Actions, failure lines also become annotations, which the public
# API returns without a login: that is how Claude reads a failed run.
report() {
  if [[ -n "${GITHUB_ACTIONS:-}" ]]; then
    while IFS= read -r l; do echo "$l"; echo "::error::${l//%/%25}"; done
  else cat; fi
}
# Same as in ios-build.sh: on failure, the failing command's distinct error lines.
run() {
  local from
  from=$(wc -l < "$LOG")
  if "$@" >>"$LOG" 2>&1; then echo "ok"; else
    echo "FAILED (see build/ios-sim-test.log)"
    local out
    out=$(tail -n +"$((from + 1))" "$LOG")
    if grep -q "error:" <<<"$out"; then
      grep "error:" <<<"$out" | awk '!seen[$0]++' | head -n 10 | report
    else
      tail -n 10 <<<"$out" | report
    fi
    exit 1
  fi
}

stage "Rust target for Simulator"
run rustup target add aarch64-apple-ios-sim

stage "Rust core for Simulator"
run cargo build --manifest-path ffi/Cargo.toml --target aarch64-apple-ios-sim
test -f ffi/target/aarch64-apple-ios-sim/debug/libhudnav_ffi.a || { echo "no Simulator core archive"; exit 1; }

stage "Swift bindings"
run cargo build --manifest-path ffi/Cargo.toml
run cargo run --quiet --manifest-path ffi/Cargo.toml --bin uniffi-bindgen -- generate \
  --library ffi/target/debug/libhudnav_ffi.dylib --language swift --out-dir "$GEN"
mv -f "$GEN/hudnav_ffiFFI.modulemap" "$GEN/module.modulemap"

stage "Mock Bluelink server"
python3 -I scripts/e2e/mock_bluelink.py --port "$PORT" \
  --constants-out "$GEN/bluelink_constants.json" >>"$LOG" 2>"$OUT/mock-sim.log" &
MOCK=$!
trap 'kill "$MOCK" 2>/dev/null || true' EXIT
up=0
for _ in $(seq 1 100); do
  if curl -fsS --noproxy 127.0.0.1 "http://127.0.0.1:$PORT/__checks" >/dev/null 2>&1; then up=1; break; fi
  kill -0 "$MOCK" 2>/dev/null || break
  sleep 0.2
done
[[ $up == 1 ]] || { { echo "FAILED: mock server did not start"; tail -n 10 "$OUT/mock-sim.log"; } | report; exit 1; }
echo "ok"

stage "Xcode project"
command -v xcodegen >/dev/null || run brew install xcodegen
run xcodegen generate --spec ios/project.yml --project ios

stage "Simulator"
SIM=$(xcrun simctl list devices available -j | python3 -I -c '
import json, sys
best = None
for runtime, devices in json.load(sys.stdin)["devices"].items():
    if ".iOS-" not in runtime:
        continue
    version = tuple(int(x) for x in runtime.rsplit(".iOS-", 1)[1].split("-") if x.isdigit())
    for d in devices:
        if d.get("isAvailable") and d["name"].startswith("iPhone"):
            if best is None or version > best[0]:
                best = (version, d["udid"], d["name"])
print(best[1] if best else "")')
[[ -n "$SIM" ]] || { echo "FAILED: no available iPhone Simulator" | report; exit 1; }
echo "$SIM"

XCODE=(xcodebuild -project ios/HudNav.xcodeproj -scheme HudNav -configuration Debug
  -destination "id=$SIM" -derivedDataPath build/xcode-sim)

stage "Build for testing"
run "${XCODE[@]}" build-for-testing

stage "Core linked statically"
APP=build/xcode-sim/Build/Products/Debug-iphonesimulator/HudNav.app
[[ -s "$APP/bluelink_constants.json" ]] || { echo "FAILED: bluelink_constants.json missing from $APP" | report; exit 1; }
if otool -L "$APP/HudNav" | grep -q "libhudnav_ffi"; then
  echo "FAILED: HudNav links libhudnav_ffi.dylib; it would crash at launch" | report
  exit 1
fi
echo "ok"

stage "UI test against the mock"
rm -rf build/ios-sim-test.xcresult
run "${XCODE[@]}" test-without-building -resultBundlePath build/ios-sim-test.xcresult

stage "Mock saw the whole chain"
curl -fsS --noproxy 127.0.0.1 "http://127.0.0.1:$PORT/__checks" > "$OUT/checks-sim.json"
python3 -I - "$OUT/checks-sim.json" <<'EOF' | report
import json, sys
checks = json.load(open(sys.argv[1]))
chain = [
    "POST /api/v1/spa/notifications/register",
    "GET /auth/api/v2/user/oauth2/authorize",
    "GET /auth/api/v1/accounts/certs",
    "POST /auth/account/signin",
    "POST /auth/v1/token",
    "POST /auth/v1/token-exchange",
    "GET /api/v1/spa/vehicles",
    "GET /api/v1/spa/vehicles/veh-0001/ccs2/carstatus/latest",
]
send = ["PUT /api/v1/user/pin", "POST /api/v2/spa/vehicles/veh-0001/location/routes"]
problems = list(checks["failures"])
if checks["calls"] != chain + chain + send:
    problems.append("calls: " + ", ".join(checks["calls"]))
if problems:
    print("FAILED")
    print("\n".join(problems))
    sys.exit(1)
print("ok")
EOF

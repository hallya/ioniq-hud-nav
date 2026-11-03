#!/usr/bin/env bash
# Build the app and install it on the paired iPhone. Run on the Mac, from the repo root:
#   scripts/ios-build.sh            build + install
#   scripts/ios-build.sh --no-install
# Full output goes to build/ios-build.log; the terminal shows one line per stage.
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$PWD"
LOG="$ROOT/build/ios-build.log"
mkdir -p "$ROOT/build"
: > "$LOG"

INSTALL=1
[[ "${1:-}" == "--no-install" ]] && INSTALL=0

stage() { printf '%-28s' "$1"; }
# Runs one command of a stage. On failure: the distinct error lines of that
# command only (else its last 20 lines), plus the fix for known causes.
run() {
  local from
  from=$(wc -l < "$LOG")
  if "$@" >>"$LOG" 2>&1; then echo "ok"; else
    echo "FAILED (see build/ios-build.log)"
    local out
    out=$(tail -n +"$((from + 1))" "$LOG")
    if grep -q "error:" <<<"$out"; then
      grep "error:" <<<"$out" | awk '!seen[$0]++' | head -n 20
    else
      tail -n 20 <<<"$out"
    fi
    if grep -q "Unable to log in with account" <<<"$out"; then
      echo "Fix: Xcode > Settings > Accounts: select your Apple ID, sign in again, then rerun."
    fi
    exit 1
  fi
}

stage "Rust target for iOS"
run rustup target add aarch64-apple-ios

stage "Rust core for iPhone"
run cargo build --manifest-path ffi/Cargo.toml --release --target aarch64-apple-ios

stage "Swift bindings"
run cargo build --manifest-path ffi/Cargo.toml
GEN="$ROOT/ios/Generated"
mkdir -p "$GEN"
run cargo run --quiet --manifest-path ffi/Cargo.toml --bin uniffi-bindgen -- generate \
  --library ffi/target/debug/libhudnav_ffi.dylib --language swift --out-dir "$GEN"
mv -f "$GEN/hudnav_ffiFFI.modulemap" "$GEN/module.modulemap"

# Bluelink app constants, read from upstream's source at each build by
# scripts/app-constants.py (text only, upstream code never runs). The JSON
# lands in ios/Generated (not committed) and in the app bundle.
# HUDNAV_UPSTREAM_REF=<40-hex commit or ref> pins upstream if master breaks.
UPSTREAM_REPO="Hyundai-Kia-Connect/hyundai_kia_connect_api"
UP="$ROOT/build/upstream"
bluelink_constants() {
  local ref="${HUDNAV_UPSTREAM_REF:-HEAD}" sha f
  if [[ "$ref" =~ ^[0-9a-f]{40}$ ]]; then sha="$ref"; else
    sha=$(git ls-remote "https://github.com/$UPSTREAM_REPO" "$ref" | head -1 | cut -f1)
  fi
  [[ "$sha" =~ ^[0-9a-f]{40}$ ]] || { echo "upstream: no commit for '$ref'"; return 1; }
  rm -rf "$UP" && mkdir -p "$UP"
  for f in const.py ApiImplType1.py KiaUvoApiEU.py cci_fingerprint.py; do
    curl -fsSL --retry 2 -o "$UP/$f" \
      "https://raw.githubusercontent.com/$UPSTREAM_REPO/$sha/hyundai_kia_connect_api/$f" || return 1
  done
  python3 -I scripts/app-constants.py "$UP" --upstream-ref "$UPSTREAM_REPO@$sha" \
    --out "$GEN/bluelink_constants.json"
}
stage "Bluelink constants"
run bluelink_constants

stage "Xcode project"
command -v xcodegen >/dev/null || run brew install xcodegen
run xcodegen generate --spec ios/project.yml --project ios

# Team ID = the OU field of the "Apple Development" certificate Xcode created.
if [[ -z "${TEAM_ID:-}" ]]; then
  TEAM_ID=$(security find-certificate -c "Apple Development" -p 2>/dev/null \
    | openssl x509 -noout -subject 2>/dev/null | sed -n 's/.*OU *= *\([A-Z0-9]\{10\}\).*/\1/p' | head -1)
fi
if [[ -z "$TEAM_ID" ]]; then
  echo "No Apple Development certificate found. In Xcode: Settings > Accounts > your Apple ID > Manage Certificates > + Apple Development."
  exit 1
fi

stage "App build (signed)"
run xcodebuild -project ios/HudNav.xcodeproj -scheme HudNav -configuration Release \
  -destination generic/platform=iOS -derivedDataPath build/xcode \
  -allowProvisioningUpdates DEVELOPMENT_TEAM="$TEAM_ID"
APP="build/xcode/Build/Products/Release-iphoneos/HudNav.app"
[[ -s "$APP/bluelink_constants.json" ]] || { echo "bluelink_constants.json is missing from $APP"; exit 1; }

[[ $INSTALL == 0 ]] && { echo "Built $APP"; exit 0; }

stage "Install on iPhone"
DEVICE=$(xcrun devicectl list devices 2>/dev/null | awk '/iPhone/ && /available|connected/ {for(i=1;i<=NF;i++) if ($i ~ /^[0-9A-F]{8}-/) {print $i; exit}}')
if [[ -z "$DEVICE" ]]; then
  echo "no iPhone found: unlock it and keep it on the same Wi-Fi as the Mac"; exit 1
fi
run xcrun devicectl device install app --device "$DEVICE" "$APP"

stage "Launch"
if xcrun devicectl device process launch --device "$DEVICE" com.hallya.ioniqhudnav >>"$LOG" 2>&1; then
  echo "ok"
else
  echo "trust needed: Settings > General > VPN & Device Management > trust your Apple ID"
fi

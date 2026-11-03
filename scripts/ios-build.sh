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
run() {
  if "$@" >>"$LOG" 2>&1; then echo "ok"; else
    echo "FAILED (see build/ios-build.log)"; tail -n 20 "$LOG"; exit 1
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

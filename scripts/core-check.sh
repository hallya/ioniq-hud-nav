#!/usr/bin/env bash
# Format, lint and test the core from vendored crates, with no network,
# then check the build-time constants script on the synthetic upstream copies.
# Same command in CI and locally. Run from the repo root:
#   scripts/core-check.sh
# Fails loudly if vendor/ does not match Cargo.lock: after changing a core
# dependency, run `cargo vendor --locked vendor` and commit vendor/.
set -euo pipefail
cd "$(dirname "$0")/.."

offline=(--config .cargo/vendor.toml --offline --locked)

test -f vendor/serde_json/Cargo.toml || { echo "core-check: vendor/ is missing or incomplete" >&2; exit 1; }

cargo fmt --all --check
cargo clippy "${offline[@]}" --all-targets -- -D warnings
cargo test "${offline[@]}" 2>&1 | tee test.log
grep -q 'test result: ok' test.log || { echo "core-check: no passing test run found" >&2; exit 1; }
python3 -I scripts/test_app_constants.py

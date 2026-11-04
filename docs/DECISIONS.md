# Decisions

One line each, newest first. Settled decisions are not reopened without new facts.

- 2026-11-04 · Step 2 starts while step 1's last check (the send) waits for the car's repair. The car leg was proven in step 0 and the whole chain up to the send works against Hyundai's servers, so rule 6's risk is small. Asked by the user.
- 2026-11-04 · Sending a destination stays on Bluelink: no known local path. Rejected: rooting the Gen5W head unit (brick risk, undone by updates, no known HUD access) and writing to the CAN bus (unsafe while driving). Open, for the user: an OBD BLE dongle for live battery data, and a CarPlay HUD test that could change the premise.
- 2026-11-04 · End-to-end tests run against `scripts/e2e/mock_bluelink.py` (stdlib Python, synthetic fixtures, fresh RSA key per start): on Linux through UniFFI Python bindings, and on the iOS Simulator through a UI test, in CI on GitHub's free macOS runners (`macos-26`). The user's iPhone only tests what a mock cannot: Hyundai's servers, signing, the car. Proposed by the user.
- 2026-11-03 · The core drives the whole Bluelink chain (`bluelink::run::Run`): Swift only performs effects (HTTP, RSA, clock, randomness). It keeps protocol order and error mapping testable on Linux and the Swift shell small enough to be right on its first Mac compile.
- 2026-11-03 · The password never crosses the bridge: the core asks Swift to encrypt it and gives the key as PKCS#1 DER (built in the core, the form `SecKeyCreateWithData` takes). The PIN and email do cross, as the requests carry them.
- 2026-11-03 · Randomness comes as one 72-byte buffer per run (UUID v4, push id, two CCI nonces), formatted in the core, so tests replay the fixtures byte for byte.
- 2026-11-03 · A stored signature override is tied to the SHA-256 of the bundled constants: a new build with newer constants never runs with an older override.
- 2026-11-03 · Test destination is a public landmark (Tour Eiffel), defined in the core, never a place of the user.
- 2026-11-03 · The first vehicle of the account is used; a charge read failure leaves `soc -` and the send goes on.
- 2026-11-03 · Build-time constants come from `scripts/app-constants.py` (Python 3.9+ stdlib, `ast` evaluation of literals only; no import or run of upstream code, no pip). The in-car repair parses only `cci_fingerprint.py` in the core with a small lexer. Both are anchored on `constants_synthetic.json` through synthetic copies of upstream's files, so they cannot drift apart unnoticed. Unreadable dict entries (a new brand) are skipped, the Hyundai entry is required.
- 2026-11-03 · Values read from upstream go into HTTP headers, so both readers accept only printable ASCII, no escapes, at most 128 bytes.

- 2026-11-03 · App constants are bundled into the app by `scripts/ios-build.sh` at each build (the 7-day profile forces a weekly rebuild anyway). Repair in the car: on a `gate9035`/`gate9036`, the app offers one button that downloads upstream's current `cci_fingerprint.py` from GitHub, the core extracts only the Hyundai signing key and client identity (hosts and ids stay bundled), stores it as an override until the next build, and retries. If upstream has not caught up yet, myHyundai's own "send to car" is the fallback for that trip (proven in step 0).

- 2026-11-03 · Bluelink core is sans-IO: one request builder and one parser per step; time, random values, password hex and PIN come in from the shell. Tokens stay raw, builders add `Bearer `. `Host`, `Connection` and `Accept-Encoding` are left to URLSession.
- 2026-11-03 · Until a real exchange is recorded, Bluelink fixtures are synthetic and come from `scripts/gen-bluelink-fixtures.py`, a stdlib-Python re-implementation of upstream's chain (independent oracle). Sanitized recordings get added next to them once the app runs.
- 2026-11-03 · Not ported from upstream: the unsigned legacy CCI fallback (transitional there; a gate rejection must surface as `gate90xx` so the constants get renewed), the pre-login cookie and session-language calls (they only set message language), token refresh (re-login each run for now; revisit if rate limits appear).
- 2026-11-03 · base64 is a 40-line module in the core rather than a new dependency.

- 2026-11-03 · Bluelink EU `X-Fingerprint` (HMAC-SHA256) is computed in the core with RustCrypto `hmac` + `sha2`, vendored (vendor/ now 17 MB, `libc` comes with `sha2`'s CPU detection). The app constants are passed in as data, never hard-coded, so they can be renewed without code changes. RSA stays in Swift.
- 2026-11-03 · Core may use serde + serde_json, vendored under `vendor/` and built offline by `scripts/core-check.sh` (CI too); no other core dependency without a new decision. `ffi/` is not vendored (79 MB) and resolves from crates.io. New fact: crates.io became reachable from the cloud workspace; Bluelink replies are nested JSON.
- 2026-11-03 · Test-first for core logic and protocols (fixtures), CI steps reproducible locally; rules in CLAUDE.md.
- 2026-11-03 · Repo, code, docs and commits in English.
- 2026-11-03 · Repo is public: free CI, nothing secret or personal ever committed.
- 2026-11-03 · ~~Core has zero dependencies~~ (superseded above); UniFFI lives in a separate `ffi` crate.
- 2026-11-03 · Chargers outside France: OpenStreetMap by default (no key); OCM re-evaluated in step 2 by measured coverage.
- 2026-11-03 · Builds and installs run on the user's Mac via Terminal (computer use); free Apple Personal Team, 7-day profiles.
- 2026-11-03 · The car guides: the app only sends the next destination through Bluelink EU; native nav drives the HUD.
- 2026-11-03 · iPhone computes everything; no server, no paid service.

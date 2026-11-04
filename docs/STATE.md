# State

Updated: 2026-11-04 (late morning)

**Step:** 2, EV brain (starts in a new conversation, 2026-11-04). Step 1 is parked: everything works against Hyundai's real servers except the send, which waits for the car's repair (see "Step 1, when the car is back").

**Done**
- Step 0: destination sent from myHyundai reaches the car's nav and the HUD guides. The car asks for confirmation on its screen. Not yet checked: battery preconditioning on a sent DC charger, multiple waypoints.
- Repo, CI, core with diagnostic line, UniFFI bridge, SwiftUI Diagnostic screen, Mac build script.
- Core deps: serde + serde_json + hmac + sha2, vendored; `scripts/core-check.sh` builds and tests the core offline (CI runs it). Both CI jobs, bridge and Swift bindings included, also run in Claude's workspace.
- Bluelink EU protocol in the core (`core/src/bluelink/`), sans-IO, 36 tests: device registration (`spa::register_request`, `Stamp`), IDP authorize / RSA key / sign-in (`idp`), CCI token and CCS exchange with `X-Fingerprint` (`cci`), vehicle list, state of charge (CCS2 and legacy), PIN control token, send destination (`spa`). Errors give short Diagnostic codes (`gate9036`, `4002`, `creds`, `login20`, `pin`...). Module doc in `bluelink/mod.rs` lists the call order.
- Fixtures: synthetic, written by `scripts/gen-bluelink-fixtures.py`, an independent stdlib-Python oracle of upstream's chain. No recording exists yet.
- App constants at build time: `scripts/app-constants.py` reads upstream's `const.py`, `ApiImplType1.py`, `KiaUvoApiEU.py`, `cci_fingerprint.py` with `ast` (never runs them) and writes `ios/Generated/bluelink_constants.json`; `scripts/ios-build.sh` stage "Bluelink constants" fetches upstream at one commit (`HUDNAV_UPSTREAM_REF` pins it) and fails if the bundle lacks the file. Dry-run in the cloud workspace on upstream `b952510`: ok. Not yet run on the Mac.
- Signature repair in the core: `bluelink::upstream::FingerprintOverride::from_upstream` reads the Hyundai signing key and Android build identity from `cci_fingerprint.py` text, header-safe values only (`parse-fpkey`, `parse-fpident`, `parse-fpvalue`); `AppConstants::with_override` applies it. Core and script agree on the real file (ignored test `live_upstream_file_parses`, run with `HUDNAV_UPSTREAM_FP` and `HUDNAV_UPSTREAM_CONSTANTS`).
- Chain driver in the core: `bluelink::run::Run`, sans-IO, drives register → IDP → CCI/CCS → vehicles → charge → PIN → send and ends with the Diagnostic report and a `repair_signature` flag (gate 9035/9036). Shell gives the clock on each call and 72 random bytes once (UUID, push id, two nonces); the password stays in Swift: the core hands over the IDP key as PKCS#1 DER (`idp::public_key_der`, checked against openssl vectors in `fixtures/bluelink_eu/rsa_vectors.json`) and takes the ciphertext. Transport failures carry the URLError code (`net1009` offline, `net1200` TLS, `net1022` ATS). Empty destination = connection check. 11 tests replay the fixture chain end to end. Charge failure does not stop the send; first vehicle of the account is used.
- Exported through UniFFI: `BluelinkRun` (`start`, `resume`), `fingerprint_override_from_upstream`, `test_destination` (Tour Eiffel, a public landmark), `bluelink_entropy_len`.
- Swift shell (not compiled yet, no Swift toolchain in the cloud): `BluelinkClient` (ephemeral URLSession per run, per-task no-redirect delegate, SecKey RSA PKCS#1), `Keychain` (email, password, PIN), `SignatureRepair` (downloads upstream `cci_fingerprint.py`, stores the override in Application Support with the SHA-256 of the bundled constants, so a new build drops it). Diagnostic screen: "Send test destination", "Check connection only", "Update signature and retry", account form.
- End-to-end tests against `scripts/e2e/mock_bluelink.py` (synthetic fixtures on 127.0.0.1; checks the IDP cookie, the unfollowed sign-in 302, the RSA-decrypted password, PIN, destination): `scripts/e2e-linux.sh` (core through UniFFI Python bindings, 4 scenarios incl. 2 self-tests; CI ffi job) and `scripts/ios-sim-test.sh` (CI job `ios-sim` on `macos-26`: Simulator build, static-link check, UI test of account entry → "Check connection only" → "Send test destination", then the mock's checks). Both green on 2026-11-04 (commit 3980d74). Failure lines of the Simulator job are annotations: readable unauthenticated through `check-runs/{job id}/annotations`.
- Synthetic copies of the four upstream files in `fixtures/bluelink_eu/upstream_synthetic/`; CI checks the script turns them into `constants_synthetic.json` (`scripts/test_app_constants.py`, run by `core-check.sh`).

**Bluelink EU findings** (reference: Hyundai-Kia-Connect/hyundai_kia_connect_api, MIT, very active; re-read at upstream `b952510`, 2026-11-03, unchanged since the morning)
- Path to use: upstream's `KiaUvoApiEU` (region 1, legacy SPA endpoints on `prd.eu-ccapi.hyundai.com:8080`). It has `set_navigation`. The newer GSPA path (`HyundaiCciApiEU`, region 9) has no send-destination call.
- Chain: (1) Device id: `POST /api/v1/spa/notifications/register` with a `Stamp` header, done first. (2) IDP login on idpconnect-eu.hyundai.com: authorize → RSA key → sign in with RSA-PKCS1 encrypted password → 302 with auth code; one cookie store, no redirect follow on sign-in. (3) CCI tokens: `POST cci-api-eu.hyundai.com/auth/v1/token?code=…`. (4) CCS token: `POST …/auth/v1/token-exchange?serviceType=CCS`, TTL 1 h. (5) Vehicle list, then `…/ccs2/carstatus/latest` (or `status/latest`) for the charge. (6) **Control token: `PUT /api/v1/user/pin` with the Bluelink PIN** (new finding 2026-11-03: send destination needs it). (7) Send destination: `POST /api/v2/spa/vehicles/{id}/location/routes` with `deviceID` and `poiInfoList` (several waypoints possible); returns `msgId`.
- Steps (3) and (4) are fingerprint-gated since 2026-10-14. `X-Fingerprint` = HMAC-SHA256, key = client identity + three constants recovered from the official Android OneApp 1.4.0, message = identity + OS version + `X-Timestamp` + body without whitespace. Headers present the Android app identity. Errors 9035 missing, 9036 bad signature, 9037 timestamp out of window. Constants rotate with app releases.
- `Stamp` = base64(app CFB constant XOR "APP_ID:unix_seconds").
- Crypto split: RSA in the Swift shell (Security framework); HMAC-SHA256 and Stamp in the core.
- Constants rotation: none seen so far (Hyundai constants and client version 1.4.0 unchanged since 2026-10-14).
- Not ported (see DECISIONS): unsigned legacy CCI fallback, pre-login cookie and session-language calls, token refresh (`/auth/v2/token-refresh`).
- Fragile: upstream changes often (WAF blocks, consent prompts, fingerprint gate). Plan a weekly scheduled check of upstream's EU files.

**Direct access to the car** (explored 2026-11-04; sources in the plan doc, section "Accès direct à la voiture")
- No known way to send a destination without Hyundai's servers, short of modifying the head unit. Bluelink stays the send path.
- CarPlay turn-by-turn in the HUD: iOS 26 lets navigation apps (Google Maps since iOS 26) send maneuver metadata to the cluster/HUD of cars that support it (WWDC25 "Turbocharge your app for CarPlay"). Reported on ccNC Hyundai/Kia (EV9), not on 2022-2024 EV6: the user's Gen5W is probably excluded. User test pending (Apple Maps, then Google Maps, HUD arrows or not). If it works, the project's premise changes: discuss before building more.
- OBD-II BLE dongle (e.g. OBDLink CX, used on EV6 with ABRP; iPhone needs BLE): live SoC and battery temperatures, car on, no server, read-only. Option for the user to decide; it would replace Hyundai's cached charge in the planner.
- Rejected: rooting the Gen5W head unit (brick risk, undone by updates, no known HUD access); writing to the CAN bus (unsafe while driving).
- ioniqforum.com sends automated readers to a TollBit paywall: do not try to read it; ask the user to open threads there.

**Next** (step 2, EV brain; plan doc "Étape 2": exit = reference trips coherent, plan under 50 ms; the user has nothing to do)
1. Charge age first, test-first: find the status timestamp upstream reads for CCS2 and legacy status, add it to the fixtures, return it with the SoC, show the age on the Diagnostic line and in every plan.
2. Charger database: a build script run weekly in GitHub Actions, IRVE (France) + OpenStreetMap `amenity=charging_station` (rest of Europe), written to a compact file bundled in the app. A small sanitized sample of each source goes in `fixtures/` before its parser (rule 2). Measure coverage against Open Charge Map on a few corridors before deciding on OCM (DECISIONS).
3. IONIQ 6 consumption model (77.4 kWh RWD, 18" wheels): speed, elevation, temperature, climate; cite the reference values used.
4. DC charging curve for the 77.4 kWh pack (power by SoC, cold-battery effect).
5. Planner: route (openrouteservice at run time, fixtures in tests) + SoC and its age + chargers → stops and arrival SoC; benchmark in CI, under 50 ms on the phone.
6. Reference trips (e.g. Paris → Lyon at 80 % in winter) as tests; the user may compare one plan with ABRP.

**Step 1, when the car is back** (user)
1. CarPlay HUD test (see "Direct access").
2. "Send test destination", confirm on the car's screen, send the line. Only the PIN control token and the route call are unproven. On `ko`: `send ko:pin` = PIN refused, `send ko:4002` = device id, other codes in `spa.rs`; reproduce in the mock or a core test first, fix, CI green (rule 7), then a new phone build.
3. Once all `ok`: record sanitized real exchanges as fixtures next to the synthetic ones.
4. Already proven on 2026-11-04 09:32: "Check connection only" gave `auth ok · veh ok · soc 83 · send -` (registration, IDP login, fingerprint gate, CCS token, vehicle list, CCS2 charge from Hyundai's cache).

**Exit criterion (step 2):** reference trips coherent, plan computed in under 50 ms.

**Blockers:** car out of service since 2026-11-04 (step 1's last check waits for it). openrouteservice sign-up page returned 404 on 2026-11-03; the user needs a free key before the planner runs on the phone, not for core tests.

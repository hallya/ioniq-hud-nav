# State

Updated: 2026-11-03 (late evening)

**Step:** 1, skeleton and proof of life (in progress).

**Done**
- Step 0: destination sent from myHyundai reaches the car's nav and the HUD guides. The car asks for confirmation on its screen. Not yet checked: battery preconditioning on a sent DC charger, multiple waypoints.
- Repo, CI, core with diagnostic line, UniFFI bridge, SwiftUI Diagnostic screen, Mac build script.
- Core deps: serde + serde_json + hmac + sha2, vendored; `scripts/core-check.sh` builds and tests the core offline (CI runs it). Both CI jobs, bridge and Swift bindings included, also run in Claude's workspace.
- Bluelink EU protocol in the core (`core/src/bluelink/`), sans-IO, 36 tests: device registration (`spa::register_request`, `Stamp`), IDP authorize / RSA key / sign-in (`idp`), CCI token and CCS exchange with `X-Fingerprint` (`cci`), vehicle list, state of charge (CCS2 and legacy), PIN control token, send destination (`spa`). Errors give short Diagnostic codes (`gate9036`, `4002`, `creds`, `login20`, `pin`...). Module doc in `bluelink/mod.rs` lists the call order.
- Fixtures: synthetic, written by `scripts/gen-bluelink-fixtures.py`, an independent stdlib-Python oracle of upstream's chain. No recording exists yet.

**Bluelink EU findings** (reference: Hyundai-Kia-Connect/hyundai_kia_connect_api, MIT, very active; re-read at upstream `b952510`, 2026-11-03, unchanged since the morning)
- Path to use: upstream's `KiaUvoApiEU` (region 1, legacy SPA endpoints on `prd.eu-ccapi.hyundai.com:8080`). It has `set_navigation`. The newer GSPA path (`HyundaiCciApiEU`, region 9) has no send-destination call.
- Chain: (1) Device id: `POST /api/v1/spa/notifications/register` with a `Stamp` header, done first. (2) IDP login on idpconnect-eu.hyundai.com: authorize → RSA key → sign in with RSA-PKCS1 encrypted password → 302 with auth code; one cookie store, no redirect follow on sign-in. (3) CCI tokens: `POST cci-api-eu.hyundai.com/auth/v1/token?code=…`. (4) CCS token: `POST …/auth/v1/token-exchange?serviceType=CCS`, TTL 1 h. (5) Vehicle list, then `…/ccs2/carstatus/latest` (or `status/latest`) for the charge. (6) **Control token: `PUT /api/v1/user/pin` with the Bluelink PIN** (new finding 2026-11-03: send destination needs it). (7) Send destination: `POST /api/v2/spa/vehicles/{id}/location/routes` with `deviceID` and `poiInfoList` (several waypoints possible); returns `msgId`.
- Steps (3) and (4) are fingerprint-gated since 2026-10-14. `X-Fingerprint` = HMAC-SHA256, key = client identity + three constants recovered from the official Android OneApp 1.4.0, message = identity + OS version + `X-Timestamp` + body without whitespace. Headers present the Android app identity. Errors 9035 missing, 9036 bad signature, 9037 timestamp out of window. Constants rotate with app releases.
- `Stamp` = base64(app CFB constant XOR "APP_ID:unix_seconds").
- Crypto split: RSA in the Swift shell (Security framework); HMAC-SHA256 and Stamp in the core.
- Constants rotation: none seen so far (Hyundai constants and client version 1.4.0 unchanged since 2026-10-14).
- Not ported (see DECISIONS): unsigned legacy CCI fallback, pre-login cookie and session-language calls, token refresh (`/auth/v2/token-refresh`).
- Fragile: upstream changes often (WAF blocks, consent prompts, fingerprint gate). Plan a weekly scheduled check of upstream's EU files.

**Next**
1. Decide how the app gets the real app constants (open question for the user). Proposed: a JSON file in this repo, kept up to date by a scheduled GitHub Action that reads upstream and commits when tests pass; the app re-fetches it on a 9035/9036/9037 rejection. `AppConstants::from_json` already reads that shape (see `fixtures/bluelink_eu/constants_synthetic.json`).
2. Wire the chain into the app: export the Bluelink steps through UniFFI; Swift executor on URLSession (one cookie store per login, no-redirect delegate, final URL in `Response.url`); RSA-PKCS1 in Swift; Keychain entries for email, password and **Bluelink PIN**; "Send test destination" runs the chain and fills the Diagnostic line.
3. First build on the Mac (`scripts/ios-build.sh`) through computer use on Terminal.
4. User: trust the developer profile, sign in to Bluelink in the app (email, password, PIN), tap "Send test destination", paste the Diagnostic line.

**Exit criterion:** test destination arrives in the car and the Diagnostic line is all `ok`.

**Blockers:** none. openrouteservice sign-up page currently returns 404 (only needed from step 2).

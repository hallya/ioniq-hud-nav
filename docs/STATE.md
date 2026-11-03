# State

Updated: 2026-11-03

**Step:** 1, skeleton and proof of life (in progress).

**Done**
- Step 0: destination sent from myHyundai reaches the car's nav and the HUD guides. The car asks for confirmation on its screen. Not yet checked: battery preconditioning on a sent DC charger, multiple waypoints.
- Repo, CI, core with diagnostic line, UniFFI bridge, SwiftUI Diagnostic screen, Mac build script.
- Core deps: serde + serde_json, vendored; `scripts/core-check.sh` builds and tests the core offline (CI runs it). Both CI jobs, bridge and Swift bindings included, now also run in Claude's workspace.

**Bluelink EU findings** (reference: Hyundai-Kia-Connect/hyundai_kia_connect_api, MIT, very active; re-read at upstream `b952510`, 2026-11-03)
- Path to use: upstream's `KiaUvoApiEU` (region 1, legacy SPA endpoints on `prd.eu-ccapi.hyundai.com:8080`). It has `set_navigation`. The newer GSPA path (`HyundaiCciApiEU`, region 9) has no send-destination call.
- Chain: (1) IDP login on idpconnect-eu.hyundai.com: authorize → RSA key → sign in with RSA-PKCS1 encrypted password → auth code. (2) CCI tokens: `POST cci-api-eu.hyundai.com/auth/v1/token?code=…`. (3) CCS token: `POST …/auth/v1/token-exchange?serviceType=CCS`, TTL now 1 h. (4) Device id: `POST /api/v1/spa/notifications/register` with a `Stamp` header. (5) Vehicle list. (6) Send destination: `POST /api/v2/spa/vehicles/{id}/location/routes` with `deviceID` and `poiInfoList` (list with `waypointID`, so several waypoints may be possible); returns `msgId`.
- New since 2026-10-14: steps (2) and (3) are fingerprint-gated. `X-Fingerprint` = HMAC-SHA256, key = client identity + three constants recovered from the official Android OneApp 1.4.0, message = identity + OS version + `X-Timestamp` + body without whitespace. Headers present the Android app identity. Errors 9035 missing, 9036 bad signature, 9037 timestamp out of window. Constants rotate with app releases.
- `Stamp` = base64(app CFB constant XOR "APP_ID:unix_seconds").
- Crypto split: RSA in the Swift shell (Security framework); HMAC-SHA256 in the core (`core/src/bluelink/fingerprint.rs`, done, tested against `fixtures/bluelink_eu/fingerprint_vectors.json`).
- Constants rotation: none seen so far (Hyundai constants and client version 1.4.0 unchanged since 2026-10-14).
- Fragile: upstream changes often (WAF blocks, consent prompts, fingerprint gate). Plan a weekly scheduled check of upstream's EU files.

**Next**
1. Bluelink EU protocol in the core, test-first against sanitized fixtures: login steps, CCS exchange (signed), device registration (`Stamp`), vehicle list, state of charge, send destination.
   Proposed, not decided: deliver the app constants as a JSON file in this repo, kept up to date by a scheduled GitHub Action that reads upstream and commits when tests pass; the app re-fetches it on a 9035/9036/9037 rejection.
2. First build on the Mac (`scripts/ios-build.sh`) through computer use on Terminal.
3. User: trust the developer profile, sign in to Bluelink in the app, tap "Send test destination", paste the Diagnostic line.

**Exit criterion:** test destination arrives in the car and the Diagnostic line is all `ok`.

**Blockers:** none. openrouteservice sign-up page currently returns 404 (only needed from step 2).

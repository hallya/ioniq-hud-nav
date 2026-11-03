# State

Updated: 2026-11-03

**Step:** 1, skeleton and proof of life (in progress).

**Done**
- Step 0: destination sent from myHyundai reaches the car's nav and the HUD guides. The car asks for confirmation on its screen. Not yet checked: battery preconditioning on a sent DC charger, multiple waypoints.
- Repo, CI, dependency-free core with diagnostic line, UniFFI bridge, SwiftUI Diagnostic screen, Mac build script.

**Bluelink EU findings** (reference: Hyundai-Kia-Connect/hyundai_kia_connect_api, MIT, very active)
- Send destination: `POST /api/v2/spa/vehicles/{id}/location/routes` with `deviceID` and `poiInfoList`, a list with `waypointID`, so several waypoints may be possible.
- Login (OneApp/CCI flow, idpconnect-eu.hyundai.com): authorize → fetch RSA key → sign in with RSA-PKCS1 encrypted password → CCI tokens → CCS token for the vehicle API. RSA is done by the Swift shell (Security framework); the core stays dependency-free.
- Fragile: upstream changes often (WAF blocks, consent prompts). Plan a weekly scheduled check of upstream's EU files.

**Next**
1. Bluelink EU protocol in the core, test-first against sanitized fixtures: login steps, vehicle list, state of charge, send destination.
2. First build on the Mac (`scripts/ios-build.sh`) through computer use on Terminal.
3. User: trust the developer profile, sign in to Bluelink in the app, tap "Send test destination", paste the Diagnostic line.

**Exit criterion:** test destination arrives in the car and the Diagnostic line is all `ok`.

**Blockers:** none. openrouteservice sign-up page currently returns 404 (only needed from step 2).

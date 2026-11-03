#!/usr/bin/env python3
"""Write the Bluelink EU protocol fixtures used by the core tests.

This is the test oracle: an independent, stdlib-only re-implementation of the
request chain of upstream hyundai_kia_connect_api (MIT), commit b952510
(`KiaUvoApiEU._login_with_password_cci`, `cci_auth.py`, `cci_fingerprint.py`,
`ApiImplType1` register / vehicles / status / pin / set_navigation).

Everything here is synthetic: example.test hosts, fake ids, fake app constants,
fake tokens. No real constant, token, VIN, name or coordinate of the user.
The response bodies follow the shapes upstream parses; they are not recordings.

Run from the repo root and commit the output:
    python3 scripts/gen-bluelink-fixtures.py
"""

import base64
import hashlib
import hmac
import json
import re
from pathlib import Path
from urllib.parse import urlencode

OUT = Path(__file__).resolve().parent.parent / "fixtures" / "bluelink_eu"
UPSTREAM = "Hyundai-Kia-Connect/hyundai_kia_connect_api (MIT) at b952510"

CONSTANTS = {
    "idp_host": "https://idp.example.test",
    "cci_host": "https://cci.example.test",
    "spa_host": "https://spa.example.test:8080",
    "oneapp_client_id": "00000000-0000-4000-8000-0000000000a1",
    "oneapp_redirect_uri": "https://oneapp.example.test/redirect",
    "ccsp_service_id": "00000000-0000-4000-8000-0000000000b2",
    "app_id": "00000000-0000-4000-8000-0000000000c3",
    "cfb_base64": base64.b64encode(bytes((i * 37 + 11) % 256 for i in range(50))).decode(),
    "push_type": "GCM",
    "idp_user_agent": "Mozilla/5.0 (Linux; Android 10; K) ExampleBrowser/1.0_CCS_APP_AOS",
    "spa_user_agent": "okhttp/3.12.0",
    "client": {
        "package_id": "com.example.oneapp.eu",
        "client_name": "ExampleEU",
        "version": "9.9.9",
        "os_code": "android",
        "os_version": "14",
        "device_model": "TEST-MODEL",
        "notification_provider": "FCM",
        "user_agent": "okhttp/4.12.0",
        "brand_label": "Example",
        "region": "eu",
    },
    "signing_key": {"secret": "TESTsecret01", "piece": "TESTpiece02", "suffix": "TESTsufx03"},
}

LOCALE = {"language": "fr", "timezone": "+01:00"}
DEVICE_ID = "11111111-2222-4333-8444-555555555555"
TIMESTAMP_MS = 1793740000000
REQUEST_NONCE = "0123456789ABCDEF01234567"
NOW_S = 1793740000
C = CONSTANTS


def dumps(value):
    """Compact JSON, field order kept: what serde_json writes."""
    return json.dumps(value, separators=(",", ":"), ensure_ascii=False)


def request(method, url, headers, body=None, follow_redirects=True):
    return {
        "method": method,
        "url": url,
        "headers": [[k, v] for k, v in headers.items()],
        "body": body,
        "follow_redirects": follow_redirects,
    }


def response(status, body="", headers=None, url=""):
    return {
        "status": status,
        "headers": [[k, v] for k, v in (headers or {}).items()],
        "url": url,
        "body": body if isinstance(body, str) else dumps(body),
    }


# --- Stamp and fingerprint, as upstream computes them ------------------------------


def stamp(now_s):
    cfb = base64.b64decode(C["cfb_base64"])
    raw = f"{C['app_id']}:{now_s}".encode()
    return base64.b64encode(bytes(a ^ b for a, b in zip(cfb, raw))).decode()


def fingerprint(headers, body=""):
    lower = {k.lower(): v for k, v in headers.items()}
    identity = "".join(
        lower.get(n, "") for n in ("client-id", "client-name", "client-version", "client-os-code")
    )
    k = C["signing_key"]
    key = identity + k["secret"] + k["piece"] + k["suffix"]
    msg = (
        identity
        + lower.get("client-os-version", "")
        + lower.get("x-timestamp", "")
        + re.sub(r"\s+", "", body or "", flags=re.ASCII)
    )
    return hmac.new(key.encode(), msg.encode(), hashlib.sha256).hexdigest()


def fp_headers(body=""):
    c = C["client"]
    h = {
        "client-id": c["package_id"],
        "client-name": c["client_name"],
        "client-version": c["version"],
        "client-os-code": c["os_code"],
        "client-os-version": c["os_version"],
        "client-device-id": DEVICE_ID,
        "client-device-model": c["device_model"],
        "client-notification-provider-type": c["notification_provider"],
        "locale": LOCALE["language"].upper(),
        "timezone": LOCALE["timezone"],
        "App-Request-Id": f"{REQUEST_NONCE}_{DEVICE_ID.replace('-', '')}_AOS",
        "X-Brand": c["brand_label"],
        "X-Region": c["region"],
        "User-Agent": c["user_agent"],
        "Accept": "*/*",
        "Accept-Language": LOCALE["language"].lower(),
    }
    h["X-Timestamp"] = str(TIMESTAMP_MS)
    h["X-Fingerprint"] = fingerprint(h, body)
    return h


def spa_headers(access_token, ccs2):
    """ApiImplType1._get_authenticated_headers without Host, Connection and
    Accept-Encoding, which URLSession sets itself."""
    return {
        "Authorization": f"Bearer {access_token}",
        "ccsp-service-id": C["ccsp_service_id"],
        "ccsp-application-id": C["app_id"],
        "Stamp": stamp(NOW_S),
        "ccsp-device-id": DEVICE_ID,
        "Ccuccs2protocolsupport": str(ccs2),
        "User-Agent": C["spa_user_agent"],
    }


def ok_envelope(res_msg, msg_id="msg-0001"):
    return {"retCode": "S", "resCode": "0000", "resMsg": res_msg, "msgId": msg_id}


# --- Fixture files -----------------------------------------------------------------


def stamp_vectors():
    cases = [{"now_s": s, "expected": stamp(s)} for s in (NOW_S, 1700000000, 4102444800)]
    return {"about": f"Stamp vectors, synthetic constants, algorithm of {UPSTREAM}.", "cases": cases}


def idp():
    idp_ua = {"User-Agent": C["idp_user_agent"]}
    authorize_url = (
        f"{C['idp_host']}/auth/api/v2/user/oauth2/authorize"
        f"?response_type=code&client_id={C['oneapp_client_id']}"
        f"&redirect_uri={C['oneapp_redirect_uri']}&lang=en&state=ccsp&country=de"
    )
    username = "user+test@example.test"
    encrypted = "ab" * 8
    form = urlencode(
        {
            "client_id": C["oneapp_client_id"],
            "encryptedPassword": "true",
            "password": encrypted,
            "redirect_uri": C["oneapp_redirect_uri"],
            "scope": "",
            "nonce": "",
            "state": "ccsp",
            "username": username,
            "connector_session_key": "",
            "kid": "test-kid",
            "_csrf": "",
        }
    )
    redirect = C["oneapp_redirect_uri"]
    loc = lambda q: {"Location": f"{redirect}?{q}"}  # noqa: E731
    login_page = f"{C['idp_host']}/auth/api/v2/user/oauth2/authorize?state=ccsp"
    return {
        "about": f"IDP login steps 1-3 (authorize, RSA key, sign-in), synthetic, shapes from {UPSTREAM}.",
        "authorize": {
            "request": request("GET", authorize_url, idp_ua),
            "responses": [
                {"name": "login page", "response": response(200, "<html>sign in</html>", url=f"{C['idp_host']}/auth/login"), "expect": {"ok": None}},
                {"name": "waf body", "response": response(200, "<html>Abusing request detected</html>"), "expect": {"err": "waf"}},
                {"name": "waf url", "response": response(200, "<html></html>", url=f"{C['idp_host']}/error?status=400"), "expect": {"err": "waf"}},
                {"name": "server error", "response": response(503, "down"), "expect": {"err": "http503"}},
            ],
        },
        "certs": {
            "request": request("GET", f"{C['idp_host']}/auth/api/v1/accounts/certs", idp_ua),
            "responses": [
                {"name": "jwk", "response": response(200, {"retCode": "S", "retValue": {"kty": "RSA", "e": "AQAB", "kid": "test-kid", "n": "sXchDaQebHnPiGvyDOAT4saGEUetSyo9MKLOoWFsueri23bOdgWp4Dy1WlUzewbgBHod5pcM9H95GQRV3JDXboIRROSBigeC5yjU1hGzHHyXss8UDprecbAYxknTcQkhslANGRUZmdTOQ5qTRsLAt6BTYuyvVRdhS8exSZEy_c4gs_7svlJJQ4H9_NxsiIoLwAEk7-Q3UXERGYw_75IDrGA84-lA_-Ct4eTlXHBIY2EaV7t7LjJaynVJCpkv4LKjTTAumiGUIuQhrNhZLuF_RJLqHpM2kgWFLU7-VTdL1VbC2tejvcI2BlMkEpk1BzBZI0KQB0GaDWFLN-aEAw3vRw"}}), "expect": {"ok": {"kid": "test-kid", "n": "sXchDaQebHnPiGvyDOAT4saGEUetSyo9MKLOoWFsueri23bOdgWp4Dy1WlUzewbgBHod5pcM9H95GQRV3JDXboIRROSBigeC5yjU1hGzHHyXss8UDprecbAYxknTcQkhslANGRUZmdTOQ5qTRsLAt6BTYuyvVRdhS8exSZEy_c4gs_7svlJJQ4H9_NxsiIoLwAEk7-Q3UXERGYw_75IDrGA84-lA_-Ct4eTlXHBIY2EaV7t7LjJaynVJCpkv4LKjTTAumiGUIuQhrNhZLuF_RJLqHpM2kgWFLU7-VTdL1VbC2tejvcI2BlMkEpk1BzBZI0KQB0GaDWFLN-aEAw3vRw", "e": "AQAB"}}},
                {"name": "server error", "response": response(500, "oops"), "expect": {"err": "http500"}},
                {"name": "no modulus", "response": response(200, {"retValue": {"kid": "k", "e": "AQAB"}}), "expect": {"err": "parse-certs"}},
            ],
        },
        "signin": {
            "inputs": {"username": username, "encrypted_password_hex": encrypted, "kid": "test-kid"},
            "request": request(
                "POST",
                f"{C['idp_host']}/auth/account/signin",
                {**idp_ua, "Content-Type": "application/x-www-form-urlencoded"},
                form,
                follow_redirects=False,
            ),
            "responses": [
                {"name": "code", "password_chars": 12, "response": response(302, headers=loc("code=CODE-123_abc.~&state=ccsp")), "expect": {"ok": "CODE-123_abc.~"}},
                {"name": "lower-case location header", "password_chars": 12, "response": response(302, headers={"location": f"{redirect}?state=ccsp&code=C%2B1"}), "expect": {"ok": "C+1"}},
                {"name": "invalid credentials", "password_chars": 12, "response": response(302, headers=loc("error=access_denied&error_description=Invalid+credentials")), "expect": {"err": "creds"}},
                {"name": "other rejection", "password_chars": 12, "response": response(302, headers=loc("error=access_denied&error_description=Account+locked")), "expect": {"err": "signin"}},
                {"name": "consent", "password_chars": 12, "response": response(302, headers={"Location": f"{C['idp_host']}/web/v1/user/authorization?x=1"}), "expect": {"err": "consent"}},
                {"name": "back to login", "password_chars": 12, "response": response(302, headers={"Location": login_page}), "expect": {"err": "login"}},
                {"name": "back to login, long password", "password_chars": 24, "response": response(302, headers={"Location": login_page}), "expect": {"err": "login20"}},
                {"name": "unexpected redirect", "password_chars": 12, "response": response(302, headers={"Location": "https://elsewhere.example.test/?no_code=1"}), "expect": {"err": "redirect"}},
                {"name": "no redirect", "password_chars": 12, "response": response(200, "<html>login</html>"), "expect": {"err": "http200"}},
            ],
        },
    }


def cci():
    tokens = {
        "accessToken": "cci-access",
        "refreshToken": "cci-refresh",
        "nonCcsToken": "non-ccs",
        "exchangeableAccessToken": "xch-access",
        "exchangeableRefreshToken": "xch-refresh",
        "nonCcsRefreshToken": "non-ccs-refresh",
        "idToken": "id-token",
        "expiresIn": 3599,
    }
    parsed = {
        "access_token": "cci-access",
        "refresh_token": "cci-refresh",
        "non_ccs_token": "non-ccs",
        "exchangeable_access_token": "xch-access",
        "exchangeable_refresh_token": "xch-refresh",
        "non_ccs_refresh_token": "non-ccs-refresh",
        "id_token": "id-token",
        "expires_in_s": 3599,
    }
    exchange_headers = fp_headers()
    exchange_headers["authorization"] = "Bearer cci-access"
    exchange_headers["Authentication"] = "non-ccs"
    exchange_headers["exchangeable-token"] = "xch-access"
    exchange_headers["non-ccs-token"] = "non-ccs"
    gate = lambda code: response(400, {"code": code, "message": "rejected"})  # noqa: E731
    return {
        "about": f"CCI auth-code and CCS token exchanges (fingerprint-gated), synthetic, shapes from {UPSTREAM}.",
        "inputs": {"device_id": DEVICE_ID, "timestamp_ms": TIMESTAMP_MS, "request_nonce": REQUEST_NONCE, "locale": LOCALE},
        "token": {
            "code": "CODE 1/+",
            "request": request("POST", f"{C['cci_host']}/auth/v1/token?" + urlencode({"code": "CODE 1/+"}), fp_headers()),
            "responses": [
                {"name": "tokens", "response": response(200, tokens), "expect": {"ok": parsed}},
                {"name": "minimal", "response": response(200, {"accessToken": "a"}), "expect": {"ok": {**{k: "" for k in parsed}, "access_token": "a", "expires_in_s": 3599}}},
                {"name": "no access token", "response": response(200, {"refreshToken": "r"}), "expect": {"err": "parse-cci"}},
                {"name": "missing fingerprint", "response": gate(9035), "expect": {"err": "gate9035"}},
                {"name": "bad signature", "response": gate(9036), "expect": {"err": "gate9036"}},
                {"name": "stale timestamp", "response": gate(9037), "expect": {"err": "gate9037"}},
                {"name": "business error", "response": response(400, {"code": 9013, "message": "x"}), "expect": {"err": "http400"}},
                {"name": "server error", "response": response(502, "bad gateway"), "expect": {"err": "http502"}},
            ],
        },
        "ccs_exchange": {
            "tokens": parsed,
            "request": request("POST", f"{C['cci_host']}/auth/v1/token-exchange?serviceType=CCS", exchange_headers),
            "request_without_exchangeable": request(
                "POST",
                f"{C['cci_host']}/auth/v1/token-exchange?serviceType=CCS",
                {k: v for k, v in exchange_headers.items() if k not in ("exchangeable-token", "non-ccs-token")},
            ),
            "responses": [
                {"name": "ccs token", "response": response(200, {"accessToken": "ccs-token", "expiresTime": 3600}), "expect": {"ok": {"access_token": "ccs-token", "expires_in_s": 3600}}},
                {"name": "alternate key, no ttl", "response": response(200, {"ccsAccessToken": "ccs-2"}), "expect": {"ok": {"access_token": "ccs-2", "expires_in_s": 3600}}},
                {"name": "no token", "response": response(200, {"expiresTime": 3600}), "expect": {"err": "parse-ccs"}},
                {"name": "bad signature", "response": gate(9036), "expect": {"err": "gate9036"}},
            ],
        },
    }


def spa():
    ccs = "ccs-token"
    control = "ctl-token"
    register_body = dumps({"pushRegId": "f" * 64, "pushType": C["push_type"], "uuid": "99999999-8888-4777-8666-555555555555"})
    vehicle = {
        "vin": "TESTVIN0000000001",
        "vehicleId": "veh-0001",
        "vehicleName": "IONIQ 6",
        "type": "EV",
        "nickname": "Test car",
        "regDate": "2023-01-01 00:00:00.000",
        "master": True,
        "carShare": 1,
        "protocolType": 0,
        "ccuCCS2ProtocolSupport": 1,
    }
    legacy_vehicle = {k: v for k, v in vehicle.items() if k != "ccuCCS2ProtocolSupport"} | {"vehicleId": "veh-0002", "nickname": "Old car"}
    poi = {
        "phone": "",
        "waypointID": 1,
        "lang": 1,
        "src": "HERE",
        "coord": {"lat": 45.764, "alt": 0, "lon": 4.8357, "type": 0},
        "addr": "1 Example street",
        "zip": "00000",
        "placeid": "",
        "name": "Test destination",
    }
    send_headers = spa_headers(control, 1) | {"AuthorizationCCSP": f"Bearer {control}", "Content-Type": "application/json"}
    fail = lambda code, msg: response(200, {"retCode": "F", "resCode": code, "resMsg": msg, "msgId": "m"})  # noqa: E731
    return {
        "about": f"Legacy SPA vehicle API (ccapi:8080), synthetic, shapes from {UPSTREAM}.",
        "inputs": {"device_id": DEVICE_ID, "now_s": NOW_S, "access_token": ccs, "control_token": control},
        "register": {
            "inputs": {"push_reg_id": "f" * 64, "uuid": "99999999-8888-4777-8666-555555555555"},
            "request": request(
                "POST",
                f"{C['spa_host']}/api/v1/spa/notifications/register",
                {
                    "ccsp-service-id": C["ccsp_service_id"],
                    "ccsp-application-id": C["app_id"],
                    "Stamp": stamp(NOW_S),
                    "Content-Type": "application/json;charset=UTF-8",
                    "User-Agent": C["spa_user_agent"],
                },
                register_body,
            ),
            "responses": [
                {"name": "device id", "response": response(200, ok_envelope({"headerTimestamp": 1, "deviceId": DEVICE_ID})), "expect": {"ok": DEVICE_ID}},
                {"name": "rejected", "response": fail("4001", "bad stamp"), "expect": {"err": "4001"}},
                {"name": "not json", "response": response(500, "<html>"), "expect": {"err": "http500"}},
            ],
        },
        "vehicles": {
            "request": request("GET", f"{C['spa_host']}/api/v1/spa/vehicles", spa_headers(ccs, 0)),
            "responses": [
                {"name": "two vehicles", "response": response(200, ok_envelope({"vehicles": [vehicle, legacy_vehicle]})), "expect": {"ok": [
                    {"id": "veh-0001", "nickname": "Test car", "model": "IONIQ 6", "ccs2": 1},
                    {"id": "veh-0002", "nickname": "Old car", "model": "IONIQ 6", "ccs2": 0},
                ]}},
                {"name": "expired token", "response": response(401, {"error": "Key not authorized: Token is expired"}), "expect": {"err": "token"}},
                {"name": "auth error code", "response": fail("7501", "auth"), "expect": {"err": "7501"}},
                {"name": "bad device id", "response": fail("4002", "Invalid request body - invalid deviceId"), "expect": {"err": "4002"}},
                {"name": "unknown shape", "response": response(200, {"hello": 1}), "expect": {"err": "parse-spa"}},
            ],
        },
        "status": {
            "ccs2_request": request("GET", f"{C['spa_host']}/api/v1/spa/vehicles/veh-0001/ccs2/carstatus/latest", spa_headers(ccs, 1)),
            "legacy_request": request("GET", f"{C['spa_host']}/api/v1/spa/vehicles/veh-0002/status/latest", spa_headers(ccs, 0)),
            "responses": [
                {"name": "ccs2 soc", "ccs2": 1, "response": response(200, ok_envelope({"state": {"Vehicle": {"Green": {"BatteryManagement": {"BatteryRemain": {"Value": 220356, "Ratio": 80}}}}}})), "expect": {"ok": 80}},
                {"name": "ccs2 fractional soc", "ccs2": 1, "response": response(200, ok_envelope({"state": {"Vehicle": {"Green": {"BatteryManagement": {"BatteryRemain": {"Ratio": 79.6}}}}}})), "expect": {"ok": 79}},
                {"name": "legacy soc", "ccs2": 0, "response": response(200, ok_envelope({"vehicleStatusInfo": {"vehicleStatus": {"evStatus": {"batteryStatus": 64}}}})), "expect": {"ok": 64}},
                {"name": "sentinel", "ccs2": 1, "response": response(200, ok_envelope({"state": {"Vehicle": {"Green": {"BatteryManagement": {"BatteryRemain": {"Ratio": 255}}}}}})), "expect": {"err": "parse-soc"}},
                {"name": "missing", "ccs2": 1, "response": response(200, ok_envelope({"state": {"Vehicle": {}}})), "expect": {"err": "parse-soc"}},
                {"name": "no data", "ccs2": 1, "response": fail("5921", "No Data Found v2"), "expect": {"err": "5921"}},
            ],
        },
        "pin": {
            "pin": "0000",
            "request": request(
                "PUT",
                f"{C['spa_host']}/api/v1/user/pin?token=",
                {"Authorization": f"Bearer {ccs}", "Content-type": "application/json", "User-Agent": C["spa_user_agent"]},
                dumps({"deviceId": DEVICE_ID, "pin": "0000"}),
            ),
            "responses": [
                {"name": "control token", "response": response(200, {"controlToken": "ctl-token", "expiresTime": 600}), "expect": {"ok": {"access_token": "ctl-token", "expires_in_s": 600}}},
                {"name": "wrong pin", "response": response(400, {"errId": "x", "errCode": "4003", "errMsg": "pin"}), "expect": {"err": "pin"}},
            ],
        },
        "send_destination": {
            "poi": {"lat": 45.764, "lon": 4.8357, "name": "Test destination", "addr": "1 Example street", "zip": "00000"},
            "request": request(
                "POST",
                f"{C['spa_host']}/api/v2/spa/vehicles/veh-0001/location/routes",
                send_headers,
                dumps({"deviceID": DEVICE_ID, "poiInfoList": [poi]}),
            ),
            "responses": [
                {"name": "accepted", "response": response(200, ok_envelope({}, "msg-0042")), "expect": {"ok": "msg-0042"}},
                {"name": "bad device id", "response": fail("4002", "Invalid request body - invalid deviceId"), "expect": {"err": "4002"}},
                {"name": "rate limited", "response": fail("5091", "Exceeds number of requests"), "expect": {"err": "5091"}},
            ],
        },
    }


def main():
    files = {
        "constants_synthetic.json": {"about": "Synthetic app constants for tests only; real values are never in this file.", **CONSTANTS},
        "stamp_vectors.json": stamp_vectors(),
        "idp.json": idp(),
        "cci.json": cci(),
        "spa.json": spa(),
    }
    for name, content in files.items():
        (OUT / name).write_text(json.dumps(content, indent=2, ensure_ascii=False) + "\n")
        print(f"wrote fixtures/bluelink_eu/{name}")


if __name__ == "__main__":
    main()

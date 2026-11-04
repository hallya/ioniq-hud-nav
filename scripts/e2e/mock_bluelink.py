#!/usr/bin/env python3
"""Mock Bluelink EU server for end-to-end tests. Python 3.9+ stdlib only.

Serves the whole chain the app runs (device id, IDP login, CCI/CCS tokens,
vehicles, charge, PIN, send destination) on 127.0.0.1, with the synthetic
responses of fixtures/bluelink_eu/. It also checks what a real server would
refuse and the protocol tests cannot see from the core alone:

- the IDP cookie set by the authorize page comes back on sign-in;
- the sign-in redirect is NOT followed (a followed one lands on /redirect);
- the password decrypts with the server's RSA key (PKCS#1 v1.5) to the
  test password: a fresh 2048-bit key is made at each start;
- the PIN and the destination arrive as sent.

Writes the app constants for this server (hosts on 127.0.0.1, synthetic ids)
to --constants-out, then serves until killed. GET /__checks returns
{"calls": [...], "failures": [...]}; POST /__reset clears them.

    python3 -I scripts/e2e/mock_bluelink.py --port 8765 --constants-out build/e2e/constants.json
"""

import argparse
import json
import math
import re
import secrets
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "fixtures" / "bluelink_eu"

# The test account every e2e harness uses. Synthetic, never a real account.
EMAIL = "e2e@example.test"
PASSWORD = "e2e-Password-1"
PIN = "1234"
KID = "e2e-kid"
CODE = "E2E-CODE"
DESTINATION = "Tour Eiffel"


def fixture_response(file, step, case):
    for c in json.loads((FIXTURES / file).read_text())[step]["responses"]:
        if c["name"] == case:
            return c["response"]["body"]
    raise KeyError(f"{file} {step} {case}")


# --- RSA, enough to decrypt what the app encrypts --------------------------

def _probable_prime(bits):
    rng = secrets.SystemRandom()
    small = [p for p in range(3, 2000, 2) if all(p % q for q in range(3, int(p**0.5) + 1, 2))]
    while True:
        n = rng.getrandbits(bits) | (1 << (bits - 1)) | (1 << (bits - 2)) | 1
        if any(n % p == 0 for p in small):
            continue
        d, r = n - 1, 0
        while d % 2 == 0:
            d //= 2
            r += 1
        for _ in range(40):
            x = pow(rng.randrange(2, n - 2), d, n)
            if x in (1, n - 1):
                continue
            for _ in range(r - 1):
                x = pow(x, 2, n)
                if x == n - 1:
                    break
            else:
                break
        else:
            return n


def rsa_key(bits=2048):
    e = 65537
    while True:
        p, q = _probable_prime(bits // 2), _probable_prime(bits // 2)
        phi = (p - 1) * (q - 1)
        if p != q and math.gcd(e, phi) == 1 and (p * q).bit_length() == bits:
            return p * q, e, pow(e, -1, phi)


def b64url(n):
    import base64
    raw = n.to_bytes((n.bit_length() + 7) // 8, "big")
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode()


def pkcs1_decrypt(cipher_hex, n, d):
    k = (n.bit_length() + 7) // 8
    try:
        c = int(cipher_hex, 16)
    except ValueError:
        return None
    if len(cipher_hex) != 2 * k or c >= n:
        return None
    m = pow(c, d, n).to_bytes(k, "big")
    if m[0:2] != b"\x00\x02":
        return None
    sep = m.find(b"\x00", 2)
    if sep < 10:
        return None
    return m[sep + 1:].decode("utf-8", "replace")


# --- server ----------------------------------------------------------------

class State:
    def __init__(self, base, key):
        self.base = base
        self.n, self.e, self.d = key
        self.lock = threading.Lock()
        self.reset()

    def reset(self):
        self.calls = []
        self.failures = []
        self.cookie = secrets.token_hex(8)
        self.device_id = None

    def fail(self, what):
        self.failures.append(what)


UUID4 = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
HEX = re.compile(r"^[0-9a-f]+$")


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    state: State = None

    def log_message(self, fmt, *args):
        sys.stderr.write("mock: " + fmt % args + "\n")

    def do_GET(self):
        self.route("GET")

    def do_POST(self):
        self.route("POST")

    def do_PUT(self):
        self.route("PUT")

    def send(self, status, body="", headers=()):
        data = body.encode()
        self.send_response(status)
        for k, v in headers:
            self.send_header(k, v)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def route(self, method):
        s = self.state
        url = urlsplit(self.path)
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length).decode("utf-8", "replace") if length else ""
        query = parse_qs(url.query, keep_blank_values=True)
        path = url.path

        if path == "/__checks":
            with s.lock:
                return self.send(200, json.dumps({"calls": s.calls, "failures": s.failures}))
        if path == "/__reset" and method == "POST":
            with s.lock:
                s.reset()
            return self.send(204)

        with s.lock:
            s.calls.append(f"{method} {path}")
            check = s.fail
            if method == "POST" and path == "/api/v1/spa/notifications/register":
                if not self.headers.get("Stamp"):
                    check("register: no Stamp header")
                try:
                    b = json.loads(body)
                except ValueError:
                    b = {}
                if not (HEX.match(b.get("pushRegId", "")) and len(b["pushRegId"]) == 64):
                    check("register: pushRegId is not 64 hex")
                if not UUID4.match(b.get("uuid", "")):
                    check("register: uuid is not a v4 UUID")
                s.device_id = "11111111-2222-4333-8444-555555555555"
                return self.send(200, fixture_response("spa.json", "register", "device id"))
            if method == "GET" and path == "/auth/api/v2/user/oauth2/authorize":
                return self.send(200, "<html>sign in</html>", [
                    ("Content-Type", "text/html"),
                    ("Set-Cookie", f"idp_session={s.cookie}; Path=/; HttpOnly"),
                ])
            if method == "GET" and path == "/auth/api/v1/accounts/certs":
                jwk = {"kty": "RSA", "e": b64url(s.e), "kid": KID, "n": b64url(s.n)}
                return self.send(200, json.dumps({"retCode": "S", "retValue": jwk}))
            if method == "POST" and path == "/auth/account/signin":
                form = parse_qs(body, keep_blank_values=True)
                one = lambda k: (form.get(k) or [""])[0]
                if f"idp_session={s.cookie}" not in (self.headers.get("Cookie") or ""):
                    check("signin: IDP cookie missing (no shared cookie store)")
                if one("username") != EMAIL:
                    check("signin: wrong username")
                if one("kid") != KID or one("encryptedPassword") != "true":
                    check("signin: kid or encryptedPassword flag wrong")
                if pkcs1_decrypt(one("password"), s.n, s.d) != PASSWORD:
                    check("signin: password does not decrypt to the test password")
                return self.send(302, "", [("Location", f"{s.base}/redirect?code={CODE}&state=ccsp")])
            if path == "/redirect":
                check("signin: the 302 was followed")
                return self.send(200, "<html>followed</html>")
            if method == "POST" and path == "/auth/v1/token":
                if query.get("code") != [CODE]:
                    check("token: wrong auth code")
                if not HEX.match(self.headers.get("X-Fingerprint", "")):
                    check("token: no X-Fingerprint")
                return self.send(200, fixture_response("cci.json", "token", "tokens"))
            if method == "POST" and path == "/auth/v1/token-exchange":
                if self.headers.get("authorization") != "Bearer cci-access":
                    check("ccs: wrong bearer")
                return self.send(200, fixture_response("cci.json", "ccs_exchange", "ccs token"))
            if method == "GET" and path == "/api/v1/spa/vehicles":
                if self.headers.get("Authorization") != "Bearer ccs-token":
                    check("vehicles: wrong bearer")
                return self.send(200, fixture_response("spa.json", "vehicles", "two vehicles"))
            if method == "GET" and path == "/api/v1/spa/vehicles/veh-0001/ccs2/carstatus/latest":
                return self.send(200, fixture_response("spa.json", "status", "ccs2 soc"))
            if method == "PUT" and path == "/api/v1/user/pin":
                try:
                    b = json.loads(body)
                except ValueError:
                    b = {}
                if b.get("pin") != PIN:
                    check("pin: wrong PIN")
                if b.get("deviceId") != s.device_id:
                    check("pin: wrong device id")
                return self.send(200, fixture_response("spa.json", "pin", "control token"))
            if method == "POST" and path == "/api/v2/spa/vehicles/veh-0001/location/routes":
                try:
                    pois = json.loads(body).get("poiInfoList", [])
                except ValueError:
                    pois = []
                if [p.get("name") for p in pois] != [DESTINATION]:
                    check("send: destination is not the test destination")
                if self.headers.get("AuthorizationCCSP") != "Bearer ctl-token":
                    check("send: wrong control token")
                return self.send(200, fixture_response("spa.json", "send_destination", "accepted"))
            check(f"unexpected call: {method} {path}")
            return self.send(404, "not found")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--port", type=int, default=8765)
    ap.add_argument("--constants-out", required=True, type=Path)
    args = ap.parse_args()

    base = f"http://127.0.0.1:{args.port}"
    constants = json.loads((FIXTURES / "constants_synthetic.json").read_text())
    for host in ("idp_host", "cci_host", "spa_host"):
        constants[host] = base
    constants["oneapp_redirect_uri"] = f"{base}/redirect"
    args.constants_out.parent.mkdir(parents=True, exist_ok=True)
    args.constants_out.write_text(json.dumps(constants, indent=2))

    Handler.state = State(base, rsa_key())
    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    print(f"mock: serving on {base}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()

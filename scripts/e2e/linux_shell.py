#!/usr/bin/env python3
"""End-to-end run of the Rust core against the mock Bluelink server, on Linux.

Plays the Swift shell's part (ios/App/BluelinkClient.swift) through the
UniFFI Python bindings: sends each request as given, one cookie store per
run, follows redirects only when asked, encrypts the password with RSA
PKCS#1 v1.5 under the key the core hands over. Python 3.9+ stdlib only.

Runs four scenarios and exits non-zero on any mismatch:
  check        connection check: auth, vehicle and charge ok, nothing sent
  send         test destination: all ok
  follow-all   self-test: following the sign-in 302 must fail as http200
  no-cookies   self-test: dropping cookies must be caught by the mock

    python3 scripts/e2e/linux_shell.py --bindings build/e2e/py \
        --constants build/e2e/constants.json --mock http://127.0.0.1:8765
"""

import argparse
import http.client
import json
import os
import secrets
import sys
import time
import urllib.parse
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from mock_bluelink import EMAIL, PASSWORD, PIN  # noqa: E402

REDIRECTS = {301, 302, 303, 307, 308}


def der_integers(der):
    """The INTEGERs of a DER SEQUENCE, as the core encodes an RSA key."""
    def length(i):
        n = der[i]
        if n < 0x80:
            return n, i + 1
        k = n & 0x7F
        return int.from_bytes(der[i + 1:i + 1 + k], "big"), i + 1 + k

    assert der[0] == 0x30, "not a SEQUENCE"
    _, i = length(1)
    out = []
    while i < len(der):
        assert der[i] == 0x02, "not an INTEGER"
        n, i = length(i + 1)
        out.append(int.from_bytes(der[i:i + n], "big"))
        i += n
    return out


def rsa_pkcs1_encrypt(message, der):
    n, e = der_integers(der)
    k = (n.bit_length() + 7) // 8
    m = message.encode()
    pad = bytes(secrets.randbelow(255) + 1 for _ in range(k - 3 - len(m)))
    em = b"\x00\x02" + pad + b"\x00" + m
    return pow(int.from_bytes(em, "big"), e, n).to_bytes(k, "big")


class Shell:
    def __init__(self, ffi, follow_all=False, cookies=True):
        self.ffi = ffi
        self.follow_all = follow_all
        self.use_cookies = cookies
        self.jar = {}

    def perform(self, req):
        url, method, body = req.url, req.method, req.body
        for _ in range(5):
            u = urllib.parse.urlsplit(url)
            conn = http.client.HTTPConnection(u.hostname, u.port, timeout=10)
            target = u.path + (f"?{u.query}" if u.query else "")
            conn.putrequest(method, target, skip_accept_encoding=True)
            for h in req.headers:
                conn.putheader(h.name, h.value)
            if self.use_cookies and self.jar:
                conn.putheader("Cookie", "; ".join(f"{k}={v}" for k, v in self.jar.items()))
            data = body.encode() if body is not None else None
            if data is not None:
                conn.putheader("Content-Length", str(len(data)))
            conn.endheaders(data)
            resp = conn.getresponse()
            text = resp.read().decode("utf-8", "replace")
            headers = resp.getheaders()
            for name, value in headers:
                if name.lower() == "set-cookie":
                    k, _, v = value.split(";", 1)[0].partition("=")
                    self.jar[k.strip()] = v.strip()
            conn.close()
            follow = req.follow_redirects or self.follow_all
            location = resp.getheader("Location")
            if follow and resp.status in REDIRECTS and location:
                url, method, body = urllib.parse.urljoin(url, location), "GET", None
                continue
            return self.ffi.HttpResponse(
                status=resp.status,
                headers=[self.ffi.Header(name=k, value=v) for k, v in headers],
                url=url,
                body=text,
            )
        return None

    def run(self, constants, destination):
        ffi = self.ffi
        run = ffi.BluelinkRun(
            constants, None, EMAIL, len(PASSWORD), PIN, "fr", "+01:00",
            os.urandom(ffi.bluelink_entropy_len()), destination,
        )
        now = lambda: int(time.time() * 1000)  # noqa: E731
        step = run.start(now())
        while True:
            if step.is_send():
                try:
                    response = self.perform(step.request)
                except OSError:
                    response = None
                given = ffi.RunInput.RECEIVED(response) if response else ffi.RunInput.FAILED(0)
                step = run.resume(given, now())
            elif step.is_encrypt_password():
                ciphertext = rsa_pkcs1_encrypt(PASSWORD, step.public_key_der)
                step = run.resume(ffi.RunInput.ENCRYPTED(ciphertext), now())
            else:
                return step


def mock_call(base, path, method="GET"):
    u = urllib.parse.urlsplit(base)
    conn = http.client.HTTPConnection(u.hostname, u.port, timeout=5)
    conn.request(method, path)
    resp = conn.getresponse()
    text = resp.read().decode()
    conn.close()
    return json.loads(text) if text else None


CHAIN = [
    "POST /api/v1/spa/notifications/register",
    "GET /auth/api/v2/user/oauth2/authorize",
    "GET /auth/api/v1/accounts/certs",
    "POST /auth/account/signin",
    "POST /auth/v1/token",
    "POST /auth/v1/token-exchange",
    "GET /api/v1/spa/vehicles",
    "GET /api/v1/spa/vehicles/veh-0001/ccs2/carstatus/latest",
]
SEND = ["PUT /api/v1/user/pin", "POST /api/v2/spa/vehicles/veh-0001/location/routes"]


def scenario(name, shell, constants, mock, destination, want_line, want_calls, want_failure=None):
    mock_call(mock, "/__reset", "POST")
    done = shell.run(constants, destination)
    checks = mock_call(mock, "/__checks")
    problems = []
    if want_line not in done.line:
        problems.append(f"line {done.line!r} lacks {want_line!r}")
    if want_calls is not None and checks["calls"] != want_calls:
        problems.append(f"calls {checks['calls']}")
    if want_failure is None and checks["failures"]:
        problems.append(f"mock failures {checks['failures']}")
    if want_failure is not None and not any(want_failure in f for f in checks["failures"]):
        problems.append(f"mock did not report {want_failure!r}: {checks['failures']}")
    print(f"e2e {name:<11} {'ok' if not problems else 'FAILED'}  {done.line}")
    for p in problems:
        print(f"    {p}")
    return not problems


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--bindings", required=True, type=Path)
    ap.add_argument("--constants", required=True, type=Path)
    ap.add_argument("--mock", default="http://127.0.0.1:8765")
    args = ap.parse_args()

    sys.path.insert(0, str(args.bindings))
    import hudnav_ffi as ffi  # noqa: E402

    constants = args.constants.read_text()
    eiffel = [ffi.test_destination()]
    results = [
        scenario("check", Shell(ffi), constants, args.mock, [],
                 "auth ok · veh ok · soc 80 · send -", CHAIN),
        scenario("send", Shell(ffi), constants, args.mock, eiffel,
                 "auth ok · veh ok · soc 80 · send ok", CHAIN + SEND),
        scenario("follow-all", Shell(ffi, follow_all=True), constants, args.mock, eiffel,
                 "auth ko:http200", None, "the 302 was followed"),
        scenario("no-cookies", Shell(ffi, cookies=False), constants, args.mock, eiffel,
                 "send ok", None, "IDP cookie missing"),
    ]
    sys.exit(0 if all(results) else 1)


if __name__ == "__main__":
    main()

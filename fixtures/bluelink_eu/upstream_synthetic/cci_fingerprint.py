"""Synthetic, reduced copy of upstream hyundai_kia_connect_api/cci_fingerprint.py
(MIT) at b952510, for tests only. Same structure as upstream; every value is
fake. Real values never go in the repo.
"""

from __future__ import annotations

import hashlib
import hmac
import re

from .const import BRAND_GENESIS, BRAND_HYUNDAI, BRAND_KIA, BRANDS

# Signing constants per brand: (secret, piece, suffix), concatenated into
# the HMAC key in this order. The pieces rotate with app releases.
_FINGERPRINT_KEYS: dict[str, tuple[str, str, str]] = {
    BRAND_HYUNDAI: ("TESTsecret01", "TESTpiece02", "TESTsufx03"),
    BRAND_KIA: ("KIAsecret001", "KIApiece002", "KIAsufx003"),
    # Genesis note kept from upstream's shape: a comment between entries.
    BRAND_GENESIS: ("GENsecret001", "GENpiece002", "GENsufx003"),
}

# Android OneApp build identity the gate validates the fingerprint against.
CLIENT_VERSION = "9.9.9"
CLIENT_OS_CODE = "android"
CLIENT_OS_VERSION = "14"
CLIENT_DEVICE_MODEL = "TEST-MODEL"
CLIENT_NOTIFICATION_PROVIDER = "FCM"
USER_AGENT = "okhttp/4.12.0"
APP_REGION = "eu"

_IDENTITY_HEADERS = ("client-id", "client-name", "client-version", "client-os-code")


def compute_x_fingerprint(headers: dict[str, str], brand: int, body: str = "") -> str:
    lower = {k.lower(): v for k, v in headers.items()}
    identity = "".join(lower.get(name, "") for name in _IDENTITY_HEADERS)
    pieces = _FINGERPRINT_KEYS[BRANDS[brand]]
    msg = (
        identity
        + lower.get("client-os-version", "")
        + lower.get("x-timestamp", "")
        + re.sub(r"\s+", "", body, flags=re.ASCII)
    )
    key = identity + "".join(pieces)
    return hmac.new(key.encode(), msg.encode(), hashlib.sha256).hexdigest()


def client_name_for_brand(brand: int) -> str:
    """'ExampleEU' / 'OtherEU' / 'ThirdEU' per the OneApp client-name scheme."""
    return f"{BRANDS[brand]}EU"

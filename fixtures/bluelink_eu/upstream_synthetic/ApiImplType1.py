# Synthetic, reduced copy of upstream hyundai_kia_connect_api/ApiImplType1.py
# (MIT) at b952510, for tests only. Every value is fake.

import logging

_LOGGER = logging.getLogger(__name__)

USER_AGENT_OK_HTTP: str = "okhttp/3.12.0"


class ApiImplType1:
    def _get_authenticated_headers(self, token, ccs2_support=None) -> dict:
        return {"User-Agent": USER_AGENT_OK_HTTP}

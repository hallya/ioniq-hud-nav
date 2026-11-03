# Synthetic, reduced copy of upstream hyundai_kia_connect_api/KiaUvoApiEU.py
# (MIT) at b952510, for tests only. Same structure as upstream where our
# parsers read it (module constants, the brand branches of __init__, the
# OneApp login); every value is fake.

import base64
import logging

from .ApiImplType1 import ApiImplType1
from .const import BRAND_GENESIS, BRAND_HYUNDAI, BRAND_KIA, BRANDS

_LOGGER = logging.getLogger(__name__)

USER_AGENT_MOZILLA: str = (
    "Mozilla/5.0 (Linux; Android 10; K) "
    "ExampleBrowser/1.0"
)

SUPPORTED_LANGUAGES_LIST = ["en", "fr"]


class KiaUvoApiEU(ApiImplType1):
    data_timezone = None

    def __init__(self, region: int, brand: int, language: str) -> None:
        language = language.lower()
        if language not in SUPPORTED_LANGUAGES_LIST:
            language = "en"
        self.LANGUAGE: str = language
        self.brand: int = brand

        if BRANDS[self.brand] == BRAND_KIA:
            self.BASE_DOMAIN: str = "spa.other.test"
            self.PORT: int = 8080
            self.CCSP_SERVICE_ID: str = "00000000-0000-4000-8000-0000000001b2"
            self.APP_ID: str = "00000000-0000-4000-8000-0000000001c3"
            self.CFB = base64.b64decode("AAAA")
            self.LOGIN_FORM_HOST = "https://idp.other.test"
            self.PUSH_TYPE = "APNS"
        elif BRANDS[self.brand] == BRAND_HYUNDAI:
            self.BASE_DOMAIN = "spa.example.test"
            self.PORT = 8080
            self.CCSP_SERVICE_ID = "00000000-0000-4000-8000-0000000000b2"
            self.APP_ID = "00000000-0000-4000-8000-0000000000c3"
            self.CFB = base64.b64decode(
                "CzBVep/E6Q4zWH2ix+wRNluApcrvFDleg6jN8hc8YYar0PUaP2SJrtP4HUJnjLHW+yA="
            )
            self.LOGIN_FORM_HOST = "https://idp.example.test"
            self.PUSH_TYPE = "GCM"
        elif BRANDS[self.brand] == BRAND_GENESIS:
            self.BASE_DOMAIN = "spa.third.test"
            self.PORT = 443
            self.CCSP_SERVICE_ID = "00000000-0000-4000-8000-0000000002b2"
            self.APP_ID = "00000000-0000-4000-8000-0000000002c3"
            self.CFB = base64.b64decode("AAAA")
            self.LOGIN_FORM_HOST = "https://idp.third.test"
            self.PUSH_TYPE = "GCM"

        self.BASE_URL: str = self.BASE_DOMAIN + ":" + str(self.PORT)
        self.USER_API_URL: str = "https://" + self.BASE_URL + "/api/v1/user/"

        self.CLIENT_ID: str = self.CCSP_SERVICE_ID

        if BRANDS[self.brand] == BRAND_HYUNDAI:
            self.ONEAPP_CLIENT_ID: str = "00000000-0000-4000-8000-0000000000a1"
            self.ONEAPP_REDIRECT_URI: str = "https://oneapp.example.test/redirect"
            self.CCI_API_URL: str = "https://cci.example.test"
            self.CCI_PACKAGE_ID: str = "com.example.oneapp.eu"
            self._cci_client_name: str = "example"
        elif BRANDS[self.brand] == BRAND_KIA:
            self.ONEAPP_CLIENT_ID = "00000000-0000-4000-8000-0000000001a1"
            self.ONEAPP_REDIRECT_URI = "https://oneapp.other.test/redirect"
            self.CCI_API_URL = "https://cci.other.test"
            self.CCI_PACKAGE_ID = "com.other.oneapp.eu"
            self._cci_client_name = "other"
        else:  # BRAND_GENESIS
            self.ONEAPP_CLIENT_ID = "00000000-0000-4000-8000-0000000002a1"
            self.ONEAPP_REDIRECT_URI = "https://oneapp.third.test/redirect"
            self.CCI_API_URL = "https://cci.third.test"
            self.CCI_PACKAGE_ID = "com.third.oneapp.eu"
            self._cci_client_name = "third"

    def _login_with_password_cci(self, username: str, password: str) -> str:
        client_id = self.ONEAPP_CLIENT_ID
        redirect_uri = self.ONEAPP_REDIRECT_URI
        mobile_ua = USER_AGENT_MOZILLA + "_CCS_APP_AOS"
        return client_id + redirect_uri + mobile_ua

#!/usr/bin/env python3
"""Checks for scripts/app-constants.py, on the synthetic upstream copies.

    python3 -I scripts/test_app_constants.py

Run by scripts/core-check.sh (CI too). A shape change upstream must stop the
build with the name of what moved, never write a silent wrong file.
"""

import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "scripts" / "app-constants.py"
SYNTHETIC = ROOT / "fixtures" / "bluelink_eu" / "upstream_synthetic"
EXPECTED = ROOT / "fixtures" / "bluelink_eu" / "constants_synthetic.json"


def run(upstream, *args):
    return subprocess.run(
        [sys.executable, "-I", str(SCRIPT), str(upstream), *args],
        capture_output=True,
        text=True,
    )


class AppConstants(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.up = self.tmp / "upstream"
        shutil.copytree(SYNTHETIC, self.up)

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def edit(self, name, old, new):
        path = self.up / name
        text = path.read_text()
        self.assertIn(old, text)
        path.write_text(text.replace(old, new, 1))

    def test_synthetic_copies_give_the_synthetic_constants(self):
        r = run(self.up, "--check", str(EXPECTED))
        self.assertEqual(r.returncode, 0, r.stderr)

    def test_writes_the_file_with_the_upstream_ref(self):
        out = self.tmp / "out" / "c.json"
        r = run(self.up, "--upstream-ref", "repo@abc123", "--out", str(out))
        self.assertEqual(r.returncode, 0, r.stderr)
        written = json.loads(out.read_text())
        self.assertEqual(written.pop("upstream"), "repo@abc123")
        expected = json.loads(EXPECTED.read_text())
        expected.pop("about")
        self.assertEqual(written, expected)

    def test_hyundai_branch_is_used_not_the_first_one(self):
        self.edit("KiaUvoApiEU.py", '"https://cci.example.test"', '"https://cci.renamed.test"')
        r = run(self.up, "--check", str(EXPECTED))
        self.assertEqual(r.returncode, 1)
        self.assertIn("cci_host", r.stderr)
        self.assertNotIn("renamed", r.stderr, "names fields, not values")

    def test_missing_hyundai_key_fails_loudly(self):
        self.edit("cci_fingerprint.py", "    BRAND_HYUNDAI: (", "    BRAND_OTHER: (")
        r = run(self.up, "--out", str(self.tmp / "c.json"))
        self.assertEqual(r.returncode, 1)
        self.assertIn("_FINGERPRINT_KEYS[BRAND_HYUNDAI]", r.stderr)
        self.assertFalse((self.tmp / "c.json").exists(), "no file on failure")

    def test_a_new_brand_entry_does_not_hide_the_hyundai_one(self):
        self.edit(
            "cci_fingerprint.py",
            "    BRAND_KIA: (",
            "    BRAND_NEW: _load_pieces(),\n    **_EXTRA,\n    BRAND_KIA: (",
        )
        r = run(self.up, "--check", str(EXPECTED))
        self.assertEqual(r.returncode, 0, r.stderr)

    def test_value_that_is_no_longer_a_literal_fails_loudly(self):
        self.edit(
            "KiaUvoApiEU.py",
            'self.APP_ID = "00000000-0000-4000-8000-0000000000c3"',
            "self.APP_ID = load_app_id()",
        )
        r = run(self.up, "--out", str(self.tmp / "c.json"))
        self.assertEqual(r.returncode, 1)
        self.assertIn("self.APP_ID", r.stderr)

    def test_unsafe_header_value_fails_loudly(self):
        self.edit("cci_fingerprint.py", 'CLIENT_VERSION = "9.9.9"', 'CLIENT_VERSION = "9.9.9\\r\\nX: 1"')
        r = run(self.up, "--out", str(self.tmp / "c.json"))
        self.assertEqual(r.returncode, 1)
        self.assertIn("client.version", r.stderr)

    def test_missing_file_fails_loudly(self):
        (self.up / "ApiImplType1.py").unlink()
        r = run(self.up, "--out", str(self.tmp / "c.json"))
        self.assertEqual(r.returncode, 1)
        self.assertIn("ApiImplType1.py", r.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=1)

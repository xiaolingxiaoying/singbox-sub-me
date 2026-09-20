import base64
from pathlib import Path
import runpy
import unittest


render = runpy.run_path(str(Path(__file__).with_name("prepare-installer.py")))["render"]


class InstallerTrustTests(unittest.TestCase):
    def test_missing_malformed_and_public_development_keys_are_rejected(self):
        for key in ["", "invalid", "00" * 31, "247f88e163242986b7107eb704a983e12186d2697a3927b7e6b42ec2b364272b"]:
            with self.subTest(key_length=len(key)), self.assertRaises(ValueError):
                render(key)

    def test_installer_pins_the_supplied_public_key(self):
        # RFC 8032 public test vector; this is never a production key.
        key = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"
        script = render(key)
        self.assertNotIn("@SBCTL_RELEASE_PUBLIC_KEY_PEM@", script)
        encoded = script.split("-----BEGIN PUBLIC KEY-----\n", 1)[1].splitlines()[0]
        self.assertEqual(base64.b64decode(encoded)[-32:], bytes.fromhex(key))
        self.assertNotIn("JH+I4WMkKYa3EH63BKmD4SGG0ml6OSe35rQuwrNkJys=", script)


if __name__ == "__main__":
    unittest.main()

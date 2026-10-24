from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from sdk_version import sdk_version


class SdkVersionTests(unittest.TestCase):
    def setUp(self):
        self.directory = TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        (self.root / "Cargo.toml").write_text(
            '[workspace]\nmembers = ["tezos-core", "tezos-rpc"]\n'
            '[workspace.package]\nversion = "0.3.0"\n'
        )
        for member in ["tezos-core", "tezos-rpc"]:
            (self.root / member).mkdir()
            (self.root / member / "Cargo.toml").write_text(
                f'[package]\nname = "{member}"\nversion.workspace = true\n'
            )

    def test_accepts_shared_version_and_matching_tag(self):
        self.assertEqual(sdk_version(self.root, "v0.3.0"), "0.3.0")

    def test_rejects_mixed_crate_versions(self):
        (self.root / "tezos-rpc" / "Cargo.toml").write_text(
            '[package]\nname = "tezos-rpc"\nversion = "0.2.0"\n'
        )
        with self.assertRaisesRegex(ValueError, "tezos-rpc does not match"):
            sdk_version(self.root)

    def test_rejects_a_mismatched_tag(self):
        with self.assertRaisesRegex(ValueError, "release tag must be v0.3.0"):
            sdk_version(self.root, "v0.2.0")

    def test_rejects_invalid_versions(self):
        manifest = self.root / "Cargo.toml"
        original = manifest.read_text()
        for version in ["03.0.0", "0.3", "0.3.0;bad", "0.3.0/path"]:
            with self.subTest(version=version):
                manifest.write_text(original.replace("0.3.0", version))
                with self.assertRaises(ValueError):
                    sdk_version(self.root)

    def test_rejects_a_missing_crate_manifest(self):
        (self.root / "tezos-rpc" / "Cargo.toml").unlink()
        with self.assertRaises(OSError):
            sdk_version(self.root)

"""Offline tests for scripts/prepare-release.py: python3 -m unittest discover -s scripts"""

import importlib.util
import pathlib
import unittest

spec = importlib.util.spec_from_file_location(
    "prepare_release", pathlib.Path(__file__).with_name("prepare-release.py")
)
rel = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rel)

CARGO = '''[package]
name = "checkdom-mcp"
version = "0.1.0"
edition = "2024"

[dependencies]
serde_json = "1.0"
version_check = { version = "0.9" }
'''

LOCK = '''[[package]]
name = "anyhow"
version = "0.1.0"

[[package]]
name = "checkdom-mcp"
version = "0.1.0"
dependencies = [
 "anyhow",
]
'''

CHANGELOG = """# Changelog

Intro.

## [Unreleased]

### Added
- A new tool.

### Fixed
- A bug.

## 0.1.0 (2026-10-31)

### Added
- Everything.
"""


class Bump(unittest.TestCase):
    def test_parts(self):
        self.assertEqual(rel.bump("0.1.0", "patch"), "0.1.1")
        self.assertEqual(rel.bump("0.1.9", "minor"), "0.2.0")
        self.assertEqual(rel.bump("1.2.3", "major"), "2.0.0")

    def test_rejects_bad_input(self):
        for version, part in [("0.1", "patch"), ("0.1.0-rc1", "patch"), ("0.1.0", "huge")]:
            with self.assertRaises(rel.ReleaseError):
                rel.bump(version, part)


class Files(unittest.TestCase):
    def test_cargo_toml_only_the_package_version_changes(self):
        self.assertEqual(rel.package_version(CARGO), "0.1.0")
        out = rel.set_package_version(CARGO, "0.1.0", "0.2.0")
        self.assertIn('name = "checkdom-mcp"\nversion = "0.2.0"', out)
        self.assertIn('version_check = { version = "0.9" }', out)
        self.assertEqual(out.count("0.2.0"), 1)

    def test_lock_only_this_package_changes(self):
        out = rel.set_lock_version(LOCK, "0.1.0", "0.2.0")
        self.assertIn('name = "checkdom-mcp"\nversion = "0.2.0"', out)
        self.assertIn('name = "anyhow"\nversion = "0.1.0"', out)
        with self.assertRaises(rel.ReleaseError):
            rel.set_lock_version(LOCK, "9.9.9", "10.0.0")


class Changelog(unittest.TestCase):
    def test_unreleased_becomes_the_version(self):
        out, notes = rel.cut_changelog(CHANGELOG, "0.2.0", "2026-11-05")
        self.assertIn("## [Unreleased]\n\n## 0.2.0 (2026-11-05)\n\n### Added\n- A new tool.", out)
        self.assertIn("## 0.1.0 (2026-10-31)", out)
        self.assertEqual(out.count("- A new tool."), 1)
        self.assertTrue(notes.startswith("### Added\n- A new tool."))
        self.assertIn("- A bug.", notes)
        self.assertNotIn("Everything", notes)

    def test_empty_unreleased_is_refused(self):
        empty = CHANGELOG.split("### Added\n- A new tool.")[0] + "## 0.1.0 (2026-10-31)\n- x\n"
        with self.assertRaises(rel.ReleaseError):
            rel.cut_changelog(empty, "0.2.0", "2026-11-05")

    def test_existing_version_is_refused(self):
        with self.assertRaises(rel.ReleaseError):
            rel.cut_changelog(CHANGELOG, "0.1.0", "2026-11-05")

    def test_real_changelog_shape(self):
        real = (rel.ROOT / "CHANGELOG.md").read_text(encoding="utf-8")
        self.assertIn(rel.UNRELEASED, real)
        self.assertEqual(rel.package_version((rel.ROOT / "Cargo.toml").read_text(encoding="utf-8")).count("."), 2)


if __name__ == "__main__":
    unittest.main()

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


class EdgeCases(unittest.TestCase):
    def test_workspace_package_is_not_the_package(self):
        cargo = '[workspace.package]\nversion = "9.9.9"\n\n[package]\nname = "x"\nversion = "0.1.0"\n'
        out = rel.set_package_version(cargo, "0.1.0", "0.1.1")
        self.assertIn('[workspace.package]\nversion = "9.9.9"', out)
        self.assertIn('[package]\nname = "x"\nversion = "0.1.1"', out)

    def test_star_bullets_count(self):
        log = "## [Unreleased]\n\n* Something.\n\n## 0.1.0 (2026-10-31)\n- x\n"
        _, notes = rel.cut_changelog(log, "0.1.1", "2026-11-01")
        self.assertEqual(notes, "* Something.\n")

    def test_heading_inside_a_code_block_does_not_end_the_section(self):
        log = "## [Unreleased]\n\n- Docs:\n```\n## not a heading\n```\n- More.\n\n## 0.1.0 (2026-10-31)\n- x\n"
        _, notes = rel.cut_changelog(log, "0.1.1", "2026-11-01")
        self.assertIn("## not a heading", notes)
        self.assertIn("- More.", notes)
        self.assertNotIn("0.1.0", notes)

    def test_trailing_link_definitions_stay_in_the_file(self):
        log = "## [Unreleased]\n\n- New.\n\n[Unreleased]: https://example.com/compare\n[0.1.0]: https://example.com/tag\n"
        out, notes = rel.cut_changelog(log, "0.2.0", "2026-11-01")
        self.assertEqual(notes, "- New.\n")
        self.assertTrue(out.rstrip().endswith("[0.1.0]: https://example.com/tag"))
        self.assertIn("## 0.2.0 (2026-11-01)\n\n- New.\n\n[Unreleased]:", out)


class EndToEnd(unittest.TestCase):
    def test_main_on_a_copy_of_the_real_files(self):
        import shutil
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            for name in ["Cargo.toml", "Cargo.lock", "CHANGELOG.md"]:
                shutil.copy(rel.ROOT / name, root / name)
            before = rel.package_version((root / "Cargo.toml").read_text())
            notes = root / "notes.md"
            unreleased_has_entries = True
            try:
                rel.cut_changelog((root / "CHANGELOG.md").read_text(), "99.0.0", "2026-01-01")
            except rel.ReleaseError:
                unreleased_has_entries = False
            code = rel.main(["minor", "--date", "2026-11-05", "--notes", str(notes)], root=root)
            if not unreleased_has_entries:
                self.assertEqual(code, 1)
                return
            self.assertEqual(code, 0)
            after = rel.package_version((root / "Cargo.toml").read_text())
            self.assertEqual(after, rel.bump(before, "minor"))
            self.assertIn(f'name = "checkdom-mcp"\nversion = "{after}"', (root / "Cargo.lock").read_text())
            log = (root / "CHANGELOG.md").read_text()
            self.assertIn(f"## [Unreleased]\n\n## {after} (2026-11-05)\n", log)
            self.assertTrue(notes.read_text().strip())


if __name__ == "__main__":
    unittest.main()

#!/usr/bin/env python3
"""Prepare a release: bump the version and turn the CHANGELOG's [Unreleased] section into it.

Usage: scripts/prepare-release.py patch|minor|major [--date YYYY-MM-DD] [--notes FILE]

Updates Cargo.toml ([package] version), Cargo.lock (this package's entry) and CHANGELOG.md
(a new "## X.Y.Z (date)" section holding what was under "## [Unreleased]", which is left
empty above it), prints the new version, and writes the release notes to FILE if given.
Fails if [Unreleased] has no entries. Dates are UTC (the runner's clock). The Prepare release
workflow runs it (RELEASE.md).
Standard library only.
"""

import datetime
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
PACKAGE = "checkdom-mcp"
UNRELEASED = "## [Unreleased]"


class ReleaseError(Exception):
    pass


def bump(version, part):
    m = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", version)
    if not m:
        raise ReleaseError(f"not a MAJOR.MINOR.PATCH version: {version!r}")
    major, minor, patch = map(int, m.groups())
    if part == "major":
        return f"{major + 1}.0.0"
    if part == "minor":
        return f"{major}.{minor + 1}.0"
    if part == "patch":
        return f"{major}.{minor}.{patch + 1}"
    raise ReleaseError(f"bump must be patch, minor or major, not {part!r}")


def package_version(cargo_toml):
    """The version in Cargo.toml's [package] table."""
    section = re.search(r"^\[package\]\n(.*?)(?=^\[|\Z)", cargo_toml, re.M | re.S)
    m = section and re.search(r'^version\s*=\s*"([^"]+)"', section.group(1), re.M)
    if not m:
        raise ReleaseError("no version in [package] of Cargo.toml")
    return m.group(1)


def set_package_version(cargo_toml, old, new):
    m = re.search(r"^\[package\]\n", cargo_toml, re.M)
    if not m:
        raise ReleaseError("no [package] table in Cargo.toml")
    head, rest = cargo_toml[: m.end()], cargo_toml[m.end() :]
    section_end = re.search(r"^\[", rest, re.M)
    end = section_end.start() if section_end else len(rest)
    section, n = re.subn(
        rf'^version\s*=\s*"{re.escape(old)}"', f'version = "{new}"', rest[:end], count=1, flags=re.M
    )
    if n != 1:
        raise ReleaseError("could not update the version in Cargo.toml")
    return head + section + rest[end:]


def set_lock_version(cargo_lock, old, new):
    pattern = rf'(\[\[package\]\]\nname = "{re.escape(PACKAGE)}"\nversion = )"{re.escape(old)}"'
    out, n = re.subn(pattern, rf'\1"{new}"', cargo_lock)
    if n != 1:
        raise ReleaseError(f"could not find {PACKAGE} {old} in Cargo.lock")
    return out


def next_heading(text):
    """Offset of the first "## " heading outside a fenced code block, or None."""
    fenced = False
    offset = 0
    for line in text.splitlines(keepends=True):
        if line.startswith("```"):
            fenced = not fenced
        elif not fenced and line.startswith("## "):
            return offset
        offset += len(line)
    return None


LINK_DEFINITIONS = re.compile(r"(?:\n\[[^\]\n]+\]:[^\n]*)+\s*\Z")


def cut_changelog(changelog, version, date):
    """Move the [Unreleased] entries under a new version heading. Returns (changelog, notes)."""
    if UNRELEASED not in changelog:
        raise ReleaseError(f"no '{UNRELEASED}' section in CHANGELOG.md")
    if f"\n## {version} " in changelog:
        raise ReleaseError(f"CHANGELOG.md already has a section for {version}")
    head, _, rest = changelog.partition(UNRELEASED)
    at = next_heading(rest)
    body, tail = (rest[:at], rest[at:]) if at is not None else (rest, "")
    # Link definitions at the end of the file belong to the file, not the notes.
    links = LINK_DEFINITIONS.search(body) if at is None else None
    if links:
        body, tail = body[: links.start()], body[links.start() :].lstrip("\n")
    notes = body.strip("\n")
    if not re.search(r"^\s*[-*] \S", notes, re.M):
        raise ReleaseError("the [Unreleased] section of CHANGELOG.md has no entries")
    new = f"{head}{UNRELEASED}\n\n## {version} ({date})\n\n{notes}\n\n{tail}"
    return new.rstrip("\n") + "\n", notes + "\n"


def main(argv, root=ROOT):
    if not argv or argv[0] not in ("patch", "minor", "major"):
        print(__doc__.strip().splitlines()[2], file=sys.stderr)
        return 2
    part = argv[0]
    date = datetime.date.today().isoformat()
    notes_file = None
    rest = argv[1:]
    while rest:
        flag, value, rest = rest[0], rest[1] if len(rest) > 1 else None, rest[2:]
        if flag == "--date" and value:
            date = value
        elif flag == "--notes" and value:
            notes_file = pathlib.Path(value)
        else:
            print(f"unknown argument {flag}", file=sys.stderr)
            return 2
    try:
        cargo = (root / "Cargo.toml").read_text(encoding="utf-8")
        old = package_version(cargo)
        new = bump(old, part)
        lock = (root / "Cargo.lock").read_text(encoding="utf-8")
        changelog, notes = cut_changelog((root / "CHANGELOG.md").read_text(encoding="utf-8"), new, date)
        (root / "Cargo.toml").write_text(set_package_version(cargo, old, new), encoding="utf-8")
        (root / "Cargo.lock").write_text(set_lock_version(lock, old, new), encoding="utf-8")
        (root / "CHANGELOG.md").write_text(changelog, encoding="utf-8")
    except ReleaseError as e:
        print(f"prepare-release: {e}", file=sys.stderr)
        return 1
    if notes_file:
        notes_file.write_text(notes, encoding="utf-8")
    print(new)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Advance an already selected local artifact channel after package.sh's smoke.

Only the selected package and the newly published package are read. Selection remains
atpkg's: its conditional link checks the expected checkout and prefix under the store
lock. An older atpkg that lacks those guards must refuse; there is no legacy fallback.
"""

import argparse
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
try:
    import tomllib
except ModuleNotFoundError:
    tomllib = None


TOML_STRING = re.compile(
    r'"(?:[^"\\\x00-\x1f\x7f]|\\(?:["\\bfnrt]|u[0-9a-fA-F]{4}))*"'
    r"|'[^'\x00-\x08\x0a-\x1f\x7f]*'")


def toml_string(value):
    if TOML_STRING.fullmatch(value) is None:
        raise ValueError("invalid or unsupported TOML string")
    if value.startswith("'"):
        result = value[1:-1]
    else:
        # JSON's string decoder also accepts escapes TOML forbids. The pattern
        # excludes those, and TOML rejects surrogate escapes even as a pair.
        for escape in re.findall(r'\\(?:["\\bfnrt]|u[0-9a-fA-F]{4})', value):
            if escape.startswith("\\u") and 0xD800 <= int(escape[2:], 16) <= 0xDFFF:
                raise ValueError("invalid TOML Unicode scalar")
        result = json.loads(value)
    if any(0xD800 <= ord(char) <= 0xDFFF for char in result):
        raise ValueError("invalid TOML Unicode scalar")
    return result


def parse_link_marker(text):
    """Parse atpkg's marker; older Python accepts only its flat one-line subset.

    The fallback supports the five generated keys, quoted scalar strings and
    string arrays. Unsupported TOML (tables, multiline values, inline comments
    or other value types) fails closed rather than being partly interpreted.
    """
    if tomllib is not None:
        return tomllib.loads(text)
    marker = {}
    for raw in text.replace("\r\n", "\n").split("\n"):
        if any((ord(char) < 0x20 and char != "\t") or char == "\x7f" for char in raw):
            raise ValueError("invalid TOML control character")
        line = raw.strip(" \t")
        if not line or line.startswith("#"):
            continue
        if "=" not in line:
            raise ValueError(f"invalid TOML line: {raw}")
        key, value = (part.strip(" \t") for part in line.split("=", 1))
        if key in marker:
            raise ValueError(f"duplicate TOML key: {key}")
        if key == "schema":
            if not re.fullmatch(r"0|[1-9][0-9]*", value) or int(value) > 2**63 - 1:
                raise ValueError("invalid schema")
            marker[key] = int(value)
        elif key in ("program", "checkout", "linked_at"):
            marker[key] = toml_string(value)
        elif key == "bins":
            if not (value.startswith("[") and value.endswith("]")):
                raise ValueError("invalid bins array")
            remaining = value[1:-1].strip(" \t")
            marker[key] = []
            while remaining:
                item = TOML_STRING.match(remaining)
                if item is None:
                    raise ValueError("invalid or unsupported bins member")
                marker[key].append(toml_string(item[0]))
                remaining = remaining[item.end():].lstrip(" \t")
                if remaining:
                    if not remaining.startswith(","):
                        raise ValueError("invalid bins separator")
                    remaining = remaining[1:].lstrip(" \t")
        else:
            raise ValueError(f"unsupported TOML key: {key}")
    return marker


LIMIT = 128 * 1024
SOURCE = re.compile(r"[0-9a-f]{40}")
# A package's dev version (alab/README.md, Decision 8): MAJOR.MINOR.0-dev.<count>+g<sha9>, or the
# compound MAJOR.MINOR.0-dev.<count>+codex.<X.Y.Z>.g<sha9> naming the upstream Codex baseline in
# the build metadata, which never orders (the count does).
NUMBER = r"(?:0|[1-9][0-9]{0,8})"
VERSION = re.compile(r"([0-9]+)\.([0-9]+)\.0-dev\.([0-9]+)\+(?:codex\." + NUMBER + r"\."
                     + NUMBER + r"\." + NUMBER + r"\.)?g([0-9a-f]{9})")


class Version(tuple):
    """(major, minor, count), ordered by count alone where it matters; `text` is the version
    exactly as the package names it."""

    def __new__(cls, numbers, text):
        made = super().__new__(cls, numbers)
        made.text = text
        return made
# Match atpkg's unattended update lane, with time left for the guarded link after waiting.
WAIT_LOCK_SECONDS = 1800


def regular_bytes(path):
    """Bounded metadata, never through a link or a special file."""
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as stream:
        metadata = os.fstat(stream.fileno())
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > LIMIT:
            raise ValueError(f"not bounded regular metadata: {path}")
        data = stream.read(LIMIT + 1)
        if len(data) > LIMIT:
            raise ValueError(f"oversized metadata: {path}")
        return data


def package(path, root):
    """A complete published Darwin package, with matching records and protected tools."""
    if path.is_symlink() or path.name.startswith(".") or path.resolve(strict=True).parent != root:
        raise ValueError("the package is not a published member of this artifact channel")
    manifest = json.loads(regular_bytes(path / "codex-package.json"))
    build = json.loads(regular_bytes(path / "alab-build.json"))
    if not isinstance(manifest, dict) or not isinstance(build, dict):
        raise ValueError("the ALab package records must be JSON objects")
    expected_layout = {"entrypoint": "bin/codex", "variant": "codex",
                       "resourcesDir": "codex-resources", "pathDir": "codex-path"}
    if any(manifest.get(field) != expected for field, expected in expected_layout.items()):
        raise ValueError("the ALab package layout does not name its staged components")
    version = VERSION.fullmatch(build.get("version", ""))
    source = build.get("source", "")
    if (type(manifest.get("layoutVersion")) is not int or manifest["layoutVersion"] != 1
            or manifest.get("version") != build.get("version") or not version
            or not SOURCE.fullmatch(source) or version[4] != source[:9]
            or not isinstance(build.get("compiler"), dict)
            or build["compiler"].get("admitted") is not True
            or manifest.get("target") != build.get("target")
            or build.get("target") != "aarch64-apple-darwin"):
        raise ValueError("the ALab package records do not agree or the compiler is unadmitted")
    # The layout smoke checks these same Darwin members before publication. Check
    # them again when reusing an artifact: a published copy may since have lost a
    # companion. lstat the fixed parent directories too, so a linked parent cannot
    # make an executable outside the artifact count as one of its staged tools.
    for relative in ("bin", "codex-resources", "codex-path"):
        if not stat.S_ISDIR((path / relative).lstat().st_mode):
            raise ValueError(f"the package directory is not a regular directory: {relative}")
    for relative in ("bin/codex", "bin/codex-code-mode-host", "codex-path/rg"):
        metadata = (path / relative).lstat()
        if (not stat.S_ISREG(metadata.st_mode) or not metadata.st_mode & 0o111
                or metadata.st_mode & 0o022):
            raise ValueError(f"the package tool is not a regular protected executable: {relative}")
    return source, Version((int(version[index]) for index in (1, 2, 3)), version[0])


def git(repo, *args):
    result = subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True,
                            timeout=60)
    return result.returncode, result.stdout.strip()


def selected_checkout(root, prefix):
    """Snapshot this managed channel's checkout, without interpreting package records.

    This admits only a handoff to fresh tooling, never a build or promotion. The
    fixed-source controller must still validate the whole package under its guardian.
    """
    marker = parse_link_marker(regular_bytes(Path(prefix) / "links/codex").decode("utf-8"))
    checkout = marker.get("checkout")
    if (type(marker.get("schema")) is not int or marker["schema"] != 1
            or marker.get("program") != "codex"
            or marker.get("bins") != ["bin/codex"] or not isinstance(checkout, str)
            or not Path(checkout).is_absolute()):
        raise ValueError("the selected development link is not this artifact channel")
    selected = Path(checkout)
    if (selected.name.startswith(".") or not stat.S_ISDIR(selected.lstat().st_mode)
            or selected.resolve(strict=True).parent != root):
        raise ValueError("the selected checkout is not a member of this artifact channel")
    return checkout


def selected_package(root, prefix):
    """Return the exact marker checkout identity, with its validated package records."""
    checkout = selected_checkout(root, prefix)
    selected = Path(checkout)
    source, version = package(selected, root)
    return checkout, source, version


def require_selector(aterm, run=subprocess.run):
    """Positively establish every guarded selector capability before mutation."""
    # Older atpkg treats unknown trailing words as bin paths, and could successfully link
    # bin/codex while ignoring both guards. Positively establish support before mutation.
    help_result = run([aterm, "pkg", "link", "--help"], capture_output=True, text=True,
                      timeout=60)
    if (help_result.returncode != 0
            or not all(re.search(r"(?<![\w-])" + flag + r"(?=[\s=<\]])", help_result.stdout)
                       for flag in ("--if-checkout", "--if-prefix", "--wait-lock"))):
        raise RuntimeError("selector lacks conditional link support; package kept; use an "
                           "aterm with --if-checkout, --if-prefix and --wait-lock (ALAB_ATERM)")


def promote(repo, root, prefix, candidate, commit, aterm="aterm", run=subprocess.run,
            expected_checkout=None):
    """Return whether selected, with a reason. No applicable lane is a harmless no-op."""
    root = Path(root).resolve(strict=True)
    candidate = Path(candidate).absolute()
    try:
        prefix = Path(prefix).resolve(strict=True)
        selected, old_source, old_version = selected_package(root, prefix)
        if expected_checkout is not None and selected != expected_checkout:
            return False, "the selected package changed during this update; leaving it in place"
        new_source, new_version = package(candidate, root)
    except (OSError, ValueError, TypeError, AttributeError) as error:
        return False, f"the selected artifact channel is not eligible ({error})"
    # Builds are ordered by the version's <count> (git rev-list --count of its source), never
    # by its line: codex-alab's own line restarted at 0.1.0 after builds named on upstream's
    # numbering (alab/README.md, Decision 8), and a descendant on origin/main, checked below,
    # always counts more commits than its ancestor.
    if new_source != commit or new_source == old_source or new_version[2] <= old_version[2]:
        return False, "the package is not a forward source/version update"
    for args in (("merge-base", "--is-ancestor", old_source, new_source),
                 ("merge-base", "--is-ancestor", new_source, "refs/remotes/origin/main")):
        if git(repo, *args)[0] != 0:
            return False, "the package does not advance the selected source along origin/main"
    require_selector(aterm, run)
    # The package cache serializes producers; atpkg independently compares the expected
    # checkout and store while holding its own writer lock. Never retry unconditionally.
    result = run([aterm, "pkg", "link", "codex", str(candidate), "bin/codex",
                  "--if-checkout", str(selected), "--if-prefix", str(prefix),
                  "--wait-lock", str(WAIT_LOCK_SECONDS)],
                 capture_output=True, text=True, timeout=WAIT_LOCK_SECONDS + 60)
    if result.returncode != 0:
        detail = (result.stderr or result.stdout or f"exit {result.returncode}").strip()
        raise RuntimeError(f"conditional package promotion was refused; package kept: {detail}")
    return True, (f"selected {candidate.name} for new launches; "
                  "existing sessions may need a safe restart and resume")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("repo", "root", "prefix", "package", "commit"):
        parser.add_argument(f"--{name}", required=True)
    parser.add_argument("--aterm", default=os.environ.get("ALAB_ATERM", "aterm"))
    arguments = parser.parse_args()
    try:
        _, reason = promote(arguments.repo, arguments.root, arguments.prefix, arguments.package,
                            arguments.commit, arguments.aterm)
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"alab: local update: {error}", file=sys.stderr)
        return 1
    print(f"alab: local update: {reason}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())

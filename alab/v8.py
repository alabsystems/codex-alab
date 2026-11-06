#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Resolve the current tree's Codex V8 pair, then independently verify its pins.

Call this inside alab_contained, granting writes only to the requested cache.
Upstream executes in a separate interpreter and cannot replace this process's
checksum implementation. Neither caller-supplied V8 overrides nor source builds
participate in resolution.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys


UPSTREAM = '''import json, sys
from pathlib import Path
sys.path.insert(0, str(Path(sys.argv[1]) / "scripts"))
from codex_package.targets import TARGET_SPECS
from codex_package.v8 import resolve_codex_v8_cargo_env, resolved_v8_crate_version
env = resolve_codex_v8_cargo_env(TARGET_SPECS[sys.argv[2]], environ={}, cache_root=Path(sys.argv[3]))
print(json.dumps(dict(env, version=resolved_v8_crate_version())))
'''


def require_python():
    if sys.version_info < (3, 11):
        raise RuntimeError("V8 resolution requires Python >= 3.11 (tomllib); "
                           "set ALAB_PYTHON to a Python 3.11 or newer interpreter")


def resolved_version(repo):
    import tomllib

    lock = tomllib.loads((repo / "codex-rs/Cargo.lock").read_text(encoding="utf-8"))
    versions = {row["version"] for row in lock["package"] if row["name"] == "v8"}
    if len(versions) != 1:
        raise ValueError(f"expected one locked V8 version, found {sorted(versions)}")
    version = versions.pop()
    if not re.fullmatch(r"[0-9]+(?:\.[0-9]+){2}", version):
        raise ValueError(f"invalid locked V8 version: {version!r}")
    return version


def digest(path):
    if not stat.S_ISREG(path.lstat().st_mode):
        raise ValueError(f"V8 input is not a regular file: {path}")
    checksum = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            checksum.update(chunk)
    return checksum.hexdigest()


def checksums(path):
    entries = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        fields = line.split()
        if (len(fields) != 2 or not re.fullmatch(r"[0-9a-f]{64}", fields[0])
                or fields[1] in entries):
            raise ValueError(f"invalid or duplicate V8 checksum entry in {path}")
        entries[fields[1]] = fields[0]
    return entries


def verify(repo, target, cache, resolved):
    """Trust only Cargo.lock and the manifest pin, not upstream's verdict."""
    version = resolved_version(repo)
    if resolved.get("version") != version:
        raise ValueError("upstream V8 version does not match the current lockfile")
    directory = cache / f"rusty-v8-{version}-{target}"
    if directory.is_symlink():
        raise ValueError(f"V8 cache directory is a symlink: {directory}")
    profile = f"ptrcomp_sandbox_release_{target}"
    archive_name = (f"rusty_v8_{profile}.lib.gz" if "windows" in target
                    else f"librusty_v8_{profile}.a.gz")
    archive = directory / archive_name
    binding = directory / f"src_binding_{profile}.rs"
    for key, expected in (("RUSTY_V8_ARCHIVE", archive),
                          ("RUSTY_V8_SRC_BINDING_PATH", binding)):
        if resolved.get(key) != str(expected):
            raise ValueError(f"upstream returned an unexpected {key}: {resolved.get(key)!r}")
    manifest = directory / f"rusty_v8_{profile}.sha256"
    trusted = repo / "third_party/v8" / (
        f"rusty_v8_{version.replace('.', '_')}_release_manifests.sha256")
    pinned = checksums(trusted).get(manifest.name)
    if pinned is None or digest(manifest) != pinned:
        raise ValueError(f"V8 checksum manifest does not match its tree pin: {manifest}")
    expected = checksums(manifest)
    if expected.keys() != {archive.name, binding.name}:
        raise ValueError(f"V8 manifest does not cover exactly the archive and binding: {manifest}")
    for path in (archive, binding):
        if digest(path) != expected[path.name]:
            raise ValueError(f"V8 artifact does not match its manifest: {path}")
    return dict(version=version, target=target, RUSTY_V8_ARCHIVE=str(archive),
                RUSTY_V8_SRC_BINDING_PATH=str(binding))


def resolve(repo, target, cache):
    repo, cache = Path(repo).resolve(), Path(cache).resolve()
    if not re.fullmatch(r"[A-Za-z0-9_-]+", target):
        raise ValueError(f"invalid V8 target: {target!r}")
    if not cache.is_dir():
        cache.mkdir(parents=True, exist_ok=True)
    environment = dict(os.environ, CODEX_REPO_ROOT=str(repo))
    child = subprocess.run([sys.executable, "-I", "-B", "-c", UPSTREAM, str(repo), target, str(cache)],
                           cwd=repo, env=environment, capture_output=True, text=True)
    if child.stderr:
        print(child.stderr, end="", file=sys.stderr)
    if child.returncode:
        raise RuntimeError(f"current-tree upstream V8 resolver failed (exit {child.returncode})")
    resolved = json.loads(child.stdout)
    if not isinstance(resolved, dict):
        raise ValueError("upstream V8 resolver returned no environment object")
    return verify(repo, target, cache, resolved)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--cache", type=Path, required=True)
    args = parser.parse_args()
    try:
        require_python()
        print(json.dumps(resolve(args.repo, args.target, args.cache), sort_keys=True))
        return 0
    except (OSError, RuntimeError, ValueError, KeyError, TypeError) as error:
        print(f"alab: V8 resolution failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())

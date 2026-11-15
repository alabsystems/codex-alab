#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Bind a successful live certified build's observations to packaged bytes.

Called only after certified-build.sh succeeds. This does not recover verifier
authority from saved JSON, and never treats a serialized favorable claim as proof.
The full report stays in the private cache because it contains local source paths.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat


def digest(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode):
            raise ValueError(f"not a regular build output: {path}")
        value = hashlib.sha256()
        while chunk := os.read(descriptor, 1024 * 1024):
            value.update(chunk)
        after = os.fstat(descriptor)
        if (before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (
                after.st_size, after.st_mtime_ns, after.st_ctime_ns):
            raise ValueError(f"build output changed while hashing: {path}")
        return value.hexdigest()
    finally:
        os.close(descriptor)


def record(compiler, report, source, target, package, binaries):
    if not re.fullmatch(r"[0-9a-f]{40}", source):
        raise ValueError("proof observation requires a full source commit")
    document = json.loads(compiler.read_text())
    if document.get("admitted") is not True or not re.fullmatch(
            r"[0-9a-f]{64}", document.get("binaries_sha256", "")):
        raise ValueError("proof observation requires the native-admitted compiler record")
    if not binaries or len(set(binaries)) != len(binaries):
        raise ValueError("proof observation requires distinct shipping binaries")
    outputs = {}
    for binary in binaries:
        if not re.fullmatch(r"[A-Za-z0-9_-]+", binary):
            raise ValueError("invalid shipping binary name")
        built = digest(target / "release" / binary)
        packaged = digest(package / "bin" / binary)
        if packaged != built:
            raise ValueError(f"packaged {binary} differs from the compiled executable")
        outputs[binary] = {"built_sha256": built, "packaged_sha256": packaged}
    document["verification"] = {
        "source": source,
        "lane": "certify",
        "include_dependencies": True,
        "report_sha256": digest(report),
        "binaries": outputs,
        "authority": "observation of successful live certified compilation; saved report bytes confer no verifier authority",
    }
    compiler.write_text(json.dumps(document, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--source", required=True)
    parser.add_argument("--target", type=Path, required=True)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--binary", action="append", required=True)
    args = parser.parse_args()
    try:
        record(args.compiler, args.report, args.source, args.target, args.package, args.binary)
    except (OSError, ValueError) as error:
        parser.exit(1, f"alab: certified-build observation refused: {error}\n")


if __name__ == "__main__":
    main()

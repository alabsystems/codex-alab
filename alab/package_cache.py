#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Delegate disposable package-cache admission and maintenance to a capable aterm.

This module never removes files or adopts an existing cache. The native receipt,
coordinator lock and process-use fences decide whether a whole target may retire.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys


VERB = "__build_cache_package"
CAPABILITY = b"aterm-package-build-cache-v2"
BUDGET_BYTES = 64 << 30
ATTEMPT_ENV = "ALAB_PACKAGE_CACHE_ATTEMPT"
GUARD_PID_ENV = "ALAB_PACKAGE_GUARD_PID"
PRESERVE_ENV = "ALAB_PACKAGE_CACHE_PRESERVE"
MAX_OUTPUT = 16 << 10
DEADLINE = 60


def host():
    """Choose the same registrar family as lib.sh, without any store discovery."""
    if os.environ.get("ALAB_ATERM"):
        program = Path(os.environ["ALAB_ATERM"])
        if not program.is_absolute():
            return None
        command = [str(program), "pkg", VERB]
    elif os.environ.get("ATPKG_BIN"):
        command = [str(Path(os.environ["ATPKG_BIN"]) / "atpkg"), VERB]
    elif program := shutil.which("atpkg"):
        command = [program, VERB]
    elif program := shutil.which("aterm"):
        command = [program, "pkg", VERB]
    else:
        return None
    try:
        done = subprocess.run([*command, "--capabilities"], capture_output=True, timeout=DEADLINE)
    except (OSError, subprocess.SubprocessError):
        return None
    if done.returncode or done.stdout.strip() != CAPABILITY:
        return None
    return command


def invoke(command, action, words, *, env=None):
    done = subprocess.run([*command, action, *words], capture_output=True,
                          timeout=DEADLINE, env=env)
    if done.returncode:
        reason = done.stderr[:1024].decode(errors="replace").strip()
        raise RuntimeError(f"native package cache {action} refused ({done.returncode}): {reason}")
    if len(done.stdout) > MAX_OUTPUT:
        raise ValueError("native package cache reply is oversized")
    result = json.loads(done.stdout) if done.stdout.strip() else {}
    if not isinstance(result, dict):
        raise ValueError("native package cache reply is not an object")
    return result


def budgeted_target(cache, target, marker):
    """New clone-scoped generations never adopt the old compiler-only target."""
    cache = Path(cache).resolve(strict=True)
    target, marker = Path(target), Path(marker)
    parent = marker.parent
    info = parent.lstat()
    if (not marker.is_absolute() or parent.resolve(strict=True) != parent
            or not stat.S_ISDIR(info.st_mode) or info.st_uid != os.geteuid()
            or info.st_mode & 0o022):
        raise ValueError("package preservation parent is uncertain")
    if (target.parent != cache or not re.fullmatch(
            r"rust-release-[A-Za-z0-9_]+(?:-[A-Za-z0-9_]+)+-[0-9a-f]{16}\.noindex", target.name)):
        raise ValueError("package target is outside the admitted compiler-keyed namespace")
    identity = os.fsencode(marker) + b"\0" + f"{info.st_dev}:{info.st_ino}".encode("ascii")
    family = hashlib.sha256(identity).hexdigest()[:32]
    return cache / ("rust-budgeted-" + target.name[len("rust-"):-len(".noindex")] + "-" + family + ".noindex")


def admit(cache, target, manifest, compiler_driver, *, budget_bytes=BUDGET_BYTES):
    """Admit inside a guardian; unavailable opt-in retains the existing registrar.

    An untrusted/noncanonical guardian parent refuses before native admission;
    native policy refusals never fall back to an unbounded generation.
    """
    attempt = os.environ.get(ATTEMPT_ENV, "")
    producer = os.environ.get(GUARD_PID_ENV, "")
    if not re.fullmatch(r"[0-9a-f]{32}", attempt) or not re.fullmatch(r"[1-9][0-9]*", producer):
        return None
    driver = shutil.which(compiler_driver) if compiler_driver else None
    if driver is None:
        return None
    marker = os.environ.get(PRESERVE_ENV)
    if not marker or os.environ.get("ALAB_PKG_KEEP_WORKTREE") == "1":
        return None
    try:
        # The guardian supplied this clone's canonical common-dir marker. An
        # existing/unavailable policy keeps legacy outputs outside disposal.
        # A changed or untrusted guardian parent instead refuses explicitly.
        target = budgeted_target(cache, target, marker)
        try:
            os.lstat(marker)
        except FileNotFoundError:
            pass
        else:
            return None
    except OSError:
        return None
    command = host()
    if command is None:
        return None
    result = invoke(command, "admit", [
        "--cache", str(Path(cache).resolve(strict=True)),
        "--target-dir", str(target),
        "--manifest-path", str(Path(manifest).resolve(strict=True)),
        "--budget-bytes", str(budget_bytes),
        "--attempt", attempt, "--producer-pid", producer,
        "--producer-group", str(os.getpgrp()),
        "--compiler-driver", str(Path(driver).resolve(strict=True)),
        "--registrar", str(Path(command[0]).resolve(strict=True)),
        "--preserve-if-present", marker,
    ])
    if (result.get("path") != str(target) or type(result.get("budgeted")) is not bool
            or type(result.get("created")) is not bool):
        raise ValueError("native package cache admission does not name the requested target")
    if result["budgeted"]:
        if result.get("attempt") != attempt:
            raise ValueError("native package cache admission belongs to another attempt")
    elif result.get("attempt") is not None:
        raise ValueError("legacy package cache unexpectedly claimed an attempt")
    return result


def settled(cache, attempt, success, *, env=None):
    """Called by the guardian after group retirement, while its cache lock is held."""
    if not re.fullmatch(r"[0-9a-f]{32}", attempt):
        raise ValueError("package cache settlement needs the exact guardian attempt")
    command = host()
    if command is None:
        return None
    words = ["--cache", str(Path(cache).resolve(strict=True)), "--attempt", attempt]
    if success:
        words.append("--success")
    return invoke(command, "settled", words, env=env)


def compact(cache):
    """Called outside the producer lock; native maintenance acquires that lock itself."""
    command = host()
    if command is None:
        return None
    return invoke(command, "compact", ["--cache", str(Path(cache).resolve(strict=True))])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("admit", "compact"))
    parser.add_argument("--cache", required=True, type=Path)
    parser.add_argument("--target-dir", type=Path)
    parser.add_argument("--manifest-path", type=Path)
    parser.add_argument("--compiler-driver")
    parser.add_argument("--budget-bytes", type=int, default=BUDGET_BYTES)
    parser.add_argument("--print-path", action="store_true")
    args = parser.parse_args()
    if args.mode == "admit":
        if not args.target_dir or not args.manifest_path or not args.compiler_driver:
            parser.error("admit needs target, manifest and compiler driver")
        try:
            result = admit(args.cache, args.target_dir, args.manifest_path, args.compiler_driver,
                           budget_bytes=args.budget_bytes)
            # A missing capability or a native legacy observation keeps the established
            # pre-creation registration flow. A policy refusal never falls back.
            if result is None or not result["budgeted"]:
                return 69
            print(result["path"] if args.print_path else json.dumps(result))
            return 0
        except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
            print(f"alab: package cache admission: {error}", file=sys.stderr)
            return 75
    try:
        result = compact(args.cache)
        if result is not None:
            print(json.dumps(result))
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        # Maintenance uncertainty retains cache state; it cannot turn a failed build green.
        print(f"alab: package cache maintenance deferred: {error}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

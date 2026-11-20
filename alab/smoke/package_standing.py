#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Say what a codex package's own alab-build.json is worth to a smoke run.

A package's record names the evidence the smoke reader can report. Signature verification
belongs to atpkg. Qualifying records name an exact source commit, target, version, ripgrep,
packager and an admitted Trust compiler whose identity matches its recorded inputs.

The native layout records SHA-256 digests of the product and ripgrep build observations.
Its release commands use the unverified lane. alab/package.sh records a certified build
in compiler.verification, including dependencies, the source commit, the report digest
and matching digests for built and packaged executables. The reader checks that record
whenever present. Older package.sh records lack it and are ineligible for smoke qualification.
A trial marker, trial version or incomplete or malformed required field also rules a package out.

Stands alone (no sibling imports).
"""

import hashlib
import json
import re

# The `trial.attestation` trial_package.py writes, and the lanes a record can name.
TRIAL_ATTESTATION = "unverified-development-build"
UNVERIFIED_LANE = "unverified"
CERTIFIED_LANE = "certify"
# The packager codex-release's native layout names (codex-rs/alab-release/src/assembly.rs).
NATIVE_PACKAGER = "codex-release/native-layout-v1"


def text(value):
    return isinstance(value, str) and bool(value.strip())


def hashed(value):
    return isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value) is not None


def compiler_refusal(compiler, source):
    """Why the compiler record fails the smoke eligibility checks, or None."""
    if not isinstance(compiler, dict):
        return "its record names no compiler"
    if compiler.get("admitted") is not True:
        return "its compiler was not admitted (compiler.admitted is not true)"
    # Neither packager writes a lane here; package.sh names it under `verification`.
    if "lane" in compiler and compiler["lane"] != CERTIFIED_LANE:
        return f"its compiler lane is {compiler['lane']!r}, not {CERTIFIED_LANE!r}"
    said, sysroot, binaries = (compiler.get(name) for name in ("rustc_vV", "sysroot",
                                                               "binaries_sha256"))
    if not text(said) or not text(sysroot):
        return "its compiler record names no rustc -vV and sysroot"
    lines = said.splitlines()
    if "binary: trustc" not in lines or compiler.get("rustc") != lines[0]:
        return "its compiler is not recorded as Trust (rustc -vV names no `binary: trustc`)"
    if not hashed(binaries):
        return "its compiler record names no hash of the compiler's binaries"
    expected = hashlib.sha256("\0".join([said, sysroot, binaries]).encode()).hexdigest()[:16]
    if compiler.get("id") != expected:
        return "its compiler id is not the hash of the compiler it records"
    if "verification" in compiler:
        verification = compiler["verification"]
        binaries = verification.get("binaries") if isinstance(verification, dict) else None
        codex = binaries.get("codex") if isinstance(binaries, dict) else None
        if (not isinstance(verification, dict) or verification.get("lane") != CERTIFIED_LANE
                or verification.get("include_dependencies") is not True
                or verification.get("source") != source
                or not hashed(verification.get("report_sha256"))
                or not isinstance(codex, dict) or not hashed(codex.get("built_sha256"))
                or not hashed(codex.get("packaged_sha256"))):
            return ("its build verification is not a certified build of its own source "
                    "with dependencies included, bound to the codex it packaged")
        for binary, observation in binaries.items():
            if (not isinstance(binary, str) or not re.fullmatch(r"[A-Za-z0-9_-]+", binary)
                    or not isinstance(observation, dict)
                    or not hashed(observation.get("built_sha256"))
                    or not hashed(observation.get("packaged_sha256"))
                    or observation["built_sha256"] != observation["packaged_sha256"]):
                return "its build verification records changed or malformed packaged executable bytes"
    return None


def packager_refusal(build):
    """Why a record lacks its packager's required evidence, or None."""
    packager = build["packager"]
    if packager == NATIVE_PACKAGER:
        for name in ("build_observation_sha256", "ripgrep_build_observation_sha256"):
            if not hashed(build.get(name)):
                return f"its record names no {name}: the native layout always writes one"
        return None
    if re.fullmatch(r"[0-9a-f]{40}", packager) is None:
        return (f"its packager {packager!r} is neither codex-release's native layout nor a "
                "commit of alab/package.sh")
    if "verification" not in build["compiler"]:
        return ("its record carries no build verification (compiler.verification): "
                "package.sh wrote none before its builds were certified, and built those "
                "packages on the unverified lane")
    for name in ("rusty_v8", "built_at"):
        if not text(build.get(name)):
            return f"its record names no {name}: package.sh always writes one"
    return None


def not_qualifying(build):
    """Why a package with this alab-build.json cannot qualify a run, or None when it can."""
    if not isinstance(build, dict):
        return "its alab-build.json is not a JSON object"
    if "trial" in build:
        trial = build["trial"]
        attestation = trial.get("attestation") if isinstance(trial, dict) else None
        return (f"its record carries a `trial` object (attestation {attestation!r}): the "
                "package was assembled from an unverified development build")
    try:
        json.dumps(build, ensure_ascii=False).encode()
    except (UnicodeEncodeError, TypeError, ValueError, RecursionError):
        # JSON can carry half of a surrogate pair, which no packager writes and no hash takes.
        return "its record holds text that is not Unicode"
    version, source = build.get("version"), build.get("source")
    if not text(version):
        return "its record names no version"
    if "trial" in version.lower():
        return f"its version {version!r} names a trial"
    if not isinstance(source, str) or re.fullmatch(r"[0-9a-f]{40}", source) is None:
        return "its record names no exact source commit"
    refusal = compiler_refusal(build.get("compiler"), source)
    if refusal is not None:
        return refusal
    for name in ("target", "packager", "ripgrep", "ripgrep_features"):
        if not text(build.get(name)):
            return f"its record names no {name}"
    return packager_refusal(build)


def summary(build):
    """What a smoke's report keeps of a package's standing: never raises."""
    refusal = not_qualifying(build)
    trial = build.get("trial") if isinstance(build, dict) else None
    if refusal is not None:
        return {"qualifies": False, "why": refusal, "trial": trial}
    evidence = ["The package record names an exact source commit and an admitted Trust compiler."]
    if build["packager"] == NATIVE_PACKAGER:
        evidence.append("The native layout records SHA-256 digests of the product and ripgrep "
                        "build observations.")
    if "verification" in build["compiler"]:
        evidence.append("compiler.verification records a certified build of that source including "
                        "dependencies, the report SHA-256, and matching built and packaged SHA-256 "
                        "digests for the recorded executables.")
    evidence.append("The smoke reader leaves signature verification to atpkg.")
    return {"qualifies": True, "why": " ".join(evidence), "trial": trial}

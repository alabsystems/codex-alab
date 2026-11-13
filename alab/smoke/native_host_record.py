#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Record which aterm `native_host` test binary a smoke ran against, and check the record.

The host binary says nothing about the source it was built from. A record therefore keeps
what can be measured (the image's hash and size, and the verdict of the host's own
no-argument self-run) apart from what is only attested: the aterm commit named on this
command line, with a mandatory sentence saying how that is known, and what kind of
attestation that is. `trial` says the commit is not proven to be the binary's source;
`built-from-clean-commit` says it was built from a clean checkout of that commit, and
names who built it and when. Only the second, with the self-run made, qualifies a run
(`not_qualifying`): a reader of the record's fields, and not of its sentence, can tell.

  native_host_record.py --host <native_host> --source <40-hex aterm commit>
      --aterm-checkout <dir> --source-evidence <text>
      --attestation trial | --attestation built-from-clean-commit --builder <who>
          --built-at <ISO 8601 time>
      [--no-probe]

writes <host dir>/build.json. Stands alone (no sibling imports), so `python3 -I` runs it.
"""

import argparse
from datetime import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import time

SCHEMA = 3
# What the record's `source` is worth. Only CLEAN, with the self-run made, qualifies a run.
TRIAL = "trial"
CLEAN = "built-from-clean-commit"
ATTESTATIONS = (TRIAL, CLEAN)
RECORD_NAME = "build.json"
LEGACY_NAME = "build.json.legacy"
RECORD_LIMIT = 1024 * 1024
SHIPPING_VERDICT = "PASS real-host shipping-qualified policy"
PROBE_SECONDS = 180
GIT = "/usr/bin/git"
GUARD_PATH = Path(__file__).resolve().parents[1] / "command_guard.py"
# Where the host's self-run keeps its world: `/tmp`, or `/tmp/.ubd` under aterm's tempfile.
WORLD_PARENTS = (Path("/private/tmp"), Path("/private/tmp/.ubd"))
WORLD_NAME = re.compile(r"native-host-[A-Za-z0-9_.-]+")
WORLD_LINE = "WORLD native_host="


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def measure(host):
    data = host.read_bytes()
    return hashlib.sha256(data).hexdigest(), len(data)


def remove_world(path):
    """Remove the self-run's kept directory only where it is provably that directory.

    The path is text the host printed. It is removed only when its resolved parent is one
    of the two places the self-run creates it, its name has the self-run's form, it is a
    real directory and the caller owns it. Anything else is left for a person.
    """
    path = Path(path)
    try:
        if not path.is_absolute() or path.is_symlink() or not path.is_dir():
            return False
        if path.parent.resolve(strict=True) not in WORLD_PARENTS:
            return False
        if WORLD_NAME.fullmatch(path.name) is None or path.stat().st_uid != os.getuid():
            return False
        shutil.rmtree(path)
    except OSError:
        return False
    return not path.exists()


def run_probe(host, timeout=PROBE_SECONDS):
    """Run the host with no arguments: its own shipping-policy self-check.

    Nothing of the caller's environment reaches it: HOME, XDG_CONFIG_HOME,
    ATERM_HARNESS_STATE and TMPDIR are private and no ATERM_* or CODEX_* name is passed.
    """
    require(GUARD_PATH.is_file(), "missing alab/command_guard.py; keep the recorder in its source tree")
    with tempfile.TemporaryDirectory(prefix="native-host-record-") as scratch:
        scratch = Path(scratch).resolve()
        for name in ("home", "config", "state", "tmp", "work"):
            (scratch / name).mkdir()
        env = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "HOME": str(scratch / "home"),
               "XDG_CONFIG_HOME": str(scratch / "config"),
               "ATERM_HARNESS_STATE": str(scratch / "state"), "TMPDIR": str(scratch / "tmp"),
               "TERM": "dumb", "NO_COLOR": "1", "LANG": "en_US.UTF-8"}
        result_path = scratch / "guardian-result.json"
        command = [sys.executable, "-I", str(GUARD_PATH), "--",
                   sys.executable, "-I", str(GUARD_PATH), "--result-file", str(result_path),
                   "--inherited-group-timeout", str(timeout), "--", str(host)]
        started = time.monotonic()
        completed = subprocess.run(command, cwd=scratch / "work", env=env,
                                   stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE)
        seconds = round(time.monotonic() - started, 3)
        try:
            guardian = json.loads(result_path.read_text())
        except (OSError, ValueError):
            guardian = {}
    returncode = guardian.get("returncode")
    if guardian.get("timed_out") is True or type(returncode) is not int:
        # No raw status: the guardian's own exit (124 on its timeout) is all that is known.
        returncode = None
    lines = [line for line in completed.stdout.decode(errors="replace").splitlines() if line.strip()]
    said = [line for line in lines if not line.startswith(WORLD_LINE)]
    verdict = said[-1][:512] if said else ""
    if not verdict:
        errors = [line for line in completed.stderr.decode(errors="replace").splitlines() if line.strip()]
        verdict = ("stderr: " + errors[-1])[:512] if errors else ""
    world = None
    printed = next((line[len(WORLD_LINE):] for line in lines if line.startswith(WORLD_LINE)), None)
    if printed:
        # A failed self-run's world is its evidence and stays where it is.
        world = {"path": printed[:1024], "removed": returncode == 0 and remove_world(printed)}
    if guardian.get("timed_out") is True:
        verdict = f"no verdict: the self-run was stopped after {timeout} s"
    return {"ran": True, "returncode": returncode, "verdict": verdict, "seconds": seconds,
            "world": world}


def commit_exists(checkout, source):
    """A read: whether `source` names a commit the checkout holds."""
    environment = {"PATH": "/usr/bin:/bin", "GIT_CONFIG_GLOBAL": "/dev/null",
                   "GIT_CONFIG_NOSYSTEM": "1", "GIT_OPTIONAL_LOCKS": "0",
                   "GIT_NO_REPLACE_OBJECTS": "1", "GIT_NO_LAZY_FETCH": "1",
                   "GIT_TERMINAL_PROMPT": "0", "HOME": "/var/empty"}
    completed = subprocess.run([GIT, "-C", str(checkout), "cat-file", "-e", source + "^{commit}"],
                               env=environment, stdin=subprocess.DEVNULL,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return completed.returncode == 0


def read_record(path):
    require(path.is_file() and not path.is_symlink(), "native host build record missing")
    require(path.stat().st_size <= RECORD_LIMIT, "native host build record is oversized")
    try:
        value = json.loads(path.read_text())
    except ValueError:
        value = None
    require(isinstance(value, dict), "native host build record is not a JSON object")
    return value


def attested(attestation, builder, built_at):
    """Refuse an attestation that is not one of the two kinds, exactly stated.

    A trial names no builder and no build time: it claims no build. A build from a clean
    commit names both, the time as ISO 8601 with its offset.
    """
    require(isinstance(attestation, str) and attestation in ATTESTATIONS,
            "the attestation must be `trial` or `built-from-clean-commit` (--attestation)")
    if attestation == TRIAL:
        require(builder is None and built_at is None,
                "a trial record names no builder and no build time")
        return
    require(isinstance(builder, str) and bool(builder.strip()) and builder == builder.strip(),
            "a build from a clean commit names who built it (--builder)")
    try:
        moment = datetime.fromisoformat(built_at) if isinstance(built_at, str) else None
    except ValueError:
        moment = None
    require(moment is not None and moment.tzinfo is not None and "T" in built_at,
            "a build from a clean commit says when it was built, as ISO 8601 with an offset "
            "(--built-at 2026-11-12T09:30:00-08:00)")


def record(host, source, aterm_checkout, source_evidence, attestation, builder=None,
           built_at=None, probe=True, probe_seconds=PROBE_SECONDS):
    """Write <host dir>/build.json for `host`, by exclusive create."""
    host = Path(host).resolve(strict=True)
    require(host.is_file(), "native host is not a regular file")
    require(isinstance(source, str) and re.fullmatch(r"[0-9a-f]{40}", source) is not None,
            "the aterm source must be a full 40-hex commit")
    require(isinstance(source_evidence, str) and bool(source_evidence.strip()),
            "say how the source commit is known (--source-evidence)")
    attested(attestation, builder, built_at)
    require(commit_exists(Path(aterm_checkout), source),
            "the aterm checkout holds no such commit")
    digest, size = measure(host)
    path = host.parent / RECORD_NAME
    legacy = None
    if path.exists() or path.is_symlink():
        existing = read_record(path)
        # A record of an earlier schema no longer loads. It is retired beside the new one
        # when it names this image and this source; a current record is never replaced.
        earlier = "schema" not in existing or (
            type(existing["schema"]) is int and existing["schema"] < SCHEMA)
        require(earlier and existing.get("source") == source
                and existing.get("host_sha256") == digest,
                "a different native host build record exists; it is not replaced")
        legacy = host.parent / LEGACY_NAME
        require(not legacy.exists() and not legacy.is_symlink(),
                "a retired native host build record already exists")
    outcome = (run_probe(host, probe_seconds) if probe else
               {"ran": False, "returncode": None, "verdict": "", "seconds": 0, "world": None})
    # A host that fails its own check gets no record: `load` would refuse it, and a
    # written record is never replaced.
    require(not outcome["ran"] or (outcome["returncode"] == 0
            and outcome["verdict"].startswith(SHIPPING_VERDICT)),
            "native host did not pass its own shipping-policy self-run: returncode "
            f"{outcome['returncode']}, last line {outcome['verdict']!r}")
    require(measure(host) == (digest, size), "native host image changed while it was recorded")
    if legacy is not None:
        os.rename(path, legacy)
    value = {"schema": SCHEMA, "source": source, "source_evidence": source_evidence.strip(),
             "attestation": attestation, "builder": builder, "built_at": built_at,
             "host_sha256": digest, "host_bytes": size,
             "platform": f"{platform.system().lower()}-{platform.machine()}",
             "shipping_probe": outcome}
    with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644), "w") as output:
        json.dump(value, output, indent=2)
        output.write("\n")
        output.flush()
        os.fsync(output.fileno())
    return value


def load(host):
    """The checked record of `host`; raises RuntimeError naming what does not hold."""
    host = Path(host).resolve(strict=True)
    value = read_record(host.parent / RECORD_NAME)
    require(type(value.get("schema")) is int and value["schema"] == SCHEMA,
            f"native host build record is not schema {SCHEMA}; record it again with "
            "native_host_record.py")
    require(isinstance(value.get("source"), str)
            and re.fullmatch(r"[0-9a-f]{40}", value["source"]) is not None,
            "native host build record names no exact source commit")
    require(isinstance(value.get("source_evidence"), str) and bool(value["source_evidence"].strip()),
            "native host build record does not say how its source is known")
    try:
        attested(value.get("attestation"), value.get("builder"), value.get("built_at"))
    except RuntimeError as error:
        raise RuntimeError(f"native host build record's attestation is not valid: {error}") from None
    digest, size = measure(host)
    require(value.get("host_sha256") == digest and value.get("host_bytes") == size,
            "native host image disagrees with build record")
    outcome = value.get("shipping_probe")
    require(isinstance(outcome, dict) and type(outcome.get("ran")) is bool,
            "native host build record has no shipping probe entry")
    if outcome["ran"]:
        require(type(outcome.get("returncode")) is int and outcome["returncode"] == 0
                and isinstance(outcome.get("verdict"), str)
                and outcome["verdict"].startswith(SHIPPING_VERDICT),
                "native host did not pass its own shipping-policy self-run")
    return value


def not_qualifying(value):
    """Why a loaded record cannot qualify a run, or None when it can.

    A run is qualified against an aterm commit only when the record attests that the host
    was built from a clean checkout of it and the host passed its own self-run (`load`
    has refused a self-run that failed). Anything else is trial evidence, whatever commit
    the record's `source` names.
    """
    if value["attestation"] != CLEAN:
        return ("its attestation is `trial`: the source commit is not proven to be the "
                "binary's source")
    if value["shipping_probe"]["ran"] is not True:
        return "the host's shipping-policy self-run was not made (--no-probe)"
    return None


def require_aterm_name(host):
    """Refuse a host aterm's live reader would not take for aterm.

    The TUI smokes need the host to publish their session's live snapshot. It does so
    only when the process that opened the session's terminal is named `aterm` or
    `aterm-*` (aterm-agent's `owned_by_aterm`). That process is the host itself, and its
    name is its file's. Under any other name every snapshot is refused as `terminal`, the
    controller never sees a proven conversation, and each case ends at its hang detector.
    """
    name = Path(host).name
    require(name == "aterm" or name.startswith("aterm-"),
            f"the native host file is named {name!r}; aterm's live snapshot accepts only a "
            "terminal opened by a process named aterm or aterm-*: copy the host to a file "
            "named aterm-native-host and record that copy")


def summary(value):
    """What a smoke's report keeps of a loaded record."""
    return {"source": value["source"], "sourceEvidence": value["source_evidence"],
            "attestation": value["attestation"], "builder": value["builder"],
            "builtAt": value["built_at"], "qualifies": not_qualifying(value) is None,
            "sha256": value["host_sha256"], "shippingProbe": value["shipping_probe"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--host", required=True, type=Path)
    parser.add_argument("--source", required=True, help="the aterm commit, 40 hex digits (attested)")
    parser.add_argument("--aterm-checkout", required=True, type=Path,
                        help="an aterm checkout that holds the commit (read only)")
    parser.add_argument("--source-evidence", required=True,
                        help="how the commit is known to be the binary's source")
    parser.add_argument("--attestation", required=True, choices=ATTESTATIONS,
                        help="what the source commit is worth: only built-from-clean-commit, "
                             "with the self-check made, qualifies a run")
    parser.add_argument("--builder", help="who built the host (built-from-clean-commit only)")
    parser.add_argument("--built-at", help="when, as ISO 8601 with an offset "
                                           "(built-from-clean-commit only)")
    parser.add_argument("--no-probe", action="store_true",
                        help="do not run the host's no-argument self-check; the record then "
                             "qualifies no run")
    args = parser.parse_args()
    try:
        value = record(args.host, args.source, args.aterm_checkout, args.source_evidence,
                       args.attestation, builder=args.builder, built_at=args.built_at,
                       probe=not args.no_probe)
        load(args.host)
    except RuntimeError as error:
        print(json.dumps({"recorded": False, "error": str(error)}, indent=2))
        return 1
    print(json.dumps(value, indent=2))
    return 0

if __name__ == "__main__":
    sys.exit(main())

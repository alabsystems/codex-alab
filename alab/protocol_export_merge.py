#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Check a regenerated protocol archive against the three stages of its Git conflict."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from smoke.dropin_schema import EXPORTS, decompress


MISSING = object()
ARCHIVES = {Path(EXPORTS).name, Path(EXPORTS).name.replace("experimental", "stable")}


def read_exports(data):
    raw = decompress(data)
    if raw is None:
        raise ValueError("could not decompress the protocol archive (requires zstd or Python 3.14)")
    exports = json.loads(raw)
    if not isinstance(exports, dict) or set(exports) != {"typescript", "json_schema", "internal_json_schema"}:
        raise ValueError("unrecognized protocol archive layout")
    for group, files in exports.items():
        if not isinstance(files, dict) or not all(isinstance(text, str) for text in files.values()):
            raise ValueError(f"invalid protocol files in {group}")
        if group != "typescript":
            exports[group] = {name: json.loads(text) for name, text in files.items()}
    return exports


def same(left, right):
    # Python considers False equal to 0; JSON Schema gives them different meanings.
    if type(left) is not type(right):
        return False
    if isinstance(left, dict):
        return left.keys() == right.keys() and all(same(v, right[k]) for k, v in left.items())
    if isinstance(left, list):
        return len(left) == len(right) and all(same(a, b) for a, b in zip(left, right))
    return left == right


def differences(base, ours, theirs, generated, path=()):
    if same(ours, theirs):
        expected = ours
    elif same(ours, base):
        expected = theirs
    elif same(theirs, base):
        expected = ours
    else:
        if all(isinstance(item, dict) for item in (base, ours, theirs, generated)):
            for key in sorted(base.keys() | ours.keys() | theirs.keys() | generated.keys()):
                yield from differences(base.get(key, MISSING), ours.get(key, MISSING),
                                       theirs.get(key, MISSING), generated.get(key, MISSING),
                                       (*path, key))
            return
        yield path, "both parents changed this value; review the merged source"
        return
    if not same(expected, generated):
        yield path, "regenerated value differs from the parents' change"


def compare(repo, generated):
    if generated.name not in ARCHIVES:
        raise ValueError(f"expected one of {', '.join(sorted(ARCHIVES))}")
    archive = Path(EXPORTS).with_name(generated.name).as_posix()
    # Read the index once, including Git's own virtual base in a recursive merge.
    records = subprocess.check_output(["git", "-C", repo, "ls-files", "--unmerged", "--", archive], text=True)
    stages = {}
    for record in records.splitlines():
        metadata, _ = record.split("\t", 1)
        _, oid, stage = metadata.split()
        stages[stage] = oid
    if stages.keys() != {"1", "2", "3"}:
        raise ValueError(f"{archive} needs three unmerged index stages for this comparison")
    parents = [read_exports(subprocess.check_output(["git", "-C", repo, "cat-file", "blob", stages[stage]]))
               for stage in ("1", "2", "3")]
    data = generated.read_bytes()
    actual = read_exports(data)
    issues = []
    for group, files in actual.items():
        base, ours, theirs = (parent[group] for parent in parents)
        for name in sorted(base.keys() | ours.keys() | theirs.keys() | files.keys()):
            for path, why in differences(base.get(name, MISSING), ours.get(name, MISSING),
                                         theirs.get(name, MISSING), files.get(name, MISSING),
                                         (group, name)):
                issues.append({"path": path, "reason": why})
    return {"archive": archive, "stages": stages, "sha256": hashlib.sha256(data).hexdigest(),
            "files": {group: len(files) for group, files in actual.items()}, "issues": issues}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("generated", type=Path, help="archive from the upstream fixture writer")
    parser.add_argument("--repo", type=Path, default=Path.cwd(), help="worktree with the unresolved archive")
    args = parser.parse_args()
    try:
        result = compare(args.repo, args.generated)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(error, file=sys.stderr)
        return 1
    print(json.dumps(result, indent=2))
    return int(bool(result["issues"]))


if __name__ == "__main__":
    sys.exit(main())

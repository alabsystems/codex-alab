#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Select runtime helpers from Cargo artifacts for one admitted source/target pair."""

import argparse
import json
import os
from pathlib import Path
import sys


class Refusal(Exception):
    pass


def require(condition, message):
    if not condition:
        raise Refusal(message)


def inside(path, root):
    resolved = Path(path).resolve(strict=True)
    require(resolved.is_relative_to(root), f"helper path escapes its declared root: {path}")
    return resolved


def select(metadata, artifacts, repo, target_dir, helpers):
    root, target_dir = repo.resolve(strict=True), target_dir.resolve(strict=True)
    require(Path(metadata["workspace_root"]).resolve(strict=True) == root / "codex-rs",
            "helper metadata belongs to another workspace")
    require(Path(metadata["target_directory"]).resolve(strict=True) == target_dir,
            "helper metadata belongs to another target directory")
    members = set(metadata["workspace_members"])
    packages = [package for package in metadata["packages"] if package["id"] in members]
    selected = {}
    for name, binary in helpers:
        require(binary not in selected, f"helper selected twice: {binary}")
        providers = [package for package in packages if package["name"] == name]
        require(len(providers) == 1, f"no unique helper package: {name}")
        package = providers[0]
        manifest = inside(package["manifest_path"], root)
        require(manifest.is_file(), f"helper manifest is not a file: {manifest}")
        targets = [item for item in package["targets"]
                   if item["name"] == binary and item["kind"] == ["bin"]]
        require(len(targets) == 1, f"no unique binary target: {name}/{binary}")
        source = inside(targets[0]["src_path"], manifest.parent)
        require(source.is_file(), f"helper source is not a file: {source}")
        paths = set()
        for artifact in artifacts:
            compiled = artifact.get("target", {})
            if (artifact.get("reason") != "compiler-artifact"
                    or artifact.get("package_id") != package["id"]
                    or compiled.get("name") != binary or compiled.get("kind") != ["bin"]
                    or artifact.get("profile", {}).get("test") is not False):
                continue
            if (not artifact.get("manifest_path") or not compiled.get("src_path")
                    or not isinstance(artifact.get("executable"), str)):
                continue
            if (Path(artifact["manifest_path"]).resolve() != manifest
                    or Path(compiled["src_path"]).resolve() != source):
                continue
            executable = inside(artifact["executable"], target_dir)
            require(executable.is_file() and os.access(executable, os.X_OK),
                    f"prepared helper is not an executable file: {executable}")
            paths.add(str(executable))
        require(len(paths) == 1, f"no unique runtime helper artifact: {name}/{binary}")
        selected[binary] = paths.pop()
    return selected


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--target-dir", type=Path, required=True)
    parser.add_argument("--metadata", type=Path, required=True)
    parser.add_argument("--helper", action="append", required=True, metavar="PACKAGE/BINARY")
    args = parser.parse_args()
    try:
        helpers = [item.split("/") for item in args.helper]
        require(all(len(item) == 2 and all(item) for item in helpers),
                "helper selectors must name PACKAGE/BINARY")
        metadata = json.loads(args.metadata.read_text())
        artifacts = [json.loads(line) for line in sys.stdin if line.strip()]
        json.dump(select(metadata, artifacts, args.repo, args.target_dir, helpers), sys.stdout)
        sys.stdout.write("\n")
    except (Refusal, OSError, ValueError, KeyError, TypeError) as error:
        print(f"alab/check.sh: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

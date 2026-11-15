#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Assemble a trial codex package from an unverified development build.

A certified package comes from alab/package.sh. Where that cannot run, a fix can still be
tried against the smokes with a development build, as long as nothing mistakes the result
for a qualification. This writes a package directory whose own record says what it is:

  trial_package.py --binary <codex built with `targo --unverified build -p codex-cli
          --bin codex`> --checkout <the worktree it was built in>
      --source <40-hex commit> --output <new directory> --builder <who>
      --built-at <ISO 8601 time> --evidence <how it was built> [--target <triple>]

The binary's hash and size are measured. Its source is the builder's statement, not a
proof. What is required of it is what can be read without building:

- the checkout is a worktree root at `--source` with a clean `git status`;
- the binary lies in that checkout, in a path Git ignores (its build directory): a codex
  from an installed package, or from another worktree, is not this checkout's build;
- beside it is the dependency list its build wrote (`codex.d`, which cargo writes with
  every binary), that list names files of this checkout, each is still there, and none
  was written after the binary. That is how cargo judges a build up to date, so a
  refused binary is one that building again in the checkout rewrites, and a commit that
  changes nothing the binary was built from (or one made after the build, of the tree
  that was built) does not make the binary stale.

So the stated source is the tree the build directory belongs to now, and by its own
build's list the binary is not older than that tree. Nothing shows the binary was
compiled from it: the list is the build's statement, a real one names no Cargo manifest
or lockfile, and a file edited while the build ran is not noticed. The record says so,
names `compiler: {admitted: false, lane: "unverified"}` and carries a `trial` object, by
which package_standing.py refuses it as a qualification: `renewal_matrix.py` will not
run with it, and a single case run with it is trial evidence.

The package holds `bin/codex` alone (a copy, never a link). The binary takes ripgrep,
its resources and its helper programs from its package only when they are there
(codex-rs/install-context), so none is copied in and the manifest names none.

It signs nothing, links nothing, installs nothing and never runs the binary. The output
is refused where it exists, under an aterm package store, or under the checkout: this
account's stores and the checkout are told by the directory itself (its device and
inode), not by how a path to it is spelled. Prints the directory.
"""

import argparse
from datetime import datetime, timezone
import errno
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import stat
import subprocess
import sys
import tempfile

import capacity_exec as probe
import package_standing

# alab/qualification.py holds the one comparison of a worktree with its commit.
sys.path.append(str(Path(__file__).resolve().parents[1]))
import qualification

PACKAGER = "alab/smoke/trial_package.py"
STATEMENT = ("TRIAL: assembled from an unverified development build. It was not built by "
             "the certified lane, is not signed, is not a release and qualifies no run. Its "
             "source is the builder's statement, not a proof: when this package was assembled "
             "the checkout was at that commit with a clean git status, the binary lay in its "
             "build directory, and no file of the checkout that the build's own dependency "
             "list names was written after the binary. Nothing shows the binary was compiled "
             "from that commit.")
LAYOUT = ("bin/codex alone. No codex-path/rg, codex-resources, bin/codex-code-mode-host or "
          "bin/codex-exec-server: the binary takes each from its package only when it is "
          "there, and ripgrep otherwise from PATH. The assembler did not run the binary.")
DARWIN_MACHINES = {"arm64": "aarch64", "x86_64": "x86_64"}


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def host_target():
    """The triple of this machine, where one can be named without guessing a libc."""
    machine = DARWIN_MACHINES.get(platform.machine())
    require(platform.system() == "Darwin" and machine is not None,
            "name the binary's target triple (--target)")
    return machine + "-apple-darwin"


def directory_above(path, roots):
    """The first of `path` and its parents that is one of the directories `roots`, or None.

    Told by the directory itself (its device and inode), not by its name: a volume that
    ignores case, a firmlink and a symbolic link each give one directory several paths.
    """
    known = set()
    for root in roots:
        try:
            found = os.stat(root)
        except OSError:
            continue
        known.add((found.st_dev, found.st_ino))
    for directory in (path, *path.parents):
        try:
            found = os.stat(directory)
        except OSError:
            continue
        if (found.st_dev, found.st_ino) in known:
            return directory
    return None


def package_store_above(path):
    """The aterm package store or root the resolved `path` is under, or None.

    By form, wherever it is: a directory named `pkg` (atpkg's root, with its store and
    links) or `dev-builds` (package.sh's output, which the local updater selects from)
    inside one named `aterm`, in any case (a volume may ignore it, and nothing else is
    named so). By place: this account's two, and the root ATPKG_BIN names.
    """
    for directory in (path, *path.parents):
        if (directory.parent.name.casefold() == "aterm"
                and directory.name.casefold() in ("pkg", "dev-builds")):
            return directory
    roots = [Path.home() / "Library" / "Application Support" / "aterm" / "pkg",
             Path.home() / ".local" / "share" / "aterm" / "dev-builds"]
    front = os.environ.get("ATPKG_BIN", "")
    if os.path.isabs(front) and len(Path(front).parts) > 2:
        roots.append(Path(front).parent)
    return directory_above(path, roots)


def placed(output, checkout):
    """The resolved path of the new directory `output`, refused where it must not be."""
    output = Path(output)
    require(not os.path.lexists(output),
            "the output exists; a trial package is assembled in a new directory")
    require(output.name not in ("", "..") and output.parent.is_dir(),
            "the output's parent is not an existing directory")
    resolved = output.parent.resolve(strict=True) / output.name
    store = package_store_above(resolved)
    require(store is None, f"the output resolves under an aterm package store ({store}); "
            "a trial package is never installed, linked or published")
    require(directory_above(resolved, [checkout]) is None,
            "the output resolves under the checkout; it would dirty the tree it describes")
    return resolved


def checked_source(checkout, source):
    """Require `checkout` to be a worktree at `source` with nothing changed or untracked.

    Returns its resolved root, the ALab version that commit states (its VERSION file) and
    the commit's own time. A read: the comparison's index is a private one.
    """
    require(isinstance(source, str) and re.fullmatch(r"[0-9a-f]{40}", source) is not None,
            "the source must be a full 40-hex commit (--source)")
    try:
        root = Path(checkout).resolve(strict=True)
        top = Path(qualification.git(root, "rev-parse", "--show-toplevel")).resolve(strict=True)
        require(top == root, f"the checkout is not a worktree's root (its root is {top})")
        head = qualification.git(root, "rev-parse", "--verify", "HEAD^{commit}")
        require(head == source, f"the checkout is at {head}, not at the stated source {source}")
        with tempfile.TemporaryDirectory(prefix="trial-package-") as scratch:
            clean = qualification.worktree_clean(root, source, scratch_dir=scratch)
        require(clean, "the checkout is not clean: it has staged, changed or untracked files, "
                "so its commit does not describe the tree the binary was built in")
        version = qualification.git(root, "show", source + ":VERSION").strip()
        committed = qualification.git(root, "show", "-s", "--format=%cI", source)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        raise RuntimeError(f"the checkout cannot be read as a clean worktree: {error}") from None
    require(re.fullmatch(r"(?:0|[1-9][0-9]*)(?:\.(?:0|[1-9][0-9]*)){2}", version) is not None,
            f"the source's VERSION file states no X.Y.Z version: {version!r}")
    return root, version, committed


def moment(nanoseconds):
    return datetime.fromtimestamp(nanoseconds // 10**9, timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def built_in(binary, root):
    """The path of `binary` within the checkout `root`, refused where it is not its build.

    The binary must lie in the checkout, in a path Git ignores: what a build left there,
    and neither a file of the commit nor a codex that some other tree or package holds.
    """
    try:
        parent = Path(binary).parent.resolve(strict=True)
    except OSError as error:
        raise RuntimeError(f"the binary cannot be found: {error}") from None
    top = directory_above(parent, [root])
    require(top is not None,
            "the binary is not under the checkout (--checkout): only what a build left in "
            "that worktree's own build directory can be stated as built from its commit")
    relative = (parent / Path(binary).name).relative_to(top)
    try:
        qualification.git(root, "check-ignore", "--quiet", "--", str(relative))
    except subprocess.CalledProcessError as error:
        require(error.returncode == 1, f"Git cannot say whether the binary is ignored: {error}")
        raise RuntimeError(f"the binary ({relative}) is not in a path Git ignores: it is a "
                           "file of the commit, not what a build left") from None
    return root / relative


def listed_inputs(listing, binary):
    """The files the dependency list `listing` (cargo's `<binary>.d`) names for `binary`.

    The list is `<target>: <file> <file> ...`, a space or a backslash in a name escaped
    by a backslash. The target is matched by the file it names, not by its spelling.
    """
    try:
        found = os.lstat(listing)
        require(stat.S_ISREG(found.st_mode) and found.st_size <= 64 * 1024 * 1024,
                "it is not a regular file of a size a build writes")
        with open(listing, encoding="utf-8") as lines:
            for line in lines:
                names = [re.sub(r"\\(.)", r"\1", name)
                         for name in re.findall(r"(?:\\.|[^\s\\])+", line)]
                if (names and names[0].endswith(":") and os.path.exists(names[0][:-1])
                        and os.path.samestat(os.stat(names[0][:-1]), binary)):
                    return names[1:]
    except (OSError, UnicodeDecodeError, RuntimeError) as error:
        raise RuntimeError(f"the build's dependency list ({listing}) cannot be read: {error}; "
                           "cargo writes one beside every binary it builds") from None
    raise RuntimeError(f"the build's dependency list ({listing}) does not name the binary: "
                       "it is another build's")


def fresh(path, binary, root):
    """Refuse a binary that its build's own dependency list shows is not the checkout's.

    `path` is the binary within the checkout `root` and `binary` its status. Every file
    of the checkout the list names must be there and not written after the binary: what
    cargo requires of a build that is up to date, so building again rewrites a binary
    refused here. Returns how many such files there are and when the newest was written.
    A necessary condition only (the module's docstring says what it does not notice).
    """
    names = [Path(name) for name in listed_inputs(path.with_name(path.name + ".d"), binary)]
    inputs = [name for name in names if name.is_absolute() and name.is_relative_to(root)]
    require(inputs, "the build's dependency list names no file of the checkout: the binary "
            "was not built in this worktree")
    newest, later = 0, []
    for name in inputs:
        try:
            written = os.lstat(name).st_mtime_ns
        except OSError:
            raise RuntimeError(f"the binary was built from {name.relative_to(root)}, which the "
                               "checkout no longer holds: it is not a build of this tree; "
                               "build it again in the checkout") from None
        newest = max(newest, written)
        if written > binary.st_mtime_ns:
            later.append((written, name))
    if later:
        written, name = max(later)
        raise RuntimeError(
            f"{len(later)} of the {len(inputs)} files the binary was built from were written "
            f"after it (the newest, {name.relative_to(root)}, at {moment(written)}; the binary "
            f"at {moment(binary.st_mtime_ns)}): it is not a build of the checkout as it is "
            "now; build it again in the checkout, which rebuilds what reads them")
    return len(inputs), newest


def stated(builder, built_at, evidence):
    require(isinstance(builder, str) and bool(builder.strip()) and builder == builder.strip(),
            "name who built the binary (--builder)")
    try:
        moment = datetime.fromisoformat(built_at) if isinstance(built_at, str) else None
    except ValueError:
        moment = None
    require(moment is not None and moment.tzinfo is not None and "T" in built_at,
            "say when the binary was built, as ISO 8601 with an offset "
            "(--built-at 2026-11-13T09:30:00-08:00)")
    require(isinstance(evidence, str) and bool(evidence.strip()),
            "say how the binary was built (--evidence)")


def opened(binary):
    """Open the regular executable `binary` itself: its descriptor and its status.

    The final path component is never followed, so what is measured and copied is the
    file that was named.
    """
    try:
        source = os.open(binary, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    except OSError as error:
        require(error.errno != errno.ELOOP,
                "the binary is a symbolic link; name the built file itself (--binary)")
        raise RuntimeError(f"the binary cannot be opened (--binary): {error}") from None
    found = os.fstat(source)
    if not (stat.S_ISREG(found.st_mode) and found.st_size > 0 and found.st_mode & 0o111):
        os.close(source)
        raise RuntimeError("the binary is not a nonempty regular executable file (--binary)")
    return source, found


def copy_binary(source, before, destination):
    """Copy the open binary `source`, whose status was `before`, to the new `destination`.

    The copy is a new file, and the image must not change while it is read: another
    build may be writing it. Returns its sha256.
    """
    digest = hashlib.sha256()
    with os.fdopen(os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o755),
                   "wb") as output:
        while chunk := os.read(source, 1024 * 1024):
            digest.update(chunk)
            output.write(chunk)
        output.flush()
        os.fchmod(output.fileno(), 0o755)
        os.fsync(output.fileno())
    after = os.fstat(source)
    require((before.st_size, before.st_mtime_ns, before.st_ctime_ns)
            == (after.st_size, after.st_mtime_ns, after.st_ctime_ns),
            "the binary changed while it was copied; another build is writing it")
    copied = hashlib.sha256()
    with open(destination, "rb") as written:
        while chunk := written.read(1024 * 1024):
            copied.update(chunk)
    require(copied.hexdigest() == digest.hexdigest()
            and os.stat(destination).st_size == before.st_size,
            "the copied binary is not the one that was read")
    return digest.hexdigest()


def write_record(path, value):
    with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644), "w") as output:
        json.dump(value, output, indent=2)
        output.write("\n")
        output.flush()
        os.fsync(output.fileno())


def assemble(binary, checkout, source, output, builder, built_at, evidence, target=None):
    """Write the trial package directory `output` and return its resolved path."""
    stated(builder, built_at, evidence)
    target = host_target() if target is None else target
    require(isinstance(target, str) and re.fullmatch(r"[a-z0-9_]+(?:-[a-z0-9_.]+)+", target)
            is not None, "the target is not a target triple (--target)")
    root, base, committed = checked_source(checkout, source)
    opened_binary, found = opened(binary)
    try:
        inputs, newest = fresh(built_in(binary, root), found, root)
        output = placed(output, root)
        # No release form: a prerelease by semver, which reads build metadata as no difference.
        version = f"{base}-trial+g{source[:9]}"
        os.mkdir(output, 0o755)
        try:
            os.mkdir(output / "bin", 0o755)
            digest = copy_binary(opened_binary, found, output / "bin" / "codex")
            write_record(output / "alab-build.json", {
                "source": source, "version": version, "target": target,
                "compiler": {"admitted": False, "lane": package_standing.UNVERIFIED_LANE,
                             "admitted_why": "stated as built with `targo --unverified`: no "
                                             "compiler was admitted and nothing was verified"},
                "packager": PACKAGER,
                "assembled_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
                "trial": {"attestation": package_standing.TRIAL_ATTESTATION,
                          "statement": STATEMENT, "source": source,
                          "source_committed_at": committed, "binary_sha256": digest,
                          "binary_bytes": found.st_size,
                          "binary_modified_at": moment(found.st_mtime_ns),
                          "build_inputs": inputs, "newest_build_input_modified_at": moment(newest),
                          "builder": builder, "built_at": built_at,
                          "evidence": evidence.strip(), "layout": LAYOUT}})
            write_record(output / "codex-package.json", {
                "layoutVersion": 1, "version": version, "target": target, "variant": "codex",
                "entrypoint": "bin/codex"})
            # What a smoke will read of it: the records load, and they qualify nothing.
            build, _, executable = probe.package_records(output)
            standing = package_standing.summary(build)
            require(executable == output / "bin" / "codex" and standing["qualifies"] is False
                    and isinstance(standing["trial"], dict),
                    "the assembled record is not read as a trial package")
        except BaseException:
            # The directory was created here, exclusively, a moment ago: leave no partial
            # package.
            shutil.rmtree(output, ignore_errors=True)
            raise
    finally:
        os.close(opened_binary)
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--binary", required=True, type=Path,
                        help="a codex executable built with `targo --unverified build`, "
                             "where the build left it in the checkout")
    parser.add_argument("--checkout", required=True, type=Path,
                        help="the worktree it was built in: at --source, and clean (read only)")
    parser.add_argument("--source", required=True, help="that commit, 40 hex digits (stated)")
    parser.add_argument("--output", required=True, type=Path,
                        help="the package directory to create; refused if it exists")
    parser.add_argument("--builder", required=True, help="who built the binary")
    parser.add_argument("--built-at", required=True, help="when, as ISO 8601 with an offset")
    parser.add_argument("--evidence", required=True, help="how it was built")
    parser.add_argument("--target", help="the binary's target triple (default: this Mac's)")
    args = parser.parse_args()
    try:
        output = assemble(args.binary, args.checkout, args.source, args.output, args.builder,
                          args.built_at, args.evidence, target=args.target)
    except RuntimeError as error:
        print(f"trial_package: refused: {error}", file=sys.stderr)
        return 1
    print(output)
    return 0


if __name__ == "__main__":
    sys.exit(main())

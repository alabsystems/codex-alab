# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""trial_package.py assembles a package that says it is a trial, and refuses anything else.

The binary is a stand-in script, its dependency list is written by hand and the checkout
is a three-file repository: nothing here is codex, and nothing is built.
"""
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

SMOKE = Path(__file__).resolve().parents[1] / "smoke"
sys.path.insert(0, str(SMOKE))
import capacity_exec as probe
import package_standing
import renewal_matrix
import trial_package

BUILDER = "the unit test"
BUILT_AT = "2026-11-13T09:30:00-08:00"
EVIDENCE = "a stand-in script written by the unit test; no build was made"
TARGET = "aarch64-apple-darwin"
GIT_ENV = {"PATH": "/usr/bin:/bin", "HOME": "/var/empty", "GIT_CONFIG_GLOBAL": "/dev/null",
           "GIT_CONFIG_NOSYSTEM": "1"}


def git(repo, *args):
    return subprocess.run(["/usr/bin/git", "-C", str(repo), *args], env=GIT_ENV, check=True,
                          capture_output=True, text=True).stdout.strip()


def escaped(name):
    """A file name as cargo writes it in a dependency list."""
    return str(name).replace("\\", "\\\\").replace(" ", "\\ ")


def at(path, seconds):
    os.utime(path, (seconds, seconds))


def utc(text):
    return datetime.fromisoformat(text).astimezone(timezone.utc)


class Workspace:
    """A checkout with two commits and a stand-in build in its ignored build directory."""

    def __init__(self, case):
        self.root = Path(case.enterContext(tempfile.TemporaryDirectory())).resolve()
        self.checkout = self.root / "codex-alab"
        self.input = self.checkout / "codex-rs" / "cli" / "src" / "main.rs"
        self.input.parent.mkdir(parents=True)
        git(self.checkout, "init", "--quiet")
        (self.checkout / "VERSION").write_text("0.8.0\n")
        (self.checkout / "codex-rs" / ".gitignore").write_text("/target/\n")
        self.input.write_text("fn main() {}\n")
        self.earlier = self.commit("fixture")
        (self.checkout / "tracked").write_text("as committed\n")
        self.source = self.commit("fixture change")
        self.binary = self.checkout / "codex-rs" / "target" / "debug" / "codex"
        self.binary.parent.mkdir(parents=True)
        self.build(self.binary, self.input)
        self.output = self.root / "trial-package"

    def commit(self, message):
        git(self.checkout, "add", "--all")
        git(self.checkout, "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
            "commit", "--quiet", "--no-gpg-sign", "-m", message)
        return git(self.checkout, "rev-parse", "HEAD")

    def build(self, binary, *inputs, target=None):
        """What a build leaves: the stand-in binary and, beside it, its dependency list.

        The list names `inputs` and, as a real one does, a generated file outside the tree.
        """
        binary.write_text("#!/bin/sh\necho 'codex-cli 0.0.0'\n")
        binary.chmod(0o755)
        self.listed(binary, *inputs, target=target)

    def listed(self, binary, *inputs, target=None):
        names = [*inputs, self.root / "cargo-build" / "out" / "generated.rs"]
        binary.with_name(binary.name + ".d").unlink(missing_ok=True)
        binary.with_name(binary.name + ".d").write_text(
            escaped(binary if target is None else target) + ": "
            + " ".join(map(escaped, names)) + "\n")

    def assemble(self, **changes):
        arguments = {"binary": self.binary, "checkout": self.checkout, "source": self.source,
                     "output": self.output, "builder": BUILDER, "built_at": BUILT_AT,
                     "evidence": EVIDENCE, "target": TARGET}
        arguments.update(changes)
        return trial_package.assemble(**arguments)


def files(directory):
    return sorted(str(path.relative_to(directory)) for path in directory.rglob("*"))


class AssemblyTests(unittest.TestCase):
    def setUp(self):
        self.work = Workspace(self)

    def test_the_package_holds_a_copy_of_the_binary_and_a_record_that_says_trial(self):
        work = self.work
        output = work.assemble(evidence="  " + EVIDENCE + "\n")
        self.assertEqual(output, work.output)
        self.assertEqual(files(output), ["alab-build.json", "bin", "bin/codex", "codex-package.json"])
        copied = output / "bin" / "codex"
        self.assertFalse(copied.is_symlink())
        self.assertEqual(copied.read_bytes(), work.binary.read_bytes())
        self.assertNotEqual(copied.stat().st_ino, work.binary.stat().st_ino)
        self.assertEqual(stat.S_IMODE(copied.stat().st_mode), 0o755)
        digest = hashlib.sha256(work.binary.read_bytes()).hexdigest()

        build = json.loads((output / "alab-build.json").read_text())
        version = "0.8.0-trial+g" + work.source[:9]
        self.assertEqual(set(build), {"source", "version", "target", "compiler", "packager",
                                      "assembled_at", "trial"})
        self.assertEqual((build["source"], build["version"], build["target"], build["packager"]),
                         (work.source, version, TARGET, "alab/smoke/trial_package.py"))
        # No release has this form: a semver prerelease, the commit in its build metadata.
        self.assertIsNotNone(re.fullmatch(r"\d+\.\d+\.\d+-trial\+g[0-9a-f]{9}", build["version"]))
        compiler = build["compiler"]
        self.assertEqual((compiler["admitted"], compiler["lane"]), (False, "unverified"))
        self.assertEqual(set(compiler), {"admitted", "lane", "admitted_why"})
        trial = build["trial"]
        self.assertEqual(set(trial), {"attestation", "statement", "source", "source_committed_at",
            "binary_sha256", "binary_bytes", "binary_modified_at", "build_inputs",
            "newest_build_input_modified_at", "builder", "built_at", "evidence", "layout"})
        # Of the two files the build's list names, one is the checkout's.
        self.assertEqual((trial["build_inputs"], trial["newest_build_input_modified_at"]), (
            1, datetime.fromtimestamp(int(work.input.stat().st_mtime), timezone.utc)
            .strftime("%Y-%m-%dT%H:%M:%SZ")))
        self.assertEqual({key: trial[key] for key in ("attestation", "source", "binary_sha256",
            "binary_bytes", "builder", "built_at", "evidence")}, {
            "attestation": "unverified-development-build", "source": work.source,
            "binary_sha256": digest, "binary_bytes": work.binary.stat().st_size,
            "builder": BUILDER, "built_at": BUILT_AT, "evidence": EVIDENCE})
        self.assertEqual(trial["source_committed_at"],
                         git(work.checkout, "show", "-s", "--format=%cI", work.source))
        # The record says what its source is worth, and what the package does not hold.
        self.assertIn("the builder's statement, not a proof", trial["statement"])
        self.assertIn("qualifies no run", trial["statement"])
        self.assertIn("Nothing shows the binary was compiled from that commit", trial["statement"])
        self.assertIn("bin/codex alone", trial["layout"])
        self.assertEqual(json.loads((output / "codex-package.json").read_text()), {
            "layoutVersion": 1, "version": version, "target": TARGET, "variant": "codex",
            "entrypoint": "bin/codex"})

        # What a smoke reads of it: the records load, and they qualify nothing.
        loaded, manifest, executable = probe.package_records(output)
        self.assertEqual((loaded, manifest["version"], executable), (build, version, copied))
        standing = package_standing.summary(loaded)
        self.assertEqual((standing["qualifies"], standing["trial"]), (False, trial))
        self.assertIn("`trial` object (attestation 'unverified-development-build')",
                      renewal_matrix.package_refusal(output))
        # The checkout was only read.
        self.assertEqual(git(work.checkout, "status", "--porcelain", "--untracked-files=all"), "")
        self.assertEqual(git(work.checkout, "rev-parse", "HEAD"), work.source)

    def test_a_checkout_that_is_not_the_clean_stated_source_is_refused(self):
        work = self.work

        def changed():
            (work.checkout / "tracked").write_text("edited\n")

        def staged():
            changed()
            git(work.checkout, "add", "tracked")

        def untracked():
            (work.checkout / "codex-rs" / "new.rs").write_text("fn main() {}\n")

        def hidden():
            changed()
            git(work.checkout, "update-index", "--assume-unchanged", "tracked")

        for name, dirty in (("a changed file", changed), ("a staged change", staged),
                            ("an untracked file", untracked),
                            ("a change the index was told to overlook", hidden)):
            with self.subTest(name=name):
                dirty()
                with self.assertRaisesRegex(RuntimeError, "the checkout is not clean"):
                    work.assemble()
                self.assertFalse(work.output.exists())
                git(work.checkout, "update-index", "--no-assume-unchanged", "tracked")
                git(work.checkout, "reset", "--quiet", "--hard", work.source)
                git(work.checkout, "clean", "--quiet", "-fd")
        # The control: the same checkout, clean again, is taken.
        self.assertTrue(work.binary.is_file())
        work.assemble(output=work.root / "control")
        # Another commit than the one checked out, real or not, and what is not a commit.
        wrong = {"an earlier commit": (work.earlier, "not at the stated source"),
                 "an unknown commit": ("0" * 39 + "1", "not at the stated source"),
                 "an abbreviation": (work.source[:12], "full 40-hex commit"),
                 "upper case": (work.source.upper(), "full 40-hex commit"),
                 "nothing": (None, "full 40-hex commit")}
        for name, (source, message) in wrong.items():
            with self.subTest(name=name):
                with self.assertRaisesRegex(RuntimeError, message):
                    work.assemble(source=source)
                self.assertFalse(work.output.exists())
        with self.assertRaisesRegex(RuntimeError, "not a worktree's root"):
            work.assemble(checkout=work.checkout / "codex-rs")
        with self.assertRaisesRegex(RuntimeError, "cannot be read as a clean worktree"):
            work.assemble(checkout=work.root)
        self.assertFalse(work.output.exists())

    def test_a_binary_that_is_not_the_checkouts_own_build_is_refused(self):
        # Nothing else ties the binary to the commit it is stated to be built from: the
        # installed package's codex, or another worktree's build, is not this one's.
        work = self.work
        installed = (work.root / "Application Support" / "aterm" / "pkg" / "store" / "codex"
                     / "2026111301" / "bin" / "codex")
        other = work.root / "other-worktree" / "codex-rs" / "target" / "debug" / "codex"
        for binary in (installed, other):
            binary.parent.mkdir(parents=True)
            # Even with a list that names this checkout's file.
            work.build(binary, work.input)
        alias = work.binary.parent / "alias"
        alias.symlink_to(other.parent)
        committed = work.checkout / "codex-rs" / "scripts" / "codex"
        committed.parent.mkdir()
        work.build(committed, work.input)
        committed.with_name("codex.d").unlink()
        source = work.commit("an executable of the commit")
        work.listed(work.binary.parent / "placeholder", work.input)
        refused = {"an installed package's": (installed, "not under the checkout"),
                   "another worktree's": (other, "not under the checkout"),
                   "one a link in the build directory leads out to": (alias / "codex",
                                                                    "not under the checkout"),
                   "a file of the commit": (committed, "not in a path Git ignores")}
        for name, (binary, message) in refused.items():
            with self.subTest(name=name):
                with self.assertRaisesRegex(RuntimeError, message):
                    work.assemble(binary=binary, source=source)
                self.assertFalse(work.output.exists())
        # The controls: the checkout's own build, by its path and through a link to its
        # build directory from outside.
        door = work.root / "door"
        door.symlink_to(work.binary.parent)
        work.assemble(source=source)
        work.assemble(source=source, binary=door / "codex", output=work.root / "through-a-link")

    def test_a_binary_older_than_what_it_was_built_from_is_refused(self):
        # A build directory outlives the trees its worktree moves through. The build's own
        # dependency list says which files the binary was made from: one written after
        # the binary is one it does not hold, and building again rewrites the binary.
        work = self.work
        spaced = work.input.with_name("with space.rs")
        spaced.write_text("// a module\n")
        source = work.commit("a second input")
        work.listed(work.binary, work.input, spaced)
        built = int(work.binary.stat().st_mtime)
        at(work.input, built - 60)
        at(spaced, built + 1)
        with self.assertRaisesRegex(RuntimeError, "1 of the 2 files the binary was built from "
                                    "were written after it .the newest, codex-rs/cli/src/with "
                                    "space.rs, at .*build it again in the checkout"):
            work.assemble(source=source)
        at(work.input, built + 2)
        with self.assertRaisesRegex(RuntimeError, "2 of the 2 files .*newest, "
                                                  "codex-rs/cli/src/main.rs, at "):
            work.assemble(source=source)
        self.assertFalse(work.output.exists())
        # The control: written in the binary's own second, both are taken, and the record
        # keeps the count and the newest time for a reader to compare.
        at(work.input, built - 60)
        at(spaced, built)
        at(work.binary, built)
        trial = json.loads((work.assemble(source=source) / "alab-build.json").read_text())["trial"]
        self.assertEqual((trial["build_inputs"], trial["newest_build_input_modified_at"],
                          trial["binary_modified_at"]),
                         (2, trial["binary_modified_at"], datetime.fromtimestamp(
                             built, timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")))
        # A file the binary was built from that the commit no longer has.
        git(work.checkout, "rm", "--quiet", "codex-rs/cli/src/with space.rs")
        source = work.commit("the second input removed")
        with self.assertRaisesRegex(RuntimeError, "built from codex-rs/cli/src/with space.rs, "
                                                  "which the checkout no longer holds"):
            work.assemble(source=source, output=work.root / "removed")
        self.assertFalse((work.root / "removed").exists())

    def test_a_commit_that_changes_nothing_the_binary_was_built_from_does_not_stale_it(self):
        # No build writes the binary again for a commit that changes none of its inputs,
        # nor for a commit of the tree that was built: the binary is then older than its
        # stated commit and is still that tree's build.
        work = self.work
        built = int(time.time()) - 3600
        at(work.input, built - 60)
        at(work.binary, built)
        (work.checkout / "tracked").write_text("a note; no build reads it\n")
        noted = work.commit("a note")
        trial = json.loads((work.assemble(source=noted) / "alab-build.json").read_text())["trial"]
        self.assertLess(utc(trial["binary_modified_at"]), utc(trial["source_committed_at"]))
        self.assertEqual(utc(trial["binary_modified_at"]).timestamp(), built)
        # Edited, built, then committed: the commit is of the tree the build read.
        work.input.write_text("fn main() { /* the fix */ }\n")
        at(work.input, built - 30)
        fixed = work.commit("the fix, committed after it was built")
        trial = json.loads((work.assemble(source=fixed, output=work.root / "fixed")
                            / "alab-build.json").read_text())["trial"]
        self.assertLess(utc(trial["binary_modified_at"]), utc(trial["source_committed_at"]))
        self.assertEqual(utc(trial["newest_build_input_modified_at"]).timestamp(), built - 30)

    def test_a_binary_without_its_own_builds_dependency_list_is_refused(self):
        work = self.work
        listing = work.binary.with_name("codex.d")
        sibling = work.binary.with_name("codex-exec-server")
        work.build(sibling, work.input)

        def missing():
            listing.unlink()

        def linked():
            listing.symlink_to(sibling.with_name("codex-exec-server.d"))

        def anothers():
            work.listed(work.binary, work.input, target=sibling)

        def elsewhere():
            work.listed(work.binary, work.root / "other-worktree" / "codex-rs" / "main.rs")

        def relative():
            listing.unlink()
            listing.write_text(escaped(work.binary) + ": codex-rs/cli/src/main.rs\n")

        refused = {"none": (missing, "dependency list .* cannot be read"),
                   "a link to another": (linked, "dependency list .* cannot be read"),
                   "another binary's": (anothers, "does not name the binary"),
                   "one that names no file of the checkout": (elsewhere,
                                                             "names no file of the checkout"),
                   "one whose names have no place": (relative, "names no file of the checkout")}
        for name, (spoil, message) in refused.items():
            with self.subTest(name=name):
                spoil()
                with self.assertRaisesRegex(RuntimeError, message):
                    work.assemble()
                self.assertFalse(work.output.exists())
        # The control: the same binary with its own list again.
        work.listed(work.binary, work.input)
        work.assemble()

    def test_the_source_must_state_a_version(self):
        work = self.work
        (work.checkout / "VERSION").write_text("next\n")
        source = work.commit("no version")
        with self.assertRaisesRegex(RuntimeError, "states no X.Y.Z version: 'next'"):
            work.assemble(source=source)
        git(work.checkout, "rm", "--quiet", "VERSION")
        source = work.commit("no VERSION file")
        with self.assertRaisesRegex(RuntimeError, "cannot be read as a clean worktree"):
            work.assemble(source=source)
        self.assertFalse(work.output.exists())

    def test_an_output_that_exists_or_lies_where_packages_live_is_refused(self):
        work = self.work
        existing = work.root / "existing"
        existing.mkdir()
        plain = work.root / "plain-file"
        plain.write_text("a file\n")
        dangling = work.root / "dangling"
        dangling.symlink_to(work.root / "nowhere")
        for name, output in (("a directory", existing), ("a file", plain), ("a link", dangling)):
            with self.subTest(name=name):
                with self.assertRaisesRegex(RuntimeError, "the output exists"):
                    work.assemble(output=output)
        self.assertEqual((files(existing), plain.read_text(), dangling.is_symlink()),
                         ([], "a file\n", True))
        with self.assertRaisesRegex(RuntimeError, "parent is not an existing directory"):
            work.assemble(output=work.root / "no-parent" / "package")

        # A store by its form, wherever it is: atpkg's root and package.sh's output.
        store = work.root / "Application Support" / "aterm" / "pkg" / "store" / "codex"
        artifacts = work.root / "share" / "aterm" / "dev-builds" / "artifacts"
        elsewhere = work.root / "managed" / "store"
        for directory in (store, artifacts, elsewhere):
            directory.mkdir(parents=True)
        door = work.root / "door"
        door.symlink_to(store)
        refused = {"atpkg's store": store / "2026111399", "atpkg's root": store.parents[1] / "trial",
                   "package.sh's output": artifacts / "0.8.0-trial",
                   "a link into a store": door / "2026111399"}
        for name, output in refused.items():
            with self.subTest(name=name):
                with self.assertRaisesRegex(RuntimeError, "under an aterm package store"):
                    work.assemble(output=output)
                self.assertFalse(os.path.lexists(output))
        # And by place: the root the session's ATPKG_BIN names, and this account's own.
        with mock.patch.dict(os.environ, {"ATPKG_BIN": str(work.root / "managed" / "bin")}):
            with self.assertRaisesRegex(RuntimeError, "under an aterm package store"):
                work.assemble(output=elsewhere / "trial")
        home = work.root / "home"
        (home / "Library" / "Application Support" / "aterm").mkdir(parents=True)
        (home / "Library" / "Application Support" / "aterm" / "pkg").symlink_to(elsewhere)
        with mock.patch.dict(os.environ, {"ATPKG_BIN": "", "HOME": str(home)}):
            with self.assertRaisesRegex(RuntimeError, "under an aterm package store"):
                work.assemble(output=elsewhere / "trial")
            # The control: with neither naming it, that directory is an ordinary one.
            (home / "Library" / "Application Support" / "aterm" / "pkg").unlink()
            work.assemble(output=elsewhere / "trial")
        self.assertEqual(files(store) + files(artifacts), [])

        # Under the checkout, even where Git would not see it.
        for name, output in (("in the tree", work.checkout / "trial-package"),
                             ("in the ignored build directory", work.binary.parent / "pkg")):
            with self.subTest(name=name):
                with self.assertRaisesRegex(RuntimeError, "under the checkout"):
                    work.assemble(output=output)
                self.assertFalse(output.exists())

    def test_a_store_or_checkout_spelled_in_another_case_is_refused_too(self):
        # On a volume that ignores case (this Mac's), one directory has many spellings.
        work = self.work
        store = work.root / "Application Support" / "aterm" / "pkg" / "store" / "codex"
        managed = work.root / "managed" / "store"
        for directory in (store, managed):
            directory.mkdir(parents=True)
        folds = (work.root / "MANAGED").exists()
        by_form = {"ATERM": work.root / "Application Support" / "ATERM" / "pkg" / "store" / "codex",
                   "PKG": work.root / "Application Support" / "aterm" / "PKG" / "store" / "codex",
                   "Dev-Builds": work.root / "share" / "Aterm" / "Dev-Builds" / "artifacts"}
        for name, directory in by_form.items():
            with self.subTest(name=name):
                # Where case is kept apart, such a directory is another one, refused alike.
                directory.mkdir(parents=True, exist_ok=True)
                with self.assertRaisesRegex(RuntimeError, "under an aterm package store"):
                    work.assemble(output=directory / "trial")
                self.assertFalse(os.path.lexists(directory / "trial"))
        if not folds:
            self.skipTest("this volume keeps case apart: a directory has one spelling")
        with mock.patch.dict(os.environ, {"ATPKG_BIN": str(work.root / "managed" / "bin")}):
            with self.assertRaisesRegex(RuntimeError, "under an aterm package store"):
                work.assemble(output=work.root / "MANAGED" / "Store" / "trial")
        for name, output in (("in the tree", work.root / "CODEX-ALAB" / "trial-package"),
                             ("in the build directory", work.root / "Codex-Alab" / "codex-rs"
                              / "TARGET" / "pkg")):
            with self.subTest(name=name):
                with self.assertRaisesRegex(RuntimeError, "under the checkout"):
                    work.assemble(output=output)
        self.assertEqual(files(store) + files(managed), [])
        self.assertEqual(git(work.checkout, "status", "--porcelain", "--untracked-files=all"), "")
        # The control: the same names where they are no store and no checkout.
        work.assemble(output=work.root / "MANAGED" / "Store" / "trial")

    def test_a_binary_that_is_not_the_built_file_itself_is_refused(self):
        work = self.work
        link = work.root / "codex-link"
        link.symlink_to(work.binary)
        plain = work.root / "not-executable"
        plain.write_bytes(work.binary.read_bytes())
        plain.chmod(0o644)
        empty = work.root / "empty"
        empty.write_bytes(b"")
        empty.chmod(0o755)
        inside = work.binary.with_name("codex-link")
        inside.symlink_to(work.binary)
        refused = {"a symbolic link": (link, "is a symbolic link"),
                   "a symbolic link in the build directory": (inside, "is a symbolic link"),
                   "a missing file": (work.root / "absent", "cannot be opened"),
                   "a directory": (work.binary.parent, "not a nonempty regular executable"),
                   "a file that is not executable": (plain, "not a nonempty regular executable"),
                   "an empty file": (empty, "not a nonempty regular executable")}
        for name, (binary, message) in refused.items():
            with self.subTest(name=name):
                with self.assertRaisesRegex(RuntimeError, message):
                    work.assemble(binary=binary)
                # Nothing is left behind, not even by a refusal met while copying.
                self.assertFalse(work.output.exists())

    def test_a_binary_that_changes_while_it_is_copied_is_refused(self):
        # Another build may be writing the same build directory.
        work = self.work
        read, written = os.read, []

        def rewriting(descriptor, size):
            chunk = read(descriptor, size)
            if chunk and not written and os.path.samestat(os.fstat(descriptor),
                                                          os.stat(work.binary)):
                written.append(work.binary.stat().st_mtime_ns - 10**9)
                os.utime(work.binary, ns=(written[0], written[0]))
            return chunk

        with mock.patch.object(trial_package.os, "read", rewriting):
            with self.assertRaisesRegex(RuntimeError, "the binary changed while it was copied"):
                work.assemble()
        # It was being copied when it changed, and the partial package was removed.
        self.assertEqual(len(written), 1)
        self.assertFalse(work.output.exists())

    def test_the_builders_statement_is_required_and_exactly_stated(self):
        work = self.work
        refused = {"no builder": {"builder": None}, "a blank builder": {"builder": "  "},
                   "a padded builder": {"builder": " someone "},
                   "no build time": {"built_at": None}, "a date alone": {"built_at": "2026-11-13"},
                   "a time with no offset": {"built_at": "2026-11-13T09:30:00"},
                   "a time that is not one": {"built_at": "this morning"},
                   "no evidence": {"evidence": None}, "blank evidence": {"evidence": " \n"},
                   "a target that is not a triple": {"target": "this Mac"}}
        for name, fields in refused.items():
            with self.subTest(name=name):
                with self.assertRaises(RuntimeError):
                    work.assemble(**fields)
                self.assertFalse(work.output.exists())
        # With no target named, this machine's is stated only where it can be.
        with mock.patch.object(trial_package.platform, "system", return_value="Darwin"), \
                mock.patch.object(trial_package.platform, "machine", return_value="arm64"):
            self.assertEqual(trial_package.host_target(), "aarch64-apple-darwin")
        with mock.patch.object(trial_package.platform, "system", return_value="Linux"):
            with self.assertRaisesRegex(RuntimeError, "--target"):
                work.assemble(target=None)

    def test_a_package_its_own_reader_would_not_call_a_trial_is_not_left_behind(self):
        work = self.work
        qualified = {"qualifies": True, "why": "", "trial": None}
        with mock.patch.object(trial_package.package_standing, "summary", return_value=qualified):
            with self.assertRaisesRegex(RuntimeError, "not read as a trial package"):
                work.assemble()
        self.assertFalse(work.output.exists())


class CommandTests(unittest.TestCase):
    def run_tool(self, work, output):
        return subprocess.run([sys.executable, "-B", str(SMOKE / "trial_package.py"),
            "--binary", str(work.binary), "--checkout", str(work.checkout),
            "--source", work.source, "--output", str(output), "--builder", BUILDER,
            "--built-at", BUILT_AT, "--evidence", EVIDENCE, "--target", TARGET],
            capture_output=True, text=True, timeout=60)

    def test_the_command_prints_the_directory_and_a_refusal_names_its_reason(self):
        work = Workspace(self)
        completed = self.run_tool(work, work.output)
        self.assertEqual((completed.returncode, completed.stdout, completed.stderr),
                         (0, str(work.output) + "\n", ""))
        self.assertTrue((work.output / "bin" / "codex").is_file())
        before = files(work.output)
        completed = self.run_tool(work, work.output)
        self.assertEqual((completed.returncode, completed.stdout), (1, ""))
        self.assertEqual(completed.stderr, "trial_package: refused: the output exists; a trial "
                         "package is assembled in a new directory\n")
        self.assertEqual(files(work.output), before)


if __name__ == "__main__":
    unittest.main()

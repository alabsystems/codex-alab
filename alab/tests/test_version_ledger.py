#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""codex-alab's compound version and its ledger (alab/README.md, Decision 8, amended
2026-10-24): codex-alab's own core with the upstream Codex baseline in SemVer build metadata
(`0.1.0+codex.0.164.0`, a dev build `0.1.0-dev.<count>+codex.0.164.0.g<sha9>`), and
alab/versions.toml, the append-only ledger mapping each released version to its baseline.

The real package.sh runs in a scratch clone whose upstream remote is a scratch repository
shaped like openai/codex (a linear main, each release a commit off its branch point), as
test_version_lines.py builds it, whose origin is a scratch bare repository each commit lands
on, and whose public release repository (ALAB_RELEASED_REPO) is a scratch bare repository
holding what the publication engine's mirror-head promote pushes: a parentless snapshot
naming its dev commit, as main and tagged v<VERSION>. publish/pre-promote runs there as the
publication engine runs it; alab/versions.py's grammar is read directly; alab_build_json runs
as package.sh defines it, sourced.
"""

import importlib.util
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ENTRY = "#!/bin/sh\n'''exec' " + shlex.quote(sys.executable) + ' -I "$0" "$@"\n\' \'\'\'\n'
SPEC = importlib.util.spec_from_file_location("versions", ROOT / "alab" / "versions.py")
VERSIONS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERSIONS)
HEADER = "# codex-alab's version ledger (test copy)\n"


class Scratch(unittest.TestCase):
    def setUp(self):
        self.scratch = Path(tempfile.mkdtemp(prefix="alab-version-ledger-"))
        self.addCleanup(shutil.rmtree, self.scratch, ignore_errors=True)
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith(("GIT_", "ALAB_", "CODEX_ALAB_", "PUB_"))}
        self.env.update(GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_NOSYSTEM="1",
                        ALAB_PYTHON=sys.executable,
                        HOME=str(self.scratch), GIT_AUTHOR_NAME="Upstream",
                        GIT_AUTHOR_EMAIL="upstream@example.invalid", GIT_COMMITTER_NAME="Upstream",
                        GIT_COMMITTER_EMAIL="upstream@example.invalid")
        self.upstream = self.scratch / "codex"
        self.git(self.scratch, "init", "-q", "-b", "main", str(self.upstream))
        # main: u1 - u2 - u3 - u4. rust-v0.163.0 branches at u1, rust-v0.164.0 at u2,
        # rust-v0.165.0 at u4.
        self.u1 = self.commit(self.upstream, "u1", "2026-10-18T10:00:00+0000")
        self.u2 = self.commit(self.upstream, "u2", "2026-10-19T10:00:00+0000")
        self.u3 = self.commit(self.upstream, "u3", "2026-10-20T10:00:00+0000")
        self.u4 = self.commit(self.upstream, "u4", "2026-10-21T10:00:00+0000")
        self.tags = {}
        for version, point in (("0.163.0", self.u1), ("0.164.0", self.u2), ("0.165.0", self.u4)):
            self.release(version, point)
        self.git(self.upstream, "checkout", "-q", "main")
        self.alab = self.scratch / "codex-alab"
        self.git(self.scratch, "init", "-q", "-b", "main", str(self.alab))
        self.git(self.alab, "remote", "add", "upstream", str(self.upstream))
        self.git(self.alab, "config", "remote.upstream.tagOpt", "--no-tags")
        self.git(self.alab, "config", "--unset-all", "remote.upstream.fetch")
        self.git(self.alab, "config", "--add", "remote.upstream.fetch",
                 "refs/heads/main:refs/remotes/upstream/main")
        self.git(self.alab, "config", "--add", "remote.upstream.fetch",
                 "+refs/tags/rust-v*:refs/upstream-tags/rust-v*")
        self.git(self.alab, "fetch", "-q", "upstream")
        self.origin = self.scratch / "origin.git"
        self.git(self.scratch, "init", "-q", "--bare", "-b", "main", str(self.origin))
        self.git(self.alab, "remote", "add", "origin", str(self.origin))
        self.public = self.scratch / "public.git"
        self.git(self.scratch, "init", "-q", "--bare", "-b", "main", str(self.public))
        # What GitHub serves: a filtered fetch, and objects by id for its lazy reads.
        self.git(self.public, "config", "uploadpack.allowFilter", "true")
        self.git(self.public, "config", "uploadpack.allowAnySHA1InWant", "true")
        self.env["ALAB_RELEASED_REPO"] = f"file://{self.public}"
        self.when = 0

    def git(self, cwd, *args):
        process = subprocess.run(["git", "-C", str(cwd), *args], env=self.env,
                                 capture_output=True, text=True, timeout=60)
        self.assertEqual(process.returncode, 0, process.stderr)
        return process.stdout.strip()

    def commit(self, repo, subject, when):
        env = dict(self.env, GIT_AUTHOR_DATE=when, GIT_COMMITTER_DATE=when)
        process = subprocess.run(["git", "-C", str(repo), "commit", "-q", "--allow-empty",
                                  "-m", subject], env=env, capture_output=True, text=True,
                                 timeout=60)
        self.assertEqual(process.returncode, 0, process.stderr)
        return self.git(repo, "rev-parse", "HEAD")

    def release(self, version, point, after=None):
        """Tag rust-v<version> on a commit off `point` (or off the release commit `after`)."""
        self.git(self.upstream, "checkout", "-q", "--detach", after or point)
        self.commit(self.upstream, f"Release {version}", "2026-10-22T12:00:00+0000")
        self.git(self.upstream, "tag", f"rust-v{version}")
        self.tags[version] = self.git(self.upstream, "rev-parse", "HEAD")

    def on(self, point, name):
        self.git(self.alab, "checkout", "-q", "-b", name, point)

    def row(self, version, baseline, date="2026-10-24", commit=None):
        return (f'\n[[release]]\nversion = "{version}"\nbaseline = "{baseline}"\n'
                f'baseline_commit = "{commit or self.tags[baseline]}"\ndate = "{date}"\n')

    def layer(self, version="0.1.0", client="0.165.0", ledger=HEADER, land=True):
        """ALab's tooling, VERSION, alab/client-version and (unless None) the ledger, committed
        and (with `land`) pushed as origin's main, this clone's origin/main moving with it."""
        (self.alab / "alab").mkdir(exist_ok=True)
        for name in ("package.sh", "lib.sh", "command_guard.py", "github.py", "versions.py"):
            shutil.copy2(ROOT / "alab" / name, self.alab / "alab" / name)
        (self.alab / "VERSION").write_text(version + "\n")
        (self.alab / "alab" / "client-version").write_text(client + "\n")
        path = self.alab / "alab" / "versions.toml"
        if ledger is None:
            path.unlink(missing_ok=True)
        else:
            path.write_text(ledger)
        self.git(self.alab, "add", "-A", "alab", ".")
        self.when += 1
        source = self.commit(self.alab, "alab(layer): the tooling",
                             f"2026-10-23T13:{self.when:02d}:00+0000")
        if land:
            self.land(source)
        return source, self.git(self.alab, "rev-list", "--count", source)

    def land(self, commit, remember=False):
        """`commit` as origin's main (forced: a test may rewrite it), and so origin/main here.
        This clone then forgets where origin/main was before (its reflog), as a fresh clone of
        each test's world would, unless a test of that memory asks it to `remember`."""
        self.git(self.alab, "push", "-q", "-f", "origin", f"{commit}:refs/heads/main")
        if not remember:
            self.git(self.alab, "reflog", "expire", "--expire=now", "--expire-unreachable=now",
                     "refs/remotes/origin/main")

    def publish(self, source, version, date="2026-10-24"):
        """What the engine's mirror-head promote pushes: a parentless snapshot of `source`'s
        tree naming it, as the public main, tagged v<version>."""
        snapshot = self.git(self.alab, "commit-tree", f"{source}^{{tree}}", "-m",
                            f"snapshot: {date} from codex-alab@{source}")
        self.git(self.alab, "push", "-q", "-f", str(self.public), f"{snapshot}:refs/heads/main")
        self.git(self.public, "tag", "-a", f"v{version}", "-m", f"v{version}", snapshot)
        return snapshot

    def package(self, *args):
        return subprocess.run(["/bin/bash", str(self.alab / "alab" / "package.sh"), *args],
                              env=self.env, capture_output=True, text=True, timeout=120)

    def refused(self, process, said):
        self.assertNotEqual(process.returncode, 0, process.stdout)
        self.assertEqual(process.stdout, "")
        self.assertIn(said, process.stderr)

    def hook(self, **env):
        """publish/pre-promote as the publication engine runs it."""
        copy = self.scratch / "engine" / "publish"
        copy.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / "publish" / "pre-promote", copy / "pre-promote")
        return subprocess.run([str(copy / "pre-promote")], cwd=self.alab,
                              env=dict(self.env, **env), stdin=subprocess.DEVNULL,
                              capture_output=True, text=True, timeout=120)


class CompoundVersionTests(Scratch):
    def test_the_version_names_the_upstream_baseline_and_the_client_line_is_unchanged(self):
        self.on(self.u3, "main-on-u3")
        source, count = self.layer()
        version = self.package("--version-only")
        self.assertEqual(version.returncode, 0, version.stderr)
        self.assertEqual(version.stdout, f"0.1.0-dev.{count}+codex.0.164.0.g{source[:9]}\n")
        self.assertIn("+codex.0.164.0: the upstream Codex baseline, rust-v0.164.0",
                      version.stderr)
        self.assertIn("it never orders", version.stderr)
        self.assertEqual(self.git(self.alab, "status", "--porcelain"), "",
                         "a version query must leave its source checkout clean")
        client = self.package("--client-version-only")
        self.assertEqual(client.returncode, 0, client.stderr)
        self.assertEqual(client.stdout, f"0.165.0-dev.{count}+g{source[:9]}\n")

    def test_the_baseline_keeps_its_real_patch_number(self):
        # A patch release off 0.164.0's release commit shares its branch point (u2).
        self.release("0.164.1", self.u2, after=self.tags["0.164.0"])
        self.git(self.alab, "fetch", "-q", "upstream")
        self.on(self.u3, "main-on-u3")
        source, count = self.layer()
        version = self.package("--version-only")
        self.assertEqual(version.returncode, 0, version.stderr)
        self.assertEqual(version.stdout, f"0.1.0-dev.{count}+codex.0.164.1.g{source[:9]}\n")
        # The client floor is still the next minor above it.
        self.assertEqual(self.package("--client-version-only").stdout,
                         f"0.165.0-dev.{count}+g{source[:9]}\n")

    def test_a_published_snapshot_reads_its_baseline_from_its_ledger_row(self):
        # One commit and no upstream history, as the publication engine pushes it.
        source, count = self.layer(ledger=HEADER + self.row("0.1.0", "0.164.0"))
        self.assertEqual(count, "1")
        version = self.package("--version-only")
        self.assertEqual(version.returncode, 0, version.stderr)
        self.assertEqual(version.stdout, f"0.1.0-dev.1+codex.0.164.0.g{source[:9]}\n")
        self.assertIn("a published snapshot", version.stderr)
        # A snapshot is never released again: it has no history to check a release from.
        self.refused(self.package("--release-check"), "no upstream history")

    def test_a_snapshot_whose_ledger_records_no_baseline_is_refused(self):
        self.layer()
        for flag in ("--version-only", "--client-version-only", "--release-check"):
            with self.subTest(flag=flag):
                self.refused(self.package(flag), "records no baseline for its VERSION 0.1.0")

    def test_a_ledger_without_codex_alab_s_own_line_is_refused(self):
        self.on(self.u3, "main-on-u3")
        (self.alab / "alab").mkdir(exist_ok=True)
        self.layer()
        (self.alab / "alab" / "client-version").unlink()
        self.git(self.alab, "add", "-A", ".")
        self.commit(self.alab, "drop the client line", "2026-10-23T14:00:00+0000")
        self.refused(self.package("--version-only"), "carries alab/versions.toml but no")


class LedgerReleaseTests(Scratch):
    def test_a_release_appends_its_row_and_says_its_compound_version(self):
        self.on(self.u3, "main-on-u3")
        source, _ = self.layer()
        # (d) no row for the version: refused, naming the remedy.
        self.refused(self.package("--release-check"), "has no row for 0.1.0")
        row = self.package("--ledger-row")
        self.assertEqual(row.returncode, 0, row.stderr)
        self.assertRegex(row.stdout, r'^\n\[\[release\]\]\nversion = "0\.1\.0"\n'
                                     r'baseline = "0\.164\.0"\n'
                                     rf'baseline_commit = "{self.tags["0.164.0"]}"\n'
                                     r'date = "[0-9]{4}-[0-9]{2}-[0-9]{2}"\n$')
        source, _ = self.layer(ledger=HEADER + row.stdout)
        released = self.package("--release-check")
        self.assertEqual(released.returncode, 0, released.stderr)
        self.assertEqual(released.stdout, "0.1.0\n")
        self.assertIn("release 0.1.0+codex.0.164.0", released.stderr)
        compound = self.package("--release-version")
        self.assertEqual(compound.returncode, 0, compound.stderr)
        self.assertEqual(compound.stdout, "0.1.0+codex.0.164.0\n")
        # Its row is recorded: a second one for the same version is refused.
        self.refused(self.package("--ledger-row"), "already has its row")

    def test_a_recorded_row_never_changes_and_is_never_removed(self):
        self.on(self.u3, "main-on-u3")
        rows = HEADER + self.row("0.1.0", "0.164.0", "2026-10-24")
        recorded, _ = self.layer("0.1.0", ledger=rows)
        self.assertEqual(self.package("--release-check").returncode, 0)
        for name, ledger, said in (
                ("date", HEADER + self.row("0.1.0", "0.164.0", "2026-10-25"), "never changes"),
                ("commit", HEADER + self.row("0.1.0", "0.164.0", commit="f" * 40),
                 "never changes"),
                ("removed", HEADER, "is gone"),
        ):
            with self.subTest(name=name):
                self.git(self.alab, "checkout", "-q", "-b", f"edit-{name}", recorded)
                self.layer("0.1.0", ledger=ledger)
                self.refused(self.package("--release-check"), said)
                self.layer("0.1.0", ledger=rows)
                restored = self.package("--release-check")
                if name == "removed":
                    # The row back as it was recorded: the mapping never had another value.
                    self.assertEqual(restored.returncode, 0, restored.stderr)
                else:
                    # Restoring a changed row does not undo the history that changed it: the
                    # version once read two ways.
                    self.refused(restored, "never changes")

    def test_an_older_row_changed_refuses_a_newer_release(self):
        self.on(self.u3, "main-on-u3")
        first = self.row("0.1.0", "0.164.0", "2026-10-24")
        self.layer("0.1.0", ledger=HEADER + first)
        moved = self.row("0.1.0", "0.164.0", "2026-10-23")
        self.layer("0.2.0", ledger=HEADER + moved + self.row("0.2.0", "0.164.0", "2026-10-25"))
        self.refused(self.package("--release-check"), "the row for 0.1.0")
        self.refused(self.package("--ledger-row"), "the row for 0.1.0")

    def test_a_version_keeps_its_baseline_and_versions_may_share_one(self):
        self.on(self.u3, "main-on-u3")
        self.layer("0.1.0", ledger=HEADER + self.row("0.1.0", "0.164.0"))
        # Two codex-alab versions on one baseline.
        two = HEADER + self.row("0.1.0", "0.164.0") + self.row("0.2.0", "0.164.0")
        self.layer("0.2.0", ledger=two)
        released = self.package("--release-version")
        self.assertEqual(released.returncode, 0, released.stderr)
        self.assertEqual(released.stdout, "0.2.0+codex.0.164.0\n")
        # main merges past rust-v0.165.0's branch point: the baseline moves forward.
        self.git(self.alab, "merge", "-q", "--no-edit", self.u4)
        source, count = self.layer("0.2.0", "0.166.0", ledger=two)
        version = self.package("--version-only")
        self.assertEqual(version.stdout, f"0.2.0-dev.{count}+codex.0.165.0.g{source[:9]}\n")
        self.assertIn("records 0.2.0 on 0.164.0", version.stderr)
        # (b) 0.2.0 is mapped to 0.164.0, and that mapping never changes.
        self.refused(self.package("--release-check"), "recorded on the upstream baseline 0.164.0")
        self.refused(self.package("--ledger-row"), "already has its row")
        # `pub bump`, then the new version's row.
        self.layer("0.3.0", "0.166.0", ledger=two)
        row = self.package("--ledger-row")
        self.assertEqual(row.returncode, 0, row.stderr)
        self.assertIn('baseline = "0.165.0"', row.stdout)
        self.layer("0.3.0", "0.166.0", ledger=two + row.stdout)
        self.assertEqual(self.package("--release-version").stdout, "0.3.0+codex.0.165.0\n")

    def test_a_release_is_above_every_recorded_version(self):
        self.on(self.u3, "main-on-u3")
        two = HEADER + self.row("0.1.0", "0.164.0") + self.row("0.2.0", "0.164.0")
        # (c) the version's row is not the newest.
        self.layer("0.1.0", ledger=two)
        self.refused(self.package("--release-check"), "is not the newest recorded version")
        # A row out of order is refused as the ledger is read.
        self.layer("0.1.0", ledger=HEADER + self.row("0.2.0", "0.164.0")
                   + self.row("0.1.0", "0.164.0"))
        self.refused(self.package("--release-check"), "strictly increasing version order")

    def test_origin_main_s_later_row_refuses_an_older_commit(self):
        self.on(self.u3, "main-on-u3")
        older, _ = self.layer("0.1.0", ledger=HEADER + self.row("0.1.0", "0.164.0"))
        newer, _ = self.layer("0.2.0", ledger=HEADER + self.row("0.1.0", "0.164.0")
                              + self.row("0.2.0", "0.164.0"))
        self.git(self.alab, "update-ref", "refs/remotes/origin/main", newer)
        self.refused(self.package("--release-check", "--commit", older), "the row for 0.2.0")
        released = self.package("--release-check", "--commit", newer)
        self.assertEqual(released.returncode, 0, released.stderr)

    def test_a_baseline_never_moves_backward(self):
        # (e) a commit on u1 is built on rust-v0.163.0, older than the row before it.
        self.on(self.u1, "main-on-u1")
        self.layer("0.2.0", "0.165.0", ledger=HEADER + self.row("0.1.0", "0.164.0"))
        self.refused(self.package("--ledger-row"), "never moves backward")
        self.layer("0.2.0", "0.165.0", ledger=HEADER + self.row("0.1.0", "0.164.0")
                   + self.row("0.2.0", "0.163.0"))
        self.refused(self.package("--release-check"), "never moves backward")

    def test_pre_promote_releases_the_version_its_ledger_maps(self):
        self.on(self.u3, "main-on-u3")
        source, _ = self.layer(ledger=HEADER + self.row("0.1.0", "0.164.0"))
        process = self.hook(PUB_SOURCE_COMMIT=source, PUB_VERSION="0.1.0")
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertIn("0.1.0+codex.0.164.0", process.stdout)
        # The engine must release the commit's own VERSION, never the client line.
        process = self.hook(PUB_SOURCE_COMMIT=source, PUB_VERSION="0.165.0")
        self.assertNotEqual(process.returncode, 0, process.stdout)
        self.assertIn("VERSION is 0.1.0", process.stderr)
        self.on(self.u3, "no-row")
        bare, _ = self.layer()
        process = self.hook(PUB_SOURCE_COMMIT=bare, PUB_VERSION="0.1.0")
        self.assertNotEqual(process.returncode, 0, process.stdout)
        self.assertIn("has no row for 0.1.0", process.stderr)


class ReleaseRecordTests(Scratch):
    """Rule (a) holds against a rewritten origin/main: the public release repository records
    every release as a snapshot naming its dev commit, which must still be on origin/main."""

    def release_0_1_0(self):
        """main on u3: 0.1.0's row appended, released as 0.1.0+codex.0.164.0 and published."""
        self.on(self.u3, "main-on-u3")
        self.layer("0.1.0")
        row = self.package("--ledger-row")
        self.assertEqual(row.returncode, 0, row.stderr)
        released, _ = self.layer("0.1.0", ledger=HEADER + row.stdout)
        check = self.package("--release-version")
        self.assertEqual(check.returncode, 0, check.stderr)
        self.assertEqual(check.stdout, "0.1.0+codex.0.164.0\n")
        self.publish(released, "0.1.0")
        return released, row.stdout

    def date_of(self, row):
        # --ledger-row uses today's UTC date. Later fixture rows must not
        # predate it as the real calendar moves beyond their historical dates.
        return json.loads(next(line.partition("=")[2] for line in row.splitlines()
                               if line.startswith("date = ")))

    def rewrite(self, row, land=True):
        """A history on u3 that never held the released row: 0.1.0 recorded on rust-v0.163.0,
        then 0.2.0 appended. Each row is recorded as --ledger-row would, so the rewrite reads
        as a well-formed history of its own."""
        forged = row.replace('baseline = "0.164.0"', 'baseline = "0.163.0"').replace(
            self.tags["0.164.0"], self.tags["0.163.0"])
        self.assertNotEqual(forged, row)
        self.git(self.alab, "checkout", "-q", "-b", "rewritten", self.u3)
        self.layer("0.1.0", ledger=HEADER + forged, land=land)
        top, _ = self.layer("0.2.0", ledger=HEADER + forged + self.row("0.2.0", "0.164.0",
                                                                       date=self.date_of(row)),
                            land=land)
        return top

    def test_a_rewritten_main_cannot_change_a_released_row(self):
        released, row = self.release_0_1_0()
        # origin's main is force-moved to the rewrite, and this clone's origin/main follows it
        # (as the engine's own `git fetch origin`, a forced refspec, takes it).
        top = self.rewrite(row)
        self.assertEqual(self.git(self.alab, "rev-parse", "refs/remotes/origin/main"), top)
        for flag in ("--release-version", "--release-check", "--ledger-row"):
            with self.subTest(flag=flag):
                self.refused(self.package(flag, "--commit", top),
                             f"was released from {released[:12]}, which is not in the history "
                             "of origin/main")
        process = self.hook(PUB_SOURCE_COMMIT=top, PUB_VERSION="0.2.0")
        self.assertNotEqual(process.returncode, 0, process.stdout)
        self.assertIn("origin's main was rewritten after that release", process.stderr)

    def test_a_rewrite_this_clone_has_not_taken_is_refused_at_the_fetch(self):
        released, row = self.release_0_1_0()
        top = self.rewrite(row, land=False)
        # Someone else force-moves origin's main; this clone's origin/main still names the
        # release: the fetch is fast-forward only.
        self.git(self.alab, "push", "-q", "origin", f"{top}:refs/heads/rewritten")
        self.git(self.origin, "update-ref", "refs/heads/main", top)
        self.assertEqual(self.git(self.alab, "rev-parse", "refs/remotes/origin/main"), released)
        self.refused(self.package("--release-version", "--commit", top),
                     "fetching origin's main failed")

    def test_the_release_record_is_read_whole(self):
        released, row = self.release_0_1_0()
        nxt, _ = self.layer("0.2.0", ledger=HEADER + row + self.row("0.2.0", "0.164.0",
                                                                   date=self.date_of(row)))
        self.assertEqual(self.package("--release-version").stdout, "0.2.0+codex.0.164.0\n")
        snapshot = self.git(self.public, "rev-parse", "refs/heads/main")

        def public_main(tree_of, message):
            commit = self.git(self.alab, "commit-tree", f"{tree_of}^{{tree}}", "-m", message)
            self.git(self.alab, "push", "-q", "-f", str(self.public), f"{commit}:refs/heads/main")

        for name, tree_of, message, said in (
                # Public main is no snapshot naming its dev commit.
                ("not a snapshot", released, "a commit", "does not name the dev commit it was cut "
                 "from"),
                # A snapshot whose ledger is not its dev commit's.
                ("another ledger", nxt, f"snapshot: 2026-10-24 from codex-alab@{released}",
                 "other than its dev commit"),
                # A snapshot of a commit this clone has never had.
                ("unknown commit", nxt, f"snapshot: 2026-10-24 from codex-alab@{'e' * 40}",
                 f"was released from {'e' * 12}"),
        ):
            with self.subTest(name=name):
                public_main(tree_of, message)
                self.refused(self.package("--release-version"), said)
                self.git(self.alab, "push", "-q", "-f", str(self.public),
                         f"{snapshot}:refs/heads/main")
                self.assertEqual(self.package("--release-version").stdout,
                                 "0.2.0+codex.0.164.0\n")
        # A release tag is read as main is: one naming a commit off main is refused.
        stray = self.git(self.alab, "commit-tree", f"{nxt}^{{tree}}", "-m",
                         f"snapshot: 2026-10-24 from codex-alab@{'e' * 40}")
        self.git(self.alab, "push", "-q", str(self.public), f"{stray}:refs/tags/v0.0.9")
        self.refused(self.package("--release-version"), f"was released from {'e' * 12}")
        self.git(self.public, "tag", "-d", "v0.0.9")
        # The record cannot be read: nothing is released.
        env = dict(self.env, ALAB_RELEASED_REPO=f"file://{self.scratch / 'missing.git'}")
        process = subprocess.run(["/bin/bash", str(self.alab / "alab" / "package.sh"),
                                  "--release-version"], env=env, capture_output=True, text=True,
                                 timeout=120)
        self.refused(process, "cannot list the releases")

    def test_a_release_from_before_the_ledger_is_read_as_one(self):
        # Public v0.164.0 (2026-10-16, upstream's numbering) was cut from a dev commit with no
        # ledger: its snapshot carries none either, and it constrains no row.
        self.on(self.u3, "main-on-u3")
        legacy, _ = self.layer("0.164.0", ledger=None)
        self.publish(legacy, "0.164.0")
        self.layer("0.1.0")
        row = self.package("--ledger-row")
        self.assertEqual(row.returncode, 0, row.stderr)
        self.layer("0.1.0", ledger=HEADER + row.stdout)
        self.assertEqual(self.package("--release-version").stdout, "0.1.0+codex.0.164.0\n")

    def test_pre_promote_runs_the_source_commit_s_own_gate(self):
        self.on(self.u3, "main-on-u3")
        source, _ = self.layer(ledger=HEADER + self.row("0.1.0", "0.164.0"))
        process = self.hook(PUB_SOURCE_COMMIT=source, PUB_VERSION="0.1.0")
        self.assertEqual(process.returncode, 0, process.stderr)
        # The checkout the engine runs in sits at another commit whose gate differs (here a
        # versions.py that passes everything): the source commit's own gate must decide.
        (self.alab / "alab" / "versions.py").write_text(
            "import sys\nprint(sys.argv[sys.argv.index('--version') + 1] + '+codex.0.164.0')\n")
        self.git(self.alab, "add", "alab/versions.py")
        self.commit(self.alab, "another gate", "2026-10-23T15:00:00+0000")
        process = self.hook(PUB_SOURCE_COMMIT=source, PUB_VERSION="0.1.0")
        self.assertNotEqual(process.returncode, 0, process.stdout)
        self.assertIn(f"this checkout's release gate", process.stderr)
        self.assertIn(f"is not {source[:9]}'s", process.stderr)
        # The source commit checked out, its gate decides again.
        self.git(self.alab, "checkout", "-q", "--detach", source)
        process = self.hook(PUB_SOURCE_COMMIT=source, PUB_VERSION="0.1.0")
        self.assertEqual(process.returncode, 0, process.stderr)

    def test_pre_promote_reads_the_release_repository_the_engine_names(self):
        released, row = self.release_0_1_0()
        top = self.rewrite(row)
        # The engine's PUB_REPO_SLUG outranks ALAB_RELEASED_REPO. Its explicit
        # Agh reader serves this owned public fixture; a Git URL rewrite cannot
        # redirect the product's GitHub transport.
        empty = self.scratch / "empty.git"
        self.git(self.scratch, "init", "-q", "--bare", str(empty))
        reader = self.scratch / "fixture-agh"
        requests = self.scratch / "agh-requests.jsonl"
        reader.write_text(PYTHON_ENTRY + f'''
import json, pathlib, subprocess, sys
public = {str(self.public)!r}
requests = pathlib.Path({str(requests)!r})
args = sys.argv[1:]
with requests.open("a") as output:
    output.write(json.dumps(args) + "\\n")
if args == ["help", "repo", "fetch"]:
    print("--ref --into --input --anonymous")
    raise SystemExit(0)
assert args.pop(0) == "--anonymous", args
def git(*words):
    return subprocess.check_output(["git", "-c", "protocol.allow=never", "-c",
        "protocol.file.allow=always", *words], text=True).strip()
if args[0] == "api":
    assert args[1].startswith("/repos/alabsystems/codex-alab/"), args
    assert args[-1] == "--all", args
    prefix = "refs/" + args[1].split("/git/matching-refs/", 1)[1]
    rows = [{{"ref": ref, "object": {{"sha": oid}}}}
            for oid, ref in (line.split() for line in git("-C", public, "show-ref").splitlines())
            if ref.startswith(prefix)]
    print(json.dumps(rows))
else:
    assert args[:3] == ["repo", "fetch", "alabsystems/codex-alab"], args
    assert args[4] == "--input", args
    rows = json.loads(pathlib.Path(args[5]).read_text())
    assert all(git("-C", public, "rev-parse", r["source"]) == r["expected_oid"] for r in rows)
    git("-C", args[3], "fetch", "--no-tags", "--no-write-fetch-head", public,
        *["+" + r["source"] + ":" + r["into"] for r in rows])
    print("{{}}")
''')
        reader.chmod(0o700)
        process = self.hook(PUB_SOURCE_COMMIT=top, PUB_VERSION="0.2.0",
                            PUB_REPO_SLUG="alabsystems/codex-alab",
                            ALAB_RELEASED_REPO=f"file://{empty}", ALAB_AGH=str(reader))
        self.assertNotEqual(process.returncode, 0, process.stdout)
        self.assertIn(f"was released from {released[:12]}", process.stderr)
        calls = [json.loads(line) for line in requests.read_text().splitlines()]
        self.assertTrue(any(call[:2] == ["--anonymous", "api"] and call[-1] == "--all" for call in calls))
        self.assertTrue(any(call[:4] == ["--anonymous", "repo", "fetch", "alabsystems/codex-alab"]
                            and "--input" in call for call in calls))
        # A slug that is not owner/name is refused before anything is read.
        for slug in ("alabsystems", "a/b/c", "../x", "a/..", "a b/c"):
            with self.subTest(slug=slug):
                process = self.hook(PUB_SOURCE_COMMIT=top, PUB_VERSION="0.2.0",
                                    PUB_REPO_SLUG=slug)
                self.assertNotEqual(process.returncode, 0, process.stdout)
                self.assertIn("which is not owner/name", process.stderr)


class OriginMainTests(Scratch):
    """The ledger is checked along origin/main as origin has it now: fetched fast-forward
    only, never skipped when the remote-tracking ref is missing or stale, and a release is cut
    from it."""

    def two_baselines(self):
        """main records 0.2.0 on rust-v0.164.0; a side commit off the 0.1.0 commit merges
        rust-v0.165.0's branch point and records 0.2.0 on it."""
        self.on(self.u3, "main-on-u3")
        one = HEADER + self.row("0.1.0", "0.164.0")
        first, _ = self.layer("0.1.0", ledger=one, land=False)
        self.land(first, remember=True)
        second, _ = self.layer("0.2.0", ledger=one + self.row("0.2.0", "0.164.0",
                                                             date="2026-10-25"), land=False)
        self.land(second, remember=True)
        self.git(self.alab, "checkout", "-q", "-b", "side", first)
        self.git(self.alab, "merge", "-q", "--no-edit", self.u4)
        side, _ = self.layer("0.2.0", "0.166.0", ledger=one + self.row("0.2.0", "0.165.0",
                                                                      date="2026-10-25"),
                             land=False)
        return first, second, side

    def test_a_missing_or_stale_origin_main_is_fetched_first(self):
        first, second, side = self.two_baselines()
        for name, ref in (("missing", None), ("stale", first)):
            with self.subTest(name=name):
                if ref is None:
                    self.git(self.alab, "update-ref", "-d", "refs/remotes/origin/main")
                else:
                    self.git(self.alab, "update-ref", "refs/remotes/origin/main", ref)
                # One version on two baselines is never released: the side commit is not on
                # origin/main as origin has it.
                self.refused(self.package("--release-version", "--commit", side),
                             f"{side[:9]} is not on origin/main ({second[:9]})")
                self.assertEqual(self.git(self.alab, "rev-parse", "refs/remotes/origin/main"),
                                 second)

    def test_a_rewrite_this_clone_saw_is_refused(self):
        first, second, side = self.two_baselines()
        # origin's main is force-moved from `second` (0.2.0 on rust-v0.164.0, never released)
        # to the side commit (0.2.0 on rust-v0.165.0), and this clone takes it, as the
        # engine's forced fetch does: its reflog remembers `second`.
        self.land(side, remember=True)
        self.refused(self.package("--release-version", "--commit", side),
                     f"this clone saw refs/remotes/origin/main at {second[:12]}")
        # Once a person has decided, forgetting what this clone saw is the remedy it names.
        self.git(self.alab, "reflog", "expire", "--expire=now", "--expire-unreachable=now",
                 "refs/remotes/origin/main")
        self.assertEqual(self.package("--release-version", "--commit", side).stdout,
                         "0.2.0+codex.0.165.0\n")

    def test_no_origin_releases_nothing(self):
        self.on(self.u3, "main-on-u3")
        self.layer("0.1.0", ledger=HEADER + self.row("0.1.0", "0.164.0"))
        self.assertEqual(self.package("--release-version").stdout, "0.1.0+codex.0.164.0\n")
        self.git(self.alab, "remote", "remove", "origin")
        for flag in ("--release-version", "--release-check", "--ledger-row"):
            with self.subTest(flag=flag):
                self.refused(self.package(flag), "no 'origin' remote here")

    def test_the_checker_refuses_a_main_it_cannot_read(self):
        self.on(self.u3, "main-on-u3")
        source, _ = self.layer("0.1.0", ledger=HEADER + self.row("0.1.0", "0.164.0"))
        args = ["--repo", str(self.alab), "--commit", source, "--version", "0.1.0",
                "--baseline", "0.164.0", "--baseline-commit", self.tags["0.164.0"],
                "--released", f"file://{self.public}", "--main"]
        self.assertEqual(self.checker("check", *args, "refs/remotes/origin/main"), 0)
        self.assertNotEqual(self.checker("check", *args, "refs/remotes/origin/gone"), 0)
        # --main and --released are required: the gate never runs without them.
        self.assertNotEqual(self.checker("check", *args[:-3]), 0)

    def checker(self, *argv):
        process = subprocess.run([sys.executable, "-I", str(ROOT / "alab" / "versions.py"),
                                  *argv], env=self.env, capture_output=True, text=True,
                                 timeout=120)
        return process.returncode


class WholeHistoryTests(Scratch):
    """The history the gate reads is the one the repository holds: no shallow cut, grafts
    file or replacement object hides an edited row."""

    def edited(self):
        """0.1.0's row recorded, then edited to another baseline, then rust-v0.165.0's branch
        point merged and 0.2.0 appended on it."""
        self.on(self.u3, "main")
        original = HEADER + self.row("0.1.0", "0.164.0")
        first, _ = self.layer("0.1.0", ledger=original)
        edited = HEADER + self.row("0.1.0", "0.163.0")
        edit, _ = self.layer("0.1.0", ledger=edited)
        self.git(self.alab, "merge", "-q", "--no-edit", self.u4)
        top, _ = self.layer("0.2.0", "0.166.0", ledger=edited + self.row("0.2.0", "0.165.0",
                                                                         date="2026-10-25"))
        self.refused(self.package("--release-version"), "a row once recorded never changes")
        return first, edit, top

    def test_a_shallow_clone_cannot_hide_an_edited_row(self):
        _, _, top = self.edited()
        shallow = self.scratch / "shallow"
        self.git(self.scratch, "clone", "-q", "--depth", "3", "-b", "main",
                 f"file://{self.origin}", str(shallow))
        self.git(shallow, "remote", "add", "upstream", str(self.upstream))
        self.git(shallow, "config", "remote.upstream.tagOpt", "--no-tags")
        self.git(shallow, "config", "--unset-all", "remote.upstream.fetch")
        self.git(shallow, "config", "--add", "remote.upstream.fetch",
                 "refs/heads/main:refs/remotes/upstream/main")
        self.git(shallow, "config", "--add", "remote.upstream.fetch",
                 "+refs/tags/rust-v*:refs/upstream-tags/rust-v*")
        self.git(shallow, "fetch", "-q", "upstream")
        self.assertEqual(self.git(shallow, "rev-parse", "--is-shallow-repository"), "true")
        self.assertEqual(self.git(shallow, "rev-parse", "HEAD"), top)
        for flag in ("--release-version", "--ledger-row", "--version-only"):
            with self.subTest(flag=flag):
                process = subprocess.run(["/bin/bash", str(shallow / "alab" / "package.sh"),
                                          flag], env=self.env, capture_output=True, text=True,
                                         timeout=120)
                self.refused(process, "this clone is shallow")

    def test_a_shallow_clone_of_a_published_snapshot_cuts_nothing(self):
        # One parentless commit, as the engine publishes it: a depth-1 clone holds all of it.
        source, _ = self.layer(ledger=HEADER + self.row("0.1.0", "0.164.0"))
        clone = self.scratch / "snapshot"
        self.git(self.scratch, "clone", "-q", "--depth", "1", f"file://{self.origin}",
                 str(clone))
        self.git(clone, "remote", "add", "upstream", str(self.upstream))
        self.git(clone, "config", "--unset-all", "remote.upstream.fetch")
        self.git(clone, "config", "--add", "remote.upstream.fetch",
                 "refs/heads/main:refs/remotes/upstream/main")
        self.git(clone, "config", "--add", "remote.upstream.fetch",
                 "+refs/tags/rust-v*:refs/upstream-tags/rust-v*")
        self.assertEqual(self.git(clone, "rev-parse", "--is-shallow-repository"), "true")
        process = subprocess.run(["/bin/bash", str(clone / "alab" / "package.sh"),
                                  "--version-only"], env=self.env, capture_output=True,
                                 text=True, timeout=120)
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertEqual(process.stdout, f"0.1.0-dev.1+codex.0.164.0.g{source[:9]}\n")

    def test_a_grafts_file_is_refused(self):
        self.on(self.u3, "main")
        self.layer("0.1.0", ledger=HEADER + self.row("0.1.0", "0.164.0"))
        grafts = self.alab / ".git" / "info" / "grafts"
        grafts.parent.mkdir(exist_ok=True)
        grafts.write_text("")
        for flag in ("--release-version", "--version-only"):
            with self.subTest(flag=flag):
                self.refused(self.package(flag), "grafts file")

    def test_a_replacement_object_cannot_hide_an_edited_row(self):
        first, edit, top = self.edited()
        # Replace the commit that recorded 0.1.0 with one recording the edited row, so a
        # history read through replacements never held the original.
        tree = self.git(self.alab, "rev-parse", f"{edit}^{{tree}}")
        parent = self.git(self.alab, "rev-parse", f"{first}^")
        stand_in = self.git(self.alab, "commit-tree", tree, "-p", parent, "-m", "stand-in")
        self.git(self.alab, "replace", first, stand_in)
        # Read through the replacement, the ledger never held the original row.
        self.assertIn('baseline = "0.163.0"', self.git(self.alab, "show",
                                                         f"{first}:alab/versions.toml"))
        self.refused(self.package("--release-version"), "a row once recorded never changes")


class AppendOnlyTests(Scratch):
    """A row is only ever appended, by a commit carrying its version, built on its baseline,
    naming the commit its baseline's tag names; rows other than the release's own included."""

    def test_a_row_inserted_below_a_recorded_one_is_refused(self):
        self.on(self.u3, "main-on-u3")
        one = self.row("0.1.0", "0.164.0")
        three = self.row("0.3.0", "0.164.0")
        self.layer("0.1.0", ledger=HEADER + one)
        recorded, _ = self.layer("0.3.0", ledger=HEADER + one + three)
        self.assertEqual(self.package("--release-version").stdout, "0.3.0+codex.0.164.0\n")
        inserted = HEADER + one + self.row("0.2.0", "0.164.0") + three + self.row("0.4.0",
                                                                                 "0.164.0")
        self.layer("0.4.0", ledger=inserted)
        self.refused(self.package("--release-version"), "the ledger is append-only")
        # Across a deleted ledger: what was recorded before the deletion still counts.
        self.git(self.alab, "checkout", "-q", "-b", "deleted", recorded)
        self.layer("0.3.0", ledger=None)
        self.layer("0.4.0", ledger=inserted)
        self.refused(self.package("--release-version"), "the ledger is append-only")

    def test_a_row_inserted_through_a_merged_branch_is_refused(self):
        self.on(self.u3, "main-on-u3")
        one = self.row("0.1.0", "0.164.0")
        two = self.row("0.2.0", "0.164.0")
        three = self.row("0.3.0", "0.164.0")
        first, _ = self.layer("0.1.0", ledger=HEADER + one)
        self.layer("0.3.0", ledger=HEADER + one + three)
        # A branch forked before 0.3.0 was recorded appends 0.2.0 there, and main merges it.
        self.git(self.alab, "checkout", "-q", "-b", "late", first)
        branch, _ = self.layer("0.2.0", ledger=HEADER + one + two, land=False)
        self.git(self.alab, "checkout", "-q", "main-on-u3")
        self.git(self.alab, "merge", "-q", "--no-commit", "-s", "ours", branch)
        self.layer("0.4.0", ledger=HEADER + one + two + three + self.row("0.4.0", "0.164.0"))
        self.assertEqual(len(self.git(self.alab, "rev-parse", "HEAD^@").split()), 2)
        self.refused(self.package("--release-version"), "the ledger is append-only")

    def test_every_row_names_the_commit_its_baseline_s_tag_names(self):
        self.on(self.u3, "main-on-u3")
        for name, row in (
                ("another commit", self.row("0.1.0", "0.164.0", commit="0" * 40)),
                ("no such tag", self.row("0.1.0", "0.162.0", commit=self.tags["0.163.0"])),
        ):
            with self.subTest(name=name):
                self.git(self.alab, "checkout", "-q", "-b", f"rows-{name.replace(' ', '-')}",
                         self.u3)
                self.layer("0.1.0", ledger=HEADER + row)
                self.layer("0.2.0", ledger=HEADER + row + self.row("0.2.0", "0.164.0",
                                                                   date="2026-10-25"))
                self.refused(self.package("--release-version"),
                             "every row names the commit its baseline's tag names")

    def test_a_row_is_recorded_by_a_commit_carrying_its_version(self):
        self.on(self.u3, "main-on-u3")
        self.layer("0.2.0", ledger=HEADER + self.row("0.1.0", "0.164.0")
                   + self.row("0.2.0", "0.164.0"))
        self.refused(self.package("--release-version"),
                     "first records 0.1.0, but that commit's VERSION is 0.2.0")

    def test_a_row_names_a_baseline_its_commit_has_merged(self):
        # 0.1.0 recorded on rust-v0.165.0 by a commit that has not merged its branch point
        # (u4): main merges it afterwards and releases 0.2.0 on it.
        self.on(self.u3, "main-on-u3")
        early = HEADER + self.row("0.1.0", "0.165.0")
        self.layer("0.1.0", ledger=early)
        self.git(self.alab, "merge", "-q", "--no-edit", self.u4)
        self.layer("0.2.0", "0.166.0", ledger=early + self.row("0.2.0", "0.165.0",
                                                               date="2026-10-25"))
        self.refused(self.package("--release-version"),
                     "first records 0.1.0 on the upstream baseline 0.165.0")


class LedgerGrammarTests(unittest.TestCase):
    ROW = ('[[release]]\nversion = "0.1.0"\nbaseline = "0.164.0"\n'
           f'baseline_commit = "{"a" * 40}"\ndate = "2026-10-24"\n')

    def test_the_committed_ledger_reads_and_is_toml(self):
        text = (ROOT / "alab" / "versions.toml").read_text()
        rows = VERSIONS.parse(text)
        try:
            import tomllib
        except ImportError:
            return
        self.assertEqual(tomllib.loads(text), {"release": rows} if rows else {})

    def test_a_row_reads_exactly(self):
        self.assertEqual(VERSIONS.parse("# a comment\n\n" + self.ROW),
                         [{"version": "0.1.0", "baseline": "0.164.0",
                           "baseline_commit": "a" * 40, "date": "2026-10-24"}])
        self.assertEqual(VERSIONS.row_text(VERSIONS.parse(self.ROW)[0]), self.ROW)

    def test_anything_else_is_refused(self):
        for name, text in (
                ("no newline", self.ROW.rstrip("\n")),
                ("patch slot", self.ROW.replace('"0.1.0"', '"0.1.1"')),
                ("leading zero", self.ROW.replace('"0.1.0"', '"0.01.0"')),
                ("v prefix", self.ROW.replace('"0.164.0"', '"v0.164.0"')),
                ("two-part baseline", self.ROW.replace('"0.164.0"', '"0.164"')),
                ("pre-release baseline", self.ROW.replace('"0.164.0"', '"0.164.0-alpha.1"')),
                ("short commit", self.ROW.replace("a" * 40, "a" * 12)),
                ("bad date", self.ROW.replace("2026-10-24", "2026-02-30")),
                ("order", self.ROW.replace('version = "0.1.0"\nbaseline = "0.164.0"',
                                           'baseline = "0.164.0"\nversion = "0.1.0"')),
                ("missing field", self.ROW.replace('date = "2026-10-24"\n', "")),
                ("extra field", self.ROW + 'note = "x"\n'),
                ("trailing comment", self.ROW.replace('"2026-10-24"', '"2026-10-24" # x')),
                ("other table", "[release]\n" + self.ROW[len("[[release]]\n"):]),
                ("single quotes", self.ROW.replace('"0.1.0"', "'0.1.0'")),
                ("duplicate", self.ROW + "\n" + self.ROW),
                ("backward baseline", self.ROW + "\n"
                 + self.ROW.replace('"0.1.0"', '"0.2.0"').replace('"0.164.0"', '"0.163.9"')),
                ("earlier date", self.ROW + "\n"
                 + self.ROW.replace('"0.1.0"', '"0.2.0"').replace("2026-10-24", "2026-10-23")),
        ):
            with self.subTest(name=name):
                with self.assertRaises(VERSIONS.Refused):
                    VERSIONS.parse(text)


class BaselineRecordTests(unittest.TestCase):
    def write(self, version, client, baseline, release="0.164.0"):
        with tempfile.TemporaryDirectory(prefix="alab-baseline-record-") as directory:
            scratch = Path(directory)
            compiler = scratch / "trusted.json"
            compiler.write_text(json.dumps({"rustc": "rustc 1.0.0", "id": "0123456789abcdef"}))
            out = scratch / "alab-build.json"
            command = 'set -euo pipefail; . "$1/alab/package.sh"; shift; alab_build_json "$@"'
            env = {key: value for key, value in os.environ.items()
                   if not key.startswith("CODEX_ALAB_")}
            env.update(CODEX_ALAB_CLIENT_VERSION=client, CODEX_ALAB_BASELINE=baseline)
            if release:
                env.update(CODEX_ALAB_UPSTREAM_VERSION=release,
                           CODEX_ALAB_UPSTREAM_COMMIT="c" * 40)
            process = subprocess.run(
                ["/bin/bash", "-c", command, "manifest", str(ROOT), sys.executable, str(out),
                 str(compiler), "a" * 40, version, "aarch64-apple-darwin", "15.1.0", "+pcre2",
                 "146.4.0", "b" * 40, "2026-10-24T00:00:00Z"],
                env=env, capture_output=True, text=True, timeout=60)
            return process, (json.loads(out.read_text()) if out.exists() else None)

    def test_the_baseline_is_recorded_beside_the_versions(self):
        process, doc = self.write("0.1.0-dev.7+codex.0.164.0.gaaaaaaaaa",
                                  "0.165.0-dev.7+gaaaaaaaaa", "0.164.0")
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertEqual(list(doc)[:5], ["source", "version", "client_version", "baseline",
                                         "upstream"])
        self.assertEqual(doc["version"], "0.1.0-dev.7+codex.0.164.0.gaaaaaaaaa")
        self.assertEqual(doc["client_version"], "0.165.0-dev.7+gaaaaaaaaa")
        self.assertEqual(doc["baseline"], "0.164.0")
        self.assertEqual(doc["upstream"]["release"], "rust-v0.164.0")

    def test_records_that_disagree_on_the_baseline_are_refused(self):
        for version, client, baseline, release in (
                # The version names one baseline, the record another.
                ("0.1.0-dev.7+codex.0.164.0.gaaaaaaaaa", "0.165.0-dev.7+gaaaaaaaaa", "0.163.0",
                 "0.164.0"),
                # The upstream release compiled in is not the baseline.
                ("0.1.0-dev.7+codex.0.164.0.gaaaaaaaaa", "0.165.0-dev.7+gaaaaaaaaa", "0.164.0",
                 "0.163.0"),
                # A compound version with no baseline, and a baseline with no compound version.
                ("0.1.0-dev.7+codex.0.164.0.gaaaaaaaaa", "0.165.0-dev.7+gaaaaaaaaa", "",
                 "0.164.0"),
                ("0.1.0-dev.7+gaaaaaaaaa", "0.165.0-dev.7+gaaaaaaaaa", "0.164.0", "0.164.0"),
                # The client version stays the plain dev form: it is what OpenAI's servers see.
                ("0.1.0-dev.7+codex.0.164.0.gaaaaaaaaa",
                 "0.165.0-dev.7+codex.0.164.0.gaaaaaaaaa", "0.164.0", "0.164.0"),
                # Another build's count or commit.
                ("0.1.0-dev.7+codex.0.164.0.gaaaaaaaaa", "0.165.0-dev.8+gaaaaaaaaa", "0.164.0",
                 "0.164.0"),
                ("0.1.0-dev.7+codex.0.164.0.gaaaaaaaaa", "", "0.164.0", "0.164.0"),
                # Leading zeros in compound version, dev count, or upstream release are refused.
                ("01.1.0-dev.7+codex.0.164.0.gaaaaaaaaa", "0.165.0-dev.7+gaaaaaaaaa", "0.164.0",
                 "0.164.0"),
                ("0.01.0-dev.7+codex.0.164.0.gaaaaaaaaa", "0.165.0-dev.7+gaaaaaaaaa", "0.164.0",
                 "0.164.0"),
                ("0.1.0-dev.07+codex.0.164.0.gaaaaaaaaa", "0.165.0-dev.07+gaaaaaaaaa", "0.164.0",
                 "0.164.0"),
                ("0.1.0-dev.7+codex.0.164.0.gaaaaaaaaa", "0.165.0-dev.7+gaaaaaaaaa", "0.164.0",
                 "0.0164.0"),
        ):
            with self.subTest(version=version, client=client, baseline=baseline,
                              release=release):
                process, doc = self.write(version, client, baseline, release)
                self.assertNotEqual(process.returncode, 0, doc)
                self.assertIsNone(doc)


if __name__ == "__main__":
    unittest.main()

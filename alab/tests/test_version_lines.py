#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""codex-alab's two versions (alab/README.md, Decision 8): its own line, VERSION, restarted at
0.1.0, which a package says it is; and the client line, alab/client-version held to the
upstream floor, which upstream's code and OpenAI's servers see.

The real package.sh runs --version-only, --client-version-only and --release-check in a
scratch clone whose upstream remote is a scratch repository shaped like openai/codex (a
linear main, each release a commit off its branch point), as test_upstream_base.py builds it;
publish/pre-promote runs there as the publication engine runs it. alab_build_json runs as
package.sh defines it, sourced.
"""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class VersionLineTests(unittest.TestCase):
    def setUp(self):
        self.scratch = Path(tempfile.mkdtemp(prefix="alab-version-lines-"))
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
        # main: u1 - u2 - u3. rust-v0.163.0 branches at u1, rust-v0.164.0 at u2.
        self.u1 = self.commit(self.upstream, "u1", "2026-10-18T10:00:00+0000")
        self.u2 = self.commit(self.upstream, "u2", "2026-10-19T10:00:00+0000")
        self.u3 = self.commit(self.upstream, "u3", "2026-10-20T10:00:00+0000")
        for version, point in (("0.163.0", self.u1), ("0.164.0", self.u2)):
            self.git(self.upstream, "checkout", "-q", "--detach", point)
            self.commit(self.upstream, f"Release {version}", "2026-10-21T12:00:00+0000")
            self.git(self.upstream, "tag", f"rust-v{version}")
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

    def layer(self, version="0.1.0", client="0.165.0"):
        """ALab's tooling, VERSION and (unless None) alab/client-version, committed."""
        (self.alab / "alab").mkdir(exist_ok=True)
        for name in ("package.sh", "lib.sh", "command_guard.py", "github.py", "versions.py"):
            shutil.copy2(ROOT / "alab" / name, self.alab / "alab" / name)
        if version is None:
            (self.alab / "VERSION").unlink(missing_ok=True)
        else:
            (self.alab / "VERSION").write_text(version + "\n")
        if client is None:
            (self.alab / "alab" / "client-version").unlink(missing_ok=True)
        else:
            (self.alab / "alab" / "client-version").write_text(client + "\n")
        self.git(self.alab, "add", "-A", "alab", ".")
        source = self.commit(self.alab, "alab(layer): the tooling", "2026-10-21T13:00:00+0000")
        return source, self.git(self.alab, "rev-list", "--count", source)

    def package(self, *args):
        return subprocess.run(["/bin/bash", str(self.alab / "alab" / "package.sh"), *args],
                              env=self.env, capture_output=True, text=True, timeout=60)

    def on_u3(self):
        """main past rust-v0.164.0's branch point: the floor is 0.165.0."""
        self.git(self.alab, "checkout", "-q", "-b", "main-on-u3", self.u3)

    def test_the_package_says_codex_alab_s_own_line_and_tells_the_server_upstream_s(self):
        self.on_u3()
        source, count = self.layer("0.1.0", "0.165.0")
        version = self.package("--version-only")
        self.assertEqual(version.returncode, 0, version.stderr)
        self.assertEqual(version.stdout, f"0.1.0-dev.{count}+g{source[:9]}\n")
        self.assertIn("codex-alab's own release line", version.stderr)
        client = self.package("--client-version-only")
        self.assertEqual(client.returncode, 0, client.stderr)
        self.assertEqual(client.stdout, f"0.165.0-dev.{count}+g{source[:9]}\n")
        # A commit from before the version ledger (alab/versions.toml) keeps the version it
        # was packaged with, and releases nothing: a release of codex-alab's own line is
        # recorded in the ledger beside its upstream baseline (test_version_ledger.py).
        released = self.package("--release-check")
        self.assertNotEqual(released.returncode, 0, released.stdout)
        self.assertEqual(released.stdout, "")
        self.assertIn("carries no alab/versions.toml", released.stderr)

    def test_the_client_line_never_sorts_below_the_upstream_code_it_holds(self):
        self.on_u3()
        source, count = self.layer("0.1.0", "0.164.0")
        client = self.package("--client-version-only")
        self.assertEqual(client.returncode, 0, client.stderr)
        # Packaging takes the floor; the version keeps codex-alab's own line.
        self.assertEqual(client.stdout, f"0.165.0-dev.{count}+g{source[:9]}\n")
        self.assertIn("alab/client-version 0.164.0 is behind it", client.stderr)
        self.assertEqual(self.package("--version-only").stdout,
                         f"0.1.0-dev.{count}+g{source[:9]}\n")
        # A release is refused until alab/client-version reaches the floor; `pub bump`,
        # which moves VERSION, is not the remedy it names.
        released = self.package("--release-check")
        self.assertNotEqual(released.returncode, 0, released.stdout)
        self.assertEqual(released.stdout, "")
        self.assertIn("alab/client-version 0.164.0", released.stderr)
        self.assertIn("Set alab/client-version to 0.165.0", released.stderr)
        # A client line above the floor is kept as it is.
        source, count = self.layer("0.1.0", "0.170.0")
        self.assertEqual(self.package("--client-version-only").stdout,
                         f"0.170.0-dev.{count}+g{source[:9]}\n")

    def test_a_commit_from_before_codex_alab_s_own_line_keeps_its_one_version(self):
        self.on_u3()
        source, count = self.layer("0.164.0", None)
        version = self.package("--version-only")
        client = self.package("--client-version-only")
        self.assertEqual(version.returncode, 0, version.stderr)
        self.assertEqual(version.stdout, f"0.165.0-dev.{count}+g{source[:9]}\n")
        self.assertEqual(client.stdout, version.stdout)
        # Its release check is the rule it was cut under: VERSION held to the floor.
        released = self.package("--release-check")
        self.assertNotEqual(released.returncode, 0)
        self.assertIn("pub bump codex-alab", released.stderr)
        source, count = self.layer("0.165.0", None)
        released = self.package("--release-check")
        self.assertEqual(released.returncode, 0, released.stderr)
        self.assertEqual(released.stdout, "0.165.0\n")

    def test_malformed_or_missing_lines_are_refused(self):
        self.on_u3()
        for version, client, said in (("0.1.1", "0.165.0", "VERSION is not of the form"),
                                      ("01.1.0", "0.165.0", "VERSION is not of the form"),
                                      ("0.08.0", "0.165.0", "VERSION is not of the form"),
                                      ("0.1.0", "0.165.1", "alab/client-version is not of the form"),
                                      ("0.1.0", "v0.165.0", "alab/client-version is not of the form"),
                                      ("0.1.0", "01.165.0", "alab/client-version is not of the form"),
                                      ("0.1.0", "0.08.0", "alab/client-version is not of the form"),
                                      ("0.1.0", "0.0165.0", "alab/client-version is not of the form"),
                                      ("0.08.0", None, "VERSION is not of the form"),
                                      (None, "0.165.0", "no VERSION")):
            with self.subTest(version=version, client=client):
                self.layer(version, client)
                for flag in ("--version-only", "--client-version-only", "--release-check"):
                    process = self.package(flag)
                    self.assertNotEqual(process.returncode, 0, process.stdout)
                    self.assertEqual(process.stdout, "")
                    self.assertIn(said, process.stderr)

    def test_a_snapshot_takes_its_client_line_from_alab_client_version(self):
        # One commit and no upstream history, as the publication engine pushes it.
        source, count = self.layer("0.1.0", "0.165.0")
        self.assertEqual(count, "1")
        self.assertEqual(self.package("--version-only").stdout, f"0.1.0-dev.1+g{source[:9]}\n")
        client = self.package("--client-version-only")
        self.assertEqual(client.stdout, f"0.165.0-dev.1+g{source[:9]}\n")
        self.assertIn("a published snapshot", client.stderr)

    def hook(self, **env):
        """publish/pre-promote as the publication engine runs it."""
        copy = self.scratch / "engine" / "publish"
        copy.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / "publish" / "pre-promote", copy / "pre-promote")
        return subprocess.run([str(copy / "pre-promote")], cwd=self.alab,
                              env=dict(self.env, **env), stdin=subprocess.DEVNULL,
                              capture_output=True, text=True, timeout=60)

    def test_pre_promote_refuses_codex_alab_s_own_line_without_its_ledger(self):
        self.on_u3()
        source, _ = self.layer("0.1.0", "0.165.0")
        # A commit from before alab/versions.toml: its release is refused (the release that
        # passes, with its ledger row, is test_version_ledger.py's).
        process = self.hook(PUB_SOURCE_COMMIT=source, PUB_VERSION="0.1.0")
        self.assertNotEqual(process.returncode, 0, process.stdout)
        self.assertIn("carries no alab/versions.toml", process.stderr)
        stale, _ = self.layer("0.1.0", "0.164.0")
        process = self.hook(PUB_SOURCE_COMMIT=stale, PUB_VERSION="0.1.0")
        self.assertNotEqual(process.returncode, 0, process.stdout)
        self.assertIn("alab/client-version 0.164.0", process.stderr)


class ClientVersionRecordTests(unittest.TestCase):
    def write(self, version, client):
        with tempfile.TemporaryDirectory(prefix="alab-client-record-") as directory:
            scratch = Path(directory)
            compiler = scratch / "trusted.json"
            compiler.write_text(json.dumps({"rustc": "rustc 1.0.0", "id": "0123456789abcdef"}))
            out = scratch / "alab-build.json"
            command = 'set -euo pipefail; . "$1/alab/package.sh"; shift; alab_build_json "$@"'
            env = {key: value for key, value in os.environ.items()
                   if not key.startswith("CODEX_ALAB_")}
            if client is not None:
                env["CODEX_ALAB_CLIENT_VERSION"] = client
            process = subprocess.run(
                ["/bin/bash", "-c", command, "manifest", str(ROOT), sys.executable, str(out),
                 str(compiler), "a" * 40, version, "aarch64-apple-darwin", "15.1.0", "+pcre2",
                 "146.4.0", "b" * 40, "2026-10-24T00:00:00Z"],
                env=env, capture_output=True, text=True, timeout=60)
            return process, (json.loads(out.read_text()) if out.exists() else None)

    def test_the_client_version_is_recorded_beside_the_version(self):
        process, doc = self.write("0.1.0-dev.7+gaaaaaaaaa", "0.165.0-dev.7+gaaaaaaaaa")
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertEqual(doc["version"], "0.1.0-dev.7+gaaaaaaaaa")
        self.assertEqual(doc["client_version"], "0.165.0-dev.7+gaaaaaaaaa")
        self.assertEqual(list(doc)[:3], ["source", "version", "client_version"])

    def test_a_package_with_one_version_records_no_client_version(self):
        for client in (None, ""):
            with self.subTest(client=client):
                process, doc = self.write("0.165.0-dev.7+gaaaaaaaaa", client)
                self.assertEqual(process.returncode, 0, process.stderr)
                self.assertNotIn("client_version", doc)

    def test_a_client_version_of_another_build_is_refused(self):
        for client in ("0.165.0-dev.8+gaaaaaaaaa", "0.165.0-dev.7+gbbbbbbbbb", "0.165.0",
                       "0.165.1-dev.7+gaaaaaaaaa", "01.165.0-dev.7+gaaaaaaaaa",
                       "0.0165.0-dev.7+gaaaaaaaaa", "0.165.0-dev.07+gaaaaaaaaa"):
            with self.subTest(client=client):
                process, doc = self.write("0.1.0-dev.7+gaaaaaaaaa", client)
                self.assertNotEqual(process.returncode, 0)
                self.assertIsNone(doc)
        for version in ("01.1.0-dev.7+gaaaaaaaaa", "0.01.0-dev.7+gaaaaaaaaa",
                        "0.1.0-dev.07+gaaaaaaaaa"):
            with self.subTest(version=version):
                process, doc = self.write(version, "0.165.0-dev.7+gaaaaaaaaa")
                self.assertNotEqual(process.returncode, 0)
                self.assertIsNone(doc)


if __name__ == "__main__":
    unittest.main()

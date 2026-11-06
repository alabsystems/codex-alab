#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The upstream base package.sh publishes: alab-build.json's "upstream", the smoke's hold on
bin/codex, the release line, and publish/release-body, which puts that line on the source
release's GitHub page.

The manifest writer and the smoke's hold run as package.sh defines them, sourced. The
release line runs the real package.sh --upstream-base in a scratch clone whose upstream
remote is a scratch repository shaped like openai/codex: a linear main, and each release a
commit off its branch point; the hook runs there as the publication engine runs it.
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
COMMIT = "e8aa7c1678caa9a368243d72bf0e9b7859d97f15"
KEYS = ["source", "version", "target", "compiler", "ripgrep", "ripgrep_features", "rusty_v8",
        "packager", "built_at"]


def scrubbed(**values):
    """This process's environment without any upstream value, plus `values`."""
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith("CODEX_ALAB_UPSTREAM_")}
    environment.update(values)
    return environment


class ManifestTests(unittest.TestCase):
    def write(self, **upstream):
        """alab-build.json as package.sh writes it, given the upstream environment."""
        with tempfile.TemporaryDirectory(prefix="alab-manifest-") as directory:
            scratch = Path(directory)
            compiler = scratch / "trusted.json"
            compiler.write_text(json.dumps({"rustc": "rustc 1.0.0", "id": "0123456789abcdef"}))
            out = scratch / "alab-build.json"
            command = 'set -euo pipefail; . "$1/alab/package.sh"; shift; alab_build_json "$@"'
            process = subprocess.run(
                ["/bin/bash", "-c", command, "manifest", str(ROOT), sys.executable, str(out),
                 str(compiler), "a" * 40, "0.164.0-dev.1+gaaaaaaaaa", "aarch64-apple-darwin",
                 "15.1.0", "+pcre2", "146.4.0", "b" * 40, "2026-10-21T00:00:00Z"],
                env=scrubbed(**upstream), capture_output=True, text=True, timeout=60)
            text = out.read_text() if out.exists() else None
            return process, text

    def test_upstream_recorded_when_the_build_was_given_it(self):
        process, text = self.write(CODEX_ALAB_UPSTREAM_VERSION="0.163.0",
                                   CODEX_ALAB_UPSTREAM_COMMIT=COMMIT,
                                   CODEX_ALAB_UPSTREAM_SINCE="241",
                                   CODEX_ALAB_UPSTREAM_DATE="2026-10-20T13:50:13Z")
        self.assertEqual(process.returncode, 0, process.stderr)
        doc = json.loads(text)
        self.assertEqual(doc["upstream"], {"release": "rust-v0.163.0",
                                           "commits_since_release": 241, "commit": COMMIT,
                                           "commit_date": "2026-10-20T13:50:13Z"})
        # The order the release line reads it in: the release, the count, the commit, the day.
        self.assertEqual(list(doc["upstream"]),
                         ["release", "commits_since_release", "commit", "commit_date"])
        self.assertEqual([key for key in doc if key != "upstream"], KEYS)
        # package.sh finds a commit already packaged by this exact text.
        self.assertIn(f'"source": "{"a" * 40}"', text)

    def test_commit_date_is_optional(self):
        process, text = self.write(CODEX_ALAB_UPSTREAM_VERSION="0.163.0",
                                   CODEX_ALAB_UPSTREAM_COMMIT=COMMIT)
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertEqual(json.loads(text)["upstream"], {"release": "rust-v0.163.0", "commit": COMMIT})

    def test_the_count_is_optional_and_zero_is_a_count(self):
        for since, recorded in (("", None), ("0", 0), ("1", 1)):
            with self.subTest(since=since):
                process, text = self.write(CODEX_ALAB_UPSTREAM_VERSION="0.163.0",
                                           CODEX_ALAB_UPSTREAM_COMMIT=COMMIT,
                                           CODEX_ALAB_UPSTREAM_SINCE=since)
                self.assertEqual(process.returncode, 0, process.stderr)
                self.assertEqual(json.loads(text)["upstream"].get("commits_since_release"),
                                 recorded)

    def test_no_upstream_block_without_the_environment(self):
        for upstream in ({}, {"CODEX_ALAB_UPSTREAM_VERSION": "", "CODEX_ALAB_UPSTREAM_COMMIT": "",
                              "CODEX_ALAB_UPSTREAM_DATE": ""}):
            with self.subTest(upstream=upstream):
                process, text = self.write(**upstream)
                self.assertEqual(process.returncode, 0, process.stderr)
                doc = json.loads(text)
                self.assertNotIn("upstream", doc)
                self.assertEqual(list(doc), KEYS)

    def test_half_or_malformed_upstream_is_refused(self):
        refused = [
            {"CODEX_ALAB_UPSTREAM_COMMIT": COMMIT},
            {"CODEX_ALAB_UPSTREAM_VERSION": "0.163.0"},
            {"CODEX_ALAB_UPSTREAM_VERSION": "rust-v0.163.0", "CODEX_ALAB_UPSTREAM_COMMIT": COMMIT},
            {"CODEX_ALAB_UPSTREAM_VERSION": "0.163.0", "CODEX_ALAB_UPSTREAM_COMMIT": COMMIT[:39]},
            {"CODEX_ALAB_UPSTREAM_VERSION": "0.163.0", "CODEX_ALAB_UPSTREAM_COMMIT": COMMIT.upper()},
            {"CODEX_ALAB_UPSTREAM_VERSION": "0.163.0", "CODEX_ALAB_UPSTREAM_COMMIT": COMMIT,
             "CODEX_ALAB_UPSTREAM_DATE": "2026-10-20"},
            {"CODEX_ALAB_UPSTREAM_DATE": "2026-10-20T13:50:13Z"},
            {"CODEX_ALAB_UPSTREAM_SINCE": "241"},
            *({"CODEX_ALAB_UPSTREAM_VERSION": "0.163.0", "CODEX_ALAB_UPSTREAM_COMMIT": COMMIT,
               "CODEX_ALAB_UPSTREAM_SINCE": since} for since in ("-1", "241 ", "0241", "2.5", "x")),
        ]
        for upstream in refused:
            with self.subTest(upstream=upstream):
                process, text = self.write(**upstream)
                self.assertNotEqual(process.returncode, 0, text)
                self.assertIsNone(text)
                self.assertIn("alab-build.json: CODEX_ALAB_UPSTREAM_", process.stderr)


class SmokeTests(unittest.TestCase):
    """The smoke's hold on bin/codex, as package.sh defines it, sourced."""

    def setUp(self):
        self.scratch = Path(tempfile.mkdtemp(prefix="alab-upstream-smoke-"))
        self.addCleanup(shutil.rmtree, self.scratch, ignore_errors=True)
        self.env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        self.env.update(GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_NOSYSTEM="1",
                        GIT_AUTHOR_NAME="ALab", GIT_AUTHOR_EMAIL="alab@example.invalid",
                        GIT_COMMITTER_NAME="ALab", GIT_COMMITTER_EMAIL="alab@example.invalid")
        self.repo = self.scratch / "codex-alab"
        self.git("init", "-q", "-b", "main", str(self.repo))
        # Before alab-core's version.rs: the ALab code reads no upstream commit.
        (self.repo / "codex-rs" / "alab-core" / "src").mkdir(parents=True)
        (self.repo / "codex-rs" / "alab-core" / "src" / "lib.rs").write_text("pub fn active() {}\n")
        self.git("-C", str(self.repo), "add", "codex-rs")
        self.git("-C", str(self.repo), "commit", "-q", "-m", "alab-core")
        self.before = self.git("-C", str(self.repo), "rev-parse", "HEAD")
        (self.repo / "codex-rs" / "alab-core" / "src" / "version.rs").write_text(
            'pub fn upstream_commit() -> Option<&\'static str> {\n'
            '    option_env!("CODEX_ALAB_UPSTREAM_COMMIT")\n}\n')
        self.git("-C", str(self.repo), "add", "codex-rs")
        self.git("-C", str(self.repo), "commit", "-q", "-m", "alab-core: version.rs")
        self.after = self.git("-C", str(self.repo), "rev-parse", "HEAD")
        self.carries = self.scratch / "carries"
        self.carries.write_bytes(b"\xcf\xfa\xed\xfe\x00" + COMMIT.encode() + b"\x00\x01")
        self.lacks = self.scratch / "lacks"
        self.lacks.write_bytes(b"\xcf\xfa\xed\xfe\x00" + COMMIT[:12].encode() + b"\x00\x01")

    def git(self, *args):
        process = subprocess.run(["git", *args], env=self.env, capture_output=True, text=True,
                                 timeout=60)
        self.assertEqual(process.returncode, 0, process.stderr)
        return process.stdout.strip()

    def carried(self, commit, binary):
        command = 'set -euo pipefail; . "$1/alab/package.sh"; shift; alab_upstream_carried "$@"'
        return subprocess.run(["/bin/bash", "-c", command, "smoke", str(ROOT), str(self.repo),
                               commit, COMMIT, str(binary)], env=self.env,
                              capture_output=True, text=True, timeout=60)

    def test_a_binary_carrying_the_commit_its_source_compiles_in_passes(self):
        process = self.carried(self.after, self.carries)
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertEqual(process.stdout, f"smoke: bin/codex carries upstream {COMMIT[:12]}\n")

    def test_a_binary_without_the_commit_its_source_compiles_in_is_refused(self):
        process = self.carried(self.after, self.lacks)
        self.assertEqual(process.returncode, 1, process.stdout)
        self.assertIn(f"does not carry the upstream commit {COMMIT}", process.stderr)

    def test_a_commit_from_before_version_rs_is_held_to_no_commit(self):
        # Packaged again (--commit, --rebuild): its source compiles no upstream commit in.
        process = self.carried(self.before, self.lacks)
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertIn("reads no CODEX_ALAB_UPSTREAM_COMMIT", process.stdout)

    def test_an_unreadable_source_is_refused(self):
        process = self.carried("f" * 40, self.carries)
        self.assertEqual(process.returncode, 1, process.stdout)
        self.assertIn("cannot read", process.stderr)


class ReleaseLineTests(unittest.TestCase):
    """package.sh --upstream-base against a scratch upstream."""

    def setUp(self):
        self.scratch = Path(tempfile.mkdtemp(prefix="alab-upstream-base-"))
        self.addCleanup(shutil.rmtree, self.scratch, ignore_errors=True)
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith(("GIT_", "ALAB_", "CODEX_ALAB_"))}
        self.env.update(GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_NOSYSTEM="1",
                        HOME=str(self.scratch), GIT_AUTHOR_NAME="Upstream",
                        GIT_AUTHOR_EMAIL="upstream@example.invalid", GIT_COMMITTER_NAME="Upstream",
                        GIT_COMMITTER_EMAIL="upstream@example.invalid")
        self.upstream = self.scratch / "codex"
        self.git(self.scratch, "init", "-q", "-b", "main", str(self.upstream))
        # main: u1 - u2 - u3 - u4. rust-v0.162.0 branches at u1, rust-v0.163.0 at u2, an
        # alpha of 0.164.0 at u3 (a pre-release, never a base). u3 is committed at 20:30 on
        # 2026-10-20 in UTC-7, which is 2026-10-21 in UTC.
        self.u1 = self.commit(self.upstream, "u1", "2026-10-18T10:00:00+0000")
        self.u2 = self.commit(self.upstream, "u2", "2026-10-19T10:00:00+0000")
        self.u3 = self.commit(self.upstream, "u3", "2026-10-20T20:30:00-0700")
        self.u4 = self.commit(self.upstream, "u4", "2026-10-21T10:00:00+0000")
        for version, point in (("0.162.0", self.u1), ("0.163.0", self.u2),
                               ("0.164.0-alpha.1", self.u3)):
            self.git(self.upstream, "checkout", "-q", "--detach", point)
            self.commit(self.upstream, f"Release {version}", "2026-10-21T12:00:00+0000")
            self.git(self.upstream, "tag", f"rust-v{version}")
        self.git(self.upstream, "checkout", "-q", "main")
        # The ALab clone, set up as alab/setup.sh does: main and the stable tags fetch only.
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

    def layer(self):
        """ALab's tooling and VERSION, committed on the clone's current branch."""
        (self.alab / "alab").mkdir(exist_ok=True)
        for name in ("package.sh", "lib.sh", "command_guard.py", "github.py"):
            shutil.copy2(ROOT / "alab" / name, self.alab / "alab" / name)
        (self.alab / "VERSION").write_text("0.164.0\n")
        self.git(self.alab, "add", "alab", "VERSION")
        return self.commit(self.alab, "alab(layer): the tooling", "2026-10-21T13:00:00+0000")

    def base(self, *args):
        return subprocess.run(["/bin/bash", str(self.alab / "alab" / "package.sh"),
                               "--upstream-base", *args], env=self.env, capture_output=True,
                              text=True, timeout=60)

    def test_names_the_newest_merged_stable_and_the_commits_past_it(self):
        self.git(self.alab, "checkout", "-q", "-b", "main-on-u3", self.u3)
        self.layer()
        process = self.base()
        self.assertEqual(process.returncode, 0, process.stderr)
        # The alpha at u3 is no base; the date is the merged commit's, in UTC.
        line = f"Based on upstream openai/codex rust-v0.163.0 + 1 commit ({self.u3[:12]}, 2026-10-21)"
        self.assertEqual(process.stdout, line + "\n")
        self.assertIn(f"alab:     {line}\n", process.stderr)

        # The next sync merges u4: the same stable, one commit further.
        env = dict(self.env, GIT_COMMITTER_DATE="2026-10-21T14:00:00+0000")
        merge = subprocess.run(["git", "-C", str(self.alab), "merge", "-q", "--no-edit", self.u4],
                               env=env, capture_output=True, text=True, timeout=60)
        self.assertEqual(merge.returncode, 0, merge.stderr)
        process = self.base()
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertEqual(process.stdout, "Based on upstream openai/codex rust-v0.163.0 + 2 commits "
                                         f"({self.u4[:12]}, 2026-10-21)\n")

    def test_names_an_older_stable_at_its_branch_point(self):
        self.git(self.alab, "checkout", "-q", "-b", "main-on-u1", self.u1)
        self.layer()
        process = self.base()
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertEqual(process.stdout, "Based on upstream openai/codex rust-v0.162.0 + 0 commits "
                                         f"({self.u1[:12]}, 2026-10-18)\n")

    def test_another_commit_names_its_own_base(self):
        self.git(self.alab, "checkout", "-q", "-b", "main-on-u1", self.u1)
        older = self.layer()
        self.git(self.alab, "checkout", "-q", "-b", "main-on-u4", self.u4)
        self.layer()
        process = self.base("--commit", older)
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertTrue(process.stdout.startswith("Based on upstream openai/codex rust-v0.162.0 "),
                        process.stdout)

    def hook(self, **env):
        """publish/release-body as the publication engine runs it: a copy of the committed hook
        outside the clone, from the clone's root."""
        copy = self.scratch / "engine" / "publish"
        copy.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / "publish" / "release-body", copy / "release-body")
        return subprocess.run([str(copy / "release-body")], cwd=self.alab,
                              env=dict(self.env, **env), stdin=subprocess.DEVNULL,
                              capture_output=True, text=True, timeout=60)

    def test_the_release_body_hook_prints_the_release_line(self):
        self.git(self.alab, "checkout", "-q", "-b", "main-on-u3", self.u3)
        source = self.layer()
        process = self.hook(PUB_SOURCE_COMMIT=source, PUB_VERSION="0.164.0")
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertEqual(process.stdout, "Based on upstream openai/codex rust-v0.163.0 + 1 commit "
                                         f"({self.u3[:12]}, 2026-10-21)\n")
        self.assertEqual(process.stdout, self.base("--commit", source).stdout)

    def test_the_release_body_hook_names_the_published_commit_s_base(self):
        # The checkout has moved on to u4; the release is of the commit on u1.
        self.git(self.alab, "checkout", "-q", "-b", "main-on-u1", self.u1)
        published = self.layer()
        self.git(self.alab, "checkout", "-q", "-b", "main-on-u4", self.u4)
        self.layer()
        process = self.hook(PUB_SOURCE_COMMIT=published)
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertEqual(process.stdout, "Based on upstream openai/codex rust-v0.162.0 + 0 commits "
                                         f"({self.u1[:12]}, 2026-10-18)\n")

    def test_the_release_body_hook_refuses_a_commit_with_no_base_and_no_commit(self):
        source = self.layer()  # a snapshot: one commit, no upstream history
        process = self.hook(PUB_SOURCE_COMMIT=source)
        self.assertNotEqual(process.returncode, 0, process.stdout)
        self.assertEqual(process.stdout, "")
        self.assertIn("names no upstream base", process.stderr)
        for env in ({}, {"PUB_SOURCE_COMMIT": ""}):
            with self.subTest(env=env):
                process = self.hook(**env)
                self.assertNotEqual(process.returncode, 0, process.stdout)
                self.assertEqual(process.stdout, "")
                self.assertIn("the engine names the source commit", process.stderr)
        process = self.hook(PUB_SOURCE_COMMIT="f" * 40)
        self.assertNotEqual(process.returncode, 0, process.stdout)
        self.assertEqual(process.stdout, "")

    def test_a_snapshot_names_no_base(self):
        # One commit and no upstream history, as the publication engine pushes: the root of
        # the clone's still unborn main.
        self.layer()
        process = self.base()
        self.assertNotEqual(process.returncode, 0, process.stdout)
        self.assertEqual(process.stdout, "")
        self.assertIn("names no upstream base", process.stderr)


if __name__ == "__main__":
    unittest.main()

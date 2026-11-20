#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Native request boundaries plus real local Git identity/race fixtures."""
import importlib.util
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("alab_github_test", ROOT / "alab/github.py")
GITHUB = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GITHUB)


def git(root, *args):
    return GITHUB.git(root, *args).decode().strip()


def disk_fixture_cli(tool, free_gib):
    """Set capacity for a fixture subprocess that runs the production CLI."""
    return ("import os, runpy\nfrom types import SimpleNamespace\n"
            f"os.statvfs = lambda path: SimpleNamespace(f_bavail={free_gib}, f_frsize=2**30)\n"
            f"runpy.run_path({str(tool)!r}, run_name='__main__')\n")


class NativeFixture:
    def __init__(self, remote):
        self.remote = remote
        self.requests = []
        self.after_fetch = None

    def api(self, path, **kwargs):
        self.requests.append((path, kwargs))
        if "/git/matching-refs/" not in path:
            return {"full_name": "fixture/codex"}
        prefix = "refs/" + path.split("/git/matching-refs/", 1)[1]
        return [{"ref": ref, "object": {"sha": oid}}
                for oid, ref in (line.split() for line in git(self.remote, "show-ref").splitlines())
                if ref.startswith(prefix)]

    def fetch_batch(self, identity, destination, rows, **kwargs):
        self.requests.append((("repo", "fetch", identity, rows), kwargs))
        assert identity == "fixture/codex", identity
        for row in rows:
            if git(self.remote, "rev-parse", row["source"]) != row["expected_oid"]:
                raise GITHUB.Refused("fixture source differs from its enumerated object ID")
        if self.after_fetch:
            callback, self.after_fetch = self.after_fetch, None
        else:
            callback = None
        git(destination, "fetch", "--no-tags", "--no-write-fetch-head", str(self.remote),
            *["+" + row["source"] + ":" + row["into"] for row in rows])
        if callback:
            callback(destination, rows[0]["into"])
        return b"{}"


class GithubTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="codex-agh-")
        self.addCleanup(self.scratch.cleanup)
        self.home = Path(self.scratch.name)
        self.repo, self.remote = self.home / "source with spaces", self.home / "remote.git"
        self.repo.mkdir()
        git(self.repo, "init", "-q", "-b", "main")
        git(self.repo, "config", "user.name", "Fixture")
        git(self.repo, "config", "user.email", "fixture@example.invalid")
        git(self.repo, "commit", "-q", "--allow-empty", "-m", "root")
        self.head = git(self.repo, "rev-parse", "HEAD")
        git(self.repo, "tag", "-a", "rust-v0.1.0", "-m", "raw tag object")
        self.tag = git(self.repo, "rev-parse", "refs/tags/rust-v0.1.0")
        self.remote.mkdir()
        git(self.remote, "init", "-q", "--bare", "-b", "main")
        git(self.repo, "push", "-q", str(self.remote), "HEAD:refs/heads/main", "refs/tags/rust-v0.1.0")
        self.client = NativeFixture(self.remote)
        self.specs = ["refs/heads/main:refs/remotes/upstream/main",
                      "+refs/tags/rust-v*:refs/upstream-tags/rust-v*"]

    def test_explicit_local_fixture_and_native_reads_preserve_raw_tag_and_source(self):
        with mock.patch.object(GITHUB, "Agh", side_effect=AssertionError("no network fallback")):
            GITHUB.fetch(self.repo, self.remote.as_uri(), self.specs)
        self.assertEqual(git(self.repo, "rev-parse", "refs/upstream-tags/rust-v0.1.0"), self.tag)
        self.assertEqual(git(self.repo, "rev-parse", "HEAD"), self.head)
        (self.repo / "origin").mkdir()
        git(self.repo, "remote", "add", "origin", "https://github.com/fixture/codex.git")
        GITHUB.fetch(self.repo, "origin", ["refs/heads/main:refs/remotes/origin/main"],
                     anonymous=True, client=self.client)
        self.assertEqual(git(self.repo, "rev-parse", "refs/remotes/origin/main"), self.head)
        self.assertEqual(git(self.repo, "rev-parse", "HEAD"), self.head)
        self.assertTrue(all(kwargs["anonymous"] for _, kwargs in self.client.requests))
        self.assertTrue(all(kwargs["all_pages"] for args, kwargs in self.client.requests if isinstance(args, str)))
        calls = len(self.client.requests)
        GITHUB.fetch(self.repo, "origin", ["refs/heads/main:refs/remotes/origin/main"],
                     anonymous=True, client=self.client)
        self.assertEqual(len(self.client.requests), calls + 1, "unchanged OIDs must not refetch the graph")

    def test_local_release_batch_rechecks_warm_named_refs_and_recovers_without_another_fetch(self):
        bare = self.home / "release snapshot.git"
        git(self.home, "init", "--bare", "-q", str(bare))
        main, tag = "refs/heads/main", "refs/tags/v0.1.0"
        git(bare, "fetch", "--no-tags", str(self.remote), main + ":" + main)
        git(self.repo, "commit", "-q", "--allow-empty", "-m", "release main")
        git(self.repo, "tag", "-a", "v0.1.0", "-m", "release annotation")
        git(self.repo, "push", "-q", str(self.remote), main, tag)
        url = self.remote.as_uri()
        observed = GITHUB.list_refs(bare, url, [main, tag], anonymous=True)
        raw_tag = git(self.repo, "rev-parse", tag)
        self.assertNotEqual(raw_tag, observed[main])
        snapshots = [ref + ":" + ref for ref in (main, tag)]
        aliases = [main + ":refs/remotes/prewarmed/main", tag + ":refs/upstream-tags/v0.1.0"]
        writers = []
        popen = subprocess.Popen

        def observe(argv, *args, **options):
            if "fetch" in argv:
                writers.append(argv)
            return popen(argv, *args, **options)

        with mock.patch.object(subprocess, "Popen", side_effect=observe):
            GITHUB.fetch(bare, url, aliases, snapshot=observed, anonymous=True)
            initial_writers = len(writers)
            self.assertEqual(git(bare, "rev-parse", "refs/upstream-tags/v0.1.0"), raw_tag)
            public = {ref: oid for ref, oid in GITHUB.local_refs(bare).items()
                      if not ref.startswith("refs/agh/")}

            git(self.repo, "commit", "-q", "--allow-empty", "-m", "move main after enumeration")
            advanced = git(self.repo, "rev-parse", "HEAD")
            git(self.repo, "push", "-q", str(self.remote), main)
            # Both slots still match the supplied snapshot. Only another named
            # transfer can expose the movement; no public ref may change on failure.
            with self.assertRaisesRegex(GITHUB.Refused, "enumerated object ID"):
                GITHUB.fetch(bare, url, snapshots, snapshot=observed, snapshot_refs=True, anonymous=True)
            after_mismatch = len(writers)
            self.assertEqual({ref: oid for ref, oid in GITHUB.local_refs(bare).items()
                              if not ref.startswith("refs/agh/")}, public)

            # A fresh advertisement now agrees with both imported objects.
            no_writer = mock.Mock(side_effect=AssertionError("cache-only recovery launched a transfer"))
            recovered = GITHUB.fetch(bare, url, snapshots, snapshot_refs=True, anonymous=True,
                                     admit_transfer=no_writer)
            no_writer.assert_not_called()
        self.assertEqual(recovered, {main: advanced, tag: raw_tag})
        self.assertEqual(git(bare, "rev-parse", main), advanced)
        self.assertEqual(git(bare, "rev-parse", tag), raw_tag)
        self.assertEqual((initial_writers, after_mismatch - initial_writers, len(writers) - after_mismatch),
                         (1, 1, 0))

    def test_unavailable_local_ref_preserves_every_warm_slot_and_public_ref(self):
        url = self.remote.as_uri()
        main, tag = "refs/heads/main", "refs/tags/rust-v0.1.0"
        GITHUB.fetch(self.repo, url, self.specs)
        git(self.repo, "commit", "-q", "--allow-empty", "-m", "main available for transfer")
        git(self.repo, "push", "-q", str(self.remote), main)
        observed = GITHUB.list_refs(self.repo, url, [main, tag])
        git(self.remote, "config", "uploadpack.hideRefs", tag)
        self.assertEqual(GITHUB.list_refs(self.repo, url, [main, tag]), {main: observed[main]})
        before = GITHUB.local_refs(self.repo)
        self.assertEqual({oid for ref, oid in before.items() if ref.startswith("refs/agh/")},
                         {self.head, self.tag})
        # The first source is still available. The second vanished after its
        # listing, and an alias still needs it even though its slot is warm.
        with self.assertRaisesRegex(GITHUB.Refused, "local Git fetch failed"):
            GITHUB.fetch(self.repo, url, [self.specs[0], tag + ":refs/upstream-tags/alias"],
                         snapshot=observed)
        self.assertEqual(GITHUB.local_refs(self.repo), before)

    def test_non_fast_forward_main_is_refused_without_losing_its_reflog(self):
        GITHUB.fetch(self.repo, str(self.remote), self.specs)
        before = git(self.repo, "reflog", "show", "--format=%H", "refs/remotes/upstream/main")
        other = git(self.repo, "commit-tree", self.head + "^{tree}", "-m", "unrelated root")
        git(self.repo, "push", "-q", "--force", str(self.remote), other + ":refs/heads/main")
        with self.assertRaises(GITHUB.Refused):
            GITHUB.fetch(self.repo, "fixture/codex", self.specs, client=self.client)
        self.assertEqual(git(self.repo, "rev-parse", "refs/remotes/upstream/main"), self.head)
        self.assertEqual(git(self.repo, "reflog", "show", "--format=%H", "refs/remotes/upstream/main"), before)

    def test_one_native_batch_binds_all_sources_and_reuses_exact_prewarmed_objects(self):
        GITHUB.fetch(self.repo, "fixture/codex", self.specs, anonymous=True, client=self.client)
        batches = [args[3] for args, _ in self.client.requests if isinstance(args, tuple)]
        self.assertEqual(len(batches), 1)
        self.assertEqual({row["source"]: row["expected_oid"] for row in batches[0]},
                         {self.head: self.head, "refs/tags/rust-v0.1.0": self.tag})
        for destination in ("refs/remotes/upstream/main", "refs/upstream-tags/rust-v0.1.0"):
            git(self.repo, "update-ref", "-d", destination)
        GITHUB.fetch(self.repo, "fixture/codex", self.specs, anonymous=True, client=self.client)
        self.assertEqual(sum(isinstance(args, tuple) for args, _ in self.client.requests), 1)
        self.assertEqual(git(self.repo, "rev-parse", "refs/upstream-tags/rust-v0.1.0"), self.tag)
        self.assertEqual(sum(isinstance(args, str) for args, _ in self.client.requests), 4,
                         "every canonical run must enumerate both prefixes again")

    def test_linked_worktree_uses_shared_refs_and_packed_ref_identities(self):
        GITHUB.fetch(self.repo, "fixture/codex", self.specs, client=self.client)
        git(self.repo, "pack-refs", "--all")
        linked = self.home / "linked source"
        git(self.repo, "worktree", "add", "--detach", str(linked), self.head)
        GITHUB.fetch(linked, "fixture/codex", self.specs, client=self.client)
        self.assertEqual(git(linked, "rev-parse", "refs/upstream-tags/rust-v0.1.0"), self.tag)
        self.assertEqual(git(linked, "rev-parse", "HEAD"), self.head)
        self.assertEqual(sum(isinstance(args, tuple) for args, _ in self.client.requests), 1)

    def test_large_current_snapshot_uses_bounded_git_calls_without_changing_refs(self):
        # Match the release's real tag scale, with every object already local.
        # Fresh remote enumeration remains required; per-tag Git processes do not.
        tags = {"refs/tags/rust-vfixture" + str(at): self.head for at in range(1490)}
        GITHUB.git(self.remote, "update-ref", "--stdin", data=("".join(
            "update " + ref + " " + oid + "\n" for ref, oid in tags.items())).encode())
        tags["refs/tags/rust-v0.1.0"] = self.tag
        destinations = {"refs/upstream-tags/" + ref.removeprefix("refs/tags/"): oid
                        for ref, oid in tags.items()}
        destinations["refs/remotes/upstream/main"] = self.head
        GITHUB.git(self.repo, "update-ref", "--stdin", data=("".join(
            "update " + ref + " " + oid + "\n" for ref, oid in destinations.items())).encode())
        before = GITHUB.local_refs(self.repo)
        with mock.patch.object(GITHUB, "git", wraps=GITHUB.git) as calls:
            GITHUB.fetch(self.repo, "fixture/codex", self.specs, anonymous=True, client=self.client)
        self.assertLess(len(calls.call_args_list), 20,
                        "the warm release fetch must not spawn a process for every tag")
        self.assertEqual(GITHUB.local_refs(self.repo), before)
        self.assertFalse(any(isinstance(args, tuple) for args, _ in self.client.requests))
        self.assertEqual(sum(isinstance(args, str) for args, _ in self.client.requests), 2)

    def test_explicit_release_catalog_validates_once_and_checks_bare_ownership(self):
        refs = {f"refs/tags/v1.0.{n}": self.head for n in range(16)}
        refs["refs/tags/rust-v0.1.0"] = self.tag
        GITHUB.git(self.remote, "update-ref", "--stdin", data="".join(
            f"update {ref} {oid}\n" for ref, oid in refs.items()).encode())
        bare = self.home / "local release snapshot.git"
        git(self.home, "clone", "--bare", str(self.remote), str(bare))
        specs = [f"{ref}:{ref}" for ref in refs]
        original = GITHUB.local_refs(bare)
        for supplied in (refs, None):
            with self.subTest(supplied=supplied is not None):
                with mock.patch.object(GITHUB, "git", wraps=GITHUB.git) as calls:
                    observed = GITHUB.fetch(bare, str(self.remote), specs, snapshot_refs=True,
                                            snapshot=supplied)
                self.assertEqual(observed, refs)
                self.assertEqual(GITHUB.local_refs(bare), original)
                commands = [call.args[1:] for call in calls.call_args_list]
                self.assertEqual(commands.count(("rev-parse", "--is-bare-repository")), 1)
                self.assertLess(len(commands), 20)
        with mock.patch.object(GITHUB, "list_refs", side_effect=AssertionError("unexpected enumeration")):
            with self.assertRaisesRegex(GITHUB.Refused, "owned bare snapshot"):
                GITHUB.fetch(self.repo, str(self.remote), specs, snapshot_refs=True)

    def test_invalid_literal_in_a_catalog_is_refused_before_observing_the_remote(self):
        original = GITHUB.local_refs(self.repo)
        for invalid in ("refs/heads/bad..name:refs/remotes/upstream/other",
                        "refs/heads/main:refs/remotes/upstream/bad..name"):
            with self.subTest(invalid=invalid), mock.patch.object(
                    GITHUB, "list_refs", side_effect=AssertionError("unexpected enumeration")):
                with self.assertRaises(GITHUB.Refused):
                    GITHUB.fetch(self.repo, "fixture/codex", [self.specs[0], invalid], client=self.client)
        self.assertEqual(GITHUB.local_refs(self.repo), original)
        self.assertEqual(self.client.requests, [])

    def test_empty_explicit_catalog_can_be_listed_outside_a_repository(self):
        plain = self.home / "outside a repository"
        plain.mkdir()
        patterns = ["refs/tags/absent-one", "refs/heads/absent-two"]
        self.assertEqual(GITHUB.list_refs(plain, str(self.remote), patterns), {})

    @unittest.skipUnless(os.name == "posix", "POSIX argv supports undecodable ref bytes")
    def test_unmatched_literal_can_contain_an_undecodable_filesystem_byte(self):
        opaque = os.fsdecode(b"refs/tags/absent-\xff")
        self.assertEqual(GITHUB.list_refs(self.repo, str(self.remote), ["refs/heads/missing", opaque]), {})

    def test_aborted_ref_batch_matches_git_parser_and_leaves_metadata_untouched(self):
        before = GITHUB.local_refs(self.repo)
        reflogs = git(self.repo, "reflog", "show", "--all", "--format=%H %gs")
        for ref in ("refs/heads/main", "refs/tags/valid", "refs/tags/ümlaut",
                    "refs/tags/a..b", "refs/tags/foo.lock", "refs/tags/foo.",
                    "refs/tags/.hidden", "refs/tags/a//b", "refs/tags/a@{b",
                    "refs/tags/a b", "refs/tags/a?b", "refs/tags/a[b",
                    "refs/tags/a\\b", "refs/tags/newline\nabort\ncommit",
                    "refs/tags/tab\tname", "refs/tags/control\x01name"):
            valid = subprocess.run(["git", "check-ref-format", ref],
                                   capture_output=True).returncode == 0
            with self.subTest(ref=ref):
                if valid:
                    GITHUB.validate_full_refs(self.repo, [ref])
                else:
                    with self.assertRaises(GITHUB.Refused):
                        GITHUB.validate_full_refs(self.repo, ["refs/tags/valid", ref])
        for ref in ("HEAD", "refs/tags/nul\0abort\0", "refs/tags/surrogate\ud800", None):
            with self.subTest(ref=ref), self.assertRaises(GITHUB.Refused):
                GITHUB.validate_full_refs(self.repo, [ref])
        self.assertEqual(GITHUB.local_refs(self.repo), before)
        self.assertEqual(git(self.repo, "reflog", "show", "--all", "--format=%H %gs"), reflogs)
        self.assertFalse(list((self.repo / ".git").rglob("*.lock")))

    def test_oversized_snapshot_refuses_before_native_transfer_or_metadata_update(self):
        snapshot = {"refs/tags/rust-vfixture" + str(at): self.head
                    for at in range(GITHUB.MAX_FETCH_REFS + 1)}
        with self.assertRaisesRegex(GITHUB.Refused, "2048-reference"):
            GITHUB.fetch(self.repo, "fixture/codex", self.specs, snapshot=snapshot, client=self.client)
        self.assertEqual(self.client.requests, [])
        self.assertEqual(git(self.repo, "for-each-ref", "refs/upstream-tags/"), "")

    def test_already_current_destination_cannot_hide_a_second_conflicting_source(self):
        destination = "refs/remotes/shared/main"
        git(self.repo, "update-ref", destination, self.head)
        specs = ["refs/heads/main:" + destination, "refs/tags/rust-v0.1.0:" + destination]
        with self.assertRaisesRegex(GITHUB.Refused, "different source references"):
            GITHUB.fetch(self.repo, "fixture/codex", specs, client=self.client)
        self.assertEqual(git(self.repo, "rev-parse", destination), self.head)
        self.assertFalse(any(isinstance(args, tuple) for args, _ in self.client.requests))

    def local_transport_scripts(self, free_gib):
        directory = self.repo / "alab"
        directory.mkdir()
        for name in ("sync.sh", "setup.sh", "lib.sh", "command_guard.py", "cli_status.py"):
            (directory / name).write_bytes((ROOT / "alab" / name).read_bytes())
        (directory / "github.py").write_text(disk_fixture_cli(ROOT / "alab/github.py", free_gib))
        library = directory / "lib.sh"
        library.write_text(library.read_text().replace(
            'ALAB_UPSTREAM_URL="https://github.com/openai/codex.git"',
            "ALAB_UPSTREAM_URL=" + shlex.quote(self.remote.as_uri()), 1))
        env = {key: value for key, value in os.environ.items()
               if not key.startswith(("GIT_", "ALAB_"))}
        env.update(ALAB_PYTHON=sys.executable,
                   ALAB_AGH=str(self.home / "unavailable-native-client"),
                   GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1")
        return directory, env

    def test_cold_sync_reports_disk_stop_before_updating_refs_or_creating_worktree(self):
        directory, env = self.local_transport_scripts(19)
        git(self.repo, "remote", "add", "upstream", self.remote.as_uri())
        git(self.repo, "config", "rerere.enabled", "true")
        refs = git(self.repo, "show-ref")
        result = subprocess.run(["/bin/bash", str(directory / "sync.sh"), "--stepwise"],
                                cwd=self.repo, env=env, capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 75, result.stderr)
        self.assertIn("20 GiB running floor", result.stderr)
        self.assertNotIn("upstream rewrote", result.stderr)
        self.assertEqual(git(self.repo, "show-ref"), refs)
        self.assertFalse((self.repo / ".git/alab-sync").exists())

    def test_setup_uses_explicit_local_transport_and_preserves_push_refusal(self):
        directory, env = self.local_transport_scripts(21)
        for _ in range(2):
            done = subprocess.run(["/bin/bash", str(directory / "setup.sh")], cwd=self.repo,
                                  env=env, capture_output=True, text=True, timeout=30)
            self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(git(self.repo, "rev-parse", "refs/upstream-tags/rust-v0.1.0"), self.tag)
        self.assertEqual(git(self.repo, "config", "remote.upstream.pushurl"), "NEVER-PUSH-TO-UPSTREAM")
        self.assertEqual(git(self.repo, "rev-parse", "HEAD"), self.head)

    def test_tracking_cas_preserves_a_concurrent_newer_observation(self):
        GITHUB.fetch(self.repo, str(self.remote), self.specs)
        git(self.repo, "commit", "-q", "--allow-empty", "-m", "next")
        next_head = git(self.repo, "rev-parse", "HEAD")
        git(self.repo, "push", "-q", str(self.remote), "HEAD:refs/heads/main")
        git(self.repo, "commit", "-q", "--allow-empty", "-m", "newer observation")
        newer = git(self.repo, "rev-parse", "HEAD")
        self.client.after_fetch = lambda destination, _slot: git(
            destination, "update-ref", "refs/remotes/upstream/main", newer)
        with self.assertRaises(GITHUB.Refused):
            GITHUB.fetch(self.repo, "fixture/codex", [self.specs[0]], client=self.client)
        self.assertNotEqual(next_head, newer)
        self.assertEqual(git(self.repo, "rev-parse", "refs/remotes/upstream/main"), newer)

    def test_public_prefixes_preserve_annotated_objects_without_rest(self):
        client = object.__new__(GITHUB.Agh)
        head = f"{self.head}\trefs/heads/main\n".encode()
        tag = f"{self.tag}\trefs/tags/rust-v0.1.0\n{self.head}\trefs/tags/rust-v0.1.0^{{}}\n".encode()
        with mock.patch.object(client, "call", side_effect=[head, tag]) as read, mock.patch.object(
                client, "api", side_effect=AssertionError("unexpected REST request")):
            observed = GITHUB.list_refs(self.repo, "fixture/codex",
                                        ["refs/heads/main", "refs/tags/rust-v*"], anonymous=True, client=client)
        self.assertEqual(observed, {"refs/heads/main": self.head, "refs/tags/rust-v0.1.0": self.tag})
        self.assertEqual(read.call_args_list, [
            mock.call("repo", "public-refs", "fixture/codex", "--prefix", prefix, anonymous=True)
            for prefix in ("refs/heads/main", "refs/tags/rust-v")])

    def test_public_prefix_failure_only_falls_back_for_an_older_client(self):
        client = object.__new__(GITHUB.Agh)
        old = GITHUB.Refused("old client")
        old.native_error = {"kind": "usage", "exit": 2, "why": "unknown flag --prefix"}
        with mock.patch.object(client, "call", side_effect=old), mock.patch.object(
                client, "api", side_effect=self.client.api):
            self.assertEqual(GITHUB.list_refs(self.repo, "fixture/codex", ["refs/heads/main"],
                                              anonymous=True, client=client), {"refs/heads/main": self.head})
        with mock.patch.object(client, "call", side_effect=GITHUB.Refused("output exceeded its bound")):
            with self.assertRaisesRegex(GITHUB.Refused, "exceeded"):
                GITHUB.list_refs(self.repo, "fixture/codex", ["refs/heads/main"], anonymous=True, client=client)
        for raw in (b"partial\trefs/heads/main", f"{self.head}\trefs/heads/elsewhere\n".encode()):
            with self.subTest(raw=raw), mock.patch.object(client, "call", return_value=raw):
                with self.assertRaisesRegex(GITHUB.Refused, "malformed"):
                    GITHUB.list_refs(self.repo, "fixture/codex", ["refs/heads/main"], anonymous=True, client=client)

    def test_changed_tag_after_enumeration_refuses_instead_of_reading_another_snapshot(self):
        bare = self.home / "public snapshot.git"
        bare.mkdir()
        git(bare, "init", "-q", "--bare")
        observed = GITHUB.list_refs(bare, "fixture/codex", ["refs/tags/rust-v*"],
                                    anonymous=True, client=self.client)
        git(self.repo, "tag", "-a", "-f", "rust-v0.1.0", "-m", "retagged")
        git(self.repo, "push", "-q", "--force", str(self.remote), "refs/tags/rust-v0.1.0")
        with self.assertRaisesRegex(GITHUB.Refused, "enumerated object ID"):
            GITHUB.fetch(bare, "fixture/codex", ["refs/tags/rust-v0.1.0:refs/tags/v0.1.0"],
                         snapshot=observed, snapshot_refs=True, anonymous=True, client=self.client)
        self.assertEqual(GITHUB.current_ref(bare, "refs/tags/v0.1.0"), GITHUB.ZERO)

    def test_ref_escape_and_source_branch_destinations_are_refused(self):
        outside = self.home / "outside"
        outside.mkdir()
        (self.repo / ".git/refs/remotes").symlink_to(outside, target_is_directory=True)
        with self.assertRaisesRegex(GITHUB.Refused, "symbolic"):
            GITHUB.fetch(self.repo, str(self.remote), [self.specs[0]])
        self.assertEqual(list(outside.iterdir()), [])
        with self.assertRaisesRegex(GITHUB.Refused, "only updates metadata"):
            GITHUB.fetch(self.repo, str(self.remote), ["refs/heads/main:refs/heads/main"])
        self.assertEqual(git(self.repo, "rev-parse", "HEAD"), self.head)

    def test_foreign_urls_and_missing_native_capability_never_fall_back(self):
        for remote in ("https://github.com.evil/fixture/codex", "https://user:secret@github.com/fixture/codex",
                       "https://github.com:0/fixture/codex", "ssh://other@github.com/fixture/codex"):
            with self.subTest(remote=remote), self.assertRaises(GITHUB.Refused):
                GITHUB.location(self.repo, remote)
        executable = self.home / "old-agh"
        executable.write_text("#!/bin/sh\necho 'old client'\n")
        executable.chmod(0o700)
        with mock.patch.dict(os.environ, {"ALAB_AGH":str(executable)}):
            with self.assertRaisesRegex(GITHUB.Refused, "lacks exact anonymous batch fetch"):
                GITHUB.fetch(self.repo, "fixture/codex", self.specs)

    def test_network_url_syntax_cannot_be_shadowed_by_a_local_directory(self):
        for remote in ("https://github.com/fixture/codex.git",
                       "ssh://git@github.com/fixture/codex.git",
                       "ssh://git@gh-fixture/fixture/codex.git",
                       "git@github.com:fixture/codex.git", "git@gh-fixture:fixture/codex.git"):
            with self.subTest(remote=remote):
                shadow = self.repo / Path(remote)
                shadow.mkdir(parents=True)
                self.assertEqual(GITHUB.location(self.repo, remote), ("fixture/codex", True))
                git(self.repo, "config", "remote.shadow.url", remote)
                self.assertEqual(GITHUB.location(self.repo, "shadow"), ("fixture/codex", True))
                self.assertTrue(shadow.is_dir())

    def test_invalid_network_urls_refuse_even_when_their_local_shadows_exist(self):
        for remote in ("https://github.com.evil/fixture/codex.git",
                       "https://user:fixture@github.com/fixture/codex.git",
                       "https://github.com:0/fixture/codex.git",
                       "https://github.com/fixture/codex.git?query",
                       "ssh://other@github.com/fixture/codex.git",
                       "ssh://git@gh-other/fixture/codex.git",
                       "git@elsewhere.invalid:fixture/codex.git",
                       "other@github.com:fixture/codex.git", "http://github.com/fixture/codex.git"):
            with self.subTest(remote=remote):
                shadow = self.repo / Path(remote)
                shadow.mkdir(parents=True)
                with self.assertRaises(GITHUB.Refused):
                    GITHUB.location(self.repo, remote)
                self.assertTrue(shadow.is_dir())

    def test_explicit_local_paths_keep_network_punctuation_local(self):
        shadow = self.repo / Path("https://github.com/fixture/codex.git")
        shadow.mkdir(parents=True)
        for remote in ("./https://github.com/fixture/codex.git", str(shadow), shadow.as_uri()):
            with self.subTest(remote=remote):
                self.assertEqual(GITHUB.location(self.repo, remote), (str(shadow.resolve()), False))
        sibling = self.home / "git@github.com:fixture" / "codex.git"
        sibling.mkdir(parents=True)
        self.assertEqual(GITHUB.location(self.repo, "../git@github.com:fixture/codex.git"),
                         (str(sibling.resolve()), False))

    def test_existing_bare_owner_repository_path_remains_local(self):
        local = self.repo / "fixture/codex"
        local.mkdir(parents=True)
        self.assertEqual(GITHUB.location(self.repo, "fixture/codex"), (str(local.resolve()), False))

    def test_existing_public_tag_is_immutable_even_with_a_new_snapshot(self):
        bare = self.home / "public snapshot.git"
        bare.mkdir()
        git(bare, "init", "-q", "--bare")
        specs = ["refs/tags/rust-v0.1.0:refs/tags/v0.1.0"]
        GITHUB.fetch(bare, str(self.remote), specs, snapshot_refs=True)
        git(self.repo, "tag", "-a", "-f", "rust-v0.1.0", "-m", "different release identity")
        git(self.repo, "push", "-q", "--force", str(self.remote), "refs/tags/rust-v0.1.0")
        with self.assertRaisesRegex(GITHUB.Refused, "immutable"):
            GITHUB.fetch(bare, str(self.remote), specs, snapshot_refs=True)
        self.assertEqual(git(bare, "rev-parse", "refs/tags/v0.1.0"), self.tag)

    def test_native_calls_pin_argv0_env_and_complete_enumeration(self):
        client = object.__new__(GITHUB.Agh)
        client.executable, client.identity = "/bin/sh", "fixture identity"
        with mock.patch.object(client, "file_identity", return_value=client.identity), \
                mock.patch.dict(os.environ, {"GIT_DIR": "/unrelated", "GIT_CONFIG_COUNT": "1"}), \
                mock.patch.object(GITHUB.subprocess, "run", return_value=
                                  subprocess.CompletedProcess([], 0, b"[]", b"")) as run:
            self.assertEqual(client.api("/repos/fixture/codex/git/matching-refs/tags/", anonymous=True,
                                        all_pages=True), [])
        self.assertEqual(run.call_args.args[0], ["agh", "--anonymous", "api",
                         "/repos/fixture/codex/git/matching-refs/tags/", "--all"])
        self.assertEqual(run.call_args.kwargs["executable"], "/bin/sh")
        self.assertFalse(any(key.startswith("GIT_") for key in run.call_args.kwargs["env"]))

    def test_batch_input_is_exact_private_file_and_removed_after_native_return(self):
        client = object.__new__(GITHUB.Agh)
        rows = [{"source": "refs/heads/main", "into": "refs/agh/owned", "expected_oid": self.head}]
        seen = []
        def call(*args, **kwargs):
            self.assertEqual(args[:5], ("repo", "fetch", "fixture/codex", str(self.repo.resolve()), "--input"))
            path = Path(args[5])
            self.assertEqual(json.loads(path.read_text()), rows)
            self.assertTrue(kwargs["anonymous"])
            seen.append(path)
            return b"{}"
        with mock.patch.object(client, "call", side_effect=call):
            client.fetch_batch("fixture/codex", self.repo, rows, anonymous=True)
        self.assertFalse(seen[0].exists())

    def test_native_missing_never_swallow_permission_or_untyped_failures(self):
        client = object.__new__(GITHUB.Agh)
        client.executable, client.identity = "/bin/sh", "fixture identity"
        for code, error, absent in ((6, {"v":1,"exit":6,"side_effect":"none","kind":"not_found","status":404}, True),
                                    (5, {"v":1,"exit":5,"side_effect":"none","kind":"unreadable","status":404}, False),
                                    (6, {"v":1,"exit":6,"side_effect":"unknown","kind":"not_found","status":404}, False),
                                    (6, {"kind":"not_found","status":404}, False)):
            stderr = ("agh: native read failed\n" + json.dumps({"error":error}) + "\n").encode()
            with self.subTest(error=error), mock.patch.object(client,"file_identity",return_value=client.identity), \
                    mock.patch.object(GITHUB.subprocess,"run",return_value=subprocess.CompletedProcess([],code,b"",stderr)):
                if absent:
                    self.assertIsNone(client.api("/repos/fixture/codex/git/ref/heads/main", absent=True))
                else:
                    with self.assertRaises(GITHUB.Refused):
                        client.api("/repos/fixture/codex/git/ref/heads/main", absent=True)


if __name__ == "__main__":
    unittest.main()

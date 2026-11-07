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
                         {"refs/heads/main": self.head, "refs/tags/rust-v0.1.0": self.tag})
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

    def test_setup_uses_explicit_local_transport_and_preserves_push_refusal(self):
        directory = self.repo / "alab"
        directory.mkdir()
        for name in ("setup.sh", "lib.sh", "command_guard.py", "github.py"):
            (directory / name).write_bytes((ROOT / "alab" / name).read_bytes())
        # Only the fixture's endpoint data changes; setup and the real transport
        # execute intact, with no global Git URL rewrite or network client.
        library = directory / "lib.sh"
        library.write_text(library.read_text().replace(
            'ALAB_UPSTREAM_URL="https://github.com/openai/codex.git"',
            "ALAB_UPSTREAM_URL=" + shlex.quote(self.remote.as_uri()), 1))
        env = {key: value for key, value in os.environ.items()
               if not key.startswith(("GIT_", "ALAB_"))}
        env.update(ALAB_PYTHON=sys.executable,
                   ALAB_AGH=str(self.home / "unavailable-native-client"),
                   GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1")
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

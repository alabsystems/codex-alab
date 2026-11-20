# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Enumerated head identities survive real local remote movement and cache races."""

import hashlib
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

import test_github as fixture


class GithubHeadSnapshotTests(unittest.TestCase):
    def setUp(self):
        self.world = fixture.GithubTests()
        self.world.setUp()
        self.addCleanup(self.world.doCleanups)
        self.repo, self.remote = self.world.repo, self.world.remote
        self.head, self.client = self.world.head, self.world.client
        self.main = "refs/remotes/upstream/main"
        self.alias = "refs/remotes/upstream/mirror"
        self.specs = ["refs/heads/main:" + self.main, "refs/heads/mirror:" + self.alias]

    def slot(self, ref):
        return "refs/agh/codex-release-" + hashlib.sha256(
            ("fixture/codex\n" + ref).encode()).hexdigest()[:24]

    def fetch(self, specs):
        return fixture.GITHUB.fetch(self.repo, "fixture/codex", specs, client=self.client)

    def test_object_transfer_admission_receives_the_common_store_of_a_linked_worktree(self):
        linked = self.repo.parent / "fetch-admission-linked"
        fixture.git(self.repo, "worktree", "add", "--detach", str(linked), "HEAD")
        common = Path(fixture.git(linked, "rev-parse", "--path-format=absolute", "--git-common-dir"))
        deny = mock.Mock(side_effect=RuntimeError("low object-store volume"))
        with self.assertRaisesRegex(RuntimeError, "low object-store volume"):
            fixture.GITHUB.fetch(linked, "fixture/codex", self.specs[:1],
                                 client=self.client, before_transfer=deny)
        deny.assert_called_once_with(common, Path(tempfile.gettempdir()))
        self.assertEqual(self.batches(), [])

        self.fetch(self.specs[:1])
        self.client.requests.clear()
        fixture.GITHUB.fetch(linked, "fixture/codex", self.specs[:1],
                             client=self.client, before_transfer=deny)
        deny.assert_called_once()
        self.assertEqual(self.batches(), [])

    def test_guarded_native_fetch_stages_its_pack_beside_the_request(self):
        client = object.__new__(fixture.GITHUB.Agh)
        client.executable = sys.executable
        client.identity = client.file_identity()
        parent = self.repo.parent

        def inspect(args, *, env, **kwargs):
            request = Path(args[args.index("--input") + 1])
            self.assertTrue(request.is_file())
            self.assertEqual(request.parent.parent, parent)
            self.assertEqual(Path(env["TMPDIR"]), request.parent)
            return subprocess.CompletedProcess(args, 0, b"{}", b"")

        with mock.patch.object(fixture.GITHUB, "run_transport", side_effect=inspect) as run:
            client.fetch_batch("fixture/codex", self.repo, [], scratch_parent=parent)
        run.assert_called_once()

    def batches(self):
        return [args[3] for args, _ in self.client.requests if isinstance(args, tuple)]

    def private_refs(self):
        return {ref: oid for ref, oid in fixture.GITHUB.local_refs(self.repo).items()
                if ref.startswith("refs/agh/")}

    def add_mirror(self):
        fixture.git(self.remote, "update-ref", "refs/heads/mirror", self.head)

    def test_concurrent_native_transfer_rechecks_the_same_authenticated_batch(self):
        self.fetch(self.specs[:1])
        old = self.head
        fixture.git(self.repo, "commit", "-q", "--allow-empty", "-m", "published meanwhile")
        new = fixture.git(self.repo, "rev-parse", "HEAD")
        fixture.git(self.repo, "push", "-q", str(self.remote), "HEAD:refs/heads/main")
        native = self.client.fetch_batch
        calls = []

        def race(identity, destination, rows, **flags):
            calls.append(rows)
            if len(calls) == 1:
                peer = fixture.NativeFixture(self.remote)
                fixture.GITHUB.fetch(destination, identity, self.specs[:1], client=peer)
                slot = rows[0]["into"]
                transaction = f"start\nupdate {slot} {new} {old}\nprepare\ncommit\n"
                refused = subprocess.run(["git", "-C", str(destination), "update-ref", "--stdin"],
                                         input=transaction, text=True, capture_output=True, check=False)
                self.assertEqual(refused.returncode, 128)
                self.assertIn(f"cannot lock ref '{slot}': is at {new} but expected {old}", refused.stderr)
                error = fixture.GITHUB.Refused("native ref transaction failed")
                error.native_error = {"v": 1, "exit": 3, "sub": "git", "kind": "could_not_run",
                                      "side_effect": "none", "retriable": True,
                                      "why": "git fetch exited 128: " + refused.stderr}
                raise error
            return native(identity, destination, rows, **flags)

        admit = mock.Mock()
        with mock.patch.object(self.client, "fetch_batch", side_effect=race):
            fixture.GITHUB.fetch(self.repo, "fixture/codex", self.specs[:1],
                                 client=self.client, before_transfer=admit)
        self.assertEqual(admit.call_count, 2)
        self.assertEqual(admit.call_args_list[0], admit.call_args_list[1])
        self.assertEqual(len(calls), 2)
        self.assertEqual(calls[0], calls[1])
        self.assertEqual(fixture.GITHUB.current_ref(self.repo, self.main), new)

    def test_authentication_failure_survives_a_peer_completing_the_same_transfer(self):
        failure = fixture.GITHUB.Refused("account authorization failed")
        failure.native_error = {"v": 1, "exit": 4, "kind": "auth", "side_effect": "none"}
        peer = fixture.NativeFixture(self.remote)

        def denied(identity, destination, rows, **flags):
            fixture.GITHUB.fetch(destination, identity, self.specs[:1], client=peer)
            raise failure

        with mock.patch.object(self.client, "fetch_batch", side_effect=denied) as transfer:
            with self.assertRaises(fixture.GITHUB.Refused) as caught:
                self.fetch(self.specs[:1])
        self.assertIs(caught.exception, failure)
        self.assertEqual(transfer.call_count, 1)

    def test_successive_identical_peers_finish_the_same_tracking_snapshot(self):
        self.add_mirror()
        actual_git = fixture.GITHUB.git
        peers = iter((self.main, self.alias))
        transactions = []

        def peer_at_commit(repo, *args, **options):
            data = options.get("data", b"")
            if args[0] == "update-ref" and b"prepare\n" in data:
                transactions.append(data)
                ref = next(peers, None)
                if ref is not None:
                    fixture.git(repo, "update-ref", ref, self.head, fixture.GITHUB.ZERO)
            return actual_git(repo, *args, **options)

        with mock.patch.object(fixture.GITHUB, "git", side_effect=peer_at_commit):
            self.fetch(self.specs)
        self.assertEqual(len(transactions), 3)
        self.assertEqual(len(self.batches()), 1)
        self.assertEqual(fixture.GITHUB.current_ref(self.repo, self.main), self.head)
        self.assertEqual(fixture.GITHUB.current_ref(self.repo, self.alias), self.head)

    def test_moved_current_head_refuses_the_whole_tracking_batch(self):
        self.add_mirror()
        fixture.git(self.repo, "update-ref", self.main, self.head)
        newer = fixture.git(self.repo, "commit-tree", self.head + "^{tree}",
                            "-p", self.head, "-m", "peer observed a newer main")
        actual_git = fixture.GITHUB.git
        moved = False

        def move_at_commit(repo, *args, **options):
            nonlocal moved
            if args[0] == "update-ref" and b"prepare\n" in options.get("data", b"") and not moved:
                moved = True
                fixture.git(self.repo, "push", "-q", str(self.remote), newer + ":refs/heads/main")
                fixture.git(self.repo, "update-ref", self.main, newer, self.head)
            return actual_git(repo, *args, **options)

        with mock.patch.object(fixture.GITHUB, "git", side_effect=move_at_commit):
            with self.assertRaisesRegex(fixture.GITHUB.Refused, "tracking reference changed"):
                self.fetch(self.specs)
        self.assertTrue(moved)
        self.assertEqual(fixture.GITHUB.current_ref(self.repo, self.main), newer)
        self.assertEqual(fixture.GITHUB.current_ref(self.repo, self.alias), fixture.GITHUB.ZERO)
        self.fetch(self.specs)
        self.assertEqual(fixture.GITHUB.current_ref(self.repo, self.main), newer)
        self.assertEqual(fixture.GITHUB.current_ref(self.repo, self.alias), self.head)

    def test_same_tracking_update_by_a_peer_keeps_the_rest_of_the_batch_atomic(self):
        self.add_mirror()
        self.client.after_fetch = lambda destination, _slot: fixture.git(
            destination, "update-ref", self.main, self.head)
        self.fetch(self.specs)
        self.assertEqual(fixture.GITHUB.current_ref(self.repo, self.main), self.head)
        self.assertEqual(fixture.GITHUB.current_ref(self.repo, self.alias), self.head)

    def test_remote_head_movement_fetches_the_enumerated_object(self):
        native = self.client.fetch_batch
        advanced = []

        def advance(*arguments, **keywords):
            next_head = fixture.git(self.repo, "commit-tree", self.head + "^{tree}",
                                    "-p", self.head, "-m", "remote moved after enumeration")
            fixture.git(self.repo, "push", "-q", str(self.remote), next_head + ":refs/heads/main")
            advanced.append(next_head)
            return native(*arguments, **keywords)

        refusal = None
        with mock.patch.object(self.client, "fetch_batch", side_effect=advance) as transfer:
            try:
                self.fetch([self.specs[0]])
            except fixture.GITHUB.Refused as error:
                refusal = error
        self.assertEqual(transfer.call_count, 1)
        self.assertEqual(len(advanced), 1)
        self.assertNotEqual(advanced[0], self.head)
        self.assertEqual(fixture.git(self.remote, "rev-parse", "refs/heads/main"), advanced[0])
        self.assertEqual(fixture.git(self.repo, "rev-parse", "HEAD"), self.head)
        self.assertIsNone(refusal, str(refusal))
        self.assertEqual(self.batches(), [[{"source": self.head,
                         "into": self.slot("refs/heads/main"), "expected_oid": self.head}]])
        self.assertEqual(fixture.GITHUB.current_ref(self.repo, self.main), self.head)

    def test_same_oid_heads_share_one_transfer_and_reuse_its_private_cache(self):
        self.add_mirror()
        self.fetch(self.specs)
        self.assertEqual(self.batches(), [[{"source": self.head,
                         "into": self.slot("refs/heads/main"), "expected_oid": self.head}]])
        for destination in (self.main, self.alias):
            self.assertEqual(fixture.GITHUB.current_ref(self.repo, destination), self.head)
            fixture.git(self.repo, "update-ref", "-d", destination)
        cache = self.private_refs()
        self.assertEqual(cache, {self.slot("refs/heads/main"): self.head})
        self.fetch(self.specs)
        self.assertEqual(len(self.batches()), 1)
        self.assertEqual(self.private_refs(), cache)
        for destination in (self.main, self.alias):
            self.assertEqual(fixture.GITHUB.current_ref(self.repo, destination), self.head)

    def test_current_desired_main_seeds_a_pending_alias_without_native_transfer(self):
        self.fetch([self.specs[0]])
        self.assertEqual(len(self.batches()), 1)
        self.add_mirror()
        self.fetch(self.specs)
        self.assertEqual(len(self.batches()), 1)
        self.assertEqual(fixture.GITHUB.current_ref(self.repo, self.main), self.head)
        self.assertEqual(fixture.GITHUB.current_ref(self.repo, self.alias), self.head)
        self.assertEqual(self.private_refs(), {self.slot("refs/heads/main"): self.head})

    def test_current_main_cannot_hide_a_symbolic_canonical_private_slot(self):
        self.add_mirror()
        fixture.git(self.repo, "update-ref", self.main, self.head)
        slot = self.slot("refs/heads/main")
        fixture.git(self.repo, "symbolic-ref", slot, "refs/heads/main")
        with self.assertRaisesRegex(fixture.GITHUB.Refused, "symbolic"):
            self.fetch(self.specs)
        self.assertEqual(self.batches(), [])
        self.assertEqual(fixture.GITHUB.current_ref(self.repo, self.main), self.head)
        self.assertEqual(fixture.GITHUB.current_ref(self.repo, self.alias), fixture.GITHUB.ZERO)
        self.assertEqual(fixture.git(self.repo, "symbolic-ref", slot), "refs/heads/main")
        self.assertEqual(fixture.git(self.repo, "rev-parse", "HEAD"), self.head)

    def test_lightweight_tag_cache_cannot_supply_a_head_group(self):
        tag = "refs/tags/rust-vlight"
        destination = "refs/upstream-tags/rust-vlight"
        fixture.git(self.repo, "tag", "rust-vlight", self.head)
        fixture.git(self.repo, "push", "-q", str(self.remote), tag)
        self.fetch(["+" + tag + ":" + destination])
        self.assertEqual(len(self.batches()), 1)
        self.assertEqual(self.batches()[0][0]["source"], tag)
        self.assertEqual(fixture.git(self.repo, "cat-file", "-t", tag), "commit")
        tag_slot = self.slot(tag)
        before = {ref: oid for ref, oid in fixture.GITHUB.local_refs(self.repo).items()
                  if ref.startswith(("refs/tags/", "refs/upstream-tags/")) or ref == tag_slot}
        self.fetch([self.specs[0]])
        self.assertEqual(len(self.batches()), 2)
        self.assertEqual(self.batches()[1], [{"source": self.head,
                         "into": self.slot("refs/heads/main"), "expected_oid": self.head}])
        after = {ref: oid for ref, oid in fixture.GITHUB.local_refs(self.repo).items()
                 if ref.startswith(("refs/tags/", "refs/upstream-tags/")) or ref == tag_slot}
        self.assertEqual(after, before)
        self.assertEqual(fixture.GITHUB.current_ref(self.repo, self.main), self.head)

    def test_changed_canonical_slot_refuses_both_head_metadata_updates(self):
        self.add_mirror()
        other = fixture.git(self.repo, "commit-tree", self.head + "^{tree}",
                            "-m", "different fetched private identity")
        mutated = []

        def mutate(destination, slot):
            self.assertEqual(slot, self.slot("refs/heads/main"))
            fixture.git(destination, "update-ref", slot, other)
            mutated.append(slot)

        self.client.after_fetch = mutate
        with self.assertRaisesRegex(fixture.GITHUB.Refused, "enumerated object ID"):
            self.fetch(self.specs)
        self.assertEqual(mutated, [self.slot("refs/heads/main")])
        self.assertEqual(len(self.batches()), 1)
        self.assertEqual(fixture.GITHUB.current_ref(self.repo, self.main), fixture.GITHUB.ZERO)
        self.assertEqual(fixture.GITHUB.current_ref(self.repo, self.alias), fixture.GITHUB.ZERO)
        self.assertEqual(fixture.GITHUB.current_ref(self.repo, mutated[0]), other)
        self.assertEqual(fixture.git(self.repo, "rev-parse", "HEAD"), self.head)

# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""One fetch destination retains its complete source and force obligations."""

import unittest
from unittest import mock

import test_github as fixture


CONFLICT = "different source references or force policies map to one fetch destination"


class GithubDestinationPolicyTests(unittest.TestCase):
    def setUp(self):
        self.world = fixture.GithubTests()
        self.world.setUp()
        self.addCleanup(self.world.doCleanups)
        self.repo, self.remote = self.world.repo, self.world.remote
        self.head, self.client = self.world.head, self.world.client
        self.destination = "refs/upstream-tags/collision"
        self.tag = "refs/tags/lightweight-collision"
        fixture.git(self.repo, "config", "core.logAllRefUpdates", "always")
        fixture.git(self.remote, "update-ref", self.tag, self.head)
        self.assertEqual(fixture.git(self.remote, "cat-file", "-t", self.tag), "commit")

    def fetch(self, specs):
        return fixture.GITHUB.fetch(self.repo, "fixture/codex", specs, client=self.client)

    def batches(self):
        return [args[3] for args, _ in self.client.requests if isinstance(args, tuple)]

    def assert_conflict_before_transfer(self, specs, *, move_tag=False):
        before = fixture.GITHUB.local_refs(self.repo)
        logs = fixture.git(self.repo, "reflog", "show", "--all", "--format=%H %gs")
        native = self.client.fetch_batch
        movement = []

        def transfer(identity, destination, rows, **kwargs):
            if move_tag:
                previous = fixture.git(self.remote, "rev-parse", self.tag)
                next_head = fixture.git(self.repo, "commit-tree", self.head + "^{tree}",
                                        "-p", self.head, "-m", "tag moved after enumeration")
                fixture.git(self.repo, "push", "-q", "--force", str(self.remote),
                            next_head + ":" + self.tag)
                movement.append({"before": previous, "after": next_head,
                                 "enumerated_oids": [row["expected_oid"] for row in rows]})
            return native(identity, destination, rows, **kwargs)

        refusal = None
        with mock.patch.object(self.client, "fetch_batch", side_effect=transfer) as delegated:
            try:
                self.fetch(specs)
            except fixture.GITHUB.Refused as error:
                refusal = error
        after = fixture.GITHUB.local_refs(self.repo)
        observed = {"refusal": str(refusal) if refusal is not None else None,
                    "delegated_calls": delegated.call_count, "native_batches": self.batches(),
                    "tag_movement": movement,
                    "remote_tag_now": fixture.git(self.remote, "rev-parse", self.tag),
                    "destination_before": before.get(self.destination, fixture.GITHUB.ZERO),
                    "destination_after": after.get(self.destination, fixture.GITHUB.ZERO),
                    "caller_HEAD": fixture.git(self.repo, "rev-parse", "HEAD")}
        self.assertEqual(observed["caller_HEAD"], self.head, observed)
        self.assertEqual(observed["refusal"], CONFLICT, observed)
        self.assertEqual(delegated.call_count, 0, observed)
        self.assertEqual(self.batches(), [], observed)
        self.assertEqual(after, before, observed)
        self.assertEqual(fixture.git(self.repo, "reflog", "show", "--all", "--format=%H %gs"),
                         logs, observed)

    def test_head_last_cannot_discard_a_named_tag_obligation(self):
        self.assert_conflict_before_transfer(
            [self.tag + ":" + self.destination, "refs/heads/main:" + self.destination],
            move_tag=True)

    def test_tag_last_cannot_replace_the_head_source_obligation(self):
        self.assert_conflict_before_transfer(
            ["refs/heads/main:" + self.destination, self.tag + ":" + self.destination],
            move_tag=True)

    def test_forced_last_cannot_remove_an_existing_non_fast_forward_obligation(self):
        unrelated = fixture.git(self.repo, "commit-tree", self.head + "^{tree}",
                                "-m", "unrelated existing destination")
        fixture.git(self.repo, "update-ref", self.destination, unrelated)
        source = "refs/heads/main:" + self.destination
        self.assert_conflict_before_transfer([source, "+" + source])

    def test_unforced_last_cannot_hide_a_conflicting_force_policy(self):
        unrelated = fixture.git(self.repo, "commit-tree", self.head + "^{tree}",
                                "-m", "unrelated existing destination")
        fixture.git(self.repo, "update-ref", self.destination, unrelated)
        source = "refs/heads/main:" + self.destination
        self.assert_conflict_before_transfer(["+" + source, source])

    def test_identical_resolved_policies_are_idempotent(self):
        destination = "refs/remotes/upstream/main"
        literal = "refs/heads/main:" + destination
        for specs in ([literal, literal], ["refs/heads/*:refs/remotes/upstream/*", literal]):
            with self.subTest(specs=specs):
                observed = self.fetch(specs)
                self.assertEqual(observed["refs/heads/main"], self.head)
                self.assertEqual(fixture.GITHUB.current_ref(self.repo, destination), self.head)
                self.assertEqual(fixture.git(self.repo, "rev-parse", "HEAD"), self.head)
        self.assertEqual(len(self.batches()), 1)
        self.assertEqual(len(self.batches()[0]), 1)
        self.assertEqual(self.batches()[0][0]["source"], self.head)
        self.assertEqual(self.batches()[0][0]["expected_oid"], self.head)

    def test_different_destinations_keep_same_oid_head_and_tag_policies(self):
        main = "refs/remotes/upstream/main"
        tag = "refs/upstream-tags/lightweight-collision"
        self.fetch(["refs/heads/main:" + main, self.tag + ":" + tag])
        self.assertEqual(len(self.batches()), 1)
        self.assertEqual({row["source"]: row["expected_oid"] for row in self.batches()[0]},
                         {self.head: self.head, self.tag: self.head})
        for destination in (main, tag):
            self.assertEqual(fixture.GITHUB.current_ref(self.repo, destination), self.head)
        self.assertEqual(fixture.git(self.remote, "rev-parse", self.tag), self.head)
        self.assertEqual(fixture.git(self.repo, "rev-parse", "HEAD"), self.head)

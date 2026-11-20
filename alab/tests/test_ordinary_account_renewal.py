# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Negative controls for the package smoke's synthetic two-slot renewal evidence."""
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing, contextmanager
import http.client
import json
from pathlib import Path
import sys
import tempfile
import threading
from types import SimpleNamespace
import unittest


sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import ordinary_account_renewal as renewal


def grant(slot, reason="proactive_expiry"):
    tokens = json.loads(slot.path.read_text())["tokens"]
    return {"grant_type": "refresh_token", "refresh_token": tokens["refresh_token"],
            "client_id": tokens["oauth_provenance"]["client_id"], "refresh_reason": reason}


def exchange(slot, reason="proactive_expiry"):
    return slot.exchange(grant(slot, reason))


def replace(path, data):
    staging = path.with_suffix(".new")
    staging.write_bytes(data)
    staging.replace(path)


class OrdinaryAccountRenewalTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root = Path(directory.name)
        self.slot = renewal.Slot(root, "a", -600, "https://127.0.0.1")
        self.sibling = renewal.Slot(root, "b", 120, "https://127.0.0.1", weekly_used=100, usage_allowed=False)

    def test_renewal_requires_proactive_expiry(self):
        with self.assertRaises(RuntimeError):
            exchange(self.slot, reason="manual")
        self.assertEqual(self.slot.exchanges, 0)

    def test_successful_exchange_and_usage_without_a_persisted_login_fail(self):
        def in_memory_only(slot):
            if not slot.exchanges:
                exchange(slot)
            slot.usage()

        with self.assertRaisesRegex(RuntimeError, "atomically replace"):
            renewal.check_slot(self.slot, self.sibling, in_memory_only)
        self.assertEqual((self.slot.exchanges, self.slot.usage_reads), (1, 1))

    def test_reopening_rejects_replacement_of_a_byte_identical_sibling(self):
        def read(slot):
            if not slot.exchanges:
                saved = json.loads(slot.path.read_text())
                saved["tokens"].update(exchange(slot))
                replace(slot.path, json.dumps(saved).encode())
            else:
                path = self.sibling.path
                replace(path, path.read_bytes())
            slot.usage()

        with self.assertRaisesRegex(RuntimeError, "reopening changed"):
            renewal.check_slot(self.slot, self.sibling, read)
        self.assertEqual((self.slot.exchanges, self.slot.usage_reads), (1, 2))

    def test_duplicate_or_missing_peer_cannot_confirm_or_strand_an_exchange(self):
        for duplicate in (False, True):
            with self.subTest(duplicate=duplicate), ThreadPoolExecutor(max_workers=3) as pool:
                overlap = renewal.PendingExchanges(("a", "b"), timeout=0.2)
                waiting = [pool.submit(overlap.hold, "a"), pool.submit(overlap.wait_until_pending)]
                if duplicate:
                    waiting.append(pool.submit(overlap.hold, "a"))
                try:
                    for pending in waiting:
                        with self.assertRaises(RuntimeError):
                            pending.result(timeout=3)
                    with self.assertRaises(RuntimeError):
                        overlap.hold("b")
                    self.assertFalse(overlap.confirmed)
                finally:
                    overlap.abort()

    @contextmanager
    def shared_issuer(self, overlap=None):
        with renewal.Server(("127.0.0.1", 0), renewal.Handler) as server:
            server.register_slots((self.slot, self.sibling), overlap)
            thread = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.02})
            thread.start()
            try:
                yield server
            finally:
                server.shutdown()
                thread.join()

    def test_shared_issuer_rejects_invalid_grants_while_a_peer_waits(self):
        payload = grant(self.sibling)
        for case, body in (
            ("unknown refresh token", json.dumps(dict(payload, refresh_token=self.sibling.new_access))),
            ("wrong client", json.dumps(dict(payload, client_id=self.sibling.account))),
            ("malformed JSON", "{"),
        ):
            overlap = renewal.PendingExchanges(("a", "b"), timeout=1)
            with self.subTest(case=case), self.shared_issuer(overlap) as server, ThreadPoolExecutor(2) as pool:
                waiting = pool.submit(overlap.hold, self.slot.name)
                controller = pool.submit(overlap.wait_until_pending)
                try:
                    with closing(http.client.HTTPConnection("127.0.0.1", server.server_port, timeout=3)) as conn:
                        conn.request("POST", "/oauth/token", body)
                        self.assertEqual(conn.getresponse().status, 400)
                    for pending in (waiting, controller):
                        with self.assertRaises(RuntimeError):
                            pending.result(timeout=3)
                finally:
                    overlap.abort()
                self.assertTrue(server.fault)
                self.assertFalse(overlap.confirmed)
                self.assertEqual((self.slot.exchanges, self.sibling.exchanges), (0, 0))

    def test_all_account_reads_require_one_issued_bearer_and_matching_workspace(self):
        bearer = "Bearer " + self.slot.new_access
        other_bearer = "Bearer " + self.sibling.new_access
        old_bearer = "Bearer " + json.loads(self.slot.path.read_text())["tokens"]["access_token"]
        account, other_account = self.slot.account, self.sibling.account
        paths = ["/backend-api/wham/usage", "/backend-api/wham/accounts/check", "/v1/models"]
        with self.shared_issuer() as server:
            def status(path, bearers, accounts):
                with closing(http.client.HTTPConnection("127.0.0.1", server.server_port, timeout=3)) as conn:
                    conn.putrequest("GET", path)
                    for value in bearers:
                        conn.putheader("Authorization", value)
                    for value in accounts:
                        conn.putheader("ChatGPT-Account-Id", value)
                    conn.endheaders()
                    response = conn.getresponse()
                    response.read()
                    return response.status

            for path in paths:
                self.assertEqual(status(path, [bearer], [account]), 400)
            exchange(self.slot)
            exchange(self.sibling)
            for path in paths:
                for bearers, accounts in [
                    ([], [account]), ([bearer], []), ([old_bearer], [account]),
                    ([other_bearer], [account]), ([bearer], [other_account]),
                    ([bearer, bearer], [account]), ([bearer, other_bearer], [account]),
                    ([bearer], [account, account]), ([bearer], [account, other_account]),
                ]:
                    with self.subTest(path=path, bearer_count=len(bearers), account_count=len(accounts)):
                        self.assertEqual(status(path, bearers, accounts), 400)
                self.assertEqual(status(path, [bearer], [account]), 200)
            self.assertTrue(server.fault)
            self.assertEqual((self.slot.usage_reads, self.sibling.usage_reads), (1, 0))

    def check_failed_overlap(self, error, message, *, corrupt=None, observed_identity=None, usage_reply=None):
        slots = self.slot, self.sibling
        overlap = renewal.PendingExchanges([slot.name for slot in slots], timeout=2)
        live = set()

        @contextmanager
        def session_for(slot):
            other = self.sibling if slot is self.slot else self.slot
            identity_reads = 0

            def initialize():
                if not slot.exchanges:
                    issued = exchange(slot)
                    overlap.hold(slot.name)
                    saved = json.loads(slot.path.read_text())
                    saved["tokens"].update(issued)
                    if corrupt:
                        corrupt(saved, other)
                    replace(slot.path, json.dumps(saved).encode())

            def request(method, _params, **_options):
                nonlocal identity_reads
                if method == "account/read":
                    identity_reads += 1
                    seen = observed_identity(slot, identity_reads, live) if observed_identity else slot
                    return {"account": {"type": "chatgpt", "email": seen.email, "planType": "pro"}}
                self.assertEqual(method, "account/rateLimits/read")
                slot.usage()
                quota = {"accountId": slot.account, "ordinaryUsageAllowed": slot.usage_allowed,
                         "rateLimits": {"secondary": {"usedPercent": slot.weekly_used,
                                                      "windowDurationMins": 10080, "resetsAt": 605_800}}}
                if usage_reply:
                    usage_reply(slot, quota)
                return quota

            live.add(slot)
            try:
                yield SimpleNamespace(initialize=initialize, request=request)
            finally:
                live.remove(slot)

        with self.assertRaisesRegex(error, message):
            renewal.check_overlapping_slots(slots, session_for, overlap)
        self.assertTrue(overlap.confirmed)
        self.assertFalse(live)

    def reject_permission_change(self, selected, read_number, change):
        def change_one_read(slot, quota):
            if slot is selected and slot.usage_reads == read_number:
                change(quota)
        self.check_failed_overlap(RuntimeError, "wrong RPC ordinary usage permission", usage_reply=change_one_read)
        self.assertEqual(selected.usage_reads, read_number)

    def test_quota_blocked_login_cannot_be_reported_as_allowed_after_renewal(self):
        self.reject_permission_change(self.sibling, 1, lambda quota: quota.update(ordinaryUsageAllowed=True))

    def test_missing_permission_fails_on_still_open_server(self):
        self.reject_permission_change(self.slot, 2, lambda quota: quota.pop("ordinaryUsageAllowed"))

    def test_conflicting_permission_fails_on_still_open_server(self):
        self.reject_permission_change(self.slot, 2, lambda quota: quota["rateLimits"].update(spendControlReached=True))

    def test_allowed_login_cannot_be_reported_as_blocked_after_reverse_reopen(self):
        self.reject_permission_change(self.slot, 3, lambda quota: quota.update(ordinaryUsageAllowed=False))

    def test_a_sibling_identity_token_cannot_pass_persistence_checks(self):
        def wrong_identity(saved, other):
            saved["tokens"]["id_token"] = other.id_token

        self.check_failed_overlap(RuntimeError, "saved credential differs", corrupt=wrong_identity)

    def test_early_writer_cannot_replace_the_still_pending_sibling(self):
        def replace_sibling(_saved, other):
            replace(other.path, other.path.read_bytes())

        self.check_failed_overlap(RuntimeError, "changed the held sibling", corrupt=replace_sibling)

    def test_second_read_on_open_server_rejects_the_other_subscription(self):
        def identity(slot, reads, live):
            if slot is self.slot and reads == 2:
                self.assertEqual(live, {self.slot, self.sibling})
                self.assertGreaterEqual(self.sibling.usage_reads, 1)
                return self.sibling
            return slot

        self.check_failed_overlap(ValueError, "account mismatch", observed_identity=identity)


if __name__ == "__main__":
    unittest.main()

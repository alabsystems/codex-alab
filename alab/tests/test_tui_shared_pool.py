# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The shared-pool interactive smoke's oracles (alab/smoke/tui_shared_pool.py), on records.

The records are shaped as the hosts traced them on 2026-11-15 (s/p1): every session is
answered `ready`, the canary's commit is taken, each follower's commit is retired every
5 s until the canary's report is answered completed, and its next commit is then taken.
Whether a real host and package pass is PARALLEL-LOGINS.md's, not these tests'.
"""
from pathlib import Path
import sys
import tempfile
import threading
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import capacity_exec as probe
import tui_shared_pool as pool

SECOND = 1_000_000
WAITING = "OK codex-rotation state=waiting"


def answer(state, receipt, thread="thread", turn="wall"):
    return (f"OK codex-rotation state={state} id={receipt} thread={thread} turn={turn} "
            "source_account=s source_user=u target_account=t target_user=v")


def receipt(id, sid, thread):
    return {"id": id, "sid": sid, "intent": {"thread": thread, "wall": f"wall-{thread}",
            "purpose": "weekly_quota"}, "old_auth": "/pool/auth-old/auth.json",
            "new_auth": "/pool/auth-new/auth.json"}


def moved(id, thread, ready, commit, consume, complete, waiting_reports=()):
    """The calls from a ready receipt `id` to its report answered completed."""
    turn = f"wall-{thread}"
    return [("prepare", answer("ready", id, thread, turn), ready),
            ("commit", answer("waiting", id, thread, turn), commit),
            ("status", answer("committed", id, thread, turn), commit + 4 * SECOND),
            ("consume", answer("consumed", id, thread, turn), consume),
            *[("complete", WAITING, stamp) for stamp in waiting_reports],
            ("complete", answer("completed", id, thread, turn), complete)]


def retired(id, thread, at):
    """A follower's ask behind the canary: ready, and its commit retired."""
    return [("prepare", answer("ready", id, thread, f"wall-{thread}"), at),
            ("commit", answer("retired", id, thread, f"wall-{thread}"), at + 50_000)]


class Records:
    """A passing three-session follow, from its walls at 0 s to its last report."""

    def __init__(self):
        canary_calls = [("prepare", WAITING, 300_000), ("prepare", WAITING, 500_000),
                        *moved("c" * 32, "t2", 5_550_000, 5_720_000, 15_450_000, 15_540_000)]
        self.sessions = [None, None,
                         {"sid": "s2", "calls": canary_calls, "refusedUs": 15_000,
                          "receipt": receipt("c" * 32, "s2", "t2")}]
        for index in (0, 1):
            id = str(index) * 32
            calls = [("prepare", WAITING, 600_000), *retired("a" * 31 + str(index), f"t{index}", 5_800_000),
                     *retired("b" * 31 + str(index), f"t{index}", 10_900_000),
                     *moved(id, f"t{index}", 16_000_000, 16_070_000 + index * 100_000, 25_400_000, 25_500_000)]
            self.sessions[index] = {"sid": f"s{index}", "calls": calls, "refusedUs": index * 1_500,
                                    "receipt": receipt(id, f"s{index}", f"t{index}")}
        self.ledger = [{"account": "main", "kind": "wall", "sid": None, "t": 1000, "why": "seven_day until 2"},
                       {"account": "main", "kind": "rotate", "sid": "s2", "t": 1005, "to": "target", "why": ""},
                       {"account": "target", "kind": "proven", "sid": None, "t": 1015, "why": ""}]
        self.state = {"active": "target", "proven": True, "canary": None, "rotations": [1005]}

    def verdict(self):
        return pool.pool_verdict(self.sessions, self.ledger, self.state)

    def follower(self, index, calls):
        self.sessions[index] = dict(self.sessions[index], calls=calls)


class FollowVerdict(unittest.TestCase):
    def test_followers_whose_commit_is_taken_after_the_report_pass(self):
        proof = Records().verdict()
        self.assertEqual((proof["canary"], proof["chargedRotations"], proof["canaryTaken"],
                          proof["provenAccount"]), (2, 1, False, "target"))
        self.assertEqual(proof["walls"]["spreadMs"], 15.0)
        self.assertEqual(proof["reportSecondsAfterWall"], 15.525)
        self.assertEqual([(item["session"], item["turnedAway"], item["commitsRetired"], item["takenSecondsAfterReport"])
                          for item in proof["followers"]], [(0, 2, 2, 0.53), (1, 2, 2, 0.63)])
        self.assertEqual(proof["slowestFollowerSecondsAfterReport"], 0.63)

    def test_a_follower_that_took_the_canary_fails(self):
        records = Records()
        records.ledger.insert(2, {"account": "target", "kind": "rotate", "sid": "s0", "t": 1605,
                                  "to": "target", "why": "canary: in place of s2, which did not answer"})
        with self.assertRaisesRegex(RuntimeError, "a session took the canary: canary: in place of s2"):
            records.verdict()

    def test_a_follower_that_waited_out_the_canary_hold_fails(self):
        records = Records()
        late = 15_540_000 + pool.CANARY_HOLD_SECONDS * SECOND
        records.follower(0, [*retired("a" * 32, "t0", 5_800_000),
                             *moved("0" * 32, "t0", late - 50_000, late, late + 9 * SECOND, late + 10 * SECOND)])
        with self.assertRaisesRegex(RuntimeError, "follower 0's commit was taken 600.0 s after the canary's "
                                    "report, not within 60 s: it waited out the canary hold"):
            records.verdict()

    def test_a_follower_whose_commit_was_taken_before_the_report_fails(self):
        records = Records()
        records.follower(1, [*retired("a" * 32, "t1", 5_800_000),
                             *moved("1" * 32, "t1", 12_000_000, 12_050_000, 13_000_000, 13_100_000)])
        with self.assertRaisesRegex(RuntimeError, "follower 1's commit was taken 3.490 s before the canary's "
                                    "report was answered completed"):
            records.verdict()

    def test_a_follower_never_turned_away_before_the_report_fails(self):
        records = Records()
        # Its only waits came before the canary was answered ready: nothing shows it asked
        # while the canary moved.
        records.follower(0, [("prepare", WAITING, 600_000), ("prepare", WAITING, 5_000_000),
                             *moved("0" * 32, "t0", 16_000_000, 16_070_000, 25_400_000, 25_500_000)])
        with self.assertRaisesRegex(RuntimeError, "follower 0 was never turned away between the canary's "
                                    "ready and its report: nothing shows it asked then and was not moved"):
            records.verdict()
        # A wait after the canary's ready counts as much as a retired commit does, although
        # the host answers it while the follower's own read is pending too (s/p2's
        # follower 1): it shows the follower asked and was not moved, not why. The order of
        # its move after the report is what ties the move to the report.
        records.follower(0, [("prepare", WAITING, 6_000_000),
                             *moved("0" * 32, "t0", 16_000_000, 16_070_000, 25_400_000, 25_500_000)])
        proof = records.verdict()["followers"][0]
        self.assertEqual((proof["turnedAway"], proof["commitsRetiredBeforeReport"]), (1, 0))
        # A wait that names a receipt is one being committed, not a turn away.
        records.follower(0, [("prepare", answer("waiting", "9" * 32, "t0", "wall-t0"), 6_000_000),
                             *moved("0" * 32, "t0", 16_000_000, 16_070_000, 25_400_000, 25_500_000)])
        with self.assertRaisesRegex(RuntimeError, "follower 0 was never turned away"):
            records.verdict()

    def test_a_commit_answered_a_bare_waiting_takes_nothing(self):
        # aterm answers `commit` a bare `waiting` when commit_pool refuses the selection and
        # the receipt cannot be retired: nothing was taken, and the next ask may be.
        self.assertEqual([pool.commit_outcome(reply) for reply in
                          (WAITING, answer("retired", "a" * 32), answer("waiting", "a" * 32),
                           answer("committed", "a" * 32), "ERR codex-rotation selection changed")],
                         ["turned away", "turned away", "taken", "taken", "refused"])
        self.assertIsNone(pool.first_taken([("commit", WAITING, 1), ("commit", answer("retired", "a" * 32), 2)]))
        records = Records()
        records.follower(0, [*retired("a" * 32, "t0", 5_800_000)[:1], ("commit", WAITING, 5_850_000),
                             *moved("0" * 32, "t0", 16_000_000, 16_070_000, 25_400_000, 25_500_000)])
        proof = records.verdict()["followers"][0]
        self.assertEqual((proof["turnedAway"], proof["takenSecondsAfterReport"]), (1, 0.53))

    def test_a_second_rotation_fails(self):
        records = Records()
        records.ledger.insert(2, {"account": "main", "kind": "rotate", "sid": "s0", "t": 1006,
                                  "to": "target", "why": ""})
        with self.assertRaisesRegex(RuntimeError, "2 rotations were charged"):
            records.verdict()
        records = Records()
        records.state["rotations"].append(1006)
        with self.assertRaisesRegex(RuntimeError, r"1 rotations were charged \(2 on record\), not one"):
            records.verdict()

    def test_a_rotation_charged_to_another_session_than_the_first_committed_fails(self):
        records = Records()
        records.ledger[1] = dict(records.ledger[1], sid="s0")
        with self.assertRaisesRegex(RuntimeError, "the charged rotation was session s0's, not the canary's"):
            records.verdict()

    def test_walls_spread_over_a_second_fail(self):
        records = Records()
        records.sessions[1] = dict(records.sessions[1], refusedUs=1_500_000)
        with self.assertRaisesRegex(RuntimeError, "the sessions were walled 1500 ms apart, not within 1000 ms"):
            records.verdict()
        records.sessions[1] = dict(records.sessions[1], refusedUs=None)
        with self.assertRaisesRegex(RuntimeError, r"session\(s\) 1 were never refused on the shared account"):
            records.verdict()

    def test_a_canary_report_withheld_or_a_followers_own_report_missing_fails(self):
        records = Records()
        records.sessions[2] = dict(records.sessions[2], calls=records.sessions[2]["calls"][:-1])
        records.state = dict(records.state, proven=False, canary={"sid": "s2", "at": 1005})
        with self.assertRaisesRegex(RuntimeError, "the pool did not end proven with no canary held"):
            records.verdict()
        records.state = Records().state
        with self.assertRaisesRegex(RuntimeError, "no `codex-rotation complete` followed the successor's consume"):
            records.verdict()
        records = Records()
        records.follower(0, records.sessions[0]["calls"][:-1])
        with self.assertRaisesRegex(RuntimeError, "no `codex-rotation complete` followed"):
            records.verdict()

    def test_a_follower_whose_taken_commit_is_not_its_consumed_receipt_fails(self):
        records = Records()
        records.sessions[0] = dict(records.sessions[0], receipt=receipt("e" * 32, "s0", "t0"))
        with self.assertRaisesRegex(RuntimeError, "follower 0's first commit taken is not its consumed receipt's"):
            records.verdict()


class ControlRecords:
    """A passing control: the canary's report withheld under the lock, then a 120 s watch."""

    HELD = {"fromUs": 15_500_000, "untilUs": 22_700_000}
    WATCH_END = 142_700_000

    def __init__(self):
        id, turn = "c" * 32, "wall-t2"
        canary = [("prepare", answer("ready", id, "t2", turn), 5_550_000),
                  ("commit", answer("waiting", id, "t2", turn), 5_720_000),
                  ("consume", answer("consumed", id, "t2", turn), 15_400_000),
                  ("consume", answer("consumed", id, "t2", turn), 16_500_000),
                  *[("complete", WAITING, 16_600_000 + step * 250_000) for step in range(21)]]
        self.sessions = [None, None, {"sid": "s2", "calls": canary, "refusedUs": 15_000}]
        for index in (0, 1):
            calls = [call for step in range(28) for call in
                     [*retired(f"{index}{step:031d}", f"t{index}", 5_800_000 + step * 5_000_000),
                      ("status", answer("retired", f"{index}{step:031d}", f"t{index}", f"wall-t{index}"),
                       5_900_000 + step * 5_000_000)]]
            self.sessions[index] = {"sid": f"s{index}", "calls": calls, "refusedUs": index * 1_500}
        self.ledger = [{"account": "main", "kind": "wall", "sid": None, "t": 1000, "why": "seven_day until 2"},
                       {"account": "main", "kind": "rotate", "sid": "s2", "t": 1005, "to": "target", "why": ""}]
        self.state = {"active": "target", "proven": False, "rotations": [1005],
                      "canary": {"sid": "s2", "at": 1005, "took": None, "native_receipt": None}}
        self.now = 1005 + 143

    def verdict(self, **changes):
        held = dict(self.HELD, **changes)
        return pool.control_verdict(self.sessions, self.ledger, self.state, held, self.WATCH_END, self.now)


class ControlVerdict(unittest.TestCase):
    def test_followers_held_with_the_report_withheld_pass(self):
        proof = ControlRecords().verdict()
        self.assertEqual((proof["canary"], proof["reportsWhileHeld"], proof["poolProven"]), (2, 21, False))
        self.assertEqual((proof["lockHeldSeconds"], proof["reportSpanSeconds"], proof["watchedSecondsAfterRelease"]),
                         (7.2, 5.0, 120.0))
        self.assertEqual(proof["canaryHoldRemainingSeconds"], 457)
        answers = {"commit retired": 28, "prepare ready": 28, "status retired": 28}
        self.assertEqual([(item["session"], item["turnedAwayAfterRelease"], item["answersAfterCanaryReady"])
                          for item in proof["followers"]], [(0, 24, answers), (1, 24, answers)])

    def test_a_follower_whose_commit_was_taken_fails(self):
        records = ControlRecords()
        records.sessions[0]["calls"].append(("commit", answer("waiting", "f" * 32, "t0", "wall-t0"), 60 * SECOND))
        with self.assertRaisesRegex(RuntimeError, "2 sessions had a commit taken with the report withheld, "
                                    "not the canary alone: 0, 2"):
            records.verdict()

    def test_a_follower_refused_rather_than_held_fails(self):
        refused = "ERR codex-rotation selection changed"
        # Refused from the lock's release to 10 s before the watch's end, and turned away
        # once more within the last 15 s: asking, never taken, and never shown held.
        records = ControlRecords()
        records.sessions[0]["calls"] = [
            (verb, refused if records.HELD["untilUs"] < stamp < records.WATCH_END - 10 * SECOND else reply, stamp)
            for verb, reply, stamp in records.sessions[0]["calls"]]
        with self.assertRaisesRegex(RuntimeError, "follower 0 was refused or moved after the canary's ready, not "
                                    "only turned away, so its wait is not shown to be the canary's: "
                                    f"commit {refused}; prepare {refused}; status {refused}$"):
            records.verdict()
        # One refusal while the lock was held is enough, as is a verb a follower that was
        # never moved has no receipt for.
        records = ControlRecords()
        records.sessions[1]["calls"].insert(9, ("prepare", "ERR codex-rotation expired or interrupted", 16_000_000))
        with self.assertRaisesRegex(RuntimeError, "follower 1 was refused or moved .*: prepare ERR codex-rotation "
                                    "expired or interrupted$"):
            records.verdict()
        records = ControlRecords()
        records.sessions[1]["calls"].append(("consume", answer("consumed", "1" * 32, "t1", "wall-t1"), 141 * SECOND))
        with self.assertRaisesRegex(RuntimeError, "follower 1 was refused or moved .*: consume OK codex-rotation "
                                    "state=consumed"):
            records.verdict()
        # What the hosts answered once the watch ended, as they were stopped, is not judged.
        records = ControlRecords()
        records.sessions[1]["calls"].append(("prepare", refused, records.WATCH_END + 1))
        self.assertEqual(records.verdict()["followers"][1]["turnedAwayAfterRelease"], 24)

    def test_a_follower_commit_answered_a_bare_waiting_is_not_taken(self):
        # commit_pool refused and the receipt could not be retired: the follower is turned
        # away, the control goes on, and the answer counts as a wait.
        records = ControlRecords()
        records.sessions[0]["calls"].append(("commit", WAITING, 141 * SECOND))
        proof = records.verdict()
        self.assertEqual((proof["followers"][0]["turnedAwayAfterRelease"],
                          proof["followers"][0]["answersAfterCanaryReady"]["commit waiting"]), (25, 1))

    def test_a_report_answered_otherwise_or_outside_the_hold_fails(self):
        records = ControlRecords()
        records.sessions[2]["calls"].append(("complete", answer("completed", "c" * 32, "t2", "wall-t2"), 21_900_000))
        with self.assertRaisesRegex(RuntimeError, "a withheld report was answered otherwise than waiting"):
            records.verdict()
        with self.assertRaisesRegex(RuntimeError, "the canary reported while the custody lock was not held"):
            ControlRecords().verdict(untilUs=20_000_000)
        records = ControlRecords()
        records.sessions[2]["calls"] = [call for call in records.sessions[2]["calls"] if call[0] != "complete"]
        with self.assertRaisesRegex(RuntimeError, "the canary sent no report while the custody lock was held"):
            records.verdict()

    def test_a_pool_proven_a_canary_taken_or_a_lapsed_hold_fails(self):
        records = ControlRecords()
        records.ledger.append({"account": "target", "kind": "proven", "sid": None, "t": 1100, "why": ""})
        with self.assertRaisesRegex(RuntimeError, "the pool was proven with the canary's report withheld"):
            records.verdict()
        records = ControlRecords()
        records.ledger.append({"account": "target", "kind": "rotate", "sid": "s0", "t": 1606, "to": "target",
                               "why": "canary: in place of s2, which did not answer"})
        with self.assertRaisesRegex(RuntimeError, "a session took the canary"):
            records.verdict()
        records = ControlRecords()
        records.now = 1005 + pool.CANARY_HOLD_SECONDS
        with self.assertRaisesRegex(RuntimeError, "the canary hold lapsed during the watch"):
            records.verdict()
        records = ControlRecords()
        records.state["canary"] = dict(records.state["canary"], sid="s0")
        with self.assertRaisesRegex(RuntimeError, "the canary hold is not the canary's"):
            records.verdict()

    def test_a_follower_that_stopped_asking_fails(self):
        records = ControlRecords()
        records.sessions[1]["calls"] = [call for call in records.sessions[1]["calls"] if call[2] < 100 * SECOND]
        with self.assertRaisesRegex(RuntimeError, r"follower 1 was last turned away 46\.9 s before the watch "
                                    "ended, not within 15 s: it stopped asking"):
            records.verdict()
        records.sessions[1]["calls"] = [call for call in records.sessions[1]["calls"] if call[2] < 20 * SECOND]
        with self.assertRaisesRegex(RuntimeError, "follower 1 was never turned away after the lock was let go"):
            records.verdict()


class RequestsVerdict(unittest.TestCase):
    def requests(self, index=0):
        walled = [{"type": "message", "role": "user", "content": [{"type": "input_text", "text": pool.prompt(index)}]},
                  {"type": "function_call", "call_id": probe.CALL_ID, "name": "update_plan", "arguments": "{}"},
                  {"type": "function_call_output", "call_id": probe.CALL_ID, "output": "Plan updated"}]
        return [{"generation": "old", "status": 200, "body": {"input": walled[:1]}},
                {"generation": "old", "status": 429, "body": {"input": walled}},
                {"generation": "new", "status": 200, "body": {"input": list(walled)}}]

    def test_a_turn_resumed_unchanged_on_the_destination_passes(self):
        self.assertEqual(pool.requests_verdict(self.requests(), 0, True),
                         {"session": 0, "requests": 3, "walledInputResumed": True})
        self.assertEqual(pool.requests_verdict(self.requests()[:2], 0, False), {"session": 0, "requests": 2})

    def test_a_changed_resent_or_misattributed_turn_fails(self):
        requests = self.requests()
        requests[2]["body"]["input"] = requests[2]["body"]["input"][:2]
        with self.assertRaisesRegex(RuntimeError, "session 0's resumed turn changed its walled input"):
            pool.requests_verdict(requests, 0, True)
        with self.assertRaisesRegex(RuntimeError, r"session 0's model requests were \[\('old', 200\), \('old', 429\)\]"):
            pool.requests_verdict(self.requests()[:2], 0, True)
        with self.assertRaisesRegex(RuntimeError, r"\('old', 429\), \('old', 429\)"):
            pool.requests_verdict(self.requests()[:2] + self.requests()[1:2], 0, False)
        with self.assertRaisesRegex(RuntimeError, "session 1's original input was lost or duplicated"):
            pool.requests_verdict(self.requests(), 1, True)


class PoolBarrier(unittest.TestCase):
    @staticmethod
    def body(index, *extra):
        return {"input": [{"type": "message", "role": "user", "content": [
                    {"type": "input_text", "text": pool.prompt(index)}]}, *extra],
                "tools": [{"type": "function", "name": "update_plan"}]}

    def test_the_walled_requests_wait_for_every_session_and_are_refused_together(self):
        with tempfile.TemporaryDirectory() as directory:
            model = pool.Pool(Path(directory), 3)
            self.assertEqual([model.reply(self.body(index), "old")[0] for index in range(3)], [200] * 3)
            statuses = {}

            def walled(index):
                statuses[index] = model.reply(self.body(index, {"type": "function_call_output"}), "old")[0]
            threads = [threading.Thread(target=walled, args=(index,)) for index in range(3)]
            for thread in threads[:2]:
                thread.start()
            threads[0].join(0.3)
            # Two sessions wait for the third, and the account still reads healthy.
            self.assertEqual(statuses, {})
            self.assertTrue(model.usage_body("old")["rate_limit"]["allowed"])
            threads[2].start()
            for thread in threads:
                thread.join(5)
            self.assertEqual(statuses, {0: 429, 1: 429, 2: 429})
            self.assertLess(max(model.refused_us.values()) - min(model.refused_us.values()), pool.WALL_SPREAD_US)
            self.assertFalse(model.usage_body("old")["rate_limit"]["allowed"])
            self.assertTrue(model.usage_body("new")["rate_limit"]["allowed"])
            # A later request on the walled account is refused; the destination answers.
            self.assertEqual(model.reply(self.body(1), "old")[0], 429)
            self.assertEqual(model.reply(self.body(0), "new")[0], 200)
            # A request no session's prompt names is unknown.
            self.assertEqual(model.reply({"input": [], "tools": [{"type": "function", "name": "update_plan"}]},
                                         "new")[0], 400)
            self.assertEqual([len(requests) for requests in model.by_session], [3, 3, 2])
            self.assertEqual(len(model.unknown_requests), 1)
            self.assertEqual(model.fixture_faults, [])

    def test_a_barrier_that_is_never_met_is_a_fixture_fault(self):
        with tempfile.TemporaryDirectory() as directory, mock.patch.object(pool, "BARRIER_SECONDS", 0.2):
            model = pool.Pool(Path(directory), 3)
            model.reply(self.body(0), "old")
            self.assertEqual(model.reply(self.body(0, {"type": "function_call_output"}), "old")[0], 500)
            self.assertEqual([fault["error"][:13] for fault in model.fixture_faults], ["BarrierFault:"])
            self.assertFalse(model.source_walled)


class CustodyLock(unittest.TestCase):
    def test_the_lock_is_exclusive_and_let_go(self):
        import fcntl
        import os
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "auth.json.lock"
            held = pool.take_custody(path)
            self.assertIsNotNone(held)
            self.assertIsNone(pool.take_custody(path))
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            fcntl.flock(held, fcntl.LOCK_UN)
            os.close(held)
            again = pool.take_custody(path)
            self.assertIsNotNone(again)
            os.close(again)


if __name__ == "__main__":
    unittest.main()

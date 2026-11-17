# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The rotation smokes' controllers, run whole against a stand-in host.

Nothing here says anything about aterm or codex: the host is
fixtures/rotation-host/stand_in_host.py, which plays a session against the controller's
own loopback provider. What is under test is the controllers' wiring, which no unit test
of their parts reaches: that a fixture fault ends a run at once, that a host without a
usable record or with a name aterm's snapshot refuses never starts, that the five-hour
case ends at its outcome and is judged again after its loop, that no run passes without
its own case's proof, that the far five-hour case fails by name while it runs and keeps
what its host answered, that a report names the modules that decided it and what its
package's own record is worth, and that the matrix pins them and counts no case without
its proof.
"""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

SMOKE = Path(__file__).resolve().parents[1] / "smoke"
sys.path.insert(0, str(SMOKE))
import capacity_exec as probe
import exec_accounts
import exec_shared_pool
import native_host_record
import package_standing
import parallel_logins
import provider_fixture
import renewal_follow as renewal
import renewal_matrix

STAND_IN = Path(__file__).resolve().parent / "fixtures" / "rotation-host" / "stand_in_host.py"
# The signed 0.7.0 package's own record, with a sysroot that names no home.
CERTIFIED = json.loads((Path(__file__).resolve().parent / "fixtures" / "package-standing"
                        / "certified-native.json").read_text())
TRIAL = {"attestation": package_standing.TRIAL_ATTESTATION, "source": "2" * 40,
         "binary_sha256": "3" * 64, "binary_bytes": 17, "builder": "the unit test",
         "built_at": "2026-11-13T09:30:00-08:00", "evidence": "written by the unit test"}
TRIAL_BUILD = {"source": "2" * 40, "version": "0.8.0-trial+g222222222",
               "target": "aarch64-apple-darwin",
               "compiler": {"admitted": False, "lane": package_standing.UNVERIFIED_LANE},
               "packager": "alab/smoke/trial_package.py", "trial": TRIAL}
PASS = "PASS real-host shipping-qualified policy and wrong-token refusal; worker EOF joined"


class World:
    """A short temp directory with a stand-in package and recorded stand-in hosts."""

    def __init__(self, case):
        # Short: the smokes put sockets and certificates under their output.
        self.root = Path(case.enterContext(tempfile.TemporaryDirectory(dir="/tmp"))).resolve()
        self.package = self.root / "package"
        (self.package / "bin").mkdir(parents=True)
        (self.package / "bin" / "codex").write_text("#!/bin/sh\nexit 9\n")
        (self.package / "alab-build.json").write_text(json.dumps(
            {"version": "0.0.0-stand-in", "target": "stand-in", "source": "0" * 40}))
        (self.package / "codex-package.json").write_text(json.dumps(
            {"version": "0.0.0-stand-in", "target": "stand-in", "entrypoint": "bin/codex"}))

    def package_records(self, build):
        """Give the stand-in package the record `build` and a manifest that agrees with it."""
        (self.package / "alab-build.json").write_text(json.dumps(build))
        (self.package / "codex-package.json").write_text(json.dumps(
            {"version": build["version"], "target": build["target"], "entrypoint": "bin/codex"}))

    def host(self, mode, name="aterm-stand-in-host", recorded=True, attestation="trial"):
        """A launcher that runs the stand-in in `mode`, with a record beside it."""
        clean = attestation == native_host_record.CLEAN
        directory = self.root / f"host-{mode}-{name}"
        directory.mkdir()
        host = directory / name
        host.write_text(f'#!/bin/sh\nexec "{sys.executable}" -B "{STAND_IN}" --mode {mode} "$@"\n')
        host.chmod(0o755)
        if recorded:
            (directory / "build.json").write_text(json.dumps({
                "schema": native_host_record.SCHEMA, "source": "1" * 40,
                "source_evidence": "a stand-in written by the unit test; no aterm build exists",
                "attestation": attestation, "builder": "the unit test" if clean else None,
                "built_at": "2026-11-12T09:30:00-08:00" if clean else None,
                "host_sha256": hashlib.sha256(host.read_bytes()).hexdigest(),
                "host_bytes": host.stat().st_size, "platform": "stand-in",
                "shipping_probe": {"ran": True, "returncode": 0, "verdict": PASS,
                                   "seconds": 0.0, "world": None}}))
        return host


def undecodable_usage(model, generation):
    """What the fixture served before provider_fixture.py: a window with no reset distance."""
    body = undecodable_usage.real(model, generation)
    del body["rate_limit"]["primary_window"]["reset_after_seconds"]
    return body


undecodable_usage.real = renewal.Model.usage_body


def run(main, name, *arguments):
    """Run a smoke's main() here, as its command line would, without its output."""
    with mock.patch.object(sys, "argv", [name, *map(str, arguments)]), mock.patch("builtins.print"):
        return main()


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


class WallCaseRunTests(unittest.TestCase):
    """renewal_follow.py --case five-hour-wall-soon, whole, with its clocks shortened."""

    CASE = "five-hour-wall-soon"

    def setUp(self):
        self.world = World(self)
        # A run that reaches no outcome ends at 20 s here, not at 270 s.
        self.enterContext(mock.patch.dict(renewal.CASE_BUDGETS, {self.CASE: (20, 25)}))
        self.enterContext(mock.patch.object(renewal, "WALL_LIFT_SECONDS", 1))
        self.output = self.world.root / "out"

    def run_case(self, mode):
        started = time.monotonic()
        code = run(renewal.main, "renewal_follow.py", "--host", self.world.host(mode),
                   "--package", self.world.package, "--output", self.output,
                   "--case", self.CASE, "--under-guardian")
        return code, json.loads((self.output / "result.json").read_text()), time.monotonic() - started

    def test_the_case_runs_to_its_verdict_and_names_what_decided_it(self):
        code, report, seconds = self.run_case("wall")
        self.assertEqual((code, report["passed"], report.get("error")), (0, True, None))
        self.assertLess(seconds, 15)
        waited = report["fiveHourWallWaitedOut"]
        self.assertGreaterEqual(waited["waitedSeconds"], 1)
        self.assertGreaterEqual(waited["walledUsageReads"], 1)
        # The stand-in traces no control verb: how often a host was asked is unknown.
        self.assertIsNone(waited["preparesTraced"])
        self.assertEqual((report["case"], report["requestCount"], report["rawHostReturncode"]),
                         (self.CASE, 3, 0))
        self.assertEqual(report["requestAttribution"],
                         {"foreground": 3, "threadTitle": 0, "unknown": 0, "unroutedPosts": 0})
        # The report names the controller, the modules that judged the fixture, the host
        # and the package, the host's attestation and the package's standing.
        self.assertEqual(report["controllerSha256"], sha256(renewal.__file__))
        self.assertEqual(report["fixtureSha256"], {
            "provider_fixture.py": sha256(provider_fixture.__file__),
            "native_host_record.py": sha256(native_host_record.__file__),
            "package_standing.py": sha256(package_standing.__file__)})
        self.assertEqual((report["nativeHost"]["attestation"], report["nativeHost"]["qualifies"]),
                         ("trial", False))
        # The stand-in package's record is no certified packager's: the run passed, and
        # its report says the package qualifies nothing.
        self.assertEqual(report["package"], {"qualifies": False, "trial": None,
                                             "why": "its record names no compiler"})
        events = json.loads((self.output / "http-events.json").read_text())
        self.assertEqual([(event["account"], event["status"]) for event in events
                          if event["method"] == "POST"],
                         [("source", 200), ("source", 429), ("source", 200)])
        self.assertEqual({event["client"] for event in events}, {"codex"})

    def test_a_result_carries_its_packages_standing_and_its_verdict_does_not_depend_on_it(self):
        expected = {
            "trial": (TRIAL_BUILD, {"qualifies": False, "trial": TRIAL,
                "why": "its record carries a `trial` object (attestation "
                       "'unverified-development-build'): the package was assembled from an "
                       "unverified development build"}),
            "certified": (CERTIFIED, {"qualifies": True, "trial": None,
                                      "why": package_standing.QUALIFIES}),
        }
        host = self.world.host("wall")
        for name, (build, standing) in expected.items():
            with self.subTest(name=name):
                self.world.package_records(build)
                output = self.world.root / ("out-" + name)
                code = run(renewal.main, "renewal_follow.py", "--host", host, "--package",
                           self.world.package, "--output", output, "--case", self.CASE,
                           "--under-guardian")
                report = json.loads((output / "result.json").read_text())
                # The same run, with the same verdict, whatever the package's record says.
                self.assertEqual((code, report["passed"], report.get("error")), (0, True, None))
                self.assertEqual(report["package"], standing)
                self.assertEqual(report["packageRecords"]["build"], build)
                self.assertEqual((report["source"], report["version"]),
                                 (build["source"], build["version"]))

    def test_a_run_whose_own_oracle_left_no_record_does_not_pass(self):
        # main() asks for the case's proof where it sets `passed`. Here the run is
        # otherwise the passing one above, and the proof asked of it is another case's.
        self.enterContext(mock.patch.dict(renewal.CASE_PROOFS, {self.CASE: "fiveHourWallHandedOver"}))
        code, report, _ = self.run_case("wall")
        self.assertEqual((code, report["passed"]), (1, False))
        self.assertEqual(report["error"], "RuntimeError: the five-hour-wall-soon oracle left no "
                         "fiveHourWallHandedOver record, so the run proves nothing of its own case")
        # Everything else of the run was as a pass leaves it.
        self.assertIn("fiveHourWallWaitedOut", report)
        self.assertEqual((report["requestCount"], report["rawHostReturncode"]), (3, 0))

    def test_a_reply_the_client_cannot_decode_fails_the_run_at_once(self):
        with mock.patch.object(renewal.Model, "usage_body", undecodable_usage):
            code, report, seconds = self.run_case("wall")
        self.assertEqual((code, report["passed"]), (1, False))
        self.assertTrue(report["error"].startswith(
            "RuntimeError: fixture served a reply the client cannot decode on /api/codex/usage: "
            "FixtureFault: usage: primary_window.reset_after_seconds"), report["error"])
        # At the loop's next pass, not at the hang detector the stand-in would poll into.
        self.assertNotIn("hang detector", report["error"])
        self.assertLess(seconds, 10)

    def test_a_handoff_fails_the_run_by_name_while_it_runs(self):
        code, report, seconds = self.run_case("handoff")
        self.assertEqual((code, report["passed"]), (1, False))
        self.assertEqual(report["error"], "RuntimeError: a five-hour wall that resets soon was "
                         "handed over, charged or replayed: native action(s): signal")
        self.assertNotIn("requestCount", report)
        self.assertLess(seconds, 10)

    def test_a_handoff_after_the_controller_stopped_looking_fails_the_verdict(self):
        code, report, _ = self.run_case("late-handoff")
        self.assertEqual((code, report["passed"]), (1, False))
        # The loop saw the case end (it recorded the count); the verdict then refused it.
        self.assertEqual(report["requestCount"], 3)
        self.assertEqual(report["error"], "RuntimeError: a five-hour wall that resets soon was "
                         "handed over, charged or replayed: native action(s): signal")
        self.assertNotIn("fiveHourWallWaitedOut", report)

    def test_a_host_that_outlives_its_exit_wait_is_sampled_and_fails_by_name(self):
        sampler = self.world.root / "sampler"
        sampler.write_text('#!/bin/sh\nprintf "threads of %s\\n" "$1" > "$5"\n')
        sampler.chmod(0o755)
        # Room in the guardian to sample, and a wait the stand-in outlives at once.
        self.enterContext(mock.patch.dict(renewal.CASE_BUDGETS, {self.CASE: (20, 60)}))
        self.enterContext(mock.patch.object(renewal, "HOST_EXIT_SECONDS", 1))
        self.enterContext(mock.patch.object(renewal, "SAMPLER", sampler))
        code, report, seconds = self.run_case("linger")
        self.assertEqual((code, report["passed"]), (1, False))
        self.assertEqual(report["error"], "RuntimeError: native host did not exit cleanly: "
                         "it outlived its 1 s wait after stop (host-exit-diagnostics.json)")
        # The case itself reached its outcome; only the host's exit failed it.
        self.assertEqual(report["requestCount"], 3)
        waited = json.loads((self.output / "host-wait.json").read_text())
        self.assertEqual((waited["returncode"], waited["stalled"]), (-15, True))
        diagnostics = json.loads((self.output / "host-exit-diagnostics.json").read_text())
        self.assertEqual((diagnostics["hostPid"], diagnostics["hostAlive"]), (waited["pid"], True))
        self.assertEqual(diagnostics["sample"]["file"], "host-sample.txt")
        self.assertEqual((self.output / "host-sample.txt").read_text(), f"threads of {waited['pid']}\n")
        self.assertLess(seconds, 15)

    def test_a_host_the_live_snapshot_would_refuse_or_one_unrecorded_never_starts(self):
        refused = {"named 'native_host'": self.world.host("wall", name="native_host"),
                   "native host build record missing": self.world.host("wall", recorded=False)}
        for message, host in refused.items():
            with self.subTest(message=message):
                with self.assertRaisesRegex(RuntimeError, message):
                    run(renewal.main, "renewal_follow.py", "--host", host, "--package",
                        self.world.package, "--output", self.output, "--case", self.CASE)
                self.assertFalse(self.output.exists())


class FarWallCaseRunTests(unittest.TestCase):
    """renewal_follow.py --case five-hour-wall-far, whole, as far as a stand-in can take it.

    Only its wrong ends are run here. A run that passes needs what no stand-in smaller
    than aterm and codex leaves: two real session processes with kernel births, a live
    snapshot of the first, a consumed receipt bound to both, the native operation's
    ledger and signal effect, the host's pool ledger and control trace, an answered
    control socket, and a rollout of the original turn completed twice. The oracle's
    acceptance is unit-tested on records (alab/smoke/renewal_follow_test.py); its pass
    against a real host and package is PARALLEL-LOGINS.md's. So no test here reaches
    main()'s call of far_wall_verdict. What holds that call is that `passed` is refused
    without the verdict's record (WallCaseRunTests, with another case's proof asked of
    a run that otherwise passes): a run that lost the call fails, it does not pass.
    """

    CASE = "five-hour-wall-far"

    def setUp(self):
        self.world = World(self)
        self.enterContext(mock.patch.dict(renewal.CASE_BUDGETS, {self.CASE: (20, 25)}))
        self.output = self.world.root / "out"

    run_case = WallCaseRunTests.run_case

    def test_a_session_that_waits_in_its_turn_fails_by_name_before_the_detector(self):
        self.enterContext(mock.patch.object(renewal, "FAR_YIELD_SECONDS", 2))
        # `wall` waits its wall out on its own account, polling its usage: the wall
        # of this case never lifts, so it never sends its request again either.
        code, report, seconds = self.run_case("wall")
        self.assertEqual((code, report["passed"]), (1, False))
        self.assertTrue(report["error"].startswith(
            "RuntimeError: a five-hour wall whose reset is far off was not handed to another "
            "account: its session waited in its turn: the host was not asked to move it within "
            "2 s of the refusal; after the first 429: "), report["error"])
        self.assertIn(" codex usage reads, 0 codex credit-list reads, 0 prepares traced", report["error"])
        self.assertNotIn("hang detector", report["error"])
        self.assertLess(seconds, 10)
        # The stand-in serves no control socket: its policy is recorded as unread, not as a no.
        self.assertEqual(report["hostFiveHourPolicy"], {"unread": "KeyError: 'socket'"})
        events = json.loads((self.output / "http-events.json").read_text())
        self.assertEqual([(event["account"], event["status"]) for event in events
                          if event["method"] == "POST"], [("source", 200), ("source", 429)])
        # Every reading after the refusal stated the five-hour wall, far off, on a usable week.
        walled = [event for event in events if event.get("allowed") is False]
        self.assertTrue(walled)
        self.assertEqual({(event["sessionUsed"], event["weekUsed"]) for event in walled}, {(100, 10)})
        self.assertGreater(min(event["sessionResetsIn"] for event in walled), 7000)

    def test_a_move_to_the_same_account_fails_by_name_at_once(self):
        code, report, seconds = self.run_case("same-account")
        self.assertEqual((code, report["passed"]), (1, False))
        self.assertEqual(report["error"], "RuntimeError: a five-hour wall whose reset is far off was "
                         "not handed to another account: it was moved to the account it is walled "
                         "on: receipt " + "cd" * 16 + " names the walled login as its destination")
        self.assertNotIn("fiveHourWallHandedOver", report)
        # At the loop's next pass: well inside the time a turn has to end (20 s).
        self.assertLess(seconds, 10)

    def test_a_host_whose_policy_prints_no_bound_fails_by_name_and_its_reply_is_kept(self):
        code, report, seconds = self.run_case("no-bound")
        self.assertEqual((code, report["passed"]), (1, False))
        reply = "OK gen=1 effective=on why=enabled contract=2,3 codex_rotation=0 limit_wait=true"
        self.assertEqual(report["error"], "RuntimeError: the host's policy prints no five-hour bound "
                         "(codex_rotation_five_hour), so it hands no five-hour wall to another account; "
                         "of rotation its reply says codex_rotation=0, and begins: " + reply)
        # The reply itself is in the run's record, judged or not.
        self.assertEqual(report["hostFiveHourPolicy"], {"reply": reply})
        self.assertNotIn("fiveHourWallHandedOver", report)
        self.assertLess(seconds, 10)


class LostLoginRunTests(unittest.TestCase):
    """renewal_follow.py's lost-login cases, whole, with their clocks shortened.

    The stand-in is no host and no codex-alab: these runs hold the controller (its two
    listeners, its loop and its verdicts) and say nothing of the product.
    """

    def setUp(self):
        self.world = World(self)
        self.enterContext(mock.patch.dict(renewal.CASE_BUDGETS, {
            case: (20, 25) for case in renewal.LOST_LOGIN_CASES}))
        self.enterContext(mock.patch.object(renewal, "LOGIN_WALL_HOLD_SECONDS", 2))
        self.output = self.world.root / "out"

    def run_case(self, case, mode):
        started = time.monotonic()
        code = run(renewal.main, "renewal_follow.py", "--host", self.world.host(mode),
                   "--package", self.world.package, "--output", self.output,
                   "--case", case, "--under-guardian")
        return code, json.loads((self.output / "result.json").read_text()), time.monotonic() - started

    def test_a_login_that_must_stay_runs_to_its_verdict_and_names_what_stopped_the_move(self):
        code, report, seconds = self.run_case("lost-login-invalidated", "login-stays")
        self.assertEqual((code, report["passed"], report.get("error")), (0, True, None))
        self.assertLess(seconds, 15)
        self.assertEqual(report["lostLogin"], {"refusal": "refresh_token_invalidated", "refusalStatus": 401,
                                               "hostMovesLostLogins": True, "issuer": "fixture-tls"})
        self.assertEqual(report["fixtureConfig"]["lost_login"], True)
        self.assertEqual((report["hostLoginPolicy"]["codexRotationLogin"], report["hostLoginPolicy"]["logins"]),
                         (True, "renew"))
        proof = report["lostLoginStayed"]
        self.assertEqual({key: proof[key] for key in (
            "refusal", "vetoKept", "stoppedBy", "codexLoginRotationLines", "otherAccountUsageReads",
            "receipts", "renewalPreparesAsked", "sourceRefreshPresentedByCodex",
            "sourceRefreshPresentedByHost", "sourceUsageReadsByHost", "hostActive", "turnEndedAtLoginWall",
            "loginFilesUnchanged", "issuer", "tokenRequests", "unroutedRequests", "tlsHandshakesRefused")},
            {"refusal": "refresh_token_invalidated", "vetoKept": "revoked", "stoppedBy": "codex-alab's veto",
             "codexLoginRotationLines": 0, "otherAccountUsageReads": 0, "receipts": 0,
             "renewalPreparesAsked": 2, "sourceRefreshPresentedByCodex": 1,
             "sourceRefreshPresentedByHost": 0, "sourceUsageReadsByHost": 0, "hostActive": "main",
             "turnEndedAtLoginWall": True, "loginFilesUnchanged": True, "issuer": "fixture-tls",
             "tokenRequests": 1, "unroutedRequests": 0, "tlsHandshakesRefused": 0})
        self.assertGreaterEqual(proof["heldSeconds"], 2)
        # The refresh went to the provider's own listener, as in every other case of the
        # controller, each request from a sender the record knows; the refused turn was
        # never answered.
        events = json.loads((self.output / "http-events.json").read_text())
        self.assertEqual([(event["path"].rsplit("/", 1)[-1], event.get("listener"), event.get("status"),
                           event["agent"]) for event in events if event["method"] == "POST"],
                         [("responses", None, 200, "codex"), ("responses", None, 401, "codex"),
                          ("token", None, 401, "codex")])
        self.assertEqual({event["method"] for event in events}, {"GET", "POST"})
        self.assertEqual((report["requestCount"], report["requestAttribution"]["unroutedPosts"]), (2, 0))
        self.assertNotIn("lostLoginMoved", report)

    def test_a_host_without_the_word_is_asked_nothing_and_the_run_says_the_policy_stopped_it(self):
        code, report, _ = self.run_case("lost-login-no-word", "login-stays")
        self.assertEqual((code, report["passed"], report.get("error")), (0, True, None))
        # The pool is the usage-limit handoff's, with no policy for a lost login.
        self.assertEqual((report["fixtureConfig"]["lost_login"], report["fixtureConfig"]["weekly_quota"]),
                         (False, True))
        self.assertEqual(report["hostLoginPolicy"]["codexRotationLogin"], False)
        proof = report["lostLoginStayed"]
        self.assertEqual((proof["refusal"], proof["vetoKept"], proof["stoppedBy"]),
                         ("refresh_token_expired", None, "the host's policy prints no word"))

    def test_an_ask_about_a_login_that_must_stay_fails_the_run_by_name_at_once(self):
        code, report, seconds = self.run_case("lost-login-reused", "login-asks")
        self.assertEqual((code, report["passed"]), (1, False))
        self.assertEqual(report["error"], "RuntimeError: a lost login that must stay a stop for a person "
                         "was moved or asked about: 1 codex-login-rotation line(s) sent, the first "
                         "answered OK codex-login-rotation state=waiting retry_ms=5000")
        self.assertNotIn("lostLoginStayed", report)
        self.assertNotIn("requestCount", report)
        self.assertLess(seconds, 10)

    def test_a_token_presented_by_neither_program_fails_the_run_by_name_at_once(self):
        # Counted as codex-alab's, it would be this run's one refused refresh and the case
        # would pass; it is neither program's, and the record says whose it looked like.
        code, report, seconds = self.run_case("lost-login-invalidated", "login-stranger")
        self.assertEqual((code, report["passed"]), (1, False))
        self.assertEqual(report["error"], "RuntimeError: a token request or a usage read came from a "
                         "program that is neither codex-alab nor the host by its User-Agent: "
                         "POST token from 'curl/8.7.1'")
        self.assertNotIn("lostLoginStayed", report)
        self.assertLess(seconds, 10)

    def test_a_session_that_never_asks_for_the_move_fails_by_name_before_the_detector(self):
        self.enterContext(mock.patch.object(renewal, "LOGIN_ASK_SECONDS", 2))
        code, report, seconds = self.run_case("lost-login-expired", "login-stays")
        self.assertEqual((code, report["passed"]), (1, False))
        self.assertEqual(report["error"], "RuntimeError: a lost login whose refresh token simply ended "
                         "was not moved to another account: codex-alab did not ask the host to move it "
                         "within 2 s of the refusal; after the first 401: token requests 1 x codex of "
                         "source (401); usage reads none; codex-login-rotation calls none; "
                         "codex-renewal calls 2 x prepare: waiting")
        self.assertEqual(report["hostLoginPolicy"]["codexRotationLogin"], True)
        self.assertNotIn("lostLoginMoved", report)
        self.assertNotIn("hang detector", report["error"])
        self.assertLess(seconds, 12)

    def test_the_move_over_a_plain_issuer_is_its_own_case_with_its_own_listener_and_record(self):
        code, report, seconds = self.run_case("lost-login-expired-plain-issuer", "login-no-word")
        self.assertEqual((code, report["passed"]), (1, False))
        self.assertEqual(report["error"], "RuntimeError: a lost login whose refresh token simply ended "
                         "was not moved to another account: the host's policy prints no "
                         "codex_rotation_login=1, so codex-alab asks it nothing")
        # The one case that serves the logins' issuer apart: the refresh arrives at that
        # second listener, and the result says which layout it is.
        self.assertEqual(report["lostLogin"]["issuer"], "loopback-http")
        events = json.loads((self.output / "http-events.json").read_text())
        self.assertEqual([(event.get("refreshOf"), event.get("listener"), event["status"]) for event in events
                          if event["path"].endswith("/oauth/token")], [("source", "issuer", 401)])
        # Its record is not the specified case's, and a failed run leaves neither.
        self.assertEqual((renewal.CASE_PROOFS["lost-login-expired-plain-issuer"],
                          renewal.CASE_PROOFS["lost-login-expired"]),
                         ("lostLoginMovedOverPlainIssuer", "lostLoginMoved"))
        self.assertFalse({"lostLoginMoved", "lostLoginMovedOverPlainIssuer", "lostLoginHeldBeforeItsAsks"}
                         & set(report))
        self.assertLess(seconds, 10)

    def test_a_host_without_the_word_cannot_pass_the_case_that_must_move(self):
        code, report, seconds = self.run_case("lost-login-expired", "login-no-word")
        self.assertEqual((code, report["passed"]), (1, False))
        self.assertEqual(report["error"], "RuntimeError: a lost login whose refresh token simply ended "
                         "was not moved to another account: the host's policy prints no "
                         "codex_rotation_login=1, so codex-alab asks it nothing")
        # The reply the refusal was read from is in the run's record.
        self.assertEqual(report["hostLoginPolicy"]["reply"], "OK gen=1 effective=on why=enabled "
                         "contract=2,3 codex_rotation=1 codex_rotation_five_hour=900 logins=renew "
                         "accounts=early")
        # The specified case keeps the issuer under the fixture's TLS, as every other case
        # does: no second listener is opened, and the refresh arrives at the provider's own.
        self.assertEqual(report["lostLogin"]["issuer"], "fixture-tls")
        events = json.loads((self.output / "http-events.json").read_text())
        self.assertEqual([(event.get("refreshOf"), event.get("listener"), event["status"]) for event in events
                          if event["path"].endswith("/oauth/token")], [("source", None, 401)])
        self.assertLess(seconds, 10)

    def test_a_refusal_kept_under_another_word_fails_the_verdict_after_the_loop_ended(self):
        # The stand-in keeps a reused token as `exhausted`; an oracle that expected
        # another word refuses the run, though its loop saw the wall stand.
        with mock.patch.object(renewal, "VETO_WORDS", dict(renewal.VETO_WORDS, refresh_token_reused="revoked")):
            code, report, _ = self.run_case("lost-login-reused", "login-stays")
        self.assertEqual((code, report["passed"]), (1, False))
        self.assertTrue(report["error"].startswith(
            "RuntimeError: codex-alab's kept refusals are not the one this case's refusal leaves "
            "(the source's token, as revoked): "), report["error"])
        self.assertTrue(report["error"].endswith(": exhausted"), report["error"])
        self.assertEqual(report["requestCount"], 2)
        self.assertNotIn("lostLoginStayed", report)


class MatrixCaseTests(unittest.TestCase):
    """What renewal_matrix.py counts as a passed case."""

    @staticmethod
    def result(**changes):
        return dict({"passed": True, "rawHostReturncode": 0,
                     "guardian": {"returncode": 0, "timed_out": False}}, **changes)

    def test_a_case_with_its_own_oracle_is_not_counted_without_that_oracles_record(self):
        passed = renewal_matrix.case_passed
        proof = {"wallUs": 1}
        self.assertTrue(passed("weekly-quota", 0, self.result()))
        self.assertTrue(passed("five-hour-wall-far", 0, self.result(fiveHourWallHandedOver=proof)))
        self.assertTrue(passed("five-hour-wall-far-unnamed", 0, self.result(fiveHourWallHandedOver=proof)))
        self.assertTrue(passed("five-hour-wall-soon", 0, self.result(fiveHourWallWaitedOut=proof)))
        # A far run that passed on the weekly oracle alone, or with another case's record.
        for case in ("five-hour-wall-far", "five-hour-wall-far-unnamed"):
            for record in ({}, {"fiveHourWallHandedOver": {}}, {"fiveHourWallHandedOver": True},
                           {"fiveHourWallWaitedOut": proof}):
                with self.subTest(case=case, record=record):
                    self.assertFalse(passed(case, 0, self.result(**record)))
        self.assertFalse(passed("five-hour-wall-soon", 0, self.result(fiveHourWallHandedOver=proof)))
        # Every case with an oracle of its own is in the matrix, but for the ones named
        # here. A case added to the controller with a proof and left out of the matrix
        # fails this until it is added to one list or the other, whatever table of the
        # controller it was put in.
        lost_login = {"lost-login-expired", "lost-login-expired-plain-issuer", "lost-login-invalidated",
                      "lost-login-reused", "lost-login-invalid-grant", "lost-login-unrecognised",
                      "lost-login-no-word"}
        outside = lost_login | {"five-hour-wall-far-unnamed"}
        self.assertEqual(set(renewal.CASE_PROOFS) - set(renewal_matrix.CASES), outside)
        self.assertLessEqual(set(renewal.CASE_PROOFS) - outside, set(renewal_matrix.CASES))
        # The lost-login cases only ran with a trial package, which the matrix refuses.
        # The unnamed far wall is declared and fixture-tested, but has not been run.
        self.assertEqual(lost_login, set(renewal.LOST_LOGIN_CASES))
        # Each is held to its own record wherever it is counted, and a move over the plain
        # issuer does not stand for the specified case, nor the other way about.
        self.assertFalse(passed("lost-login-expired", 0, self.result(lostLoginStayed=proof)))
        self.assertTrue(passed("lost-login-expired", 0, self.result(lostLoginMoved=proof)))
        self.assertFalse(passed("lost-login-expired", 0, self.result(lostLoginMovedOverPlainIssuer=proof)))
        self.assertFalse(passed("lost-login-expired-plain-issuer", 0, self.result(lostLoginMoved=proof)))
        self.assertTrue(passed("lost-login-expired-plain-issuer", 0,
                               self.result(lostLoginMovedOverPlainIssuer=proof)))
        self.assertFalse(passed("lost-login-invalidated", 0, self.result()))
        self.assertTrue(passed("lost-login-invalidated", 0, self.result(lostLoginStayed=proof)))

    def test_a_case_is_counted_only_on_a_clean_controller_guardian_and_host(self):
        passed = renewal_matrix.case_passed
        refused = {
            "the controller's exit": (1, self.result()),
            "no result": (0, None),
            "a result that is no object": (0, []),
            "not passed": (0, self.result(passed=False)),
            "passed, but not true": (0, self.result(passed=1)),
            "no guardian": (0, self.result(guardian=None)),
            "the guardian's exit": (0, self.result(guardian={"returncode": 1, "timed_out": False})),
            "a guardian timeout": (0, self.result(guardian={"returncode": 0, "timed_out": True})),
            "a guardian that does not say": (0, self.result(guardian={"returncode": 0})),
            "the host's exit": (0, self.result(rawHostReturncode=1)),
            "no host exit": (0, self.result(rawHostReturncode=None)),
        }
        for name, (code, result) in refused.items():
            with self.subTest(name=name):
                self.assertIs(passed("weekly-quota", code, result), False)


class MatrixInputTests(unittest.TestCase):
    def test_the_matrix_pins_every_module_that_decides_a_run(self):
        world = World(self)
        host = world.host("exit", attestation=native_host_record.CLEAN)
        # The matrix runs a case only with a package whose record qualifies one.
        world.package_records(CERTIFIED)
        names = [path.name for path in renewal_matrix.pinned_inputs(host, world.package)]
        self.assertEqual(names, ["renewal_matrix.py", "renewal_follow.py", "capacity_exec.py",
            "provider_fixture.py", "native_host_record.py", "package_standing.py",
            "command_guard.py", "cli_status.py", "file_hash.py",
            "aterm-stand-in-host", "build.json", "codex", "alab-build.json", "codex-package.json"])
        # Everything the controller imports from its own directory is among them.
        imported = {Path(module.__file__).name for module in (
            renewal, probe, provider_fixture, native_host_record, package_standing)}
        self.assertLessEqual(imported, set(names))
        # And a run writes them down: here one whose host says nothing and exits.
        output = world.root / "matrix"
        completed = subprocess.run([sys.executable, "-B", str(SMOKE / "renewal_matrix.py"),
            "--host", str(host), "--package", str(world.package), "--output", str(output),
            "--case", "weekly-quota"], capture_output=True, text=True, timeout=120)
        self.assertEqual(completed.returncode, 1, completed.stderr)
        manifest = json.loads((output / "qualification.json").read_text())
        self.assertEqual((manifest["passed"], manifest["error"]),
                         (False, "RuntimeError: paired case failed: weekly-quota"))
        self.assertEqual(manifest["inputs"], {str(path): sha256(path)
            for path in renewal_matrix.pinned_inputs(host, world.package)})
        result = json.loads((output / "weekly-quota" / "result.json").read_text())
        self.assertIn("native host exited before qualification completed", result["error"])
        self.assertIs(result["package"]["qualifies"], True)


class ExecFixtureFaultTests(unittest.TestCase):
    """exec_accounts.py and exec_shared_pool.py end at a fixture fault, not at their deadline."""

    def setUp(self):
        self.world = World(self)
        self.enterContext(mock.patch.object(probe, "COMMAND_TIMEOUT_SECONDS",
                                            probe.COMMAND_TIMEOUT_SECONDS))
        self.enterContext(mock.patch.object(renewal.Model, "usage_body", undecodable_usage))

    def fails_on_the_fault(self, main, name, *more):
        output = self.world.root / "out"
        started = time.monotonic()
        with self.assertRaisesRegex(RuntimeError, "fixture served a reply the client cannot decode "
                                    "on /api/codex/usage: FixtureFault: usage: primary_window"):
            run(main, name, "--host", self.world.host("exec"), "--package", self.world.package,
                "--output", output, *more)
        # The stand-in lives 60 s and the smokes' own deadlines are 180 s and more.
        self.assertLess(time.monotonic() - started, 30)
        report = json.loads((output / "result.json").read_text())
        self.assertIs(report["passed"], False)
        self.assertIn("FixtureFault: usage: primary_window.reset_after_seconds", report["failure"])
        self.assertEqual((report["nativeHost"]["attestation"], report["nativeHost"]["qualifies"]),
                         ("trial", False))
        self.assertEqual(report["package"], {"qualifies": False, "trial": None,
                                             "why": "its record names no compiler"})

    def test_the_exec_handoff_smoke_fails_at_once(self):
        self.fails_on_the_fault(exec_accounts.main, "exec_accounts.py")

    def test_the_shared_pool_smoke_fails_at_once(self):
        self.fails_on_the_fault(exec_shared_pool.main, "exec_shared_pool.py", "--sessions", "2")


class ParallelLoginsStandingTests(unittest.TestCase):
    """parallel_logins.py takes a package and no host: its result says what the package is worth."""

    def test_a_result_carries_its_packages_standing(self):
        world = World(self)
        expected = {
            "trial": (TRIAL_BUILD, {"qualifies": False, "trial": TRIAL,
                "why": "its record carries a `trial` object (attestation "
                       "'unverified-development-build'): the package was assembled from an "
                       "unverified development build"}),
            "certified": (CERTIFIED, {"qualifies": True, "trial": None,
                                      "why": package_standing.QUALIFIES}),
        }
        for name, (build, standing) in expected.items():
            with self.subTest(name=name):
                world.package_records(build)
                output = world.root / ("logins-" + name)
                code = run(parallel_logins.main, "parallel_logins.py", "--package",
                           world.package, "--output", output, "--count", "1")
                report = json.loads((output / "result.json").read_text())
                # The stand-in codex exits 9, so the run fails as it would with any record:
                # the standing is the record's, and the verdict is the run's.
                self.assertEqual((code, report["passed"], report["error"]), (
                    1, False, "RuntimeError: account 0 failed; see its worker/exec logs"))
                self.assertEqual(report["package"], standing)
                self.assertEqual((report["source"], report["version"]),
                                 (build["source"], build["version"]))


class ExecHostRecordTests(unittest.TestCase):
    def test_a_host_without_a_record_is_refused_before_anything_is_created(self):
        world = World(self)
        host = world.host("exec", recorded=False)
        for main, name in ((exec_accounts.main, "exec_accounts.py"),
                           (exec_shared_pool.main, "exec_shared_pool.py")):
            with self.subTest(name=name):
                output = world.root / ("out-" + name)
                with self.assertRaisesRegex(RuntimeError, "native host build record missing"):
                    run(main, name, "--host", host, "--package", world.package, "--output", output)
                self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()

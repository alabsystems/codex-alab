# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Negative controls for the production-bound pre-Signal Stop oracle."""
import copy
import unittest

from renewal_follow import validate_presignal_stop


class ParallelLoginOracle(unittest.TestCase):
    def test_a_wrong_account_or_task_cannot_count_toward_concurrency(self):
        from parallel_logins import Accounts, prompt
        accounts = Accounts(2)
        headers = {"Authorization": "Bearer fixture-access-0",
                   "ChatGPT-Account-Id": "parallel-account-0"}
        body = {"input": [{"role": "user", "content": [
            {"type": "input_text", "text": prompt(1)}]}]}
        with self.assertRaises(ValueError):
            accounts.respond(headers, body)
        with self.assertRaises(ValueError):
            accounts.identity(dict(headers, **{"ChatGPT-Account-Id": "parallel-account-1"}))
        self.assertEqual(accounts.arrived, set())

    def test_first_request_is_held_until_every_distinct_account_arrives(self):
        import threading
        from parallel_logins import Accounts, prompt
        accounts = Accounts(2)
        finished = [threading.Event(), threading.Event()]
        errors = []

        def request(index):
            try:
                accounts.respond({"Authorization": f"Bearer fixture-access-{index}",
                    "ChatGPT-Account-Id": f"parallel-account-{index}"},
                    {"input": [{"role": "user", "content": [
                        {"type": "input_text", "text": prompt(index)}]}]})
                finished[index].set()
            except ValueError as error:
                errors.append(str(error))

        first = threading.Thread(target=request, args=(0,))
        second = threading.Thread(target=request, args=(1,))
        first.start()
        try:
            with accounts.condition:
                self.assertTrue(accounts.condition.wait_for(lambda: accounts.arrived == {0}, timeout=2))
                self.assertFalse(finished[0].is_set())
            second.start()
            self.assertTrue(finished[0].wait(2))
            self.assertTrue(finished[1].wait(2))
        finally:
            with accounts.condition:
                accounts.aborted = True
                accounts.condition.notify_all()
            first.join(timeout=2)
            if second.ident is not None:
                second.join(timeout=2)
        self.assertEqual(errors, [])


class QuotaFixture(unittest.TestCase):
    def test_chain_exhausts_middle_only_after_its_failed_request(self):
        import tempfile
        from pathlib import Path
        from renewal_follow import Model
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            model = Model(root, weekly_quota=True, chain=True)
            body = {"input": [{"role": "user", "content": [
                {"type": "input_text", "text": "original"}]}],
                "tools": [{"type": "function", "name": "update_plan"}]}
            model.reply(body, "old")
            self.assertEqual(model.reply(body, "old")[0], 429)
            root.joinpath("renewed").touch()
            self.assertEqual(model.usage(False, True)["rate_limit"]["secondary_window"]["used_percent"], 0)
            self.assertEqual(model.reply(body, "new")[0], 429)
            self.assertEqual(model.usage(False, True)["rate_limit"]["secondary_window"]["used_percent"], 100)
            self.assertEqual(model.usage(False, False, True)["rate_limit"]["secondary_window"]["used_percent"], 0)
            self.assertEqual(model.reply(body, "next")[0], 200)

    def test_usage_server_requires_its_explicit_tls_root(self):
        import json
        import ssl
        import tempfile
        import urllib.error
        import urllib.request
        from pathlib import Path
        from renewal_follow import Model, serve
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            server, worker = serve(Model(root, weekly_quota=True))
            try:
                request = urllib.request.Request(
                    f"https://127.0.0.1:{server.server_port}/api/codex/usage",
                    headers={"Authorization": "Bearer fixture-access-old"})
                with self.assertRaises(urllib.error.URLError):
                    urllib.request.urlopen(request, timeout=2)
                context = ssl.create_default_context(cafile=str(root / "fixture-ca.pem"))
                with urllib.request.urlopen(request, context=context, timeout=2) as reply:
                    usage = json.load(reply)
                self.assertEqual(usage["rate_limit"]["secondary_window"]["used_percent"], 100)
                self.assertEqual(ssl.PEM_cert_to_DER_cert((root / "fixture-ca.pem").read_text()),
                                 (root / "fixture-ca.der").read_bytes())
            finally:
                server.shutdown()
                server.server_close()
                worker.join(timeout=2)

    def test_recovery_marker_cannot_mix_login_and_quota_custody(self):
        from renewal_follow import recovery_marker
        marker = {"receipt": "ab" * 16}
        self.assertEqual(recovery_marker({"rotation": marker}), marker)
        self.assertEqual(recovery_marker({"renewal": marker}), marker)
        for invalid in ({"renewal": marker, "rotation": marker}, {"rotation": True}):
            with self.assertRaises(RuntimeError):
                recovery_marker(invalid)

    def test_weekly_provider_exhausts_source_and_keeps_target_fresh(self):
        import json
        import tempfile
        from pathlib import Path
        from renewal_follow import Model
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            model = Model(root, weekly_quota=True)
            request = {"input": [{"role": "user", "content": [
                {"type": "input_text", "text": "original request"}]}],
                "tools": [{"type": "function", "name": "update_plan"}]}
            self.assertEqual(model.reply(request)[0], 200)
            status, body, *_ = model.reply(request)
            self.assertEqual(status, 429)
            self.assertEqual(json.loads(body)["error"]["type"], "usage_limit_reached")
            root.joinpath("renewed").touch()
            source = model.usage(True, False)
            target = model.usage(False, True)
            self.assertEqual(source["rate_limit"]["secondary_window"]["used_percent"], 100)
            self.assertFalse(source["rate_limit"]["allowed"])
            self.assertEqual(target["rate_limit"]["secondary_window"]["used_percent"], 0)
            self.assertTrue(target["rate_limit"]["allowed"])
            exhausted = Model(root, weekly_quota=True, all_exhausted=True)
            self.assertEqual(exhausted.usage(False, True)["rate_limit"]["secondary_window"]["used_percent"], 100)
            self.assertFalse(exhausted.usage(False, True)["rate_limit"]["allowed"])
            with self.assertRaises(RuntimeError):
                model.usage(False, False)

    def test_quota_fixture_credentials_name_distinct_native_accounts(self):
        import base64
        import json
        import tempfile
        from pathlib import Path
        from renewal_follow import fake_auth
        with tempfile.TemporaryDirectory() as directory:
            claims = []
            for label in ("source", "target"):
                path = Path(directory) / label / "auth.json"
                fake_auth(path, label, "https://127.0.0.1:1234", account=label,
                          email=f"{label}@example.invalid", user=f"{label}-user")
                auth = json.loads(path.read_text())
                encoded = auth["tokens"]["id_token"].split(".")[1]
                claim = json.loads(base64.urlsafe_b64decode(encoded + "=" * (-len(encoded) % 4)))
                self.assertEqual(claim["email"], f"{label}@example.invalid")
                self.assertEqual(claim["sub"], f"{label}-user")
                claims.append(claim["https://api.openai.com/auth"]["chatgpt_account_id"])
            self.assertEqual(claims, ["source", "target"])


class PreSignalStop(unittest.TestCase):
    def setUp(self):
        self.operation = "ab" * 16
        self.initial = {"intent": {"thread": "thread", "wall": "wall"}, "human_seq": 3,
            "native_prepare": "deadline-us=120000001", "expires_ms": 120001}
        self.state = {"id": self.operation, "intent": self.initial["intent"],
            "phase": "retired", "revoked": True, "launch_attempted": True,
            "native_prepare": self.initial["native_prepare"], "expires_ms": 120001}
        self.cancel = {"root": self.operation, "intent": self.initial["intent"],
            "reason": "owner_stop", "human_seq": 4}
        self.worker = {"operation": self.operation, "deadlineUs": 120000001, "monotonicUs": 100}
        self.observation = {"operation": self.operation, "observationTriggeredReconciliation": True,
            "state": "completed", "decision": "rollback-safe", "deadlineUs": 120000001,
            "deadlineExpired": False, "monotonicUs": 101, "signalSent": False, "signalAccepted": None,
            "resumeSent": False, "inputCustodySettled": True, "inputAttemptSettled": True,
            "goGrantedChildren": 0, "retainedChildren": 0}

    def validate(self):
        validate_presignal_stop(self.operation, self.initial, self.state,
            self.cancel, self.worker, self.observation)

    def test_durable_effect_free_closure_does_not_require_a_signal_rpc(self):
        self.validate()

    def test_cancellation_of_another_intent_or_old_person_sequence_is_rejected(self):
        for key, value in (("root", "cd" * 16), ("intent", {"wall": "other"}),
                           ("reason", "new_task"), ("human_seq", 3)):
            with self.subTest(key=key):
                old = copy.deepcopy(self.cancel)
                self.cancel[key] = value
                with self.assertRaises(RuntimeError):
                    self.validate()
                self.cancel = old

    def test_missing_terminal_proof_mutation_or_unsettled_input_is_rejected(self):
        for key, value in (("state", "active"), ("decision", "none"), ("operation", "cd" * 16),
                           ("monotonicUs", 99), ("monotonicUs", 120000001),
                           ("signalSent", True), ("signalAccepted", True), ("signalAccepted", 0), ("resumeSent", True),
                           ("inputCustodySettled", False), ("inputAttemptSettled", False),
                           ("goGrantedChildren", 1), ("retainedChildren", 1)):
            with self.subTest(key=key):
                old = self.observation.copy()
                self.observation[key] = value
                with self.assertRaises(RuntimeError):
                    self.validate()
                self.observation = old

    def test_expired_or_changed_budget_and_revived_attempt_are_rejected(self):
        self.worker["monotonicUs"] = self.worker["deadlineUs"]
        with self.assertRaises(RuntimeError):
            self.validate()
        self.worker["monotonicUs"] = 100
        self.worker["deadlineUs"] += 1
        with self.assertRaises(RuntimeError):
            self.validate()
        self.worker["deadlineUs"] -= 1
        self.state["native_prepare"] += " changed=1"
        with self.assertRaises(RuntimeError):
            self.validate()
        self.state["native_prepare"] = self.initial["native_prepare"]
        self.state["expires_ms"] += 1
        with self.assertRaises(RuntimeError):
            self.validate()
        self.state["expires_ms"] -= 1
        self.state["current_attempt"] = {"id": "child"}
        with self.assertRaises(RuntimeError):
            self.validate()


class RollbackSignals(unittest.TestCase):
    def test_two_signals_require_distinct_exact_live_predecessors_in_order(self):
        import json
        from pathlib import Path
        import tempfile
        from renewal_follow import rollback_signal_effects
        report = {"originalPid": 11, "originalProcess": {"kernelBirth": 21}}
        rollback = {"pid": 12, "kernelBirth": 22}
        effects = [{"schema": 1, "kind": "native-signal-effect", "pid": pid,
            "birth": birth, "group": pid, "signal": 15, "monotonicUs": mono}
            for pid, birth, mono in [(11, 21, 1), (12, 22, 2)]]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "native-signal-effects.jsonl"
            def write(value):
                path.write_text("".join(json.dumps(event) + "\n" for event in value))
            write(effects)
            self.assertEqual(rollback_signal_effects(root, report, rollback), effects)
            for changed in (effects[:1], effects + effects[:1], list(reversed(effects)),
                            [effects[0], dict(effects[1], pid=13)],
                            [effects[0], dict(effects[1], birth=23)],
                            [effects[0], dict(effects[1], signal=9)],
                            [effects[0], dict(effects[1], monotonicUs=1)]):
                write(changed)
                with self.assertRaises(RuntimeError):
                    rollback_signal_effects(root, report, rollback)


class PrefixCrashClosure(unittest.TestCase):
    def setUp(self):
        import hashlib
        self.boundary = {"operation": "ab" * 16, "prefix": {"pid": 41, "birth": 51,
            "parent": 31, "group": 41, "session": 31, "forward": True},
            "originalDeadlineUs": 120000001, "originalExpiresMs": 120001,
            "originalPrepareSha256": hashlib.sha256(b"immutable-plan").hexdigest()}
        self.state = {"id": self.boundary["operation"], "phase": "committing", "revoked": False,
            "candidate": self.boundary["prefix"].copy(), "native_prepare": "immutable-plan", "expires_ms": 120001}
        self.closed = {"operation": self.boundary["operation"], "observationTriggeredReconciliation": True,
            "state": "completed", "decision": "retired-cohort-exited", "deadlineExpired": True,
            "deadlineUs": 120000001, "monotonicUs": 121000001, "signalAccepted": True, "resumeSent": True,
            "sourceGroupGone": True, "retainedSourceWatch": True, "sourceWatchExited": True,
            "closedRestartWitness": True, "closedRestartQuiescent": True, "closedRestartAvailable": True,
            "inputAttemptSettled": True, "inputCustodySettled": True, "sourceBirthAlive": False,
            "goGrantedChildren": 0, "retainedChildren": 0}

    def validate(self):
        from renewal_follow import validate_prefix_crash_closure
        validate_prefix_crash_closure(self.boundary, self.state, self.closed)

    def test_go_granted_prefix_requires_positive_cohort_death_and_drain(self):
        self.validate()
        for key, value in (("decision", "retired-no-exec-grant"), ("sourceBirthAlive", None),
                           ("sourceWatchExited", False), ("closedRestartQuiescent", False),
                           ("inputCustodySettled", False), ("inputAttemptSettled", False),
                           ("goGrantedChildren", 1), ("monotonicUs", 120000000)):
            with self.subTest(key=key):
                old = self.closed.copy()
                self.closed[key] = value
                with self.assertRaises(RuntimeError):
                    self.validate()
                self.closed = old

    def test_prefix_identity_plan_and_logical_intent_cannot_change(self):
        for key, value in (("candidate", dict(self.boundary["prefix"], birth=52)),
                           ("candidate", dict(self.boundary["prefix"], pid=42)),
                           ("current_attempt", {"id": "child"}), ("native_prepare", "different"),
                           ("expires_ms", 120002), ("revoked", True), ("successor_pid", 42)):
            with self.subTest(key=key):
                old = self.state.copy()
                self.state[key] = value
                with self.assertRaises(RuntimeError):
                    self.validate()
                self.state = old


class LauncherFaultBoundary(unittest.TestCase):
    def test_fault_and_checkpoint_follow_the_inherited_launcher_home(self):
        from pathlib import Path
        import tempfile
        from renewal_follow import launcher_fixture_root
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            host_faults = root / "host-faults"
            home = root / "inherited-home"
            launcher = launcher_fixture_root({"HOME": str(home)})
            launcher.mkdir(parents=True)
            host_faults.mkdir()
            pause = launcher / "forward-after-go.pause"
            pause.touch()
            # Match the actual native_host callback: HOME/.native-renewal-fixture.
            callback_root = home / ".native-renewal-fixture"
            self.assertTrue((callback_root / pause.name).exists())
            self.assertFalse((host_faults / pause.name).exists())
            (callback_root / "forward-after-go.reached").write_text("41")
            self.assertEqual(int((launcher / "forward-after-go.reached").read_text()), 41)


class RollbackCallbackScope(unittest.TestCase):
    def test_actual_record_callback_keeps_outer_auth_path_across_other_case_assignments(self):
        import ast
        import copy
        from pathlib import Path
        source = ast.parse(Path(__file__).with_name("renewal_follow.py").read_text())
        drive = next(node for node in ast.walk(source)
            if isinstance(node, ast.FunctionDef) and node.name == "drive")
        assignment = next(node for node in ast.walk(drive) if isinstance(node, ast.Assign)
            and any(isinstance(target, ast.Name) and target.id == "rollback_record" for target in node.targets))
        def compile_callback(reintroduce_shadow):
            # Keep every real drive binding in a nonexecuted branch. Python still
            # applies their lexical scope to the actual generator callback below.
            bindings = copy.deepcopy(drive.body)
            if reintroduce_shadow:
                bindings.append(ast.Assign(targets=[ast.Name(id="old", ctx=ast.Store())], value=ast.Constant(None)))
            callback = ast.FunctionDef(name="callback",
                args=ast.arguments(posonlyargs=[], args=[ast.arg(arg=name) for name in
                    ("observed", "previous", "report")], kwonlyargs=[], kw_defaults=[], defaults=[]),
                body=[ast.If(test=ast.Constant(False), body=bindings, orelse=[]),
                    ast.Return(value=copy.deepcopy(assignment.value))], decorator_list=[])
            module = ast.fix_missing_locations(ast.Module(body=[callback], type_ignores=[]))
            namespace = {"old": Path("/owned/auth-old/auth.json")}
            exec(compile(module, "<actual-drive-record-callback>", "exec"), namespace)
            return namespace["callback"]
        record = {"pid": 2, "sessionId": "thread", "env": {"CODEX_AUTH_PATH": "/owned/auth-old/auth.json"}}
        arguments = ([record], {"pid": 1}, {"thread": "thread"})
        self.assertIs(compile_callback(False)(*arguments), record)
        with self.assertRaises(NameError):
            compile_callback(True)(*arguments)


class CaseObservationBudgets(unittest.TestCase):
    def test_real_claim_grace_rounding_fits_before_cleanup(self):
        import math
        from renewal_follow import CASE_BUDGETS
        # Source: Supervisor.ready_for waits human_grace after EVERY new claim;
        # renewal_follow schedules a 60s RECHECK for readiness Wait. g5 reached
        # initial Ready~181s, fresh Ready~421s and a successor claim~424s.
        policy_poll = 60
        rounded_grace = math.ceil(121 / policy_poll) * policy_poll
        required = rounded_grace + rounded_grace + policy_poll + rounded_grace + 120
        drive, guardian = CASE_BUDGETS["grace-beyond-budget"]
        self.assertGreaterEqual(drive, required)
        self.assertGreaterEqual(guardian - drive, 30)
        # Ordinary source admission can require two policy checks, then cold
        # successor observation another two; preserve bounded cleanup headroom.
        for case in ("happy", "lost-signal-ack", "panic-before-signal", "lost-consume-ack",
                     "late-hold", "owner-stop", "owner-stop-after-signal"):
            drive, guardian = CASE_BUDGETS[case]
            self.assertGreaterEqual(drive, 2 * policy_poll + 2 * policy_poll + 30)
            self.assertGreaterEqual(guardian - drive, 30)

    def test_fault_and_postsignal_cases_keep_their_independent_bounds(self):
        from renewal_follow import CASE_BUDGETS
        for case in ("failed-forward-rollback", "successor-prefix-crash"):
            self.assertEqual(CASE_BUDGETS[case], (450, 480))
        for case in ("post-signal-resume", "post-signal-expiry-diagnostic"):
            self.assertEqual(CASE_BUDGETS[case], (390, 420))


if __name__ == "__main__":
    unittest.main()

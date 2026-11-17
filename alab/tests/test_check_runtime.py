#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Execute runtime-smoke dispatch with inert child programs; policy behavior stays in the real smoke."""

import contextlib
import hashlib
import importlib.util
import io
import json
import os
import runpy
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
CASES = ["preflight-five-hour", "preflight-below-99", "preflight-weekly",
         "preflight-beyond-48h", "preflight-weekly-24h", "preflight-weekly-25h",
         "preflight-weekly-48h", "recovery-weekly-48h", "recovery-weekly-49h", "preflight-elapsed-reset",
         "prepost-recovery-restored", "prepost-recovery-shifted", "prepost-recovery-foreign",
         "prepost-recovery-below-99", "prepost-recovery-denied", "prepost-preflight-99"]
FIXTURE_PROBE = '''import argparse,hashlib,json,os,subprocess
from pathlib import Path
import provider_fixture
assert provider_fixture.SOURCE == "owned source fixture"
parser=argparse.ArgumentParser()
parser.add_argument("--codex",type=Path,required=True)
parser.add_argument("--output",type=Path,required=True)
parser.add_argument("--case",action="append",required=True)
args=parser.parse_args()
args.output.mkdir()
mode=os.environ.get("FIXTURE_MODE", "success")
cases=[]
for name in args.case:
    directory=args.output/name
    directory.mkdir()
    subprocess.run([str(args.codex),name,str(directory/"executed.txt")],check=True)
    positive=name in ("preflight-weekly","preflight-weekly-25h","preflight-weekly-48h",
                      "preflight-beyond-48h","recovery-weekly-48h","recovery-weekly-49h",
                      "prepost-recovery-restored",
                      "prepost-recovery-denied","prepost-preflight-99")
    consumes=int(positive and name!="prepost-recovery-restored")
    if (mode=="five-hour-redemption" and name=="preflight-five-hour"
            or mode=="24h-redemption" and name=="preflight-weekly-24h"):
        consumes=1
    models=3 if name in ("prepost-recovery-restored","prepost-recovery-denied",
                        "recovery-weekly-48h","recovery-weekly-49h") else 2
    (directory/"requests.json").write_text(json.dumps({"source":provider_fixture.SOURCE,"case":name}))
    cases.append({"case":name,"passed":True,"guardianTimedOut":False,"modelRequests":models,"postSelectionUsageReads":int(name.startswith("prepost-")),
                  "consumeRequests":consumes if mode!="bad-redemption" else 1,
                  "exit":0 if positive else 130})
    if mode=="bad-48h" and name in ("preflight-weekly-48h","recovery-weekly-48h"):
        cases[-1].update(consumeRequests=0,exit=130,modelRequests=2)
    if mode=="bad-49h" and name=="recovery-weekly-49h":
        cases[-1].update(consumeRequests=0,exit=130,modelRequests=2)
report={"passed":mode!="failure","fullSuite":False,"requestedCases":args.case,
        "binarySha256":hashlib.sha256(args.codex.read_bytes()).hexdigest(),
        "probeSha256":hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "cases":cases[:-1] if mode=="missing-case" else cases}
if mode!="missing-report":
    (args.output/"result.json").write_text(json.dumps(report))
if mode=="changed-binary":
    args.codex.write_text(args.codex.read_text()+"\\n# changed after execution\\n")
if mode in ("changed-cli-status", "changed-file-hash"):
    helper=Path(__file__).resolve().parents[1]/("file_hash.py" if mode=="changed-file-hash" else "cli_status.py")
    helper.write_text(helper.read_text()+"\\n# changed during observation\\n")
raise SystemExit(23 if mode=="failure" else 0)
'''


FIXTURE_HISTORY = '''import argparse,hashlib,json,os,subprocess
from pathlib import Path
parser=argparse.ArgumentParser()
parser.add_argument("--codex",type=Path,required=True)
parser.add_argument("--output",type=Path,required=True)
args=parser.parse_args()
args.output.mkdir()
subprocess.run([str(args.codex),"history",str(args.output/"executed.txt")],check=True)
report={"passed":True,"sha256":hashlib.sha256(args.codex.read_bytes()).hexdigest(),
        "turns":260,"model_requests_after_seed":260,"older_history_page_requests":3,
        "custody":{"exit":0,"owning_reap":True,"group_absent_after_reap":True,"forced":False}}
mode=os.environ.get("FIXTURE_MODE")
if mode=="bad-history":
    report["custody"]["owning_reap"]=False
if mode=="failed-history":
    logs=args.output/"logs"
    logs.mkdir()
    (logs/"codex-tui.log").write_text("retained history failure\\n")
    for name in ("home","codex-home"):
        private=args.output/name
        private.mkdir()
        (private/"auth.json").write_text("private fake-auth fixture; do not retain")
        (private/"state.sqlite").write_text("private database fixture; do not retain")
    (logs/"linked-auth.json").symlink_to(args.output/"codex-home/auth.json")
    report["passed"]=False
(args.output/"result.json").write_text(json.dumps(report))
if mode in ("changed-history-cli-status", "changed-history-file-hash"):
    helper=Path(__file__).resolve().parents[1]/("file_hash.py" if mode=="changed-history-file-hash" else "cli_status.py")
    helper.write_text(helper.read_text()+"\\n# changed during history observation\\n")
raise SystemExit(29 if mode=="failed-history" else 0)
'''


class RuntimePolicyContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        spec = importlib.util.spec_from_file_location('runtime_policy_controller', ROOT / 'alab/check_runtime.py')
        cls.runtime = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.runtime)
        sys.path.insert(0, str(ROOT / 'alab/smoke'))
        spec = importlib.util.spec_from_file_location('runtime_policy_weekly', ROOT / 'alab/smoke/weekly_reset.py')
        cls.weekly = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.weekly)

    def backend(self, fixture, method, endpoint, body=None):
        authentication = {'authorizationPresent': True, 'authorizationMatches': True,
                          'accountPresent': True, 'accountMatches': True}
        return fixture.reply(method, '/backend-api/wham/' + endpoint, body, authentication=authentication)

    def usage(self, fixture):
        code, data, _content = self.backend(fixture, 'GET', 'usage')
        self.assertEqual(code, 200)
        return json.loads(data)

    def model_inputs(self):
        first = {'model': self.weekly.MODEL, 'input': [{'type': 'message', 'role': 'user',
                 'content': [{'type': 'input_text', 'text': self.weekly.PROMPT}]}]}
        second = {'model': self.weekly.MODEL, 'input': first['input'] + [
            {'type': 'function_call_output', 'call_id': self.weekly.CALL_ID, 'output': 'Plan updated'}]}
        return first, second

    def seed_usage_wall(self, fixture):
        first, second = self.model_inputs()
        self.assertEqual(fixture.reply('POST', '/v1/responses', first)[0], 200)
        self.assertEqual(fixture.reply('POST', '/v1/responses', second)[0], 429)
        return second

    def consume(self, fixture):
        return self.backend(fixture, 'POST', 'rate-limit-reset-credits/consume',
                            {'redeem_request_id': 'a' * 64, 'credit_id': 'earned-weekly-credit'})

    def test_every_gate_case_is_dispatched_by_the_real_weekly_parser(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            executable = root / 'unexecuted-binary'
            executable.write_bytes(b'parser control; never executed')
            output = root / 'result'
            arguments = ['weekly_reset.py', '--codex', str(executable), '--output', str(output)]
            for name in self.runtime.CASES:
                arguments.extend(('--case', name))
            with mock.patch.object(sys, 'argv', arguments), \
                    mock.patch.object(self.weekly, 'case', side_effect=lambda _binary, _output, name:
                                      {'case': name, 'passed': True}) as execute, \
                    contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(self.weekly.cli(), 0)
            self.assertEqual([call.args[2] for call in execute.call_args_list], list(self.runtime.CASES))
            self.assertEqual(json.loads((output / 'result.json').read_text())['requestedCases'], list(self.runtime.CASES))

    def test_five_hour_and_exact_24h_are_real_preservation_fixtures(self):
        # These requirements are independent of the probe's category lists and
        # the inert dispatch fixture. The actual model must reject a POST.
        scenarios = [('preflight-five-hour', 99, 0, 72),
                     ('preflight-weekly-24h', 0, 99, 24)]
        with mock.patch.object(self.weekly.time, 'time', return_value=1_800_000_000):
            for name, five_hour_used, week_used, hours in scenarios:
                with self.subTest(name=name):
                    self.assertIn(name, self.runtime.CASES)
                    self.assertEqual(self.runtime.EXPECTATIONS[name], (2, 0, False))
                    fixture = self.weekly.Mock(name)
                    before = self.usage(fixture)
                    self.assertTrue(before['rate_limit']['allowed'])
                    self.assertEqual(before['rate_limit']['primary_window']['used_percent'], five_hour_used)
                    self.assertEqual(before['rate_limit']['secondary_window']['used_percent'], week_used)
                    self.seed_usage_wall(fixture)
                    denied = self.usage(fixture)
                    self.assertFalse(denied['rate_limit']['allowed'])
                    self.assertEqual(denied['rate_limit']['secondary_window']['reset_after_seconds'], hours * 3600)
                    code, inventory, _content = self.backend(fixture, 'GET', 'rate-limit-reset-credits')
                    self.assertEqual(code, 200)
                    self.assertEqual(json.loads(inventory)['available_count'], 1)
                    self.usage(fixture)
                    with self.assertRaisesRegex(RuntimeError, 'more than 24 hours'):
                        self.consume(fixture)
                    self.assertEqual(len(fixture.models), 2)
                    self.assertTrue(fixture.credit)
                    self.assertFalse(fixture.consumed)
                    self.assertEqual(fixture.spent, [])

    def test_beyond_24h_positive_controls_use_the_real_preflight_and_recovery_models(self):
        with mock.patch.object(self.weekly.time, 'time', return_value=1_800_000_000):
            for name, reactive, models, hours in [('preflight-weekly-25h', False, 2, 25),
                                                  ('preflight-weekly-48h', False, 2, 48),
                                                  ('preflight-beyond-48h', False, 2, 49),
                                                  ('recovery-weekly-48h', True, 3, 48),
                                                  ('recovery-weekly-49h', True, 3, 49)]:
                with self.subTest(name=name):
                    self.assertIn(name, self.runtime.CASES)
                    self.assertEqual(self.runtime.EXPECTATIONS[name], (models, 1, True))
                    fixture = self.weekly.Mock(name)
                    if reactive:
                        retained = self.seed_usage_wall(fixture)
                    reading = self.usage(fixture)
                    week = reading['rate_limit']['secondary_window']
                    self.assertEqual((week['used_percent'], week['limit_window_seconds'], week['reset_after_seconds']),
                                     (99, 604800, hours * 3600))
                    self.assertEqual(reading['rate_limit']['allowed'], not reactive)
                    self.backend(fixture, 'GET', 'rate-limit-reset-credits')
                    self.usage(fixture)
                    code, body, _content = self.consume(fixture)
                    self.assertEqual((code, json.loads(body)['code']), (200, 'reset'))
                    self.assertEqual(fixture.spent, ['earned-weekly-credit'])
                    self.assertTrue(self.usage(fixture)['rate_limit']['allowed'])
                    if not reactive:
                        first, retained = self.model_inputs()
                        self.assertEqual(fixture.reply('POST', '/v1/responses', first)[0], 200)
                    code, body, _content = fixture.reply('POST', '/v1/responses', retained)
                    self.assertEqual(code, 200)
                    self.assertIn(self.weekly.ANSWER.encode(), body)
                    self.assertEqual(len(fixture.models), models)


class RuntimeDispatchTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="alab-runtime-dispatch-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.repo = self.root / "source with spaces"
        smoke = self.repo / "alab/smoke"
        smoke.mkdir(parents=True)
        shutil.copy2(ROOT / "alab/check_runtime.py", self.repo / "alab/check_runtime.py")
        shutil.copy2(ROOT / "alab/cli_status.py", self.repo / "alab/cli_status.py")
        shutil.copy2(ROOT / "alab/file_hash.py", self.repo / "alab/file_hash.py")
        (self.repo / "alab/command_guard.py").write_text("# inert dispatch fixture\n")
        (smoke / "provider_fixture.py").write_text('SOURCE="owned source fixture"\n')
        (smoke / "weekly_reset.py").write_text(FIXTURE_PROBE)
        (smoke / "history_pagination.py").write_text(FIXTURE_HISTORY)
        for name in ("capacity_exec.py", "quota_resume.py"):
            (smoke / name).write_text("# inert dispatch fixture; no native ownership\n")
        self.executable = self.root / "new codex"
        self.executable.write_text('#!/bin/sh\nprintf "%s\\n" "$1" >> "$2"\n')
        self.executable.chmod(0o755)
        self.env = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "HOME": str(self.root),
                    "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1",
                    "GIT_AUTHOR_NAME": "Fixture", "GIT_AUTHOR_EMAIL": "fixture@example.invalid",
                    "GIT_COMMITTER_NAME": "Fixture", "GIT_COMMITTER_EMAIL": "fixture@example.invalid"}
        self.git("init", "-q", "--template=", "-b", "main")
        self.git("add", ".")
        self.git("commit", "-qm", "runtime dispatch fixture")
        self.source = self.git("rev-parse", "HEAD")
        self.scratch = self.root / "short scratch"
        self.output = self.root / "retained evidence"

    def git(self, *args):
        return subprocess.check_output(["/usr/bin/git", "-C", str(self.repo), *args],
                                       env=self.env, text=True).strip()

    def run_smoke(self, mode="success", source=None, **env):
        return subprocess.run([sys.executable, "-I", "-B", "-S", str(self.repo / "alab/check_runtime.py"),
                               "--source", source or self.source, "--codex", str(self.executable),
                               "--scratch", str(self.scratch), "--output", str(self.output)],
                              cwd=self.repo, env=dict(self.env, FIXTURE_MODE=mode, **env),
                              capture_output=True, text=True, timeout=20)

    def test_dispatch_executes_each_case_and_retains_source_and_binary_bound_evidence(self):
        poison = self.root / "inherited python modules"
        poison.mkdir()
        (poison / "provider_fixture.py").write_text('raise RuntimeError("inherited fixture imported")\n')
        (poison / "file_hash.py").write_text('raise RuntimeError("inherited hash helper imported")\n')
        (poison / "sitecustomize.py").write_text('raise RuntimeError("inherited site imported")\n')
        done = self.run_smoke(PYTHONPATH=str(poison), PYTHONUSERBASE=str(poison))
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        result = json.loads((self.output / "result.json").read_text())
        self.assertIs(result["passed"], True)
        self.assertEqual(result["source"], self.source)
        self.assertEqual(result["requested_cases"], CASES)
        self.assertEqual(result["binary_sha256"], hashlib.sha256(self.executable.read_bytes()).hexdigest())
        self.assertIn("alab/cli_status.py", result["probe_inputs"])
        self.assertEqual(result["probe_inputs"]["alab/cli_status.py"],
                         hashlib.sha256((self.repo / "alab/cli_status.py").read_bytes()).hexdigest())
        self.assertEqual(result["probe_inputs"]["alab/file_hash.py"],
                         hashlib.sha256((self.repo / "alab/file_hash.py").read_bytes()).hexdigest())
        self.assertEqual((self.output / "smoke/history/executed.txt").read_text(), "history\n")
        self.assertIn("history_report_sha256", result)
        for name in CASES:
            self.assertEqual((self.output / "smoke" / name / "executed.txt").read_text(), name + "\n")
            self.assertEqual(json.loads((self.output / "smoke" / name / "requests.json").read_text())["source"],
                             "owned source fixture")

    def test_changed_shared_cli_policy_cannot_leave_a_passing_runtime_record(self):
        helper = self.repo / "alab/cli_status.py"
        before = helper.read_bytes()
        done = self.run_smoke("changed-cli-status")
        self.assertNotEqual(done.returncode, 0, done.stdout + done.stderr)
        result = json.loads((self.output / "result.json").read_text())
        self.assertIs(result["passed"], False)
        self.assertIn("source inputs changed", result["error"])
        self.assertEqual(result["probe_inputs"]["alab/cli_status.py"], hashlib.sha256(before).hexdigest())
        self.assertNotEqual(helper.read_bytes(), before)
        self.assertEqual(result["history_exit"], 0)

    def test_changed_hash_helper_in_either_probe_cannot_leave_a_passing_runtime_record(self):
        helper = self.repo / "alab/file_hash.py"
        before = helper.read_bytes()
        for mode in ("changed-file-hash", "changed-history-file-hash"):
            with self.subTest(mode=mode):
                helper.write_bytes(before)
                self.output = self.root / (mode + " evidence")
                self.scratch = self.root / (mode + " scratch")
                done = self.run_smoke(mode)
                self.assertNotEqual(done.returncode, 0, done.stdout + done.stderr)
                result = json.loads((self.output / "result.json").read_text())
                self.assertIs(result["passed"], False)
                self.assertIn("source inputs changed", result["error"])
                self.assertEqual(result["probe_inputs"]["alab/file_hash.py"], hashlib.sha256(before).hexdigest())
                self.assertNotEqual(helper.read_bytes(), before)

    def test_failed_runtime_child_propagates_its_failure_and_keeps_diagnostics(self):
        done = self.run_smoke("failure")
        self.assertEqual(done.returncode, 23, done.stdout + done.stderr)
        result = json.loads((self.output / "result.json").read_text())
        self.assertIs(result["passed"], False)
        self.assertEqual(result["exit"], 23)
        self.assertTrue((self.output / "smoke/result.json").is_file())

    def test_failed_history_keeps_its_explicit_log_after_scratch_disposal_only(self):
        done = self.run_smoke("failed-history")
        self.assertEqual(done.returncode, 29, done.stdout + done.stderr)
        result = json.loads((self.output / "result.json").read_text())
        self.assertIs(result["passed"], False)
        self.assertEqual(result["history_exit"], 29)
        self.assertEqual((self.output / "smoke/history/executed.txt").read_text(), "history\n")
        retained = self.output / "smoke/history/logs/codex-tui.log"
        self.assertFalse(retained.is_symlink())
        shutil.rmtree(self.scratch)
        self.assertEqual(retained.read_text(), "retained history failure\n")
        for private in ("home", "codex-home", "logs/linked-auth.json"):
            self.assertFalse((self.output / "smoke/history" / private).exists())
            self.assertFalse((self.output / "smoke/history" / private).is_symlink())

    def test_zero_exit_cannot_hide_missing_cases_or_a_redemption_in_a_negative_case(self):
        for mode in ("missing-report", "missing-case", "bad-redemption", "five-hour-redemption",
                     "24h-redemption", "bad-48h", "bad-49h", "changed-binary",
                     "changed-cli-status", "changed-history-cli-status", "bad-history"):
            with self.subTest(mode=mode):
                self.output = self.root / (mode + " evidence")
                self.scratch = self.root / (mode + " scratch")
                done = self.run_smoke(mode)
                self.assertNotEqual(done.returncode, 0, done.stdout + done.stderr)
                self.assertIs(json.loads((self.output / "result.json").read_text())["passed"], False)

    def test_runtime_case_arguments_reach_the_real_probe_before_executable_validation(self):
        dispatch = runpy.run_path(str(ROOT / "alab/check_runtime.py"))
        output = self.root / "real probe admission"
        missing = self.root / "absent codex"
        command = [sys.executable, "-I", "-B", "-S", "-c", dispatch["BOOTSTRAP"],
                   str(ROOT / "alab/smoke/weekly_reset.py"), "--codex", str(missing),
                   "--output", str(output)]
        for name in dispatch["CASES"]:
            command.extend(("--case", name))
        done = subprocess.run(command, cwd=self.repo, env=self.env,
                              capture_output=True, text=True, timeout=20)
        self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
        report = json.loads((output / "result.json").read_text())
        self.assertEqual(report["requestedCases"], list(dispatch["CASES"]))
        self.assertIs(report["passed"], False)
        self.assertEqual(report["cases"], [])
        self.assertIn("FileNotFoundError", report["error"])
        self.assertFalse(missing.exists())

    def test_stale_evidence_or_wrong_source_never_executes_the_probe(self):
        done = self.run_smoke(source="f" * 40)
        self.assertNotEqual(done.returncode, 0)
        self.assertFalse(self.scratch.exists())
        self.output.mkdir()
        (self.output / "result.json").write_text('{"passed":true}')
        done = self.run_smoke()
        self.assertNotEqual(done.returncode, 0)
        self.assertFalse(self.scratch.exists())
        self.assertEqual((self.output / "result.json").read_text(), '{"passed":true}')


if __name__ == "__main__":
    unittest.main()

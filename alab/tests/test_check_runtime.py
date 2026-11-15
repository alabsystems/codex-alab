#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Execute runtime-smoke dispatch with inert child programs; policy behavior stays in the real smoke."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
CASES = ["preflight-five-hour", "preflight-below-99", "preflight-weekly",
         "preflight-beyond-24h", "preflight-weekly-24h", "preflight-weekly-25h", "preflight-elapsed-reset",
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
    positive=name in ("preflight-five-hour","preflight-weekly","preflight-weekly-24h",
                      "preflight-beyond-24h","preflight-weekly-25h",
                      "prepost-recovery-restored",
                      "prepost-recovery-denied","prepost-preflight-99")
    consumes=int(positive and name!="prepost-recovery-restored")
    models=3 if name in ("prepost-recovery-restored","prepost-recovery-denied") else 2
    (directory/"requests.json").write_text(json.dumps({"source":provider_fixture.SOURCE,"case":name}))
    cases.append({"case":name,"passed":True,"guardianTimedOut":False,"modelRequests":models,"postSelectionUsageReads":int(name.startswith("prepost-")),
                  "consumeRequests":consumes if mode!="bad-redemption" else 1,
                  "exit":0 if positive else 130})
report={"passed":mode!="failure","fullSuite":False,"requestedCases":args.case,
        "binarySha256":hashlib.sha256(args.codex.read_bytes()).hexdigest(),
        "probeSha256":hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "cases":cases[:-1] if mode=="missing-case" else cases}
if mode!="missing-report":
    (args.output/"result.json").write_text(json.dumps(report))
if mode=="changed-binary":
    args.codex.write_text(args.codex.read_text()+"\\n# changed after execution\\n")
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
raise SystemExit(29 if mode=="failed-history" else 0)
'''


class RuntimeDispatchTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="alab-runtime-dispatch-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.repo = self.root / "source with spaces"
        smoke = self.repo / "alab/smoke"
        smoke.mkdir(parents=True)
        shutil.copy2(ROOT / "alab/check_runtime.py", self.repo / "alab/check_runtime.py")
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
        (poison / "sitecustomize.py").write_text('raise RuntimeError("inherited site imported")\n')
        done = self.run_smoke(PYTHONPATH=str(poison), PYTHONUSERBASE=str(poison))
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        result = json.loads((self.output / "result.json").read_text())
        self.assertIs(result["passed"], True)
        self.assertEqual(result["source"], self.source)
        self.assertEqual(result["requested_cases"], CASES)
        self.assertEqual(result["binary_sha256"], hashlib.sha256(self.executable.read_bytes()).hexdigest())
        self.assertEqual((self.output / "smoke/history/executed.txt").read_text(), "history\n")
        self.assertIn("history_report_sha256", result)
        for name in CASES:
            self.assertEqual((self.output / "smoke" / name / "executed.txt").read_text(), name + "\n")
            self.assertEqual(json.loads((self.output / "smoke" / name / "requests.json").read_text())["source"],
                             "owned source fixture")

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
        for mode in ("missing-report", "missing-case", "bad-redemption", "changed-binary", "bad-history"):
            with self.subTest(mode=mode):
                self.output = self.root / (mode + " evidence")
                self.scratch = self.root / (mode + " scratch")
                done = self.run_smoke(mode)
                self.assertNotEqual(done.returncode, 0, done.stdout + done.stderr)
                self.assertIs(json.loads((self.output / "result.json").read_text())["passed"], False)

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

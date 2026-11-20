# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Run the selected CLI against the candidate's real Kimi and quota smoke servers."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

workspace = Path(os.environ['ALAB_FEATURE_WORKSPACE'])
binary = Path(os.environ['ALAB_FEATURE_BINARY'])
results = Path(os.environ['ALAB_FEATURE_RESULTS'])
smoke = workspace / 'alab/smoke'
spec = importlib.util.spec_from_file_location('candidate_kimi', smoke / 'kimi.py')
kimi = importlib.util.module_from_spec(spec)
spec.loader.exec_module(kimi)


def binary_hash():
    with binary.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


class KimiWireControls(unittest.TestCase):
    def exchange(self, message, expect_success):
        before = binary_hash()
        mock = kimi.Mock()
        mock.failures.append(message)
        server = kimi.serve(mock)
        try:
            with tempfile.TemporaryDirectory() as directory:
                kimi.run_model(binary, Path(directory), server.server_port, 'kimi', 'k3-256k',
                               expect_success=expect_success, label='wire')
            self.assertEqual(mock.failures, [])
            self.assertEqual(len(mock.requests), 3 if expect_success else 1)
            self.assertEqual(mock.authorization,
                             ['Bearer loopback-kimi-subscription-key'] * len(mock.requests))
            self.assertEqual(binary_hash(), before)
        finally:
            server.shutdown()
            server.server_close()

    def test_transient_overload_retries_once_and_finishes_the_same_turn(self):
        self.exchange('The engine is currently overloaded, please try again later', True)

    def test_five_hour_quota_is_terminal_on_the_wire(self):
        self.exchange("You've reached your 5-hour usage limit", False)

    def test_weekly_quota_is_terminal_on_the_wire(self):
        self.exchange("You've reached your weekly (7-day) usage limit", False)

    def test_monthly_quota_is_terminal_on_the_wire(self):
        self.exchange("You've reached your monthly usage limit for this billing cycle", False)

    def test_extra_usage_quota_is_terminal_on_the_wire(self):
        self.exchange("Your organization's Extra Usage balance has run out", False)


class WeeklyQuotaControls(unittest.TestCase):
    def test_preflight_and_reactive_24_and_25_hour_boundaries(self):
        cases = [f'{path}-weekly-{hours}h' for path in ('preflight', 'recovery')
                 for hours in (24, 25)]
        before = binary_hash()
        # Under isolated Python the smoke's sibling modules need its directory on sys.path.
        bootstrap = ('import runpy,sys;from pathlib import Path;'
                     'p=Path(sys.argv.pop(1));sys.path.insert(0,str(p.parent));'
                     'runpy.run_path(str(p),run_name="__main__")')
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / 'weekly'
            command = [sys.executable, '-I', '-B', '-S', '-c', bootstrap,
                       str(smoke / 'weekly_reset.py'), '--codex', str(binary), '--output', str(output)]
            for case in cases:
                command.extend(['--case', case])
            subprocess.run(command, check=True)
            raw = (output / 'result.json').read_bytes()
            report = json.loads(raw)
            self.assertEqual(binary_hash(), before)
            self.assertEqual(report['binarySha256'], before)
            self.assertTrue(report['passed'])
            self.assertFalse(report['fullSuite'])
            self.assertCountEqual(report['requestedCases'], cases)
            self.assertCountEqual([row['case'] for row in report['cases']], cases)
            for row in report['cases']:
                self.assertEqual(row, json.loads((output / row['case'] / 'result.json').read_bytes()))
                self.assertTrue(row['passed'])
                self.assertFalse(row['guardianTimedOut'])
                self.assertGreater(row['authenticatedBackendRequests'], 0)
                self.assertEqual(row['consumeRequests'], int(row['case'].endswith('25h')))
            with (results / 'weekly-wire.json').open('xb') as stream:
                stream.write(raw)

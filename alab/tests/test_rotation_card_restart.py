# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
from contextlib import ExitStack
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'smoke'))
import rotation_card_restart as restart


class DestinationCardTests(unittest.TestCase):
    def setUp(self):
        self.pid, self.birth = 123, 456000007
        self.home = Path('/fixture')
        self.report = dict(original_input_and_completed_plan_preserved=True, successor_pid=self.pid,
                           thread='thread', codex_sha256='image')
        self.card = mock.Mock()
        self.card.open.return_value = 100
        self.card.clock.return_value = 100.2
        self.ctl = mock.Mock(return_value='OK restarting pid 123 cause=asked')

    def simulated_kernel(self):
        patches = self.enterContext(ExitStack())
        kernel = patches.enter_context(mock.patch.object(restart, 'select'))
        kernel.KQ_FILTER_PROC, kernel.KQ_NOTE_EXIT, kernel.KQ_EV_ERROR = -5, 4, 32
        kernel.KQ_EV_ADD, kernel.KQ_EV_ONESHOT = 1, 16
        self.watcher = kernel.kqueue.return_value
        self.event = SimpleNamespace(ident=self.pid, filter=-5, flags=0, fflags=4)
        self.watcher.control.side_effect = [[], [], [self.event]]
        self.live_birth = patches.enter_context(mock.patch.object(restart, 'live_birth', return_value=self.birth))
        self.records = patches.enter_context(mock.patch.object(restart.fixture, 'records', return_value=[
            {'pid': self.pid, 'sessionId': 'thread'}]))
        patches.enter_context(mock.patch.object(restart.fixture, 'process_evidence', return_value={
            'image': {'sha256': 'image'}, 'kernelBirth': self.birth}))

    def run_check(self):
        restart.verify_claim_loss(self.card, self.ctl, pid=self.pid, birth=self.birth,
                                  thread='thread', codex_home=self.home, report=self.report)

    def test_birth_thread_receipt_and_unique_record_are_required_before_restart(self):
        self.simulated_kernel()
        original = self.records.return_value
        for records, birth, receipt_pid in ((original, self.birth + 1, self.pid),
                                            (original, self.birth, self.pid + 1),
                                            ([{'pid': self.pid, 'sessionId': 'elsewhere'}], self.birth, self.pid),
                                            (original * 2, self.birth, self.pid)):
            with self.subTest(records=records, birth=birth, receipt_pid=receipt_pid):
                self.records.return_value = records
                self.live_birth.return_value = birth
                self.report['successor_pid'] = receipt_pid
                with self.assertRaises(RuntimeError):
                    self.run_check()
                self.ctl.assert_not_called()
                self.card.open.assert_not_called()

    def test_destination_is_rechecked_after_show_and_before_asking_for_restart(self):
        self.simulated_kernel()
        self.live_birth.side_effect = [self.birth, self.birth, None]
        with self.assertRaises(RuntimeError):
            self.run_check()
        self.card.require_open.assert_called_once()
        self.ctl.assert_not_called()

    def test_only_the_acknowledged_plain_restart_of_the_observed_pid_qualifies(self):
        self.simulated_kernel()
        for reply in ('OK restart gone pid 123', 'OK restarting pid 124 cause=asked',
                      'OK restarting pid 123 cause=asked account=private', 'OK restarting pid 123 cause=stall',
                      'OK restarting pid 123 cause=asked\nprivate', 'OK restarting pid 123 cause=asked discarded=4'):
            with self.subTest(reply=reply):
                self.watcher.control.side_effect = [[], []]
                self.ctl.return_value = reply
                with self.assertRaises(RuntimeError) as error:
                    self.run_check()
                self.assertNotIn('private', str(error.exception))
                self.card.await_withdrawal.assert_not_called()
                self.assertNotIn('destination_card_claim_loss', self.report)

    def test_other_kernel_event_and_late_exit_do_not_prove_withdrawal(self):
        self.simulated_kernel()
        self.event.ident += 1
        with self.assertRaises(RuntimeError):
            self.run_check()
        self.card.await_withdrawal.assert_not_called()
        self.watcher.control.side_effect = [[], [], []]
        self.card.clock.side_effect = [101, 102, 128]
        with self.assertRaises(RuntimeError):
            self.run_check()
        self.assertNotIn('destination_card_claim_loss', self.report)

    def test_no_restart_if_the_card_observation_already_timed_out(self):
        self.simulated_kernel()
        self.card.clock.return_value = 128
        with self.assertRaises(RuntimeError):
            self.run_check()
        self.ctl.assert_not_called()

    def test_zero_discarded_acknowledges_the_plain_restart_after_exit_watch_is_armed(self):
        self.simulated_kernel()
        def open_card():
            self.watcher.control.assert_called_once()
            return 100
        self.card.open.side_effect = open_card
        self.ctl.return_value = 'OK restarting pid 123 cause=asked discarded=0'
        self.run_check()
        self.ctl.assert_called_once_with('restart', 'pid=123')
        self.card.await_withdrawal.assert_called_once_with(100)
        self.assertEqual(self.report['destination_card_claim_loss'], {
            'pid': 123, 'guarded_restart': True, 'kernel_exit': True,
            'withdrawn_before_expiry': True, 'elapsed_ms': 200,
        })

    @unittest.skipUnless(sys.platform == 'darwin', 'requires Darwin process birth and kqueue')
    def test_real_kernel_birth_and_exit_of_the_private_process(self):
        self.card.clock.side_effect = time.monotonic
        self.card.open.side_effect = time.monotonic
        with tempfile.TemporaryDirectory() as tmp, subprocess.Popen(
                [sys.executable, '-c', 'import sys; sys.stdin.buffer.read()'], stdin=subprocess.PIPE) as process:
            self.pid = self.report['successor_pid'] = process.pid
            self.birth = restart.live_birth(self.pid)
            self.assertIsInstance(self.birth, int)
            sec, micros = divmod(self.birth, 1_000_000)
            self.home = Path(tmp)
            sessions = self.home / 'alab/sessions'
            sessions.mkdir(parents=True)
            record = dict(pid=self.pid, sessionId='thread', exe=sys.executable,
                          identity=dict(startSeconds=sec, startMicroseconds=micros))
            (sessions / 'process.json').write_text(json.dumps(record))
            self.report['codex_sha256'] = restart.fixture.process_evidence(record)['image']['sha256']

            def release(*command):
                self.assertEqual(command, ('restart', f'pid={self.pid}'))
                process.stdin.close()
                return f'OK restarting pid {self.pid} cause=asked'

            self.ctl.side_effect = release
            try:
                self.run_check()
            finally:
                process.stdin.close()
            self.assertTrue(self.report['destination_card_claim_loss']['kernel_exit'])
            self.card.await_withdrawal.assert_called_once()

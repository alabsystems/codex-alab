# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The weekly-reset smoke must observe one uninterrupted Codex process."""

from contextlib import closing
import os
from pathlib import Path
import select
import socket
import sqlite3
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'smoke'))
import tui_goal_reset as smoke


class ResetFailureEvidenceTests(unittest.TestCase):
    def test_a_failed_run_keeps_the_reset_and_last_saved_goal_before_teardown(self):
        for state in ('usage_limited', 'active', 'cleared', 'unreadable'):
            with self.subTest(state=state), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                (root / 'codex-home').mkdir()
                home = root / 'private-home'
                db = home / 'goals_1.sqlite'
                thread = 'original-thread'
                reader, writer = socket.socketpair()
                model = smoke.GoalProvider(writer)
                report = {}

                def seed(*args):
                    with closing(sqlite3.connect(db)) as conn:
                        conn.execute('CREATE TABLE thread_goals(thread_id, goal_id, objective, status)')
                        conn.execute('INSERT INTO thread_goals VALUES (?, ?, ?, ?)',
                                     (thread, 'original-id', smoke.OBJECTIVE, 'active'))
                        conn.commit()
                    return thread

                def fail_after_wall(*args):
                    model.release_wall.set()
                    self.assertEqual(model.reply({'input': smoke.OBJECTIVE})[0], 429)
                    with closing(sqlite3.connect(db)) as conn:
                        if state == 'cleared':
                            conn.execute('DELETE FROM thread_goals')
                        elif state == 'unreadable':
                            conn.execute('DROP TABLE thread_goals')
                        else:
                            conn.execute('UPDATE thread_goals SET status=?', (state,))
                        conn.commit()
                    raise RuntimeError('lost the private host')

                with reader, writer, mock.patch.object(smoke, 'seed_idle_goal', seed):
                    with self.assertRaisesRegex(RuntimeError, 'lost the private host') as failure:
                        smoke.observe_goal_reset(root, home, None, fail_after_wall, None, {}, report, reader, model)
                self.assertEqual(report['reset_unix'], model.reset)
                self.assertEqual(len(report['sampling_requests']), 1)
                if state == 'unreadable':
                    self.assertNotIn('goal_at_failure', report)
                    self.assertIn('goal at failure', failure.exception.__notes__[0])
                else:
                    expected = None if state == 'cleared' else {'goal_id': 'original-id', 'status': state}
                    self.assertEqual(report['goal_at_failure'], expected)


@unittest.skipUnless(sys.platform == 'darwin', 'the private packaged TUI smoke runs on Darwin')
class ProcessContinuityTests(unittest.TestCase):
    def test_provider_retry_wakes_one_kernel_wait_even_if_already_recorded(self):
        for already_recorded in (False, True):
            with self.subTest(already_recorded=already_recorded):
                reader, writer = socket.socketpair()
                with reader, writer, closing(select.kqueue()) as original, closing(select.kqueue()) as gate:
                    provider = smoke.GoalProvider(writer)
                    body = {'input': smoke.OBJECTIVE}
                    provider.release_wall.set()
                    provider.release_answer.set()
                    self.assertEqual(provider.reply(body)[0], 429)
                    self.assertFalse(select.select([reader], [], [], 0)[0])
                    armed = threading.Event()
                    result = []

                    def second_request():
                        if already_recorded or armed.wait(5):
                            result.append(provider.reply(body)[0])

                    def wait(changes, count, timeout):
                        armed.set()
                        return gate.control(changes, count, timeout)

                    kernel = mock.Mock(wraps=gate)
                    kernel.control.side_effect = wait
                    sender = threading.Thread(target=second_request)
                    sender.start()
                    try:
                        if already_recorded:
                            sender.join(5)
                        with mock.patch.object(smoke.select, 'kqueue', return_value=kernel):
                            smoke.wait_for_automatic_retry(original, mock.Mock(pid=os.getpid(), poll=lambda: None),
                                                           reader, timeout=5)
                    finally:
                        armed.set()
                        sender.join(5)
                    kernel.control.assert_called_once()
                    self.assertEqual(result, [200])
                    self.assertEqual(len(provider.observed()), 2)

    def test_private_host_exit_and_a_missing_retry_have_distinct_failures(self):
        reader, writer = socket.socketpair()
        with reader, writer, closing(select.kqueue()) as original:
            with self.assertRaisesRegex(RuntimeError, 'timed out waiting for automatic sampling'):
                smoke.wait_for_automatic_retry(original, mock.Mock(pid=os.getpid(), poll=lambda: None),
                                               reader, timeout=0)
            with subprocess.Popen([sys.executable, '-c', 'pass']) as host:
                with self.assertRaisesRegex(RuntimeError, 'private aterm host exited'):
                    smoke.wait_for_automatic_retry(original, host, reader, timeout=5)

    def test_reexec_of_the_same_binary_and_pid_invalidates_the_wake(self):
        replacement = 'import sys; print("replacement", flush=True); sys.stdin.readline()'
        program = (
            'import os, sys; print("original", flush=True); sys.stdin.readline(); '
            'os.execv(sys.executable, [sys.executable, "-c", sys.argv[1]])'
        )
        with subprocess.Popen([sys.executable, '-c', program, replacement], stdin=subprocess.PIPE,
                              stdout=subprocess.PIPE, text=True) as child, closing(select.kqueue()) as watch:
            try:
                def expect(line):
                    self.assertTrue(select.select([child.stdout], [], [], 5)[0])
                    self.assertEqual(child.stdout.readline().strip(), line)

                expect('original')
                birth = smoke.live_birth(child.pid)
                self.assertIsNotNone(birth)
                smoke.watch_original_process(watch, {'pid': child.pid, 'kernelBirth': birth})
                smoke.require_original_process(watch)
                child.stdin.write('\n')
                child.stdin.flush()
                expect('replacement')
                self.assertIsNone(child.poll())
                self.assertEqual(smoke.live_birth(child.pid), birth)
                reader, writer = socket.socketpair()
                with reader, writer:
                    writer.sendall(b'R')
                    with self.assertRaisesRegex(RuntimeError, 'exited or re-executed'):
                        smoke.wait_for_automatic_retry(watch, mock.Mock(pid=os.getpid(), poll=lambda: None),
                                                       reader, timeout=5)
            finally:
                child.kill()
                child.wait(timeout=5)

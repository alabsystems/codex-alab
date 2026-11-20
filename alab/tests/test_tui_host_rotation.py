# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
from http.client import HTTPConnection
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
from types import SimpleNamespace
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'smoke'))
import exec_named_limit
import tui_host_rotation as rotation


class HostShutdownReportTests(unittest.TestCase):
    def test_private_launcher_refusal_survives_in_the_reset_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            lab = root / 'old-lab'
            lab.write_text('#!/bin/sh\necho "aterm-lab: instance: unexpected --world" >&2\nexit 2\n')
            lab.chmod(0o755)
            with self.assertRaisesRegex(RuntimeError, 'exited 2.*unexpected --world'):
                with rotation.window(lab, root / 'host', root / 'world', {}, goal_reset=True):
                    self.fail('a refused private host cannot run the reset case')

    def test_reset_and_host_shutdown_outcomes_survive_in_the_json_report(self):
        stall = 'timed out waiting for automatic sampling after the weekly reset'
        shutdown = 'aterm-lab did not shut down cleanly'
        for primary, slow_host in [(stall, True), (None, True), (stall, False), (None, False)]:
            with self.subTest(primary=primary, slow_host=slow_host), tempfile.TemporaryDirectory() as tmp:
                result = Path(tmp) / 'result.json'
                child = mock.Mock(pid=123, **{'poll.return_value': None})
                if slow_host:
                    child.wait.side_effect = [subprocess.TimeoutExpired('aterm-lab', 45), -9]

                def check(root, args, report, *, goal_reset):
                    world = root / 'world'
                    (world / 'run').mkdir(parents=True)
                    (world / 'run/a.sock').touch()
                    with rotation.window(args.aterm_lab, args.aterm, world, {}, goal_reset=goal_reset):
                        if primary:
                            raise RuntimeError(primary)

                argv = ['tui_host_rotation.py', '--goal-reset', '--package', tmp, '--aterm', 'host',
                        '--aterm-lab', 'lab', '--result', str(result)]
                with (mock.patch.object(sys, 'argv', argv), mock.patch.object(sys, 'platform', 'darwin'),
                      mock.patch.object(rotation, 'smoke', check), mock.patch.object(rotation.signal, 'signal'),
                      mock.patch.object(rotation.subprocess, 'Popen', return_value=child),
                      mock.patch.object(rotation.os, 'killpg') as kill, mock.patch('builtins.print')):
                    code = rotation.main()
                report = json.loads(result.read_text())
                self.assertEqual(code, int(bool(primary or slow_host)))
                self.assertEqual(report['passed'], code == 0)
                if primary or slow_host:
                    self.assertEqual(report['error'], f'RuntimeError: {primary or shutdown}')
                else:
                    self.assertNotIn('error', report)
                if slow_host:
                    self.assertEqual(report['errorNotes'], [f'host cleanup shutdown: RuntimeError: {shutdown}'])
                    kill.assert_called_once_with(child.pid, rotation.signal.SIGKILL)
                    self.assertEqual(child.wait.call_args_list, [mock.call(timeout=45), mock.call(timeout=10)])
                else:
                    self.assertNotIn('errorNotes', report)
                    kill.assert_not_called()


@unittest.skipUnless(sys.platform == 'darwin', 'requires macOS executable staging')
class HostImageTests(unittest.TestCase):
    def test_window_receives_both_reported_images_when_build_outputs_change(self):
        root = Path(self.enterContext(tempfile.TemporaryDirectory())).resolve()
        scratch = root / 'fixture'
        scratch.mkdir()
        host = root / 'host-build'
        lab = root / 'lab-build'
        host.write_text('#!/bin/sh\necho "build: debug / macos-arm64"\n')
        lab.write_text('#!/bin/sh\nexit 0\n')
        for source in (host, lab):
            source.chmod(0o700)
        expected = [path.read_bytes() for path in (host, lab)]
        report = {}

        class ReachedWindow(Exception):
            pass

        def window(staged_lab, staged_host, *_args, **_kwargs):
            for source, image, content, key in zip((host, lab), (staged_host, staged_lab), expected,
                                                   ('host_sha256', 'lab_sha256'), strict=True):
                self.assertEqual(image.parent, scratch)
                self.assertFalse(source.samefile(image))
                self.assertEqual(source.stat().st_mode & 0o777, 0o700)
                source.write_text('new build')
                self.assertEqual(image.read_bytes(), content)
                self.assertEqual(report[key], hashlib.sha256(content).hexdigest())
            raise ReachedWindow()

        args = SimpleNamespace(aterm=host, aterm_lab=lab, package=root, limit='codex',
                               account_card=False, device_login=False)
        package = ({'source': 'fixture', 'version': 'fixture'}, {}, host)
        with mock.patch.object(rotation, 'window', window), mock.patch.object(rotation, 'listener'), \
                mock.patch.object(rotation.probe, 'package_records', return_value=package), \
                self.assertRaises(ReachedWindow):
            rotation.smoke(scratch, args, report)


class WeeklyFooterTests(unittest.TestCase):
    source = 'fixture@example.invalid'
    target = 'target@example.invalid'
    status = 'OK program=codex supervisor_proven=1 supervisor=codex-alab@72 gen=1.20'

    def frame(self, email):
        return {'gen': '1.20', 'dims': {'rows': 6, 'cols': 110}, 'inputs': [3, 3],
                'cursor': {'row': 2, 'col': 2, 'visible': True}, 'rows': [
            'An earlier message mentioned ' + self.target,
            '   codex-alab 0.11.0 · based on OpenAI Codex 0.169.0 ───',
            '» Ask Codex to do anything', '', f'  {email} / Pro 200 / weekly [░░░░░░░░] 0% used',
            '  ← for agents · ? for shortcuts     ⚠ 1 warning · f2 to view']}

    def test_only_the_current_composer_footer_supplies_the_address(self):
        self.assertEqual(rotation.weekly_footer_email(self.frame(self.source)), self.source)
        for change in ('open frame', 'transcript', 'cursor elsewhere', 'prompt', 'masked account', 'two addresses'):
            with self.subTest(change=change):
                frame = self.frame(self.target)
                if change == 'open frame':
                    frame['frame'] = 'open'
                elif change == 'transcript':
                    frame['rows'].extend(['A later message', ''])
                elif change == 'cursor elsewhere':
                    frame['cursor']['row'] = 0
                elif change == 'prompt':
                    frame['rows'][-2] = '» ' + frame['rows'][-2]
                elif change == 'masked account':
                    frame['rows'][-2] = frame['rows'][-2].replace(self.target, 't…')
                else:
                    frame['rows'][-2] += ' · ' + self.source
                self.assertIsNone(rotation.weekly_footer_email(frame))

    def test_inline_composer_requires_the_live_cursor_and_whole_blank_grid_tail(self):
        # The 110×30 headless fixture leaves 11 rows below the source and 6 below its successor.
        for cursor, padding in [(15, 11), (20, 6)]:
            with self.subTest(cursor=cursor):
                frame = self.frame(self.target)
                frame['rows'] = [''] * (cursor - 2) + frame['rows'] + [''] * padding
                frame['dims']['rows'] = 30
                frame['cursor']['row'] = cursor
                self.assertEqual(rotation.weekly_footer_email(frame), self.target)
                # A sliced frame can start above the composer but must include the physical bottom.
                frame['first'] = 3
                del frame['rows'][:3]
                self.assertEqual(rotation.weekly_footer_email(frame), self.target)
                frame['cursor']['row'] = 29
                self.assertIsNone(rotation.weekly_footer_email(frame))
                frame['cursor']['row'] = cursor
                frame['rows'][-1] = 'current popup'
                self.assertIsNone(rotation.weekly_footer_email(frame))
                frame['rows'].pop()
                self.assertIsNone(rotation.weekly_footer_email(frame))

    def test_ambiguous_or_malformed_frame_cannot_supply_a_footer(self):
        frame = self.frame(self.source)
        for sample in [None, [], {}, {**frame, 'inputs': [None, 3]}, {**frame, 'inputs': [2, 3]},
                       {**frame, 'cursor': {'row': 2, 'col': 0, 'visible': True}},
                       {**frame, 'first': 10}, {**frame, 'rows': [None] * 6}, {**frame, 'inputs': [1, True]},
                       {**frame, 'dims': {'rows': 6.0, 'cols': 110}}, {**frame, 'frame': 'partial'}]:
            with self.subTest(sample=sample):
                self.assertIsNone(rotation.weekly_footer_email(sample))

    def test_destination_waits_for_the_newly_drawn_footer(self):
        ctl = mock.Mock(side_effect=[json.dumps(self.frame(self.source)), json.dumps(self.frame(self.target)),
                                     self.status])
        with mock.patch.object(rotation.time, 'sleep'):
            seen = rotation.observe_weekly_footer(ctl, self.target, 'destination', 72)
        self.assertEqual(seen, {'email': self.target, 'pid': 72, 'frame': '1.20', 'cursor_row': 2,
                                'grid_rows': 6, 'blank_rows_below': 0})
        self.assertEqual(ctl.call_args_list, [mock.call('text', '--json')] * 2 + [mock.call('status')])

    def test_stale_source_or_another_foreground_cannot_pass_for_the_destination(self):
        for email, status in [(self.source, self.status), (self.target, self.status.replace('@72', '@71')),
                              (self.target, self.status.replace('1.20', '1.21')),
                              (self.target, self.status.replace('proven=1', 'proven=0'))]:
            with self.subTest(email=email, status=status):
                ctl = mock.Mock(side_effect=[json.dumps(self.frame(email)), status])
                with (mock.patch.object(rotation.time, 'monotonic', side_effect=[0, 1, 11]),
                      mock.patch.object(rotation.time, 'sleep'),
                      self.assertRaisesRegex(RuntimeError, 'destination Codex footer')):
                    rotation.observe_weekly_footer(ctl, self.target, 'destination', 72)


class HostReceiptTests(unittest.TestCase):
    def setUp(self):
        self.home = Path('/fixture/home')
        # Known native Identity::from_native digests for QUOTA_ACCOUNTS['old'] and ['new'].
        self.source = {'account': 'ee85d409da75b4f52c6ff6dc57ceb3f2ca104430f54dd0f3e26059f4e59af4d5',
                       'user': '558a6296c712b05b24cad9b8e8ed1ae2d8b55cfc70ec67c2a7213445740220fd'}
        self.target = {'account': 'b3cd724e8d9cb59cd34928eee28128e8e7045247783c870d1db168e261e4ae02',
                       'user': '14a6bb7f75797b7b034eb6bcd1bcc052e8352ca772e6eb8dbc737924f5992341'}
        self.receipt = {
            'intent': {'purpose': 'weekly_quota', 'identity': self.source, 'thread': 'thread', 'wall': 'turn'},
            'destination': self.target, 'old_auth': str(self.home / 'main/auth.json'),
            'new_auth': str(self.home / 'target/auth.json'), 'phase': 'consumed', 'accepted_turn': 'turn',
            'old_pid': 10, 'old_birth': 1, 'successor_pid': 20, 'successor_birth': 2,
        }
        self.model = SimpleNamespace(
            limit="codex",
            requests=[{'requestIdentity': {'thread_id': 'thread', 'turn_id': 'turn'},
                       'newTokenMatched': i == 2, 'body': {'model': rotation.probe.MODEL, 'input': ['same prompt']}} for i in range(3)],
            http_events=[{'kind': 'foreground', 'account': account, 'status': status, 'routingAccountMatched': True}
                         for account, status in [('source', 200), ('source', 429), ('target', 200)]],
        )
        for event, start, end in zip(self.model.http_events, (10, 30, 80), (20, 40, 90)):
            event.update(monotonicUs=start, responseWrittenUs=end)
        self.reads = [{'method': 'GET', 'path': '/wham/usage', 'client': 'host', 'account': account,
                       'weekUsed': used, 'allowed': allowed, 'routingAccountMatched': True, 'usageEndpointMatched': True,
                       'monotonicUs': start, 'responseWrittenUs': end}
                      for account, used, allowed, start, end in
                      [('source', 100, False, 45, 50), ('target', 0, True, 55, 60)]]
        self.model.http_events.extend(self.reads)
        # Hold the independent process and rollout checks constant while varying only the durable receipt.
        self.enterContext(mock.patch.object(rotation.fixture, 'validate_completed_original_turn'))
        self.enterContext(mock.patch.object(rotation.fixture.provider_fixture, 'check_unsteered_input'))
        self.enterContext(mock.patch.object(rotation.fixture, 'records', return_value=[{'pid': 20, 'sessionId': 'thread'}]))
        self.enterContext(mock.patch.object(rotation.fixture, 'process_evidence',
                                            return_value={'kernelBirth': 2, 'image': {'sha256': 'image'}}))

    def verify(self, receipt, **measurements):
        report = {'target_proof_count': 1, 'codex_sha256': 'image', **measurements}
        rotation.record_native_usage(self.model.http_events, self.model.limit, report)
        source = {'pid': 10, 'kernelBirth': 1, 'image': {'sha256': 'image'}}
        rotation.verify(self.model, self.home / '.codex', receipt, [], source, True, report)
        self.assertTrue(report['receipt_slot_bindings_verified'])
        self.assertTrue(report['original_input_and_completed_plan_preserved'])

    def test_named_move_requires_the_refused_bucket_and_matching_model(self):
        self.model.limit = 'codex_other'
        intent = {**self.receipt['intent'], 'limit': self.model.limit}
        receipt = {**self.receipt, 'intent': intent}
        measured = {'refusal_limit': self.model.limit}
        for read in self.reads:
            named = {'weekUsed': read['weekUsed'], 'allowed': read['allowed']}
            read.update(sessionUsed=5, weekUsed=10, allowed=True, additionalLimits={self.model.limit: named})
        self.verify(receipt, **measured)
        with self.assertRaisesRegex(RuntimeError, 'receipt bound a different limit'):
            self.verify(self.receipt, **measured)
        for fields in [{'additionalLimits': {}}, {'responseWrittenUs': 85}, {'routingAccountMatched': False},
                       {'usageEndpointMatched': False}]:
            with mock.patch.dict(self.reads[-1], fields), self.assertRaisesRegex(RuntimeError, 'eligible target'):
                self.verify(receipt, **measured)
        self.model.requests[-1]['body']['model'] = 'another-model'
        with self.assertRaisesRegex(RuntimeError, 'model'):
            self.verify(receipt, **measured)

    def test_device_login_requires_exact_grants_on_the_model_and_native_usage_paths(self):
        for event in self.model.http_events:
            event['issuedBearerMatched'] = True
        self.verify(self.receipt, device_logins={'main': {}, 'target': {}})
        for event in self.model.http_events:
            with self.subTest(account=event['account'], at=event['monotonicUs']):
                with mock.patch.dict(event, {'issuedBearerMatched': False}):
                    with self.assertRaisesRegex(RuntimeError, 'issued login'):
                        self.verify(self.receipt, device_logins={'main': {}, 'target': {}})

    def test_completed_request_matches_both_persisted_accounts_and_the_wall(self):
        self.verify(self.receipt)
        intent = self.receipt['intent']
        for label, change in {
            'swapped files': {'old_auth': self.receipt['new_auth'], 'new_auth': self.receipt['old_auth']},
            'same file': {'new_auth': self.receipt['old_auth']},
            'wrong source account': {'intent': {**intent, 'identity': {**self.source, 'account': self.target['account']}}},
            'wrong source user': {'intent': {**intent, 'identity': {**self.source, 'user': self.target['user']}}},
            'wrong target account': {'destination': {**self.target, 'account': self.source['account']}},
            'wrong target user': {'destination': {**self.target, 'user': self.source['user']}},
            'missing destination': {'destination': None},
            'different wall': {'intent': {**intent, 'wall': 'another turn'}},
            'different purpose': {'intent': {**intent, 'purpose': 'login_rotation'}},
        }.items():
            with self.subTest(label=label), self.assertRaisesRegex(RuntimeError, 'receipt'):
                self.verify({**self.receipt, **change})

    def test_weekly_reads_must_finish_between_the_refusal_and_the_target(self):
        self.verify(self.receipt)
        for label, index, fields in [
            ('source before refusal', 0, {'monotonicUs': 25, 'responseWrittenUs': 28}),
            ('source overlaps refusal', 0, {'monotonicUs': 35}),
            ('target begins after model request', 1, {'monotonicUs': 85, 'responseWrittenUs': 90}),
            ('target reply after model request', 1, {'responseWrittenUs': 85}),
            ('target reply unfinished', 1, {'responseWrittenUs': None}),
            ('same clock tick at refusal', 0, {'monotonicUs': 40}),
            ('same clock tick at target', 1, {'responseWrittenUs': 80}),
            ('codex rather than native host', 0, {'client': 'codex'}),
            ('source usage routed to another workspace', 0, {'routingAccountMatched': False}),
            ('unconfirmed target usage routing', 1, {'routingAccountMatched': None}),
            ('target usage reached another endpoint', 1, {'usageEndpointMatched': False}),
        ]:
            with self.subTest(label=label), mock.patch.dict(self.reads[index], fields):
                with self.assertRaisesRegex(RuntimeError, 'eligible target'):
                    self.verify(self.receipt)


    def test_weekly_move_uses_the_last_completed_response_for_each_login(self):
        for read, other in ((self.reads[0], self.reads[1]), (self.reads[1], self.reads[0])):
            changed = {**read, 'weekUsed': other['weekUsed'], 'allowed': other['allowed']}
            for end, supported in ((read['responseWrittenUs'] - 1, True),
                                   (read['responseWrittenUs'], False), (70, False)):
                # Capture order need not match the times when the writes completed.
                event = {**changed, 'responseWrittenUs': end}
                with self.subTest(account=read['account'], end=end), mock.patch.object(
                        self.model, 'http_events', [event, *self.model.http_events]):
                    if supported:
                        self.verify(self.receipt)
                    else:
                        with self.assertRaisesRegex(RuntimeError, 'eligible target'):
                            self.verify(self.receipt)
        with mock.patch.object(self.model, 'http_events', [*self.model.http_events, self.reads[-1].copy()]):
            self.verify(self.receipt)

    def test_later_missing_or_spent_named_quota_blocks_tui_and_exec_evidence(self):
        limit = self.model.limit = 'codex_other'
        self.model.lock = threading.Lock()
        receipt = {**self.receipt, 'intent': {**self.receipt['intent'], 'limit': limit}}
        self.model.requests[1].update(responseHeaders={'x-codex-active-limit': limit}, refusalNamedReset=100)
        for read in self.reads:
            named = {'weekUsed': read['weekUsed'], 'allowed': read['allowed']}
            read.update(sessionUsed=5, weekUsed=10, allowed=True, additionalLimits={limit: named})
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            self.verify(receipt, refusal_limit=limit)
            self.assertTrue(exec_named_limit.summary(self.model, root)['latestNativeUsageMatched'])
            for changes in ({'additionalLimits': {}}, {'additionalLimits': {limit: {'weekUsed': 100, 'allowed': False}}},
                            {'sessionUsed': 100}, {'weekUsed': 100, 'allowed': False}):
                newer = {**self.reads[-1], 'monotonicUs': 65, 'responseWrittenUs': 70, **changes}
                with self.subTest(changes=changes), mock.patch.object(
                        self.model, 'http_events', [newer, *self.model.http_events]):
                    with self.assertRaisesRegex(RuntimeError, 'eligible target'):
                        self.verify(receipt, refusal_limit=limit)
                    evidence = exec_named_limit.summary(self.model, root)
                    self.assertFalse(evidence['latestNativeUsageMatched'])
                    with mock.patch.object(exec_named_limit, 'verify_provider'):
                        with self.assertRaisesRegex(RuntimeError, 'named quota decision'):
                            exec_named_limit.verify(self.model, root, receipt, self.model.requests, evidence)


class FailedHandoffTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.home = self.root / 'codex'
        self.thread = '019aa000-0000-7000-8000-000000000001'
        self.turn = '019aa000-0000-7000-8000-000000000002'
        self.identity = {'thread_id': self.thread, 'turn_id': self.turn}
        self.intent = {'purpose': 'weekly_quota', 'thread': self.thread, 'wall': self.turn}
        self.receipt = {'id': 'a' * 32, 'phase': 'retired', 'intent': self.intent}
        self.ledger = self.root / 'harness/claude-harness/accounts-codex/renewal-follow'
        self.ledger.mkdir(parents=True)
        self.cancel = self.ledger / f"{self.receipt['id']}.revoked.json"
        self.cancel.write_text(json.dumps({'root': self.receipt['id'], 'intent': self.intent,
                                           'reason': 'successor_never_attached', 'extra': 'private'}))
        self.path = self.home / 'sessions' / f'rollout-stamp-{self.thread}.jsonl'
        self.path.parent.mkdir(parents=True)
        self.user = {'role': 'user', 'type': 'message', 'content': [{'type': 'input_text', 'text': rotation.probe.PROMPT}]}
        self.rows = [
            {'type': 'session_meta', 'payload': {'id': self.thread}},
            {'type': 'event_msg', 'payload': {'type': 'task_started', 'turn_id': self.turn}},
            {'type': 'response_item', 'payload': self.user},
            {'type': 'response_item', 'payload': rotation.fixture.provider_fixture.plan_call()},
            {'type': 'response_item', 'payload': {'type': 'function_call_output', 'call_id': rotation.probe.CALL_ID,
                                                  'output': 'Plan updated'}},
            {'type': 'event_msg', 'payload': {'type': 'task_complete', 'turn_id': self.turn,
                'error': {'message': 'private', 'codex_error_info': 'usage_limit_exceeded'}}},
        ]
        self.model = rotation.QuotaModel(self.root)
        self.model.snapshot_ready.set()
        for account, status in rotation.EXPECTED_CALLS[:2]:
            self.model.requests.append({'requestIdentity': self.identity, 'body': {'input': [self.user]}})
            self.model.http_events.append({'kind': 'foreground', 'account': account, 'status': status})

    def run_failure(self, message, *, receipts=None, rows=None):
        for receipt in receipts if receipts is not None else [self.receipt]:
            (self.ledger / f"{receipt['id']}.json").write_text(json.dumps(receipt))
        if rows is None:
            rows = self.rows
        self.path.write_bytes(b''.join(json.dumps(row).encode() + b'\n' for row in rows) + b'{"partial":')
        report = {'passed': False}
        with mock.patch.object(rotation.select, 'kqueue', create=True), mock.patch.object(
                rotation.time, 'monotonic', side_effect=[0, 1, 451]), mock.patch.object(rotation.time, 'sleep'):
            with self.assertRaisesRegex(RuntimeError, message):
                rotation.run_account_turn(self.model, mock.Mock(), report, state=self.root, codex_home=self.home,
                                          binary=Path('/fixture/codex'), issuer='https://fixture.invalid',
                                          host_running=lambda: True, auth_paths={'old': self.root}, target_label='target')
        self.assertFalse(report['passed'])
        self.assertNotIn('private', json.dumps(report))
        return report

    def test_retry_on_exhausted_source_records_missing_rotation_receipt(self):
        self.model.http_events.append({'kind': 'foreground', 'account': 'source', 'status': 429})
        self.model.requests.append({'requestIdentity': self.identity, 'body': {'input': [self.user]}})
        report = self.run_failure('unexpected provider history', receipts=[])
        self.assertEqual(report['model_calls'], [('source', 200), ('source', 429), ('source', 429)])
        self.assertEqual(report['interrupted_turn_history'], {'status': 'no_matching_receipt'})

    def test_retired_and_repeated_handoffs_retain_the_original_turn_and_bound_cancellation(self):
        other = {**self.receipt, 'id': '0' * 32, 'phase': 'prepared', 'intent': {**self.intent, 'wall': self.thread}}
        for receipts, message in [([self.receipt], 'retired the rotation'), ([other, self.receipt], 'second logical')]:
            with self.subTest(message=message):
                report = self.run_failure(message, receipts=receipts)
                self.assertEqual(report['receipts'][-1]['retirement_reason'], 'successor_never_attached')
                self.assertEqual(report['interrupted_turn_history'], {
                    'status': 'read', 'thread': self.thread, 'turn': self.turn, 'receipt_ids': [self.receipt['id']], 'starts': 1,
                    'terminal_events': ['usage_limit_exceeded'], 'original_input_verified': True,
                    'completed_plan_verified': True})

    def test_timeout_records_unfinished_turn_and_lost_work_independently(self):
        receipt = {**self.receipt, 'phase': 'ready'}
        rows = self.rows[:-2]  # No flushed tool result or terminal event.
        history = self.run_failure('within 450 seconds', receipts=[receipt], rows=rows)['interrupted_turn_history']
        self.assertEqual(history['terminal_events'], [])
        self.assertTrue(history['original_input_verified'])
        self.assertFalse(history['completed_plan_verified'])
        rows = self.rows[:2] + self.rows[3:]  # Only the persisted prompt is missing.
        history = self.run_failure('within 450 seconds', receipts=[receipt], rows=rows)['interrupted_turn_history']
        self.assertFalse(history['original_input_verified'])
        self.assertTrue(history['completed_plan_verified'])

    def test_untrusted_cancellation_and_unrelated_history_do_not_pass_or_replace_the_failure(self):
        for changed in ({'root': '0' * 32}, {'intent': {}}, {'reason': 'private'}):
            self.cancel.write_text(json.dumps({'root': self.receipt['id'], 'intent': self.intent,
                                               'reason': 'owner_stop', **changed}))
            report = self.run_failure('retired the rotation', rows=[{'type': 'session_meta', 'payload': {'id': self.turn}}])
            self.assertIsNone(report['receipts'][0]['retirement_reason'])
            self.assertEqual(report['interrupted_turn_history']['status'], 'rollout_unverified')
        wrong = {**self.receipt, 'intent': {**self.intent, 'wall': self.thread}}
        report = self.run_failure('retired the rotation', receipts=[wrong])
        self.assertEqual(report['interrupted_turn_history'], {'status': 'no_matching_receipt'})
        report = self.run_failure('retired the rotation', rows=[None])
        self.assertEqual(report['interrupted_turn_history'], {'status': 'evidence_unavailable'})


class DeviceBearerListenerTests(unittest.TestCase):
    def test_unissued_login_is_refused_and_the_controller_can_read_the_failure(self):
        with tempfile.TemporaryDirectory() as scratch:
            model = rotation.QuotaModel(Path(scratch))
            with rotation.listener(model, False, issued_bearer_matches=lambda _generation, _value: False) as port:
                connection = HTTPConnection('127.0.0.1', port, timeout=4)
                try:
                    connection.request('GET', rotation.fixture.ROOT_USAGE_PATH, headers={'Authorization': 'Bearer other'})
                    response = connection.getresponse()
                    self.assertEqual(response.status, 401)
                    response.read()
                finally:
                    connection.close()
            with self.assertRaisesRegex(RuntimeError, 'did not present an issued fixture bearer'):
                rotation.fixture.require_no_fixture_fault(model)


if __name__ == '__main__':
    unittest.main()

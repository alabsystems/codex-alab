# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Distinguish fatal stream events from advisory error items without a backend."""
import importlib.util
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock


FIXTURE_PATH = Path(__file__).with_name('test_request_recovery_smoke.py')
SPEC = importlib.util.spec_from_file_location('request_recovery_fatal_fixture', FIXTURE_PATH)
FIXTURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FIXTURE)
SMOKE = FIXTURE.SMOKE


class RequestRecoveryFatalEventsTests(unittest.TestCase):
    def test_wait_duration_matches_the_client_unsigned_seconds_format(self):
        fixture = SMOKE.probe.provider_fixture
        for code, prefix in fixture.CAPACITY_MESSAGES.items():
            for value in (0, 324, 2**64 - 1):
                message = f"{prefix} (Retry scheduled in {value}s.)"
                self.assertEqual(fixture.capacity_notice_seconds(message, code), value)
            for text in ("01", "-1", "2.5", "２", str(2**64), "1" * 300):
                with self.subTest(code=code, text=text):
                    self.assertIsNone(fixture.capacity_notice_seconds(f"{prefix} (Retry scheduled in {text}s.)", code))
            self.assertIsNone(fixture.capacity_notice_seconds(prefix, code))
            self.assertIsNone(fixture.capacity_notice_seconds(f"{prefix} (Retry scheduled in 1s.)", "unknown"))

    def test_a_legacy_capacity_notice_does_not_hide_an_early_retry(self):
        events, requests = FIXTURE.trace()
        events.insert(1, {'type': 'error', 'message': 'Model is at capacity'})
        requests[2]['requestStartedAt'] = requests[1]['headerAt'] + 0.7
        with self.assertRaisesRegex(RuntimeError, 'Retry-After bypassed'):
            SMOKE.check_result(0, events, requests, True, expect_capacity_notice=True)

    def test_fatal_stream_error_prevents_recovery_at_every_event_position(self):
        for position in range(5):
            with self.subTest(position=position):
                events, requests = FIXTURE.trace()
                events.insert(position, {'type': 'error', 'message': 'unrecoverable fixture stream'})
                with self.assertRaisesRegex(RuntimeError, 'top-level JSON error'):
                    SMOKE.check_result(0, events, requests, True)

    def test_nonfatal_error_items_remain_successful_at_every_event_position(self):
        for position in range(5):
            with self.subTest(position=position):
                events, requests = FIXTURE.trace()
                events.insert(position, {'type': 'item.completed', 'item': {
                    'type': 'error', 'message': 'Retrying the original request'}})
                self.assertAlmostEqual(SMOKE.check_result(0, events, requests, True), 2.001)

    def test_overload_requires_one_timed_advisory_before_the_answer(self):
        prefix = 'Model is at capacity; retrying automatically (interrupt to stop)...'
        for positions, suffix, accepted in (
                ([1], ' (Retry scheduled in 2s.)', True),
                ([1], ' (Retry scheduled in 1s.)', True),
                ([], '', False), ([1, 1], ' (Retry scheduled in 2s.)', False),
                ([0], ' (Retry scheduled in 2s.)', False), ([2], ' (Retry scheduled in 2s.)', False),
                ([1], '', False), ([1], ' (Retry scheduled in 3s.)', False)):
            with self.subTest(positions=positions, suffix=suffix):
                events, requests = FIXTURE.trace()
                for position in positions:
                    events.insert(position + 1, {'type': 'item.completed', 'item': {
                        'type': 'error', 'message': prefix + suffix}})
                if accepted:
                    self.assertAlmostEqual(SMOKE.check_result(
                        0, events, requests, True, expect_capacity_notice=True), 2.001)
                else:
                    with self.assertRaisesRegex(RuntimeError, 'capacity notice'):
                        SMOKE.check_result(0, events, requests, True, expect_capacity_notice=True)

    def test_live_notice_requires_a_matched_monotonic_stdout_sample(self):
        fixture = SMOKE.probe.provider_fixture
        events = [{"type": "turn.started"}, {"type": "item.completed", "item": {
            "type": "error", "message": fixture.CAPACITY_NOTICE + " (Retry scheduled in 2s.)"}}]
        requests = [{"headerStartedAt": 20.0}, {"requestStartedAt": 22.5}]
        fixture.check_live_notices(events, [18.0, 20.1], requests, (2,))
        for observed in (None, [20.1], [21, 20.1], [18, float("nan")], [18, True],
                         [18, 19.9], [18, 22], [18, 22.5]):
            with self.subTest(observed=observed), self.assertRaises(ValueError):
                fixture.check_live_notices(events, observed, requests, (2,))

    def test_startup_notice_can_arrive_during_foreground_backoff_after_provider_deadline(self):
        fixture = SMOKE.probe.provider_fixture
        events = [{"type": "item.completed", "item": {
            "type": "error", "message": fixture.CAPACITY_NOTICE + " (Retry scheduled in 1s.)"}}]
        handshakes = [{"headerStartedAt": 20.0}, {"requestStartedAt": 22.5}]
        fixture.check_live_notices(events, [22.25], handshakes, (2,), startup=True)
        with self.assertRaisesRegex(ValueError, "not visible during"):
            fixture.check_live_notices(events, [22.5], handshakes, (2,), startup=True)

    def test_local_notices_must_arrive_between_the_matching_refusal_and_retry(self):
        fixture = SMOKE.probe.provider_fixture
        events = [{"type": "item.completed", "item": {"type": "error", "message":
                  f"{fixture.CAPACITY_NOTICE} (Retry scheduled in {seconds}s.)"}} for seconds in (1, 2, 3)]
        requests = [{"headerStartedAt": 10}, {"requestStartedAt": 10.8, "headerStartedAt": 11},
                    {"requestStartedAt": 12.5, "headerStartedAt": 13}, {"requestStartedAt": 15.75}]
        for waits in ((None, None, 1), (0, 0, 0)):
            # Local backoff can still be running after short or absent advice.
            fixture.check_live_notices(events, [10.1, 11.3, 14.5], requests, waits)
            for received in ([9.99, 11.3, 14.5], [10.1, 12.5, 14.5], [16, 16, 16]):
                with self.subTest(waits=waits, received=received), self.assertRaisesRegex(ValueError, "not visible"):
                    fixture.check_live_notices(events, received, requests, waits)

        # The third retry took 2.75s from the refusal. Delayed stdout delivery
        # allows the three-second notice above, but four seconds is too long.
        events[2]["item"]["message"] = f"{fixture.CAPACITY_NOTICE} (Retry scheduled in 4s.)"
        with self.assertRaisesRegex(ValueError, "too early for its announced wait"):
            fixture.check_live_notices(events, [10.1, 11.3, 14.5], requests, (None, None, 1))

    def test_live_notice_covers_the_time_remaining_when_stdout_delivers_it(self):
        fixture = SMOKE.probe.provider_fixture
        requests = [{"headerStartedAt": 20.0}, {"requestStartedAt": 25.0}]
        for received, seconds, accepted in ((20.001, 1, False), (20.001, 2, False),
                                            (20.001, 3, True), (21.0, 2, True),
                                            (21.999, 1, False), (22.0, 1, True),
                                            (22.5, 3, True), (22.5, 4, False)):
            with self.subTest(received=received, seconds=seconds):
                message = f"{fixture.CAPACITY_NOTICE} (Retry scheduled in {seconds}s.)"
                events = [{"type": "item.completed", "item": {"type": "error", "message": message}}]
                if accepted:
                    fixture.check_live_notices(events, [received], requests, (3,))
                else:
                    with self.assertRaisesRegex(ValueError, "selected wait"):
                        fixture.check_live_notices(events, [received], requests, (3,))

    def test_only_the_terminal_event_can_supply_the_fixture_refusal(self):
        _, requests = FIXTURE.trace()
        for name, status, envelope, recover in SMOKE.CASES:
            if recover:
                continue
            if name == 'forbidden-overload':
                expected = ('unexpected status 403 Forbidden: Model is at capacity., '
                            'url: http://127.0.0.1:16789/v1/responses, request id: req_opaque_smoke-1')
                kind = {'http_connection_failed': {'http_status_code': 403}}
            elif name in ('misalignment-400', 'misalignment-403', 'streamed-misalignment'):
                expected = SMOKE.MISALIGNMENT['message']
                kind = 'misalignment_policy_violation'
            else:
                expected = envelope['error']['message'] if isinstance(envelope, dict) else json.dumps(envelope)
                kind = 'invalid_prompt' if name == 'specific' else 'other'
            refusal = SMOKE.expected_refusal(status, envelope, 16789)
            self.assertEqual(refusal, {'message': expected, 'codex_error_info': kind})
            for error in (None, {}, {'message': 'unrelated local failure'}, {'message': 'Model is at capacity.'},
                          {'message': expected}):
                with self.subTest(envelope=envelope, error=error):
                    events = [*FIXTURE.refusal_events()[:2], {'type': 'error', 'message': expected},
                              {'type': 'turn.failed', 'error': error}]
                    if error == {'message': expected}:
                        SMOKE.check_result(1, events, requests[:2], False, refusal=refusal)
                    else:
                        with self.assertRaisesRegex(RuntimeError, 'fixture refusal'):
                            SMOKE.check_result(1, events, requests[:2], False, refusal=refusal)

    def test_terminal_refusal_cannot_announce_a_capacity_retry(self):
        _, requests = FIXTURE.trace()
        thread, started, failed = FIXTURE.refusal_events()
        for prefix in SMOKE.probe.provider_fixture.CAPACITY_MESSAGES.values():
            item = {'type': 'error', 'message': f'{prefix} (Retry scheduled in 2s.)'}
            for notice in (item, {'type': 'item.completed', 'item': item}):
                with self.subTest(notice=notice), self.assertRaisesRegex(RuntimeError, 'announced an automatic'):
                    SMOKE.check_result(1, [thread, started, notice, failed], requests[:2], False)

    def test_terminal_refusal_can_retain_its_fatal_error_diagnostic(self):
        _, requests = FIXTURE.trace()
        fatal = {'type': 'error', 'message': 'Invalid tool schema'}
        thread, started, failed = FIXTURE.refusal_events()
        for events in ([thread, started, fatal, failed], [thread, started, failed, fatal]):
            with self.subTest(events=events):
                self.assertIsNone(SMOKE.check_result(1, events, requests[:2], False))

    def test_public_case_retains_failed_fatal_trace_and_completes_cleanup(self):
        events, requests = FIXTURE.trace()
        events.append({'type': 'error', 'message': 'unrecoverable\u2028fixture\u0085stream\u2029details'})
        server = SimpleNamespace(server_port=1, shutdown=mock.Mock(), server_close=mock.Mock())
        thread = SimpleNamespace(join=mock.Mock())

        def serve(model):
            model.requests = requests
            return server, thread

        def run(_binary, _arguments, destination, _environment, _label):
            (destination / 'exec-stdout.jsonl').write_text(
                ''.join(json.dumps(event, ensure_ascii=False) + '\n' for event in events), encoding='utf-8')
            return 0

        with tempfile.TemporaryDirectory(prefix='alab-recovery-fatal-') as directory:
            output = Path(directory)
            with mock.patch.object(SMOKE.probe, 'fresh_environment', return_value={}), \
                    mock.patch.object(SMOKE.probe, 'configure'), \
                    mock.patch.object(SMOKE.probe, 'serve', side_effect=serve), \
                    mock.patch.object(SMOKE.probe, 'run', side_effect=run):
                result = SMOKE.case(output / 'unexecuted-fixture', output, 'opaque',
                                    400, {'detail': 'Bad Request'}, True)
            self.assertFalse(result['passed'])
            self.assertIn('top-level JSON error', result['error'])
            self.assertEqual(result['turnEvents'], ['turn.started', 'turn.completed'])
            self.assertEqual(result['requestCount'], 3)
            self.assertEqual(result['returncode'], 0)
            self.assertEqual(json.loads((output / 'opaque/requests.json').read_text()), requests)
            with (output / 'opaque/exec-stdout.jsonl').open('rb') as log:
                self.assertEqual([json.loads(line) for line in log], events)
        server.shutdown.assert_called_once_with()
        thread.join.assert_called_once_with()
        server.server_close.assert_called_once_with()


if __name__ == '__main__':
    unittest.main()

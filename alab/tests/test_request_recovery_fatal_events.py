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
    def test_fatal_stream_error_prevents_recovery_at_every_event_position(self):
        for position in range(4):
            with self.subTest(position=position):
                events, requests = FIXTURE.trace()
                events.insert(position, {'type': 'error', 'message': 'unrecoverable fixture stream'})
                with self.assertRaisesRegex(RuntimeError, 'fatal'):
                    SMOKE.check_result(0, events, requests, True)

    def test_nonfatal_error_items_remain_successful_at_every_event_position(self):
        for position in range(4):
            with self.subTest(position=position):
                events, requests = FIXTURE.trace()
                events.insert(position, {'type': 'item.completed', 'item': {
                    'type': 'error', 'message': 'Retrying the original request'}})
                self.assertAlmostEqual(SMOKE.check_result(0, events, requests, True), 2.001)

    def test_terminal_refusal_can_retain_its_fatal_error_diagnostic(self):
        _, requests = FIXTURE.trace()
        fatal = {'type': 'error', 'message': 'Invalid tool schema'}
        for events in ([fatal, {'type': 'turn.failed'}], [{'type': 'turn.failed'}, fatal]):
            with self.subTest(events=events):
                self.assertIsNone(SMOKE.check_result(1, events, requests[:2], False))

    def test_public_case_retains_failed_fatal_trace_and_completes_cleanup(self):
        events, requests = FIXTURE.trace()
        events.append({'type': 'error', 'message': 'unrecoverable fixture stream'})
        server = SimpleNamespace(server_port=1, shutdown=mock.Mock(), server_close=mock.Mock())
        thread = SimpleNamespace(join=mock.Mock())

        def serve(model):
            model.requests = requests
            return server, thread

        def run(_binary, _arguments, destination, _environment, _label):
            (destination / 'exec-stdout.jsonl').write_text(
                ''.join(json.dumps(event) + '\n' for event in events))
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
            self.assertIn('fatal', result['error'])
            self.assertEqual(result['requestCount'], 3)
            self.assertEqual(result['returncode'], 0)
            self.assertEqual(json.loads((output / 'opaque/requests.json').read_text()), requests)
            self.assertEqual([json.loads(line) for line in
                              (output / 'opaque/exec-stdout.jsonl').read_text().splitlines()], events)
        server.shutdown.assert_called_once_with()
        thread.join.assert_called_once_with()
        server.server_close.assert_called_once_with()


if __name__ == '__main__':
    unittest.main()

# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Keep the fixture's native tool identity; run no Codex or provider."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock


FIXTURE_PATH = Path(__file__).with_name('test_request_recovery_smoke.py')
SPEC = importlib.util.spec_from_file_location('request_recovery_namespace_fixture', FIXTURE_PATH)
FIXTURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FIXTURE)
SMOKE = FIXTURE.SMOKE
ABSENT = object()


def trace(recover, *, call_namespace=ABSENT, output_name=ABSENT, output_namespace=ABSENT):
    events, requests = FIXTURE.trace()
    call, output = requests[1]['body']['input'][1:]
    for item, name, value in ((call, 'namespace', call_namespace),
                              (output, 'name', output_name),
                              (output, 'namespace', output_namespace)):
        if value is not ABSENT:
            item[name] = value
    requests[2]['body']['input'] = copy.deepcopy(requests[1]['body']['input'])
    if not recover:
        events = FIXTURE.refusal_events()
        requests = requests[:2]
    return events, requests


class RequestRecoveryNamespaceTests(unittest.TestCase):
    def assert_accepted(self, events, requests, recover):
        before = copy.deepcopy((events, requests))
        result = SMOKE.check_result(0 if recover else 1, events, requests, recover)
        if recover:
            self.assertAlmostEqual(result, 2.001)
        else:
            self.assertIsNone(result)
        self.assertEqual((events, requests), before)

    def assert_refused(self, events, requests, recover):
        before = copy.deepcopy((events, requests))
        with self.assertRaises(RuntimeError):
            SMOKE.check_result(0 if recover else 1, events, requests, recover)
        self.assertEqual((events, requests), before)

    def test_native_default_call_namespace_forms_remain_accepted(self):
        # ToolName::with_default_namespace treats these four forms alike.
        for namespace in (ABSENT, None, '', 'functions'):
            for recover in (True, False):
                with self.subTest(namespace=repr(namespace), recover=recover):
                    self.assert_accepted(*trace(recover, call_namespace=namespace), recover)

    def test_a_nondefault_call_namespace_cannot_be_the_fixture_tool(self):
        for namespace in ('other_provider', 'Functions', ' functions '):
            for recover in (True, False):
                with self.subTest(namespace=namespace, recover=recover):
                    self.assert_refused(*trace(recover, call_namespace=namespace), recover)

    def test_optional_matching_output_identity_remains_accepted(self):
        # Native function outputs normally omit both fields. Explicit matching
        # identities must retain the same default-namespace interpretation.
        for name in (ABSENT, None, 'update_plan'):
            for namespace in (ABSENT, None, '', 'functions'):
                for recover in (True, False):
                    with self.subTest(name=repr(name), namespace=repr(namespace), recover=recover):
                        self.assert_accepted(*trace(recover, output_name=name,
                                                    output_namespace=namespace), recover)

    def test_a_conflicting_output_name_cannot_identify_the_fixture_result(self):
        for name in ('other_tool', '', 'UPDATE_PLAN'):
            for recover in (True, False):
                with self.subTest(name=name, recover=recover):
                    self.assert_refused(*trace(recover, output_name=name), recover)

    def test_a_conflicting_output_namespace_cannot_identify_the_fixture_result(self):
        for namespace in ('other_provider', 'Functions', ' functions '):
            for recover in (True, False):
                with self.subTest(namespace=namespace, recover=recover):
                    self.assert_refused(*trace(recover, output_namespace=namespace), recover)

    def test_matching_identity_preserves_original_literals_and_other_native_metadata(self):
        for recover in (True, False):
            with self.subTest(recover=recover):
                events, requests = trace(recover, call_namespace='functions',
                                         output_name='update_plan', output_namespace='functions')
                call, output = requests[1]['body']['input'][1:]
                call.update(id='fc_fixture', status='completed')
                output['id'] = 'fco_fixture'
                for request in requests[2:]:
                    request['body']['input'] = copy.deepcopy(requests[1]['body']['input'])
                self.assertEqual((call['name'], call['call_id'], call['arguments']),
                                 ('update_plan', SMOKE.probe.CALL_ID,
                                  json.dumps({'plan': [{'step': SMOKE.probe.PLAN,
                                                        'status': 'completed'}]})))
                self.assertEqual((output['call_id'], output['output']),
                                 (SMOKE.probe.CALL_ID, 'Plan updated'))
                self.assert_accepted(events, requests, recover)

    def public_case(self, root, recover, **identity):
        events, requests = trace(recover, **identity)
        before = copy.deepcopy((events, requests))
        destination = root / 'results'
        destination.mkdir()
        binary = root / 'unexecuted-codex-fixture'
        binary.write_bytes(b'owned data; never executed as a program\n')
        server = SimpleNamespace(server_port=1234, shutdown=mock.Mock(),
                                 server_close=mock.Mock())
        thread = SimpleNamespace(join=mock.Mock())

        def serve(model):
            model.requests = copy.deepcopy(requests)
            return server, thread

        def run(codex, args, output, _environment, label):
            self.assertEqual(codex, binary)
            self.assertIn(SMOKE.probe.PROMPT, args)
            self.assertEqual(label, 'exec')
            sessions = output / 'codex-home/sessions'
            sessions.mkdir(parents=True, exist_ok=True)
            records = [{'type': 'session_meta', 'payload': {'id': 'original'}},
                       *({'type': 'event_msg', 'payload': {'type': kind, 'turn_id': 'active'}}
                         for kind in ('task_started', 'task_complete'))]
            if recover:
                records[-1]['payload']['last_agent_message'] = SMOKE.probe.ANSWER
            else:
                records[-1]['payload']['error'] = {'message': 'Invalid tool schema', 'codex_error_info': 'invalid_prompt'}
            records[2:2] = [{'type': 'response_item', 'payload': item} for item in requests[1]['body']['input']]
            (sessions / 'rollout-fixture-original.jsonl').write_text(''.join(json.dumps(row) + '\n' for row in records))
            with (output / 'exec-stdout.jsonl').open('x') as stream:
                for event in events:
                    stream.write(json.dumps(event) + '\n')
            return 0 if recover else 1

        with mock.patch.object(SMOKE.probe, 'serve', side_effect=serve), \
                mock.patch.object(SMOKE.probe, 'configure') as configure, \
                mock.patch.object(SMOKE.probe, 'run', side_effect=run):
            envelope = ({'detail': 'Bad Request'} if recover else
                        {'error': {'code': 'invalid_prompt', 'message': 'Invalid tool schema'}})
            result = SMOKE.case(binary, destination, 'fixture', 400, envelope, recover)
        configure.assert_called_once_with(destination / 'fixture', 1234, stream_max_retries=2, supports_websockets=False)
        server.shutdown.assert_called_once_with()
        thread.join.assert_called_once_with()
        server.server_close.assert_called_once_with()
        self.assertEqual(json.loads((destination / 'fixture/requests.json').read_text()), requests)
        self.assertEqual((events, requests), before)
        self.assertEqual(binary.read_bytes(), b'owned data; never executed as a program\n')
        self.assertEqual(result['requestCount'], 3 if recover else 2)
        self.assertEqual(result['returncode'], 0 if recover else 1)
        self.assertEqual(result['turnEvents'], ['turn.started', 'turn.completed' if recover else 'turn.failed'])
        return result

    def test_public_case_cannot_pass_conflicting_native_identity(self):
        for identity in ({'call_namespace': 'other_provider'},
                         {'output_name': 'other_tool'},
                         {'output_namespace': 'other_provider'}):
            for recover in (True, False):
                with self.subTest(identity=identity, recover=recover), \
                        tempfile.TemporaryDirectory() as directory:
                    result = self.public_case(Path(directory).resolve(), recover, **identity)
                self.assertFalse(result['passed'])
                self.assertIn('error', result)

    def test_public_case_keeps_explicit_matching_native_identity(self):
        for recover in (True, False):
            with self.subTest(recover=recover), tempfile.TemporaryDirectory() as directory:
                result = self.public_case(Path(directory).resolve(), recover,
                                          call_namespace='functions', output_name='update_plan',
                                          output_namespace='')
            self.assertTrue(result['passed'])
            self.assertNotIn('error', result)
            if recover:
                self.assertAlmostEqual(result['retryAfterSeconds'], 2.001)
            else:
                self.assertIsNone(result['retryAfterSeconds'])


if __name__ == '__main__':
    unittest.main()

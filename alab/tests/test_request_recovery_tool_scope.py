# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Keep a fresh recovery trace to its one native fixture tool; run no backend."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock


FIXTURE_PATH = Path(__file__).with_name('test_request_recovery_smoke.py')
SPEC = importlib.util.spec_from_file_location('request_recovery_tool_scope_fixture', FIXTURE_PATH)
FIXTURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FIXTURE)
SMOKE = FIXTURE.SMOKE


def custom_records():
    # ResponseItem::CustomToolCall and CustomToolCallOutput use these native wire shapes.
    return ({'type': 'custom_tool_call', 'call_id': 'unexpected-custom-call',
             'name': 'exec', 'input': 'text("unexpected work");'},
            {'type': 'custom_tool_call_output', 'call_id': 'unexpected-custom-call',
             'output': 'unexpected work completed'})


def other_native_records():
    # The other work tags are also ResponseItem variants, not suffix guesses.
    return ({'type': 'local_shell_call', 'call_id': 'unexpected-shell', 'status': 'completed',
             'action': {'type': 'exec', 'command': ['/usr/bin/true'], 'timeout_ms': None,
                        'working_directory': None, 'env': None, 'user': None}},
            {'type': 'tool_search_call', 'call_id': 'unexpected-search', 'execution': 'client',
             'arguments': {'query': 'unexpected tool'}},
            {'type': 'tool_search_output', 'call_id': 'unexpected-search', 'status': 'completed',
             'execution': 'client', 'tools': []},
            {'type': 'web_search_call', 'status': 'completed',
             'action': {'type': 'search', 'query': 'unexpected lookup'}},
            {'type': 'image_generation_call', 'status': 'completed', 'result': 'Zml4dHVyZQ=='})


def trace_with_records(records, recover):
    events, requests = FIXTURE.trace()
    requests[1]['body']['input'].extend(copy.deepcopy(records))
    requests[2]['body']['input'] = copy.deepcopy(requests[1]['body']['input'])
    if not recover:
        events = FIXTURE.refusal_events()
        requests = requests[:2]
    return events, requests


class RequestRecoveryToolScopeTests(unittest.TestCase):
    def assert_unexpected_records_refused(self, records):
        for recover in (True, False):
            with self.subTest(recover=recover):
                events, requests = trace_with_records(records, recover)
                with self.assertRaises(RuntimeError):
                    SMOKE.check_result(0 if recover else 1, events, requests, recover)

    def test_unexpected_native_custom_call_is_refused(self):
        self.assert_unexpected_records_refused(custom_records()[:1])

    def test_unexpected_native_custom_output_is_refused(self):
        self.assert_unexpected_records_refused(custom_records()[1:])

    def test_unexpected_native_custom_pair_is_refused(self):
        self.assert_unexpected_records_refused(custom_records())

    def test_other_native_tool_calls_and_outputs_are_refused(self):
        for item in other_native_records():
            with self.subTest(type=item['type']):
                self.assert_unexpected_records_refused((item,))

    def test_unexpected_function_records_remain_refused(self):
        _, requests = FIXTURE.trace()
        call, output = copy.deepcopy(requests[1]['body']['input'][1:])
        call['call_id'] = output['call_id'] = 'unexpected-function-call'
        for records in ((call,), (output,), (call, output)):
            with self.subTest(types=[item['type'] for item in records]):
                self.assert_unexpected_records_refused(records)

    def test_expected_pair_cannot_change_to_native_custom_wire_types(self):
        for changed in ('call', 'output'):
            for recover in (True, False):
                with self.subTest(changed=changed, recover=recover):
                    events, requests = trace_with_records((), recover)
                    original = requests[1]['body']['input']
                    replacement = ({'type': 'custom_tool_call', 'call_id': SMOKE.probe.CALL_ID,
                                    'name': 'update_plan', 'input': original[1]['arguments']}
                                   if changed == 'call' else
                                   {'type': 'custom_tool_call_output',
                                    'call_id': SMOKE.probe.CALL_ID, 'output': 'Plan updated'})
                    for request in requests[1:]:
                        request['body']['input'][1 if changed == 'call' else 2] = copy.deepcopy(replacement)
                    with self.assertRaises(RuntimeError):
                        SMOKE.check_result(0 if recover else 1, events, requests, recover)

    def test_reasoning_model_messages_and_catalog_metadata_remain_allowed(self):
        for item in ({'type': 'reasoning', 'summary': []},
                     {'type': 'message', 'role': 'assistant', 'content': [
                         {'type': 'output_text', 'text': 'The completed plan remains relevant.'}]},
                     {'type': 'additional_tools', 'role': 'developer', 'tools': []}):
            for recover in (True, False):
                with self.subTest(item=item['type'], recover=recover):
                    events, requests = trace_with_records((item,), recover)
                    observed = SMOKE.check_result(0 if recover else 1, events, requests, recover)
                    if recover:
                        self.assertAlmostEqual(observed, 2.001)
                    else:
                        self.assertIsNone(observed)

    def test_nonfatal_warning_items_remain_allowed(self):
        for recover in (True, False):
            with self.subTest(recover=recover):
                events, requests = trace_with_records((), recover)
                events.insert(1 if recover else 0, {
                    'type': 'item.completed',
                    'item': {'type': 'error', 'message': 'Retrying the original request'},
                })
                observed = SMOKE.check_result(0 if recover else 1, events, requests, recover)
                if recover:
                    self.assertAlmostEqual(observed, 2.001)
                else:
                    self.assertIsNone(observed)

    def test_original_tool_and_result_literals_remain_unchanged(self):
        expected_arguments = json.dumps({'plan': [{'step': SMOKE.probe.PLAN,
                                                  'status': 'completed'}]})
        for recover in (True, False):
            with self.subTest(recover=recover):
                events, requests = trace_with_records((), recover)
                before = copy.deepcopy((events, requests))
                call, output = requests[1]['body']['input'][1:]
                self.assertEqual((call['name'], call['call_id'], call['arguments']),
                                 ('update_plan', SMOKE.probe.CALL_ID, expected_arguments))
                self.assertEqual((output['call_id'], output['output']),
                                 (SMOKE.probe.CALL_ID, 'Plan updated'))
                SMOKE.check_result(0 if recover else 1, events, requests, recover)
                self.assertEqual((events, requests), before)

    def test_public_case_cannot_pass_unexpected_native_tool_records(self):
        call, output = custom_records()
        variants = ((call,), (output,), (call, output),
                    *((item,) for item in other_native_records()))
        for records in variants:
            for recover in (True, False):
                with self.subTest(types=[item['type'] for item in records], recover=recover), \
                        tempfile.TemporaryDirectory() as directory:
                    root = Path(directory)
                    events, requests = trace_with_records(records, recover)
                    server = SimpleNamespace(server_port=1234, shutdown=mock.Mock(),
                                             server_close=mock.Mock())
                    thread = SimpleNamespace(join=mock.Mock())
                    expected_code = 0 if recover else 1

                    def serve(model):
                        model.requests = copy.deepcopy(requests)
                        return server, thread

                    def run(binary, args, destination, _environment, label):
                        self.assertEqual(binary, Path('/unexecuted-fixture/codex'))
                        self.assertIn(SMOKE.probe.PROMPT, args)
                        self.assertEqual(label, 'exec')
                        with (destination / 'exec-stdout.jsonl').open('x') as stream:
                            for event in events:
                                stream.write(json.dumps(event) + '\n')
                        return expected_code

                    with mock.patch.object(SMOKE.probe, 'serve', side_effect=serve), \
                            mock.patch.object(SMOKE.probe, 'configure') as configure, \
                            mock.patch.object(SMOKE.probe, 'run', side_effect=run):
                        envelope = ({'detail': 'Bad Request'} if recover else
                                    {'error': {'code': 'invalid_prompt',
                                               'message': 'Invalid tool schema'}})
                        result = SMOKE.case(Path('/unexecuted-fixture/codex'), root, 'fixture',
                                            400, envelope, recover)
                    configure.assert_called_once_with(root / 'fixture', 1234, stream_max_retries=2, supports_websockets=False)
                    server.shutdown.assert_called_once_with()
                    thread.join.assert_called_once_with()
                    server.server_close.assert_called_once_with()
                    self.assertEqual(json.loads((root / 'fixture/requests.json').read_text()), requests)
                    self.assertEqual(result['requestCount'], 3 if recover else 2)
                    self.assertEqual(result['returncode'], expected_code)
                    self.assertFalse(result['passed'])
                    self.assertIn('error', result)


if __name__ == '__main__':
    unittest.main()

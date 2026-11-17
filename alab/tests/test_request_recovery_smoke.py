# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Reject false-positive packaged recovery traces; no binary is built or run."""
import copy
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import tomllib
from types import SimpleNamespace
import unittest
from unittest import mock

SOURCE = Path(__file__).resolve().parents[1] / 'smoke/request_recovery.py'
sys.path.insert(0, str(SOURCE.parent))
SPEC = importlib.util.spec_from_file_location('request_recovery_smoke', SOURCE)
SMOKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SMOKE)


def trace():
    original = [{'type': 'message', 'role': 'user', 'content': 'keep working'}]
    completed = copy.deepcopy(original) + [{'type': 'function_call', 'name': 'update_plan',
                             'call_id': SMOKE.probe.CALL_ID,
                             'arguments': json.dumps({'plan': [{'step': SMOKE.probe.PLAN,
                                                               'status': 'completed'}]})},
                            {'type': 'function_call_output',
                             'call_id': SMOKE.probe.CALL_ID, 'output': 'Plan updated'}]
    requests = [{'body': {'input': original}},
                {'body': {'input': completed}, 'headerStartedAt': 10.0, 'headerAt': 10.001},
                {'body': {'input': copy.deepcopy(completed)}, 'requestStartedAt': 12.002}]
    events = [{'type': 'turn.started'},
              {'type': 'item.completed', 'item': {'type': 'agent_message',
                                                'text': SMOKE.probe.ANSWER}},
              {'type': 'turn.completed'}]
    return events, requests


class RequestRecoverySmokeTests(unittest.TestCase):
    def test_initial_binary_read_failure_retains_a_failed_summary(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            binary = root / 'codex'
            binary.write_bytes(b'unexecuted fixture')
            output = root / 'result'
            with mock.patch.object(sys, 'argv', [str(SOURCE), '--codex', str(binary),
                                                '--output', str(output)]), \
                    mock.patch.object(SMOKE, 'digest', side_effect=PermissionError('fixture read denied')), \
                    mock.patch.object(SMOKE, 'case') as run_case, \
                    mock.patch('builtins.print'):
                try:
                    code = SMOKE.main()
                except PermissionError as error:
                    code = error
            self.assertTrue((output / 'result.json').is_file(), 'initial read failure lost the summary')
            report = json.loads((output / 'result.json').read_text())
            self.assertEqual(code, 1)
            self.assertFalse(report['passed'])
            self.assertIn('PermissionError: fixture read denied', report['error'])
            self.assertEqual(report['binary'], str(binary))
            self.assertNotIn('sha256', report)
            self.assertEqual(report['cases'], [])
            run_case.assert_not_called()

    def test_case_setup_failure_retains_the_original_error_in_the_summary(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            binary = root / 'codex'
            binary.write_bytes(b'unexecuted fixture')
            output = root / 'result'
            with mock.patch.object(sys, 'argv', [str(SOURCE), '--codex', str(binary),
                                                '--output', str(output)]), \
                    mock.patch.object(SMOKE, 'case', side_effect=RuntimeError('fixture setup failed')) as run_case, \
                    mock.patch('builtins.print'):
                try:
                    code = SMOKE.main()
                except FileNotFoundError as error:
                    code = error
            self.assertTrue((output / 'result.json').is_file(), 'case setup failure lost the summary')
            report = json.loads((output / 'result.json').read_text())
            self.assertEqual(code, 1)
            self.assertFalse(report['passed'])
            self.assertEqual(report['activeCase'], SMOKE.CASES[0][0])
            self.assertEqual(report['cases'], [{'case': SMOKE.CASES[0][0], 'passed': False,
                                                'error': 'RuntimeError: fixture setup failed'}])
            self.assertIn('FileNotFoundError', report['error'])
            run_case.assert_called_once()

    def test_interruption_retains_partial_evidence_and_its_original_exit(self):
        for stop in (KeyboardInterrupt(), SystemExit(130), SystemExit(0)):
            with self.subTest(stop=repr(stop)), tempfile.TemporaryDirectory() as directory:
                root = Path(directory).resolve()
                binary = root / 'codex'
                binary.write_bytes(b'unexecuted fixture')
                output = root / 'result'
                seen = []

                def run_case(_codex, destination, name, _status, _envelope, _recover):
                    seen.append(name)
                    if len(seen) == 2:
                        raise stop
                    (destination / name).mkdir()
                    return {'case': name, 'passed': True}

                with mock.patch.object(sys, 'argv', [str(SOURCE), '--codex', str(binary),
                                                    '--output', str(output)]), \
                        mock.patch.object(SMOKE, 'case', side_effect=run_case), \
                        mock.patch('builtins.print'):
                    with self.assertRaises(type(stop)) as caught:
                        SMOKE.main()
                self.assertIs(caught.exception, stop)
                self.assertTrue((output / 'result.json').is_file(), 'interruption lost the summary')
                report = json.loads((output / 'result.json').read_text())
                self.assertFalse(report['passed'])
                self.assertTrue(report['interrupted'])
                self.assertEqual(report['activeCase'], SMOKE.CASES[1][0])
                self.assertEqual(report['cases'], [{'case': SMOKE.CASES[0][0], 'passed': True}])
                self.assertEqual(seen, [item[0] for item in SMOKE.CASES[:2]])

    def test_lost_or_changed_binary_identity_retains_the_summary(self):
        for change in ('removed', 'changed'):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                binary = root / 'codex'
                binary.write_bytes(b'original executable')
                output = root / 'result'

                def run_case(codex, destination, name, _status, _envelope, _recover):
                    (destination / name).mkdir()
                    if name == SMOKE.CASES[-1][0]:
                        if change == 'removed':
                            codex.unlink()
                        else:
                            codex.write_bytes(b'replacement executable')
                    return {'case': name, 'passed': True}

                with mock.patch.object(sys, 'argv', [str(SOURCE), '--codex', str(binary),
                                                    '--output', str(output)]), \
                        mock.patch.object(SMOKE, 'case', side_effect=run_case), \
                        mock.patch('builtins.print'):
                    self.assertEqual(SMOKE.main(), 1)
                report = json.loads((output / 'result.json').read_text())
                self.assertFalse(report['passed'])
                self.assertIn('binary', report['error'])
                self.assertEqual(len(report['cases']), len(SMOKE.CASES))

    def test_summary_output_failure_does_not_replace_an_interruption(self):
        for stop in (KeyboardInterrupt(), SystemExit(130), SystemExit(0)):
            for failure in ('write', 'print'):
                with self.subTest(stop=repr(stop), failure=failure), tempfile.TemporaryDirectory() as directory:
                    root = Path(directory).resolve()
                    binary = root / 'codex'
                    binary.write_bytes(b'unexecuted fixture')
                    output = root / 'result'
                    save = SMOKE.probe.write_json
                    attempted = []

                    def save_summary(path, value):
                        attempted.append(path)
                        if failure == 'write':
                            raise OSError('fixture summary write denied')
                        save(path, value)

                    with mock.patch.object(sys, 'argv', [str(SOURCE), '--codex', str(binary),
                                                        '--output', str(output)]), \
                            mock.patch.object(SMOKE, 'case', side_effect=stop), \
                            mock.patch.object(SMOKE.probe, 'write_json', side_effect=save_summary), \
                            mock.patch('builtins.print', side_effect=BrokenPipeError('fixture pipe closed')):
                        with self.assertRaises(type(stop)) as caught:
                            SMOKE.main()
                    self.assertIs(caught.exception, stop)
                    self.assertEqual(attempted, [output / 'result.json'])
                    self.assertIsInstance(stop.__context__, OSError)
                    if failure == 'print':
                        self.assertFalse(json.loads((output / 'result.json').read_text())['passed'])

    def test_valid_recovery_preserves_completed_work_and_waits_for_advice(self):
        events, requests = trace()
        self.assertAlmostEqual(SMOKE.check_result(0, events, requests, True), 2.001)

    def test_recovery_requires_one_ordered_turn(self):
        for change in ('missing-start', 'repeated-start', 'missing-end', 'repeated-end',
                       'reversed', 'answer-before-start', 'answer-after-end'):
            with self.subTest(change=change):
                events, requests = trace()
                if change == 'missing-start':
                    events.pop(0)
                elif change == 'repeated-start':
                    events.insert(0, events[0])
                elif change == 'missing-end':
                    events.pop()
                elif change == 'repeated-end':
                    events.append(events[-1])
                elif change == 'reversed':
                    events[0], events[-1] = events[-1], events[0]
                else:
                    answer = events.pop(1)
                    events.insert(0 if change == 'answer-before-start' else len(events), answer)
                with self.assertRaises(RuntimeError):
                    SMOKE.check_result(0, events, requests, True)

    def test_case_cannot_report_success_without_its_original_turn(self):
        for change in ('missing-start', 'repeated-start', 'reversed',
                       'answer-before-start', 'answer-after-end'):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                events, requests = trace()
                if change == 'missing-start':
                    events.pop(0)
                elif change == 'repeated-start':
                    events.insert(0, events[0])
                elif change == 'reversed':
                    events[0], events[-1] = events[-1], events[0]
                else:
                    answer = events.pop(1)
                    events.insert(0 if change == 'answer-before-start' else len(events), answer)
                server = SimpleNamespace(server_port=1234, shutdown=mock.Mock(), server_close=mock.Mock())
                thread = SimpleNamespace(join=mock.Mock())

                def serve(model):
                    model.requests = requests
                    return server, thread

                def run(_binary, _args, output, _env, _label):
                    with (output / 'exec-stdout.jsonl').open('x') as stream:
                        for event in events:
                            stream.write(json.dumps(event) + '\n')
                    return 0

                with mock.patch.object(SMOKE.probe, 'serve', side_effect=serve), \
                        mock.patch.object(SMOKE.probe, 'run', side_effect=run):
                    result = SMOKE.case(Path('/unexecuted-fixture/codex'), root,
                                        'opaque', 400, {'detail': 'Bad Request'}, True)
                self.assertFalse(result['passed'])
                self.assertIn('error', result)
                self.assertEqual(json.loads((root / 'opaque/requests.json').read_text()), requests)
                server.shutdown.assert_called_once_with()
                thread.join.assert_called_once_with()
                server.server_close.assert_called_once_with()

    def test_successful_turn_allows_nonfatal_warning_items(self):
        events, requests = trace()
        events.insert(1, {'type': 'item.completed', 'item': {'type': 'error',
                                                         'message': 'Retrying the original request'}})
        self.assertAlmostEqual(SMOKE.check_result(0, events, requests, True), 2.001)

    def test_preserved_fixture_allows_unrelated_reasoning_items(self):
        events, requests = trace()
        requests[1]['body']['input'].insert(1, {'type': 'reasoning', 'summary': []})
        requests[2]['body']['input'] = copy.deepcopy(requests[1]['body']['input'])
        self.assertAlmostEqual(SMOKE.check_result(0, events, requests, True), 2.001)
        self.assertIsNone(SMOKE.check_result(1, [{'type': 'turn.failed'}], requests[:2], False))

    def test_missing_duplicate_or_changed_work_cannot_pass(self):
        for change in ('missing', 'duplicate', 'changed', 'count', 'answer', 'failed', 'exit', 'empty'):
            with self.subTest(change=change):
                events, requests = trace()
                if change == 'missing':
                    requests[1]['body']['input'].pop()
                elif change == 'duplicate':
                    requests[1]['body']['input'].append(requests[1]['body']['input'][-1])
                elif change == 'changed':
                    requests[2]['body']['input'][-1]['output'] = 'different'
                elif change == 'count':
                    requests.pop()
                elif change == 'answer':
                    events.insert(1, events[1])
                elif change == 'failed':
                    events.append({'type': 'turn.failed'})
                elif change == 'empty':
                    events.clear()
                with self.assertRaises(RuntimeError):
                    SMOKE.check_result(1 if change == 'exit' else 0, events, requests, True)

    def test_a_shortened_retry_delay_cannot_pass(self):
        events, requests = trace()
        requests[2]['requestStartedAt'] = 11.999
        with self.assertRaisesRegex(RuntimeError, 'Retry-After bypassed'):
            SMOKE.check_result(0, events, requests, True)

    def test_identically_malformed_retry_history_cannot_pass(self):
        for change in ('missing_call', 'duplicate_call', 'wrong_tool', 'wrong_id',
                       'reversed_pair', 'missing_prompt', 'changed_prompt'):
            with self.subTest(change=change):
                events, requests = trace()
                items = requests[1]['body']['input']
                if change == 'missing_call':
                    items.pop(1)
                elif change == 'duplicate_call':
                    items.insert(1, copy.deepcopy(items[1]))
                elif change == 'wrong_tool':
                    items[1]['name'] = 'shell'
                elif change == 'wrong_id':
                    items[1]['call_id'] = 'unrelated'
                elif change == 'reversed_pair':
                    items[1], items[2] = items[2], items[1]
                elif change == 'missing_prompt':
                    items.pop(0)
                else:
                    items[0]['content'] = 'different user instruction'
                requests[2]['body']['input'] = copy.deepcopy(items)
                with self.assertRaises(RuntimeError):
                    SMOKE.check_result(0, events, requests, True)

    def test_identically_changed_plan_arguments_or_output_cannot_pass(self):
        for recover in (True, False):
            for change in ('missing_arguments', 'invalid_arguments', 'changed_step',
                           'pending_step', 'missing_output', 'failed_output', 'changed_output'):
                with self.subTest(recover=recover, change=change):
                    events, requests = trace()
                    call, output = requests[1]['body']['input'][1:]
                    if change == 'missing_arguments':
                        del call['arguments']
                    elif change == 'invalid_arguments':
                        call['arguments'] = '{invalid'
                    elif change in ('changed_step', 'pending_step'):
                        arguments = json.loads(call['arguments'])
                        arguments['plan'][0]['step' if change == 'changed_step' else 'status'] = (
                            'different work' if change == 'changed_step' else 'pending')
                        call['arguments'] = json.dumps(arguments)
                    elif change == 'missing_output':
                        del output['output']
                    else:
                        output['output'] = ('Error: failed to parse update_plan arguments'
                                            if change == 'failed_output' else 'different result')
                    requests[2]['body']['input'] = copy.deepcopy(requests[1]['body']['input'])
                    with self.assertRaises(RuntimeError):
                        if recover:
                            SMOKE.check_result(0, events, requests, True)
                        else:
                            SMOKE.check_result(1, [{'type': 'turn.failed'}], requests[:2], False)

    def test_refusals_must_end_without_an_extra_request(self):
        _, requests = trace()
        events = [{'type': 'turn.failed'}]
        self.assertIsNone(SMOKE.check_result(1, events, requests[:2], False))
        for code, outcome, sent in [(0, events, requests[:2]),
                                    (1, [{'type': 'turn.completed'}], requests[:2]),
                                    (1, events, requests)]:
            with self.assertRaises(RuntimeError):
                SMOKE.check_result(code, outcome, sent, False)

    def test_unexpected_tool_records_cannot_pass(self):
        for recover in (True, False):
            for change in ('extra_call', 'extra_output', 'extra_pair'):
                with self.subTest(recover=recover, change=change):
                    events, requests = trace()
                    history = requests[1]['body']['input']
                    call, output = copy.deepcopy(history[1:])
                    call['call_id'] = output['call_id'] = 'unexpected-call'
                    if change != 'extra_output':
                        history.append(call)
                    if change != 'extra_call':
                        history.append(output)
                    requests[2]['body']['input'] = copy.deepcopy(history)
                    with self.assertRaises(RuntimeError):
                        if recover:
                            SMOKE.check_result(0, events, requests, True)
                        else:
                            SMOKE.check_result(1, [{'type': 'turn.failed'}], requests[:2], False)

    def test_every_fixture_completes_a_plan_then_emits_the_exact_rejection(self):
        for name, expected_status, envelope, _ in SMOKE.CASES:
            with self.subTest(name=name):
                fixture = SMOKE.Rejection(expected_status, envelope)
                first = fixture.reply({})
                self.assertEqual(first[0], 200)
                items = [json.loads(line.removeprefix('data: '))['item']
                         for line in first[1].decode().splitlines()
                         if line.startswith('data: ') and 'response.output_item.done' in line]
                self.assertEqual(items, [trace()[1][1]['body']['input'][1]])
                status, body, _, advice, record = fixture.reply({})
                self.assertEqual((status, json.loads(body)), (expected_status, envelope))
                self.assertEqual(advice, SMOKE.probe.RETRY_SECONDS)
                self.assertIn('x-request-id', record['responseHeaders'])
                self.assertEqual(fixture.reply({})[0], 200)

    def test_configuration_selects_bounded_stream_retries(self):
        for retries in (0, 2):
            with self.subTest(retries=retries), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                SMOKE.probe.fresh_environment(root)
                if retries:
                    SMOKE.probe.configure(root, 1234, stream_max_retries=retries)
                else:
                    SMOKE.probe.configure(root, 1234)
                config = tomllib.loads((root / 'codex-home/config.toml').read_text())
                provider = config['model_providers']['capacity-smoke']
                self.assertEqual(provider['stream_max_retries'], retries)
                self.assertEqual(provider['request_max_retries'], 0)
                self.assertEqual(provider['base_url'], 'http://127.0.0.1:1234/v1')


if __name__ == '__main__':
    unittest.main()

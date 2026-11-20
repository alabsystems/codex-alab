# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Reject false-positive packaged recovery traces; no binary is built or run."""
import copy
from contextlib import ExitStack
import http.client
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
    original = [{'type': 'message', 'role': 'user', 'content': [
        {'type': 'input_text', 'text': SMOKE.probe.PROMPT}]}]
    completed = copy.deepcopy(original) + [{'type': 'function_call', 'name': 'update_plan',
                             'call_id': SMOKE.probe.CALL_ID,
                             'arguments': json.dumps({'plan': [{'step': SMOKE.probe.PLAN,
                                                               'status': 'completed'}]})},
                            {'type': 'function_call_output',
                             'call_id': SMOKE.probe.CALL_ID, 'output': 'Plan updated'}]
    requests = [{'body': {'model': SMOKE.probe.MODEL, 'input': original}},
                {'body': {'model': SMOKE.probe.MODEL, 'input': completed}, 'retryAfter': 2,
                 'headerStartedAt': 10.0, 'headerAt': 10.001},
                {'body': {'model': SMOKE.probe.MODEL, 'input': copy.deepcopy(completed)}, 'requestStartedAt': 12.002}]
    metadata = json.dumps({'thread_id': 'original', 'turn_id': 'active', 'request_kind': 'turn'})
    for request in requests:
        request['body']['client_metadata'] = {'x-codex-turn-metadata': metadata}
    events = [{'type': 'thread.started', 'thread_id': 'original'}, {'type': 'turn.started'},
              {'type': 'item.completed', 'item': {'type': 'agent_message',
                                                'text': SMOKE.probe.ANSWER}},
              {'type': 'turn.completed'}]
    return events, requests


def refusal_events():
    return [{'type': 'thread.started', 'thread_id': 'original'}, {'type': 'turn.started'},
            {'type': 'turn.failed', 'error': {'message': 'Invalid tool schema'}}]


class RequestRecoverySmokeTests(unittest.TestCase):
    def test_preserved_history_must_contain_the_exact_operator_prompt_once(self):
        for defect in ('changed', 'wrong role', 'wrong content type', 'duplicate', 'added on retry'):
            with self.subTest(defect=defect):
                events, requests = trace()
                for index, request in enumerate(requests):
                    history = request['body']['input']
                    message = history[0]
                    if defect == 'changed':
                        message['content'][0]['text'] += ' changed'
                    elif defect == 'wrong role':
                        message['role'] = 'developer'
                    elif defect == 'wrong content type':
                        message['content'][0]['type'] = 'output_text'
                    elif defect == 'duplicate' or index > 0:
                        history.insert(1, copy.deepcopy(message))
                error = 'unexpected user content' if defect == 'wrong content type' else 'operator prompt'
                with self.assertRaisesRegex(RuntimeError, error):
                    SMOKE.check_result(0, events, requests, True)

    def test_compact_user_messages_are_counted_and_mixed_content_is_rejected(self):
        events, requests = trace()
        prompt = {'role': 'user', 'content': SMOKE.probe.PROMPT}
        for request in requests:
            request['body']['input'][0] = prompt
        self.assertAlmostEqual(SMOKE.check_result(0, events, requests, True), 2.001)
        for request in requests:
            request['body']['input'].insert(0, prompt)
        with self.assertRaisesRegex(RuntimeError, 'operator prompt'):
            SMOKE.check_result(0, events, requests, True)
        events, requests = trace()
        for request in requests:
            request['body']['input'][0]['content'].append({'type': 'input_image', 'image_url': 'fixture'})
        with self.assertRaisesRegex(RuntimeError, 'unexpected user content'):
            SMOKE.check_result(0, events, requests, True)

    def test_retry_retains_the_failed_requests_reasoning_settings(self):
        selected = {'effort': 'medium', 'summary': 'auto'}
        for retry in (None, {'effort': 'high', 'summary': 'auto'}, {'effort': 'medium'}):
            with self.subTest(retry=retry):
                events, requests = trace()
                requests[0]['body']['reasoning'] = selected
                requests[1]['body']['reasoning'] = selected
                if retry is not None:
                    requests[2]['body']['reasoning'] = retry
                with self.assertRaisesRegex(RuntimeError, 'reasoning'):
                    SMOKE.check_result(0, events, requests, True)
        events, requests = trace()
        for request in requests:
            request['body']['reasoning'] = selected.copy()
        self.assertAlmostEqual(SMOKE.check_result(0, events, requests, True), 2.001)

    def test_first_tool_continuation_must_keep_the_original_reasoning_settings(self):
        events, requests = trace()
        requests[0]['body']['reasoning'] = {'effort': 'medium', 'summary': 'auto'}
        with self.assertRaisesRegex(RuntimeError, 'request 2 changed the original reasoning'):
            SMOKE.check_result(0, events, requests, True)
        with self.assertRaisesRegex(RuntimeError, 'request 2 changed the original reasoning'):
            SMOKE.check_result(1, refusal_events(), requests[:2], False)

    def test_provider_requests_stay_in_the_original_exec_thread_and_turn(self):
        for recover in (True, False):
            for field, value, error in (('turn_id', 'other', 'changed turns'),
                                        ('thread_id', 'other', 'original exec thread'),
                                        ('request_kind', 'compaction', 'original exec thread')):
                with self.subTest(recover=recover, field=field):
                    events, posts = trace()
                    if not recover:
                        events, posts = refusal_events(), posts[:2]
                    metadata = posts[-1]['body']['client_metadata']
                    key = 'x-codex-turn-metadata'
                    metadata[key] = json.dumps({**json.loads(metadata[key]), field: value})
                    with self.assertRaisesRegex(RuntimeError, error):
                        SMOKE.check_result(0 if recover else 1, events, posts, recover)

    def test_exec_thread_must_be_reported_once_before_the_turn(self):
        events, posts = trace()
        thread, *turn = events
        for invalid in (turn, [thread, *events], [*turn, thread]):
            with self.subTest(events=invalid), self.assertRaisesRegex(RuntimeError, 'exec thread'):
                SMOKE.check_result(0, invalid, posts, True)

    def test_wire_identity_must_match_the_selected_rollouts_single_completed_turn(self):
        events, posts = trace()
        meta = {'type': 'session_meta', 'payload': {'id': 'original'}}

        def event(kind, turn='active'):
            payload = {'type': kind, 'turn_id': turn}
            if kind in ('task_complete', 'turn_complete'):
                payload['last_agent_message'] = SMOKE.probe.ANSWER
            return {'type': 'event_msg', 'payload': payload}

        start, end = event('task_started'), event('task_complete')
        prompt, call, output = [{'type': 'response_item', 'payload': item} for item in posts[1]['body']['input']]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            sessions = root / 'codex-home/sessions'
            sessions.mkdir(parents=True)
            path = sessions / 'rollout-fixture-original.jsonl'
            foreign = sessions / 'rollout-fixture-foreign.jsonl'
            foreign.write_text('\n'.join(map(json.dumps, [meta, start, end])))
            with self.assertRaisesRegex(ValueError, 'one saved rollout'):
                SMOKE.check_result(0, events, posts, True, case_dir=root)
            path.symlink_to(foreign)
            with self.assertRaisesRegex(ValueError, 'one saved rollout'):
                SMOKE.check_result(0, events, posts, True, case_dir=root)
            path.unlink()
            for rows in ([meta, start, prompt, call, output, end],
                         [meta, event('turn_started'), prompt, call, output, event('turn_complete')]):
                path.write_text('\n'.join(map(json.dumps, rows)))
                self.assertAlmostEqual(SMOKE.check_result(0, events, posts, True, case_dir=root), 2.001)
            for records in ([meta, event('task_started', 'other'), event('task_complete', 'other')],
                            [meta, start, end, start, end], [meta, end, start], [meta, start],
                            [meta, start, event('turn_aborted'), end],
                            [{'type': 'session_meta', 'payload': {'id': 'foreign'}}, start, end]):
                path.write_text('\n'.join(map(json.dumps, records)))
                with self.subTest(records=records), self.assertRaisesRegex(ValueError, 'capacity rollout'):
                    SMOKE.check_result(0, events, posts, True, case_dir=root)
            other_turn = {'type': 'response_item', 'payload': {
                **output['payload'], 'internal_chat_message_metadata_passthrough': {'turn_id': 'another'}}}
            for tail, error in (([call, end], 'output is missing'),
                                ([call, output, output, end], 'duplicated'),
                                ([output, call, end], 'precedes its call'),
                                ([call, end, output], 'outside the active turn'),
                                ([call, other_turn, end], 'conflicting turn attribution')):
                path.write_text('\n'.join(map(json.dumps, [meta, start, prompt, *tail])))
                with self.subTest(error=error), self.assertRaisesRegex(ValueError, error):
                    SMOKE.check_result(0, events, posts, True, case_dir=root)
            for recover, saved in ((True, {'last_agent_message': 'unrelated answer'}),
                                   (True, {'last_agent_message': SMOKE.probe.ANSWER, 'error': {}}),
                                   (False, {}), (False, {'error': {'message': 'different refusal'}})):
                path.write_text('\n'.join(map(json.dumps, [meta, start, prompt, call, output, {'type': 'event_msg', 'payload': {
                    'type': 'task_complete', 'turn_id': 'active', **saved}}])))
                with self.subTest(saved=saved), self.assertRaisesRegex(ValueError, 'saved'):
                    if recover:
                        SMOKE.check_result(0, events, posts, True, case_dir=root)
                    else:
                        refusal = {'message': 'Invalid tool schema', 'codex_error_info': 'invalid_prompt'}
                        SMOKE.check_result(1, refusal_events(), posts[:2], False, case_dir=root, refusal=refusal)
            refusal = SMOKE.expected_refusal(403, {'error': SMOKE.OVERLOAD}, 16789)
            refused = refusal_events()
            refused[-1]['error'] = {'message': refusal['message']}
            for kind, valid in ((None, False), ('server_overloaded', False),
                                ({'http_connection_failed': {'http_status_code': 503}}, False),
                                ({'http_connection_failed': {'http_status_code': 403.0}}, False),
                                (refusal['codex_error_info'], True)):
                error = {'message': refusal['message']}
                if kind is not None:
                    error['codex_error_info'] = kind
                terminal = event('task_complete')
                terminal['payload'].update(last_agent_message=None, error=error)
                path.write_text('\n'.join(map(json.dumps, [meta, start, prompt, call, output, terminal])))
                with self.subTest(kind=kind):
                    if valid:
                        self.assertIsNone(SMOKE.check_result(1, refused, posts[:2], False, case_dir=root, refusal=refusal))
                    else:
                        with self.assertRaisesRegex(ValueError, 'wrong error classification'):
                            SMOKE.check_result(1, refused, posts[:2], False, case_dir=root, refusal=refusal)
            (sessions / 'rollout-duplicate-original.jsonl').touch()
            with self.assertRaisesRegex(ValueError, 'one saved rollout'):
                SMOKE.check_result(1, refusal_events(), posts[:2], False, case_dir=root)

    def test_every_request_must_use_the_configured_model(self):
        for recover, count in ((True, 3), (False, 2)):
            for index in range(count):
                for replacement in (None, 'another-model'):
                    with self.subTest(recover=recover, index=index, replacement=replacement):
                        events, requests = trace()
                        body = requests[index]['body']
                        if replacement is None:
                            del body['model']
                        else:
                            body['model'] = replacement
                        if not recover:
                            events = refusal_events()
                        with self.assertRaisesRegex(RuntimeError, 'configured model'):
                            SMOKE.check_result(0 if recover else 1, events, requests[:count], recover)

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
                    case_dir = destination / name
                    case_dir.mkdir()
                    if len(seen) == 2:
                        (case_dir / "requests.json").write_text("[{}, {}]")
                        (case_dir / "exec-stdout.jsonl").write_bytes(b'{"type":"turn.started"}\n{"type":')
                        raise stop
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
                self.assertEqual(report['activeCaseObservations'], {
                    'requestCount': 2, 'turnEvents': ['turn.started'], 'invalidStdoutLines': [2]})
                self.assertEqual(report['cases'], [{'case': SMOKE.CASES[0][0], 'passed': True}])
                self.assertEqual(seen, [item[0] for item in SMOKE.CASES[:2]])

    def test_partial_evidence_read_cannot_replace_the_case_interruption(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / 'codex'
            binary.write_bytes(b'unexecuted fixture')
            output = root / 'result'
            stop = SystemExit(130)
            with mock.patch.object(sys, 'argv', [str(SOURCE), '--codex', str(binary), '--output', str(output)]), \
                    mock.patch.object(SMOKE, 'case', side_effect=stop), \
                    mock.patch.object(SMOKE, 'partial_observations', side_effect=KeyboardInterrupt('read stopped')), \
                    mock.patch('builtins.print'), self.assertRaises(SystemExit) as caught:
                SMOKE.main()
            self.assertIs(caught.exception, stop)
            report = json.loads((output / 'result.json').read_bytes())
            self.assertEqual(report['error'], 'SystemExit: 130')
            self.assertEqual(report['activeCaseObservationError'], 'KeyboardInterrupt: read stopped')
            self.assertNotIn('activeCaseObservations', report)

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

    def test_repeated_overload_checks_each_deadline_history_and_notice(self):
        prefix = 'Model is at capacity; retrying automatically (interrupt to stop)...'
        for defect in (None, 'first wait', 'second wait', 'intermediate input', 'first notice', 'extra notice'):
            with self.subTest(defect=defect):
                events, requests = trace()
                for index, seconds in enumerate((2, 3), start=1):
                    events.insert(index + 1, {'type': 'item.completed', 'item': {'type': 'error',
                        'message': f'{prefix} (Retry scheduled in {seconds}s.)'}})
                requests.append(copy.deepcopy(requests[-1]))
                requests[2].update(headerStartedAt=13.0, headerAt=13.001, retryAfter=3)
                requests[3]['requestStartedAt'] = 16.002
                if defect == 'first wait':
                    requests[2]['requestStartedAt'] = 11.99
                elif defect == 'second wait':
                    requests[3]['requestStartedAt'] = 15.5
                elif defect == 'intermediate input':
                    requests[2]['body']['input'].pop()
                elif defect == 'first notice':
                    del events[2]
                elif defect == 'extra notice':
                    events.insert(2, events[2])
                options = dict(expect_capacity_notice=True, waits=(2, 3))
                if defect:
                    with self.assertRaises(RuntimeError):
                        SMOKE.check_result(0, events, requests, True, **options)
                else:
                    self.assertAlmostEqual(SMOKE.check_result(0, events, requests, True, **options), 3.001)

    def test_unadvised_retries_grow_the_local_wait_without_changing_tool_history(self):
        events, requests = trace()
        requests.append(copy.deepcopy(requests[-1]))
        requests[1].update(status=503, retryAfter=None)
        requests[2].update(status=503, retryAfter=None, requestStartedAt=10.75,
                           headerStartedAt=11.0, headerAt=11.001)
        requests[3]["requestStartedAt"] = 12.5
        for index in (1, 2):
            events.insert(index + 1, {"type": "item.completed", "item": {"type": "error", "message":
                f"Model is at capacity; retrying automatically (interrupt to stop)... (Retry scheduled in {index}s.)"}})
        options = dict(expect_capacity_notice=True, waits=(None, None))
        self.assertAlmostEqual(SMOKE.check_result(0, events, requests, True, **options), 1.499)
        requests.append(copy.deepcopy(requests[-1]))
        requests[3].update(status=503, retryAfter=1, headerStartedAt=13.0, headerAt=13.001)
        requests[4]["requestStartedAt"] = 16.5
        events.insert(4, {"type": "item.completed", "item": {"type": "error", "message":
            "Model is at capacity; retrying automatically (interrupt to stop)... (Retry scheduled in 4s.)"}})
        options["waits"] = (None, None, 1)
        self.assertAlmostEqual(SMOKE.check_result(0, events, requests, True, **options), 3.499)
        third_notice = events[4]["item"]
        third_notice["message"] = third_notice["message"].replace("4s.", "1s.")
        with self.assertRaisesRegex(RuntimeError, "capacity notice did not report the selected wait"):
            SMOKE.check_result(0, events, requests, True, **options)
        third_notice["message"] = third_notice["message"].replace("1s.", "2s.")
        self.assertAlmostEqual(SMOKE.check_result(0, events, requests, True, **options), 3.499)
        requests[4]["requestStartedAt"] = 14.5
        with self.assertRaisesRegex(RuntimeError, "local capacity backoff bypassed"):
            SMOKE.check_result(0, events, requests, True, **options)
        requests[4]["requestStartedAt"] = 16.5
        requests[3]["requestStartedAt"] = 11.75
        with self.assertRaisesRegex(RuntimeError, "local capacity backoff bypassed"):
            SMOKE.check_result(0, events, requests, True, **options)

    def test_recovery_requires_one_ordered_turn(self):
        for change in ('missing-start', 'repeated-start', 'missing-end', 'repeated-end',
                       'reversed', 'answer-before-start', 'answer-after-end'):
            with self.subTest(change=change):
                events, requests = trace()
                if change == 'missing-start':
                    events.pop(1)
                elif change == 'repeated-start':
                    events.insert(1, events[1])
                elif change == 'missing-end':
                    events.pop()
                elif change == 'repeated-end':
                    events.append(events[-1])
                elif change == 'reversed':
                    events[1], events[-1] = events[-1], events[1]
                else:
                    answer = events.pop(2)
                    events.insert(1 if change == 'answer-before-start' else len(events), answer)
                with self.assertRaises(RuntimeError):
                    SMOKE.check_result(0, events, requests, True)

    def test_case_cannot_report_success_without_its_original_turn(self):
        for change in ('missing-start', 'repeated-start', 'reversed',
                       'answer-before-start', 'answer-after-end'):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                events, requests = trace()
                if change == 'missing-start':
                    events.pop(1)
                elif change == 'repeated-start':
                    events.insert(1, events[1])
                elif change == 'reversed':
                    events[1], events[-1] = events[-1], events[1]
                else:
                    answer = events.pop(2)
                    events.insert(1 if change == 'answer-before-start' else len(events), answer)
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
        self.assertIsNone(SMOKE.check_result(1, refusal_events(), requests[:2], False))

    def test_unattended_tool_continuation_cannot_include_another_user_message(self):
        for position in (1, 2, 3):
            with self.subTest(position=position):
                events, requests = trace()
                nudge = {'type': 'message', 'role': 'user',
                         'content': [{'type': 'input_text', 'text': 'continue'}]}
                for request in requests[1:]:
                    request['body']['input'].insert(position, nudge)
                with self.assertRaisesRegex(RuntimeError, 'new user message'):
                    SMOKE.check_result(0, events, requests, True)

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
                    events.insert(2, events[2])
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

    def test_failed_tool_stream_retries_once_with_its_completed_work(self):
        for defect in (None, 'early', 'missing output', 'unexpected input', 'extra request'):
            with self.subTest(defect=defect):
                events, (failed, _, retry) = trace()
                failed.update(headerStartedAt=10.0, headerAt=10.001)
                attempts = [failed, retry]
                if defect == 'early':
                    retry['requestStartedAt'] = 11.9
                elif defect == 'missing output':
                    retry['body']['input'].pop()
                elif defect == 'unexpected input':
                    retry['body']['input'].append({'type': 'message', 'role': 'user', 'content': []})
                elif defect == 'extra request':
                    attempts.append(retry)
                if defect:
                    with self.assertRaises(RuntimeError):
                        SMOKE.check_result(0, events, attempts, True, fail_with_tool=True)
                else:
                    self.assertAlmostEqual(
                        SMOKE.check_result(0, events, attempts, True, fail_with_tool=True), 2.001)

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
                            SMOKE.check_result(1, refusal_events(), requests[:2], False)

    def test_refusal_must_fail_its_original_turn_without_an_answer(self):
        _, requests = trace()
        thread, started, failed = refusal_events()
        answer = {'type': 'item.completed', 'item': {'type': 'agent_message', 'text': SMOKE.probe.ANSWER}}
        for events in ([failed], [failed, started], [started, started, failed],
                       [started, failed, started], [started, answer, failed], [started, failed, answer]):
            with self.subTest(events=events), self.assertRaises(RuntimeError):
                SMOKE.check_result(1, [thread, *events], requests[:2], False)

    def test_refusals_must_end_without_an_extra_request(self):
        _, requests = trace()
        events = refusal_events()
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
                            SMOKE.check_result(1, refusal_events(), requests[:2], False)

    def test_model_post_refusals_complete_a_plan_then_emit_the_exact_rejection(self):
        for name, expected_status, envelope, _ in SMOKE.CASES:
            if name == "prewarm-overload":
                continue
            with self.subTest(name=name):
                fail_with_tool = name == 'tool-stream-overload'
                waits = {"repeated-overload": (2, 3), "unadvised-overload": (None, None, 1)}.get(name, (2,))
                codes = ("server_is_overloaded", "policy_capacity_unavailable") if name == "repeated-overload" else ()
                fixture = SMOKE.Rejection(expected_status, envelope, fail_with_tool=fail_with_tool, waits=waits,
                                          capacity_codes=codes)
                first = fixture.reply({})
                self.assertEqual(first[0], 200)
                items = [json.loads(line.removeprefix('data: '))['item']
                         for line in first[1].decode().splitlines()
                         if line.startswith('data: ') and 'response.output_item.done' in line]
                self.assertEqual(items, [trace()[1][1]['body']['input'][1]])
                if fail_with_tool:
                    frames = [json.loads(line.removeprefix('data: ')) for line in first[1].decode().splitlines()
                              if line.startswith('data: ')]
                    self.assertEqual([f['type'] for f in frames],
                                     ['response.created', 'response.output_item.done', 'response.failed'])
                    self.assertEqual(frames[-1]['response'], {
                        'id': frames[0]['response']['id'], 'status': 'failed',
                        'error': {'code': 'server_is_overloaded', 'message': 'Model is at capacity.'}})
                    self.assertEqual(first[2:4], ('text/event-stream', SMOKE.probe.RETRY_SECONDS))
                    self.assertIn(SMOKE.probe.ANSWER.encode(), fixture.reply({})[1])
                    self.assertEqual(fixture.reply({})[0], 400)
                    continue
                status, body, content_type, advice, record = fixture.reply({})
                self.assertEqual(status, expected_status)
                if status == 200:
                    self.assertEqual(content_type, 'text/event-stream')
                    self.assertEqual(body.decode().splitlines()[0], 'event: response.failed')
                    data, = [json.loads(line.removeprefix('data: ')) for line in body.decode().splitlines()
                             if line.startswith('data: ')]
                    error = ({'code': 'misalignment_policy_violation', 'message': 'Fixture policy review is required.'}
                             if name == 'streamed-misalignment' else
                             {'code': 'server_is_overloaded', 'message': 'Model is at capacity.'})
                    self.assertEqual(data, {'type': 'response.failed', 'response': {
                        'id': 'response-2', 'status': 'failed', 'error': error}})
                else:
                    self.assertEqual(content_type, 'application/json')
                    self.assertEqual(json.loads(body), envelope)
                self.assertEqual(advice, waits[0])
                self.assertIn('x-request-id', record['responseHeaders'])
                for attempt, expected_advice in enumerate(waits[1:], 1):
                    status, body, _, advice, record = fixture.reply({})
                    expected = envelope
                    if codes:
                        expected = {"error": {**envelope["error"], "code": codes[attempt]}}
                        self.assertEqual(record["capacityCode"], codes[attempt])
                    self.assertEqual((status, json.loads(body), advice), (503, expected, expected_advice))
                self.assertEqual(fixture.reply({})[0], 200)
                self.assertEqual(fixture.reply({})[0], 400)

    def test_streamed_failure_counts_only_a_retry_before_http_eof(self):
        name, status, envelope, _ = next(case for case in SMOKE.CASES if case[0] == "streamed-overload")
        for close_first in (False, True):
            with self.subTest(case=name, close_first=close_first), ExitStack() as stack:
                model = SMOKE.OpenFailureStream(status, envelope)
                server, worker = SMOKE.probe.serve(model, send_failure_stream=model.send_failure_stream)
                stack.callback(server.server_close)
                stack.callback(worker.join)
                stack.callback(server.shutdown)
                stack.callback(model.release.set)

                def post():
                    conn = http.client.HTTPConnection("127.0.0.1", server.server_port, timeout=3)
                    stack.callback(conn.close)
                    conn.request("POST", "/v1/responses", body=b"{}")
                    return stack.enter_context(conn.getresponse())

                post().read()
                failure = post()
                self.assertIsNone(failure.getheader("content-length"))
                frame = SMOKE.probe.sse([envelope])
                self.assertEqual(failure.read(len(frame)), frame)
                self.assertTrue(model.stream_open.wait(2))
                if close_first:
                    model.release.set()
                    self.assertEqual(failure.read(), b"")
                post().read()
                failed, retry = model.requests[1:]
                if close_first:
                    with self.assertRaisesRegex(ValueError, "while the failed SSE body was open"):
                        SMOKE.probe.provider_fixture.check_open_stream_retry(failed, retry)
                else:
                    SMOKE.probe.provider_fixture.check_open_stream_retry(failed, retry)
                    self.assertEqual(failure.read(), b"")

    def test_prewarm_refusal_and_foreground_fallback_use_the_same_thread(self):
        fixture = SMOKE.PrewarmRejection(503, {"error": SMOKE.OVERLOAD})
        server, worker = SMOKE.probe.serve(fixture, upgrade=fixture.upgrade)
        self.addCleanup(server.server_close)
        self.addCleanup(worker.join)
        self.addCleanup(server.shutdown)
        for kind, turn, status, delay in (("prewarm", "", 503, "2"), ("turn", "active", 426, None)):
            conn = http.client.HTTPConnection("127.0.0.1", server.server_port, timeout=3)
            self.addCleanup(conn.close)
            metadata = dict(thread_id="original", turn_id=turn, request_kind=kind)
            conn.request("GET", "/v1/responses", headers={"Upgrade": "websocket",
                         "x-codex-turn-metadata": json.dumps(metadata)})
            response = conn.getresponse()
            self.assertEqual(response.version, 11)
            self.assertEqual((response.status, response.getheader("Retry-After")), (status, delay))
            body = json.loads(response.read())
            if kind == "prewarm":
                self.assertEqual(body, {"error": SMOKE.OVERLOAD})

        # Use deterministic elapsed time without sleeping through a provider delay.
        events, posts = trace()
        startup, turn = fixture.handshakes
        turn["requestStartedAt"] = startup["headerAt"] + 2
        turn["headerStartedAt"] = turn["headerAt"] = turn["requestStartedAt"]
        posts = posts[:2]
        for index, post in enumerate(posts):
            post["requestStartedAt"] = turn["requestStartedAt"] + index + 1
            post["headerStartedAt"] = post["headerAt"] = post["requestStartedAt"]
        events.insert(2, {"type": "item.completed", "item": {"type": "error", "message":
                      "Model is at capacity; retrying automatically (interrupt to stop)... (Retry scheduled in 2s.)"}})
        self.assertEqual(SMOKE.check_result(0, events, posts, True, expect_capacity_notice=True,
                                            waits=(), handshakes=fixture.handshakes), 2)
        turn["threadId"] = "other"
        with self.assertRaisesRegex(ValueError, "original thread"):
            SMOKE.check_result(0, events, posts, True, waits=(), handshakes=fixture.handshakes)

    def test_case_configures_independent_capacity_and_ordinary_retry_budgets(self):
        for case in SMOKE.CASES:
            name = case[0]
            with self.subTest(case=name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                server = mock.Mock(server_port=1234)
                with mock.patch.object(SMOKE.probe, 'serve', return_value=(server, mock.Mock())), \
                        mock.patch.object(SMOKE.probe, 'run', side_effect=FileNotFoundError('probe binary')), \
                        self.assertRaisesRegex(FileNotFoundError, 'probe binary'):
                    SMOKE.case(root / 'unexecuted', root, *case)
                config = tomllib.loads((root / name / 'codex-home/config.toml').read_text())
                provider = config['model_providers']['capacity-smoke']
                capacity = name in ('overload', 'streamed-overload', 'late-stream-overload', 'tool-stream-overload', 'prewarm-overload', 'repeated-overload', 'unadvised-overload')
                self.assertEqual(provider.get('supports_websockets', False), name == 'prewarm-overload')
                self.assertEqual(provider['stream_max_retries'], 0 if capacity else 2)
                self.assertEqual(provider['request_max_retries'], 0)
                self.assertEqual(provider['base_url'], 'http://127.0.0.1:1234/v1')


if __name__ == '__main__':
    unittest.main()

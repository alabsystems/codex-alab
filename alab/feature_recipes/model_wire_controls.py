# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise model discovery against the source-selected CLI and loopback providers."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import threading
import time
import unittest

workspace = Path(os.environ['ALAB_FEATURE_WORKSPACE'])
binary = Path(os.environ['ALAB_FEATURE_BINARY'])
results = Path(os.environ['ALAB_FEATURE_RESULTS'])
smoke = workspace / 'alab/smoke'
sys.path.insert(0, str(smoke))
import capacity_exec as fixture
import kimi
import model_resume
from quota_resume import Rpc


def run_smoke(script, output, *args):
    # Isolated Python still needs the source checkout's sibling smoke modules.
    bootstrap = ('import runpy,sys;from pathlib import Path;'
                 'p=Path(sys.argv.pop(1));sys.path.insert(0,str(p.parent));'
                 'runpy.run_path(str(p),run_name="__main__")')
    with binary.open('rb') as stream:
        digest = hashlib.file_digest(stream, 'sha256').hexdigest()
    subprocess.run([sys.executable, '-IB', '-c', bootstrap, str(smoke / script),
                    '--codex', str(binary), '--output', str(output), *args], check=True)
    report = json.loads((output / 'result.json').read_bytes())
    if report.get('passed') is not True or report['binary_sha256'] != digest:
        raise AssertionError(f'{script} result does not verify the selected executable')
    return report


def live_shell_handle(output):
    session = re.search(r'^Process running with session ID (\d+)$', output, re.M)
    invocation = re.search(r'^Invocation ID: ([0-9a-f-]+)$', output, re.M)
    if session and invocation:
        return {'session_id': int(session[1]), 'invocation_id': invocation[1]}
    return None


class Inference:
    def __init__(self):
        self.requests = []

    def record(self, body, authorization):
        reasoning = body.get('reasoning') or {}
        self.requests.append((body['model'], reasoning.get('effort'), 'effort' in reasoning,
                              authorization == 'Bearer loopback-kimi-subscription-key'))
        return 200, kimi.reply_events(continued=True), 'text/event-stream'


class ModelWireControls(unittest.TestCase):
    def test_kimi_profiles_list_and_serve_every_documented_model(self):
        backend = Inference()
        server = kimi.serve(backend)
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        for profile in ('kimi', 'kimi-cn'):
            with self.subTest(profile=profile), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                env = fixture.fresh_environment(root)
                env['KIMI_API_KEY'] = 'loopback-kimi-subscription-key'
                url = f'http://127.0.0.1:{server.server_port}/coding/v1'
                config = (f'model_providers.{profile}.base_url = "{url}"\n'
                          'analytics.enabled = false\nfeatures.daemon_auto_start = false\n')
                (root / 'codex-home/config.toml').write_text(config)

                def discover(process, log):
                    rpc = Rpc(process, log)
                    rpc.initialize()
                    catalog = rpc.request('model/list', {'modelProvider': profile, 'includeHidden': True})
                    self.assertIsNone(catalog['nextCursor'])
                    rows = {row['model'] for row in catalog['data']}
                    self.assertTrue(kimi.MODEL_EFFORTS.keys() <= rows, rows)

                with self.subTest(surface='model/list'):
                    self.assertEqual(fixture.run(binary, ['app-server', '--listen', 'stdio://'],
                                                 root, env, 'catalog', interact=discover), 0)
                previous = len(backend.requests)
                for model in kimi.MODEL_EFFORTS:
                    kimi.run_model(binary, root, server.server_port, profile, model)
                kimi.run_model(binary, root, server.server_port, profile, 'kimi-for-coding', label='explicit-max',
                               overrides=[('model_reasoning_effort', 'max')])
                highspeed = 'kimi-for-coding-highspeed'
                catalog = root / 'administrator-models.json'
                catalog.write_text(json.dumps({'models': [model_resume.model(highspeed)]}))
                kimi.run_model(binary, root, server.server_port, profile, highspeed, label='administrator-catalog',
                               overrides=[('model_catalog_json', str(catalog))])
                expected = [*kimi.MODEL_EFFORTS.items(), ('kimi-for-coding', 'max'), (highspeed, 'high')]
                self.assertEqual(backend.requests[previous:],
                                 [(model, effort, effort is not None, True) for model, effort in expected])

    def test_cold_and_loaded_threads_use_the_announced_successor(self):
        for surface in ('exec', 'live', 'loaded', 'unloaded'):
            with self.subTest(surface=surface):
                run_smoke('model_resume.py', results / ('model-resume-' + surface),
                          '--surface', surface)

    def test_one_catalog_refresh_updates_each_following_thread_and_preserves_a_pin(self):
        self.check_loaded_model_families('shared-model-refresh')

    def test_a_peer_process_catalog_refresh_reaches_idle_following_threads(self):
        self.check_loaded_model_families('peer-model-refresh', peer=True)

    def test_first_turn_immediately_after_a_peer_catalog_refresh_uses_the_successor(self):
        self.check_loaded_model_families('peer-immediate-turn', peer=True, immediate=True)

    def test_peer_release_during_a_tool_turn_takes_effect_on_the_next_turn(self):
        old, new = 'gpt-6.1-sol', 'gpt-6.2-sol'
        active_prompt = 'Read the current goal and finish this turn.'
        arrived, release = threading.Event(), threading.Event()
        requests = []

        class Catalog(model_resume.Gateway):
            def catalog_rows(self):
                original = model_resume.model(old, new if self.released else None)
                original['multi_agent_reasoning_effort'] = 'xhigh'
                return [original, model_resume.model(new)] if self.released else [original]

            def response(self, body, headers):
                users = model_resume.messages(body, 'user')
                outputs = [item['output'] for item in body['input']
                           if item.get('type') == 'function_call_output' and item['call_id'] == 'check-goal']
                requests.append({'thread': headers['thread-id'],
                                 'turn': json.loads(headers['x-codex-turn-metadata'])['turn_id'],
                                 'model': body['model'], 'effort': body['reasoning']['effort'], 'users': users,
                                 'replies': model_resume.messages(body, 'assistant'), 'goal_outputs': outputs})
                if users[-1] == active_prompt and not outputs:
                    arrived.set()
                    fixture.require(release.wait(30), 'the peer did not finish listing the released model')
                    item = {'type': 'function_call', 'name': 'get_goal', 'call_id': 'check-goal', 'arguments': '{}'}
                else:
                    reply = model_resume.REPLY if users[-1] == model_resume.SEED else model_resume.ANSWER
                    item = {'type': 'message', 'role': 'assistant', 'id': f'answer-{len(requests)}',
                            'content': [{'type': 'output_text', 'text': reply}]}
                return fixture.response(f'active-release-{len(requests)}', item)

        root = results / 'active-tool-release'
        root.mkdir()
        env = fixture.fresh_environment(root)
        gateway = Catalog([])
        server, worker = fixture.start_listener(gateway.handler())
        self.addCleanup(worker.join)
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        self.addCleanup(release.set)
        self.addCleanup(fixture.write_json, root / 'requests.json', requests)
        model_resume.configure(root, server.server_port, 'ultra', model_preference=old)
        config = root / 'codex-home/config.toml'
        config.write_text('features.goals = true\n' + config.read_text())
        (root / 'codex-home/auth.json').write_text(json.dumps({'OPENAI_API_KEY': 'sk-loopback'}))
        work = str(root / 'work')
        thread = None
        turns = []

        def app(label, interact):
            def connected(process, log):
                rpc = Rpc(process, log)
                rpc.initialize()
                interact(rpc)
            self.assertEqual(fixture.run(binary, ['app-server', '--listen', 'stdio://'], root, env,
                                         label, interact=connected), 0)

        def seed(rpc):
            nonlocal thread
            thread = rpc.request('thread/start', {'cwd': work, 'ephemeral': False})['thread']['id']
            turn = rpc.turn(thread, model_resume.SEED)
            self.assertEqual(turn['status'], 'completed')
            turns.append(turn['id'])

        def peer(rpc):
            self.assertIn(new, [row['model'] for row in rpc.request('model/list', {})['data']])

        def resume(rpc):
            saved = rpc.request('thread/resume', {'threadId': thread, 'cwd': work})
            self.assertEqual((saved['thread']['id'], saved['model'], saved['reasoningEffort']), (thread, old, 'ultra'))
            active = rpc.request('turn/start', {'threadId': thread, 'input': [
                {'type': 'text', 'text': active_prompt, 'text_elements': []}]})['turn']['id']
            try:
                self.assertTrue(arrived.wait(20), 'the resumed turn did not reach the provider')
                with gateway.lock:
                    gateway.released = True
                app('peer', peer)
            finally:
                release.set()
            completed = rpc.wait(lambda m: m.get('method') == 'turn/completed' and m['params']['threadId'] == thread
                                 and m['params']['turn']['id'] == active)
            self.assertEqual(completed['params']['turn']['status'], 'completed')
            final = rpc.turn(thread, model_resume.AFTER_RELEASE)
            self.assertEqual(final['status'], 'completed')
            turns.extend([active, final['id']])

        app('seed', seed)
        app('resume', resume)
        first, active, final = turns
        self.assertEqual(len(set(turns)), 3)
        self.assertEqual([(r['thread'], r['turn'], r['model'], r['effort']) for r in requests], [
            (thread, first, old, 'xhigh'), (thread, active, old, 'xhigh'),
            (thread, active, old, 'xhigh'), (thread, final, new, 'high')])
        self.assertIsNone(json.loads(requests[2]['goal_outputs'][0])['goal'])
        prompts = [model_resume.SEED, active_prompt, model_resume.AFTER_RELEASE]
        self.assertEqual([text for text in requests[-1]['users'] if text in prompts], prompts)
        self.assertIn(model_resume.REPLY, requests[-1]['replies'])
        self.assertIn(model_resume.ANSWER, requests[-1]['replies'])

    def test_pre_stream_model_retirement_continues_the_same_resumed_turn(self):
        self.check_retired_model('successor')

    def test_retirement_on_a_tool_result_reuses_the_completed_call_in_the_same_turn(self):
        self.check_retired_model('after-tool')

    def test_successor_writes_to_the_original_live_shell_after_retirement(self):
        for case in ('shell-control', 'shell-retired'):
            with self.subTest(case=case):
                self.check_retired_model(case)
        for pair in [('gpt-6.1-astra', 'gpt-6.2-astra'), ('deployment-amber', 'deployment-jade')]:
            with self.subTest(retired=pair[0], successor=pair[1]):
                self.check_retired_model('shell-retired', models=pair)

    def test_retirement_requires_a_following_session_and_an_eligible_successor(self):
        for case in ('pinned', 'turn-pin', 'no-successor', 'unrelated', 'catalog-unavailable', 'unknown-error'):
            with self.subTest(case=case):
                self.check_retired_model(case)

    def test_retirement_recovery_stops_if_the_successor_also_refuses_the_turn(self):
        self.check_retired_model('second-retirement')

    def test_streamed_model_refusal_preserves_visible_output_and_recovers_following_turns(self):
        for case in ('stream-first', 'stream-created', 'stream-delivered', 'stream-pin', 'stream-generic'):
            with self.subTest(case=case):
                self.check_retired_model(case)

    def check_retired_model(self, case, models=None):
        old, new = models or ('gpt-6.1-sol', 'gpt-6.2-sol')
        later = 'gpt-6.3-sol'
        root = results / ('model-retirement-' + case + ('-' + old if models else ''))
        root.mkdir()
        env = fixture.fresh_environment(root)
        events = []
        shell = case in ('shell-control', 'shell-retired')
        streamed = case.startswith('stream-')
        delivered = case == 'stream-delivered'
        partial = 'This part of the answer has already been delivered.'
        pinned = case in ('pinned', 'stream-pin')
        continues = shell or case in ('successor', 'after-tool', 'stream-first', 'stream-created')

        class Catalog(model_resume.Gateway):
            successor_retired = False

            def catalog_rows(self):
                successor = new if self.released and case not in ('no-successor', 'unrelated') else None
                original = model_resume.model(old, successor)
                original['multi_agent_reasoning_effort'] = 'xhigh'
                rows = [original]
                if successor:
                    rows.append(model_resume.model(new, later if self.successor_retired else None))
                if self.successor_retired:
                    rows.append(model_resume.model(later))
                if self.released and case == 'unrelated':
                    rows.append(model_resume.model('gpt-6.2-astra'))
                return rows

        gateway = Catalog(events)

        class Handler(gateway.handler()):
            def do_GET(self):
                if case == 'catalog-unavailable' and gateway.released and self.path.split('?')[0] == '/v1/models':
                    self.send_error(503)
                else:
                    super().do_GET()

            def do_POST(self):
                if self.path != '/v1/responses':
                    self.send_error(404)
                    return
                body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                model = body['model']
                users = model_resume.messages(body, 'user')
                turn = json.loads(self.headers['x-codex-turn-metadata'])['turn_id']
                seed = users[-1] == model_resume.SEED
                outputs = [i['output'] for i in body['input'] if i.get('type') == 'function_call_output'
                           and i.get('call_id') == 'read-once']
                poll_outputs = [i['output'] for i in body['input'] if i.get('type') == 'function_call_output'
                                and i.get('call_id') == 'write-once']
                continuing = users[-1] == model_resume.CONTINUE
                issue_tool = (shell or case == 'after-tool') and continuing and not outputs
                retired = model == old or case == 'second-retirement' and model == new
                reject = not seed and not issue_tool and case != 'shell-control' and retired
                with gateway.lock:
                    events.append({'event': 'request', 'thread': self.headers['thread-id'], 'turn': turn,
                                   'model': model, 'effort': body['reasoning']['effort'], 'users': users,
                                   'history': model_resume.messages(body, 'assistant'), 'outputs': outputs,
                                   'issue_tool': issue_tool, 'poll_outputs': poll_outputs,
                                   'status': 404 if reject and not streamed else 200})
                    if reject:
                        gateway.released = True
                        gateway.successor_retired = model == new
                if reject:
                    code = 'resource_not_found' if case in ('unknown-error', 'stream-generic') else 'model_not_found'
                    error = {'type': 'invalid_request_error', 'code': code, 'message': 'This resource is unavailable.'}
                    if streamed:
                        response = {'id': 'response-' + turn}
                        frames = [] if case == 'stream-first' else [{'type': 'response.created', 'response': response}]
                        if delivered and continuing:
                            item = {'type': 'message', 'id': 'partial', 'role': 'assistant', 'phase': 'commentary',
                                    'content': [{'type': 'output_text', 'text': partial}]}
                            frames.append({'type': 'response.output_item.done', 'item': item})
                        frames.append({'type': 'response.failed', 'response': {**response, 'status': 'failed', 'error': error}})
                        self.send_body('text/event-stream', fixture.sse(frames))
                        return
                    payload = json.dumps({'error': error}).encode()
                    self.send_response(404)
                    self.send_header('Content-Type', 'application/json')
                    self.send_header('Content-Length', str(len(payload)))
                    self.end_headers()
                    self.wfile.write(payload)
                    return
                if issue_tool:
                    item = {'type': 'function_call', 'name': 'get_goal', 'call_id': 'read-once', 'arguments': '{}'}
                    if shell:
                        command = ("printf '%s\\n' \"$$\" >> shell-start; IFS= read -r reply && "
                                   "printf '%s:%s\\n' \"$$\" \"$reply\" | tee shell-finish")
                        item.update(name='exec_command', arguments=json.dumps({
                            'cmd': command, 'tty': True, 'yield_time_ms': 50}))
                    self.send_body('text/event-stream', fixture.response('tool-' + turn, item))
                    return
                if shell and continuing and not poll_outputs:
                    # Use the handle the live process actually returned, including its invocation binding.
                    handle = live_shell_handle(outputs[0])
                    if handle:
                        poll = {**handle, 'chars': 'still-running\n', 'yield_time_ms': 1000}
                        item = {'type': 'function_call', 'name': 'write_stdin', 'call_id': 'write-once',
                                'arguments': json.dumps(poll)}
                        self.send_body('text/event-stream', fixture.response('poll-' + turn, item))
                        return
                answer = model_resume.REPLY if seed else model_resume.ANSWER
                item = {'type': 'message', 'role': 'assistant', 'id': 'answer-' + turn,
                        'content': [{'type': 'output_text', 'text': answer}]}
                self.send_body('text/event-stream', fixture.response('response-' + turn, item))

        server, worker = fixture.start_listener(Handler)
        self.addCleanup(worker.join)
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        self.addCleanup(fixture.write_json, root / 'events.json', events)
        model_resume.configure(root, server.server_port, 'ultra', model_preference=old)
        if case == 'after-tool':
            config = root / 'codex-home/config.toml'
            config.write_text('features.goals = true\n' + config.read_text())
        if shell:
            config = root / 'codex-home/config.toml'
            config.write_text(config.read_text().replace('sandbox_mode = "read-only"', 'sandbox_mode = "workspace-write"'))
        fixture.write_json(root / 'codex-home/auth.json', {'OPENAI_API_KEY': 'sk-loopback'})
        work = str(root / 'work')
        thread = None

        def app(label, interact):
            def connected(process, log):
                rpc = Rpc(process, log)
                rpc.initialize()
                interact(rpc)
            self.assertEqual(fixture.run(binary, ['app-server', '--listen', 'stdio://'], root, env,
                                         label, interact=connected), 0)

        def seed(rpc):
            nonlocal thread
            thread = rpc.request('thread/start', {'cwd': work, 'ephemeral': False})['thread']['id']
            self.assertEqual(rpc.turn(thread, model_resume.SEED)['status'], 'completed')

        def resume(rpc):
            pin = {'model': old} if pinned else {}
            opened = rpc.request('thread/resume', {'threadId': thread, 'cwd': work, **pin})
            self.assertEqual((opened['model'], opened['reasoningEffort']), (old, 'ultra'))
            if case == 'turn-pin':
                active = rpc.request('turn/start', {'threadId': thread, 'model': old, 'input': [
                    {'type': 'text', 'text': model_resume.CONTINUE, 'text_elements': []}]})['turn']['id']
                done = rpc.wait(lambda m: m.get('method') == 'turn/completed' and m['params']['threadId'] == thread
                                and m['params']['turn']['id'] == active)
                turn = done['params']['turn']
            else:
                turn = rpc.turn(thread, model_resume.CONTINUE)
            attempts = [row for row in events if row.get('turn') == turn['id']]
            if shell:
                first_output = next((row['outputs'][0] for row in attempts if row['outputs']), None)
                if first_output is not None and live_shell_handle(first_output) is None:
                    marker = 'present' if (root / 'work/shell-start').is_file() else 'absent'
                    self.fail(f'Initial exec_command returned no live shell handle (shell-start marker {marker}).\n'
                              f'{first_output}')
            if case == 'shell-control':
                expected = [(old, 'xhigh', 200)] * 3
            else:
                expected = [(old, 'xhigh', 200 if streamed else 404)]
                if shell or case == 'after-tool':
                    expected.insert(0, (old, 'xhigh', 200))
                if continues or case == 'second-retirement':
                    expected.append((new, 'high', 200 if continues else 404))
                if shell:
                    expected.append((new, 'high', 200))
            self.assertEqual([(r['model'], r['effort'], r['status']) for r in attempts], expected)
            self.assertEqual(turn['status'], 'completed' if continues else 'failed')
            if delivered:
                self.assertEqual(turn['error']['codexErrorInfo'], 'modelNotFound')
                notices = [json.loads(line) for line in (root / 'resume-stdout.jsonl').read_text().splitlines()]
                self.assertEqual(sum(n.get('method') == 'item/completed' and n['params']['turnId'] == turn['id']
                                     and n['params']['item'].get('text') == partial for n in notices), 1)
            retry = next(((a, b) for a, b in zip(attempts, attempts[1:]) if a['model'] != b['model']), None)
            if retry:
                before_retry = events[events.index(retry[0]) + 1:events.index(retry[1])]
                self.assertTrue(any(e['event'] == 'catalog' and new in e['models'] for e in before_retry),
                                'the serving provider must confirm the successor before retry')
            if case == 'after-tool':
                self.assertEqual([r['issue_tool'] for r in attempts], [True, False, False])
                self.assertEqual(attempts[1]['outputs'], attempts[2]['outputs'])
                output, = attempts[2]['outputs']
                self.assertIsNone(json.loads(output)['goal'])
            if shell:
                self.assertEqual(sum(r['issue_tool'] for r in attempts), 1)
                if retry:
                    self.assertEqual(retry[0]['outputs'], retry[1]['outputs'])
                pid, = (root / 'work/shell-start').read_text().splitlines()
                self.assertRegex(pid, r'^\d+$')
                self.assertEqual((root / 'work/shell-finish').read_text().splitlines(), [pid + ':still-running'])
                output, = attempts[-1]['poll_outputs']
                self.assertIn(pid + ':still-running', output)
                self.assertIn('Process exited with code 0', output)
            for row in attempts:
                self.assertEqual(row['thread'], thread)
                self.assertEqual([p for p in row['users'] if p in (model_resume.SEED, model_resume.CONTINUE)],
                                 [model_resume.SEED, model_resume.CONTINUE])
                self.assertEqual(row['history'], [model_resume.REPLY])
            if continues or delivered:
                following = rpc.turn(thread, model_resume.AFTER_RELEASE)
                self.assertEqual(following['status'], 'completed')
                row, = [r for r in events if r.get('turn') == following['id']]
                selected = (old, 'xhigh') if case == 'shell-control' else (new, 'high')
                self.assertEqual((row['thread'], row['model'], row['effort']), (thread, *selected))
                self.assertEqual(row['history'], [model_resume.REPLY, partial if delivered else model_resume.ANSWER])
            if pinned or case == 'stream-generic':
                following = rpc.turn(thread, model_resume.AFTER_RELEASE)
                self.assertEqual(following['status'], 'failed')
                row, = [r for r in events if r.get('turn') == following['id']]
                self.assertEqual((row['thread'], row['model'], row['effort']), (thread, old, 'xhigh'))

        app('seed', seed)
        app('resume', resume)

    def test_next_turn_retries_a_peer_release_after_catalog_unavailability(self):
        self.check_peer_catalog_recovery('turn')

    def test_idle_thread_retries_a_peer_release_after_catalog_unavailability(self):
        self.check_peer_catalog_recovery('idle')

    def check_peer_catalog_recovery(self, recovery):
        root = results / ('peer-catalog-recovery-' + recovery)
        root.mkdir()
        events = []
        failed = threading.Event()
        restored = threading.Event()
        gateway = model_resume.Gateway(events)

        class Handler(gateway.handler()):
            def do_GET(self):
                with gateway.lock:
                    peer_received_release = any(row.get('successor') == model_resume.NEW for row in events)
                if peer_received_release and not restored.is_set():
                    failed.set()
                    self.send_error(503)
                    return
                super().do_GET()

        server, worker = fixture.start_listener(Handler)
        self.addCleanup(fixture.cleanup_resources, (('gateway', server, worker),), (), {}, None)
        env = fixture.fresh_environment(root)
        model_resume.configure(root, server.server_port, 'ultra')
        (root / 'codex-home/auth.json').write_text(json.dumps({'OPENAI_API_KEY': 'sk-loopback'}))
        args = ['app-server', '--listen', 'stdio://']

        def run(name, test):
            def connected(process, log):
                client = Rpc(process, log)
                client.initialize()
                test(client)
            self.assertEqual(fixture.run(binary, args, root, env, name, interact=connected), 0)

        def seed(client):
            nonlocal thread, first
            opened = client.request('thread/start', {'cwd': work, 'model': model_resume.OLD, 'ephemeral': False})
            thread = opened['thread']['id']
            first = client.turn(thread, model_resume.SEED)
            self.assertEqual(first['status'], 'completed')

        work = str(root / 'work')
        thread = first = None
        run('seed', seed)
        model_resume.configure(root, server.server_port, 'low', model_preference=model_resume.OTHER)

        def observe(client):
            opened = client.request('thread/resume', {'threadId': thread, 'cwd': work})
            self.assertEqual((opened['model'], opened['reasoningEffort']), (model_resume.OLD, 'ultra'))
            self.assertEqual([row['id'] for row in opened['thread']['turns']], [first['id']])
            with gateway.lock:
                gateway.resumed = gateway.released = True

            def publish(peer):
                rows = peer.request('model/list', {})['data']
                self.assertIn(model_resume.NEW, [row['model'] for row in rows])
                if recovery == 'turn':
                    pending = client.turn(thread, model_resume.CONTINUE)
                    self.assertEqual(pending['status'], 'completed')
                    row, = [r for r in events if r.get('turn') == pending['id']]
                    self.assertEqual((row['thread'], row['model']), (thread, model_resume.OLD))
            run('publisher', publish)
            self.assertTrue(failed.wait(15), 'serving process never saw the outage')
            restored.set()
            if recovery == 'idle':
                def successor(message):
                    params = message.get('params', {})
                    return (message.get('method') == 'thread/settings/updated'
                            and params.get('threadId') == thread
                            and params['threadSettings'].get('model') == model_resume.NEW)
                self.assertIsNotNone(client.poll(successor, 55), 'idle thread missed provider recovery')
            turn = client.turn(thread, model_resume.AFTER_RELEASE)
            self.assertEqual(turn['status'], 'completed')
            row, = [r for r in events if r.get('turn') == turn['id']]
            self.assertEqual((row['thread'], row['model'], row['effort']), (thread, model_resume.NEW, 'high'))
            self.assertTrue(row['seed_prompt'] and row['prior_reply'] and row['after_release'])

        run('serving', observe)

    def test_next_turn_follows_the_work_clients_own_shared_catalog(self):
        self.check_originator_peer_release(idle=False)

    def test_idle_session_follows_the_work_clients_own_shared_catalog(self):
        self.check_originator_peer_release(idle=True)

    def check_originator_peer_release(self, *, idle):
        service = 'codex_work_cca'
        old, new = 'gpt-6.1-sol', 'gpt-6.2-sol'
        events = []

        class ScopedGateway(model_resume.Gateway):
            def catalog_for_originator(self, originator):
                with self.lock:
                    released = self.released and originator == service
                    first = model_resume.model(old, new if released else None)
                    first['multi_agent_reasoning_effort'] = 'xhigh'
                    rows = [first, model_resume.model(new)] if released else [first]
                    return (json.dumps({'models': rows}).encode(),
                            model_resume.NEW_ETAG if released else model_resume.OLD_ETAG)

            def response_etag(self):
                return model_resume.NEW_ETAG if self.released else model_resume.OLD_ETAG

        gateway = ScopedGateway(events, live=True)
        listener, worker = fixture.start_listener(gateway.handler())
        self.addCleanup(worker.join)
        self.addCleanup(listener.server_close)
        self.addCleanup(listener.shutdown)
        root = results / ('originator-peer-idle' if idle else 'originator-peer-turn')
        root.mkdir()
        env = fixture.fresh_environment(root)
        model_resume.configure(root, listener.server_port, 'ultra', model_preference=old)
        (root / 'codex-home/auth.json').write_text(json.dumps({'OPENAI_API_KEY': 'sk-loopback'}))
        work = str(root / 'work')
        saved = {}

        def run(label, use):
            def connected(process, log):
                client = Rpc(process, log)
                client.initialize()
                use(client)
            status = fixture.run(binary, ['app-server', '--listen', 'stdio://'], root, env,
                                 label, interact=connected)
            self.assertEqual(status, 0)

        def start(client, scope):
            return client.request('thread/start', {'model': old, 'cwd': work, 'serviceName': scope,
                                                   'ephemeral': False})['thread']['id']

        def seed(client):
            for scope in (service, None):
                thread = start(client, scope)
                self.assertEqual(client.turn(thread, model_resume.SEED)['status'], 'completed')
                saved[scope] = thread

        def cached(filename):
            path = root / 'codex-home' / filename
            return [r['slug'] for r in json.loads(path.read_bytes())['models']]

        def serving(client):
            for thread in saved.values():
                resumed = client.request('thread/resume', {'threadId': thread, 'cwd': work})
                self.assertEqual((resumed['model'], resumed['reasoningEffort']), (old, 'ultra'))
            def publish(peer):
                publisher = start(peer, service)
                thread = saved[service]
                self.assertEqual(client.turn(thread, model_resume.CONTINUE)['status'], 'completed')
                with gateway.lock:
                    gateway.resumed = gateway.released = True
                self.assertEqual(peer.turn(publisher, model_resume.CONTINUE)['status'], 'completed')
                deadline = time.monotonic() + 15
                while new not in cached('models_cache.alab.codex_work_cca.json'):
                    self.assertLess(time.monotonic(), deadline, 'peer never published the Work catalog')
                    time.sleep(0.05)
                self.assertNotIn(new, cached('models_cache.alab.json'))
                if idle:
                    def promoted(message):
                        params = message.get('params', {})
                        return (message.get('method') == 'thread/settings/updated'
                                and params.get('threadId') == thread
                                and params['threadSettings'].get('model') == new)
                    self.assertIsNotNone(client.poll(promoted, 15), 'idle Work session missed the release')
                for scope, expected in ((service, (new, 'high')), (None, (old, 'xhigh'))):
                    tid = saved[scope]
                    completed = client.turn(tid, model_resume.AFTER_RELEASE)
                    self.assertEqual(completed['status'], 'completed')
                    row, = [r for r in events if r.get('turn') == completed['id']]
                    self.assertEqual((row['thread'], row['model'], row['effort']), (tid, *expected))
                    self.assertTrue(row['seed_prompt'] and row['prior_reply'])
            run('peer', publish)

        run('seed', seed)
        run('serving', serving)

    def check_loaded_model_families(self, name, *, peer=False, immediate=False):
        successors = {'gpt-6.1-sol': 'gpt-6.2-sol', 'gpt-6-astra': 'gpt-6.2-astra'}

        class Catalog(model_resume.Gateway):
            def catalog_rows(self):
                rows = []
                for old, new in successors.items():
                    original = model_resume.model(old, new if self.released else None)
                    original['multi_agent_reasoning_effort'] = 'xhigh'
                    rows.append(original)
                    if self.released:
                        rows.append(model_resume.model(new))
                return rows

        root = results / name
        root.mkdir()
        env = fixture.fresh_environment(root)
        events = []
        gateway = Catalog(events)
        server, worker = fixture.start_listener(gateway.handler())
        self.addCleanup(worker.join)
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        (root / 'codex-home/auth.json').write_text(json.dumps({'OPENAI_API_KEY': 'sk-loopback'}))
        args = ['app-server', '--listen', 'stdio://']
        work = str(root / 'work')
        saved = {}
        expected = {}

        def seed(process, log):
            rpc = Rpc(process, log)
            rpc.initialize()
            for old, pinned in [('gpt-6.1-sol', False), ('gpt-6-astra', False), ('gpt-6.1-sol', True)]:
                opened = rpc.request('thread/start', {'cwd': work, 'model': old, 'ephemeral': False})
                thread = opened['thread']['id']
                turn = rpc.turn(thread, model_resume.SEED)
                self.assertEqual(turn['status'], 'completed')
                saved[thread] = old, pinned, turn['id']

        model_resume.configure(root, server.server_port, 'ultra', model_preference='gpt-6.1-sol')
        self.assertEqual(fixture.run(binary, args, root, env, 'seed', interact=seed), 0)
        self.assertEqual(len(saved), 3)
        model_resume.configure(root, server.server_port, 'low', model_preference='gpt-6-astra')

        def observe(process, log):
            rpc = Rpc(process, log)
            rpc.initialize()
            for thread, (old, pinned, seed_turn) in saved.items():
                selection = {'model': old} if pinned else {}
                opened = rpc.request('thread/resume', {'threadId': thread, 'cwd': work, **selection})
                self.assertEqual((opened['model'], opened['reasoningEffort']), (old, 'ultra'))
                self.assertEqual([t['id'] for t in opened['thread']['turns']], [seed_turn])
            self.assertEqual(set(rpc.request('thread/loaded/list', {})['data']), set(saved))
            with gateway.lock:
                gateway.resumed = gateway.released = True
                offset = len(events)
            submitted = {}
            if peer:
                listed = []

                def fetch(other_process, other_log):
                    other = Rpc(other_process, other_log)
                    other.initialize()
                    listed.extend(other.request('model/list', {})['data'])
                    if immediate:
                        # Submit from the original server before the peer even disconnects.
                        thread = next(t for t, (old, pinned, _) in saved.items()
                                      if old == 'gpt-6.1-sol' and not pinned)
                        submitted[thread] = rpc.turn(thread, model_resume.CONTINUE)

                self.assertEqual(fixture.run(binary, args, root, env, 'peer', interact=fetch), 0)
            else:
                listed = rpc.request('model/list', {})['data']
            self.assertTrue(set(successors.values()) <= {row['model'] for row in listed})
            for thread, (old, pinned, _) in saved.items():
                if pinned or immediate:
                    continue
                notice = rpc.wait(lambda m: m.get('method') == 'thread/settings/updated'
                                  and m['params']['threadId'] == thread
                                  and m['params']['threadSettings']['model'] == successors[old])
                self.assertEqual(notice['params']['threadSettings']['effort'], 'ultra')
            with gateway.lock:
                self.assertTrue(any(e['event'] == 'catalog' for e in events[offset:]))
                self.assertEqual(sum(e['event'] == 'request' for e in events[offset:]), len(submitted))
            for thread, (old, pinned, seed_turn) in saved.items():
                turn = submitted.get(thread) or rpc.turn(thread, model_resume.CONTINUE)
                self.assertEqual(turn['status'], 'completed')
                expected[thread] = (seed_turn, turn['id'], old if pinned else successors[old],
                                    'xhigh' if pinned else 'high')
            for message in rpc.pending:
                if message.get('method') == 'thread/settings/updated':
                    update = message['params']
                    old, pinned, _ = saved[update['threadId']]
                    if pinned:
                        self.assertEqual(update['threadSettings']['model'], old)

        self.assertEqual(fixture.run(binary, args, root, env, 'refresh', interact=observe), 0)
        requests = [row for row in events if row['event'] == 'request']
        self.assertEqual(len(requests), 2 * len(saved))
        for thread, (seed_id, next_id, model, effort) in expected.items():
            first, second = [r for r in requests if r['thread'] == thread]
            self.assertEqual((first['turn'], first['model'], first['effort']), (seed_id, saved[thread][0], 'xhigh'))
            self.assertEqual((second['turn'], second['model'], second['effort']), (next_id, model, effort))
            self.assertTrue(second['seed_prompt'] and second['prior_reply'] and second['resume_prompt'])

    def test_fork_and_parent_follow_the_saved_family_on_their_own_threads(self):
        root = results / 'fork-model'
        root.mkdir()
        env = fixture.fresh_environment(root)
        env.update(TERM='xterm-256color', TERM_PROGRAM='xterm')
        events = []
        gateway = model_resume.Gateway(events)
        server, worker = fixture.start_listener(gateway.handler())
        self.addCleanup(fixture.cleanup_resources, (('gateway', server, worker),), (), {}, None)
        work = str(root / 'work')
        model_resume.configure(root, server.server_port, 'ultra')
        (root / 'codex-home/auth.json').write_text(json.dumps({'OPENAI_API_KEY': 'sk-loopback'}))
        args = ['app-server', '--listen', 'stdio://']
        parent = seed_turn = None

        def seed(process, log):
            nonlocal parent, seed_turn
            rpc = Rpc(process, log)
            rpc.initialize()
            start = rpc.request('thread/start', {'cwd': work, 'model': model_resume.OLD, 'ephemeral': False})
            parent = start['thread']['id']
            turn = rpc.turn(parent, model_resume.SEED)
            self.assertEqual(turn['status'], 'completed')
            seed_turn = turn['id']

        status = fixture.run(binary, args, root, env, 'before-release', interact=seed)
        gateway.publish_successor(status, parent)
        model_resume.configure(root, server.server_port, 'low', model_preference=model_resume.OTHER)
        child = None

        def branch(process, log):
            nonlocal child
            rpc = Rpc(process, log)
            rpc.initialize()
            forked = rpc.request('thread/fork', {'threadId': parent, 'cwd': work})
            child = forked['thread']['id']
            self.assertNotEqual(child, parent)
            self.assertEqual(forked['thread']['forkedFromId'], parent)
            self.assertEqual(rpc.turn(child, model_resume.CONTINUE)['status'], 'completed')
            self.assertEqual((forked['model'], forked['reasoningEffort']), (model_resume.NEW, 'ultra'))

        self.assertEqual(fixture.run(binary, args, root, env, 'fork', interact=branch), 0)
        fork_args = ['fork', parent, model_resume.CONTINUE]
        model_resume.workspace_exec(binary, fork_args, root, env, 'exec-fork', model_resume.ANSWER)
        banner = (root / 'exec-fork-stderr.txt').read_text().splitlines()
        self.assertIn('model: ' + model_resume.NEW, banner)
        self.assertIn('reasoning effort: ultra', banner)

        completed = []
        custody, end, _ = model_resume.terminal_session(
            binary, root, env, 'terminal', fork_args, gateway, completed)
        self.assertEqual(custody['exit'], 0)
        self.assertFalse(custody['forced'])
        rendered = (root / 'terminal.raw').read_bytes()[:end]
        selection = model_resume.CHECK['terminal_model_selection']
        self.assertEqual(selection(rendered, False, after_reply=False, first=True), (model_resume.NEW.encode(), b'ultra'))
        self.assertEqual(selection(rendered, False), (model_resume.NEW.encode(), b'ultra'))

        seed_request, server_fork, exec_fork, terminal_fork = [r for r in events if r['event'] == 'request']
        self.assertEqual(seed_request['turn'], seed_turn)
        self.assertEqual(server_fork['thread'], child)
        self.assertEqual(completed, [terminal_fork['turn']])
        forks = [server_fork, exec_fork, terminal_fork]
        self.assertEqual(len({parent, *(row['thread'] for row in forks)}), 4)

        def reopen_parent(process, log):
            rpc = Rpc(process, log)
            rpc.initialize()
            for row in forks:
                fork = rpc.request('thread/read', {'threadId': row['thread'], 'includeTurns': True})['thread']
                self.assertEqual(fork['forkedFromId'], parent)
                self.assertEqual([t['id'] for t in fork['turns']], [seed_turn, row['turn']])
            opened = rpc.request('thread/resume', {'threadId': parent, 'cwd': work})
            self.assertEqual([t['id'] for t in opened['thread']['turns']], [seed_turn])
            self.assertEqual(rpc.turn(parent, model_resume.CONTINUE)['status'], 'completed')

        self.assertEqual(fixture.run(binary, args, root, env, 'parent', interact=reopen_parent), 0)
        requests = [r for r in events if r['event'] == 'request']
        self.assertEqual(len(requests), 5)
        self.assertEqual([requests[0]['thread'], requests[-1]['thread']], [parent, parent])
        self.assertEqual((seed_request['model'], seed_request['effort']), (model_resume.OLD, 'xhigh'))
        for row in requests[1:]:
            self.assertEqual((row['model'], row['effort']), (model_resume.NEW, 'high'))
            self.assertTrue(row['seed_prompt'] and row['prior_reply'] and row['resume_prompt'])

    def test_withdrawal_and_return_of_the_saved_effort_on_the_wire(self):
        report = run_smoke('model_withdrawal.py', results / 'model-withdrawal')
        self.assertEqual(set(report['cases']), {'first_party', 'custom'})
        for case in report['cases'].values():
            self.assertIs(case['passed'], True)
            self.assertTrue(case['thread'])

    def test_catalog_and_client_readiness(self):
        run_smoke('model_readiness.py', results / 'model-readiness')

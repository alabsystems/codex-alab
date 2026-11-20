# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The --goal-reset case of tui_host_rotation.py, driven only through its private real aterm."""

from contextlib import closing
import json
from pathlib import Path
import select
import shlex
import socket
import sqlite3
import threading
import time

import capacity_exec as probe
import provider_fixture
import renewal_follow as fixture
from quota_resume import Rpc
from rotation_card_restart import live_birth


OBJECTIVE = 'Verify that the original native goal resumes after its weekly allowance returns.'
ANSWER = 'The original native goal resumed after its weekly allowance returned.'


class GoalProvider:
    def __init__(self, retry_signal):
        self.lock = threading.Lock()
        self.retry_signal = retry_signal
        self.calls = []
        self.release_wall = threading.Event()
        self.release_answer = threading.Event()
        self.reset = None

    def reply(self, body):
        # A background title request cannot count as foreground goal work.
        if fixture.request_kind(body) == 'thread-title':
            return self.message('title', '{"title":"Weekly allowance"}', {})
        metadata = body.get('client_metadata', {}).get('x-codex-turn-metadata', '{}')
        meta = json.loads(metadata)
        record = {'thread': meta.get('thread_id'), 'turn': meta.get('turn_id'), 'at': time.time(),
                  'objective': OBJECTIVE in json.dumps(body.get('input'))}
        with self.lock:
            self.calls.append(record)
            number = len(self.calls)
        if number == 1:
            # Let the observer capture the initial durable ID before the provider refuses it.
            if not self.release_wall.wait(45):
                return 500, b'goal observation timeout', 'text/plain', None, record
            now = int(time.time())
            weekly = provider_fixture.usage('fixture-account', 'fixture-user', week_used=100,
                                            allowed=False, week_resets_in=4, now=now)
            wall = provider_fixture.refusal(weekly, 'Weekly allowance exhausted')
            self.reset = wall['body']['error']['resets_at']
            record['responseHeaders'] = wall['headers']
            return wall['status'], json.dumps(wall['body']).encode(), 'application/json', None, record
        if number == 2:
            # The observer reads the reactivated goal before this successful answer goes out.
            self.retry_signal.sendall(b'R')
            if not self.release_answer.wait(45):
                return 500, b'resumed goal observation timeout', 'text/plain', None, record
            return self.message('resumed', ANSWER, record)
        # The goal is still active and can start another turn after our successful answer.
        # Complete this request with an ordinary terminal failure while the host tears down.
        return 400, b'{"error":{"message":"fixture finished"}}', 'application/json', None, record

    @staticmethod
    def message(response_id, text, record):
        item = {'type': 'message', 'role': 'assistant', 'id': response_id + '-message',
                'content': [{'type': 'output_text', 'text': text}]}
        return 200, probe.response(response_id, item), 'text/event-stream', None, record

    def observed(self):
        with self.lock:
            return [dict(record) for record in self.calls]


def saved_goal(home, thread):
    files = list(home.glob('goals_*.sqlite'))
    probe.require(len(files) == 1, 'expected the private TUI to have one goals database')
    with closing(sqlite3.connect(files[0].as_uri() + '?mode=ro', uri=True)) as database:
        return database.execute('SELECT goal_id, objective, status FROM thread_goals WHERE thread_id=?',
                                (thread,)).fetchone()


def history(home, thread):
    paths = list((home / 'sessions').rglob(f'rollout-*-{thread}.jsonl'))
    if not paths:
        return []
    probe.require(len(paths) == 1, 'more than one rollout for the original thread')
    rows = [json.loads(line) for line in paths[0].read_bytes().split(b'\n')[:-1] if line]
    probe.require(all(row['payload']['id'] == thread for row in rows if row['type'] == 'session_meta'),
                  'rollout belongs to another thread')
    return [row['payload'] for row in rows if row['type'] == 'event_msg']


def watch_original_process(watcher, process):
    """Bind the watch to the supervisor's recorded Darwin process before releasing the wall."""
    pid = process['pid']
    event = select.kevent(pid, filter=select.KQ_FILTER_PROC, flags=select.KQ_EV_ADD | select.KQ_EV_ONESHOT,
                          fflags=select.KQ_NOTE_EXIT | select.KQ_NOTE_EXEC)
    watcher.control([event], 0, 0)
    probe.require(live_birth(pid) == process['kernelBirth'], 'the verified Codex process changed before its watch')


def require_original_process(watcher, wait=0):
    probe.require(not watcher.control(None, 1, wait), 'the original Codex process exited or re-executed')


def wait_for_automatic_retry(watcher, proc, retry_signal, timeout=180):
    # The provider records the request before signaling. Its socket and the existing
    # process watch stay readable if either event arrives before registration.
    interests = [select.kevent(descriptor, filter=select.KQ_FILTER_READ)
                 for descriptor in (retry_signal.fileno(), watcher.fileno())]
    interests.append(select.kevent(proc.pid, filter=select.KQ_FILTER_PROC, fflags=select.KQ_NOTE_EXIT))
    with closing(select.kqueue()) as wait:
        try:
            events = wait.control(interests, len(interests), timeout)
        except ProcessLookupError:
            probe.require(proc.poll() is None, 'the private aterm host exited')
            raise
    probe.require(proc.poll() is None and all(event.filter != select.KQ_FILTER_PROC for event in events),
                  'the private aterm host exited')
    require_original_process(watcher)
    probe.require(any(event.filter == select.KQ_FILTER_READ and event.ident == retry_signal.fileno()
                      for event in events) and retry_signal.recv(1) == b'R',
                  'timed out waiting for automatic sampling after the weekly reset')


def seed_idle_goal(root, home, binary, env):
    """Set an Active goal on an unloaded thread through the same binary's public RPC."""
    thread = None

    def prepare(process, output):
        nonlocal thread
        rpc = Rpc(process, output)
        rpc.initialize()
        thread = rpc.request('thread/start', {'cwd': str(root / 'work')})['thread']['id']
        params = {'threadId': thread, 'origin': 'user'}
        paused = rpc.request('thread/goal/set', {**params, 'objective': OBJECTIVE, 'status': 'paused'})
        probe.require(paused['goal']['status'] == 'paused', 'failed to create the original paused goal')
        probe.require(rpc.request('thread/unsubscribe', {'threadId': thread})['status'] == 'unsubscribed',
                      'could not unload the original goal thread')
        rpc.wait(lambda msg: msg.get('method') == 'thread/closed' and msg['params']['threadId'] == thread)
        active = rpc.request('thread/goal/set', {**params, 'status': 'active'})['goal']
        probe.require((active['threadId'], active['objective'], active['status']) == (thread, OBJECTIVE, 'active')
                      and thread not in rpc.request('thread/loaded/list', {})['data'],
                      'the saved Active goal must stay unloaded until the TUI resumes it')

    probe.require(probe.run(binary, ['app-server', '--listen', 'stdio://'], root,
                            {**env, 'CODEX_HOME': str(home)}, 'seed', interact=prepare) == 0,
                  'the app-server seeding process did not exit cleanly')
    return thread


def run_goal_reset(root, home, binary, ctl, proc, env, report):
    reader, writer = socket.socketpair()
    with reader, writer:
        observe_goal_reset(root, home, binary, ctl, proc, env, report, reader, GoalProvider(writer))


def observe_goal_reset(root, home, binary, ctl, proc, env, report, retry_signal, model):
    watcher = None

    def wait_for(label, test, timeout=45):
        deadline = time.monotonic() + timeout
        while True:
            probe.require(proc.poll() is None, 'the private aterm host exited')
            if value := test():
                return value
            probe.require(time.monotonic() < deadline, f'timed out waiting for {label}')
            if watcher is None:
                time.sleep(.15)
            else:
                require_original_process(watcher, .15)

    def claimed_thread():
        status = fixture.reply_fields(ctl('status'))
        for record in fixture.records(home):
            if (record.get('sessionId') == thread and status.get('program') == 'codex'
                    and status.get('supervisor_proven') == '1'
                    and status.get('supervisor') == f"codex-alab@{record['pid']}"):
                probe.require(Path(record['exe']).samefile(binary), 'the host claimed a different Codex executable')
                return record

    server, worker = probe.serve(model)
    try:
        probe.configure(root, server.server_port)
        home.mkdir(exist_ok=True)
        project = json.dumps(str(root / 'work'))
        (home / 'config.toml').write_text('features.goals=true\nfeatures.daemon_auto_start=false\n'
            'cli_auth_credentials_store="file"\nmcp_oauth_credentials_store="file"\nfeedback.enabled=false\n'
            'tui.screen_reader_detection_done=true\ntui.disable_paste_burst=true\n'
            'analytics.enabled=false\nthread_unload_delay_secs=0\ncheck_for_update_on_startup=false\n'
            + (root / 'codex-home/config.toml').read_text() + f'\n[projects.{project}]\ntrust_level="trusted"\n')
        network_env = {'HTTP_PROXY': 'http://127.0.0.1:1', 'HTTPS_PROXY': 'http://127.0.0.1:1',
                       'ALL_PROXY': 'http://127.0.0.1:1', 'NO_PROXY': '127.0.0.1,localhost,::1'}
        thread = seed_idle_goal(root, home, binary, {**env, **network_env})
        report['thread'] = thread
        original = saved_goal(home, thread)
        probe.require(original and original[1:] == (OBJECTIVE, 'active'), 'missing original Active goal')
        probe.require(not model.observed(), 'seeding the idle goal unexpectedly started a model turn')
        report.update(goal_id=original[0], objective=OBJECTIVE)
        ctl('meta', 'set', 'autopilot', 'cruise')
        # Only the shell receives input. Core must start both goal turns inside the resumed TUI.
        launch = ['env', *(f'{key}={value}' for key, value in network_env.items()),
                  str(binary), '--no-alt-screen', 'resume', thread]
        ctl('send', f"cd {shlex.quote(str(root / 'work'))} && {shlex.join(launch)}" + '\\n')
        claim = wait_for('a proven supervisor claim on the resumed packaged TUI', claimed_thread)
        source = fixture.process_evidence(claim)
        watcher = select.kqueue()
        watch_original_process(watcher, source)
        report.update(tui_pid=source['pid'], tui_birth=source['kernelBirth'])
        wait_for('the first native goal request', model.observed)
        model.release_wall.set()
        wait_for('the original goal to become usage limited',
                 lambda: saved_goal(home, thread) == (original[0], OBJECTIVE, 'usage_limited'))
        def observed_wall():
            events = history(home, thread)
            first_call = model.observed()[0]
            turn = first_call['turn']
            probe.require(turn and first_call['thread'] == thread and first_call['objective'],
                          'first provider request lacks its original thread, turn or objective')
            if not any(e['type'] in ('task_complete', 'turn_complete') and e['turn_id'] == turn
                       and (e.get('error') or {}).get('codex_error_info') == 'usage_limit_exceeded' for e in events):
                return False
            starts = [e for e in events if e['type'] in ('task_started', 'turn_started')]
            probe.require(len(starts) == 1 and starts[0]['turn_id'] == turn
                          and (starts[0].get('turn_attribution') or {}).get('turn_trigger') == 'goal',
                          'the walled turn must be the first native goal turn')
            return turn

        first = wait_for('a terminal weekly-limit error on the first native goal turn', observed_wall)
        report.update(walled_turn=first, goal_at_wall='usage_limited')
        # The production scheduler includes 60 s grace and up to 60 s of per-session jitter.
        wait_for_automatic_retry(watcher, proc, retry_signal)
        calls = model.observed()
        probe.require(len(calls) == 2 and calls[1]['thread'] == thread and calls[1]['objective'],
                      'automatic request changed its thread or lost the original objective')
        second = calls[1]['turn']
        probe.require(second and first != second, 'weekly recovery must start a distinct turn')
        probe.require(calls[1]['at'] >= model.reset, 'the goal retried before its advertised weekly reset')
        probe.require(saved_goal(home, thread) == original, 'wake did not reactivate the original goal ID')
        probe.require((claimed_thread() or {}).get('pid') == source['pid'],
                      'a different supervisor claimed the resumed goal turn')
        require_original_process(watcher)
        model.release_answer.set()

        def observed_completion():
            events = history(home, thread)
            starts = [e for e in events if e['type'] in ('task_started', 'turn_started')]
            terminals = [e for e in events if e['type'] in ('task_complete', 'turn_complete')]
            if not any(e['turn_id'] == second and e.get('last_agent_message') == ANSWER
                       and e.get('error') is None for e in terminals):
                return False
            probe.require([e['turn_id'] for e in starts[:2]] == [first, second] and all(
                (e.get('turn_attribution') or {}).get('turn_trigger') == 'goal' for e in starts[:2]),
                'the initial and resumed work must both be native goal turns')
            return True

        wait_for('the resumed native goal turn to finish', observed_completion)
        require_original_process(watcher)
        report.update(resumed_turn=second, resumed_unix=calls[1]['at'], resumed_answer=ANSWER)
    except (Exception, KeyboardInterrupt) as error:
        if 'goal_id' in report:
            try:
                goal = saved_goal(home, thread)
                report['goal_at_failure'] = {'goal_id': goal[0], 'status': goal[2]} if goal else None
            except (OSError, RuntimeError, sqlite3.Error) as read_error:
                error.add_note(f'could not read the saved goal at failure: {read_error}')
        raise
    finally:
        if model.reset is not None:
            report['reset_unix'] = model.reset
        if watcher is not None:
            watcher.close()
        model.release_wall.set()
        model.release_answer.set()
        report['observation_finished_unix'] = time.time()
        report['sampling_requests'] = [{k: call[k] for k in ('thread', 'turn', 'at', 'objective')}
                                       for call in model.observed()]
        server.shutdown()
        server.server_close()
        worker.join(timeout=5)

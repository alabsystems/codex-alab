#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise quota rotation or native goal recovery through a real debug aterm host and packaged Codex.

Requires Python 3.11+ on macOS. Uses a private aterm-lab instance, its integrated shell and
a loopback provider. Rotation uses two fixture logins. --goal-reset uses a saved active goal
and a TUI resumed without input. Only --account-card needs a visible window.
Supply trusted local binaries and a new result file. The host must report a debug build.
"""

import argparse
from contextlib import ExitStack, closing, contextmanager
import hashlib
import json
import os
from pathlib import Path
import re
import select
import shlex
import signal
import subprocess
import sys
import tempfile
import threading
import time
import tomllib

import capacity_exec as probe
import device_login_fixture as device
import renewal_follow as fixture
from rotation_account_card import CardCheck, focused_session
from rotation_card_restart import verify_claim_loss


EXPECTED_CALLS = [('source', 200), ('source', 429), ('target', 200)]
CANCELLATION_REASONS = {'owner_stop', 'new_person_task', 'changed_record', 'changed_selection',
                        'policy_revoked', 'successor_never_attached'}


class QuotaModel(fixture.Model):
    def __init__(self, root, limit='codex'):
        super().__init__(root, weekly_quota=True)
        self.limit = limit
        self.snapshot_ready = threading.Event()

    def reply(self, body, generation=None):
        # The live host must observe the original agent before it hits the wall.
        # Its normal watcher is asynchronous, even with an instantaneous provider.
        foreground = fixture.request_kind(body) == 'foreground'
        if foreground and not self.snapshot_ready.wait(45):
            raise fixture.provider_fixture.FixtureFault('the host never kept the original conversation')
        refused = generation == 'old' and self.source_walled and (foreground or self.limit != 'codex')
        result = super().reply(body, generation)
        if refused:
            return self.refused(result[-1], generation, 'Fixture quota exhausted')
        return result


def run(args, env):
    return subprocess.run(args, env=env, stdin=subprocess.DEVNULL, capture_output=True,
                          text=True, timeout=20, check=True).stdout.strip()


@contextmanager
def listener(model, secure, *, handoff_usage_path=None, issued_bearer_matches=None):
    server, thread = fixture.serve(model, secure=secure, handoff_usage_path=handoff_usage_path,
                                   issued_bearer_matches=issued_bearer_matches)
    try:
        yield server.server_port
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


@contextmanager
def window(lab, host, world, env, *, visible=False, goal_reset=False):
    # A lab instance starts its own private HOME, XDG dirs and control socket. Only
    # the debug build above may run with account rotation enabled on the live LAN.
    # The goal-reset case measures one running TUI. A scheduled package upgrade
    # would replace the process that owns the timer before we can observe its wake.
    if goal_reset:
        policy = ('accounts="off"', 'upgrade=false', 'probe_api=false', 'continue=false', 'limit_wait=true')
    else:
        policy = ('accounts="early"', 'relaunch=true')
    with (world.parent / 'lab.log').open('x+b') as log:
        proc = subprocess.Popen([lab, 'instance', *(['--window', '--functional-memory-mib', '2048'] if visible else []),
            '--aterm', host, '--world', world, '--harness',
            '--toml', 'shell="/bin/zsh"', '--toml-after', '[keeper]', '--toml-after', 'enabled=false',
            '--harness-key', 'headless=true', '--harness-key', 'human_grace_s=0',
            *(arg for key in policy for arg in ('--harness-key', key)),
            '--harness-key', 'resume_limits=true'], env=env,
            stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        primary = None
        try:
            sock = world / 'run/a.sock'
            until = time.monotonic() + 45
            while not sock.exists():
                if (status := proc.poll()) is not None:
                    offset = max(0, os.fstat(log.fileno()).st_size - 2048)
                    lines = os.pread(log.fileno(), 2048, offset).decode(errors='replace').splitlines()
                    detail = next((line for line in reversed(lines) if line.startswith('aterm-lab:')), '')
                    message = f'aterm-lab exited {status} before opening its private control socket'
                    raise RuntimeError(f'{message}: {detail}' if detail else message)
                probe.require(time.monotonic() < until, 'timed out waiting for the private aterm control socket')
                time.sleep(.1)
            yield sock, proc
        except BaseException as error:
            primary = error
            raise
        finally:
            # lab instance owns the window, shell and foreground and reaps them on TERM.
            failures = []
            try:
                if proc.poll() is None:
                    proc.terminate()
                    try:
                        proc.wait(timeout=45)
                    except subprocess.TimeoutExpired:
                        failures.append(('shutdown', RuntimeError('aterm-lab did not shut down cleanly')))
                        os.killpg(proc.pid, signal.SIGKILL)
                        proc.wait(timeout=10)
            except BaseException as error:
                failures.append(('retire private host', error))
            fixture._host_cleanup_failures(primary, failures)


def fixture_slots(home, *, shared_mask=False):
    slots = {}
    source_email = 'twin@example.invalid' if shared_mask else 'fixture@example.invalid'
    for label, generation, email in [('main', 'old', source_email),
                                     ('target', 'new', 'target@example.invalid')]:
        _, account, user = fixture.QUOTA_ACCOUNTS[generation]
        slots[label] = device.Slot(generation, account, user, email, home / label / 'auth.json')
    return slots


def configure(root, world, port, issuer, usage_url, slots, *, device_login):
    codex_home = world / 'home/.codex'
    codex_home.mkdir(exist_ok=True)
    config = fixture.configure_provider(root, port, usage_url).read_text()
    project = json.dumps(str(root / 'work'))
    (codex_home / 'config.toml').write_text(config + f'\n[projects.{project}]\ntrust_level="trusted"\n')
    rows = []
    for label, slot in slots.items():
        directory = slot.auth_path.parent
        if device_login:
            directory.mkdir()
        else:
            fixture.fake_auth(slot.auth_path, slot.generation, issuer, slot.account, email=slot.email, user=slot.user)
        (directory / 'config.toml').write_text(f'chatgpt_base_url = "{usage_url}"\n')
        rows.append(f'[[account]]\nlabel="{label}"\nclass="dedicated"\nemail="{slot.email}"\n'
                    f'dir={json.dumps(str(directory))}\n')
    (world / 'cfg/aterm/accounts-codex.toml').write_text('\n'.join(rows))
    return codex_home


def receipts(pool):
    return [json.loads(path.read_text()) for path in sorted((pool / 'renewal-follow').glob('*.json'))
            if re.fullmatch('[a-f0-9]{32}', path.stem)]


def flushed_rollout(path):
    # A live rollout writer may have flushed only a prefix of its last JSONL record.
    return [json.loads(line) for line in path.read_bytes().split(b'\n')[:-1] if line]


def completed_rollout(codex_home, thread, turn):
    rows = [row for path in sorted((codex_home / 'sessions').rglob(f'*{thread}*.jsonl'))
            for row in flushed_rollout(path)]
    for event in rows:
        part = event.get('payload', {})
        if (event.get('type') == 'event_msg' and part.get('turn_id') == turn and part.get('error') is None
                and part.get('type') in ('task_complete', 'turn_complete')):
            return rows
    return None


def record_failed_handoff(pool, codex_home, roots, requests, report):
    """Keep bounded evidence before the disposable host and its rollouts are removed."""
    for root, summary in zip(roots, report.get('receipts', []), strict=True):
        if root.get('phase') != 'retired' or not re.fullmatch('[a-f0-9]{32}', root['id']):
            continue
        summary['retirement_reason'] = None
        try:
            cancelled = json.loads((pool / 'renewal-follow' / f"{root['id']}.revoked.json").read_bytes())
            if (cancelled['root'] == root['id'] and cancelled['intent'] == root['intent']
                    and cancelled['reason'] in CANCELLATION_REASONS):
                summary['retirement_reason'] = cancelled['reason']
        except (OSError, ValueError, KeyError, TypeError):
            pass

    history = report['interrupted_turn_history'] = {'status': 'source_refusal_unverified'}
    if report.get('model_calls', [])[:2] != EXPECTED_CALLS[:2] or len(requests) < 2:
        return
    identity = requests[1].get('requestIdentity', {})
    if requests[0].get('requestIdentity') != identity:
        return
    thread, turn = identity.get('thread_id'), identity.get('turn_id')
    if not all(isinstance(i, str) and re.fullmatch(r'[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}', i)
               for i in (thread, turn)):
        return
    history['status'] = 'no_matching_receipt'
    matching = [row['id'] for row in roots if re.fullmatch('[a-f0-9]{32}', row['id'])
                and row['intent'].get('purpose') == 'weekly_quota' and row['intent'].get('thread') == thread
                and row['intent'].get('wall') == turn]
    if not matching:
        return
    history.update(thread=thread, turn=turn, receipt_ids=matching, status='rollout_unverified')
    paths = list((codex_home / 'sessions').rglob(f'rollout-*-{thread}.jsonl'))
    if len(paths) != 1 or paths[0].is_symlink():
        return
    rollout = flushed_rollout(paths[0])
    if not rollout or rollout[0]['type'] != 'session_meta' or any(
            row['payload'].get('id') != thread for row in rollout if row['type'] == 'session_meta'):
        return
    starts = 0
    terminals = []
    for row in rollout:
        if row['type'] != 'event_msg':
            continue
        event = row['payload']
        kind = event.get('type')
        if kind not in ('task_started', 'turn_started', 'task_complete', 'turn_complete', 'turn_aborted'):
            continue
        if event.get('turn_id') != turn:
            return
        if kind in ('task_started', 'turn_started'):
            starts += 1
        elif kind == 'turn_aborted':
            terminals.append('aborted')
        elif event.get('error') is None:
            terminals.append('completed')
        elif isinstance(event['error'], dict) and event['error'].get('codex_error_info') == 'usage_limit_exceeded':
            terminals.append('usage_limit_exceeded')
        else:
            terminals.append('failed')
    if not starts:
        return
    history.update(status='read', starts=starts, terminal_events=terminals,
                   original_input_verified=False, completed_plan_verified=False)
    try:
        fixture.provider_fixture.check_unsteered_input(rollout, requests[:2])
        history['original_input_verified'] = True
    except fixture.provider_fixture.FixtureFault:
        pass
    try:
        fixture.validate_durable_plan(rollout, turn, expected_starts=starts)
        history['completed_plan_verified'] = True
    except RuntimeError:
        pass


def record_native_usage(events, limit, report):
    reads = [e for e in events if e.get('method') == 'GET' and e.get('client') == 'host'
             and e.get('path', '').endswith('/usage') and 'weekUsed' in e and 'allowed' in e]
    report['native_usage'] = [(e['account'], e['weekUsed'], e['allowed']) for e in reads]
    wall, before, during = fixture.native_usage_between_requests(events)
    report['native_usage_interval_us'] = {'source_429_written': wall, 'target_request': before}
    report['latest_native_usage_matched'] = fixture.latest_handoff_usage_matches(during, limit)
    report['native_handoff_usage'] = [
        {key: e[key] for key in ('account', 'routingAccountMatched', 'usageEndpointMatched', 'monotonicUs', 'responseWrittenUs',
                                 'weekUsed', 'allowed')}
        for e in during]
    if 'device_logins' in report:
        for observed, event in zip(report['native_handoff_usage'], during):
            observed['issuedBearerMatched'] = event.get('issuedBearerMatched')
    if limit != 'codex':
        report['native_named_usage'] = fixture.named_usage_readings(during, limit)


def verify(model, codex_home, receipt, rollout, source, exited, report, *, auth_paths=None):
    requests = model.requests
    order = [(event.get('account'), event.get('status')) for event in model.http_events
             if event.get('kind') == 'foreground']
    probe.require(order == EXPECTED_CALLS,
                  f'unexpected provider history {order}')
    probe.require(report['native_usage_interval_us']['target_request'] is not None,
                  'missing time of the target model request')
    if model.limit == 'codex':
        probe.require(report['latest_native_usage_matched'],
                      'latest native usage does not show an exhausted source and eligible target')
    else:
        probe.require(report['refusal_limit'] == model.limit, 'the provider refusal named another limit')
        probe.require(report['latest_native_usage_matched'],
                      'latest native usage does not show a spent source and eligible target for the named limit')
        probe.require(all(allowed and used < 100 for _, used, allowed in report['native_usage']),
                      'ordinary quota was unavailable during a named-limit move')
    report['native_usage_verified'] = True
    routing = [event.get('routingAccountMatched') for event in model.http_events if event.get('kind') == 'foreground']
    probe.require(routing == [True, True, True], 'the model request routed to a different account than its bearer')
    report['model_routing_headers_verified'] = True
    if 'device_logins' in report:
        foreground = [e.get('issuedBearerMatched') for e in model.http_events if e.get('kind') == 'foreground']
        reads = report['native_handoff_usage']
        probe.require(foreground == [True, True, True] and reads and
                      all(e.get('issuedBearerMatched') is True for e in reads) and
                      {e['account'] for e in reads} == {'source', 'target'} and
                      not any(e.get('issuedBearerMatched') is False for e in model.http_events),
                      'the provider did not verify the issued login on every model and native quota request')
        report['device_bearers_verified'] = True
    probe.require(len(requests) == 3 and requests[-1]['newTokenMatched'], 'missing target request')
    probe.require(all(req['body'].get('model') == probe.MODEL for req in requests),
                  'a foreground request changed the configured model')
    thread, turn = (requests[0]['requestIdentity'][key] for key in ('thread_id', 'turn_id'))
    probe.require(all(req['requestIdentity'] == {'thread_id': thread, 'turn_id': turn} for req in requests),
                  'the replacement changed the original thread or turn')
    probe.require(requests[-1]['body']['input'] == requests[1]['body']['input'],
                  'the replacement changed the refused request input')
    fixture.validate_completed_original_turn(rollout, turn)
    fixture.provider_fixture.check_unsteered_input(rollout, requests)
    old, new = receipt['old_pid'], receipt['successor_pid']
    intent = receipt['intent']
    expected_limit = None if model.limit == 'codex' else model.limit
    probe.require(intent.get('limit') == expected_limit, 'the native receipt bound a different limit')
    report['receipt_limit'] = intent.get('limit')
    probe.require(new and new != old and receipt['phase'] == 'consumed' and intent.get('purpose') == 'weekly_quota'
                  and receipt['accepted_turn'] == intent.get('wall') == turn and intent['thread'] == thread,
                  'the replacement did not consume the weekly quota receipt for the original turn')
    # A correct provider request can coexist with a misbound receipt on disk.
    if auth_paths is None:
        auth_paths = {key: codex_home.parent / label / 'auth.json' for key, label in [('old', 'main'), ('new', 'target')]}
    for generation, identity, auth_path in (
        ('old', intent.get('identity'), receipt.get('old_auth')),
        ('new', receipt.get('destination'), receipt.get('new_auth')),
    ):
        expected = fixture.fixture_renewal_identity(generation)
        probe.require(identity == expected and auth_path == str(auth_paths[generation]),
                      f'the receipt does not bind the {generation} login to its fixture account and user')
    successor = next((record for record in fixture.records(codex_home) if record['pid'] == new), {})
    probe.require(source['pid'] == old and source['kernelBirth'] == receipt['old_birth'] and exited,
                  'the native receipt did not replace the observed original process')
    probe.require(successor.get('sessionId') == thread, 'missing replacement on its conversation')
    replacement = fixture.process_evidence(successor)
    probe.require(replacement['kernelBirth'] == receipt['successor_birth']
                  and replacement['image']['sha256'] == source['image']['sha256'] == report['codex_sha256'],
                  'the replacement process differs from the receipt or the supplied Codex package')
    probe.require(report['target_proof_count'] == 1, f"target was proven {report['target_proof_count']} times")
    report.update(thread=thread, turn=turn, source_pid=old, successor_pid=new, receipt_slot_bindings_verified=True,
                  original_input_and_completed_plan_preserved=True)


def weekly_footer_email(frame):
    """Read only the status row immediately below the live, empty composer."""
    if not isinstance(frame, dict) or frame.get('frame') is not None:
        return None
    rows, cursor, dims = frame.get('rows'), frame.get('cursor'), frame.get('dims')
    first = frame.get('first', 0)
    if (not isinstance(rows, list) or not all(isinstance(row, str) for row in rows)
            or not isinstance(cursor, dict) or not isinstance(dims, dict)
            or type(dims.get('rows')) is not int or type(dims.get('cols')) is not int or dims['cols'] <= 2
            or type(first) is not int or first < 0 or first + len(rows) != dims['rows']
            or cursor.get('visible') is not True or type(cursor.get('row')) is not int
            or type(cursor.get('col')) is not int or cursor['col'] != 2):
        return None
    inputs = frame.get('inputs')
    if 'inputs' in frame and not (isinstance(inputs, list) and len(inputs) == 2
                                  and all(type(count) is int and count >= 0 for count in inputs)
                                  and inputs[0] == inputs[1]):
        return None
    # The inline composer can leave unused grid rows below its hints. Locate it
    # from the actual cursor, and require an untrimmed view through the grid bottom.
    composer_row = cursor['row'] - first
    if not 1 <= composer_row <= len(rows) - 4 or any(row.strip() for row in rows[composer_row + 4:]):
        return None
    banner, composer, blank, account, hints = rows[composer_row - 1:composer_row + 4]
    if (not re.match(r'^ +codex-alab \S+ · based on OpenAI Codex \S+ ─', banner)
            or not composer.startswith(('» ', '› ')) or blank.strip()
            or not re.fullmatch(r'  (?:← for agents · )?\? for shortcuts(?: +⚠ \d+ warnings? · f2 to view)? *', hints)):
        return None
    reading = re.fullmatch(r'  ([^\s/@]+@[^\s/@]+) / [^/]+ / weekly \[[█░]{8}\] (?:100|[1-9]?[0-9])% used(?: .*)?',
                           account)
    return reading[1] if reading and account.count('@') == 1 else None


def observe_weekly_footer(ctl, expected_email, phase, pid):
    # A verified successor may finish before its terminal finishes drawing.
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        try:
            frame = json.loads(ctl('text', '--json'))
        except json.JSONDecodeError:
            frame = None
        email = weekly_footer_email(frame)
        if email == expected_email:
            status = fixture.reply_fields(ctl('status'))
            generation = frame.get('gen')
            if (isinstance(generation, str) and re.fullmatch(r'[0-9]+\.[0-9]+', generation)
                    and status.get('gen') == generation and status.get('program') == 'codex'
                    and status.get('supervisor_proven') == '1' and status.get('supervisor') == f'codex-alab@{pid}'):
                cursor = frame['cursor']['row']
                return {'email': email, 'pid': pid, 'frame': generation, 'cursor_row': cursor,
                        'grid_rows': frame['dims']['rows'], 'blank_rows_below': frame['dims']['rows'] - cursor - 4}
        time.sleep(.1)
    raise RuntimeError(f'could not verify the enrolled account in the {phase} Codex footer')


def run_account_turn(model, ctl, report, *, state, codex_home, binary, issuer, host_running, auth_paths, target_label,
                     card=None, footer_emails=None):
    """Submit one prompt in a fixture host and verify its completion on the enrolled destination."""
    root = model.root
    pool = state / 'harness/claude-harness/accounts-codex'
    command = (f'cd {shlex.quote(str(root / "work"))} && '
               f'export CODEX_CA_CERTIFICATE={shlex.quote(str(root / "fixture-ca.pem"))} && '
               f'CODEX_AUTH_PATH={shlex.quote(str(auth_paths["old"]))} ' + shlex.join([
                   str(binary), '-c', f'chatgpt_base_url="{issuer}"', '--no-alt-screen',
                   '--cd', str(root / 'work'), probe.PROMPT]))
    source = None
    roots = []
    source_pane_closed = False
    with closing(select.kqueue()) as exit_watch:
        ctl('send', command + '\\n')
        deadline = time.monotonic() + 450
        while time.monotonic() < deadline:
            probe.require(host_running(), 'aterm exited while the turn was running')
            if not model.snapshot_ready.is_set():
                log = (state / 'logs/aterm.log').read_text()
                for record in fixture.records(codex_home):
                    if record.get('sessionId') and f"kept names pid {record['pid']} and its conversation" in log:
                        source = fixture.process_evidence(record)
                        # The kernel watches this lifetime even if the successor later prunes its old record.
                        event = select.kevent(source['pid'], filter=select.KQ_FILTER_PROC,
                                              flags=select.KQ_EV_ADD | select.KQ_EV_ONESHOT, fflags=select.KQ_NOTE_EXIT)
                        exit_watch.control([event], 0, 0)
                        if footer_emails is not None:
                            report['visible_weekly_footers'] = {'source': observe_weekly_footer(
                                ctl, footer_emails['old'], 'source', source['pid'])}
                        if card is not None:
                            card.open()
                            report['source_address_card_opened'] = True
                        model.snapshot_ready.set()
                        break
            fixture.require_no_fixture_fault(model)
            if model.source_walled and not (root / 'renewed').exists():
                (root / 'renewed').touch()
            roots = receipts(pool)
            with model.lock:
                events = [e.copy() for e in model.http_events]
            record_native_usage(events, model.limit, report)
            report['model_calls'] = [(e.get('account'), e.get('status')) for e in events
                                     if e.get('kind') == 'foreground' and 'status' in e]
            report['foreground_models'] = [req['body'].get('model') for req in model.requests]
            if model.limit != 'codex' and len(model.requests) >= 2:
                wall = model.requests[1]
                headers = wall.get('responseHeaders', {})
                report['refusal_limit'] = headers.get('x-codex-active-limit')
                report['refusal_headers'] = {key: value for key, value in headers.items() if key.startswith('x-')}
                report['foreground_identity'] = [
                    {'identity': req.get('requestIdentity'), 'same_input_as_wall': req['body']['input'] == wall['body']['input']}
                    for req in model.requests if 'requestIdentity' in req]
                report['thread_title_calls'] = len(model.auxiliary_requests)
            report['receipts'] = [{k: row.get(k) for k in ('id', 'phase', 'old_pid', 'successor_pid',
                                                           'accepted_turn', 'launch_attempted')} for row in roots]
            calls = report['model_calls']
            if calls != EXPECTED_CALLS[:len(calls)]:
                failure = f'unexpected provider history {calls}'
                break
            if card is not None and not source_pane_closed:
                probe.require(not any(e.get('kind') == 'foreground' and e.get('account') == 'target' for e in events)
                              and not any(r.get('launch_attempted') or r.get('phase') == 'consumed' for r in roots),
                              'a replacement started before the source accounts pane was dismissed')
                if report['native_usage_interval_us']['source_429_written'] is not None:
                    probe.require(source is not None and not exit_watch.control(None, 1, 0),
                                  'the observed source exited with the accounts pane open')
                    report['source_card_at_refusal'] = card.dismiss_source()
                    source_pane_closed = True
            if len(roots) >= 2:
                failure = 'the host started a second logical rotation'
                break
            if roots:
                receipt = roots[0]
                if receipt['phase'] == 'retired':
                    failure = 'the host retired the rotation without completion'
                    break
                if receipt['phase'] == 'consumed' and calls == EXPECTED_CALLS:
                    state_now = json.loads((pool / 'state.json').read_text())
                    ledger = fixture.pool_ledger(pool)
                    report['target_proof_count'] = sum(row.get('kind') == 'proven' and row.get('account') == target_label
                                                       for row in ledger)
                    if state_now['active'] == target_label and state_now['proven'] and report['target_proof_count']:
                        rollout = completed_rollout(codex_home, receipt['intent']['thread'], receipt['intent']['wall'])
                        if rollout is not None:
                            exits = exit_watch.control(None, 1, 0)
                            exited = any(event.ident == source['pid'] and event.fflags & select.KQ_NOTE_EXIT
                                         for event in exits)
                            verify(model, codex_home, receipt, rollout, source, exited, report, auth_paths=auth_paths)
                            if footer_emails is not None:
                                report['visible_weekly_footers']['target'] = observe_weekly_footer(
                                    ctl, footer_emails['new'], 'destination', receipt['successor_pid'])
                            if card is not None:
                                probe.require(source_pane_closed, 'the source card inspection was incomplete')
                                verify_claim_loss(card, ctl, pid=receipt['successor_pid'],
                                                  birth=receipt['successor_birth'], thread=receipt['intent']['thread'],
                                                  codex_home=codex_home, report=report)
                            return
            time.sleep(.2)
        else:
            failure = 'the original turn did not complete on the target account within 450 seconds'
        try:
            with model.lock:
                requests = list(model.requests)
            record_failed_handoff(pool, codex_home, roots, requests, report)
        except Exception:
            # A malformed or missing artifact must not replace the actual handoff error or expose its contents.
            report['interrupted_turn_history'] = {'status': 'evidence_unavailable'}
        raise RuntimeError(failure)


def pin_executable(source, destination):
    source = source.resolve(strict=True)
    probe.require(source.is_file() and os.access(source, os.X_OK), f'requires an executable {destination.name}')
    # macOS cp clones on APFS and falls back to a copy across filesystems.
    subprocess.run(['/bin/cp', '-c', source, destination], check=True)
    destination.chmod(0o500)
    return destination


def smoke(root, args, report, *, goal_reset=False):
    env = probe.fresh_environment(root)
    env.update(XDG_CONFIG_HOME=str(root / 'xdg'), ATERM_STATE_HOME=str(root / 'home/Library/Application Support/aterm'))
    profile = r'^build:\s+debug / macos-[\w-]+$'
    host = pin_executable(args.aterm, root / 'aterm')
    diagnosis = run([host, '--diagnose'], env)
    probe.require(re.search(profile, diagnosis, re.M),
                  'requires an aterm macOS debug build; no window was started')
    lab = pin_executable(args.aterm_lab, root / 'aterm-lab')
    package = args.package.resolve(strict=True)
    build, _, binary = probe.package_records(package)
    report.update(host_sha256=fixture.image_digest(host)[1], lab_sha256=fixture.image_digest(lab)[1],
                  codex_sha256=fixture.image_digest(binary)[1], codex_source=build['source'], codex_version=build['version'])
    world = root / 'world'
    state = world / 'home/Library/Application Support/aterm'
    with ExitStack() as stack:
        if not goal_reset:
            model = QuotaModel(root, args.limit)
            slots = fixture_slots(world / 'home', shared_mask=args.account_card)
            device_login = args.device_login or args.account_card
            device_issuer = device.device_login_issuer(root / 'fixture-ca.pem', list(slots.values())) if device_login else None
            check_bearer = device_issuer.bearer_matches if device_issuer else None
            port = stack.enter_context(listener(model, True, issued_bearer_matches=check_bearer))
            issuer = f'https://127.0.0.1:{port}'
            usage_port = stack.enter_context(listener(model, False, handoff_usage_path=fixture.ROOT_USAGE_PATH,
                                                     issued_bearer_matches=check_bearer))
            usage_url = f'http://127.0.0.1:{usage_port}'
        sock, proc = stack.enter_context(window(lab, host, world, env,
                                              visible=args.account_card, goal_reset=goal_reset))
        if not goal_reset:
            stack.callback(model.snapshot_ready.set)
        host_env = {**env, 'HOME': str(world / 'home'), 'ATERM_STATE_HOME': str(state),
                    'XDG_CONFIG_HOME': str(world / 'cfg'), 'XDG_RUNTIME_DIR': str(world / 'run')}
        host_env.pop('CODEX_HOME')
        def ctl(*command):
            return run([host, 'ctl', '--sock', sock, '--timeout', '8', *command], host_env)
        if not goal_reset:
            policy = fixture.login_policy(ctl('harness', 'policy'))
            probe.require(policy['accounts'] == 'early' and policy['codexRotation'], 'native rotation is unavailable')
        lab_config = tomllib.loads((world / 'cfg/aterm/aterm.toml').read_text())
        probe.require((world / 'lab-world').is_file() and not (state / 'pkg/store/codex').exists()
                      and all(lab_config[lane]['enabled'] is False for lane in ('keeper', 'packages', 'update'))
                      and lab_config['machine']['universal_control'] == 'leave', 'unexpected lab world configuration')
        absent = subprocess.run([host, 'pkg', 'which', 'codex'], env=host_env, stdin=subprocess.DEVNULL,
                                capture_output=True, text=True, timeout=20)
        probe.require(absent.returncode == 1 and 'codex is not installed' in absent.stderr,
                      'the private resolver did not report an empty Codex slot')
        run([host, 'pkg', 'link', 'codex', package, 'bin/codex'], host_env)
        probe.require((state / 'pkg/store/codex/exec/codex').resolve(strict=True) == binary,
                      'private package registration did not select the input package')
        if goal_reset:
            from tui_goal_reset import run_goal_reset
            run_goal_reset(root, world / 'home/.codex', binary, ctl, proc, host_env, report)
            return
        codex_home = configure(root, world, port, issuer, usage_url, slots, device_login=device_login)
        login_digests = {}
        if device_issuer:
            stack.enter_context(device_issuer)
            report['device_logins'] = {}
            for label, slot in slots.items():
                report['device_logins'][label] = device_issuer.login(binary, codex_home, slot, host_env)
                login_digests[slot.auth_path] = hashlib.sha256(slot.auth_path.read_bytes()).digest()
        auth_paths = {slot.generation: slot.auth_path for slot in slots.values()}
        footer_emails = ({slot.generation: slot.email for slot in slots.values()}
                         if args.device_login and not args.account_card and args.limit == 'codex' else None)
        run_account_turn(model, ctl, report, state=state, codex_home=codex_home, binary=binary,
                         issuer=issuer, host_running=lambda: proc.poll() is None, auth_paths=auth_paths,
                         target_label='target', footer_emails=footer_emails,
                         card=CardCheck(ctl, focused_session(ctl)) if args.account_card else None)
        if device_issuer:
            probe.require(not (codex_home / 'auth.json').exists() and all(
                hashlib.sha256(path.read_bytes()).digest() == digest
                for path, digest in login_digests.items()), 'rotation changed an isolated login')
            report['device_slots_preserved'] = True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--aterm', type=Path, required=True, help='trusted debug aterm executable')
    parser.add_argument('--aterm-lab', type=Path, required=True, help='trusted aterm-lab executable')
    parser.add_argument('--package', type=Path, required=True, help='completed codex-alab package directory')
    parser.add_argument('--goal-reset', action='store_true',
                        help='verify a native goal in an idle TUI resumes after a dated weekly limit, with no further input')
    parser.add_argument('--device-login', action='store_true',
                        help='sign in to both empty fixture slots through the packaged Codex device login')
    parser.add_argument('--account-card', action='store_true',
                        help='open a focused private Mac window and verify the address card through handoff and restart; '
                             'also signs in both fixture slots')
    parser.add_argument('--limit', choices=('codex', fixture.provider_fixture.GOLDEN_MODEL_LIMIT), default='codex',
                        help='exhaust the weekly window of this limit, leaving ordinary quota usable for a named limit')
    parser.add_argument('--result', type=Path, required=True, help='new JSON report file')
    args = parser.parse_args()
    if args.goal_reset and (args.device_login or args.account_card or args.limit != 'codex'):
        parser.error('--goal-reset requires the ordinary weekly limit and a private world without account rotation')
    if sys.platform != 'darwin':
        parser.error('requires macOS')

    def interrupt(signum, _frame):
        raise KeyboardInterrupt(f'received signal {signum}')

    signal.signal(signal.SIGTERM, interrupt)
    report = {'passed': False, 'limit': args.limit}
    if args.goal_reset:
        report['scenario'] = 'native-goal-weekly-reset'
    if args.account_card:
        report['account_card_requested'] = True
    # The macOS sockaddr limit requires a short private path, independent of TMPDIR.
    with args.result.open('x') as result, tempfile.TemporaryDirectory(prefix='hr-', dir='/tmp') as scratch:
        try:
            smoke(Path(scratch).resolve(), args, report, goal_reset=args.goal_reset)
            report['passed'] = True
        except KeyboardInterrupt as error:
            report.update(probe.case_error_fields(error), error='interrupted')
            raise
        except Exception as error:
            report.update(probe.case_error_fields(error))
        finally:
            json.dump(report, result, indent=2)
            result.write('\n')
    print(json.dumps(report))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())

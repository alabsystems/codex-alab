#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Run actual saved-history pagination and resizing with fake auth and loopback only.

Builds nothing. The app creates260 completed turns through its real app-server,
then resumes the saved thread in a native PTY at76 and38 columns. The dedicated
single-threaded custodian owns its actual unreaped child/group through cleanup.
Runtime observations confer no proof, SDK, artifact or installation authority.
"""
import argparse
import base64
import errno
import fcntl
import hashlib
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import struct
import subprocess
import sys
import termios
import time


def require(test, why):
    if not test:
        raise RuntimeError(why)

def emit(value):
    print(json.dumps(value), flush=True)

GUARDIAN_SECONDS = 180
OUTPUT_LIMIT_BYTES = 32 * 1024 * 1024

def drain_pty(master, raw, deadline, cancellation, lifeline):
    """Drain the same retained PTY through EOF/EIO under its original bounds."""
    while True:
        require(cancellation() is None, 'PTY guardian cancelled during drain')
        remaining = deadline - time.monotonic()
        require(remaining > 0, 'PTY drain exceeded original guardian deadline')
        readable, _, _ = select.select([master, lifeline], [], [], min(.05, remaining))
        require(time.monotonic() < deadline, 'PTY drain exceeded original guardian deadline')
        if lifeline in readable:
            data = os.read(lifeline, 65536)
            require(data, 'parent lifeline EOF during drain')
            raise RuntimeError('unexpected parent command during completed PTY drain')
        if master not in readable:
            continue
        try:
            data = os.read(master, min(65536, OUTPUT_LIMIT_BYTES - raw.tell() + 1))
        except OSError as error:
            if error.errno == errno.EIO:
                break
            raise
        require(time.monotonic() < deadline, 'PTY drain exceeded original guardian deadline')
        if not data:
            break
        require(raw.tell() + len(data) <= OUTPUT_LIMIT_BYTES, 'PTY drain output byte bound')
        raw.write(data)
    require(cancellation() is None, 'PTY guardian cancelled during final drain bookend')
    require(time.monotonic() < deadline, 'PTY drain exceeded original guardian deadline')


def guardian(spec):
    """Single-threaded forkpty custodian; stdin EOF always cancels actual owned child."""
    setting = json.loads(Path(spec).read_text())
    pid = master = None
    reaped = False
    outcome = {'forced': False, 'command': setting['argv'],
               'forced_definition': 'guardian cancellation, timeout or protocol error; group teardown is recorded separately'}
    pending = None
    prior = signal.signal(signal.SIGCHLD, signal.SIG_DFL)
    def cancel(signum, _frame):
        # Never raise across forkpty's returned originals or waitpid's native
        # return before their ownership state has been published in Python.
        nonlocal pending
        pending = signum
    for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(signum, cancel)
    with Path(setting['raw']).open('xb', buffering=0) as raw:
        try:
            pid, master = pty.fork()
            if pid == 0:
                try:
                    for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
                        signal.signal(signum, signal.SIG_DFL)
                    os.chdir(setting['cwd'])
                    # forkpty has provided actual controlling tty/session/group.
                    os.execve(setting['argv'][0], setting['argv'], setting['env'])
                except BaseException as error:
                    os.write(2, str(error).encode())
                    os._exit(127)
            fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack('HHHH', 32, 76, 0, 0))
            emit({'started': pid})
            incoming, query = b'', b''
            end = time.monotonic() + GUARDIAN_SECONDS
            while True:
                require(pending is None, f'PTY guardian cancelled by signal {pending}')
                observed = os.waitid(os.P_PID, pid, os.WEXITED | os.WNOWAIT | os.WNOHANG)
                if observed is not None:
                    break
                require(time.monotonic() < end, 'PTY child exceeded original guardian deadline')
                readable, _, _ = select.select([0, master], [], [], .1)
                if 0 in readable:
                    data = os.read(0, 65536)
                    if not data:
                        raise RuntimeError('parent lifeline EOF')
                    incoming += data
                    require(len(incoming) <= 65536, 'oversize parent command')
                    while b'\n' in incoming:
                        line, incoming = incoming.split(b'\n', 1)
                        command = json.loads(line)
                        if 'write' in command:
                            os.write(master, base64.b64decode(command['write'], validate=True))
                        elif 'resize' in command:
                            rows, cols = command['resize']
                            require(rows == 32 and cols in (38, 76), 'unexpected resize')
                            fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack('HHHH', rows, cols, 0, 0))
                        else:
                            raise RuntimeError('unknown parent command')
                        emit({'ack': command['id']})
                if master in readable:
                    try:
                        data = os.read(master, 65536)
                    except OSError as error:
                        if error.errno != errno.EIO:
                            raise
                        data = b''
                    if data:
                        require(raw.tell() + len(data) <= OUTPUT_LIMIT_BYTES, 'PTY output byte bound')
                        raw.write(data)
                        query += data
                        for request, reply in ((b'\x1b[6n', b'\x1b[1;1R'),
                                               (b'\x1b[c', b'\x1b[?62;22c'),
                                               (b'\x1b[0c', b'\x1b[?62;22c'),
                                               (b'\x1b[?u', b'\x1b[?0u')):
                            if request in query:
                                os.write(master, reply)
                                query = query.replace(request, b'')
                        query = query[-16:]
            # WNOWAIT retains the actual original child/group identity until
            # any remaining group members have been cancelled. This is our
            # child, never a PID supplied by a fixture result or process scan.
            outcome['cleanup_signal'] = {'signal': int(signal.SIGKILL), 'result': 'attempted'}
            try:
                os.killpg(pid, signal.SIGKILL)
                outcome['cleanup_signal']['result'] = 'sent'
            except ProcessLookupError:
                outcome['cleanup_signal']['result'] = 'not-found'
            except PermissionError:
                # A refused signal is no closure observation. The actual owning
                # wait and subsequent native absence check remain mandatory.
                outcome['cleanup_signal']['result'] = 'permission-denied'
            _, status = os.waitpid(pid, 0)
            reaped = True
            outcome['exit'] = os.waitstatus_to_exitcode(status)
            try:
                os.killpg(pid, 0)
            except ProcessLookupError:
                outcome['group_absent_after_reap'] = True
            except PermissionError:
                outcome['group_absent_after_reap'] = False
            else:
                outcome['group_absent_after_reap'] = False
            require(outcome['group_absent_after_reap'], 'PTY group remains after actual owning reap')
            # Quiet is not EOF; the shared drain retains the SAME original
            # endpoint/byte cap and watches the parent's actual lifeline too.
            outcome['phase'] = 'post-reap-drain'
            drain_pty(master, raw, end, lambda: pending, 0)
        except BaseException as error:
            outcome['error'] = f'{type(error).__name__}: {error}'
            outcome['forced'] = True
        finally:
            # Close PTY first to remove output backpressure on forced cleanup.
            if master is not None:
                os.close(master)
            if pid is not None and pid != 0 and not reaped:
                # This is exclusively the unreaped forkpty child's owned group;
                # no externally observed PID is accepted as signaling authority.
                outcome['cleanup_signal'] = {'signal': int(signal.SIGKILL), 'result': 'attempted'}
                try:
                    os.killpg(pid, signal.SIGKILL)
                    outcome['cleanup_signal']['result'] = 'sent'
                except ProcessLookupError:
                    outcome['cleanup_signal']['result'] = 'not-found'
                except PermissionError:
                    outcome['cleanup_signal']['result'] = 'permission-denied'
                _, status = os.waitpid(pid, 0)
                outcome['exit'] = os.waitstatus_to_exitcode(status)
                reaped = True
            outcome['owning_reap'] = reaped
            outcome['cancellation_signal'] = pending
            if reaped and 'group_absent_after_reap' not in outcome:
                try:
                    os.killpg(pid, 0)
                except ProcessLookupError:
                    outcome['group_absent_after_reap'] = True
                except PermissionError:
                    outcome['group_absent_after_reap'] = False
                else:
                    outcome['group_absent_after_reap'] = False
            signal.signal(signal.SIGCHLD, prior)
    Path(setting['result']).write_text(json.dumps(outcome, indent=2) + '\n')
    emit({'completed': outcome})
    return 0 if outcome.get('exit') == 0 and not outcome['forced'] else 1

if len(sys.argv) > 1 and sys.argv[1] == '--guardian':
    raise SystemExit(guardian(sys.argv[2]))

import threading
import types
sys.dont_write_bytecode = True

def module(name):
    file = Path(__file__).resolve().parent / (name + '.py')
    value = types.ModuleType(name)
    value.__file__ = str(file)
    sys.modules[name] = value
    exec(compile(file.read_bytes(), str(file), 'exec'), value.__dict__)
    return value

provider = module('provider_fixture')
probe = module('capacity_exec')
weekly = module('weekly_reset')
resume = module('quota_resume')

class Model:
    def __init__(self):
        self.lock = threading.Lock()
        self.requests, self.models = [], []

    def reply(self, method, path, body):
        with self.lock:
            self.requests.append({'method': method, 'path': path, 'body': body})
            if method == 'POST' and path == '/v1/responses':
                self.models.append(body)
                number = len(self.models)
                text = f'HISTORY-ANSWER-{number:03d} alpha bravo charlie delta echo foxtrot golf hotel india juliet.\nSecond persisted line for turn {number:03d}.'
                item = {'type':'message','role':'assistant','id':f'answer-{number}',
                        'content':[{'type':'output_text','text':text}]}
                return 200, weekly.response(f'response-{number}', item), 'text/event-stream'
            if path.endswith('/usage'):
                return 200, json.dumps(provider.usage(weekly.ACCOUNT, weekly.USER,
                    week_used=1, session_used=1, allowed=True)).encode(), 'application/json'
            if path.endswith('/rate-limit-reset-credits'):
                return 200, json.dumps(provider.credit_list()).encode(), 'application/json'
            if method == 'GET':
                return 200, b'{"data":[],"models":[]}', 'application/json'
            return 404, b'{"error":"unexpected fixture route"}', 'application/json'

def main():
    global ROOT, SOURCE, BINARY, EXPECTED
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--codex', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    ROOT = args.output.absolute()
    SOURCE = Path(__file__).resolve().parents[2]
    BINARY = args.codex.resolve(strict=True)
    with BINARY.open('rb') as original:
        EXPECTED = hashlib.file_digest(original, 'sha256').hexdigest()
    ROOT.mkdir(mode=0o700)
    env = probe.fresh_environment(ROOT)
    model = Model()
    server, worker = weekly.serve(model)
    state = {'binary':str(BINARY),'sha256':EXPECTED,'turns':260,'limits':[
        'Isolated fake auth + loopback provider only; no user configuration/channel/credits.',
        'Actual app-created saved thread and native TUI render; no old-bug reproduction or proof claim.',
        'PTY custodian owns actual child/reap; no arbitrary descendant containment claim.'],
        'passed':False}
    completed = False
    try:
        # Pagination exercises a custom provider with a fake local API key.
        # ChatGPT workspace routing correctly requires HTTPS and is a separate
        # family from these loopback-only history/rendering observations.
        weekly.configure(ROOT, server.server_port, openai_auth=False)
        config = ROOT / 'codex-home/config.toml'
        config.write_text('log_dir = ' + json.dumps(str(ROOT / 'logs')) + '\n' + config.read_text())
        (ROOT / 'codex-home/auth.json').write_text(json.dumps({'OPENAI_API_KEY':'sk-fixture-local-only'}))
        def seed(process, record):
            rpc = resume.Rpc(process, record)
            rpc.initialize()
            response = rpc.request('thread/start', {'cwd':str(ROOT/'work'),'model':'gpt-5',
                'modelProvider':'weekly-smoke','ephemeral':False})
            state['thread'] = response['thread']['id']
            for number in range(1, state['turns'] + 1):
                turn = rpc.turn(state['thread'], f'HISTORY-PROMPT-{number:03d} persist this independent completed turn.')
                require(turn['status'] == 'completed', f'seed turn{number} failed: {turn}')
                if number % 25 == 0:
                    emit({'seeded':number})
            # Query the genuine saved history API and retain its returned population.
            page = rpc.request('thread/turns/list', {'threadId':state['thread'],'limit':100,
                'sortDirection':'asc','itemsView':'summary'})
            (ROOT/'saved-first-turn-page.json').write_text(json.dumps(page, indent=2)+'\n')
        require(probe.run(BINARY, ['app-server','--listen','stdio://'], ROOT, env, 'seed', interact=seed) == 0,
                'real seeding app-server did not exit0')
        state['model_requests_after_seed'] = len(model.models)
        require(len(model.models) == state['turns'], 'unexpected model request count')
        (ROOT/'seed-state.json').write_text(json.dumps(state,indent=2)+'\n')
        emit({'seed_complete':state['thread']})
        run_tui(env, state)
        require(not any(request['path'].startswith('/backend-api/') for request in model.requests),
                'custom-provider history smoke consulted a ChatGPT account endpoint')
        completed = True
    finally:
        server.shutdown()
        worker.join()
        server.server_close()
        (ROOT/'http.json').write_text(json.dumps(model.requests,indent=2)+'\n')
        try:
            if completed:
                with BINARY.open('rb') as original:
                    require(hashlib.file_digest(original, 'sha256').hexdigest() == EXPECTED,
                            'immutable binary SHA changed during smoke')
                state['passed'] = True
        finally:
            (ROOT/'result.json').write_text(json.dumps(state,indent=2)+'\n')
    emit({'passed':True,'older_pages':state['older_history_page_requests'],'tui_bytes':state['tui_bytes']})

def run_tui(env, state):
    env = dict(env)
    env.pop('NO_COLOR', None)
    env['TERM'] = 'xterm-256color'
    env['RUST_LOG'] = 'codex_tui=debug'
    raw = ROOT/'tui.raw'
    spec = {'argv':[str(BINARY),'--no-daemon','resume',state['thread']],
        'env':env,'cwd':str(ROOT/'work'),'raw':str(raw),'result':str(ROOT/'tui-custody.json')}
    path = ROOT/'tui-spec.json'
    path.write_text(json.dumps(spec))
    with (ROOT/'guardian-stderr.log').open('xb') as stderr:
        previous_sigchld = signal.signal(signal.SIGCHLD, signal.SIG_DFL)
        try:
            child = subprocess.Popen([sys.executable,'-I','-B','-S',__file__,'--guardian',str(path)],
                stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=stderr,env=env)
        except BaseException:
            signal.signal(signal.SIGCHLD, previous_sigchld)
            raise
        pending, buffer, counter = [], b'', 0
        def events():
            nonlocal buffer
            if select.select([child.stdout],[],[],.05)[0]:
                data = os.read(child.stdout.fileno(),65536)
                buffer += data
                while b'\n' in buffer:
                    line, buffer = buffer.split(b'\n',1)
                    pending.append(json.loads(line))
        def command(value):
            nonlocal counter
            counter += 1
            value['id'] = counter
            child.stdin.write((json.dumps(value)+'\n').encode())
            child.stdin.flush()
            deadline = time.monotonic()+5
            while not any(v.get('ack') == counter for v in pending):
                require(time.monotonic()<deadline,'guardian command acknowledgment timeout')
                events()
        def key(value):
            command({'write':base64.b64encode(value).decode()})
        def pump(seconds):
            end = time.monotonic()+seconds
            while time.monotonic()<end:
                events()
                require(child.poll() is None,'PTY guardian exited before planned shutdown')
        try:
            deadline = time.monotonic()+20
            while not raw.exists() or b'HISTORY-' not in raw.read_bytes():
                require(time.monotonic()<deadline,'saved thread never rendered in TUI')
                events()
            pump(1)
            (ROOT/'render-initial.raw').write_bytes(raw.read_bytes())
            require(b'HISTORY-ANSWER-260' in raw.read_bytes(),'latest saved answer absent from actual rendering')
            steps=[]
            for phase in range(6):
                width = 38 if phase%2==0 else 76
                command({'resize':[32,width]})
                pump(.3)
                start = raw.stat().st_size
                for _ in range(45):
                    key(b'\x1b[5~')
                    pump(.045)
                pump(.4)
                fragment = raw.read_bytes()[start:]
                (ROOT/f'render-up-{phase}-{width}.raw').write_bytes(fragment)
                steps.append({'phase':phase,'width':width,'bytes':len(fragment),
                    'markers':sorted(set(m.decode() for m in re.findall(rb'HISTORY-(?:PROMPT|ANSWER)-\d{3}',fragment)))})
                emit({'page_up_phase':phase,'width':width,'render_bytes':len(fragment)})
            require(b'HISTORY-PROMPT-001' in raw.read_bytes(),'oldest persisted prompt never actually rendered')
            for phase,width in enumerate((76,38,76)):
                command({'resize':[32,width]})
                for _ in range(80):
                    key(b'\x1b[6~')
                    pump(.025)
            pump(.4)
            state['render_steps']=steps
            state['tui_bytes']=raw.stat().st_size
            # The first Ctrl-C may already quit. A second is needed only if
            # the SAME still-owned guardian/child has not completed naturally.
            key(b'\x03')
            try:
                child.wait(timeout=1)
            except subprocess.TimeoutExpired:
                key(b'\x03')
            child.wait(timeout=20)
            child.communicate(timeout=20)
            require(child.returncode==0,'PTY guardian/child failed or required forced cleanup')
            custody=json.loads((ROOT/'tui-custody.json').read_text())
            require(custody.get('exit')==0 and custody.get('owning_reap') and custody.get('group_absent_after_reap') is True and not custody['forced'],
                'actual TUI successful owning retirement absent')
            state['custody']=custody
            log=ROOT/'logs/codex-tui.log'
            text=log.read_text(errors='replace') if log.exists() else ''
            state['older_history_page_requests']=text.count('loading older transcript history page')
            require(state['older_history_page_requests']>=3,'fewer than3 actual older history page requests observed')
            require(b'panicked at' not in raw.read_bytes() and 'panicked at' not in text,'native TUI panic observed')
        finally:
            if child.poll() is None:
                # EOF is the guardian's actual lifeline cancellation path; it owns cleanup/reap.
                if child.stdin is not None:
                    child.stdin.close()
                    child.stdin=None
                child.communicate(timeout=20)
            else:
                child.communicate(timeout=20)
            signal.signal(signal.SIGCHLD, previous_sigchld)
            state['guardian_returncode']=child.returncode
            (ROOT/'guardian-events.json').write_text(json.dumps(pending,indent=2)+'\n')

if __name__=='__main__':
    try:
        main()
    except BaseException as error:
        emit({'failed':f'{type(error).__name__}: {error}'})
        raise

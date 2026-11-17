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
    handlers = {}
    primary = None
    published = None
    def cancel(signum, _frame):
        # Never raise across forkpty's returned originals or waitpid's native
        # return before their ownership state has been published in Python.
        nonlocal pending
        pending = signum

    def signal_original(*, group):
        require(pid is not None and pid > 0 and not reaped,
                'PTY signaling requires the actual unreaped child')
        effect = {'signal': int(signal.SIGKILL), 'result': 'attempted'}
        try:
            (os.killpg if group else os.kill)(pid, signal.SIGKILL)
            effect['result'] = 'sent'
        except ProcessLookupError:
            effect['result'] = 'not-found'
        except PermissionError:
            effect['result'] = 'permission-denied'
        return effect

    def cancellation_bookend():
        outcome['cancellation_signal'] = pending
        if pending is not None:
            outcome['forced'] = True
            outcome.setdefault('error', f'PTY guardian cancelled by signal {pending}')

    def publish():
        nonlocal published
        text = json.dumps(outcome, indent=2) + '\n'
        Path(setting['result']).write_text(text)
        published = text

    def diagnostic(error):
        kind = 'BaseException'
        try:
            label = type(error).__name__
            if type(label) is str:
                kind = label
        except BaseException:
            pass
        try:
            message = str(error)
            if type(message) is str:
                return kind + ': ' + message
        except BaseException:
            pass
        return kind

    def note_primary(message):
        try:
            primary.add_note(message)
        except BaseException:
            pass

    def successful_primary_exit():
        # Keep this standalone policy aligned with alab/cli_status.py.
        if not issubclass(type(primary), SystemExit):
            return False
        if type(primary) is not SystemExit:
            raise RuntimeError("unsupported SystemExit type") from None
        code = primary.code
        if code is None:
            return True
        if not issubclass(type(code), int):
            return False
        code = int.__index__(code)
        if os.name == 'posix' and -sys.maxsize - 1 <= code <= sys.maxsize:
            return code & 0xff == 0
        return code == 0

    try:
        for signum, handler in ((signal.SIGCHLD, signal.SIG_DFL), (signal.SIGINT, cancel),
                                (signal.SIGTERM, cancel), (signal.SIGHUP, cancel)):
            handlers[signum] = signal.getsignal(signum)
            signal.signal(signum, handler)
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
                outcome['cleanup_signal'] = signal_original(group=True)
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
                outcome['error'] = diagnostic(error)
                outcome['forced'] = True
            finally:
                # Close PTY first to remove output backpressure on forced cleanup.
                if master is not None:
                    os.close(master)
                if pid is not None and pid != 0 and not reaped:
                    # This is exclusively the unreaped forkpty child's owned group;
                    # no externally observed PID is accepted as signaling authority.
                    outcome['cleanup_signal'] = signal_original(group=True)
                    if outcome['cleanup_signal']['result'] == 'not-found':
                        # forkpty's parent can return before the child's session/group
                        # exists. Its actual unreaped PID is still ours to cancel.
                        # A permission refusal never authorizes this fallback.
                        outcome['owned_child_signal'] = signal_original(group=False)
                        if outcome['owned_child_signal']['result'] == 'sent':
                            # Keep the original child's identity pinned after it exits,
                            # then clear any group it formed during the signal race.
                            os.waitid(os.P_PID, pid, os.WEXITED | os.WNOWAIT)
                            outcome['cleanup_group_after_child_exit'] = signal_original(group=True)
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
        cancellation_bookend()
        publish()
        cancellation_bookend()
        if json.dumps(outcome, indent=2) + '\n' != published:
            publish()
        emit({'completed': outcome})
    except BaseException as error:
        primary = error
        raise
    finally:
        restoration_errors = []
        for signum, handler in reversed(list(handlers.items())):
            try:
                signal.signal(signum, handler)
            except BaseException as error:
                restoration_errors.append(f'signal restoration {signum}: {diagnostic(error)}')
        if restoration_errors:
            outcome['forced'] = True
            outcome['signal_restore_errors'] = restoration_errors
            outcome.setdefault('error', 'PTY guardian signal restoration failed')
            if primary is not None:
                for error in restoration_errors:
                    note_primary(error)
        # Include signals witnessed during descriptor close, result writing,
        # completion emission or restoration in the final saved verdict.
        cancellation_bookend()
        if published is not None and json.dumps(outcome, indent=2) + '\n' != published:
            try:
                publish()
            except BaseException as error:
                failure = 'final custody publication: ' + diagnostic(error)
                if primary is None:
                    raise RuntimeError(failure) from error
                note_primary(failure)
                if successful_primary_exit():
                    # A healthy earlier file plus a success-shaped primary exit
                    # cannot qualify an uncommitted corrective publication.
                    raise RuntimeError(failure) from primary
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

def _arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--codex', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    return parser.parse_args()

def main(args=None):
    global ROOT, SOURCE, BINARY, EXPECTED
    if args is None:
        args = _arguments()
    ROOT = args.output.absolute()
    SOURCE = Path(__file__).resolve().parents[2]
    BINARY = args.codex.resolve(strict=True)
    EXPECTED = weekly.file_sha256(BINARY)
    ROOT.mkdir(mode=0o700)
    env = probe.fresh_environment(ROOT)
    model = Model()
    state = {'binary':str(BINARY),'sha256':EXPECTED,'turns':260,'limits':[
        'Isolated fake auth + loopback provider only; no user configuration/channel/credits.',
        'Actual app-created saved thread and native TUI render; no old-bug reproduction or proof claim.',
        'PTY custodian owns actual child/reap; no arbitrary descendant containment claim.'],
        'passed':False}
    completed = False
    primary = None
    listener = None

    def mark_failure(error):
        state['passed'] = False
        state.update(probe.case_error_fields(error))
        if not isinstance(error, Exception):
            state['interrupted'] = True

    def write_result():
        (ROOT/'result.json').write_text(json.dumps(state,indent=2)+'\n')

    def publish_result(active=None):
        healthy = state['passed']
        try:
            write_result()
        except BaseException as error:
            selected = active if active is not None else error
            mark_failure(selected)
            if healthy:
                # An interrupted healthy publication gets one failed correction.
                try:
                    write_result()
                finally:
                    raise selected
            raise selected
        if active is not None:
            raise active

    def verify_identity():
        if completed:
            require(weekly.file_sha256(BINARY) == EXPECTED,
                    'immutable binary SHA changed during smoke')

    try:
        listener = weekly.serve(model)
        server, worker = listener
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
    except BaseException as error:
        primary = error
    finally:
        try:
            probe.cleanup_resources(
                (('history', *listener),) if listener is not None else (),
                (('http.json', lambda: (ROOT/'http.json').write_text(json.dumps(model.requests,indent=2)+'\n')),
                 ('binary-identity', verify_identity)), state, primary)
        except BaseException as error:
            primary = error
            mark_failure(error)
        else:
            state['passed'] = completed and not state.get('cleanupErrors')
        publish_result(primary)
    require(state['passed'], state.get('error', 'history cleanup or evidence export failed'))
    try:
        emit({'passed':True,'older_pages':state['older_history_page_requests'],'tui_bytes':state['tui_bytes']})
    except BaseException as error:
        mark_failure(error)
        publish_result(error)

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

def cli():
    args = _arguments()
    try:
        main(args)
    except BaseException as error:
        failure = {'failed': probe.case_error_fields(error)['error']}
        if isinstance(error, SystemExit) and weekly.CLI['successful_exit'](error):
            return weekly.CLI['report_failure']('history pagination: ', error, incomplete=True,
                                                 stdout_line=json.dumps(failure))
        try:
            emit(failure)
        finally:
            raise error
    return 0

if __name__=='__main__':
    sys.exit(cli())

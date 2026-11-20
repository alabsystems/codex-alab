#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Run the existing native gate and retain a source-bound local observation."""
import argparse
from contextlib import contextmanager
from datetime import datetime, timezone
import io
import json
import os
from pathlib import Path
import re
import runpy
import subprocess
import sys
import tempfile

if __name__ == '__main__':
    sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from upstream_intake import git, git_environment, integrated_upstream, upstream_target

validate_ident = runpy.run_path(str(Path(__file__).resolve().with_name('source_content.py')))['validate_ident']
successful_exit = runpy.run_path(str(Path(__file__).with_name('cli_status.py')))['successful_exit']
FILE_HASH = runpy.run_path(str(Path(__file__).resolve().with_name('file_hash.py')))

LIMITS = ('Local gate execution and Git-state observation; not authenticated attestation, '
          'a replacement gate, artifact promotion or live entitlement.')
SCHEMA = 5
MAX_FILTER_CONFIG_BYTES = 64 * 1024


class GateObservationError(OSError):
    """A completed gate failed and its observation could not be retained."""

    def __new__(cls, gate_status, original_error):
        return super().__new__(cls)

    def __init__(self, gate_status, original_error):
        super().__init__('qualification observation failed after a completed gate')
        self.gate_status = gate_status
        self.original_error = original_error


def checked_root(repo):
    repo = Path(repo).resolve(strict=True)
    observed = Path(git(repo, 'rev-parse', '--show-toplevel')).resolve(strict=True)
    if observed != repo:
        raise ValueError('qualification root differs from the Git worktree root')
    return repo


def external_filter_config(repo):
    """Disable external conversion callbacks without changing builtin Git text rules."""
    keys = git(repo, 'config', '--name-only', '--null', '--list')
    if (len(os.fsencode(keys)) > MAX_FILTER_CONFIG_BYTES
            or keys and not keys.endswith('\0')):
        raise ValueError('Git filter configuration is unbounded or not NUL-framed')
    drivers = set()
    for key in keys.split('\0')[:-1]:
        if not key:
            raise ValueError('Git filter configuration has an ambiguous empty key')
        match = re.fullmatch(r'(filter\.(.*))\.(?:clean|smudge|process|required)', key, re.DOTALL)
        if match:
            if not match[2] or '=' in match[1]:
                raise ValueError('Git filter driver cannot be safely overridden')
            drivers.add(match[1])
    options = []
    for driver in sorted(drivers):
        options.extend(('-c', driver + '.clean=', '-c', driver + '.smudge=', '-c', driver + '.process=',
                        '-c', driver + '.required=false'))
    return tuple(options)


def worktree_clean(repo, source, parents=frozenset(), *, scratch_dir=None):
    repo = checked_root(repo)
    if repo in parents:
        raise ValueError('qualification submodule roots form a cycle')
    if git(repo, 'rev-parse', '--verify', 'HEAD^{commit}') != source:
        return False
    staged = git(repo, 'diff', '--cached', '--no-ext-diff', '--no-textconv',
                 '--ita-visible-in-index', '--ignore-submodules=none',
                 '--name-only', '-z', source, '--')
    common = Path(git(repo, 'rev-parse', '--path-format=absolute', '--git-common-dir'))
    filter_config = external_filter_config(repo)
    # The real index can hide edits through flags or cached stat data. Seed a
    # private index from the commit so Git checks the working files afresh.
    scratch = common if scratch_dir is None else Path(scratch_dir).resolve(strict=True)
    if scratch_dir is not None and (not Path(scratch_dir).is_absolute()
                                    or scratch.is_relative_to(common.resolve(strict=True))):
        raise ValueError('source comparison scratch must be absolute and outside Git metadata')
    with tempfile.TemporaryDirectory(prefix='alab-status-', dir=scratch) as directory:
        index = Path(directory) / 'index'
        index_config = ('-c', 'core.splitIndex=false', '-c', 'core.sparseCheckout=false')
        git(repo, *index_config, 'read-tree', source, index_file=index)
        validate_ident(repo, source, env=git_environment(index_file=index), filter_config=filter_config)
        changed = git(
            repo, *index_config, *filter_config,
            '-c', 'core.untrackedCache=false', '-c', 'core.ignorestat=false',
            '-c', 'core.trustctime=true', '-c', 'core.checkStat=default',
            '-c', 'core.filemode=true', '-c', 'core.symlinks=true',
            'status', '--porcelain', '--untracked-files=all', '--ignore-submodules=all',
            index_file=index)
    if staged or changed:
        return False
    # Parent status otherwise delegates to each child's real stat cache. Check
    # every committed gitlink independently against its exact source instead.
    for entry in git(repo, 'ls-tree', '-r', '-z', source).split('\0'):
        if not entry:
            continue
        header, name = entry.split('\t', 1)
        mode, kind, identity = header.split()
        if mode != '160000':
            continue
        child = repo / name
        if (kind != 'commit' or child.is_symlink() or not child.is_dir()
                or not child.resolve(strict=True).is_relative_to(repo)):
            raise ValueError('qualification submodule has no contained working directory')
        if not worktree_clean(child, identity, parents | {repo}, scratch_dir=scratch_dir):
            return False
    return True


def snapshot(repo):
    repo = checked_root(repo)
    source = git(repo, 'rev-parse', '--verify', 'HEAD^{commit}')
    clean = worktree_clean(repo, source)
    return {'source': source, 'tree': git(repo, 'rev-parse', source + '^{tree}'),
            'base': git(repo, 'rev-parse', '--verify', 'origin/main^{commit}'),
            'upstream': integrated_upstream(repo, source),
            'clean': clean}


def now():
    return datetime.now(timezone.utc).isoformat()


def _cleanup_failure(primary, failures):
    selected = primary
    for _phase, error in failures:
        if isinstance(selected, Exception) and not isinstance(error, Exception):
            selected = error
    for phase, error in [('qualification operation', primary), *failures]:
        if error is selected:
            continue
        try:
            if hasattr(selected, 'add_note'):
                selected.add_note(f'{phase} failed: {error!r}')
        except BaseException as note_error:
            # Reporting must not replace the failure it describes.
            if isinstance(selected, Exception) and not isinstance(note_error, Exception):
                selected = note_error
    return selected


@contextmanager
def _closing(resource, phase):
    try:
        yield resource
    except BaseException as primary:
        failures = []
        try:
            resource.close()
        except BaseException as error:
            failures.append((phase, error))
        selected = _cleanup_failure(primary, failures)
        if selected is not primary:
            raise selected from primary
        raise
    else:
        resource.close()


def _cleanup_process(process, primary):
    failures = []

    def attempt(action, phase):
        try:
            action()
            return True
        except BaseException as error:
            failures.append((phase, error))
            return False

    try:
        if process.returncode is None:
            attempt(lambda: process.terminate(), 'qualification terminate')
            if process.returncode is None:
                try:
                    process.wait(timeout=30)
                except subprocess.TimeoutExpired:
                    pass  # The bounded wait requests escalation, not a new failure.
                except BaseException as error:
                    failures.append(('qualification wait', error))
            if process.returncode is None:
                killed = attempt(lambda: process.kill(), 'qualification kill')
                while process.returncode is None:
                    try:
                        process.wait(timeout=None if killed else 30)
                        break
                    except BaseException as error:
                        failures.append(('qualification reap', error))
                        if not killed or isinstance(error, Exception):
                            break
    except BaseException as error:
        failures.append(('qualification retirement', error))
    finally:
        attempt(lambda: process.stdout.close(), 'qualification stdout cleanup')
    return _cleanup_failure(primary, failures)


def write_record(directory, record):
    temporary = None
    primary = None
    try:
        with _closing(tempfile.NamedTemporaryFile(
                mode='w', encoding='utf-8', dir=directory,
                prefix='.qualification-record-', suffix='.json', delete=False),
                'qualification final record') as out:
            temporary = Path(out.name)
            json.dump(record, out, indent=2)
            out.write('\n')
        # Completed critical I/O precedes publication. An existing final name
        # is never replaced, even if it appeared while the gate was running.
        os.link(temporary, directory / 'record.json')
    except BaseException as error:
        primary = error
        raise
    finally:
        if temporary is not None:
            try:
                temporary.unlink()
            except BaseException as error:
                if primary is not None:
                    selected = _cleanup_failure(
                        primary, [('qualification record temporary cleanup', error)])
                    if selected is not primary:
                        raise selected from primary
                elif isinstance(error, OSError):
                    # The record is already committed; ordinary cleanup I/O
                    # cannot invalidate it or replace its completed gate result.
                    _report_failure(error, prefix='qualification record temporary cleanup: ')
                else:
                    raise


def run(repo, directory=None, stream=None):
    repo = Path(repo).resolve(strict=True)
    before = snapshot(repo)
    if not before['clean']:
        raise ValueError('qualification requires a committed clean candidate')
    common = Path(git(repo, 'rev-parse', '--path-format=absolute', '--git-common-dir')).resolve()
    if directory is None:
        parent = common / 'alab-qualification' / before['source']
        parent.mkdir(parents=True, exist_ok=True)
        directory = Path(tempfile.mkdtemp(prefix='run-', dir=parent))
    else:
        directory = Path(directory)
        if not directory.is_absolute():
            raise ValueError('qualification output directory must be absolute')
        directory = directory.resolve()
        if directory.is_relative_to(repo) and not directory.is_relative_to(common):
            raise ValueError('qualification output must stay outside candidate inputs')
        directory.mkdir(mode=0o700)
    # Output inside an unignored source path must not silently dirty its candidate.
    if snapshot(repo) != before:
        raise ValueError('qualification output must stay outside candidate inputs')
    command = [str(repo / 'alab/check.sh')]
    started = now()
    with _closing((directory / 'start.json').open('x'), 'qualification start record') as out:
        json.dump({'schema': SCHEMA, 'before': before, 'command': command,
                   'started_at': started}, out, indent=2)
        out.write('\n')
    if stream is None:
        stream = sys.stdout.buffer
    stream.write(('qualification log: ' + str(directory / 'gate.log') + '\n').encode())
    stream.flush()
    status = None
    try:
        with _closing((directory / 'gate.log').open('xb'), 'qualification log') as log:
            environment = os.environ.copy()
            for name in list(environment):
                if name.startswith('GIT_'):
                    environment.pop(name)
            environment.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1',
                               GIT_NO_REPLACE_OBJECTS='1', GIT_NO_LAZY_FETCH='1',
                               GIT_OPTIONAL_LOCKS='0', GIT_TERMINAL_PROMPT='0')
            for name in ('OPENAI_API_KEY', 'CODEX_API_KEY', 'CODEX_ACCESS_TOKEN'):
                environment.pop(name, None)
            process = subprocess.Popen(command, cwd=repo, stdout=subprocess.PIPE,
                                       stderr=subprocess.STDOUT, env=environment)
            try:
                while True:
                    chunk = process.stdout.read1(65536)
                    if not chunk:
                        break
                    log.write(chunk)
                    log.flush()
                    stream.write(chunk)
                    stream.flush()
                status = process.wait()
            except BaseException as primary:
                selected = _cleanup_process(process, primary)
                if selected is not primary:
                    raise selected from primary
                raise
            else:
                process.stdout.close()
        after = snapshot(repo)
        qualified = status == 0 and before == after and after['clean']
        record = {'schema': SCHEMA, 'kind': 'native-check-execution-observation',
                  'before': before, 'after': after, 'command': command,
                  'status': status, 'qualified_source': before['source'] if qualified else None,
                  'started_at': started, 'completed_at': now(), 'log': 'gate.log',
                  'log_sha256': FILE_HASH['file_sha256'](directory / 'gate.log'),
                  'limits': LIMITS}
        write_record(directory, record)
        stream.write(('qualification record: ' + str(directory / 'record.json') + '\n').encode())
        stream.flush()
        if status:
            return 128 - status if status < 0 else status
        if not qualified:
            stream.write(b'qualification inputs changed; no source qualified\n')
            stream.flush()
        return 0 if qualified else 1
    except OSError as error:
        if status is not None and status != 0:
            raise GateObservationError(status, error) from error
        raise


def inspect(repo, path):
    path = Path(path)
    if path.is_symlink() or (path.parent / 'gate.log').is_symlink():
        raise ValueError('qualification record and log must not be symlinks')
    _metadata, data = FILE_HASH['file_bytes_snapshot'](path)
    with io.TextIOWrapper(io.BytesIO(data)) as text:
        record = json.loads(text.read())
    if (not isinstance(record, dict) or type(record.get('schema')) is not int
            or record['schema'] != SCHEMA or record.get('kind') != 'native-check-execution-observation'
            or type(record.get('status')) is not int or record['status'] != 0
            or not isinstance(record.get('before'), dict) or not isinstance(record.get('after'), dict)
            or record['before'] != record['after']
            or record['before'].get('clean') is not True or record['after'].get('clean') is not True
            or record.get('log') != 'gate.log'):
        raise ValueError('qualification record has no successful unchanged-source gate outcome')
    source = record.get('qualified_source')
    if not isinstance(source, str) or source != record['before'].get('source'):
        raise ValueError('qualification source differs from the observed candidate')
    for field in ('source', 'tree', 'base', 'upstream'):
        value = record['before'].get(field)
        if not isinstance(value, str) or not re.fullmatch(r'(?:[a-f0-9]{40}|[a-f0-9]{64})', value):
            raise ValueError('qualification context needs full object identities')
    if git(repo, 'rev-parse', '--verify', '--end-of-options', source + '^{commit}') != source:
        raise ValueError('qualification source is not an exact local commit')
    if git(repo, 'rev-parse', source + '^{tree}') != record['before'].get('tree'):
        raise ValueError('qualification tree differs from source')
    upstream = record['before'].get('upstream')
    if not isinstance(upstream, str) or git(repo, 'merge-base', source, upstream) != upstream:
        raise ValueError('qualification upstream differs from the source frontier')
    upstream_target(repo, source, upstream)
    if FILE_HASH['file_sha256'](path.parent / 'gate.log') != record.get('log_sha256'):
        raise ValueError('qualification log hash changed')
    return {'source': source, 'upstream': record['before'].get('upstream'),
            'completed_at': record.get('completed_at'), 'record': str(path.resolve()),
            'limits': LIMITS}


def _report_failure(error, *, incomplete=False, prefix='qualification observation: '):
    try:
        if incomplete:
            message = 'qualification observation incomplete: observer requested a successful exit'
        else:
            message = prefix + str(error)
        print(message, file=sys.stderr)
        for note in getattr(error, '__notes__', ()):
            print('qualification observation note: ' + str(note), file=sys.stderr)
    except SystemExit as report_error:
        if not successful_exit(report_error):
            raise
    except Exception:
        pass  # A diagnostic sink failure cannot make the observation successful.
    return 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--directory', type=Path)
    args = parser.parse_args()
    try:
        return run(args.repo, args.directory)
    except GateObservationError as error:
        _report_failure(error.original_error)
        return 128 - error.gate_status if error.gate_status < 0 else error.gate_status
    except SystemExit as error:
        if successful_exit(error):
            return _report_failure(error, incomplete=True)
        raise
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        return _report_failure(error)


def _quiesce_broken_output(stream):
    descriptor = stream.fileno()
    null = os.open(os.devnull, os.O_WRONLY)
    if null == descriptor:
        return  # The output descriptor itself now owns the null device.
    try:
        os.dup2(null, descriptor)
    except BaseException as primary:
        failures = []
        try:
            os.close(null)
        except BaseException as error:
            failures.append(('qualification null output cleanup', error))
        selected = _cleanup_failure(primary, failures)
        if selected is not primary:
            raise selected from primary
        raise
    else:
        os.close(null)


def cli():
    """Finish CLI-owned output before Python can replace the selected exit."""
    status = None
    primary = None
    try:
        status = main()
    except BaseException as error:
        primary = error
    failures = []
    for name in ('stdout', 'stderr'):
        stream = getattr(sys, name)
        try:
            stream.flush()
        except BaseException as error:
            failures.append(('qualification ' + name + ' flush', error))
            if isinstance(error, BrokenPipeError):
                try:
                    _quiesce_broken_output(stream)
                except BaseException as cleanup:
                    failures.append(('qualification ' + name + ' quiescence', cleanup))
                    # The failed CLI sink must not retry buffered writes at
                    # interpreter shutdown when a null device is unavailable.
                    try:
                        stream.close()
                    except BaseException as close_error:
                        failures.append(('qualification ' + name + ' close', close_error))
    if (primary is not None and failures and type(primary) is SystemExit
            and successful_exit(primary)):
        # A successful parser exit still requires its help output to finish.
        primary = None
        status = 0
    if primary is not None:
        selected = _cleanup_failure(primary, failures)
        if (selected is not primary and type(selected) is SystemExit
                and successful_exit(selected)):
            return 1
        if selected is not primary:
            raise selected from primary
        raise primary
    if failures:
        selected = _cleanup_failure(failures[0][1], failures[1:])
        if type(selected) is SystemExit and successful_exit(selected):
            return status or 1
        if not isinstance(selected, Exception):
            raise selected
        return status or 1
    return status


if __name__ == '__main__':
    sys.exit(cli())

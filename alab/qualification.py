#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Run the existing native gate and retain a source-bound local observation."""
import argparse
from datetime import datetime, timezone
import hashlib
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
from upstream_intake import git, git_environment

validate_ident = runpy.run_path(str(Path(__file__).resolve().with_name('source_content.py')))['validate_ident']

LIMITS = ('Local gate execution and Git-state observation; not authenticated attestation, '
          'a replacement gate, artifact promotion or live entitlement.')
SCHEMA = 5
MAX_FILTER_CONFIG_BYTES = 64 * 1024


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
            'upstream': git(repo, 'merge-base', source, 'upstream/main'),
            'clean': clean}


def now():
    return datetime.now(timezone.utc).isoformat()


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
    with (directory / 'start.json').open('x') as out:
        json.dump({'schema': SCHEMA, 'before': before, 'command': command,
                   'started_at': started}, out, indent=2)
        out.write('\n')
    if stream is None:
        stream = sys.stdout.buffer
    stream.write(('qualification log: ' + str(directory / 'gate.log') + '\n').encode())
    stream.flush()
    with (directory / 'gate.log').open('xb') as log:
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
        except BaseException:
            process.terminate()
            try:
                process.wait(timeout=30)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            raise
        finally:
            process.stdout.close()
    after = snapshot(repo)
    qualified = status == 0 and before == after and after['clean']
    record = {'schema': SCHEMA, 'kind': 'native-check-execution-observation',
              'before': before, 'after': after, 'command': command,
              'status': status, 'qualified_source': before['source'] if qualified else None,
              'started_at': started, 'completed_at': now(), 'log': 'gate.log',
              'log_sha256': hashlib.sha256((directory / 'gate.log').read_bytes()).hexdigest(),
              'limits': LIMITS}
    with (directory / 'record.json').open('x') as out:
        json.dump(record, out, indent=2)
        out.write('\n')
    stream.write(('qualification record: ' + str(directory / 'record.json') + '\n').encode())
    stream.flush()
    if status:
        return 128 - status if status < 0 else status
    if not qualified:
        stream.write(b'qualification inputs changed; no source qualified\n')
        stream.flush()
    return 0 if qualified else 1


def inspect(repo, path):
    path = Path(path)
    if path.is_symlink() or (path.parent / 'gate.log').is_symlink():
        raise ValueError('qualification record and log must not be symlinks')
    record = json.loads(path.read_text())
    if (not isinstance(record, dict) or type(record.get('schema')) is not int
            or record['schema'] != SCHEMA or record.get('kind') != 'native-check-execution-observation'
            or type(record.get('status')) is not int or record['status'] != 0
            or not isinstance(record.get('before'), dict) or record['before'] != record.get('after')
            or record['before'].get('clean') is not True or record.get('log') != 'gate.log'):
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
    if hashlib.sha256((path.parent / 'gate.log').read_bytes()).hexdigest() != record.get('log_sha256'):
        raise ValueError('qualification log hash changed')
    return {'source': source, 'upstream': record['before'].get('upstream'),
            'completed_at': record.get('completed_at'), 'record': str(path.resolve()),
            'limits': LIMITS}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--directory', type=Path)
    args = parser.parse_args()
    try:
        return run(args.repo, args.directory)
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        print('qualification observation: ' + str(error), file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())

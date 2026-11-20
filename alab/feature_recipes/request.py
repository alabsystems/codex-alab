# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Identify the stable, failed native gate that actually requested a feature bundle."""
import hashlib
import json
from pathlib import Path
import re

from feature_evidence import FILE_HASH, read_record
from feature_gate import admission, bundle_path
from qualification import SCHEMA, worktree_clean
from upstream_intake import git, integrated_upstream


def common_dir(repo):
    return Path(git(repo, 'rev-parse', '--path-format=absolute', '--git-common-dir'))


def context(repo, base):
    source = git(repo, 'rev-parse', 'HEAD^{commit}')
    return dict(source=source, tree=git(repo, 'rev-parse', source + '^{tree}'),
                base=git(repo, 'rev-parse', '--verify', '--end-of-options', base + '^{commit}'),
                upstream=integrated_upstream(repo, source), clean=True)


class NoFeatureRequest(ValueError):
    """The observed gate failed for a different reason."""


def request(repo, record_file=None):
    head = git(repo, 'rev-parse', 'HEAD^{commit}')
    directory = common_dir(repo) / 'alab-qualification' / head
    if record_file is None:
        records = list(directory.glob('run-*/record.json'))
        if not records:
            raise ValueError('no completed native gate for the current source')
        record_file = max(records, key=lambda p: (p.stat().st_mtime_ns, p.name))
    record_file = record_file.absolute()
    if (record_file.name != 'record.json' or record_file.parent.parent != directory
            or record_file.parent.is_symlink() or record_file.is_symlink()):
        raise ValueError('expected the native qualification record for this source')
    record = read_record(record_file)
    if (record.get('schema') != SCHEMA or record.get('kind') != 'native-check-execution-observation'
            or type(record.get('status')) is not int or record['status'] != 1
            or record.get('qualified_source') is not None or record.get('log') != 'gate.log'
            or record.get('command') != [str(repo / 'alab/check.sh')]):
        raise ValueError('expected an unsuccessful native check.sh observation')
    before = record.get('before')
    if not isinstance(before, dict) or not re.fullmatch(r'(?:[a-f0-9]{40}|[a-f0-9]{64})', str(before.get('base'))):
        raise ValueError('native gate has no exact recorded base')
    current = context(repo, before['base'])
    if (not worktree_clean(repo, head) or before != record.get('after') or before != current
            or before.get('clean') is not True or record['after'].get('clean') is not True):
        raise ValueError('source, base, frontier or cleanliness changed since native qualification')
    if (record_file.parent / 'gate.log').is_symlink():
        raise ValueError('native gate log must not be a symlink')
    _metadata, raw = FILE_HASH['file_bytes_snapshot'](record_file.parent / 'gate.log')
    if hashlib.sha256(raw).hexdigest() != record.get('log_sha256'):
        raise ValueError('native gate log digest mismatch')
    log = raw.decode()
    declared = []
    for start in re.finditer(r'(?m)^\{', log):
        try:
            entry, _ = json.JSONDecoder().raw_decode(log[start.start():])
        except ValueError:
            continue
        if isinstance(entry, dict) and entry.get('kind') == 'declared-feature-evidence-admission':
            declared.append(entry)
    source, base, upstream = (current[k] for k in ('source', 'base', 'upstream'))
    dest = bundle_path(repo, source)
    missing = ['complete exact-source feature bundle required at ' + str(dest)]
    if (len(declared) != 1 or declared[0].get('errors') != missing
            or 'alab: feature evidence admission failed; complete the exact-source affected-feature bundle' not in log):
        raise NoFeatureRequest('the native gate did not stop solely for a missing feature bundle')
    actual = admission(repo, base, source, upstream)
    if {**actual, 'errors': missing} != declared[0] or actual['errors'] not in ([], missing):
        raise ValueError('recorded request differs from the candidate feature gate')
    return current, actual, record_file

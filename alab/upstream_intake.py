#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Snapshot and validate upstream review coverage; never fetch, merge or qualify."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys


DECISIONS = {'adopt', 'adapt', 'keep-opt-in', 'superseded', 'defer-activation'}
CLASSES = {'fix', 'security', 'model', 'feature', 'refactor', 'protocol',
           'defaults', 'background', 'telemetry', 'removed-path', 'packaging', 'other'}


def git_environment(*, index_file=None):
    env = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
    env.update(GIT_NO_LAZY_FETCH='1', GIT_NO_REPLACE_OBJECTS='1',
               GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1',
               GIT_OPTIONAL_LOCKS='0')
    if index_file is not None:
        env['GIT_INDEX_FILE'] = str(index_file)
    return env


def git(repo, *args, index_file=None):
    return subprocess.check_output(
        ['git', '-c', 'core.fsmonitor=false', '-C', str(repo), *args],
        env=git_environment(index_file=index_file), stderr=subprocess.PIPE).decode('utf-8', errors='surrogateescape').rstrip('\n')


def snapshot(repo, base, target):
    base = git(repo, 'rev-parse', '--verify', '--end-of-options', base + '^{commit}')
    target = git(repo, 'rev-parse', '--verify', '--end-of-options', target + '^{commit}')
    frontier = git(repo, 'merge-base', base, target)
    # The export is linear today; explicitly refuse a future divergent history.
    previous = frontier
    entries = []
    for oid in git(repo, 'rev-list', '--reverse', base + '..' + target).splitlines():
        parents = git(repo, 'show', '-s', '--format=%P', oid).split()
        if parents != [previous]:
            raise ValueError('upstream range is not linear from its merge base')
        paths = git(repo, 'diff-tree', '--no-commit-id', '--name-only', '-r', '-z', oid)
        entries.append({'sha': oid, 'subject': git(repo, 'show', '-s', '--format=%s', oid),
                        'paths': paths.rstrip('\0').split('\0') if paths else [],
                        'classes': [], 'features': [], 'decision': None,
                        'reason': '', 'evidence': [], 'revisit': ''})
        previous = oid
    return {'schema': 1, 'base': base, 'target': target, 'frontier': frontier,
            'commits': entries}


def validate(record, expected, feature_ids=None):
    errors = []
    for field in ('schema', 'base', 'target', 'frontier'):
        if (record.get(field) != expected[field]
                or field == 'schema' and type(record.get(field)) is not int):
            errors.append('stale or invalid ' + field)
    entries = record.get('commits')
    if not isinstance(entries, list):
        return errors + ['commits must be an ordered list']
    if any(not isinstance(e, dict) for e in entries) or [e.get('sha') for e in entries] != [e['sha'] for e in expected['commits']]:
        return errors + ['commit coverage differs (missing, extra, reordered or duplicate commits)']
    for entry, source in zip(entries, expected['commits']):
        prefix = source['sha'][:12] + ': '
        for field in ('subject', 'paths'):
            if entry.get(field) != source[field]:
                errors.append(prefix + 'changed source ' + field)
        if not isinstance(entry.get('decision'), str) or entry['decision'] not in DECISIONS:
            errors.append(prefix + 'review decision required')
        for field, allowed in (('classes', CLASSES),):
            values = entry.get(field)
            if not isinstance(values, list) or not values or any(not isinstance(v, str) or v not in allowed for v in values):
                errors.append(prefix + 'valid ' + field + ' required')
        for field in ('reason',):
            if not isinstance(entry.get(field), str) or not entry[field].strip():
                errors.append(prefix + field + ' required')
        for field in ('features', 'evidence'):
            values = entry.get(field)
            if not isinstance(values, list) or any(not isinstance(v, str) or not v.strip() for v in values):
                errors.append(prefix + field + ' must be a list of nonempty strings')
        if feature_ids is not None and isinstance(entry.get('features'), list):
            for feature in entry['features']:
                if isinstance(feature, str) and feature not in feature_ids:
                    errors.append(prefix + 'unknown feature ID: ' + feature)
        if not entry.get('evidence'):
            errors.append(prefix + 'review evidence required (does not imply qualification)')
        if entry.get('decision') == 'defer-activation' and (not isinstance(entry.get('revisit'), str) or not entry['revisit'].strip()):
            errors.append(prefix + 'deferred activation needs a revisit trigger')
    return errors


def main():
    import tomllib

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=['snapshot', 'validate'])
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--base', required=True, help='ALab integration base, resolved again on validation')
    parser.add_argument('--target', required=True, help='fetched upstream target, resolved again on validation')
    parser.add_argument('--record', required=True, type=Path)
    args = parser.parse_args()
    try:
        expected = snapshot(args.repo, args.base, args.target)
        if args.operation == 'snapshot':
            # Refuse overwriting a resolver's review decisions.
            with args.record.open('x') as output:
                json.dump(expected, output, indent=2, ensure_ascii=True)
                output.write('\n')
            print(f"snapshotted {len(expected['commits'])} commits; review pending")
        else:
            record = json.loads(args.record.read_text())
            if not isinstance(record, dict):
                raise ValueError('record must be an object')
            with (args.repo / 'alab/features.toml').open('rb') as source:
                inventory = tomllib.load(source)
            if type(inventory.get('schema')) is not int or inventory.get('schema') != 1:
                raise ValueError('unsupported feature inventory schema')
            feature_ids = {feature['id'] for feature in inventory['features']}
            errors = validate(record, expected, feature_ids)
            if errors:
                print('\n'.join(errors), file=sys.stderr)
                return 1
            print(f"review coverage complete: {len(expected['commits'])} commits; qualification is separate")
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print('intake: ' + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())

#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Snapshot and validate upstream review coverage; never fetch, merge or qualify."""
import argparse
import json
import os
from pathlib import Path
import re
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


def git(repo, *args, index_file=None, input_bytes=None):
    output = subprocess.check_output(
        ['git', '-c', 'core.fsmonitor=false', '-C', str(repo), *args],
        env=git_environment(index_file=index_file), input=input_bytes, stderr=subprocess.PIPE)
    return output.decode('utf-8', errors='surrogateescape').rstrip('\n')


RELEASE_DESCRIPTOR = 'alab/upstream-release'
UPSTREAM_MAIN = 'refs/remotes/upstream/main'


def unique_frontier(repo, source, upstream):
    bases = git(repo, 'merge-base', '--all', source, upstream).splitlines()
    if len(bases) != 1:
        raise ValueError('candidate has no unambiguous integrated upstream frontier')
    return bases[0]


def pinned_release(repo, source):
    """A marked release leaf qualifies against its committed official annotated tag."""
    entry = git(repo, 'ls-tree', '-z', source, '--', RELEASE_DESCRIPTOR)
    if not entry:
        return None
    blob = re.fullmatch(r'100644 blob ([a-f0-9]+)\t' + re.escape(RELEASE_DESCRIPTOR) + r'\x00', entry)
    if not blob:
        raise ValueError('upstream release descriptor must be a regular committed file')
    size = int(git(repo, 'cat-file', '-s', blob[1]))
    if size > 256:
        raise ValueError('invalid committed upstream release descriptor')
    data = git(repo, 'cat-file', 'blob', blob[1])
    version = r'(?:0|[1-9][0-9]*)'
    oid = r'([0-9a-f]{40}|[0-9a-f]{64})'
    fields = re.fullmatch(r'(rust-v' + r'\.'.join([version] * 3) + ') ' + oid + ' ' + oid, data)
    if not fields or len(data) + 1 != size:
        raise ValueError('invalid committed upstream release descriptor')
    tag, annotation, commit = fields.groups()
    ref = 'refs/upstream-tags/' + tag
    fmt = '%(refname) %(objecttype) %(objectname) %(*objecttype) %(*objectname) %(tag)'
    if git(repo, 'for-each-ref', '--format=' + fmt, ref) != f'{ref} tag {annotation} commit {commit} {tag}':
        raise ValueError('official release tag missing or changed: ' + ref)
    main = git(repo, 'rev-parse', '--verify', UPSTREAM_MAIN + '^{commit}')
    if unique_frontier(repo, source, commit) != commit:
        raise ValueError('candidate does not contain its pinned upstream release')
    if unique_frontier(repo, source, main) != unique_frontier(repo, commit, main):
        raise ValueError('candidate mainline frontier differs from its pinned release branch point')
    return tag, commit


def upstream_target(repo, source, requested=None):
    release = pinned_release(repo, source)
    if release is None:
        return requested or UPSTREAM_MAIN
    _, commit = release
    if requested is not None:
        actual = git(repo, 'rev-parse', '--verify', '--end-of-options', requested + '^{commit}')
        if actual != commit:
            raise ValueError('requested upstream differs from the pinned release')
    return commit


def integrated_upstream(repo, source, requested=None):
    return unique_frontier(repo, source, upstream_target(repo, source, requested))


def snapshot(repo, base, target):
    base = git(repo, 'rev-parse', '--verify', '--end-of-options', base + '^{commit}')
    target = git(repo, 'rev-parse', '--verify', '--end-of-options', target + '^{commit}')
    frontier = git(repo, 'merge-base', base, target)
    # The export is linear today; explicitly refuse a future divergent history.
    previous = frontier
    oids = []
    for line in git(repo, 'rev-list', '--reverse', '--parents', base + '..' + target).splitlines():
        oid, *parents = line.split()
        if parents != [previous]:
            raise ValueError('upstream range is not linear from its merge base')
        oids.append(oid)
        previous = oid

    entries = []
    if oids:
        # Each header starts with NUL. Files are nonempty NUL-terminated names, so
        # two consecutive NULs separate commits even with empty subjects or diffs.
        changes = git(repo, 'diff-tree', '--stdin', '--always', '--format=tformat:%x00%H%x20%s',
                      '--name-only', '-r', '-z', input_bytes=('\n'.join(oids) + '\n').encode('ascii'))
        if not changes.startswith('\0') or not changes.endswith('\0'):
            raise ValueError('incomplete upstream diff-tree output')
        blocks = changes[1:-1].split('\0\0')
        if len(blocks) != len(oids):
            raise ValueError('incomplete upstream diff-tree commit list')
        for oid, block in zip(oids, blocks):
            header, *paths = block.split('\0')
            actual, separator, subject = header.partition(' ')
            if actual != oid or not separator:
                raise ValueError('unexpected upstream diff-tree header')
            if paths:
                # Git puts one formatting newline before the first filename.
                if not paths[0].startswith('\n') or len(paths[0]) == 1:
                    raise ValueError('invalid upstream diff-tree path framing')
                paths[0] = paths[0][1:]
            entries.append({'sha': oid, 'subject': subject, 'paths': paths, 'classes': [],
                            'features': [], 'decision': None, 'reason': '', 'evidence': [], 'revisit': ''})
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


def catalog_feature_ids(inventory):
    """Validate the feature IDs consumed by intake, status and admission readers."""
    if (not isinstance(inventory, dict) or type(inventory.get('schema')) is not int
            or inventory['schema'] != 1):
        raise ValueError('unsupported feature inventory schema')
    features = inventory.get('features')
    if not isinstance(features, list):
        raise ValueError('feature inventory features must be a list')
    identities = set()
    for feature in features:
        if not isinstance(feature, dict):
            raise ValueError('feature must be a table')
        identity = feature.get('id')
        if not isinstance(identity, str) or not identity.strip():
            raise ValueError('feature ID required')
        if identity in identities:
            raise ValueError('duplicate feature ID: ' + identity)
        identities.add(identity)
    return identities


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=['snapshot', 'validate', 'frontier', 'pinned-release'])
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--source', default='HEAD')
    parser.add_argument('--base', help='ALab integration base, resolved again on validation')
    parser.add_argument('--target', help='fetched upstream target, resolved again on validation')
    parser.add_argument('--record', type=Path)
    args = parser.parse_args()
    try:
        if args.operation in ('frontier', 'pinned-release'):
            source = git(args.repo, 'rev-parse', '--verify', '--end-of-options', args.source + '^{commit}')
            if args.operation == 'frontier':
                print(integrated_upstream(args.repo, source))
            elif release := pinned_release(args.repo, source):
                print(*release)
            return 0
        if not (args.base and args.target and args.record):
            parser.error('--base, --target and --record are required for ' + args.operation)
        expected = snapshot(args.repo, args.base, args.target)
        if args.operation == 'snapshot':
            # Refuse overwriting a resolver's review decisions.
            with args.record.open('x') as output:
                json.dump(expected, output, indent=2, ensure_ascii=True)
                output.write('\n')
            print(f"snapshotted {len(expected['commits'])} commits; review pending")
        else:
            import tomllib

            record = json.loads(args.record.read_text())
            if not isinstance(record, dict):
                raise ValueError('record must be an object')
            with (args.repo / 'alab/features.toml').open('rb') as source:
                inventory = tomllib.load(source)
            feature_ids = catalog_feature_ids(inventory)
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

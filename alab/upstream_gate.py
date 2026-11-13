#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Admit newly integrated upstream commits only with committed review coverage.

This checks source identities and declared decisions, not their semantic quality,
runtime qualification, remote publication or installed adoption.
"""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib

sys.path.insert(0, str(Path(__file__).resolve().parent))
from upstream_intake import git, snapshot, validate


RECORD = re.compile(r'alab/upstream/[A-Za-z0-9][A-Za-z0-9._-]*-intake\.json')


def admission(repo, base, source='HEAD', upstream='upstream/main'):
    source = git(repo, 'rev-parse', '--verify', '--end-of-options', source + '^{commit}')
    upstream = git(repo, 'rev-parse', '--verify', '--end-of-options', upstream + '^{commit}')
    frontiers = git(repo, 'merge-base', '--all', source, upstream).splitlines()
    if len(frontiers) != 1:
        raise ValueError('candidate has no unambiguous integrated upstream frontier')
    expected = snapshot(repo, base, frontiers[0])
    wanted = {entry['sha'] for entry in expected['commits']}
    result = {'schema': 1, 'kind': 'committed-upstream-review-admission',
              'base': expected['base'], 'source': source,
              'integrated_upstream': expected['target'],
              'commits': len(wanted), 'reviewed': 0, 'missing': [], 'errors': [],
              'affected_features': [],
              'limits': 'Committed review declarations only; semantic review quality, '
                        'runtime qualification, publication and adoption remain separate.'}
    if not wanted:
        return result
    catalog = tomllib.loads(git(repo, 'show', source + ':alab/features.toml'))
    if type(catalog.get('schema')) is not int or catalog.get('schema') != 1:
        raise ValueError('unsupported feature inventory schema')
    feature_ids = {feature['id'] for feature in catalog['features']}
    names = git(repo, 'ls-tree', '-r', '--name-only', '-z', source, '--', 'alab/upstream')
    records = {}
    for name in names.rstrip('\0').split('\0'):
        if not name.endswith('-intake.json'):
            continue
        if not RECORD.fullmatch(name):
            raise ValueError('unsupported committed intake path')
        raw = git(repo, 'show', source + ':' + name)
        if len(raw.encode('utf-8', errors='surrogateescape')) > 8 * 1024 * 1024:
            raise ValueError('oversized committed intake: ' + name)
        record = json.loads(raw)
        if (not isinstance(record, dict) or not isinstance(record.get('commits'), list)
                or any(not isinstance(entry, dict) for entry in record['commits'])):
            raise ValueError('invalid committed intake: ' + name)
        records[name] = record
    seen, reviewed, affected = set(), set(), set()
    for name, record in records.items():
        # Historical batches are not revalidated at every future sync. A batch
        # covering a stepwise prefix is still validated against its whole source.
        if not any(entry.get('sha') in wanted for entry in record['commits']):
            continue
        if type(record.get('schema')) is not int or record.get('schema') != 1:
            result['errors'].append(name + ': unsupported intake record schema')
            continue
        if 'superseded_by' in record:
            replacement_name = record['superseded_by']
            replacement = records.get(replacement_name) if isinstance(replacement_name, str) else None
            reason = record.get('supersession_reason')
            relevant = {entry.get('sha') for entry in record['commits']} & wanted
            if (replacement is None or 'superseded_by' in replacement
                    or not isinstance(reason, str) or not reason.strip()
                    or not relevant <= {entry.get('sha') for entry in replacement['commits']}):
                result['errors'].append(name + ': supersession needs an active committed replacement covering this range and a reason')
            # The replacement still has to pass its own immutable-source and
            # decision checks. Supersession cannot fill a missing review.
            continue
        actual = snapshot(repo, record['base'], record['target'])
        fields = ('schema', 'base', 'target', 'frontier')
        immutable = lambda batch: [{key: entry.get(key) for key in ('sha', 'subject', 'paths')}
                                   for entry in batch['commits']]
        if (any(record.get(field) != actual[field] for field in fields)
                or immutable(record) != immutable(actual)):
            result['errors'].append(name + ': stale or invalid immutable source coverage')
            continue
        findings = validate(record, actual, feature_ids)
        for entry in record['commits']:
            identity = entry['sha']
            if identity not in wanted:
                continue
            if identity in seen:
                result['errors'].append(name + ': duplicate coverage: ' + identity)
                reviewed.discard(identity)
                continue
            seen.add(identity)
            invalid = [error for error in findings if error.startswith(identity[:12] + ': ')]
            if invalid:
                result['errors'].extend(name + ': ' + error for error in invalid)
            else:
                reviewed.add(identity)
                affected.update(entry['features'])
    result['reviewed'] = len(reviewed)
    result['missing'] = sorted(wanted - reviewed)
    # An incomplete review cannot supply an authoritative qualification scope.
    if not result['missing'] and not result['errors']:
        result['affected_features'] = sorted(affected)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--base', required=True)
    parser.add_argument('--source', default='HEAD')
    parser.add_argument('--upstream', default='upstream/main')
    args = parser.parse_args()
    try:
        result = admission(args.repo, args.base, args.source, args.upstream)
    except (OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        print('upstream review admission: ' + str(error), file=sys.stderr)
        return 1
    print(json.dumps(result, indent=2))
    return 1 if result['errors'] or result['missing'] else 0


if __name__ == '__main__':
    sys.exit(main())

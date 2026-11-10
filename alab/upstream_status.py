#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Report locally observed upstream lag without fetching or inferring qualification."""
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import subprocess
import sys

from upstream_intake import git


def report(repo, main='origin/main', upstream='upstream/main', now=None):
    now = now or datetime.now(timezone.utc)
    result = {
        'schema': 1, 'observed_at': now.isoformat(),
        'main_ref': main, 'upstream_ref': upstream,
        'main_sha': None, 'upstream_sha': None, 'integrated_frontier_sha': None,
        'pending_count': None, 'oldest_pending_committed_at': None,
        'oldest_pending_age_seconds': None,
        # Ref timestamps are not fetch timestamps, and ancestry is not proof.
        'last_successful_fetch_at': None, 'qualified_sha': None,
        'packaged_sha': None, 'selected_sha': None, 'running_sha': None,
        'unknown_fields_reason': 'No authenticated fetch, qualification or adoption evidence is read by this local report.',
        'errors': [],
    }
    for field, ref in (('main_sha', main), ('upstream_sha', upstream)):
        try:
            result[field] = git(repo, 'rev-parse', '--verify', '--end-of-options', ref + '^{commit}')
        except subprocess.CalledProcessError:
            result['errors'].append('Cannot resolve ' + ref)
    if not result['main_sha'] or not result['upstream_sha']:
        return result
    base, target = result['main_sha'], result['upstream_sha']
    frontiers = git(repo, 'merge-base', '--all', base, target).splitlines()
    if len(frontiers) != 1:
        result['errors'].append('Integration frontier is ambiguous')
        return result
    result['integrated_frontier_sha'] = frontiers[0]
    pending = git(repo, 'rev-list', base + '..' + target).splitlines()
    result['pending_count'] = len(pending)
    if pending:
        # Use the oldest actual timestamp, not rev-list traversal ordering.
        timestamp = min(map(int, git(repo, 'log', '--format=%ct', base + '..' + target).splitlines()))
        result['oldest_pending_committed_at'] = datetime.fromtimestamp(timestamp, timezone.utc).isoformat()
        result['oldest_pending_age_seconds'] = max(0, int(now.timestamp()) - timestamp)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--main', default='origin/main')
    parser.add_argument('--upstream', default='upstream/main')
    args = parser.parse_args()
    try:
        result = report(args.repo, args.main, args.upstream)
        print(json.dumps(result, indent=2))
        return 1 if result['errors'] else 0
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print('upstream status: ' + str(error), file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())

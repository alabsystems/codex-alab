#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Read-only source/intake status against explicit local Git observations.

Does not fetch, run qualification or verify a remote server. Optional explicit
artifact/process inputs inspect local adoption independently of source ancestry.
"""
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import subprocess
import sys
import tomllib

# Resolve only this tool's owned sibling, including under Python's isolated mode.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from upstream_intake import git, snapshot, validate
from artifact_status import observe as artifact_observation
from qualification import inspect as qualification_observation


def report(repo, main='origin/main', upstream='upstream/main', now=None, *,
           package=None, selected_executable=None, running_pid=None, qualification_record=None):
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
    if any(value is not None for value in (package, selected_executable, running_pid)):
        adoption = artifact_observation(package, selected_executable, running_pid)
        for field in ('packaged_sha', 'selected_sha', 'running_sha'):
            result[field] = adoption[field]
        result['adoption'] = adoption
        result['errors'].extend(adoption['errors'])
        result['unknown_fields_reason'] = (
            'Fetch time and qualification are not observed; adoption fields use only '
            'explicit local artifact/executable/PID inputs. Unrequested stages remain unknown.')
    if qualification_record is not None:
        try:
            observation = qualification_observation(repo, qualification_record)
            result['qualification_observation'] = observation
            result['qualified_sha'] = observation['source']
        except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
            result['errors'].append('qualification: ' + str(error))
        result['unknown_fields_reason'] = (
            'Fetch freshness is not observed; qualification and adoption use only explicit '
            'local inputs. A local gate record is not an authenticated execution attestation.')
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


def contains(repo, source, target):
    try:
        return git(repo, 'merge-base', source, target) == target
    except subprocess.CalledProcessError as error:
        if error.returncode != 1:
            raise
        return False


def observe(repo, base, target, candidate, main, records, feature_ids):
    expected = snapshot(repo, base, target)
    candidate = git(repo, 'rev-parse', '--verify', '--end-of-options', candidate + '^{commit}')
    main = git(repo, 'rev-parse', '--verify', '--end-of-options', main + '^{commit}')
    wanted = {entry['sha'] for entry in expected['commits']}
    seen, reviewed, errors = set(), set(), []
    for name, record in records:
        actual = snapshot(repo, record['base'], record['target'])
        findings = validate(record, actual, feature_ids)
        # Invalid immutable metadata invalidates this record, not just one decision.
        immutable = {key: record.get(key) for key in ('schema', 'base', 'target', 'frontier')}
        source_fields = lambda entries: [
            {key: entry.get(key) for key in ('sha', 'subject', 'paths')} for entry in entries]
        if (type(record.get('schema')) is not int
                or immutable != {key: actual[key] for key in immutable}
                or not isinstance(record.get('commits'), list)
                or any(not isinstance(entry, dict) for entry in record['commits'])
                or source_fields(record['commits']) != source_fields(actual['commits'])):
            errors.append(name + ': stale or invalid immutable source coverage')
            continue
        for entry in record['commits']:
            identity = entry['sha']
            if identity not in wanted:
                errors.append(name + ': commit outside requested range: ' + identity)
                continue
            if identity in seen:
                errors.append(name + ': duplicate coverage: ' + identity)
                reviewed.discard(identity)
                continue
            seen.add(identity)
            invalid = [error for error in findings if error.startswith(identity[:12] + ': ')]
            if entry.get('decision') is not None:
                if invalid:
                    errors.extend(name + ': ' + error for error in invalid)
                else:
                    reviewed.add(identity)
    missing = sorted(wanted - seen)
    return {
        'schema': 1,
        'kind': 'local-source-and-intake-observation',
        'base': expected['base'], 'upstream': expected['target'],
        'candidate': candidate, 'origin_tracking_main': main,
        'commits': len(wanted), 'reviewed': len(reviewed),
        'pending': len(wanted - reviewed), 'missing': missing, 'errors': errors,
        'intake_complete': not errors and reviewed == wanted,
        'candidate_contains_target': contains(repo, candidate, expected['target']),
        'origin_tracking_contains_target': contains(repo, main, expected['target']),
        'qualification': 'unproven', 'installed_adoption': 'unproven',
        'limits': 'Local refs only; no fresh fetch or remote-server observation. '
                  'Decision validity is not semantic review or execution evidence. '
                  'Source ancestry does not prove qualification or installed adoption.',
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--base', help='Enable review coverage over this explicit integration range')
    parser.add_argument('--upstream', default='upstream/main')
    parser.add_argument('--candidate', default='HEAD')
    parser.add_argument('--main', default='origin/main')
    parser.add_argument('--intake', type=Path, action='append', default=[])
    parser.add_argument('--package', type=Path, help='inspect this explicit packaged artifact')
    parser.add_argument('--selected-executable', type=Path,
                        help='inspect this explicit executable pointer without executing it')
    parser.add_argument('--running-pid', type=int, help='inspect this local Codex process on macOS')
    parser.add_argument('--qualification-record', type=Path,
                        help='inspect a retained qualification.py gate record and its log')
    args = parser.parse_args()
    try:
        if args.intake and not args.base:
            raise ValueError('--intake requires an explicit --base')
        result = report(args.repo, args.main, args.upstream, package=args.package,
                        selected_executable=args.selected_executable, running_pid=args.running_pid,
                        qualification_record=args.qualification_record)
        if (args.base and result['main_sha'] and result['upstream_sha']
                and result['integrated_frontier_sha']):
            candidate = git(args.repo, 'rev-parse', '--verify', '--end-of-options',
                            args.candidate + '^{commit}')
            features = tomllib.loads(git(args.repo, 'show', candidate + ':alab/features.toml'))
            if type(features.get('schema')) is not int or features.get('schema') != 1:
                raise ValueError('unsupported feature inventory schema')
            records = [(str(path), json.loads(path.read_text())) for path in args.intake]
            result['assessment'] = observe(
                args.repo, args.base, result['upstream_sha'], candidate,
                result['main_sha'], records, {entry['id'] for entry in features['features']})
    except (OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        print('upstream status: ' + str(error), file=sys.stderr)
        return 1
    print(json.dumps(result, indent=2))
    # Successful observation may report incomplete work. This is not an admission gate.
    return 1 if result['errors'] or result.get('assessment', {}).get('errors') else 0


if __name__ == '__main__':
    sys.exit(main())

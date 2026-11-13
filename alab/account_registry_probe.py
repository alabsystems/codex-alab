#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Read aterm's registry associations; never enroll or authenticate an account."""
import argparse
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import subprocess
import sys


QUERIES = {
    'machines': ['aterm', 'db', 'computers'],
    'accounts': ['aterm', 'db', 'table', 'scan', 'alab/registry', 'account', '--json'],
    'preferences': ['aterm', 'db', 'alab', 'preferences'],
    'observations': ['aterm', 'db', 'table', 'scan', 'alab/registry', 'account-obs', '--json'],
}
KEY = re.compile(r'(?:mc|ac)-[0-9a-f]{16}\Z')
SEAT = re.compile(r'[0-9a-f]{32}\Z')
LIMITS = [
    'These are local replicated registry observations, not fresh provider authentication.',
    'Preferences, retained observations and placements establish associations, not login usability.',
    'Commands are separate reads, not an atomic registry snapshot.',
]


def query(command):
    receipt = {'command': command}
    try:
        completed = subprocess.run(command, capture_output=True, text=True, timeout=20)
        receipt['exit_code'] = completed.returncode
        if completed.returncode:
            receipt['error'] = 'command-failed'
            return None, receipt
        value = json.loads(completed.stdout)
        if not isinstance(value, dict):
            raise ValueError('object required')
        return value, receipt
    except subprocess.TimeoutExpired:
        receipt['error'] = 'timeout'
    except OSError:
        receipt['error'] = 'unavailable'
    except (ValueError, UnicodeError):
        receipt['error'] = 'invalid-json'
    return None, receipt


def text(value):
    return isinstance(value, str) and bool(value) and not any(ord(c) < 32 for c in value)


def key(value, prefix):
    return text(value) and value.startswith(prefix + '-') and KEY.fullmatch(value) is not None


def rows(value, field, complete=True):
    if not isinstance(value, dict) or (complete and value.get('complete') is not True):
        raise ValueError('incomplete-view')
    result = value.get(field)
    if not isinstance(result, list) or not all(isinstance(row, dict) for row in result):
        raise ValueError('invalid-rows')
    return result


def analyze(machine, expected, data, details):
    """Only selected metadata is retained; source notes and address fields never escape."""
    machine_rows = rows(data.get('machines'), 'computers')
    # The native view appends seats with no registry machine yet. They cannot
    # match a requested label/key and are not malformed registered machines.
    unbound = [row for row in machine_rows if set(row) == {'mc', 'seats'}
               and row['mc'] is None and isinstance(row['seats'], list)
               and all(text(s) and SEAT.fullmatch(s) for s in row['seats'])]
    machines = [row for row in machine_rows if row not in unbound]
    accounts = rows(data.get('accounts'), 'rows', complete=False)
    if (data['accounts'].get('collection') != 'account'
            or data['accounts'].get('malformed') != 0):
        raise ValueError('invalid-account-collection')
    preferences = rows(data.get('preferences'), 'preferences')
    observations = rows(data.get('observations'), 'rows', complete=False)
    if data['observations'].get('collection') != 'account-obs':
        raise ValueError('invalid-observation-collection')
    if data['observations'].get('malformed', 0) != 0:
        raise ValueError('malformed-observations')
    lifecycle = ('live', 'retired', 'ended', 'merged', 'locked')
    ids = set()
    for row in machines:
        if (not key(row.get('mc'), 'mc') or row['mc'] in ids
                or not text(row.get('label')) or row.get('row') not in lifecycle
                or (row.get('merged_into') is not None
                    and not key(row['merged_into'], 'mc'))):
            raise ValueError('invalid-machine')
        ids.add(row['mc'])
    selected = [row for row in machines if row['row'] == 'live'
                and not row.get('merged_into') and machine in (row['mc'], row['label'])]
    by_account = {}
    for row in accounts:
        account = row.get('pk')
        if (not key(account, 'ac') or account in by_account
                or not text(row.get('provider')) or row.get('row') not in lifecycle
                or row.get('status') not in ('pending', 'active', 'paused', 'broken', 'banned')
                or (row.get('label') is not None and not text(row['label']))):
            raise ValueError('invalid-account')
        by_account[account] = row
    for row in preferences:
        if (not key(row.get('mc'), 'mc') or not key(row.get('ac'), 'ac')
                or row.get('row') not in lifecycle):
            raise ValueError('invalid-preference')
    for observation in observations:
        row = observation.get('row')
        seat = observation.get('machine')
        if (not text(seat) or not SEAT.fullmatch(seat)
                or not isinstance(row, dict) or not key(row.get('account'), 'ac')
                or observation.get('pk') != row['account'] + '.' + seat
                or row.get('state') not in ('running', 'idle', 'signed-out')
                or not text(row.get('cli')) or type(row.get('at')) is not int
                or not 0 <= row['at'] <= 253402300799000):
            raise ValueError('invalid-observation')
    reports = []
    for host in selected:
        seats = host.get('seats')
        if (not isinstance(seats, list) or not all(text(s) and SEAT.fullmatch(s) for s in seats)
                or len(set(seats)) != len(seats)):
            raise ValueError('invalid-machine-seats')
        detail = details.get(host['mc'])
        # These are separate reads; a rename or retirement invalidates the
        # selected live/name binding even when the stable key still resolves.
        if (not isinstance(detail, dict) or detail.get('complete') is not True
                or detail.get('mc') != host['mc']
                or detail.get('label') != host['label'] or detail.get('row') != 'live'):
            raise ValueError('invalid-machine-details')
        # Both native views derive these UUIDs from the same machine-facts binding.
        # A changed binding cannot join an earlier seat to a later account observation.
        detail_seats = [row.get('uuid') for row in rows(detail, 'facts')]
        if (not all(text(s) and SEAT.fullmatch(s) for s in detail_seats)
                or len(set(detail_seats)) != len(detail_seats) or set(detail_seats) != set(seats)):
            raise ValueError('invalid-machine-seats')
        links = {}
        retained = {}

        def add(account, source):
            row = by_account.get(account)
            if row is None:
                raise ValueError('dangling-account-reference')
            if row['row'] == 'live' and row['provider'] == 'openai':
                links.setdefault(account, set()).add(source)

        for row in preferences:
            if row.get('mc') == host['mc']:
                if row['row'] == 'live':
                    add(row.get('ac'), 'preference')
        for observation in observations:
            if observation.get('machine') in seats:
                row = observation.get('row')
                account = row['account']
                add(account, 'retained-observation')
                if account in links:
                    retained.setdefault(account, set()).add((
                        observation['machine'], row['at'], row['cli'], row['state']))
        for row in rows(detail, 'places'):
            add(row.get('ac'), 'placement')
        # The detailed view independently exposes preferences, including those
        # that may have appeared between the separate metadata reads.
        for row in rows(detail, 'prefers'):
            add(row.get('ac'), 'machine-preference')
        reports.append({
            'machine_key': host['mc'], 'label': host['label'],
            'openai_account_count': len(links),
            'accounts': [{'account_key': account, 'label': by_account[account].get('label'),
                          'status': by_account[account]['status'],
                          'sources': sorted(sources),
                          'observations': [
                              {'seat': seat, 'cli': cli, 'state': state, 'observed_at_ms': at}
                              for seat, at, cli, state in sorted(retained.get(account, []))
                          ]} for account, sources in sorted(links.items())],
        })
    if not reports:
        status = 'machine-not-found'
    elif len(reports) != 1:
        status = 'ambiguous-machine'
    elif reports[0]['openai_account_count'] < expected:
        status = 'insufficient-accounts'
    else:
        status = 'matched'
    return {'status': status, 'proven': status == 'matched', 'machines': reports,
            'unbound_seat_count': sum(len(row['seats']) for row in unbound)}


def probe(machine, expected, reader=query):
    report = {'schema': 1, 'checked_at_utc': datetime.now(timezone.utc).isoformat(),
              'requested_machine': machine, 'expect_at_least': expected,
              'status': 'unavailable', 'proven': False, 'machines': [],
              'commands': [], 'errors': [], 'limits': LIMITS}
    with ThreadPoolExecutor(max_workers=len(QUERIES)) as executor:
        results = list(executor.map(reader, QUERIES.values()))
    data = {}
    for name, (value, receipt) in zip(QUERIES, results):
        report['commands'].append(receipt)
        data[name] = value
    if any(receipt.get('error') for _, receipt in results):
        report['errors'].append('metadata-query-failed')
        return report
    try:
        selected = [r for r in rows(data['machines'], 'computers')
                    if r.get('row') == 'live' and not r.get('merged_into')
                    and machine in (r.get('mc'), r.get('label'))]
        details = {}
        for host in selected:
            if not key(host.get('mc'), 'mc'):
                raise ValueError('invalid-machine')
            value, receipt = reader(['aterm', 'db', 'alab', 'machines', host['mc']])
            report['commands'].append(receipt)
            if receipt.get('error'):
                raise ValueError('machine-query-failed')
            details[host['mc']] = value
        report.update(analyze(machine, expected, data, details))
    except (ValueError, TypeError, KeyError) as error:
        # Errors are constant classifications, never provider output or raw rows.
        report['errors'].append(str(error) if isinstance(error, ValueError) else 'invalid-metadata')
    return report


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--machine', required=True, help='exact registry label or stable mc- key')
    parser.add_argument('--expect-at-least', type=int, default=2)
    parser.add_argument('--output', type=Path, help='create a new private JSON file; never overwrite')
    args = parser.parse_args(argv)
    if not text(args.machine) or len(args.machine) > 128 or args.expect_at_least < 1:
        parser.error('a nonempty machine label/key and positive count are required')
    report = probe(args.machine, args.expect_at_least)
    encoded = json.dumps(report, indent=2) + '\n'
    if args.output:
        try:
            fd = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(fd, 'w') as output:
                output.write(encoded)
        except OSError:
            print('Cannot create the requested new evidence file.', file=sys.stderr)
            return 2
    print(encoded, end='')
    return 0 if report['proven'] else 2 if report['status'] in ('ambiguous-machine', 'unavailable') else 1


if __name__ == '__main__':
    sys.exit(main())

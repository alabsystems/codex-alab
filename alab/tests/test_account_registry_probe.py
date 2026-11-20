# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import copy
import io
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import account_registry_probe as PROBE


M1, M2 = 'mc-' + '1' * 16, 'mc-' + '2' * 16
A1, A2, A3 = ('ac-' + ch * 16 for ch in 'abc')
S1, S2 = '1' * 32, '2' * 32
ADDRESS_SCAN = ['aterm', 'db', 'table', 'scan', 'alab/registry', 'address', '--json']
MISSING_CELL = object()


def fixture():
    return {
        'machines': {'complete': True, 'state': 'live', 'why': None, 'computers': [
            {'mc': M1, 'label': 'm3', 'row': 'live', 'seats': [S1]}]},
        'canonical': {'complete': True, 'machines': [
            {'mc': M1, 'label': 'm3', 'row': 'live', 'uuids': [S1]}]},
        'accounts': {'collection': 'account', 'malformed': 0, 'rows': [
            {'pk': A1, 'provider': 'openai', 'label': 'first', 'row': 'live',
             'status': 'active', 'note': 'PRIVATE-NOTE', 'login': 'PRIVATE-ADDRESS'},
            {'pk': A2, 'provider': 'openai', 'label': 'second', 'row': 'live',
             'status': 'pending'},
            {'pk': A3, 'provider': 'anthropic', 'label': 'third', 'row': 'live',
             'status': 'active'}]},
        'preferences': {'complete': True, 'preferences': [
            {'mc': M1, 'ac': A1, 'row': 'live'}]},
        'observations': {'collection': 'account-obs', 'rows': [
            {'machine': S1, 'pk': A1 + '.' + S1,
             'row': {'account': A1, 'at': 10, 'cli': 'codex', 'state': 'idle'}},
            {'machine': S1, 'pk': A2 + '.' + S1,
             'row': {'account': A2, 'at': 10, 'cli': 'codex', 'state': 'signed-out'}}]},
    }


def details():
    return {M1: {'complete': True, 'mc': M1, 'label': 'm3', 'row': 'live',
                 'facts': [{'uuid': S1}], 'places': [{'ac': A1}], 'prefers': [{'ac': A1}]}}


def reader_for(data, view):
    replies = {tuple(command): data[name] for name, command in PROBE.QUERIES.items()}
    replies.update({('aterm', 'db', 'alab', 'machines', machine): detail
                    for machine, detail in view.items()})

    def reader(command):
        return copy.deepcopy(replies[tuple(command)]), {'command': command, 'exit_code': 0}

    return reader


def published_machine(mc=M1, seats=(S1,), rev=1):
    cells = {'here': 'yes', 'seats': ', '.join(seats), 'label': 'private-machine@example.invalid'}
    return {'file': 'lab/machines', 'state': 'live', 'why': None, 'complete': True,
            'columns': ['label', 'here', 'seats', 'pk'], 'rev': rev,
            'rows': [{'pk': mc, 'live': True, 'cells': cells, 'values': dict(cells)}]}


def here_reader(base, *snapshots):
    snapshots = iter(snapshots)

    def read(cmd):
        if cmd == ['aterm', 'file', 'cat', 'lab/machines', '--published', '--json']:
            snapshot = next(snapshots)
            if snapshot is None:
                return None, {'command': cmd, 'error': 'command-failed', 'exit_code': 1}
            return copy.deepcopy(snapshot), {'command': cmd, 'exit_code': 0}
        return base(cmd)

    return read


def complete_login_view(noun, entries):
    prefix, fields = ('ad', ('row', 'addr', 'alias_of')) if noun == 'emails' else ('ac', ('row', 'provider', 'login', 'status'))
    shown = []
    for row in entries:
        cells = {prefix: row['pk']}
        for field in fields:
            value = row.get(field)
            if isinstance(value, dict):
                value = '⚠ ' + ' ‖ '.join(candidate or '' for candidate in value['conflict'])
            cells[field] = value
        shown.append(cells)
    return {'table': noun, 'complete': True, 'state': 'live', 'why': None, noun: shown}


def login_field_fixture(field, typed, shown, *, read_number=None, row=-1):
    scan, noun = (ADDRESS_SCAN, 'emails') if field == 'addr' else (PROBE.QUERIES['accounts'], 'ai-accounts')

    def change(cmd, answer, occurrence):
        if read_number is None or read_number == occurrence:
            if cmd == scan:
                answer['rows'][row][field] = typed
            elif cmd[2] == noun:
                if shown is MISSING_CELL:
                    del answer[noun][row][field]
                else:
                    answer[noun][row][field] = shown

    return change


class RegistryProbeTests(unittest.TestCase):
    def test_explicit_working_views_cannot_verify_a_published_machine_association(self):
        for name in ('canonical', 'preferences', 'detail'):
            for source, pending in (('commons_with_local_edits', 1), ('retained_files', 0),
                                    ('held_commons', 0), ('commons', 1)):
                data, view = fixture(), details()
                record = view[M1] if name == 'detail' else data[name]
                record.update(registry_source=source, pending_cells=pending, complete=True)
                with self.subTest(name=name, source=source, pending=pending):
                    report = PROBE.probe('m3', 1, reader_for(data, view))
                    self.assertFalse(report['proven'])
                    self.assertEqual(PROBE.association_for(report, A1),
                                     {'status': 'unverified', 'reason': 'registry-view-unconfirmed'})
        data, view = fixture(), details()
        for record in (data['canonical'], data['preferences'], view[M1]):
            record.update(registry_source='commons', pending_cells=0)
        self.assertTrue(PROBE.probe('m3', 1, reader_for(data, view))['proven'])

    def test_association_projection_keeps_only_the_requested_accounts_source_kinds(self):
        report = PROBE.probe('m3', 1, reader_for(fixture(), details()))
        for chosen in ('m3', M1):
            with self.subTest(selector=chosen):
                report['requested_machine'] = chosen
                self.assertEqual(PROBE.association_for(report, A1), {
                    'status': 'associated',
                    'sources': ['machine-preference', 'placement', 'preference', 'retained-observation']})
                self.assertEqual(PROBE.association_for(report, A2), {
                    'status': 'associated', 'sources': ['retained-observation']})
                self.assertEqual(PROBE.association_for(report, A3), {'status': 'missing'})
        data, view = fixture(), details()
        data['observations']['rows'] = []
        view[M1].update(places=[], prefers=[])
        empty = PROBE.probe('m3', 1, reader_for(data, view))
        self.assertEqual(empty['status'], 'insufficient-accounts')
        self.assertEqual(PROBE.association_for(empty, A1), {'status': 'missing'})

    def test_association_projection_classifies_failures_without_returning_native_fields(self):
        def unverified(reason):
            return {'status': 'unverified', 'reason': reason}

        missing = PROBE.probe('PRIVATE-MACHINE', 1, reader_for(fixture(), details()))
        self.assertEqual(PROBE.association_for(missing, A1), unverified('machine-not-found'))
        for status, reason in [('ambiguous-machine', 'ambiguous-machine'),
                               ('PRIVATE-STATUS', 'registry-unavailable')]:
            self.assertEqual(PROBE.association_for(dict(missing, status=status), A1), unverified(reason))
        receipt = {'error': 'command-failed', 'reason': 'commons-membership-not-ready'}
        unavailable = dict(missing, status='unavailable', errors=['PRIVATE-ERROR'], commands=[receipt])
        self.assertEqual(PROBE.association_for(unavailable, A1), unverified('commons-membership-not-ready'))
        for bad in (None, A1.upper(), 'PRIVATE-KEY', ['PRIVATE-KEY']):
            self.assertEqual(PROBE.association_for(unavailable, bad), unverified('unconfirmed-account-key'))
        receipt['reason'] = 'PRIVATE-NATIVE-DETAIL'
        self.assertEqual(PROBE.association_for(unavailable, A1), unverified('registry-unavailable'))

    def test_association_projection_withholds_positive_and_negative_when_the_report_is_incomplete(self):
        for change in (
            lambda r, m: r.update(proven=False),
            lambda r, m: r.update(errors=['PRIVATE-DETAIL']),
            lambda r, m: r['machines'].append(m),
            lambda r, m: r.update(requested_machine='different machine'),
            lambda r, m: m.pop('accounts'),
            lambda r, m: m.update(openai_account_count=3),
            lambda r, m: m['accounts'].append(m['accounts'][0]),
            lambda r, m: m['accounts'][0].update(account_key='PRIVATE-KEY'),
            lambda r, m: m['accounts'][0].update(sources=[]),
            lambda r, m: m['accounts'][0].update(sources=['PRIVATE-SOURCE']),
        ):
            report = PROBE.probe('m3', 1, reader_for(fixture(), details()))
            change(report, report['machines'][0])
            for candidate in (A1, A3):
                self.assertEqual(PROBE.association_for(report, candidate),
                                 {'status': 'unverified', 'reason': 'registry-unavailable'})

    def test_registered_login_selects_its_unique_account_across_both_reads(self):
        data = fixture()
        first, second = 'ad-' + '1' * 16, 'ad-' + '2' * 16
        names = {'a@example.invalid': first, 'a.2@example.invalid': second}
        data['accounts']['rows'][0].update(login=first, identity=second)
        data['accounts']['rows'][1].update(login=second, identity=first)
        data['accounts']['rows'][2]['login'] = first  # Another provider.
        calls = []

        def run(change=None, view=None):
            calls.clear()
            base = reader_for(data, view or details())
            latest = {}

            def read(cmd):
                calls.append(cmd)
                if cmd == ADDRESS_SCAN:
                    addresses = [{'pk': pk, 'addr': name, 'row': 'live', 'note': 'PRIVATE'}
                                 for name, pk in names.items()]
                    value = {'collection': 'address', 'rows': addresses, 'malformed': 0}
                    receipt = {'command': cmd, 'exit_code': 0}
                elif cmd[2] in ('emails', 'ai-accounts'):
                    value = complete_login_view(cmd[2], latest[cmd[2]])
                    receipt = {'command': cmd, 'exit_code': 0}
                else:
                    value, receipt = base(cmd)
                if change:
                    change(cmd, value, calls.count(cmd))
                if cmd == ADDRESS_SCAN:
                    latest['emails'] = value['rows']
                if cmd == PROBE.QUERIES['accounts']:
                    latest['ai-accounts'] = value['rows']
                return value, receipt
            return PROBE.probe('m3', 2, read, expected_logins=['A.2@example.invalid', 'a@example.invalid'])

        result = run()
        self.assertTrue(result['proven'], result)
        self.assertEqual(result['resolved_login_account_keys'], [A2, A1])
        self.assertNotIn('@', json.dumps(result))
        self.assertNotIn('PRIVATE', json.dumps(result))
        self.assertEqual(len([c for c in calls if c[5:6] == ['address']]), 2)
        for name in ('ai-accounts', 'emails'):
            self.assertEqual(sum(cmd[2] == name for cmd in calls), 2)
        for name in ('machines', 'preferences', 'canonical', 'observations'):
            self.assertEqual(calls.count(PROBE.QUERIES[name]), 2)

        def unwritten_lifecycle(cmd, value, count):
            if cmd == ADDRESS_SCAN or cmd == PROBE.QUERIES['accounts']:
                for row in value['rows']:
                    row.pop('row')
                    row.pop('status', None)

        unwritten = run(unwritten_lifecycle)
        self.assertTrue(unwritten['proven'])
        self.assertIsNone(unwritten['machines'][0]['accounts'][0]['status'])

        def legacy_spelling(cmd, value, count):
            if cmd == ADDRESS_SCAN:
                value['rows'][1]['addr'] = ' A.2@example.INVALID '
                value['rows'].append({'pk': 'ad-' + '3' * 16, 'row': 'retired',
                                      'addr': {'conflict': ['a.2@example.invalid', 'b@example.invalid']}})

        self.assertTrue(run(legacy_spelling)['proven'])
        for kind in ('absent', 'ambiguous', 'legacy-duplicate', 'conflicted-address',
                     'replaced-address', 'retired-address',
                     'wrong-returned-address', 'changed-login', 'ambiguous-login', 'conflicted-login'):
            def change(cmd, value, count):
                if cmd == ADDRESS_SCAN:
                    addr = value['rows'][1]
                    if kind == 'absent':
                        value['rows'].clear()
                    if kind == 'ambiguous':
                        value['rows'].append(dict(addr, pk='ad-' + '3' * 16))
                    if kind == 'legacy-duplicate':
                        value['rows'].append(dict(addr, pk='ad-' + '3' * 16, addr=' A.2@example.INVALID '))
                    if kind == 'conflicted-address':
                        value['rows'].append(dict(addr, pk='ad-' + '3' * 16,
                                                  addr={'conflict': [' A.2@EXAMPLE.invalid', 'b@example.invalid']}))
                    if kind == 'replaced-address' and count == 2:
                        addr['pk'] = 'ad-' + '3' * 16
                    if kind == 'retired-address':
                        addr['row'] = 'retired'
                    if kind == 'wrong-returned-address':
                        addr['addr'] = 'private@example.invalid'
                if cmd == PROBE.QUERIES['accounts']:
                    if kind == 'changed-login' and count == 2:
                        value['rows'][0]['login'], value['rows'][1]['login'] = second, first
                    if kind == 'ambiguous-login':
                        value['rows'].append(dict(value['rows'][1], pk='ac-' + '4' * 16))
                    if kind == 'conflicted-login':
                        value['rows'].append(dict(value['rows'][1], pk='ac-' + '4' * 16,
                                                  login={'conflict': [first, second]}))
            with self.subTest(kind=kind):
                result = run(change)
                self.assertEqual(result['status'], 'unavailable')
                self.assertNotIn('resolved_login_account_keys', result)
                self.assertNotIn('missing_account_keys', result)
                self.assertNotIn('@', json.dumps(result))

        data['observations']['rows'].pop()
        result = run()
        self.assertEqual(result['status'], 'missing-accounts')
        self.assertEqual(result['missing_account_keys'], [A2])

    def test_distinct_accounts_deduplicated_across_all_sources(self):
        result = PROBE.analyze('m3', 2, fixture(), details())
        self.assertTrue(result['proven'])
        host = result['machines'][0]
        self.assertEqual(host['openai_account_count'], 2)
        self.assertEqual(host['machine_facts_seat_count'], 1)
        self.assertEqual(len(host['accounts'][0]['sources']), 4)
        self.assertEqual(host['accounts'][1]['status'], 'pending')
        self.assertNotIn('PRIVATE', json.dumps(result))
        self.assertIn('not fresh provider authentication', ' '.join(PROBE.LIMITS))

    def discover(self, change=None, required=1, *, expected_logins=(), here=None, mail=None, setup=(), machine='m3',
                 aliases=False, calls=None):
        data, view = fixture(), details()
        first, second, other = ('ad-' + ch * 16 for ch in '123')
        retained = 'ac-' + 'd' * 16
        data['accounts']['rows'][0].update(login=first, identity=other, label='canonical@example.invalid')
        data['accounts']['rows'][1].update(login=second, identity=other, label='inbox@example.invalid')
        data['accounts']['rows'].append({'pk': retained, 'provider': 'openai',
                                        'login': other, 'label': 'retained@example.invalid'})
        data['observations']['rows'].append({'pk': retained + '.' + S1, 'machine': S1,
            'row': {'account': retained, 'state': 'idle', 'cli': 'codex', 'at': 10}})
        view[M1]['places'] = [{'ac': A2, 'name': 'inbox@example.invalid'}]
        view[M1]['prefers'] = [{'ac': A1, 'name': 'canonical@example.invalid'}]
        addresses = {'collection': 'address', 'malformed': 0, 'rows': [
            {'pk': first, 'addr': ' FIRST@Example.invalid ', 'alias_of': other},
            {'pk': second, 'addr': 'second@example.invalid', 'mailbox': 'inbox@example.invalid'},
            {'pk': other, 'addr': 'retained@example.invalid'}]}
        if mail is not None:
            addresses['rows'][0]['mailbox'] = 'mb-' + 'a' * 16
            del addresses['rows'][1]['mailbox']
            addresses['rows'][1]['alias_of'] = 'ad-' + '4' * 16
            addresses['rows'].append({'pk': 'ad-' + '4' * 16, 'addr': 'relay@example.invalid'})
        original = reader_for(data, view)
        if calls is None:
            calls = []
        scanned = {'address': {}, 'account': {}}

        def read(cmd):
            calls.append(cmd)
            if cmd == ADDRESS_SCAN:
                answer, receipt = copy.deepcopy(addresses), {'command': cmd, 'exit_code': 0}
            elif cmd[3:5] == ['get', 'alab/registry'] and cmd[5] in scanned:
                row = copy.deepcopy(scanned[cmd[5]][cmd[6]])
                conflicts = [k for k, v in row.items() if isinstance(v, dict) and 'conflict' in v]
                answer = {'collection': cmd[5], 'row': row, 'lifecycle': row.get('row') or 'live',
                          'conflicts': conflicts, 'raw': []}
                receipt = {'command': cmd, 'exit_code': 0}
            elif cmd == ['aterm', 'db', 'alab', 'mail'] and mail is not None:
                answer, receipt = copy.deepcopy(mail), {'command': cmd, 'exit_code': 0}
                for row in answer['addresses']:
                    alias = scanned['address'].get(row['ad'], {}).get('alias_of')
                    # The native summary shows a scalar or null, including a
                    # provisional value for conflicts. Only get reports the dispute.
                    row.setdefault('alias_of', None if isinstance(alias, dict) else alias)
            elif cmd[:6] == ['aterm', 'db', 'table', 'get', 'alab/registry', 'mailbox'] and mail is not None:
                box, = [b for b in mail['mailboxes'] if b['mb'] == cmd[6]]
                row = {'pk': box['mb'], 'label': 'private@detail.invalid'}
                if box['row'] is not None:
                    row['row'] = box['row']
                answer = {'collection': 'mailbox', 'lifecycle': box['row'] or 'live', 'row': row,
                          'conflicts': [], 'raw': []}
                receipt = {'command': cmd, 'exit_code': 0}
            elif cmd[2] in ('emails', 'ai-accounts'):
                noun = cmd[2]
                table = addresses if noun == 'emails' else data['accounts']
                answer = complete_login_view(noun, table['rows'])
                receipt = {'command': cmd, 'exit_code': 0}
            else:
                answer, receipt = original(cmd)
            if change is not None:
                change(cmd, answer, calls.count(cmd))
            if cmd == ADDRESS_SCAN or cmd == PROBE.QUERIES['accounts']:
                scanned[cmd[5]] = {r['pk']: r for r in answer['rows']}
            return answer, receipt

        options = dict(expected_logins=expected_logins, show_logins=not (expected_logins or mail is not None or aliases),
                       show_mail_routes=mail is not None, show_alias_declarations=aliases, setup_accounts=setup)
        if here is not None:
            return PROBE.probe_here(required, here_reader(read, *here), **options)
        return PROBE.probe(machine, required, read, **options)

    def test_setup_checks_only_requested_current_assignments_in_both_registry_views(self):
        def held_other(cmd, view, _):
            if cmd == PROBE.QUERIES['accounts']:
                view['rows'][0]['status'] = 'paused'
                view['rows'][1].pop('status')
            elif cmd[2] == 'ai-accounts':
                view['ai-accounts'][0]['status'] = 'paused'
                view['ai-accounts'][1]['status'] = None

        local = [published_machine(), published_machine()]
        result = self.discover(held_other, here=local, setup=[A2])
        self.assertIs(result['setup_confirmed'], True)
        self.assertTrue(result['proven'])
        self.assertTrue(all('status' not in row for row in result['machines'][0]['accounts']))
        self.assertNotIn('setup_confirmed', self.discover(held_other))
        self.assertFalse(self.discover(held_other, setup=[A1])['setup_confirmed'])
        self.assertTrue(self.discover(setup=[A1, A2])['setup_confirmed'])
        changed_machine = [published_machine(), published_machine(seats=(S2,))]
        self.assertFalse(self.discover(here=changed_machine, setup=[A1])['setup_confirmed'])
        for keys, minimum in [([A3], 2), (['ac-' + 'd' * 16], 2), ([A1], 3)]:
            with self.subTest(keys=keys, minimum=minimum):
                self.assertFalse(self.discover(required=minimum, setup=keys)['setup_confirmed'])

        for which in (1, 2):
            for status in ('paused', 'broken', 'banned', 'suspended', 'cancelled', 'unknown',
                           {'conflict': ['active', 'pending']}, 'pending'):
                with self.subTest(which=which, status=status):
                    def change(cmd, view, occurrence):
                        if cmd == PROBE.QUERIES['accounts'] and occurrence == which:
                            view['rows'][0]['status'] = status
                        elif cmd[2] == 'ai-accounts' and occurrence == which:
                            view['ai-accounts'][0]['status'] = (status if isinstance(status, str)
                                                               else '⚠ active ‖ pending')

                    result = self.discover(change, setup=[A1])
                    self.assertFalse(result['setup_confirmed'])
                    self.assertFalse(result['proven'])
                    self.assertEqual(result['machines'], [])

            for status in ('missing', None, 'pending', '⚠ active ‖ paused'):
                with self.subTest(grid_occurrence=which, grid_status=status):
                    def mismatch(cmd, view, occurrence):
                        if cmd[2] == 'ai-accounts' and occurrence == which:
                            account = view['ai-accounts'][0]
                            if status == 'missing':
                                del account['status']
                            else:
                                account['status'] = status

                    self.assertFalse(self.discover(mismatch, setup=[A1])['setup_confirmed'])

    def test_setup_reads_only_the_selected_account_details_once_per_pass(self):
        for lifecycle in ('live', 'missing', None):
            for status in ('active', 'pending', 'missing', None):
                with self.subTest(lifecycle=lifecycle, status=status):
                    def change(cmd, view, _):
                        if cmd == PROBE.QUERIES['accounts']:
                            row = view['rows'][0]
                            for field, value in (('row', lifecycle), ('status', status)):
                                if value == 'missing':
                                    del row[field]
                                else:
                                    row[field] = value
                        elif cmd[2] == 'ai-accounts':
                            row = view['ai-accounts'][0]
                            row['row'] = None if lifecycle == 'missing' else lifecycle
                            row['status'] = None if status == 'missing' else status

                    calls = []
                    result = self.discover(change, setup=[A1], calls=calls)
                    self.assertTrue(result['setup_confirmed'], result)
                    self.assertEqual([cmd[5:7] for cmd in calls if cmd[3:4] == ['get']], [['account', A1]] * 2)

        calls = []
        self.assertTrue(self.discover(calls=calls)['proven'])
        self.assertFalse(any(cmd[3:4] == ['get'] for cmd in calls))

    def test_setup_requires_settled_account_details_on_each_pass(self):
        for occurrence in (1, 2):
            for mode, field in [('raw', 'absent-status'), ('raw', 'status'), ('conflicts', 'status'),
                                ('changed', 'status'), ('changed', 'row'), ('changed', 'provider'),
                                ('changed', 'login'), ('changed', 'lifecycle'), ('absent-to-null', 'status')]:
                with self.subTest(occurrence=occurrence, mode=mode, field=field):
                    def change(cmd, view, n):
                        if field == 'absent-status' or mode == 'absent-to-null':
                            if cmd == PROBE.QUERIES['accounts']:
                                del view['rows'][0]['status']
                            elif cmd[2] == 'ai-accounts':
                                view['ai-accounts'][0]['status'] = None
                        if cmd[3:7] == ['get', 'alab/registry', 'account', A1] and n == occurrence:
                            if mode in ('raw', 'conflicts'):
                                view[mode].append('status')
                            elif field == 'lifecycle':
                                view[field] = 'retired'
                            elif mode == 'absent-to-null':
                                view['row'][field] = None
                            else:
                                view['row'][field] = 'changed@private.invalid'

                    result = self.discover(change, setup=[A1])
                    self.assertFalse(result['setup_confirmed'], result)
                    self.assertFalse(result['proven'])
                    self.assertEqual(result['machines'], [])
                    self.assertNotIn('@', json.dumps(result))
                    self.assertNotIn(A1, json.dumps(result))

    def test_setup_tracks_the_exact_form_of_an_empty_status_across_passes(self):
        def change(cmd, view, n):
            if cmd == PROBE.QUERIES['accounts']:
                if n == 1:
                    del view['rows'][0]['status']
                else:
                    view['rows'][0]['status'] = None
            elif cmd[2] == 'ai-accounts':
                view['ai-accounts'][0]['status'] = None

        result = self.discover(change, setup=[A1])
        self.assertFalse(result['setup_confirmed'], result)
        self.assertEqual(result['errors'], ['setup-accounts-unconfirmed'])

    def test_setup_ignores_an_unrelated_status_dispute(self):
        def disputed_other(cmd, view, _):
            if cmd == PROBE.QUERIES['accounts']:
                view['rows'][0]['status'] = {'conflict': ['active', 'paused']}
            elif cmd[2] == 'ai-accounts':
                view['ai-accounts'][0]['status'] = '⚠ active ‖ paused'

        self.assertTrue(self.discover(disputed_other, setup=[A2])['setup_confirmed'])
        self.assertFalse(self.discover(disputed_other)['proven'])

    def test_mail_lookups_report_routing_metadata_through_a_corroborated_status_dispute(self):
        partial = self.mail_view()
        partial['addresses'][1]['mailbox'] = None
        for row, candidates, shown in ((0, ['active', 'paused'], '⚠ active ‖ paused'),
                                       (-1, ['suspended', None, 'cancelled'], '⚠ suspended ‖  ‖ cancelled')):
            change = login_field_fixture('status', {'conflict': candidates}, shown, row=row)
            for options in ({'aliases': True}, {'mail': partial}):
                with self.subTest(row=row, mode=next(iter(options))):
                    report = self.discover(change, **options)
                    self.assertTrue(report['proven'], report)
                    accounts = report['machines'][0]['accounts']
                    self.assertEqual([r['account_key'] for r in accounts], [A1, A2])
                    self.assertTrue(all('status' not in r for r in accounts))
                    self.assertNotIn('setup_confirmed', report)
                    if 'aliases' in options:
                        self.assertEqual([r['alias_declaration'] for r in accounts], ['declared', 'absent'])
                    else:
                        self.assertEqual([r['mail_route']['status'] for r in accounts],
                                         ['reported', 'no-effective-route'])
                    for private in ('"conflict"', 'paused', 'suspended', 'cancelled', 'retained@example.invalid'):
                        self.assertNotIn(private, json.dumps(report))

        dispute = {'conflict': ['active', 'paused']}
        change = login_field_fixture('status', dispute, '⚠ active ‖ paused', row=0)
        for options in ({}, {'expected_logins': ['first@example.invalid']}, {'aliases': True, 'setup': [A1]},
                        {'mail': partial, 'setup': [A1]}):
            report = self.discover(change, **options)
            self.assertEqual(report['status'], 'unavailable')
            self.assertEqual(report['machines'], [])
            self.assertFalse(report.get('setup_confirmed'))
        data = fixture()
        data['accounts']['rows'][0]['status'] = dispute
        for options in ({}, {'expected_accounts': [A1]}):
            report = PROBE.probe('m3', 1, reader_for(data, details()), **options)
            self.assertEqual(report['status'], 'unavailable')
            self.assertEqual(report['machines'], [])

    def test_mail_lookups_withhold_malformed_or_uncorroborated_status_disputes_on_either_read(self):
        dispute = {'conflict': ['active', None, 'paused']}
        exact = '⚠ active ‖  ‖ paused'
        for typed, shown in ((dispute, MISSING_CELL), (dispute, None), (dispute, 'paused'),
                             (dispute, '⚠ active ‖ paused'), (dispute, '⚠ paused ‖  ‖ active'),
                             ({**dispute, 'extra': True}, exact), ({'conflict': ['active']}, '⚠ active'),
                             ({'conflict': ['paused', 'paused']}, '⚠ paused ‖ paused'),
                             ({'conflict': ['active', False]}, '⚠ active ‖ '),
                             ({'conflict': ['active', 'private@status.invalid']}, '⚠ active ‖ private@status.invalid'),
                             (exact, exact), ('active', exact), (None, exact),
                             ('private@status.invalid', 'private@status.invalid')):
            for read_number, row in ((1, 0), (1, -1), (2, 0), (2, -1)):
                change = login_field_fixture('status', typed, shown, read_number=read_number, row=row)
                for options in ({'aliases': True}, {'mail': self.mail_view()}):
                    with self.subTest(typed=typed, shown=shown, read=read_number, row=row, mode=next(iter(options))):
                        report = self.discover(change, **options)
                        self.assertEqual(report['status'], 'unavailable')
                        self.assertEqual(report['machines'], [])
                        for private in ('@', 'paused', 'ad-', 'mb-'):
                            self.assertNotIn(private, json.dumps(report))

    def mail_view(self):
        first, second, canonical = ('ad-' + c * 16 for c in '123')
        shared, wrong = ('mb-' + c * 16 for c in 'ab')

        def route(mailbox, how, via):
            return {'mb': mailbox, 'how': how, 'via': via, 'label': 'private@inbox.invalid'}

        def address(pk, box, accounts):
            return {'ad': pk, 'mailbox': box, 'row': 'live',
                    'accounts': [{'ac': acc, 'slug': 'private@slug.invalid'} for acc in accounts]}

        return {'complete': True, 'addresses': [
            address(first, route(shared, 'owner', []), [A1]),
            address(second, route(shared, 'observed', ['ad-' + '4' * 16]), [A2]),
            address(canonical, route(wrong, 'owner', []), [A1, A2]),
            address('ad-' + '4' * 16, route(shared, 'observed', []), [])],
            'mailboxes': [{'mb': pk, 'row': 'live', 'label': 'private@box.invalid'} for pk in (shared, wrong)]}

    def test_alias_declarations_check_only_selected_addresses_without_mail(self):
        first, second = ('ad-' + c * 16 for c in '12')
        for here in (None, (published_machine(), published_machine())):
            with self.subTest(here=bool(here)):
                calls = []
                result = self.discover(aliases=True, here=here, calls=calls)
                self.assertTrue(result['proven'], result)
                self.assertNotIn('check', result)
                self.assertEqual(result['machines'][0]['accounts'], [
                    {'account_key': A1, 'sources': ['machine-preference', 'preference'],
                     'registered_login': 'first@example.invalid', 'alias_declaration': 'declared'},
                    {'account_key': A2, 'sources': ['placement'],
                     'registered_login': 'second@example.invalid', 'alias_declaration': 'absent'},
                ])
                self.assertCountEqual([cmd[6] for cmd in calls if cmd[3:6] == ['get', 'alab/registry', 'address']],
                                      [first, second] * 2)
                self.assertNotIn(['aterm', 'db', 'alab', 'mail'], calls)
                self.assertFalse(any(cmd[3:6] == ['get', 'alab/registry', 'mailbox'] for cmd in calls))
                for private in ('ad-', 'mb-', 'inbox@', 'canonical@', 'retained@'):
                    self.assertNotIn(private, json.dumps(result))

        for result in (self.discover(), self.discover(mail=self.mail_view())):
            self.assertTrue(all('alias_declaration' not in row for row in result['machines'][0]['accounts']))

    def test_alias_declarations_distinguish_clear_dispute_raw_and_missing_detail(self):
        for variant, expected in (('clear', 'absent'), ('dispute', 'disputed'), ('raw', 'unconfirmed'),
                                  ('missing', 'unconfirmed'), ('raw-address', 'unconfirmed')):
            with self.subTest(variant=variant):
                def change(cmd, answer, _):
                    if cmd == ADDRESS_SCAN and variant == 'clear':
                        answer['rows'][1]['alias_of'] = None
                    if variant == 'raw':
                        if cmd == ADDRESS_SCAN:
                            answer['rows'][1]['alias_of'] = 'ad-' + '3' * 16
                        elif cmd[2] == 'emails':
                            answer['emails'][1]['alias_of'] = 'ad-' + '3' * 16
                    if variant == 'dispute':
                        candidates = ['ad-' + '3' * 16, 'ad-' + '4' * 16]
                        if cmd == ADDRESS_SCAN:
                            answer['rows'][1]['alias_of'] = {'conflict': candidates}
                        elif cmd[2] == 'emails':
                            answer['emails'][1]['alias_of'] = '⚠ ' + ' ‖ '.join(candidates)
                    if cmd[5:7] == ['address', 'ad-' + '2' * 16]:
                        if variant == 'missing':
                            answer.clear()
                        if variant in ('raw', 'raw-address'):
                            answer['raw'].append('alias_of' if variant == 'raw' else 'addr')

                result = self.discover(change, aliases=True)
                self.assertTrue(result['proven'], result)
                rows = result['machines'][0]['accounts']
                self.assertEqual([row['alias_declaration'] for row in rows], ['declared', expected])
                self.assertNotIn('ad-', json.dumps(result))

    def test_alias_declarations_withhold_changed_target_conflict_or_machine(self):
        for variant in ('target', 'conflict', 'missing-to-clear'):
            with self.subTest(variant=variant):
                def change(cmd, answer, occurrence):
                    if cmd != ADDRESS_SCAN and cmd[2] != 'emails':
                        return
                    target = 'ad-' + ('3' if occurrence == 1 else '4') * 16
                    if variant == 'missing-to-clear':
                        if cmd == ADDRESS_SCAN and occurrence == 2:
                            answer['rows'][1]['alias_of'] = None
                    elif cmd == ADDRESS_SCAN:
                        answer['rows'][0]['alias_of'] = {'conflict': [None, target]} if variant == 'conflict' else target
                    else:
                        answer['emails'][0]['alias_of'] = '⚠  ‖ ' + target if variant == 'conflict' else target

                result = self.discover(change, aliases=True)
                self.assertEqual(result['errors'], ['alias-declarations-changed'])
                self.assertEqual(result['machines'], [])
                self.assertFalse(result['proven'])
                self.assertNotIn('ad-', json.dumps(result))
                self.assertNotIn('@', json.dumps(result))

        changed = self.discover(aliases=True, here=(published_machine(), published_machine(mc=M2)))
        self.assertFalse(changed['proven'])
        self.assertEqual(changed['machines'], [])
        self.assertNotIn('@', json.dumps(changed))

    def test_alias_declarations_require_each_settled_cell_in_the_complete_email_view(self):
        for field_value in ('missing', None, 'ad-' + '9' * 16):
            with self.subTest(field_value=field_value):
                def disagree(cmd, answer, occurrence):
                    if cmd[2] == 'emails' and occurrence == 2:
                        row = answer['emails'][0]
                        if field_value == 'missing':
                            del row['alias_of']
                        else:
                            row['alias_of'] = field_value

                result = self.discover(disagree, aliases=True)
                self.assertEqual(result['errors'], ['alias-declarations-changed'])
                self.assertEqual(result['machines'], [])
                self.assertNotIn('@', json.dumps(result))
                self.assertNotIn('ad-', json.dumps(result))

    def test_disputed_alias_requires_the_complete_email_views_ordered_candidates(self):
        first, second, replacement = ('ad-' + c * 16 for c in '543')
        # Native orders by writer and operation, not by address; a clear is an empty value.
        candidates = [first, None, second]
        exact = f'⚠ {first} ‖  ‖ {second}'
        for when in (1, 2):
            for mode, projected in (('agrees', exact), ('missing', None), ('clear', None),
                                    ('settled', first), ('reversed', f'⚠ {second} ‖  ‖ {first}'),
                                    ('dropped-clear', f'⚠ {first} ‖ {second}'),
                                    ('replaced', f'⚠ {first} ‖  ‖ {replacement}')):
                with self.subTest(when=when, mode=mode):
                    def change(cmd, view, n):
                        if cmd == ADDRESS_SCAN:
                            view['rows'][1]['alias_of'] = {'conflict': candidates}
                        elif cmd[2] == 'emails':
                            row = view['emails'][1]
                            if mode == 'missing' and n == when:
                                del row['alias_of']
                            else:
                                row['alias_of'] = projected if n == when else exact

                    result = self.discover(change, aliases=True)
                    if mode == 'agrees':
                        self.assertTrue(result['proven'], result)
                        self.assertEqual(result['machines'][0]['accounts'][1]['alias_declaration'], 'disputed')
                    else:
                        self.assertEqual(result['errors'], ['alias-declarations-changed'])
                        self.assertEqual(result['machines'], [])
                        self.assertFalse(result['proven'])
                        self.assertNotIn('@', json.dumps(result))
                    self.assertNotIn('ad-', json.dumps(result))

    def test_alias_disputes_need_distinct_typed_candidates_and_cannot_be_established_by_a_grid_alone(self):
        first, second = ('ad-' + c * 16 for c in '34')
        for mode, candidates in (('duplicate-link', [first, first]), ('duplicate-clear', [None, None]),
                                 ('repeated', [first, second, first]), ('raw', [first, second]),
                                 ('valid', [first, second])):
            for alias_only in (True, False):
                with self.subTest(mode=mode, alias_only=alias_only):
                    def change(cmd, view, _):
                        if cmd == ADDRESS_SCAN:
                            view['rows'][0]['alias_of'] = {'conflict': candidates}
                        elif cmd[2] == 'emails':
                            view['emails'][0]['alias_of'] = '⚠ ' + ' ‖ '.join(v or '' for v in candidates)
                        elif mode == 'raw' and cmd[5:7] == ['address', 'ad-' + '1' * 16]:
                            view['raw'].append('alias_of')
                        elif cmd[-1] == 'mail':
                            view['addresses'][0]['alias_of'] = first  # The provisional scalar.

                    result = self.discover(change, aliases=alias_only, mail=None if alias_only else self.mail_view())
                    self.assertTrue(result['proven'], result)
                    row = result['machines'][0]['accounts'][0]
                    declaration = row if alias_only else row['mail_route']
                    self.assertEqual(declaration['alias_declaration'], 'disputed' if mode == 'valid' else 'unconfirmed')
                    if not alias_only:
                        self.assertEqual(declaration['status'], 'reported')  # An independent owner.
                    self.assertNotIn('ad-', json.dumps(result))

    def test_alias_declarations_name_the_held_typed_detail_without_reading_mail(self):
        for collection, scan, noun in (('address', ADDRESS_SCAN, 'emails'),
                                       ('account', PROBE.QUERIES['accounts'], 'ai-accounts')):
            with self.subTest(collection=collection):
                def held(cmd, answer, _):
                    if cmd == scan:
                        answer['rows'][0]['row'] = None
                    elif cmd[2] == noun:
                        answer[noun][0]['row'] = None
                    elif cmd[3:6] == ['get', 'alab/registry', collection]:
                        answer.update(state='held', why='private@reason.invalid')

                calls = []
                report = self.discover(held, aliases=True, calls=calls)
                self.assertEqual(report['status'], 'unavailable')
                self.assertEqual(report['errors'], [f'{collection}-detail-unconfirmed'])
                self.assertEqual(report['machines'], [])
                self.assertNotIn(['aterm', 'db', 'alab', 'mail'], calls)
                self.assertEqual(report['commands'][-1]['command'][6], f'<selected-{collection}>')
                for private in ('@', 'ad-', A1):
                    self.assertNotIn(private, json.dumps(report))

    def test_alias_declarations_cli_dispatch_and_exit_codes(self):
        for location, method in ((['--here'], 'probe_here'), (['--machine', 'm3'], 'probe')):
            for status, words, outcome, code in (
                    ('matched', ['absent', 'declared'], 'complete', 0),
                    ('matched', ['unconfirmed'], 'unresolved', 1), ('matched', ['disputed'], 'unresolved', 1),
                    ('insufficient-accounts', [], 'insufficient-accounts', 1),
                    ('machine-not-found', [], 'machine-not-found', 1),
                    ('ambiguous-machine', [], 'ambiguous-machine', 2), ('unavailable', [], 'unavailable', 2)):
                with self.subTest(location=location, status=status, words=words):
                    rows = [{'alias_declaration': word} for word in words]
                    machines = [{'accounts': rows}] if status in ('matched', 'insufficient-accounts') else []
                    report = {'status': status, 'proven': status == 'matched', 'machines': machines}
                    check = {'name': 'alias-declarations', 'status': outcome, 'exit_code': code}
                    expected = dict(report, check=check)
                    with tempfile.TemporaryDirectory() as temp, patch.object(PROBE, method, return_value=report) as read, \
                            patch.object(sys, 'stdout', io.StringIO()) as stdout:
                        target = Path(temp) / 'alias.json'
                        self.assertEqual(PROBE.main([*location, '--show-alias-declarations', '--output', str(target)]), code)
                        self.assertEqual(target.read_text(), stdout.getvalue())
                        saved = json.loads(stdout.getvalue())
                        self.assertEqual(saved, expected)
                        self.assertIs(type(saved['check']['exit_code']), int)
                    self.assertEqual(read.call_args.args, (1,) if method == 'probe_here' else ('m3', 1))
                    self.assertIs(read.call_args.kwargs['show_alias_declarations'], True)
        for other in (['--expect-account', A1], ['--expect-login', 'first@example.invalid'],
                      ['--show-logins'], ['--show-mail-routes']):
            with self.subTest(other=other), patch.object(PROBE, 'probe_here') as read, \
                    patch.object(sys, 'stderr', io.StringIO()), self.assertRaises(SystemExit) as error:
                PROBE.main(['--here', '--show-alias-declarations', *other])
            self.assertEqual(error.exception.code, 2)
            read.assert_not_called()

    def test_saved_mail_route_check_distinguishes_assignments_from_confirmed_routes(self):
        partial = self.mail_view()
        partial['addresses'][1]['mailbox'] = None
        cases = (
            (['--machine', 'm3'], self.discover(mail=self.mail_view()), 'complete', 0),
            (['--here'], self.discover(mail=partial, here=(published_machine(), published_machine())), 'unresolved', 1),
            (['--machine', 'missing'], self.discover(mail=partial, machine='missing'), 'machine-not-found', 1),
            (['--here'], self.discover(mail=partial, here=(published_machine(), None)), 'unavailable', 2),
        )
        for location, report, outcome, code in cases:
            with self.subTest(location=location, outcome=outcome), tempfile.TemporaryDirectory() as temp:
                self.assertNotIn('check', report)
                if outcome == 'unresolved':
                    self.assertTrue(report['proven'])
                    self.assertEqual([row['mail_route']['status'] for row in report['machines'][0]['accounts']],
                                     ['reported', 'no-effective-route'])
                check = {'name': 'mail-routes', 'status': outcome, 'exit_code': code}
                expected = dict(report, check=check)
                target = Path(temp) / 'routes.json'
                method = 'probe_here' if location == ['--here'] else 'probe'
                with patch.object(PROBE, method, return_value=report), patch.object(sys, 'stdout', io.StringIO()) as stdout:
                    self.assertEqual(PROBE.main([*location, '--show-mail-routes', '--output', str(target)]), code)
                self.assertEqual(target.read_text(), stdout.getvalue())
                saved = json.loads(stdout.getvalue())
                self.assertEqual(saved, expected)
                self.assertIs(type(saved['check']['exit_code']), int)

    def test_association_only_cli_modes_preserve_the_api_report(self):
        for selectors, report in (
                ([], PROBE.probe('m3', 2, self.reader)),
                (['--expect-account', A1], PROBE.probe('m3', 1, self.reader, expected_accounts=[A1])),
                (['--expect-login', 'first@example.invalid'], self.discover(expected_logins=['first@example.invalid'])),
                (['--show-logins'], self.discover())):
            with self.subTest(selectors=selectors):
                expected = copy.deepcopy(report)
                self.assertNotIn('check', expected)
                with patch.object(PROBE, 'probe', return_value=report), patch.object(sys, 'stdout', io.StringIO()) as stdout:
                    self.assertEqual(PROBE.main(['--machine', 'm3', *selectors]), 0)
                self.assertEqual(json.loads(stdout.getvalue()), expected)

    def test_mail_routes_join_the_registered_alias_and_support_a_shared_inbox(self):
        view = self.mail_view()
        for here in (None, (published_machine(), published_machine())):
            if here:
                view['addresses'][0]['row'] = None
                view['mailboxes'][0]['row'] = None
            result = self.discover(required=2, mail=view, here=here)
            self.assertTrue(result['proven'], result)
            accounts = result['machines'][0]['accounts']
            self.assertEqual([r['registered_login'] for r in accounts], ['first@example.invalid', 'second@example.invalid'])
            self.assertEqual([r['mail_route'] for r in accounts],
                             [{'status': 'reported', 'mailbox_key': 'mb-' + 'a' * 16, 'evidence': how,
                               'alias_declaration': 'declared'}
                              for how in ('owner', 'observed')])
            self.assertEqual(sum(r['command'][-1] == 'mail' for r in result['commands']), 2)
            for kind, count in (('mailbox', 2), ('address', 6)):
                self.assertEqual(sum(r['command'][3:6] == ['get', 'alab/registry', kind]
                                     for r in result['commands']), count)
            for private in ('private@', 'mb-' + 'b' * 16, 'ad-', 'retained@example.invalid'):
                self.assertNotIn(private, json.dumps(result))
        # Existing address discovery does not ask for mail or expose routing.
        original = self.discover()
        self.assertFalse(any(r['command'][-1] == 'mail' for r in original['commands']))
        self.assertNotIn('mail_route', json.dumps(original))

    def test_mail_summary_must_agree_with_each_consulted_settled_alias(self):
        other = 'ad-' + '9' * 16
        for index, change in ((0, 'retarget'), (0, 'clear'), (0, 'omit'), (3, 'invent')):
            for when in (1, 2):
                with self.subTest(index=index, change=change, when=when):
                    def disagree(cmd, answer, occurrence):
                        if cmd[-1] == 'mail' and occurrence == when:
                            row = answer['addresses'][index]
                            if change == 'omit':
                                del row['alias_of']
                            else:
                                row['alias_of'] = None if change == 'clear' else other

                    result = self.discover(disagree, mail=self.mail_view())
                    self.assertEqual(result['errors'], ['mail-routes-changed'])
                    self.assertEqual(result['machines'], [])
                    for private in ('ad-', 'mb-', '@'):
                        self.assertNotIn(private, json.dumps(result))

    def test_provisional_and_unused_summary_aliases_leave_a_confirmed_owner_intact(self):
        mail = self.mail_view()
        mail['addresses'][1]['mailbox']['how'] = 'owner'
        mail['addresses'][3]['mailbox']['how'] = 'owner'

        def provisional(cmd, answer, _):
            if cmd == ADDRESS_SCAN:
                answer['rows'][3]['mailbox'] = mail['addresses'][1]['mailbox']['mb']
            elif cmd[5:7] == ['address', mail['addresses'][0]['ad']]:
                answer['raw'].append('alias_of')
            elif cmd[-1] == 'mail':
                for index in (0, 3):
                    answer['addresses'][index]['alias_of'] = 'ad-' + '9' * 16

        result = self.discover(provisional, mail=mail)
        first, second = [a['mail_route'] for a in result['machines'][0]['accounts']]
        self.assertEqual((first['alias_declaration'], first['status']), ('unconfirmed', 'reported'))
        self.assertEqual((second['alias_declaration'], second['status']), ('declared', 'reported'))
        self.assertNotIn('ad-', json.dumps(result))

    def test_unconfirmed_alias_identity_keeps_the_null_route_protection_witness(self):
        mail = self.mail_view()
        selected = mail['addresses'][1]
        selected['mailbox'] = None

        def changing_protection(cmd, answer, occurrence):
            if cmd[5:7] == ['address', selected['ad']]:
                answer['raw'].append('addr')
            if cmd == ADDRESS_SCAN and occurrence == 2:
                answer['rows'][1]['protected'] = True

        result = self.discover(changing_protection, mail=mail)
        self.assertEqual(result['errors'], ['mail-routes-changed'])
        self.assertEqual(result['machines'], [])
        self.assertNotIn('@', json.dumps(result))

    def test_observed_fallback_requires_the_tried_alias_rows_in_the_summary(self):
        mail = self.mail_view()
        mail['addresses'][1]['mailbox']['via'] = []
        relay = mail['addresses'].pop()

        def discarded_link(cmd, answer, _):
            if cmd == ADDRESS_SCAN:
                self.assertEqual(answer['rows'][3]['pk'], relay['ad'])
                answer['rows'][3]['alias_of'] = answer['rows'][2]['pk']

        result = self.discover(discarded_link, mail=mail)
        self.assertEqual(result['errors'], ['mail-routes-changed'])
        self.assertEqual(result['machines'], [])
        self.assertNotIn('mb-', json.dumps(result))

    def test_restored_login_lifecycle_needs_stable_typed_evidence(self):
        for collection, scan, noun in [('account', PROBE.QUERIES['accounts'], 'ai-accounts'),
                                       ('address', ADDRESS_SCAN, 'emails')]:
            for mode in ('stable', 'raw', 'conflicts', 'missing-detail', 'omitted-cell', 'retired', 'wrong-key',
                         'restored-on-second', 'removed-on-second'):
                with self.subTest(collection=collection, mode=mode):
                    def change(cmd, answer, n):
                        restored = (mode != 'restored-on-second' or n == 2) and (mode != 'removed-on-second' or n == 1)
                        if cmd == scan:
                            if restored:
                                answer['rows'][0]['row'] = None
                            else:
                                answer['rows'][0].pop('row', None)
                        elif cmd[2] == noun:
                            answer[noun][0]['row'] = None
                        elif cmd[3:6] == ['get', 'alab/registry', collection]:
                            if mode in ('raw', 'conflicts'):
                                answer[mode].append('row')
                            elif mode == 'missing-detail':
                                answer.clear()
                            elif mode == 'omitted-cell':
                                answer['row'].pop('row')
                            elif mode == 'retired':
                                answer['lifecycle'] = 'retired'
                            elif mode == 'wrong-key':
                                answer['row']['pk'] = A2 if collection == 'account' else 'ad-' + '2' * 16

                    report = self.discover(change, expected_logins=['first@example.invalid'])
                    if mode == 'stable':
                        self.assertTrue(report['proven'], report)
                        self.assertEqual(report['resolved_login_account_keys'], [A1])
                        details = [r['command'] for r in report['commands'] if r['command'][3:4] == ['get']]
                        self.assertEqual([c[6] for c in details], [f'<selected-{collection}>'] * 2)
                    else:
                        self.assertFalse(report['proven'], report)
                        self.assertEqual(report['machines'], [])
                        self.assertNotIn('@', json.dumps(report))
                    self.assertNotIn('first@example.invalid', json.dumps(report))
                    self.assertNotIn('ad-', json.dumps(report))

    def test_restored_detail_corroborates_used_join_fields_only(self):
        for collection, field, used in (('account', 'provider', True), ('account', 'login', True),
                                        ('account', 'status', True), ('address', 'addr', True),
                                        ('account', 'label', False), ('address', 'mailbox', False)):
            scan = PROBE.QUERIES['accounts'] if collection == 'account' else ADDRESS_SCAN
            noun = 'ai-accounts' if collection == 'account' else 'emails'
            for variant in ('different', 'raw', 'conflicts'):
                with self.subTest(collection=collection, field=field, variant=variant):
                    def change(cmd, answer, n):
                        if cmd == scan:
                            answer['rows'][0]['row'] = None
                        elif cmd[2] == noun:
                            answer[noun][0]['row'] = None
                        elif cmd[3:6] == ['get', 'alab/registry', collection] and n == 2:
                            if variant == 'different':
                                answer['row'][field] = 'changed@private.invalid'
                            else:
                                answer[variant].append(field)

                    result = self.discover(change, setup=[A1])
                    self.assertIs(result['proven'], not used, result)
                    self.assertNotIn('changed@', json.dumps(result))
                    if used:
                        self.assertEqual(result['errors'], ['unconfirmed-restored-lifecycle'])
                        self.assertEqual(result['machines'], [])

    def test_restored_source_and_alias_relay_can_report_their_settled_mailbox(self):
        for index in (0, 3):
            for mode in ('stable', 'missing-detail', 'raw', 'wrong-summary', 'changed'):
                with self.subTest(index=index, mode=mode):
                    mail = self.mail_view()
                    selected = mail['addresses'][index]
                    selected['row'] = 'live' if mode == 'wrong-summary' else None

                    def change(cmd, answer, n):
                        if cmd == ADDRESS_SCAN and (mode != 'changed' or n == 1):
                            answer['rows'][index]['row'] = None
                        elif cmd[2] == 'emails':
                            answer['emails'][index]['row'] = None
                        elif cmd[5:7] == ['address', selected['ad']]:
                            if mode == 'raw':
                                answer['raw'].append('row')
                            elif mode == 'missing-detail':
                                answer.clear()

                    result = self.discover(change, mail=mail)
                    if mode == 'stable':
                        self.assertTrue(result['proven'], result)
                        routes = [row['mail_route'] for row in result['machines'][0]['accounts']]
                        self.assertEqual([r.get('mailbox_key') for r in routes], [mail['mailboxes'][0]['mb']] * 2)
                        self.assertEqual(sum(r['command'][3:6] == ['get', 'alab/registry', 'address']
                                             for r in result['commands']), 6)
                    elif mode == 'changed' or index == 0:
                        self.assertFalse(result['proven'], result)
                        self.assertEqual(result['machines'], [])
                    else:
                        self.assertTrue(result['proven'], result)
                        routes = [row['mail_route'] for row in result['machines'][0]['accounts']]
                        self.assertEqual([r['status'] for r in routes], ['reported', 'unconfirmed'])
                        self.assertNotIn('mailbox_key', routes[1])
                    self.assertNotIn(selected['ad'], json.dumps(result))

    def test_scan_only_account_inventory_still_requires_a_known_lifecycle(self):
        data = fixture()
        data['accounts']['rows'][0]['row'] = None
        result = PROBE.probe('m3', 1, reader_for(data, details()))
        self.assertEqual(result['errors'], ['invalid-account'])
        self.assertEqual(result['machines'], [])

    def test_mailbox_detail_settles_the_native_lifecycle_and_filters_private_metadata(self):
        for lifecycle in (None, 'live'):
            view = self.mail_view()
            view['mailboxes'][0]['row'] = lifecycle
            for field in ('conflicts', 'raw'):
                def disputed(cmd, answer, _):
                    if cmd[3:6] == ['get', 'alab/registry', 'mailbox']:
                        answer[field].append('row')
                        answer['row']['row'] = {'conflict': ['live', 'private@value.invalid']}
                result = self.discover(disputed, mail=view)
                self.assertTrue(result['proven'], result)
                self.assertEqual([a['mail_route']['status'] for a in result['machines'][0]['accounts']],
                                 ['unconfirmed', 'unconfirmed'])
                self.assertNotIn('private@', json.dumps(result))
                self.assertNotIn('mb-', json.dumps(result))

    def test_null_route_distinguishes_an_unset_declared_disputed_or_unreadable_alias(self):
        first, alias, other = ('ad-' + c * 16 for c in '135')
        unset = object()
        for value, raw, expected in (
                (unset, False, 'absent'), (None, False, 'absent'), (alias, False, 'declared'),
                ({'conflict': [None, other]}, False, 'disputed'), (alias, True, 'unconfirmed'),
                ('private@invalid.example', False, 'unconfirmed')):
            with self.subTest(value=value, raw=raw):
                mail = self.mail_view()
                mail['addresses'][0]['mailbox'] = None

                def change(cmd, answer, _):
                    if cmd == ADDRESS_SCAN:
                        row = answer['rows'][0]
                        row.pop('mailbox')
                        if value is unset:
                            row.pop('alias_of')
                        else:
                            row['alias_of'] = value
                    if raw and cmd[5:7] == ['address', first]:
                        answer['raw'].append('alias_of')

                result = self.discover(change, mail=mail)
                self.assertTrue(result['proven'], result)
                self.assertEqual(result['machines'][0]['accounts'][0]['mail_route'],
                                 {'status': 'no-effective-route', 'alias_declaration': expected})
                for private in ('ad-', 'private@', '"conflict"'):
                    self.assertNotIn(private, json.dumps(result))

    def test_unavailable_alias_detail_keeps_an_independently_confirmed_owner(self):
        def unavailable(cmd, answer, _):
            if cmd[5:7] == ['address', 'ad-' + '1' * 16]:
                answer.clear()
        result = self.discover(unavailable, mail=self.mail_view())
        self.assertTrue(result['proven'], result)
        self.assertEqual(result['machines'][0]['accounts'][0]['mail_route'],
                         {'status': 'reported', 'evidence': 'owner', 'mailbox_key': 'mb-' + 'a' * 16,
                          'alias_declaration': 'unconfirmed'})
        with patch.object(PROBE, 'probe', return_value=result), patch.object(sys, 'stdout', io.StringIO()):
            self.assertEqual(PROBE.main(['--machine', 'm3', '--show-mail-routes']), 0)

    def test_raw_or_disputed_source_identity_withholds_only_its_own_direct_mailbox(self):
        for index, evidence in enumerate(('owner', 'observed')):
            for kind in ('raw', 'conflicts'):
                for field in ('addr', 'row'):
                    with self.subTest(evidence=evidence, kind=kind, field=field):
                        mail = self.mail_view()
                        mail['addresses'][1]['mailbox']['via'] = []
                        address = mail['addresses'][index]['ad']

                        def provisional(cmd, answer, _):
                            if cmd == ADDRESS_SCAN:
                                del answer['rows'][1]['alias_of']
                            elif cmd[5:7] == ['address', address]:
                                answer[kind].append(field)

                        result = self.discover(provisional, mail=mail)
                        self.assertTrue(result['proven'], result)
                        routes = [a['mail_route'] for a in result['machines'][0]['accounts']]
                        self.assertEqual(routes[index], {'status': 'unconfirmed', 'evidence': evidence,
                                                         'alias_declaration': 'unconfirmed'})
                        self.assertEqual(routes[1 - index]['status'], 'reported')
                        self.assertIn('mailbox_key', routes[1 - index])
                        self.assertNotIn('ad-', json.dumps(result))

    def test_known_provisional_identity_cannot_confirm_an_empty_route_or_hide_changes(self):
        for changes in (False, True):
            with self.subTest(changes=changes):
                mail = self.mail_view()
                mail['addresses'][0]['mailbox'] = None

                def provisional(cmd, answer, occurrence):
                    if cmd[5:7] == ['address', mail['addresses'][0]['ad']]:
                        kind = 'conflicts' if changes and occurrence == 2 else 'raw'
                        answer[kind].append('addr')

                result = self.discover(provisional, mail=mail)
                if changes:
                    self.assertEqual(result['errors'], ['mail-routes-changed'])
                    self.assertEqual(result['machines'], [])
                    for private in ('ad-', 'mb-', '@'):
                        self.assertNotIn(private, json.dumps(result))
                else:
                    self.assertTrue(result['proven'], result)
                    self.assertEqual(result['machines'][0]['accounts'][0]['mail_route'],
                                     {'status': 'unconfirmed', 'alias_declaration': 'unconfirmed'})

    def test_observed_mailbox_requires_a_stored_alias_only_when_the_route_follows_one(self):
        for detail_available in (True, False):
            for follows_alias in (True, False):
                with self.subTest(detail_available=detail_available, follows_alias=follows_alias):
                    mail = self.mail_view()
                    if not follows_alias:
                        mail['addresses'][1]['mailbox']['via'] = []

                    def change(cmd, answer, _):
                        if cmd == ADDRESS_SCAN and not follows_alias:
                            del answer['rows'][1]['alias_of']
                        if cmd[5:7] == ['address', 'ad-' + '2' * 16]:
                            if not detail_available:
                                answer.clear()
                            elif follows_alias:
                                answer['raw'].append('alias_of')

                    result = self.discover(change, mail=mail)
                    self.assertTrue(result['proven'], result)
                    route = result['machines'][0]['accounts'][1]['mail_route']
                    self.assertEqual(route['status'], 'unconfirmed' if follows_alias else 'reported')
                    self.assertEqual('mailbox_key' in route, not follows_alias)
                    self.assertEqual(route['alias_declaration'],
                                     'absent' if detail_available and not follows_alias else 'unconfirmed')

    def test_route_through_a_disputed_address_cannot_be_settled_by_missing_detail_flags(self):
        for summary in ('owner', 'null'):
            for get in ('unavailable', 'unmarked', 'marked'):
                with self.subTest(summary=summary, get=get):
                    mail = self.mail_view()
                    first, second, _, relay = mail['addresses']
                    box = mail['mailboxes'][0]['mb']
                    typed = {
                        first['ad']: {'pk': first['ad'], 'addr': 'first@example.invalid', 'mailbox': box},
                        second['ad']: {'pk': second['ad'], 'addr': 'second@example.invalid', 'alias_of': relay['ad']},
                        relay['ad']: {'pk': relay['ad'], 'mailbox': box,
                                      'addr': {'conflict': ['relay@example.invalid', 'other@example.invalid']}},
                    }
                    first['alias_of'], second['alias_of'], relay['alias_of'] = None, relay['ad'], None
                    relay['mailbox'] = dict(first['mailbox'])
                    second['mailbox'] = dict(relay['mailbox'], via=[relay['ad']]) if summary == 'owner' else None

                    def read(cmd):
                        if cmd[-1] == 'mail':
                            return mail, {'command': cmd}
                        collection, pk = cmd[5:7]
                        if pk == relay['ad'] and get == 'unavailable':
                            return None, {'command': cmd, 'error': 'command-failed'}
                        row = typed[pk] if collection == 'address' else {'pk': pk, 'row': 'live'}
                        conflicts = ['addr'] if pk == relay['ad'] and get == 'marked' else []
                        return {'collection': collection, 'row': row, 'lifecycle': 'live',
                                'conflicts': conflicts, 'raw': []}, {'command': cmd}

                    logins = [(typed[r['ad']]['addr'], r['ad'], ac) for r, ac in ((first, A1), (second, A2))]
                    fields, _ = PROBE.read_login_mail_fields(logins, typed.values(), read, [], {}, None)
                    self.assertEqual(fields[A1]['mail_route']['status'], 'reported')
                    chosen = fields[A2]['mail_route']
                    self.assertEqual(chosen['status'], 'unconfirmed' if summary == 'owner' else 'no-effective-route')
                    self.assertNotIn('mailbox_key', chosen)
                    self.assertNotIn('ad-', json.dumps(fields))
                    self.assertNotIn('@', json.dumps(fields))

    def test_shared_two_hop_route_uses_only_settled_aliases_at_each_consulted_address(self):
        first, second, owner, relay = ('ad-' + c * 16 for c in '1234')
        mailbox = 'mb-' + 'a' * 16
        for mode in ('settled', 'raw', 'missing', 'disputed', 'raw-retargeted',
                     'owner-address-raw', 'owner-lifecycle-disputed', 'owner-alias-changed'):
            with self.subTest(mode=mode):
                mail = self.mail_view()
                for row in mail['addresses'][:2]:
                    row['mailbox'].update(mb=mailbox, how='owner', via=[relay, owner])
                mail['addresses'][2]['mailbox'].update(mb=mailbox)
                mail['addresses'][3]['mailbox'].update(how='owner', via=[owner])
                consulted = []

                def change(cmd, answer, occurrence):
                    if cmd == ADDRESS_SCAN:
                        source, _, destination, middle = answer['rows']
                        del source['mailbox']
                        source['alias_of'] = relay
                        destination['mailbox'] = mailbox
                        middle['alias_of'] = owner
                        if mode == 'owner-alias-changed':
                            destination['alias_of'] = first if occurrence == 1 else second
                        if mode == 'disputed':
                            middle['alias_of'] = {'conflict': [owner, None]}
                        if mode == 'raw-retargeted' and occurrence == 2:
                            middle['alias_of'] = 'ad-' + '5' * 16
                    elif cmd[3:6] == ['get', 'alab/registry', 'address']:
                        consulted.append(cmd[6])
                        if cmd[6] == owner and mode == 'owner-address-raw':
                            answer['raw'].append('addr')
                        if cmd[6] == owner and mode == 'owner-lifecycle-disputed':
                            answer['conflicts'].append('row')
                        if cmd[6] == relay:
                            if mode.startswith('raw'):
                                answer['raw'].append('alias_of')
                            elif mode == 'missing':
                                answer.clear()

                result = self.discover(change, mail=mail)
                # Both logins share the intermediate and terminal addresses.
                # Each is checked once per pass, only when reached.
                reaches_owner = mode == 'settled' or mode.startswith('owner-')
                path = [first, second, relay] + ([owner] if reaches_owner else [])
                self.assertCountEqual(consulted, path * 2)
                self.assertNotIn('ad-', json.dumps(result))
                if mode == 'raw-retargeted':
                    self.assertEqual(result['errors'], ['mail-routes-changed'])
                    self.assertEqual(result['machines'], [])
                    continue
                self.assertTrue(result['proven'], result)
                for account in result['machines'][0]['accounts']:
                    route = account['mail_route']
                    self.assertEqual(route['alias_declaration'], 'declared')
                    if mode in ('settled', 'owner-alias-changed'):
                        self.assertEqual((route['status'], route['mailbox_key']), ('reported', mailbox))
                    else:
                        self.assertEqual(route['status'], 'unconfirmed')
                        self.assertNotIn('mailbox_key', route)

    def test_alias_change_with_the_same_owner_and_public_declaration_withholds_the_report(self):
        for before, after in (('ad-' + '3' * 16, 'ad-' + '4' * 16),
                              ({'conflict': [None, 'ad-' + '3' * 16]}, {'conflict': [None, 'ad-' + '4' * 16]})):
            def change(cmd, answer, occurrence):
                if cmd == ADDRESS_SCAN:
                    answer['rows'][0]['alias_of'] = before if occurrence == 1 else after
            result = self.discover(change, mail=self.mail_view())
            self.assertFalse(result['proven'], result)
            self.assertEqual(result['errors'], ['mail-routes-changed'])
            self.assertEqual(result['machines'], [])
            for private in ('ad-', 'mb-', '@'):
                self.assertNotIn(private, json.dumps(result))

    def test_address_route_priorities_reject_provisional_choices_in_the_signed_mail_summary(self):
        owner, other = ('mb-' + c * 16 for c in 'ab')
        side, relay = ('ad-' + c * 16 for c in '34')
        for index, field, values, expected in (
                (0, 'mailbox', [owner, other], ['unconfirmed', 'reported']),
                (0, 'protected', [False, True], ['unconfirmed', 'reported']),
                (1, 'alias_of', [relay, side], ['reported', 'unconfirmed']),
                (1, 'mailbox', [None, other], ['reported', 'unconfirmed']),
                (3, 'protected', [False, True], ['reported', 'unconfirmed']),
                (3, 'mailbox', [None, other], ['reported', 'unconfirmed']),
                # The settled owner on the first login takes priority over its alias.
                (0, 'alias_of', [relay, side], ['reported', 'reported']),
                # A conflict on a separate address cannot affect these routes.
                (2, 'mailbox', [owner, other], ['reported', 'reported'])):
            with self.subTest(index=index, field=field, expected=expected):
                def dispute(cmd, answer, occurrence):
                    if cmd == ADDRESS_SCAN:
                        answer['rows'][index][field] = {'conflict': values}
                result = self.discover(dispute, mail=self.mail_view())
                self.assertTrue(result['proven'], result)
                accounts = result['machines'][0]['accounts']
                self.assertEqual([a['mail_route']['status'] for a in accounts], expected)
                if field == 'alias_of':
                    self.assertEqual(accounts[index]['mail_route']['alias_declaration'], 'disputed')
                for account, status in zip(accounts, expected):
                    self.assertEqual('mailbox_key' in account['mail_route'], status == 'reported')
                self.assertNotIn(other, json.dumps(result))

    def test_observed_tag_validates_the_base_protection_and_only_consulted_owner_cells(self):
        relay = 'ad-' + '4' * 16
        for mode, expected in (('base-owner-dispute', 'reported'), ('base-protected', 'unconfirmed'),
                               ('base-protection-dispute', 'unconfirmed'), ('base-address-raw', 'unconfirmed'),
                               ('unrelated-base', 'unconfirmed'),
                               ('discarded-alias-dispute', 'unconfirmed'), ('protected-alias', 'reported')):
            with self.subTest(mode=mode):
                def change(cmd, view, occurrence):
                    if cmd[5:7] == ['address', relay] and mode == 'base-address-raw':
                        view['raw'].append('addr')
                    if cmd == ADDRESS_SCAN or cmd[2] == 'emails':
                        rows = view.get('rows', view.get('emails'))
                        rows[1]['addr'] = 'relay+moon@example.invalid' if mode != 'unrelated-base' else 'plain@example.invalid'
                    if cmd == ADDRESS_SCAN:
                        del rows[1]['alias_of']
                        rows[3]['mailbox'] = {'conflict': [None, 'mb-' + 'b' * 16]}
                        if mode == 'base-protected':
                            rows[3]['protected'] = True
                        if mode == 'base-protection-dispute':
                            rows[3]['protected'] = {'conflict': [False, True]}
                        if mode in ('discarded-alias-dispute', 'protected-alias'):
                            rows[1]['alias_of'] = rows[2]['pk']
                            rows[2]['mailbox'] = {'conflict': [None, 'mb-' + 'b' * 16]}
                            if mode == 'protected-alias':
                                rows[2]['protected'] = True
                result = self.discover(change, mail=self.mail_view())
                self.assertTrue(result['proven'], result)
                _, account = result['machines'][0]['accounts']
                self.assertEqual(account['mail_route']['status'], expected)
                if mode == 'base-owner-dispute':
                    self.assertEqual(account['mail_route']['alias_declaration'], 'absent')
                self.assertEqual('mailbox_key' in account['mail_route'], expected == 'reported')
                self.assertNotIn(relay, json.dumps(result))

    def test_tag_inference_requires_evidence_the_saved_alias_did_not_supply_that_hop(self):
        for mode, expected in (
                ('same', 'unconfirmed'), ('null', 'reported'), ('different', 'reported'),
                ('missing', 'unconfirmed'), ('malformed', 'unconfirmed'), ('changed', 'changed'),
                ('retargeted', 'changed'), ('malformed-change', 'changed'),
                ('direct-observation', 'reported'), ('direct-owner', 'reported'),
                ('intermediate', 'unconfirmed')):
            with self.subTest(mode=mode):
                mail = self.mail_view()
                _, source, middle, base = mail['addresses']
                parent = middle if mode == 'intermediate' else source
                if mode == 'intermediate':
                    source['mailbox']['via'] = [middle['ad'], base['ad']]
                    middle['mailbox'] = dict(source['mailbox'], via=[base['ad']])
                if mode in ('direct-observation', 'direct-owner'):
                    source['mailbox']['via'] = []
                    if mode == 'direct-owner':
                        source['mailbox']['how'] = 'owner'
                    else:
                        base['mailbox'] = None  # Native may fall back to the source's own observation.
                index = mail['addresses'].index(parent)
                privately_named = 'ad-' + 'f' * 16

                def change(cmd, view, n):
                    if cmd == ADDRESS_SCAN or cmd[2] == 'emails':
                        rows = view.get('rows', view.get('emails'))
                        rows[index]['addr'] = 'relay+moon@example.invalid'
                        if cmd == ADDRESS_SCAN:
                            rows[index].pop('alias_of', None)
                            if mode == 'intermediate':
                                rows[1]['alias_of'] = middle['ad']
                            if mode == 'direct-owner':
                                rows[1]['mailbox'] = source['mailbox']['mb']
                    elif cmd[5:7] == ['address', parent['ad']]:
                        view.clear()  # The typed alias cannot be settled by get.
                    elif cmd[-1] == 'mail':
                        shown = view['addresses'][index]
                        if mode == 'missing':
                            del shown['alias_of']
                        elif mode == 'malformed':
                            shown['alias_of'] = 'private@invalid.example'
                        elif mode == 'malformed-change':
                            shown['alias_of'] = True if n == 1 else 1
                        elif mode == 'null' or (mode == 'changed' and n == 1):
                            shown['alias_of'] = None
                        elif mode in ('different', 'retargeted'):
                            shown['alias_of'] = privately_named if n == 1 or mode == 'different' else 'ad-' + 'e' * 16
                        else:
                            shown['alias_of'] = base['ad']

                report = self.discover(change, mail=mail, here=(published_machine(), published_machine()))
                if expected == 'changed':
                    self.assertEqual(report['errors'], ['mail-routes-changed'])
                    self.assertEqual(report['machines'], [])
                else:
                    self.assertTrue(report['proven'], report)
                    sibling, chosen = [row['mail_route'] for row in report['machines'][0]['accounts']]
                    self.assertEqual(sibling['status'], 'reported')
                    self.assertEqual(chosen['status'], expected)
                    self.assertEqual(chosen['alias_declaration'], 'declared' if mode == 'intermediate' else 'unconfirmed')
                    self.assertEqual('mailbox_key' in chosen, expected == 'reported')
                for private in ('ad-', 'private@invalid.example'):
                    self.assertNotIn(private, json.dumps(report))

    def test_inferred_tag_uses_the_unique_live_base_row_across_both_reads(self):
        extra = 'ad-' + 'e' * 16
        replacement = 'ad-' + 'f' * 16
        for mode, expected in (
                ('unrelated', 'reported'), ('retired', 'reported'), ('retired-protected', 'reported'),
                ('live', 'unconfirmed'), ('protected', 'unconfirmed'), ('restored', 'unconfirmed'),
                ('unwritten', 'unconfirmed'), ('arrives', 'changed'), ('replaced', 'changed'),
                ('explicit-alias', 'reported'), ('direct-observation', 'reported')):
            with self.subTest(mode=mode):
                mail = self.mail_view()
                base = mail['addresses'][3]
                base['row'] = None  # The native lifecycle of this restored row is live.
                if mode == 'direct-observation':
                    mail['addresses'][1]['mailbox']['via'] = []

                def change(cmd, view, occurrence):
                    scan, noun = cmd == ADDRESS_SCAN, cmd[2] == 'emails'
                    summary = cmd == ['aterm', 'db', 'alab', 'mail']
                    if not (scan or noun or summary):
                        return
                    rows = view['rows' if scan else 'emails' if noun else 'addresses']
                    if not summary:
                        rows[1]['addr'] = 'relay+moon@example.invalid'
                        rows[3]['row'] = None
                    if scan and mode != 'explicit-alias':
                        del rows[1]['alias_of']
                    if mode == 'arrives' and occurrence == 1:
                        return
                    pk = replacement if mode == 'replaced' and occurrence == 2 else extra
                    word = 'live'
                    if mode.startswith('retired'):
                        word = 'retired'
                    elif mode in ('restored', 'unwritten'):
                        word = None
                    if summary:
                        rows.append({'ad': pk, 'row': word, 'accounts': [], 'mailbox': None})
                    else:
                        spelling = 'else@example.invalid' if mode == 'unrelated' else ' RELAY@EXAMPLE.INVALID '
                        other = {'pk' if scan else 'ad': pk, 'addr': spelling, 'row': word}
                        if scan and mode == 'unwritten':
                            del other['row']
                        if scan and mode in ('protected', 'retired-protected'):
                            other['protected'] = True
                        rows.append(other)

                report = self.discover(change, mail=mail, here=(published_machine(), published_machine()))
                if expected == 'changed':
                    self.assertEqual(report['errors'], ['mail-routes-changed'])
                    self.assertEqual(report['machines'], [])
                else:
                    self.assertTrue(report['proven'], report)
                    first, second = [row['mail_route'] for row in report['machines'][0]['accounts']]
                    self.assertEqual(first['status'], 'reported')
                    self.assertEqual(second['status'], expected)
                    self.assertEqual('mailbox_key' in second, expected == 'reported')
                for private in (extra, replacement, base['ad'], 'relay@example.invalid'):
                    self.assertNotIn(private, json.dumps(report).lower())

    def test_inferred_tag_after_an_explicit_alias_requires_a_well_formed_base(self):
        for local, expected in (('relay', 'reported'), ('bad base', 'unconfirmed')):
            with self.subTest(local=local):
                mail = self.mail_view()
                _, selected, tagged, base = mail['addresses']
                selected['mailbox']['via'] = [tagged['ad'], base['ad']]
                tagged['mailbox'] = dict(selected['mailbox'], via=[base['ad']])

                def change(cmd, view, _):
                    if cmd == ADDRESS_SCAN or cmd[2] == 'emails':
                        rows = view.get('rows', view.get('emails'))
                        rows[2]['addr'] = local + '+tag@example.invalid'
                        rows[3]['addr'] = local + '@example.invalid'
                        if cmd == ADDRESS_SCAN:
                            rows[1]['alias_of'] = tagged['ad']
                    elif cmd[5:6] == ['address'] and cmd[6] in (tagged['ad'], base['ad']):
                        # Older text is visible in both tables; neither optional get
                        # is available to signal that its addr would be raw.
                        view.clear()

                report = self.discover(change, mail=mail)
                self.assertTrue(report['proven'], report)
                route = report['machines'][0]['accounts'][1]['mail_route']
                self.assertEqual(route['alias_declaration'], 'declared')
                self.assertEqual(route['status'], expected)
                self.assertEqual('mailbox_key' in route, expected == 'reported')
                self.assertNotIn(local + '@example.invalid', json.dumps(report))

    def test_inferred_tag_counts_all_candidate_spellings_and_requires_a_valid_base(self):
        tag, base, other = ('ad-' + c * 16 for c in '123')
        mailbox = 'mb-' + 'a' * 16

        def route(tagged, plain, candidate):
            typed = {tag: {'addr': tagged}, base: {'addr': plain}, other: {'addr': candidate}}
            return PROBE.settled_address_route(tag, mailbox, 'observed', [base], typed,
                                               lambda _pk, _field: (), {}, self.fail)

        self.assertTrue(route('list+tag@example.invalid', 'list@example.invalid',
                              {'conflict': ['one@example.invalid', None]}))
        self.assertFalse(route('list+tag@example.invalid', 'list@example.invalid',
                               {'conflict': ['one@example.invalid', ' LIST@example.INVALID ']}))
        self.assertFalse(route('bad base+tag@example.invalid', 'bad base@example.invalid', None))

    def test_a_failed_alias_hint_precedes_its_parents_provider_observation(self):
        for variant, index, hint, expected in (
                ('provisional-null', 3, {'conflict': [None, 'mb-' + 'c' * 16]}, 'unconfirmed'),
                ('settled-null', 3, None, 'reported'),
                ('owner-before-hint', 0, {'conflict': [None, 'mb-' + 'c' * 16]}, 'reported'),
                ('observation-before-hint', 1, {'conflict': [None, 'mb-' + 'c' * 16]}, 'reported')):
            with self.subTest(variant=variant):
                mail = self.mail_view()
                # second.alias_of points to relay. There is no effective delivery
                # for relay, so the native display uses second's own observation.
                mail['addresses'][3]['mailbox'] = None
                mail['addresses'][1]['mailbox']['via'] = []
                def change(cmd, answer, occurrence):
                    if cmd == ADDRESS_SCAN:
                        answer['rows'][index]['mailbox_hint'] = hint
                result = self.discover(change, mail=mail)
                self.assertTrue(result['proven'], result)
                first, second = [row['mail_route'] for row in result['machines'][0]['accounts']]
                self.assertEqual(first['status'], 'reported')
                self.assertEqual(second['status'], expected)
                self.assertEqual('mailbox_key' in second, expected == 'reported')
                self.assertNotIn('mb-' + 'c' * 16, json.dumps(result))

    def test_route_becoming_disputed_between_typed_reads_withholds_the_report(self):
        def change(cmd, answer, occurrence):
            if cmd == ADDRESS_SCAN and occurrence == 2:
                answer['rows'][3]['protected'] = {'conflict': [False, True]}
        result = self.discover(change, mail=self.mail_view())
        self.assertFalse(result['proven'])
        self.assertEqual(result['errors'], ['mail-routes-changed'])
        self.assertEqual(result['machines'], [])
        self.assertNotIn('mb-', json.dumps(result))

    def test_mail_route_checks_explicit_flags_only_on_cells_that_decide_delivery(self):
        for scenario, address, field, trusted in (
                ('owner', 0, 'mailbox', False), ('owner', 0, 'protected', False),
                ('owner', 0, 'mailbox_hint', True), ('owner', 0, 'alias_of', True),
                ('observed', 1, 'mailbox', False), ('observed', 1, 'alias_of', False),
                ('observed', 1, 'mailbox_hint', True),
                ('discarded', 3, 'mailbox_hint', False), ('discarded', 1, 'mailbox_hint', True),
                ('protected-relay', 3, 'protected', False), ('protected-relay', 3, 'mailbox', True),
                ('retired-relay', 3, 'row', False), ('retired-relay', 3, 'protected', True),
                ('tag', 3, 'protected', False), ('tag', 3, 'mailbox', True),
                ('tag', 3, 'alias_of', True), ('tag', 3, 'mailbox_hint', True)):
            for kind in ('raw', 'conflicts'):
                with self.subTest(scenario=scenario, field=field, kind=kind, address=address):
                    mail = self.mail_view()
                    if scenario in ('observed', 'discarded', 'protected-relay', 'retired-relay'):
                        mail['addresses'][1]['mailbox']['via'] = []
                    if scenario in ('discarded', 'protected-relay', 'retired-relay'):
                        mail['addresses'][3]['mailbox'] = None
                    if scenario == 'retired-relay':
                        mail['addresses'][3]['row'] = 'retired'
                    pk = mail['addresses'][address]['ad']

                    def mark(cmd, answer, _):
                        if cmd == ADDRESS_SCAN or cmd[2] == 'emails':
                            rows = answer.get('rows', answer.get('emails'))
                            if scenario == 'tag':
                                rows[1]['addr'] = 'relay+tag@example.invalid'
                            if scenario == 'retired-relay':
                                rows[3]['row'] = 'retired'
                        if cmd == ADDRESS_SCAN:
                            if scenario in ('observed', 'tag'):
                                rows[1].pop('alias_of')
                            if scenario == 'protected-relay':
                                rows[3]['protected'] = True
                        if cmd[5:7] == ['address', pk]:
                            answer[kind].append(field)

                    result = self.discover(mark, mail=mail)
                    self.assertTrue(result['proven'], result)
                    routes = [a['mail_route'] for a in result['machines'][0]['accounts']]
                    selected = 0 if scenario == 'owner' else 1
                    self.assertEqual(routes[selected]['status'], 'reported' if trusted else 'unconfirmed')
                    self.assertEqual('mailbox_key' in routes[selected], trusted)
                    self.assertEqual(routes[1 - selected]['status'], 'reported')

    def test_consulted_mailbox_value_and_flags_must_agree_with_each_read(self):
        for change in ('value', 'raw-to-conflict', 'second-read-conflict'):
            mail = self.mail_view()
            pk = mail['addresses'][0]['ad']

            def unstable(cmd, answer, occurrence):
                if cmd[5:7] != ['address', pk]:
                    return
                if change == 'value':
                    answer['row']['mailbox'] = 'mb-' + '9' * 16
                elif change == 'raw-to-conflict' and occurrence == 1:
                    answer['raw'].append('mailbox')
                elif occurrence == 2:
                    answer['conflicts'].append('mailbox')

            result = self.discover(unstable, mail=mail)
            self.assertEqual(result['errors'], ['mail-routes-changed'], change)
            self.assertEqual(result['machines'], [])
            for private in ('ad-', 'mb-', '@'):
                self.assertNotIn(private, json.dumps(result))

    def test_mail_views_withhold_routes_after_a_refusal_or_retained_read(self):
        cases = [{field: 'private@reason.invalid'} for field in ('kept', 'refused', 'problem')]
        cases.append({'registry_source': 'held_commons', 'pending_cells': 0})
        for change in cases:
            for command, source in ((['alab', 'mail'], 'mail-view'), (['table', 'get'], 'mailbox-detail')):
                def refused(cmd, answer, occurrence):
                    if cmd[2:4] == command and occurrence == 2:
                        answer.update(change)
                result = self.discover(refused, mail=self.mail_view())
                self.assertFalse(result['proven'], result)
                self.assertEqual(result['machines'], [])
                reason = 'registry-view-unconfirmed' if 'registry_source' in change else f'{source}-unconfirmed'
                self.assertEqual(result['errors'], [reason])
                self.assertNotIn('@', json.dumps(result))
                self.assertNotIn('mb-', json.dumps(result))

    def test_failed_metadata_command_names_the_source_and_redacts_selected_keys(self):
        def failed(command):
            return None, {'command': command, 'error': 'command-failed', 'exit_code': 1}

        for collection in (None, 'account', 'address', 'mailbox'):
            with self.subTest(collection=collection):
                receipts = []
                reason = 'mail-view' if collection is None else f'{collection}-detail'
                with self.assertRaisesRegex(ValueError, f'^{reason}-unavailable$'):
                    PROBE.read_mail_metadata(failed, receipts, collection, 'private-key')
                command = receipts[0]['command']
                if collection is None:
                    self.assertEqual(command, ['aterm', 'db', 'alab', 'mail'])
                else:
                    self.assertEqual(command[5:7], [collection, f'<selected-{collection}>'])
                    self.assertNotIn('private-key', json.dumps(receipts))

    def test_unconfirmed_routes_never_name_an_inbox_or_disrupt_other_rows(self):
        for change, expected in (
                (lambda view: view['addresses'][0].update(mailbox=None), {'status': 'unconfirmed'}),
                (lambda view: view['addresses'][0]['mailbox'].update(how='hint'),
                 {'status': 'unconfirmed', 'evidence': 'hint'}),
                (lambda view: view['addresses'][0]['mailbox'].update(how='routed'),
                 {'status': 'unconfirmed', 'evidence': 'routed'}),
                (lambda view: view['addresses'][0]['mailbox'].update(mb='mb-' + 'f' * 16),
                 {'status': 'unconfirmed', 'evidence': 'owner'})):
            view = self.mail_view()
            change(view)
            result = self.discover(mail=view)
            self.assertTrue(result['proven'], result)
            accounts = result['machines'][0]['accounts']
            self.assertEqual(accounts[0]['mail_route'], {**expected, 'alias_declaration': 'declared'})
            self.assertEqual(accounts[1]['mail_route']['status'], 'reported')
            with patch.object(PROBE, 'probe', return_value=result) as check, patch.object(sys, 'stdout', io.StringIO()):
                self.assertEqual(PROBE.main(['--machine', 'm3', '--show-mail-routes']), 1)
                self.assertTrue(check.call_args.kwargs['show_mail_routes'])

    def test_null_route_without_a_settled_usable_direct_owner(self):
        owner = 'mb-' + 'b' * 16
        for mode in ('unassigned', 'protected', 'disputed-protection', 'disputed-owner', 'absent-box',
                     'retired-box', 'disputed-box'):
            with self.subTest(mode=mode):
                view = self.mail_view()
                view['addresses'][0]['mailbox'] = None
                if mode == 'retired-box':
                    view['mailboxes'][1]['row'] = 'retired'

                def change(cmd, answer, _):
                    if cmd == ADDRESS_SCAN:
                        address = answer['rows'][0]
                        address.pop('alias_of')
                        address['mailbox'] = {
                            'unassigned': None, 'absent-box': 'mb-' + 'c' * 16,
                            'disputed-owner': {'conflict': [None, owner]},
                        }.get(mode, owner)
                        if mode in ('protected', 'disputed-protection'):
                            address['protected'] = True if mode == 'protected' else {'conflict': [True, False]}
                    if mode == 'disputed-box' and cmd[5:7] == ['mailbox', owner]:
                        answer['conflicts'].append('row')

                result = self.discover(change, mail=view)
                self.assertTrue(result['proven'], result)
                first, second = [a['mail_route'] for a in result['machines'][0]['accounts']]
                self.assertEqual(first, {'status': 'no-effective-route', 'alias_declaration': 'absent'})
                self.assertEqual(second['status'], 'reported')
                self.assertNotIn(owner, json.dumps(result))

    def test_null_route_with_a_changing_direct_owner_withholds_the_report(self):
        view = self.mail_view()
        view['addresses'][0]['mailbox'] = None

        def change(cmd, answer, occurrence):
            if cmd == ADDRESS_SCAN and occurrence == 2:
                answer['rows'][0]['mailbox'] = 'mb-' + 'b' * 16

        result = self.discover(change, mail=view)
        self.assertFalse(result['proven'], result)
        self.assertEqual(result['errors'], ['mail-routes-changed'])
        self.assertEqual(result['machines'], [])
        self.assertNotIn('mb-', json.dumps(result))

    def test_null_route_checks_an_explicit_alias_chain_for_a_settled_owner(self):
        for mode in ('one-hop', 'two-hops', 'cycle', 'protected', 'retired', 'raw-link', 'raw-owner',
                     'missing-link-detail', 'missing-owner-detail', 'missing-summary-address',
                     'retired-box', 'observation-only', 'retargeted', 'owner-changed'):
            with self.subTest(mode=mode):
                mail = self.mail_view()
                first, second, terminal, relay = mail['addresses']
                box = mail['mailboxes'][1]['mb']
                second['mailbox'] = None
                relay['mailbox'] = {'how': 'owner', 'mb': box, 'via': []}
                if mode == 'two-hops':
                    relay['mailbox']['via'] = [terminal['ad']]
                if mode == 'missing-summary-address':
                    mail['addresses'].remove(relay)
                if mode == 'retired-box':
                    mail['mailboxes'][1]['row'] = 'retired'
                if mode == 'retired':
                    relay['row'] = 'retired'
                if mode == 'observation-only':
                    relay['mailbox']['how'] = 'observed'

                def change(cmd, answer, occurrence):
                    if cmd == ['aterm', 'db', 'alab', 'mail'] and occurrence == 2:
                        if mode == 'retargeted':
                            answer['addresses'][3]['mailbox']['via'] = [terminal['ad']]
                        if mode == 'owner-changed':
                            answer['addresses'][3]['mailbox']['mb'] = first['mailbox']['mb']
                    if mode == 'retired' and cmd[2] == 'emails':
                        answer['emails'][3]['row'] = 'retired'
                    if cmd == ADDRESS_SCAN:
                        _, start, end, middle = answer['rows']
                        if mode == 'cycle':
                            middle['alias_of'] = start['pk']
                        elif mode == 'two-hops':
                            middle['alias_of'] = end['pk']
                            end['mailbox'] = box
                        elif mode != 'observation-only':
                            middle['mailbox'] = box
                        if mode == 'protected':
                            middle['protected'] = True
                        if mode == 'retired':
                            middle['row'] = 'retired'
                        if mode == 'retargeted' and occurrence == 2:
                            del middle['mailbox']
                            middle['alias_of'] = end['pk']
                            end['mailbox'] = box
                        if mode == 'owner-changed' and occurrence == 2:
                            middle['mailbox'] = first['mailbox']['mb']
                    if cmd[5:7] == ['address', second['ad']]:
                        if mode == 'raw-link':
                            answer['raw'].append('alias_of')
                        elif mode == 'missing-link-detail':
                            answer.clear()
                    if cmd[5:7] == ['address', relay['ad']]:
                        if mode == 'raw-owner':
                            answer['raw'].append('mailbox')
                        elif mode == 'missing-owner-detail':
                            answer.clear()

                result = self.discover(change, mail=mail)
                if mode in ('retargeted', 'owner-changed'):
                    self.assertEqual(result['errors'], ['mail-routes-changed'])
                    self.assertEqual(result['machines'], [])
                    self.assertNotIn('@', json.dumps(result))
                else:
                    self.assertTrue(result['proven'], result)
                    sibling, selected = [a['mail_route'] for a in result['machines'][0]['accounts']]
                    self.assertEqual(sibling['status'], 'reported')
                    self.assertEqual(selected['status'], 'unconfirmed' if mode in ('one-hop', 'two-hops')
                                     else 'no-effective-route')
                    self.assertNotIn('mailbox_key', selected)
                for private in (box, terminal['ad'], relay['ad']):
                    self.assertNotIn(private, json.dumps(result))

    def test_mail_route_verification_withholds_changes_including_alias_chain(self):
        for change in (
                lambda v: v.update(complete=False), lambda v: v.update(state='held'),
                lambda v: v['addresses'].append(v['addresses'][0]), lambda v: v['mailboxes'].append(v['mailboxes'][0]),
                lambda v: v['addresses'][0].update(accounts=[]), lambda v: v['addresses'][0].pop('mailbox'),
                lambda v: v['addresses'][0]['mailbox'].update(how='private@wrong.invalid'),
                lambda v: v['addresses'][0]['mailbox'].update(via=['ad-' + '1' * 16]),
                lambda v: v['addresses'][0]['mailbox'].update(via=['ad-' + '3' * 16]),
                lambda v: v['addresses'][0]['mailbox'].update(mb='mb-' + 'b' * 16),
                lambda v: v['mailboxes'][0].update(row='retired')):
            def later(cmd, view, occurrence):
                if cmd[-1] == 'mail' and occurrence == 2:
                    change(view)
            result = self.discover(later, mail=self.mail_view(), here=(published_machine(), published_machine()))
            self.assertFalse(result['proven'], result)
            self.assertEqual(result['machines'], [])
            self.assertNotIn('@', json.dumps(result))
            self.assertNotIn('mb-', json.dumps(result))

    def test_here_corroborates_the_published_machine_key_and_exposes_no_labels_or_seats(self):
        data, view = fixture(), details()
        private = 'private-native@example.invalid'
        data['accounts']['rows'][0]['label'] = private
        data['observations']['rows'][0]['row']['cli'] = private
        for row in (data['canonical']['machines'][0], data['machines']['computers'][0], view[M1]):
            row['label'] = private
        before, after = published_machine(), published_machine(rev=17)
        # Only published rows participate. A different machine or queued edit can change between reads.
        after.update(queued=1)
        after['rows'].append({'pk': M2, 'live': False, 'cells': {'label': private}, 'values': {'label': private}})
        for expected_accounts in ((), [A1, A2]):
            result = PROBE.probe_here(2, here_reader(reader_for(data, view), before, after),
                                      expected_accounts=expected_accounts)
            self.assertTrue(result['proven'], result)
            self.assertEqual(result['requested_machine'], M1)
            self.assertEqual(result['machines'][0]['machine_facts_seat_count'], 1)
            self.assertEqual(PROBE.association_for(result, A2),
                             {'status': 'associated', 'sources': ['retained-observation']})
            self.assertEqual(result['commands'][0]['command'], PROBE.PUBLISHED_MACHINES)
            self.assertEqual(result['commands'][-1]['command'], PROBE.PUBLISHED_MACHINES)
            for sensitive in ('@', 'PRIVATE', S1):
                self.assertNotIn(sensitive, json.dumps(result))
        data['observations']['rows'].clear()
        view[M1].update(prefers=[], places=[])
        result = PROBE.probe_here(1, here_reader(reader_for(data, view), before, after))
        self.assertEqual(result['status'], 'insufficient-accounts')
        self.assertEqual(PROBE.association_for(result, A1), {'status': 'missing'})

    def test_here_requires_one_live_published_marker_and_consistent_typed_cells(self):
        for change in (
            lambda v: v.update(file='ai/links'), lambda v: v.update(state='held'),
            lambda v: v.update(why='membership'), lambda v: v.update(complete=False),
            lambda v: v.update(problem='private@example.invalid'), lambda v: v.update(kept='behind'),
            lambda v: v.update(refused='private@example.invalid'), lambda v: v['columns'].remove('here'),
            lambda v: v.update(rows=[]), lambda v: v['rows'].append(dict(v['rows'][0], pk=M2)),
            lambda v: v['rows'].append(dict(v['rows'][0], cells={}, values={})),
            lambda v: v['rows'][0].update(live=False), lambda v: v['rows'][0].update(pk=A1),
            lambda v: v['rows'][0]['cells'].update(here='conflicted'),
            lambda v: v['rows'][0]['values'].pop('here'), lambda v: v['rows'][0]['cells'].pop('here'),
            lambda v: v['rows'][0]['values'].update(seats=S2), lambda v: v['rows'][0]['cells'].pop('seats'),
            lambda v: v['rows'][0]['values'].update(row='retired'),
            lambda v: v['rows'].append({'pk': M2, 'live': True, 'cells': {}, 'values': {'here': 'yes'}}),
        ):
            source = published_machine()
            change(source)
            with self.subTest(source=source):
                result = PROBE.probe_here(1, here_reader(self.fail, source), show_logins=True)
                self.assertEqual(result['status'], 'unavailable')
                self.assertEqual(result['errors'], ['current-machine-binding-unconfirmed'])
                self.assertIsNone(result['requested_machine'])
                self.assertFalse(result['machines'])
                self.assertNotIn('@', json.dumps(result))
        for source in (None, published_machine(seats=()), published_machine(seats=(S1, S1))):
            self.assertFalse(PROBE.probe_here(1, here_reader(self.fail, source))['proven'])

    def test_here_refuses_a_typed_machine_with_a_different_binding(self):
        # The same seat count on a different signed UUID cannot corroborate the published row.
        for source in (published_machine(M2), published_machine(seats=(S2,))):
            result = PROBE.probe_here(1, here_reader(self.reader, source, source))
            self.assertEqual(result['errors'], ['current-machine-binding-unconfirmed'])
            self.assertEqual(result['machines'], [])
            self.assertIsNone(result['requested_machine'])
            self.assertEqual(PROBE.association_for(result, A1),
                             {'status': 'unverified', 'reason': 'current-machine-binding-unconfirmed'})

    def test_here_rechecks_after_address_discovery_and_withholds_prior_accounts_on_change(self):
        first = published_machine()
        result = self.discover(required=2, here=(first, published_machine(rev=2)))
        self.assertTrue(result['proven'], result)
        self.assertEqual([row['registered_login'] for row in result['machines'][0]['accounts']],
                         ['first@example.invalid', 'second@example.invalid'])
        self.assertNotIn('inbox@', json.dumps(result))
        for after in (None, published_machine(M2), published_machine(seats=(S2,))):
            for logins in ((), ('first@example.invalid',)):
                result = self.discover(here=(first, after), expected_logins=logins)
                self.assertEqual(result['status'], 'unavailable')
                self.assertFalse(result['proven'])
                self.assertIsNone(result['requested_machine'])
                self.assertEqual(result['errors'], ['current-machine-binding-unconfirmed'])
                self.assertEqual(result['machines'], [])
                for private in ('@', M1, A1, S1):
                    self.assertNotIn(private, json.dumps(result))

    def test_here_preserves_only_recognized_published_file_metadata_on_failure(self):
        view = dict(published_machine(), state='never', complete=False, why='custody',
                    problem='PRIVATE-NATIVE-DETAIL')
        fields = {'state': 'never', 'complete': False}
        for change, expected in [
            ({}, dict(fields, why='custody')),
            ({'why': 'PRIVATE-UNRECOGNIZED'}, fields),
            ({'file': 'PRIVATE/other'}, None),
            ({'state': 'PRIVATE-STATE'}, None),
            ({'complete': 0}, None),
            ({'complete': 'false'}, None),
        ]:
            with self.subTest(change=change):
                result = PROBE.probe_here(2, here_reader(None, dict(view, **change)))
                self.assertEqual(result['errors'], [PROBE.HERE_UNCONFIRMED])
                receipt, = result['commands']
                self.assertEqual(receipt['command'], PROBE.PUBLISHED_MACHINES)
                if expected is None:
                    self.assertNotIn('published_file', receipt)
                else:
                    self.assertEqual(receipt['published_file'], expected)
                for private in ('PRIVATE', '@', M1, S1):
                    self.assertNotIn(private, json.dumps(result))

    def test_here_final_published_read_reports_its_own_state(self):
        base = reader_for(fixture(), details())
        never = {'file': 'lab/machines', 'state': 'never', 'complete': False, 'why': 'custody'}
        for last in (never, None):
            with self.subTest(last=last):
                result = PROBE.probe_here(2, here_reader(base, published_machine(), last))
                self.assertFalse(result['proven'])
                self.assertEqual(result['errors'], [PROBE.HERE_UNCONFIRMED])
                self.assertEqual(result['machines'], [])
                self.assertEqual(len(result['commands']), 2)
                final = result['commands'][-1]
                if last is None:
                    self.assertEqual(final.get('error'), 'command-failed')
                    self.assertNotIn('published_file', final)
                else:
                    self.assertEqual(final.get('published_file'),
                                     {'state': 'never', 'complete': False, 'why': 'custody'})

    def test_here_cli_requires_one_location_and_validates_selectors_before_reading(self):
        for args in ([], ['--here', '--machine', 'm3'], ['--here', '--expect-account', 'invalid'],
                     ['--here', '--expect-login', 'invalid'], ['--here', '--expect-at-least', '0']):
            with self.subTest(args=args), self.assertRaises(SystemExit) as error, patch.object(sys, 'stderr', io.StringIO()):
                PROBE.main(args)
            self.assertEqual(error.exception.code, 2)
        with patch.object(PROBE, 'probe_here', return_value={'status': 'unavailable', 'proven': False}) as check, \
                patch.object(sys, 'stdout', io.StringIO()):
            self.assertEqual(PROBE.main(['--here', '--show-logins']), 2)
            check.assert_called_once_with(1, expected_accounts=[], expected_logins=[], show_logins=True)

    def test_discovery_shows_only_current_registered_addresses_in_account_key_order(self):
        result = self.discover(required=2)
        self.assertTrue(result['proven'], result['errors'])
        machine, = result['machines']
        self.assertEqual(machine['openai_account_count'], 2)
        self.assertEqual(machine['accounts'], [
            {'account_key': A1, 'registered_login': 'first@example.invalid',
             'sources': ['machine-preference', 'preference']},
            {'account_key': A2, 'registered_login': 'second@example.invalid', 'sources': ['placement']},
        ])
        for private in ('canonical@', 'inbox@', 'retained@', 'PRIVATE', 'ad-' + '3' * 16, 'ac-' + 'd' * 16):
            self.assertNotIn(private, json.dumps(result))
        calls = [receipt['command'] for receipt in result['commands']]
        self.assertEqual(calls.count(PROBE.QUERIES['accounts']), 2)
        for name in ('canonical', 'machines', 'preferences'):
            self.assertEqual(calls.count(PROBE.QUERIES[name]), 2)
        detail = ['aterm', 'db', 'alab', 'machines', M1]
        addresses = ['aterm', 'db', 'table', 'scan', 'alab/registry', 'address', '--json']
        self.assertEqual([cmd for cmd in calls if cmd in (detail, addresses)], [addresses, detail, addresses, detail])

        def retained_only(cmd, answer, _):
            if cmd == detail:
                answer.update(prefers=[], places=[])

        none = self.discover(retained_only)
        self.assertEqual(none['status'], 'insufficient-accounts')
        self.assertEqual(none['machines'][0]['accounts'], [])
        self.assertNotIn('@', json.dumps(none))

    def test_current_assignments_do_not_read_retained_account_observations(self):
        for here in (None, (published_machine(), published_machine())):
            for mail in (None, self.mail_view()):
                for assigned in (True, False):
                    with self.subTest(here=bool(here), routes=bool(mail), assigned=assigned):
                        def current(cmd, answer, _):
                            self.assertNotEqual(cmd, PROBE.QUERIES['observations'])
                            if not assigned and cmd == ['aterm', 'db', 'alab', 'machines', M1]:
                                answer.update(places=[], prefers=[])

                        report = self.discover(current, required=2, here=here, mail=mail)
                        self.assertEqual(report['status'], 'matched' if assigned else 'insufficient-accounts')
                        accounts = report['machines'][0]['accounts']
                        self.assertEqual([row['account_key'] for row in accounts], [A1, A2] if assigned else [])
                        if assigned and mail:
                            self.assertEqual([row['mail_route']['status'] for row in accounts], ['reported'] * 2)

    def test_inventory_and_explicit_selectors_still_require_retained_account_observations(self):
        retained = PROBE.probe('m3', 2, self.reader, expected_accounts=[A2])
        self.assertEqual(PROBE.association_for(retained, A2),
                         {'status': 'associated', 'sources': ['retained-observation']})

        scan = PROBE.QUERIES['observations']

        def unavailable(cmd):
            if cmd == scan:
                return None, {'command': cmd, 'error': 'timeout'}
            return self.reader(cmd)

        for options in ({}, {'expected_accounts': [A2]}, {'expected_logins': ['second@example.invalid']}):
            with self.subTest(options=options):
                report = PROBE.probe('m3', 2, unavailable, **options)
                self.assertEqual(report['status'], 'unavailable')
                self.assertEqual(report['errors'], ['metadata-query-failed'])
                self.assertEqual(report['machines'], [])
                self.assertIn({'command': scan, 'error': 'timeout'}, report['commands'])

    def test_discovery_cannot_substitute_a_canonical_address_or_skip_reverse_uniqueness(self):
        first = 'ad-' + '1' * 16
        def change_for(case):
            def change(cmd, answer, _):
                if cmd == PROBE.QUERIES['accounts']:
                    if case == 'missing-login':
                        del answer['rows'][0]['login']
                    if case == 'multiple-owners':
                        answer['rows'][-1]['login'] = first
                    if case == 'disputed-owner':
                        answer['rows'][-1]['login'] = {'conflict': [first, None]}
                    if case == 'selected-conflict':
                        answer['rows'][0]['login'] = {'conflict': [first, 'ad-' + '3' * 16]}
                if cmd == ADDRESS_SCAN:
                    if case == 'duplicate-spelling':
                        answer['rows'].append({'pk': 'ad-' + '4' * 16, 'addr': 'first@EXAMPLE.invalid'})
                    if case == 'disputed-spelling':
                        answer['rows'].append({'pk': 'ad-' + '4' * 16,
                                               'addr': {'conflict': ['first@example.invalid', None]}})
                    if case == 'retired-address':
                        answer['rows'][0]['row'] = 'retired'
                    if case == 'missing-address':
                        answer['rows'].pop(0)
                    if case == 'invalid-address-key':
                        answer['rows'][-1]['pk'] = {'conflict': []}
            return change
        for case in ('missing-login', 'multiple-owners', 'disputed-owner', 'selected-conflict',
                     'duplicate-spelling', 'disputed-spelling', 'retired-address', 'missing-address', 'invalid-address-key'):
            with self.subTest(case=case):
                result = self.discover(change_for(case))
                self.assertEqual(result['status'], 'unavailable')
                self.assertEqual(result['machines'], [])
                self.assertNotIn('@', json.dumps(result))

    def test_only_conflicts_that_can_claim_a_selected_login_block_its_lookup(self):
        for field, private, selected in (('login', 'ad-' + '3' * 16, 'ad-' + '1' * 16),
                                         ('addr', 'offscope@example.invalid', ' FIRST@EXAMPLE.INVALID ')):
            for options in ({}, {'aliases': True}, {'expected_logins': ['first@example.invalid']}):
                with self.subTest(field=field, options=options):
                    unrelated = login_field_fixture(field, {'conflict': [None, private]}, '⚠  ‖ ' + private)
                    report = self.discover(unrelated, **options)
                    self.assertTrue(report['proven'], report)
                    accounts = report['machines'][0]['accounts']
                    if 'expected_logins' in options:
                        self.assertEqual(report['resolved_login_account_keys'], [A1])
                        self.assertNotIn('@', json.dumps(report))
                    else:
                        self.assertEqual([r['registered_login'] for r in accounts],
                                         ['first@example.invalid', 'second@example.invalid'])
                        if options.get('aliases'):
                            self.assertEqual([r['alias_declaration'] for r in accounts], ['declared', 'absent'])
                    self.assertNotIn(private, json.dumps(report))

                for read_number in (1, 2):
                    for row in (-1, 0):  # Another claimant or the selected row itself.
                        with self.subTest(field=field, options=options, read_number=read_number, row=row):
                            change = login_field_fixture(field, {'conflict': [private, selected]},
                                                         f'⚠ {private} ‖ {selected}', read_number=read_number, row=row)
                            report = self.discover(change, **options)
                            self.assertEqual(report['status'], 'unavailable')
                            self.assertEqual(report['machines'], [])
                            self.assertNotIn('resolved_login_account_keys', report)
                            self.assertNotIn('@', json.dumps(report))
                            self.assertNotIn('ad-', json.dumps(report))

    def test_unrelated_login_disputes_require_unambiguous_corroborated_candidates(self):
        for field, private in (('login', 'ad-' + '3' * 16), ('addr', 'offscope@example.invalid')):
            exact = '⚠  ‖ ' + private
            selected = 'first@example.invalid' if field == 'addr' else 'ad-' + '1' * 16
            hidden_claim = f'⚠ {private} ‖ {selected}'
            cases = [(hidden_claim, hidden_claim),  # A raw scalar could conceal a selected claimant.
                     ({'conflict': [None, private]}, private),  # Only a provisional choice is shown.
                     ({'conflict': [None, private]}, f'⚠ {private} ‖ '),
                     ({'conflict': [private, private]}, f'⚠ {private} ‖ {private}'),
                     ({'conflict': [private]}, '⚠ ' + private),
                     ({'conflict': [None, private], 'extra': True}, exact),
                     ({'conflict': [False, private]}, exact)]
            if field == 'addr':
                ambiguous = 'first@example.invalid ‖ ' + private
                cases.append(({'conflict': [ambiguous, None]}, f'⚠ {ambiguous} ‖ '))
                cases.append(({'conflict': ['', private]}, exact))
            else:
                cases.append(({'conflict': [None, 'invalid-key']}, '⚠  ‖ invalid-key'))
            for typed, shown in cases:
                for read_number in (1, 2):
                    with self.subTest(field=field, typed=typed, shown=shown, read_number=read_number):
                        change = login_field_fixture(field, typed, shown, read_number=read_number)
                        report = self.discover(change, aliases=True)
                        self.assertEqual(report['status'], 'unavailable')
                        self.assertEqual(report['machines'], [])
                        self.assertNotIn('@', json.dumps(report))
                        self.assertNotIn('ad-', json.dumps(report))

        for address in ('⚠local@example.invalid', 'local‖tag@example.invalid'):
            with self.subTest(address=address):
                selected = self.discover(login_field_fixture('addr', address, address, row=0), aliases=True)
                self.assertTrue(selected['proven'], selected)
                self.assertEqual(selected['machines'][0]['accounts'][0]['registered_login'], address)
                unrelated = login_field_fixture('addr', {'conflict': [address, None]}, f'⚠ {address} ‖ ')
                result = self.discover(unrelated, aliases=True)
                self.assertTrue(result['proven'], result)
                self.assertNotIn(address, json.dumps(result, ensure_ascii=False))

    def test_discovery_withholds_all_logins_on_a_changed_or_unconfirmed_read(self):
        for case in ('login-ref', 'address', 'account-retired', 'account-provider', 'ad-replaced',
                     'placement-removed', 'new-namesake', 'machine-retired', 'detail-held',
                     'address-partial', 'account-partial', 'address-pending'):
            def change(cmd, answer, which):
                if which != 2:
                    return
                if cmd == PROBE.QUERIES['accounts']:
                    if case == 'login-ref':
                        answer['rows'][0]['login'], answer['rows'][1]['login'] = (
                            answer['rows'][1]['login'], answer['rows'][0]['login'])
                    if case == 'account-retired':
                        answer['rows'][0]['row'] = 'retired'
                    if case == 'account-provider':
                        answer['rows'][0]['provider'] = 'other'
                    if case == 'ad-replaced':
                        answer['rows'][0]['login'] = 'ad-' + '4' * 16
                    if case == 'account-partial':
                        answer['complete'] = False
                if cmd == ADDRESS_SCAN:
                    if case == 'address':
                        answer['rows'][0]['addr'] = 'new-address@example.invalid'
                    if case == 'ad-replaced':
                        answer['rows'][0]['pk'] = 'ad-' + '4' * 16
                    if case == 'address-partial':
                        answer['complete'] = False
                    if case == 'address-pending':
                        answer.update(registry_source='commons', pending_cells=1)
                if cmd == PROBE.QUERIES['canonical']:
                    if case == 'new-namesake':
                        answer['machines'].append({'mc': M2, 'label': 'm3', 'row': 'live', 'uuids': []})
                    if case == 'machine-retired':
                        answer['machines'][0]['row'] = 'retired'
                if cmd[-1] == M1:
                    if case == 'placement-removed':
                        answer['places'] = []
                    if case == 'detail-held':
                        answer.update(registry_source='working', pending_cells=0)

            with self.subTest(case=case):
                result = self.discover(change)
                self.assertEqual(result['status'], 'unavailable')
                self.assertEqual(result['machines'], [])
                self.assertNotIn('@', json.dumps(result))

    def test_discovery_rechecks_placement_after_the_last_account_and_address_reads(self):
        for changed_during in (PROBE.QUERIES['accounts'],
                               ['aterm', 'db', 'table', 'scan', 'alab/registry', 'address', '--json']):
            moved = False

            def move(cmd, answer, which):
                nonlocal moved
                if cmd == changed_during and which == 2:
                    moved = True
                if moved and cmd[-1] == M1:
                    answer.update(prefers=[], places=[])

            with self.subTest(changed_during=changed_during):
                result = self.discover(move)
                self.assertTrue(moved)
                self.assertEqual(result['status'], 'unavailable')
                self.assertEqual(result['machines'], [])
                self.assertNotIn('@', json.dumps(result))

    def test_expected_login_rechecks_the_machine_before_confirming_or_reporting_a_missing_key(self):
        for case in ('placement-lost', 'placement-gained', 'placement-to-observation', 'observation-lost',
                     'seat-moved', 'renamed', 'held', 'missing-standing'):
            moved = False

            def move(cmd, answer, which):
                nonlocal moved
                if cmd[2] == 'emails' and which == 2:
                    moved = True
                if cmd == PROBE.QUERIES['observations']:
                    if case in ('placement-lost', 'placement-gained') or (case == 'observation-lost' and moved):
                        answer['rows'] = []
                if cmd[-1] == M1:
                    if (case == 'observation-lost' or (case == 'placement-gained' and not moved)
                            or (case in ('placement-lost', 'placement-to-observation') and moved)):
                        answer['places'] = []
                if moved and cmd == PROBE.QUERIES['canonical']:
                    if case == 'seat-moved':
                        answer['machines'][0]['uuids'] = [S2]
                    if case == 'renamed':
                        answer['machines'][0]['label'] = 'm4'
                if moved and cmd == PROBE.QUERIES['machines']:
                    if case == 'seat-moved':
                        answer['computers'][0]['seats'] = [S2]
                    if case == 'held':
                        answer.update(state='held', why='succession')
                    if case == 'missing-standing':
                        del answer['state']

            with self.subTest(case=case):
                result = self.discover(move, expected_logins=['second@example.invalid'])
                self.assertTrue(moved)
                self.assertEqual(result['status'], 'unavailable')
                self.assertEqual(result['machines'], [])
                self.assertNotIn('resolved_login_account_keys', result)
                self.assertNotIn('missing_account_keys', result)
                self.assertNotIn('@', json.dumps(result))

    def test_retained_only_login_uses_the_latest_observation_on_its_unchanged_machine(self):
        def refresh(cmd, answer, which):
            if cmd[-1] == M1:
                answer['places'] = []
            if cmd == PROBE.QUERIES['observations'] and which == 2:
                next(row for row in answer['rows'] if row['row']['account'] == A2)['row'].update(
                    state='idle', at=90)

        result = self.discover(refresh, expected_logins=['second@example.invalid'])
        self.assertTrue(result['proven'], result['errors'])
        account = next(row for row in result['machines'][0]['accounts'] if row['account_key'] == A2)
        self.assertEqual(account['sources'], ['retained-observation'])
        self.assertEqual(account['observations'], [
            {'seat': S1, 'cli': 'codex', 'state': 'idle', 'observed_at_ms': 90}])

    def test_named_login_reports_keep_unrelated_addresses_out_of_public_fields(self):
        private = 'other-machine@example.invalid'
        for selector, label in (('m3', 'm3'), (M1, private), (private, private)):
            def names(cmd, answer, _):
                if cmd == PROBE.QUERIES['canonical']:
                    answer['machines'][0]['label'] = label
                if cmd == PROBE.QUERIES['machines']:
                    answer['computers'][0]['label'] = label
                if cmd[-1] == M1:
                    answer['label'] = label
                if cmd == PROBE.QUERIES['observations']:
                    answer['rows'][0]['row']['cli'] = 'other-client@example.invalid'

            with self.subTest(selector=selector):
                result = self.discover(names, machine=selector, expected_logins=['first@example.invalid'])
                self.assertTrue(result['proven'], result['errors'])
                self.assertEqual(result['resolved_login_account_keys'], [A1])
                self.assertNotIn('@', json.dumps(result))
                self.assertEqual(PROBE.association_for(result, A1)['status'], 'associated')
                first = result['machines'][0]['accounts'][0]
                self.assertEqual(first['observations'], [{'seat': S1, 'state': 'idle', 'observed_at_ms': 10}])
                for mail in (None, self.mail_view()):
                    shown = self.discover(names, machine=selector, mail=mail)
                    self.assertTrue(shown['proven'], shown['errors'])
                    self.assertNotIn(private, json.dumps(shown))
                    self.assertEqual(shown['machines'][0]['accounts'][0]['registered_login'], 'first@example.invalid')

        unknown = self.discover(machine=private, expected_logins=['first@example.invalid'])
        self.assertEqual(unknown['status'], 'machine-not-found')
        self.assertNotIn('@', json.dumps(unknown))

        def drop_retained(cmd, answer, _):
            if cmd == PROBE.QUERIES['observations']:
                answer['rows'].pop()

        missing = self.discover(drop_retained, expected_logins=['retained@example.invalid'])
        self.assertEqual(missing['status'], 'missing-accounts')
        self.assertEqual(missing['missing_account_keys'], ['ac-' + 'd' * 16])
        self.assertNotIn('@', json.dumps(missing))

    def test_incomplete_raw_scans_cannot_hide_a_claimant_from_the_complete_native_view(self):
        for noun in ('emails', 'ai-accounts'):
            for case in ('additional-owner', 'changed-field', 'missing-field', 'partial', 'held', 'missing-standing'):
                def change(cmd, answer, which):
                    if cmd[2] != noun or which != failed_read:
                        return
                    table = answer[noun]
                    if case == 'additional-owner':
                        prefix = 'ad' if noun == 'emails' else 'ac'
                        table.append(dict(table[0], **{prefix: prefix + '-' + 'e' * 16}))
                    elif case == 'changed-field':
                        field = 'addr' if noun == 'emails' else 'login'
                        table[-1][field] = table[0][field]
                    elif case == 'missing-field':
                        del table[-1]['row']
                    elif case == 'partial':
                        answer['complete'] = False
                    elif case == 'held':
                        answer.update(state='held', why='succession')
                    else:
                        del answer['state']

                for failed_read in (1, 2):
                    for selected in ((), ('first@example.invalid',)):
                        with self.subTest(noun=noun, case=case, failed_read=failed_read, selected=selected):
                            result = self.discover(change, expected_logins=selected)
                            self.assertEqual(result['status'], 'unavailable')
                            self.assertEqual(result['machines'], [])
                            self.assertNotIn('resolved_login_account_keys', result)
                            self.assertNotIn('missing_account_keys', result)
                            self.assertNotIn('@', json.dumps(result))

    def test_discovery_cli_and_machine_ambiguity(self):
        def ambiguous(cmd, answer, _):
            if cmd == PROBE.QUERIES['canonical']:
                answer['machines'].append({'mc': M2, 'label': 'm3', 'row': 'live', 'uuids': []})

        result = self.discover(ambiguous)
        self.assertEqual(result['status'], 'ambiguous-machine')
        self.assertEqual(result['machines'], [])
        self.assertEqual([r['command'] for r in result['commands']],
                         [cmd for name, cmd in PROBE.QUERIES.items() if name != 'observations'])
        with patch.object(PROBE, 'probe', return_value={'proven': True}) as probe, \
                patch.object(sys, 'stdout', io.StringIO()):
            self.assertEqual(PROBE.main(['--machine', 'm3', '--show-logins']), 0)
            probe.assert_called_once_with('m3', 1, expected_accounts=[], expected_logins=[], show_logins=True)

    def test_duplicate_labels_never_combined_to_meet_expected_count(self):
        data, view = fixture(), details()
        data['machines']['computers'].append(
            {'mc': M2, 'label': 'm3', 'row': 'live', 'seats': [S2]})
        data['canonical']['machines'].append(
            {'mc': M2, 'label': 'm3', 'row': 'live', 'uuids': [S2]})
        data['observations']['rows'] = []
        view[M2] = {'complete': True, 'mc': M2, 'label': 'm3', 'row': 'live',
                    'facts': [{'uuid': S2}], 'places': [{'ac': A2}], 'prefers': []}
        result = PROBE.analyze('m3', 2, data, view)
        self.assertEqual(result['status'], 'ambiguous-machine')
        self.assertFalse(result['proven'])
        self.assertEqual([m['openai_account_count'] for m in result['machines']], [1, 1])
        selected = PROBE.analyze(M1, 1, data, view)
        self.assertTrue(selected['proven'])
        self.assertEqual(len(selected['machines']), 1)

    def test_discovery_with_a_missing_or_retired_machine_key_ignores_a_live_namesake(self):
        for target_state in ('absent', 'retired'):
            with self.subTest(target_state=target_state):
                def collision(cmd, answer, _):
                    if cmd == PROBE.QUERIES['canonical']:
                        answer['machines'][0]['label'] = M2
                        if target_state == 'retired':
                            answer['machines'].append({'mc': M2, 'row': 'retired', 'label': 'former', 'uuids': [S2]})
                    if cmd == PROBE.QUERIES['machines']:
                        answer['computers'][0]['label'] = M2
                        if target_state == 'retired':
                            answer['computers'].append({'mc': M2, 'row': 'retired', 'label': 'former', 'seats': [S2]})
                    if cmd[-1] == M1:
                        answer['label'] = M2

                result = self.discover(collision, machine=M2)
                self.assertEqual(result['status'], 'machine-not-found')
                self.assertFalse(result['proven'])
                self.assertEqual(result['machines'], [])
                self.assertNotIn('@', json.dumps(result))
                commands = [r['command'] for r in result['commands']]
                self.assertNotIn(ADDRESS_SCAN, commands)
                self.assertNotIn(['aterm', 'db', 'alab', 'machines', M1], commands)

    def test_missing_machine_requires_a_complete_typed_binding_view(self):
        for selector in (M2, 'unknown-label'):
            for invalid in ('incomplete', 'pending', 'malformed'):
                with self.subTest(selector=selector, invalid=invalid):
                    def change(cmd, view, _):
                        if cmd == PROBE.QUERIES['machines']:
                            if invalid == 'incomplete':
                                view['complete'] = False
                            elif invalid == 'pending':
                                view.update(registry_source='commons_with_local_edits', pending_cells=1)
                            else:
                                view['computers'][0]['seats'] = ['invalid']

                    report = self.discover(change, machine=selector, aliases=True)
                    self.assertEqual(report['status'], 'unavailable')
                    self.assertFalse(report['proven'])
                    self.assertEqual(report['machines'], [])
                    self.assertNotIn(ADDRESS_SCAN, [r['command'] for r in report['commands']])

    def test_missing_canonical_key_cannot_override_a_live_binding_for_that_key(self):
        for canonical in ('absent', 'retired', 'merged'):
            with self.subTest(canonical=canonical):
                def change(cmd, view, _):
                    if cmd == PROBE.QUERIES['canonical']:
                        row, = view['machines']
                        if canonical == 'absent':
                            view['machines'].clear()
                        elif canonical == 'retired':
                            row['row'] = 'retired'
                        else:
                            row['merged_into'] = M2

                report = self.discover(change, machine=M1, aliases=True)
                self.assertEqual(report['status'], 'unavailable')
                self.assertEqual(report['errors'], ['machine-association-changed'])
                self.assertEqual(report['machines'], [])
                self.assertNotIn('@', json.dumps(report))
                with patch.object(PROBE, 'probe', return_value=report), patch.object(sys, 'stdout', io.StringIO()):
                    self.assertEqual(PROBE.main(['--machine', M1, '--show-alias-declarations']), 2)

        data = fixture()
        data['canonical']['machines'].clear()
        inventory = PROBE.probe(M1, 1, reader_for(data, details()), expected_accounts=[A1])
        self.assertEqual(inventory['status'], 'unavailable')
        self.assertEqual(inventory['errors'], ['machine-association-changed'])
        self.assertNotIn('missing_account_keys', inventory)

    def test_authored_label_alone_does_not_contradict_canonical_absence(self):
        def rename(cmd, view, _):
            if cmd == PROBE.QUERIES['machines']:
                view['computers'][0]['label'] = 'former-name'

        report = self.discover(rename, machine='former-name', aliases=True)
        self.assertEqual(report['status'], 'machine-not-found')
        self.assertEqual(report['errors'], [])

    def test_discovery_selects_the_live_machine_by_key_even_when_another_label_matches(self):
        def collision(cmd, answer, _):
            if cmd == PROBE.QUERIES['canonical']:
                answer['machines'].append({'mc': M2, 'row': 'live', 'label': M1, 'uuids': [S2]})
            if cmd == PROBE.QUERIES['machines']:
                answer['computers'].append({'mc': M2, 'row': 'live', 'label': M1, 'seats': [S2]})

        for selector in (M1, 'm3'):
            with self.subTest(selector=selector):
                result = self.discover(collision, machine=selector)
                self.assertTrue(result['proven'], result)
                machine, = result['machines']
                self.assertEqual(machine['machine_key'], M1)
                self.assertEqual(result['requested_machine'], selector)
                self.assertEqual(machine.get('label'), 'm3' if selector == 'm3' else None)
                self.assertEqual([r['registered_login'] for r in machine['accounts']],
                                 ['first@example.invalid', 'second@example.invalid'])
                self.assertNotIn(['aterm', 'db', 'alab', 'machines', M2], [r['command'] for r in result['commands']])
                self.assertEqual(PROBE.association_for(result, A1)['status'], 'associated')

    def test_association_ignores_a_label_matching_a_different_machine_key(self):
        result = self.discover(machine=M1)
        result['requested_machine'] = M2
        result['machines'][0]['label'] = M2
        for account in (A1, A3):
            self.assertEqual(PROBE.association_for(result, account),
                             {'status': 'unverified', 'reason': 'registry-unavailable'})

    def test_disputed_authored_label_is_selected_by_canonical_name_or_key(self):
        data, view = fixture(), details()
        data['machines']['computers'][0]['label'] = 'm57'
        data['canonical']['machines'][0]['label'] = 'm57-2dc6'
        view[M1]['label'] = 'm57-2dc6'
        data['machines']['computers'].append(
            {'mc': M2, 'label': 'm57', 'row': 'live', 'seats': []})
        data['canonical']['machines'].append(
            {'mc': M2, 'label': 'm57', 'row': 'live', 'uuids': []})
        view[M2] = dict(view[M1], mc=M2, label='m57', facts=[], places=[], prefers=[])
        for selector in [M1, 'm57-2dc6']:
            result = PROBE.probe(selector, 2, reader_for(data, view))
            self.assertTrue(result['proven'])
            self.assertEqual(result['machines'][0]['machine_key'], M1)
            self.assertEqual(result['machines'][0]['label'], 'm57-2dc6')
        result = PROBE.probe('m57', 1, reader_for(data, view))
        self.assertEqual(result['status'], 'insufficient-accounts')
        self.assertEqual(result['machines'][0]['machine_key'], M2)

    def test_canonical_selection_cannot_join_conflicting_or_retired_seat_bindings(self):
        for change in [
            lambda d: d['canonical']['machines'][0].update(uuids=[S2]),
            lambda d: d['canonical']['machines'].append(d['canonical']['machines'][0].copy()),
            lambda d: d['canonical']['machines'].append(
                {'mc': M2, 'label': 'other', 'row': 'live', 'uuids': [S1]}),
            lambda d: d['machines']['computers'].append({'mc': None, 'seats': [S1]}),
            lambda d: d['machines']['computers'].append(
                {'mc': M2, 'label': 'other', 'row': 'live', 'seats': [S1]}),
            lambda d: d['machines']['computers'][0].update(row='retired'),
            lambda d: d['machines']['computers'][0].update(merged_into=M2),
        ]:
            data = fixture()
            change(data)
            result = PROBE.probe(M1, 1, reader_for(data, details()))
            self.assertFalse(result['proven'])
            self.assertEqual(result['status'], 'unavailable')

    def test_retained_observations_keep_state_time_and_selected_seat_only(self):
        data = fixture()
        data['machines']['computers'][0]['seats'].append(S2)
        data['canonical']['machines'][0]['uuids'].append(S2)
        data['observations']['rows'][1]['row']['at'] = 4
        running = {'machine': S2, 'pk': A1 + '.' + S2,
                   'row': {'account': A1, 'at': 20, 'cli': 'codex-alab',
                           'state': 'running', 'note': 'PRIVATE-OBSERVATION'}}
        foreign_seat = '3' * 32
        data['observations']['rows'].extend([
            running, copy.deepcopy(running),
            {'machine': foreign_seat, 'pk': A1 + '.' + foreign_seat,
             'row': {'account': A1, 'at': 999, 'cli': 'codex', 'state': 'idle'}}])

        view = details()
        view[M1]['facts'] = [{'uuid': S2, 'chip': 'PRIVATE-HARDWARE'}, {'uuid': S1}]
        result = PROBE.analyze(M1, 2, data, view)
        self.assertTrue(result['proven'])
        self.assertEqual(result['machines'][0]['openai_account_count'], 2)
        accounts = {r['account_key']: r for r in result['machines'][0]['accounts']}
        self.assertEqual(accounts[A1]['observations'], [
            {'seat': S1, 'cli': 'codex', 'state': 'idle', 'observed_at_ms': 10},
            {'seat': S2, 'cli': 'codex-alab', 'state': 'running', 'observed_at_ms': 20}])
        self.assertEqual(accounts[A2]['observations'], [
            {'seat': S1, 'cli': 'codex', 'state': 'signed-out', 'observed_at_ms': 4}])
        self.assertNotIn('PRIVATE', json.dumps(result))
        data['observations']['rows'].reverse()
        self.assertEqual(PROBE.analyze(M1, 2, data, view), result)

    def test_changed_seat_bindings_cannot_join_retained_observations(self):
        for later_seats in [[], [S2], [S1, S2]]:
            data, view = fixture(), details()
            data['preferences']['preferences'] = []
            view[M1].update(facts=[{'uuid': seat} for seat in later_seats], places=[], prefers=[])
            for selector in ['m3', M1]:
                with self.subTest(later_seats=later_seats, selector=selector):
                    result = PROBE.probe(selector, 2, reader_for(data, view))
                    self.assertEqual(result['status'], 'unavailable')
                    self.assertFalse(result['proven'])
                    self.assertEqual(result['machines'], [])
                    self.assertEqual(result['errors'], ['invalid-machine-seats'])

    def test_malformed_seat_bindings_cannot_prove_success(self):
        changes = [
            lambda data, view: data['machines']['computers'][0].update(seats=[S1, S1]),
            lambda data, view: data['machines']['computers'][0].update(seats=[{}]),
            lambda data, view: view[M1].pop('facts'),
            lambda data, view: view[M1].update(facts=None),
            lambda data, view: view[M1].update(facts={}),
            lambda data, view: view[M1].update(facts=[{}]),
            lambda data, view: view[M1].update(facts=[{'uuid': 'not-a-seat'}]),
            lambda data, view: view[M1].update(facts=[{'uuid': {}}]),
            lambda data, view: view[M1].update(facts=[{'uuid': S1}, {'uuid': S1}]),
        ]
        for change in changes:
            data, view = fixture(), details()
            change(data, view)
            with self.subTest(change=change):
                result = PROBE.probe(M1, 1, reader_for(data, view))
                self.assertEqual(result['status'], 'unavailable')
                self.assertFalse(result['proven'])
                self.assertEqual(result['machines'], [])
                self.assertEqual(len(result['errors']), 1)

    def test_machine_without_seats_keeps_both_registry_preferences(self):
        data, view = fixture(), details()
        data['machines']['computers'][0]['seats'] = []
        data['canonical']['machines'][0]['uuids'] = []
        data['preferences']['preferences'].append({'mc': M1, 'ac': A2, 'row': 'live'})
        view[M1].update(facts=[], places=[], prefers=[{'ac': A1}, {'ac': A2}])
        result = PROBE.probe(M1, 2, reader_for(data, view), expected_accounts=[A1, A2])
        self.assertTrue(result['proven'])
        host = result['machines'][0]
        self.assertEqual(host['machine_facts_seat_count'], 0)
        self.assertEqual(host['openai_account_count'], 2)
        self.assertEqual([(row['account_key'], row['sources']) for row in host['accounts']],
                         [(key, ['machine-preference', 'preference']) for key in (A1, A2)])
        self.assertNotIn(S1, json.dumps(host))

        for change in ({'facts': [{'uuid': S1}]}, {'facts': None}):
            with self.subTest(change=change):
                result = PROBE.probe(M1, 2, reader_for(data, {M1: dict(view[M1], **change)}))
                self.assertEqual(result['status'], 'unavailable')
                self.assertEqual(result['machines'], [])

    def test_preference_without_observation_does_not_invent_one(self):
        data = fixture()
        data['observations']['rows'] = []
        result = PROBE.analyze(M1, 1, data, details())
        self.assertTrue(result['proven'])
        self.assertEqual(result['machines'][0]['accounts'][0]['observations'], [])

    def test_suspended_or_cancelled_rows_remain_metadata(self):
        for status in ('suspended', 'cancelled'):
            with self.subTest(status=status):
                data = fixture()
                data['accounts']['rows'][1]['status'] = status
                data['accounts']['rows'][2]['status'] = status
                result = PROBE.probe('m3', 2, reader_for(data, details()))
                self.assertTrue(result['proven'])
                self.assertEqual(result['machines'][0]['accounts'][1]['status'], status)

    def test_requested_accounts_are_independent_of_the_minimum_count(self):
        def check(required, count=2):
            return PROBE.probe('m3', count, self.reader, expected_accounts=required)

        missing = check([A2, A3])
        self.assertEqual(missing['machines'][0]['openai_account_count'], 2)
        self.assertFalse(missing['proven'])
        self.assertEqual(missing['status'], 'missing-accounts')
        self.assertEqual(missing['missing_account_keys'], [A3])
        self.assertEqual(missing['expected_account_keys'], [A2, A3])
        self.assertEqual(check([A2, A1])['status'], 'matched')
        self.assertEqual(check([A2], 3)['status'], 'insufficient-accounts')
        self.assertNotIn('expected_account_keys', check([]))

    def test_exact_keys_need_one_known_machine_before_claiming_absence(self):
        data, view = fixture(), details()
        data['machines']['computers'].append(dict(data['machines']['computers'][0], mc=M2, seats=[]))
        data['canonical']['machines'].append(dict(data['canonical']['machines'][0], mc=M2, uuids=[]))
        view[M2] = dict(view[M1], mc=M2, facts=[], places=[{'ac': A2}], prefers=[])
        for machine, status in [('m3', 'ambiguous-machine'), ('m99', 'machine-not-found'), (M1, 'unavailable')]:
            if machine == M1:
                view[M1]['complete'] = False
            result = PROBE.probe(machine, 1, reader_for(data, view), expected_accounts=[A1, A3])
            self.assertEqual(result['status'], status)
            self.assertFalse(result['proven'])
            self.assertNotIn('missing_account_keys', result)

    def test_expected_login_requires_one_machine_before_reading_candidate_details(self):
        data, view = fixture(), details()
        data['accounts']['rows'][0]['label'] = 'unrelated-private@example.invalid'
        data['machines']['computers'].append(dict(data['machines']['computers'][0], mc=M2, seats=[]))
        data['canonical']['machines'].append(dict(data['canonical']['machines'][0], mc=M2, uuids=[]))
        view[M2] = dict(view[M1], mc=M2, facts=[], places=[{'ac': A2}], prefers=[])
        for machine, status in (('m3', 'ambiguous-machine'), ('m99', 'machine-not-found')):
            with self.subTest(machine=machine):
                result = PROBE.probe(machine, 1, reader_for(data, view), expected_logins=['requested@example.invalid'])
                self.assertEqual(result['status'], status)
                self.assertFalse(result['proven'])
                self.assertEqual(result['machines'], [])
                self.assertNotIn('resolved_login_account_keys', result)
                self.assertNotIn('missing_account_keys', result)
                self.assertNotIn('@', json.dumps(result))
                self.assertEqual([r['command'] for r in result['commands']], list(PROBE.QUERIES.values()))

    def test_exact_keys_recheck_live_rows_after_the_machine_detail(self):
        for change, status, error in [
            ({'row': 'retired'}, 'unavailable', 'account-associations-changed'),
            ({'provider': 'anthropic'}, 'unavailable', 'account-associations-changed'),
            ({'label': 'renamed'}, 'matched', None),
            (None, 'unavailable', 'account-query-failed'),
        ]:
            with self.subTest(change=change):
                commands = []

                def reader(command):
                    later = command == PROBE.QUERIES['accounts'] and command in commands
                    commands.append(command)
                    data, receipt = self.reader(command)
                    if later:
                        if change is None:
                            return None, dict(receipt, error='timeout')
                        data['rows'][0].update(change)
                    return data, receipt

                result = PROBE.probe('m3', 1, reader, expected_accounts=[A1])
                self.assertEqual(result['status'], status)
                self.assertEqual(result['errors'], [error] if error else [])
                self.assertEqual(commands[-2:], [['aterm', 'db', 'alab', 'machines', M1], PROBE.QUERIES['accounts']])
                if error:
                    self.assertFalse(result['proven'])
                    self.assertEqual(result['machines'], [])
                    self.assertNotIn('missing_account_keys', result)
                else:
                    self.assertEqual(result['machines'][0]['accounts'][0]['label'], 'renamed')

    def test_cli_checks_keys_before_reading_and_defaults_to_requested_count(self):
        for keys in [[A1, A1], ['a-label'], [M1]]:
            args = ['--machine', 'm3'] + [word for account in keys for word in ('--expect-account', account)]
            with self.subTest(keys=keys), patch.object(sys, 'stderr', io.StringIO()), \
                    self.assertRaises(SystemExit) as error:
                PROBE.main(args)
            self.assertEqual(error.exception.code, 2)
        with patch.object(PROBE, 'probe', return_value={'proven': True}) as probe, \
                patch.object(sys, 'stdout', io.StringIO()):
            self.assertEqual(PROBE.main(['--machine', 'm3', '--expect-account', A1]), 0)
            probe.assert_called_once_with('m3', 1, expected_accounts=[A1], expected_logins=[], show_logins=False)

    def test_cli_checks_login_inputs_before_any_native_reads(self):
        for selectors in [
            ['--expect-login', 'first@example.invalid', '--expect-login', 'FIRST@example.invalid'],
            ['--expect-login', 'one@example.invalid', '--expect-account', A1],
            ['--expect-login', 'broken'],
            ['--expect-login', 'bad\x7f@example.invalid'],
        ]:
            with self.subTest(selectors=selectors), patch.object(PROBE, 'ThreadPoolExecutor') as executor, \
                    patch.object(sys, 'stderr', io.StringIO()), self.assertRaises(SystemExit):
                PROBE.main(['--machine', 'm3', *selectors])
            executor.assert_not_called()
        with patch.object(PROBE, 'probe', return_value={'proven': True}) as probe, \
                patch.object(sys, 'stdout', io.StringIO()):
            self.assertEqual(PROBE.main(['--machine', 'm3', '--expect-login', 'a@example.invalid']), 0)
            probe.assert_called_once_with('m3', 1, expected_accounts=[], expected_logins=['a@example.invalid'], show_logins=False)

    def test_registry_login_padding_matches_native_trim_ascii(self):
        for pad in ('\t', '\f', '\v'):
            with self.subTest(padding=repr(pad)):
                padded = pad + 'First@Example.INVALID' + pad

                def stored(cmd, view, _):
                    if cmd == ADDRESS_SCAN:
                        view['rows'][0]['addr'] = padded
                    elif cmd[2] == 'emails':
                        view['emails'][0]['addr'] = padded

                report = self.discover(stored, aliases=True)
                if pad == '\v':
                    self.assertEqual(report['status'], 'unavailable')
                    self.assertEqual(report['machines'], [])
                    with self.assertRaises(ValueError):
                        self.discover(expected_logins=[padded])
                else:
                    self.assertTrue(report['proven'], report['errors'])
                    self.assertEqual(report['machines'][0]['accounts'][0]['registered_login'], 'first@example.invalid')
                    self.assertEqual(self.discover(expected_logins=[padded])['resolved_login_account_keys'], [A1])

    def test_registry_tag_base_and_competing_spellings_use_native_padding(self):
        tag, base, other = ('ad-' + c * 16 for c in '123')

        def route(base_spelling, rival):
            addresses = {tag: {'addr': 'list+tag@example.invalid'}, base: {'addr': base_spelling}, other: {'addr': rival}}
            return PROBE.settled_address_route(tag, 'mb-' + 'a' * 16, 'observed', [base], addresses,
                                               lambda _pk, _field: (), {}, self.fail)

        for pad, native_match in (('\t', True), ('\f', True), ('\v', False)):
            with self.subTest(padding=repr(pad)):
                padded = pad + 'List@Example.INVALID' + pad
                self.assertEqual(route(padded, None), native_match)
                self.assertEqual(route('list@example.invalid', {'conflict': [None, padded]}), not native_match)

    def test_count_cannot_use_an_account_that_became_live_between_reads(self):
        def reader(command):
            data, receipt = self.reader(command)
            if command == PROBE.QUERIES['accounts'] and not seen:
                data['rows'][0]['row'] = 'retired'
                seen.append(command)
            return data, receipt

        for required in ([], [A1]):
            seen = []
            result = PROBE.probe('m3', 2, reader, expected_accounts=required)
            self.assertEqual(result['status'], 'unavailable')
            self.assertFalse(result['proven'])
            self.assertEqual(result['errors'], ['account-associations-changed'])
            self.assertNotIn('missing_account_keys', result)

    def test_replaced_preference_cannot_establish_two_current_associations(self):
        data, view = fixture(), details()
        data['observations']['rows'] = []
        view[M1].update(places=[], prefers=[{'ac': A2}])
        for selector in ('m3', M1):
            with self.subTest(selector=selector):
                result = PROBE.probe(selector, 2, reader_for(data, view))
                self.assertEqual(result['status'], 'insufficient-accounts')
                self.assertFalse(result['proven'])
                self.assertEqual([(a['account_key'], a['sources']) for a in result['machines'][0]['accounts']],
                                 [(A2, ['machine-preference'])])

        view[M1]['places'] = [{'ac': A1}]
        result = PROBE.probe(M1, 2, reader_for(data, view))
        self.assertTrue(result['proven'])
        self.assertEqual([(a['account_key'], a['sources']) for a in result['machines'][0]['accounts']],
                         [(A1, ['placement']), (A2, ['machine-preference'])])

    def test_other_provider_retired_links_and_other_machine_not_counted(self):
        data, view = fixture(), details()
        data['accounts']['rows'][1]['row'] = 'retired'
        data['preferences']['preferences'].extend([
            {'mc': M1, 'ac': A3, 'row': 'live'},
            {'mc': M1, 'ac': A2, 'row': 'ended'},
            {'mc': M2, 'ac': A2, 'row': 'live'}])
        view[M1]['places'].append({'ac': A3})
        result = PROBE.analyze(M1, 2, data, view)
        self.assertEqual(result['status'], 'insufficient-accounts')
        self.assertEqual(result['machines'][0]['openai_account_count'], 1)

    def test_only_native_unbound_seat_shape_is_skipped(self):
        data = fixture()
        data['machines']['computers'].append({'mc': None, 'seats': [S2]})
        result = PROBE.analyze('m3', 2, data, details())
        self.assertTrue(result['proven'])
        self.assertEqual(result['unbound_seat_count'], 1)
        self.assertEqual(len(result['machines']), 1)
        for unknown in [
            {'mc': None, 'label': 'm3', 'seats': [S2]},
            {'mc': None, 'seats': ['not-a-seat']},
            {'mc': None, 'seats': [S2], 'row': 'live'}]:
            data['machines']['computers'][-1] = unknown
            with self.subTest(unknown=unknown), self.assertRaises(ValueError):
                PROBE.analyze('m3', 1, data, details())

    def test_unknown_or_retired_machine_is_not_a_match(self):
        data = fixture()
        data['machines']['computers'][0]['row'] = 'retired'
        data['canonical']['machines'][0]['row'] = 'retired'
        self.assertEqual(PROBE.analyze('m3', 1, data, {})['status'], 'machine-not-found')
        self.assertEqual(PROBE.analyze('missing', 1, fixture(), {})['status'],
                         'machine-not-found')

    def test_incomplete_and_conflicted_views_cannot_prove_success(self):
        cases = []
        for name in ['machines', 'preferences']:
            data = fixture()
            data[name]['complete'] = False
            cases.append(data)
        for name, field, column in [('machines', 'computers', 'label'),
                                    ('accounts', 'rows', 'provider'),
                                    ('preferences', 'preferences', 'ac')]:
            data = fixture()
            data[name][field][0][column] = {'conflict': ['first', 'second']}
            cases.append(data)
        data = fixture()
        data['accounts']['malformed'] = 1
        cases.append(data)
        data = fixture()
        data['accounts']['malformed'] = False
        cases.append(data)
        data = fixture()
        data['accounts']['rows'][0]['status'] = 'signed in · on m3'
        cases.append(data)
        for data in cases:
            with self.subTest(data=data), self.assertRaises((ValueError, TypeError)):
                PROBE.analyze('m3', 1, data, details())

    def test_dangling_reference_and_bad_observation_cannot_prove_success(self):
        for mutate in [
            lambda d: d['accounts']['rows'].pop(1),
            lambda d: d['observations']['rows'][0]['row'].update(at=True),
            lambda d: d['observations'].update(malformed=1),
            lambda d: d['observations']['rows'][0].update(pk='mismatch')]:
            data = fixture()
            mutate(data)
            with self.subTest(mutate=mutate), self.assertRaises(ValueError):
                PROBE.analyze('m3', 1, data, details())
        view = details()
        view[M1]['complete'] = False
        with self.assertRaises(ValueError):
            PROBE.analyze('m3', 1, fixture(), view)

    def reader(self, command):
        return reader_for(fixture(), details())(command)

    def test_probe_uses_only_known_read_commands_and_keeps_receipts(self):
        def reader(command):
            # The default computers view on aterm 0.206 flattens the seats
            # field to a display string, even when it has a single UUID.
            if command == ['aterm', 'db', 'computers']:
                view = fixture()['machines']
                view['computers'][0]['seats'] = S1
                return view, {'command': command, 'exit_code': 0}
            return self.reader(command)

        result = PROBE.probe('m3', 2, reader)
        self.assertTrue(result['proven'])
        self.assertEqual(result['commands'][0]['command'], ['aterm', 'db', 'computers', '--registry'])
        self.assertEqual(len(result['commands']), 7)
        self.assertEqual(result['commands'][-1]['command'], PROBE.QUERIES['accounts'])
        self.assertNotIn('PRIVATE', json.dumps(result))

    def test_changed_machine_details_prevent_success_for_label_or_key(self):
        for change in [
            {'row': 'retired'}, {'row': 'merged'}, {'row': None},
            {'label': 'm99'}, {'label': None}, {'mc': M2},
        ]:
            view = details()
            view[M1].update(change)

            def changed_reader(command):
                if command == ['aterm', 'db', 'alab', 'machines', M1]:
                    return copy.deepcopy(view[M1]), {'command': command, 'exit_code': 0}
                return self.reader(command)

            for selector in ['m3', M1]:
                with self.subTest(change=change, selector=selector):
                    with self.assertRaisesRegex(ValueError, '^invalid-machine-details$'):
                        PROBE.analyze(selector, 2, fixture(), view)
                    result = PROBE.probe(selector, 2, changed_reader)
                    self.assertEqual(result['status'], 'unavailable')
                    self.assertFalse(result['proven'])
                    self.assertEqual(result['machines'], [])
                    self.assertEqual(result['errors'], ['invalid-machine-details'])
                    self.assertEqual(len(result['commands']), 6)

    def test_query_failure_is_redacted_and_fail_closed(self):
        with patch.object(PROBE.subprocess, 'run', return_value=subprocess.CompletedProcess(
                ['aterm'], 1, 'PRIVATE-STDOUT', 'PRIVATE-STDERR')):
            data, receipt = PROBE.query(['aterm', 'db', 'computers'])
        self.assertIsNone(data)
        self.assertEqual(receipt['error'], 'command-failed')
        self.assertNotIn('PRIVATE', json.dumps(receipt))
        result = PROBE.probe('m3', 2, lambda cmd: (None, receipt))
        self.assertEqual(result['status'], 'unavailable')
        self.assertFalse(result['proven'])
        with patch.object(PROBE.subprocess, 'run', side_effect=subprocess.TimeoutExpired('aterm', 20)):
            self.assertEqual(PROBE.query(['aterm'])[1]['error'], 'timeout')
        with patch.object(PROBE.subprocess, 'run', return_value=subprocess.CompletedProcess(
                ['aterm'], 0, '[]', '')):
            self.assertEqual(PROBE.query(['aterm'])[1]['error'], 'invalid-json')

    def test_ambiguous_native_json_cannot_confirm_a_machine_association(self):
        canonical = fixture()['canonical']
        canonical['machines'].append(dict(canonical['machines'][0], mc=M2, label='other', uuids=[]))
        canonical['note'] = 'PRIVATE-NOTE'
        valid = json.dumps(canonical)
        command = PROBE.QUERIES['canonical']

        def probe(raw):
            def reader(cmd):
                if cmd != command:
                    return self.reader(cmd)
                completed = subprocess.CompletedProcess(cmd, 0, raw, '')
                with patch.object(PROBE.subprocess, 'run', return_value=completed):
                    return PROBE.query(cmd)
            return PROBE.probe('m3', 2, reader, expected_accounts=[A1, A2])

        # The same field names in two distinct machine objects are ordinary JSON.
        control = probe(valid)
        self.assertEqual(control['status'], 'matched')
        self.assertTrue(control['proven'])
        for original, duplicate in [('"complete": true', '"complete": false'),
                                    ('"label": "m3"', '"label": "PRIVATE-MACHINE"')]:
            for order in ((original, duplicate), (duplicate, original)):
                with self.subTest(order=order):
                    report = probe(valid.replace(original, ', '.join(order), 1))
                    self.assertEqual(report['status'], 'unavailable')
                    self.assertFalse(report['proven'])
                    self.assertEqual(report['machines'], [])
                    receipt, = [r for r in report['commands'] if r['command'] == command]
                    self.assertEqual(receipt, {'command': command, 'exit_code': 0, 'error': 'invalid-json'})
                    self.assertNotIn('PRIVATE', json.dumps(report))

    def test_native_table_readiness_is_recognized_without_echoing_arbitrary_errors(self):
        for source in ('canonical', 'machines', 'accounts'):
            command = PROBE.QUERIES[source]
            standard = ' '.join(command[:3]) + ': Commons membership is not ready for table access'
            for stderr, recognized in [(standard + '\n', True), ('PRIVATE ' + standard, False),
                                       (standard + '\nPRIVATE', False), ('PRIVATE', False)]:
                with self.subTest(source=source, recognized=recognized), patch.object(
                        PROBE.subprocess, 'run', return_value=subprocess.CompletedProcess(command, 1, 'PRIVATE', stderr)):
                    value, receipt = PROBE.query(command)
                    self.assertIsNone(value)
                    self.assertEqual(receipt.get('reason'), 'commons-membership-not-ready' if recognized else None)
                    self.assertEqual(receipt['error'], 'command-failed')
                    self.assertNotIn('PRIVATE', json.dumps(receipt))

    def test_unavailable_account_collection_reports_the_local_file_state(self):
        status_command = ['aterm', 'file', 'status', 'ai/accounts', '--json']

        def probe_with(status):
            def reader(command):
                if command == PROBE.QUERIES['accounts']:
                    return None, {'command': command, 'exit_code': 1, 'error': 'command-failed'}
                if command == status_command:
                    return status, {'command': command, 'exit_code': 0}
                return self.reader(command)
            return PROBE.probe('m3', 2, reader)

        for state, why, complete in [('never', 'custody', False), ('held', 'succession', True),
                                     ('live', None, True)]:
            with self.subTest(state=state):
                status = {'file': 'ai/accounts', 'state': state, 'why': why, 'complete': complete,
                          'rows': 0, 'problem': 'PRIVATE-DIAGNOSTIC'}
                result = probe_with(status)
                self.assertEqual(result['status'], 'unavailable')
                self.assertFalse(result['proven'])
                self.assertEqual(result['machines'], [])
                self.assertEqual(result['local_account_file'],
                                 {'state': state, 'why': why, 'complete': complete})
                self.assertEqual(result['commands'][-1]['command'], status_command)
                self.assertNotIn('PRIVATE', json.dumps(result))

        valid = {'file': 'ai/accounts', 'state': 'never', 'why': 'custody'}
        self.assertEqual(probe_with(valid)['local_account_file'], {'state': 'never', 'why': 'custody'})
        for change in [{'file': 'ai/other'}, {'why': 'PRIVATE'}, {'state': ['PRIVATE']}]:
            with self.subTest(change=change):
                result = probe_with(dict(valid, **change))
                self.assertNotIn('local_account_file', result)
                self.assertNotIn('PRIVATE', json.dumps(result))
                self.assertEqual(result['errors'], ['metadata-query-failed'])

    def test_private_output_and_no_overwrite(self):
        with tempfile.TemporaryDirectory() as temp:
            target = Path(temp) / 'proof.json'
            with patch.object(PROBE, 'probe', return_value={'proven': True}), \
                    patch.object(sys, 'stdout', io.StringIO()) as stdout:
                self.assertEqual(PROBE.main(['--machine', 'm3', '--output', str(target)]), 0)
            self.assertEqual(stat.S_IMODE(target.stat().st_mode), 0o600)
            saved = target.read_bytes()
            self.assertEqual(saved.decode('utf-8'), stdout.getvalue())
            with patch.object(PROBE, 'probe', return_value={'proven': False}), \
                    patch.object(sys, 'stderr', io.StringIO()):
                self.assertEqual(PROBE.main(['--machine', 'm3', '--output', str(target)]), 2)
            self.assertEqual(target.read_bytes(), saved)

    def test_failed_staged_write_leaves_the_name_available_for_retry(self):
        with tempfile.TemporaryDirectory() as temp:
            target = Path(temp) / 'proof.json'
            args = ['--machine', 'm3', '--output', str(target)]

            def fail_sync(fd):
                self.assertGreater(PROBE.os.fstat(fd).st_size, 0)
                self.assertFalse(target.exists())
                raise OSError('PRIVATE')

            with patch.object(PROBE, 'probe', return_value={'proven': True}), \
                    patch.object(sys, 'stdout', io.StringIO()) as stdout, \
                    patch.object(sys, 'stderr', io.StringIO()) as stderr:
                with patch.object(PROBE.os, 'fsync', side_effect=fail_sync):
                    self.assertEqual(PROBE.main(args), 2)
                self.assertEqual(stdout.getvalue(), '')
                self.assertNotIn('PRIVATE', stderr.getvalue())
                self.assertEqual(list(Path(temp).iterdir()), [])
                self.assertEqual(PROBE.main(args), 0)
                self.assertEqual(json.loads(target.read_text()), {'proven': True})

    def test_concurrent_destination_is_preserved(self):
        with tempfile.TemporaryDirectory() as temp:
            target = Path(temp) / 'proof.json'
            link = PROBE.os.link

            def link_after_other_writer(source, destination):
                self.assertEqual(json.loads(Path(source).read_text()), {'proven': True})
                target.symlink_to('missing')
                link(source, destination)

            with patch.object(PROBE, 'probe', return_value={'proven': True}), \
                    patch.object(PROBE.os, 'link', side_effect=link_after_other_writer), \
                    patch.object(sys, 'stdout', io.StringIO()) as stdout, \
                    patch.object(sys, 'stderr', io.StringIO()):
                self.assertEqual(PROBE.main(['--machine', 'm3', '--output', str(target)]), 2)
            self.assertEqual(stdout.getvalue(), '')
            self.assertTrue(target.is_symlink())
            self.assertEqual(list(Path(temp).iterdir()), [target])

    def test_cleanup_failure_preserves_the_published_result(self):
        report = {'proven': False, 'status': 'insufficient'}
        with tempfile.TemporaryDirectory() as temp:
            target = Path(temp) / 'proof.json'
            with patch.object(PROBE, 'probe', return_value=report), \
                    patch.object(PROBE.os, 'unlink', side_effect=OSError('PRIVATE')), \
                    patch.object(sys, 'stdout', io.StringIO()) as stdout, \
                    patch.object(sys, 'stderr', io.StringIO()) as stderr:
                self.assertEqual(PROBE.main(['--machine', 'm3', '--output', str(target)]), 1)
            self.assertEqual(target.read_text(), stdout.getvalue())
            self.assertEqual(json.loads(stdout.getvalue()), report)
            self.assertEqual(stderr.getvalue(), 'Cannot remove the private temporary evidence file.\n')

    def test_unencodable_report_returns_a_value_free_error(self):
        with tempfile.TemporaryDirectory() as temp, \
                patch.object(PROBE, 'probe', return_value={'PRIVATE': object()}), \
                patch.object(sys, 'stdout', io.StringIO()) as stdout, \
                patch.object(sys, 'stderr', io.StringIO()) as stderr:
            target = Path(temp) / 'proof.json'
            self.assertEqual(PROBE.main(['--machine', 'm3', '--output', str(target)]), 2)
            self.assertEqual(stdout.getvalue(), '')
            self.assertEqual(stderr.getvalue(), 'Cannot encode the evidence report as JSON.\n')
            self.assertEqual(list(Path(temp).iterdir()), [])


class RegistryReportOutputTests(unittest.TestCase):
    report = {'proven': True, 'registered_login': 'private-login@example.invalid'}

    def run_probe(self, stdout, *, target=None, unbuffered=False, configured_stdout=False):
        # Only the Registry lookup is synthetic. Keep the CLI, file, standard streams and
        # interpreter shutdown real, including the flush of a small buffered report.
        child = r"""
import json, sys
sys.path.insert(0, sys.argv.pop(1))
import account_registry_probe as probe
report = json.loads(sys.argv.pop(1))
probe.probe = lambda *args, **kwargs: report
configured = sys.argv.pop(1) == 'configured'
if configured:
    sys.stdout.reconfigure(encoding='utf-8-sig', newline='\r\n')
    sys.stdout.write('before\n')
status = probe.main()
if configured:
    sys.stdout.write('after\n')
sys.exit(status)
"""
        command = [sys.executable, '-I', '-B']
        if unbuffered:
            command.append('-u')
        command.extend(['-c', child, str(Path(PROBE.__file__).parent), json.dumps(self.report),
                        'configured' if configured_stdout else 'default',
                        '--machine', 'synthetic', '--show-logins'])
        if target is not None:
            command.extend(['--output', str(target)])
        return subprocess.run(command, stdout=stdout, stderr=subprocess.PIPE, timeout=10)

    def test_closed_pipe_reports_if_private_evidence_was_saved(self):
        for unbuffered in (False, True):
            for save in (False, True):
                with self.subTest(unbuffered=unbuffered, save=save), tempfile.TemporaryDirectory() as temp:
                    target = Path(temp) / 'private-filename.json'
                    reader, writer = os.pipe()
                    os.close(reader)
                    with os.fdopen(writer, 'wb') as pipe:
                        result = self.run_probe(pipe, target=target if save else None, unbuffered=unbuffered)
                    self.assertEqual(result.returncode, 2, result.stderr)
                    error = result.stderr.decode().lower()
                    self.assertRegex(error, r'stdout|standard output')
                    for private in (self.report['registered_login'], str(target).lower(), target.name):
                        self.assertNotIn(private, error)
                    for diagnostic in ('traceback', 'exception ignored', 'brokenpipeerror', 'unraisable'):
                        self.assertNotIn(diagnostic, error)
                    if save:
                        self.assertIn('file', error)
                        self.assertIn('saved', error)
                        self.assertEqual(json.loads(target.read_bytes()), self.report)
                        self.assertEqual(stat.S_IMODE(target.stat().st_mode), 0o600)
                    else:
                        self.assertNotIn('saved', error)
                    self.assertEqual(list(Path(temp).iterdir()), [target] if save else [])

    def test_working_stdout_matches_the_private_file_after_process_exit(self):
        with tempfile.TemporaryDirectory() as temp:
            target = Path(temp) / 'private-filename.json'
            result = self.run_probe(subprocess.PIPE, target=target)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stderr, b'')
            self.assertEqual(json.loads(result.stdout), self.report)
            self.assertEqual(target.read_bytes(), result.stdout)
            self.assertEqual(stat.S_IMODE(target.stat().st_mode), 0o600)

    def test_healthy_stdout_retains_its_encoder_and_configured_line_endings(self):
        result = self.run_probe(subprocess.PIPE, configured_stdout=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, b'')
        expected = 'before\n' + json.dumps(self.report, indent=2) + '\nafter\n'
        self.assertEqual(result.stdout, expected.replace('\n', '\r\n').encode('utf-8-sig'))


if __name__ == '__main__':
    unittest.main()

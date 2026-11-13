# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import copy
import io
import json
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


def fixture():
    return {
        'machines': {'complete': True, 'computers': [
            {'mc': M1, 'label': 'm3', 'row': 'live', 'seats': [S1]}]},
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


class RegistryProbeTests(unittest.TestCase):
    def test_distinct_accounts_deduplicated_across_all_sources(self):
        result = PROBE.analyze('m3', 2, fixture(), details())
        self.assertTrue(result['proven'])
        host = result['machines'][0]
        self.assertEqual(host['openai_account_count'], 2)
        self.assertEqual(len(host['accounts'][0]['sources']), 4)
        self.assertEqual(host['accounts'][1]['status'], 'pending')
        self.assertNotIn('PRIVATE', json.dumps(result))
        self.assertIn('not fresh provider authentication', ' '.join(PROBE.LIMITS))

    def test_duplicate_labels_never_combined_to_meet_expected_count(self):
        data, view = fixture(), details()
        data['machines']['computers'].append(
            {'mc': M2, 'label': 'm3', 'row': 'live', 'seats': [S2]})
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

    def test_retained_observations_keep_state_time_and_selected_seat_only(self):
        data = fixture()
        data['machines']['computers'][0]['seats'].append(S2)
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

    def test_machine_without_seats_keeps_direct_account_associations(self):
        data, view = fixture(), details()
        data['machines']['computers'][0]['seats'] = []
        data['observations']['rows'] = []
        view[M1]['facts'] = []
        result = PROBE.probe(M1, 1, reader_for(data, view))
        self.assertTrue(result['proven'])
        self.assertEqual(result['machines'][0]['accounts'][0]['account_key'], A1)

    def test_preference_without_observation_does_not_invent_one(self):
        data = fixture()
        data['observations']['rows'] = []
        result = PROBE.analyze(M1, 1, data, details())
        self.assertTrue(result['proven'])
        self.assertEqual(result['machines'][0]['accounts'][0]['observations'], [])

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
        result = PROBE.probe('m3', 2, self.reader)
        self.assertTrue(result['proven'])
        self.assertEqual(len(result['commands']), 5)
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
                    self.assertEqual(len(result['commands']), 5)

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

    def test_private_output_and_no_overwrite(self):
        with tempfile.TemporaryDirectory() as temp:
            target = Path(temp) / 'proof.json'
            with patch.object(PROBE, 'probe', return_value={'proven': True}), \
                    patch.object(sys, 'stdout', io.StringIO()):
                self.assertEqual(PROBE.main(['--machine', 'm3', '--output', str(target)]), 0)
            self.assertEqual(stat.S_IMODE(target.stat().st_mode), 0o600)
            saved = target.read_bytes()
            with patch.object(PROBE, 'probe', return_value={'proven': False}), \
                    patch.object(sys, 'stderr', io.StringIO()):
                self.assertEqual(PROBE.main(['--machine', 'm3', '--output', str(target)]), 2)
            self.assertEqual(target.read_bytes(), saved)


if __name__ == '__main__':
    unittest.main()

# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Real SQLite fixtures for the bounded metadata-only request timeline."""
from contextlib import closing, redirect_stderr, redirect_stdout
import io
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


ALAB = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ALAB))
import request_timeline as timeline

MIGRATIONS = ALAB.parent / 'codex-rs/state/logs_migrations'
PROCESS = 'pid:42:11111111-1111-4111-8111-111111111111'
REUSED = 'pid:42:22222222-2222-4222-8222-222222222222'
OTHER = 'pid:142:33333333-3333-4333-8333-333333333333'
THREAD = '44444444-4444-7444-8444-abcdefabcdef'
OTHER_THREAD = '55555555-5555-7555-8555-555555555555'
PRIVATE = 'never-render-private-body-or-span'


class FaultOnClose:
    def __init__(self, connection, fault):
        self.connection, self.fault = connection, fault

    def __getattr__(self, name):
        return getattr(self.connection, name)

    def close(self):
        self.connection.close()
        raise self.fault


class TimelineTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.db = Path(self.directory.name) / 'logs_2.sqlite'
        self.writer = sqlite3.connect(self.db)
        self.addCleanup(self.writer.close)
        for migration in sorted(MIGRATIONS.glob('*.sql')):
            self.writer.executescript(migration.read_text())
        self.writer.commit()

    def insert(self, *, ts=100, nanos=0, target=timeline.TARGETS[0], level='INFO',
               process=PROCESS, thread=THREAD, body=PRIVATE):
        self.writer.execute(
            'INSERT INTO logs (ts,ts_nanos,level,target,process_uuid,thread_id,'
            'feedback_log_body,module_path,file) VALUES (?,?,?,?,?,?,?,?,?)',
            (ts, nanos, level, target, process, thread, body, PRIVATE, PRIVATE))
        self.writer.commit()

    def read(self, **kwargs):
        return timeline.read_timeline(self.db, pid=42, **kwargs)

    def error(self, category, operation):
        with self.assertRaises(timeline.TimelineError) as caught:
            operation()
        self.assertEqual(caught.exception.category, category)
        self.assertNotIn(PRIVATE, str(caught.exception))
        return caught.exception

    def cli(self, arguments):
        return subprocess.run(
            [sys.executable, '-I', '-B', str(ALAB / 'request_timeline.py'),
             '--db', str(self.db), *arguments], capture_output=True, text=True,
            timeout=5, env={**os.environ, 'PYTHONDONTWRITEBYTECODE': '1'})

    def _encoded_fixture(self, encoding):
        path = self.db.with_name('encoding-' + encoding + '.sqlite')
        with closing(sqlite3.connect(path)) as writer, writer:
            writer.execute("PRAGMA encoding='" + encoding + "'")
            for migration in sorted(MIGRATIONS.glob('*.sql')):
                writer.executescript(migration.read_text())
            writer.execute(
                'INSERT INTO logs(ts,ts_nanos,level,target,process_uuid,thread_id,'
                'feedback_log_body) VALUES(100,0,?,?,?,?,?)',
                ('INFO', timeline.TARGETS[0], PROCESS, THREAD, PRIVATE * 4096))
            self.assertEqual(writer.execute('PRAGMA encoding').fetchone()[0], encoding)
            self.assertEqual(writer.execute(
                'SELECT id,ts,ts_nanos,level,target,process_uuid,thread_id FROM logs'
            ).fetchall(), [(1, 100, 0, 'INFO', timeline.TARGETS[0], PROCESS, THREAD)])
        path.chmod(0o444)
        self.addCleanup(path.chmod, 0o600)
        return path

    def test_database_encoding_refuses_non_utf8_before_metadata_selection(self):
        original = sqlite3.connect
        scopes = (
            ('pid', {'pid': 42}), ('process', {'process_uuid': PROCESS}),
            ('thread', {'thread_id': THREAD}),
            ('intersection', {'pid': 42, 'process_uuid': PROCESS, 'thread_id': THREAD}),
            ('scope-miss', {'pid': 42, 'process_uuid': OTHER, 'thread_id': THREAD}),
        )
        for encoding in ('UTF-8', 'UTF-16le', 'UTF-16be'):
            path = self._encoded_fixture(encoding)
            before = path.read_bytes()
            for name, scope in scopes:
                with self.subTest(encoding=encoding, scope=name):
                    queries = []

                    def observed(*arguments, **keywords):
                        connection = original(*arguments, **keywords)
                        connection.set_trace_callback(queries.append)
                        return connection

                    result, failure = None, None
                    with patch.object(timeline.sqlite3, 'connect', side_effect=observed):
                        try:
                            result = timeline.read_timeline(path, **scope)
                        except timeline.TimelineError as error:
                            failure = error
                    self.assertEqual(path.read_bytes(), before)
                    self.assertNotIn(PRIVATE, json.dumps(result))
                    self.assertTrue(queries, 'reader used the real fixture connection')
                    if encoding == 'UTF-8':
                        self.assertIsNone(failure)
                        expected = [] if name == 'scope-miss' else [{
                            'id': 1, 'ts': 100, 'ts_nanos': 0, 'level': 'INFO',
                            'target': timeline.TARGETS[0], 'process_uuid': PROCESS,
                            'thread_id': THREAD, 'selected_row_gap_ns': None,
                            'row_gap_status': 'first_selected_row',
                        }]
                        self.assertEqual(result['rows'], expected)
                        self.assertEqual(result['omitted_invalid_scope_rows'], 0)
                    else:
                        self.assertIsInstance(failure, timeline.TimelineError,
                                              'unsupported encoding must not report success')
                        self.assertEqual(failure.category, 'unsupported_database_encoding')
                        self.assertIsNone(result)
                        self.assertFalse(any(query.lstrip().upper().startswith('SELECT')
                                             for query in queries),
                                         'encoding is refused before metadata selection')

    def test_cli_reports_unsupported_database_encoding_without_success_json(self):
        for encoding in ('UTF-16le', 'UTF-16be'):
            with self.subTest(encoding=encoding):
                path = self._encoded_fixture(encoding)
                before = path.read_bytes()
                result = subprocess.run(
                    [sys.executable, '-I', '-B', '-W', 'error::ResourceWarning',
                     str(ALAB / 'request_timeline.py'), '--db', str(path), '--pid', '42'],
                    capture_output=True, text=True, timeout=5,
                    env={**os.environ, 'PYTHONDONTWRITEBYTECODE': '1'})
                self.assertEqual(path.read_bytes(), before)
                self.assertNotIn(PRIVATE, result.stdout + result.stderr)
                self.assertEqual(result.returncode, 1)
                self.assertEqual(result.stdout, '')
                self.assertEqual(result.stderr,
                                 'request timeline: unsupported_database_encoding\n')

    def test_utf8_embedded_nul_identities_remain_unavailable_without_pairing(self):
        self.assertEqual(self.writer.execute('PRAGMA encoding').fetchone()[0], 'UTF-8')
        bad_process, bad_thread = PROCESS + '\x00' + PRIVATE, THREAD + '\x00' + PRIVATE
        self.insert(ts=10)
        self.insert(ts=20, process=bad_process)
        self.insert(ts=30, process=bad_process)
        self.insert(ts=40, thread=bad_thread)
        self.insert(ts=50, thread=bad_thread)
        by_thread = timeline.read_timeline(self.db, thread_id=THREAD)
        self.assertEqual([row['process_uuid'] for row in by_thread['rows']],
                         [PROCESS, None, None])
        self.assertEqual([row['thread_id'] for row in by_thread['rows']], [THREAD] * 3)
        by_process = timeline.read_timeline(self.db, process_uuid=PROCESS)
        self.assertEqual([row['process_uuid'] for row in by_process['rows']], [PROCESS] * 3)
        self.assertEqual([row['thread_id'] for row in by_process['rows']],
                         [THREAD, None, None])
        for result in (by_thread, by_process):
            self.assertEqual(result['omitted_invalid_scope_rows'], 0)
            self.assertEqual([row['row_gap_status'] for row in result['rows']],
                             ['first_selected_row', 'identity_unavailable',
                              'identity_unavailable'])
            self.assertTrue(all(row['selected_row_gap_ns'] is None for row in result['rows']))
            self.assertNotIn(PRIVATE, json.dumps(result))

    def test_fixed_projection_never_reads_body_or_spans(self):
        self.insert(body=PRIVATE * 200_000)
        original = sqlite3.connect
        queries = []

        def observed(*args, **kwargs):
            connection = original(*args, **kwargs)
            connection.set_trace_callback(queries.append)
            return connection

        with patch.object(timeline.sqlite3, 'connect', side_effect=observed):
            result = self.read()
        self.assertEqual(len(result['rows']), 1)
        self.assertNotIn(PRIVATE, json.dumps(result))
        selects = [query for query in queries if query.startswith('SELECT')]
        self.assertEqual(len(selects), 1)
        for forbidden in ('feedback_log_body', 'module_path', 'file', 'message'):
            self.assertNotIn(forbidden, selects[0])
        for column in ('feedback_log_body', 'module_path', 'file', 'line'):
            self.assertEqual(timeline._authorize(sqlite3.SQLITE_READ, 'logs',
                                                column, 'main', None), sqlite3.SQLITE_DENY)

    def test_fixed_targets_and_standard_levels_only(self):
        for target in timeline.TARGETS:
            self.insert(target=target)
        self.insert(target=PRIVATE)
        self.insert(level=PRIVATE)
        self.assertEqual([row['target'] for row in self.read()['rows']], list(timeline.TARGETS))

    def test_ordinary_retry_target_keeps_exact_scope_gap_and_window_bounds(self):
        target = 'codex_core::responses_retry'
        for seconds, label, level, process, thread in (
            (10, 'codex_api::endpoint::alab_request_diagnostics', 'INFO', PROCESS, THREAD),
            (20, target, 'WARN', PROCESS, THREAD),
            (25, 'codex_core::responses_retry::alab_capacity_retry', 'WARN', PROCESS, THREAD),
            (30, target, 'INFO', PROCESS, THREAD),
            (40, target, 'WARN', REUSED, THREAD),
            (50, target, 'WARN', PROCESS, OTHER_THREAD),
            (60, target, 'WARN', OTHER, THREAD),
            (70, target, 'WARN', PROCESS, None),
            (80, target, 'WARN', PROCESS, THREAD),
            (90, target + '::not_admitted', 'WARN', PROCESS, THREAD),
            (100, target + '_not_admitted', 'WARN', PROCESS, THREAD),
            (110, target, 'warn', PROCESS, THREAD),
        ):
            self.insert(ts=seconds, target=label, level=level, process=process, thread=thread)
        before = self.db.read_bytes()
        original = sqlite3.connect
        queries = []

        def observed(*args, **kwargs):
            connection = original(*args, **kwargs)
            connection.set_trace_callback(queries.append)
            return connection

        with patch.object(timeline.sqlite3, 'connect', side_effect=observed):
            exact = self.read(process_uuid=PROCESS, thread_id=THREAD)
            by_pid = self.read()
            by_process = timeline.read_timeline(self.db, process_uuid=PROCESS)
            by_thread = timeline.read_timeline(self.db, thread_id=THREAD)
            limited = self.read(process_uuid=PROCESS, thread_id=THREAD, limit=2)
        results = (exact, by_pid, by_process, by_thread, limited)
        self.assertTrue(self.db.read_bytes() == before, 'reader changed controlled database')
        self.assertNotIn(PRIVATE, json.dumps(results))
        selects = [query for query in queries if query.startswith('SELECT')]
        self.assertEqual(len(selects), 5)
        for query in selects:
            for forbidden in ('feedback_log_body', 'module_path', 'file', 'message'):
                self.assertNotIn(forbidden, query)
        self.assertEqual([row['id'] for row in exact['rows']], [1, 2, 3, 4, 9])
        self.assertEqual([row['selected_row_gap_ns'] for row in exact['rows']],
                         [None, None, None, None, 60_000_000_000])
        self.assertEqual([row['id'] for row in by_pid['rows']], [1, 2, 3, 4, 5, 6, 8, 9])
        self.assertEqual([row['row_gap_status'] for row in by_pid['rows'][4:7]],
                         ['first_selected_row', 'first_selected_row', 'identity_unavailable'])
        self.assertEqual([row['id'] for row in by_process['rows']], [1, 2, 3, 4, 6, 8, 9])
        self.assertEqual([row['id'] for row in by_thread['rows']], [1, 2, 3, 4, 5, 7, 9])
        self.assertEqual([row['id'] for row in limited['rows']], [4, 9])
        self.assertTrue(all(row['selected_row_gap_ns'] is None for row in limited['rows']))
        self.assertTrue(limited['more_rows_available'])
        self.assertTrue(all(result['omitted_invalid_scope_rows'] == 0 for result in results))

    def test_cli_includes_literal_ordinary_retry_warning_metadata(self):
        target = 'codex_core::responses_retry'
        for seconds, label, process, thread in (
            (10, 'codex_api::endpoint::alab_request_diagnostics', PROCESS, THREAD),
            (20, target, PROCESS, THREAD),
            (30, 'codex_core::responses_retry::alab_capacity_retry', PROCESS, THREAD),
            (40, target, PROCESS, THREAD),
            (50, target + '::not_admitted', PROCESS, THREAD),
            (60, target + '_not_admitted', PROCESS, THREAD),
            (70, target, REUSED, THREAD),
            (80, target, PROCESS, OTHER_THREAD),
        ):
            self.insert(ts=seconds, target=label, level='WARN', process=process, thread=thread)
        before = self.db.read_bytes()
        result = self.cli(['--pid', '42', '--process-uuid', PROCESS,
                           '--thread-id', THREAD, '--limit', '2'])
        self.assertTrue(self.db.read_bytes() == before, 'CLI changed controlled database')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, '')
        self.assertEqual(len(result.stdout.splitlines()), 1)
        self.assertNotIn(PRIVATE, result.stdout)
        output = json.loads(result.stdout)
        self.assertEqual([row['id'] for row in output['rows']], [3, 4])
        self.assertEqual([row['target'] for row in output['rows']],
                         ['codex_core::responses_retry::alab_capacity_retry', target])
        self.assertEqual([row['level'] for row in output['rows']], ['WARN', 'WARN'])
        self.assertTrue(all(row['process_uuid'] == PROCESS and row['thread_id'] == THREAD
                            and row['selected_row_gap_ns'] is None for row in output['rows']))
        self.assertTrue(output['more_rows_available'])
        self.assertEqual(output['omitted_invalid_scope_rows'], 0)

    def test_http_operation_target_keeps_exact_scope_and_body_free_projection(self):
        target = 'codex_api::endpoint::alab_request_diagnostics::http_operations'
        for seconds, label, level, process, thread in (
            (10, 'codex_api::endpoint::alab_request_diagnostics', 'INFO', PROCESS, THREAD),
            (20, target, 'INFO', PROCESS, THREAD),
            (25, 'codex_core::responses_retry::alab_capacity_retry', 'WARN', PROCESS, THREAD),
            (40, target, 'INFO', PROCESS, THREAD),
            (50, target + '::private', 'INFO', PROCESS, THREAD),
            (60, target + '_private', 'INFO', PROCESS, THREAD),
            (70, target, 'INFO', REUSED, THREAD),
            (80, target, 'INFO', OTHER, THREAD),
            (90, target, 'INFO', PROCESS, OTHER_THREAD),
            (100, target, 'INFO', PROCESS, None),
        ):
            self.insert(ts=seconds, target=label, level=level, process=process, thread=thread)
        before = self.db.read_bytes()
        original = sqlite3.connect
        queries = []

        def observed(*args, **kwargs):
            connection = original(*args, **kwargs)
            connection.set_trace_callback(queries.append)
            return connection

        with patch.object(timeline.sqlite3, 'connect', side_effect=observed):
            exact = self.read(process_uuid=PROCESS, thread_id=THREAD)
            by_pid = self.read()
            by_process = timeline.read_timeline(self.db, process_uuid=PROCESS)
            by_thread = timeline.read_timeline(self.db, thread_id=THREAD)
            limited = self.read(process_uuid=PROCESS, thread_id=THREAD, limit=2)
        results = (exact, by_pid, by_process, by_thread, limited)
        self.assertTrue(self.db.read_bytes() == before, 'reader changed controlled database')
        self.assertNotIn(PRIVATE, json.dumps(results))
        selects = [query for query in queries if query.startswith('SELECT')]
        self.assertEqual(len(selects), 5)
        for query in selects:
            for forbidden in ('feedback_log_body', 'module_path', 'file', 'message'):
                self.assertNotIn(forbidden, query)
        for column in ('feedback_log_body', 'module_path', 'file', 'line'):
            self.assertEqual(timeline._authorize(sqlite3.SQLITE_READ, 'logs', column,
                                                'main', None), sqlite3.SQLITE_DENY)
        self.assertEqual([row['id'] for row in exact['rows']], [1, 2, 3, 4])
        self.assertEqual([row['selected_row_gap_ns'] for row in exact['rows']],
                         [None, None, None, 20_000_000_000])
        self.assertEqual([row['id'] for row in by_pid['rows']], [1, 2, 3, 4, 7, 9, 10])
        self.assertEqual(by_pid['rows'][-1]['row_gap_status'], 'identity_unavailable')
        self.assertEqual([row['id'] for row in by_process['rows']], [1, 2, 3, 4, 9, 10])
        self.assertEqual([row['id'] for row in by_thread['rows']], [1, 2, 3, 4, 7, 8])
        self.assertEqual([row['id'] for row in limited['rows']], [3, 4])
        self.assertTrue(all(row['selected_row_gap_ns'] is None for row in limited['rows']))
        self.assertTrue(limited['more_rows_available'])
        self.assertTrue(all(result['omitted_invalid_scope_rows'] == 0 for result in results))

    def test_cli_includes_literal_http_operation_metadata(self):
        target = 'codex_api::endpoint::alab_request_diagnostics::http_operations'
        for seconds, label, process, thread in (
            (10, 'codex_api::endpoint::alab_request_diagnostics', PROCESS, THREAD),
            (20, target, PROCESS, THREAD),
            (30, target, PROCESS, THREAD),
            (40, target + '::private', PROCESS, THREAD),
            (50, target, OTHER, THREAD),
            (60, target + '_private', PROCESS, THREAD),
            (70, target, PROCESS, OTHER_THREAD),
            (80, 'codex_core::responses_retry::alab_capacity_retry', PROCESS, THREAD),
        ):
            self.insert(ts=seconds, target=label, process=process, thread=thread)
        before = self.db.read_bytes()
        result = self.cli(['--pid', '42', '--process-uuid', PROCESS,
                           '--thread-id', THREAD, '--limit', '2'])
        self.assertTrue(self.db.read_bytes() == before, 'CLI changed controlled database')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, '')
        self.assertEqual(len(result.stdout.splitlines()), 1)
        self.assertNotIn(PRIVATE, result.stdout)
        output = json.loads(result.stdout)
        self.assertEqual([row['id'] for row in output['rows']], [3, 8])
        self.assertEqual([row['target'] for row in output['rows']],
                         [target, 'codex_core::responses_retry::alab_capacity_retry'])
        self.assertTrue(output['more_rows_available'])
        self.assertEqual(output['omitted_invalid_scope_rows'], 0)
        self.assertTrue(all(row['process_uuid'] == PROCESS and row['thread_id'] == THREAD
                            for row in output['rows']))

    def test_target_filter_precedes_latest_row_limit_and_intersects_thread_scope(self):
        target = 'codex_core::responses_retry::alab_capacity_retry'
        for seconds in (10, 20, 30):
            self.insert(ts=seconds, target=target)
        self.insert(ts=40, target=target, thread=OTHER_THREAD)
        self.insert(ts=50, target=target + '::other')
        self.insert(ts=60)
        default = self.read(thread_id=THREAD, limit=1)
        self.assertEqual([row['id'] for row in default['rows']], [6])
        self.assertNotIn('target', default['scope'])
        for since, more in ((10, True), (20, False)):
            result = self.cli(['--pid', '42', '--thread-id', THREAD, '--target', target,
                               '--since', str(since), '--limit', '2'])
            self.assertEqual(result.returncode, 0, result.stderr)
            output = json.loads(result.stdout)
            self.assertEqual(output['scope']['target'], target)
            self.assertEqual([row['id'] for row in output['rows']], [2, 3])
            self.assertIs(output['more_rows_available'], more)
            self.assertNotIn(PRIVATE, result.stdout)

    def test_target_filter_requires_identity_and_an_exact_allowed_target(self):
        target = timeline.TARGETS[0]
        self.error('scope_required', lambda: timeline.read_timeline(self.db, target=target))
        for invalid in (PRIVATE, target.upper(), target + '::other', '%', '', 1, []):
            self.error('invalid_target', lambda: self.read(target=invalid))
        result = self.cli(['--pid', '42', '--target', PRIVATE])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('invalid_target', result.stderr)
        self.assertNotIn(PRIVATE, result.stderr + result.stdout)

    def test_pid_scope_does_not_match_pid_prefix_or_group_reused_pid(self):
        self.insert(ts=10)
        self.insert(ts=20, process=OTHER)
        self.insert(ts=30, process=REUSED)
        self.insert(ts=40)
        rows = self.read()['rows']
        self.assertEqual([row['process_uuid'] for row in rows], [PROCESS, REUSED, PROCESS])
        self.assertEqual([row['selected_row_gap_ns'] for row in rows], [None, None, 30_000_000_000])

    def test_exact_process_and_thread_scopes_are_intersections(self):
        self.insert()
        self.insert(process=REUSED)
        self.insert(thread=OTHER_THREAD)
        self.assertEqual(len(self.read(process_uuid=PROCESS, thread_id=THREAD)['rows']), 1)
        self.assertEqual(timeline.read_timeline(self.db, pid=142, process_uuid=PROCESS)['rows'], [])

    def test_thread_only_scope_keeps_unknown_process_unavailable(self):
        for process in (None, PRIVATE, 'pid:42:bad', PROCESS):
            self.insert(process=process)
        rows = timeline.read_timeline(self.db, thread_id=THREAD)['rows']
        self.assertEqual([row['process_uuid'] for row in rows], [None, None, None, PROCESS])
        self.assertTrue(all(row['selected_row_gap_ns'] is None for row in rows))

    def test_threadless_and_invalid_thread_rows_never_share_a_gap(self):
        for thread in (None, None, PRIVATE, THREAD.upper(), b'not-text', THREAD):
            self.insert(thread=thread)
        rows = self.read()['rows']
        self.assertEqual([row['thread_id'] for row in rows], [None, None, None, None, None, THREAD])
        self.assertTrue(all(row['selected_row_gap_ns'] is None for row in rows))

    def test_gap_group_requires_same_target_and_level(self):
        self.insert(ts=10)
        self.insert(ts=20, level='WARN')
        self.insert(ts=30, target=timeline.TARGETS[1])
        self.insert(ts=40)
        rows = self.read()['rows']
        self.assertEqual([row['selected_row_gap_ns'] for row in rows], [None, None, None, 30_000_000_000])
        self.assertIn('not retry, deadline or outage', self.read()['gap_meaning'])

    def test_backward_and_invalid_timestamps_do_not_become_durations(self):
        self.insert(ts=100, nanos=9)
        self.insert(ts=99)
        self.insert(ts=100, nanos=1_000_000_000)
        self.insert(ts=101)
        self.insert(ts=102, nanos=4)
        rows = self.read()['rows']
        self.assertEqual([row['row_gap_status'] for row in rows],
                         ['first_selected_row', 'backward_timestamp', 'timestamp_unavailable',
                          'first_selected_row', 'measured'])
        self.assertEqual(rows[-1]['selected_row_gap_ns'], 1_000_000_004)
        self.assertIsNone(rows[2]['ts'])

    def test_latest_limit_and_since_are_explicit(self):
        for ts in range(10, 15):
            self.insert(ts=ts)
        result = self.read(limit=2, since=11)
        self.assertEqual([row['ts'] for row in result['rows']], [13, 14])
        self.assertTrue(result['more_rows_available'])
        self.assertFalse(self.read(limit=2, since=13)['more_rows_available'])

    def test_since_excludes_noninteger_seconds_before_limited_window(self):
        for storage_class, seconds in (('text', PRIVATE),
                                       ('blob', PRIVATE.encode()), ('real', 100.5)):
            with self.subTest(storage_class=storage_class):
                self.writer.execute('DELETE FROM logs')
                self.writer.commit()
                self.insert(ts=100)
                self.insert(ts=seconds)
                self.assertEqual(self.writer.execute(
                    'SELECT typeof(ts) FROM logs ORDER BY id DESC LIMIT 1'
                ).fetchone()[0], storage_class)
                result = self.read(limit=1, since=100)
                self.assertEqual(len(result['rows']), 1)
                self.assertNotIn(PRIVATE, json.dumps(result))
                row = result['rows'][0]
                self.assertEqual((row['ts'], row['ts_nanos'], row['row_gap_status']),
                                 (100, 0, 'first_selected_row'))
                self.assertFalse(result['more_rows_available'])

    def test_since_integer_boundary_affinity_and_maximum_are_preserved(self):
        for seconds in (99, 100, '100', 2**63 - 1):
            self.insert(ts=seconds)
        self.assertEqual(self.writer.execute(
            'SELECT typeof(ts) FROM logs ORDER BY id'
        ).fetchall(), [('integer',)] * 4)
        result = self.read(since=100)
        self.assertEqual([row['ts'] for row in result['rows']], [100, 100, 2**63 - 1])
        self.assertFalse(result['more_rows_available'])
        maximum = self.read(since=2**63 - 1, limit=1)
        self.assertEqual([row['ts'] for row in maximum['rows']], [2**63 - 1])
        self.assertFalse(maximum['more_rows_available'])

    def test_without_since_keeps_noninteger_seconds_unavailable_without_gaps(self):
        for seconds in (10, PRIVATE, 20, PRIVATE.encode(), 30, 30.5, 40):
            self.insert(ts=seconds)
        result = self.read()
        rows = result['rows']
        self.assertEqual([row['ts'] for row in rows], [10, None, 20, None, 30, None, 40])
        self.assertEqual([row['row_gap_status'] for row in rows],
                         ['first_selected_row', 'timestamp_unavailable',
                          'first_selected_row', 'timestamp_unavailable',
                          'first_selected_row', 'timestamp_unavailable', 'first_selected_row'])
        self.assertTrue(all(row['selected_row_gap_ns'] is None for row in rows))
        self.assertTrue(all(row['ts_nanos'] is None for row in rows if row['ts'] is None))
        self.assertNotIn(PRIVATE, json.dumps(result))

    def test_maximum_row_limit_still_bounds_actual_results(self):
        self.writer.executemany(
            'INSERT INTO logs(ts,ts_nanos,level,target,process_uuid,thread_id) '
            'VALUES(100,0,?,?,?,?)',
            [('INFO', timeline.TARGETS[0], PROCESS, THREAD)] * 1002)
        self.writer.commit()
        result = self.read(limit=1000)
        self.assertEqual(len(result['rows']), 1000)
        self.assertEqual(result['rows'][0]['id'], 3)
        self.assertTrue(result['more_rows_available'])

    def test_invalid_scopes_limits_and_since_are_rejected(self):
        self.error('scope_required', lambda: timeline.read_timeline(self.db))
        for pid in (0, -1, 2**32, True):
            self.error('invalid_scope', lambda: timeline.read_timeline(self.db, pid=pid))
        for process in ('pid:042:' + PROCESS.split(':')[2], PROCESS.upper(),
                        PROCESS + PRIVATE, 'pid:0:' + PROCESS.split(':')[2]):
            self.error('invalid_scope', lambda: self.read(process_uuid=process))
        for thread in (PRIVATE, THREAD.upper(), '{' + THREAD + '}'):
            self.error('invalid_scope', lambda: self.read(thread_id=thread))
        for limit in (0, 1001, True):
            self.error('invalid_limit', lambda: self.read(limit=limit))
        for since in (-1, 2**63, 1.5, True):
            self.error('invalid_since', lambda: self.read(since=since))

    def test_invalid_identity_metadata_is_not_rendered_or_repaired(self):
        for process in ('pid:42:' + PRIVATE, 'pid:42:' + 'x' * 2000):
            self.insert(process=process)
        result = self.read()
        self.assertEqual(result['rows'], [])
        self.assertEqual(result['omitted_invalid_scope_rows'], 2)
        self.assertNotIn(PRIVATE, json.dumps(result))
        self.insert(process=PROCESS.encode())
        rows = timeline.read_timeline(self.db, thread_id=THREAD)['rows']
        self.assertTrue(all(row['process_uuid'] is None for row in rows))

    def test_oversized_selected_metadata_fails_without_rendering_content(self):
        self.insert(process=PRIVATE * 3000)
        self.error('metadata_too_large', lambda: timeline.read_timeline(self.db, thread_id=THREAD))

    def test_missing_database_is_not_created_and_directories_are_rejected(self):
        absent = self.db.with_name('missing.sqlite')
        self.error('database_unavailable', lambda: timeline.read_timeline(absent, pid=42))
        self.assertFalse(absent.exists())
        self.error('database_not_regular', lambda: timeline.read_timeline(self.db.parent, pid=42))

    def test_legacy_resolve_runtime_error_is_normalized_before_sqlite(self):
        for entry in ('api', 'main'):
            with self.subTest(entry=entry):
                fault = RuntimeError('Symlink loop from /' + PRIVATE)
                stdout, stderr = io.StringIO(), io.StringIO()
                result, failure = None, None
                with patch.object(timeline.Path, 'resolve', side_effect=fault) as resolve, \
                        patch.object(timeline.sqlite3, 'connect') as connect, \
                        redirect_stdout(stdout), redirect_stderr(stderr):
                    try:
                        result = (timeline.read_timeline(self.db, pid=42) if entry == 'api'
                                  else timeline.main(['--db', str(self.db), '--pid', '42']))
                    except Exception as error:
                        failure = error
                resolve.assert_called_once_with(strict=True)
                connect.assert_not_called()
                self.assertEqual(stdout.getvalue(), '')
                if entry == 'api':
                    self.assertIsInstance(failure, timeline.TimelineError)
                    self.assertEqual(failure.category, 'database_unavailable')
                    self.assertNotIn(PRIVATE, str(failure))
                    self.assertEqual(stderr.getvalue(), '')
                else:
                    self.assertIsNone(failure)
                    self.assertEqual(result, 1)
                    self.assertEqual(stderr.getvalue(), 'request timeline: database_unavailable\n')
                    self.assertNotIn(PRIVATE, stderr.getvalue())

    def test_real_symlink_loop_is_a_sanitized_path_failure(self):
        first = self.db.with_name(PRIVATE + '-a.sqlite')
        second = self.db.with_name(PRIVATE + '-b.sqlite')
        first.symlink_to(second.name)
        second.symlink_to(first.name)
        with self.assertRaises((OSError, RuntimeError)):
            first.resolve(strict=True)
        failure = None
        with patch.object(timeline.sqlite3, 'connect') as connect:
            try:
                timeline.read_timeline(first, pid=42)
            except Exception as error:
                failure = error
        connect.assert_not_called()
        self.assertIsInstance(failure, timeline.TimelineError)
        self.assertEqual(failure.category, 'database_unavailable')
        self.assertNotIn(PRIVATE, str(failure))
        result = subprocess.run(
            [sys.executable, '-I', '-B', str(ALAB / 'request_timeline.py'),
             '--db', str(first), '--pid', '42'], capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, '')
        self.assertEqual(result.stderr, 'request timeline: database_unavailable\n')
        self.assertNotIn(PRIVATE, result.stdout + result.stderr)

    def test_path_normalization_preserves_validation_cancellation_and_later_errors(self):
        with patch.object(timeline.Path, 'resolve', side_effect=RuntimeError(PRIVATE)) as resolve, \
                patch.object(timeline.sqlite3, 'connect') as connect:
            self.error('scope_required', lambda: timeline.read_timeline(self.db))
            self.error('invalid_scope', lambda: timeline.read_timeline(self.db, pid=0))
        resolve.assert_not_called()
        connect.assert_not_called()
        for fault in (SystemExit(0), KeyboardInterrupt(PRIVATE)):
            with self.subTest(cancellation=type(fault).__name__):
                with patch.object(timeline.Path, 'resolve', side_effect=fault), \
                        patch.object(timeline.sqlite3, 'connect') as connect:
                    with self.assertRaises(type(fault)) as caught:
                        timeline.read_timeline(self.db, pid=42)
                self.assertIs(caught.exception, fault)
                connect.assert_not_called()
        fault = RuntimeError(PRIVATE)
        original = sqlite3.connect
        with patch.object(timeline, '_schema', side_effect=fault), \
                patch.object(timeline.sqlite3, 'connect', wraps=original) as connect:
            with self.assertRaises(RuntimeError) as caught:
                timeline.read_timeline(self.db, pid=42)
        self.assertIs(caught.exception, fault)
        connect.assert_called_once()

    def test_invalid_database_error_does_not_render_sqlite_text(self):
        bad = self.db.with_name(PRIVATE + '.sqlite')
        bad.write_text(PRIVATE)
        failure = self.error('invalid_database', lambda: timeline.read_timeline(bad, pid=42))
        self.assertEqual(failure.sqlite_code & 0xff, sqlite3.SQLITE_NOTADB)

    def test_current_schema_required_and_views_and_generated_columns_rejected(self):
        for setup in (
            'DROP TABLE logs; CREATE TABLE logs(message TEXT)',
            'ALTER TABLE logs RENAME COLUMN feedback_log_body TO message',
            'ALTER TABLE logs RENAME TO actual; CREATE VIEW logs AS SELECT * FROM actual',
            'ALTER TABLE logs ADD COLUMN generated TEXT GENERATED ALWAYS AS (feedback_log_body)',
        ):
            with self.subTest(setup=setup):
                path = self.db.with_name('schema.sqlite')
                self.writer.backup(destination := sqlite3.connect(path))
                destination.executescript(setup)
                destination.close()
                self.error('unsupported_schema', lambda: timeline.read_timeline(path, pid=42))
                path.unlink()

    def test_primary_key_desc_cannot_render_an_arbitrary_text_id(self):
        path = self.db.with_name('descending.sqlite')
        with closing(sqlite3.connect(path)) as writer, writer:
            for migration in sorted(MIGRATIONS.glob('*.sql')):
                writer.executescript(migration.read_text().replace(
                    'id INTEGER PRIMARY KEY AUTOINCREMENT', 'id INTEGER PRIMARY KEY DESC'))
            writer.execute(
                'INSERT INTO logs(id,ts,ts_nanos,level,target,process_uuid,thread_id) '
                'VALUES(?,100,0,?,?,?,?)', (PRIVATE, 'INFO', timeline.TARGETS[0], PROCESS, THREAD))
            self.assertEqual(writer.execute('SELECT typeof(id) FROM logs').fetchone()[0], 'text')
        self.error('invalid_metadata', lambda: timeline.read_timeline(path, pid=42))

    def test_close_fault_preserves_primary_and_cannot_report_success(self):
        original = sqlite3.connect

        self.insert()
        for primary in ('none', 'schema', 'locked'):
            if primary == 'schema':
                self.writer.execute('DROP TABLE logs')
                self.writer.commit()
            elif primary == 'locked':
                self.writer.execute('BEGIN EXCLUSIVE')
            for fault in (RuntimeError(PRIVATE), sqlite3.OperationalError(PRIVATE),
                          SystemExit(0), KeyboardInterrupt(PRIVATE)):
                with self.subTest(primary=primary, fault=type(fault).__name__):
                    def connect(*args, **kwargs):
                        return FaultOnClose(original(*args, **kwargs), fault)

                    stdout, stderr = io.StringIO(), io.StringIO()
                    with patch.object(timeline.sqlite3, 'connect', side_effect=connect), \
                            redirect_stdout(stdout), redirect_stderr(stderr):
                        result = timeline.main(['--db', str(self.db), '--pid', '42'])
                    self.assertEqual(result, 1)
                    self.assertEqual(stdout.getvalue(), '')
                    expected = {'schema': 'unsupported_schema',
                                'none': 'database_close_failed',
                                'locked': 'database_busy (sqlite_code=5)'}[primary]
                    self.assertEqual(stderr.getvalue(), 'request timeline: ' + expected + '\n')
                    self.assertNotIn(PRIVATE, stderr.getvalue())
            if primary == 'locked':
                self.writer.rollback()

    def test_callers_active_exception_cannot_hide_close_only_failure(self):
        self.insert()
        original = sqlite3.connect
        for fault in (RuntimeError(PRIVATE), SystemExit(0)):
            def connect(*args, **kwargs):
                return FaultOnClose(original(*args, **kwargs), fault)

            stdout, stderr = io.StringIO(), io.StringIO()
            try:
                raise RuntimeError('unrelated caller exception')
            except RuntimeError:
                with patch.object(timeline.sqlite3, 'connect', side_effect=connect), \
                        redirect_stdout(stdout), redirect_stderr(stderr):
                    result = timeline.main(['--db', str(self.db), '--pid', '42'])
            self.assertEqual(result, 1)
            self.assertEqual(stdout.getvalue(), '')
            self.assertEqual(stderr.getvalue(), 'request timeline: database_close_failed\n')

    def test_vm_budget_is_enforced_without_partial_result(self):
        self.writer.executemany(
            'INSERT INTO logs(ts,ts_nanos,level,target,process_uuid,thread_id) '
            'VALUES(100,0,?,?,?,?)', [('INFO', PRIVATE, PROCESS, THREAD)] * 2000)
        self.writer.commit()
        with patch.object(timeline, 'VM_STEPS', 5000):
            failure = self.error('query_budget_exceeded', self.read)
        self.assertEqual(failure.sqlite_code & 0xff, sqlite3.SQLITE_INTERRUPT)

    def test_elapsed_budget_is_enforced_even_for_small_queries(self):
        self.insert()
        with patch.object(timeline, 'QUERY_SECONDS', 0):
            self.error('query_budget_exceeded', self.read)

    def test_database_lock_is_bounded_and_error_is_sanitized(self):
        self.writer.execute('BEGIN EXCLUSIVE')
        try:
            failure = self.error('database_busy', self.read)
            self.assertIn(failure.sqlite_code & 0xff, (sqlite3.SQLITE_BUSY, sqlite3.SQLITE_LOCKED))
        finally:
            self.writer.rollback()

    def test_read_only_file_is_unchanged(self):
        self.insert()
        before = self.db.read_bytes()
        self.db.chmod(0o444)
        self.addCleanup(self.db.chmod, 0o600)
        self.assertEqual(len(self.read()['rows']), 1)
        self.assertEqual(self.db.read_bytes(), before)

    def test_active_wal_commits_are_visible_without_immutable(self):
        self.assertEqual(self.writer.execute('PRAGMA journal_mode=WAL').fetchone()[0], 'wal')
        self.writer.execute('PRAGMA wal_autocheckpoint=0')
        self.insert(ts=123)
        self.assertTrue(Path(str(self.db) + '-wal').exists())
        self.assertEqual(self.read()['rows'][0]['ts'], 123)
        self.insert(ts=124)
        self.assertEqual([row['ts'] for row in self.read()['rows']], [123, 124])
        self.writer.execute(
            'INSERT INTO logs(ts,ts_nanos,level,target,process_uuid,thread_id) '
            'VALUES(999,0,?,?,?,?)', ('INFO', timeline.TARGETS[0], PROCESS, THREAD))
        self.assertEqual([row['ts'] for row in self.read()['rows']], [123, 124])
        self.writer.rollback()

    def test_column_collations_cannot_expand_fixed_labels_or_exact_scopes(self):
        path = self.db.with_name('collated.sqlite')
        with closing(sqlite3.connect(path)) as writer, writer:
            for migration in sorted(MIGRATIONS.glob('*.sql')):
                sql = migration.read_text()
                for column in ('level', 'target', 'process_uuid', 'thread_id'):
                    sql = sql.replace(column + ' TEXT', column + ' TEXT COLLATE NOCASE')
                writer.executescript(sql)
            writer.executemany(
                'INSERT INTO logs(ts,ts_nanos,level,target,process_uuid,thread_id) '
                'VALUES(100,0,?,?,?,?)', [
                    ('info', timeline.TARGETS[0], PROCESS, THREAD),
                    ('INFO', timeline.TARGETS[0].upper(), PROCESS, THREAD),
                    ('INFO', timeline.TARGETS[0], PROCESS, THREAD.upper()),
                    ('INFO', timeline.TARGETS[0], PROCESS.upper(), THREAD),
                    ('INFO\x00' + PRIVATE, timeline.TARGETS[0], PROCESS, THREAD),
                ])
        result = timeline.read_timeline(path, process_uuid=PROCESS, thread_id=THREAD)
        self.assertEqual(result['rows'], [])

    def test_cli_emits_one_json_document_and_no_private_content(self):
        self.insert()
        result = self.cli(['--pid', '42'])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, '')
        self.assertEqual(len(result.stdout.splitlines()), 1)
        self.assertEqual(len(json.loads(result.stdout)['rows']), 1)
        self.assertNotIn(PRIVATE, result.stdout)

    def test_cli_failure_is_nonzero_and_never_partial_success(self):
        self.insert()
        result = self.cli(['--pid', '42', '--limit', '1001'])
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, '')
        self.assertEqual(result.stderr, 'request timeline: invalid_limit\n')
        self.writer.execute('DROP TABLE logs')
        self.writer.commit()
        result = self.cli(['--pid', '42'])
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, '')
        self.assertNotIn(PRIVATE, result.stderr)

    def test_cli_sanitizes_unexpected_failure_and_zero_exit_is_incomplete(self):
        for fault, expected in ((RuntimeError(PRIVATE), 'operation_failed'),
                                (SystemExit(0), 'operation incomplete')):
            stdout, stderr = io.StringIO(), io.StringIO()
            with patch.object(timeline, 'read_timeline', side_effect=fault), \
                    redirect_stdout(stdout), redirect_stderr(stderr):
                result = timeline.main(['--db', str(self.db), '--pid', '42'])
            self.assertEqual(result, 1)
            self.assertEqual(stdout.getvalue(), '')
            self.assertIn(expected, stderr.getvalue())
            self.assertNotIn(PRIVATE, stderr.getvalue())


if __name__ == '__main__':
    unittest.main()

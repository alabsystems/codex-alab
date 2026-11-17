# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Public diagnostics retain naturally generated cleanup notes and healthy stages."""
import json
from pathlib import Path
import unittest
from unittest.mock import patch

import test_artifact_cleanup as cleanup_tests
import test_upstream_intake as intake_tests
import upstream_status


class ReportingFailure(ValueError):
    def __init__(self, message, fault, mode='read'):
        super().__init__(message)
        self.reporting_fault = fault
        self.reporting_mode = mode
        self.inspection_complete = False

    def add_note(self, note):
        super().add_note(note)

    def __getattribute__(self, name):
        if name == '__notes__' and super().__getattribute__('inspection_complete'):
            fault = super().__getattribute__('reporting_fault')
            if super().__getattribute__('reporting_mode') == 'read':
                raise fault
            notes = super().__getattribute__('__notes__')

            def interrupted_iteration():
                yield notes[0]
                raise fault

            return interrupted_iteration()
        return super().__getattribute__(name)


class ArtifactDiagnosticsTests(unittest.TestCase):
    setUp = cleanup_tests.ArtifactCleanupTests.setUp
    tearDown = cleanup_tests.ArtifactCleanupTests.tearDown
    inspect = cleanup_tests.ArtifactCleanupTests.inspect
    eio = cleanup_tests.ArtifactCleanupTests.eio

    def healthy_selection(self):
        package = self.package.parent / 'healthy'
        (package / 'bin').mkdir(parents=True)
        (package / 'bin/codex').write_bytes((self.package / 'bin/codex').read_bytes())
        (package / 'bin/codex').chmod(0o755)
        (package / 'alab-build.json').write_text(
            json.dumps({'source': 'b' * 40, 'version': 'healthy fixture'}))
        return package / 'bin/codex'

    def observe_failure(self, primary, faults, *, source_case=None):
        status = cleanup_tests.STATUS
        actual_package_identity = status.package_identity
        selected = self.healthy_selection()

        def package_stage(directory):
            if Path(directory).resolve() == self.package:
                try:
                    with patch.object(status, 'package_identity', actual_package_identity):
                        return self.inspect(primary, close_faults=faults)
                finally:
                    if isinstance(primary, ReportingFailure):
                        primary.inspection_complete = True
            return actual_package_identity(directory)

        with patch.object(status, 'package_identity', side_effect=package_stage):
            if source_case is not None:
                return upstream_status.report(
                    source_case.repo, source_case.base, source_case.target,
                    package=self.package, selected_executable=selected)
            return status.observe(package=self.package, selected_executable=selected)

    def test_natural_cleanup_notes_follow_primary_in_close_order(self):
        primary = ValueError('primary metadata refusal')
        faults = {3: self.eio(3), 1: self.eio(1)}
        result = self.inspect(primary, close_faults=faults, observation=True)
        notes = [f'artifact descriptor cleanup failed: {faults[index]!r}' for index in (3, 1)]
        self.assertEqual(primary.__notes__, notes)
        self.assertEqual(result['errors'], ['packaged: primary metadata refusal'] +
                         ['packaged note: ' + note for note in notes])
        self.assertIsNone(result['packaged_sha'])
        self.assertNotIn('packaged', result['observations'])

    def test_error_without_notes_retains_exact_primary_output(self):
        result = self.inspect(ValueError('primary metadata refusal'), observation=True)
        self.assertEqual(result['errors'], ['packaged: primary metadata refusal'])

    def test_packaged_notes_do_not_discard_healthy_selected_identity(self):
        primary = ValueError('primary metadata refusal')
        result = self.observe_failure(primary, {3: self.eio(3), 1: self.eio(1)})
        self.assertEqual(result['errors'], ['packaged: primary metadata refusal'] +
                         ['packaged note: ' + note for note in primary.__notes__])
        self.assertIsNone(result['packaged_sha'])
        self.assertEqual(result['selected_sha'], 'b' * 40)
        self.assertEqual(result['observations']['selected']['source'], 'b' * 40)

    def test_ordinary_note_access_failure_keeps_primary_and_healthy_selection(self):
        primary = ReportingFailure('primary metadata refusal', RuntimeError('note access failure'))
        result = self.observe_failure(primary, {3: self.eio(3), 1: self.eio(1)})
        self.assertEqual(len(BaseException.__getattribute__(primary, '__notes__')), 2)
        self.assertEqual(result['errors'], ['packaged: primary metadata refusal'])
        self.assertEqual(result['selected_sha'], 'b' * 40)

    def test_interrupted_note_iteration_keeps_already_read_note_and_healthy_selection(self):
        primary = ReportingFailure('primary metadata refusal', RuntimeError('note iteration failure'),
                                   mode='iterate')
        result = self.observe_failure(primary, {3: self.eio(3), 1: self.eio(1)})
        notes = BaseException.__getattribute__(primary, '__notes__')
        self.assertEqual(len(notes), 2)
        self.assertEqual(result['errors'], ['packaged: primary metadata refusal',
                                           'packaged note: ' + notes[0]])
        self.assertEqual(result['selected_sha'], 'b' * 40)

    def test_note_reporting_keyboard_interrupt_preserves_exact_identity(self):
        cancellation = KeyboardInterrupt('stop note observation')
        primary = ReportingFailure('primary metadata refusal', cancellation)
        with self.assertRaises(BaseException) as caught:
            self.observe_failure(primary, {3: self.eio(3)})
        self.assertIs(caught.exception, cancellation)
        self.assertEqual(len(BaseException.__getattribute__(primary, '__notes__')), 1)

    def test_note_reporting_system_exit_preserves_exact_identity(self):
        cancellation = SystemExit(47)
        primary = ReportingFailure('primary metadata refusal', cancellation)
        with self.assertRaises(BaseException) as caught:
            self.observe_failure(primary, {3: self.eio(3)})
        self.assertIs(caught.exception, cancellation)
        self.assertEqual(len(BaseException.__getattribute__(primary, '__notes__')), 1)

    def test_source_report_retains_cleanup_notes_and_independent_source_fields(self):
        source_case = intake_tests.IntakeTests()
        source_case.setUp()
        self.addCleanup(source_case.doCleanups)
        primary = ValueError('primary metadata refusal')
        result = self.observe_failure(primary, {3: self.eio(3)}, source_case=source_case)
        expected = ['packaged: primary metadata refusal', 'packaged note: ' + primary.__notes__[0]]
        self.assertEqual(result['errors'], expected)
        self.assertEqual(result['adoption']['errors'], expected)
        self.assertIsNone(result['packaged_sha'])
        self.assertEqual(result['selected_sha'], 'b' * 40)
        self.assertEqual(result['main_sha'], source_case.base)
        self.assertEqual(result['upstream_sha'], source_case.target)
        self.assertEqual(result['integrated_frontier_sha'], source_case.base)
        self.assertEqual(result['pending_count'], 1)


if __name__ == '__main__':
    unittest.main()

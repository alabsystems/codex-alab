# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""CLI interruption guards leave updater APIs, journals and guardian custody intact."""

from contextlib import redirect_stderr, redirect_stdout
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location(
    'local_update_cli', Path(__file__).resolve().parents[1] / 'local_update.py')
UPDATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(UPDATE)


CHILD = r'''
import ast
import builtins
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys
from types import SimpleNamespace

path, mode, case, witness_path = Path(sys.argv[1]), sys.argv[2], sys.argv[3], Path(sys.argv[4])
spec = importlib.util.spec_from_file_location('controlled_local_update', path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
faults = {'zero': SystemExit(0), 'none': SystemExit(None), 'false': SystemExit(False),
          'wrapped': SystemExit(256), 'negative_wrapped': SystemExit(-256),
          'nonzero': SystemExit(47), 'floating_zero': SystemExit(0.0),
          'huge': SystemExit(sys.maxsize + 1),
          'keyboard': KeyboardInterrupt('controlled updater cancellation')}
primary = faults.get(case)
report_fault = None
if case == 'ordinary' or case.startswith('report_'):
    primary = OSError(5, 'controlled updater failure')
if case.startswith('report_'):
    report_fault = faults[case.removeprefix('report_')]

    class FailingStderr:
        def write(self, _text):
            raise report_fault

        def flush(self):
            pass

    sys.stderr = FailingStderr()
if case == 'format_wrapped':
    report_fault = SystemExit(256)

    class FormattingFailure(OSError):
        def __str__(self):
            raise report_fault

    primary = FormattingFailure('controlled formatting failure')
witness = dict(mode=mode, case=case, run_called=False, run_returned=False,
               admission_called=False, cache_hint_called=False, guardian_load_called=False)

def operation(*arguments, **keywords):
    witness['run_called'] = True
    witness['run_arguments'] = arguments
    witness['run_keywords'] = keywords
    if primary is not None:
        raise primary
    witness['run_returned'] = True
    return 7 if case == 'returned_failure' else 0

def hint(_cache):
    witness['cache_hint_called'] = True
    return case == 'busy'

def admission(_argv):
    witness['admission_called'] = True
    return SimpleNamespace(fixed_source='a' * 40,
        expected_checkout=str(witness_path.parent / 'original checkout'),
        expected_prefix=str(witness_path.parent / 'original prefix'),
        initial_selected_source='b' * 40, initial_selected_version='0.1.0')

module.run_locked = operation
module.cache_busy = hint
os.environ['ALAB_PACKAGE_CACHE'] = str(witness_path.parent / 'unopened-cache')
argv = ['--repo', str(witness_path.parent), '--out', str(witness_path.parent / 'unstarted-output'),
        '--prefix', str(witness_path.parent / 'unselected-prefix')]
tree = ast.parse(path.read_bytes(), filename=str(path))
if mode == 'main':
    entries = [node for node in tree.body if isinstance(node, ast.If)
               and isinstance(node.test, ast.Compare) and isinstance(node.test.left, ast.Name)
               and node.test.left.id == '__name__']
    assert len(entries) == 1
    program = ast.Module(body=entries, type_ignores=[])
    witness['entry_ast_sha256'] = hashlib.sha256(ast.dump(program).encode()).hexdigest()
    sys.argv = [str(path), '--help'] if case == 'help' else [str(path), *argv]
    module.__name__ = '__main__'
    namespace = module.__dict__
elif mode == 'fixed':
    module.fixed_source_args = admission
    # Use the source's own guardian target, including the old target in the
    # counterfactual. A lookup of a newly added adapter would not prove the bug.
    programs = [node.value for node in ast.walk(tree) if isinstance(node, ast.Constant)
                and isinstance(node.value, str) and 'sys.exit(ns[' in node.value
                and 'run_fixed_' in node.value]
    assert len(programs) == 1
    program = programs[0]
    witness['guardian_program_sha256'] = hashlib.sha256(program.encode()).hexdigest()
    real_import = builtins.__import__

    def controlled_load(supplied):
        assert supplied == str(path)
        witness['guardian_load_called'] = True
        return module.__dict__

    def scoped_import(name, *arguments, **keywords):
        if name == 'runpy':
            return SimpleNamespace(run_path=controlled_load)
        return real_import(name, *arguments, **keywords)

    # The original inline program imports its already loaded owned source;
    # no guardian process, shell, lock or production entry is launched.
    namespace = {'__builtins__': dict(vars(builtins), __import__=scoped_import)}
    sys.argv = ['-c', str(path), *argv]
else:
    raise AssertionError(mode)
try:
    exec(compile(program, str(path), 'exec'), namespace)
except BaseException as error:
    witness.update(escaped_type=type(error).__name__, escaped_code=getattr(error, 'code', None),
                   escaped_primary_identity=error is primary, escaped_report_identity=error is report_fault)
    raise
finally:
    witness['output_exists'] = (witness_path.parent / 'unstarted-output').exists()
    witness['cache_exists'] = (witness_path.parent / 'unopened-cache').exists()
    witness['python_version'] = sys.version
    witness_path.write_text(json.dumps(witness))
'''


class LocalUpdateCliTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory(prefix='alab-local-update-cli-')
        self.addCleanup(scratch.cleanup)
        self.directory = Path(scratch.name)
        self.child_observations = []

    def arguments(self):
        return ['--repo', str(self.directory), '--out', str(self.directory / 'unstarted-output'),
                '--prefix', str(self.directory / 'unselected-prefix')]

    def admission(self):
        return SimpleNamespace(fixed_source='a' * 40,
            expected_checkout=str(self.directory / 'original checkout'),
            expected_prefix=str(self.directory / 'original prefix'),
            initial_selected_source='b' * 40, initial_selected_version='0.1.0')

    def child(self, mode, case):
        script = self.directory / 'child.py'
        script.write_text(CHILD)
        witness_path = self.directory / (mode + '-' + case + '.json')
        done = subprocess.run([
            sys.executable, '-I', '-B', str(script), str(Path(UPDATE.__file__)),
            mode, case, str(witness_path),
        ], cwd=self.directory, capture_output=True, text=True, timeout=10)
        witness = json.loads(witness_path.read_text())
        self.child_observations.append(dict(mode=mode, case=case, actual_exit=done.returncode,
                                            witness=witness, stdout=done.stdout, stderr=done.stderr))
        self.assertFalse(witness['output_exists'])
        self.assertFalse(witness['cache_exists'])
        self.assertEqual(witness['run_called'], case not in ('help', 'busy'))
        self.assertEqual(witness['run_returned'], case in ('success', 'returned_failure'))
        if mode == 'fixed':
            self.assertTrue(witness['guardian_load_called'])
            self.assertTrue(witness['admission_called'])
            self.assertFalse(witness['cache_hint_called'])
            self.assertEqual(witness['run_keywords'], dict(
                source='a' * 40, expected_checkout=str(self.directory / 'original checkout'),
                expected_prefix=str(self.directory / 'original prefix'),
                initial_details=dict(initial_selected_source='b' * 40,
                                     initial_selected_version='0.1.0'),
                guardian_contract=UPDATE.GUARDIAN_CONTRACT))
        else:
            self.assertEqual(witness['cache_hint_called'], case != 'help')
            self.assertFalse(witness['guardian_load_called'])
            self.assertFalse(witness['admission_called'])
        return done, witness

    def cancellations(self):
        return (SystemExit(0), SystemExit(None), SystemExit(False), SystemExit(256),
                SystemExit(-256), SystemExit(47), SystemExit(0.0),
                SystemExit(sys.maxsize + 1), KeyboardInterrupt('controlled updater cancellation'))

    def api_interruption(self, primary, *, through_main=False):
        statuses, channels, calls = [], [], []

        def update(*arguments, **keywords):
            calls.append((arguments, keywords))
            raise primary

        def status(_repo, code, reason, **details):
            statuses.append(dict(code=code, reason=reason, details=details))

        def channel(_display, code, phase, last_phase, details):
            channels.append(dict(code=code, phase=phase, last_phase=last_phase, details=dict(details)))

        returned, caught = None, None
        with mock.patch.object(UPDATE, 'update_once', side_effect=update), \
                mock.patch.object(UPDATE, 'record_status', side_effect=status), \
                mock.patch.object(UPDATE, 'record_channel_status', side_effect=channel), \
                mock.patch.object(UPDATE, 'cache_busy', return_value=False), \
                redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()) as stderr:
            try:
                returned = UPDATE.main(self.arguments()) if through_main else UPDATE.run_locked(self.arguments())
            except BaseException as error:
                caught = error
        self.assertEqual(len(calls), 1)
        self.assertEqual(len(statuses), 1)
        self.assertIsNone(statuses[0]['code'])
        self.assertEqual(statuses[0]['details']['phase'], 'checking')
        self.assertIn('completion not recorded', statuses[0]['reason'])
        self.assertEqual(len(channels), 1)
        self.assertIsNone(channels[0]['code'])
        self.assertEqual(channels[0]['phase'], 'checking')
        self.assertFalse((self.directory / 'unstarted-output').exists())
        return returned, caught, stderr.getvalue()

    def test_original_main_refuses_success_shaped_operation_interruptions(self):
        for case in ('zero', 'none', 'false', 'wrapped', 'negative_wrapped'):
            with self.subTest(case=case):
                done, witness = self.child('main', case)
                self.assertEqual(done.returncode, 1)
                self.assertFalse(witness['escaped_primary_identity'])
                self.assertIn('operation incomplete', done.stderr)

    def test_original_guardian_program_refuses_success_shaped_interruptions(self):
        for case in ('zero', 'none', 'false', 'wrapped', 'negative_wrapped'):
            with self.subTest(case=case):
                done, witness = self.child('fixed', case)
                self.assertEqual(done.returncode, 1)
                self.assertFalse(witness['escaped_primary_identity'])
                self.assertIn('operation incomplete', done.stderr)

    def test_original_main_reporting_faults_cannot_restore_success(self):
        for case in ('report_zero', 'report_none', 'report_false', 'report_wrapped',
                     'report_negative_wrapped', 'format_wrapped'):
            with self.subTest(case=case):
                done, witness = self.child('main', case)
                self.assertEqual(done.returncode, 1)
                self.assertFalse(witness['escaped_report_identity'])

    def test_original_programs_preserve_other_cancellation_identity(self):
        for mode in ('main', 'fixed'):
            for case, expected in (('nonzero', 47), ('keyboard', -2),
                                   ('floating_zero', 1), ('huge', 255)):
                with self.subTest(mode=mode, case=case):
                    done, witness = self.child(mode, case)
                    self.assertEqual(done.returncode, expected)
                    self.assertTrue(witness['escaped_primary_identity'])

    def test_main_reporting_nonzero_and_keyboard_identity_is_preserved(self):
        for case, expected in (('report_nonzero', 47), ('report_keyboard', -2),
                               ('report_floating_zero', 1), ('report_huge', 255)):
            with self.subTest(case=case):
                done, witness = self.child('main', case)
                self.assertEqual(done.returncode, expected)
                self.assertTrue(witness['escaped_report_identity'])

    def test_original_programs_keep_returned_status_help_and_busy_behavior(self):
        cases = [('main', case, status) for case, status in
                 (('success', 0), ('returned_failure', 7), ('help', 0), ('busy', 75), ('ordinary', 1))]
        cases += [('fixed', case, status) for case, status in
                  (('success', 0), ('returned_failure', 7), ('ordinary', 1))]
        for mode, case, status in cases:
            with self.subTest(mode=mode, case=case):
                done, _witness = self.child(mode, case)
                self.assertEqual(done.returncode, status)
                if case == 'help':
                    self.assertIn('usage:', done.stdout)
                elif case == 'busy':
                    self.assertIn('another packager owns the cache', done.stderr)
                elif mode == 'main' and case == 'ordinary':
                    self.assertEqual(done.stderr, 'alab: local update: [Errno 5] controlled updater failure\n')

    def test_run_locked_api_retains_cancellation_and_incomplete_journals(self):
        for primary in self.cancellations():
            with self.subTest(code=getattr(primary, 'code', None), kind=type(primary).__name__):
                returned, caught, _stderr = self.api_interruption(primary)
                self.assertIsNone(returned)
                self.assertIs(caught, primary)

    def test_run_fixed_locked_api_retains_cancellation_and_delegated_custody(self):
        for primary in self.cancellations():
            with self.subTest(code=getattr(primary, 'code', None), kind=type(primary).__name__):
                args = self.admission()
                with mock.patch.object(UPDATE, 'fixed_source_args', return_value=args), \
                        mock.patch.object(UPDATE, 'run_locked', side_effect=primary) as operation:
                    with self.assertRaises(BaseException) as caught:
                        UPDATE.run_fixed_locked(self.arguments())
                self.assertIs(caught.exception, primary)
                operation.assert_called_once_with(self.arguments(), source=args.fixed_source,
                    expected_checkout=args.expected_checkout, expected_prefix=args.expected_prefix,
                    initial_details=dict(initial_selected_source='b' * 40,
                                         initial_selected_version='0.1.0'),
                    guardian_contract=UPDATE.GUARDIAN_CONTRACT)

    def test_main_refusal_does_not_complete_an_interrupted_api_journal(self):
        for code in (0, None, False, 256, -256):
            with self.subTest(code=code):
                returned, caught, stderr = self.api_interruption(SystemExit(code), through_main=True)
                self.assertIsNone(caught)
                self.assertEqual(returned, 1)
                self.assertIn('operation incomplete', stderr)


if __name__ == '__main__':
    unittest.main()

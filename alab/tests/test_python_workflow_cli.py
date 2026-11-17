# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Failure-only CLI controls never substitute or execute native prerequisites."""

import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / 'python_tests.py'
SPEC = importlib.util.spec_from_file_location('python_workflow_cli', SCRIPT)
WORKFLOW = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(WORKFLOW)
NATIVE_CLASS = 'test_unit_runner_native.NativeRunnerTests'
REQUIRED_NATIVE = [NATIVE_CLASS + suffix for suffix in (
    '.test_existing_matching_cfg_runner_is_preserved_and_conflicts',
    '.test_existing_triple_runner_wins_and_missing_receipts_refuse',
    '.test_native_entry_preserves_real_cargo_context_and_detects_python_startup')]


CHILD = r'''
import ast
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys

path, root = Path(sys.argv[1]), Path(sys.argv[2])
config = json.loads((root / 'config.json').read_text())
spec = importlib.util.spec_from_file_location('controlled_python_workflow', path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
# These are the discovery-location seams, not a replacement discovery or
# native class. Every source entry still requires its real native prerequisites.
module.TESTS = root / 'suite'
module.SMOKE = root / 'smoke'
faults = {'zero': SystemExit(0), 'wrapped': SystemExit(256), 'nonzero': SystemExit(47),
          'keyboard': KeyboardInterrupt('controlled observation cancellation'),
          'oserror': OSError('private observation failure'),
          'runtime': RuntimeError('private observation failure')}
out_fault = faults.get(config.get('stdout_fault'))
err_fault = faults.get(config.get('stderr_fault'))
witness = dict(stdout_sink_installed=out_fault is not None, stderr_sink_installed=err_fault is not None,
               manifest_attempted=False, summary_attempted=False, diagnostic_attempts=0)

class OutputSink(io.StringIO):
    def write(self, text):
        if text.startswith(module.MANIFEST_PREFIX):
            witness['manifest_attempted'] = True
        if text.startswith(module.SUMMARY_PREFIX):
            witness['summary_attempted'] = True
        target = {'manifest': module.MANIFEST_PREFIX, 'summary': module.SUMMARY_PREFIX}[config['stdout_at']]
        if text.startswith(target):
            raise out_fault
        return super().write(text)

class DiagnosticSink(io.StringIO):
    def write(self, _text):
        witness['diagnostic_attempts'] += 1
        raise err_fault

output = OutputSink() if out_fault is not None else None
if output is not None:
    sys.stdout = output
if err_fault is not None:
    sys.stderr = DiagnosticSink()
sys.argv = [str(path), '--unsupported'] if config.get('arguments') else [str(path)]
tree = ast.parse(path.read_bytes(), filename=str(path))
entries = [node for node in tree.body if isinstance(node, ast.If)
           and isinstance(node.test, ast.Compare) and isinstance(node.test.left, ast.Name)
           and node.test.left.id == '__name__']
assert len(entries) == 1
program = ast.Module(body=entries, type_ignores=[])
witness['entry_ast_sha256'] = hashlib.sha256(ast.dump(program).encode()).hexdigest()
module.__name__ = '__main__'
try:
    exec(compile(program, str(path), 'exec'), module.__dict__)
except BaseException as error:
    owned = sys.modules.get('test_owned')
    witness.update(escaped_type=type(error).__name__, escaped_code=getattr(error, 'code', None),
                   escaped_hook_identity=error is getattr(owned, 'CONTROL', None),
                   escaped_stdout_identity=error is out_fault, escaped_stderr_identity=error is err_fault)
    raise
finally:
    for name in ('imported', 'hook', 'body'):
        witness[name] = (root / name).read_text() if (root / name).exists() else None
    witness['native_class'] = module.NATIVE_CLASS
    witness['required_native_ids'] = list(module.REQUIRED_NATIVE)
    witness['native_registered'] = module._native_class() is not None
    witness['captured_stdout'] = output.getvalue() if output is not None else None
    witness['python_version'] = sys.version
    (root / 'witness.json').write_text(json.dumps(witness))
'''


class PythonWorkflowCliTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory(prefix='alab-python-workflow-cli-')
        self.addCleanup(scratch.cleanup)
        self.directory = Path(scratch.name)
        self.child_observations = []

    def child(self, name, *, hook='healthy', arguments=False, stdout_at=None,
              stdout_fault=None, stderr_fault=None):
        root = self.directory / name
        root.mkdir()
        suite = root / 'suite'
        suite.mkdir()
        controls = {'zero': 'SystemExit(0)', 'none': 'SystemExit(None)', 'false': 'SystemExit(False)',
                    'wrapped': 'SystemExit(256)', 'negative_wrapped': 'SystemExit(-256)',
                    'nonzero': 'SystemExit(47)', 'keyboard': 'KeyboardInterrupt("owned hook cancellation")',
                    'floating_zero': 'SystemExit(0.0)', 'huge': 'SystemExit(__import__("sys").maxsize + 1)',
                    'ordinary': 'ValueError("private owned hook failure")'}
        lines = ['import unittest', 'from pathlib import Path',
                 f'Path({str(root / "imported")!r}).write_text("1")',
                 'class Owned(unittest.TestCase):', ' def test_one(self):',
                 f'  Path({str(root / "body")!r}).write_text("1")']
        if hook in controls:
            lines.append('CONTROL = ' + controls[hook])
        lines += ['def load_tests(loader, tests, pattern):',
                  f' Path({str(root / "hook")!r}).write_text("1")',
                  ' raise CONTROL' if hook in controls else ' return tests']
        (suite / 'test_owned.py').write_text('\n'.join(lines) + '\n')
        smoke = root / 'smoke'
        smoke.mkdir()
        for filename in WORKFLOW.SMOKE_TEST_FILES:
            (smoke / filename).write_text(
                'import unittest\nfrom pathlib import Path\n'
                'class Smoke(unittest.TestCase):\n def test_fixture(self):\n'
                f'  Path({str(root / "body")!r}).write_text("smoke")\n')
        config = dict(hook=hook, arguments=arguments, stdout_at=stdout_at,
                      stdout_fault=stdout_fault, stderr_fault=stderr_fault)
        (root / 'config.json').write_text(json.dumps(config))
        child = root / 'child.py'
        child.write_text(CHILD)
        done = subprocess.run([sys.executable, '-I', '-B', str(child), str(SCRIPT), str(root)],
                              cwd=root, capture_output=True, text=True, timeout=10)
        witness = json.loads((root / 'witness.json').read_text())
        source_stdout = witness['captured_stdout'] if witness['stdout_sink_installed'] else done.stdout
        manifests, summaries = [], []
        for line in source_stdout.splitlines():
            if line.startswith(WORKFLOW.MANIFEST_PREFIX):
                manifests.append(json.loads(line[len(WORKFLOW.MANIFEST_PREFIX):]))
            elif line.startswith(WORKFLOW.SUMMARY_PREFIX):
                summaries.append(json.loads(line[len(WORKFLOW.SUMMARY_PREFIX):]))
        self.child_observations.append(dict(case=name, config=config, actual_exit=done.returncode,
            witness=witness, manifests=manifests, summaries=summaries,
            source_stdout=source_stdout, stdout=done.stdout, stderr=done.stderr))
        self.assertEqual(witness['native_class'], NATIVE_CLASS)
        self.assertEqual(witness['required_native_ids'], REQUIRED_NATIVE)
        self.assertFalse(witness['native_registered'])
        self.assertIsNone(witness['body'])
        for manifest in manifests:
            self.assertEqual(manifest['native_ids'], [])
            self.assertEqual(manifest['required_native_ids'], REQUIRED_NATIVE)
        for summary in summaries:
            self.assertFalse(summary['success'])
            self.assertFalse(summary['full_scope'])
            self.assertEqual(summary['tests_run'], 0)
            self.assertEqual(summary['passed'], 0)
            self.assertFalse(summary['phases']['native']['executed'])
            self.assertFalse(summary['phases']['pure']['executed'])
        self.assertNotIn('private owned hook failure', source_stdout)
        return done, witness, manifests, summaries

    def complete_failure(self, manifests, summaries):
        self.assertEqual(len(manifests), 1)
        self.assertEqual(len(summaries), 1)
        self.assertEqual(summaries[0]['manifest_digest'], manifests[0]['digest'])

    def test_natural_success_shaped_hook_exits_emit_failed_discovery_observations(self):
        for hook in ('zero', 'none', 'false', 'wrapped', 'negative_wrapped'):
            with self.subTest(hook=hook):
                done, witness, manifests, summaries = self.child(hook, hook=hook)
                self.assertEqual(done.returncode, 1)
                self.assertEqual(witness['imported'], '1')
                self.assertEqual(witness['hook'], '1')
                self.complete_failure(manifests, summaries)
                self.assertFalse(manifests[0]['discovery_complete'])
                self.assertEqual(summaries[0]['discovery_error_count'], 1)
                self.assertIn('discovery_errors', summaries[0]['preflight_errors'])
                self.assertIn('discovery incomplete', done.stderr)

    def test_healthy_and_ordinary_hooks_still_refuse_missing_native_before_bodies(self):
        for hook in ('healthy', 'ordinary'):
            with self.subTest(hook=hook):
                done, witness, manifests, summaries = self.child(hook, hook=hook)
                self.assertEqual(done.returncode, 1)
                self.assertEqual(witness['imported'], '1')
                self.assertEqual(witness['hook'], '1')
                self.complete_failure(manifests, summaries)
                self.assertIn('missing_required_native_cases', summaries[0]['preflight_errors'])
                self.assertEqual(summaries[0]['discovery_error_count'], int(hook == 'ordinary'))
                self.assertEqual(manifests[0]['discovery_complete'], hook == 'healthy')

    def test_nonzero_float_overflow_and_keyboard_hook_identity_is_preserved(self):
        for hook, expected in (('nonzero', 47), ('keyboard', -2),
                               ('floating_zero', 1), ('huge', 255)):
            with self.subTest(hook=hook):
                done, witness, manifests, summaries = self.child(hook, hook=hook)
                self.assertEqual(done.returncode, expected)
                self.assertTrue(witness['escaped_hook_identity'])
                self.assertEqual(manifests + summaries, [])

    def test_arguments_refuse_without_discovery_or_execution(self):
        done, witness, manifests, summaries = self.child('arguments', arguments=True)
        self.assertEqual(done.returncode, 1)
        self.assertIsNone(witness['imported'])
        self.assertIsNone(witness['hook'])
        self.complete_failure(manifests, summaries)
        self.assertIn('unsupported_arguments', summaries[0]['preflight_errors'])
        self.assertIn('takes no arguments', done.stderr)

    def test_diagnostic_zero_exits_cannot_abort_failure_reporting_with_success(self):
        for name, options in (
                ('diag_arguments', dict(arguments=True)),
                ('diag_discovery', dict(hook='zero')),
                ('diag_loader', dict(hook='ordinary')),
                ('diag_workflow', dict(stdout_at='manifest', stdout_fault='zero'))):
            with self.subTest(case=name):
                done, witness, manifests, summaries = self.child(name, stderr_fault='wrapped', **options)
                self.assertEqual(done.returncode, 1)
                self.assertGreater(witness['diagnostic_attempts'], 0)
                if name != 'diag_workflow':
                    self.complete_failure(manifests, summaries)

    def test_discovery_and_argument_fallback_output_interruptions_are_failures(self):
        for fallback in ('discovery', 'arguments'):
            for phase in ('manifest', 'summary'):
                name = fallback + '_' + phase
                with self.subTest(case=name):
                    done, witness, manifests, summaries = self.child(name,
                        hook='zero' if fallback == 'discovery' else 'healthy',
                        arguments=fallback == 'arguments', stdout_at=phase, stdout_fault='wrapped')
                    self.assertEqual(done.returncode, 1)
                    self.assertTrue(witness[phase + '_attempted'])
                    self.assertEqual(len(manifests), int(phase == 'summary'))
                    self.assertEqual(summaries, [])

    def test_real_missing_native_workflow_output_interruptions_are_failures(self):
        for phase in ('manifest', 'summary'):
            with self.subTest(phase=phase):
                done, witness, manifests, summaries = self.child('workflow_' + phase,
                    stdout_at=phase, stdout_fault='wrapped')
                self.assertEqual(done.returncode, 1)
                self.assertEqual(witness['hook'], '1')
                self.assertTrue(witness[phase + '_attempted'])
                self.assertEqual(len(manifests), int(phase == 'summary'))
                self.assertEqual(summaries, [])

    def test_nonzero_output_and_diagnostic_interruptions_keep_identity(self):
        cases = (
            ('out_arguments', 47, 'escaped_stdout_identity', dict(arguments=True,
                stdout_at='manifest', stdout_fault='nonzero')),
            ('out_discovery', -2, 'escaped_stdout_identity', dict(hook='zero',
                stdout_at='summary', stdout_fault='keyboard')),
            ('diag_arguments', 47, 'escaped_stderr_identity', dict(arguments=True,
                stderr_fault='nonzero')),
            ('diag_discovery', -2, 'escaped_stderr_identity', dict(hook='zero',
                stderr_fault='keyboard')),
        )
        for name, expected, field, options in cases:
            with self.subTest(case=name):
                done, witness, _manifests, _summaries = self.child(name, **options)
                self.assertEqual(done.returncode, expected)
                self.assertTrue(witness[field])

    def test_ordinary_observation_failures_cannot_qualify_the_workflow(self):
        cases = (
            ('diag_arguments', dict(arguments=True, stderr_fault='oserror')),
            ('diag_loader', dict(hook='ordinary', stderr_fault='runtime')),
            ('out_arguments', dict(arguments=True, stdout_at='manifest', stdout_fault='oserror')),
            ('out_workflow', dict(stdout_at='summary', stdout_fault='runtime')),
        )
        for name, options in cases:
            with self.subTest(case=name):
                done, _witness, manifests, summaries = self.child(name, **options)
                self.assertEqual(done.returncode, 1)
                if name.startswith('diag_'):
                    self.complete_failure(manifests, summaries)

    def test_run_cases_api_preserves_output_cancellation_objects(self):
        for phase in ('manifest', 'summary'):
            for primary in (SystemExit(0), SystemExit(256), SystemExit(47),
                            KeyboardInterrupt('controlled API output cancellation')):
                with self.subTest(phase=phase, code=getattr(primary, 'code', None)):
                    class InterruptedOutput(io.StringIO):
                        def write(self, text):
                            prefix = (WORKFLOW.MANIFEST_PREFIX if phase == 'manifest'
                                      else WORKFLOW.SUMMARY_PREFIX)
                            if text.startswith(prefix):
                                raise primary
                            return super().write(text)

                    with self.assertRaises(BaseException) as caught:
                        WORKFLOW._run_cases((), stdout=InterruptedOutput(), stderr=io.StringIO())
                    self.assertIs(caught.exception, primary)


if __name__ == '__main__':
    unittest.main()

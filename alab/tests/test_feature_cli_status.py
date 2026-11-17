# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Original feature, review and status CLIs refuse interrupted-operation success."""

import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ALAB = Path(__file__).resolve().parents[1]
CONSUMERS = ('feature_evidence', 'feature_gate', 'upstream_gate', 'upstream_status')
ZERO_CASES = ('zero', 'none', 'false', 'wrapped', 'negative_wrapped')
CONTROL_CASES = (('nonzero', 47), ('keyboard', -2), ('floating_zero', 1), ('huge', 255))


CHILD = r'''
import ast
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import threading

path, consumer, case, invocation, witness_path = (
    Path(sys.argv[1]), sys.argv[2], sys.argv[3], sys.argv[4], Path(sys.argv[5]))
spec = importlib.util.spec_from_file_location('controlled_feature_cli', path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
source_bytes = path.read_bytes()
witness = {'source_sha256': hashlib.sha256(source_bytes).hexdigest(),
           'operation_calls': 0, 'operation_returned': False, 'owner_returned': False,
           'original_entry_executed': invocation == 'entry', 'actor_calls': {},
           'stdout_faults': 0, 'stderr_faults': 0, 'raw_main_returned': False,
           'assessment_calls': 0, 'assessment_returned': False, 'controlled_git_calls': [],
           'classification_callbacks': 0}


def forbidden(name):
    witness['actor_calls'][name] = 0
    def call(*_args, **_keywords):
        witness['actor_calls'][name] += 1
        raise AssertionError('forbidden actor ' + name)
    return call


for owner, names in ((subprocess, ('Popen', 'run', 'call', 'check_call', 'check_output')),
                     (os, ('fork', 'kill', 'killpg', 'system')),
                     (socket, ('socket',)), (threading, ('Thread',))):
    for name in names:
        if hasattr(owner, name):
            setattr(owner, name, forbidden(name))

source = 'a' * 40
expected = {'schema': 1, 'source': source, 'upstream': source,
            'inventory_sha256': 'b' * 64, 'features': [{'id': 'controlled',
            'scenarios': [{'description': 'Controlled CLI operation.', 'checks': []}]}]}
log = witness_path.with_suffix('.log')
raw = (b'running 1 test\ntest controlled::case ... ok\n'
       b'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n')
log.write_bytes(raw)
record = copy.deepcopy(expected)
record['features'][0]['scenarios'][0]['checks'] = [{
    'source': source, 'command': ['never-executed-fixture'],
    'status': 1 if case == 'returned_failure' else 0,
    'log': str(log), 'log_sha256': hashlib.sha256(raw).hexdigest(),
    'tests': ['controlled::case'], 'reason': 'Synthetic CLI fixture; no test command ran.'}]
record_path = witness_path.with_suffix('.record.json')
record_path.write_text(json.dumps(record) + '\n')
record_sha = hashlib.sha256(record_path.read_bytes()).hexdigest()
pending = witness_path.with_suffix('.pending.json')
destination = witness_path.with_suffix('.bundle')
faults = {'zero': SystemExit(0), 'none': SystemExit(None), 'false': SystemExit(False),
          'wrapped': SystemExit(256), 'negative_wrapped': SystemExit(-256),
          'nonzero': SystemExit(47), 'floating_zero': SystemExit(0.0),
          'huge': SystemExit(sys.maxsize + 1),
          'keyboard': KeyboardInterrupt('controlled feature cancellation'),
          'ordinary': OSError(5, 'controlled feature operation failure'),
          'runtime': RuntimeError('controlled feature operation failure')}
class HostileExit(SystemExit):
    def __getattribute__(self, name):
        if name in ('code', '__notes__', 'add_note'):
            witness['classification_callbacks'] += 1
            raise SystemExit(0)
        return super().__getattribute__(name)
    def __str__(self):
        witness['classification_callbacks'] += 1
        raise SystemExit(0)

class HostilePayload:
    def __getattribute__(self, name):
        if name == '__class__':
            witness['classification_callbacks'] += 1
            raise SystemExit(0)
        return super().__getattribute__(name)

class TransparentExit(SystemExit):
    pass

class HostileInteger(int):
    def __getattribute__(self, name):
        if name == '__class__':
            witness['classification_callbacks'] += 1
            raise SystemExit(0)
        return super().__getattribute__(name)
    def __index__(self):
        witness['classification_callbacks'] += 1
        raise SystemExit(0)
    def __and__(self, other):
        witness['classification_callbacks'] += 1
        raise SystemExit(0)
    def __eq__(self, other):
        witness['classification_callbacks'] += 1
        raise SystemExit(0)

faults.update(hostile_exit=HostileExit(47), hostile_payload=SystemExit(HostilePayload()),
              transparent_exit=TransparentExit(47), int_wrapped=SystemExit(HostileInteger(256)))

primary, report_fault = None, None
status_assessment = consumer == 'upstream_status' and case.startswith('status_assessment_')
if case.startswith('operation_'):
    primary = faults[case.removeprefix('operation_')]
elif case.startswith(('stderr_', 'status_assessment_stderr_')):
    primary = faults['ordinary']
    report_fault = faults[case.split('stderr_', 1)[1]]
elif case.startswith(('format_', 'status_assessment_format_')):
    report_fault = faults[case.split('format_', 1)[1]]
    class FormatFailure(OSError):
        def __str__(self):
            raise report_fault
    primary = FormatFailure('controlled feature formatting failure')
elif case.startswith('refused_stdout_'):
    report_fault = faults[case.removeprefix('refused_stdout_')]
elif case.startswith('missing_stdout_'):
    report_fault = faults[case.removeprefix('missing_stdout_')]
elif case.startswith('stdout_'):
    report_fault = faults[case.removeprefix('stdout_')]
elif case.startswith('status_assessment_stdout_'):
    report_fault = faults[case.removeprefix('status_assessment_stdout_')]
elif status_assessment and case.removeprefix('status_assessment_') in faults:
    primary = faults[case.removeprefix('status_assessment_')]

result = {'schema': 1, 'kind': 'declared-feature-evidence-admission',
          'source': source, 'base': 'b' * 40, 'upstream': 'c' * 40,
          'affected_features': [], 'bundle': None,
          'errors': ['controlled declared refusal'] if case == 'returned_failure' or
              case.startswith('refused_stdout_') else [],
          'limits': 'synthetic admission boundary; no native evidence admitted'}
if consumer == 'upstream_gate':
    missing = ['c' * 40] if case == 'missing_failure' or case.startswith('missing_stdout_') else []
    result = {'schema': 1, 'kind': 'committed-upstream-review-admission',
              'base': 'b' * 40, 'source': source, 'integrated_upstream': 'c' * 40,
              'commits': 1, 'reviewed': 0 if result['errors'] or missing else 1,
              'missing': missing, 'errors': result['errors'], 'affected_features': [],
              'limits': 'synthetic admission boundary; no review/native evidence admitted'}
elif consumer == 'upstream_status':
    result = {'schema': 1, 'observed_at': '2026-11-16T00:00:00+00:00',
              'main_ref': 'origin/main', 'upstream_ref': 'upstream/main',
              'main_sha': source, 'upstream_sha': 'b' * 40, 'integrated_frontier_sha': source,
              'pending_count': 3, 'oldest_pending_committed_at': None,
              'oldest_pending_age_seconds': None, 'last_successful_fetch_at': None,
              'qualified_sha': None, 'packaged_sha': None, 'selected_sha': None,
              'running_sha': None, 'errors': result['errors'],
              'unknown_fields_reason': 'synthetic CLI observation; no Git/native evidence read'}


def operation(*arguments, **keywords):
    witness['operation_calls'] += 1
    if primary is not None and not status_assessment:
        raise primary
    witness['operation_returned'] = True
    witness['result_errors'] = result['errors']
    witness['result_missing'] = result.get('missing', [])
    return copy.deepcopy(expected if consumer == 'feature_evidence' else result)


if consumer == 'feature_evidence':
    module.snapshot = operation
elif consumer == 'upstream_status':
    module.report = operation

    def assessment(*arguments, **keywords):
        witness['assessment_calls'] += 1
        if primary is not None:
            raise primary
        witness['assessment_returned'] = True
        errors = (['controlled assessment error'] if case == 'status_assessment_errors' or
                  case.startswith('status_assessment_stdout_') else [])
        witness['assessment_errors'] = errors
        return {'schema': 1, 'kind': 'local-source-and-intake-observation',
                'base': source, 'upstream': 'b' * 40, 'candidate': 'c' * 40,
                'origin_tracking_main': source, 'commits': 3, 'reviewed': 0,
                'pending': 3, 'missing': ['d' * 40], 'intake_complete': False,
                'candidate_contains_target': False, 'origin_tracking_contains_target': False,
                'qualification': 'unproven', 'installed_adoption': 'unproven',
                'limits': 'synthetic observation; no actual intake inspected', 'errors': errors}

    def local_git(repo, *arguments):
        witness['controlled_git_calls'].append(list(arguments))
        if arguments == ('rev-parse', '--verify', '--end-of-options', 'HEAD^{commit}'):
            return 'c' * 40
        if arguments == ('show', 'c' * 40 + ':alab/features.toml'):
            return 'schema = 1\nfeatures = []\n'
        raise AssertionError('unmodeled Git boundary')

    module.observe, module.git = assessment, local_git
    for name in ('artifact_observation', 'qualification_observation', 'snapshot', 'validate', 'contains'):
        setattr(module, name, forbidden(consumer + '.' + name))
else:
    module.admission = operation
    names = (('git', 'review_admission', 'snapshot', 'validate', 'read_record')
             if consumer == 'feature_gate' else ('git', 'snapshot', 'validate', 'catalog_feature_ids'))
    for name in names:
        setattr(module, name, forbidden(consumer + '.' + name))

original_stdout, original_stderr = sys.stdout, sys.stderr
class FailingSink:
    def __init__(self, counter):
        self.counter = counter
    def write(self, _text):
        witness[self.counter] += 1
        raise report_fault
    def flush(self):
        pass


if case.startswith(('stderr_', 'status_assessment_stderr_')):
    sys.stderr = FailingSink('stderr_faults')
if case.startswith(('stdout_', 'refused_stdout_', 'missing_stdout_', 'status_assessment_stdout_')):
    sys.stdout = FailingSink('stdout_faults')
if consumer == 'feature_evidence':
    operation_name = 'snapshot' if case in ('snapshot_success', 'semantic_log_root') else (
        'bundle' if case in ('bundle_success', 'semantic_missing_destination') else 'validate')
    arguments = [str(path), operation_name, '--source', source, '--upstream', source,
                 '--record', str(pending if case == 'snapshot_success' else record_path)]
    if case == 'bundle_success':
        arguments += ['--destination', str(destination)]
    elif case == 'semantic_log_root':
        arguments += ['--log-root', str(witness_path.parent)]
    elif case == 'semantic_destination':
        arguments += ['--destination', str(destination)]
elif consumer == 'upstream_status':
    arguments = [str(path)]
    if status_assessment:
        arguments += ['--base', source]
    elif case == 'status_intake_without_base':
        arguments += ['--intake', 'not-read.json']
else:
    arguments = [str(path), '--base', 'b' * 40]
sys.argv = [str(path), '--help'] if case == 'help' else [str(path)] if case == 'parser_error' else arguments
if consumer == 'upstream_status' and case == 'parser_error':
    sys.argv += ['--running-pid', 'not-an-integer']
tree = ast.parse(source_bytes, filename=str(path))
entries = [node for node in tree.body if isinstance(node, ast.If)
           and isinstance(node.test, ast.Compare) and isinstance(node.test.left, ast.Name)
           and node.test.left.id == '__name__']
assert len(entries) == 1
entry = ast.Module(body=entries, type_ignores=[])
witness['original_entry_guards'] = len(entries)
witness['entry_ast_sha256'] = hashlib.sha256(ast.dump(entry).encode()).hexdigest()
witness['python_version'] = sys.version
owner = getattr(module, '_run', module.main)
witness['operation_owner'] = owner.__name__


def observe(frame, event, argument):
    if frame.f_code is owner.__code__ and event == 'return':
        witness['owner_returned'] = type(argument) is int
        if type(argument) is int:
            witness['owner_return_value'] = argument
    return observe


module.__name__ = '__main__'
sys.settrace(observe)
try:
    if invocation == 'entry':
        exec(compile(entry, str(path), 'exec'), module.__dict__)
    else:
        status = module.main()
        witness['raw_main_returned'] = True
        witness['raw_main_status'] = status
        sys.exit(status)
except BaseException as error:
    witness['escaped_type'] = type(error).__name__
    error_type = type(error)
    code = SystemExit.code.__get__(error, error_type) if issubclass(error_type, SystemExit) else None
    # Never ask a foreign exception getter, payload __class__, repr or JSON encoder.
    witness['escaped_code'] = (code if type(code) in (type(None), bool, int, float, str) else
                               int.__index__(code) if issubclass(type(code), int) else '<opaque>')
    witness['escaped_primary_identity'] = error is primary
    witness['escaped_report_identity'] = error is report_fault
    raise
finally:
    sys.settrace(None)
    sys.stdout, sys.stderr = original_stdout, original_stderr
    witness['pending_output_exists'] = pending.exists()
    witness['bundle_output_exists'] = destination.exists()
    witness['input_record_unchanged'] = hashlib.sha256(record_path.read_bytes()).hexdigest() == record_sha
    witness['source_unchanged'] = path.read_bytes() == source_bytes
    witness_path.write_text(json.dumps(witness, indent=2) + '\n')
'''


class FeatureCliStatusTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory(prefix='alab-feature-cli-status-')
        self.addCleanup(scratch.cleanup)
        self.directory = Path(scratch.name)
        self.script = self.directory / 'child.py'
        self.script.write_text(CHILD)
        self.observations = []

    def child(self, consumer, case, invocation='entry'):
        source = ALAB / (consumer + '.py')
        witness_path = self.directory / (f'{len(self.observations)}-{consumer}-{case}-{invocation}.json')
        done = subprocess.run([
            sys.executable, '-I', '-B', str(self.script), str(source), consumer,
            case, invocation, str(witness_path),
        ], cwd=self.directory, capture_output=True, text=True, timeout=10)
        self.assertTrue(witness_path.is_file(), done.stderr)
        witness = json.loads(witness_path.read_text())
        self.observations.append(dict(consumer=consumer, case=case, invocation=invocation,
                                      actual_exit=done.returncode, witness=witness))
        self.assertEqual(witness['original_entry_guards'], 1)
        self.assertEqual(len(witness['entry_ast_sha256']), 64)
        self.assertEqual(witness['source_sha256'], hashlib.sha256(source.read_bytes()).hexdigest())
        self.assertTrue(witness['source_unchanged'])
        self.assertEqual(witness['original_entry_executed'], invocation == 'entry')
        self.assertTrue(witness['input_record_unchanged'])
        self.assertEqual(witness['pending_output_exists'], case == 'snapshot_success')
        self.assertEqual(witness['bundle_output_exists'], case == 'bundle_success')
        self.assertTrue(all(count == 0 for count in witness['actor_calls'].values()), witness)
        parsing = (case in ('help', 'parser_error', 'status_intake_without_base') or
                   case.startswith('semantic_'))
        self.assertEqual(witness['operation_calls'], 0 if parsing else 1)
        self.assertEqual(len(witness['controlled_git_calls']), 2 if
                         consumer == 'upstream_status' and case.startswith('status_assessment_') else 0)
        return done, witness

    def test_success_shaped_operation_exits_refuse_incomplete_original_entries(self):
        for consumer in CONSUMERS:
            for case in ZERO_CASES:
                with self.subTest(consumer=consumer, case=case):
                    done, witness = self.child(consumer, 'operation_' + case)
                    self.assertEqual(done.returncode, 1)
                    self.assertFalse(witness['operation_returned'])
                    self.assertFalse(witness['owner_returned'])
                    self.assertFalse(witness['escaped_primary_identity'])
                    self.assertIn('operation incomplete: requested a successful exit', done.stderr)

    def test_success_shaped_diagnostic_and_formatting_exits_refuse_success(self):
        for consumer in CONSUMERS:
            for phase in ('stderr_', 'format_'):
                for case in ZERO_CASES:
                    with self.subTest(consumer=consumer, phase=phase, case=case):
                        done, witness = self.child(consumer, phase + case)
                        self.assertEqual(done.returncode, 1)
                        self.assertFalse(witness['operation_returned'])
                        self.assertFalse(witness['owner_returned'])
                        self.assertFalse(witness['escaped_report_identity'])
                        if phase == 'stderr_':
                            self.assertGreater(witness['stderr_faults'], 0)

    def test_success_shaped_output_exits_refuse_success_after_returned_results(self):
        for consumer in CONSUMERS:
            for case in ZERO_CASES:
                with self.subTest(consumer=consumer, case=case):
                    done, witness = self.child(consumer, 'stdout_' + case)
                    self.assertEqual(done.returncode, 1)
                    self.assertTrue(witness['operation_returned'])
                    self.assertFalse(witness['owner_returned'])
                    self.assertEqual(witness['stdout_faults'], 1)
                    self.assertFalse(witness['escaped_report_identity'])
                    self.assertEqual(done.stdout, '')

    def test_returned_gate_refusal_cannot_exit_zero_when_json_output_is_interrupted(self):
        for consumer in ('feature_gate', 'upstream_gate'):
            for case in ZERO_CASES:
                with self.subTest(consumer=consumer, case=case):
                    done, witness = self.child(consumer, 'refused_stdout_' + case)
                    self.assertEqual(done.returncode, 1)
                    self.assertTrue(witness['operation_returned'])
                    self.assertEqual(witness['result_errors'], ['controlled declared refusal'])
                    self.assertEqual(witness['result_missing'], [])
                    self.assertFalse(witness['owner_returned'])
                    self.assertEqual(witness['stdout_faults'], 1)
                    self.assertEqual(done.stdout, '')

    def test_upstream_missing_only_json_output_faults_refuse_success(self):
        for case in ZERO_CASES:
            with self.subTest(case=case):
                done, witness = self.child('upstream_gate', 'missing_stdout_' + case)
                self.assertEqual(done.returncode, 1)
                self.assertTrue(witness['operation_returned'])
                self.assertEqual(witness['result_errors'], [])
                self.assertEqual(witness['result_missing'], ['c' * 40])
                self.assertFalse(witness['owner_returned'])
                self.assertEqual(witness['stdout_faults'], 1)
                self.assertEqual(done.stdout, '')

    def test_upstream_missing_only_refusal_keeps_json_and_status(self):
        for invocation in ('entry', 'main'):
            with self.subTest(invocation=invocation):
                done, witness = self.child('upstream_gate', 'missing_failure', invocation)
                self.assertEqual(done.returncode, 1)
                self.assertTrue(witness['operation_returned'])
                self.assertTrue(witness['owner_returned'])
                self.assertEqual(witness['owner_return_value'], 1)
                self.assertEqual(witness['raw_main_returned'], invocation == 'main')
                output = json.loads(done.stdout)
                self.assertEqual(output['errors'], [])
                self.assertEqual(output['missing'], ['c' * 40])
                self.assertEqual(done.stderr, '')

    def test_status_completed_assessment_and_failures_retain_source_json(self):
        for invocation in ('entry', 'main'):
            for case, status in (('pending', 0), ('errors', 1), ('ordinary', 1)):
                with self.subTest(invocation=invocation, case=case):
                    done, witness = self.child('upstream_status', 'status_assessment_' + case, invocation)
                    self.assertEqual(done.returncode, status)
                    self.assertTrue(witness['operation_returned'])
                    self.assertTrue(witness['owner_returned'])
                    self.assertEqual(witness['owner_return_value'], status)
                    self.assertEqual(witness['raw_main_returned'], invocation == 'main')
                    self.assertEqual(witness['assessment_calls'], 1)
                    self.assertEqual(witness['assessment_returned'], case != 'ordinary')
                    output = json.loads(done.stdout)
                    self.assertEqual(output['main_sha'], 'a' * 40)
                    self.assertEqual(output['upstream_sha'], 'b' * 40)
                    self.assertEqual(output['integrated_frontier_sha'], 'a' * 40)
                    self.assertEqual(output['pending_count'], 3)
                    if case == 'ordinary':
                        self.assertNotIn('assessment', output)
                        self.assertEqual(output['errors'], [
                            'assessment: [Errno 5] controlled feature operation failure'])
                        self.assertEqual(done.stderr, 'upstream status: '
                                         '[Errno 5] controlled feature operation failure\n')
                    else:
                        self.assertEqual(output['errors'], [])
                        self.assertFalse(output['assessment']['intake_complete'])
                        self.assertTrue(output['assessment']['missing'])
                        self.assertEqual(output['assessment']['errors'], [] if case == 'pending'
                                         else ['controlled assessment error'])
                        self.assertEqual(done.stderr, '')

    def test_status_intake_without_base_is_handled_failure_before_report(self):
        for invocation in ('entry', 'main'):
            with self.subTest(invocation=invocation):
                done, witness = self.child('upstream_status', 'status_intake_without_base', invocation)
                self.assertEqual(done.returncode, 1)
                self.assertFalse(witness['operation_returned'])
                self.assertTrue(witness['owner_returned'])
                self.assertEqual(witness['owner_return_value'], 1)
                self.assertEqual(witness['raw_main_returned'], invocation == 'main')
                self.assertEqual(done.stdout, '')
                self.assertEqual(done.stderr, 'upstream status: --intake requires an explicit --base\n')

    def test_status_assessment_controls_preserve_raw_api_and_cli_failure(self):
        cases = tuple((name, 0) for name in ZERO_CASES) + CONTROL_CASES + (('runtime', 1),)
        for invocation in ('entry', 'main'):
            for case, status in cases:
                with self.subTest(invocation=invocation, case=case):
                    done, witness = self.child('upstream_status', 'status_assessment_' + case, invocation)
                    self.assertEqual(done.returncode, 1 if invocation == 'entry' and case in ZERO_CASES
                                     else status)
                    self.assertTrue(witness['operation_returned'])
                    self.assertEqual(witness['assessment_calls'], 1)
                    self.assertFalse(witness['assessment_returned'])
                    self.assertFalse(witness['owner_returned'])
                    self.assertFalse(witness['raw_main_returned'])
                    self.assertEqual(done.stdout, '')
                    if invocation == 'main' or case in dict(CONTROL_CASES):
                        self.assertTrue(witness['escaped_primary_identity'])

    def test_status_assessment_diagnostic_faults_preserve_failure_and_raw_identity(self):
        for phase in ('stderr_', 'format_'):
            cases = [(case, 'entry', 1) for case in ZERO_CASES]
            cases += [('wrapped', 'main', 0), ('nonzero', 'entry', 47), ('keyboard', 'entry', -2)]
            for case, invocation, status in cases:
                with self.subTest(phase=phase, case=case, invocation=invocation):
                    done, witness = self.child('upstream_status',
                                               'status_assessment_' + phase + case, invocation)
                    self.assertEqual(done.returncode, status)
                    self.assertTrue(witness['operation_returned'])
                    self.assertEqual(witness['assessment_calls'], 1)
                    self.assertFalse(witness['assessment_returned'])
                    self.assertFalse(witness['owner_returned'])
                    self.assertFalse(witness['raw_main_returned'])
                    self.assertEqual(done.stdout, '')
                    if invocation == 'main' or case in ('nonzero', 'keyboard'):
                        self.assertTrue(witness['escaped_report_identity'])
                    if phase == 'stderr_':
                        self.assertGreater(witness['stderr_faults'], 0)

    def test_status_returned_source_and_assessment_errors_refuse_interrupted_output(self):
        for phase in ('refused_stdout_', 'status_assessment_stdout_'):
            for case in ZERO_CASES:
                with self.subTest(phase=phase, case=case):
                    done, witness = self.child('upstream_status', phase + case)
                    self.assertEqual(done.returncode, 1)
                    self.assertTrue(witness['operation_returned'])
                    self.assertFalse(witness['owner_returned'])
                    self.assertEqual(witness['stdout_faults'], 1)
                    self.assertEqual(done.stdout, '')
                    if phase == 'refused_stdout_':
                        self.assertEqual(witness['result_errors'], ['controlled declared refusal'])
                        self.assertEqual(witness['assessment_calls'], 0)
                    else:
                        self.assertEqual(witness['result_errors'], [])
                        self.assertEqual(witness['assessment_calls'], 1)
                        self.assertTrue(witness['assessment_returned'])
                        self.assertEqual(witness['assessment_errors'], ['controlled assessment error'])

    def test_genuine_operation_controls_keep_original_identity_and_os_status(self):
        for consumer in CONSUMERS:
            for case, status in CONTROL_CASES:
                with self.subTest(consumer=consumer, case=case):
                    done, witness = self.child(consumer, 'operation_' + case)
                    self.assertEqual(done.returncode, status)
                    self.assertTrue(witness['escaped_primary_identity'])
                    self.assertFalse(witness['operation_returned'])
                    self.assertFalse(witness['owner_returned'])

    def test_genuine_diagnostic_controls_keep_original_identity_and_os_status(self):
        for consumer in CONSUMERS:
            for phase in ('stderr_', 'format_', 'stdout_'):
                for case, status in CONTROL_CASES:
                    with self.subTest(consumer=consumer, phase=phase, case=case):
                        done, witness = self.child(consumer, phase + case)
                        self.assertEqual(done.returncode, status)
                        self.assertTrue(witness['escaped_report_identity'])
                        self.assertFalse(witness['owner_returned'])
                        self.assertEqual(witness['operation_returned'], phase == 'stdout_')

    def test_raw_main_keeps_operation_cancellation_identity(self):
        for consumer in CONSUMERS:
            for case, status in tuple((name, 0) for name in ZERO_CASES) + CONTROL_CASES:
                with self.subTest(consumer=consumer, case=case):
                    done, witness = self.child(consumer, 'operation_' + case, 'main')
                    self.assertEqual(done.returncode, status)
                    self.assertTrue(witness['escaped_primary_identity'])
                    self.assertFalse(witness['raw_main_returned'])
                    self.assertFalse(witness['operation_returned'])

    def test_raw_main_keeps_diagnostic_fault_identity(self):
        for consumer in CONSUMERS:
            for phase in ('stderr_', 'format_', 'stdout_'):
                for case, status in (('wrapped', 0), ('nonzero', 47), ('keyboard', -2)):
                    with self.subTest(consumer=consumer, phase=phase, case=case):
                        done, witness = self.child(consumer, phase + case, 'main')
                        self.assertEqual(done.returncode, status)
                        self.assertTrue(witness['escaped_report_identity'])
                        self.assertFalse(witness['raw_main_returned'])

    def test_uncaught_ordinary_operation_failure_keeps_raw_main_identity(self):
        for consumer in CONSUMERS:
            for invocation in ('entry', 'main'):
                with self.subTest(consumer=consumer, invocation=invocation):
                    done, witness = self.child(consumer, 'operation_runtime', invocation)
                    self.assertEqual(done.returncode, 1)
                    self.assertFalse(witness['operation_returned'])
                    self.assertFalse(witness['owner_returned'])
                    self.assertFalse(witness['raw_main_returned'])
                    self.assertIn('controlled feature operation failure', done.stderr)
                    if invocation == 'main':
                        self.assertTrue(witness['escaped_primary_identity'])

    def test_original_status_entry_rejects_callback_exit_types_before_observation(self):
        for case in ('hostile_exit', 'hostile_payload', 'transparent_exit', 'int_wrapped'):
            with self.subTest(case=case):
                done, witness = self.child('upstream_status', 'operation_' + case)
                self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                self.assertEqual(done.stdout, '')
                self.assertEqual(witness['classification_callbacks'], 0)
                self.assertFalse(witness['operation_returned'])
                self.assertFalse(witness['owner_returned'])
                self.assertFalse(witness['raw_main_returned'])
                self.assertTrue(all(count == 0 for count in witness['actor_calls'].values()))
                if case in ('hostile_exit', 'transparent_exit'):
                    self.assertIn('unsupported SystemExit type', done.stderr)
                    self.assertEqual(witness['escaped_type'], 'RuntimeError')
                    self.assertFalse(witness['escaped_primary_identity'])
                elif case == 'hostile_payload':
                    self.assertEqual(witness['escaped_type'], 'SystemExit')
                    self.assertEqual(witness['escaped_code'], '<opaque>')
                    self.assertTrue(witness['escaped_primary_identity'])

    def test_raw_status_main_preserves_hostile_objects_without_classification(self):
        for case in ('hostile_exit', 'hostile_payload'):
            with self.subTest(case=case):
                done, witness = self.child('upstream_status', 'operation_' + case, 'main')
                self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                self.assertEqual(done.stdout, '')
                self.assertEqual(witness['classification_callbacks'], 0)
                self.assertTrue(witness['escaped_primary_identity'])
                self.assertFalse(witness['operation_returned'])
                self.assertFalse(witness['owner_returned'])
                self.assertFalse(witness['raw_main_returned'])

    def test_healthy_original_entries_and_raw_main_keep_output_and_status(self):
        for consumer in CONSUMERS:
            for invocation in ('entry', 'main'):
                for case, status in (('success', 0), ('returned_failure', 1),
                                     ('operation_ordinary', 1)):
                    with self.subTest(consumer=consumer, invocation=invocation, case=case):
                        done, witness = self.child(consumer, case, invocation)
                        self.assertEqual(done.returncode, status)
                        self.assertTrue(witness['owner_returned'])
                        self.assertEqual(witness['owner_return_value'], status)
                        self.assertEqual(witness['raw_main_returned'], invocation == 'main')
                        if case == 'operation_ordinary':
                            prefix = {'feature_evidence': 'feature evidence: ',
                                      'feature_gate': 'feature evidence admission: ',
                                      'upstream_gate': 'upstream review admission: ',
                                      'upstream_status': 'upstream status: '}[consumer]
                            self.assertEqual(done.stderr, prefix +
                                             '[Errno 5] controlled feature operation failure\n')
                            self.assertEqual(done.stdout, '')
                        elif consumer != 'feature_evidence':
                            output = json.loads(done.stdout)
                            self.assertEqual(output['errors'], [] if status == 0 else
                                             ['controlled declared refusal'])
                            self.assertEqual(done.stderr, '')
                            if consumer == 'upstream_gate':
                                self.assertEqual(output['missing'], [])
                            elif consumer == 'upstream_status':
                                self.assertEqual(output['pending_count'], 3)
                                self.assertEqual(output['main_sha'], 'a' * 40)
                        elif status == 0:
                            self.assertEqual(done.stdout, 'Declared scenario evidence is complete; '
                                             'semantic review and full gate remain separate\n')
                            self.assertEqual(done.stderr, '')
                        else:
                            self.assertEqual(done.stdout, '')
                            self.assertIn('status', done.stderr)

    def test_evidence_snapshot_and_bundle_creation_still_finish(self):
        for case in ('snapshot_success', 'bundle_success'):
            with self.subTest(case=case):
                done, witness = self.child('feature_evidence', case)
                self.assertEqual(done.returncode, 0)
                self.assertTrue(witness['operation_returned'])
                self.assertTrue(witness['owner_returned'])
                self.assertEqual(witness['owner_return_value'], 0)
                self.assertEqual(done.stderr, '')
                self.assertIn('no scenarios qualified' if case == 'snapshot_success' else
                              'no tests were run', done.stdout)

    def test_help_and_parser_errors_keep_status_without_observing_operation(self):
        for consumer in CONSUMERS:
            for invocation in ('entry', 'main'):
                for case, status in (('help', 0), ('parser_error', 2)):
                    with self.subTest(consumer=consumer, invocation=invocation, case=case):
                        done, witness = self.child(consumer, case, invocation)
                        self.assertEqual(done.returncode, status)
                        self.assertFalse(witness['operation_returned'])
                        self.assertFalse(witness['owner_returned'])
                        self.assertFalse(witness['raw_main_returned'])
                        self.assertIn('usage:', done.stdout if status == 0 else done.stderr)

    def test_evidence_semantic_parser_errors_stay_outside_operation_guard(self):
        for invocation in ('entry', 'main'):
            for case in ('semantic_log_root', 'semantic_destination', 'semantic_missing_destination'):
                with self.subTest(invocation=invocation, case=case):
                    done, witness = self.child('feature_evidence', case, invocation)
                    self.assertEqual(done.returncode, 2)
                    self.assertFalse(witness['operation_returned'])
                    self.assertFalse(witness['raw_main_returned'])
                    self.assertIn('--log-root requires validate; --destination is required only for bundle',
                                  done.stderr)


if __name__ == '__main__':
    unittest.main()

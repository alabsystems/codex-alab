# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Bind matrix verdicts and hashes to one read; inert seams launch no native host."""
from contextlib import redirect_stderr, redirect_stdout
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch


SOURCE = Path(__file__).resolve().parents[1] / 'smoke/renewal_matrix.py'
sys.path.insert(0, str(SOURCE.parent))
SPEC = importlib.util.spec_from_file_location('renewal_matrix_results', SOURCE)
MATRIX = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MATRIX)


def record(passed=True):
    return {'passed': passed, 'rawHostReturncode': 0,
            'guardian': {'returncode': 0, 'timed_out': False},
            'literal': 'naïve 雪'}


def canonical(passed=True):
    return (json.dumps(record(passed), ensure_ascii=False) + '\n').encode('utf-8')


def noncanonical(passed=True):
    value = json.dumps(record(passed), ensure_ascii=False, indent=3)
    return (' \r\n' + value.replace('\n', '\r\n') + '\r\n\t').encode('utf-8')


class RenewalMatrixResultTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix='alab-renewal-matrix-result-')
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name).resolve()

    def run_matrix(self, label, original, replacement=None, *, result_open=None):
        """Use public main with explicit inert admission/controller seams.

        Only the exact owned result path's first real read may trigger a finite
        physical rewrite. JSON decoding, case_passed, digest and save stay real.
        Legacy text/byte reads and the shared bytes snapshot delegate completely
        before the replacement hook. Optional result_open caps actual owned reads;
        JSON decoding, case_passed, digest and report bytes are never replaced.
        """
        root = self.root / label
        root.mkdir()
        host = root / 'owned host' / 'aterm-owned-inert'
        host.parent.mkdir()
        host.write_bytes(b'owned inert host; never executed\n')
        (host.parent / 'build.json').write_text('{}\n')
        package = root / 'owned package'
        (package / 'bin').mkdir(parents=True)
        (package / 'bin/codex').write_bytes(b'owned inert package; never executed\n')
        (package / 'alab-build.json').write_text('{}\n')
        (package / 'codex-package.json').write_text('{}\n')
        output = root / 'matrix'
        result_path = output / 'weekly-quota/result.json'
        reads, rewrites, calls, admission = [], [], [], []
        marker = {'controlled_fixture_only': True}

        def native_record(path):
            self.assertEqual(path, host)
            admission.append('native record seam')
            return dict(marker)

        def native_standing(value):
            self.assertEqual(value, marker)
            admission.append('native standing seam')
            return None

        def native_name(path):
            self.assertEqual(path, host)

        def package_records(path):
            self.assertEqual(path, package)
            admission.append('package records seam')
            return dict(marker), {}, package / 'bin/codex'

        def package_standing(value):
            self.assertEqual(value, marker)
            admission.append('package standing seam')
            return None

        def controller(command, **keywords):
            self.assertEqual(command[command.index('--case') + 1], 'weekly-quota')
            self.assertEqual(Path(command[1]), SOURCE.with_name('renewal_follow.py').resolve())
            self.assertEqual(command[command.index('--host') + 1], str(host))
            self.assertEqual(command[command.index('--package') + 1], str(package))
            destination = Path(command[command.index('--output') + 1])
            self.assertEqual(destination, result_path.parent)
            self.assertEqual(calls, [])
            calls.append('weekly-quota')
            destination.mkdir()
            if original is not None:
                result_path.write_bytes(original)
            keywords['stdout'].write(b'intercepted inert controller; no native process\n')
            return SimpleNamespace(returncode=0)

        def after_read(path, method):
            if path == result_path:
                reads.append(method)
                if replacement is not None and not rewrites:
                    result_path.write_bytes(replacement)
                    rewrites.append(True)

        class ReadingPath(type(Path())):
            def open(path, *arguments, **keywords):
                mode = arguments[0] if arguments else keywords.get('mode', 'r')
                if result_open is not None and path == result_path and mode == 'rb':
                    return result_open(*arguments, **keywords)
                return super().open(*arguments, **keywords)

            def read_text(path, *arguments, **keywords):
                value = super().read_text(*arguments, **keywords)
                after_read(path, 'read_text')
                return value

            def read_bytes(path, *arguments, **keywords):
                value = super().read_bytes(*arguments, **keywords)
                after_read(path, 'read_bytes')
                return value

        original_snapshot = MATRIX.FILE_HASH['file_bytes_snapshot']

        def snapshot(path, *arguments, **keywords):
            value = original_snapshot(path, *arguments, **keywords)
            after_read(path, 'file_bytes_snapshot')
            return value

        native = SimpleNamespace(load=native_record, not_qualifying=native_standing,
                                 require_aterm_name=native_name,
                                 require=MATRIX.native_host_record.require)
        probe = SimpleNamespace(**{**vars(MATRIX.probe), 'package_records': package_records})
        standing = SimpleNamespace(not_qualifying=package_standing)
        process = SimpleNamespace(run=controller, STDOUT=subprocess.STDOUT)
        arguments = [str(SOURCE), '--host', str(host), '--package', str(package),
                     '--output', str(output), '--case', 'weekly-quota']
        pinned = {str(path): hashlib.sha256(path.read_bytes()).hexdigest()
                  for path in MATRIX.pinned_inputs(host, package)}
        stdout, stderr = io.StringIO(), io.StringIO()
        with patch.object(MATRIX, 'Path', ReadingPath), \
                patch.dict(MATRIX.FILE_HASH, {'file_bytes_snapshot': snapshot}), \
                patch.object(MATRIX, 'native_host_record', native), \
                patch.object(MATRIX, 'probe', probe), \
                patch.object(MATRIX, 'package_standing', standing), \
                patch.object(MATRIX, 'subprocess', process), \
                patch.object(sys, 'argv', arguments), redirect_stdout(stdout), redirect_stderr(stderr):
            status = MATRIX.main()
        manifest = json.loads((output / 'qualification.json').read_text())
        self.assertEqual(calls, ['weekly-quota'])
        self.assertEqual(admission, ['native record seam', 'native standing seam',
                                     'package records seam', 'package standing seam'])
        self.assertEqual(manifest['inputs'], pinned)
        self.assertEqual(pinned, {str(path): hashlib.sha256(path.read_bytes()).hexdigest()
                                  for path in MATRIX.pinned_inputs(host, package)})
        return SimpleNamespace(status=status, manifest=manifest, result=result_path,
                               reads=reads, rewrites=rewrites,
                               stdout=stdout.getvalue(), stderr=stderr.getvalue())

    def assert_case(self, run, passed, raw):
        self.assertEqual(run.status, 0 if passed else 1)
        self.assertIs(run.manifest['passed'], passed)
        self.assertEqual(len(run.manifest['cases']), 1)
        entry = run.manifest['cases'][0]
        self.assertEqual(entry['case'], 'weekly-quota')
        self.assertEqual(entry['controllerExit'], 0)
        self.assertIs(entry['passed'], passed)
        self.assertEqual(entry['result'], str(run.result))
        self.assertEqual(entry['resultSha256'],
                         hashlib.sha256(raw).hexdigest() if raw is not None else None)
        self.assertFalse(run.manifest['allSixCasesPassed'])
        self.assertFalse(run.manifest['allAvailableCasesPassed'])
        if not passed:
            self.assertEqual(run.manifest['error'], 'RuntimeError: paired case failed: weekly-quota')

    def test_canonical_healthy_result_keeps_its_original_digest(self):
        raw = canonical()
        run = self.run_matrix('canonical', raw)
        self.assert_case(run, True, raw)
        self.assertEqual(run.result.read_bytes(), raw)
        self.assertEqual(run.rewrites, [])

    def test_noncanonical_crlf_and_literal_utf8_hash_original_bytes(self):
        raw = noncanonical()
        self.assertIn(b'\r\n', raw)
        self.assertIn('naïve 雪'.encode('utf-8'), raw)
        self.assertNotEqual(hashlib.sha256(raw).hexdigest(), hashlib.sha256(canonical()).hexdigest())
        run = self.run_matrix('literal-bytes', raw)
        self.assert_case(run, True, raw)
        self.assertEqual(run.result.read_bytes(), raw)

    def test_passing_first_read_keeps_its_digest_after_failing_replacement(self):
        original, replacement = noncanonical(), canonical(False)
        run = self.run_matrix('pass-to-fail', original, replacement)
        self.assertEqual(run.rewrites, [True])
        self.assertTrue(run.reads)
        self.assertEqual(run.result.read_bytes(), replacement)
        self.assert_case(run, True, original)

    def test_failing_first_read_keeps_its_digest_after_passing_replacement(self):
        original, replacement = noncanonical(False), canonical()
        run = self.run_matrix('fail-to-pass', original, replacement)
        self.assertEqual(run.rewrites, [True])
        self.assertTrue(run.reads)
        self.assertEqual(run.result.read_bytes(), replacement)
        self.assert_case(run, False, original)

    def test_valid_falsey_json_keeps_present_file_hash_and_fails_the_case(self):
        for label, raw in [('null', b'null\n'), ('false', b'false\n'), ('zero', b'0\n'),
                           ('string', b'""\n'), ('array', b'[]\n'), ('object', b'{}\n')]:
            with self.subTest(form=label):
                run = self.run_matrix('falsey-' + label, raw)
                self.assert_case(run, False, raw)
                self.assertEqual(run.result.read_bytes(), raw)

    def test_absent_result_still_has_no_digest_and_fails_the_case(self):
        run = self.run_matrix('absent', None)
        self.assert_case(run, False, None)
        self.assertFalse(run.result.exists())
        self.assertEqual(run.reads, [])

    def test_malformed_or_non_utf8_text_remains_rejected_without_case_acceptance(self):
        text = canonical().decode('utf-8')
        invalid = [('malformed', b'{"passed":'), ('invalid-utf8', b'\xff' + canonical()),
                   ('utf8-bom', b'\xef\xbb\xbf' + canonical()),
                   ('utf16', text.encode('utf-16')), ('utf32', text.encode('utf-32'))]
        for label, raw in invalid:
            with self.subTest(encoding=label):
                run = self.run_matrix('rejected-' + label, raw)
                self.assertEqual(run.status, 1)
                self.assertIs(run.manifest['passed'], False)
                self.assertEqual(run.manifest['cases'], [])
                self.assertRegex(run.manifest['error'], r'^(JSONDecodeError|UnicodeDecodeError): ')
                self.assertEqual(run.result.read_bytes(), raw)


if __name__ == '__main__':
    unittest.main()

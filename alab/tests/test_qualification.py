# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import copy
import io
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import shutil
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import qualification as QUAL
import upstream_status as STATUS

ALAB = Path(QUAL.__file__).parent


class QualificationTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.repo = self.root / 'candidate'
        (self.repo / 'alab').mkdir(parents=True)
        self.git('init', '-q')
        self.git('config', 'user.name', 'Fixture')
        self.git('config', 'user.email', 'fixture@invalid')
        self.git('config', 'commit.gpgsign', 'false')
        (self.repo / 'tracked').write_text('original')
        self.gate = self.repo / 'alab/check.sh'
        self.install('printf "fixture stdout\\n"\nprintf "fixture stderr\\n" >&2\n')
        self.git('update-ref', 'refs/remotes/origin/main', 'HEAD')
        self.git('update-ref', 'refs/remotes/upstream/main', 'HEAD')
        self.source = self.git('rev-parse', 'HEAD')

    def git(self, *args):
        env = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
        env.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1')
        return subprocess.check_output(['git', '-C', str(self.repo), *args],
                                       env=env, text=True, stderr=subprocess.PIPE).strip()

    def install(self, body):
        self.gate.write_text('#!/bin/sh\nset -eu\n' + body)
        self.gate.chmod(0o755)
        self.git('add', '.')
        self.git('commit', '-qm', 'fixture gate')

    def execute(self, name='observation'):
        output = self.root / name
        stream = io.BytesIO()
        status = QUAL.run(self.repo, output, stream)
        return status, output, stream.getvalue()

    def test_actual_gate_streams_and_retains_a_complete_source_observation(self):
        status, output, streamed = self.execute()
        self.assertEqual(status, 0)
        self.assertIn(b'fixture stdout', streamed)
        self.assertIn(b'fixture stderr', streamed)
        record = json.loads((output / 'record.json').read_text())
        self.assertEqual(record['schema'], 5)
        self.assertEqual(record['command'], [str(self.gate.resolve())])
        self.assertEqual(record['before'], record['after'])
        self.assertEqual(record['qualified_source'], self.source)
        self.assertEqual(QUAL.inspect(self.repo, output / 'record.json')['source'], self.source)

    def test_failed_resource_stop_and_signal_statuses_do_not_qualify(self):
        for code in (1, 2, 75, 143):
            with self.subTest(code=code):
                self.install('exit ' + str(code) + '\n')
                status, output, _ = self.execute('exit-' + str(code))
                self.assertEqual(status, code)
                record = json.loads((output / 'record.json').read_text())
                self.assertIsNone(record['qualified_source'])
                with self.assertRaises(ValueError):
                    QUAL.inspect(self.repo, output / 'record.json')

    def test_dirty_candidate_is_refused_before_its_gate_runs(self):
        self.install('touch .git/gate-ran\n')
        (self.repo / 'tracked').write_text('dirty')
        with self.assertRaisesRegex(ValueError, 'committed clean'):
            self.execute()
        self.assertFalse((self.repo / '.git/gate-ran').exists())

    def test_external_clean_filter_cannot_qualify_an_altered_gate(self):
        self.install('touch .git/original-gate-ran\nexit 17\n')
        original = self.root / 'original-gate'
        original.write_bytes(self.gate.read_bytes())
        (self.repo / '.gitattributes').write_text('alab/check.sh filter=fixture.gate\n')
        self.git('add', '.gitattributes')
        self.git('commit', '-qm', 'gate filter attribute')
        self.git('config', 'filter.fixture.gate.clean', '/bin/cat ' + shlex.quote(str(original)))
        self.git('config', 'filter.fixture.gate.required', 'true')
        self.gate.write_text('#!/bin/sh\nset -eu\ntouch .git/altered-gate-ran\nexit 0\n')
        with self.assertRaisesRegex(ValueError, 'committed clean'):
            self.execute()
        self.assertFalse((self.repo / '.git/altered-gate-ran').exists())
        self.assertFalse((self.repo / '.git/original-gate-ran').exists())

    def test_configured_external_filters_do_not_run_for_clean_source(self):
        (self.repo / '.gitattributes').write_text('alab/check.sh filter=fixture.gate\n')
        self.git('add', '.gitattributes')
        self.git('commit', '-qm', 'gate filter attribute')
        self.git('config', 'filter.fixture.gate.clean', '/usr/bin/false')
        self.git('config', 'filter.fixture.gate.process', '/usr/bin/false')
        self.git('config', 'filter.fixture.gate.required', 'true')
        self.git('config', 'filter.Odd.Name With\tSpace.clean', '/usr/bin/false')
        self.assertEqual(self.execute()[0], 0)

    def test_builtin_ident_cannot_qualify_an_altered_gate(self):
        self.install("printf '%s\\n' '$Id$'\n")
        (self.repo / '.gitattributes').write_text('alab/check.sh ident\n')
        self.git('add', '.gitattributes')
        self.git('commit', '-qm', 'gate ident attribute')
        self.gate.unlink()
        self.git('checkout-index', '-f', '--', 'alab/check.sh')
        self.git('add', 'alab/check.sh')
        original = self.gate.read_bytes()
        start, end = original.index(b'$Id: '), original.index(b'$', original.index(b'$Id: ') + 1)
        replacement = b"$Id: '; touch .git/ran; echo '"
        replacement = replacement.ljust(end - start, b' ') + b'$'
        self.gate.write_bytes(original[:start] + replacement + original[end + 1:])
        self.assertEqual(self.git('status', '--porcelain'), '',
                         'CONTROL: builtin ident conceals the changed gate bytes')
        with self.assertRaises(ValueError):
            self.execute()
        self.assertFalse((self.repo / '.git/ran').exists())

    def test_expanded_ident_gate_does_not_run_a_smudge_only_driver(self):
        self.install("printf '%s\\n' '$Id$'\n")
        (self.repo / '.gitattributes').write_text('alab/check.sh ident filter=ident-smudge\n')
        self.git('add', '.gitattributes')
        self.git('commit', '-qm', 'gate checkout attributes')
        self.gate.unlink()
        self.git('checkout-index', '-f', '--', 'alab/check.sh')
        self.git('add', 'alab/check.sh')
        self.assertIn(b'$Id: ', self.gate.read_bytes())
        callback = self.root / 'smudge-called'
        converter = self.root / 'smudge.py'
        converter.write_text('from pathlib import Path\nimport sys\n'
                             f'Path({str(callback)!r}).touch()\n'
                             'sys.stdout.buffer.write(sys.stdin.buffer.read())\n')
        self.git('config', 'filter.ident-smudge.smudge',
                 shlex.join([sys.executable, '-I', '-B', str(converter)]))
        self.assertEqual(self.execute()[0], 0)
        self.assertFalse(callback.exists())

    def test_external_filter_cannot_hide_a_submodule_input_change(self):
        self.install('touch .git/gate-ran\n')
        child, child_git = self.add_submodule()
        (child / '.gitattributes').write_text('input filter=fixture\n')
        child_git(child, 'add', '.gitattributes')
        child_git(child, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@invalid',
                  '-c', 'commit.gpgsign=false', 'commit', '-qm', 'submodule filter')
        self.git('add', 'dependency')
        self.git('commit', '-qm', 'updated submodule')
        original = self.root / 'original-input'
        original.write_bytes((child / 'input').read_bytes())
        child_git(child, 'config', 'filter.fixture.clean', '/bin/cat ' + shlex.quote(str(original)))
        child_git(child, 'config', 'filter.fixture.required', 'true')
        (child / 'input').write_text('uncommitted child input')
        with self.assertRaisesRegex(ValueError, 'committed clean'):
            self.execute()
        self.assertFalse((self.repo / '.git/gate-ran').exists())

    def test_builtin_line_ending_conversion_remains_qualifiable(self):
        (self.repo / '.gitattributes').write_text('tracked text eol=crlf\n')
        (self.repo / 'tracked').write_bytes(b'line one\nline two\n')
        self.git('add', '.gitattributes', 'tracked')
        self.git('commit', '-qm', 'builtin line endings')
        (self.repo / 'tracked').write_bytes(b'line one\r\nline two\r\n')
        self.assertEqual(self.execute()[0], 0)

    def test_unsupported_filter_driver_name_refuses_before_the_gate(self):
        self.install('touch .git/gate-ran\n')
        self.git('config', 'filter.Name=Ambiguous.clean', '/bin/cat')
        with self.assertRaisesRegex(ValueError, 'filter'):
            self.execute()
        self.assertFalse((self.repo / '.git/gate-ran').exists())

    def test_filter_configuration_requires_bounded_unambiguous_key_records(self):
        for keys in ('filter.fixture.clean', '\0', 'filter.fixture.clean\0\0',
                     'filter..clean\0', 'x' * (QUAL.MAX_FILTER_CONFIG_BYTES + 1) + '\0'):
            with self.subTest(keys=keys[:40]), patch.object(QUAL, 'git', return_value=keys) as query:
                with self.assertRaisesRegex(ValueError, 'filter'):
                    QUAL.external_filter_config(self.repo)
                query.assert_called_once_with(self.repo, 'config', '--name-only', '--null', '--list')

    def test_redirected_git_root_cannot_qualify_another_gate_directory(self):
        self.install('exit 17\n')
        observed = self.root / 'observed'
        shutil.copytree(self.repo, observed, ignore=shutil.ignore_patterns('.git'))
        self.git('config', 'core.worktree', str(observed))
        self.gate.write_text('#!/bin/sh\ntouch .git/replacement-ran\n')
        self.assertEqual(self.git('status', '--porcelain'), '')
        with self.assertRaisesRegex(ValueError, 'root'):
            self.execute()
        self.assertFalse((self.repo / '.git/replacement-ran').exists())

    def test_subdirectory_is_not_the_gate_root(self):
        with self.assertRaisesRegex(ValueError, 'root'):
            QUAL.run(self.repo / 'alab', self.root / 'subdirectory', io.BytesIO())

    def test_canonical_root_alias_uses_the_actual_gate_directory(self):
        alias = self.root / 'alias'
        alias.symlink_to(self.repo, target_is_directory=True)
        output = self.root / 'alias-observation'
        self.assertEqual(QUAL.run(alias, output, io.BytesIO()), 0)
        record = json.loads((output / 'record.json').read_text())
        self.assertEqual(record['command'], [str(self.gate.resolve())])

    def add_submodule(self, parent=None, name='dependency'):
        parent = parent or self.repo
        source = self.root / (name + '-origin')
        source.mkdir()
        environment = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
        environment.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1')

        def child_git(repo, *args):
            return subprocess.check_output(['git', '-C', str(repo), *args], env=environment,
                                           text=True, stderr=subprocess.PIPE).strip()

        child_git(source, 'init', '-q')
        child_git(source, 'config', 'user.name', 'Fixture')
        child_git(source, 'config', 'user.email', 'fixture@invalid')
        child_git(source, 'config', 'commit.gpgsign', 'false')
        (source / 'input').write_text('original')
        child_git(source, 'add', '.')
        child_git(source, 'commit', '-qm', 'dependency')
        child_git(parent, '-c', 'protocol.file.allow=always', 'submodule', 'add', '-q',
                  str(source), name)
        child_git(parent, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@invalid',
                  '-c', 'commit.gpgsign=false', 'commit', '-qm', 'add dependency')
        return parent / name, child_git

    def test_hidden_submodule_edit_prevents_qualification(self):
        self.install('touch .git/gate-ran\n')
        child, child_git = self.add_submodule()
        child_git(child, 'update-index', '--assume-unchanged', 'input')
        (child / 'input').write_text('hidden change')
        self.assertEqual(self.git('status', '--porcelain', '--ignore-submodules=none'), '')
        with self.assertRaisesRegex(ValueError, 'committed clean'):
            self.execute()
        self.assertFalse((self.repo / '.git/gate-ran').exists())

    def test_hidden_nested_submodule_edit_prevents_qualification(self):
        child, child_git = self.add_submodule()
        nested, _ = self.add_submodule(child, 'nested')
        self.git('add', 'dependency')
        self.git('commit', '-qm', 'advance nested dependency')
        child_git(nested, 'update-index', '--assume-unchanged', 'input')
        (nested / 'input').write_text('hidden nested change')
        self.assertEqual(self.git('status', '--porcelain', '--ignore-submodules=none'), '')
        with self.assertRaisesRegex(ValueError, 'committed clean'):
            self.execute()

    def test_clean_submodule_is_checked_without_changing_its_index(self):
        child, child_git = self.add_submodule()
        gitdir = Path(child_git(child, 'rev-parse', '--absolute-git-dir'))
        before = (gitdir / 'index').read_bytes()
        self.assertEqual(self.execute()[0], 0)
        self.assertEqual((gitdir / 'index').read_bytes(), before)

    def test_submodule_at_another_commit_is_not_the_recorded_input(self):
        child, child_git = self.add_submodule()
        child_git(child, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@invalid',
                  '-c', 'commit.gpgsign=false', 'commit', '--allow-empty', '-qm', 'new head')
        with self.assertRaisesRegex(ValueError, 'committed clean'):
            self.execute()

    def test_unbound_submodule_directory_cannot_supply_a_clean_result(self):
        child, _ = self.add_submodule()
        (child / '.git').unlink()
        with self.assertRaisesRegex(ValueError, 'root'):
            self.execute()

    def test_index_flags_cannot_hide_uncommitted_gate_or_source(self):
        self.install('touch .git/gate-ran\nexit 17\n')
        for flag in ('assume-unchanged', 'skip-worktree'):
            for name in ('alab/check.sh', 'tracked'):
                with self.subTest(flag=flag, path=name):
                    self.git('update-index', '--' + flag, name)
                    try:
                        (self.repo / name).write_text(
                            '#!/bin/sh\ntouch .git/gate-ran\nexit 0\n')
                        self.assertEqual(self.git('status', '--porcelain'), '')
                        with self.assertRaisesRegex(ValueError, 'committed clean'):
                            self.execute(flag + '-' + Path(name).name)
                        self.assertFalse((self.repo / '.git/gate-ran').exists())
                    finally:
                        self.git('update-index', '--no-' + flag, name)
                        self.git('reset', '--hard', 'HEAD')
                        (self.repo / '.git/gate-ran').unlink(missing_ok=True)

    def test_index_flags_introduced_by_gate_prevent_qualification(self):
        for flag in ('assume-unchanged', 'skip-worktree'):
            with self.subTest(flag=flag):
                self.install('git update-index --' + flag + ' tracked\n'
                             'printf changed > tracked\n')
                try:
                    status, output, _ = self.execute(flag)
                    self.assertEqual(status, 1)
                    record = json.loads((output / 'record.json').read_text())
                    self.assertEqual(record['status'], 0)
                    self.assertIsNone(record['qualified_source'])
                    with self.assertRaises(ValueError):
                        QUAL.inspect(self.repo, output / 'record.json')
                finally:
                    self.git('update-index', '--no-' + flag, 'tracked')
                    self.git('reset', '--hard', 'HEAD')

    def test_relaxed_stat_config_cannot_hide_a_same_size_gate_replacement(self):
        self.install('touch .git/gate-old\n')
        self.git('config', 'core.trustctime', 'false')
        self.git('config', 'core.checkStat', 'minimal')
        old_time = 946684800 * 10**9
        os.utime(self.gate, ns=(old_time, old_time))
        self.git('update-index', '--refresh')
        original = self.gate.read_text()
        replacement = original.replace('gate-old', 'gate-new')
        self.assertEqual(len(original), len(replacement))
        self.gate.write_text(replacement)
        os.utime(self.gate, ns=(old_time, old_time))
        self.assertEqual(self.git('ls-files', '-v', '--', 'alab/check.sh'), 'H alab/check.sh')
        self.assertEqual(self.git('status', '--porcelain'), '')
        with self.assertRaisesRegex(ValueError, 'committed clean'):
            self.execute()
        self.assertFalse((self.repo / '.git/gate-new').exists())

    def test_local_status_config_cannot_hide_untracked_inputs(self):
        self.install('touch .git/gate-ran\n')
        self.git('config', 'status.showUntrackedFiles', 'no')
        (self.repo / 'unexpected-input').write_text('not committed')
        self.assertEqual(self.git('status', '--porcelain'), '')
        with self.assertRaisesRegex(ValueError, 'committed clean'):
            self.execute()
        self.assertFalse((self.repo / '.git/gate-ran').exists())

    def test_checked_paths_with_control_characters_remain_qualifiable(self):
        (self.repo / 'tab\tnewline\nS name').write_text('committed')
        self.git('add', '.')
        self.git('commit', '-qm', 'control characters in a path')
        status, output, _ = self.execute()
        self.assertEqual(status, 0)
        self.assertEqual(QUAL.inspect(self.repo, output / 'record.json')['source'],
                         self.git('rev-parse', 'HEAD'))

    def test_staged_changes_stay_dirty_when_working_files_match_head(self):
        (self.repo / 'tracked').write_text('staged change')
        self.git('add', 'tracked')
        (self.repo / 'tracked').write_text('original')
        with self.assertRaisesRegex(ValueError, 'committed clean'):
            self.execute()

    def test_deleted_intent_to_add_entry_stays_dirty(self):
        path = self.repo / 'planned'
        path.write_text('planned')
        self.git('add', '-N', 'planned')
        path.unlink()
        with self.assertRaisesRegex(ValueError, 'committed clean'):
            self.execute()

    def test_fresh_comparison_preserves_real_index_and_matching_exempt_paths(self):
        self.git('config', 'core.splitIndex', 'true')
        self.git('update-index', '--assume-unchanged', 'tracked')
        self.git('update-index', '--skip-worktree', 'alab/check.sh')
        index = self.repo / '.git/index'
        before = index.read_bytes()
        shared_before = sorted((self.repo / '.git').glob('sharedindex.*'))
        status, _, _ = self.execute()
        self.assertEqual(status, 0)
        self.assertEqual(index.read_bytes(), before)
        self.assertEqual(sorted((self.repo / '.git').glob('sharedindex.*')), shared_before)
        self.assertEqual(list((self.repo / '.git').glob('alab-status-*')), [])

    def test_changed_source_or_base_cannot_qualify_even_after_gate_exit_zero(self):
        for label, body in [
                ('dirty', 'printf changed > tracked\n'),
                ('commit', 'git commit -q --allow-empty -m changed\n'),
                ('base', 'git update-ref refs/remotes/origin/main HEAD\n')]:
            with self.subTest(label=label):
                self.git('reset', '--hard', self.source)
                self.git('update-ref', 'refs/remotes/origin/main', self.source)
                self.install(body)
                status, output, _ = self.execute(label)
                self.assertEqual(status, 1)
                record = json.loads((output / 'record.json').read_text())
                self.assertEqual(record['status'], 0)
                self.assertIsNone(record['qualified_source'])

    def test_existing_output_is_not_overwritten_or_executed_again(self):
        self.install('touch .git/gate-ran\n')
        status, output, _ = self.execute()
        self.assertEqual(status, 0)
        before = (output / 'record.json').read_bytes()
        (self.repo / '.git/gate-ran').unlink()
        with self.assertRaises(FileExistsError):
            QUAL.run(self.repo, output, io.BytesIO())
        self.assertEqual((output / 'record.json').read_bytes(), before)
        self.assertFalse((self.repo / '.git/gate-ran').exists())

    def test_default_output_is_outside_inputs_and_record_can_move(self):
        self.assertEqual(QUAL.run(self.repo, stream=io.BytesIO()), 0)
        records = list((self.repo / '.git/alab-qualification').glob('*/run-*/record.json'))
        self.assertEqual(len(records), 1)
        moved = self.root / 'moved'
        records[0].parent.rename(moved)
        self.assertEqual(QUAL.inspect(self.repo, moved / 'record.json')['source'], self.source)

    def test_log_changes_and_inconsistent_records_are_refused(self):
        _, output, _ = self.execute()
        path = output / 'record.json'
        original = json.loads(path.read_text())
        for mutation in ('status', 'qualified_source', 'tree', 'upstream'):
            record = copy.deepcopy(original)
            if mutation == 'status':
                record['status'] = False
            elif mutation == 'qualified_source':
                record['qualified_source'] = 'b' * 40
            else:
                value = '--help' if mutation == 'upstream' else 'b' * 40
                record['before'][mutation] = value
                record['after'][mutation] = value
            path.write_text(json.dumps(record))
            with self.assertRaises((ValueError, subprocess.CalledProcessError)):
                QUAL.inspect(self.repo, path)
        path.write_text(json.dumps(original))
        (output / 'gate.log').write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError, 'hash changed'):
            QUAL.inspect(self.repo, path)

    def test_inherited_runtime_credentials_and_git_redirect_are_not_used(self):
        self.install('[ -z "${OPENAI_API_KEY+x}" ]\n[ -z "${CODEX_API_KEY+x}" ]\n'
                     '[ -z "${CODEX_ACCESS_TOKEN+x}" ]\n[ -z "${GIT_DIR+x}" ]\n')
        with patch.dict(os.environ, OPENAI_API_KEY='fixture-only', CODEX_API_KEY='fixture-only',
                        CODEX_ACCESS_TOKEN='fixture-only', GIT_DIR='/missing-redirect'):
            self.assertEqual(self.execute()[0], 0)

    def test_output_under_candidate_inputs_is_refused_before_creation(self):
        output = self.repo / 'untracked-results'
        with self.assertRaisesRegex(ValueError, 'outside candidate inputs'):
            QUAL.run(self.repo, output, io.BytesIO())
        self.assertFalse(output.exists())

    def test_actual_child_signal_is_preserved_as_interruption_not_qualification(self):
        self.install('kill -TERM $$\n')
        status, output, _ = self.execute()
        self.assertEqual(status, 143)
        record = json.loads((output / 'record.json').read_text())
        self.assertEqual(record['status'], -15)
        self.assertIsNone(record['qualified_source'])

    def test_status_reads_qualification_without_inventing_artifact_or_fetch_stages(self):
        _, output, _ = self.execute()
        result = STATUS.report(self.repo, qualification_record=output / 'record.json')
        self.assertEqual(result['qualified_sha'], self.source)
        self.assertIsNone(result['packaged_sha'])
        self.assertIsNone(result['running_sha'])
        self.assertIsNone(result['last_successful_fetch_at'])
        self.assertFalse(result['errors'])

    def test_old_or_malformed_schema_cannot_report_a_qualified_source(self):
        _, output, _ = self.execute()
        path = output / 'record.json'
        record = json.loads(path.read_text())
        for schema in (1, 2, 3, 4, 5.0, True, None, '5', 6):
            with self.subTest(schema=schema, kind=type(schema).__name__):
                record['schema'] = schema
                path.write_text(json.dumps(record))
                result = STATUS.report(self.repo, qualification_record=path)
                self.assertIsNone(result['qualified_sha'])
                self.assertTrue(result['errors'])

    def test_production_sync_runs_the_recorder_and_preserves_gate_exit_classes(self):
        for name in ('qualification.py', 'upstream_intake.py', 'source_content.py'):
            shutil.copyfile(ALAB / name, self.repo / 'alab' / name)
        source = (ALAB / 'sync.sh').read_text()
        start = source.index('\nrun_check() {') + 1
        end = source.index('\n}\n', start) + 3
        body = source[start:end] + '''
wt="$1"
check=1
mode=continue
in_wt() { git -C "$wt" "$@"; }
alab_python_311() { printf '%s\\n' "$FIXTURE_PYTHON"; }
alab_say() { printf '%s\\n' "$*"; }
alab_die() { printf '%s\\n' "$*" >&2; exit 1; }
exit_with() { exit "$1"; }
check_failed() { exit "$1"; }
run_check
'''
        for code, expected in [(0, 0), (2, 2), (75, 75), (143, 1)]:
            # run_check intentionally reports a signal interruption as an error
            # without recording a source fault; preserve its existing behavior.
            self.install('exit ' + str(code) + '\n')
            environment = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
            environment.update(FIXTURE_PYTHON=sys.executable, GIT_CONFIG_GLOBAL=os.devnull,
                               GIT_CONFIG_NOSYSTEM='1')
            done = subprocess.run(['/bin/bash', '-c', body, 'sync-recorder-fixture', str(self.repo)],
                                  env=environment, capture_output=True, text=True)
            self.assertEqual(done.returncode, expected, done.stdout + done.stderr)
            self.assertIn('qualification record:', done.stdout)


if __name__ == '__main__':
    unittest.main()

# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import copy
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).parents[1]))
import feature_inventory


ROOT = Path(__file__).resolve().parents[2]


class InventoryTests(unittest.TestCase):
    def policy_catalog(self, lifecycle, **fields):
        feature = dict(id='example', lifecycle=lifecycle, purpose='Own fixture hooks',
                       topics=['owned'], implementation=['owned'],
                       invariants=['Keep fixture state'], scenarios=['Inspect fixture state'],
                       equivalence='Equivalent fixture behavior')
        feature.update(fields)
        return dict(schema=1, features=[feature])

    def test_schema_requires_integer_version_one(self):
        catalog = self.policy_catalog('active')
        self.assertEqual(feature_inventory.validate(
            catalog, set(), Path('.'), path_exists=lambda _path: True), [])
        for schema in (True, 1.0, 2, '1', None):
            with self.subTest(schema=schema):
                changed = copy.deepcopy(catalog)
                changed['schema'] = schema
                errors = feature_inventory.validate(
                    changed, set(), Path('.'), path_exists=lambda _path: True)
                self.assertIn('unsupported inventory schema', errors)

    def test_policy_files_structure_is_checked_for_every_lifecycle(self):
        malformed = ['policy.conf', True, {}, ['owned', 3], [None], [''], [' \t'], None]
        for lifecycle in ('active', 'retiring', 'retired'):
            for value in malformed:
                with self.subTest(lifecycle=lifecycle, policy_files=value):
                    catalog = self.policy_catalog(lifecycle, policy_files=value)
                    errors = feature_inventory.validate(
                        catalog, set(), Path('.'), path_exists=lambda _path: True)
                    self.assertIn(
                        'example: policy_files must be a list of nonempty strings', errors)

    def test_policy_files_are_optional_or_empty_for_every_lifecycle(self):
        for lifecycle in ('active', 'retiring', 'retired'):
            for fields in ({}, {'policy_files': []}):
                with self.subTest(lifecycle=lifecycle, fields=fields):
                    self.assertEqual(feature_inventory.validate(
                        self.policy_catalog(lifecycle, **fields), set(), Path('.'),
                        path_exists=lambda _path: True), [])

    def test_live_policy_paths_must_exist_and_retired_paths_are_not_looked_up(self):
        for lifecycle in ('active', 'retiring'):
            with self.subTest(lifecycle=lifecycle):
                catalog = self.policy_catalog(lifecycle, policy_files=['policy.conf'])
                self.assertEqual(feature_inventory.validate(
                    catalog, set(), Path('.'), path_exists=lambda _path: True), [])
                errors = feature_inventory.validate(
                    catalog, set(), Path('.'), path_exists=lambda path: path == Path('owned'))
                self.assertIn("example: invalid owned path 'policy.conf'", errors)

        def unexpected_lookup(path):
            self.fail(f'retired feature path must not be looked up: {path}')

        catalog = self.policy_catalog(
            'retired', implementation=['absent-implementation'], policy_files=['absent-policy'])
        self.assertEqual(feature_inventory.validate(
            catalog, set(), Path('.'), path_exists=unexpected_lookup), [])

    def test_ownership_retirement_and_path_failures(self):
        with tempfile.TemporaryDirectory() as scratch:
            repo = Path(scratch)
            (repo / 'owned').touch()
            feature = dict(id='recovery', lifecycle='active', purpose='Retain input',
                           topics=['exec-input'], implementation=['owned'],
                           invariants=['No lost input'], scenarios=['Cancel handoff'],
                           equivalence='Same cancellation and ownership behavior')
            catalog = dict(schema=1, features=[feature])
            topics = {'exec-input'}
            self.assertEqual(feature_inventory.validate(catalog, topics, repo), [])
            self.assertIn('unowned hook topic: new-state', feature_inventory.validate(catalog, topics | {'new-state'}, repo))
            for field, value in [('lifecycle', 'retired'), ('implementation', ['missing']),
                                 ('implementation', ['../owned']), ('scenarios', [])]:
                with self.subTest(field=field, value=value):
                    changed = copy.deepcopy(catalog)
                    changed['features'][0][field] = value
                    self.assertTrue(feature_inventory.validate(changed, topics, repo))
            catalog['features'].append(copy.deepcopy(feature))
            errors = feature_inventory.validate(catalog, topics, repo)
            self.assertIn('duplicate feature ID: recovery', errors)
            self.assertIn('duplicate topic owner: exec-input', errors)


class CommittedInventoryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='alab-feature-inventory-')
        self.addCleanup(self.temporary.cleanup)
        self.parent = Path(self.temporary.name)
        self.repo = self.parent / 'repo'
        self.repo.mkdir()
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith('GIT_')}
        self.env.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1',
                        GIT_AUTHOR_NAME='Inventory test',
                        GIT_AUTHOR_EMAIL='inventory@example.invalid',
                        GIT_COMMITTER_NAME='Inventory test',
                        GIT_COMMITTER_EMAIL='inventory@example.invalid', LC_ALL='C')
        self.git = shutil.which('git')
        self.run_git('init', '-q', '-b', 'main')
        (self.repo / 'alab').mkdir()
        for name in ('delta.sh', 'lib.sh', 'feature_inventory.py', 'hook_topics.py'):
            shutil.copy2(ROOT / 'alab' / name, self.repo / 'alab' / name)
        self.write('src/hooks.rs', '// ALAB(owned): committed fixture hook\n')
        self.write('src/implementation.rs', 'pub fn fixture() {}\n')
        self.catalog()
        self.initial = self.commit('initial inventory')

    def write(self, path, text):
        target = self.repo / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def catalog(self, topics=('owned',), implementation=('src/implementation.rs',),
                policy_files=()):
        feature = dict(id='example', lifecycle='active', purpose='Own fixture hooks',
                       topics=list(topics), implementation=list(implementation),
                       invariants=['Keep fixture state'], scenarios=['Inspect fixture state'],
                       equivalence='Equivalent fixture behavior')
        if policy_files:
            feature['policy_files'] = list(policy_files)
        fields = ['schema = 1', '[[features]]']
        fields += [f'{key} = {json.dumps(value)}' for key, value in feature.items()]
        self.write('alab/features.toml', '\n'.join(fields) + '\n')

    def run_git(self, *args, cwd=None):
        result = subprocess.run([self.git, *args], cwd=cwd or self.repo, env=self.env,
                                text=True, capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.strip()

    def commit(self, message):
        self.run_git('add', '-A')
        self.run_git('commit', '-q', '-m', message)
        return self.run_git('rev-parse', 'HEAD')

    def inventory(self, *args, cwd=None, env=None):
        return subprocess.run(
            [sys.executable, '-I', str(self.repo / 'alab/feature_inventory.py'), *args],
            cwd=cwd or self.repo, env=self.env if env is None else env,
            text=True, capture_output=True, timeout=15)

    def assert_source_passes(self, result, source):
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('ownership complete: 1 hook topics', result.stdout)
        self.assertIn(source, result.stdout)

    def test_committed_schema_requires_integer_version_one(self):
        self.assert_source_passes(self.inventory(), self.initial)
        path = self.repo / 'alab/features.toml'
        original = path.read_text()
        for schema in ('true', '1.0', '2', '"1"'):
            with self.subTest(schema=schema):
                path.write_text(original.replace('schema = 1\n', f'schema = {schema}\n', 1))
                self.commit('invalid catalog schema')
                result = self.inventory()
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertIn('unsupported inventory schema', result.stderr)
                self.assertNotIn('ownership complete:', result.stdout)

    def assert_exact_topic_ownership(self, topic, other_owner, *, hook=None):
        if hook is None:
            hook = f'// ALAB({topic}): committed fixture hook\n'.encode()
        (self.repo / 'src/hooks.rs').write_bytes(hook)
        self.catalog(topics=(other_owner,))
        source = self.commit('hook without its exact topic owner')
        rejected = self.inventory()
        self.assertNotEqual(rejected.returncode, 0, rejected.stdout)
        self.assertIn('unowned hook topic:', rejected.stderr)
        self.assertNotIn('ownership complete:', rejected.stdout)

        self.catalog(topics=(topic,))
        owned_source = self.commit('own the exact committed hook topic')
        self.assert_source_passes(self.inventory(), owned_source)
        return source, rejected

    def test_leading_space_topic_requires_its_exact_owner(self):
        self.assert_exact_topic_ownership(' owned', 'owned')

    def test_embedded_tab_topic_requires_its_exact_owner(self):
        self.assert_exact_topic_ownership('owned\talias', 'owned')

    def test_carriage_return_topic_survives_committed_byte_extraction(self):
        source, _ = self.assert_exact_topic_ownership('owned\r', 'owned')
        result = subprocess.run(
            [self.git, 'show', source + ':src/hooks.rs'], cwd=self.repo, env=self.env,
            capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(b'ALAB(owned\r)', result.stdout)

    def test_binary_attributed_source_reports_its_actual_named_topic(self):
        self.write('.gitattributes', 'src/hooks.rs binary\n')
        _, rejected = self.assert_exact_topic_ownership('binary-topic', '(untagged)')
        self.assertIn('unowned hook topic: binary-topic', rejected.stderr)
        self.assertNotIn('unowned hook topic: (untagged)', rejected.stderr)

    def test_nul_source_reports_its_actual_named_topic(self):
        _, rejected = self.assert_exact_topic_ownership(
            'nul-topic', '(untagged)', hook=b'\0// ALAB(nul-topic): committed fixture hook\n')
        self.assertIn('unowned hook topic: nul-topic', rejected.stderr)
        self.assertNotIn('unowned hook topic: (untagged)', rejected.stderr)

    def test_dirty_catalog_cannot_repair_committed_unowned_hook(self):
        self.assertEqual(self.run_git('remote'), '')
        self.write('src/hooks.rs', '// ALAB(unowned): committed without an owner\n')
        self.commit('unowned hook')
        self.catalog(topics=('owned', 'unowned'))
        result = self.inventory()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('unowned hook topic: unowned', result.stderr)
        self.assertNotIn('ownership complete:', result.stdout)

    def test_untracked_implementation_cannot_fill_missing_committed_path(self):
        self.catalog(implementation=('src/not-committed.rs',))
        self.commit('missing implementation')
        self.write('src/not-committed.rs', 'pub fn untracked() {}\n')
        result = self.inventory()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("invalid owned path 'src/not-committed.rs'", result.stderr)
        self.assertNotIn('ownership complete:', result.stdout)

    def test_committed_scalar_policy_files_cannot_report_success(self):
        path = self.repo / 'alab/features.toml'
        path.write_text(path.read_text() + 'policy_files = "unlisted-policy.conf"\n')
        self.commit('malformed scalar policy paths')
        result = self.inventory()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('example: policy_files must be a list of nonempty strings', result.stderr)
        self.assertNotIn('ownership complete:', result.stdout)

    def test_worktree_deletion_does_not_break_selected_commit(self):
        (self.repo / 'src/implementation.rs').unlink()
        self.assert_source_passes(self.inventory(), self.initial)

    def test_staged_hook_does_not_change_default_committed_source(self):
        self.write('src/hooks.rs', '// ALAB(unowned): staged but not committed\n')
        self.run_git('add', 'src/hooks.rs')
        self.assert_source_passes(self.inventory(), self.initial)

    def test_older_source_uses_its_own_catalog_topics_and_paths(self):
        self.write('src/hooks.rs', '// ALAB(newer): next committed fixture hook\n')
        (self.repo / 'src/implementation.rs').unlink()
        self.write('src/newer.rs', 'pub fn newer() {}\n')
        self.catalog(topics=('newer',), implementation=('src/newer.rs',))
        newer = self.commit('newer inventory')
        self.write('alab/features.toml', 'not valid TOML [\n')
        self.assert_source_passes(self.inventory('--source', self.initial), self.initial)
        self.assert_source_passes(self.inventory('--source', newer), newer)
        self.assert_source_passes(self.inventory(), newer)

    def test_annotated_source_tag_uses_its_peeled_commit(self):
        self.run_git('tag', '-a', 'original', '-m', 'original inventory', self.initial)
        self.write('src/hooks.rs', '// ALAB(unowned): newer hook\n')
        self.commit('newer unowned hook')
        self.assert_source_passes(self.inventory('--source', 'original'), self.initial)

    def test_invalid_and_noncommit_sources_do_not_report_success(self):
        tree = self.run_git('rev-parse', 'HEAD^{tree}')
        blob = self.run_git('rev-parse', 'HEAD:src/hooks.rs')
        for source in ('missing-source', tree, blob):
            with self.subTest(source=source):
                result = self.inventory('--source', source)
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn('ownership complete:', result.stdout)

    def test_relative_repo_path_is_resolved_from_calling_directory(self):
        result = self.inventory('--repo', 'repo', cwd=self.parent)
        self.assert_source_passes(result, self.initial)

    def test_committed_directories_and_policy_files_ignore_worktree_deletion(self):
        self.write('policy.conf', 'fixture policy\n')
        self.catalog(implementation=('src',), policy_files=('policy.conf',))
        source = self.commit('directory and policy paths')
        shutil.rmtree(self.repo / 'src')
        (self.repo / 'policy.conf').unlink()
        self.assert_source_passes(self.inventory(), source)

    def test_inherited_git_directory_cannot_redirect_requested_repository(self):
        other = self.parent / 'other'
        other.mkdir()
        self.run_git('init', '-q', '-b', 'main', cwd=other)
        (other / 'unrelated').write_text('another repository\n')
        self.run_git('add', '.', cwd=other)
        self.run_git('commit', '-q', '-m', 'unrelated commit', cwd=other)
        foreign = dict(self.env, GIT_DIR=str(other / '.git'), GIT_WORK_TREE=str(other),
                       GIT_INDEX_FILE=str(other / '.git/index'))
        result = self.inventory('--repo', str(self.repo), cwd=self.parent, env=foreign)
        self.assert_source_passes(result, self.initial)

    def test_replacement_commits_cannot_change_original_catalog_hooks_or_paths(self):
        self.write('src/hooks.rs', '// ALAB(replacement): different hook\n'
                   '// ALAB(second): another replacement hook\n')
        (self.repo / 'src/implementation.rs').unlink()
        self.write('src/replacement.rs', 'pub fn replacement() {}\n')
        self.catalog(topics=('replacement', 'second'), implementation=('src/replacement.rs',))
        replacement = self.commit('replacement tree')
        self.run_git('replace', self.initial, replacement)
        self.assert_source_passes(self.inventory('--source', self.initial), self.initial)

        (self.repo / 'alab/features.toml').unlink()
        self.write('src/hooks.rs', '// ALAB(missing-catalog): cannot affect original\n')
        replacement_without_catalog = self.commit('replacement without a catalog')
        self.run_git('replace', '-f', self.initial, replacement_without_catalog)
        self.assert_source_passes(self.inventory('--source', self.initial), self.initial)

    def test_committed_symlink_counts_without_following_its_worktree_target(self):
        target = self.parent / 'outside-repository'
        link = self.repo / 'src/link'
        try:
            link.symlink_to(target, target_is_directory=True)
        except (OSError, NotImplementedError) as error:
            self.skipTest(f'symlink creation unavailable: {error}')
        self.assertTrue(link.is_symlink())
        self.assertFalse(link.exists())
        self.catalog(implementation=('src/link',))
        source = self.commit('dangling symlink is a tree entry')
        self.assert_source_passes(self.inventory(), source)

        target.mkdir()
        (target / 'child.rs').write_text('pub fn outside() {}\n')
        self.assertTrue((link / 'child.rs').exists())
        self.catalog(implementation=('src/link/child.rs',))
        self.commit('symlink child is not a tree entry')
        result = self.inventory()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("invalid owned path 'src/link/child.rs'", result.stderr)
        self.assertNotIn('ownership complete:', result.stdout)

    def test_missing_or_malformed_committed_catalog_cannot_report_success(self):
        for malformed in (False, True):
            with self.subTest(malformed=malformed):
                path = self.repo / 'alab/features.toml'
                if malformed:
                    path.write_text('not valid TOML [\n')
                else:
                    path.unlink()
                self.commit('invalid committed catalog')
                result = self.inventory()
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn('ownership complete:', result.stdout)


if __name__ == '__main__':
    unittest.main()

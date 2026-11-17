# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import importlib.util
import os
import sys
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location('publication', Path(__file__).parents[1] / 'github_push.py')
PUSH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PUSH)
G = PUSH.GITHUB


def git(repo, *args):
    return G.git(repo, *args).decode().strip()


class Native:
    def __init__(self, remote):
        self.remote = remote
        self.calls = []
        self.failure = None
        self.race = None
        self.transport_refs = None
        self.transport_shallow = None

    def call(self, *args):
        self.calls.append(args)
        assert args[:2] == ('repo', 'push'), args
        self.transport_refs = git(args[3], 'for-each-ref', '--format=%(refname)').splitlines()
        self.transport_shallow = git(args[3], 'rev-parse', '--is-shallow-repository')
        row = json.loads(Path(args[5]).read_text())[0]
        assert git(args[3], 'rev-parse', 'refs/alab/candidate') == row['after']
        if self.race:
            git(self.remote, 'update-ref', 'refs/heads/main', self.race)
        if self.failure:
            error = G.Refused('fixture native failure')
            error.native_error = self.failure
            raise error
        git(self.remote, 'fetch', '--no-tags', args[3], 'refs/alab/candidate')
        git(self.remote, 'update-ref', row['ref'], row['after'], row['before'])
        return b'{"outcome":"created"}\n'

    def api(self, path, **kwargs):
        if '/git/matching-refs/' not in path:
            return {'full_name': 'fixture/codex'}
        return [{'ref': 'refs/heads/main', 'object': {'sha': git(self.remote, 'rev-parse', 'main')}}]

    def fetch_batch(self, identity, repo, rows, **kwargs):
        self.calls.append(('repo', 'fetch', identity,
                           tuple((row['source'], row['into']) for row in rows)))
        git(repo, 'fetch', '--no-tags', str(self.remote),
            *[row['source'] + ':' + row['into'] for row in rows])


class PublicationTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix='alab-publish-test-')
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.repo = self.root / 'source with spaces'
        self.repo.mkdir()
        git(self.repo, 'init', '-q', '-b', 'main')
        git(self.repo, 'config', 'user.name', 'Fixture')
        git(self.repo, 'config', 'user.email', 'fixture@invalid')
        git(self.repo, 'commit', '-q', '--allow-empty', '-m', 'base')
        self.before = git(self.repo, 'rev-parse', 'HEAD')
        self.remote = self.root / 'remote.git'
        git(self.repo, 'clone', '--bare', str(self.repo), str(self.remote))
        git(self.repo, 'remote', 'add', 'origin', 'https://github.com/fixture/codex.git')
        git(self.repo, 'remote', 'add', 'upstream', 'https://github.com/openai/codex.git')
        git(self.repo, 'config', 'remote.upstream.pushurl', 'NEVER-PUSH-TO-UPSTREAM')
        git(self.repo, 'config', 'remote.origin.pushurl', 'NEVER-PUSH-USE-AGH')
        git(self.repo, 'commit', '-q', '--allow-empty', '-m', 'candidate')
        self.after = git(self.repo, 'rev-parse', 'HEAD')
        self.native = Native(self.remote)

    def publish(self):
        return PUSH.publish(self.repo, 'origin', self.before, client=self.native)

    def advance(self):
        git(self.repo, 'checkout', '-q', '--detach', self.before)
        git(self.repo, 'commit', '-q', '--allow-empty', '-m', 'concurrent')
        oid = git(self.repo, 'rev-parse', 'HEAD')
        git(self.remote, 'fetch', '--no-tags', str(self.repo), oid)
        git(self.repo, 'checkout', '-q', 'main')
        return oid

    def test_exact_graph_is_published_and_all_source_guards_remain(self):
        config = (self.repo / '.git/config').read_bytes()
        self.assertIn(b'created', self.publish())
        self.assertEqual(git(self.remote, 'rev-parse', 'main'), self.after)
        self.assertEqual((self.repo / '.git/config').read_bytes(), config)
        self.assertEqual([c[1] for c in self.native.calls], ['fetch', 'push'])

    def test_network_origin_publishes_through_native_client_despite_a_local_url_shadow(self):
        remote_url = git(self.repo, 'config', 'remote.origin.url')
        shadow = self.repo / Path(remote_url)
        shadow.parent.mkdir(parents=True)
        git(self.repo, 'clone', '--bare', str(self.remote), str(shadow))
        config = (self.repo / '.git/config').read_bytes()
        self.assertEqual(git(shadow, 'rev-parse', 'main'), self.before)
        self.publish()
        observed = {'shadow_main': git(shadow, 'rev-parse', 'main'),
                    'intended_remote_main': git(self.remote, 'rev-parse', 'main'),
                    'native_calls': [call[1] for call in self.native.calls]}
        self.assertEqual(observed, {'shadow_main': self.before, 'intended_remote_main': self.after,
                                    'native_calls': ['fetch', 'push']})
        self.assertEqual(self.native.calls[0][:3], ('repo', 'fetch', 'fixture/codex'))
        self.assertEqual(self.native.calls[1][:3], ('repo', 'push', 'fixture/codex'))
        self.assertEqual((self.repo / '.git/config').read_bytes(), config)
        self.assertEqual(git(self.repo, 'rev-parse', 'HEAD'), self.after)

    def test_transport_has_complete_candidate_history_and_only_authoritative_main(self):
        unrelated = self.advance()
        git(self.remote, 'update-ref', 'refs/heads/unrelated', unrelated)
        git(self.remote, 'update-ref', 'refs/tags/unrelated', unrelated)
        git(self.remote, 'symbolic-ref', 'HEAD', 'refs/heads/unrelated')
        self.publish()
        self.assertEqual(self.native.transport_shallow, 'false')
        self.assertEqual([ref for ref in self.native.transport_refs
                          if not ref.startswith('refs/agh/')],
                         ['refs/alab/candidate', 'refs/remotes/origin/main'])
        self.assertEqual(self.native.calls[0][:3], ('repo', 'fetch', 'fixture/codex'))
        self.assertEqual([source for source, _into in self.native.calls[0][3]],
                         ['refs/heads/main'])

    def test_a_candidate_lost_during_local_transfer_refuses_before_any_remote_request(self):
        run_git = G.git

        def lose_candidate_at_local_transfer(repo, *args, **kwargs):
            if args[0] == 'fetch' and str(self.repo.resolve()) in args:
                (self.repo / '.git/objects' / self.after[:2] / self.after[2:]).unlink()
            return run_git(repo, *args, **kwargs)

        with mock.patch.object(G, 'git', side_effect=lose_candidate_at_local_transfer):
            with self.assertRaises(G.Refused): self.publish()
        self.assertEqual(self.native.calls, [])

    def test_local_publication_keeps_the_validated_commit_when_head_moves(self):
        git(self.repo, 'remote', 'set-url', 'origin', str(self.remote))
        run_git = G.git

        def move_head_before_push(repo, *args, **kwargs):
            if args[0] == 'push':
                run_git(self.repo, 'commit', '-q', '--allow-empty', '-m', 'later work')
            return run_git(repo, *args, **kwargs)

        with mock.patch.object(G, 'git', side_effect=move_head_before_push):
            self.publish()
        self.assertNotEqual(git(self.repo, 'rev-parse', 'HEAD'), self.after)
        self.assertEqual(git(self.remote, 'rev-parse', 'main'), self.after)
        self.assertEqual(self.native.calls, [])

    def test_advance_before_fetch_is_a_proven_race_without_push(self):
        git(self.remote, 'update-ref', 'refs/heads/main', self.advance())
        with self.assertRaises(PUSH.Advanced): self.publish()
        self.assertEqual(len(self.native.calls), 1)

    def test_typed_lease_conflict_requires_a_confirmed_forward_advance(self):
        self.native.failure = dict(v=1, exit=9, kind='conflict', side_effect='none')
        self.native.race = self.advance()
        with self.assertRaises(PUSH.Advanced): self.publish()

    def test_auth_network_unknown_effect_and_untyped_failures_are_not_races(self):
        for failure in [dict(v=1, exit=4, kind='unauthenticated', side_effect='none'),
                        dict(v=1, exit=3, kind='could_not_run', side_effect='none'),
                        dict(v=1, exit=9, kind='conflict', side_effect='unknown'),
                        dict(v=0, exit=9, kind='conflict', side_effect='none')]:
            with self.subTest(failure=failure):
                git(self.remote, 'update-ref', 'refs/heads/main', self.before)
                self.native.failure = failure
                self.native.race = self.advance()
                with self.assertRaises(G.Refused) as caught: self.publish()
                self.assertNotIsInstance(caught.exception, PUSH.Advanced)

    def test_typed_conflict_without_main_movement_is_not_a_race(self):
        self.native.failure = dict(v=1, exit=9, kind='conflict', side_effect='none')
        with self.assertRaises(G.Refused) as caught: self.publish()
        self.assertNotIsInstance(caught.exception, PUSH.Advanced)

    def test_rewritten_remote_or_nonforward_candidate_refuses(self):
        git(self.repo, 'checkout', '-q', '--orphan', 'rewritten')
        git(self.repo, 'commit', '-q', '--allow-empty', '-m', 'different history')
        rewritten = git(self.repo, 'rev-parse', 'HEAD')
        with self.assertRaises(G.Refused): self.publish()
        git(self.remote, 'fetch', '--no-tags', str(self.repo), rewritten)
        git(self.remote, 'update-ref', 'refs/heads/main', rewritten)
        git(self.repo, 'checkout', '-q', 'main')
        with self.assertRaises((G.Refused, subprocess.CalledProcessError)): self.publish()


class PublicationPreparationTests(unittest.TestCase):
    """Only the selected local preparation command fails; authority stays inert."""

    CHILD = r"""
import ast, hashlib, importlib.util, json, os, socket, subprocess, sys, threading
from pathlib import Path
from unittest import mock
source, repo, before, after, stage, mode, stderr_hex, witness_path = sys.argv[1:]
source, repo, witness_path = Path(source), Path(repo), Path(witness_path)
spec = importlib.util.spec_from_file_location('owned_publication', source)
push = importlib.util.module_from_spec(spec)
spec.loader.exec_module(push)
g = push.GITHUB
paths = [source, source.with_name('github.py')]
def hashes():
    return {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in paths}
witness = {'source_before': hashes(), 'original_guard': mode == 'guard',
           'authority_fetch': 0, 'native_fetch': 0, 'native_push': 0,
           'native_api': 0, 'agh_construct': 0, 'actual_fetch_push': 0,
           'network': 0, 'thread': 0, 'signal': 0, 'replaced': 0,
           'local_git': []}
def forbidden(counter):
    def stop(*args, **kwargs):
        witness[counter] += 1
        raise AssertionError('unexpected controlled publication boundary')
    return stop
class Inert:
    api = staticmethod(forbidden('native_api'))
    fetch_batch = staticmethod(forbidden('native_fetch'))
    call = staticmethod(forbidden('native_push'))
client = Inert()
real_run = subprocess.run
payload = bytes.fromhex(stderr_hex)
def run(argv, **kwargs):
    assert argv[0] == 'git', 'only owned local Git children are admitted'
    command = argv[argv.index('-C') + 2:]
    selected = ((stage == 'publication-init' and command[:3] == ['init', '--bare', '--quiet'])
                or (stage == 'publication-candidate-fetch' and command == [
                    'fetch', '--no-tags', '--no-write-fetch-head', '--recurse-submodules=no',
                    str(repo.resolve()), after + ':refs/alab/candidate']))
    if selected:
        witness['replaced'] += 1
        assert witness['replaced'] == 1
        # These untrusted fields must never reach a refusal or stdout.
        return subprocess.CompletedProcess(['argv-secret', 'https://sensitive.invalid/secret'],
                                           128, b'PRIVATE-STDOUT-SENTINEL', payload)
    if command[0] in ('fetch', 'push'):
        witness['actual_fetch_push'] += 1
        raise AssertionError('no transfer is admitted by this negative fixture')
    witness['local_git'].append(command[0])
    return real_run(argv, **kwargs)
original_publish = push.publish
def supplied_client(repo, remote, expected):
    return original_publish(repo, remote, expected, client=client)
arguments = [str(source), '--repo', str(repo), '--expected', before, 'origin']
sys.argv = arguments
try:
    with mock.patch.object(g.subprocess, 'run', side_effect=run), \
         mock.patch.object(g, 'fetch', side_effect=forbidden('authority_fetch')), \
         mock.patch.object(g, 'Agh', side_effect=forbidden('agh_construct')), \
         mock.patch.object(socket, 'socket', side_effect=forbidden('network')), \
         mock.patch.object(threading.Thread, 'start', side_effect=forbidden('thread')), \
         mock.patch.object(os, 'kill', side_effect=forbidden('signal')):
        if mode == 'publish':
            try:
                original_publish(repo, 'origin', before, client=client)
            except g.Refused as error:
                witness.update(exception_type=type(error).__name__, message=str(error),
                               native_error=error.native_error,
                               advanced=isinstance(error, push.Advanced))
            else:
                raise AssertionError('preparation unexpectedly returned')
        else:
            # Keep main and its original guard, replacing only dependency injection.
            with mock.patch.object(push, 'publish', side_effect=supplied_client):
                if mode == 'main':
                    status = push.main()
                    witness['main_return'] = status
                    raise SystemExit(status)
                tree = ast.parse(source.read_bytes(), filename=str(source))
                entries = [node for node in tree.body if isinstance(node, ast.If)
                           and isinstance(node.test, ast.Compare)
                           and isinstance(node.test.left, ast.Name)
                           and node.test.left.id == '__name__']
                assert len(entries) == 1
                entry = ast.Module(body=entries, type_ignores=[])
                witness['guard_ast_sha256'] = hashlib.sha256(ast.dump(entry).encode()).hexdigest()
                push.__dict__['__name__'] = '__main__'
                exec(compile(entry, str(source), 'exec'), push.__dict__)
finally:
    witness['source_after'] = hashes()
    witness_path.write_text(json.dumps(witness))
"""

    STAGES = {
        'publication-init': 'publication transport initialization',
        'publication-candidate-fetch': 'publication candidate preparation',
    }
    SOURCE = Path(__file__).resolve().parents[1] / 'github_push.py'

    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix='alab-preparation-test-')
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name).resolve()
        self.repo = self.root / 'private source with spaces'
        self.repo.mkdir()
        git(self.repo, 'init', '-q', '-b', 'main')
        git(self.repo, 'config', 'user.name', 'Fixture')
        git(self.repo, 'config', 'user.email', 'fixture@invalid')
        git(self.repo, 'commit', '-q', '--allow-empty', '-m', 'base')
        self.before = git(self.repo, 'rev-parse', 'HEAD')
        git(self.repo, 'remote', 'add', 'origin', 'https://github.com/fixture/codex.git')
        git(self.repo, 'commit', '-q', '--allow-empty', '-m', 'candidate')
        self.after = git(self.repo, 'rev-parse', 'HEAD')
        self.sequence = 0

    def snapshot(self):
        return {'head': git(self.repo, 'rev-parse', 'HEAD'),
                'refs': git(self.repo, 'for-each-ref', '--format=%(refname) %(objectname)'),
                'config': (self.repo / '.git/config').read_bytes()}

    def child(self, stage, payload, mode, *, source=None):
        self.sequence += 1
        script = self.root / ('child-%d.py' % self.sequence)
        receipt = self.root / ('child-%d.json' % self.sequence)
        script.write_text(self.CHILD)
        before = self.snapshot()
        done = subprocess.run([
            sys.executable, '-I', '-B', str(script), str(source or self.SOURCE),
            str(self.repo), self.before, self.after, stage, mode, payload.hex(), str(receipt)],
            capture_output=True, text=True, timeout=20, cwd=self.root)
        witness = json.loads(receipt.read_text())
        self.assertEqual(self.snapshot(), before)
        self.assertEqual(witness['source_before'], witness['source_after'])
        self.assertEqual(witness['replaced'], 1)
        for counter in ('authority_fetch', 'native_fetch', 'native_push', 'native_api',
                        'agh_construct', 'actual_fetch_push', 'network', 'thread', 'signal'):
            self.assertEqual(witness[counter], 0, counter)
        self.assertEqual(witness['local_git'],
                         ['rev-parse', 'merge-base', 'config']
                         + (['init'] if stage == 'publication-candidate-fetch' else []))
        self.assertEqual(done.stdout, '')
        for sentinel in ('PRIVATE', 'sensitive.invalid', 'argv-secret', str(self.repo),
                         'fetch first', '[rejected]'):
            self.assertNotIn(sentinel, done.stderr)
        return done, witness

    def test_public_publish_main_and_original_guard_expose_only_fixed_preparation_diagnostics(self):
        for stage, label in self.STAGES.items():
            for mode in ('publish', 'main', 'guard'):
                for payload, no_space in (
                    (b'PRIVATE stderr /secret https://sensitive.invalid: No space left on device', True),
                    (b'PRIVATE arbitrary output and URL https://sensitive.invalid', False),
                    (b'PRIVATE No space left on device\xff', False),
                ):
                    with self.subTest(stage=stage, mode=mode, payload=payload):
                        done, witness = self.child(stage, payload, mode)
                        message = 'local Git ' + label + ' failed (exit 128)'
                        if no_space:
                            message += ': Git reported no space left on device'
                        if mode == 'publish':
                            self.assertEqual(done.returncode, 0, done.stderr)
                            self.assertEqual(witness['message'], message)
                            self.assertEqual(witness['exception_type'], 'Refused')
                            self.assertFalse(witness['advanced'])
                            self.assertIsNone(witness['native_error'])
                            self.assertEqual(done.stderr, '')
                        else:
                            self.assertEqual(done.returncode, 1, done.stderr)
                            self.assertEqual(done.stderr, 'ALab publication refused: ' + message + '\n')
                            if mode == 'main':
                                self.assertEqual(witness['main_return'], 1)
                            else:
                                self.assertEqual(len(witness['guard_ast_sha256']), 64)

    def test_stderr_tail_and_utf8_boundaries_fail_closed_without_child_text_disclosure(self):
        phrase = b'No space left on device'
        for payload, expected in (
            (phrase + b'x' * 4096, False),
            (phrase + b'x' * (4096 - len(phrase) + 1), False),
            ('Kein Speicherplatz auf dem Gerät'.encode(), False),
            (b'\xc3\xa9' + b'x' * (4095 - len(phrase)) + phrase, False),
            (phrase + b'x' * (4096 - len(phrase)), True),
            (b'\xff' + phrase + b'x' * (4096 - len(phrase)), True),
            ('No space left on device', False),
            (None, False),
        ):
            with self.subTest(payload_type=type(payload).__name__, expected=expected):
                with mock.patch.object(G.subprocess, 'run', return_value=
                                       subprocess.CompletedProcess([], 128, b'PRIVATE', payload)):
                    with self.assertRaises(G.Refused) as caught:
                        G.git(self.repo, 'fetch', failure_stage='publication-candidate-fetch')
                message = 'local Git publication candidate preparation failed (exit 128)'
                if expected:
                    message += ': Git reported no space left on device'
                self.assertEqual(str(caught.exception), message)

    def test_default_failure_and_allowed_codes_preserve_the_original_contract(self):
        for stage in (None, *self.STAGES):
            with self.subTest(stage=stage):
                output = b'owned original stdout\x00\xff'
                class Allowed:
                    returncode = 1
                    stdout = output
                    @property
                    def stderr(self):
                        raise AssertionError('allowed result stderr must not be inspected')
                with mock.patch.object(G.subprocess, 'run', return_value=Allowed()) as run:
                    self.assertIs(G.git(self.repo, 'config', '--get', 'owned.absent',
                                        codes=(0, 1), failure_stage=stage), output)
                self.assertEqual(run.call_args.kwargs['timeout'], 120)
                self.assertTrue(run.call_args.kwargs['capture_output'])
                self.assertIsNone(run.call_args.kwargs['input'])
                argv = run.call_args.args[0]
                self.assertEqual(argv[-5:], ['-C', str(self.repo), 'config', '--get', 'owned.absent'])
        with mock.patch.object(G.subprocess, 'run', return_value=
                               subprocess.CompletedProcess([], 128, b'PRIVATE',
                                                           b'No space left on device')):
            with self.assertRaises(G.Refused) as caught:
                G.git(self.repo, 'config', '--get', 'owned.absent')
        self.assertEqual(str(caught.exception), 'local Git config failed (exit 128)')
        self.assertIsNone(caught.exception.native_error)

    def test_unknown_stages_refuse_before_subprocess_or_foreign_callbacks(self):
        class Foreign:
            def __str__(self):
                raise AssertionError('stage text callback')
            def __hash__(self):
                raise AssertionError('stage hash callback')
            def __eq__(self, _other):
                raise AssertionError('stage equality callback')
        class Stage(str):
            pass
        for stage in ('PRIVATE-unknown', '', 0, [], Foreign(), Stage('publication-init')):
            with self.subTest(stage_type=type(stage).__name__):
                with mock.patch.object(G.subprocess, 'run') as run:
                    with self.assertRaises(G.Refused) as caught:
                        G.git(self.repo, 'init', failure_stage=stage)
                run.assert_not_called()
                self.assertEqual(str(caught.exception), 'unsupported local Git failure stage')
                self.assertIsNone(caught.exception.native_error)

    def test_child_launch_exceptions_keep_identity(self):
        for stage in (None, *self.STAGES):
            for error in (OSError(28, 'controlled local failure'),
                          subprocess.TimeoutExpired('owned', 120), KeyboardInterrupt(),
                          SystemExit(0), SystemExit(47)):
                with self.subTest(stage=stage, error_type=type(error).__name__):
                    with mock.patch.object(G.subprocess, 'run', side_effect=error):
                        try:
                            G.git(self.repo, 'init', failure_stage=stage)
                        except BaseException as caught:
                            self.assertIs(caught, error)
                        else:
                            self.fail('child exception unexpectedly returned')


if __name__ == '__main__': unittest.main()

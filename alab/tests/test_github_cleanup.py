# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Public scratch owners preserve failures while retiring only real owned directories."""
from contextlib import ExitStack
import importlib.util
import json
import os
from pathlib import Path
import select
import shlex
import shutil
import signal
import subprocess
import sys
import unittest
from unittest.mock import patch

from test_github import disk_fixture_cli
import test_github_push as publication_tests

SPEC = importlib.util.spec_from_file_location('cleanup_versions',
                                             Path(__file__).parents[1] / 'versions.py')
VERSIONS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERSIONS)


class ReportingFailure(RuntimeError):
    def __init__(self, fault):
        super().__init__('primary body failure')
        self.fault = fault

    def add_note(self, _note):
        raise self.fault


class UnrepresentableCleanup(OSError):
    def __init__(self, fault):
        super().__init__('owned cleanup failure')
        self.fault = fault

    def __repr__(self):
        raise self.fault


class GithubCleanupTests(unittest.TestCase):
    def setUp(self):
        publication_tests.PublicationTests.setUp(self)
        self.empty_releases = self.root / 'empty releases.git'
        publication_tests.git(self.root, 'init', '--quiet', '--bare', str(self.empty_releases))

    @unittest.skipUnless(os.name == 'posix', 'POSIX publication cancellation')
    def test_real_sigterm_retires_staged_publication_before_push(self):
        import fcntl
        # Git keeps a fetch of at least 100 objects as a pack in the independent transport.
        for index in range(128):
            (self.repo / f'blob-{index}').write_text(f'candidate object {index}\n')
        publication_tests.git(self.repo, 'add', '--all')
        publication_tests.git(self.repo, 'commit', '--quiet', '-m', 'packed candidate')
        scratch = self.root / 'publication'
        scratch.mkdir()
        neighbor = scratch / 'unrelated'
        neighbor.write_bytes(b'keep this')
        ready, release = self.root / 'ready', self.root / 'release'
        for path in (ready, release):
            os.mkfifo(path, 0o600)
        lock = self.root / 'reader.lock'
        reader = self.root / 'reader.py'
        reader.write_text(
            'import fcntl, sys\n'
            'from pathlib import Path\n'
            'root = Path(sys.argv[1])\n'
            'with (root / "reader.lock").open("a") as lock:\n'
            '    fcntl.flock(lock, fcntl.LOCK_EX)\n'
            '    with (root / "ready").open("wb", buffering=0) as ready:\n'
            '        ready.write(b"r")\n'
            '    with (root / "release").open("rb", buffering=0) as release:\n'
            '        release.read(1)\n'
            '    if not any((root / "publication").glob("*/transport.git/objects/pack/*.pack")):\n'
            '        (root / "removed-during-read").touch()\n')
        real_git = shutil.which('git')
        shim = self.root / 'git'
        shim.write_text(
            '#!/bin/sh\nfor arg do\n  if [ "$arg" = ls-remote ]; then\n    '
            + shlex.join([sys.executable, '-I', '-B', str(reader), str(self.root)])
            + ' || exit $?\n    break\n  fi\ndone\nexec ' + shlex.quote(real_git) + ' "$@"\n')
        shim.chmod(0o700)
        env = {**os.environ, 'TMPDIR': str(scratch),
               'PATH': str(self.root) + os.pathsep + os.environ['PATH']}
        # The first local ls-remote reads the destination after the candidate fetch
        # and before any push. Its grandchild holds the lifetime of that read.
        with ExitStack() as stack:
            incoming, outgoing = [os.open(path, os.O_RDWR | os.O_NONBLOCK)
                                  for path in (ready, release)]
            for descriptor in (incoming, outgoing):
                stack.callback(os.close, descriptor)
            cli = subprocess.Popen(
                [sys.executable, '-I', '-B', '-c', disk_fixture_cli(Path(__file__).parents[1] / 'github_push.py', 24),
                 '--repo', str(self.repo), '--expected', self.before, str(self.remote)],
                env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
            try:
                self.assertTrue(select.select([incoming], [], [], 60)[0], 'remote read never began')
                self.assertEqual(os.read(incoming, 1), b'r')
                packs = list(scratch.glob('*/transport.git/objects/pack/*.pack'))
                size = sum(path.stat().st_size for path in packs)
                self.assertGreater(size, 0, 'candidate graph was not staged')
                cli.send_signal(signal.SIGTERM)
                os.write(outgoing, b'x')
                _, stderr = cli.communicate(timeout=60)
            finally:
                os.write(outgoing, b'x')
                if cli.poll() is None:
                    os.killpg(cli.pid, signal.SIGKILL)
                    cli.communicate()
            self.assertIn(cli.returncode, (-signal.SIGTERM, 128 + signal.SIGTERM), stderr)
            self.assertEqual(publication_tests.git(self.remote, 'rev-parse', 'main'), self.before)
            self.assertEqual(neighbor.read_bytes(), b'keep this')
            self.assertEqual(list(scratch.iterdir()), [neighbor], f'SIGTERM left a staged {size}-byte pack')
            with lock.open('rb') as holder:
                fcntl.flock(holder, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.assertFalse((self.root / 'removed-during-read').exists())

    @unittest.skipUnless(os.name == 'posix', 'POSIX transport process groups')
    def test_timeout_retires_writer_holding_stdout_after_git_parent_exits(self):
        import fcntl

        release = self.root / 'timeout-release'
        os.mkfifo(release, 0o600)
        lock = self.root / 'timeout-writer.lock'
        child = (
            'import fcntl, os, sys\n'
            'if os.fork(): os._exit(0)\n'
            'with open(sys.argv[1], "w") as holder:\n'
            '    fcntl.flock(holder, fcntl.LOCK_EX)\n'
            '    print("writer is active", flush=True)\n'
            '    with open(sys.argv[2], "rb", buffering=0) as gate: gate.read(1)\n')
        communicate = subprocess.Popen.communicate

        def when_writer_is_ready(process, *args, **kwargs):
            # Arm the real short timeout only after the grandchild acquired its lock.
            self.assertTrue(select.select([process.stdout], [], [], 60)[0])
            return communicate(process, *args, **kwargs)

        rescue = os.open(release, os.O_RDWR | os.O_NONBLOCK)
        try:
            with self.assertRaises(subprocess.TimeoutExpired) as caught:
                with publication_tests.G.temporary_directory(dir=self.root) as scratch:
                    with patch.object(subprocess.Popen, 'communicate', when_writer_is_ready):
                        publication_tests.G.run_transport(
                            [sys.executable, '-I', '-B', '-c', child, str(lock), str(release)],
                            capture_output=True, timeout=0.1)
            self.assertIn(b'writer is active', caught.exception.output)
            self.assertFalse(Path(scratch).exists())
            with lock.open('rb') as holder:
                fcntl.flock(holder, fcntl.LOCK_EX | fcntl.LOCK_NB)
        finally:
            os.write(rescue, b'x')
            os.close(rescue)

    def test_cleanup_preserves_a_replacement_directory(self):
        github = publication_tests.G
        with self.assertRaisesRegex(github.Refused, 'ownership changed'):
            with github.temporary_directory(dir=self.root) as scratch:
                original = Path(scratch)
                saved = self.root / 'renamed'
                original.rename(saved)
                original.mkdir()
                (original / 'neighbor').write_bytes(b'keep this')
        self.assertEqual((original / 'neighbor').read_bytes(), b'keep this')
        self.assertTrue(saved.is_dir())

    def invoke(self, consumer, primary=None, cleanup_fault=None, creation_fault=None):
        created, cleaned, bodies = [], [], []

        github = VERSIONS.GITHUB if consumer == 'released' else publication_tests.G
        original_owner = github._ScratchDirectory

        class OwnedDirectory(original_owner):
            def __init__(self, **keywords):
                if creation_fault is not None:
                    raise creation_fault
                super().__init__(**keywords)
                created.append(self)

            def cleanup(self):
                super().cleanup()
                cleaned.append(self.name)
                if cleanup_fault is not None:
                    raise cleanup_fault

        with ExitStack() as patches:
            patches.enter_context(patch.object(github, '_ScratchDirectory', OwnedDirectory))
            if consumer == 'fetch_batch':
                client = object.__new__(github.Agh)
                rows = [dict(source='refs/heads/main', into='refs/agh/owned', expected_oid=self.after)]

                def call(*arguments, **_keywords):
                    bodies.append('fetch_batch')
                    self.assertEqual(json.loads(Path(arguments[5]).read_text()), rows)
                    if primary is not None:
                        raise primary
                    return b'{}'

                patches.enter_context(patch.object(client, 'call', side_effect=call))
                action = lambda: client.fetch_batch('fixture/codex', self.repo, rows)
            elif consumer == 'publish':
                actual_git = github.git

                def git(repo, *arguments, **keywords):
                    if arguments[0] == 'push':
                        bodies.append('publish')
                        if primary is not None:
                            raise primary
                    return actual_git(repo, *arguments, **keywords)

                patches.enter_context(patch.object(github, 'git', side_effect=git))
                action = lambda: publication_tests.PUSH.publish(
                    self.repo, str(self.remote), self.before, client=self.native)
            else:
                actual_list = github.list_refs

                def list_refs(*arguments, **keywords):
                    value = actual_list(*arguments, **keywords)
                    bodies.append('released')
                    self.assertEqual(value, {})
                    if primary is not None:
                        raise primary
                    return value

                patches.enter_context(patch.object(github, 'list_refs', side_effect=list_refs))
                action = lambda: VERSIONS.released(self.repo, str(self.empty_releases), self.before)
            try:
                return action()
            finally:
                leaked = []
                for owner in created:
                    if Path(owner.name).exists():
                        leaked.append(owner.name)
                        original_owner.cleanup(owner)
                self.assertEqual(leaked, [])
                self.assertEqual(cleaned, [owner.name for owner in created])
                self.assertEqual(len(created), 0 if creation_fault is not None else 1)
                self.assertEqual(len(bodies), 0 if creation_fault is not None else 1)

    def assert_escapes(self, expected, consumer, **faults):
        with self.assertRaises(BaseException) as caught:
            self.invoke(consumer, **faults)
        self.assertTrue(caught.exception is expected, 'the exact selected exception must escape')
        return caught.exception

    def test_successful_public_contexts_remove_their_directory_once(self):
        for consumer in ('fetch_batch', 'publish', 'released'):
            with self.subTest(consumer=consumer):
                self.assertIsNotNone(self.invoke(consumer))

    def test_clean_cleanup_preserves_primary_identity(self):
        for consumer in ('fetch_batch', 'publish', 'released'):
            for primary in (KeyboardInterrupt('body cancellation'), RuntimeError('body failure')):
                with self.subTest(consumer=consumer, primary=type(primary).__name__):
                    self.assert_escapes(primary, consumer, primary=primary)

    def test_pending_cancellation_survives_ordinary_cleanup_failure(self):
        for consumer in ('fetch_batch', 'publish', 'released'):
            for primary in (KeyboardInterrupt('body cancellation'), SystemExit(47)):
                with self.subTest(consumer=consumer, primary=type(primary).__name__):
                    cleanup = OSError('owned cleanup failure')
                    self.assert_escapes(primary, consumer, primary=primary,
                                        cleanup_fault=cleanup)
                    self.assertIn('temporary directory cleanup failed: ' + repr(cleanup), primary.__notes__)

    def test_pending_cancellation_beats_later_cleanup_cancellation(self):
        for consumer in ('fetch_batch', 'publish', 'released'):
            for primary, cleanup in ((KeyboardInterrupt('body cancellation'), SystemExit(47)),
                                     (SystemExit(47), KeyboardInterrupt('cleanup cancellation'))):
                with self.subTest(consumer=consumer, primary=type(primary).__name__):
                    self.assert_escapes(primary, consumer, primary=primary, cleanup_fault=cleanup)
                    self.assertIn('temporary directory cleanup failed: ' + repr(cleanup), primary.__notes__)

    def test_zero_and_none_system_exit_survive_cleanup_failure(self):
        for consumer in ('fetch_batch', 'publish', 'released'):
            for code in (0, None):
                with self.subTest(consumer=consumer, code=code):
                    primary = SystemExit(code)
                    self.assert_escapes(primary, consumer, primary=primary,
                                        cleanup_fault=OSError('owned cleanup failure'))

    def test_ordinary_primary_survives_with_natural_secondary_note(self):
        for consumer in ('fetch_batch', 'publish', 'released'):
            with self.subTest(consumer=consumer):
                primary, cleanup = RuntimeError('body failure'), OSError('owned cleanup failure')
                self.assert_escapes(primary, consumer, primary=primary, cleanup_fault=cleanup)
                self.assertIn('temporary directory cleanup failed: ' + repr(cleanup), primary.__notes__)

    def test_cleanup_cancellation_promotes_over_ordinary_primary(self):
        cases = (('fetch_batch', KeyboardInterrupt('cleanup cancellation')),
                 ('publish', SystemExit(0)), ('released', SystemExit(None)))
        for consumer, cancellation in cases:
            with self.subTest(consumer=consumer):
                primary = RuntimeError('body failure')
                caught = self.assert_escapes(cancellation, consumer, primary=primary,
                                             cleanup_fault=cancellation)
                self.assertIs(caught.__cause__, primary)

    def test_successful_body_cleanup_failure_remains_visible(self):
        for consumer in ('fetch_batch', 'publish', 'released'):
            with self.subTest(consumer=consumer):
                cleanup = OSError('owned cleanup failure')
                self.assert_escapes(cleanup, consumer, cleanup_fault=cleanup)

    def test_creation_failure_starts_no_body_and_owns_no_directory(self):
        for consumer in ('fetch_batch', 'publish', 'released'):
            with self.subTest(consumer=consumer):
                failure = OSError('owned scratch creation failed')
                self.assert_escapes(failure, consumer, creation_fault=failure)

    def test_cleanup_repr_cancellation_cannot_replace_pending_cancellation(self):
        primary = KeyboardInterrupt('body cancellation')
        self.assert_escapes(primary, 'fetch_batch', primary=primary,
                            cleanup_fault=UnrepresentableCleanup(SystemExit(47)))

    def test_reporting_cancellation_promotes_over_ordinary_primary(self):
        cancellation = SystemExit(0)
        primary = ReportingFailure(cancellation)
        caught = self.assert_escapes(cancellation, 'fetch_batch', primary=primary,
                                     cleanup_fault=OSError('owned cleanup failure'))
        self.assertIs(caught.__cause__, primary)

    def test_ordinary_reporting_failure_preserves_primary(self):
        primary = ReportingFailure(RuntimeError('note failure'))
        self.assert_escapes(primary, 'fetch_batch', primary=primary,
                            cleanup_fault=OSError('owned cleanup failure'))


if __name__ == '__main__':
    unittest.main()

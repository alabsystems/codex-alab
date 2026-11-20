# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise the disk observer while real Git and captured Agh processes are writing."""
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
import json
import os
from pathlib import Path
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

import test_github as fixture


HANG_SECONDS = 60
TRANSFER = r'''
import fcntl
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

root = Path(os.environ["FETCH_FIXTURE_ROOT"])
mode = os.environ["FETCH_FIXTURE_MODE"]
signal.alarm(int(os.environ["FETCH_FIXTURE_TIMEOUT"]))

def writer():
    with (root / "writer.lock").open("wb") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        (root / "ready").touch()
        if mode in ("raw-exit", "spontaneous", "parent-term"):
            while not (root / "release").exists():
                time.sleep(0.01)
            (Path(os.environ["TMPDIR"]) / "writer-finished").touch()
        elif mode in ("cli-timeout", "cli-term"):
            while not (root / "release").exists():
                time.sleep(0.01)
        else:
            signal.pause()

if "help" in sys.argv:
    print("--ref --into --input --anonymous")
elif "public-refs" in sys.argv:
    print(os.environ["FETCH_FIXTURE_HEAD"] + "\trefs/heads/main")
elif "repo" not in sys.argv:
    writer()  # Local Git upload-pack, or Agh's separately grouped Git stand-in.
else:
    scratch = Path(os.environ["TMPDIR"])
    (root / "scratch").write_text(str(scratch))
    (scratch / "owned.pack").touch()
    if mode == "typed-failure":
        print(json.dumps({"error": {"v": 1, "exit": 3, "kind": "could_not_run",
                                    "side_effect": "none", "why": "fixture fetch refused"}}),
              file=sys.stderr)
        sys.exit(3)
    if mode == "output":
        sys.stdout.buffer.write(b"a" * 262144)
        sys.stdout.buffer.flush()
        sys.stderr.buffer.write(b" " * 262144)
        sys.stderr.buffer.flush()
        (root / "ready").touch()
        while not (root / "release").exists():
            time.sleep(0.01)
        sys.stdout.buffer.write(b"b" * 262144)
        rows = json.loads(Path(sys.argv[sys.argv.index("--input") + 1]).read_text())
        repo = sys.argv[sys.argv.index("fetch") + 2]
        for row in rows:
            subprocess.run([os.environ["FETCH_FIXTURE_GIT"], "-C", repo, "update-ref",
                            row["into"], row["expected_oid"]], check=True)
    elif mode in ("raw-exit", "spontaneous", "parent-term"):
        # A raw frontend exit is no receipt for its independent Git writer.
        child = subprocess.Popen([sys.executable, "-IB", __file__, "--writer"],
                                 start_new_session=True, stdin=subprocess.DEVNULL,
                                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if mode != "raw-exit":
            while not (root / "ready").is_file():
                time.sleep(0.01)
            if mode == "parent-term":
                os.kill(os.getppid(), signal.SIGTERM)
            os.kill(os.getpid(), signal.SIGKILL)
        child.wait()
    else:
        class Stop(Exception):
            pass

        def stop(_signum, _frame):
            raise Stop()

        signal.signal(signal.SIGTERM, stop)
        child = subprocess.Popen([sys.executable, "-IB", __file__, "--writer"],
                                 start_new_session=True)
        try:
            if mode == "cli-term":
                while not (root / "ready").is_file():
                    time.sleep(0.01)
                os.kill(os.getppid(), signal.SIGTERM)
            child.wait()
        except Stop:
            child.terminate()
            child.wait()
            (root / "settled").touch()
            print(json.dumps({"error": {"v": 1, "exit": 130, "kind": "interrupted",
                  "sub": None, "side_effect": "none", "retriable": False,
                  "why": "native Git operation interrupted"}}),
                  file=sys.stderr)
            sys.exit(130)
        finally:
            if child.poll() is None:
                child.kill()
                child.wait()
'''


@unittest.skipUnless(os.name == "posix", "requires POSIX Git process groups and flock")
class FetchPressureTests(unittest.TestCase):
    def setUp(self):
        world = fixture.GithubTests()
        world.setUp()
        self.addCleanup(world.doCleanups)
        self.repo, self.remote, self.root = world.repo, world.remote, world.home
        self.target = "refs/remotes/upstream/main"
        self.old = world.head
        fixture.git(self.repo, "update-ref", self.target, self.old)
        fixture.git(self.repo, "commit", "-q", "--allow-empty", "-m", "next")
        self.new = fixture.git(self.repo, "rev-parse", "HEAD")
        fixture.git(self.repo, "push", "-q", str(self.remote), "HEAD:refs/heads/main")
        self.temp = self.root / "staging"
        self.temp.mkdir()
        self.cli = self.python_tool("transfer", TRANSFER)
        env = mock.patch.dict(os.environ, {"ALAB_AGH": str(self.cli),
                              "FETCH_FIXTURE_ROOT": str(self.root),
                              "FETCH_FIXTURE_MODE": "native",
                              "FETCH_FIXTURE_GIT": shutil.which("git"),
                              "FETCH_FIXTURE_TIMEOUT": str(HANG_SECONDS)})
        env.start()
        self.addCleanup(env.stop)
        temp = mock.patch.object(tempfile, "tempdir", str(self.temp))
        temp.start()
        self.addCleanup(temp.stop)

    def python_tool(self, name, source):
        script = self.root / (name + ".py")
        script.write_text(source)
        # exec preserves the PID even when the interpreter's path contains spaces.
        tool = self.root / name
        command = shlex.join([sys.executable, "-IB", str(script)])
        tool.write_text(f'#!/bin/sh\nexec {command} "$@"\n')
        tool.chmod(0o700)
        return tool

    def fetch(self, remote, check, client=None):
        return fixture.GITHUB.fetch(self.repo, remote, ["refs/heads/main:" + self.target],
                                    snapshot={"refs/heads/main": self.new},
                                    before_transfer=check, client=client)

    def refuse_when_writing(self, common, scratch):
        self.assertEqual(Path(common), (self.repo / ".git").resolve())
        if scratch is not None:
            self.assertEqual(Path(scratch).resolve(), self.temp.resolve())
        if (self.root / "ready").is_file():
            raise RuntimeError("fixture fetch floor reached")

    def assert_stopped(self):
        import fcntl

        self.assertEqual(fixture.git(self.repo, "rev-parse", self.target), self.old)
        with (self.root / "writer.lock").open("rb") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)

    def assert_refuses_before_writer(self):
        popen = subprocess.Popen

        def read_only(argv, *args, **kwargs):
            if "fetch" in argv and ("--input" in argv or "--no-write-fetch-head" in argv):
                raise OSError(f"guarded object writer reached Popen: {argv[0]}")
            return popen(argv, *args, **kwargs)

        refs = fixture.GITHUB.local_refs(self.repo)
        handler = signal.getsignal(signal.SIGCHLD)
        with mock.patch.object(subprocess, "Popen", read_only):
            for remote in (str(self.remote), "fixture/codex"):
                with self.subTest(remote=remote), self.assertRaises(fixture.GITHUB.Refused):
                    self.fetch(remote, lambda _common, _scratch: None)
        self.assertIs(signal.getsignal(signal.SIGCHLD), handler)
        self.assertEqual(fixture.GITHUB.local_refs(self.repo), refs)
        self.assertEqual(list(self.temp.iterdir()), [])

    def test_guard_measures_exact_common_dir_from_bare_and_linked_worktrees(self):
        common = self.root.resolve() / "metadata \r"
        fixture.git(self.root, "clone", "--template=", "--bare", str(self.repo), str(common))
        linked = self.root / "linked"
        fixture.git(common, "worktree", "add", "--detach", str(linked), self.new)

        fixture.git(self.remote, "update-ref", "refs/heads/linked", self.old)
        for repo, source, oid in ((common, "main", self.new), (linked, "linked", self.old)):
            with self.subTest(repo=repo):
                destination = "refs/remotes/seen/" + ("bare" if repo == common else "linked")
                checks = []

                def check(directory, scratch):
                    self.assertEqual(directory, common)
                    self.assertIsNone(scratch)
                    checks.append(directory)

                fixture.GITHUB.fetch(repo, str(self.remote), [f"refs/heads/{source}:" + destination],
                                     before_transfer=check)
                self.assertTrue(checks)
                self.assertEqual(fixture.git(repo, "rev-parse", destination), oid)

    def traced_local_fetches(self, receiver, destinations, check=None):
        specs = ["refs/heads/main:" + target for target in destinations]
        run = fixture.GITHUB.run_transport
        with tempfile.NamedTemporaryFile(dir=self.root) as trace:
            def traced(argv, *, env, **kwargs):
                return run(argv, env={**env, "GIT_TRACE2_EVENT": trace.name}, **kwargs)

            with mock.patch.object(fixture.GITHUB, "run_transport", traced):
                fixture.GITHUB.fetch(receiver, str(self.remote), specs, before_transfer=check)
            events = (json.loads(line) for line in trace)
            return sum(event["event"] == "cmd_name" and event["name"] == "fetch" for event in events)

    def test_one_local_fetch_updates_both_destinations(self):
        receiver = self.root / "receiver.git"
        fixture.git(self.root, "init", "--bare", "--template=", str(receiver))
        targets = [self.target, "refs/remotes/second/main"]
        guard = mock.Mock()
        transfers = self.traced_local_fetches(receiver, targets, guard)
        guard.assert_called_with(receiver.resolve(), None)
        self.assertEqual([fixture.GITHUB.current_ref(receiver, ref) for ref in targets],
                         [self.new, self.new])
        self.assertEqual(transfers, 1)

    def test_observed_local_slot_needs_no_writer_until_the_source_changes(self):
        self.assertEqual(self.traced_local_fetches(self.repo, [self.target]), 1)
        alias = "refs/remotes/second/main"
        floor = mock.Mock(side_effect=RuntimeError("fixture fetch floor reached"))
        with self.subTest("observed source matches cached slot"):
            self.assertEqual(self.traced_local_fetches(self.repo, [alias], floor), 0)
            floor.assert_not_called()
            self.assertEqual(fixture.GITHUB.current_ref(self.repo, alias), self.new)

        fixture.git(self.repo, "commit", "-q", "--allow-empty", "-m", "advance source")
        fixture.git(self.repo, "push", "-q", str(self.remote), "HEAD:refs/heads/main")
        before = fixture.GITHUB.local_refs(self.repo)
        floor.reset_mock()
        with self.subTest("changed source still requires the transfer guard"):
            with self.assertRaisesRegex(RuntimeError, "fixture fetch floor reached"):
                self.traced_local_fetches(self.repo, [alias], floor)
            floor.assert_called_once_with((self.repo / ".git").resolve(), None)
            self.assertEqual(fixture.GITHUB.local_refs(self.repo), before)

    def test_retained_local_slot_with_missing_tip_fetches_the_object(self):
        self.assertEqual(self.traced_local_fetches(self.repo, [self.target]), 1)
        slot, = [ref for ref in fixture.GITHUB.local_refs(self.repo) if ref.startswith("refs/agh/")]
        receiver = self.root / "restored.git"
        fixture.git(self.root, "init", "--bare", "--template=", str(receiver))
        retained = receiver / slot
        retained.parent.mkdir(parents=True)
        retained.write_text(self.new + "\n")
        self.assertEqual(fixture.GITHUB.local_refs(receiver), {slot: self.new})
        fixture.GITHUB.git(receiver, "rev-parse", "--verify", "--quiet", self.new + "^{commit}", codes=(1,))

        guard = mock.Mock()
        transfers = self.traced_local_fetches(receiver, [self.target], guard)
        self.assertEqual(transfers, 1)
        guard.assert_called_with(receiver.resolve(), None)
        self.assertEqual(fixture.git(receiver, "cat-file", "-t", self.new), "commit")
        self.assertEqual(fixture.GITHUB.current_ref(receiver, self.target), self.new)

    def test_stale_supplied_local_snapshot_still_checks_the_named_ref(self):
        self.assertEqual(self.traced_local_fetches(self.repo, [self.target]), 1)
        fixture.git(self.repo, "commit", "-q", "--allow-empty", "-m", "advance source")
        fixture.git(self.repo, "push", "-q", str(self.remote), "HEAD:refs/heads/main")
        alias = "refs/remotes/second/main"
        with self.assertRaisesRegex(fixture.GITHUB.Refused, "differs from its enumerated object ID"):
            fixture.GITHUB.fetch(self.repo, str(self.remote), ["refs/heads/main:" + alias],
                                 snapshot={"refs/heads/main": self.new})
        self.assertEqual(fixture.GITHUB.current_ref(self.repo, alias), fixture.GITHUB.ZERO)
        self.assertEqual(fixture.GITHUB.current_ref(self.repo, self.target), self.new)

    def test_guarded_fetch_refuses_when_caller_owns_sigchld(self):
        def handler(_signum, _frame):
            pass

        previous = signal.signal(signal.SIGCHLD, handler)
        try:
            self.assert_refuses_before_writer()
        finally:
            signal.signal(signal.SIGCHLD, previous)

    def test_guarded_fetch_refuses_from_thread_with_default_sigchld(self):
        previous = signal.signal(signal.SIGCHLD, signal.SIG_DFL)
        try:
            with ThreadPoolExecutor(max_workers=1) as worker:
                worker.submit(self.assert_refuses_before_writer).result(timeout=HANG_SECONDS)
        finally:
            signal.signal(signal.SIGCHLD, previous)

    def test_local_fetch_stops_git_and_its_upload_pack(self):
        # Interpose only to give the real Git fetch a controlled local upload-pack.
        self.python_tool("git",
                        "import os, sys\n"
                        "args = sys.argv[1:]\n"
                        "if 'fetch' in args:\n"
                        f"    args.insert(args.index('fetch') + 1, {'--upload-pack=' + shlex.quote(str(self.cli))!r})\n"
                        "git = os.environ['FETCH_FIXTURE_GIT']\n"
                        "os.execv(git, [git, *args])\n")
        with mock.patch.dict(os.environ, {"PATH": str(self.root) + os.pathsep + os.environ["PATH"]}):
            with self.assertRaisesRegex(RuntimeError, "fixture fetch floor reached"):
                self.fetch(str(self.remote), self.refuse_when_writing)
        self.assert_stopped()
        self.fetch(str(self.remote), lambda _common, _scratch: None)
        self.assertEqual(fixture.git(self.repo, "rev-parse", self.target), self.new)

    @contextmanager
    def managed_local_writer(self, github, on_ready=None):
        # Interpose on the exact-main fetch only. Publication's candidate seed still
        # transfers its real objects before Git starts the controlled upload-pack.
        import time

        ready, release = self.root / "ready", self.root / "release"
        option = "--upload-pack=" + shlex.quote(str(self.cli))
        transport = github.run_transport
        communicate = subprocess.Popen.communicate
        sampled = False

        def local_fetch(argv, **kwargs):
            if "fetch" in argv and "--quiet" in argv:
                index = argv.index("fetch") + 1
                argv = [*argv[:index], option, *argv[index:]]
            return transport(argv, **kwargs)

        def at_writer(child, input=None, timeout=None):
            nonlocal sampled
            if option in child.args and not sampled:
                limit = time.monotonic() + HANG_SECONDS
                while time.monotonic() < limit:
                    try:
                        # Git reads the refspecs before starting its upload-pack.
                        return communicate(child, input=input, timeout=1)
                    except subprocess.TimeoutExpired:
                        input = None  # Resume the existing pipe transfer.
                        if not ready.is_file():
                            continue
                        sampled = True
                        if on_ready is not None:
                            on_ready()
                        if timeout <= 1:
                            raise  # Let the real poll check TERM and the disk.
                        release.touch()  # Let an unmonitored baseline finish.
                        break
                else:
                    self.fail("local upload-pack did not start")
            return communicate(child, input=input, timeout=timeout)

        try:
            with mock.patch.dict(os.environ, {"FETCH_FIXTURE_MODE": "cli-timeout"}), \
                 mock.patch.object(github, "run_transport", local_fetch), \
                 mock.patch.object(subprocess.Popen, "communicate", at_writer):
                yield
        finally:
            release.touch()
        self.assertTrue(sampled)
        self.assert_stopped()

    def check_managed_release_floor(self, github, refused, operation):
        from types import SimpleNamespace

        def space(path):
            low = (self.root / "ready").is_file() and Path(path).name == "pack"
            return SimpleNamespace(f_frsize=2**30, f_bavail=19 if low else 21)

        with self.managed_local_writer(github), mock.patch.object(os, "statvfs", space):
            with self.assertRaisesRegex(refused, "20 GiB running floor"):
                operation()
        self.assertEqual(list(self.temp.iterdir()), [])

    def test_release_ledger_stops_local_upload_pack_below_floor(self):
        from test_version_ledger import VERSIONS

        self.check_managed_release_floor(VERSIONS.GITHUB, VERSIONS.Refused,
                                         lambda: VERSIONS.released(self.repo, str(self.remote), self.new))

    def test_publication_stops_local_exact_main_upload_pack_below_floor(self):
        from test_github_push import PUSH

        self.check_managed_release_floor(PUSH.GITHUB, PUSH.GITHUB.Refused,
                                         lambda: PUSH.publish(self.repo, str(self.remote), self.old,
                                                              admit_transfer=PUSH.GITHUB.admit_release_transfer))

    def test_local_checks_run_the_bound_monitor_then_the_existing_observer(self):
        events = []

        def admit(common, scratch):
            self.assertEqual(common, (self.repo / ".git").resolve())
            self.assertIsNone(scratch)
            events.append("admit")
            return lambda: events.append("floor")

        fixture.GITHUB.fetch(self.repo, str(self.remote), ["refs/heads/main:" + self.target],
                             before_transfer=lambda *_: events.append("observer"), admit_transfer=admit)
        self.assertEqual(events[:2], ["admit", "observer"])
        self.assertGreaterEqual(len(events), 4)
        self.assertEqual(events[2:], ["floor", "observer"] * ((len(events) - 2) // 2))

    def test_monitored_local_wait_retires_writer_before_observer_after_term(self):
        github = fixture.GITHUB

        def observe(_common, _scratch):
            self.assertFalse((self.root / "ready").is_file(), "observer ran after TERM")

        def request_term():
            handler = signal.getsignal(signal.SIGTERM)
            self.assertTrue(callable(handler), "managed fetch must install its TERM handler")
            handler(signal.SIGTERM, None)

        # Invoke the installed handler after upload-pack holds the lock and
        # capture its final re-delivery without signaling the test runner.
        original = signal.signal(signal.SIGTERM, signal.SIG_DFL)
        try:
            with self.managed_local_writer(github, request_term), \
                 mock.patch.object(signal, "raise_signal") as delivered:
                with self.assertRaises(SystemExit) as stop:
                    self.fetch(str(self.remote), observe)
                self.assertEqual(stop.exception.code, 128 + signal.SIGTERM)
                delivered.assert_called_once_with(signal.SIGTERM)
        finally:
            signal.signal(signal.SIGTERM, original)

    def test_native_stop_retires_separate_writer_before_cleaning_scratch(self):
        with self.assertRaisesRegex(RuntimeError, "fixture fetch floor reached"):
            self.fetch("fixture/codex", self.refuse_when_writing)
        self.assert_stopped()
        self.assertTrue((self.root / "settled").is_file())
        scratch = Path((self.root / "scratch").read_text())
        self.assertEqual(scratch.parent.resolve(), self.temp.resolve())
        self.assertFalse(scratch.exists())

    def release_cli(self, *, inherited_ignore=False):
        # Keep release admission independent of how much free space the test host has.
        driver = fixture.disk_fixture_cli(fixture.GITHUB.__file__, 24)
        command = [sys.executable, "-IB", "-c", driver, "--repo", self.repo, "--anonymous", "fetch", "fixture/codex",
                   "refs/heads/main:" + self.target]
        if inherited_ignore:
            launch = ("import os, signal, sys; signal.signal(signal.SIGCHLD, signal.SIG_IGN); "
                      "os.execv(sys.executable, sys.argv[1:])")
            command = [sys.executable, "-IB", "-c", launch, *command]
        return subprocess.run(command,
                              env=dict(os.environ, TMPDIR=str(self.temp), FETCH_FIXTURE_HEAD=self.new),
                              capture_output=True, text=True, timeout=HANG_SECONDS)

    def test_release_cli_can_monitor_a_native_fetch_after_inheriting_ignored_sigchld(self):
        with mock.patch.dict(os.environ, {"FETCH_FIXTURE_MODE": "typed-failure"}):
            result = self.release_cli(inherited_ignore=True)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("fixture fetch refused", result.stderr)
        self.assertFalse(Path((self.root / "scratch").read_text()).exists())

    def test_release_fetch_stops_native_writer_when_scratch_falls_below_floor(self):
        import io
        import time
        from types import SimpleNamespace

        ready = self.root / "ready"
        release = self.root / "release"
        communicate = subprocess.Popen.communicate
        sampled = False

        def wait_for_writer(child, *args, **kwargs):
            nonlocal sampled
            if "fetch" in child.args and "--input" in child.args and kwargs.get("timeout", 2) <= 1:
                if sampled:
                    # Let a baseline without a running check finish its ordinary wait.
                    release.touch()
                else:
                    limit = time.monotonic() + HANG_SECONDS
                    while not ready.is_file():
                        if time.monotonic() >= limit:
                            self.fail("native fixture did not start its writer")
                        time.sleep(0.01)
                    sampled = True
            return communicate(child, *args, **kwargs)

        def space(path):
            low = ready.is_file() and Path(path) == self.temp.resolve()
            return SimpleNamespace(f_frsize=2**30, f_bavail=19 if low else 21)

        argv = [fixture.GITHUB.__file__, "--repo", str(self.repo), "--anonymous", "fetch",
                "fixture/codex", "refs/heads/main:" + self.target]
        try:
            with mock.patch.dict(os.environ, {"FETCH_FIXTURE_MODE": "cli-timeout",
                                               "FETCH_FIXTURE_HEAD": self.new}), \
                 mock.patch.object(os, "statvfs", space), mock.patch.object(sys, "argv", argv), \
                 mock.patch.object(subprocess.Popen, "communicate", wait_for_writer), \
                 mock.patch.object(sys, "stderr", new_callable=io.StringIO) as err:
                self.assertEqual(fixture.GITHUB.main(), 75)
            self.assertIn("20 GiB running floor", err.getvalue())
            self.assertIn(str(self.temp.resolve()), err.getvalue())
            self.assertTrue(sampled)
            self.assertTrue((self.root / "settled").is_file())
            self.assert_stopped()
            self.assertFalse(Path((self.root / "scratch").read_text()).exists())
        finally:
            release.touch()

    def test_release_fetch_retains_scratch_when_native_exits_with_an_independent_writer(self):
        with self.uncertain_writer("spontaneous"):
            result = self.release_cli()
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIn("exit -9", result.stderr)
            self.assertIn("retained scratch", result.stderr)

    def test_release_fetch_retires_scratch_on_typed_native_failure(self):
        with mock.patch.dict(os.environ, {"FETCH_FIXTURE_MODE": "typed-failure"}):
            result = self.release_cli()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("fixture fetch refused", result.stderr)
        self.assertNotIn("retained scratch", result.stderr)
        self.assertFalse(Path((self.root / "scratch").read_text()).exists())

    def test_release_fetch_observes_term_and_retires_a_separate_native_writer(self):
        with mock.patch.dict(os.environ, {"FETCH_FIXTURE_MODE": "cli-term"}):
            result = self.release_cli()
        self.assertEqual(result.returncode, -signal.SIGTERM, result.stderr)
        self.assertTrue((self.root / "settled").is_file())
        self.assert_stopped()
        self.assertFalse(Path((self.root / "scratch").read_text()).exists())

    def test_native_deadline_retires_separate_writer_without_a_pressure_observer(self):
        import time

        transport = fixture.GITHUB.run_transport
        def at_deadline(argv, **kwargs):
            if "--input" in argv and "fetch" in argv:
                self.assertEqual(kwargs["timeout"], 120)
                kwargs["timeout"] = 0
            return transport(argv, **kwargs)

        communicate = subprocess.Popen.communicate
        expired = False
        def deadline(child, *args, **kwargs):
            nonlocal expired
            if not expired and "--input" in child.args and "fetch" in child.args:
                limit = time.monotonic() + HANG_SECONDS
                while not (self.root / "ready").is_file():
                    if time.monotonic() >= limit:
                        self.fail("native fixture did not start its writer")
                    time.sleep(0.01)
                expired = True
                raise subprocess.TimeoutExpired(child.args, kwargs["timeout"])
            return communicate(child, *args, **kwargs)

        try:
            with mock.patch.dict(os.environ, {"FETCH_FIXTURE_MODE": "cli-timeout"}), \
                 mock.patch.object(fixture.GITHUB, "run_transport", at_deadline), \
                 mock.patch.object(subprocess.Popen, "communicate", deadline), \
                 self.assertRaises(subprocess.TimeoutExpired):
                fixture.GITHUB.fetch(self.repo, "fixture/codex", ["refs/heads/main:" + self.target],
                                     snapshot={"refs/heads/main": self.new},
                                     admit_transfer=lambda _common, _scratch: None)
            self.assertTrue(expired)
            self.assertTrue((self.root / "settled").is_file())
            self.assert_stopped()
            self.assertFalse(Path((self.root / "scratch").read_text()).exists())
        finally:
            # Release a bounded stand-in even when testing a failing older implementation.
            (self.root / "release").touch()
            lock = self.root / "writer.lock"
            if lock.exists():
                import fcntl
                with lock.open("rb") as handle:
                    fcntl.flock(handle, fcntl.LOCK_EX)

    def test_raw_native_signal_exit_preserves_uncertain_scratch(self):
        with self.uncertain_writer("raw-exit"):
            with self.assertRaisesRegex(RuntimeError, "fixture fetch floor reached"):
                self.fetch("fixture/codex", self.refuse_when_writing)

    def test_spontaneous_native_signal_exit_preserves_live_writer_scratch(self):
        with self.uncertain_writer("spontaneous"):
            with self.assertRaisesRegex(fixture.GITHUB.Refused, "exit -9"):
                self.fetch("fixture/codex", lambda _common, _scratch: None)

    def test_deferred_term_preserves_independent_writer_after_raw_exit(self):
        # Deliver TERM to a real updater caller while it awaits the Agh exit.
        driver = (
            'import runpy, sys\n'
            'fetch = runpy.run_path(sys.argv[1])["fetch"]\n'
            'fetch(sys.argv[2], "fixture/codex", ["refs/heads/main:" + sys.argv[3]],\n'
            '      snapshot={"refs/heads/main": sys.argv[4]}, before_transfer=lambda *_: None)\n')
        with self.uncertain_writer("parent-term"):
            status = subprocess.run([sys.executable, "-IB", "-c", driver,
                                     fixture.GITHUB.__file__, self.repo, self.target, self.new],
                                    env=dict(os.environ, TMPDIR=str(self.temp)),
                                    capture_output=True, timeout=HANG_SECONDS)
            self.assertEqual(status.returncode, -signal.SIGTERM, status.stderr)

    @contextmanager
    def uncertain_writer(self, mode):
        import fcntl

        lock_path = self.root / "writer.lock"
        try:
            with mock.patch.dict(os.environ, {"FETCH_FIXTURE_MODE": mode}):
                yield
            scratch = Path((self.root / "scratch").read_text())
            with lock_path.open("rb") as lock, self.assertRaises(BlockingIOError):
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.assertTrue((scratch / "owned.pack").is_file())
        finally:
            (self.root / "release").touch()
            if lock_path.exists():
                with lock_path.open("rb") as lock:
                    fcntl.flock(lock, fcntl.LOCK_EX)
        self.assertTrue((scratch / "writer-finished").is_file())
        self.assert_stopped()

    def test_success_drains_both_pipes_across_mid_transfer_check(self):
        class RecordingAgh(fixture.GITHUB.Agh):
            def fetch_batch(self, *args, **kwargs):
                self.output = super().fetch_batch(*args, **kwargs)
                return self.output

        def release(_common, _scratch):
            if (self.root / "ready").is_file():
                (self.root / "release").touch()

        with mock.patch.dict(os.environ, {"FETCH_FIXTURE_MODE": "output"}):
            client = RecordingAgh()
            self.fetch("fixture/codex", release, client)
        self.assertEqual(client.output, b"a" * 262144 + b"b" * 262144)
        self.assertEqual(fixture.git(self.repo, "rev-parse", self.target), self.new)
        self.assertEqual(list(self.temp.iterdir()), [])

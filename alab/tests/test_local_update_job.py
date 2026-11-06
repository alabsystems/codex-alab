#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The public scheduled entry holds the package cache and prepares a concrete job."""

import errno
import fcntl
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import select
import shutil
import signal
import shlex
import subprocess
import sys
import time
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
# A shebang cannot contain a Python path with spaces.
PYTHON_ENTRY = "#!/bin/sh\n'''exec' " + shlex.quote(sys.executable) + ' -I "$0" "$@"\n\' \'\'\'\n'
SPEC = importlib.util.spec_from_file_location("local_update_fixtures",
                                             ROOT / "alab/tests/test_local_update.py")
FIXTURES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FIXTURES)


class LocalUpdateJobTests(unittest.TestCase):
    git = FIXTURES.LocalUpdateTests.git
    package = FIXTURES.LocalUpdateTests.package
    select = FIXTURES.LocalUpdateTests.select
    selected = FIXTURES.LocalUpdateTests.selected
    publish_next_source = FIXTURES.LocalUpdateTests.publish_next_source

    def setUp(self):
        FIXTURES.LocalUpdateTests.setUp(self)
        tooling = self.repo / "alab"
        tooling.mkdir()
        for name in ("local_update.py", "promote.py", "lib.sh", "command_guard.py", "retention.py",
                     "package_cache.py"):
            shutil.copyfile(ROOT / "alab" / name, tooling / name)
        self.cache = self.home / "cache"
        self.journal = self.home / "built.jsonl"
        self.calls = self.home / "selector.jsonl"
        host = Path(self.aterm)
        host.parent.mkdir()
        host.write_text(PYTHON_ENTRY + f'''import json
from pathlib import Path
import re
import sys
args = sys.argv[1:]
if args[:2] == ["pkg", "__build_cache_package"]:
    raise SystemExit(2)
with Path({str(self.calls)!r}).open("a") as stream:
    stream.write(json.dumps(args) + "\\n")
if args == ["pkg", "link", "--help"]:
    print("link --if-checkout <path> --if-prefix <path> --wait-lock <secs>")
else:
    marker = Path({str(self.marker)!r})
    match = re.search(r'^checkout\\s*=\\s*(.+)$', marker.read_text(), re.M)
    assert match is not None
    checkout = json.loads(match.group(1).strip())
    assert args[args.index("--if-checkout") + 1] == checkout
    assert args[args.index("--if-prefix") + 1] == {str(self.prefix)!r}
    assert args[args.index("--wait-lock") + 1] == "1800"
    marker.write_text('schema = 1\\nprogram = "codex"\\ncheckout = '
                      + json.dumps(args[3]) + '\\nbins = ["bin/codex"]\\n')
''')
        host.chmod(0o755)
        builder = self.home / "build_fixture.py"
        builder.write_text(f'''
import argparse
import fcntl
import json
import os
from pathlib import Path
import sys
Path({str(self.home / 'builder-python.json')!r}).write_text(json.dumps({{
    "executable": sys.executable, "version": list(sys.version_info[:3])}}))
parser = argparse.ArgumentParser()
parser.add_argument("--commit", required=True)
parser.add_argument("--out", required=True)
parser.add_argument("--keep", required=True)
parser.add_argument("--no-promote", action="store_true", required=True)
args = parser.parse_args()
progress = json.loads(Path({str(self.repo / '.git/alab-local-update.status.json')!r}).read_text())
assert progress["complete"] is False and progress["exit"] is None
assert progress["phase"] == "building"
assert progress["target_source"] == args.commit
assert progress["initial_selected_source"] == {self.old_source!r}
assert progress["initial_selected_checkout"] == {str(self.old)!r}
assert progress["initial_selected_version"] == {self.old.name!r}
assert "current activity unverified" in progress["reason"]
display = json.loads((Path(args.out) / "codex/.alab-local-update-status.json").read_text())
assert display["phase"] == "building" and display["complete"] is False
assert display["target_source"] == args.commit
assert display["initial_package"] == {self.old.name!r}
assert display["initial_source"] == {self.old_source!r}
assert "reason" not in display and "controller_path" not in display
with (Path(os.environ["ALAB_PACKAGE_CACHE"]) / ".alab-package.lockf").open("a") as stream:
    try:
        fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        pass
    else:
        raise AssertionError("build ran without its parent's package cache lock")
version = "0.164.0-dev.3+g" + args.commit[:9]
package = Path(args.out) / "codex" / version
(package / "bin").mkdir(parents=True)
(package / "codex-resources").mkdir()
(package / "codex-path").mkdir()
for relative in ("bin/codex", "bin/codex-code-mode-host", "codex-path/rg"):
    executable = package / relative
    executable.write_bytes(b"fixture, never executed")
    executable.chmod(0o755)
(package / "alab-build.json").write_text(json.dumps({{
    "source": args.commit, "version": version, "target": "aarch64-apple-darwin",
    "compiler": {{"admitted": True}}}}))
(package / "codex-package.json").write_text(json.dumps({{
    "layoutVersion": 1, "version": version, "target": "aarch64-apple-darwin",
    "variant": "codex", "entrypoint": "bin/codex",
    "resourcesDir": "codex-resources", "pathDir": "codex-path"}}))
with Path({str(self.journal)!r}).open("a") as stream:
    stream.write(json.dumps({{"source": args.commit, "lock": "held"}}) + "\\n")
''')
        (tooling / "package.sh").write_text(
            'package_locked_entry() { source="$1"; shift; package_main --commit "$source" "$@"; }\n'
            'package_main() { '
            '"$ALAB_PYTHON" -I -c \'import sys; sys.exit(sys.version_info < (3, 11))\' '
            '|| return 1; "$ALAB_PYTHON" -I "$ALAB_LOCAL_TEST_BUILD" "$@"; }\n')
        self.environment = dict(os.environ, HOME=str(self.home), ALAB_ATERM=str(host),
                                ALAB_PYTHON=os.environ.get("ALAB_PYTHON", sys.executable),
                                ALAB_PACKAGE_CACHE=str(self.cache),
                                ATPKG_BIN=str(self.prefix / "bin"),
                                ALAB_LOCAL_TEST_BUILD=str(builder))
        # The remote carries the actual controller/helper sources that a poll adopts.
        self.git("add", "alab")
        self.git("commit", "-q", "-m", "scheduled controller fixtures")
        self.source = self.git("rev-parse", "HEAD")
        self.git("push", "-q", "origin", "main")
        self.new = self.package(self.source, 2)

    def run_job(self, *arguments):
        command = [
            sys.executable, "-I", str(self.repo / "alab/local_update.py"),
            "--repo", str(self.repo), "--out", str(self.out), "--prefix", str(self.prefix),
            *arguments]
        return self.run_command(command, self.environment)

    def run_command(self, command, environment):
        child = subprocess.Popen(command, env=environment, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE, text=True, start_new_session=True)
        try:
            stdout, stderr = child.communicate(timeout=60)
        except BaseException:
            # Do not reap/poll before signaling: the unreaped session leader holds its
            # process-group identity while its Bash/lockf/build descendants are stopped.
            if child.returncode is not None:
                self.keep_scratch = True
                raise
            try:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                child.communicate(timeout=60)
            except BaseException:
                self.keep_scratch = True
                raise
            raise
        return subprocess.CompletedProcess(command, child.returncode, stdout, stderr)

    def test_prepared_job_preserves_explicit_modern_builder_on_cache_miss(self):
        # Run this control under an older controller with ALAB_PYTHON bound to a
        # modern builder too: the emitted job must not replace that binding.
        builder = os.environ.get("ALAB_PYTHON", sys.executable)
        capable = subprocess.run([builder, "-I", "-c",
                                  'import sys; sys.exit(sys.version_info < (3, 11))'],
                                 capture_output=True, text=True, timeout=60)
        self.assertEqual(capable.returncode, 0, capable.stderr)
        self.environment["ALAB_PYTHON"] = builder
        fresh = self.publish_next_source()
        prepared = self.run_job("--launchd-plist")
        self.assertEqual(prepared.returncode, 0, prepared.stderr)
        job = plistlib.loads(prepared.stdout.encode())
        self.assertEqual(job["ProgramArguments"][0], sys.executable)
        done = self.run_command(job["ProgramArguments"], job["EnvironmentVariables"] | {
            "ALAB_LOCAL_TEST_BUILD": self.environment["ALAB_LOCAL_TEST_BUILD"]})
        self.assertEqual(done.returncode, 0, done.stderr + done.stdout)
        observed = json.loads((self.home / "builder-python.json").read_text())
        self.assertGreaterEqual(observed["version"], [3, 11])
        self.assertEqual(
            Path(observed["executable"]).resolve(),
            Path(shutil.which(builder) or builder).resolve(),
        )
        self.assertEqual(self.selected().name, f"0.164.0-dev.3+g{fresh[:9]}")
        self.assertEqual(json.loads(self.journal.read_text()), {"source": fresh, "lock": "held"})

    def test_prepared_job_uses_only_a_known_capable_controller_as_default_builder(self):
        self.environment.pop("ALAB_PYTHON")
        self.environment["PATH"] = "/usr/bin:/bin"
        done = self.run_job("--launchd-plist")
        self.assertEqual(done.returncode, 0, done.stderr)
        job = plistlib.loads(done.stdout.encode())
        environment = job["EnvironmentVariables"]
        self.assertEqual(job["ProgramArguments"][0], sys.executable)
        if sys.version_info >= (3, 11):
            self.assertEqual(environment["ALAB_PYTHON"], sys.executable)
        else:
            self.assertNotIn("ALAB_PYTHON", environment)

    def test_prepared_job_refuses_an_explicit_unsupported_builder(self):
        builders = ["/usr/bin/false"]
        if sys.version_info < (3, 11):
            builders.append(sys.executable)
        for builder in builders:
            with self.subTest(builder=builder):
                self.environment["ALAB_PYTHON"] = builder
                done = self.run_job("--launchd-plist")
                self.assertEqual(done.returncode, 1, done.stderr + done.stdout)
                self.assertEqual(done.stdout, "")
                self.assertFalse(self.cache.exists())
                self.assertFalse(self.journal.exists())
                self.assertEqual(self.selected(), self.old)

    def test_public_entry_holds_existing_cache_lock_and_builds_only_once(self):
        fresh = self.publish_next_source()
        done = self.run_job()
        self.assertEqual(done.returncode, 0, done.stderr + done.stdout)
        self.assertEqual(json.loads(self.journal.read_text()), {"source": fresh, "lock": "held"})
        self.assertEqual(self.selected().name, f"0.164.0-dev.3+g{fresh[:9]}")
        path = self.repo / ".git/alab-local-update.status.json"
        completed = json.loads(path.read_bytes())
        self.assertTrue(completed["complete"])
        self.assertEqual(completed["initial_selected_source"], self.old_source)
        self.assertEqual(completed["initial_selected_checkout"], str(self.old))
        self.assertEqual(completed["initial_selected_version"], self.old.name)
        self.assertEqual(completed["target_source"], fresh)
        display_path = self.root / FIXTURES.UPDATE.CHANNEL_STATUS
        display = json.loads(display_path.read_bytes())
        self.assertTrue(display["complete"])
        self.assertEqual(display["exit"], 0)
        self.assertEqual(display["last_phase"], "selecting")
        self.assertEqual(display["initial_package"], self.old.name)
        self.assertEqual(display["target_source"], fresh)
        before = self.journal.read_bytes()
        again = self.run_job()
        self.assertEqual(again.returncode, 0, again.stderr + again.stdout)
        self.assertEqual(self.journal.read_bytes(), before)
        contents = path.read_bytes()
        status = json.loads(contents)
        self.assertEqual(status["exit"], 0)
        self.assertTrue(status["complete"])
        self.assertEqual(status["phase"], "completed")
        self.assertEqual(status["initial_selected_source"], fresh)
        self.assertEqual(status["target_source"], fresh)
        self.assertIn("already", status["reason"])
        shown = self.run_job("--status")
        self.assertEqual(shown.returncode, 0, shown.stderr)
        self.assertEqual(json.loads(shown.stdout), status)
        calls = self.calls.read_bytes()

        def refused_status():
            refused = self.run_job("--status")
            self.assertEqual(refused.returncode, 1, refused.stderr + refused.stdout)

        path.unlink()
        os.mkfifo(path, 0o600)
        refused_status()
        path.unlink()
        target = self.home / "status target"
        target.write_bytes(contents)
        path.symlink_to(target)
        refused_status()
        self.assertEqual(target.read_bytes(), contents)
        path.unlink()
        path.write_bytes(b"x" * (FIXTURES.UPDATE.PROMOTE.LIMIT + 1))
        refused_status()
        path.unlink()
        path.mkdir()
        refused_status()
        path.rmdir()
        missing = self.run_job("--status")
        self.assertEqual(missing.returncode, 0, missing.stderr)
        self.assertIn("no local update run", missing.stdout)
        path.write_bytes(contents)
        self.assertEqual(self.selected().name, f"0.164.0-dev.3+g{fresh[:9]}")
        self.assertEqual(self.journal.read_bytes(), before)
        self.assertEqual(self.calls.read_bytes(), calls)

    def test_channel_display_failure_and_interruption_cannot_change_the_transaction(self):
        update = FIXTURES.UPDATE
        display_path = self.root / update.CHANNEL_STATUS
        arguments = ["--repo", str(self.repo), "--out", str(self.out),
                     "--prefix", str(self.prefix), "--aterm", self.aterm]
        initial = {"initial_selected_checkout": str(self.old),
                   "initial_selected_source": self.old_source,
                   "initial_selected_version": self.old.name}
        context = update.channel_context(self.out, initial)
        self.assertTrue(update.record_channel_status(
            context, None, "building", "building", {"target_source": self.source}))
        before = display_path.read_bytes()
        replace = Path.replace

        def refuse_display(path, destination):
            if Path(destination) == display_path:
                raise OSError("private injected display failure")
            return replace(path, destination)

        def successful_update(*_args, progress, **_kwargs):
            progress("fetching", **initial)
            progress("building", target_source=self.source)
            self.select(self.new)
            progress("selecting")
            return True, "selected private fixture"

        with mock.patch.object(Path, "replace", refuse_display), \
                mock.patch.object(update, "update_once", successful_update):
            self.assertEqual(update.run_locked(arguments), 0)
        self.assertEqual(self.selected(), self.new)
        self.assertEqual(display_path.read_bytes(), before, "a failed replace keeps the old record")
        journal = json.loads((self.repo / ".git/alab-local-update.status.json").read_bytes())
        self.assertEqual(journal["exit"], 0)
        self.assertTrue(journal["complete"])

        def interrupt_display(path, destination):
            if Path(destination) == display_path:
                raise KeyboardInterrupt
            return replace(path, destination)

        with mock.patch.object(Path, "replace", interrupt_display):
            with self.assertRaises(KeyboardInterrupt):
                update.record_channel_status(context, 0, "completed", "selecting", {})
        self.assertEqual(display_path.read_bytes(), before, "interruption never publishes a prefix")

        def failed_update(*_args, progress, **_kwargs):
            progress("fetching", **initial)
            progress("building", target_source=self.source)
            raise RuntimeError("private secret diagnostic must stay in the Git journal")

        self.select(self.old)
        with mock.patch.object(update, "update_once", failed_update):
            self.assertEqual(update.run_locked(arguments), 1)
        failed = json.loads(display_path.read_bytes())
        self.assertEqual(failed["exit"], 1)
        self.assertEqual(failed["phase"], "completed")
        self.assertEqual(failed["last_phase"], "building")
        self.assertNotIn("secret", display_path.read_text())
        self.assertNotIn("reason", failed)
        self.assertEqual(self.selected(), self.old)

    def test_controller_crash_cannot_unlock_cache_while_its_builder_still_runs(self):
        self.cache_termination(lambda _child, controller: os.kill(controller, signal.SIGKILL))

    @unittest.skipUnless(Path("/usr/sbin/lsof").is_file(), "requires macOS cache-owner inspection")
    def test_cache_lock_owner_sigterm_cannot_unlock_before_its_writer_stops(self):
        def terminate_owner(child, controller):
            lock = self.cache / ".alab-package.lockf"
            owners = subprocess.run(["/usr/sbin/lsof", "-t", str(lock)],
                                    check=True, capture_output=True, text=True, timeout=60)
            pids = {int(line) for line in owners.stdout.splitlines()}

            def parent(pid):
                observed = subprocess.run(["/bin/ps", "-o", "ppid=", "-p", str(pid)],
                                          check=True, capture_output=True, text=True, timeout=60)
                return int(observed.stdout.strip())

            # The controller is the custodian's direct command child. Both its
            # custodian and original guardian hold the flock; interrupt only the
            # proven guardian, leaving the custodian to exclude surviving writers.
            custodian = parent(controller)
            owner = parent(custodian)
            self.assertTrue({owner, custodian} <= pids, (pids, owner, custodian))
            self.assertGreater(owner, 1)
            self.assertEqual(os.getpgid(owner), child.pid)
            self.assertEqual(os.getpgid(custodian), custodian)
            print(json.dumps({"guardian_lock_owner": owner, "custodian_lock_owner": custodian,
                              "owned_job_group": child.pid}), flush=True)
            os.kill(owner, signal.SIGTERM)

        self.cache_termination(terminate_owner)

    def test_job_group_sigterm_cannot_unlock_before_its_private_writer_stops(self):
        self.cache_termination(lambda child, _controller: os.killpg(child.pid, signal.SIGTERM))

    def cache_termination(self, terminate):
        self.keep_scratch = True
        controller_file = self.home / "controller-pid"
        started = self.home / "orphan-builder-started.json"
        challenge = self.home / "builder-challenge"
        response = self.home / "builder-response.json"
        builder = Path(self.environment["ALAB_LOCAL_TEST_BUILD"])
        builder.write_text(builder.read_text().replace(
            'version = "0.164.0-dev.3+g" + args.commit[:9]', f'''
import time
Path({str(started)!r}).write_text(json.dumps({{"pid": os.getpid(), "pgid": os.getpgrp()}}))
while not Path({str(challenge)!r}).exists():
    time.sleep(0.01)
Path({str(response)!r}).write_text(json.dumps({{"pid": os.getpid(), "responded": True}}))
while True:
    time.sleep(1)
'''))
        package = self.repo / "alab/package.sh"
        package.write_text('package_locked_entry() { source="$1"; shift; package_main --commit "$source" "$@"; }\n'
                           'package_main() { printf "%s\\n" "$PPID" > '
                           + "'" + str(controller_file).replace("'", "'\\''") + "'"
                           + '; "$ALAB_PYTHON" -I "$ALAB_LOCAL_TEST_BUILD" "$@"; }\n')
        self.git("add", "alab/package.sh")
        self.publish_next_source()
        child = subprocess.Popen([
            sys.executable, "-I", "-B", str(self.repo / "alab/local_update.py"),
            "--repo", str(self.repo), "--out", str(self.out), "--prefix", str(self.prefix)],
            env=self.environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, start_new_session=True)
        info = None
        outcome = None

        def until(read):
            deadline = time.monotonic() + 60
            while time.monotonic() < deadline:
                if value := read():
                    return value
                time.sleep(0.01)
            self.fail("cache-termination fixture exceeded the one-minute hang detector")

        def available():
            with (self.cache / ".alab-package.lockf").open("a") as stream:
                try:
                    fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
                except BlockingIOError:
                    return False
                fcntl.flock(stream, fcntl.LOCK_UN)
                return True

        try:
            until(started.exists)
            info = json.loads(started.read_text())
            controller = int(controller_file.read_text())
            self.assertGreater(controller, 1)
            self.assertNotIn(controller, (child.pid, info["pid"]))
            self.assertEqual(os.getpgid(controller), info["pgid"])
            self.assertFalse(available(), "the actual builder starts under the cache lock")
            terminate(child, controller)
            until(available)
            try:
                self.assertNotEqual(os.getpgid(info["pid"]), info["pgid"],
                                    "cache reopened before the original writer exited")
            except ProcessLookupError:
                pass
            challenge.write_text("prove builder progress after cache acquisition\n")

            def stopped_or_responded():
                if response.exists():
                    return "responded"
                try:
                    return "stopped" if os.getpgid(info["pid"]) != info["pgid"] else None
                except ProcessLookupError:
                    return "stopped"

            outcome = until(stopped_or_responded)
            print(json.dumps({"fixture": str(self.home), "controller": controller,
                              "builder": info, "after_cache_acquisition": outcome}), flush=True)
            self.assertEqual(outcome, "stopped",
                             "the interrupted controller's builder ran after cache acquisition")
        finally:
            # The public-entry leader has not been reaped. On the RED source its
            # group also owns the builder; a guardian must stop any nested group
            # before returning the lock, so it is already absent on GREEN.
            if outcome != "stopped":
                try:
                    # Let the guardian handle ordinary job cancellation and
                    # stop its private group before this fixture confirms EOF.
                    os.killpg(child.pid, signal.SIGTERM)
                except (ProcessLookupError, PermissionError):
                    pass
            child.communicate(timeout=60)

    def publish_preserved_controller(self):
        self.keep_scratch = True
        controller = self.repo / "alab/local_update.py"
        controller.write_text(controller.read_text().replace(
            '    args = parser().parse_args(argv)\n',
            '    if source is not None:\n'
            '        (ROOT / "retained-controller-audit").write_text("preserve this fixture\\n")\n'
            '    args = parser().parse_args(argv)\n', 1))
        self.git("add", "alab/local_update.py")
        return self.publish_next_source()

    def test_old_bootstrap_cannot_adopt_a_controller_requiring_guardian_lifetime(self):
        fresh = self.publish_preserved_controller()
        self.package(fresh, 3)
        for revision in ("2b43d9c9497c2bb92c8c39cafad1ceabe2518993",
                         "5946ee9e99d51733d673267749eb7423a5332f7d"):
            with self.subTest(bootstrap=revision):
                for name in ("local_update.py", "lib.sh"):
                    old = subprocess.run(["git", "-C", str(ROOT), "show",
                                          f"{revision}:alab/{name}"],
                                         check=True, capture_output=True, timeout=60)
                    (self.repo / "alab" / name).write_bytes(old.stdout)
                done = self.run_job()
                self.assertEqual(done.returncode, 1, done.stderr + done.stdout)
                self.assertEqual(self.selected(), self.old)
                self.assertFalse(self.journal.exists())
                self.assertFalse(self.calls.exists())
                status = json.loads((self.repo / ".git/alab-local-update.status.json").read_text())
                self.assertEqual(status["exit"], 1)
                self.assertEqual(status["initial_selected_checkout"], str(self.old))
                self.assertTrue((Path(status["controller_path"]) / "retained-controller-audit").is_file())

    def test_dispatcher_uses_fetched_guardian_when_repo_names_another_tree(self):
        fresh = self.publish_preserved_controller()
        candidate = self.package(fresh, 3)
        (self.repo / "alab/lib.sh").write_text(
            'alab_package_cache_locked() { echo "unrelated repository helper" >&2; return 97; }\n')
        done = self.run_command([
            sys.executable, "-I", "-B", str(ROOT / "alab/local_update.py"),
            "--repo", str(self.repo), "--out", str(self.out), "--prefix", str(self.prefix)],
            self.environment)
        self.assertEqual(done.returncode, 0, done.stderr + done.stdout)
        self.assertEqual(self.selected(), candidate)
        status = json.loads((self.repo / ".git/alab-local-update.status.json").read_text())
        self.assertEqual(status["controller_source"], fresh)
        self.assertTrue((Path(status["controller_path"]) / "retained-controller-audit").is_file())

    def test_future_version_hints_reach_the_controller_without_becoming_options(self):
        for version in ("--future-version", "\ud800"):
            with self.subTest(version=version):
                for name in ("alab-build.json", "codex-package.json"):
                    path = self.old / name
                    record = json.loads(path.read_text())
                    record["version"] = version
                    path.write_text(json.dumps(record))
                before = self.marker.read_bytes()
                done = self.run_job()
                self.assertEqual(done.returncode, 0, done.stderr + done.stdout)
                self.assertEqual(self.marker.read_bytes(), before)
                self.assertFalse(self.journal.exists())
                self.assertFalse(self.calls.exists())
                status = json.loads((self.repo / ".git/alab-local-update.status.json").read_text())
                self.assertEqual(status["controller_source"], self.source)
                self.assertEqual(status["initial_selected_source"], self.old_source)
                self.assertTrue(status["complete"])
                if version == "--future-version":
                    self.assertEqual(status["initial_selected_version"], version)
                else:
                    self.assertNotIn("initial_selected_version", status)

    def test_controller_handoff_preserves_a_selection_changed_after_initial_observation(self):
        fresh = self.publish_next_source()
        candidate = self.package(fresh, 3)
        manual = self.package(self.old_source, 2)
        marker = ('schema = 1\nprogram = "codex"\ncheckout = ' + json.dumps(str(manual))
                  + '\nbins = ["bin/codex"]\nlinked_at = ""\n')
        observed = self.home / "manual-selection-observed"
        git_bin = self.home / "interleaving-bin"
        git_bin.mkdir()
        real_git = shutil.which("git")
        wrapper = git_bin / "git"
        # Select B only after the real source worktree exists: the bootstrap has
        # already observed A, while the fetched controller has not started yet.
        wrapper.write_text(PYTHON_ENTRY + f'''import subprocess
import sys
from pathlib import Path
code = subprocess.run([{real_git!r}, *sys.argv[1:]]).returncode
if code == 0 and "worktree" in sys.argv and "add" in sys.argv:
    Path({str(self.marker)!r}).write_text({marker!r})
    Path({str(observed)!r}).write_text({str(manual)!r})
sys.exit(code)
''')
        wrapper.chmod(0o755)
        self.environment["PATH"] = str(git_bin) + os.pathsep + os.environ.get("PATH", "/usr/bin:/bin")
        done = self.run_job()
        self.assertEqual(done.returncode, 0, done.stderr + done.stdout)
        self.assertEqual(observed.read_text(), str(manual))
        self.assertEqual(self.selected(), manual)
        self.assertTrue(candidate.is_dir(), "the unused published candidate remains available")
        self.assertFalse(self.journal.exists(), "a changed selection needs no build")
        self.assertFalse(self.calls.exists(), "a changed selection needs no selector")
        status = json.loads((self.repo / ".git/alab-local-update.status.json").read_text())
        self.assertEqual(status["initial_selected_checkout"], str(self.old))
        self.assertEqual(status["initial_selected_source"], self.old_source)
        self.assertEqual(status["initial_selected_version"], self.old.name)
        self.assertTrue(status["complete"])

    def test_source_controller_without_original_selection_custody_cannot_select(self):
        fresh = self.publish_next_source()
        self.package(fresh, 3)
        self.git("update-ref", "refs/remotes/origin/main", fresh)
        self.cache.mkdir()
        with (self.cache / ".alab-package.lockf").open("a") as stream:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
            for custody in ({}, {"expected_checkout": str(self.old)},
                            {"expected_prefix": str(self.prefix)},
                            {"expected_checkout": "relative", "expected_prefix": str(self.prefix)}):
                with self.subTest(custody=custody):
                    # A plausible journal snapshot is historical data, never a
                    # substitute for custody supplied by the lock-owning caller.
                    (self.repo / ".git/alab-local-update.status.json").write_text(json.dumps({
                        "initial_selected_checkout": str(self.old), "target_source": fresh,
                        "expected_prefix": str(self.prefix), "complete": True, "exit": 0}))
                    code = FIXTURES.UPDATE.run_locked([
                        "--repo", str(self.repo), "--out", str(self.out),
                        "--prefix", str(self.prefix), "--aterm", self.aterm], source=fresh,
                        guardian_contract=FIXTURES.UPDATE.GUARDIAN_CONTRACT, **custody)
                    self.assertEqual(code, 1)
                    self.assertEqual(self.selected(), self.old)
                    self.assertFalse(self.journal.exists())
                    self.assertFalse(self.calls.exists())

    def test_source_controller_cannot_adopt_another_prefix_with_the_same_checkout(self):
        fresh = self.publish_next_source()
        self.package(fresh, 3)
        self.git("update-ref", "refs/remotes/origin/main", fresh)
        other = self.home / "another-prefix"
        (other / "links").mkdir(parents=True)
        marker = other / "links/codex"
        original = self.marker.read_bytes()
        marker.write_bytes(original)
        marker.chmod(0o600)
        self.cache.mkdir()
        with (self.cache / ".alab-package.lockf").open("a") as stream:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
            code = FIXTURES.UPDATE.run_locked([
                "--repo", str(self.repo), "--out", str(self.out),
                "--prefix", str(other), "--aterm", self.aterm], source=fresh,
                expected_checkout=str(self.old), expected_prefix=str(self.prefix),
                guardian_contract=FIXTURES.UPDATE.GUARDIAN_CONTRACT)
        self.assertEqual(code, 0)
        self.assertEqual(self.marker.read_bytes(), original)
        self.assertEqual(marker.read_bytes(), original)
        self.assertFalse(self.journal.exists())
        self.assertFalse(self.calls.exists())

    def test_fetched_controller_and_helpers_preserve_canonical_work_under_one_lock(self):
        tooling = self.repo / "alab"
        bootstrap = (tooling / "local_update.py").read_bytes()
        controller = bootstrap.decode().replace('GUARDIAN_CONTRACT = "cache-lock-v2"',
                                                'GUARDIAN_CONTRACT = "fixture-cache-lock-v3"').replace(
            '"controller_python": sys.executable} if source is not None else {})',
            '"controller_python": sys.executable, "fixture_controller": "fetched"}'
            ' if source is not None else {})')
        self.assertNotEqual(controller.encode(), bootstrap)
        (tooling / "local_update.py").write_text(controller)
        guardian_observation = self.home / "fetched-guardian.json"
        guardian = tooling / "command_guard.py"
        guardian.write_text(guardian.read_text().replace(
            'if __name__ == "__main__":',
            'import json\nfrom pathlib import Path\n'
            f'Path({str(guardian_observation)!r}).write_text(json.dumps({{"path": __file__}}))\n'
            'if __name__ == "__main__":'))
        helper_observation = self.home / "fetched-helper.json"
        (tooling / "fixture_helper.py").write_text(
            'import json\nfrom pathlib import Path\n'
            f'Path({str(helper_observation)!r}).write_text(json.dumps({{"path": __file__}}))\n')
        package = tooling / "package.sh"
        package.write_text(package.read_text().replace(
            'package_main() { ', 'package_main() { '
            '"$ALAB_PYTHON" -I "$(dirname "${BASH_SOURCE[0]}")/fixture_helper.py" || return 1; '))
        promotion_observation = self.home / "fetched-promotion.json"
        promote = tooling / "promote.py"
        promote.write_text(promote.read_text() + f'''
_fixture_promote = promote
def promote(*args, **kwargs):
    Path({str(promotion_observation)!r}).write_text(json.dumps({{"path": __file__}}))
    return _fixture_promote(*args, **kwargs)
''')
        retention_observation = self.home / "fetched-retention.json"
        retention = tooling / "retention.py"
        retention.write_text(retention.read_text() + f'''
import json
_fixture_cleanup_allowed = cleanup_allowed
def cleanup_allowed(repo):
    Path({str(retention_observation)!r}).write_text(json.dumps({{"path": __file__}}))
    return _fixture_cleanup_allowed(repo)
''')
        (self.repo / ".gitignore").write_text("owner-cache/\n")
        self.git("add", "alab", ".gitignore")
        fresh = self.publish_next_source()

        # The canonical entry is a working old bootstrap; its package/promotion
        # helpers deliberately cannot finish the transaction. Owner edits stay put.
        (tooling / "local_update.py").write_bytes(bootstrap)
        package.write_text('package_main() { echo "stale package helper" >&2; return 97; }\n')
        promote.write_text(promote.read_text() + '\ndef promote(*args, **kwargs):\n'
                           '    raise RuntimeError("stale promotion helper")\n')
        (tooling / "lib.sh").write_text(
            'alab_package_cache_locked() { echo "stale lock helper" >&2; return 97; }\n')
        guardian.write_text('raise RuntimeError("stale guardian")\n')
        retention.write_text('raise RuntimeError("stale retention helper")\n')
        (tooling / "fixture_helper.py").write_text('raise RuntimeError("stale helper")\n')
        untracked = self.repo / "owner work.txt"
        untracked.write_text("leave this draft alone\n")
        ignored = self.repo / "owner-cache/kept"
        ignored.parent.mkdir()
        ignored.write_text("leave this cache alone\n")
        preserved = {path: path.read_bytes() for path in (
            tooling / "local_update.py", package, promote, tooling / "fixture_helper.py",
            tooling / "lib.sh", guardian, retention, untracked, ignored)}
        before = {args: self.git(*args) for args in (
            ("rev-parse", "HEAD"), ("ls-files", "--stage"),
            ("status", "--porcelain=v1", "--untracked-files=all", "--ignored"),
            ("diff", "--binary"))}

        # Observe actual Git calls and a nested public entry, using private files
        # only. The existing builder checks the kernel lock independently too.
        git_calls = self.home / "git-calls.jsonl"
        git_bin = self.home / "fixture-bin"
        git_bin.mkdir()
        real_git = shutil.which("git")
        wrapper = git_bin / "git"
        wrapper.write_text(PYTHON_ENTRY + f'''import json, os, sys
from pathlib import Path
with Path({str(git_calls)!r}).open("a") as stream:
    stream.write(json.dumps(sys.argv[1:]) + "\\n")
os.execv({real_git!r}, [{real_git!r}, *sys.argv[1:]])
''')
        wrapper.chmod(0o755)
        self.environment["PATH"] = str(git_bin) + os.pathsep + os.environ.get("PATH", "/usr/bin:/bin")
        builder = Path(self.environment["ALAB_LOCAL_TEST_BUILD"])
        builder.write_text(builder.read_text() + f'''
import subprocess
nested = subprocess.run([sys.executable, "-I", "-B", {str(tooling / 'local_update.py')!r},
    "--repo", {str(self.repo)!r}, "--out", {str(self.out)!r}, "--prefix", {str(self.prefix)!r}],
    capture_output=True, text=True, timeout=60)
assert nested.returncode == 75, (nested.returncode, nested.stdout, nested.stderr)
''')
        done = self.run_command([
            sys.executable, "-I", str(tooling / "local_update.py"),
            "--out", str(self.out), "--prefix", str(self.prefix)], self.environment)
        self.assertEqual(done.returncode, 0, done.stderr + done.stdout)
        status = json.loads((self.repo / ".git/alab-local-update.status.json").read_text())
        self.assertEqual(status["controller_source"], fresh)
        self.assertEqual(status["fixture_controller"], "fetched")
        self.assertEqual(status["controller_python"], sys.executable)
        controller_path = Path(status["controller_path"])
        self.assertFalse(controller_path.exists(), "clean source-only controller is retired")
        self.assertEqual(json.loads(helper_observation.read_text())["path"],
                         str(controller_path / "alab/fixture_helper.py"))
        self.assertEqual(json.loads(promotion_observation.read_text())["path"],
                         str(controller_path / "alab/promote.py"))
        self.assertEqual(json.loads(guardian_observation.read_text())["path"],
                         str(controller_path / "alab/command_guard.py"))
        self.assertEqual(json.loads(retention_observation.read_text())["path"],
                         str(controller_path / "alab/retention.py"))
        self.assertEqual(self.selected().name, f"0.164.0-dev.3+g{fresh[:9]}")
        self.assertEqual(json.loads(self.journal.read_text()), {"source": fresh, "lock": "held"})
        observed = [json.loads(line) for line in git_calls.read_text().splitlines()]
        self.assertEqual(sum("fetch" in call for call in observed), 1)
        self.assertEqual(sum("worktree" in call and "add" in call for call in observed), 1)
        self.assertEqual(sum("worktree" in call and "remove" in call for call in observed), 1)
        ancestry = [call for call in observed if "merge-base" in call]
        self.assertTrue(ancestry)
        self.assertTrue(all(call[call.index("-C") + 1] == str(self.repo) for call in ancestry))
        for arguments, expected in before.items():
            self.assertEqual(self.git(*arguments), expected)
        for path, contents in preserved.items():
            self.assertEqual(path.read_bytes(), contents)

    def test_marked_clean_controller_is_kept_after_success_and_body_error(self):
        self.keep_scratch = True
        (self.repo / ".git/alab-preserve-builds").write_text("retain every worktree\n")
        actual_git = FIXTURES.UPDATE.git
        removed = []

        def retained_git(repo, *arguments):
            if arguments[:2] == ("worktree", "remove"):
                removed.append(Path(arguments[-1]))
                return ""
            return actual_git(repo, *arguments)

        for fails in (False, True):
            with self.subTest(body_error=fails), mock.patch.object(FIXTURES.UPDATE, "git", retained_git):
                failure = RuntimeError("owned fixture body error")
                if fails:
                    with self.assertRaises(RuntimeError) as caught:
                        with FIXTURES.UPDATE.controller_checkout(self.repo, self.source) as controller:
                            raise failure
                    self.assertIs(caught.exception, failure)
                else:
                    with FIXTURES.UPDATE.controller_checkout(self.repo, self.source) as controller:
                        pass
                self.assertTrue(controller.is_dir())
                self.assertEqual(actual_git(controller, "rev-parse", "HEAD"), self.source)
                self.assertEqual(removed, [])

    def test_marked_empty_failed_controller_registration_is_kept(self):
        self.keep_scratch = True
        (self.repo / ".git/alab-preserve-builds").write_text("retain failed worktree paths\n")
        actual_git = FIXTURES.UPDATE.git
        allocated, removed = [], []
        failure = RuntimeError("owned registration failure")

        def failed_git(repo, *arguments):
            if arguments[:2] == ("worktree", "add"):
                allocated.append(Path(arguments[-2]))
                raise failure
            return actual_git(repo, *arguments)

        def retain_rmdir(path):
            removed.append(Path(path))

        with (mock.patch.object(FIXTURES.UPDATE, "git", failed_git),
              mock.patch.object(Path, "rmdir", retain_rmdir), self.assertRaises(RuntimeError) as caught):
            with FIXTURES.UPDATE.controller_checkout(self.repo, self.source):
                self.fail("registration failed before yielding")
        self.assertIs(caught.exception, failure)
        self.assertEqual(len(allocated), 1)
        self.assertTrue(allocated[0].is_dir())
        self.assertEqual(list(allocated[0].iterdir()), [])
        self.assertEqual(removed, [])

    def test_unmarked_clean_controller_requests_normal_cleanup_without_deleting_evidence(self):
        self.keep_scratch = True
        actual_git = FIXTURES.UPDATE.git
        removed = []

        def retained_git(repo, *arguments):
            if arguments[:2] == ("worktree", "remove"):
                removed.append(Path(arguments[-1]))
                return ""
            return actual_git(repo, *arguments)

        with mock.patch.object(FIXTURES.UPDATE, "git", retained_git):
            with FIXTURES.UPDATE.controller_checkout(self.repo, self.source) as controller:
                pass
        self.assertEqual(removed, [controller])
        self.assertTrue(controller.is_dir(), "the normal cleanup request is recorded, never executed")

    def test_failed_controller_cleanup_keeps_the_registration_error_and_directory(self):
        self.keep_scratch = True
        actual_git = FIXTURES.UPDATE.git
        actual_iterdir = Path.iterdir
        for stage in ("enumeration", "removal"):
            with self.subTest(stage=stage):
                allocated, removed, remove_requests = [], [], []
                failure = RuntimeError("owned registration failure")

                def failed_git(repo, *arguments):
                    if arguments[:2] == ("worktree", "add"):
                        allocated.append(Path(arguments[-2]))
                        raise failure
                    if arguments[:2] == ("worktree", "remove"):
                        remove_requests.append(arguments)
                        return ""
                    return actual_git(repo, *arguments)

                def failed_iterdir(path):
                    if stage == "enumeration" and allocated and path == allocated[0]:
                        raise PermissionError(errno.EACCES, "owned enumeration failure")
                    return actual_iterdir(path)

                def failed_rmdir(path):
                    removed.append(Path(path))
                    raise OSError(errno.ENOTEMPTY, "owned removal failure")

                with (mock.patch.object(FIXTURES.UPDATE, "git", failed_git),
                      mock.patch.object(Path, "iterdir", failed_iterdir),
                      mock.patch.object(Path, "rmdir", failed_rmdir),
                      self.assertRaises(Exception) as caught):
                    with FIXTURES.UPDATE.controller_checkout(self.repo, self.source):
                        self.fail("registration failed before yielding")
                self.assertEqual(len(allocated), 1)
                self.assertTrue(allocated[0].is_dir())
                self.assertEqual(list(actual_iterdir(allocated[0])), [])
                self.assertEqual(remove_requests, [])
                self.assertEqual(removed, [] if stage == "enumeration" else allocated)
                self.assertIs(caught.exception, failure)

    def test_unknown_controller_cleanup_policy_keeps_clean_source(self):
        self.keep_scratch = True
        actual_git = FIXTURES.UPDATE.git
        removed = []

        def retained_git(repo, *arguments):
            if arguments[:2] == ("worktree", "remove"):
                removed.append(Path(arguments[-1]))
                return ""
            return actual_git(repo, *arguments)

        with (mock.patch.object(FIXTURES.UPDATE.runpy, "run_path", side_effect=OSError("unreadable policy")),
              mock.patch.object(FIXTURES.UPDATE, "git", retained_git)):
            with FIXTURES.UPDATE.controller_checkout(self.repo, self.source) as controller:
                pass
        self.assertTrue(controller.is_dir())
        self.assertEqual(removed, [])

    def test_marked_same_source_poll_creates_no_controller_worktree(self):
        self.keep_scratch = True
        self.select(self.new)
        (self.repo / ".git/alab-preserve-builds").write_text("preserve all controller worktrees\n")
        before = self.git("worktree", "list", "--porcelain")
        calls = self.home / "noop-git-calls.jsonl"
        git_bin = self.home / "noop-bin"
        git_bin.mkdir()
        real_git = shutil.which("git")
        wrapper = git_bin / "git"
        wrapper.write_text(PYTHON_ENTRY + f'''import json, os, sys
from pathlib import Path
with Path({str(calls)!r}).open("a") as stream:
    stream.write(json.dumps(sys.argv[1:]) + "\\n")
if "worktree" in sys.argv and "remove" in sys.argv:
    sys.exit(0)
os.execv({real_git!r}, [{real_git!r}, *sys.argv[1:]])
''')
        wrapper.chmod(0o755)
        self.environment["PATH"] = str(git_bin) + os.pathsep + os.environ.get("PATH", "/usr/bin:/bin")
        done = self.run_job()
        self.assertEqual(done.returncode, 0, done.stderr + done.stdout)
        observed = [json.loads(line) for line in calls.read_text().splitlines()]
        self.assertFalse(any("worktree" in call and "add" in call for call in observed))
        self.assertEqual(self.git("worktree", "list", "--porcelain"), before)
        self.assertEqual(self.selected(), self.new)
        self.assertFalse(self.journal.exists())
        self.assertFalse(self.calls.exists())
        status = json.loads((self.repo / ".git/alab-local-update.status.json").read_text())
        self.assertEqual(status["initial_selected_checkout"], str(self.new))
        self.assertEqual(status["target_source"], self.source)
        self.assertNotIn("controller_path", status)

    def test_controller_preserves_changed_untracked_and_ignored_work(self):
        self.keep_scratch = True
        for kind in ("tracked", "untracked", "ignored"):
            with self.subTest(kind=kind):
                self.select(self.old)
                controller = self.repo / "alab/local_update.py"
                original = controller.read_text()
                destination = {"tracked": "alab/lib.sh", "untracked": "owner-note",
                               "ignored": "owner-cache/kept"}[kind]
                if kind == "ignored":
                    (self.repo / ".gitignore").write_text("owner-cache/\n")
                controller.write_text(original.replace(
                    '    args = parser().parse_args(argv)\n',
                    '    if source is not None:\n'
                    f'        path = ROOT / {destination!r}\n'
                    '        path.parent.mkdir(parents=True, exist_ok=True)\n'
                    '        with path.open("a") as stream:\n'
                    '            stream.write("retained controller work\\n")\n'
                    '    args = parser().parse_args(argv)\n', 1))
                paths = ["alab/local_update.py"] + ([".gitignore"] if kind == "ignored" else [])
                self.git("add", *paths)
                fresh = self.publish_next_source()
                done = self.run_job()
                self.assertEqual(done.returncode, 0, done.stderr + done.stdout)
                status = json.loads((self.repo / ".git/alab-local-update.status.json").read_text())
                kept = Path(status["controller_path"])
                self.assertTrue((kept / destination).read_text().endswith("retained controller work\n"))
                self.assertIn("kept controller source", done.stderr)
                self.assertEqual(self.selected().name, f"0.164.0-dev.3+g{fresh[:9]}")
                # An unchanged source allocates nothing and never resets the
                # retained controller from the completed forward update.
                before = self.git("worktree", "list", "--porcelain")
                again = self.run_job()
                self.assertEqual(again.returncode, 0, again.stderr + again.stdout)
                later = json.loads((self.repo / ".git/alab-local-update.status.json").read_text())
                self.assertNotIn("controller_path", later)
                self.assertEqual(self.git("worktree", "list", "--porcelain"), before)
                self.assertTrue((kept / destination).read_text().endswith("retained controller work\n"))
                controller.write_text(original)

    def test_contended_package_cache_never_fetches_builds_or_selects(self):
        self.cache.mkdir()
        status = self.repo / ".git/alab-local-update.status.json"
        previous = b'{"reason":"a prior completed poll"}'
        status.write_bytes(previous)
        self.git("update-ref", "refs/remotes/origin/main", self.old_source)
        with (self.cache / ".alab-package.lockf").open("a") as stream:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
            done = self.run_job()
        self.assertEqual(done.returncode, 75, done.stderr + done.stdout)
        self.assertIn("another packager", done.stderr)
        self.assertEqual(self.git("rev-parse", "refs/remotes/origin/main"), self.old_source)
        self.assertEqual(self.selected(), self.old)
        self.assertFalse(self.journal.exists())
        self.assertFalse(self.calls.exists())
        self.assertEqual(status.read_bytes(), previous)

    def disk_poll(self, low_path, *, unreadable=False):
        # The bootstrap and real fetched guardian share only a simulated space
        # measurement. All Git worktree, handoff and refusal paths remain real.
        self.keep_scratch = True
        launcher = self.home / "disk-fixture-python"
        if not launcher.exists():
            launcher.write_text(PYTHON_ENTRY + f'''import json, os, runpy, sys
from pathlib import Path
from types import SimpleNamespace
sys.dont_write_bytecode = True
def space(path):
    os.stat(path)  # Preserve nearest-existing-ancestor behavior.
    path = Path(path).resolve()
    low = path == Path(os.environ["ALAB_TEST_LOW_DISK"]).resolve()
    with open(os.environ["ALAB_TEST_DISK_LOG"], "a") as log:
        print(json.dumps({{"path": str(path), "low": low}}), file=log)
    if low and os.environ["ALAB_TEST_UNREADABLE_DISK"] == "1":
        raise PermissionError("fixture disk measurement unavailable")
    return SimpleNamespace(f_bavail=(19 if low else 35) * 2**30, f_frsize=1)
os.statvfs = space
args = sys.argv[1:]
while args and args[0] in ("-I", "-B"):
    args.pop(0)
if args[0] == "-c":
    sys.argv = ["-c", *args[2:]]
    exec(compile(args[1], "<disk-fixture-command>", "exec"))
else:
    sys.argv = args
    runpy.run_path(args[0], run_name="__main__")
''')
            launcher.chmod(0o700)
        environment = {key: self.environment[key] for key in
                       ("HOME", "ALAB_ATERM", "ALAB_PACKAGE_CACHE", "ATPKG_BIN",
                        "ALAB_LOCAL_TEST_BUILD")}
        environment.update(PATH="/usr/bin:/bin:/usr/sbin:/sbin", ALAB_PYTHON=str(launcher),
                           ALAB_CARGO_HOME=self.environment.get("ALAB_CARGO_HOME",
                                                               str(self.home / "cargo home")),
                           TMPDIR=self.environment.get("TMPDIR", str(self.home / "temporary")),
                           ALAB_TEST_LOW_DISK=str(low_path),
                           ALAB_TEST_DISK_LOG=str(self.home / "disk-measurements.jsonl"),
                           ALAB_TEST_UNREADABLE_DISK="1" if unreadable else "0")
        return self.run_command([
            str(launcher), "-I", str(self.repo / "alab/local_update.py"),
            "--repo", str(self.repo), "--out", str(self.out), "--prefix", str(self.prefix),
        ], environment)

    def test_repeated_low_disk_polls_allocate_no_controller_worktrees(self):
        self.keep_scratch = True
        print(f"retained low-disk poll fixture: {self.home}", flush=True)
        fresh = self.publish_next_source()
        before = self.git("worktree", "list", "--porcelain")
        marker = self.marker.read_bytes()
        observations = []
        for attempt in (1, 2):
            done = self.disk_poll(self.repo / ".git")
            (self.home / f"low-disk-{attempt}-stdout").write_text(done.stdout)
            (self.home / f"low-disk-{attempt}-stderr").write_text(done.stderr)
            observations.append({"exit": done.returncode,
                                 "controllers": sorted(str(p) for p in
                                     (self.repo / ".git").glob("alab-local-controller-*"))})
            self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
            self.assertIn("19.00 GiB", done.stderr + done.stdout)
        (self.home / "low-disk-result.json").write_text(json.dumps(observations, indent=2) + "\n")
        self.assertEqual(self.git("rev-parse", "refs/remotes/origin/main"), fresh,
                         "the ordinary single fetch still observes the newer source")
        self.assertEqual(self.marker.read_bytes(), marker)
        self.assertFalse(self.journal.exists())
        self.assertFalse(self.calls.exists())
        self.assertEqual(observations[-1]["controllers"], [],
                         "repeated resource refusal must not allocate retained source trees")
        self.assertEqual(self.git("worktree", "list", "--porcelain"), before)
        status = json.loads((self.repo / ".git/alab-local-update.status.json").read_text())
        self.assertTrue(status["complete"])
        self.assertEqual(status["exit"], 1)
        self.assertIn("19.00 GiB", status["reason"])

    def test_each_other_build_volume_refuses_before_source_allocation(self):
        self.keep_scratch = True
        self.publish_next_source()
        cargo = self.home / "cargo volume"
        temporary = self.home / "temporary volume"
        self.environment.update(ALAB_CARGO_HOME=str(cargo), TMPDIR=str(temporary))
        for path in (self.cache, self.home / "Developer", cargo, temporary):
            path.mkdir()
        before = self.git("worktree", "list", "--porcelain")
        marker = self.marker.read_bytes()
        for path in (self.cache, self.out, self.home / "Developer", cargo, Path("/tmp"), temporary):
            with self.subTest(low=path):
                done = self.disk_poll(path)
                self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                self.assertIn("19.00 GiB", done.stdout + done.stderr)
                self.assertEqual(self.marker.read_bytes(), marker)
                self.assertFalse(self.journal.exists())
                self.assertFalse(self.calls.exists())
                self.assertEqual(list((self.repo / ".git").glob("alab-local-controller-*")), [])
                self.assertEqual(self.git("worktree", "list", "--porcelain"), before)

    def test_sufficient_disk_preflight_keeps_successful_public_handoff(self):
        self.keep_scratch = True
        fresh = self.publish_next_source()
        (self.repo / ".git/alab-preserve-builds").write_text("Retain successful fixture source\n")
        done = self.disk_poll(self.home / "unused low-volume name")
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        self.assertEqual(json.loads(self.journal.read_text()), {"source": fresh, "lock": "held"})
        self.assertEqual(self.selected().name, f"0.164.0-dev.3+g{fresh[:9]}")
        self.assertTrue(self.calls.exists())
        self.assertEqual(len(list((self.repo / ".git").glob("alab-local-controller-*"))), 1)

    def test_low_linked_output_volume_refuses_before_source_allocation(self):
        self.keep_scratch = True
        self.publish_next_source()
        destination = self.home / "linked artifact volume"
        self.root.rename(destination)
        self.root.symlink_to(destination, target_is_directory=True)
        self.old = destination / self.old.name
        self.select(self.old)
        before = self.git("worktree", "list", "--porcelain")
        marker = self.marker.read_bytes()
        done = self.disk_poll(destination)
        self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
        self.assertIn("19.00 GiB", done.stdout + done.stderr)
        self.assertEqual(self.marker.read_bytes(), marker)
        self.assertFalse(self.journal.exists())
        self.assertFalse(self.calls.exists())
        self.assertEqual(list((self.repo / ".git").glob("alab-local-controller-*")), [])
        self.assertEqual(self.git("worktree", "list", "--porcelain"), before)

    def test_unreadable_controller_volume_refuses_before_source_allocation(self):
        self.keep_scratch = True
        self.publish_next_source()
        marker = self.marker.read_bytes()
        before = self.git("worktree", "list", "--porcelain")
        done = self.disk_poll(self.repo / ".git", unreadable=True)
        self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
        self.assertIn("fixture disk measurement unavailable", done.stdout + done.stderr)
        self.assertEqual(self.marker.read_bytes(), marker)
        self.assertFalse(self.journal.exists())
        self.assertFalse(self.calls.exists())
        self.assertEqual(list((self.repo / ".git").glob("alab-local-controller-*")), [])
        self.assertEqual(self.git("worktree", "list", "--porcelain"), before)

    def test_dispatch_probe_refuses_nonregular_or_link_locks_without_fetching(self):
        self.cache.mkdir()
        lock = self.cache / ".alab-package.lockf"
        target = self.home / "lock target"
        target.write_bytes(b"preserve this private target\n")
        status = self.repo / ".git/alab-local-update.status.json"
        previous = b'{"reason":"a prior completed poll"}'
        status.write_bytes(previous)
        self.git("update-ref", "refs/remotes/origin/main", self.old_source)
        for kind in ("symlink", "dangling", "fifo", "directory"):
            with self.subTest(lock=kind):
                if kind in ("symlink", "dangling"):
                    lock.symlink_to(target if kind == "symlink" else self.home / "missing target")
                elif kind == "fifo":
                    os.mkfifo(lock, 0o600)
                else:
                    lock.mkdir()
                try:
                    done = self.run_job()
                    self.assertEqual(done.returncode, 1, done.stderr + done.stdout)
                    self.assertEqual(self.git("rev-parse", "refs/remotes/origin/main"), self.old_source)
                    self.assertEqual(self.selected(), self.old)
                    self.assertEqual(status.read_bytes(), previous)
                    self.assertEqual(target.read_bytes(), b"preserve this private target\n")
                    self.assertFalse(self.journal.exists())
                    self.assertFalse(self.calls.exists())
                finally:
                    lock.rmdir() if kind == "directory" else lock.unlink()

    def test_dispatch_probe_rejects_home_or_relative_paths_before_opening(self):
        for cache in ("relative", str(Path.home()), str(Path.home().parent),
                      FIXTURES.UPDATE.pwd.getpwuid(os.getuid()).pw_dir):
            with self.subTest(cache=cache), mock.patch.object(
                    FIXTURES.UPDATE.os, "open", side_effect=AssertionError("probe traversed a rejected root")):
                with self.assertRaises(ValueError):
                    FIXTURES.UPDATE.cache_busy(cache)

    def test_fetched_guardian_rechecks_a_cache_that_became_busy_after_the_probe(self):
        fresh = self.publish_next_source()
        self.package(fresh, 3)
        bootstrap = self.repo / "alab/local_update.py"
        # Model the probe's now-stale free observation; actual transaction
        # admission must still use the fetched guardian's kernel lock.
        original = bootstrap.read_text()
        observed = original.replace("if cache_busy(cache):", "if False:  # stale free probe fixture")
        self.assertNotEqual(observed, original)
        bootstrap.write_text(observed)
        self.cache.mkdir()
        before = self.marker.read_bytes()
        with (self.cache / ".alab-package.lockf").open("a") as stream:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
            done = self.run_job()
        self.assertEqual(done.returncode, 75, done.stderr + done.stdout)
        self.assertEqual(self.git("rev-parse", "refs/remotes/origin/main"), fresh)
        self.assertEqual(self.marker.read_bytes(), before)
        self.assertEqual(bootstrap.read_text(), observed)
        self.assertFalse(self.journal.exists())
        self.assertFalse(self.calls.exists())
        self.assertEqual(len(self.git("worktree", "list", "--porcelain").split("worktree ")) - 1, 2,
                         "a nonzero public handoff retains its source conservatively")

    def test_public_fixed_source_requires_scope_and_never_fetches_or_dispatches(self):
        fresh = self.publish_next_source()
        candidate = self.package(fresh, 3)
        self.git("remote", "remove", "origin")
        self.git("update-ref", "refs/remotes/origin/main", fresh)
        for arguments in (("--fixed-source", fresh),
                          ("--fixed-source", self.old_source, "--expected-checkout", str(self.old),
                           "--expected-prefix", str(self.prefix)),
                          ("--fixed-source", fresh, "--expected-checkout", "relative",
                           "--expected-prefix", str(self.prefix))):
            with self.subTest(arguments=arguments):
                done = self.run_job(*arguments)
                self.assertEqual(done.returncode, 1, done.stderr + done.stdout)
                self.assertEqual(self.selected(), self.old)
                self.assertFalse(self.cache.exists())
                self.assertFalse(self.calls.exists())
        before = self.git("worktree", "list", "--porcelain")
        done = self.run_job("--fixed-source", fresh, "--expected-checkout", str(self.old),
                            "--expected-prefix", str(self.prefix))
        self.assertEqual(done.returncode, 0, done.stderr + done.stdout)
        self.assertEqual(self.selected(), candidate)
        self.assertEqual(self.git("worktree", "list", "--porcelain"), before)
        self.assertFalse(self.journal.exists(), "the exact published candidate requires no builder")
        status = json.loads((self.repo / ".git/alab-local-update.status.json").read_text())
        self.assertEqual(status["controller_source"], fresh)
        self.assertEqual(status["controller_path"], str(self.repo))

    def test_abnormal_public_handoff_preserves_source_with_a_remaining_child(self):
        self.keep_scratch = True
        self.publish_next_source()
        actual_run = subprocess.run
        for interrupted in (False, True):
            with self.subTest(interrupted=interrupted):
                remaining = []

                def handoff(command, *args, **kwargs):
                    if (len(command) > 3 and command[:3] == [sys.executable, "-I", "-B"]
                            and Path(command[3]).name == "local_update.py"):
                        source = Path(command[3])
                        child = subprocess.Popen([
                            sys.executable, "-I", "-B", "-c",
                            'from pathlib import Path; import sys; '
                            'print("ready", flush=True); sys.stdin.buffer.read(1); '
                            'assert Path(sys.argv[1]).is_file(), "source vanished under child"',
                            str(source)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, start_new_session=True)
                        remaining.append((child, source))
                        self.assertTrue(select.select([child.stdout], [], [], 60)[0],
                                        "remaining-child readiness exceeded hang detector")
                        self.assertEqual(child.stdout.readline(), b"ready\n")
                        if interrupted:
                            raise KeyboardInterrupt("private handoff interruption")
                        return subprocess.CompletedProcess(command, 1)
                    return actual_run(command, *args, **kwargs)

                try:
                    with mock.patch.object(FIXTURES.UPDATE.subprocess, "run", handoff):
                        arguments = ["--repo", str(self.repo), "--out", str(self.out),
                                     "--prefix", str(self.prefix), "--aterm", self.aterm]
                        if interrupted:
                            with self.assertRaises(KeyboardInterrupt):
                                FIXTURES.UPDATE.run_locked(arguments)
                        else:
                            self.assertEqual(FIXTURES.UPDATE.run_locked(arguments), 1)
                    self.assertEqual(len(remaining), 1)
                    child, source = remaining[0]
                    self.assertTrue(source.is_file(), "unknown handoff retains the whole source")
                    os.kill(child.pid, 0)
                    stdout, stderr = child.communicate(input=b"x", timeout=60)
                    self.assertEqual((child.returncode, stdout, stderr), (0, b"", b""))
                    self.assertTrue(source.is_file(), "the preserved source is not retired later")
                    self.assertEqual(self.selected(), self.old)
                    self.assertFalse(self.journal.exists())
                    self.assertFalse(self.calls.exists())
                finally:
                    for child, _source in remaining:
                        if child.returncode is None:
                            # This owned leader is still unreaped; kill its group
                            # before the final wait, preserving all fixture files.
                            try:
                                os.killpg(child.pid, signal.SIGKILL)
                            except ProcessLookupError:
                                pass
                            child.communicate(timeout=60)

    def test_public_entry_keeps_status_in_the_exact_external_common_directory(self):
        for name in ("external metadata ", "external meta\rdata "):
            with self.subTest(directory=name):
                common = self.home / name
                trimmed = common.with_name(name.rstrip())
                trimmed.mkdir()
                self.git("init", "-q", "--separate-git-dir", str(common), str(self.repo))
                done = self.run_job()
                self.assertEqual(done.returncode, 0, done.stderr + done.stdout)
                path = common / "alab-local-update.status.json"
                self.assertTrue(path.is_file(), "the actual Git directory owns the journal")
                status = json.loads(path.read_bytes())
                self.assertTrue(status["complete"])
                self.assertEqual(status["target_source"], self.git("rev-parse", "HEAD"))
                shown = self.run_job("--status")
                self.assertEqual(shown.returncode, 0, shown.stderr)
                self.assertEqual(json.loads(shown.stdout), status)
                self.assertEqual(list(trimmed.iterdir()), [], "a trimmed sibling is unrelated")

    def test_corrupt_exact_source_manifest_records_failure_without_rebuilding(self):
        path = self.new / "codex-package.json"
        for broken in ([], None):
            with self.subTest(manifest=broken):
                path.write_text(json.dumps(broken))
                done = self.run_job()
                self.assertEqual(done.returncode, 1, done.stderr + done.stdout)
                status = json.loads((self.repo / ".git/alab-local-update.status.json").read_text())
                self.assertEqual(status["exit"], 1)
                self.assertTrue(status["complete"])
                self.assertEqual(status["phase"], "completed")
                self.assertIn("JSON objects", status["reason"])
                self.assertEqual(self.selected(), self.old)
                self.assertFalse(self.journal.exists())
                calls = [json.loads(line) for line in self.calls.read_text().splitlines()]
                self.assertTrue(all(call == ["pkg", "link", "--help"] for call in calls))

    def test_prepared_launch_agent_binds_host_interpreter_and_prefix_without_starting(self):
        cargo_home = self.home / "custom cargo home"
        cargo_home.mkdir()
        requested = {"CARGO_BUILD_JOBS": "4", "RUST_TEST_THREADS": "4", "ALAB_CONTAIN": "1",
                     "ALAB_CARGO_HOME": str(cargo_home),
                     "ALAB_CARGO": str(self.home / "raw compiler/targo")}
        for name in requested:
            self.environment.pop(name, None)
        done = self.run_job("--launchd-plist")
        self.assertEqual(done.returncode, 0, done.stderr)
        job = plistlib.loads(done.stdout.encode())
        self.assertEqual(job["Label"], "systems.alab.codex-alab-local-update")
        self.assertTrue(job["RunAtLoad"])
        self.assertEqual(job["StartInterval"], 900)
        self.assertEqual(job["ProgramArguments"][:2], [sys.executable, "-I"])
        env = job["EnvironmentVariables"]
        self.assertEqual(env["ALAB_ATERM"], self.aterm)
        expected_builder = str(
            Path(shutil.which(self.environment["ALAB_PYTHON"]) or self.environment["ALAB_PYTHON"]).absolute()
        )
        self.assertEqual(env["ALAB_PYTHON"], expected_builder)
        self.assertEqual(env["ATPKG_BIN"], str(self.prefix / "bin"))
        self.assertEqual(env["ALAB_PACKAGE_CACHE"], str(self.cache))
        self.assertFalse(self.cache.exists())
        self.assertFalse(self.journal.exists())
        self.assertEqual(self.selected(), self.old)

        for name in requested:
            self.assertNotIn(name, env, "unset controls retain their existing defaults")

        self.environment.update(requested)
        prepared = self.run_job("--launchd-plist")
        self.assertEqual(prepared.returncode, 0, prepared.stderr)
        environment = plistlib.loads(prepared.stdout.encode())["EnvironmentVariables"]
        for name, value in requested.items():
            self.assertEqual(environment.get(name), value)

        def selected_home(env):
            observed = self.run_command([
                "/bin/bash", "-c", '. "$1/alab/lib.sh"; cd "$2"; alab_cargo_home',
                "prepared-cargo-home", str(ROOT), str(self.repo)], env)
            self.assertEqual(observed.returncode, 0, observed.stderr)
            return Path(observed.stdout.rstrip("\n"))

        self.assertEqual(selected_home(environment), cargo_home)
        without_custom = dict(environment)
        without_custom.pop("ALAB_CARGO_HOME")
        default_home = Path(self.git("rev-parse", "--path-format=absolute", "--git-common-dir")) / "alab-cargo-home"
        self.assertEqual(selected_home(without_custom), default_home)
        self.assertNotEqual(default_home, cargo_home)
        self.assertFalse(default_home.exists(), "preparing a job creates no replacement Cargo cache")
        self.assertNotIn("ALAB_LOCAL_TEST_BUILD", environment)
        self.assertFalse(self.cache.exists())
        self.assertFalse(self.journal.exists())
        self.assertEqual(self.selected(), self.old)

    def test_prepared_job_keeps_the_explicit_compiler_without_running_it(self):
        driver = self.home / 'compiler & <native> files' / 'targo "raw"'
        driver.parent.mkdir()
        called = self.home / "compiler-was-executed"
        driver.write_text(PYTHON_ENTRY + f""
                          "from pathlib import Path\n"
                          f"Path({str(called)!r}).write_text('unexpected compiler call')\n"
                          "raise SystemExit(97)\n")
        driver.chmod(0o700)
        self.environment["ALAB_CARGO"] = str(driver)
        marker_before = self.marker.read_bytes()
        prepared = self.run_job("--launchd-plist")
        self.assertEqual(prepared.returncode, 0, prepared.stderr)
        environment = plistlib.loads(prepared.stdout.encode())["EnvironmentVariables"]
        self.assertTrue(driver.is_absolute())
        self.assertEqual(environment.get("ALAB_CARGO"), str(driver))
        self.assertFalse(called.exists(), "printing a job must not execute its compiler")
        self.assertFalse(self.cache.exists())
        self.assertFalse(self.journal.exists())
        self.assertEqual(self.marker.read_bytes(), marker_before)
        self.assertEqual(self.selected(), self.old)
        self.assertEqual([json.loads(line) for line in self.calls.read_text().splitlines()],
                         [["pkg", "link", "--help"]])


if __name__ == "__main__":
    unittest.main()

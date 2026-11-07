#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Real package/updater entry wiring, stopping at an inert guardian recorder."""

import importlib.util
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
# A shebang cannot contain a Python path with spaces.
PYTHON_ENTRY = "#!/bin/sh\n'''exec' " + shlex.quote(sys.executable) + ' -I "$0" "$@"\n\' \'\'\'\n'


class DiskGuardWiringTests(unittest.TestCase):
    def setUp(self):
        self.fixture = Path(tempfile.mkdtemp(prefix="alab-disk-wiring-")).resolve()
        print(f"preserved disk-wiring fixture: {self.fixture}", flush=True)
        self.home = self.fixture / "account home"
        self.home.mkdir()
        self.output = self.fixture / "output volume/new packages"
        self.cargo = self.fixture / "cargo volume/new home"
        self.temporary = self.fixture / "temporary volume/new scratch"
        self.record = self.fixture / "guardian-argv.json"
        self.recorder = self.fixture / "fake-python"
        self.recorder.write_text(
            PYTHON_ENTRY + f"import json, os, sys\nfrom pathlib import Path\n"
            "if (any(Path(arg).name == 'package_cache.py' for arg in sys.argv[1:])\n"
            "    or sys.argv[1:] == ['-I', '-c', 'import os, pwd; print(pwd.getpwuid(os.getuid()).pw_dir)']):\n"
            f"    os.execv({sys.executable!r}, [{sys.executable!r}, *sys.argv[1:]])\n"
            f"with Path({str(self.record)!r}).open('x') as stream:\n"
            "    json.dump(sys.argv[1:], stream)\n")
        self.recorder.chmod(0o755)
        git_program = shutil.which("git")
        self.assertIsNotNone(git_program, "Git is required for disk-wiring fixtures")
        fixture_path = str(Path(git_program).parent) + ":/usr/bin:/bin:/usr/sbin:/sbin"
        self.environment = {"HOME": str(self.home), "PATH": fixture_path,
                            "ALAB_PYTHON": str(self.recorder),
                            "ALAB_PACKAGE_CACHE": str(self.fixture / "cache"),
                            "ALAB_CARGO_HOME": str(self.cargo), "TMPDIR": str(self.temporary)}

    def shell(self, body, *arguments):
        done = subprocess.run(["/bin/bash", "-c", body, "disk-fixture", str(ROOT), *arguments],
                              env=self.environment, capture_output=True, text=True, timeout=20)
        self.assertEqual(done.returncode, 0, done.stderr)
        return json.loads(self.record.read_text())

    def common_dir(self, repo):
        common = subprocess.run(["git", "-C", str(repo), "rev-parse", "--path-format=absolute",
                                 "--git-common-dir"], env=self.environment,
                                capture_output=True, text=True, check=True, timeout=60)
        return Path(common.stdout.strip())

    def assert_paths(self, args, common=None):
        self.assertEqual(args[args.index("--min-free-gib") + 1], "20")
        paths = [args[i + 1] for i, value in enumerate(args) if value == "--disk-path"]
        self.assertEqual(paths, [str(self.fixture / "cache"), str(self.output), str(self.output / "codex"),
                                 str(self.home / "Developer"), str(common or self.common_dir(ROOT)),
                                 str(self.cargo),
                                 "/tmp", str(self.temporary)])
        self.assertIn("--cache-lock", args)
        self.assertFalse(self.output.exists(), "wiring must not create a package")

    def test_package_entry_covers_all_actual_output_volumes(self):
        args = self.shell('. "$1/alab/package.sh"; shift; '
                          'alab_account_home() { printf "%s\\n" "$HOME"; }; '
                          'alab_project_selected() { :; }; package_selected_entry --out "$1" --no-promote', str(self.output))
        self.assert_paths(args)
        self.assertIn("--no-promote", args)

    def test_plain_cache_guard_and_read_only_package_modes_keep_old_defaults(self):
        args = self.shell('. "$1/alab/lib.sh"; shift; '
                          'alab_account_home() { printf "%s\\n" "$HOME"; }; '
                          'alab_package_cache_locked "$ALAB_PACKAGE_CACHE" /usr/bin/true')
        self.assertNotIn("--min-free-gib", args)
        self.assertNotIn("--disk-path", args)
        other = self.fixture / "read-only-argv.json"
        recorder = self.fixture / "read-only-recorder"
        recorder.write_text(self.recorder.read_text().replace(str(self.record), str(other)))
        recorder.chmod(0o755)
        environment = dict(self.environment, ALAB_PYTHON=str(recorder))
        done = subprocess.run(["/bin/bash", "-c", '. "$1/alab/package.sh"; package_entry --help',
                               "disk-fixture", str(ROOT)], env=environment,
                              capture_output=True, text=True, timeout=20)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertNotIn("--min-free-gib", json.loads(other.read_text()))

    def test_default_cargo_home_volume_comes_from_the_driving_clone(self):
        self.environment.pop("ALAB_CARGO_HOME")
        args = self.shell('. "$1/alab/lib.sh"; repo="$1"; shift; '
                          'alab_account_home() { printf "%s\\n" "$HOME"; }; '
                          'alab_package_cache_locked "$ALAB_PACKAGE_CACHE" '
                          '--build-disk "$1" "$repo" /usr/bin/true', str(self.output))
        common = self.common_dir(ROOT)
        self.assertIn(str(common), args)
        self.assertIn(str(common / "alab-cargo-home"), args)

    def test_linked_artifact_child_cannot_hide_a_low_output_volume(self):
        self.output.mkdir(parents=True)
        destination = self.fixture / "separate artifact volume"
        destination.mkdir()
        (self.output / "codex").symlink_to(destination, target_is_directory=True)
        args = self.shell('. "$1/alab/lib.sh"; repo="$1"; shift; '
                          'alab_account_home() { printf "%s\\n" "$HOME"; }; '
                          'alab_package_cache_locked "$ALAB_PACKAGE_CACHE" '
                          '--build-disk "$1" "$repo" /usr/bin/true', str(self.output))
        paths = [args[i + 1] for i, value in enumerate(args) if value == "--disk-path"]
        spec = importlib.util.spec_from_file_location("artifact_disk_guard", ROOT / "alab/command_guard.py")
        guard = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(guard)
        checked = []

        def space(path):
            checked.append(Path(path))
            free = 19 if Path(path) == destination else 35
            return type("Space", (), {"f_bavail": free * 2**30, "f_frsize": 1})()

        with (mock.patch.object(guard.os, "statvfs", side_effect=space),
              mock.patch.object(guard.subprocess, "Popen",
                                side_effect=AssertionError("low artifact volume spawned")) as spawn):
            with self.assertRaisesRegex(RuntimeError, "19.00 GiB"):
                guard.run(["unused"], disk_floor=guard.DiskFloor(paths, 20))
            spawn.assert_not_called()
        self.assertIn(self.output, checked)
        self.assertIn(destination, checked, "resolve the actual linked artifact destination")

    def test_missing_artifact_child_checks_its_existing_output_ancestor(self):
        self.output.mkdir(parents=True)
        child = self.output / "codex"
        args = self.shell('. "$1/alab/lib.sh"; repo="$1"; shift; '
                          'alab_account_home() { printf "%s\\n" "$HOME"; }; '
                          'alab_package_cache_locked "$ALAB_PACKAGE_CACHE" '
                          '--build-disk "$1" "$repo" /usr/bin/true', str(self.output))
        paths = [args[i + 1] for i, value in enumerate(args) if value == "--disk-path"]
        self.assertIn(str(child), paths)
        spec = importlib.util.spec_from_file_location("missing_artifact_disk_guard", ROOT / "alab/command_guard.py")
        guard = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(guard)
        checked = []

        def space(path):
            checked.append(Path(path))
            os.stat(path)  # The real missing child raises before the fake space value.
            return type("Space", (), {"f_bavail": 35 * 2**30, "f_frsize": 1})()

        with mock.patch.object(guard.os, "statvfs", side_effect=space):
            guard.DiskFloor([child], 20).check()
        self.assertEqual(checked, [child, self.output])
        self.assertFalse(child.exists(), "checking an absent output must not create it")

    def test_custom_cargo_home_cannot_hide_a_low_git_metadata_volume(self):
        repo = self.fixture / "working source"
        common = self.fixture / "metadata volume"
        subprocess.run(["git", "init", "--quiet", "--separate-git-dir", str(common), str(repo)],
                       env=self.environment, capture_output=True, text=True, check=True, timeout=60)
        args = self.shell('. "$1/alab/lib.sh"; repo="$2"; shift 2; '
                          'alab_account_home() { printf "%s\\n" "$HOME"; }; '
                          'alab_package_cache_locked "$ALAB_PACKAGE_CACHE" '
                          '--build-disk "$1" "$repo" /usr/bin/true', str(repo), str(self.output))
        self.assert_paths(args, common)
        paths = [args[i + 1] for i, value in enumerate(args) if value == "--disk-path"]
        spec = importlib.util.spec_from_file_location("disk_guard", ROOT / "alab/command_guard.py")
        guard = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(guard)

        def space(path):
            free = 19 if Path(path) == common else 35
            return type("Space", (), {"f_bavail": free * 2**30, "f_frsize": 1})()

        with (mock.patch.object(guard.os, "statvfs", side_effect=space),
              mock.patch.object(guard.subprocess, "Popen", side_effect=AssertionError("low Git volume spawned"))):
            with self.assertRaisesRegex(RuntimeError, "19.00 GiB"):
                guard.run(["unused"], disk_floor=guard.DiskFloor(paths, 20))

    def test_local_update_fixed_source_entry_enables_the_same_floor(self):
        spec = importlib.util.spec_from_file_location("disk_update", ROOT / "alab/local_update.py")
        update = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(update)
        argv = ["--repo", str(ROOT), "--out", str(self.output), "--fixed-source", "a" * 40,
                "--expected-checkout", str(self.fixture / "selected"),
                "--expected-prefix", str(self.fixture / "prefix")]
        # Avoid Git/source/selection work; execute only the real entry's shell wiring.
        with (mock.patch.object(update, "fixed_source_args"),
              mock.patch.dict(os.environ, self.environment, clear=True)):
            self.assertEqual(update.main(argv), 0)
        self.assert_paths(json.loads(self.record.read_text()))

    def test_sync_compiler_guard_covers_source_target_homes_and_scratch(self):
        target = self.fixture / "target volume/new target"
        trust = self.fixture / "trust volume/new home"
        args = self.shell('. "$1/alab/lib.sh"; cd "$1/codex-rs"; '
                          'export CARGO_TARGET_DIR="$2" CARGO_HOME="$3" TARGO_HOME="$4"; '
                          'alab_build_command /usr/bin/true',
                          str(target), str(self.cargo), str(trust))
        paths = [args[i + 1] for i, value in enumerate(args) if value == "--disk-path"]
        self.assertEqual(paths, [str(target), str(target / "build.noindex"), str(ROOT / "codex-rs"), str(ROOT / "codex-rs"),
                                 str(self.common_dir(ROOT)), str(self.cargo), str(trust), str(self.cargo),
                                 "/tmp", str(self.temporary)])
        self.assertIn("--inherit-fds", args)
        self.assertEqual(args[args.index("--min-free-gib") + 1], "20")
        self.assertFalse(target.exists(), "guard wiring does not allocate a build target")

    def test_routed_build_volume_is_checked_even_with_a_healthy_nominal_target(self):
        target = self.fixture / "routed target"
        build = self.fixture / "routed intermediates"
        self.environment.update(CARGO_BUILD_TARGET_DIR=str(target), CARGO_BUILD_BUILD_DIR=str(build))
        args = self.shell('. "$1/alab/lib.sh"; cd "$1/codex-rs"; alab_build_command /usr/bin/true')
        paths = [args[i + 1] for i, value in enumerate(args) if value == "--disk-path"]
        spec = importlib.util.spec_from_file_location("routed_disk_guard", ROOT / "alab/command_guard.py")
        guard = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(guard)
        for low in (target, build):
            with self.subTest(low=low):
                def space(path):
                    return type("Space", (), {"f_bavail": (19 if Path(path) == low else 35) * 2**30,
                                               "f_frsize": 1})()
                with (mock.patch.object(guard.os, "statvfs", side_effect=space),
                      mock.patch.object(guard.subprocess, "Popen", side_effect=AssertionError("low route spawned"))):
                    with self.assertRaisesRegex(guard.DiskSpaceError, "19.00 GiB"):
                        guard.run(["unused"], disk_floor=guard.DiskFloor(paths, 20))

    @unittest.skipUnless(sys.platform == "darwin", "native shared build shim is macOS-only")
    def test_unset_target_covers_the_native_shims_shared_intermediate_root(self):
        args = self.shell('. "$1/alab/lib.sh"; cd "$1/codex-rs"; alab_build_command /usr/bin/true')
        paths = [args[i + 1] for i, value in enumerate(args) if value == "--disk-path"]
        shared = self.home / "Library/Caches/aterm/.ubd/cargo-build.noindex"
        spec = importlib.util.spec_from_file_location("shared_disk_guard", ROOT / "alab/command_guard.py")
        guard = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(guard)
        def space(path):
            return type("Space", (), {"f_bavail": (19 if Path(path) == shared else 35) * 2**30,
                                       "f_frsize": 1})()
        with (mock.patch.object(guard.os, "statvfs", side_effect=space),
              mock.patch.object(guard.subprocess, "Popen", side_effect=AssertionError("low shared route spawned"))):
            with self.assertRaisesRegex(guard.DiskSpaceError, "19.00 GiB"):
                guard.run(["unused"], disk_floor=guard.DiskFloor(paths, 20))

    def test_build_template_and_target_precedence_preserve_the_effective_routes(self):
        target = self.fixture / "selected target"
        ignored = self.fixture / "lower priority target"
        self.environment.update(CARGO_TARGET_DIR=str(target), CARGO_BUILD_TARGET_DIR=str(ignored),
                                CARGO_BUILD_BUILD_DIR="{workspace-root}/route with spaces")
        args = self.shell('. "$1/alab/lib.sh"; cd "$1/codex-rs"; alab_build_command /usr/bin/true')
        paths = [Path(args[i + 1]) for i, value in enumerate(args) if value == "--disk-path"]
        self.assertIn(target, paths)
        self.assertNotIn(ignored, paths)
        self.assertIn(ROOT / "codex-rs/route with spaces", paths)

    def test_unresolved_build_template_stops_before_the_compiler_guard(self):
        self.environment['CARGO_BUILD_BUILD_DIR'] = "{unknown-route}/build"
        body = '. "$1/alab/lib.sh"; cd "$1/codex-rs"; alab_build_command /usr/bin/true'
        done = subprocess.run(['/bin/bash', '-c', body, 'unknown-route', str(ROOT)],
                              env=self.environment, capture_output=True, text=True, timeout=60)
        self.assertEqual(done.returncode, 75, done.stderr)
        self.assertIn('unresolved Cargo build directory', done.stderr)
        self.assertFalse(self.record.exists(), 'uncertain route starts no command')

    def test_cross_workspace_manifest_routes_its_template_beyond_the_callers_volume(self):
        workspace = self.fixture / "different workspace"
        workspace.mkdir()
        manifest = workspace / "Cargo.toml"
        manifest.write_text('[workspace]\nmembers = []\n')
        self.environment['CARGO_BUILD_BUILD_DIR'] = '{workspace-root}/intermediates'
        args = self.shell('. "$1/alab/lib.sh"; cd "$1/codex-rs"; '
                          'alab_build_command /usr/bin/true --manifest-path "$2"', str(manifest))
        paths = [args[i + 1] for i, value in enumerate(args) if value == '--disk-path']
        self.assertIn(str(workspace / 'target'), paths)
        self.assertIn(str(workspace / 'intermediates'), paths)
        spec = importlib.util.spec_from_file_location('manifest_disk_guard', ROOT / 'alab/command_guard.py')
        guard = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(guard)
        def space(path):
            return type('Space', (), {'f_bavail': (19 if Path(path) == workspace / 'intermediates' else 35) * 2**30,
                                      'f_frsize': 1})()
        with (mock.patch.object(guard.os, 'statvfs', side_effect=space),
              mock.patch.object(guard.subprocess, 'Popen', side_effect=AssertionError('low manifest route spawned'))):
            with self.assertRaisesRegex(guard.DiskSpaceError, '19.00 GiB'):
                guard.run(['unused'], disk_floor=guard.DiskFloor(paths, 20))

    def test_relative_build_override_is_relative_to_the_callers_cwd(self):
        workspace = self.fixture / 'manifest workspace'
        workspace.mkdir()
        manifest = workspace / 'Cargo.toml'
        manifest.write_text('[workspace]\nmembers = []\n')
        self.environment['CARGO_BUILD_BUILD_DIR'] = 'relative intermediates'
        args = self.shell('. "$1/alab/lib.sh"; cd "$1/codex-rs"; '
                          'alab_build_command /usr/bin/true --manifest-path="$2"', str(manifest))
        paths = [Path(args[i + 1]) for i, value in enumerate(args) if value == '--disk-path']
        self.assertIn(ROOT / 'codex-rs/relative intermediates', paths)
        self.assertNotIn(workspace / 'relative intermediates', paths)

    def test_cache_home_template_covers_both_explicit_cargo_and_trust_homes(self):
        trust = self.fixture / 'different trust home'
        self.environment.update(CARGO_HOME=str(self.cargo), TARGO_HOME=str(trust),
                                CARGO_BUILD_BUILD_DIR='{cargo-cache-home}/intermediates')
        args = self.shell('. "$1/alab/lib.sh"; cd "$1/codex-rs"; alab_build_command /usr/bin/true')
        paths = [Path(args[i + 1]) for i, value in enumerate(args) if value == '--disk-path']
        self.assertIn(self.cargo / 'intermediates', paths)
        self.assertIn(trust / 'intermediates', paths)

    def test_unit_cargo_is_guarded_inside_its_detached_session_with_argv_env_and_fd(self):
        compiler = self.fixture / "inert compiler"
        compiler.write_text(PYTHON_ENTRY + f'''import errno, json, os, sys
from pathlib import Path
try:
    fd = os.open('/dev/tty', os.O_RDONLY)
except OSError:
    terminal = False
else:
    os.close(fd)
    terminal = True
print(json.dumps({{'args': sys.argv[1:], 'session': os.getsid(0) == os.getpid(),
                  'terminal': terminal, 'stdin': sys.stdin.read(),
                  'descriptor': os.read(43, 64).decode(),
                  'environment': os.environ['GUARD_CONTEXT']}}))
raise SystemExit(23)
''')
        compiler.chmod(0o755)
        input_file = self.fixture / "caller input"
        input_file.write_text("inherited descriptor bytes")
        self.temporary.mkdir(parents=True)
        environment = dict(self.environment, ALAB_PYTHON=sys.executable,
                           CARGO_TARGET_DIR=str(self.fixture / "target.noindex"),
                           CARGO_HOME=str(self.cargo), TARGO_HOME=str(self.cargo),
                           GUARD_CONTEXT="value with spaces")
        body = ('set -euo pipefail; . "$1/alab/lib.sh"; . "$1/alab/unit-tests.sh"; '
                'cargo="$2"; packages=(-p fixture); exec 43<"$3"; '
                'unit_tests "$1" "$CARGO_TARGET_DIR" --exact "test with spaces"')
        done = subprocess.run(["/bin/bash", "-c", body, "unit-disk-fixture", str(ROOT),
                               str(compiler), str(input_file)], env=environment,
                              capture_output=True, text=True, timeout=60)
        self.assertEqual(done.returncode, 23, done.stderr)
        observed = json.loads(done.stdout)
        self.assertEqual(observed['args'], ['--unverified', 'test', '--locked', '--no-fail-fast',
                                             '--lib', '-p', 'fixture', '--', '--exact', 'test with spaces'])
        self.assertTrue(observed['session'])
        self.assertFalse(observed['terminal'])
        self.assertEqual(observed['stdin'], '')
        self.assertEqual(observed['descriptor'], input_file.read_text())
        self.assertEqual(observed['environment'], environment['GUARD_CONTEXT'])


if __name__ == "__main__":
    unittest.main()

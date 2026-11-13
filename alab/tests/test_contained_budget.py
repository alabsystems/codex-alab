#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise resource limits and account isolation through the macOS boundary."""

import ctypes
import errno
import json
import mmap
import os
from pathlib import Path
import secrets
import select
import shlex
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(Path("/usr/bin/sandbox-exec").is_file(), "requires macOS sandbox-exec")
class ContainedBudgetTests(unittest.TestCase):
    def contained_command(self, values, child=("/usr/bin/env",), reads=(), check=True,
                          writable_roots=()):
        with tempfile.TemporaryDirectory(prefix="codex-budget-") as directory:
            temporary = Path(directory)
            cargo_home = temporary / "cargo-home"
            cargo_home.mkdir()
            (cargo_home / ".alab-seeded").touch()
            writable = temporary / "output"
            writable.mkdir()
            environment = {key: value for key, value in os.environ.items()
                           if not key.startswith("ALAB_")}
            for key in ("CARGO_BUILD_JOBS", "RUST_TEST_THREADS", "CARGO_NET_OFFLINE"):
                environment.pop(key, None)
            environment.update(values, ALAB_CARGO_HOME=str(cargo_home))
            environment.setdefault("ALAB_PYTHON", sys.executable)
            command = 'set -euo pipefail; . "$1/alab/lib.sh"; shift; alab_contained "$@"'
            read_args = [argument for path in reads for argument in ("--read", str(path))]
            self.contained_child_eof = True
            process = subprocess.Popen(
                ["/bin/bash", "-c", command, "contained-budget", str(ROOT),
                 *read_args, str(writable), *map(str, writable_roots), "--", *child],
                cwd=ROOT, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                text=True, start_new_session=True,
            )
            self.contained_child_eof = False
            try:
                stdout, stderr = process.communicate(timeout=60)
            except subprocess.TimeoutExpired:
                # Only this test's new process group, including its sandboxed child.
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.communicate(timeout=60)
                self.contained_child_eof = True
                self.fail("contained child did not finish within the one-minute hang detector")
            self.contained_child_eof = True
            if not check:
                return process.returncode, stdout, stderr
            self.assertEqual(process.returncode, 0, stderr)
            return stdout

    def contained_environment(self, values):
        output = self.contained_command(values)
        return dict(line.split("=", 1) for line in output.splitlines() if "=" in line)

    def portable_runtime(self):
        import pwd

        runtime = Path(sys.base_prefix).resolve()
        home = Path(pwd.getpwuid(os.getuid()).pw_dir).resolve()
        if not runtime.is_relative_to(home) or runtime.parent == home:
            self.skipTest("selected portable Python must live below the protected account home")
        return runtime, home

    def test_selected_portable_python_imports_and_runtime_stays_read_only(self):
        import encodings

        runtime, _ = self.portable_runtime()
        # Prove ordinary file permissions would allow this open outside the boundary.
        # No bytes are written, inside or outside containment.
        descriptor = os.open(Path(encodings.__file__).resolve(), os.O_WRONLY)
        os.close(descriptor)
        child = r'''
import encodings, errno, json, os, ssl, sys, tomllib
from pathlib import Path
result = {"toml": tomllib.loads("value = 7")["value"], "ssl": ssl.OPENSSL_VERSION}
target = Path(encodings.__file__).resolve()
assert target.is_file()
try:
    descriptor = os.open(target, os.O_WRONLY)
except OSError as error:
    result["write"] = error.errno
else:
    os.close(descriptor)
    result["write"] = "allowed"
print(json.dumps(result))
'''
        result = json.loads(self.contained_command(
            {}, (sys.executable, "-I", "-B", "-c", child),
            writable_roots=(runtime.parent,)))
        self.assertEqual(result["toml"], 7, result)
        self.assertTrue(result["ssl"], result)
        self.assertIn(result["write"], (errno.EPERM, errno.EACCES), result)

    def test_runtime_grant_does_not_read_its_siblings_or_account_canaries(self):
        runtime, home = self.portable_runtime()
        with tempfile.TemporaryDirectory(prefix=".alab-runtime-sibling-", dir=runtime.parent) as sibling, \
                tempfile.TemporaryDirectory(prefix=".alab-runtime-home-", dir=home) as account:
            canaries = [Path(sibling) / "private.txt", Path(account) / "private.txt"]
            for canary in canaries:
                canary.write_text("owned-runtime-boundary-canary")
            child = r'''
import json, sys
from pathlib import Path
result = []
for name in sys.argv[1:]:
    try:
        Path(name).read_bytes()
        result.append("allowed")
    except OSError as error:
        result.append(error.errno)
print(json.dumps(result))
'''
            result = json.loads(self.contained_command(
                {}, (sys.executable, "-I", "-B", "-c", child, *map(str, canaries))))
            self.assertTrue(all(code in (errno.EPERM, errno.EACCES) for code in result), result)

    def test_unsafe_python_runtime_prefixes_are_refused_after_canonicalization(self):
        import pwd

        home = Path(pwd.getpwuid(os.getuid()).pw_dir).resolve()
        with tempfile.TemporaryDirectory(prefix="alab-python-prefix-") as directory:
            temporary = Path(directory)
            private_home = temporary / "private-home"
            private_home.mkdir()
            alias = temporary / "home-alias"
            alias.symlink_to(home, target_is_directory=True)
            for prefix in (home, home.parent, Path("/"), private_home, alias):
                with self.subTest(prefix=prefix):
                    wrapper = temporary / "python"
                    measured = temporary / "measured"
                    measured.unlink(missing_ok=True)
                    injection = ("import sys; "
                                 f"sys.prefix = sys.base_prefix = sys.exec_prefix = sys.base_exec_prefix = {str(prefix)!r}; "
                                 "exec(sys.argv[1])")
                    wrapper.write_text(
                        '#!/bin/sh\nif [ "$1 $2 $3" = "-I -B -c" ]; then\n'
                        '  case "$4" in *sys.base_exec_prefix*)\n'
                        f'    printf measured >{shlex.quote(str(measured))}\n'
                        f'    exec {shlex.quote(sys.executable)} -I -B -c {shlex.quote(injection)} "$4" ;;\n'
                        '  esac\n'
                        f'fi\nexec {shlex.quote(sys.executable)} "$@"\n')
                    wrapper.chmod(0o755)
                    code, _, stderr = self.contained_command(
                        {"ALAB_PYTHON": str(wrapper), "HOME": str(private_home)}, check=False)
                    self.assertTrue(measured.is_file(), "the runtime-prefix query was intercepted")
                    self.assertNotEqual(code, 0, stderr)
                    self.assertIn("Python runtime", stderr)

    def test_admitted_compile_and_test_limits_survive(self):
        environment = self.contained_environment({"CARGO_BUILD_JOBS": "6", "RUST_TEST_THREADS": "3",
                                                  "CARGO_NET_OFFLINE": "true"})
        self.assertEqual(environment.get("CARGO_BUILD_JOBS"), "6")
        self.assertEqual(environment.get("RUST_TEST_THREADS"), "3")
        self.assertEqual(environment.get("CARGO_NET_OFFLINE"), "true")

    def test_absent_limits_remain_absent(self):
        environment = self.contained_environment({})
        self.assertNotIn("CARGO_BUILD_JOBS", environment)
        self.assertNotIn("RUST_TEST_THREADS", environment)
        self.assertNotIn("CARGO_NET_OFFLINE", environment)

    def test_fixture_scratch_is_the_allocated_root_even_after_tmpdir_changes(self):
        child = r'''
import json, os, shutil, stat, tempfile
from pathlib import Path
root = Path(os.environ["ALAB_TEST_SCRATCH"])
assert root.resolve() == Path(os.environ["TMPDIR"]).resolve()
with tempfile.TemporaryDirectory(prefix="nested-", dir=root) as nested:
    os.environ["TMPDIR"] = nested
    path = root / "c123456"
    path.mkdir(mode=0o700)
    try:
        (path / "witness").write_text("owned fixture")
        print(json.dumps({"unchanged": os.environ["ALAB_TEST_SCRATCH"] == str(root),
                          "short": len(os.fsencode(path.resolve())) <= 32,
                          "private": stat.S_IMODE(root.stat().st_mode) == 0o700,
                          "forged": str(root) == "/tmp/not-the-allocated-root"}))
    finally:
        shutil.rmtree(path)
'''
        result = json.loads(self.contained_command(
            {"ALAB_TEST_SCRATCH": "/tmp/not-the-allocated-root"},
            (sys.executable, "-I", "-c", child)))
        self.assertEqual(result, {"unchanged": True, "short": True,
                                  "private": True, "forged": False})

    def test_unrelated_environment_is_still_scrubbed(self):
        environment = self.contained_environment({"CONTAINMENT_UNRELATED_SENTINEL": "private-fixture"})
        self.assertNotIn("CONTAINMENT_UNRELATED_SENTINEL", environment)

    def test_process_inspection_uses_a_read_only_unprivileged_executable(self):
        child = r'''
import errno, json, os, shutil, stat, subprocess
executable = shutil.which("ps")
assert executable is not None
observed = subprocess.run([executable, "-o", "pid=", "-p", str(os.getpid())],
                          check=True, capture_output=True, text=True)
result = {"pid": int(observed.stdout.strip()), "self": os.getpid(),
          "setid": os.stat(executable).st_mode & (stat.S_ISUID | stat.S_ISGID)}
try:
    descriptor = os.open(executable, os.O_WRONLY)
except OSError as error:
    result["tool_write"] = error.errno
else:
    os.close(descriptor)
    result["tool_write"] = "allowed"
print(json.dumps(result))
'''
        result = json.loads(self.contained_command({}, (sys.executable, "-I", "-c", child)))
        self.assertEqual(result["pid"], result["self"], result)
        self.assertEqual(result["setid"], 0, result)
        self.assertIn(result["tool_write"], (errno.EPERM, errno.EACCES), result)

    def fake_trustc(self, directory, sysroot):
        """A trustc for PATH, outside every home, that names <sysroot> and logs where it ran."""
        tools = Path(directory) / "tools"
        tools.mkdir()
        trustc = tools / "trustc"
        trustc.write_text('#!/bin/sh\npwd >>"$0.cwd"\n'
                          f'[ "$1 $2" = "--print sysroot" ] && echo \'{sysroot}\' && exit 0\nexit 1\n')
        trustc.chmod(0o755)
        return tools

    def test_the_trust_sysroot_paths_trustc_names_is_readable_and_never_writable(self):
        import pwd

        # atpkg's shims exec a sysroot wherever the toolchain is linked, such as a Trust
        # checkout's build under the account home, which no other root admits. This test's
        # only account-home files are its exclusive directory.
        account_home = Path(pwd.getpwuid(os.getuid()).pw_dir)
        with tempfile.TemporaryDirectory(prefix=".codex-sysroot-", dir=account_home) as sysroot, \
                tempfile.TemporaryDirectory(prefix="codex-trustc-") as directory:
            (Path(sysroot) / "lib").mkdir()
            (Path(sysroot) / "lib/canary.txt").write_text("sysroot-canary")
            tools = self.fake_trustc(directory, sysroot)
            child = r'''
import json, sys
from pathlib import Path
lib = Path(sys.argv[1]) / "lib"
result = {"read": (lib / "canary.txt").read_text()}
try:
    (lib / "planted.txt").write_text("x")
    result["write"] = "allowed"
except OSError as error:
    result["write"] = error.errno
print(json.dumps(result))
'''
            output = self.contained_command(
                {"PATH": f"{tools}:/usr/bin:/bin:/usr/sbin:/sbin"},
                (sys.executable, "-I", "-c", child, sysroot))
            result = json.loads(output)
            self.assertEqual(result["read"], "sysroot-canary", result)
            self.assertIn(result["write"], (errno.EPERM, errno.EACCES), result)
            self.assertFalse((Path(sysroot) / "lib/planted.txt").exists())
            # Asked outside any checkout, so no rust-toolchain file there could steer it.
            self.assertEqual(set((tools / "trustc.cwd").read_text().split()), {"/"})

    def test_a_trust_sysroot_at_a_home_or_above_is_refused(self):
        import pwd

        account_home = pwd.getpwuid(os.getuid()).pw_dir
        for sysroot in (account_home, "/"):
            with self.subTest(sysroot=sysroot), \
                    tempfile.TemporaryDirectory(prefix="codex-trustc-") as directory:
                tools = self.fake_trustc(directory, sysroot)
                code, _, stderr = self.contained_command(
                    {"PATH": f"{tools}:/usr/bin:/bin:/usr/sbin:/sbin"}, check=False)
                self.assertNotEqual(code, 0, stderr)
                self.assertIn("refusing the Trust sysroot", stderr)

    def test_private_home_keeps_account_files_and_preferences_outside_the_boundary(self):
        import pwd

        # The only account-home files this test reads or writes are its exclusive directory.
        # It is outside the checkout and every tool/cache grant, even with HOME elsewhere.
        account_home = Path(pwd.getpwuid(os.getuid()).pw_dir)
        with tempfile.TemporaryDirectory(prefix=".codex-boundary-", dir=account_home) as directory, \
                tempfile.TemporaryDirectory(prefix="codex-private-home-") as fake_home:
            owned = Path(directory)
            denied = owned / "denied.txt"
            denied.write_bytes(b"owned-account-canary")
            allowed = owned / "allowed"
            allowed.mkdir()
            (allowed / "canary.txt").write_bytes(b"explicit-read-canary")

            # POSIX shm names on macOS have a 31-byte limit. These are new O_EXCL objects,
            # not CFPreferences objects: no preference or TCC service is ever contacted.
            unique = secrets.token_hex(6)
            names = [f"/apple.cfprefs.{unique}", f"apple.cfprefs.{unique}", f"/alab.{unique}"]
            library = ctypes.CDLL("/usr/lib/libSystem.B.dylib", use_errno=True)
            # mode is variadic: Apple arm64 passes it differently from fixed arguments.
            library.shm_open.argtypes = [ctypes.c_char_p, ctypes.c_int]
            library.shm_open.restype = ctypes.c_int
            library.shm_unlink.argtypes = [ctypes.c_char_p]
            library.shm_unlink.restype = ctypes.c_int
            created = []
            self.contained_child_eof = True
            try:
                for name in names:
                    self.assertLess(len(name.encode()), 31)
                    fd = library.shm_open(name.encode(), os.O_CREAT | os.O_EXCL | os.O_RDWR,
                                          ctypes.c_uint(0o600))
                    if fd < 0:
                        raise OSError(ctypes.get_errno(), f"create owned shm {name}")
                    created.append((name, fd))
                    self.assertEqual(stat.S_IMODE(os.fstat(fd).st_mode), 0o600, name)
                    os.ftruncate(fd, 1)
                    with mmap.mmap(fd, 1, access=mmap.ACCESS_WRITE) as memory:
                        memory[:] = b"x"
                        self.assertEqual(memory[:], b"x")
                    reader = library.shm_open(name.encode(), os.O_RDONLY, ctypes.c_uint(0))
                    if reader < 0:
                        raise OSError(ctypes.get_errno(), f"open owned shm read-only {name}")
                    try:
                        with mmap.mmap(reader, 1, access=mmap.ACCESS_READ) as memory:
                            self.assertEqual(memory[:], b"x", f"parent read-only mapping {name}")
                    finally:
                        os.close(reader)

                child = r'''
import ctypes, json, mmap, os, sys
from pathlib import Path
library = ctypes.CDLL("/usr/lib/libSystem.B.dylib", use_errno=True)
check = library.sandbox_check
check.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int]
check.restype = ctypes.c_int
no_report = ctypes.c_int.in_dll(library, "SANDBOX_CHECK_NO_REPORT").value
def permitted(operation, kind, name=None):
    arguments = [] if name is None else [ctypes.c_char_p(name.encode())]
    result = check(os.getpid(), operation.encode(), kind | no_report, *arguments)
    assert result >= 0, (operation, name, result)
    return result == 0
services = [
    (2, "com.apple.cfprefsd.daemon"),
    (2, "com.apple.cfprefsd.agent"),
    (3, "com.apple.cfprefsd.agent"),
]
result = {"services": [permitted("mach-lookup", kind, name) for kind, name in services],
          "preferences": permitted("user-preference-read", 0),
          "trust_service": permitted("mach-lookup", 2, "com.apple.TrustEvaluationAgent")}
try:
    Path(sys.argv[1]).read_bytes()
    result["account_read"] = "allowed"
except OSError as error:
    result["account_read"] = error.errno
result["explicit_read"] = (Path(sys.argv[2]) / "canary.txt").read_text()
library.shm_open.argtypes = [ctypes.c_char_p, ctypes.c_int]
library.shm_open.restype = ctypes.c_int
result["shm"] = []
for name in sys.argv[3:]:
    fd = library.shm_open(name.encode(), os.O_RDONLY, ctypes.c_uint(0))
    if fd < 0:
        result["shm"].append({"stage": "open", "errno": ctypes.get_errno()})
    else:
        try:
            try:
                memory = mmap.mmap(fd, 1, access=mmap.ACCESS_READ)
            except OSError as error:
                result["shm"].append({"stage": "map", "errno": error.errno})
            else:
                with memory:
                    try:
                        value = memory[:].decode()
                    except OSError as error:
                        result["shm"].append({"stage": "read", "errno": error.errno})
                    else:
                        result["shm"].append({"stage": "read", "value": value})
        finally:
            os.close(fd)
print(json.dumps(result))
'''
                output = self.contained_command(
                    {"HOME": fake_home},
                    (sys.executable, "-I", "-c", child, str(denied), str(allowed), *names),
                    reads=(allowed,),
                )
                result = json.loads(output)
                self.assertEqual(result["services"], [False, False, False], result)
                self.assertFalse(result["preferences"], result)
                self.assertTrue(result["trust_service"], result)
                self.assertIn(result["account_read"], (errno.EPERM, errno.EACCES), result)
                self.assertEqual(result["explicit_read"], "explicit-read-canary", result)
                for target in result["shm"][:2]:
                    self.assertIn(target.get("errno"), (errno.EPERM, errno.EACCES), result)
                self.assertEqual(result["shm"][2], {"stage": "read", "value": "x"}, result)
            finally:
                # Keep each parent's descriptor until the contained child has reached EOF;
                # unlink only names for which this parent successfully created O_EXCL.
                if not self.contained_child_eof:
                    self.fail(f"child EOF unconfirmed; retained owned shm descriptors/names: {names}")
                cleanup_errors = []
                for name, fd in created:
                    os.close(fd)
                    if library.shm_unlink(name.encode()) != 0:
                        cleanup_errors.append((name, ctypes.get_errno()))
                self.assertFalse(cleanup_errors, f"unlink owned shm: {cleanup_errors}")


@unittest.skipUnless(Path("/usr/bin/lockf").is_file() or shutil.which("flock"),
                     "requires a kernel-lock wrapper")
class PackageCacheLockTests(unittest.TestCase):
    def test_packagers_serialize_each_physical_cache_and_release_it(self):
        directory = Path(tempfile.mkdtemp(prefix="codex-package-cache-lock-"))
        owner = None
        children, ended = [], set()
        command = 'set -euo pipefail; . "$1/alab/package.sh"; shift; alab_project_selected() { :; }; package_selected_entry "$@"'
        environment = {key: value for key, value in os.environ.items()
                       if not key.startswith("ALAB_")}
        environment["ALAB_CARGO_HOME"] = str(directory / "cargo-home")
        environment["ALAB_PYTHON"] = sys.executable

        def start(cache, code, *arguments):
            process = subprocess.Popen(
                ["/bin/bash", "-c", command, "package-cache-lock", str(fixture),
                 sys.executable, "-I", "-B", "-c", code, *map(str, arguments)],
                cwd=fixture, env=dict(environment, ALAB_PACKAGE_CACHE=str(cache)),
                stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                stderr=subprocess.PIPE, text=True, start_new_session=True,
            )
            children.append(process)
            return process

        def finish(process, text=None):
            try:
                output = process.communicate(text, timeout=60)
                ended.add(process)
                return output
            except subprocess.TimeoutExpired:
                # Only this test's new process group, including lockf/flock's child.
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.communicate(timeout=60)
                ended.add(process)
                self.fail("package cache fixture exceeded the one-minute hang detector")

        try:
            fixture = directory / "fixture"
            (fixture / "alab").mkdir(parents=True)
            # The build-volume guard also accounts for the driving Git metadata.
            subprocess.run(["git", "init", "--quiet", "--template=", str(fixture)],
                           env=dict(environment, GIT_CONFIG_GLOBAL="/dev/null",
                                    GIT_CONFIG_NOSYSTEM="1"),
                           capture_output=True, text=True, check=True, timeout=60)
            # Keep the production entrypoint and its locked-child re-source intact.
            # Both sources see this final override, so only our inert callback can run.
            with (fixture / "alab/package.sh").open("xb") as script:
                script.write((ROOT / "alab/package.sh").read_bytes())
                script.write(b'\npackage_main() { "$@"; }\n')
            with (fixture / "alab/lib.sh").open("xb") as library:
                library.write((ROOT / "alab/lib.sh").read_bytes())
            with (fixture / "alab/command_guard.py").open("xb") as guardian:
                guardian.write((ROOT / "alab/command_guard.py").read_bytes())
            cache = directory / "cache"
            cache.mkdir()
            alias = directory / "alias"
            alias.symlink_to(cache, target_is_directory=True)
            value = cache / "value"
            owner = start(cache, '''
import os, sys
from pathlib import Path
cache = Path(sys.argv[1]).resolve()
assert os.environ["ALAB_PACKAGE_CACHE"] == str(cache)
(cache / "value").write_text("held")
print("READY", flush=True)
assert sys.stdin.readline() == "release\\n"
(cache / "value").write_text("released")
''', cache)
            ready, _, _ = select.select([owner.stdout], [], [], 60)
            self.assertTrue(ready, "cache holder never reported its acquisition")
            self.assertEqual(owner.stdout.readline(), "READY\n")
            self.assertEqual(value.read_text(), "held")

            write_value = ('import os, sys; from pathlib import Path; '
                           'Path(sys.argv[1]).write_text(os.environ["ALAB_PACKAGE_CACHE"])')
            contender = start(alias, write_value, value)
            _, stderr = finish(contender)
            self.assertEqual(contender.returncode, 75, stderr)
            self.assertEqual(value.read_text(), "held", "a contender cannot overwrite a live cache")

            independent = directory / "independent"
            independent.mkdir()
            independent_value = directory / "independent-value"
            separate = start(independent, write_value, independent_value)
            _, stderr = finish(separate)
            self.assertEqual(separate.returncode, 0, stderr)
            self.assertEqual(independent_value.read_text(), str(independent.resolve()))

            _, stderr = finish(owner, "release\n")
            self.assertEqual(owner.returncode, 0, stderr)
            self.assertEqual(value.read_text(), "released")
            successor = start(alias, write_value, value)
            _, stderr = finish(successor)
            self.assertEqual(successor.returncode, 0, stderr)
            self.assertEqual(value.read_text(), str(cache.resolve()),
                             "EOF releases the kernel lock; aliases retain one physical key")
        finally:
            cleanup_errors = []
            for process in children:
                if process not in ended:
                    try:
                        finish(process, "release\n" if process is owner else None)
                    except (AssertionError, subprocess.SubprocessError) as error:
                        cleanup_errors.append(str(error))
            if len(ended) == len(children):
                shutil.rmtree(directory)
            else:
                self.fail(f"cache child EOF unconfirmed; retained owned fixture: {directory}")
            self.assertFalse(cleanup_errors, cleanup_errors)


if __name__ == "__main__":
    unittest.main()

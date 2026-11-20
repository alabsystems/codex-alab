#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise live-build dispatch, refusal propagation and observational byte binding."""

import hashlib
import json
import os
from pathlib import Path
import runpy
import shlex
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest


ROOT = Path(__file__).resolve().parents[2]
RECORD = runpy.run_path(str(ROOT / "alab/proof_build.py"))["record"]
PYTHON_ENTRY = "#!/bin/sh\n'''exec' " + shlex.quote(sys.executable) + ' -I "$0" "$@"\n\' \'\'\'\n'


class CertifiedDispatchTests(unittest.TestCase):
    package_inputs = ROOT

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="alab-certified-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.repo = self.root / "source with spaces"
        (self.repo / "alab").mkdir(parents=True)
        (self.repo / "codex-rs").mkdir()
        (self.repo / "codex-rs/Cargo.toml").write_text("[workspace]\nmembers = []\n")
        for name in ("certified-build.sh", "lib.sh", "project-targo.sh", "proof-policy.toml",
                     "pristine_state.py", "qualification.py", "upstream_intake.py",
                     "source_content.py", "cli_status.py", "file_hash.py"):
            shutil.copy2(ROOT / "alab" / name, self.repo / "alab" / name)
        self.env = dict(HOME=str(self.root), TMPDIR=str(self.root), PATH="/usr/bin:/bin:/usr/sbin:/sbin",
                        GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1",
                        GIT_AUTHOR_NAME="Fixture", GIT_AUTHOR_EMAIL="fixture@example.invalid",
                        GIT_COMMITTER_NAME="Fixture", GIT_COMMITTER_EMAIL="fixture@example.invalid")
        self.git("init", "-q", "--template=", "-b", "main")
        self.git("add", ".")
        self.git("commit", "-qm", "fixture")
        self.source = self.git("rev-parse", "HEAD")
        self.calls = self.root / "calls.json"
        self.front = self.root / "native fixture"
        self.front.write_text(PYTHON_ENTRY + '''import json,os,sys
from pathlib import Path
args = sys.argv[1:]
Path(os.environ["FIXTURE_CALLS"]).write_text(json.dumps(args))
assert args[:4] == ["__build_admit", "/usr/bin/true", "trust", "build"]
status = int(os.environ.get("FIXTURE_STATUS", "0"))
if not status and os.environ.get("FIXTURE_REPORT", "yes") == "yes":
    root = Path(args[args.index("--report-dir") + 1])
    root.mkdir()
    (root / "report.json").write_text('{"fixture":"observation, not proof"}')
raise SystemExit(status)
''')
        self.front.chmod(0o755)
        self.report = self.root / "report with spaces"
        self.env.update(ATPKG_PROJECT_FRONT=str(self.front), ATPKG_PROJECT_TARGO="/usr/bin/true",
                        ATPKG_PROJECT_TRUSTC="/usr/bin/true", ATPKG_PROJECT_TRUSTDOC="/usr/bin/true",
                        ATPKG_PROJECT_PAYLOAD=str(self.root), ATPKG_PROJECT_EXECUTION=str(self.root),
                        ALAB_PYTHON=sys.executable, FIXTURE_CALLS=str(self.calls))

    def git(self, *args):
        return subprocess.run(["/usr/bin/git", "-C", str(self.repo), *args], env=self.env,
                              capture_output=True, text=True, check=True, timeout=20).stdout.strip()

    def build(self, *extra, source=None, arguments=None, cwd=None, **env):
        arguments = arguments if arguments is not None else [
            "--release", "--locked", "-p", "codex-cli", "--bin", "codex"]
        return subprocess.run([str(self.repo / "alab/certified-build.sh"), str(self.report),
                               source or self.source, *arguments, *extra], cwd=cwd or self.repo / "codex-rs", env=dict(self.env, **env),
                              capture_output=True, text=True, timeout=20)

    def worktree(self, path):
        subprocess.run(["/usr/bin/git", "-C", str(self.repo), "worktree", "add", "--quiet",
                        "--detach", str(path), self.source], env=self.env, check=True,
                       capture_output=True, text=True, timeout=20)
        self.addCleanup(lambda: subprocess.run(
            ["/usr/bin/git", "-C", str(self.repo), "worktree", "remove", "--force", str(path)],
            env=self.env, capture_output=True, text=True, timeout=20))

    @unittest.skipUnless(Path("/usr/bin/sandbox-exec").is_file(), "requires macOS containment")
    def test_package_controller_inputs_are_readable_but_readonly_outside_target(self):
        # The target may carry an older policy. Packaging must keep the current
        # controller's policy without admitting the whole controller checkout.
        (self.repo / "alab/proof-policy.toml").write_text("# older target policy\n")
        self.git("add", ".")
        self.git("commit", "-qm", "older target policy")
        source = self.git("rev-parse", "HEAD")
        output = self.root / "output"
        output.mkdir()
        compiler = self.root / "compiler"
        compiler.mkdir()
        calls = output / "calls.json"
        controller = self.package_inputs / "alab"
        self.front.write_text(PYTHON_ENTRY + f'''import errno,json,os,sys,tomllib
from pathlib import Path
controller = Path({str(controller)!r})
try:
    (controller / "package.sh").read_bytes()
except PermissionError:
    pass
else:
    raise AssertionError("unneeded controller file became readable")
for name in ("certified-build.sh", "lib.sh", "proof-policy.toml", "project-targo.sh"):
    try:
        fd = os.open(controller / name, os.O_WRONLY)
    except OSError as error:
        assert error.errno in (errno.EPERM, errno.EACCES), error
    else:
        os.close(fd)
        raise AssertionError("controller input became writable: " + name)
args = sys.argv[1:]
assert args[:4] == ["__build_admit", "/usr/bin/true", "trust", "build"], args
policy = Path(args[args.index("--config") + 1])
assert policy == controller / "proof-policy.toml", policy
flags = tomllib.loads(policy.read_text())["target"]["cfg(all())"]["rustflags"]
assert "-Ztrust-policy=certify" in flags
assert "-Ztrust-verify-include-dependencies=yes" in flags
assert "--certify" in args and "--unverified" not in args
Path({str(calls)!r}).write_text(json.dumps(args))
report = Path(args[args.index("--report-dir") + 1])
report.mkdir()
(report / "report.json").write_text('{{"fixture":"not proof"}}')
''')
        body = r'''set -euo pipefail
. "$5/alab/package.sh"
repo_root="$1"
compiler_root="$2"
cargo="$1/alab/project-targo.sh"
# Resource brokerage and Cargo cache seeding are outside this fixture; the
# production containment profile and certified dispatcher are exercised intact.
alab_verified_contained() { alab_contained "$@"; }
alab_cargo_home_ready() { mkdir -p "$1"; }
package_verified_contained "$3" -- "$1/alab/certified-build.sh" "$3/report" \
  "$4" --release --locked -p codex-cli --bin codex
'''
        def run(script):
            return subprocess.run(
                ["/bin/bash", "-c", script, "package-proof-fixture", str(self.package_inputs),
                 str(compiler), str(output), source, str(ROOT)], cwd=self.repo / "codex-rs",
                env=dict(self.env, ALAB_PYTHON=sys.executable,
                         ALAB_CARGO_HOME=str(self.root / "cargo-cache"), ALAB_CONTAIN="1"),
                capture_output=True, text=True, timeout=30,
            )

        prior_admission = r'''package_verified_contained() {
  alab_verified_contained --read "$ATPKG_PROJECT_FRONT" --read "$compiler_root" --read "$cargo" "$@"
}
'''
        missing = run(body.replace('package_verified_contained "$3" --',
                                   prior_admission + 'package_verified_contained "$3" --'))
        self.assertNotEqual(missing.returncode, 0, missing.stdout + missing.stderr)
        self.assertIn("certified-build.sh", missing.stderr)
        self.assertFalse(calls.exists(), "missing admission reached the dispatcher")
        self.assertFalse((output / "report").exists())
        done = run(body)
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        self.assertTrue(calls.is_file(), "the live dispatcher never reached its stand-in")
        self.assertTrue((output / "report/report.json").is_file())

    def test_live_compile_uses_certify_and_transitive_application_scope(self):
        done = self.build()
        self.assertEqual(done.returncode, 0, done.stderr)
        args = json.loads(self.calls.read_text())
        self.assertIn("--certify", args)
        self.assertNotIn("--unverified", args)
        policy = Path(args[args.index("--config") + 1])
        self.assertEqual(policy, self.repo / "alab/proof-policy.toml")
        flags = tomllib.loads(policy.read_text())["target"]["cfg(all())"]["rustflags"]
        self.assertIn("-Ztrust-verify=on", flags)
        self.assertIn("-Ztrust-policy=certify", flags)
        self.assertIn("-Ztrust-verify-include-dependencies=yes", flags)
        self.assertEqual(args[-6:], ["--release", "--locked", "-p", "codex-cli", "--bin", "codex"])

    def test_pristine_mode_derives_vendor_config_and_keeps_the_fixed_policy(self):
        base = Path(self.git("rev-parse", "--path-format=absolute", "--git-path", "alab-pristine"))
        pristine = Path(str(base) + "-" + self.source)
        self.worktree(pristine)
        vendor = self.repo / "alab/vendor/fixture"
        vendor.mkdir(parents=True)
        (vendor / "ALAB-SOURCE.json").write_text("{}\n")
        product = self.repo / "alab/vendor/product-behavior"
        product.mkdir()
        (product / "ALAB-SOURCE.json").write_text(json.dumps({"pristine_override": False}))
        done = self.build("--pristine-upstream", cwd=pristine / "codex-rs")
        self.assertEqual(done.returncode, 0, done.stderr)
        args = json.loads(self.calls.read_text())
        self.assertIn("--certify", args)
        self.assertNotIn("--unverified", args)
        configs = [args[index + 1] for index, word in enumerate(args) if word == "--config"]
        self.assertEqual(configs, [str(self.repo / "alab/proof-policy.toml"),
                                   f'paths = ["{vendor}"]'])

    def test_pristine_mode_refuses_invalid_vendor_metadata_before_compiler(self):
        base = Path(self.git("rev-parse", "--path-format=absolute", "--git-path", "alab-pristine"))
        pristine = Path(str(base) + "-" + self.source)
        self.worktree(pristine)
        vendor = self.repo / "alab/vendor/fixture"
        vendor.mkdir(parents=True)
        for payload in ("", "{}{}", '{"pristine_override": "false"}', "[]"):
            with self.subTest(payload=payload):
                (vendor / "ALAB-SOURCE.json").write_text(payload)
                done = self.build("--pristine-upstream", cwd=pristine / "codex-rs")
                self.assertNotEqual(done.returncode, 0, done.stderr)
                self.assertIn("cannot derive its vendor configuration", done.stderr)
                self.assertFalse(self.calls.exists())

    def test_pristine_mode_refuses_dirty_or_unrelated_worktrees_before_compiler(self):
        base = Path(self.git("rev-parse", "--path-format=absolute", "--git-path", "alab-pristine"))
        pristine = Path(str(base) + "-" + self.source)
        self.worktree(pristine)
        dirty = pristine / "unexpected-source"
        dirty.write_text("not committed\n")
        done = self.build("--pristine-upstream", cwd=pristine / "codex-rs")
        self.assertNotEqual(done.returncode, 0)
        self.assertIn("clean exact upstream worktree", done.stderr)
        self.assertFalse(self.calls.exists())
        dirty.unlink()

        unrelated = self.root / "another worktree"
        self.worktree(unrelated)
        done = self.build("--pristine-upstream", cwd=unrelated / "codex-rs")
        self.assertNotEqual(done.returncode, 0)
        self.assertIn("prepared exact upstream worktree", done.stderr)
        self.assertFalse(self.calls.exists())

    def test_unproved_or_unavailable_compilation_never_becomes_success(self):
        for status in (1, 2, 3, 75):
            with self.subTest(status=status):
                done = self.build(FIXTURE_STATUS=str(status))
                self.assertEqual(done.returncode, status, done.stderr)
                self.assertFalse(self.report.exists())
                self.assertNotIn("passed", done.stderr)

    def test_missing_report_is_an_error_even_after_successful_child_exit(self):
        done = self.build(FIXTURE_REPORT="no")
        self.assertEqual(done.returncode, 2)
        self.assertIn("no retained canonical report", done.stderr)

    def test_stale_reports_and_wrong_source_start_no_compiler(self):
        self.report.mkdir()
        (self.report / "report.json").write_text('{"claimed_proved":true}')
        done = self.build()
        self.assertNotEqual(done.returncode, 0)
        self.assertFalse(self.calls.exists())
        self.report.rename(self.root / "old report")
        done = self.build(source="f" * 40)
        self.assertNotEqual(done.returncode, 0)
        self.assertFalse(self.calls.exists())

    def test_absent_native_selection_starts_no_compiler(self):
        done = self.build(ATPKG_PROJECT_FRONT="")
        self.assertNotEqual(done.returncode, 0)
        self.assertFalse(self.calls.exists())

    def test_precompiler_resource_and_signal_stops_propagate_without_a_verdict(self):
        library = self.repo / "alab/lib.sh"
        library.write_text(library.read_text() +
                           '\nalab_project_selected() { return "$FIXTURE_PREFLIGHT_STATUS"; }\n')
        for status in (75, 130):
            with self.subTest(status=status):
                done = self.build(FIXTURE_PREFLIGHT_STATUS=str(status))
                self.assertEqual(done.returncode, status, done.stderr)
                self.assertFalse(self.calls.exists())

    def test_extra_config_target_manifest_and_lane_options_start_no_compiler(self):
        for arguments in (("--config", "target.'cfg(all())'.rustflags=[]"),
                          ("--config=env.CARGO_ENCODED_RUSTFLAGS.value=''",),
                          ("--target", "aarch64-apple-darwin"),
                          ("--manifest-path", "another/Cargo.toml"),
                          ("--report-dir", str(self.root / "other report")),
                          ("--allow-l0-gaps",), ("--memory-safe",), ("--", "--help")):
            with self.subTest(arguments=arguments):
                done = self.build(*arguments)
                self.assertNotEqual(done.returncode, 0, done.stderr)
                self.assertFalse(self.calls.exists())

    def test_empty_or_populated_flag_environments_cannot_shadow_dependency_policy(self):
        for key in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS",
                    "TARGO_BUILD_RUSTFLAGS", "CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS",
                    "RUSTDOCFLAGS", "CARGO_ENCODED_RUSTDOCFLAGS", "CARGO_BUILD_TARGET"):
            for value in ("", "-Ztrust-verify-include-dependencies=no"):
                with self.subTest(key=key, value=value):
                    done = self.build(**{key: value})
                    self.assertNotEqual(done.returncode, 0, done.stderr)
                    self.assertIn(key, done.stderr)
                    self.assertFalse(self.calls.exists())

    def test_unlocked_or_unselected_builds_start_no_compiler(self):
        for arguments in (("-p", "codex-cli", "--bin", "codex"),
                          ("--locked", "-p", "codex-cli"),
                          ("--locked", "--bin", "codex")):
            with self.subTest(arguments=arguments):
                done = self.build(arguments=arguments)
                self.assertNotEqual(done.returncode, 0, done.stderr)
                self.assertFalse(self.calls.exists())

    def test_another_directory_under_the_same_git_head_starts_no_compiler(self):
        ignored = self.repo / "target" / "another crate"
        ignored.mkdir(parents=True)
        for cwd in (self.repo, ignored):
            with self.subTest(cwd=cwd):
                done = self.build(cwd=cwd)
                self.assertNotEqual(done.returncode, 0, done.stderr)
                self.assertIn("codex-rs workspace", done.stderr)
                self.assertFalse(self.calls.exists())


class ProofObservationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="alab-proof-observation-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.compiler = self.root / "compiler.json"
        self.original = {"admitted": True, "binaries_sha256": "a" * 64, "id": "1" * 16}
        self.compiler.write_text(json.dumps(self.original))
        self.report = self.root / "report.json"
        self.report.write_text('{"observation":"serialized claims are never proof authority"}')
        self.target = self.root / "target"
        self.package = self.root / "package"
        (self.target / "release").mkdir(parents=True)
        (self.package / "bin").mkdir(parents=True)
        (self.target / "release/codex").write_bytes(b"compiled code")
        (self.package / "bin/codex").write_bytes(b"compiled code")

    def record(self, binaries=("codex",)):
        RECORD(self.compiler, self.report, "b" * 40, self.target, self.package, binaries)

    def test_observation_binds_source_report_and_unchanged_binary_copy(self):
        self.record()
        result = json.loads(self.compiler.read_text())["verification"]
        self.assertEqual(result["source"], "b" * 40)
        self.assertEqual(result["lane"], "certify")
        self.assertIs(result["include_dependencies"], True)
        self.assertEqual(result["report_sha256"], hashlib.sha256(self.report.read_bytes()).hexdigest())
        self.assertEqual(result["binaries"]["codex"], {
            "built_sha256": hashlib.sha256(b"compiled code").hexdigest(),
            "packaged_sha256": hashlib.sha256(b"compiled code").hexdigest(),
        })
        self.assertIn("no verifier authority", result["authority"])
        self.assertNotIn(str(self.root), json.dumps(result))

    def test_missing_packaged_binary_cannot_create_verification_observation(self):
        before = self.compiler.read_bytes()
        (self.package / "bin/codex").unlink()
        with self.assertRaises(OSError):
            self.record()
        self.assertEqual(self.compiler.read_bytes(), before)

    def test_each_first_party_package_input_is_bound_at_its_actual_location(self):
        binaries = ("codex", "codex-code-mode-host", "codex-exec-server")
        for binary in binaries[1:]:
            (self.target / "release" / binary).write_bytes((binary + " compiled").encode())
            (self.package / "bin" / binary).write_bytes((binary + " compiled").encode())
        self.record(binaries)
        observed = json.loads(self.compiler.read_text())["verification"]["binaries"]
        self.assertEqual(set(observed), set(binaries))
        for binary in binaries:
            self.assertEqual(observed[binary]["built_sha256"], hashlib.sha256(
                (self.target / "release" / binary).read_bytes()).hexdigest())
            self.assertEqual(observed[binary]["packaged_sha256"], hashlib.sha256(
                (self.package / "bin" / binary).read_bytes()).hexdigest())

    def test_transformed_or_replaced_shipping_bytes_leave_the_record_unchanged(self):
        binaries = ("codex", "codex-code-mode-host", "codex-exec-server")
        for binary in binaries:
            payload = (binary + " compiled").encode()
            (self.target / "release" / binary).write_bytes(payload)
            (self.package / "bin" / binary).write_bytes(payload)
        before = self.compiler.read_bytes()
        for binary in binaries:
            packaged = self.package / "bin" / binary
            payload = packaged.read_bytes()
            for changed in (b"stripped " + payload, b"unrelated executable"):
                with self.subTest(binary=binary, changed=changed):
                    packaged.write_bytes(changed)
                    with self.assertRaisesRegex(ValueError, "packaged.*differs"):
                        self.record(binaries)
                    self.assertEqual(self.compiler.read_bytes(), before)
            packaged.write_bytes(payload)

    def test_redirected_report_is_refused_without_updating_compiler_record(self):
        before = self.compiler.read_bytes()
        self.report.unlink()
        self.report.symlink_to(self.compiler)
        with self.assertRaises(OSError):
            self.record()
        self.assertEqual(self.compiler.read_bytes(), before)

    def test_unadmitted_compiler_and_invalid_binary_sets_cannot_be_recorded(self):
        for binaries in ((), ("codex", "codex"), ("../codex",)):
            with self.subTest(binaries=binaries), self.assertRaises(ValueError):
                self.record(binaries)
        self.compiler.write_text(json.dumps({**self.original, "admitted": False}))
        with self.assertRaises(ValueError):
            self.record()


if __name__ == "__main__":
    unittest.main()

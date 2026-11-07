#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise the real sync pre-creation boundary with inert registrar/compiler stand-ins."""

import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ENTRY = "#!/bin/sh\n'''exec' " + shlex.quote(sys.executable) + ' -I "$0" "$@"\n\' \'\'\'\n'


class BuildCacheRegistrationTests(unittest.TestCase):
    def setUp(self):
        self.fixture = Path(tempfile.mkdtemp(prefix="alab-cache-registration-", suffix=".noindex")).resolve()
        print(f"preserved cache-registration fixture: {self.fixture}", flush=True)
        self.home = self.fixture / "home"
        self.workspace = self.home / "source/codex-rs"
        self.workspace.mkdir(parents=True)
        (self.workspace / "Cargo.toml").write_text("[workspace]\nmembers = []\n")
        self.target = self.home / "outputs/chosen.noindex"
        self.events = self.fixture / "events.jsonl"
        self.helper = self.fixture / "fake-aterm"
        self.helper.write_text(
            PYTHON_ENTRY + "import json, os, sys\nfrom pathlib import Path\n"
            "args = sys.argv[1:]\n"
            "assert os.environ['HOME'] == os.environ['EXPECT_CANONICAL_HOME']\n"
            "assert args[:2] == ['pkg', '__build_cache_register'], args\n"
            "mode = os.environ.get('FIXTURE_MODE', 'supported')\n"
            "if args[2:] == ['--capabilities']:\n"
            "    if mode == 'old': sys.exit(2)\n"
            "    print('aterm-build-cache-register-v1')\n"
            "    sys.exit(0)\n"
            "target = Path(next(a.split('=', 1)[1] for a in args if a.startswith('--target-dir=')))\n"
            "manifest = Path(next(a.split('=', 1)[1] for a in args if a.startswith('--manifest-path=')))\n"
            "with Path(os.environ['FIXTURE_EVENTS']).open('a') as out:\n"
            "    out.write(json.dumps({'kind': 'register', 'target': str(target), 'manifest': str(manifest),\n"
            "                          'existed': target.exists(), 'strict': '--require-owned' in args, 'cwd': os.getcwd()}) + '\\n')\n"
            "if mode == 'refuse': sys.exit(75)\n"
            "if (str(target) == os.environ.get('FIXTURE_REFUSE_TARGET') and\n"
            "        str(manifest) == os.environ.get('FIXTURE_REFUSE_MANIFEST')): sys.exit(75)\n"
            "if mode == 'unmanaged': sys.exit(75 if '--require-owned' in args else 0)\n"
            "if not target.exists():\n"
            "    target.mkdir()\n"
            "    (target / '.aterm-build-cache.json').write_text('fixture-owned')\n"
            "if mode == 'interrupted': sys.exit(130)\n"
        )
        self.helper.chmod(0o700)
        self.environment = {
            "HOME": str(self.home), "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
            "ALAB_ATERM": str(self.helper), "FIXTURE_EVENTS": str(self.events),
            "EXPECT_CANONICAL_HOME": str(self.home),
            "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_OPTIONAL_LOCKS": "0",
        }

    def shell(self, body, *arguments):
        return subprocess.run(
            ["/bin/bash", "-c", 'set -euo pipefail; . "$1/alab/lib.sh"; shift; ' + body,
             "cache-registration-fixture", str(ROOT), *map(str, arguments)],
            cwd=self.fixture, env=self.environment, capture_output=True, text=True, timeout=60)

    def recorded(self):
        return [json.loads(line) for line in self.events.read_text().splitlines()] if self.events.exists() else []

    def test_fresh_target_is_registered_before_creation_with_its_real_manifest(self):
        done = self.shell('alab_build_target_ready "$1" "$2"', self.workspace, self.target)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(done.stdout.strip(), str(self.target))
        self.assertEqual(self.recorded(), [{"kind": "register", "target": str(self.target),
                                           "manifest": str(self.workspace / "Cargo.toml"),
                                           "existed": False, "strict": False, "cwd": str(self.workspace)}])
        self.assertTrue((self.target / ".aterm-build-cache.json").is_file())

    def test_existing_unmarked_outputs_keep_the_same_location_and_are_not_adopted(self):
        self.target.mkdir(parents=True)
        output = self.target / "keep"
        output.write_bytes(b"reusable output")
        done = self.shell('alab_build_target_ready "$1" "$2"', self.workspace, self.target)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertTrue(self.recorded()[0]["existed"])
        self.assertEqual(output.read_bytes(), b"reusable output")
        self.assertFalse((self.target / ".aterm-build-cache.json").exists())
        self.assertFalse((self.target / "build.noindex").exists())

    def test_old_host_keeps_fresh_unmarked_flow_but_refuses_a_marked_cache(self):
        self.environment["FIXTURE_MODE"] = "old"
        done = self.shell('alab_build_target_ready "$1" "$2"', self.workspace, self.target)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertTrue(self.target.is_dir())
        self.assertEqual(self.recorded(), [])
        (self.target / ".aterm-build-cache.json").write_text("fixture-owned")
        compiler = self.fixture / "compiler-ran"
        done = self.shell('alab_build_target_ready "$1" "$2"; touch "$3"',
                          self.workspace, self.target, compiler)
        self.assertEqual(done.returncode, 75, done.stderr)
        self.assertFalse(compiler.exists())

    def test_unavailable_or_refused_registrar_starts_no_compiler_on_a_marked_target(self):
        self.target.mkdir(parents=True)
        marker = self.target / "build.noindex/.aterm-build-cache.json"
        marker.parent.mkdir()
        marker.write_text("fixture-owned")
        compiler = self.fixture / "compiler-ran"
        self.environment["ALAB_ATERM"] = str(self.fixture / "missing-aterm")
        done = self.shell('alab_build_target_ready "$1" "$2"; touch "$3"',
                          self.workspace, self.target, compiler)
        self.assertEqual(done.returncode, 75, done.stderr)
        self.assertFalse(compiler.exists())
        self.environment.update(ALAB_ATERM=str(self.helper), FIXTURE_MODE="refuse")
        done = self.shell('alab_build_target_ready "$1" "$2"; touch "$3"',
                          self.workspace, self.target, compiler)
        self.assertEqual(done.returncode, 75, done.stderr)
        self.assertFalse(compiler.exists())
        self.assertTrue(marker.is_file())
        self.assertTrue(self.recorded()[-1]["strict"])
        self.environment["FIXTURE_MODE"] = "unmanaged"
        done = self.shell('alab_build_target_ready "$1" "$2"; touch "$3"',
                          self.workspace, self.target, compiler)
        self.assertEqual(done.returncode, 75, done.stderr)
        self.assertFalse(compiler.exists())
        self.assertTrue(self.recorded()[-1]["strict"])
        fresh = self.home / "interrupted.noindex"
        self.environment["FIXTURE_MODE"] = "interrupted"
        done = self.shell('alab_build_target_ready "$1" "$2"; touch "$3"',
                          self.workspace, fresh, compiler)
        self.assertEqual(done.returncode, 75, done.stderr)
        self.assertTrue((fresh / ".aterm-build-cache.json").is_file())
        self.assertFalse(compiler.exists())

    def test_relative_target_uses_cargos_workspace_cwd_and_shared_owners_are_observed(self):
        other = self.home / "other-source/codex-rs"
        other.mkdir(parents=True)
        (other / "Cargo.toml").write_text("[workspace]\nmembers = []\n")
        done = self.shell('alab_build_target_ready "$1" ../shared.noindex; '
                          'alab_build_target_ready "$2" "$3"',
                          self.workspace, other, self.workspace.parent / "shared.noindex")
        self.assertEqual(done.returncode, 0, done.stderr)
        events = self.recorded()
        self.assertEqual([event["manifest"] for event in events],
                         [str(self.workspace / "Cargo.toml"), str(other / "Cargo.toml")])
        self.assertEqual(events[0]["target"], events[1]["target"])

    def test_supported_noop_cannot_admit_a_marked_target_outside_scratch_home(self):
        external = self.fixture / "outside-scratch-home.noindex"
        external.mkdir()
        (external / ".aterm-build-cache.json").write_text("fixture-owned")
        compiler = self.fixture / "compiler-ran"
        done = self.shell('alab_build_target_ready "$1" "$2"; touch "$3"',
                          self.workspace, external, compiler)
        self.assertEqual(done.returncode, 75, done.stderr)
        self.assertFalse(compiler.exists())
        self.assertEqual(self.recorded(), [])
        alias = self.fixture / "home-alias"
        alias.symlink_to(self.home, target_is_directory=True)
        self.environment["HOME"] = str(alias)
        done = self.shell('alab_build_target_ready "$1" "$2"', self.workspace, self.target)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertTrue((self.target / ".aterm-build-cache.json").is_file())

    def sync_function(self, name):
        text = (ROOT / "alab/sync.sh").read_text()
        return name + "() {" + text.split(name + "() {", 1)[1].split("\n}\n", 1)[0] + "\n}\n"

    def test_gate_inventory_registers_both_workspaces_before_followup(self):
        pristine = self.home / "pristine/codex-rs"
        pristine.mkdir(parents=True)
        (pristine / "Cargo.toml").write_text("[workspace]\nmembers = []\n")
        body = 'wt="$1"; '
        body += 'chosen=$(alab_check_targets_ready "$wt" "$PRISTINE" "${CARGO_TARGET_DIR:-$wt/codex-rs/target}"); '
        body += 'printf \'{"kind":"ready","target":"%s"}\\n\' "$chosen" >> "$FIXTURE_EVENTS"'
        self.environment.update(PRISTINE=str(pristine.parent), CARGO_TARGET_DIR=str(self.target))
        done = self.shell(body, self.workspace.parent)
        self.assertEqual(done.returncode, 0, done.stderr)
        events = self.recorded()
        self.assertEqual([event["kind"] for event in events],
                         ["register"] * 5 + ["ready"])
        self.assertEqual([event["manifest"] for event in events[:5]],
                         [str(self.workspace / "Cargo.toml"), str(pristine / "Cargo.toml"),
                          str(self.workspace / "Cargo.toml"), str(self.workspace / "Cargo.toml"),
                          str(pristine / "Cargo.toml")])
        self.assertEqual(events[1]["target"], events[2]["target"],
                         "the ALab unit runner also produces into the pristine target")
        self.assertTrue(events[2]["existed"] and events[2]["strict"],
                        "a marked pristine target requires the runner workspace declaration")
        self.assertEqual([event["target"] for event in events[3:5]],
                         [str(self.target / "tippy"), str(pristine / "target/tippy")],
                         "both dedicated lint targets must be declared before containment")
        self.assertEqual(events[-1]["target"], str(self.target))
        self.environment.pop("CARGO_TARGET_DIR")
        done = self.shell(body, self.workspace.parent)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(self.recorded()[-1]["target"], str(self.workspace / "target"))

        self.events.write_text("")
        self.environment.update(FIXTURE_REFUSE_TARGET=str(pristine / "target"),
                                FIXTURE_REFUSE_MANIFEST=str(self.workspace / "Cargo.toml"))
        refused = self.shell(body, self.workspace.parent)
        self.assertEqual(refused.returncode, 75, refused.stderr)
        events = self.recorded()
        self.assertEqual([event["kind"] for event in events], ["register"] * 3,
                         "refused runner ownership must not enter containment")
        self.assertEqual((events[-1]["target"], events[-1]["manifest"]),
                         (str(pristine / "target"), str(self.workspace / "Cargo.toml")))
        self.assertTrue(events[-1]["strict"])

        self.environment.pop("FIXTURE_REFUSE_MANIFEST")
        self.environment["CARGO_TARGET_DIR"] = str(self.target)
        for lint_target in (self.target / "tippy", pristine / "target/tippy"):
            self.events.write_text("")
            self.environment["FIXTURE_REFUSE_TARGET"] = str(lint_target)
            self.environment["FIXTURE_REFUSE_MANIFEST"] = str(
                (self.workspace if lint_target == self.target / "tippy" else pristine) / "Cargo.toml")
            refused = self.shell(body, self.workspace.parent)
            self.assertEqual(refused.returncode, 75, refused.stderr)
            self.assertNotIn("ready", [event["kind"] for event in self.recorded()],
                             "a refused dedicated lint target starts no gate/compiler")

    def test_real_sync_delegates_to_gate_and_preserves_target_environment(self):
        gate = self.workspace.parent / "alab/check.sh"
        gate.parent.mkdir()
        gate.write_text(
            PYTHON_ENTRY + "import json, os, sys\nfrom pathlib import Path\n"
            "with Path(os.environ['FIXTURE_EVENTS']).open('a') as out:\n"
            "    out.write(json.dumps({'kind': 'gate', 'cwd': os.getcwd(), "
            "'target': os.environ.get('CARGO_TARGET_DIR')}) + '\\n')\n"
            "sys.exit(int(os.environ.get('FIXTURE_GATE_STATUS', '0')))\n")
        gate.chmod(0o700)
        body = self.sync_function("run_check")
        body += '\nwt="$1"; check=1; mode=continue; '
        body += 'in_wt() { printf "fixture-source\\n"; }; '
        body += 'exit_with() { exit "$1"; }; check_failed() { exit "$1"; }; run_check'
        for target in (str(self.target), None):
            for status in (0, 1, 75):
                with self.subTest(target=target, status=status):
                    self.events.write_text("")
                    self.environment.pop("CARGO_TARGET_DIR", None)
                    if target is not None:
                        self.environment["CARGO_TARGET_DIR"] = target
                    self.environment["FIXTURE_GATE_STATUS"] = str(status)
                    done = self.shell(body, self.workspace.parent)
                    self.assertEqual(done.returncode, status, done.stderr)
                    self.assertEqual(self.recorded(), [{
                        "kind": "gate", "cwd": str(self.workspace.parent), "target": target}])

    def test_real_metadata_resolver_registers_before_its_contained_command(self):
        (self.workspace / "Cargo.lock").write_text("fixture-lock\n")
        body = self.sync_function("reresolve_cargo_lock")
        body += '\nwt="$1"; cargo=/usr/bin/true; in_wt() { return 0; }; drift_check() { :; }; '
        body += 'alab_contained() { printf \'{"kind":"contained","target":"%s"}\\n\' "${CARGO_TARGET_DIR-unset}" >> "$FIXTURE_EVENTS"; }; '
        body += 'reresolve_cargo_lock'
        self.environment["CARGO_TARGET_DIR"] = str(self.target)
        done = self.shell(body, self.workspace.parent)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual([event["kind"] for event in self.recorded()], ["register", "contained"])
        self.assertEqual(self.recorded()[-1]["target"], str(self.target))
        self.environment.pop("CARGO_TARGET_DIR")
        done = self.shell(body, self.workspace.parent)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(self.recorded()[-1]["target"], "unset")

    def package_target(self, *, run_compiler=False):
        self.package_cache = self.home / "package-cache"
        self.package_cache.mkdir(exist_ok=True)
        self.package_output = self.package_cache / "rust-release-aarch64-apple-darwin-0123456789abcdef.noindex"
        self.compiler_ran = self.fixture / "package-compiler-ran"
        body = '. "$1/alab/package.sh"; python="$2"; cache="$3"; wt="$4"; '
        body += 'compiler_id=0123456789abcdef; target_triple=aarch64-apple-darwin; '
        body += 'alab_account_home() { printf "%s\\n" "$HOME"; }; '
        body += 'package_rust_target'
        if run_compiler:
            body += '; touch "$5"'
        return self.shell(body, ROOT, sys.executable, self.package_cache,
                          self.workspace.parent, self.compiler_ran)

    def test_real_package_registers_before_creation_and_reuses_the_same_target(self):
        done = self.package_target()
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(done.stdout.strip(), str(self.package_output))
        self.assertEqual(self.recorded(), [{"kind": "register", "target": str(self.package_output),
                                           "manifest": str(self.workspace / "Cargo.toml"),
                                           "existed": False, "strict": False, "cwd": str(self.workspace)}])
        marker = self.package_output / ".aterm-build-cache.json"
        self.assertEqual(marker.read_text(), "fixture-owned")
        inode = self.package_output.stat().st_ino
        (self.package_output / "output").write_bytes(b"inert reusable output")
        done = self.package_target()
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(done.stdout.strip(), str(self.package_output))
        self.assertEqual(self.package_output.stat().st_ino, inode)
        self.assertEqual((self.package_output / "output").read_bytes(), b"inert reusable output")
        self.assertTrue(self.recorded()[-1]["existed"])
        self.assertTrue(self.recorded()[-1]["strict"])

    def test_real_package_refused_registration_starts_no_compiler(self):
        self.environment["FIXTURE_MODE"] = "refuse"
        done = self.package_target(run_compiler=True)
        self.assertNotEqual(done.returncode, 0, done.stderr)
        self.assertFalse(self.compiler_ran.exists())
        self.assertFalse(self.package_output.exists())
        self.assertFalse(self.recorded()[0]["existed"])

    def test_real_package_marked_target_requires_successful_owner_publication(self):
        self.package_target()
        marker = self.package_output / ".aterm-build-cache.json"
        marker.write_text("fixture-owned")
        for mode in ("old", "refuse", "unmanaged"):
            with self.subTest(mode=mode):
                self.environment["FIXTURE_MODE"] = mode
                done = self.package_target(run_compiler=True)
                self.assertNotEqual(done.returncode, 0, done.stderr)
                self.assertFalse(self.compiler_ran.exists())
                self.assertEqual(marker.read_text(), "fixture-owned")

    def test_real_package_legacy_unmarked_and_old_host_targets_stay_unmanaged(self):
        self.environment["FIXTURE_MODE"] = "old"
        done = self.package_target()
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(done.stdout.strip(), str(self.package_output))
        self.assertTrue(self.package_output.is_dir())
        self.assertEqual(self.recorded(), [])
        (self.package_output / "output").write_bytes(b"legacy reusable output")
        self.environment["FIXTURE_MODE"] = "unmanaged"
        done = self.package_target()
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(done.stdout.strip(), str(self.package_output))
        self.assertEqual((self.package_output / "output").read_bytes(), b"legacy reusable output")
        self.assertTrue(self.recorded(), "the capable registrar must observe existing outputs")
        self.assertTrue(self.recorded()[0]["existed"])
        self.assertFalse((self.package_output / ".aterm-build-cache.json").exists())

    def test_containment_adds_only_read_authority_for_cache_receipts(self):
        done = self.shell('alab_account_home() { printf "%s\\n" "$HOME"; }; '
                          'cd "$1"; alab_contain_profile "$2" "$3" "$4"',
                          self.workspace, self.fixture / "scratch", self.fixture / "cargo-home", self.target)
        self.assertEqual(done.returncode, 0, done.stderr)
        index = str(self.home / "Library/Caches/aterm/.ubd/registered-builds.noindex")
        self.assertIn(f'(subpath "{index}")', done.stdout)
        writes = done.stdout.split("(allow file-write*", 1)[1].split("\n)", 1)[0]
        self.assertNotIn(index, writes)
        self.assertIn(f'(subpath "{index}")', done.stdout.split("(deny file-write*", 2)[-1])

    def test_containment_admits_only_the_account_capacity_protocol_and_refuses_aliases(self):
        account = self.fixture / "account"
        capacity = account / "Library/Caches/aterm/capacity"
        capacity.mkdir(parents=True)
        body = ('fixture_account="$5"; alab_account_home() { printf "%s\\n" "$fixture_account"; }; '
                'cd "$1"; alab_contain_profile "$2" "$3" --read "$4" "$6"')
        # Declare the account home separately from the scratch HOME. The exact
        # capacity directory is writable, while an explicit --read fence wins.
        done = self.shell(body, self.workspace, self.fixture / "scratch",
                          self.fixture / "cargo-home", capacity, account, self.target)
        self.assertEqual(done.returncode, 0, done.stderr)
        writes = done.stdout.split("(allow file-write*", 1)[1].split("\n)", 1)[0]
        self.assertIn(f'(subpath "{capacity}")', writes)
        self.assertNotIn(f'(subpath "{capacity.parent}")', writes)
        self.assertIn(f'(subpath "{capacity}")', done.stdout.split("(deny file-write*", 2)[-1])
        capacity.rmdir()
        capacity.symlink_to(self.fixture)
        done = self.shell(body, self.workspace, self.fixture / "scratch",
                          self.fixture / "cargo-home", capacity, account, self.target)
        self.assertNotEqual(done.returncode, 0)
        self.assertIn("aliased or unsafe", done.stderr)


if __name__ == "__main__":
    unittest.main()

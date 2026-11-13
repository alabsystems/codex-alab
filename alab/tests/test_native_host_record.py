# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The native host build record keeps what was measured apart from what was attested."""
import hashlib
import json
import os
from pathlib import Path
import secrets
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

SMOKE = Path(__file__).resolve().parents[1] / "smoke"
sys.path.insert(0, str(SMOKE))
import native_host_record as record

PASS = "PASS real-host shipping-qualified policy and wrong-token refusal; worker EOF joined"
EVIDENCE = "stand-in host written by the unit test; no aterm build exists for it"
BUILDER = "the unit test"
BUILT_AT = "2026-11-12T09:30:00-08:00"
CLEAN = {"attestation": "built-from-clean-commit", "builder": BUILDER, "built_at": BUILT_AT}
GIT_ENV = {"PATH": "/usr/bin:/bin", "HOME": "/var/empty", "GIT_CONFIG_GLOBAL": "/dev/null",
           "GIT_CONFIG_NOSYSTEM": "1"}


def git(repo, *args):
    return subprocess.run(["/usr/bin/git", "-C", str(repo), *args], env=GIT_ENV, check=True,
                          capture_output=True, text=True).stdout.strip()


class Workspace:
    """A temp directory holding a stand-in host and an `aterm` checkout with one commit."""

    def __init__(self, case):
        self.root = Path(case.enterContext(tempfile.TemporaryDirectory())).resolve()
        self.checkout = self.root / "aterm"
        self.checkout.mkdir()
        git(self.checkout, "init", "--quiet")
        git(self.checkout, "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
            "commit", "--quiet", "--no-gpg-sign", "--allow-empty", "-m", "fixture")
        self.source = git(self.checkout, "rev-parse", "HEAD")
        self.world = Path("/tmp") / ("native-host-unittest-" + secrets.token_hex(8))
        case.addCleanup(shutil.rmtree, self.world, True)
        self.host = self.stand_in("host", 0, PASS)

    def stand_in(self, name, code, verdict, world=True, file="native_host"):
        directory = self.root / name
        directory.mkdir()
        host = directory / file
        lines = ["#!/bin/sh"]
        if world:
            lines += [f"/bin/mkdir -p {self.world}", f"echo 'WORLD native_host={self.world}'"]
        lines += [f"echo '{verdict}'" if code == 0 else f"echo '{verdict}' >&2", f"exit {code}"]
        host.write_text("\n".join(lines) + "\n")
        host.chmod(0o755)
        return host

    def write(self, value, host=None):
        path = (host or self.host).parent / "build.json"
        path.write_text(json.dumps(value))
        return path

    def good(self, host=None, **changes):
        host = host or self.host
        value = {"schema": 3, "source": self.source, "source_evidence": EVIDENCE, **CLEAN,
                 "host_sha256": hashlib.sha256(host.read_bytes()).hexdigest(),
                 "host_bytes": host.stat().st_size, "platform": "darwin-arm64",
                 "shipping_probe": {"ran": True, "returncode": 0, "verdict": PASS,
                                    "seconds": 1.0, "world": None}}
        value.update(changes)
        return value


class RecordTests(unittest.TestCase):
    def setUp(self):
        self.work = Workspace(self)

    def test_a_recorded_host_loads_and_names_only_what_was_measured_or_attested(self):
        work = self.work
        written = record.record(work.host, work.source, work.checkout, "  " + EVIDENCE + " ",
                                **CLEAN)
        loaded = record.load(work.host)
        self.assertEqual(written, loaded)
        self.assertEqual(set(loaded), {"schema", "source", "source_evidence", "attestation",
                                       "builder", "built_at", "host_sha256", "host_bytes",
                                       "platform", "shipping_probe"})
        self.assertEqual((loaded["schema"], loaded["source"], loaded["source_evidence"]),
                         (3, work.source, EVIDENCE))
        self.assertEqual((loaded["attestation"], loaded["builder"], loaded["built_at"]),
                         ("built-from-clean-commit", BUILDER, BUILT_AT))
        self.assertEqual(loaded["host_sha256"], hashlib.sha256(work.host.read_bytes()).hexdigest())
        self.assertEqual(loaded["host_bytes"], work.host.stat().st_size)
        outcome = loaded["shipping_probe"]
        self.assertEqual(set(outcome), {"ran", "returncode", "verdict", "seconds", "world"})
        self.assertEqual((outcome["ran"], outcome["returncode"], outcome["verdict"]), (True, 0, PASS))
        # The self-run's kept world was the host's own and is gone.
        self.assertEqual(outcome["world"], {"path": str(work.world), "removed": True})
        self.assertFalse(work.world.exists())
        self.assertEqual(record.summary(loaded), {"source": work.source, "sourceEvidence": EVIDENCE,
            "attestation": "built-from-clean-commit", "builder": BUILDER, "builtAt": BUILT_AT,
            "qualifies": True, "sha256": loaded["host_sha256"], "shippingProbe": outcome})

    def test_only_a_clean_build_that_ran_its_self_check_qualifies_a_run(self):
        work = self.work
        clean = record.record(work.host, work.source, work.checkout, EVIDENCE, **CLEAN)
        self.assertIsNone(record.not_qualifying(clean))
        # A trial names the same commit and passes the same self-run, and qualifies nothing.
        host = work.stand_in("trial", 0, PASS)
        trial = record.record(host, work.source, work.checkout, EVIDENCE, "trial")
        self.assertEqual(record.load(host), trial)
        self.assertEqual((trial["attestation"], trial["builder"], trial["built_at"]),
                         ("trial", None, None))
        self.assertEqual((trial["source"], trial["shipping_probe"]["returncode"]), (work.source, 0))
        self.assertIn("`trial`", record.not_qualifying(trial))
        self.assertIs(record.summary(trial)["qualifies"], False)
        self.assertEqual(record.summary(trial)["attestation"], "trial")
        # A clean build whose self-run was skipped qualifies nothing either.
        host = work.stand_in("unprobed-clean", 0, PASS)
        unprobed = record.record(host, work.source, work.checkout, EVIDENCE, probe=False, **CLEAN)
        self.assertEqual(record.load(host), unprobed)
        self.assertIn("--no-probe", record.not_qualifying(unprobed))
        self.assertIs(record.summary(unprobed)["qualifies"], False)

    def test_the_attestation_is_one_of_two_kinds_exactly_stated(self):
        work = self.work
        refused = {
            "none": {"attestation": None},
            "an unknown kind": {"attestation": "qualified"},
            "a trial with a builder": {"attestation": "trial", "builder": BUILDER},
            "a trial with a build time": {"attestation": "trial", "built_at": BUILT_AT},
            "a clean build by nobody": dict(CLEAN, builder=None),
            "a clean build by a blank": dict(CLEAN, builder="  "),
            "a clean build at no time": dict(CLEAN, built_at=None),
            "a clean build on a date alone": dict(CLEAN, built_at="2026-11-12"),
            "a clean build at a time with no offset": dict(CLEAN, built_at="2026-11-12T09:30:00"),
            "a clean build at a time that is not one": dict(CLEAN, built_at="this morning"),
        }
        for name, fields in refused.items():
            with self.subTest(name=name):
                with self.assertRaises(RuntimeError):
                    record.record(work.host, work.source, work.checkout, EVIDENCE, **fields)
                self.assertFalse((work.host.parent / "build.json").exists())
                # The same record, written by hand, does not load.
                work.write(work.good(**dict({"builder": None, "built_at": None}, **fields)))
                with self.assertRaisesRegex(RuntimeError, "attestation is not valid"):
                    record.load(work.host)
                (work.host.parent / "build.json").unlink()
        with self.assertRaisesRegex(RuntimeError, "attestation is not valid"):
            work.write({key: value for key, value in work.good().items() if key != "attestation"})
            record.load(work.host)

    def test_the_self_run_inherits_nothing_of_the_callers_session(self):
        work = self.work
        seen = work.root / "seen-environment"
        host = work.root / "spy" / "native_host"
        host.parent.mkdir()
        host.write_text(f"#!/bin/sh\n/usr/bin/env > {seen}\necho '{PASS}'\n")
        host.chmod(0o755)
        with mock.patch.dict(os.environ, {"ATERM_SESSION_ID": "leak", "CODEX_HOME": "/leak",
                                          "ATERM_HARNESS_STATE": "/leak-state"}):
            outcome = record.run_probe(host)
        self.assertEqual((outcome["returncode"], outcome["world"]), (0, None))
        environment = dict(line.split("=", 1) for line in seen.read_text().splitlines() if "=" in line)
        self.assertNotIn("leak", json.dumps(environment))
        self.assertFalse([name for name in environment if name.startswith("CODEX_")])
        self.assertEqual([name for name in environment if name.startswith("ATERM_")],
                         ["ATERM_HARNESS_STATE"])
        for name in ("HOME", "XDG_CONFIG_HOME", "ATERM_HARNESS_STATE", "TMPDIR"):
            self.assertIn("native-host-record-", environment[name])

    def test_a_host_that_fails_its_self_run_is_not_recorded(self):
        work = self.work
        for name, code, verdict in (("release", 2, "native_host: NOT RUN: requires a Unix debug build"),
                                    ("failed", 1, "native_host: FAIL: policy"),
                                    ("silent", 0, "")):
            with self.subTest(name=name):
                host = work.stand_in(name, code, verdict)
                with self.assertRaisesRegex(RuntimeError, "did not pass its own"):
                    record.record(host, work.source, work.checkout, EVIDENCE, "trial")
                self.assertFalse((host.parent / "build.json").exists())
        # A failed self-run's world is evidence: it is reported, never removed.
        host = work.stand_in("kept", 1, "native_host: FAIL: policy")
        outcome = record.run_probe(host)
        self.assertEqual(outcome["world"], {"path": str(work.world), "removed": False})
        self.assertTrue(work.world.is_dir())
        # Without the self-run the record says so, and is usable as such.
        host = work.stand_in("unprobed", 2, "native_host: NOT RUN: requires a Unix debug build")
        written = record.record(host, work.source, work.checkout, EVIDENCE, "trial", probe=False)
        self.assertEqual(written["shipping_probe"], {"ran": False, "returncode": None,
            "verdict": "", "seconds": 0, "world": None})
        self.assertEqual(record.load(host), written)

    def test_a_self_run_that_never_answers_is_stopped_and_refused(self):
        work = self.work
        host = work.root / "hung" / "native_host"
        host.parent.mkdir()
        host.write_text("#!/bin/sh\nexec /bin/sleep 600\n")
        host.chmod(0o755)
        with self.assertRaisesRegex(RuntimeError, "stopped after 1 s"):
            record.record(host, work.source, work.checkout, EVIDENCE, "trial", probe_seconds=1)
        self.assertFalse((host.parent / "build.json").exists())

    def test_the_source_is_attested_but_must_be_exact_real_and_explained(self):
        work = self.work
        unknown = "0" * 39 + "1"
        cases = {"abbreviated": (work.source[:12], EVIDENCE), "not hex": ("g" * 40, EVIDENCE),
                 "upper case": (work.source.upper(), EVIDENCE), "unknown commit": (unknown, EVIDENCE),
                 "no evidence": (work.source, "  "), "a tree, not a commit":
                     (git(work.checkout, "rev-parse", "HEAD^{tree}"), EVIDENCE)}
        for name, (source, evidence) in cases.items():
            with self.subTest(name=name):
                with self.assertRaises(RuntimeError):
                    record.record(work.host, source, work.checkout, evidence, "trial")
                self.assertFalse((work.host.parent / "build.json").exists())
        with self.assertRaises(RuntimeError):
            record.record(work.host, work.source, work.root / "no-checkout", EVIDENCE, "trial")

    def test_an_existing_record_is_never_replaced_and_a_matching_legacy_one_is_kept(self):
        work = self.work
        record.record(work.host, work.source, work.checkout, EVIDENCE, "trial")
        before = (work.host.parent / "build.json").read_bytes()
        # Not even to turn a trial into a qualifying record.
        for fields in ({"attestation": "trial"}, CLEAN):
            with self.assertRaisesRegex(RuntimeError, "not replaced"):
                record.record(work.host, work.source, work.checkout, "another story", **fields)
        self.assertEqual((work.host.parent / "build.json").read_bytes(), before)

        digest = hashlib.sha256(work.host.read_bytes()).hexdigest()
        legacy = {"source": work.source, "host_sha256": digest, "production_constructor_enabled": False}
        for name, value in (("other source", dict(legacy, source="1" * 40)),
                            ("other image", dict(legacy, host_sha256="0" * 64)),
                            ("a later schema", dict(legacy, schema=4)),
                            ("a schema that is no number", dict(legacy, schema=True))):
            with self.subTest(name=name):
                path = work.write(value)
                with self.assertRaisesRegex(RuntimeError, "not replaced"):
                    record.record(work.host, work.source, work.checkout, EVIDENCE, "trial")
                self.assertEqual(json.loads(path.read_text()), value)
        path = work.write(legacy)
        with self.assertRaisesRegex(RuntimeError, "not schema 3"):
            record.load(work.host)
        record.record(work.host, work.source, work.checkout, EVIDENCE, "trial")
        self.assertEqual(json.loads((work.host.parent / "build.json.legacy").read_text()), legacy)
        self.assertEqual(record.load(work.host)["schema"], 3)
        # A second retirement would have to overwrite the first: refused.
        path.unlink()
        work.write(legacy)
        with self.assertRaisesRegex(RuntimeError, "retired"):
            record.record(work.host, work.source, work.checkout, EVIDENCE, "trial")

    def test_a_schema_2_record_no_longer_loads_and_is_retired_by_a_new_recording(self):
        # Schema 2 had no attestation: a trial and a qualifying record were the same to a
        # program. Such a record of this image and source is kept as build.json.legacy.
        work = self.work
        old = {key: value for key, value in work.good(schema=2).items()
               if key not in ("attestation", "builder", "built_at")}
        work.write(old)
        with self.assertRaisesRegex(RuntimeError, "not schema 3"):
            record.load(work.host)
        written = record.record(work.host, work.source, work.checkout, EVIDENCE, "trial")
        self.assertEqual(json.loads((work.host.parent / "build.json.legacy").read_text()), old)
        self.assertEqual(record.load(work.host), written)
        self.assertEqual((written["schema"], written["attestation"]), (3, "trial"))

    def test_load_refuses_every_record_that_does_not_describe_this_host(self):
        work = self.work
        path = work.host.parent / "build.json"
        with self.assertRaisesRegex(RuntimeError, "missing"):
            record.load(work.host)
        work.write(work.good())
        self.assertEqual(record.load(work.host)["source"], work.source)
        probe = work.good()["shipping_probe"]
        refused = {
            "schema 1": work.good(schema=1), "schema 2": work.good(schema=2),
            "schema true": work.good(schema=True), "schema text": work.good(schema="3"),
            "short source": work.good(source=work.source[:12]),
            "no evidence": work.good(source_evidence=""),
            "wrong hash": work.good(host_sha256="0" * 64),
            "wrong size": work.good(host_bytes=work.host.stat().st_size + 1),
            "no probe entry": {key: value for key, value in work.good().items() if key != "shipping_probe"},
            "probe returncode 2": work.good(shipping_probe=dict(probe, returncode=2)),
            "probe not run verdict": work.good(shipping_probe=dict(probe, returncode=0,
                verdict="native_host: NOT RUN: requires a Unix debug build")),
            "probe stopped": work.good(shipping_probe=dict(probe, returncode=None)),
            "probe ran is text": work.good(shipping_probe=dict(probe, ran="yes")),
            "a list": [work.good()],
        }
        for name, value in refused.items():
            with self.subTest(name=name):
                work.write(value)
                with self.assertRaises(RuntimeError):
                    record.load(work.host)
        path.write_text("{not json")
        with self.assertRaises(RuntimeError):
            record.load(work.host)
        path.write_text(json.dumps(work.good()) + " " * (1024 * 1024))
        with self.assertRaisesRegex(RuntimeError, "oversized"):
            record.load(work.host)
        path.unlink()
        elsewhere = work.root / "elsewhere.json"
        elsewhere.write_text(json.dumps(work.good()))
        path.symlink_to(elsewhere)
        with self.assertRaisesRegex(RuntimeError, "missing"):
            record.load(work.host)
        # The image is what is bound: a changed host no longer has a record.
        path.unlink()
        work.write(work.good())
        work.host.write_text(work.host.read_text() + "# changed\n")
        with self.assertRaisesRegex(RuntimeError, "disagrees"):
            record.load(work.host)


class WorldCleanupTests(unittest.TestCase):
    def directory(self, parent, name=None):
        parent.mkdir(exist_ok=True)
        path = parent / (name or "native-host-unittest-" + secrets.token_hex(8))
        path.mkdir()
        (path / "kept-file").write_text("world\n")
        self.addCleanup(shutil.rmtree, path, True)
        return path

    def test_only_an_owned_self_run_directory_in_its_two_homes_is_removed(self):
        for parent in (Path("/private/tmp"), Path("/private/tmp/.ubd"), Path("/tmp"), Path("/tmp/.ubd")):
            with self.subTest(parent=str(parent)):
                path = self.directory(parent)
                self.assertTrue(record.remove_world(str(path)))
                self.assertFalse(path.exists())

    def test_any_other_parent_name_owner_or_kind_is_left_alone(self):
        scratch = Path(self.enterContext(tempfile.TemporaryDirectory())).resolve()
        elsewhere = self.directory(scratch, "native-host-1_ab_0")
        nested = self.directory(self.directory(Path("/private/tmp")), "native-host-1_ab_0")
        misnamed = self.directory(Path("/private/tmp"), "other-host-" + secrets.token_hex(8))
        spaced = self.directory(Path("/private/tmp"), "native-host-" + secrets.token_hex(4) + " x")
        target = self.directory(scratch, "target")
        link = Path("/private/tmp") / ("native-host-unittest-" + secrets.token_hex(8))
        link.symlink_to(target)
        self.addCleanup(link.unlink)
        plain = Path("/private/tmp") / ("native-host-unittest-" + secrets.token_hex(8))
        plain.write_text("a file\n")
        self.addCleanup(plain.unlink)
        for path in (elsewhere, nested, misnamed, spaced, link, plain):
            with self.subTest(path=str(path)):
                self.assertFalse(record.remove_world(str(path)))
                self.assertTrue(path.exists())
        self.assertTrue((target / "kept-file").is_file())
        relative = self.directory(Path("/private/tmp"))
        with mock.patch.object(record.os, "getuid", return_value=os.getuid() + 1):
            self.assertFalse(record.remove_world(str(relative)))
        self.assertTrue((relative / "kept-file").is_file())
        self.assertFalse(record.remove_world("native-host-relative"))
        self.assertFalse(record.remove_world("/private/tmp/native-host-absent-" + secrets.token_hex(8)))


class MatrixTests(unittest.TestCase):
    def test_a_host_without_a_record_is_refused_by_name_before_any_case_runs(self):
        work = Workspace(self)
        package = work.root / "package"
        package.mkdir()
        output = work.root / "matrix"
        completed = subprocess.run([sys.executable, "-B", str(SMOKE / "renewal_matrix.py"),
            "--host", str(work.host), "--package", str(package), "--output", str(output),
            "--case", "weekly-quota"], capture_output=True, text=True, timeout=60)
        self.assertEqual(completed.returncode, 1)
        self.assertNotIn("Traceback", completed.stderr)
        self.assertEqual(json.loads((output / "qualification.json").read_text()), {
            "passed": False, "cases": [],
            "error": "native host build record missing; run native_host_record.py"})
        self.assertEqual(sorted(path.name for path in output.iterdir()), ["qualification.json"])
        # A record that does not describe the host is refused with its own reason.
        work.write(work.good(host_sha256="0" * 64))
        second = work.root / "matrix-2"
        completed = subprocess.run([sys.executable, "-B", str(SMOKE / "renewal_matrix.py"),
            "--host", str(work.host), "--package", str(package), "--output", str(second),
            "--case", "weekly-quota"], capture_output=True, text=True, timeout=60)
        self.assertEqual(completed.returncode, 1)
        self.assertNotIn("Traceback", completed.stderr)
        self.assertEqual(json.loads((second / "qualification.json").read_text())["error"],
                         "native host image disagrees with build record")
        # A recorded host whose process aterm's live reader would not take for aterm.
        work.write(work.good())
        third = work.root / "matrix-3"
        completed = subprocess.run([sys.executable, "-B", str(SMOKE / "renewal_matrix.py"),
            "--host", str(work.host), "--package", str(package), "--output", str(third),
            "--case", "weekly-quota"], capture_output=True, text=True, timeout=60)
        self.assertEqual(completed.returncode, 1)
        self.assertNotIn("Traceback", completed.stderr)
        error = json.loads((third / "qualification.json").read_text())["error"]
        self.assertIn("named 'native_host'", error)
        self.assertIn("aterm-native-host", error)

    def test_a_record_that_qualifies_no_run_is_refused_before_any_case_runs(self):
        work = Workspace(self)
        package = work.root / "package"
        package.mkdir()
        host = work.stand_in("named", 0, PASS, file="aterm-native-host")
        probe = work.good()["shipping_probe"]
        refused = {
            "its attestation is `trial`": work.good(host, attestation="trial", builder=None,
                                                    built_at=None),
            "self-run was not made": work.good(host, shipping_probe=dict(
                probe, ran=False, returncode=None, verdict="", seconds=0)),
        }
        for index, (reason, value) in enumerate(refused.items()):
            with self.subTest(reason=reason):
                work.write(value, host)
                # The record loads, and names an exact commit: only its fields refuse it.
                self.assertEqual(record.load(host)["source"], work.source)
                output = work.root / f"matrix-{index}"
                completed = subprocess.run([sys.executable, "-B", str(SMOKE / "renewal_matrix.py"),
                    "--host", str(host), "--package", str(package), "--output", str(output),
                    "--case", "weekly-quota"], capture_output=True, text=True, timeout=60)
                self.assertEqual(completed.returncode, 1)
                self.assertNotIn("Traceback", completed.stderr)
                manifest = json.loads((output / "qualification.json").read_text())
                self.assertEqual((manifest["passed"], manifest["cases"]), (False, []))
                self.assertIn("qualifies no run", manifest["error"])
                self.assertIn(reason, manifest["error"])
                self.assertIn("renewal_follow.py", manifest["error"])
                self.assertEqual(sorted(path.name for path in output.iterdir()), ["qualification.json"])


class ProcessNameTests(unittest.TestCase):
    def test_only_a_host_file_named_as_aterm_can_publish_a_live_snapshot(self):
        for name in ("aterm", "aterm-native-host", "aterm-gui"):
            with self.subTest(name=name):
                record.require_aterm_name(Path("/hosts") / name)
        for name in ("native_host", "native_host-0f3a", "aterminal", "xaterm-host", "Aterm-host",
                     "aterm_native_host"):
            with self.subTest(name=name):
                with self.assertRaisesRegex(RuntimeError, "aterm-native-host"):
                    record.require_aterm_name(Path("/hosts/aterm-dir") / name)


if __name__ == "__main__":
    unittest.main()

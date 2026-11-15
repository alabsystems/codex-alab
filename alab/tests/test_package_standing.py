# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""A package qualifies a run only by the record a certified packager writes.

fixtures/package-standing/certified-native.json is the signed 0.7.0 package's own
alab-build.json (codex-release's native layout) with its sysroot replaced by a path that
names no home and its compiler id recomputed; certified-package-sh.json has the keys
alab/package.sh's alab_build_json and proof_build.py write. No package.sh package with a
`verification` exists on this Mac to copy one from; the one that does exist
(0.1.0-dev.13916, built 2026-10-28 on the unverified lane) has that fixture's keys
without `verification`: the shape test_a_package_sh_record_must_carry_its_certified_build
refuses.
"""
import copy
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest

SMOKE = Path(__file__).resolve().parents[1] / "smoke"
sys.path.insert(0, str(SMOKE))
import native_host_record
import package_standing as standing

FIXTURES = Path(__file__).resolve().parent / "fixtures" / "package-standing"
PASS = "PASS real-host shipping-qualified policy and wrong-token refusal; worker EOF joined"
TRIAL = {"attestation": "unverified-development-build", "source": "2" * 40,
         "binary_sha256": "3" * 64, "binary_bytes": 17, "builder": "the unit test",
         "built_at": "2026-11-13T09:30:00-08:00", "evidence": "written by the unit test"}


def certified(name="certified-native.json"):
    return json.loads((FIXTURES / name).read_text())


def trial_record():
    return {"source": "2" * 40, "version": "0.8.0-trial+g222222222",
            "target": "aarch64-apple-darwin",
            "compiler": {"admitted": False, "lane": "unverified"},
            "packager": "alab/smoke/trial_package.py", "trial": dict(TRIAL)}


def changed(record, *path, value=None, drop=False):
    """A deep copy of `record` with the field at `path` replaced, or removed."""
    record = copy.deepcopy(record)
    at = record
    for key in path[:-1]:
        at = at[key]
    if drop:
        del at[path[-1]]
    else:
        at[path[-1]] = value
    return record


def identified(record):
    """`record` with the compiler id its other compiler fields give."""
    compiler = record["compiler"]
    compiler["id"] = hashlib.sha256("\0".join(
        [compiler["rustc_vV"], compiler["sysroot"], compiler["binaries_sha256"]]).encode()
    ).hexdigest()[:16]
    return record


class StandingTests(unittest.TestCase):
    def test_changed_or_malformed_shipping_byte_observations_do_not_qualify(self):
        record = certified("certified-package-sh.json")
        for binary in ("codex", "codex-code-mode-host", "codex-exec-server"):
            for observation in ({"built_sha256": "a" * 64, "packaged_sha256": "b" * 64},
                                {"built_sha256": "a" * 64}, "not an observation"):
                with self.subTest(binary=binary, observation=observation):
                    candidate = changed(record, "compiler", "verification", "binaries", binary,
                                        value=observation)
                    self.assertIsNotNone(standing.not_qualifying(candidate))
                    self.assertFalse(standing.summary(candidate)["qualifies"])

    def test_the_records_both_certified_packagers_write_qualify(self):
        native = certified()
        # The signed record's own keys: what the predicate may rely on.
        self.assertEqual(set(native), {"baseline", "build_observation_sha256", "client_version",
            "compiler", "packager", "ripgrep", "ripgrep_build_observation_sha256",
            "ripgrep_features", "source", "target", "updater", "upstream", "version"})
        self.assertEqual(set(native["compiler"]), {"admitted", "binaries_sha256", "id", "rustc",
                                                   "rustc_vV", "sysroot"})
        for name in ("certified-native.json", "certified-package-sh.json"):
            with self.subTest(name=name):
                record = certified(name)
                self.assertIsNone(standing.not_qualifying(record))
                self.assertEqual(standing.summary(record), {
                    "qualifies": True, "why": standing.QUALIFIES, "trial": None})
        self.assertIn("no signature was verified", standing.QUALIFIES)

    def test_a_trial_record_never_qualifies(self):
        record = trial_record()
        why = standing.not_qualifying(record)
        self.assertIn("`trial` object", why)
        self.assertIn("'unverified-development-build'", why)
        self.assertEqual(standing.summary(record), {"qualifies": False, "why": why, "trial": TRIAL})
        # Not even on a record that is certified in every other field, whatever it holds.
        for marker in (TRIAL, {}, None, False, "no"):
            with self.subTest(marker=marker):
                record = dict(certified(), trial=marker)
                self.assertIn("`trial` object", standing.not_qualifying(record))
                self.assertEqual((standing.summary(record)["qualifies"],
                                  standing.summary(record)["trial"]), (False, marker))
        # Nor is a trial without its marker taken for certified: its compiler says so.
        unmarked = changed(trial_record(), "trial", drop=True)
        self.assertEqual(standing.not_qualifying(unmarked), "its version '0.8.0-trial+g222222222' "
                         "names a trial")
        self.assertEqual(standing.not_qualifying(dict(unmarked, version="0.8.0")),
                         "its compiler was not admitted (compiler.admitted is not true)")

    def test_a_certified_record_must_hold_every_field_a_certified_packager_writes(self):
        native, script = certified(), certified("certified-package-sh.json")
        other = native["compiler"]["rustc_vV"].replace("binary: trustc", "binary: rustc")
        refused = {
            "admitted false": (changed(native, "compiler", "admitted", value=False), "not admitted"),
            "admitted as text": (changed(native, "compiler", "admitted", value="true"), "not admitted"),
            "admitted as a number": (changed(native, "compiler", "admitted", value=1), "not admitted"),
            "no admitted": (changed(native, "compiler", "admitted", drop=True), "not admitted"),
            "no compiler": (changed(native, "compiler", drop=True), "no compiler"),
            "a compiler that is text": (changed(native, "compiler", value="trustc"), "no compiler"),
            "the unverified lane": (changed(native, "compiler", "lane", value="unverified"),
                                    "lane is 'unverified'"),
            "an unknown lane": (changed(native, "compiler", "lane", value=None), "lane is None"),
            "no rustc -vV": (changed(native, "compiler", "rustc_vV", drop=True), "no rustc -vV"),
            "no sysroot": (changed(native, "compiler", "sysroot", value=" "), "no rustc -vV"),
            "a stock compiler": (identified(changed(native, "compiler", "rustc_vV", value=other)),
                                 "not recorded as Trust"),
            "another rustc line": (changed(native, "compiler", "rustc", value="rustc 1.0.0"),
                                   "not recorded as Trust"),
            "a short binaries hash": (changed(native, "compiler", "binaries_sha256", value="fd3b"),
                                      "no hash of the compiler's binaries"),
            "another id": (changed(native, "compiler", "id", value="0" * 16), "compiler id"),
            "no id": (changed(native, "compiler", "id", drop=True), "compiler id"),
            "a sysroot the id was not made from": (
                changed(native, "compiler", "sysroot", value="/elsewhere"), "compiler id"),
            "a verification on another lane": (
                changed(script, "compiler", "verification", "lane", value="unverified"),
                "not a certified build"),
            "a verification without dependencies": (
                changed(script, "compiler", "verification", "include_dependencies", value=False),
                "not a certified build"),
            "a verification of another source": (
                changed(script, "compiler", "verification", "source", value="4" * 40),
                "not a certified build"),
            "a verification that is text": (
                changed(script, "compiler", "verification", value="certify"),
                "not a certified build"),
            "a verification with no report": (
                changed(script, "compiler", "verification", "report_sha256", drop=True),
                "not a certified build"),
            "a verification that binds no codex": (
                changed(script, "compiler", "verification", "binaries", value={}),
                "not a certified build"),
            "a verification whose codex has no hash": (
                changed(script, "compiler", "verification", "binaries", "codex", "packaged_sha256",
                        value="c"), "not a certified build"),
            "a native record with no build observation": (
                changed(native, "build_observation_sha256", drop=True),
                "no build_observation_sha256"),
            "a native record with no ripgrep build observation": (
                changed(native, "ripgrep_build_observation_sha256", value="abfb"),
                "no ripgrep_build_observation_sha256"),
            "a packager nobody knows": (changed(native, "packager", value="someone's script"),
                                        "neither codex-release's native layout nor a commit"),
            "a package.sh record with no rusty_v8": (changed(script, "rusty_v8", drop=True),
                                                     "no rusty_v8"),
            "a package.sh record with no build time": (changed(script, "built_at", value=""),
                                                       "no built_at"),
            "a trial version in capitals": (
                changed(native, "version", value="0.8.0-TRIAL+gd75c9b471"), "names a trial"),
            "an abbreviated source": (changed(native, "source", value=native["source"][:12]),
                                      "no exact source commit"),
            "no source": (changed(native, "source", drop=True), "no exact source commit"),
            "no version": (changed(native, "version", drop=True), "no version"),
            "a version that is a number": (changed(native, "version", value=7), "no version"),
            "a trial version": (changed(native, "version", value="0.7.0+trial.d75c9b471"),
                                "names a trial"),
            "no target": (changed(native, "target", drop=True), "no target"),
            "no packager": (changed(native, "packager", value=""), "no packager"),
            "no ripgrep": (changed(native, "ripgrep", drop=True), "no ripgrep"),
            "no ripgrep features": (changed(native, "ripgrep_features", drop=True),
                                    "no ripgrep_features"),
        }
        for name, (record, reason) in refused.items():
            with self.subTest(name=name):
                why = standing.not_qualifying(record)
                self.assertIsNotNone(why)
                self.assertIn(reason, why)
                self.assertEqual(standing.summary(record), {"qualifies": False, "why": why,
                                                            "trial": None})
        # The controls: each fixture as written, and a lane named as the certified one.
        self.assertIsNone(standing.not_qualifying(native))
        self.assertIsNone(standing.not_qualifying(script))
        self.assertIsNone(standing.not_qualifying(changed(native, "compiler", "lane", value="certify")))

    def test_a_package_sh_record_must_carry_its_certified_build(self):
        # package.sh wrote `admitted: true` while it still built on the unverified lane,
        # until 18a4e8404b (2026-11-12): such a record says nothing of a certified build.
        script = certified("certified-package-sh.json")
        self.assertIsNotNone(re.fullmatch(r"[0-9a-f]{40}", script["packager"]))
        before = changed(script, "compiler", "verification", drop=True)
        self.assertEqual((before["compiler"]["admitted"], "trial" in before), (True, False))
        why = standing.not_qualifying(before)
        self.assertIn("carries no build verification (compiler.verification)", why)
        self.assertIn("unverified lane", why)
        self.assertEqual(standing.summary(before), {"qualifies": False, "why": why, "trial": None})
        # The native layout's evidence is its own: its record needs no `verification`,
        # and a package.sh record cannot borrow the native layout's fields instead.
        native = certified()
        self.assertNotIn("verification", native["compiler"])
        self.assertIsNone(standing.not_qualifying(native))
        borrowed = dict(before, build_observation_sha256=native["build_observation_sha256"],
            ripgrep_build_observation_sha256=native["ripgrep_build_observation_sha256"])
        self.assertEqual(standing.not_qualifying(borrowed), why)
        # Nor does the native layout's name excuse a verification that is not one.
        named = changed(dict(script, packager=native["packager"],
            build_observation_sha256=native["build_observation_sha256"],
            ripgrep_build_observation_sha256=native["ripgrep_build_observation_sha256"]),
            "compiler", "verification", "lane", value="unverified")
        self.assertIn("not a certified build", standing.not_qualifying(named))

    def test_a_record_that_is_not_unicode_text_is_refused_and_not_raised_on(self):
        # json.loads takes half of a surrogate pair; UTF-8, and so the compiler id, does not.
        half = json.loads('"\\ud800"')
        native = certified()
        why = "its record holds text that is not Unicode: no certified packager wrote it"
        for name, record in {
                "sysroot": changed(native, "compiler", "sysroot", value="/opt/" + half),
                "rustc -vV": changed(native, "compiler", "rustc_vV",
                                     value=native["compiler"]["rustc_vV"] + half + "\n"),
                "version": changed(native, "version", value="0.7.0" + half),
                "a field the predicate does not read": dict(native, note=[{"said": half}]),
                "a key": dict(native, **{half: 1})}.items():
            with self.subTest(name=name):
                self.assertEqual(standing.not_qualifying(record), why)
                self.assertEqual(standing.summary(record), {"qualifies": False, "trial": None,
                                                            "why": why})
        # A trial marker is still what its record is refused for, and is kept as written.
        marked = dict(native, trial={"attestation": half})
        summary = standing.summary(marked)
        self.assertIn("`trial` object (attestation '\\ud800')", summary["why"])
        self.assertEqual((summary["qualifies"], summary["trial"]), (False, {"attestation": half}))
        json.dumps(summary)

    def test_an_unknown_or_malformed_record_does_not_qualify(self):
        stand_in = {"version": "0.0.0-stand-in", "target": "stand-in", "source": "0" * 40}
        refused = {"nothing": None, "a list": [certified()], "text": "certified", "a number": 1,
                   "true": True, "an empty object": {}, "a stand-in": stand_in}
        for name, record in refused.items():
            with self.subTest(name=name):
                summary = standing.summary(record)
                self.assertEqual((summary["qualifies"], summary["trial"]), (False, None))
                self.assertEqual(summary["why"], standing.not_qualifying(record))
                self.assertTrue(summary["why"])
        self.assertEqual(standing.not_qualifying([]), "its alab-build.json is not a JSON object")
        self.assertEqual(standing.not_qualifying(stand_in), "its record names no compiler")


class MatrixTests(unittest.TestCase):
    """renewal_matrix.py refuses a package that qualifies nothing as it refuses a trial host."""

    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory())).resolve()
        # A host whose record qualifies a run, so that only the package can be refused.
        self.host = self.root / "host" / "aterm-stand-in-host"
        self.host.parent.mkdir()
        self.host.write_text("#!/bin/sh\nexit 0\n")
        self.host.chmod(0o755)
        (self.host.parent / "build.json").write_text(json.dumps({
            "schema": native_host_record.SCHEMA, "source": "1" * 40,
            "source_evidence": "a stand-in written by the unit test; no aterm build exists",
            "attestation": native_host_record.CLEAN, "builder": "the unit test",
            "built_at": "2026-11-12T09:30:00-08:00",
            "host_sha256": hashlib.sha256(self.host.read_bytes()).hexdigest(),
            "host_bytes": self.host.stat().st_size, "platform": "stand-in",
            "shipping_probe": {"ran": True, "returncode": 0, "verdict": PASS, "seconds": 0.0,
                               "world": None}}))
        self.assertIsNone(native_host_record.not_qualifying(native_host_record.load(self.host)))

    def package(self, name, build, manifest=None):
        package = self.root / name
        (package / "bin").mkdir(parents=True)
        (package / "bin" / "codex").write_text("#!/bin/sh\nexit 9\n")
        if build is not None:
            (package / "alab-build.json").write_text(json.dumps(build))
        if manifest is None and isinstance(build, dict):
            manifest = {"version": build.get("version"), "target": build.get("target"),
                        "entrypoint": "bin/codex"}
        if manifest is not None:
            (package / "codex-package.json").write_text(json.dumps(manifest))
        return package

    def matrix(self, package):
        output = self.root / (package.name + "-matrix")
        completed = subprocess.run([sys.executable, "-B", str(SMOKE / "renewal_matrix.py"),
            "--host", str(self.host), "--package", str(package), "--output", str(output),
            "--case", "weekly-quota"], capture_output=True, text=True, timeout=60)
        return completed, output

    def test_a_package_that_qualifies_no_run_is_refused_before_any_case_runs(self):
        stand_in = {"version": "0.0.0-stand-in", "target": "stand-in", "source": "0" * 40}
        refused = {
            "trial": (trial_record(), None, "its record carries a `trial` object "
                      "(attestation 'unverified-development-build')"),
            "marked": (dict(certified(), trial={}), None, "its record carries a `trial` object"),
            "unadmitted": (changed(certified(), "compiler", "admitted", value=False), None,
                           "its compiler was not admitted"),
            "unknown": (stand_in, None, "its record names no compiler"),
            "unverified": (changed(certified("certified-package-sh.json"), "compiler",
                                   "verification", drop=True), None,
                           "its record carries no build verification"),
            "unhashable": (changed(certified(), "compiler", "sysroot",
                                   value="/opt/" + json.loads('"\\ud800"')), None,
                           "its record holds text that is not Unicode"),
            "a-list": ([certified()], {"version": "0.7.0", "target": "t", "entrypoint": "bin/codex"},
                       "its records do not load: TypeError"),
            "no-record": (None, {"version": "0.7.0", "target": "t", "entrypoint": "bin/codex"},
                          "its records do not load: RuntimeError: missing regular alab-build.json"),
            "disagreeing": (certified(), {"version": "0.7.1", "target": certified()["target"],
                                          "entrypoint": "bin/codex"},
                            "its records do not load: RuntimeError: package versions disagree"),
        }
        for name, (build, manifest, reason) in refused.items():
            with self.subTest(name=name):
                completed, output = self.matrix(self.package(name, build, manifest))
                self.assertEqual(completed.returncode, 1)
                self.assertNotIn("Traceback", completed.stderr)
                written = json.loads((output / "qualification.json").read_text())
                self.assertEqual((written["passed"], written["cases"]), (False, []))
                self.assertTrue(written["error"].startswith(
                    "the package's own record qualifies no run: " + reason), written["error"])
                self.assertTrue(written["error"].endswith(
                    "; run a case singly with renewal_follow.py for trial evidence"))
                self.assertEqual(completed.stderr.strip(), written["error"])
                # Nothing was hashed and no case ran.
                self.assertEqual(sorted(path.name for path in output.iterdir()),
                                 ["qualification.json"])

    def test_a_trial_host_is_still_refused_first_and_by_its_own_reason(self):
        record = json.loads((self.host.parent / "build.json").read_text())
        record.update(attestation="trial", builder=None, built_at=None)
        (self.host.parent / "build.json").write_text(json.dumps(record))
        completed, output = self.matrix(self.package("trial", trial_record()))
        self.assertEqual(completed.returncode, 1)
        error = json.loads((output / "qualification.json").read_text())["error"]
        self.assertTrue(error.startswith("the native host record qualifies no run: "), error)


if __name__ == "__main__":
    unittest.main()

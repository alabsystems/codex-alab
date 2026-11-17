#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Source-verdict transport cannot turn setup/status collisions into attribution."""

import os
import shlex
import sys
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class CheckVerdictProtocolTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="alab-check-verdict-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.target = self.root / "target"
        self.target.mkdir()
        self.pristine = self.root / "pristine"
        (self.pristine / "codex-rs/target").mkdir(parents=True)
        self.env = dict(os.environ, ALAB_PYTHON=sys.executable, PATH="/usr/bin:/bin:/usr/sbin:/sbin",
                        FIXTURE_TARGET=str(self.target), FIXTURE_PRISTINE=str(self.pristine),
                        ALAB_FEATURE_BUNDLE=str(self.root / "absent-feature-bundle"),
                        ATPKG_PROJECT_FRONT="/usr/bin/true",
                        ATPKG_PROJECT_EXECUTION=str(self.root))

    def shell(self, body, *args, **env):
        return subprocess.run(
            ["/bin/bash", "-c", 'set -uo pipefail; . "$1/alab/check.sh"; shift; ' + body,
             "check-verdict-fixture", str(ROOT), *map(str, args)],
            env=dict(self.env, **{key: str(value) for key, value in env.items()}),
            capture_output=True, text=True, timeout=30)

    @staticmethod
    def check_main_function(name):
        source = (ROOT / "alab/check.sh").read_text()
        return name + "() {" + source.split(name + "() {", 1)[1].split("\n}\n", 1)[0] + "\n}\n"

    def test_inner_verdict_uses_only_the_fixed_private_descriptor(self):
        function = self.check_main_function("check_source_verdict")
        direct = self.shell(function + 'own_target="$FIXTURE_TARGET"; check_source_verdict alab-fault 80')
        self.assertEqual(direct.returncode, 2, direct.stderr)
        record = self.root / "private-record"
        private = self.shell(function +
            'exec 9>"$1"; ALAB_CHECK_VERDICT_FD=9; '
            'check_source_verdict lane-fault 81', record)
        self.assertEqual(private.returncode, 81, private.stderr)
        self.assertEqual(record.read_text(), "lane-fault 81\n")
        refused = self.shell(function + 'ALAB_CHECK_VERDICT_FD=7; '
            'check_source_verdict lane-fault 81')
        self.assertEqual(refused.returncode, 1, refused.stderr)

    def selected(self, status, inner="none", occupied=""):
        control = self.root / f"outer-control-{status}-{inner}"
        observed = self.root / f"outer-observed-{status}-{inner}"
        victim = self.root / f"occupied-{occupied or 'none'}"
        victim.write_text("preserve\n")
        body = r'''
exec 18<"$1"
exec 19>"$1"
rm -f "$1"
case "$FIXTURE_OCCUPIED" in
  8) exec 8<>"$3" ;; 11) exec 11<>"$3" ;; 13) exec 13<>"$3" ;;
esac
alab_project_selected() { :; }
git() {
  case "$*" in
    *--git-common-dir*) printf '%s\n' "$FIXTURE_COMMON" ;;
    *) printf '%040d\n' 0 ;;
  esac
}
alab_pristine() { printf '%s\n' "$FIXTURE_PRISTINE"; }
alab_pristine_at() { :; }
alab_check_targets_ready() { printf '%s\n' "$FIXTURE_TARGET"; }
alab_real() { printf '%s\n' "$1"; }
alab_verified_contained() {
  for fd in 8 9 10 11 12 13; do [[ -e "/dev/fd/$fd" ]] || return 96; done
  [[ "$*" != *alab-check-control* && -z "${ALAB_CHECK_VERDICT_FD:-}" \
    && -z "${ALAB_CHECK_RESULT_FD:-}" ]] || return 97
  case "$FIXTURE_INNER" in
    alab) printf 'alab-fault 80\n' >&9 ;;
    lane) printf 'lane-fault 81\n' >&9 ;;
    extra) printf 'lane-fault 81\nforged\n' >&9 ;;
    blank) printf 'lane-fault 81\n\n' >&9 ;;
  esac
  return "$FIXTURE_STATUS"
}
selected_status=0
ALAB_CHECK_OUTCOME_FD=19
check_selected || selected_status=$?
case "$FIXTURE_OCCUPIED" in 8) exec 8>&- ;; 11) exec 11>&- ;; 13) exec 13>&- ;; esac
exec 19>&-
cat <&18 >"$2"
exec 18>&-
exit "$selected_status"
'''
        control.touch()
        done = self.shell(body, control, observed, victim, FIXTURE_STATUS=status,
                          FIXTURE_INNER=inner, FIXTURE_OCCUPIED=occupied,
                          FIXTURE_COMMON=self.root)
        self.assertEqual(list(self.root.glob("alab-check-control.*")), [])
        self.assertEqual(victim.read_text(), "preserve\n")
        outer = observed.read_text() if observed.exists() else ""
        observed.unlink(missing_ok=True)
        return done, outer

    def test_selected_transport_requires_matching_private_status_and_exact_inner_bytes(self):
        for status, inner, expected, outer in (
                (80, "alab", 82, "alab-fault 82\n"),
                (81, "lane", 83, "lane-fault 83\n"),
                (80, "none", 80, ""),
                (81, "alab", 81, ""),
                (81, "extra", 81, ""),
                (81, "blank", 81, ""),
                (0, "alab", 1, ""),
                (75, "lane", 75, ""),
                (130, "lane", 130, "")):
            with self.subTest(status=status, inner=inner):
                done, observed = self.selected(status, inner)
                self.assertEqual(done.returncode, expected, done.stderr)
                self.assertEqual(observed, outer)

    def entry(self, status, verdict="", python=sys.executable):
        body = r'''
alab_python_311() { printf '%s\n' "$FIXTURE_PYTHON"; }
git() { printf '%s\n' "$FIXTURE_COMMON"; }
alab_project_run() {
  [[ -z "$FIXTURE_VERDICT" ]] || printf '%s' "$FIXTURE_VERDICT" >&19
  return "$FIXTURE_STATUS"
}
check_entry
'''
        done = self.shell(body, FIXTURE_COMMON=self.root, FIXTURE_STATUS=status,
                          FIXTURE_VERDICT=verdict, FIXTURE_PYTHON=python)
        self.assertEqual(list(self.root.glob("alab-check-control.*")), [])
        return done

    def test_public_entry_requires_matching_outer_status_and_prioritizes_stops(self):
        for status, verdict, expected in (
                (82, "alab-fault 82\n", 2), (83, "lane-fault 83\n", 6),
                (82, "lane-fault 83\n", 1), (83, "lane-fault 83\nextra\n", 1),
                (83, "lane-fault 83\n\n", 1),
                (75, "lane-fault 83\n", 75), (130, "alab-fault 82\n", 130),
                (80, "", 1), (81, "", 1), (82, "", 1), (83, "", 1),
                (37, "", 37)):
            with self.subTest(status=status, verdict=verdict):
                done = self.entry(status, verdict)
                self.assertEqual(done.returncode, expected, done.stderr)

    def test_public_entry_reads_through_short_reads_before_authenticating(self):
        for width in (3, 14):
            with self.subTest(width=width):
                payload = self.root / f"short-read-python-{width}.py"
                payload.write_text("""import os
import sys

if sys.argv[1:4] != ["-I", "-B", "-c"]:
    raise SystemExit(97)
script = sys.argv[4]
arguments = sys.argv[5:]
original_read = os.read

def short_read(descriptor, size):
    return original_read(descriptor, min(size, READ_WIDTH))

os.read = short_read
sys.argv = ["-c", *arguments]
exec(compile(script, "<short-read>", "exec"), {"__name__": "__main__"})
""".replace("READ_WIDTH", str(width)))
                short_reader = self.root / f"short-read-python-{width}"
                short_reader.write_text("#!/bin/sh\nexec " + shlex.join(
                    [sys.executable, "-I", "-B", str(payload)]) + ' "$@"\n')
                short_reader.chmod(0o755)
                valid = self.entry(82, "alab-fault 82\n", short_reader)
                self.assertEqual(valid.returncode, 2, valid.stderr)
                forged = self.entry(82, "alab-fault 82\nforged\n", short_reader)
                self.assertEqual(forged.returncode, 1, forged.stderr)

    def test_fixed_control_descriptor_collisions_refuse_without_replacement(self):
        for descriptor in (8, 11, 13):
            with self.subTest(descriptor=descriptor):
                done, observed = self.selected(80, "alab", descriptor)
                self.assertEqual(done.returncode, 1, done.stderr)
                self.assertEqual(observed, "")
        for descriptor in (18, 19):
            with self.subTest(outer=descriptor):
                victim = self.root / f"outer-victim-{descriptor}"
                victim.write_text("preserve\n")
                done = self.shell(
                    f'exec {descriptor}<>"$1"; '
                    'alab_python_311() { printf "%s\\n" "$ALAB_PYTHON"; }; '
                    'git() { printf "%s\\n" "$FIXTURE_COMMON"; }; check_entry',
                    victim, FIXTURE_COMMON=self.root)
                self.assertEqual(done.returncode, 1, done.stderr)
                self.assertEqual(victim.read_text(), "preserve\n")

        # Refusal must retain the caller's open file description, not just bytes.
        import json

        probe = """import fcntl, json, os, sys
fd, phase = int(sys.argv[1]), sys.argv[2]
try:
    state = os.fstat(fd)
    witness = os.fstat(43)
    initial = os.lseek(fd, 0, os.SEEK_CUR)
    if phase == "before":
        os.lseek(fd, len(b"prefix\\n"), os.SEEK_SET)
    record = {"device": state.st_dev, "inode": state.st_ino,
              "offset": os.lseek(fd, 0, os.SEEK_CUR),
              "access": fcntl.fcntl(fd, fcntl.F_GETFL) & os.O_ACCMODE,
              "witness": {"device": witness.st_dev, "inode": witness.st_ino,
                          "offset": os.lseek(43, 0, os.SEEK_CUR),
                          "access": fcntl.fcntl(43, fcntl.F_GETFL) & os.O_ACCMODE}}
    if phase == "before":
        record["initial_offset"] = initial
    else:
        record["next"] = os.read(fd, 4096).hex()
        record["witness_after_read"] = os.lseek(43, 0, os.SEEK_CUR)
except OSError as error:
    record = {"errno": error.errno}
print(json.dumps(record))
"""
        for descriptor in (8, 9, 10, 11, 12, 13, 18, 19):
            with self.subTest(custody=descriptor):
                victim = self.root / f"custody-victim-{descriptor}"
                payload = b"prefix\nnext marker\n"
                victim.write_bytes(payload)
                original = victim.stat()
                before = self.root / f"custody-before-{descriptor}.json"
                after = self.root / f"custody-after-{descriptor}.json"
                operation = "check_selected" if descriptor < 18 else "check_entry"
                body = r"""
unset ALAB_CHECK_OUTCOME_FD ALAB_CHECK_VERDICT_FD ALAB_CHECK_RESULT_FD
alab_python_311() { printf '%s\n' "$ALAB_PYTHON"; }
alab_project_selected() { :; }
git() {
  case "$*" in
    *--git-common-dir*) printf '%s\n' "$FIXTURE_COMMON" ;;
    *) printf '%040d\n' 0 ;;
  esac
}
alab_pristine() { printf '%s\n' "$FIXTURE_PRISTINE"; }
alab_pristine_at() { :; }
alab_check_targets_ready() { printf '%s\n' "$FIXTURE_TARGET"; }
alab_real() { printf '%s\n' "$1"; }
alab_verified_contained() { return 98; }
alab_project_run() { return 97; }
"""
                body += (
                    f'exec {descriptor}<>"$1"\n'
                    f'exec 43<&{descriptor}\n'
                    f'"$ALAB_PYTHON" -I -B -c "$FIXTURE_PROBE" {descriptor} before >"$2"\n'
                    'status=0\n' + operation + ' || status=$?\n'
                    f'"$ALAB_PYTHON" -I -B -c "$FIXTURE_PROBE" {descriptor} after >"$3"\n'
                    f'exec {descriptor}>&-\n'
                    'exec 43>&-\n'
                    'exit "$status"\n')
                done = self.shell(body, victim, before, after,
                                  FIXTURE_COMMON=self.root, FIXTURE_PROBE=probe,
                                  FIXTURE_CUSTODY_VICTIM=victim)
                self.assertEqual(done.returncode, 1, done.stderr)
                initial = json.loads(before.read_text())
                retained = json.loads(after.read_text())
                expected = {"device": original.st_dev, "inode": original.st_ino,
                            "offset": len(b"prefix\n"), "access": os.O_RDWR}
                expected["witness"] = dict(expected)
                self.assertEqual(initial, dict(expected, initial_offset=0),
                                 "owned descriptor precondition failed")
                self.assertEqual(retained, dict(expected, next=b"next marker\n".hex(),
                                                witness_after_read=len(payload)),
                                 "refusal changed the caller's descriptor or offset")
                self.assertEqual(victim.read_bytes(), payload)
                self.assertEqual((victim.stat().st_dev, victim.stat().st_ino),
                                 (original.st_dev, original.st_ino))
                self.assertEqual(list(self.root.glob("alab-check-control.*")), [])


if __name__ == "__main__":
    unittest.main()

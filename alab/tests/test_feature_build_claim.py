#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise the reviewed dispatcher with a private host claimant and candidate."""

import json
import os
from pathlib import Path
import shlex
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import unittest


ALAB = Path(__file__).resolve().parents[1]
MARKER = "fixture-private-admission-marker"


def native_claim(arguments):
    import fcntl

    if arguments[:2] != ["pkg", "claim"]:
        raise ValueError("expected the configured package claimant")
    if arguments[2:] == ["--capabilities"]:
        status = int(os.environ["CAPABILITY_STATUS"])
        if status == 128 + signal.SIGTERM:
            os.kill(os.getpid(), signal.SIGTERM)
        if status:
            return status
        return print("aterm-claim-exec-v1")
    split = arguments.index("--", 2)
    fields = dict(zip(arguments[2:split:2], arguments[3:split:2], strict=True))
    fd = int(fields["--status-fd"])
    mode = os.fstat(fd).st_mode
    access = fcntl.fcntl(fd, fcntl.F_GETFL) & os.O_ACCMODE
    if not ((stat.S_ISFIFO(mode) and access == os.O_WRONLY) or
            (stat.S_ISSOCK(mode) and access == os.O_RDWR)):
        raise ValueError("claim status requires a private pipe or socket")
    with os.scandir("/dev/fd") as descriptors:
        status_handles = sum(os.path.sameopenfile(fd, int(entry.name)) for entry in descriptors)
    with open(os.environ["CALLS"], "a") as stream:
        print(json.dumps({"claim": fields, "status_handles": status_handles}), file=stream)
    outcome = os.environ["NATIVE_OUTCOME"]
    # Native command admission can reuse a CPU-only cover only when its
    # marker is inherited. Unmarked ancestry covers positive-memory work.
    if outcome == "cpu-only":
        outcome = "covered" if os.environ.get("ATERM_CAPACITY_COVER") == MARKER else "refused"
    if outcome == "covered" and os.environ.get("ATERM_CAPACITY_COVER") == "none":
        outcome = "refused"
    if outcome == "refused":
        return 75
    jobs = int(fields["--cores"])
    if outcome == "covered":
        jobs = min(jobs, 1)
    status = {"v": 1, "event": outcome, "cores": jobs, "marker": MARKER}
    if outcome == "admitted":
        status.update(rank=fields["--rank"], mem_bytes=16 * 1024**3)
    with os.fdopen(fd, "w") as pipe:
        print(json.dumps(status), file=pipe)
    command = arguments[split + 1:]
    os.execvpe(command[0], command, dict(os.environ, CARGO_BUILD_JOBS=str(jobs),
                                         ATERM_CAPACITY_COVER=MARKER))


@unittest.skipUnless(Path("/bin/bash").is_file(), "feature dispatch requires /bin/bash")
class FeatureBuildClaim(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory())).resolve()
        self.repo = self.root / "candidate tree"
        self.calls = self.root / "calls"
        (self.repo / "codex-rs").mkdir(parents=True)
        recipes = self.root / "collector" / "alab" / "feature_recipes"
        shutil.copytree(ALAB / "feature_recipes", recipes)
        self.runner = recipes / "run.sh"
        # Retain the collector's native resolver and dispatcher. The last
        # boundary records containment and runs the selected candidate script.
        collector = (ALAB / "lib.sh").read_text() + '''
alab_project_selected() { export ALAB_CARGO=/unused ATPKG_PROJECT_FRONT=/front ATPKG_PROJECT_EXECUTION=/execution; }
alab_python_311() { echo "$TEST_PYTHON"; }
alab_build_target_ready() { echo "$2"; }
alab_build_command() { printf '{"python_admission":"%s"}\n' "${ATERM_ADMIT-unset}" >> "$CALLS"; }
alab_contained() {
    echo '"contained"' >> "$CALLS"
    while [[ "$1" != -- ]]; do shift; done
    shift
    unset ATERM_CAPACITY_COVER
    "$@"
}
'''
        (recipes.parent / "lib.sh").write_text(collector)
        candidate = self.repo / "alab"
        candidate.mkdir()
        (candidate / "lib.sh").write_text('alab_project_selected() { export ALAB_CARGO=/unused; }\n')
        (candidate / "unit-tests.sh").touch()
        (candidate / "focused-tests.sh").write_text('''
alab_run_focused() {
    printf '{"candidate_jobs":"%s","admission":"%s","test_threads":"%s"}\n' \
        "${CARGO_BUILD_JOBS:-}" "${ATERM_ADMIT-unset}" "${RUST_TEST_THREADS:-}" >> "$CALLS"
    return "$CANDIDATE_EXIT"
}
''')
        self.aterm = self.root / "host claimant"
        self.aterm.write_text("#!/bin/bash\nexec " + shlex.join(
            [sys.executable, "-I", "-B", str(Path(__file__).resolve()), "native-claim"]) + ' "$@"\n')
        self.aterm.chmod(0o700)

    def dispatch(self, *, stage="queue-review", jobs=None, threads=None, outcome="admitted",
                 candidate_exit=0, capability_status=0, marker=None, hard_nofile=None):
        self.calls.write_text("")
        attempt = Path(tempfile.mkdtemp(dir=self.root))
        out = attempt / "artifacts"
        out.mkdir()
        environment = {"PATH": os.defpath, "HOME": str(self.root), "TMPDIR": str(self.root),
                       "ALAB_ATERM": str(self.aterm), "CALLS": str(self.calls), "TEST_PYTHON": sys.executable,
                       "NATIVE_OUTCOME": outcome, "CANDIDATE_EXIT": str(candidate_exit),
                       "CAPABILITY_STATUS": str(capability_status)}
        if jobs is not None:
            environment["CARGO_BUILD_JOBS"] = str(jobs)
        if threads is not None:
            environment["RUST_TEST_THREADS"] = str(threads)
        if marker is not None:
            environment["ATERM_CAPACITY_COVER"] = marker
        command = ["/bin/bash", str(self.runner), "selected", str(self.repo),
                   "a" * 40, str(out), stage, "fixture_test"]
        if hard_nofile is not None:
            command = ["/bin/bash", "-c", 'ulimit -Sn 64 && ulimit -Hn "$1" && shift && exec "$@"',
                       "fixture-nofile", str(hard_nofile), *command]
        result = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=30)
        self.assertNotIn(MARKER, result.stdout + result.stderr)
        for record in attempt.rglob("*"):
            if record.is_file():
                self.assertNotIn(MARKER.encode(), record.read_bytes(), record)
        return result, [json.loads(line) for line in self.calls.read_text().splitlines()]

    def test_host_admits_before_containment_and_honors_the_build_width(self):
        host = os.cpu_count() or 1
        for requested, expected, threads in ((None, 4, None), (2, 2, 1),
                                             ("default", host, None), (-1, max(1, host - 1), None)):
            with self.subTest(requested=requested, threads=threads):
                result, calls = self.dispatch(jobs=requested, threads=threads)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(len(calls), 3, calls)
                claim = calls[0]["claim"]
                self.assertEqual(claim["--cores"], str(expected))
                self.assertEqual(claim["--rank"], "ordinary")
                self.assertEqual(Path(claim["--checkout"]), self.repo)
                self.assertGreater(int(claim["--wait-ms"]), 0)
                self.assertLessEqual(int(claim["--wait-ms"]), 60000)
                self.assertNotIn("--mem-bytes", claim)
                self.assertEqual(calls[1:], ["contained", {"candidate_jobs": str(expected), "admission": "unset",
                                                           "test_threads": str(min(threads or 4, expected))}])

    def test_native_candidate_can_open_more_files_only_within_the_granted_hard_limit(self):
        probe = self.root / "descriptor_probe.py"
        probe.write_text('''import errno, json, os, resource
soft, hard = resource.getrlimit(resource.RLIMIT_NOFILE)
try:
    for _ in range(120):
        os.open(os.devnull, os.O_RDONLY)
except OSError as err:
    if err.errno != errno.EMFILE:
        raise
    opened = False
else:
    opened = True
print(json.dumps([soft, hard, opened]))
''')
        (self.repo / "alab/focused-tests.sh").write_text(
            'alab_run_focused() { "$TEST_PYTHON" -IB ' + shlex.quote(str(probe)) + '; }\n')
        for hard, opened in ((192, True), (96, False)):
            with self.subTest(hard=hard):
                result, calls = self.dispatch(hard_nofile=hard)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("claim", calls[0])
                self.assertEqual(calls[-1], "contained")
                self.assertEqual(json.loads(result.stdout), [hard, hard, opened])

    def test_refused_claim_returns_75_before_candidate_work(self):
        result, calls = self.dispatch(outcome="refused")
        self.assertEqual(result.returncode, 75, result.stderr)
        self.assertEqual(len(calls), 1, calls)
        self.assertIn("claim", calls[0])

    def test_claimant_receives_only_the_original_status_descriptor(self):
        for outcome in ("admitted", "covered"):
            with self.subTest(outcome=outcome):
                result, calls = self.dispatch(outcome=outcome)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(calls[0]["status_handles"], 1)

    def test_ancestor_cover_limits_workers_and_preserves_the_candidate_exit(self):
        for status in (23, 75):
            with self.subTest(status=status):
                result, calls = self.dispatch(outcome="covered", jobs=3, threads=3, candidate_exit=status,
                                              marker=MARKER)
                self.assertEqual(result.returncode, status, result.stderr)
                self.assertEqual(calls[-1], {"candidate_jobs": "1", "admission": "unset", "test_threads": "1"})

    def test_cpu_only_ancestor_and_explicit_no_cover_require_separate_admission(self):
        for outcome, marker in (("cpu-only", MARKER), ("covered", "none")):
            with self.subTest(outcome=outcome):
                result, calls = self.dispatch(outcome=outcome, marker=marker)
                self.assertEqual(result.returncode, 75, result.stderr)
                self.assertEqual(len(calls), 1, calls)
                self.assertIn("claim", calls[0])

    def test_libtest_positive_integers_are_capped_to_the_granted_width(self):
        for threads in ("+3", "03", "9223372036854775808"):
            with self.subTest(threads=threads):
                result, calls = self.dispatch(outcome="covered", jobs=3, threads=threads)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(calls[-1]["test_threads"], "1")

    def test_refused_or_interrupted_capability_query_stops_before_containment(self):
        for status in (75, 143):
            with self.subTest(status=status):
                result, calls = self.dispatch(capability_status=status)
                self.assertEqual(result.returncode, status, result.stderr)
                self.assertEqual(calls, [])

    def test_older_package_manager_preserves_per_build_admission(self):
        result, calls = self.dispatch(capability_status=64, jobs=3)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls, ["contained", {"candidate_jobs": "3", "admission": "unset", "test_threads": "3"}])

    def test_python_stage_runs_when_build_capacity_is_unavailable(self):
        result, calls = self.dispatch(stage="qualification", outcome="refused")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls, [{"python_admission": "unset"}])


if __name__ == "__main__":
    if sys.argv[1:2] == ["native-claim"]:
        sys.exit(native_claim(sys.argv[2:]))
    unittest.main()

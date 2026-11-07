#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Source identity, normal checkout reuse, and conservative source custody."""

import fcntl
import json
import os
from pathlib import Path
import runpy
import shlex
import signal
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ENTRY = "#!/bin/sh\n'''exec' " + shlex.quote(sys.executable) + ' -I -B "$0" "$@"\n\' \'\'\'\n'
SOURCE = runpy.run_path(str(ROOT / "alab/package_source.py"))
GUARD = runpy.run_path(str(ROOT / "alab/command_guard.py"))
COMPILER = "0123456789abcdef"
TARGET = "aarch64-apple-darwin"


class PackageSourceTests(unittest.TestCase):
    def setUp(self):
        parent = os.environ.get("ALAB_SOURCE_FIXTURE_ROOT")
        self.root = Path(tempfile.mkdtemp(prefix="package-source-", dir=parent)).resolve()
        print(f"retained package source fixture: {self.root}", file=sys.stderr)
        self.cache = self.root / "private cache"
        self.cache.mkdir(mode=0o700)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        self.home = self.root / "home"
        self.home.mkdir()
        self.git("init", "--quiet")
        self.git("config", "user.name", "Source fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        (self.repo / "Cargo.toml").write_text('[package]\nname = "source-fixture"\nversion = "0.0.0"\nedition = "2021"\n[workspace]\n')
        (self.repo / "src").mkdir()
        (self.repo / "src/lib.rs").write_text("pub fn unchanged() -> u32 { 7 }\n")
        (self.repo / "changing").write_text("first\n")
        (self.repo / ".gitignore").write_text("ignored\n")
        self.first = self.commit("first")
        (self.repo / "changing").write_text("second\n")
        self.second = self.commit("second")

    def git(self, *args, repo=None):
        done = subprocess.run(["git", "-C", str(repo or self.repo), "-c", "commit.gpgsign=false", *args],
                              capture_output=True, text=True, timeout=60)
        self.assertEqual(done.returncode, 0, done.stderr)
        return done.stdout.strip()

    def commit(self, message):
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", message)
        return self.git("rev-parse", "HEAD")

    def session(self):
        return SOURCE["Session"](self.cache, self.repo)

    def acquire(self, session, commit=None):
        with mock.patch.dict(os.environ, session.environment()):
            result = SOURCE["acquire"](self.cache, self.repo, commit or self.first, COMPILER, TARGET)
        return Path(result["path"]), result

    def returned(self, session, source, commit=None):
        with mock.patch.dict(os.environ, session.environment()):
            SOURCE["release"](self.cache, source, commit or self.first)

    def receipt(self, source):
        return source.with_suffix(".json")

    def ready(self):
        session = self.session()
        source, _ = self.acquire(session)
        self.returned(session, source)
        session.finish()
        return source

    def test_clean_commits_reuse_identity_path_and_unchanged_mtime(self):
        first = self.session()
        source, result = self.acquire(first)
        self.assertFalse(result["reused"])
        unchanged = source / "src/lib.rs"
        os.utime(unchanged, ns=(1_600_000_000_000_000_000,) * 2)
        before = (source.stat().st_ino, unchanged.stat().st_ino, unchanged.stat().st_mtime_ns)
        self.returned(first, source)
        requested = SOURCE["read"](self.receipt(source))
        self.assertEqual(requested["phase"], "active", "worker return alone never publishes Ready")
        self.assertTrue(requested["returned"])
        first.finish()
        second = self.session()
        reused, result = self.acquire(second, self.second)
        self.assertTrue(result["reused"])
        self.assertEqual(reused, source)
        self.assertEqual(before, (source.stat().st_ino, unchanged.stat().st_ino, unchanged.stat().st_mtime_ns))
        self.assertEqual((source / "changing").read_text(), "second\n")
        self.returned(second, source, self.second)
        second.finish()
        self.assertEqual(SOURCE["read"](self.receipt(source))["phase"], "ready")

    @unittest.skipUnless(os.environ.get("ALAB_PACKAGE_SOURCE_NATIVE") == "1",
                         "set ALAB_PACKAGE_SOURCE_NATIVE=1 for the real Trust cache fixture")
    def test_native_external_path_dependency_reuses_build_units_across_commits(self):
        cargo = shutil.which("targo")
        compiler = shutil.which("trustc")
        self.assertIsNotNone(cargo, "the native fixture requires Trust targo")
        self.assertIsNotNone(compiler, "the native fixture requires trustc")
        compiler_info = subprocess.run([compiler, "-vV"], capture_output=True, text=True, timeout=60)
        self.assertEqual(compiler_info.returncode, 0, compiler_info.stderr)
        self.assertIn("binary: trustc", compiler_info.stdout)
        workspace = self.repo / "codex-rs"
        (workspace / "app/src").mkdir(parents=True)
        (workspace / "Cargo.toml").write_text('[workspace]\nmembers = ["app"]\nresolver = "2"\n')
        (workspace / "app/Cargo.toml").write_text(
            '[package]\nname = "fixture-app"\nversion = "0.0.0"\nedition = "2021"\n'
            '[dependencies]\nfixture-vendor = { path = "../../vendored/fixture-vendor" }\n')
        (workspace / "app/src/main.rs").write_text('fn main() { println!("{}", fixture_vendor::value()); }\n')
        vendor = self.repo / "vendored/fixture-vendor"
        (vendor / "src").mkdir(parents=True)
        (vendor / "Cargo.toml").write_text(
            '[package]\nname = "fixture-vendor"\nversion = "0.0.0"\nedition = "2021"\n[workspace]\n')
        (vendor / "src/lib.rs").write_text('pub fn value() -> u32 { 13 }\n')
        (workspace / "Cargo.lock").write_text(
            'version = 4\n\n[[package]]\nname = "fixture-app"\nversion = "0.0.0"\n'
            'dependencies = ["fixture-vendor"]\n\n[[package]]\nname = "fixture-vendor"\nversion = "0.0.0"\n')
        first_commit = self.commit("native path dependency fixture")
        (self.repo / "changing").write_text("change outside compilation inputs\n")
        second_commit = self.commit("unchanged compilation inputs in a new source commit")
        observations = {"compiler": compiler_info.stdout, "commits": [first_commit, second_commit], "builds": {}}

        def build(source, target, label):
            target.mkdir(mode=0o700, exist_ok=True)
            started = time.monotonic()
            done = subprocess.run([cargo, "--unverified", "build", "--locked", "--offline",
                                   "--manifest-path", str(source / "codex-rs/Cargo.toml"),
                                   "--target-dir", str(target), "--message-format=json", "-j", "1"],
                                  cwd=source / "codex-rs", env={**os.environ, "CARGO_INCREMENTAL": "0"},
                                  capture_output=True, text=True, timeout=60)
            elapsed = time.monotonic() - started
            (self.root / f"{label}.jsonl").write_text(done.stdout)
            (self.root / f"{label}.stderr").write_text(done.stderr)
            self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
            messages = [json.loads(line) for line in done.stdout.splitlines() if line.startswith("{")]
            artifacts = [message for message in messages if message.get("reason") == "compiler-artifact"]
            fresh = {message["target"]["name"]: message["fresh"] for message in artifacts}
            self.assertEqual(set(fresh), {"fixture-app", "fixture_vendor"})
            executable, = [message["executable"] for message in artifacts if message.get("executable")]
            proof = subprocess.run([executable], capture_output=True, text=True, timeout=60)
            self.assertEqual((proof.returncode, proof.stdout), (0, "13\n"), proof.stderr)
            allocated = subprocess.run(["du", "-sk", str(target)], capture_output=True, text=True, timeout=60)
            self.assertEqual(allocated.returncode, 0, allocated.stderr)
            observations["builds"][label] = {"source": str(source), "target": str(target), "fresh": fresh,
                                               "allocated_bytes": int(allocated.stdout.split()[0]) * 1024,
                                               "elapsed_seconds": elapsed, "execution_proof": proof.stdout}
            return fresh

        stable_session = self.session()
        stable, _ = self.acquire(stable_session, first_commit)
        inputs = ("codex-rs/app/Cargo.toml", "codex-rs/app/src/main.rs", "vendored/fixture-vendor/Cargo.toml",
                  "vendored/fixture-vendor/src/lib.rs")
        before = {name: [getattr((stable / name).stat(), field) for field in ("st_dev", "st_ino", "st_mtime_ns")]
                  for name in inputs}
        stable_target = self.root / "stable-target.noindex"
        self.assertTrue(all(not value for value in build(stable, stable_target, "stable-first").values()))
        self.returned(stable_session, stable, first_commit)
        stable_session.finish()
        next_session = self.session()
        reused, state = self.acquire(next_session, second_commit)
        self.assertTrue(state["reused"])
        self.assertEqual(reused, stable)
        after = {name: [getattr((stable / name).stat(), field) for field in ("st_dev", "st_ino", "st_mtime_ns")]
                 for name in inputs}
        self.assertEqual(before, after)
        self.assertTrue(all(build(reused, stable_target, "stable-second").values()))
        self.returned(next_session, reused, second_commit)
        next_session.finish()
        control_target = self.root / "fresh-target.noindex"
        for label, commit in (("fresh-first", first_commit), ("fresh-second", second_commit)):
            fresh = self.root / label
            self.git("worktree", "add", "--quiet", "--detach", str(fresh), commit)
            self.assertTrue(all(not value for value in build(fresh, control_target, label).values()))
        observations["unchanged_inputs_before"] = before
        observations["unchanged_inputs_after"] = after
        (self.root / "native-cache-measurement.json").write_text(json.dumps(observations, indent=2) + "\n")
        print(f"native cache measurement: {self.root / 'native-cache-measurement.json'}", file=sys.stderr)

    def test_dirty_replaced_unknown_and_crashed_generations_stay(self):
        for case in ("tracked", "untracked", "ignored", "head", "source-replaced", "source-link",
                     "entry-replaced", "git-locked", "git-metadata", "git-lock", "git-ref",
                     "assume-unchanged", "skip-worktree", "receipt", "active"):
            with self.subTest(case=case):
                # Each case has its own pool; no previous generation can hide the refusal.
                prior_cache = self.cache
                self.cache = self.root / case
                self.cache.mkdir(mode=0o700)
                session = self.session()
                source, _ = self.acquire(session)
                if case != "active":
                    self.returned(session, source)
                    session.finish()
                receipt = self.receipt(source)
                original = SOURCE["read"](receipt)
                away = self.root / f"away-{case}"
                if case == "tracked":
                    (source / "Cargo.toml").write_bytes(b"owner's manifest change\n")
                elif case in ("untracked", "ignored"):
                    (source / ("notes" if case == "untracked" else "ignored")).write_bytes(b"owner work\n")
                elif case == "head":
                    self.git("commit", "--allow-empty", "--quiet", "-m", "unpublished", repo=source)
                elif case in ("source-replaced", "source-link"):
                    source.rename(away)
                    if case == "source-link":
                        source.symlink_to(away, target_is_directory=True)
                    else:
                        source.mkdir()
                        (source / "notes").write_bytes(b"replacement work\n")
                elif case == "entry-replaced":
                    entry = Path(original["entry"])
                    entry.rename(away)
                    entry.mkdir()
                    (entry / "notes").write_bytes(b"replacement Git data\n")
                elif case == "receipt":
                    receipt.write_bytes(b"unknown owner receipt\n")
                elif case == "git-locked":
                    self.git("worktree", "lock", "--reason", "owner keeps source", str(source))
                elif case in ("git-metadata", "git-lock"):
                    (Path(original["entry"]) / ("notes" if case == "git-metadata" else "HEAD.lock")).write_text("owner Git data\n")
                elif case == "git-ref":
                    self.git("commit", "--allow-empty", "--quiet", "-m", "unpublished owner object", repo=source)
                    new_commit = self.git("rev-parse", "HEAD", repo=source)
                    self.git("update-ref", "refs/worktree/owner", new_commit, repo=source)
                    self.git("checkout", "--quiet", "--detach", self.first, repo=source)
                elif case in ("assume-unchanged", "skip-worktree"):
                    self.git("update-index", f"--{case}", "changing", repo=source)
                    (source / "changing").write_bytes(b"owner work hidden from status\n")
                before_receipt = receipt.read_bytes()
                next_session = self.session()
                fresh, result = self.acquire(next_session, self.second)
                self.assertFalse(result["reused"])
                self.assertNotEqual(fresh, source)
                self.assertEqual(receipt.read_bytes(), before_receipt)
                if away.exists() and case != "entry-replaced":
                    self.assertEqual((away / "changing").read_text(), "first\n")
                if case == "tracked":
                    self.assertEqual((source / "Cargo.toml").read_bytes(), b"owner's manifest change\n")
                if case in ("untracked", "ignored"):
                    self.assertEqual((source / ("notes" if case == "untracked" else "ignored")).read_bytes(), b"owner work\n")
                if case == "entry-replaced":
                    self.assertEqual((Path(original["entry"]) / "notes").read_bytes(), b"replacement Git data\n")
                if case in ("assume-unchanged", "skip-worktree"):
                    self.assertEqual((source / "changing").read_bytes(), b"owner work hidden from status\n")
                if case == "git-ref":
                    self.assertEqual(self.git("rev-parse", "refs/worktree/owner", repo=source), new_commit)
                    self.git("cat-file", "-e", new_commit, repo=source)
                if case in ("git-metadata", "git-lock"):
                    self.assertEqual((Path(original["entry"]) / ("notes" if case == "git-metadata" else "HEAD.lock")).read_text(), "owner Git data\n")
                self.cache = prior_cache

    def test_return_then_late_work_is_never_marked_ready(self):
        session = self.session()
        source, _ = self.acquire(session)
        self.returned(session, source)
        manifest = source / "Cargo.toml"
        manifest.write_bytes(b"owner changed the same input a legacy builder stamps\n")
        session.finish()
        self.assertEqual(SOURCE["read"](self.receipt(source))["phase"], "active")
        fresh, result = self.acquire(self.session(), self.second)
        self.assertFalse(result["reused"])
        self.assertNotEqual(fresh, source)
        self.assertEqual(manifest.read_bytes(), b"owner changed the same input a legacy builder stamps\n")

    def test_active_driver_source_is_preserved_and_nested_pool_refused(self):
        driver = self.ready()
        before = self.receipt(driver).read_bytes()
        session = self.session()
        with mock.patch.dict(os.environ, session.environment()):
            fresh = SOURCE["acquire"](self.cache, driver, self.second, COMPILER, TARGET)
        self.assertNotEqual(Path(fresh["path"]), driver)
        self.assertFalse(fresh["reused"])
        self.assertEqual(self.receipt(driver).read_bytes(), before)
        self.assertEqual(self.git("rev-parse", "HEAD", repo=driver), self.first)
        nested_cache = self.repo / "configured cache"
        nested_cache.mkdir(mode=0o700)
        (nested_cache / "owner-data").write_bytes(b"keep nested cache\n")
        self.cache = nested_cache
        with self.assertRaisesRegex(ValueError, "overlaps its active driver"):
            self.acquire(self.session())
        alias = self.root / "driver-alias"
        alias.symlink_to(self.repo, target_is_directory=True)
        with mock.patch.dict(os.environ, self.session().environment()):
            with self.assertRaisesRegex(ValueError, "overlaps its active driver"):
                SOURCE["acquire"](alias / "configured cache", alias, self.first, COMPILER, TARGET)
        self.assertFalse(list(nested_cache.rglob("source-*.noindex")))
        self.assertEqual((nested_cache / "owner-data").read_bytes(), b"keep nested cache\n")

    def test_full_pool_can_reuse_ready_but_cannot_grow(self):
        source = self.ready()
        receipt = self.receipt(source)
        # A small injected ceiling exercises the production admission boundary.
        for number, phase in enumerate(("active", "unknown")):
            SOURCE["write"](receipt.parent / f"source-{number:032x}.json",
                            {"schema": 1, "phase": phase})
        before = {p: p.read_bytes() for p in receipt.parent.glob("source-*.json") if p != receipt}
        with mock.patch.dict(SOURCE["acquire"].__globals__, LIMIT=3):
            reused, result = self.acquire(self.session(), self.second)
            self.assertTrue(result["reused"])
            self.assertEqual(reused, source)
            self.assertEqual(len(list(receipt.parent.glob("source-*.json"))), 3)
            with self.assertRaisesRegex(ValueError, "pool is full"):
                self.acquire(self.session(), self.second)
        self.assertEqual(before, {p: p.read_bytes() for p in before})

    def test_late_inputs_during_checkout_are_preserved(self):
        for ignored in (False, True):
            with self.subTest(ignored=ignored):
                self.cache = self.root / f"late-{ignored}"
                self.cache.mkdir(mode=0o700)
                source = self.ready()
                target = self.second
                if ignored:
                    (self.repo / ".gitignore").write_text("")
                    (self.repo / "ignored").write_text("new committed content\n")
                    target = self.commit("new tracked path")
                original_git = SOURCE["acquire"].__globals__["git"]
                def late_git(repo, *args, **kwargs):
                    if Path(repo) == source and args[0] == "checkout":
                        (source / ("ignored" if ignored else "changing")).write_text("late owner work\n")
                    return original_git(repo, *args, **kwargs)
                with mock.patch.dict(SOURCE["acquire"].__globals__, git=late_git):
                    fresh, result = self.acquire(self.session(), target)
                self.assertFalse(result["reused"])
                self.assertNotEqual(fresh, source)
                self.assertEqual((source / ("ignored" if ignored else "changing")).read_text(), "late owner work\n")

    def test_incomplete_registration_binding_and_receipts_bound_growth(self):
        for failure in ("registration", "binding", "publication", "unknown"):
            with self.subTest(failure=failure):
                self.cache = self.root / failure
                self.cache.mkdir(mode=0o700)
                session = self.session()
                original_bind = SOURCE["bind"]
                original_write = SOURCE["write"]
                def failed_bind(*args, **kwargs):
                    original_bind(*args, **kwargs)
                    raise ValueError("fixture stopped after actual source binding")
                def failed_write(path, *args, **kwargs):
                    if Path(path).name.startswith("source-") and Path(path).suffix == ".json":
                        raise OSError("fixture stopped before receipt publication")
                    return original_write(path, *args, **kwargs)
                overrides = {"LIMIT": 2}
                if failure == "binding":
                    overrides["bind"] = failed_bind
                elif failure == "publication":
                    overrides["write"] = failed_write
                with mock.patch.dict(SOURCE["acquire"].__globals__, **overrides):
                    if failure == "unknown":
                        key, _, _ = SOURCE["pool_key"](self.repo, COMPILER, TARGET)
                        pool = SOURCE["private_child"](SOURCE["metadata_root"](self.cache, SOURCE["POOL"]), key)
                        (pool / "unknown-owner-data").write_bytes(b"owner data\n")
                        (pool / "source-not-a-receipt.noindex").mkdir()
                    else:
                        for _ in range(2):
                            with self.assertRaises((ValueError, RuntimeError, OSError)):
                                self.acquire(session, "f" * 40 if failure == "registration" else self.first)
                        pool, = (self.cache / SOURCE["POOL"]).iterdir()
                    reservations = {path.name: (path.lstat().st_dev, path.lstat().st_ino)
                                    for path in pool.iterdir() if path.name != ".lock"}
                    registered = self.git("worktree", "list", "--porcelain")
                    self.assertEqual(len(reservations), 2)
                    self.assertFalse(list(pool.glob("source-*.json")))
                    # Once the fault is gone, an ordinary next request must
                    # still refuse growth before adding a third reservation.
                    with mock.patch.dict(SOURCE["acquire"].__globals__, bind=original_bind, write=original_write):
                        with self.assertRaisesRegex(ValueError, "pool is full"):
                            self.acquire(session)
                    self.assertEqual(reservations, {path.name: (path.lstat().st_dev, path.lstat().st_ino)
                                                   for path in pool.iterdir() if path.name != ".lock"})
                    self.assertEqual(self.git("worktree", "list", "--porcelain"), registered)
                    if failure in ("binding", "publication"):
                        for name in reservations:
                            source = pool / name
                            self.assertIn(f"worktree {source}\n", registered)
                            self.assertEqual((source / "changing").read_text(), "first\n")
                    elif failure == "unknown":
                        self.assertEqual((pool / "unknown-owner-data").read_bytes(), b"owner data\n")

    def worker(self, *, late=False, ready=None):
        return f'''
import json, os, runpy, signal
from pathlib import Path
source = runpy.run_path({str(ROOT / 'alab/package_source.py')!r})
result = source['acquire'](Path({str(self.cache)!r}), Path({str(self.repo)!r}), {self.first!r}, {COMPILER!r}, {TARGET!r})
source['release'](Path({str(self.cache)!r}), result['path'], {self.first!r})
if {late!r}:
    (Path(result['path']) / 'Cargo.toml').write_text('late writer after return\\n')
Path({str(ready)!r}).write_text(json.dumps({{'path': result['path'], 'pid': os.getpid(), 'pgid': os.getpgrp(), 'start': source['process_start'](os.getpid())}}))
signal.alarm(60)
signal.pause()
'''

    def await_ready(self, path):
        deadline = time.monotonic() + 60
        while not path.exists():
            if time.monotonic() >= deadline:
                self.fail("source worker exceeded the one-minute startup hang detector")
            time.sleep(0.01)
        return json.loads(path.read_text())

    def test_actual_guardian_returns_ready_only_after_child_group_retirement(self):
        for late in (False, True):
            with self.subTest(late=late):
                self.cache = self.root / f"guardian-{late}"
                self.cache.mkdir(mode=0o700)
                ready = self.root / f"worker-{late}.json"
                observe = GUARD["run"].__globals__["observe_exit"]
                def inspect(pid):
                    current = self.await_ready(ready)
                    source = Path(current["path"])
                    self.assertEqual(current["pid"], current["pgid"])
                    self.assertNotEqual(current["pid"], pid,
                                        "custodian stays outside the real command group")
                    self.assertEqual(SOURCE["read"](self.receipt(source))["phase"], "active")
                    os.kill(current["pid"], signal.SIGTERM)
                    observe(pid)
                with mock.patch.dict(GUARD["run"].__globals__, observe_exit=inspect):
                    code = GUARD["run_with_cache_lock"](
                        self.cache / ".alab-package.lockf",
                        [sys.executable, "-I", "-B", "-c", self.worker(late=late, ready=ready)],
                        package_repo=self.repo)
                self.assertEqual(code, 128 + signal.SIGTERM)
                current = json.loads(ready.read_text())
                self.assertFalse(GUARD["group_exists"](current["pgid"]))
                source = Path(current["path"])
                self.assertEqual(SOURCE["read"](self.receipt(source))["phase"], "active" if late else "ready")
                fresh, result = self.acquire(self.session(), self.second)
                self.assertEqual(result["reused"], not late)
                if late:
                    self.assertNotEqual(fresh, source)
                    self.assertEqual((source / "Cargo.toml").read_text(), "late writer after return\n")

    def test_actual_guardian_crash_preserves_active_generation(self):
        ready = self.root / "crashed-worker.json"
        guardian = subprocess.Popen([sys.executable, "-I", "-B", str(ROOT / "alab/command_guard.py"),
            "--cache-lock", str(self.cache / ".alab-package.lockf"), "--package-source-repo", str(self.repo),
            "--", sys.executable, "-I", "-B", "-c", self.worker(ready=ready)])
        current = None
        try:
            current = self.await_ready(ready)
            guardian.kill()
            self.assertEqual(guardian.wait(timeout=60), -signal.SIGKILL)
            # The surviving custodian retires this group while keeping the
            # inherited cache lease. A crashed generation stays Active regardless.
            deadline = time.monotonic() + 60
            while GUARD["group_exists"](current["pgid"]):
                if time.monotonic() >= deadline:
                    self.fail("crash fixture worker did not retire in one minute")
                time.sleep(0.01)
            source = Path(current["path"])
            before = self.receipt(source).read_bytes()
            fresh, result = self.acquire(self.session(), self.second)
            self.assertFalse(result["reused"])
            self.assertNotEqual(fresh, source)
            self.assertEqual(self.receipt(source).read_bytes(), before)
            self.assertEqual((source / "changing").read_text(), "first\n")
        finally:
            if guardian.poll() is None:
                guardian.kill()
                guardian.wait(timeout=60)
            if current is not None:
                try:
                    if SOURCE["process_start"](current["pid"]) == current["start"]:
                        os.kill(current["pid"], signal.SIGKILL)
                except (ValueError, ProcessLookupError):
                    pass

    def test_concurrent_cache_guard_refuses_before_allocating_source(self):
        lock = self.cache / ".alab-package.lockf"
        with lock.open("w") as held:
            fcntl.flock(held, fcntl.LOCK_EX)
            code = GUARD["run_with_cache_lock"](lock, ["must-not-start"], package_repo=self.repo)
            self.assertEqual(code, 75)
        self.assertFalse((self.cache / ".ubd").exists())

    def test_legacy_stamps_reclaimed_after_real_guardian_retirement_but_owner_work_kept(self):
        tools = self.repo / "alab"
        tools.mkdir()
        for name in ("package_source.py", "retention.py"):
            shutil.copyfile(ROOT / "alab" / name, tools / name)
        manifest = self.repo / "codex-rs/Cargo.toml"
        manifest.parent.mkdir()
        manifest.write_text('[workspace.package]\nversion = "0.0.0"\n[workspace]\nmembers = []\n')
        (manifest.parent / "Cargo.lock").write_text('[[package]]\nname = "fixture"\nversion = "0.0.0"\n')
        legacy = self.commit("legacy fixture")
        body = r'''set -euo pipefail
. "$1/alab/package.sh"
repo_root="$2" python="$3" cache="$4" commit="$5" compiler_id="$6" target_triple="$7"
sha9="${commit:0:9}" version=0.166.0-dev.1+gfixture
CARGO_TARGET_DIR="$8" cargo=fixture_cargo scratch_paths=()
fixture_cargo() { "$python" -I -B -c 'from pathlib import Path; p=Path("Cargo.lock"); p.write_text(p.read_text().replace("0.0.0", "0.166.0-dev.1+gfixture"))'; }
package_contained() { shift 3; "$@"; }
package_prepare_source
printf '%s\n' "$wt" > "$9"
package_source_version
package_cleanup
[[ -d "$wt" ]] # The worker requested cleanup; only the guardian can remove it.
case "${10}" in
  owner) printf '\n# owner later changed stamped input\n' >> "$wt/codex-rs/Cargo.toml" ;;
  replacement) cp "$wt/codex-rs/Cargo.toml" "$wt/codex-rs/replacement"; mv "$wt/codex-rs/replacement" "$wt/codex-rs/Cargo.toml" ;;
  git-metadata) printf 'owner Git data\n' > "$(git -C "$wt" rev-parse --absolute-git-dir)/notes" ;;
  git-lock) printf 'owner Git lock\n' > "$(git -C "$wt" rev-parse --absolute-git-dir)/HEAD.lock" ;;
esac
'''
        for case in ("tool-only", "owner", "replacement", "git-metadata", "git-lock"):
            with self.subTest(case=case):
                self.cache = self.root / f"legacy-{case}"
                self.cache.mkdir(mode=0o700)
                path_record = self.root / f"legacy-{case}.path"
                code = GUARD["run_with_cache_lock"](
                    self.cache / ".alab-package.lockf",
                    ["/usr/bin/env", f"HOME={self.home}", "/bin/bash", "-c", body, "legacy-guarded",
                     str(ROOT), str(self.repo), sys.executable, str(self.cache), legacy, COMPILER, TARGET,
                     str(self.root / "target.noindex"), str(path_record), case], package_repo=self.repo)
                self.assertEqual(code, 0)
                source = Path(path_record.read_text().strip())
                registrations = self.git("worktree", "list", "--porcelain")
                if case == "tool-only":
                    self.assertFalse(source.exists())
                    self.assertNotIn(f"worktree {source}\n", registrations)
                else:
                    self.assertTrue(source.is_dir())
                    self.assertIn(f"worktree {source}\n", registrations)
                    self.assertIn('version = "0.166.0-dev.1+gfixture"', (source / "codex-rs/Cargo.toml").read_text())
                    if case == "owner":
                        self.assertIn("owner later changed", (source / "codex-rs/Cargo.toml").read_text())
                    elif case in ("git-metadata", "git-lock"):
                        entry = Path(self.git("rev-parse", "--absolute-git-dir", repo=source))
                        self.assertTrue((entry / ("notes" if case == "git-metadata" else "HEAD.lock")).is_file())

    def test_actual_current_version_helper_keeps_source_reusable_and_legacy_is_fresh(self):
        tools = self.repo / "alab"
        tools.mkdir()
        for name in ("package_source.py", "retention.py"):
            shutil.copyfile(ROOT / "alab" / name, tools / name)
        manifest = self.repo / "codex-rs/Cargo.toml"
        manifest.parent.mkdir()
        manifest.write_text('[workspace.package]\nversion = "0.0.0"\n[workspace]\nmembers = []\n')
        (manifest.parent / "Cargo.lock").write_text('version = 4\n')
        version = self.repo / "codex-rs/alab-core/src/version.rs"
        version.parent.mkdir(parents=True)
        version.write_text('// CODEX_ALAB_BUILD_VERSION CODEX_ALAB_PRODUCT_VERSION\n')
        modern = self.commit("current executable-only version capability")
        session = self.session()
        body = r'''set -euo pipefail
. "$1/alab/package.sh"
repo_root="$2" python="$3" cache="$4" commit="$5" compiler_id="$6" target_triple="$7"
sha9="${commit:0:9}" own_line=1 version=0.1.0-dev.1+codex.0.165.0.ga
client_version=0.166.0-dev.1+ga
package_prepare_source
package_source_version
printf '%s\n' "$wt"
"$python" -I -B "$repo_root/alab/package_source.py" release --cache "$cache" --source "$wt" --commit "$commit"
'''
        done = subprocess.run(["/bin/bash", "-c", body, "source-version-integration", str(ROOT), str(self.repo),
                               sys.executable, str(self.cache), modern, COMPILER, TARGET],
                              env={**session.environment(), "HOME": str(self.home)}, capture_output=True, text=True, timeout=60)
        self.assertEqual(done.returncode, 0, done.stderr)
        session.finish()
        source, result = self.acquire(self.session(), modern)
        self.assertTrue(result["reused"], "the actual stamping helper must leave a reusable clean source")
        self.assertEqual((source / "codex-rs/Cargo.toml").read_bytes(), manifest.read_bytes())
        version.unlink()
        legacy = self.commit("historical source")
        fresh_body = body.split("package_source_version\n")[0] + 'printf "%s\\n%s\\n" "$wt" "$wt_managed"\n'
        done = subprocess.run(["/bin/bash", "-c", fresh_body, "legacy-source-integration", str(ROOT), str(self.repo),
                               sys.executable, str(self.cache), legacy, COMPILER, TARGET],
                              env={**self.session().environment(), "HOME": str(self.home)}, capture_output=True, text=True, timeout=60)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(done.stdout.splitlines()[-1], "0")
        self.assertNotEqual(Path(done.stdout.splitlines()[-2]), source)

    def test_real_package_helper_preserves_opt_out_and_failed_pool_fallbacks(self):
        tools = self.repo / "alab"
        tools.mkdir()
        for name in ("package_source.py", "retention.py"):
            shutil.copyfile(ROOT / "alab" / name, tools / name)
        version = self.repo / "codex-rs/alab-core/src/version.rs"
        version.parent.mkdir(parents=True)
        version.write_text("// CODEX_ALAB_BUILD_VERSION CODEX_ALAB_PRODUCT_VERSION\n")
        modern = self.commit("current source capability")
        body = r'''set -euo pipefail
. "$1/alab/package.sh"
repo_root="$2" python="$3" cache="$4" commit="$5" compiler_id="$6" target_triple="$7"
sha9="${commit:0:9}" scratch_paths=()
package_prepare_source
printf '%s\n%s\n' "$wt" "$wt_managed" > "$8"
package_cleanup
[[ -d "$wt" ]] # Fresh removal must wait for the enclosing guardian.
'''
        for case in ("managed", "keep", "policy", "unknown-policy", "no-guardian", "pool-unavailable", "nested-cache"):
            with self.subTest(case=case):
                self.cache = (self.repo if case == "nested-cache" else self.root) / f"fallback-{case}"
                self.cache.mkdir(mode=0o700)
                previous, before = None, None
                if case not in ("pool-unavailable", "nested-cache"):
                    previous = self.ready()
                    before = self.receipt(previous).read_bytes()
                if case == "pool-unavailable":
                    key, _, _ = SOURCE["pool_key"](self.repo, COMPILER, TARGET)
                    pool = SOURCE["private_child"](SOURCE["metadata_root"](self.cache, SOURCE["POOL"]), key)
                    for number in range(SOURCE["LIMIT"]):
                        (pool / f"unknown-{number}").write_bytes(b"keep unknown generation\n")
                marker = self.repo / ".git/alab-preserve-builds"
                policy = tools / "retention.py"
                original_policy = policy.read_bytes()
                if case == "policy":
                    marker.write_text("preserve all build sources\n")
                elif case == "unknown-policy":
                    policy.unlink()
                record = self.root / f"fallback-{case}.path"
                argv = ["/usr/bin/env", f"HOME={self.home}", f"ALAB_PKG_KEEP_WORKTREE={int(case == 'keep')}",
                        "/bin/bash", "-c", body, "fallback-source", str(ROOT), str(self.repo), sys.executable,
                        str(self.cache), modern, COMPILER, TARGET, str(record)]
                try:
                    if case == "no-guardian":
                        environment = {key: value for key, value in os.environ.items() if key != SOURCE["GUARD_ENV"]}
                        done = subprocess.run(argv, capture_output=True, text=True, timeout=60, env=environment)
                        self.assertEqual(done.returncode, 0, done.stderr)
                    else:
                        code = GUARD["run_with_cache_lock"](self.cache / ".alab-package.lockf", argv, package_repo=self.repo)
                        self.assertEqual(code, 0)
                finally:
                    marker.unlink(missing_ok=True)
                    if not policy.exists():
                        policy.write_bytes(original_policy)
                path, managed = record.read_text().splitlines()
                source = Path(path)
                self.assertEqual(managed, "1" if case == "managed" else "0")
                if case == "managed":
                    self.assertEqual(source, previous)
                    self.assertEqual(SOURCE["read"](self.receipt(source))["phase"], "ready")
                else:
                    self.assertEqual(source.exists(), case in ("keep", "policy", "unknown-policy", "no-guardian"))
                    if previous is not None:
                        self.assertNotEqual(source, previous)
                        self.assertEqual(self.receipt(previous).read_bytes(), before)
                    registered = self.git("worktree", "list", "--porcelain")
                    self.assertEqual(f"worktree {source}\n" in registered, source.exists())
                    if case == "pool-unavailable":
                        self.assertEqual(len(list(pool.glob("unknown-*"))), SOURCE["LIMIT"])

    @unittest.skipUnless(sys.version_info >= (3, 11), "upstream's current packager requires Python 3.11")
    def admission_fixture(self):
        tools = self.repo / "alab"
        tools.mkdir()
        for name in ("package_source.py", "retention.py"):
            shutil.copyfile(ROOT / "alab" / name, tools / name)
        manifest = self.repo / "codex-rs/Cargo.toml"
        manifest.parent.mkdir()
        manifest.write_text('[workspace.package]\nversion = "0.0.0"\n[workspace]\nmembers = []\n')
        (manifest.parent / "Cargo.lock").write_text('[[package]]\nname = "fixture"\nversion = "0.0.0"\n')
        revision = self.commit("legacy source admission fixture")
        hook = self.repo / ".git/hooks/post-checkout"
        hook.write_text(r'''#!/bin/sh
set -eu
source=$(git rev-parse --show-toplevel)
entry=$(git rev-parse --absolute-git-dir)
printf '%s\n' "$source" > "$ALAB_ADMISSION_RECORD"
printf '%s\n' "$entry" > "$ALAB_ADMISSION_RECORD.git"
case "$ALAB_ADMISSION_FAULT" in
  ignored) printf 'owner ignored input\n' > "$source/ignored" ;;
  assume-unchanged|skip-worktree)
    git update-index "--$ALAB_ADMISSION_FAULT" changing
    printf 'owner hidden input\n' > "$source/changing"
    ;;
  git-note) printf 'owner Git note\n' > "$entry/notes" ;;
  git-ref)
    mkdir -p "$entry/refs/worktree"
    printf '%s\n' "$ALAB_ADMISSION_UNPUBLISHED" > "$entry/refs/worktree/owner"
    ;;
  git-orig-head) printf '%s\n' "$ALAB_ADMISSION_UNPUBLISHED" > "$entry/ORIG_HEAD" ;;
  git-reflog)
    mkdir -p "$entry/logs"
    printf '%s %s Source <fixture@example.invalid> 1 +0000\towner unpublished work\n' \
      "$(git rev-parse HEAD)" "$ALAB_ADMISSION_UNPUBLISHED" >> "$entry/logs/HEAD"
    ;;
  git-same-oid-note)
    head=$(git rev-parse HEAD)
    printf '%s %s Source <fixture@example.invalid> 1 +0000\towner-only audit note\n' \
      "$head" "$head" >> "$entry/logs/HEAD"
    ;;
esac
''')
        hook.chmod(0o700)
        return revision

    def run_admission_helper(self, revision, fault):
        body = r'''set -euo pipefail
. "$1/alab/package.sh"
repo_root="$2" python="$3" cache="$4" commit="$5" compiler_id="$6" target_triple="$7"
sha9="${commit:0:9}" version=0.166.0-dev.1+gfixture
CARGO_TARGET_DIR="$8" cargo=fixture_cargo scratch_paths=()
trap package_cleanup EXIT
fixture_cargo() {
  "$python" -I -B -c 'from pathlib import Path; p=Path("Cargo.lock"); p.write_text(p.read_text().replace("0.0.0", "0.166.0-dev.1+gfixture"))'
  case "$ALAB_ADMISSION_FAULT" in
    stamp-content) printf '\n# owner changed the stamped input\n' >> Cargo.toml ;;
    stamp-hidden)
      git -C "$wt" update-index --assume-unchanged changing
      printf 'owner hidden stamp input\n' > "$wt/changing"
      ;;
    stamp-metadata) printf 'owner Git note\n' > "$(git -C "$wt" rev-parse --absolute-git-dir)/notes" ;;
  esac
}
package_contained() { shift 3; "$@"; }
package_prepare_source
package_source_version
package_validate_source
printf 'compile allowed\n' > "$9"
'''
        record = self.root / f"admission-{fault}.source"
        compiled = self.root / f"admission-{fault}.compiled"
        tree = self.git("rev-parse", f"{revision}^{{tree}}")
        unpublished = self.git("commit-tree", tree, "-p", revision, "-m", "owner unpublished work")
        code = GUARD["run_with_cache_lock"](
            self.cache / ".alab-package.lockf",
            ["/usr/bin/env", f"HOME={self.home}", f"ALAB_ADMISSION_RECORD={record}",
             f"ALAB_ADMISSION_FAULT={fault}", f"ALAB_ADMISSION_UNPUBLISHED={unpublished}",
             "/bin/bash", "-c", body, "package-admission", str(ROOT), str(self.repo),
             sys.executable, str(self.cache), revision, COMPILER, TARGET,
             str(self.root / "target.noindex"), str(compiled)], package_repo=self.repo)
        source = Path(record.read_text().strip())
        entry = Path(record.with_suffix(".source.git").read_text().strip())
        witness = {"case": fault, "status": code, "compile_allowed": compiled.exists(),
                   "source_exists": source.is_dir(), "git_entry_exists": entry.is_dir()}
        print("admission witness: " + json.dumps(witness, sort_keys=True), file=sys.stderr)
        (self.root / f"admission-{fault}.json").write_text(json.dumps(witness, sort_keys=True) + "\n")
        return code, source, entry, compiled, unpublished

    def test_fresh_source_refusal_stops_before_compilation_and_preserves_inputs(self):
        revision = self.admission_fixture()
        for fault in ("ignored", "assume-unchanged", "skip-worktree", "none"):
            with self.subTest(fault=fault):
                code, source, _, compiled, _ = self.run_admission_helper(revision, fault)
                if fault == "none":
                    self.assertEqual(code, 0)
                    self.assertTrue(compiled.is_file())
                    self.assertFalse(source.exists(), "CONTROL: exact tool-only source is reclaimed")
                else:
                    self.assertNotEqual(code, 0, "known source refusal must stop the transaction")
                    self.assertFalse(compiled.exists())
                    self.assertTrue(source.is_dir())
                    path = source / ("ignored" if fault == "ignored" else "changing")
                    self.assertIn("owner", path.read_text())

    def test_legacy_stamp_refusal_stops_before_compilation_and_preserves_inputs(self):
        revision = self.admission_fixture()
        for fault in ("stamp-content", "stamp-hidden", "stamp-metadata"):
            with self.subTest(fault=fault):
                code, source, entry, compiled, _ = self.run_admission_helper(revision, fault)
                self.assertNotEqual(code, 0, "known stamp refusal must stop the transaction")
                self.assertFalse(compiled.exists())
                self.assertTrue(source.is_dir())
                if fault == "stamp-content":
                    self.assertIn("owner changed", (source / "codex-rs/Cargo.toml").read_text())
                elif fault == "stamp-hidden":
                    self.assertIn("owner", (source / "changing").read_text())
                else:
                    self.assertEqual((entry / "notes").read_text(), "owner Git note\n")

    def test_new_source_never_adopts_unproved_initial_git_work(self):
        revision = self.admission_fixture()
        for fault in ("git-note", "git-ref", "git-orig-head", "git-reflog", "git-same-oid-note"):
            with self.subTest(fault=fault):
                code, source, entry, compiled, unpublished = self.run_admission_helper(revision, fault)
                self.assertNotEqual(code, 0, "initial owner Git state is not tool-owned birth state")
                self.assertFalse(compiled.exists())
                self.assertTrue(source.is_dir())
                if fault == "git-note":
                    self.assertEqual((entry / "notes").read_text(), "owner Git note\n")
                elif fault == "git-ref":
                    self.assertEqual((entry / "refs/worktree/owner").read_text().strip(), unpublished)
                elif fault == "git-orig-head":
                    self.assertEqual((entry / "ORIG_HEAD").read_text().strip(), unpublished)
                elif fault == "git-reflog":
                    self.assertIn(unpublished, (entry / "logs/HEAD").read_text())
                else:
                    self.assertIn("owner-only audit note", (entry / "logs/HEAD").read_text())

    def test_reference_transaction_birth_work_is_preserved_before_checkout(self):
        revision = self.admission_fixture()
        hook = self.repo / ".git/hooks/reference-transaction"
        hook.write_text(r'''#!/bin/sh
set -eu
[ "$1" = committed ] || exit 0
common=$(git rev-parse --path-format=absolute --git-common-dir)
entry=
for candidate in "$common"/worktrees/*; do
  [ -f "$candidate/locked" ] && [ -f "$candidate/gitdir" ] || continue
  entry=$candidate
done
[ -n "$entry" ] || exit 0
source=$(cat "$entry/gitdir")
source=${source%/.git}
printf '%s\n' "$source" > "$ALAB_ADMISSION_RECORD"
printf '%s\n' "$entry" > "$ALAB_ADMISSION_RECORD.git"
printf 'before initial checkout\n' > "$ALAB_ADMISSION_RECORD.birth"
case "$ALAB_ADMISSION_FAULT" in
  reference-note) printf 'owner reference-hook note\n' > "$entry/notes" ;;
  reference-same-oid-note)
    head=$(git rev-parse HEAD)
    printf '%s %s Source <fixture@example.invalid> 1 +0000\towner reference audit\n' \
      "$head" "$head" >> "$entry/logs/HEAD"
    ;;
esac
''')
        hook.chmod(0o700)
        for fault in ("reference-note", "reference-same-oid-note", "none"):
            with self.subTest(fault=fault):
                code, source, entry, compiled, _ = self.run_admission_helper(revision, fault)
                if fault == "none":
                    self.assertEqual(code, 0)
                    self.assertTrue(compiled.is_file())
                    self.assertFalse(source.exists())
                    self.assertFalse(entry.exists())
                else:
                    self.assertNotEqual(code, 0)
                    self.assertFalse(compiled.exists())
                    self.assertTrue(source.is_dir())
                    self.assertTrue(entry.is_dir())
                    self.assertFalse((entry / "index").exists(), "birth refusal precedes initial checkout")
                    self.assertFalse((source / "changing").exists(), "no tracked input was checked out")
                    if fault == "reference-note":
                        self.assertEqual((entry / "notes").read_text(), "owner reference-hook note\n")
                    else:
                        self.assertIn("owner reference audit", (entry / "logs/HEAD").read_text())

    def test_reuse_checkout_never_recaptures_hook_metadata_as_ownership(self):
        for moved in (False, True):
            for kind in ("note", "same-oid-note"):
                with self.subTest(moved=moved, kind=kind):
                    self.cache = self.root / f"reuse-hook-{moved}-{kind}"
                    self.cache.mkdir(mode=0o700)
                    source = self.ready()
                    old = SOURCE["read"](self.receipt(source))
                    entry = Path(old["entry"])
                    hook = self.repo / ".git/hooks/post-checkout"
                    hook.write_text(r'''#!/bin/sh
set -eu
[ "$(git rev-parse --show-toplevel)" = "$ALAB_OWNER_SOURCE" ] || exit 0
entry=$(git rev-parse --absolute-git-dir)
if [ "$ALAB_OWNER_KIND" = note ]; then
  printf 'owner reuse hook note\n' > "$entry/notes"
else
  head=$(git rev-parse HEAD)
  printf '%s %s Source <fixture@example.invalid> 1 +0000\towner reuse audit\n' \
    "$head" "$head" >> "$entry/logs/HEAD"
fi
''')
                    hook.chmod(0o700)
                    session = self.session()
                    with mock.patch.dict(os.environ, ALAB_OWNER_SOURCE=str(source), ALAB_OWNER_KIND=kind):
                        replacement, result = self.acquire(session, self.second if moved else self.first)
                    self.assertFalse(result["reused"])
                    self.assertNotEqual(replacement, source)
                    retained = SOURCE["read"](self.receipt(source))
                    self.assertEqual(retained["phase"], "active")
                    self.assertEqual(retained["metadata"], old["metadata"])
                    self.returned(session, replacement, self.second if moved else self.first)
                    session.finish()
                    self.assertTrue(source.is_dir())
                    if kind == "note":
                        self.assertEqual((entry / "notes").read_text(), "owner reuse hook note\n")
                    else:
                        self.assertIn("owner reuse audit", (entry / "logs/HEAD").read_text())
                    hook.unlink()

    def test_old_after_the_fact_bindings_are_preserved_without_upgrade(self):
        source = self.ready()
        path = self.receipt(source)
        current = SOURCE["read"](path)
        legacy = {key: value for key, value in current.items() if key != "custody"}
        SOURCE["write"](path, legacy, existing=current)
        before = path.read_bytes()
        session = self.session()
        replacement, result = self.acquire(session, self.second)
        self.assertFalse(result["reused"])
        self.assertNotEqual(replacement, source)
        self.assertEqual(path.read_bytes(), before)
        self.assertEqual((source / "changing").read_text(), "first\n")
        self.assertEqual(SOURCE["read"](self.receipt(replacement))["custody"], 2)
        self.returned(session, replacement, self.second)
        session.finish()
        self.assertEqual(path.read_bytes(), before)

    def test_source_custody_needs_no_configured_human_git_identity(self):
        self.git("config", "--unset-all", "user.name")
        self.git("config", "--unset-all", "user.email")
        self.git("config", "user.useConfigOnly", "true")
        environment = {key: value for key, value in os.environ.items()
                       if not key.startswith("GIT_")}
        environment.update(HOME=str(self.home), GIT_CONFIG_GLOBAL=os.devnull,
                           GIT_CONFIG_NOSYSTEM="1")
        with mock.patch.dict(os.environ, environment, clear=True):
            absent = subprocess.run(["git", "-C", str(self.repo), "var", "GIT_COMMITTER_IDENT"],
                                    capture_output=True, timeout=60)
            self.assertNotEqual(absent.returncode, 0, "control: no ambient committer exists")
            source = self.ready()
            session = self.session()
            try:
                reused, result = self.acquire(session, self.second)
                self.assertTrue(result["reused"])
                self.assertEqual(reused, source)
                self.assertEqual((source / "changing").read_text(), "second\n")
                self.returned(session, source, self.second)
            finally:
                session.finish()
            fresh = self.root / "fresh without identity"
            fresh.mkdir(mode=0o700)
            record = SOURCE["create_fresh"](self.repo, fresh, self.first)
            self.assertEqual(SOURCE["validate_binding"](record), fresh)
            self.assertEqual((fresh / "changing").read_text(), "first\n")
            self.assertEqual(self.git("rev-parse", "HEAD", repo=source), self.second)
            self.assertEqual(self.git("rev-parse", "HEAD", repo=self.repo), self.second)

    def test_normal_checkout_reuse_supports_reflog_configuration(self):
        for setting in ("false", "true", "always"):
            with self.subTest(setting=setting):
                self.git("config", "core.logAllRefUpdates", setting)
                self.cache = self.root / f"reflog-{setting}"
                self.cache.mkdir(mode=0o700)
                source = self.ready()
                session = self.session()
                reused, result = self.acquire(session, self.second)
                self.assertTrue(result["reused"])
                self.assertEqual(reused, source)
                self.assertEqual((source / "changing").read_text(), "second\n")
                self.returned(session, source, self.second)
                session.finish()

    def test_fsmonitor_cannot_write_git_work_during_guardian_removal(self):
        for write_after in (0, 2):
            with self.subTest(write_after=write_after):
                source = self.root / f"monitor-source-{write_after}"
                source.mkdir(mode=0o700)
                record = SOURCE["create_fresh"](self.repo, source, self.first)
                entry = Path(record["entry"])
                session = self.session()
                with mock.patch.dict(os.environ, session.environment()):
                    SOURCE["release_fresh"](self.cache, record)
                counter = self.root / f"monitor-count-{write_after}"
                hook = self.repo / ".git/hooks/fsmonitor-custody"
                hook.write_text(PYTHON_ENTRY + f'''
from pathlib import Path
counter=Path({str(counter)!r})
count=int(counter.read_text())+1 if counter.exists() else 1
counter.write_text(str(count))
if count > {write_after}:
    Path({str(entry / 'notes')!r}).write_text("owner monitor note\\n")
import sys
sys.stdout.buffer.write(b"token\\0")
''')
                hook.chmod(0o700)
                # Prove the callback can run before relying on its absence as evidence.
                control = subprocess.run([str(hook)], capture_output=True, timeout=60)
                self.assertEqual(control.returncode, 0, control.stderr)
                self.assertEqual(control.stdout, b"token\0")
                counter.unlink()
                (entry / "notes").unlink(missing_ok=True)
                self.git("config", "core.fsmonitor", str(hook))
                session.finish()
                self.assertFalse(counter.exists(), "custody queries/removal must not invoke fsmonitor")
                self.assertFalse(source.exists(), "CONTROL: exact clean source is still reclaimed")
                self.assertFalse(entry.exists())
                self.git("config", "--unset", "core.fsmonitor")

    def test_source_validation_ignores_stat_cache_and_preserves_staged_index_work(self):
        for kind in ("cached-content", "staged-only"):
            with self.subTest(kind=kind):
                self.cache = self.root / kind
                self.cache.mkdir(mode=0o700)
                source = self.ready()
                path = self.receipt(source)
                record = SOURCE["read"](path)
                original = path.read_bytes()
                changing = source / "changing"
                self.git("config", "core.trustctime", "false")
                self.git("config", "core.checkStat", "minimal")
                if kind == "cached-content":
                    # Seed a genuinely old stat entry without a timing wait;
                    # a just-written entry is deliberately rehashed by racy-Git.
                    os.utime(changing, ns=(1_600_000_000_000_000_000,) * 2)
                    self.git("update-index", "--refresh", repo=source)
                before = changing.stat()
                changing.write_text("owner\n")  # Same length as first\n.
                if kind == "staged-only":
                    self.git("add", "changing", repo=source)
                    changing.write_text("first\n")
                else:
                    os.utime(changing, ns=(before.st_atime_ns, before.st_mtime_ns))
                    self.assertEqual(self.git("status", "--porcelain", repo=source), "",
                                     "CONTROL: ordinary status trusts the configured stale stat cache")
                with self.assertRaisesRegex(ValueError, "work"):
                    SOURCE["validate_binding"](record)
                self.assertEqual(path.read_bytes(), original)
                self.assertEqual(changing.read_text(), "first\n" if kind == "staged-only" else "owner\n")
                self.git("config", "--unset", "core.trustctime")
                self.git("config", "--unset", "core.checkStat")

    def test_removal_suppresses_filter_callbacks_and_retains_converted_sources(self):
        driver = "Mixed.Driver.Name"
        (self.repo / ".gitattributes").write_text(f"changing filter={driver}\n")
        revision = self.commit("filtered source")
        # NUL framing must preserve quoted subsection whitespace, case and dots.
        self.git("config", "filter.Odd.Name With\tSpace.clean", "cat")
        for kind in ("clean", "process", "transform", "ambiguous"):
            with self.subTest(kind=kind):
                home = self.root / f"filter-{kind}"
                home.mkdir()
                script = home / "filter.py"
                script.write_text('''import sys
from pathlib import Path
root=Path(__file__).parent
mode=sys.argv[1]
def convert(data, operation):
    if operation == "clean" and (root/"armed").exists():
        counter=root/"count"
        count=int(counter.read_text())+1 if counter.exists() else 1
        counter.write_text(str(count))
        if count > 1:
            Path((root/"entry").read_text()).joinpath("notes").write_text("owner filter note\\n")
    if (root/"transform").exists():
        data=data.upper() if operation == "smudge" else data.lower()
    return data
if mode != "process":
    sys.stdout.buffer.write(convert(sys.stdin.buffer.read(), mode))
else:
    def group():
        result=[]
        while True:
            header=sys.stdin.buffer.read(4)
            if not header:
                raise EOFError
            size=int(header, 16)
            if size == 0:
                return result
            result.append(sys.stdin.buffer.read(size-4))
    def send(rows):
        for row in rows:
            sys.stdout.buffer.write(f"{len(row)+4:04x}".encode()+row)
        sys.stdout.buffer.write(b"0000")
        sys.stdout.buffer.flush()
    group()
    send([b"git-filter-server\\n", b"version=2\\n"])
    group()
    send([b"capability=clean\\n", b"capability=smudge\\n"])
    while True:
        try:
            headers=group()
        except EOFError:
            break
        operation=next(row.strip().split(b"=", 1)[1].decode() for row in headers if row.startswith(b"command="))
        data=convert(b"".join(group()), operation)
        send([b"status=success\\n"])
        send([data] if data else [])
        send([])
''')
                command = [sys.executable, "-I", "-B", str(script)]
                self.git("config", f"filter.{driver}.clean", shlex.join([*command, "clean"]))
                self.git("config", f"filter.{driver}.smudge", shlex.join([*command, "smudge"]))
                self.git("config", f"filter.{driver}.required", "false")
                if kind == "process":
                    self.git("config", f"filter.{driver}.process", shlex.join([*command, "process"]))
                if kind == "transform":
                    (home / "transform").touch()
                source = home / "source"
                source.mkdir(mode=0o700)
                record = SOURCE["create_fresh"](self.repo, source, revision)
                entry = Path(record["entry"])
                (home / "entry").write_text(str(entry))
                session = self.session()
                with mock.patch.dict(os.environ, session.environment()):
                    SOURCE["release_fresh"](self.cache, record)
                # Ensure ordinary removal would rehash the real index's content.
                os.utime(source / "changing", ns=(1_600_000_000_000_000_000,) * 2)
                (home / "armed").touch()
                if kind == "ambiguous":
                    self.git("config", "filter.Name=Ambiguous.clean", "cat")
                session.finish()
                self.assertEqual((home / "count").read_text(), "1",
                                 "only the checked validation may run the configured callback")
                self.assertFalse((entry / "notes").exists())
                kept = kind in ("transform", "ambiguous")
                self.assertEqual(source.is_dir(), kept)
                self.assertEqual(entry.is_dir(), kept)
                if kept:
                    self.assertEqual((source / "changing").read_text(), "SECOND\n" if kind == "transform" else "second\n")
                self.git("config", "--remove-section", f"filter.{driver}")
                if kind == "ambiguous":
                    self.git("config", "--remove-section", "filter.Name=Ambiguous")

    def test_custody_ignores_inherited_repository_redirection_but_keeps_config(self):
        source = self.ready()
        record = SOURCE["read"](self.receipt(source))
        redirections = {name: str(self.root / "not-this-repository")
                        for name in SOURCE["repository_environment"]()}
        # Explicit caller configuration is preserved independently of repository paths.
        redirections.update(GIT_CONFIG_COUNT="1", GIT_CONFIG_KEY_0="core.logAllRefUpdates",
                            GIT_CONFIG_VALUE_0="false")
        with mock.patch.dict(os.environ, redirections):
            self.assertEqual(SOURCE["validate_binding"](record), source)
            self.assertEqual(SOURCE["git"](source, "config", "--get", "core.logAllRefUpdates"), "false")
            self.assertEqual(SOURCE["blob"](source, f"{self.first}:changing"), b"first\n")

        changing = source / "changing"
        changing.write_text("owner\n")
        self.git("add", "changing", repo=source)
        changing.write_text("first\n")
        alternate = self.root / "separate-clean-index"
        with mock.patch.dict(os.environ, GIT_INDEX_FILE=str(alternate)):
            self.git("read-tree", self.first, repo=source)
            self.assertEqual(self.git("diff", "--cached", "--name-only", repo=source), "",
                             "CONTROL: inherited alternate index conceals the actual staged change")
            with self.assertRaisesRegex(ValueError, "staged work"):
                SOURCE["validate_binding"](record)
        self.assertEqual(self.git("show", ":changing", repo=source), "owner")
        self.assertTrue(source.is_dir())
        self.assertTrue(Path(record["entry"]).is_dir())

    def test_current_source_stamp_inputs_and_real_assembly_return_clean(self):
        actual = self.root / "actual-source"
        subprocess.run(["git", "clone", "--quiet", "--shared", "--no-checkout", str(ROOT), str(actual)],
                       check=True, timeout=60)
        self.git("config", "user.name", "Source fixture", repo=actual)
        self.git("config", "user.email", "fixture@example.invalid", repo=actual)
        current = self.git("rev-parse", "HEAD", repo=ROOT)
        self.git("checkout", "--quiet", "--detach", current, repo=actual)
        # The target is the actual committed source; the new optimization is in
        # its controller only, exactly like packaging historical target commits.
        shutil.copyfile(ROOT / "alab/package_source.py", actual / "alab/package_source.py")
        original_repo = self.repo
        self.repo = actual
        session = self.session()
        binaries = self.root / "prebuilt.noindex"
        binaries.mkdir()
        for name in ("codex", "codex-code-mode-host", "codex-exec-server", "rg"):
            path = binaries / name
            path.write_bytes(f"inert fixture {name}\n".encode())
            path.chmod(0o755)
        staged = self.root / "assembled.noindex"
        body = r'''set -euo pipefail
. "$1/alab/package.sh"
repo_root="$2" python="$3" cache="$4" commit="$5" compiler_id="$6" target_triple="$7"
sha9="${commit:0:9}" own_line=1 version=0.1.0-dev.1+gfixture
client_version=0.166.0-dev.1+gfixture CARGO_TARGET_DIR="$8"
package_prepare_source
package_source_version
package_binary_inputs
[[ "$package_leaf_versions" == 1 && "$package_exec_server" == 1 ]]
scripts="$wt/scripts"
package_contained() { while [[ "$1" != -- ]]; do shift; done; shift; "$@"; }
upstream_py "$9" -- 'from unittest.mock import patch; from codex_package.cli import main
with patch("codex_package.cargo.subprocess.run", side_effect=AssertionError("unexpected native build")):
    sys.argv = ["fixture", *sys.argv[2:]]
    raise SystemExit(main())' --target "$target_triple" --package-version "$version" \
  --package-dir "$9" --rg-bin "$CARGO_TARGET_DIR/release/rg" --strip none "${package_prebuilt_args[@]}"
"$python" -I -B "$repo_root/alab/package_source.py" release --cache "$cache" --source "$wt" --commit "$commit"
printf '%s\n' "$wt"
'''
        # Match the package helper's real release-input layout without compiling.
        (binaries / "release").mkdir()
        for path in list(binaries.iterdir()):
            if path.is_file():
                path.rename(binaries / "release" / path.name)
        done = subprocess.run(["/bin/bash", "-c", body, "current-source-assembly", str(ROOT), str(actual),
                               sys.executable, str(self.cache), current, COMPILER, TARGET, str(binaries), str(staged)],
                              env={**session.environment(), "HOME": str(self.home)},
                              capture_output=True, text=True, timeout=60)
        (self.root / "assembly.stdout").write_text(done.stdout)
        (self.root / "assembly.stderr").write_text(done.stderr)
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        source = Path(done.stdout.splitlines()[-1])
        self.assertFalse(self.git("status", "--porcelain=v1", "--untracked-files=all", "--ignored", repo=source))
        self.assertFalse(list(source.rglob("__pycache__")))
        self.assertEqual((staged / "bin/codex-exec-server").read_bytes(),
                         (binaries / "release/codex-exec-server").read_bytes())
        session.finish()
        next_session = self.session()
        reused, result = self.acquire(next_session, current)
        self.assertTrue(result["reused"])
        self.assertEqual(reused, source)
        # Editing the same manifest the historical helper stamps must survive.
        self.returned(next_session, source, current)
        manifest = source / "codex-rs/Cargo.toml"
        owner = manifest.read_bytes() + b"\n# owner's later manifest work\n"
        manifest.write_bytes(owner)
        next_session.finish()
        fresh, result = self.acquire(self.session(), current)
        self.assertFalse(result["reused"])
        self.assertNotEqual(fresh, source)
        self.assertEqual(manifest.read_bytes(), owner)
        self.repo = original_repo


if __name__ == "__main__":
    unittest.main()

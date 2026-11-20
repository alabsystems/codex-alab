#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Canonical ident contents are checked without callbacks or source writes."""

import importlib.util
import errno
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import threading
from types import SimpleNamespace
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("alab_source_content", ROOT / "alab/source_content.py")
CONTENT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CONTENT)


class SourceContentTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="alab-source-content-")
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name).resolve()
        self.repo = self.root / "repo"
        self.repo.mkdir()
        self.environment = {key: value for key, value in os.environ.items()
                            if not key.startswith("GIT_")}
        self.environment.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1",
                                GIT_OPTIONAL_LOCKS="0", GIT_NO_REPLACE_OBJECTS="1")
        self.git("init", "--quiet")
        self.git("config", "user.name", "Source content fixture")
        self.git("config", "user.email", "fixture@invalid")
        self.git("config", "commit.gpgsign", "false")

    def git(self, *args, environment=None, data=None):
        done = subprocess.run(["git", "-C", str(self.repo), *args], input=data, capture_output=True,
                              env=self.environment if environment is None else environment,
                              timeout=60)
        self.assertEqual(done.returncode, 0, done.stderr.decode(errors="replace"))
        return done.stdout

    def commit(self, path, data, attributes):
        (self.repo / ".gitattributes").write_text(attributes)
        selected = self.repo / path
        selected.parent.mkdir(parents=True, exist_ok=True)
        selected.write_bytes(data)
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", "source content")
        self.source = self.git("rev-parse", "HEAD").decode().strip()
        return self.git("rev-parse", self.source + ":" + path).decode().strip()

    def observe(self, *, filter_config=()):
        index = self.root / "comparison-index"
        environment = {**self.environment, "GIT_INDEX_FILE": str(index)}
        self.git("-c", "core.splitIndex=false", "read-tree", self.source, environment=environment)
        real_index = self.repo / ".git/index"
        before = real_index.read_bytes(), index.read_bytes()
        try:
            result = CONTENT.validate_ident(self.repo, self.source, env=environment,
                                            filter_config=filter_config)
            self.assertIsNone(result)
        finally:
            self.assertEqual((real_index.read_bytes(), index.read_bytes()), before,
                             "content observation must not rewrite either index")

    def assert_git_clean(self, path):
        index = self.root / "git-control-index"
        environment = {**self.environment, "GIT_INDEX_FILE": str(index)}
        self.git("read-tree", self.source, environment=environment)
        self.assertEqual(self.git("status", "--porcelain", "--", path,
                                  environment=environment), b"",
                         "CONTROL: ordinary fresh-index Git normalizes the altered ident")

    def observe_owned_descriptor(self, selected, *, first_error=None, open_error=None,
                                 close_error=None):
        """Exercise the public reader, observing real ownership before fallback cleanup."""
        original = CONTENT.os
        opened, closes, fallback, violations, validation = [], [], [], [], []
        selected_identity = selected.stat()
        before = selected.read_bytes(), (self.repo / ".gitattributes").read_bytes()
        owner = self

        class SelectedOS:
            calls = 0

            def __getattr__(self, name):
                return getattr(original, name)

            def open(self, path, *args, **kwargs):
                if Path(path) != selected:
                    violations.append("unrelated open")
                    raise AssertionError("reader opened an unrelated path")
                if open_error is not None:
                    raise open_error
                descriptor = original.open(path, *args, **kwargs)
                opened.append(descriptor)
                return descriptor

            def fstat(self, descriptor):
                if descriptor not in opened or descriptor in closes:
                    violations.append("unowned fstat")
                    raise AssertionError("reader stat is outside descriptor ownership")
                self.calls += 1
                if self.calls == 1 and first_error is not None:
                    raise first_error
                return original.fstat(descriptor)

            def close(self, descriptor):
                closes.append(descriptor)
                if descriptor not in opened or closes.count(descriptor) != 1:
                    violations.append("unowned or repeated close")
                    raise AssertionError("reader close is outside descriptor ownership")
                original.close(descriptor)
                if close_error is not None:
                    raise close_error

        def trace(frame, event, argument):
            if (frame.f_code is CONTENT.validate_ident.__code__ and event == "exception"
                    and type(argument[1]) is ValueError and not validation):
                validation.append(argument[1])
            return trace

        caught = None
        previous_trace = sys.gettrace()
        try:
            sys.settrace(trace)
            with mock.patch.object(CONTENT, "os", SelectedOS()):
                try:
                    self.observe()
                except BaseException as error:
                    caught = error
        finally:
            sys.settrace(previous_trace)
        states = []
        for descriptor in opened:
            try:
                info = original.fstat(descriptor)
            except OSError as error:
                states.append(error.errno)
            else:
                states.append(None)
                if (descriptor not in closes and (info.st_dev, info.st_ino) ==
                        (selected_identity.st_dev, selected_identity.st_ino)):
                    original.close(descriptor)
                    fallback.append(descriptor)
                else:
                    violations.append("unexpected open descriptor identity")
        # These checks run after owned fallback cleanup, even on the old leaking source.
        owner.assertEqual(violations, [])
        owner.assertEqual((selected.read_bytes(), (self.repo / ".gitattributes").read_bytes()), before)
        for descriptor in fallback:
            with owner.assertRaises(OSError) as closed:
                original.fstat(descriptor)
            owner.assertEqual(closed.exception.errno, errno.EBADF)
        return caught, opened, closes, states, fallback, validation

    def test_success_and_open_refusal_preserve_descriptor_ownership(self):
        self.commit("probe", b"$Id$\n", "probe ident -text\n")
        selected = self.repo / "probe"
        caught, opened, closes, states, fallback, _ = self.observe_owned_descriptor(selected)
        self.assertIsNone(caught)
        self.assertEqual(len(opened), 1)
        self.assertEqual(closes, opened)
        self.assertEqual(states, [errno.EBADF])
        self.assertEqual(fallback, [])
        refused = OSError(errno.EACCES, "controlled selected-file open refusal")
        caught, opened, closes, states, fallback, _ = self.observe_owned_descriptor(
            selected, open_error=refused)
        self.assertIs(caught, refused)
        self.assertEqual((opened, closes, states, fallback), ([], [], [], []))

    def test_first_fstat_failure_closes_once_without_losing_the_primary(self):
        class NoNotes(OSError):
            def add_note(self, _note):
                raise KeyboardInterrupt("unavailable note transport")

        class UnprintableClose(OSError):
            def __str__(self):
                raise SystemExit(0)

            __repr__ = __str__

        self.commit("probe", b"$Id$\n", "probe ident -text\n")
        faults = [
            (OSError(errno.EIO, "first fstat"), None),
            (KeyboardInterrupt("first fstat"), None),
            (OSError(errno.EIO, "first fstat"), KeyboardInterrupt("close")),
            (KeyboardInterrupt("first fstat"), SystemExit(0)),
            (NoNotes(errno.EIO, "first fstat"), SystemExit(47)),
            (KeyboardInterrupt("first fstat"), UnprintableClose(errno.EIO, "close")),
        ]
        for primary, secondary in faults:
            with self.subTest(primary=type(primary).__name__, close=type(secondary).__name__):
                caught, opened, closes, states, fallback, _ = self.observe_owned_descriptor(
                    self.repo / "probe", first_error=primary, close_error=secondary)
                self.assertIs(caught, primary)
                if (type(primary) in (OSError, KeyboardInterrupt)
                        and type(secondary) in (KeyboardInterrupt, SystemExit)):
                    self.assertTrue(any("close" in note and type(secondary).__name__ in note
                                        for note in getattr(caught, "__notes__", ())))
                self.assertEqual(len(opened), 1)
                self.assertEqual(closes, opened)
                self.assertEqual(states, [errno.EBADF])
                self.assertEqual(fallback, [])

    def test_real_content_refusal_survives_descriptor_close_failure(self):
        self.commit("probe", b"$Id$\n", "probe ident -text\n")
        selected = self.repo / "probe"
        selected.write_bytes(b"$Id: forged $\n")
        for secondary in (OSError(errno.EIO, "close"), KeyboardInterrupt("close")):
            with self.subTest(close=type(secondary).__name__):
                caught, opened, closes, states, fallback, validation = self.observe_owned_descriptor(
                    selected, close_error=secondary)
                self.assertEqual(len(validation), 1)
                self.assertIs(caught, validation[0])
                self.assertIn("differs from its raw blob and canonical checkout", str(caught))
                self.assertEqual(closes, opened)
                self.assertEqual(states, [errno.EBADF])
                self.assertEqual(fallback, [])

    def test_sole_descriptor_close_failure_cannot_return_success(self):
        self.commit("probe", b"$Id$\n", "probe ident -text\n")
        for failure in (OSError(errno.EIO, "close"), KeyboardInterrupt("close"), SystemExit(0)):
            with self.subTest(close=type(failure).__name__):
                caught, opened, closes, states, fallback, _ = self.observe_owned_descriptor(
                    self.repo / "probe", close_error=failure)
                self.assertIs(caught, failure)
                self.assertEqual(len(opened), 1)
                self.assertEqual(closes, opened)
                self.assertEqual(states, [errno.EBADF])
                self.assertEqual(fallback, [])

    def canonical_checkout(self):
        blob = self.commit("probe", b"owned source $Id$\n", "probe ident\n")
        canonical = self.git("cat-file", "--filters", "--path=probe", blob)
        self.assertIn(b"$Id: ", canonical)
        (self.repo / "probe").write_bytes(canonical)
        return canonical

    def checkout_cleanup(self, canonical, *, primary=None, secondary=None,
                         cancel_error=None, setup_error=None, inflight=False):
        """Observe real checkout ownership, with only module-local declared faults."""
        owner = self
        selected = self.repo / "probe"
        index = self.root / "checkout-index"
        environment = {**self.environment, "GIT_INDEX_FILE": str(index)}
        self.git("read-tree", self.source, environment=environment)
        paths = (selected, self.repo / ".gitattributes", self.repo / ".git/index", index)
        before = [path.read_bytes() for path in paths]
        descriptors, closed, children, timers, output, events, fallback = [], [], [], [], [], [], []
        entered, release = threading.Event(), threading.Event()
        thrown = False

        class ReaderOS:
            def __getattr__(self, name):
                return getattr(os, name)

            def open(self, path, flags, *args, **kwargs):
                descriptor = os.open(path, flags, *args, **kwargs)
                owner.assertEqual(Path(path), selected)
                descriptors.append(descriptor)
                return descriptor

            def read(self, descriptor, size):
                nonlocal thrown
                if primary is not None and not thrown:
                    owner.assertIn(descriptor, descriptors)
                    owner.assertEqual(b"".join(output), canonical)
                    thrown = True
                    events.append("source-read-fault")
                    raise primary
                return os.read(descriptor, size)

            def close(self, descriptor):
                owner.assertIn(descriptor, descriptors)
                owner.assertNotIn(descriptor, closed)
                os.close(descriptor)
                closed.append(descriptor)

        class Stdout:
            def __init__(self, real):
                self.real = real

            def read(self, size):
                data = self.real.read(size)
                output.append(data)
                return data

            def close(self):
                self.real.close()
                events.append("stdout-closed")
                if secondary is not None:
                    raise secondary

        def spawn(command, **kwargs):
            owner.assertIn("cat-file", command)
            child = subprocess.Popen(command, **kwargs)
            children.append(child)
            child.stdout = Stdout(child.stdout)
            events.append("child-started")
            return child

        def event():
            if setup_error is not None:
                raise setup_error
            return threading.Event()

        def timer(delay, callback):
            def held():
                entered.set()
                if not release.wait(15):
                    raise AssertionError("owned timer release timeout")
                callback()
            result = threading.Timer(0 if inflight else delay, held if inflight else callback)
            timers.append(result)
            real_start, real_cancel = result.start, result.cancel

            def start():
                real_start()
                if inflight:
                    owner.assertTrue(entered.wait(2), "owned timer did not enter")

            def cancel():
                real_cancel()
                events.append("timer-cancelled")
                if cancel_error is not None:
                    raise cancel_error
            result.start, result.cancel = start, cancel
            return result

        caught = None
        try:
            with mock.patch.object(CONTENT, "os", ReaderOS()), \
                    mock.patch.object(CONTENT, "subprocess", SimpleNamespace(
                        **{**vars(subprocess), "Popen": spawn})), \
                    mock.patch.object(CONTENT, "threading", SimpleNamespace(
                        **{**vars(threading), "Event": event, "Timer": timer})):
                try:
                    CONTENT.validate_ident(self.repo, self.source, env=environment, filter_config=())
                except BaseException as error:
                    caught = error
        finally:
            observed = {"child_status": [p.returncode for p in children],
                        "stdout_closed": [p.stdout.real.closed for p in children],
                        "timer_alive": [t.is_alive() for t in timers]}
            release.set()
            for t in timers:
                t.cancel = threading.Timer.cancel.__get__(t)
                t.cancel()
                if t.ident is not None:
                    t.join(timeout=2)
                    owner.assertFalse(t.is_alive(), "proof could not retire its owned timer")
            for child in children:
                if child.returncode is None:
                    fallback.append("child-wait")
                    child.wait(timeout=5)
                if not child.stdout.real.closed:
                    fallback.append("stdout-close")
                    child.stdout.real.close()
            for descriptor in descriptors:
                if descriptor not in closed:
                    fallback.append("source-close")
                    os.close(descriptor)
        owner.assertEqual(len(children), 1)
        owner.assertEqual(closed, descriptors)
        for descriptor in descriptors:
            with owner.assertRaises(OSError) as invalid:
                os.fstat(descriptor)
            owner.assertEqual(invalid.exception.errno, errno.EBADF)
        owner.assertEqual([path.read_bytes() for path in paths], before)
        if setup_error is None:
            owner.assertEqual(b"".join(output), canonical)
        owner.assertTrue(all(child.returncode is not None for child in children))
        print({"checkout_cleanup": {"primary": type(primary).__name__ if primary else None,
              "caught": type(caught).__name__ if caught else None,
              "caught_is_primary": caught is primary if primary else None,
              "observed": observed, "events": events, "fallback": fallback}})
        return caught, observed, events, fallback

    def test_checkout_read_failure_survives_stdout_close_failure(self):
        canonical = self.canonical_checkout()
        for primary in (OSError(errno.EIO, "source read"), KeyboardInterrupt("source read")):
            with self.subTest(primary=type(primary).__name__):
                caught, observed, _, fallback = self.checkout_cleanup(
                    canonical, primary=primary, secondary=OSError(errno.EIO, "stdout close"))
                self.assertIs(caught, primary)
                self.assertEqual(observed["stdout_closed"], [True])
                self.assertEqual(fallback, [])

    def test_checkout_single_faults_and_cancel_failure_keep_cleanup_visible(self):
        canonical = self.canonical_checkout()
        for label in ("healthy", "read", "close", "cancel"):
            with self.subTest(phase=label):
                fault = OSError(errno.EIO, label)
                caught, observed, events, fallback = self.checkout_cleanup(
                    canonical, primary=fault if label == "read" else None,
                    secondary=fault if label == "close" else None,
                    cancel_error=fault if label == "cancel" else None)
                self.assertIs(caught, None if label == "healthy" else fault)
                self.assertEqual(observed["stdout_closed"], [True])
                self.assertIn("stdout-closed", events)
                self.assertEqual(fallback, [])

    def test_checkout_setup_failure_retires_the_returned_child(self):
        canonical = self.canonical_checkout()
        fault = OSError(errno.EIO, "event setup after Popen")
        caught, observed, _, fallback = self.checkout_cleanup(canonical, setup_error=fault)
        self.assertIs(caught, fault)
        self.assertEqual(observed["stdout_closed"], [True])
        self.assertEqual(fallback, [])

    def test_checkout_inflight_timer_cannot_supply_success(self):
        canonical = self.canonical_checkout()
        caught, observed, _, fallback = self.checkout_cleanup(canonical, inflight=True)
        self.assertIsInstance(caught, ValueError)
        self.assertIn("timer", str(caught))
        self.assertEqual(observed["timer_alive"], [True])
        self.assertEqual(observed["stdout_closed"], [True])
        self.assertEqual(fallback, [])

    def test_raw_and_canonical_ident_forms_are_admitted_with_lf_and_crlf(self):
        raw = b"# $Id$\nsource body\n"
        for ending in ("lf", "crlf"):
            with self.subTest(ending=ending):
                path = "probe-" + ending
                blob = self.commit(path, raw, f"{path} ident text eol={ending}\n")
                self.observe()  # The exact committed bytes remain an admitted form.
                canonical = self.git("cat-file", "--filters", "--path=" + path, blob)
                self.assertIn(b"$Id: " + blob.encode() + b" $", canonical)
                self.assertEqual(b"\r\n" in canonical, ending == "crlf")
                (self.repo / path).write_bytes(canonical)
                self.observe()
                self.assertEqual((self.repo / path).read_bytes(), canonical)

    def test_noncanonical_ident_fields_are_refused_even_when_fresh_git_is_clean(self):
        raw = b"#!/bin/sh\n# $Id$\nprintf harmless\n"
        path = "probe"
        blob = self.commit(path, raw, "probe ident -text\n")
        for replacement in (b"$Id: forged $", b"$Id: ; printf hidden # $", b"$Id: \x00payload $"):
            with self.subTest(replacement=replacement):
                altered = raw.replace(b"$Id$", replacement)
                (self.repo / path).write_bytes(altered)
                self.assert_git_clean(path)
                self.assertEqual(self.git("hash-object", "--path=" + path, path).decode().strip(), blob)
                with self.assertRaises(ValueError):
                    self.observe()
                self.assertEqual((self.repo / path).read_bytes(), altered)

    def assert_multiple_ident_tokens(self, path, blob):
        canonical = self.git("cat-file", "--filters", "--path=" + path, blob)
        self.assertEqual(canonical.count(b"$Id: " + blob.encode() + b" $"), 2)
        self.assertIn(b"\xff", canonical)
        selected = self.repo / path
        selected.write_bytes(canonical)
        self.observe()
        altered = canonical.replace(blob.encode(), b"0" * len(blob), 1)
        selected.write_bytes(altered)
        self.assert_git_clean(path)
        with self.assertRaises(ValueError):
            self.observe()
        self.assertEqual(selected.read_bytes(), altered)
        selected.write_bytes(canonical)

    def test_multiple_ident_tokens_and_non_utf8_content_are_checked_as_bytes(self):
        path = "nested/-odd:é\nname"
        raw = b"$Id$ / $Id$ / non-UTF8 content: \xff\n"
        blob = self.commit(path, raw, "* ident -text\n")
        self.assert_multiple_ident_tokens(path, blob)

    def test_raw_git_ident_path_requires_its_exact_checkout_entry(self):
        name = b"nested/-odd:\xff\nname"
        path = os.fsdecode(name)
        raw = b"$Id$ / $Id$ / non-UTF8 content: \xff\n"
        selected = self.repo / path
        selected.parent.mkdir()
        (self.repo / ".gitattributes").write_text("* ident -text\n")
        self.git("add", ".gitattributes")
        blob = self.git("hash-object", "-w", "--stdin", data=raw).strip()
        self.git("update-index", "-z", "--index-info", data=b"100644 " + blob + b"\t" + name + b"\0")
        self.git("commit", "--quiet", "-m", "raw ident path")
        self.source = self.git("rev-parse", "HEAD").decode().strip()
        entries = self.git("ls-tree", "-r", "-z", self.source).split(b"\0")
        self.assertIn(b"100644 blob " + blob + b"\t" + name, entries)
        self.assertEqual(self.git("check-attr", "-z", "--stdin", "ident", data=name + b"\0"),
                         name + b"\0ident\0set\0")
        with self.assertRaises(OSError) as missing:
            self.observe()
        self.assertIn(missing.exception.errno, (errno.ENOENT, errno.EILSEQ))
        self.assertEqual(os.fsencode(missing.exception.filename), os.fsencode(self.repo) + b"/" + name)

        try:
            selected.write_bytes(raw)
        except OSError as error:
            # APFS can store the raw name in Git, while refusing it as a filesystem leaf.
            if error.errno != errno.EILSEQ:
                raise
        else:
            self.observe()
            self.assert_multiple_ident_tokens(path, blob.decode("ascii"))

    def test_smudge_only_driver_is_disabled_during_canonical_comparison(self):
        path = "probe"
        blob = self.commit(path, b"$Id$\n", "probe ident -text filter=Smudge.Only\n")
        canonical = self.git("cat-file", "--filters", "--path=" + path, blob)
        (self.repo / path).write_bytes(canonical)
        witness = self.root / "smudge-called"
        callback = self.root / "smudge.py"
        callback.write_text("from pathlib import Path\nimport sys\n"
                            f"Path({str(witness)!r}).write_bytes(b'called')\n"
                            "sys.stdout.buffer.write(sys.stdin.buffer.read())\n")
        self.git("config", "filter.Smudge.Only.smudge",
                 shlex.join([sys.executable, "-I", "-B", str(callback)]))
        self.git("config", "filter.Smudge.Only.required", "true")
        self.assertEqual(self.git("cat-file", "--filters", "--path=" + path, blob), canonical)
        self.assertTrue(witness.is_file(), "CONTROL: the configured smudge-only callback runs")
        witness.unlink()
        suppression = ("-c", "filter.Smudge.Only.clean=", "-c", "filter.Smudge.Only.smudge=",
                       "-c", "filter.Smudge.Only.process=", "-c", "filter.Smudge.Only.required=false")
        self.observe(filter_config=suppression)
        self.assertFalse(witness.exists())
        self.assertEqual((self.repo / path).read_bytes(), canonical)

    def test_ident_paths_refuse_nonregular_leaves_and_symlinked_parents(self):
        path = "nested/probe"
        raw = b"$Id$\n"
        self.commit(path, raw, "nested/probe ident -text\n")
        selected = self.repo / path
        external = self.root / "external"
        external.write_bytes(raw)
        for kind in ("leaf-symlink", "leaf-directory", "parent-symlink"):
            with self.subTest(kind=kind):
                if kind == "parent-symlink":
                    moved = self.root / "moved-parent"
                    selected.parent.rename(moved)
                    selected.parent.symlink_to(moved, target_is_directory=True)
                else:
                    selected.unlink()
                    if kind == "leaf-symlink":
                        selected.symlink_to(external)
                    else:
                        selected.mkdir()
                try:
                    with self.assertRaises((ValueError, OSError)):
                        self.observe()
                    self.assertEqual(external.read_bytes(), raw)
                finally:
                    if kind == "parent-symlink":
                        selected.parent.unlink()
                        moved.rename(selected.parent)
                    else:
                        selected.rmdir() if selected.is_dir() and not selected.is_symlink() else selected.unlink()
                        selected.write_bytes(raw)

    def test_ident_content_larger_than_one_mebibyte_has_no_small_blob_cutoff(self):
        path = "large"
        raw = b"large source line\n" * 70000 + b"$Id$\n"
        self.assertGreater(len(raw), 1024 * 1024)
        blob = self.commit(path, raw, "large ident -text\n")
        self.observe()
        canonical = self.git("cat-file", "--filters", "--path=" + path, blob)
        (self.repo / path).write_bytes(canonical)
        self.observe()
        altered = canonical.replace(blob.encode(), b"0" * len(blob))
        (self.repo / path).write_bytes(altered)
        self.assert_git_clean(path)
        with self.assertRaises(ValueError):
            self.observe()
        self.assertEqual((self.repo / path).read_bytes(), altered)


if __name__ == "__main__":
    unittest.main()

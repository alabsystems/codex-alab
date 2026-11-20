#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Prune real scratch directories while recoverable session records hold custody."""

import errno
import fcntl
import json
import os
from pathlib import Path
import select
import shutil
import subprocess
import sys
import tempfile
import unittest


PRUNE = Path(__file__).resolve().parents[1] / "prune.py"
CORE_RECORD = PRUNE.parents[1] / "codex-rs/alab-record-contracts/fixtures/session-v1-core.json"


class PruneCustodyTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory(prefix="alab-prune-custody-")
        self.addCleanup(scratch.cleanup)
        self.home = Path(scratch.name).resolve()
        git_program = shutil.which("git")
        self.assertIsNotNone(git_program, "Git is required for prune-custody fixtures")
        self.environment = {"PATH": str(Path(git_program).parent) + ":/usr/bin:/bin",
                            "HOME": str(self.home)}
        subprocess.run(["git", "init", "-q", str(self.home)], check=True)
        self.root = self.home / "artifacts with spaces/codex"
        self.prefix = self.home / "pkg"
        self.codex_home = self.home / "codex home"
        self.sessions = self.codex_home / "alab/sessions"
        self.sessions.mkdir(parents=True)
        (self.prefix / "links").mkdir(parents=True)
        (self.prefix / "store.lock").touch(mode=0o600)
        self.old = self.package("old pinned", "2026-10-20T00:00:00Z")
        self.unused = self.package("old unused", "2026-10-20T01:00:00Z")
        self.new = self.package("new selected", "2026-10-21T00:00:00Z")
        (self.prefix / "links/codex").write_text(
            'schema = 1\nprogram = "codex"\n'
            f"checkout = {json.dumps(str(self.new))}\nbins = [\"bin/codex\"]\n"
        )

    def package(self, name, built_at):
        package = self.root / name
        (package / "bin").mkdir(parents=True)
        # No Mach-O is copied or executed; only the record and package paths matter.
        (package / "bin/codex").write_bytes(b"")
        (package / "alab-build.json").write_text(json.dumps({"built_at": built_at}))
        return package

    def record(self, field):
        record = {
            "v": 1, "pid": 123, "procStart": None, "identity": None,
            "sessionId": "019a0000-0000-7000-8000-000000000001",
            "displayedThreadId": None, "cwd": str(self.home), "version": "test",
            "exe": None, "launchArgv0": None, "execGeneration": 0,
            "resumeArgv": None, "env": {}, "status": "busy", "exit": None,
            "state": "working", "blockedKind": None, "draftEmpty": True,
            "lastHumanInputAt": None, "autopilot": "active", "claim": None,
            "statusUpdatedAt": 1000, "updatedAt": 1000,
            "kind": "interactive", "entrypoint": "cli", "carriedOn": 0,
        }
        path = str(self.old / "bin/codex")
        record[field] = [path, "resume", record["sessionId"]] if field == "resumeArgv" else path
        return json.dumps(record).encode()

    def prune(self, *, rename_after_listing=False, marker_race=None, marker_error=None, extra=()):
        command = [sys.executable, "-I", str(PRUNE)]
        if rename_after_listing:
            # Interpose the writer's actual custody rename at the census/read boundary,
            # rather than relying on scheduling or a wall-clock race.
            command = [sys.executable, "-I", "-c", """
import os, runpy, sys
script, sessions = sys.argv[1:3]
sys.argv = [script, *sys.argv[3:]]
listed = os.listdir
def listdir(path):
    names = listed(path)
    if os.fspath(path) == sessions:
        os.rename(os.path.join(sessions, "123.json"),
                  os.path.join(sessions, "123.json.interrupted.tmp"))
    return names
os.listdir = listdir
runpy.run_path(script, run_name="__main__")
""", str(PRUNE), str(self.sessions)]
        if marker_race is not None:
            # Replace only this owned marker at the actual reader boundary. The
            # store lock still excludes cooperating atpkg writers; this models
            # a local replacement that does not participate in that protocol.
            command = [sys.executable, "-I", "-c", """
import builtins, os, runpy, sys
script, marker, replacement, transition = sys.argv[1:5]
sys.argv = [script, *sys.argv[5:]]
plain_open, plain_os_open = builtins.open, os.open
plain_lstat, plain_fstat = os.lstat, os.fstat
changed = False
opened = set()
def replace():
    global changed
    if changed:
        return
    changed = True
    if transition == "grow":
        with plain_open(replacement, "rb") as source, plain_open(marker, "wb") as target:
            target.write(source.read())
        return
    os.unlink(marker)
    if transition == "symlink":
        os.symlink(replacement, marker)
    elif transition == "fifo":
        os.mkfifo(marker)
    else:
        with plain_open(replacement, "rb") as source, plain_open(marker, "wb") as target:
            target.write(source.read())
def file_open(path, *args, **kwargs):
    if path == marker and transition != "grow":
        replace()
    return plain_open(path, *args, **kwargs)
def descriptor_open(path, *args, **kwargs):
    if path == marker and transition != "grow":
        replace()
    descriptor = plain_os_open(path, *args, **kwargs)
    if path == marker:
        opened.add(descriptor)
    return descriptor
def lstat(path, *args, **kwargs):
    value = plain_lstat(path, *args, **kwargs)
    if path == marker and transition == "grow" and sys._getframe(1).f_code.co_name == "small_text":
        replace()
    return value
def fstat(descriptor):
    value = plain_fstat(descriptor)
    if descriptor in opened and transition == "grow" and sys._getframe(1).f_code.co_name == "small_text":
        replace()
    return value
builtins.open, os.open, os.lstat, os.fstat = file_open, descriptor_open, lstat, fstat
runpy.run_path(script, run_name="__main__")
""", str(PRUNE), *(str(value) for value in marker_race)]
        if marker_error is not None:
            # A controlled observation keeps both EACCES and EIO portable across
            # effective identities; it is distinct from a real permission probe.
            command = [sys.executable, "-I", "-c", """
import os, runpy, sys
script, marker, number = sys.argv[1:4]
sys.argv = [script, *sys.argv[4:]]
plain_lstat = os.lstat
def lstat(path, *args, **kwargs):
    if os.fspath(path) == marker:
        raise OSError(int(number), "controlled marker lstat observation", marker)
    return plain_lstat(path, *args, **kwargs)
os.lstat = lstat
runpy.run_path(script, run_name="__main__")
""", str(PRUNE), str(self.prefix / "links/codex"), str(marker_error)]
        try:
            done = subprocess.run(
                [*command, "--root", str(self.root),
                 "--keep", "1", "--prefix", str(self.prefix),
                 "--codex-home", str(self.codex_home), "--repo", str(self.home), *extra],
                env=self.environment,
                capture_output=True, text=True,
                timeout=2 if marker_race is not None and marker_race[2] == "fifo" else 60,
            )
        except subprocess.TimeoutExpired:
            self.fail("the bounded owned pruner blocked reading its replacement marker")
        self.assertEqual(done.returncode, 0, done.stderr)
        return done.stderr

    def test_nonobject_manifest_keeps_its_directory_and_allows_collection(self):
        unfinished = self.root / "unfinished"
        unfinished.mkdir()
        (unfinished / "alab-build.json").write_text("null")
        self.prune()
        self.assertTrue(unfinished.is_dir())
        self.assertFalse(self.unused.exists())

    def test_unrankable_manifests_neither_authorize_deletion_nor_take_a_retention_slot(self):
        records = [b"{", b"{}", b'{"built_at":17}', b'{"built_at":"9999-92-87"}',
                   b'{"built_at":"2026-10-20T00:00:00"}', b'{"built_at":"2027-01-01T00:00:00Z","name":"\xff"}']
        unknown = []
        for i, raw in enumerate(records):
            directory = self.root / f"unknown-{i}"
            directory.mkdir()
            (directory / "alab-build.json").write_bytes(raw)
            unknown.append(directory)
        newest = self.package("newest unlinked", "2026-10-22T00:00:00Z")
        self.prune()
        self.assertTrue(newest.is_dir())
        self.assertTrue(all(directory.is_dir() for directory in unknown))
        self.assertFalse(self.unused.exists())

    def test_retention_orders_offset_build_dates_by_the_instant_they_name(self):
        older = self.package("looks later", "2026-10-25T18:00:00+09:00")
        newest = self.package("actually later", "2026-10-25T10:00:00Z")
        self.prune()
        self.assertTrue(newest.is_dir())
        self.assertFalse(older.exists())

    def marker_race_defers(self, transition):
        marker = self.prefix / "links/codex"
        marker.write_text(f"checkout = {json.dumps(str(self.old))}\nbins = [\"bin/codex\"]\n")
        replacement = self.home / "replacement-marker.toml"
        text = f"checkout = {json.dumps(str(self.new))}\nbins = [\"bin/codex\"]\n"
        if transition in ("oversized", "grow"):
            text += "#" + "x" * (64 * 1024)
        replacement.write_text(text)
        said = self.prune(marker_race=(marker, replacement, transition))
        self.assertTrue(self.old.exists() and self.unused.exists() and self.new.exists(), said)
        self.assertIn("link marker", said)
        self.assertIn("stands but cannot be read", said)

    def test_a_preexisting_symlink_marker_defers_all_deletion(self):
        marker = self.prefix / "links/codex"
        target = self.home / "owned-marker.toml"
        target.write_bytes(marker.read_bytes())
        marker.unlink()
        marker.symlink_to(target)
        said = self.prune()
        self.assertTrue(self.old.exists() and self.unused.exists() and self.new.exists(), said)
        self.assertIn("stands but cannot be read", said)

    def test_a_marker_replaced_with_a_symlink_at_open_defers_all_deletion(self):
        self.marker_race_defers("symlink")

    def test_a_marker_replaced_with_an_oversized_file_at_open_defers_all_deletion(self):
        self.marker_race_defers("oversized")

    def test_a_marker_growing_after_its_size_check_defers_all_deletion(self):
        self.marker_race_defers("grow")

    def test_a_marker_replaced_with_a_fifo_at_open_does_not_block_or_delete(self):
        self.marker_race_defers("fifo")

    def old_marker_with_new_shim(self):
        marker = self.prefix / "links/codex"
        marker.write_text('schema = 1\nprogram = "codex"\n'
                          f"checkout = {json.dumps(str(self.old))}\nbins = [\"bin/codex\"]\n")
        shim = self.prefix / "bin/codex"
        shim.parent.mkdir()
        shim.write_text(f"#!/bin/sh\nexec {str(self.new / 'bin/codex')!r} \"$@\"\n")
        return marker, shim

    def test_marker_inspection_errors_defer_deletion_despite_a_readable_shim(self):
        marker, shim = self.old_marker_with_new_shim()
        before = marker.read_bytes(), shim.read_bytes()
        for number in (errno.EACCES, errno.EIO):
            with self.subTest(controlled_errno=number):
                self.restore_packages()
                said = self.prune(marker_error=number)
                self.assertTrue(self.old.exists() and self.unused.exists() and self.new.exists(), said)
                self.assertIn("cannot inspect", said)
                self.assertIn("link marker", said)
                self.assertEqual((marker.read_bytes(), shim.read_bytes()), before)

    def test_readable_marker_keeps_its_old_package_beside_a_newer_shim(self):
        marker, shim = self.old_marker_with_new_shim()
        before = marker.read_bytes(), shim.read_bytes()
        said = self.prune()
        self.assertTrue(self.old.exists() and self.new.exists(), said)
        self.assertFalse(self.unused.exists(), said)
        self.assertEqual((marker.read_bytes(), shim.read_bytes()), before)

    def test_absent_marker_allows_collection_with_a_readable_shim(self):
        marker, shim = self.old_marker_with_new_shim()
        before = shim.read_bytes()
        marker.unlink()
        said = self.prune()
        self.assertFalse(self.old.exists() or self.unused.exists(), said)
        self.assertTrue(self.new.exists(), said)
        self.assertFalse(marker.exists())
        self.assertEqual(shim.read_bytes(), before)

    def restore_packages(self):
        if not self.old.exists():
            self.old = self.package("old pinned", "2026-10-20T00:00:00Z")
        if not self.unused.exists():
            self.unused = self.package("old unused", "2026-10-20T01:00:00Z")

    def core_record(self, **changes):
        record = json.loads(CORE_RECORD.read_text())
        record.update(changes)
        return record

    def prune_record(self, record):
        return self.prune_record_bytes(json.dumps(record).encode())

    def prune_record_bytes(self, payload):
        path = self.sessions / "123.json"
        path.write_bytes(payload)
        said = self.prune()
        self.assertEqual(path.read_bytes(), payload)
        return said

    def assert_record_defers_collection(self, record):
        self.assert_record_bytes_defers_collection(json.dumps(record).encode())

    def assert_record_bytes_defers_collection(self, payload):
        self.restore_packages()
        said = self.prune_record_bytes(payload)
        self.assertTrue(self.old.exists() and self.unused.exists() and self.new.exists(), said)
        self.assertIn("session record", said)
        self.assertIn("stands but cannot be read", said)

    def test_missing_record_core_fields_defer_collection(self):
        for record in ({}, {"v": 1}):
            with self.subTest(record=record):
                self.assert_record_defers_collection(record)
        for field in self.core_record():
            with self.subTest(missing=field):
                record = self.core_record()
                del record[field]
                self.assert_record_defers_collection(record)

    def test_malformed_record_core_values_defer_collection(self):
        invalid = [("v", value) for value in (True, 1.0, 0, 2)]
        invalid += [("pid", value) for value in (None, True, 1.0, -1, 1 << 32)]
        for field in ("execGeneration", "statusUpdatedAt", "updatedAt"):
            invalid += [(field, value) for value in (None, True, 1.0, -1, 1 << 64)]
        for field in ("version", "kind", "entrypoint"):
            invalid += [(field, value) for value in (None, 17, "\ud800")]
        invalid += [("env", value) for value in
                    (None, [], {"key": 17}, {"\ud800": "value"}, {"key": "\ud800"})]
        invalid += [("status", value) for value in (None, [], "future")]
        invalid += [("autopilot", value) for value in (None, {}, "future")]
        invalid += [("draftEmpty", value) for value in (None, 1)]
        for field, value in invalid:
            with self.subTest(field=field, value=value):
                self.assert_record_defers_collection(self.core_record(**{field: value}))

    def test_malformed_consumed_record_paths_defer_collection(self):
        invalid = []
        for field in ("exe", "launchArgv0"):
            invalid += [(field, value) for value in (17, {}, "\ud800")]
        invalid += [("resumeArgv", value) for value in
                    ("not an array", [17], ["relative", None], ["\ud800"])]
        for field, value in invalid:
            with self.subTest(field=field, value=value):
                self.assert_record_defers_collection(self.core_record(**{field: value}))

    def test_recognized_pathless_exec_records_allow_collection(self):
        variants = [
            {},
            {"exe": None, "launchArgv0": None, "resumeArgv": None,
             "status": "idle", "autopilot": "passive"},
            {"resumeArgv": [], "status": "waiting", "autopilot": "active"},
            {"exe": "", "launchArgv0": "relative", "resumeArgv": ["relative"],
             "status": "upgrading"},
            {"futureExtension": {"unknown": [True, 17, None]},
             "assignment": {"legacy": "ignored"}, "status": "exited"},
        ]
        for changes in variants:
            with self.subTest(changes=changes):
                self.restore_packages()
                said = self.prune_record(self.core_record(**changes))
                self.assertFalse(self.old.exists() or self.unused.exists(), said)
                self.assertTrue(self.new.exists(), said)

    def test_recognized_exec_records_keep_their_old_image(self):
        variants = [
            {},
            {"pid": 0, "version": "", "kind": "", "entrypoint": "",
             "execGeneration": 0, "statusUpdatedAt": 0, "updatedAt": 0,
             "draftEmpty": False, "env": {"": ""}},
            {"pid": (1 << 32) - 1, "execGeneration": (1 << 64) - 1,
             "statusUpdatedAt": (1 << 64) - 1, "updatedAt": (1 << 64) - 1},
        ]
        for changes in variants:
            with self.subTest(changes=changes):
                self.restore_packages()
                record = self.core_record(exe=str(self.old / "bin/codex"), **changes)
                said = self.prune_record(record)
                self.assertTrue(self.old.exists() and self.new.exists(), said)
                self.assertFalse(self.unused.exists(), said)

    def duplicate_record_bytes(self, field, first, last, *, escaped=False):
        record = self.core_record(**{field: first})
        name = json.dumps(field)
        if escaped:
            name = '"\\u' + f"{ord(field[0]):04x}" + field[1:] + '"'
        return (json.dumps(record)[:-1] + "," + name + ":" + json.dumps(last) + "}").encode()

    def test_duplicate_root_core_and_path_members_defer_collection(self):
        old_exe = str(self.old / "bin/codex")
        old_argv = [old_exe, "resume", "fixture-thread"]
        fields = self.core_record(exe=old_exe, launchArgv0=old_exe, resumeArgv=old_argv)
        for field, value in fields.items():
            for escaped in (False, True):
                with self.subTest(field=field, identical=True, escaped=escaped):
                    self.assert_record_bytes_defers_collection(
                        self.duplicate_record_bytes(field, value, value, escaped=escaped))
        transitions = [("v", 2, 1), ("exe", old_exe, None), ("exe", old_exe, ""),
                       ("launchArgv0", old_exe, None), ("launchArgv0", old_exe, ""),
                       ("resumeArgv", old_argv, None), ("resumeArgv", old_argv, [])]
        for field, first, last in transitions:
            with self.subTest(field=field, first=first, last=last):
                self.assert_record_bytes_defers_collection(
                    self.duplicate_record_bytes(field, first, last))

    def test_other_record_duplicates_keep_their_existing_collection_behavior(self):
        base = json.dumps(self.core_record())
        old_exe = json.dumps(str(self.old / "bin/codex"))
        nested_env = '{"exe":"first","exe":' + old_exe + ',"v":"first","v":"last"}'
        nested_extension = '{"exe":null,"exe":' + old_exe + ',"v":2,"v":1}'
        variants = [
            ("normal-old-image", json.dumps(self.core_record(
                exe=str(self.old / "bin/codex"))), True),
            ("normal-pathless", base, False),
            ("nested-env", base.replace('"env": {}', '"env": ' + nested_env, 1), False),
            ("nested-extension", base[:-1] + ',"future":' + nested_extension + '}', False),
            ("unknown-root", base[:-1] + ',"future":null,"future":' + nested_extension + '}', False),
        ]
        for name, raw, keeps_old in variants:
            with self.subTest(case=name):
                self.restore_packages()
                said = self.prune_record_bytes(raw.encode())
                self.assertEqual(self.old.exists(), keeps_old, said)
                self.assertFalse(self.unused.exists(), said)
                self.assertTrue(self.new.exists(), said)

    def test_ambiguous_or_unreadable_link_markers_defer_before_any_package_deletion(self):
        marker = self.prefix / "links/codex"
        old = f"checkout = {json.dumps(str(self.old))}\n"
        newest = f"checkout = {json.dumps(str(self.new))}\n"
        variants = [
            ('duplicate-checkout', (old + newest + 'bins = ["bin/codex"]\n').encode()),
            ('invalid-utf8', newest.encode() + b'bins = ["bin/codex"]\n# invalid byte: \xff\n'),
            ('invalid-toml', (newest + 'bins = ["bin/codex"\n').encode()),
            ('string-bins', (newest + 'bins = "bin/codex"\n').encode()),
            ('mixed-bins', (newest + 'bins = ["bin/codex", false]\n').encode()),
            ('empty-bin', (newest + 'bins = [""]\n').encode()),
            ('invalid-schema', ('schema = true\n' + newest + 'bins = ["bin/codex"]\n').encode()),
            ('wrong-program', ('program = "claude"\n' + newest + 'bins = ["bin/codex"]\n').encode()),
        ]
        for name, payload in variants:
            with self.subTest(marker=name):
                self.restore_packages()
                marker.write_bytes(payload)
                said = self.prune()
                self.assertEqual(marker.read_bytes(), payload)
                self.assertTrue(self.old.exists() and self.unused.exists() and self.new.exists(), said)
                self.assertIn('link marker', said)
                self.assertIn('stands but cannot be read', said)

    def test_valid_selected_checkout_markers_keep_their_packages_without_freezing_collection(self):
        marker = self.prefix / "links/codex"
        for name, selected in [('canonical-old', self.old), ('checkout-only-old', self.old),
                               ('canonical-new', self.new)]:
            with self.subTest(marker=name):
                self.restore_packages()
                text = f"checkout = {json.dumps(str(selected))}\n"
                if name != 'checkout-only-old':
                    text = 'schema = 1\nprogram = "codex"\n' + text + 'bins = ["bin/codex"]\n'
                marker.write_text(text)
                said = self.prune()
                self.assertEqual(marker.read_text(), text)
                self.assertEqual(self.old.exists(), selected == self.old, said)
                self.assertFalse(self.unused.exists(), said)
                self.assertTrue(self.new.exists(), said)

    def test_a_sole_canonical_atomic_or_quarantined_record_keeps_its_package(self):
        for name in ["123.json", "123.json.1a2b.tmp", "123.json.interrupted.tmp",
                     "123.json..tmp", "123.json.nonce\npart.tmp"]:
            for field in ["resumeArgv", "exe", "launchArgv0"]:
                with self.subTest(name=name, field=field):
                    self.restore_packages()
                    path = self.sessions / name
                    path.write_bytes(self.record(field))
                    try:
                        said = self.prune()
                        self.assertTrue(self.old.exists(), said)
                        self.assertFalse(self.unused.exists(), said)
                        self.assertTrue(self.new.exists(), said)
                    finally:
                        path.unlink()

    def test_an_existing_empty_canonical_or_temporary_record_fails_closed(self):
        for name in ["123.json", "123.json.interrupted.tmp"]:
            with self.subTest(name=name):
                self.restore_packages()
                path = self.sessions / name
                path.write_bytes(b"")
                try:
                    said = self.prune()
                    self.assertTrue(self.old.exists() and self.unused.exists(), said)
                finally:
                    path.unlink()

    def test_malformed_and_unreadable_records_fail_closed(self):
        for name in ["123.json", "123.json.interrupted.tmp"]:
            for kind in ["malformed", "nonobject", "invalid-utf8", "oversized",
                         "unreadable", "symlink", "fifo", "directory"]:
                with self.subTest(name=name, kind=kind):
                    self.restore_packages()
                    path = self.sessions / name
                    if kind == "directory":
                        path.mkdir()
                    elif kind == "fifo":
                        os.mkfifo(path)
                    elif kind == "symlink":
                        path.symlink_to(self.home / "missing record")
                    else:
                        payloads = {"malformed": b"{broken", "nonobject": b"[]",
                                    "invalid-utf8": b"\xff", "oversized": b" " * (1024 * 1024 + 1),
                                    "unreadable": self.record("resumeArgv")}
                        path.write_bytes(payloads[kind])
                        if kind == "unreadable":
                            path.chmod(0)
                    try:
                        said = self.prune()
                        self.assertTrue(self.old.exists() and self.unused.exists(), said)
                    finally:
                        if kind == "directory":
                            path.rmdir()
                        else:
                            path.unlink()

    def test_nonrecord_junk_does_not_freeze_collection(self):
        for name in ["notes", "123.json.tmp", "123.json.pending", "123.json.x.tmp.extra",
                     "4294967296.json", "4294967296.json.x.tmp", "١٢٣.json"]:
            (self.sessions / name).write_bytes(b"{not a record")
        said = self.prune()
        self.assertFalse(self.old.exists(), said)
        self.assertFalse(self.unused.exists(), said)
        self.assertTrue(self.new.exists(), said)

    def test_a_record_renamed_after_listing_defers_collection_until_the_next_census(self):
        (self.sessions / "123.json").write_bytes(self.record("resumeArgv"))
        said = self.prune(rename_after_listing=True)
        self.assertTrue((self.sessions / "123.json.interrupted.tmp").exists())
        self.assertTrue(self.old.exists() and self.unused.exists(), said)
        # A normal later census recovers the sole renamed witness and still frees junk.
        said = self.prune()
        self.assertTrue(self.old.exists(), said)
        self.assertFalse(self.unused.exists(), said)

    def test_an_inaccessible_sessions_ancestor_fails_closed(self):
        ancestor = self.sessions.parent
        ancestor.chmod(0)
        try:
            with self.assertRaises(PermissionError):
                self.sessions.stat()
            said = self.prune()
            self.assertTrue(self.old.exists() and self.unused.exists(), said)
        finally:
            ancestor.chmod(0o700)

    def test_a_truly_absent_sessions_directory_does_not_freeze_collection(self):
        self.sessions.rmdir()
        self.sessions.parent.rmdir()
        self.codex_home.rmdir()
        said = self.prune()
        self.assertFalse(self.old.exists(), said)
        self.assertFalse(self.unused.exists(), said)
        self.assertTrue(self.new.exists(), said)

    def test_a_missing_special_or_contended_store_lock_defers_all_deletion(self):
        lock = self.prefix / "store.lock"
        lock.unlink()
        for kind in ["missing", "directory", "symlink", "fifo", "held"]:
            with self.subTest(kind=kind):
                if kind == "directory":
                    lock.mkdir()
                elif kind == "symlink":
                    lock.symlink_to(self.home / "not the store lock")
                elif kind == "fifo":
                    os.mkfifo(lock)
                elif kind == "held":
                    lock.touch(mode=0o600)
                try:
                    if kind == "held":
                        with lock.open("rb") as stream:
                            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
                            said = self.prune()
                    else:
                        said = self.prune()
                    self.assertIn("pruning stopped", said)
                    self.assertTrue(self.old.exists() and self.unused.exists(), said)
                finally:
                    if kind == "directory":
                        lock.rmdir()
                    elif kind != "missing":
                        lock.unlink()

    def test_a_manual_selection_holds_the_same_lock_and_is_seen_by_the_next_census(self):
        marker = self.prefix / "links/codex"
        with (self.prefix / "store.lock").open("rb") as stream:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
            marker.write_text(f"checkout = {json.dumps(str(self.old))}\n")
            said = self.prune()
            self.assertTrue(self.old.exists() and self.unused.exists(), said)
        said = self.prune()
        self.assertTrue(self.old.exists(), said)
        self.assertFalse(self.unused.exists(), said)

    def test_a_selector_cannot_change_the_link_between_its_snapshot_and_deletion(self):
        # Pause after the real link snapshot, exactly where a recheck-only fix races.
        # An independent writer using atpkg's BSD flock protocol must be excluded.
        child = subprocess.Popen([
            sys.executable, "-I", "-c", """
import runpy, sys
script = sys.argv[1]
ns = runpy.run_path(script)
g = ns["main"].__globals__
linked = g["linked"]
def snapshot(prefix):
    paths = linked(prefix)
    print("snapshot", flush=True)
    assert sys.stdin.readline() == "continue\\n"
    return paths
g["linked"] = snapshot
sys.argv = [script, *sys.argv[2:]]
sys.exit(ns["main"]())
""", str(PRUNE), "--root", str(self.root), "--keep", "1", "--prefix", str(self.prefix),
            "--codex-home", str(self.codex_home), "--repo", str(self.home)],
            env=self.environment,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        try:
            ready, _, _ = select.select([child.stdout], [], [], 60)
            self.assertTrue(ready, "pruner snapshot hang detector")
            self.assertEqual(child.stdout.readline(), "snapshot\n")
            before = (self.prefix / "links/codex").read_bytes()
            writer = subprocess.run([sys.executable, "-I", "-c", """
import fcntl, sys
with open(sys.argv[1], "rb") as stream:
    try:
        fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        sys.exit(75)
    raise AssertionError("selector acquired the store lock inside the prune census")
""", str(self.prefix / "store.lock")], capture_output=True, text=True, timeout=60)
            self.assertEqual(writer.returncode, 75, writer.stderr)
            self.assertEqual((self.prefix / "links/codex").read_bytes(), before)
            _, stderr = child.communicate("continue\n", timeout=60)
            self.assertEqual(child.returncode, 0, stderr)
            self.assertFalse(self.old.exists() or self.unused.exists(), stderr)
            self.assertTrue(self.new.exists(), stderr)
            # The guard is released at completion; it was not a permanent claim.
            with (self.prefix / "store.lock").open("rb") as stream:
                fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        finally:
            if child.poll() is None:
                child.kill()
            child.communicate(timeout=60)

    def test_clone_policy_preserves_all_packages_and_dry_run_never_deletes(self):
        said = self.prune(extra=("--dry-run",))
        self.assertIn("would prune", said)
        self.assertTrue(self.old.exists() and self.unused.exists(), said)
        (self.home / ".git/alab-preserve-builds").write_text("keep the user's builds\n")
        said = self.prune()
        self.assertIn("cleanup policy preserves all packages", said)
        self.assertTrue(self.old.exists() and self.unused.exists(), said)


if __name__ == "__main__":
    unittest.main()

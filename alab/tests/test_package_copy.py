#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Retained fixtures check the actual package copy boundary and raced inputs."""

import importlib.util
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]


class PackageCopyTests(unittest.TestCase):
    def setUp(self):
        self.home = Path(tempfile.mkdtemp(prefix="alab-package-copy-")).resolve()
        print(f"retained copy fixture: {self.home}", file=sys.stderr)
        self.source = self.home / "source with spaces"
        self.source.mkdir(mode=0o700)
        self.destination = self.home / "staged with spaces"
        self.destination.mkdir(mode=0o700)
        self.decoy = self.home / "decoy"
        self.decoy.mkdir()
        (self.decoy / "sentinel").write_bytes(b"fixture bytes outside the input tree")

    def helper(self):
        spec = importlib.util.spec_from_file_location("copy_package", ROOT / "alab/copy_package.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def actual_boundary(self, cleanup=False):
        command = '''set -euo pipefail
. "$1/alab/package.sh"
python="$2"
package_cleanup_allowed() { return 1; }
rm() { return 93; }
if [[ "$5" == 1 ]]; then
  package_cleanup_allowed() { return 0; }
  fixture_rm_calls="$6"
  rm() { printf '%s\\n' "$@" >>"$fixture_rm_calls"; }
  wt=unused-fixture-worktree
  scratch_paths=("$3" "$4" "$7")
  trap package_cleanup EXIT
fi
package_copy_out "$3" "$4"
'''
        result = subprocess.run(["/bin/bash", "-c", command, "copy-fixture", str(ROOT),
                                 sys.executable, str(self.source), str(self.destination),
                                 "1" if cleanup else "0", str(self.home / "rm-calls"),
                                 str(self.home / "other scratch")],
                                capture_output=True, text=True, timeout=30)
        (self.home / "stdout").write_text(result.stdout)
        (self.home / "stderr").write_text(result.stderr)
        return result

    def test_actual_boundary_refuses_a_root_directory_link(self):
        original = self.home / "original source"
        self.source.rename(original)
        self.source.symlink_to(self.decoy, target_is_directory=True)
        result = self.actual_boundary()
        self.assertNotEqual(result.returncode, 0, "a root link must not be copied through")
        self.assertEqual(list(self.destination.iterdir()), [])
        self.assertTrue(self.source.is_symlink())
        self.assertTrue(original.is_dir())
        self.assertEqual((self.decoy / "sentinel").read_bytes(),
                         b"fixture bytes outside the input tree")

    def test_failed_copy_survives_normal_exit_cleanup(self):
        (self.source / "linked entry").symlink_to(self.decoy / "sentinel")
        result = self.actual_boundary(cleanup=True)
        self.assertNotEqual(result.returncode, 0)
        calls = (self.home / "rm-calls").read_text().splitlines()
        self.assertNotIn(str(self.source), calls)
        self.assertNotIn(str(self.destination), calls)
        self.assertIn(str(self.home / "other scratch"), calls)
        self.assertTrue((self.source / "linked entry").is_symlink())
        self.assertTrue(self.destination.is_dir())

    def test_actual_boundary_copies_binary_bytes_and_execute_modes(self):
        (self.source / "bin").mkdir(mode=0o755)
        executable = self.source / "bin/codex"
        executable.write_bytes(bytes(range(256)) * 5000)
        executable.chmod(0o755)
        (self.source / "NOTICE").write_bytes(b"notice\n")
        result = self.actual_boundary()
        self.assertEqual(result.returncode, 0, result.stderr)
        copied = self.destination / "bin/codex"
        self.assertEqual(copied.read_bytes(), executable.read_bytes())
        self.assertEqual(stat.S_IMODE(copied.stat().st_mode), 0o755)
        self.assertEqual(stat.S_IMODE(self.destination.stat().st_mode), 0o755)
        self.assertEqual((self.destination / "NOTICE").read_bytes(), b"notice\n")
        self.assertTrue(executable.is_file(), "preservation keeps the source")

    def test_descendant_links_and_special_files_are_refused(self):
        for kind in ("file-link", "directory-link", "fifo", "writable", "special-mode"):
            with self.subTest(kind=kind):
                source = self.home / kind
                source.mkdir()
                destination = self.home / (kind + "-copy")
                destination.mkdir()
                entry = source / "entry"
                if kind == "file-link":
                    entry.symlink_to(self.decoy / "sentinel")
                elif kind == "directory-link":
                    entry.symlink_to(self.decoy, target_is_directory=True)
                elif kind == "fifo":
                    os.mkfifo(entry)
                else:
                    entry.write_bytes(b"unsafe mode fixture")
                    # macOS may clear setuid in a sandbox. Sticky is still an
                    # unsafe package mode and remains observable on this fixture.
                    mode = 0o666 if kind == "writable" else 0o1755
                    entry.chmod(mode)
                    self.assertEqual(stat.S_IMODE(entry.stat().st_mode), mode)
                with self.assertRaises((OSError, ValueError)):
                    self.helper().copy_package(source, destination)
                self.assertEqual(list(destination.iterdir()), [])
                self.assertTrue(entry.exists())

    def test_a_link_substituted_between_stat_and_open_is_refused(self):
        entry = self.source / "entry"
        entry.write_bytes(b"original bytes")
        helper = self.helper()
        real_open = os.open
        substituted = []

        def replace_before_open(path, flags, mode=0o777, *, dir_fd=None):
            if path == "entry" and dir_fd is not None and not substituted:
                entry.rename(self.home / "saved entry")
                entry.symlink_to(self.decoy / "sentinel")
                substituted.append(True)
            return real_open(path, flags, mode, dir_fd=dir_fd)

        with mock.patch.object(helper.os, "open", side_effect=replace_before_open):
            with self.assertRaises(OSError):
                helper.copy_package(self.source, self.destination)
        self.assertEqual(substituted, [True])
        self.assertEqual(list(self.destination.iterdir()), [])
        self.assertEqual((self.home / "saved entry").read_bytes(), b"original bytes")

    def test_an_open_source_root_cannot_be_redirected_by_renaming_it(self):
        (self.source / "original").write_bytes(b"original directory bytes")
        helper = self.helper()
        real_scandir = os.scandir
        moved = []

        def move_after_open(fd):
            if not moved:
                self.source.rename(self.home / "saved root")
                self.source.symlink_to(self.decoy, target_is_directory=True)
                moved.append(True)
            return real_scandir(fd)

        with mock.patch.object(helper.os, "scandir", side_effect=move_after_open):
            helper.copy_package(self.source, self.destination)
        self.assertEqual(moved, [True])
        self.assertEqual(sorted(p.name for p in self.destination.iterdir()), ["original"])
        self.assertEqual((self.destination / "original").read_bytes(), b"original directory bytes")

    def test_existing_or_overlapping_staging_is_refused(self):
        helper = self.helper()
        (self.destination / "keep").write_bytes(b"existing staging")
        with self.assertRaises(ValueError):
            helper.copy_package(self.source, self.destination)
        self.assertEqual((self.destination / "keep").read_bytes(), b"existing staging")
        with self.assertRaises(ValueError):
            helper.copy_package(self.source, self.source)
        nested = self.source / "nested staging"
        nested.mkdir()
        with self.assertRaises(ValueError):
            helper.copy_package(self.source, nested)
        self.assertEqual(list(nested.iterdir()), [])

    def test_continuously_growing_file_is_refused_after_its_initial_size(self):
        entry = self.source / "entry"
        entry.write_bytes(b"original")
        helper = self.helper()
        real_read = os.read
        reads = []

        def append_before_read(fd, size):
            with entry.open("ab") as output:
                output.write(b"growth")
            reads.append(size)
            return real_read(fd, size)

        with mock.patch.object(helper.os, "read", side_effect=append_before_read):
            with self.assertRaisesRegex(ValueError, "changed"):
                helper.copy_package(self.source, self.destination)
        self.assertEqual(reads, [len(b"original"), 1])
        self.assertEqual((self.destination / "entry").read_bytes(), b"original")


if __name__ == "__main__":
    unittest.main()

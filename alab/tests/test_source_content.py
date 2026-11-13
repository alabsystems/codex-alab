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
import unittest


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

    def git(self, *args, environment=None):
        done = subprocess.run(["git", "-C", str(self.repo), *args], capture_output=True,
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

    def test_multiple_ident_tokens_and_non_utf8_path_are_checked_as_bytes(self):
        raw = b"$Id$ / $Id$ / non-UTF8 content: \xff\n"
        for path in ("nested/-odd:é\nname", os.fsdecode(b"nested/-odd:\xff\nname")):
            with self.subTest(path=repr(path)):
                try:
                    blob = self.commit(path, raw, "* ident -text\n")
                except OSError as error:
                    if error.errno == errno.EILSEQ and "\udcff" in path:
                        self.skipTest("filesystem cannot represent a non-UTF8 filename")
                    raise
                canonical = self.git("cat-file", "--filters", "--path=" + path, blob)
                self.assertEqual(canonical.count(b"$Id: " + blob.encode() + b" $"), 2)
                self.assertIn(b"\xff", canonical)
                (self.repo / path).write_bytes(canonical)
                self.observe()
                altered = canonical.replace(blob.encode(), b"0" * len(blob), 1)
                (self.repo / path).write_bytes(altered)
                self.assert_git_clean(path)
                with self.assertRaises(ValueError):
                    self.observe()
                self.assertEqual((self.repo / path).read_bytes(), altered)
                (self.repo / path).write_bytes(canonical)

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

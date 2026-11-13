#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Local publication advances only its selected artifact lane through guarded atpkg."""

import importlib.util
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ENTRY = "#!/bin/sh\n'''exec' " + shlex.quote(sys.executable) + ' -I "$0" "$@"\n\' \'\'\'\n'
SPEC = importlib.util.spec_from_file_location("alab_promote", ROOT / "alab/promote.py")
PROMOTE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PROMOTE)


class PromotionTests(unittest.TestCase):
    def setUp(self):
        self.home = Path(tempfile.mkdtemp(prefix="alab-promote-")).resolve()
        self.keep_scratch = False

        def cleanup():
            if self.keep_scratch:
                print(f"retaining fixture with unconfirmed child EOF: {self.home}",
                      file=sys.stderr)
            else:
                shutil.rmtree(self.home)

        self.addCleanup(cleanup)
        self.repo = self.home / "repo"
        self.repo.mkdir()
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "Package test")
        self.git("config", "user.email", "package@example.invalid")
        self.git("config", "commit.gpgsign", "false")
        self.git("commit", "-q", "--allow-empty", "-m", "first")
        self.old_source = self.git("rev-parse", "HEAD")
        self.git("commit", "-q", "--allow-empty", "-m", "second")
        self.source = self.git("rev-parse", "HEAD")
        self.git("update-ref", "refs/remotes/origin/main", self.source)
        self.root = self.home / "output with spaces/codex"
        self.root.mkdir(parents=True)
        self.prefix = self.home / "pkg with spaces"
        (self.prefix / "links").mkdir(parents=True)
        self.old = self.package(self.old_source, 1)
        self.new = self.package(self.source, 2)
        self.select(self.old)
        self.calls = []

    def git(self, *args):
        done = subprocess.run(["git", "-C", str(self.repo), *args], capture_output=True,
                              text=True, timeout=60)
        self.assertEqual(done.returncode, 0, done.stderr)
        return done.stdout.strip()

    def package(self, source, count, parent=None, line="0.164.0", baseline=None):
        named = f"codex.{baseline}." if baseline else ""
        version = f"{line}-dev.{count}+{named}g{source[:9]}"
        path = (parent or self.root) / version
        (path / "bin").mkdir(parents=True)
        (path / "codex-resources").mkdir()
        (path / "codex-path").mkdir()
        for relative in ("bin/codex", "bin/codex-code-mode-host", "codex-path/rg"):
            executable = path / relative
            executable.write_bytes(b"fixture, never executed")
            executable.chmod(0o755)
        (path / "codex-package.json").write_text(json.dumps({
            "layoutVersion": 1, "version": version, "target": "aarch64-apple-darwin",
            "variant": "codex", "entrypoint": "bin/codex",
            "resourcesDir": "codex-resources", "pathDir": "codex-path"}))
        (path / "alab-build.json").write_text(json.dumps({
            "source": source, "version": version, "target": "aarch64-apple-darwin",
            "compiler": {"admitted": True}}))
        return path

    def select(self, path):
        self.marker = self.prefix / "links/codex"
        self.marker.write_text(f'schema = 1\nprogram = "codex"\ncheckout = '
                               f'{json.dumps(str(path))}\nbins = ["bin/codex"]\nlinked_at = ""\n')
        self.marker.chmod(0o600)

    def link(self, args, **kwargs):
        self.calls.append(args)
        if args[-1] == "--help":
            return subprocess.CompletedProcess(args, 0,
                                               "link --if-checkout <path> --if-prefix <path> "
                                               "--wait-lock <secs>", "")
        return subprocess.CompletedProcess(args, 0, "selected", "")

    def promote(self, candidate=None, **kwargs):
        return PROMOTE.promote(self.repo, self.root, self.prefix, candidate or self.new,
                               self.source, aterm="/owned host/aterm", run=self.link, **kwargs)

    def test_a_valid_forward_package_for_another_requested_source_is_not_selected(self):
        # The artifact's version and manifests agree and it advances the current
        # selection, but it does not contain the source the caller requested.
        self.git("commit", "-q", "--allow-empty", "-m", "required fix")
        required = self.git("rev-parse", "HEAD")
        self.git("update-ref", "refs/remotes/origin/main", required)
        selected, _ = PROMOTE.promote(self.repo, self.root, self.prefix, self.new,
                                     required, run=self.link)
        self.assertFalse(selected)
        self.assertEqual(self.calls, [])
        # The same intact artifact is eligible when its actual source is requested.
        self.assertTrue(self.promote()[0])
        self.assertEqual(len(self.calls), 2)

    def test_forward_published_package_uses_only_the_atomic_selector(self):
        selected, _ = self.promote()
        self.assertTrue(selected)
        self.assertEqual(self.calls, [["/owned host/aterm", "pkg", "link", "--help"],
                                     ["/owned host/aterm", "pkg", "link", "codex",
                                      str(self.new), "bin/codex", "--if-checkout", str(self.old),
                                      "--if-prefix", str(self.prefix), "--wait-lock", "1800"]])

    def test_inherited_git_directory_cannot_choose_promotion_ancestry(self):
        foreign = self.home / "foreign.git"
        self.git("clone", "--bare", "--quiet", str(self.repo), str(foreign))
        self.git("--git-dir", str(foreign), "update-ref", "refs/remotes/origin/main", self.source)
        self.git("update-ref", "refs/remotes/origin/main", self.old_source)
        marker = self.marker.read_bytes()
        self.assertFalse(self.promote()[0])
        self.assertEqual(self.calls, [])

        # The foreign repository admits the candidate; the requested one does not.
        with mock.patch.dict(os.environ, GIT_DIR=str(foreign)):
            self.assertFalse(self.promote()[0])
        self.assertEqual(self.calls, [])
        self.assertEqual(self.marker.read_bytes(), marker)

        # Reverse the two refs to prove the explicit repository remains authoritative.
        self.git("update-ref", "refs/remotes/origin/main", self.source)
        self.git("--git-dir", str(foreign), "update-ref", "refs/remotes/origin/main", self.old_source)
        with mock.patch.dict(os.environ, GIT_DIR=str(foreign)):
            self.assertTrue(self.promote()[0])
        self.assertEqual(len(self.calls), 2)
        self.assertEqual(self.calls[-1][4], str(self.new))
        self.assertEqual(self.marker.read_bytes(), marker)

    def test_codex_alab_s_own_line_advances_a_selection_named_on_upstream_s(self):
        # Decision 8: the selected build was named on upstream's numbering (0.164.0); the
        # next one on origin/main carries codex-alab's own line (0.1.0). It is the forward
        # build its source and count say it is, whatever its line.
        restarted = self.package(self.source, 2, parent=self.home / "own", line="0.1.0")
        own_root = self.home / "own"
        shutil.copytree(self.old, own_root / self.old.name)
        self.select(own_root / self.old.name)
        selected, reason = PROMOTE.promote(self.repo, own_root, self.prefix, restarted,
                                           self.source, aterm="/owned host/aterm",
                                           run=self.link)
        self.assertTrue(selected, reason)
        self.assertEqual(self.calls[-1][4], str(restarted))
        # The count, not the line, is the order: an older count on a higher line is refused.
        self.calls.clear()
        self.select(restarted)
        older = self.package(self.old_source, 1, parent=own_root, line="0.2.0")
        self.assertFalse(PROMOTE.promote(self.repo, own_root, self.prefix, older,
                                         self.old_source, run=self.link)[0])
        self.assertEqual(self.calls, [])

    def test_a_compound_version_advances_by_its_count_and_keeps_its_name(self):
        # Decision 8 (amended 2026-10-24): the package names its upstream Codex baseline in the
        # build metadata, 0.1.0-dev.<count>+codex.<X.Y.Z>.g<sha9>. It advances a selection by
        # its count, and the version it reports is the one it names, baseline included.
        own_root = self.home / "compound"
        compound = self.package(self.source, 2, parent=own_root, line="0.1.0",
                                baseline="0.164.0")
        shutil.copytree(self.old, own_root / self.old.name)
        self.select(own_root / self.old.name)
        source, version = PROMOTE.package(compound, own_root)
        self.assertEqual((source, tuple(version)), (self.source, (0, 1, 2)))
        self.assertEqual(version.text, f"0.1.0-dev.2+codex.0.164.0.g{self.source[:9]}")
        self.assertEqual(version.text, compound.name)
        selected, reason = PROMOTE.promote(self.repo, own_root, self.prefix, compound,
                                           self.source, aterm="/owned host/aterm",
                                           run=self.link)
        self.assertTrue(selected, reason)
        self.assertEqual(self.calls[-1][4], str(compound))
        # A newer baseline is no newer build: an older count is refused whatever it names.
        self.calls.clear()
        self.select(compound)
        older = self.package(self.old_source, 1, parent=own_root, line="0.1.0",
                             baseline="0.170.0")
        self.assertFalse(PROMOTE.promote(self.repo, own_root, self.prefix, older,
                                         self.old_source, run=self.link)[0])
        self.assertEqual(self.calls, [])
        # Malformed metadata is no version at all.
        for bad in ("codex.0.164.", "codex.0.164.00.", "codex.v0.164.0.", "upstream.0.164.0."):
            path = own_root / f"bad-{len(self.calls)}-{bad}"
            shutil.copytree(compound, path)
            version_text = f"0.1.0-dev.2+{bad}g{self.source[:9]}"
            for record in ("codex-package.json", "alab-build.json"):
                doc = json.loads((path / record).read_text())
                doc["version"] = version_text
                (path / record).write_text(json.dumps(doc))
            with self.assertRaises(ValueError, msg=bad):
                PROMOTE.package(path, own_root)

    def test_fallback_marker_rejects_complete_invalid_values_before_selection(self):
        original = self.marker.read_text()
        invalid = (
            '["bin/codex", 123]', '["bin/codex", true]',
            '["bin/codex" garbage]', '["bin/codex" "other"]',
            '["bin/codex",,]', '["bin/codex", ["other"]]',
            '["bin/codex"] trailing', '["bin/codex\\/"]',
            '["bin/codex", "\\ud800"]',
            '["bin/codex", "\\U0001f600"]',
            '["bin/codex", "raw\tcontrol"]',
        )
        replacements = [(original.replace('["bin/codex"]', value), value)
                        for value in invalid]
        replacements.extend((original + suffix, suffix) for suffix in (
            'extra = { invalid garbage }\n', '[unknown]\n', 'schema = 1\n'))
        replacements.extend(((original.rstrip("\n") + "\r", "bare final CR"),
                             (original + "# raw\x01control\n", "comment control byte")))
        replacements.extend((original.replace('program = "codex"', value), value)
                            for value in (
                                "program = 'codex' garbage'", 'program = "codex" garbage',
                                'program = "codex\\/"', 'program = "\\ud800"',
                                'program = "\\ud800\\udc00"'))
        replacements.append((original.replace('schema = 1', 'schema = 01'), 'schema = 01'))
        with mock.patch.object(PROMOTE, "tomllib", None):
            for text, label in replacements:
                with self.subTest(value=label):
                    self.marker.write_text(text)
                    self.calls.clear()
                    with self.assertRaises(ValueError):
                        PROMOTE.parse_link_marker(text)
                    result = self.promote()
                    self.assertIs(result[0], False)
                    self.assertIsInstance(result[1], str)
                    self.assertEqual(self.marker.read_bytes(), text.encode("utf-8"))
                    self.assertEqual(self.calls, [])

    def test_fallback_marker_preserves_generated_and_quoted_string_values(self):
        expected = {"schema": 1, "program": "codex", "checkout": str(self.old),
                    "bins": ["bin/codex"], "linked_at": ""}
        with mock.patch.object(PROMOTE, "tomllib", None):
            self.assertEqual(PROMOTE.parse_link_marker(self.marker.read_text()), expected)
            self.assertEqual(PROMOTE.parse_link_marker(
                self.marker.read_text().replace("\n", "\r\n")), expected)
            self.assertTrue(self.promote()[0])
            for value in ('/space #=, path/"quote"/back\\slash/é', "literal #=, /path",
                          r"/literal\ud800/escape text"):
                for quoted in (json.dumps(value), "'" + value + "'"):
                    with self.subTest(value=quoted):
                        text = (f'schema = 1\nprogram = "codex"\ncheckout = {quoted}\n'
                                'bins = ["bin/codex", \'other,=# path\',]\nlinked_at = ""\n')
                        self.assertEqual(PROMOTE.parse_link_marker(text), {
                            **expected, "checkout": value,
                            "bins": ["bin/codex", "other,=# path"]})

    def test_promotion_waits_for_a_bounded_store_lock_without_retrying(self):
        calls = []

        def selector(args, **kwargs):
            calls.append((args, kwargs))
            if args[-1] == "--help":
                return subprocess.CompletedProcess(
                    args, 0, "link --if-checkout <path> --if-prefix <path> --wait-lock <secs>", "")
            return subprocess.CompletedProcess(args, 75, "", "store lock wait expired")

        with self.assertRaisesRegex(RuntimeError, "package kept"):
            PROMOTE.promote(self.repo, self.root, self.prefix, self.new, self.source,
                            run=selector)
        self.assertEqual(len(calls), 2, "one capability check and one bounded selector call")
        args, options = calls[1]
        self.assertIn("--wait-lock", args)
        at = args.index("--wait-lock")
        bound = int(args[at + 1])
        self.assertGreater(bound, 0)
        self.assertGreater(options["timeout"], bound)

    def test_an_incomplete_or_failed_package_does_not_reach_selection(self):
        for field in ("codex-package.json", "alab-build.json", "bin/codex",
                      "bin/codex-code-mode-host", "codex-path/rg"):
            with self.subTest(missing=field):
                self.calls.clear()
                path = self.new / field
                content = path.read_bytes()
                mode = path.stat().st_mode
                path.unlink()
                self.assertFalse(self.promote()[0])
                self.assertEqual(self.calls, [])
                path.write_bytes(content)
                path.chmod(mode)
        for field in ("bin", "codex-resources", "codex-path"):
            with self.subTest(missing_directory=field):
                self.calls.clear()
                path = self.new / field
                temporary = self.home / "held directory"
                path.rename(temporary)
                try:
                    self.assertFalse(self.promote()[0])
                    self.assertEqual(self.calls, [])
                finally:
                    temporary.rename(path)
        hidden = self.root / ".partial"
        self.new.rename(hidden)
        self.calls.clear()
        self.assertFalse(self.promote(hidden)[0])
        self.assertEqual(self.calls, [])

    def test_truncated_package_tools_do_not_reach_selection(self):
        for relative in ("bin/codex", "bin/codex-code-mode-host", "codex-path/rg"):
            with self.subTest(tool=relative):
                path = self.new / relative
                original = path.read_bytes()
                try:
                    path.write_bytes(b"")
                    self.assertFalse(self.promote()[0])
                    self.assertEqual(self.calls, [])
                finally:
                    path.write_bytes(original)
                    self.calls.clear()
        self.assertTrue(self.promote()[0], "the restored complete artifact remains eligible")

    def test_declared_executor_is_required_before_selection(self):
        manifest = self.new / "codex-package.json"
        record = json.loads(manifest.read_bytes())
        record["execServer"] = "bin/codex-exec-server"
        manifest.write_text(json.dumps(record))
        executable = self.new / record["execServer"]
        self.assertFalse(self.promote()[0], "a missing declared executor is incomplete")
        self.assertEqual(self.calls, [])
        executable.write_bytes(b"inert executor fixture, never executed")
        executable.chmod(0o755)
        self.assertTrue(self.promote()[0], "the complete executor package is eligible")

        for mode in (0o644, 0o777):
            with self.subTest(mode=oct(mode)):
                self.calls.clear()
                executable.chmod(mode)
                self.assertFalse(self.promote()[0])
                self.assertEqual(self.calls, [])
        executable.chmod(0o755)
        original = executable.read_bytes()
        executable.write_bytes(b"")
        self.calls.clear()
        self.assertFalse(self.promote()[0])
        self.assertEqual(self.calls, [])
        executable.write_bytes(original)
        held = self.home / "held executor"
        executable.rename(held)
        executable.symlink_to(held)
        self.assertFalse(self.promote()[0])
        self.assertEqual(self.calls, [])

    def test_executor_layout_cannot_redirect_selection(self):
        manifest = self.new / "codex-package.json"
        record = json.loads(manifest.read_bytes())
        for value in (None, False, [], "bin/codex", "../outside-executor"):
            with self.subTest(executor=value):
                record["execServer"] = value
                manifest.write_text(json.dumps(record))
                self.calls.clear()
                self.assertFalse(self.promote()[0])
                self.assertEqual(self.calls, [])

    def test_required_package_paths_are_regular_protected_components(self):
        for field in ("bin", "codex-resources", "codex-path"):
            with self.subTest(linked_directory=field):
                self.calls.clear()
                path = self.new / field
                held = self.home / "linked directory"
                path.rename(held)
                path.symlink_to(held, target_is_directory=True)
                try:
                    self.assertFalse(self.promote()[0])
                    self.assertEqual(self.calls, [])
                finally:
                    path.unlink()
                    held.rename(path)
        for field in ("bin/codex", "bin/codex-code-mode-host", "codex-path/rg"):
            path = self.new / field
            for mode in (0o644, 0o777):
                with self.subTest(executable=field, mode=oct(mode)):
                    self.calls.clear()
                    path.chmod(mode)
                    try:
                        self.assertFalse(self.promote()[0])
                        self.assertEqual(self.calls, [])
                    finally:
                        path.chmod(0o755)
            with self.subTest(linked_executable=field):
                self.calls.clear()
                held = self.home / "linked executable"
                path.rename(held)
                path.symlink_to(held)
                try:
                    self.assertFalse(self.promote()[0])
                    self.assertEqual(self.calls, [])
                finally:
                    path.unlink()
                    held.rename(path)

    def test_required_package_layout_matches_its_staged_paths(self):
        manifest = self.new / "codex-package.json"
        original = manifest.read_bytes()
        for field in ("variant", "entrypoint", "resourcesDir", "pathDir"):
            with self.subTest(layout_field=field):
                record = json.loads(original)
                record[field] = "different"
                manifest.write_text(json.dumps(record))
                self.calls.clear()
                try:
                    self.assertFalse(self.promote()[0])
                    self.assertEqual(self.calls, [])
                finally:
                    manifest.write_bytes(original)

    def test_an_unrelated_fixed_checkout_or_absent_link_is_preserved(self):
        candidate_before = {p.relative_to(self.new): p.read_bytes()
                            for p in self.new.rglob("*") if p.is_file()}
        unrelated = self.package(self.old_source, 1, self.home / "fixed checkout")
        self.select(unrelated)
        before = self.marker.read_bytes()
        self.assertFalse(self.promote()[0])
        self.assertEqual(self.marker.read_bytes(), before)
        self.assertEqual(self.calls, [])
        self.marker.unlink()
        self.assertFalse(self.promote()[0])
        self.assertEqual(self.calls, [])
        missing_prefix = self.home / "no atpkg prefix"
        self.assertFalse(PROMOTE.promote(self.repo, self.root, missing_prefix, self.new,
                                        self.source, run=self.link)[0])
        self.assertFalse(missing_prefix.exists())
        self.assertEqual(self.calls, [])
        self.assertEqual({p.relative_to(self.new): p.read_bytes()
                          for p in self.new.rglob("*") if p.is_file()}, candidate_before)

    def test_older_same_source_divergent_and_unmerged_builds_do_not_replace_selection(self):
        self.select(self.new)
        self.assertFalse(PROMOTE.promote(self.repo, self.root, self.prefix, self.old,
                                        self.old_source, run=self.link)[0])
        self.assertFalse(self.promote()[0])
        self.select(self.old)
        self.git("checkout", "-q", "-b", "side", self.old_source)
        self.git("commit", "-q", "--allow-empty", "-m", "divergent")
        divergent = self.git("rev-parse", "HEAD")
        other = self.package(divergent, 3)
        self.assertFalse(PROMOTE.promote(self.repo, self.root, self.prefix, other,
                                        divergent, run=self.link)[0])
        self.select(self.new)
        self.git("checkout", "-q", "main")
        self.git("commit", "-q", "--allow-empty", "-m", "not on origin/main")
        unmerged = self.git("rev-parse", "HEAD")
        other = self.package(unmerged, 4)
        self.assertFalse(PROMOTE.promote(self.repo, self.root, self.prefix, other,
                                        unmerged, run=self.link)[0])
        self.assertEqual(self.calls, [])

    def test_malformed_or_mismatched_metadata_is_not_authority(self):
        for field, value in (("source", "HEAD"), ("source", self.old_source),
                             ("version", "0.164.0-dev.1+g" + self.source[:9]),
                             ("target", "x86_64-apple-darwin"),
                             ("compiler", {"admitted": False})):
            with self.subTest(field=field):
                path = self.new / "alab-build.json"
                original = path.read_text()
                value_at = json.loads(original)
                value_at[field] = value
                path.write_text(json.dumps(value_at))
                self.assertFalse(self.promote()[0])
                self.assertEqual(self.calls, [])
                path.write_text(original)
        self.marker.write_text('schema = 1\nprogram = "claude"\n')
        self.assertFalse(self.promote()[0])
        self.assertEqual(self.calls, [])

    def test_a_stale_selector_never_falls_back(self):
        for failure in ("selected checkout changed", "configured prefix changed",
                        "selected bins changed"):
            with self.subTest(failure=failure):
                self.calls.clear()
                before = self.marker.read_bytes()

                def refused(args, **kwargs):
                    if args[-1] == "--help":
                        return self.link(args, **kwargs)
                    self.calls.append(args)
                    return subprocess.CompletedProcess(args, 2, "", failure)

                with self.assertRaisesRegex(RuntimeError, "package kept"):
                    PROMOTE.promote(self.repo, self.root, self.prefix, self.new, self.source,
                                    run=refused)
                self.assertEqual(len(self.calls), 2)
                self.assertIn("--if-checkout", self.calls[1])
                self.assertIn("--if-prefix", self.calls[1])
                self.assertEqual(self.marker.read_bytes(), before)

    def test_old_selectors_cannot_ignore_unknown_guards_and_mutate(self):
        for help_text in ("link <program> <checkout> [bins...]",
                          "link --if-checkout <path>", "link --if-prefix <path>",
                          "link --if-checkout <path> --if-prefix <path>"):
            with self.subTest(help=help_text):
                self.calls.clear()

                def legacy(args, **kwargs):
                    self.calls.append(args)
                    if args[-1] == "--help":
                        return subprocess.CompletedProcess(args, 0, help_text, "")
                    self.fail("legacy cmd_link would ignore guard words and change selection")

                with self.assertRaisesRegex(RuntimeError, "lacks conditional link support"):
                    PROMOTE.promote(self.repo, self.root, self.prefix, self.new, self.source,
                                    run=legacy)
                self.assertEqual(self.calls, [["aterm", "pkg", "link", "--help"]])

    @unittest.skipUnless(sys.platform == "darwin", "the package producer requires macOS")
    def test_publish_failure_and_opt_out_cannot_call_the_selector(self):
        # Exercise the producer's actual publish boundary, without building Codex. The
        # selector deliberately does not exist: neither path may attempt to execute it.
        stage = self.home / "staged"
        stage.mkdir()
        destination = self.root / "published"
        command = ('set -euo pipefail; . "$1/alab/package.sh"; shift; '
                   'alab_publish_package "$@"')
        args = [sys.executable, str(stage), str(destination), str(self.root),
                str(self.prefix), self.source, "0"]
        environment = dict(os.environ, ALAB_ATERM="/no/such/selector")
        done = subprocess.run(["/bin/bash", "-c", command, "publish", str(ROOT), *args],
                              capture_output=True, text=True, env=environment, timeout=60)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertTrue(destination.is_dir())
        args[1] = str(self.home / "missing-stage")
        args[-1] = "1"
        done = subprocess.run(["/bin/bash", "-c", command, "publish", str(ROOT), *args],
                              capture_output=True, text=True, env=environment, timeout=60)
        self.assertNotEqual(done.returncode, 0)
        self.assertNotIn("conditional package promotion", done.stderr)

    @unittest.skipUnless(sys.platform == "darwin", "the package producer requires macOS")
    def test_successful_publication_selects_after_the_final_rename(self):
        tooling = self.repo / "alab"
        tooling.mkdir()
        for name in ("package.sh", "lib.sh", "command_guard.py", "promote.py", "publish_directory.py"):
            shutil.copyfile(ROOT / "alab" / name, tooling / name)
        stage = self.home / ".staged"
        self.new.rename(stage)
        journal = self.home / "selection.json"
        selector = self.home / "aterm"
        selector.write_text(PYTHON_ENTRY + f'''
import json
from pathlib import Path
import sys
if sys.argv[1:] == ["pkg", "link", "--help"]:
    print("link --if-checkout <path> --if-prefix <path> --wait-lock <secs>")
else:
    assert Path({str(self.new)!r}).is_dir(), "publication must precede selection"
    assert not Path({str(stage)!r}).exists(), "the staging path must be gone"
    Path({str(journal)!r}).write_text(json.dumps(sys.argv[1:]))
''')
        selector.chmod(0o755)
        command = ('set -euo pipefail; . "$1/alab/package.sh"; shift; '
                   'alab_publish_package "$@"')
        done = subprocess.run(
            ["/bin/bash", "-c", command, "publish", str(self.repo), sys.executable, str(stage),
             str(self.new), str(self.root), str(self.prefix), self.source, "1"],
            capture_output=True, text=True, timeout=60,
            env=dict(os.environ, ALAB_ATERM=str(selector)))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(json.loads(journal.read_text()),
                         ["pkg", "link", "codex", str(self.new), "bin/codex", "--if-checkout",
                          str(self.old), "--if-prefix", str(self.prefix),
                          "--wait-lock", "1800"])


if __name__ == "__main__":
    unittest.main()

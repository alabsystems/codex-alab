#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""A local producer builds fetched source only for its still-selected artifact lane."""

import importlib.util
import itertools
import json
import os
from pathlib import Path
import subprocess
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


PROMOTION_TESTS = load("local_update_promotion_fixtures", ROOT / "alab/tests/test_promote.py")
UPDATE = load("alab_local_update", ROOT / "alab/local_update.py")
parse_link_marker = UPDATE.PROMOTE.parse_link_marker


class LocalUpdateTests(unittest.TestCase):
    git = PROMOTION_TESTS.PromotionTests.git
    package = PROMOTION_TESTS.PromotionTests.package
    select = PROMOTION_TESTS.PromotionTests.select

    def setUp(self):
        PROMOTION_TESTS.PromotionTests.setUp(self)
        self.origin = self.home / "origin with spaces.git"
        self.git("init", "--bare", "-q", "-b", "main", str(self.origin))
        self.git("remote", "add", "origin", str(self.origin))
        self.git("push", "-q", "origin", "main")
        self.out = self.root.parent
        self.aterm = str(self.home / "owned host/aterm")
        self.builds = []

    def selector(self, args, **kwargs):
        self.calls.append(args)
        self.assertEqual(args[0], self.aterm)
        if args[1:] == ["pkg", "link", "--help"]:
            return subprocess.CompletedProcess(
                args, 0, "link --if-checkout <path> --if-prefix <path> --wait-lock <secs>", "")
        self.assertEqual(args[1:4], ["pkg", "link", "codex"])
        self.assertEqual(args[5], "bin/codex")
        expected = args[args.index("--if-checkout") + 1]
        prefix = args[args.index("--if-prefix") + 1]
        self.assertIn("--wait-lock", args)
        marker = parse_link_marker(self.marker.read_text())
        if marker["checkout"] != expected or prefix != str(self.prefix):
            return subprocess.CompletedProcess(args, 2, "", "selection changed")
        self.select(Path(args[4]))
        return subprocess.CompletedProcess(args, 0, "selected", "")

    def publish_next_source(self):
        self.git("commit", "-q", "--allow-empty", "-m", "fresh remote source")
        source = self.git("rev-parse", "HEAD")
        self.git("push", "-q", "origin", "HEAD:main")
        # The poll must fetch, rather than trusting this stale tracking ref.
        self.git("update-ref", "refs/remotes/origin/main", self.old_source)
        return source

    def build(self, source):
        self.builds.append(source)
        self.package(source, 3)
        return subprocess.CompletedProcess(["build", source], 0, "published", "")

    def update(self, **kwargs):
        return UPDATE.update_once(self.repo, self.out, self.prefix, self.aterm,
                                  run=kwargs.pop("run", self.selector),
                                  build=kwargs.pop("build", self.build), **kwargs)

    def selected(self):
        return Path(parse_link_marker(self.marker.read_text())["checkout"])

    def mutations(self):
        return [args for args in self.calls if args[-1] != "--help"]

    def test_fetch_and_selection_use_the_requested_repository(self):
        foreign = self.home / "foreign repo"
        foreign_origin = self.home / "foreign origin.git"
        self.git("clone", "-q", str(self.repo), str(foreign))
        self.git("-C", str(foreign), "-c", "user.name=Foreign fixture", "-c",
                 "user.email=foreign@invalid", "-c", "commit.gpgsign=false",
                 "commit", "-q", "--allow-empty", "-m", "foreign source")
        foreign_source = self.git("-C", str(foreign), "rev-parse", "HEAD")
        self.git("init", "--bare", "-q", "-b", "main", str(foreign_origin))
        self.git("-C", str(foreign), "remote", "set-url", "origin", str(foreign_origin))
        self.git("-C", str(foreign), "push", "-q", "origin", "HEAD:main")
        with mock.patch.dict(os.environ, GIT_DIR=str(foreign / ".git"),
                             GIT_WORK_TREE=str(foreign)):
            changed, _ = self.update()
        self.assertTrue(changed)
        self.assertEqual(self.selected(), self.new)
        self.assertEqual(self.builds, [], "the requested origin already has its artifact")
        self.assertEqual(self.git("rev-parse", "refs/remotes/origin/main"), self.source)
        self.assertEqual(self.git("-C", str(foreign), "rev-parse", "HEAD"), foreign_source)

    def test_controller_metadata_uses_the_requested_clones_common_directory(self):
        foreign = self.home / "foreign metadata"
        self.git("init", "-q", str(foreign))
        with mock.patch.dict(os.environ, GIT_DIR=str(foreign / ".git"),
                             GIT_COMMON_DIR=str(foreign / ".git")):
            self.assertEqual(UPDATE.common_dir(self.repo), self.repo / ".git")

    def test_an_exact_published_source_is_reused_without_a_build(self):
        changed, _ = self.update()
        self.assertTrue(changed)
        self.assertEqual(self.builds, [])
        self.assertEqual(self.selected(), self.new)
        self.assertEqual(len(self.mutations()), 1)
        mutation = self.mutations()[0]
        self.assertEqual(mutation[mutation.index("--if-checkout") + 1], str(self.old))

    def test_a_healthy_exact_source_rebuild_is_reused_beside_a_corrupt_artifact(self):
        staging = self.home / "private rebuild"
        healthy = self.package(self.source, 2, staging)
        rebuilt = self.root / (self.new.name + ".rebuilt-test")
        healthy.rename(rebuilt)
        broken = self.new / "codex-package.json"
        original = broken.read_bytes()
        for contents in (b"[]", b"null", b"invalid JSON", None):
            if contents is None:
                broken.unlink()
            else:
                broken.write_bytes(contents)
            for order in itertools.permutations((self.old, self.new, rebuilt)):
                with self.subTest(contents=contents, order=[path.name for path in order]):
                    self.select(self.old)
                    self.calls.clear()
                    with mock.patch.object(Path, "iterdir", return_value=iter(order)):
                        changed, _ = self.update()
                    self.assertTrue(changed)
                    self.assertEqual(self.selected(), rebuilt)
                    self.assertEqual(self.builds, [])
                    self.assertEqual(len(self.mutations()), 1)
                    if contents is None:
                        self.assertFalse(broken.exists())
                    else:
                        self.assertEqual(broken.read_bytes(), contents)
        broken.write_bytes(original)

    def test_only_corrupt_exact_source_artifacts_remain_a_failure_without_a_build(self):
        alternate = self.package(self.source, 3)
        (self.new / "codex-package.json").write_text("[]")
        (alternate / "bin/codex").unlink()
        before = self.marker.read_bytes()
        for order in itertools.permutations((self.old, self.new, alternate)):
            with self.subTest(order=[path.name for path in order]):
                with mock.patch.object(Path, "iterdir", return_value=iter(order)):
                    with self.assertRaises((ValueError, OSError)):
                        self.update()
                self.assertEqual(self.marker.read_bytes(), before)
                self.assertEqual(self.builds, [])
                self.assertEqual(self.mutations(), [])

    def test_damaged_required_companions_are_not_reused_or_selected(self):
        for field in ("bin/codex-code-mode-host", "codex-path/rg"):
            with self.subTest(missing=field):
                self.select(self.old)
                self.calls.clear()
                broken = self.new / field
                contents, mode = broken.read_bytes(), broken.stat().st_mode
                before = self.marker.read_bytes()
                broken.unlink()
                try:
                    with self.assertRaises((ValueError, OSError)):
                        self.update()
                    self.assertEqual(self.marker.read_bytes(), before)
                    self.assertEqual(self.builds, [])
                    self.assertEqual(self.mutations(), [])
                finally:
                    broken.write_bytes(contents)
                    broken.chmod(mode)

    def test_a_complete_rebuild_is_reused_beside_an_artifact_missing_a_companion(self):
        damaged = self.root / (self.new.name + ".rebuilt-z-damaged")
        self.new.rename(damaged)
        healthy = self.package(self.source, 2)
        (damaged / "bin/codex-code-mode-host").unlink()
        for order in itertools.permutations((self.old, damaged, healthy)):
            with self.subTest(order=[path.name for path in order]):
                self.select(self.old)
                self.calls.clear()
                with mock.patch.object(Path, "iterdir", return_value=iter(order)):
                    changed, _ = self.update()
                self.assertTrue(changed)
                self.assertEqual(self.selected(), healthy)
                self.assertEqual(self.builds, [])
                self.assertEqual(len(self.mutations()), 1)
                self.assertFalse((damaged / "bin/codex-code-mode-host").exists())

    def test_missing_artifact_builds_the_fetched_immutable_descendant_once(self):
        fresh = self.publish_next_source()
        self.git("checkout", "-q", "--detach", self.old_source)
        changed, _ = self.update()
        self.assertTrue(changed)
        self.assertEqual(self.builds, [fresh])
        self.assertEqual(self.git("rev-parse", "HEAD"), self.old_source)
        self.assertEqual(self.git("rev-parse", "refs/remotes/origin/main"), fresh)
        self.assertEqual(self.selected().name, f"0.164.0-dev.3+g{fresh[:9]}")
        self.assertEqual(len(self.mutations()), 1)

    def test_a_manual_selection_during_build_is_preserved(self):
        canonical = str(self.old)
        alias = canonical + "/."
        for original, manual in ((canonical, str(self.new)),
                                 (alias, canonical), (canonical, alias)):
            with self.subTest(original=original, manual=manual):
                fresh = self.publish_next_source()
                self.select(original)
                self.calls.clear()
                self.builds.clear()

                def build(source):
                    result = self.build(source)
                    self.select(manual)
                    return result

                changed, _ = self.update(build=build)
                self.assertFalse(changed)
                self.assertEqual(self.builds, [fresh])
                self.assertEqual(parse_link_marker(self.marker.read_text())["checkout"], manual)
                self.assertEqual(self.mutations(), [])

        fresh = self.publish_next_source()
        self.select(alias)
        self.calls.clear()
        self.builds.clear()
        changed, _ = self.update()
        self.assertTrue(changed)
        self.assertEqual(self.builds, [fresh])
        self.assertEqual(len(self.mutations()), 1)
        mutation = self.mutations()[0]
        self.assertEqual(mutation[mutation.index("--if-checkout") + 1], alias)
        self.assertNotEqual(parse_link_marker(self.marker.read_text())["checkout"], alias)

    def test_unsupported_selector_is_detected_before_an_expensive_build(self):
        self.publish_next_source()

        def legacy(args, **kwargs):
            self.calls.append(args)
            self.assertEqual(args[1:], ["pkg", "link", "--help"])
            return subprocess.CompletedProcess(args, 0, "link <program> <checkout>", "")

        with self.assertRaises(RuntimeError):
            self.update(run=legacy)
        self.assertEqual(self.builds, [])
        self.assertEqual(self.selected(), self.old)
        self.assertEqual(len(self.calls), 1)

    def test_unchanged_origin_does_not_build_or_change_selection(self):
        self.select(self.new)
        before = self.marker.read_bytes()
        changed, _ = self.update()
        self.assertFalse(changed)
        self.assertEqual(self.builds, [])
        self.assertEqual(self.marker.read_bytes(), before)
        self.assertEqual(self.mutations(), [])

    def test_divergent_origin_does_not_build_or_change_selection(self):
        self.select(self.new)
        self.git("checkout", "-q", "-b", "side", self.old_source)
        self.git("commit", "-q", "--allow-empty", "-m", "divergent remote source")
        self.git("push", "-q", "--force", "origin", "HEAD:main")
        before = self.marker.read_bytes()
        changed, _ = self.update()
        self.assertFalse(changed)
        self.assertEqual(self.builds, [])
        self.assertEqual(self.marker.read_bytes(), before)
        self.assertEqual(self.mutations(), [])

    def test_failed_build_is_not_retried_and_cannot_select_an_older_artifact(self):
        fresh = self.publish_next_source()

        def fail(source):
            self.builds.append(source)
            return subprocess.CompletedProcess(["build", source], 1, "", "build failed")

        with self.assertRaises(RuntimeError):
            self.update(build=fail)
        self.assertEqual(self.builds, [fresh])
        self.assertEqual(self.selected(), self.old)
        self.assertEqual(self.mutations(), [])

    def test_refused_selection_is_not_retried_without_guards(self):
        def refused(args, **kwargs):
            if args[-1] == "--help":
                return self.selector(args, **kwargs)
            self.calls.append(args)
            self.assertIn("--if-checkout", args)
            self.assertIn("--if-prefix", args)
            return subprocess.CompletedProcess(args, 2, "", "selection refused")

        with self.assertRaises(RuntimeError):
            self.update(run=refused)
        self.assertEqual(self.builds, [])
        self.assertEqual(len(self.mutations()), 1)
        self.assertEqual(self.selected(), self.old)

    def test_no_eligible_lane_returns_before_a_failing_origin_fetch(self):
        self.marker.unlink()
        self.git("remote", "set-url", "origin", str(self.home / "missing origin"))
        changed, _ = self.update()
        self.assertFalse(changed)
        self.assertEqual(self.builds, [])
        self.assertEqual(self.calls, [])

    def test_bootstrap_dispatch_does_not_interpret_the_selected_package_version(self):
        for version in (self.old.name, "0.1.0-dev.1+codex.0.164.0.g" + self.old_source[:9],
                        "a future package identity", "--future-version", "\ud800"):
            with self.subTest(version=version):
                for name in ("alab-build.json", "codex-package.json"):
                    path = self.old / name
                    record = json.loads(path.read_text())
                    record["version"] = version
                    path.write_text(json.dumps(record))
                before = self.marker.read_bytes()
                handed = []

                def dispatch(source, checkout, prefix):
                    handed.append((source, checkout, prefix))
                    return False, "fresh controller owns admission"

                with mock.patch.object(UPDATE.PROMOTE, "selected_package",
                                       side_effect=AssertionError("stale package grammar")):
                    changed, _ = self.update(dispatch=dispatch)
                self.assertFalse(changed)
                self.assertEqual(handed, [(self.source, str(self.old), str(self.prefix))])
                self.assertEqual(self.marker.read_bytes(), before)
                self.assertEqual(self.builds, [])
                self.assertEqual(self.calls, [])

    def test_bootstrap_snapshot_is_not_controller_package_or_selection_authority(self):
        before = (self.old / "alab-build.json").read_bytes()
        for change in ("invalid package", "replaced selection"):
            with self.subTest(change=change):
                (self.old / "alab-build.json").write_bytes(before)
                self.select(self.old)

                def dispatch(source, checkout, prefix):
                    if change == "invalid package":
                        (self.old / "alab-build.json").write_text("[]")
                    else:
                        self.select(self.new)
                    marker = self.marker.read_bytes()
                    result = self.update(source=source, expected_checkout=checkout,
                                         expected_prefix=prefix)
                    self.assertEqual(self.marker.read_bytes(), marker)
                    return result

                changed, _ = self.update(dispatch=dispatch)
                self.assertFalse(changed)
                self.assertEqual(self.builds, [])
                self.assertEqual(self.calls, [])

    def test_invalid_bootstrap_channel_never_fetches_or_dispatches(self):
        outside = self.home / "outside channel"
        outside.mkdir()
        linked = self.root / "linked package"
        linked.symlink_to(self.old, target_is_directory=True)
        hidden = self.root / ".hidden package"
        hidden.mkdir()
        for selected in (outside, linked, hidden):
            with self.subTest(selected=selected):
                self.select(selected)
                before = self.marker.read_bytes()
                with (mock.patch.object(UPDATE, "git", side_effect=AssertionError("fetch")),
                      mock.patch.object(UPDATE.PROMOTE, "selected_package",
                                        side_effect=AssertionError("package admission"))):
                    changed, _ = self.update(dispatch=mock.Mock(
                        side_effect=AssertionError("dispatch")))
                self.assertFalse(changed)
                self.assertEqual(self.marker.read_bytes(), before)
                self.assertEqual(self.builds, [])
                self.assertEqual(self.calls, [])


if __name__ == "__main__":
    unittest.main()

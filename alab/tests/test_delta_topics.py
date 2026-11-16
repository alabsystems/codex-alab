# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise topic inventory through real scratch Git repositories, without a remote."""

import json
import os
import re
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class DeltaTopicsTests(unittest.TestCase):
    def setUp(self):
        excluded = Path(tempfile.gettempdir()) / ".ubd"
        excluded.mkdir(exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="delta-topics-", dir=excluded)
        self.addCleanup(self.temporary.cleanup)
        self.repo = Path(self.temporary.name)
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith("GIT_")}
        self.env.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1",
                        GIT_AUTHOR_NAME="Topic test", GIT_AUTHOR_EMAIL="topic@example.invalid",
                        GIT_COMMITTER_NAME="Topic test", GIT_COMMITTER_EMAIL="topic@example.invalid",
                        ALAB_PYTHON=sys.executable, LC_ALL="C")
        self.git = shutil.which("git")
        self.run_git("init", "-q", "-b", "main")
        (self.repo / "alab").mkdir()
        for name in ("delta.sh", "lib.sh", "feature_inventory.py", "hook_topics.py"):
            shutil.copy2(ROOT / "alab" / name, self.repo / "alab" / name)
        self.write("src/hooks.rs", "// ALAB(beta): second group\n// ALAB(alpha): first group\n")
        self.write("alab/owned.rs", "// ALAB(ignored-alab): owned implementation\n")
        self.write("AGENTS.md", "ALAB(ignored-instructions): instructions\n")
        self.write("codex-rs/alab-example/src/lib.rs", "// ALAB(ignored-crate): owned crate\n")
        self.initial = self.commit("initial hooks")

    def write(self, path, text):
        target = self.repo / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def run_git(self, *args):
        result = subprocess.run([self.git, *args], cwd=self.repo, env=self.env,
                                text=True, capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.strip()

    def commit(self, message):
        self.run_git("add", "-A")
        self.run_git("commit", "-q", "-m", message)
        return self.run_git("rev-parse", "HEAD")

    def delta(self, *args, env=None):
        return subprocess.run(["/bin/bash", str(self.repo / "alab/delta.sh"), *args],
                              cwd=self.repo, env=env or self.env, text=True,
                              capture_output=True, timeout=15)

    def topic_names(self, source):
        query = (
            "import json, os, sys; from pathlib import Path; "
            "sys.path.insert(0, str(Path.cwd() / 'alab')); "
            "from hook_topics import read_topics; "
            "print(json.dumps(sorted(read_topics(Path.cwd(), sys.argv[1], os.environ.copy()))))"
        )
        result = subprocess.run([sys.executable, "-I", "-c", query, source],
                                cwd=self.repo, env=self.env, text=True,
                                capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)

    def test_topics_need_no_remote_and_keep_existing_exclusions(self):
        self.assertEqual(self.run_git("remote"), "")
        result = self.delta("--topics")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, (
            "alpha\n  src/hooks.rs:2:// ALAB(alpha): first group\n"
            "beta\n  src/hooks.rs:1:// ALAB(beta): second group\n"
        ))

    def test_revision_tags_and_dirty_worktree_do_not_change_selected_commit(self):
        expected = self.delta("--topics", self.initial)
        self.assertEqual(expected.returncode, 0, expected.stderr)
        self.run_git("tag", "-a", "original", "-m", "original hooks", self.initial)
        self.write("src/hooks.rs", "// ALAB(gamma): newer commit\n")
        self.commit("newer hooks")
        self.write("src/hooks.rs", "// ALAB(uncommitted): ignored worktree edit\n")
        for revision in (self.initial, "original", "HEAD^"):
            with self.subTest(revision=revision):
                result = self.delta("--topics", revision)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, expected.stdout)
        current = self.delta("--topics")
        self.assertEqual(current.returncode, 0, current.stderr)
        self.assertIn("gamma\n", current.stdout)
        self.assertNotIn("uncommitted", current.stdout)

    def test_every_marker_is_grouped_and_tabs_in_source_are_preserved(self):
        line = "\t// ALAB(beta): shared; ALAB(alpha): also shared; ALAB(beta): repeated\ttext\n"
        self.write("src/hooks.rs", line + "// ALAB: untagged\n")
        self.commit("shared hook line")
        self.run_git("config", "color.grep", "always")
        result = self.delta("--topics")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, (
            "(untagged)\n  src/hooks.rs:2:// ALAB: untagged\n"
            f"alpha\n  src/hooks.rs:1:{line}"
            f"beta\n  src/hooks.rs:1:{line}"
        ))
        self.assertNotIn("\x1b", result.stdout)

    def test_whitespace_in_topic_names_preserves_the_exact_identity(self):
        self.write("src/hooks.rs", "// ALAB( unowned): leading space\n"
                   "// ALAB(owned\tunowned): embedded tab\n")
        source = self.commit("whitespace in distinct topic names")
        result = self.delta("--topics")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, (
            '" unowned"\n  src/hooks.rs:1:// ALAB( unowned): leading space\n'
            '"owned\\tunowned"\n'
            '  src/hooks.rs:2:// ALAB(owned\tunowned): embedded tab\n'
        ))
        self.assertEqual(self.topic_names(source), [" unowned", "owned\tunowned"])

    def test_c1_topic_control_preserves_identity_and_one_record_display(self):
        self.write("src/hooks.rs", "// ALAB(owned\u0085unowned): C1 topic\n")
        source = self.commit("C1 control in a distinct topic")
        result = self.delta("--topics")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, (
            '"owned\\u0085unowned"\n'
            '  src/hooks.rs:1:"// ALAB(owned\\u0085unowned): C1 topic"\n'
        ))
        self.assertEqual(len(result.stdout.splitlines()), 2)
        self.assertEqual(self.topic_names(source), ["owned\u0085unowned"])

    def test_binary_attributes_do_not_hide_named_hooks(self):
        self.write(".gitattributes", "*.rs binary\n")
        self.write("src/hooks.rs", "// ALAB(unowned): binary-marked text\n")
        self.commit("binary attribute on a text hook")
        result = self.delta("--topics")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout,
                         "unowned\n  src/hooks.rs:1:// ALAB(unowned): binary-marked text\n")

    def test_nul_source_bytes_do_not_hide_named_hooks(self):
        (self.repo / "src/hooks.rs").write_bytes(b"\0// ALAB(unowned): binary source\n")
        self.commit("NUL byte beside a named hook")
        result = self.delta("--topics")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout,
                         'unowned\n  src/hooks.rs:1:"\\u0000// ALAB(unowned): binary source"\n')

    def test_newline_filename_stays_in_one_quoted_record(self):
        self.write("src/hooks.rs", "// no hooks in this file\n")
        self.write("src/line\nbreak.rs", "// ALAB(path): unusual filename\n")
        self.commit("newline in a hook filename")
        result = self.delta("--topics")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout,
                         'path\n  "src/line\\nbreak.rs":1:// ALAB(path): unusual filename\n')
        self.assertEqual(len(result.stdout.splitlines()), 2)

    def test_git_display_configuration_cannot_change_topic_records(self):
        expected = self.delta("--topics")
        self.assertEqual(expected.returncode, 0, expected.stderr)
        for key, value in (("color.grep", "always"), ("grep.column", "true"),
                           ("grep.heading", "true"), ("grep.break", "true")):
            self.run_git("config", key, value)
        result = self.delta("--topics")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, expected.stdout)

    def test_no_matches_is_empty_success(self):
        self.write("src/hooks.rs", "// no hooks here\n")
        self.commit("no upstream hooks")
        result = self.delta("--topics")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "")

    def test_invalid_and_noncommit_revisions_fail_without_topic_output(self):
        tree = self.run_git("rev-parse", "HEAD^{tree}")
        blob = self.run_git("rev-parse", "HEAD:src/hooks.rs")
        for revision in ("missing-revision", tree, blob):
            with self.subTest(revision=revision):
                result = self.delta("--topics", revision)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")

    def test_fatal_grep_error_does_not_become_empty_or_partial_success(self):
        tools = self.repo / "test-bin"
        tools.mkdir()
        wrapper = tools / "git"
        wrapper.write_text(
            "#!/bin/sh\n"
            'if [ "$1" = "-c" ] && [ "$3" = "grep" ]; then\n'
            "  printf 'partial invented output\\n'\n"
            "  printf 'synthetic grep failure\\n' >&2\n"
            "  exit 2\n"
            "fi\n"
            f"exec {shlex.quote(self.git)} \"$@\"\n"
        )
        wrapper.chmod(0o700)
        env = dict(self.env, PATH=str(tools) + os.pathsep + self.env.get("PATH", ""))
        result = self.delta("--topics", env=env)
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertIn("synthetic grep failure", result.stderr)

    def test_malformed_grep_results_do_not_become_empty_or_partial_success(self):
        prefix = self.initial.encode("ascii") + b":src/hooks.rs\0"
        valid = prefix + b"1\0// ALAB(alpha): complete record\n"
        cases = (
            ("empty success", 0, b"", "missing hook output"),
            ("output on no match", 1, valid, "unexpected hook output"),
            ("incomplete after valid", 0, valid + prefix + b"2\0// ALAB(beta): truncated",
             "incomplete hook record"),
            ("wrong source", 0, b"f" * 40 + b":src/hooks.rs\0" + b"1\0// ALAB(alpha): hook\n",
             "invalid hook location"),
            ("zero line", 0, prefix + b"0\0// ALAB(alpha): hook\n", "invalid hook location"),
            ("nonnumeric line", 0, prefix + b"one\0// ALAB(alpha): hook\n",
             "invalid hook location"),
            ("markerless record", 0, prefix + b"1\0// no hook here\n", "contains no ALAB marker"),
        )
        tools = self.repo / "test-bin"
        tools.mkdir()
        wrapper = tools / "git"
        env = dict(self.env, PATH=str(tools) + os.pathsep + self.env.get("PATH", ""))
        for label, status, payload, error in cases:
            with self.subTest(case=label):
                escaped = "".join(f"\\0{byte:03o}" for byte in payload)
                wrapper.write_text(
                    "#!/bin/sh\n"
                    'if [ "$1" = "-c" ] && [ "$3" = "grep" ]; then\n'
                    f"  printf '%b' '{escaped}'\n"
                    f"  exit {status}\n"
                    "fi\n"
                    f"exec {shlex.quote(self.git)} \"$@\"\n"
                )
                wrapper.chmod(0o700)
                result = self.delta("--topics", env=env)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertIn(error, result.stderr)

    def test_history_modes_still_require_upstream_and_topics_keep_precedence(self):
        for args in ((), ("--patch",)):
            with self.subTest(args=args):
                result = self.delta(*args)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("no 'upstream' remote", result.stderr)
        expected = self.delta("--topics")
        self.assertEqual(expected.returncode, 0, expected.stderr)
        for args in (("--topics", "--patch"), ("--patch", "--topics")):
            result = self.delta(*args)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, expected.stdout)

    def test_explicit_base_measures_summary_and_patch_without_upstream(self):
        self.write("src/hooks.rs", "// ALAB(gamma): measured change\n")
        self.write("alab/new.txt", "owned addition\n")
        self.commit("change after explicit base")
        summary = self.delta("--base=" + self.initial)
        self.assertEqual(summary.returncode, 0, summary.stderr)
        self.assertIn("over explicit base", summary.stdout)
        self.assertIn("upstream/main lag is unknown", summary.stdout)
        self.assertIn("baseline file(s)", summary.stdout)
        self.assertNotIn("upstream file(s)", summary.stdout)
        self.assertIn("src/hooks.rs", summary.stdout)
        patch = self.delta("--base=" + self.initial, "--patch")
        self.assertEqual(patch.returncode, 0, patch.stderr)
        self.assertIn("+// ALAB(gamma): measured change", patch.stdout)
        self.assertNotIn("owned addition", patch.stdout)
        self.assertEqual(self.run_git("remote"), "")
        self.run_git("remote", "add", "upstream", str(self.repo / "unused-remote"))
        self.run_git("update-ref", "refs/remotes/upstream/main", "HEAD")
        with_ref = self.delta("--base=" + self.initial)
        self.assertEqual(with_ref.returncode, 0, with_ref.stderr)
        self.assertEqual(with_ref.stdout, summary.stdout,
                         "an explicit baseline must not silently switch to a local upstream ref")

    def test_patch_preserves_literal_paths_and_excludes_other_change_classes(self):
        paths = ("src/[hook].rs", "src/a*b.rs", "src/tab\tname.rs", "src/line\nname.rs",
                 ":(glob)src/colon.rs")
        for index, path in enumerate(paths):
            self.write(path, f"// before-{index}\n")
        self.write("src/derived.rs", "// old derived\n")
        self.write("src/deleted.rs", "// old deletion\n")
        self.write("alab/merge-attributes", "src/derived.rs merge=alab-upstream\n")
        baseline = self.commit("literal path baseline")
        for index, path in enumerate(paths):
            self.write(path, f"// changed-{index}\n")
        self.write("src/derived.rs", "// changed-derived\n")
        self.write("src/added.rs", "// added-file\n")
        (self.repo / "src/deleted.rs").unlink()
        self.commit("modify literal paths and other change classes")
        result = self.delta("--base=" + baseline, "--patch")
        self.assertEqual(result.returncode, 0, result.stderr)
        for index in range(len(paths)):
            self.assertEqual(result.stdout.count(f"+// changed-{index}\n"), 1,
                             f"literal path omitted or duplicated: {paths[index]!r}")
        self.assertNotIn("changed-derived", result.stdout)
        self.assertNotIn("added-file", result.stdout)
        self.assertNotIn("old deletion", result.stdout)
        self.run_git("remote", "add", "upstream", str(self.repo / "unused-remote"))
        self.run_git("update-ref", "refs/remotes/upstream/main", baseline)
        default = self.delta("--patch")
        self.assertEqual(default.returncode, 0, default.stderr)
        self.assertEqual(default.stdout, result.stdout)
        applicable = subprocess.run([self.git, "apply", "--reverse", "--check", "-"],
                                   input=result.stdout, cwd=self.repo, env=self.env,
                                   text=True, capture_output=True, timeout=15)
        self.assertEqual(applicable.returncode, 0, applicable.stderr)

    def test_reports_read_manifests_from_the_selected_commit(self):
        self.write("src/derived.rs", "// original derived\n")
        self.write("src/deleted.rs", "// original deletion\n")
        self.write("alab/merge-attributes", "src/derived.rs merge=alab-upstream\n")
        self.write("alab/removed-paths", "src/deleted.rs\n")
        baseline = self.commit("baseline manifests")
        self.write("src/derived.rs", "// regenerated derived\n")
        self.write("src/hooks.rs", "// ALAB(changed): selected hook\n")
        (self.repo / "src/deleted.rs").unlink()
        selected = self.commit("selected source")
        expected = {}
        for mode in ((), ("--patch",)):
            result = self.delta("--base=" + baseline, *mode, selected)
            self.assertEqual(result.returncode, 0, result.stderr)
            expected[mode] = result.stdout
        self.assertIn("derived     1 file(s)", expected[()])
        self.assertIn("1 under alab/removed-paths", expected[()])
        self.assertNotIn("regenerated derived", expected[("--patch",)])
        self.write("alab/merge-attributes", "# uncommitted exclusion edit\n")
        self.write("alab/removed-paths", "# uncommitted deletion edit\n")
        for state in ("dirty", "later commit"):
            if state == "later commit":
                self.commit("later manifests")
            for mode in ((), ("--patch",)):
                with self.subTest(state=state, mode=mode):
                    actual = self.delta("--base=" + baseline, *mode, selected)
                    self.assertEqual(actual.returncode, 0, actual.stderr)
                    self.assertEqual(actual.stdout, expected[mode])

    def test_untracked_manifests_do_not_supply_policy_missing_from_source(self):
        self.write("src/hooks.rs", "// ALAB(changed): selected hook\n")
        selected = self.commit("selected source without manifests")
        expected = self.delta("--base=" + self.initial, "--patch", selected)
        self.assertEqual(expected.returncode, 0, expected.stderr)
        self.write("alab/merge-attributes", "src/hooks.rs merge=alab-upstream\n")
        actual = self.delta("--base=" + self.initial, "--patch", selected)
        self.assertEqual(actual.returncode, 0, actual.stderr)
        self.assertEqual(actual.stdout, expected.stdout)
        self.assertIn("selected hook", actual.stdout)

    def test_nonregular_source_manifests_fail_before_report_output(self):
        manifest = self.repo / "alab/merge-attributes"
        manifest.symlink_to("owned.rs")
        linked = self.commit("symlink manifest")
        manifest.unlink()
        self.write("alab/merge-attributes/nested", "not a manifest\n")
        directory = self.commit("directory manifest")
        for selected in (linked, directory):
            for mode in ((), ("--patch",)):
                with self.subTest(source=selected, mode=mode):
                    result = self.delta("--base=" + self.initial, *mode, selected)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertEqual(result.stdout, "")
                    self.assertIn("must be a regular file", result.stderr)

    def test_manifest_read_errors_cannot_become_empty_exclusions(self):
        self.write("alab/merge-attributes", "src/hooks.rs merge=alab-upstream\n")
        selected = self.commit("source manifest")
        tools = self.repo / "test-bin"
        tools.mkdir()
        wrapper = tools / "git"
        wrapper.write_text(
            "#!/bin/sh\n"
            'if [ "$1" = "show" ]; then\n'
            "  printf 'synthetic source-manifest failure\\n' >&2\n"
            "  exit 2\n"
            "fi\n"
            f"exec {shlex.quote(self.git)} \"$@\"\n"
        )
        wrapper.chmod(0o700)
        env = dict(self.env, PATH=str(tools) + os.pathsep + self.env.get("PATH", ""))
        result = self.delta("--base=" + self.initial, "--patch", selected, env=env)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn("synthetic source-manifest failure", result.stderr)

    def test_churn_ranking_preserves_filename_whitespace(self):
        paths = ("src/ordinary.rs", "src/two  spaces.rs", " leading.rs")
        for path in paths:
            self.write(path, "// before\n")
        baseline = self.commit("create ranked files")
        for path in paths:
            self.write(path, "// after\n")
        self.commit("modify ranked files")
        result = self.delta("--base=" + baseline)
        self.assertEqual(result.returncode, 0, result.stderr)
        rows = result.stdout.splitlines()
        for path in paths:
            with self.subTest(path=path):
                matching = [row for row in rows if row.endswith(" " + path)]
                self.assertEqual(len(matching), 1, result.stdout)
                values = re.match(r"\s*(\d+)\s+(\d+)\s+\+1/-1\s+", matching[0])
                self.assertIsNotNone(values, matching[0])
                self.assertEqual(values.groups(), ("2", "1"), matching[0])

    def test_explicit_base_rejects_a_nonancestor_without_reporting_a_delta(self):
        unrelated = self.run_git("commit-tree", self.initial + "^{tree}", "-m", "unrelated")
        result = self.delta("--base=" + unrelated)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn("is not an ancestor", result.stderr)

    def test_explicit_base_resolves_historical_source_and_empty_patch(self):
        self.write("src/hooks.rs", "// ALAB(gamma): later change\n")
        self.commit("later source")
        result = self.delta("--base=" + self.initial, "--patch", self.initial)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "")

    def test_explicit_base_rejects_empty_unknown_and_topic_only_requests(self):
        for option in ("--base=", "--base=missing-commit"):
            with self.subTest(option=option):
                result = self.delta(option)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
        result = self.delta("--base=" + self.initial, "--topics")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn("--base cannot be combined with --topics", result.stderr)
        repeated = self.delta("--base=" + self.initial, "--base=" + self.initial)
        self.assertNotEqual(repeated.returncode, 0)
        self.assertEqual(repeated.stdout, "")
        self.assertIn("--base may be specified only once", repeated.stderr)

    def synced_history_with_stale_upstream(self):
        self.run_git("checkout", "-q", "-b", "upstream-work", self.initial)
        self.write("src/upstream.rs", "// integrated upstream change\n")
        upstream = self.commit("upstream change")
        self.run_git("checkout", "-q", "main")
        self.write("src/hooks.rs", "// ALAB(local): carried local hook\n")
        self.commit("local hook before sync")
        self.run_git("merge", "--no-ff", "upstream-work", "-m",
                     "alab sync: merge upstream openai/codex@" + upstream)
        self.sync_merge = self.run_git("rev-parse", "HEAD")
        self.run_git("remote", "add", "upstream", str(self.repo / "unused-remote"))
        self.run_git("update-ref", "refs/remotes/upstream/main", self.initial)
        return upstream

    def test_stale_upstream_ref_keeps_the_committed_sync_baseline(self):
        upstream = self.synced_history_with_stale_upstream()
        summary = self.delta()
        self.assertEqual(summary.returncode, 0, summary.stderr)
        self.assertIn("over upstream " + self.run_git("rev-parse", "--short", upstream),
                      summary.stdout)
        patch = self.delta("--patch")
        self.assertEqual(patch.returncode, 0, patch.stderr)
        self.assertIn("+// ALAB(local): carried local hook", patch.stdout)
        self.assertNotIn("integrated upstream change", patch.stdout)

    def test_sync_marker_quoted_in_a_merge_body_cannot_select_its_local_parent(self):
        upstream = self.synced_history_with_stale_upstream()
        self.run_git("checkout", "-q", "-b", "maintenance")
        self.write("src/maintenance.rs", "// local maintenance change\n")
        self.commit("maintenance branch work")
        self.run_git("checkout", "-q", "main")
        self.write("alab/maintenance.txt", "independent local work\n")
        self.commit("main work before maintenance merge")
        self.run_git("merge", "--no-ff", "maintenance", "-m",
                     "alab(maintenance): compose local work", "-m",
                     "alab sync: merge upstream openai/codex@" + upstream)
        history = subprocess.run(
            ["/bin/bash", "-c", 'set -euo pipefail; . "$1"; alab_sync_merges HEAD; alab_last_sync_upstream HEAD',
             "sync-subject-fixture", str(self.repo / "alab/lib.sh")],
            cwd=self.repo, env=self.env, text=True, capture_output=True, timeout=15)
        self.assertEqual(history.returncode, 0, history.stderr)
        self.assertEqual(history.stdout.splitlines(), [self.sync_merge, upstream])
        summary = self.delta()
        self.assertEqual(summary.returncode, 0, summary.stderr)
        self.assertIn("over upstream " + self.run_git("rev-parse", "--short", upstream),
                      summary.stdout)
        patch = self.delta("--patch")
        self.assertEqual(patch.returncode, 0, patch.stderr)
        self.assertIn("+// ALAB(local): carried local hook", patch.stdout)

    def test_sync_history_read_errors_cannot_become_a_fallback_baseline(self):
        self.run_git("remote", "add", "upstream", str(self.repo / "unused-remote"))
        self.run_git("update-ref", "refs/remotes/upstream/main", self.initial)
        tools = self.repo / "test-bin"
        tools.mkdir()
        wrapper = tools / "git"
        wrapper.write_text(
            "#!/bin/sh\n"
            'if [ "$1" = "log" ]; then\n'
            "  printf 'synthetic sync-history failure\\n' >&2\n"
            "  exit 2\n"
            "fi\n"
            f"exec {shlex.quote(self.git)} \"$@\"\n"
        )
        wrapper.chmod(0o700)
        env = dict(self.env, PATH=str(tools) + os.pathsep + self.env.get("PATH", ""))
        result = self.delta(env=env)
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertIn("synthetic sync-history failure", result.stderr)

    def test_summary_and_patch_still_compare_upstream_when_available(self):
        self.run_git("remote", "add", "upstream", str(self.repo / "unused-remote"))
        self.run_git("update-ref", "refs/remotes/upstream/main", self.initial)
        self.write("src/hooks.rs", "// ALAB(gamma): changed implementation\n")
        self.commit("change a hook")
        summary = self.delta()
        self.assertEqual(summary.returncode, 0, summary.stderr)
        self.assertIn("over upstream", summary.stdout)
        self.assertIn("src/hooks.rs", summary.stdout)
        patch = self.delta("--patch")
        self.assertEqual(patch.returncode, 0, patch.stderr)
        self.assertIn("+// ALAB(gamma): changed implementation", patch.stdout)

    def test_feature_inventory_cli_works_without_upstream_and_still_rejects_unowned(self):
        catalog = {
            "schema": 1,
            "features": [{"id": "example", "lifecycle": "active", "purpose": "Test hooks",
                          "equivalence": "Equivalent behavior", "topics": ["alpha", "beta"],
                          "implementation": ["alab/owned.rs"], "invariants": ["Keep state"],
                          "scenarios": ["Observe state"]}],
        }
        feature = catalog["features"][0]
        fields = ["schema = 1", "[[features]]"]
        fields += [f"{key} = {json.dumps(value)}" for key, value in feature.items()]
        self.write("alab/features.toml", "\n".join(fields) + "\n")
        self.commit("inventory")
        command = [sys.executable, str(self.repo / "alab/feature_inventory.py")]
        valid = subprocess.run(command, cwd=self.repo, env=self.env,
                               text=True, capture_output=True, timeout=15)
        self.assertEqual(valid.returncode, 0, valid.stderr)
        self.assertIn("ownership complete: 2 hook topics", valid.stdout)
        self.write("src/hooks.rs", "// ALAB(unowned): must be rejected\n")
        self.commit("unowned hook")
        invalid = subprocess.run(command, cwd=self.repo, env=self.env,
                                 text=True, capture_output=True, timeout=15)
        self.assertNotEqual(invalid.returncode, 0)
        self.assertIn("unowned hook topic: unowned", invalid.stderr)


if __name__ == "__main__":
    unittest.main()

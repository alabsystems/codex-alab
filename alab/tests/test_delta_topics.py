# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise topic inventory through real scratch Git repositories, without a remote."""

import json
import os
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

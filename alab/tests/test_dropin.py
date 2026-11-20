#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Command provenance uses clap declarations at the artifact's source commit; codex-alab's
own CODEX_HOME names compare as the vendor's."""

import importlib.util
from http.client import HTTPConnection
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch


SMOKE = Path(__file__).resolve().parents[1] / "smoke"
sys.path.insert(0, str(SMOKE))
SPEC = importlib.util.spec_from_file_location("dropin", SMOKE / "dropin.py")
dropin = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(dropin)


class ComparisonCleanupTests(unittest.TestCase):
    def test_app_server_closes_its_pipes_after_reply_and_early_eof(self):
        root = self.enterContext(tempfile.TemporaryDirectory())
        binary = SimpleNamespace(exe=sys.executable, root=root, env=lambda home: {})
        for output, expected in [('{"id": 1, "result": {}}', {1: {"id": 1, "result": {}}}), ("", {})]:
            with self.subTest(output=output):
                script = "import sys; sys.stdin.readline(); sys.stdin.readline(); sys.stdout.write(" + repr(output) + ")"
                with subprocess.Popen([sys.executable, "-c", script], stdin=subprocess.PIPE,
                                      stdout=subprocess.PIPE, text=True) as child:
                    with patch.object(dropin.subprocess, "Popen", return_value=child):
                        self.assertEqual(dropin.app_server(binary, [], codex_home=root), expected)
                    self.assertIsNotNone(child.returncode)
                    self.assertTrue(child.stdin.closed)
                    self.assertTrue(child.stdout.closed)

    def test_interruption_closes_listener_and_honors_keep(self):
        root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        neighbor = root / "existing"
        neighbor.write_text("keep")
        alab = root / "alab"
        vendor = root / "vendor"
        for package in (alab, vendor):
            package.mkdir()
            (package / "codex-package.json").write_text('{"entrypoint":"bin/codex","version":"1"}')
        (alab / "alab-build.json").write_text('{"source":"abcdef"}')
        self.enterContext(patch.object(dropin.tempfile, "tempdir", str(root)))
        self.enterContext(patch.object(dropin, "checked_upstream_base", return_value="base"))
        self.enterContext(patch.object(dropin.Bin, "run", return_value=SimpleNamespace(stdout="codex-cli 1")))
        self.enterContext(patch.object(dropin, "help_check", return_value=(True, [], 1)))
        for keep in (False, True):
            with self.subTest(keep=keep):
                server = dropin.serve(dropin.Mock())
                self.addCleanup(server.server_close)
                self.addCleanup(server.shutdown)
                client = HTTPConnection(*server.server_address, timeout=60)
                try:
                    client.request("GET", "/models")
                    self.assertEqual(client.getresponse().read(), b'{"models":[]}')
                finally:
                    client.close()
                argv = ["dropin", "--alab", str(alab), "--vendor", str(vendor)]
                if keep:
                    argv.append("--keep")
                with patch.object(sys, "argv", argv), patch.object(dropin, "serve", return_value=server), \
                        patch.object(dropin, "exec_check", side_effect=KeyboardInterrupt) as interrupted, \
                        patch.object(server, "shutdown", wraps=server.shutdown) as shutdown:
                    with self.assertRaises(KeyboardInterrupt):
                        dropin.main()
                binary, port = interrupted.call_args.args
                self.assertEqual(port, server.server_address[1])
                scratch = Path(binary.root).parent
                self.assertEqual((scratch.exists(), server.fileno(), shutdown.call_count), (keep, -1, 1))
                self.assertEqual(neighbor.read_text(), "keep")
                if keep:
                    self.assertTrue((scratch / "alab").is_dir())


class GitSourceFixture(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="alab-dropin-source-")
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.env = {
            **os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_AUTHOR_NAME": "Fixture", "GIT_AUTHOR_EMAIL": "fixture@example.invalid",
            "GIT_COMMITTER_NAME": "Fixture", "GIT_COMMITTER_EMAIL": "fixture@example.invalid",
        }
        self.git("init", "-q")
        self.enterContext(patch.object(dropin, "REPO", str(self.root)))

    def git(self, *args):
        return subprocess.run(["git", "-C", str(self.root), *args], env=self.env,
                              check=True, capture_output=True, text=True).stdout.strip()

    def commit(self):
        self.git("add", ".")
        self.git("commit", "-q", "-m", "source fixture")
        return self.git("rev-parse", "HEAD")


class CommandAttributionTests(GitSourceFixture):
    def setUp(self):
        super().setUp()
        self.cli = self.root / "codex-rs/cli/src/main.rs"
        self.cli.parent.mkdir(parents=True)
        self.upstream = (
            "#[derive(Debug, clap::Subcommand)]\n"
            "enum DebugSubcommand {\n"
            "    Existing,\n"
            "}\n"
            "enum ConfigLayerSource {\n"
            "    PackagedDefaults { file: String },\n"
            "}\n"
        )
        self.cli.write_text(self.upstream)
        self.base = self.commit()

    def verdict(self, item, source):
        return dropin.attribute("codex-alab", item, self.base, source, "0.163.0",
                                subcommand=True)

    def test_new_marked_command_beats_unrelated_upstream_data_variant(self):
        self.cli.write_text(self.upstream.replace(
            "    Existing,", "    Existing,\n    PackagedDefaults, // ALAB(defaults): inspect\n"))
        source = self.commit()
        self.cli.write_text(self.upstream)
        self.commit()  # The checkout is not the artifact source.
        verdict, good = self.verdict("packaged-defaults", source)
        self.assertTrue(good)
        self.assertTrue(verdict.startswith("ALab:"), verdict)

    def test_inline_cfg_identifies_a_marked_command_on_its_variant_line(self):
        variant = "    #[cfg(unix)] AtermModelPolicy(Args),"
        self.cli.write_text(self.upstream.replace("    Existing,", variant + " // ALAB(fleet-model): scoped command"))
        verdict, good = self.verdict("aterm-model-policy", self.commit())
        self.assertTrue(good, verdict)
        self.cli.write_text(self.upstream.replace("    Existing,", variant))
        verdict, good = self.verdict("aterm-model-policy", self.commit())
        self.assertFalse(good, verdict)

    def test_marking_an_existing_command_does_not_change_its_origin(self):
        self.cli.write_text(self.upstream.replace(
            "    Existing,", "    Existing, // ALAB(test): existing command\n"))
        verdict, good = self.verdict("existing", self.commit())
        self.assertTrue(good)
        self.assertTrue(verdict.startswith("upstream:"), verdict)

    def test_data_variants_handler_arms_and_marker_prose_are_not_commands(self):
        self.cli.write_text(self.upstream + (
            "enum LocalData {\n"
            "    PackagedDefaults, // ALAB(test): data only\n"
            "}\n"
            "fn dispatch() {\n"
            "    DebugSubcommand::PackagedDefaults => (), // ALAB(test): handler only\n"
            "}\n"
            "// ALAB(test): PackagedDefaults, is mentioned in prose only\n"
        ))
        verdict, good = self.verdict("packaged-defaults", self.commit())
        self.assertFalse(good)
        self.assertTrue(verdict.startswith("UNEXPLAINED:"), verdict)

    def test_a_different_upstream_command_with_the_same_name_is_ambiguous(self):
        self.cli.write_text(self.upstream + (
            "#[derive(clap::Subcommand)]\n"
            "enum OtherSubcommand {\n"
            "    PackagedDefaults,\n"
            "}\n"
        ))
        self.base = self.commit()
        self.cli.write_text(self.cli.read_text().replace(
            "    Existing,", "    Existing,\n    PackagedDefaults, // ALAB(test): new\n"))
        verdict, good = self.verdict("packaged-defaults", self.commit())
        self.assertFalse(good)
        self.assertTrue(verdict.startswith("UNEXPLAINED:"), verdict)

    def test_explicit_command_name_is_read_from_its_clap_attribute(self):
        self.cli.write_text(self.upstream.replace("    Existing,", (
            '    #[command(name = "packaged-defaults")]\n'
            "    InspectDefaults, // ALAB(test): explicit name\n"
            "    Existing,"
        )))
        verdict, good = self.verdict("packaged-defaults", self.commit())
        self.assertTrue(good)
        self.assertTrue(verdict.startswith("ALab:"), verdict)

    def test_an_unmarked_same_name_declaration_makes_the_source_ambiguous(self):
        self.cli.write_text(self.upstream.replace(
            "    Existing,", "    Existing,\n    PackagedDefaults, // ALAB(test): new\n") + (
            "#[derive(clap::Subcommand)]\n"
            "enum OtherSubcommand {\n"
            "    PackagedDefaults,\n"
            "}\n"
        ))
        verdict, good = self.verdict("packaged-defaults", self.commit())
        self.assertFalse(good)
        self.assertTrue(verdict.startswith("UNEXPLAINED:"), verdict)

    def test_variants_that_do_not_declare_a_named_command_are_not_evidence(self):
        for attribute in ("flatten", "skip", "external_subcommand"):
            with self.subTest(attribute=attribute):
                self.cli.write_text(self.upstream.replace("    Existing,", (
                    "    #[command(%s)]\n" % attribute +
                    "    PackagedDefaults(CommonCommands), // ALAB(test): unnamed\n"
                    "    Existing,"
                )))
                verdict, good = self.verdict("packaged-defaults", self.commit())
                self.assertFalse(good)
                self.assertTrue(verdict.startswith("UNEXPLAINED:"), verdict)

    def test_non_code_and_other_derives_cannot_declare_commands(self):
        declaration = (
            "#[derive(clap::Subcommand)]\n"
            "enum ProseSubcommand {\n"
            "    PackagedDefaults, // ALAB(test): prose only\n"
            "}\n"
        )
        cases = (
            "/* outer /* nested */\n" + declaration + "*/\n",
            'const EXAMPLE: &str = "\n' + declaration + '";\n',
            'const EXAMPLE: &str = r##"\n' + declaration + '"##;\n',
            declaration.replace("clap::Subcommand", "NotSubcommand"),
            declaration.replace("clap::Subcommand", "other::Subcommand"),
        )
        for index, prose in enumerate(cases):
            with self.subTest(index=index):
                self.cli.write_text(self.upstream + prose)
                verdict, good = self.verdict("packaged-defaults", self.commit())
                self.assertFalse(good)
                self.assertTrue(verdict.startswith("UNEXPLAINED:"), verdict)

    def test_prose_cannot_rename_a_real_command(self):
        for prose in ('// #[command(name = "packaged-defaults")]\n',
                      '/* #[command(name = "packaged-defaults")] */\n'):
            with self.subTest(prose=prose):
                self.cli.write_text(self.upstream.replace("    Existing,", (
                    "    " + prose +
                    "    InspectDefaults, // ALAB(test): actual name is inspect-defaults\n"
                    "    Existing,"
                )))
                verdict, good = self.verdict("packaged-defaults", self.commit())
                self.assertFalse(good)
                self.assertTrue(verdict.startswith("UNEXPLAINED:"), verdict)

    def test_duplicate_enum_and_variant_names_do_not_overwrite_ownership(self):
        modules = ""
        for module, marker in (("first", ""), ("second", " // ALAB(test): new")):
            modules += (
                "mod %s {\n" % module +
                "    #[derive(clap::Subcommand)]\n"
                "    enum Commands {\n"
                "        PackagedDefaults,%s\n" % marker +
                "    }\n"
                "}\n"
            )
        self.cli.write_text(self.upstream + modules)
        verdict, good = self.verdict("packaged-defaults", self.commit())
        self.assertFalse(good)
        self.assertTrue(verdict.startswith("UNEXPLAINED:"), verdict)


class RuntimeAttributionTests(GitSourceFixture):
    def test_feature_flip_requires_exact_boolean_values_at_all_three_sources(self):
        registry = self.root / "codex-rs/features/src/lib.rs"
        registry.parent.mkdir(parents=True)
        entry = ('    FeatureSpec {\n        id: Feature::Fixture,\n        key: "fixture",\n'
                 '        stage: Stage::Stable,\n        default_enabled: false,\n    },\n')
        table = "pub const FEATURES: &[FeatureSpec] = &[\n" + entry + "];\n"
        registry.write_text(table)
        tag = self.commit()
        registry.write_text(table.replace(": false,", ": true,"))
        base = self.commit()
        def accepted(vendor, fork, source=base):
            return dropin.upstream_feature_flip("features.fixture", vendor, fork, tag, base, source)
        self.assertTrue(accepted(False, True))
        self.assertFalse(accepted(True, False))
        self.assertFalse(accepted(0, True))
        self.assertFalse(accepted(False, True, tag))
        for text in (table.replace("Feature::Fixture", "Feature::Other"),
                     table.replace(entry, entry + entry), "/*\n" + table + "*/\n",
                     table.replace(entry, "    #[cfg(test)]\n" + entry),
                     table.replace("default_enabled: false", "default_enabled: cfg!(unix)")):
            registry.write_text(text.replace(": false,", ": true,"))
            self.assertFalse(accepted(False, True, self.commit()))

    def test_bundled_skill_requires_new_upstream_path_and_identical_installed_bytes(self):
        assets = self.root / "codex-rs/skills/src/assets/samples"
        assets.mkdir(parents=True)
        (assets / "old").write_text("existing")
        tag = self.commit()
        asset = assets / "new/part2.md"
        asset.parent.mkdir()
        asset.write_text("versioned bytes")
        base = self.commit()
        home = Path(self.enterContext(tempfile.TemporaryDirectory())).resolve()
        installed = home / "skills/.alab-system/new/part2.md"
        installed.parent.mkdir(parents=True)
        name = "skills/.system/new/part2.md"
        def compare(source):
            return dropin.new_bundled_skills(home, tag, base, source)
        self.assertEqual(compare(base), (set(), [name]))
        installed.write_bytes(asset.read_bytes())
        self.assertEqual(compare(base), ({name}, []))
        (installed.parent / "part3.md").write_text("extra")
        shapes, clashes = dropin.as_vendor_names(dropin.shapes(home))
        self.assertEqual(shapes - compare(base)[0], {"skills/.system/new/part3.md"})
        self.assertFalse(clashes)
        installed.write_text("changed")
        self.assertEqual(compare(base), (set(), [name]))
        asset.write_text("changed")
        self.assertEqual(compare(self.commit()), (set(), [name]))


class TaggedPackageTests(GitSourceFixture):
    def test_vanilla_comparison_accepts_only_the_pinned_tag_on_a_release_leaf(self):
        (self.root / "README").write_text("upstream main")
        branchpoint = self.commit()
        self.git("update-ref", "refs/remotes/upstream/main", branchpoint)
        (self.root / "README").write_text("release-only fix")
        released = self.commit()
        tag = "rust-v1.2.3"
        self.git("tag", "-a", tag, "-m", "official release")
        annotation = self.git("rev-parse", "refs/tags/" + tag)
        ref = "refs/upstream-tags/" + tag
        self.git("update-ref", ref, annotation)
        (self.root / "alab").mkdir()
        (self.root / "alab/upstream-release").write_text(f"{tag} {annotation} {released}\n")
        candidate = self.commit()
        self.assertEqual(dropin.checked_upstream_base(released, {}), branchpoint)
        manifest = {"upstream": {"commit": released}}
        self.assertEqual(dropin.checked_upstream_base(candidate, manifest), released)
        with self.assertRaisesRegex(ValueError, "package records upstream"):
            dropin.checked_upstream_base(candidate, {"upstream": {"commit": branchpoint}})
        self.git("update-ref", "-d", ref)
        with self.assertRaisesRegex(ValueError, "official release tag missing"):
            dropin.checked_upstream_base(candidate, manifest)


class UpstreamBaseTests(unittest.TestCase):
    def test_uses_source_merge_base_for_matching_and_historical_manifests(self):
        base = "a" * 40
        source = "b" * 40
        for manifest in ({"upstream": {"commit": base}}, {}, {"upstream": None}):
            with self.subTest(manifest=manifest):
                with patch.object(dropin, "upstream_base", return_value=base) as lookup:
                    self.assertEqual(dropin.checked_upstream_base(source, manifest), base)
                    lookup.assert_called_once_with(source)

    def test_stamped_package_requires_matching_local_merge_base(self):
        manifest = {"upstream": {"commit": "a" * 40}}
        for computed in ("c" * 40, None):
            with self.subTest(computed=computed), patch.object(dropin, "upstream_base",
                                                               return_value=computed):
                with self.assertRaises(ValueError):
                    dropin.checked_upstream_base("b" * 40, manifest)


class FreshThreadEffortTests(unittest.TestCase):
    def test_reads_packaged_effort_from_an_unpinned_ephemeral_thread(self):
        with tempfile.TemporaryDirectory() as scratch:
            binary = SimpleNamespace(root=scratch)
            for expected in ("ultra", "high"):
                with self.subTest(expected=expected):
                    result = {"model": "gpt-6.2-sol", "reasoningEffort": expected}
                    with patch.object(dropin, "app_server", return_value={2: {"result": result}}) as rpc:
                        good, detail = dropin.fresh_thread_effort_check(binary, expected)
                    self.assertTrue(good, detail)
                    rpc.assert_called_once()
                    target, requests = rpc.call_args.args
                    self.assertIs(target, binary)
                    [(method, params)] = requests
                    self.assertEqual(method, "thread/start")
                    self.assertIs(params["ephemeral"], True)
                    self.assertFalse({"model", "config", "reasoningEffort"} & params.keys())
                    self.assertEqual(params.get("cwd", scratch), scratch)

    def test_requires_the_effective_thread_effort(self):
        for reply in ({}, {2: {"error": {"message": "thread unavailable"}}},
                      {2: {"result": {"model": "gpt-6.2-sol", "defaultReasoningEffort": "ultra"}}},
                      {2: {"result": {"model": "gpt-6.2-sol", "reasoningEffort": "low",
                                      "defaultReasoningEffort": "ultra"}}}):
            with self.subTest(reply=reply), patch.object(dropin, "app_server", return_value=reply):
                good, detail = dropin.fresh_thread_effort_check(SimpleNamespace(root="/scratch"), "ultra")
                self.assertFalse(good, detail)


class OwnNamesTests(unittest.TestCase):
    """codex-alab's own names in the shared CODEX_HOME compare as the vendor's, and a write
    under the vendor's own name is a clash."""

    def test_own_names_compare_as_the_vendors(self):
        renamed, clashes = dropin.as_vendor_names({
            "skills/.alab-system/skill-creator/SKILL.md",
            "skills/.alab-system/.codex-system-skills.marker",
            "models_cache.alab.json",
            "config.toml",
        })
        self.assertEqual(renamed, {
            "skills/.system/skill-creator/SKILL.md",
            "skills/.system/.codex-system-skills.marker",
            "models_cache.json",
            "config.toml",
        })
        self.assertEqual(clashes, [])

    def test_a_write_under_the_vendors_own_name_is_a_clash(self):
        renamed, clashes = dropin.as_vendor_names({
            "skills/.system/imagegen/SKILL.md",
            "models_cache.json",
        })
        self.assertEqual(clashes, ["models_cache.json", "skills/.system/imagegen/SKILL.md"])
        self.assertEqual(renamed, set(clashes))


if __name__ == "__main__":
    unittest.main()

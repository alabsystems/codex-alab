#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Command provenance uses clap declarations at the artifact's source commit; codex-alab's
own CODEX_HOME names compare as the vendor's."""

import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


SMOKE = Path(__file__).resolve().parents[1] / "smoke"
sys.path.insert(0, str(SMOKE))
SPEC = importlib.util.spec_from_file_location("dropin", SMOKE / "dropin.py")
dropin = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(dropin)


class CommandAttributionTests(unittest.TestCase):
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
        repo = patch.object(dropin, "REPO", str(self.root))
        repo.start()
        self.addCleanup(repo.stop)

    def git(self, *args):
        return subprocess.run(["git", "-C", str(self.root), *args], env=self.env,
                              check=True, capture_output=True, text=True).stdout.strip()

    def commit(self):
        self.git("add", ".")
        self.git("commit", "-q", "-m", "source fixture")
        return self.git("rev-parse", "HEAD")

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

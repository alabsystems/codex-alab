#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Verify release schema attribution against the Rust sources in the two git trees."""

from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import dropin


class NotificationAttributionTests(unittest.TestCase):
    method = "alab/retainedFrontier"
    schema = "ServerNotification.json method"
    row = '    Frontier => "alab/retainedFrontier" (v2::Frontier), // ALAB(test): expose it.'

    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name)
        self.protocol = self.root / "codex-rs/app-server-protocol/src/protocol/common.rs"
        self.protocol.parent.mkdir(parents=True)
        self.git("init", "-q")
        repo = patch.object(dropin, "REPO", str(self.root))
        repo.start()
        self.addCleanup(repo.stop)
        self.base = self.save(self.macro(""))

    def git(self, *args):
        return subprocess.run(["git", "-c", "user.name=Fixture", "-c", "user.email=test@example.invalid",
                               "-C", str(self.root), *args], check=True, capture_output=True,
                              text=True).stdout.strip()

    def save(self, text):
        self.protocol.write_text(text)
        self.git("add", ".")
        return self.git("write-tree")

    @staticmethod
    def macro(body):
        return f"server_notification_definitions! {{\n{body}\n}}\n"

    def test_attribution_requires_the_exact_schema_path_and_artifact_source(self):
        source = self.save(self.macro(self.row))
        self.protocol.write_text("")  # Changing the checkout cannot remove release evidence.
        for path in (self.schema, "codex_app_server_protocol.schemas.json ServerNotification.method",
                     "codex_app_server_protocol.v2.schemas.json ServerNotification.method"):
            self.assertTrue(dropin.alab_notification_addition(f"{path}={self.method}", self.base, source))
        for path in ("ClientNotification.json method", "ServerNotification.json nested.method"):
            self.assertFalse(dropin.alab_notification_addition(f"{path}={self.method}", self.base, source))

    def test_not_a_marked_declaration(self):
        for text in (self.macro("// " + self.row), self.macro("/*\n" + self.row + "\n*/"),
                     'const S: &str = r#"\n' + self.macro(self.row) + '"#;',
                     self.macro(self.row).replace("server_notification", "client_notification"),
                     self.macro(self.row.replace("// ALAB(test): expose it.", "")),
                     self.macro(self.row.replace("// ALAB(test): expose it.", "/* ALAB(test): expose it */")),
                     self.macro(self.row.replace("alab/retainedFrontier", "alab/different"))):
            with self.subTest(text=text):
                source = self.save(text)
                self.assertFalse(dropin.alab_notification_addition(
                    f"{self.schema}={self.method}", self.base, source))

    def test_upstream_wire_name_precludes_a_claim_across_different_declaration_layouts(self):
        base = self.save(self.macro('    PriorName =>\n        "alab/retainedFrontier" (v1::Prior),'))
        source = self.save(self.macro(self.row))
        self.assertFalse(dropin.alab_notification_addition(f"{self.schema}={self.method}", base, source))


if __name__ == "__main__":
    unittest.main()

# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Compare actual binary fields with the committed source, merge base and vendor exports."""
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import dropin
import dropin_schema


BUNDLE = "codex_app_server_protocol.schemas.json"
FILE = "ServerRequest.json"
TAG = "refs/upstream-tags/rust-v1.2.3"


def object_with(*fields):
    return {"type": "object", "properties": {name: {"type": "string"} for name in fields}}


def exports(**definitions):
    return {
        FILE: {"definitions": definitions},
        BUNDLE: {"definitions": {"v2": definitions}},
        **{f"{name}.json": schema for name, schema in definitions.items()},
    }


class SchemaCheckTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory(prefix="alab-dropin-schema-")
        self.addCleanup(scratch.cleanup)
        self.roots = [Path(scratch.name) / side for side in ("alab", "vendor")]
        self.binaries = [SimpleNamespace(label=root.name, root=root, version="1.2.3",
                                         run=lambda argv: SimpleNamespace(returncode=0))
                         for root in self.roots]
        self.shared = {"Other": object_with("threadId", "action")}
        self.release = exports(**self.shared, Clock=object_with("threadId"))
        self.pristine = exports(**self.shared)
        self.candidate = exports(**self.shared)
        self.write_actual(self.candidate, self.release)

    def write_actual(self, alab, vendor):
        for root, schemas in zip(self.roots, (alab, vendor)):
            path = root / "schema"
            path.mkdir(exist_ok=True, parents=True)
            for old in path.glob("*.json"):
                old.unlink()
            for file, schema in schemas.items():
                (path / file).write_text(json.dumps(schema))

    def check(self):
        archives = {"source": self.candidate, "base": self.pristine, TAG: self.release}
        def read(_repo, revision):
            return {file: json.dumps(schema) for file, schema in archives[revision].items()}
        with patch.object(dropin_schema, "experimental_schemas", read):
            good, rows, _ = dropin.schema_check(*self.binaries, "base", "source")
        return good, {row[1]: row[-1] for row in rows}

    def test_removing_a_complete_type_attributes_its_common_field_in_each_export(self):
        good, reasons = self.check()
        self.assertTrue(good, reasons)
        for item in ("Clock.json threadId", FILE + " Clock.threadId", BUNDLE + " v2.Clock.threadId"):
            self.assertTrue(reasons[item].startswith("upstream: present in vendor"), reasons[item])
        self.assertNotIn(BUNDLE + " v2.Other.threadId", reasons)

    def test_vendor_field_present_in_upstream_and_removed_only_by_fork_fails(self):
        self.candidate = exports(Other=object_with("action"))
        self.write_actual(self.candidate, self.release)
        good, reasons = self.check()
        self.assertFalse(good)
        self.assertTrue(reasons[FILE + " Other.threadId"].startswith("UNEXPLAINED"))

    def test_binary_missing_a_committed_field_or_adding_a_fabricated_field_fails(self):
        for fields, changed in [(("action",), "threadId"), (("threadId", "action", "extra"), "extra")]:
            with self.subTest(fields=fields):
                self.write_actual(exports(Other=object_with(*fields)), self.release)
                good, reasons = self.check()
                self.assertFalse(good)
                self.assertTrue(reasons[FILE + " Other." + changed].startswith("UNEXPLAINED"))

    def test_common_field_on_fork_owned_payload_requires_that_owner(self):
        self.candidate = exports(**self.shared, Frontier=object_with("threadId"))
        self.write_actual(self.candidate, self.release)
        def declarations(_repo, revision, owner, paths):
            return ("",) if revision == "source" and owner == "Frontier" and list(paths) == dropin.ALAB else ()
        with patch.object(dropin, "schema_declarations", declarations):
            good, reasons = self.check()
            self.assertTrue(good, reasons)
            for item in ("Frontier.json threadId", FILE + " Frontier.threadId", BUNDLE + " v2.Frontier.threadId"):
                self.assertIn("ALab's Frontier payload", reasons[item])
        with patch.object(dropin, "schema_declarations", return_value=()):
            good, reasons = self.check()
            self.assertFalse(good)
            self.assertTrue(reasons["Frontier.json threadId"].startswith("UNEXPLAINED"))

    def test_existing_type_requires_new_marked_field_or_enum_variant_in_that_type(self):
        decls = {("base", "Other"): ("    pub id: String,\n",),
                 ("source", "Other"): ("    pub id: String,\n    pub reason: String, // ALAB(test): expose\n",)}
        def declarations(_repo, revision, owner, _paths):
            return decls.get((revision, owner), ())
        with patch.object(dropin, "schema_declarations", declarations):
            reason = dropin.fork_schema_reason("Other.json reason", "base", "source")
            self.assertIn("marked Other extension", reason)
            self.assertIsNone(dropin.fork_schema_reason("Other.json id", "base", "source"))
            decls["source", "Other"] = ("    pub reason: String,\n",)
            self.assertIsNone(dropin.fork_schema_reason("Other.json reason", "base", "source"))
            decls["source", "Other"] = ("    // ALAB(test): add wire value\n    BioPolicy,\n",)
            self.assertIn("marked Other extension", dropin.fork_schema_reason("Other.json =bioPolicy", "base", "source"))

    def test_missing_or_invalid_archive_fails_closed(self):
        for archives in ({}, {BUNDLE: "bad json"}, {BUNDLE: "[]"}, {BUNDLE: "{}"}):
            with self.subTest(archives=archives), patch.object(dropin_schema, "experimental_schemas",
                                                               return_value=archives):
                good, rows, _ = dropin.schema_check(*self.binaries, "base", "source")
                self.assertFalse(good)
                self.assertIn("export", rows[0][-1])

# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import importlib.util
import os
from pathlib import Path
import tempfile
import unittest

script = Path(__file__).resolve().parents[1] / "retain_release_controller.py"
spec = importlib.util.spec_from_file_location("retain_release_controller", script)
controller = importlib.util.module_from_spec(spec)
spec.loader.exec_module(controller)


class RetainedControllerTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.source = Path(scratch.name) / "compiler"
        self.out = Path(scratch.name) / "owner"
        self.out.mkdir()
        self.source.mkdir()
        for name in ("metadata.json", "artifacts.jsonl", "codex-release"):
            (self.source / name).write_bytes(name.encode())
        (self.source / "codex-release").chmod(0o500)

    def test_retains_independent_executable_and_receipts(self):
        controller.retain(self.source, self.out)
        retained = self.out / "codex-release"
        (self.source / "codex-release").unlink()
        (self.source / "codex-release").write_bytes(b"later")
        self.assertEqual(retained.read_bytes(), b"codex-release")
        self.assertEqual(retained.stat().st_mode & 0o777, 0o500)
        self.assertEqual((self.out / "artifacts.jsonl").read_bytes(), b"artifacts.jsonl")

    def test_owner_outputs_are_create_only(self):
        kept = self.out / "metadata.json"
        kept.write_bytes(b"existing")
        with self.assertRaises(FileExistsError):
            controller.retain(self.source, self.out)
        self.assertEqual(kept.read_bytes(), b"existing")
        self.assertFalse((self.out / "codex-release").exists())

    def test_duplicate_link_is_refused_before_kernel_probe(self):
        os.link(self.source / "codex-release", self.source / "other")
        with self.assertRaisesRegex(ValueError, "owned regular compiler output"):
            controller.retain(self.source, self.out)
        self.assertFalse((self.out / "codex-release").exists())


if __name__ == "__main__":
    unittest.main()

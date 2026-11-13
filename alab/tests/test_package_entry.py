#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The shell entry binds source before handing unchanged arguments to native selection."""
import json
from pathlib import Path
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]
SOURCE = "1" * 40


class PackageEntryTests(unittest.TestCase):
    def entry(self, *arguments, refuse=False):
        # Stop at the native boundary: this tests shell argument handling, not
        # compiler admission, cache ownership or production source qualification.
        body = r'''
. "$1/alab/package.sh"
fixture_python="$2"
shift 2
alab_python_311() { printf '%s\n' "$fixture_python"; }
alab_project_source() { SOURCE_BODY; }
alab_project_run() {
  "$fixture_python" -I -c 'import json,sys; print(json.dumps(sys.argv[1:]))' "$@"
}
package_entry "$@"
'''.replace("SOURCE_BODY", "return 17" if refuse else "printf '%s\\n' '" + SOURCE + "'")
        return subprocess.run(
            ["/bin/bash", "-c", body, "package-entry-fixture", str(ROOT), sys.executable, *arguments],
            capture_output=True, text=True, timeout=30,
        )

    def test_no_arguments_reaches_native_selection_with_exact_source(self):
        done = self.entry()
        self.assertEqual(done.returncode, 0, done.stderr)
        arguments = json.loads(done.stdout)
        self.assertEqual(arguments[:2], [str(ROOT), SOURCE])
        self.assertEqual(arguments[-3:], [str(ROOT / "alab/package.sh"), "--commit", SOURCE])

    def test_original_arguments_keep_spaces_and_order(self):
        original = ["--out", "/a path with spaces", "--keep", "2", "--commit", "main"]
        done = self.entry(*original)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(json.loads(done.stdout)[-len(original)-2:], original + ["--commit", SOURCE])

    def test_source_refusal_never_reaches_native_selection(self):
        done = self.entry(refuse=True)
        self.assertEqual(done.returncode, 17, done.stderr)
        self.assertEqual(done.stdout, "")


if __name__ == "__main__":
    unittest.main()

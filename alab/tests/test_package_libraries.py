#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise the real package smoke loop with retained, inert inspector fixtures."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
BINARIES = ("bin/codex", "bin/codex-code-mode-host", "bin/codex-exec-server", "codex-path/rg")


class PackageLibrariesTests(unittest.TestCase):
    def setUp(self):
        self.home = Path(tempfile.mkdtemp(prefix="alab-package-libraries-")).resolve()
        print(f"retained library fixture: {self.home}", file=sys.stderr)
        self.staged = self.home / "stage with spaces and \\backslash"
        for binary in BINARIES:
            path = self.staged / binary
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"inert fixture; never executed")
        source = (ROOT / "alab/package.sh").read_text()
        start = source.index('for f in "${package_first_party[@]/#/bin/}" codex-path/rg; do\n')
        end = source.index("\ndone\nupstream_py", start) + len("\ndone\n")
        self.loop = source[start:end]
        self.inspector = self.home / "inspector.py"
        self.inspector.write_text('''import json, os, sys
from pathlib import Path
assert sys.argv[1] == "-L" and len(sys.argv) == 3
binary = sys.argv[2]
with open(os.environ["FIXTURE_CALLS"], "a") as calls:
    print(json.dumps(sys.argv[1:]), file=calls)
mode = os.environ["FIXTURE_MODE"] if binary.endswith(os.environ["FIXTURE_TARGET"]) else "valid"
header = binary + ":"
row = "\\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1359.0.0)"
outputs = {
    "valid": header + "\\n" + row + "\\n\\t/System/Library/Frameworks/Foundation.framework/Versions/C/Foundation (compatibility version 300.0.0, current version 5027.0.69)\\n",
    "failed_valid": header + "\\n" + row + "\\n",
    "empty": "", "failed_empty": "", "header_only": header + "\\n",
    "wrong_header": "another-binary:\\n" + row + "\\n",
    "truncated": header + "\\n\\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version ",
    "bare_path": header + "\\n\\t/usr/lib/libSystem.B.dylib\\n",
    "garbage": header + "\\n" + row + "\\nnot a dependency\\n",
    "external": header + "\\n\\t/opt/homebrew/lib/libpcre2.dylib (compatibility version 1.0.0, current version 1.0.0)\\n",
    "rpath": header + "\\n\\t@rpath/libpcre2.dylib (compatibility version 1.0.0, current version 1.0.0)\\n",
    "short_version": header + "\\n" + row.replace("1.0.0", "1") + "\\n",
    "long_version": header + "\\n" + row.replace("1.0.0", "1.0.0.0") + "\\n",
    "bad_current_version": header + "\\n" + row.replace("1359.0.0", "1359.0") + "\\n",
    "unknown_flag": header + "\\n" + row[:-1] + ", unknown)\\n",
    "traversal": header + "\\n" + row.replace("libSystem.B.dylib", "../../tmp/lib.dylib") + "\\n",
    "parent": header + "\\n" + row.replace("libSystem.B.dylib", "..") + "\\n",
    "system_path_chars": header + "\\n" + row.replace("libSystem", "dir with spaces/\\\\libSystem") + "\\n",
}
for flag in ("weak", "reexport", "upward", "lazy"):
    outputs[flag] = header + "\\n" + row[:-1] + ", " + flag + ")\\n"
sys.stdout.write(outputs[mode])
sys.exit(17 if mode.startswith("failed_") else 0)
''')
        self.attempt = 0

    def smoke(self, mode="valid", target=BINARIES[0], codesign_rc=0, file_rc=0):
        self.attempt += 1
        attempt = self.home / f"attempt-{self.attempt}"
        attempt.mkdir()
        accepted = attempt / "accepted"
        calls = attempt / "inspector-calls.jsonl"
        env = {
            "PATH": "/usr/bin:/bin", "HOME": str(self.home), "TMPDIR": str(self.home),
            "LC_ALL": "C", "FIXTURE_MODE": mode, "FIXTURE_TARGET": target,
            "FIXTURE_CALLS": str(calls), "FIXTURE_CODESIGN_RC": str(codesign_rc),
            "FIXTURE_FILE_RC": str(file_rc),
        }
        command = '''set -euo pipefail
. "$1/alab/package.sh"
staged="$2"
fixture_python="$3"
fixture_inspector="$4"
fixture_accepted="$5"
package_first_party=(codex codex-code-mode-host codex-exec-server)
codesign() { return "$FIXTURE_CODESIGN_RC"; }
file() { printf '%s\\n' 'Mach-O 64-bit executable arm64'; return "$FIXTURE_FILE_RC"; }
otool() { "$fixture_python" "$fixture_inspector" "$@"; }
''' + self.loop + '\nprintf accepted > "$fixture_accepted"\n'
        done = subprocess.run(
            ["/bin/bash", "-c", command, "library-fixture", str(ROOT), str(self.staged),
             sys.executable, str(self.inspector), str(accepted)],
            env=env, capture_output=True, text=True, timeout=30)
        (attempt / "stdout").write_text(done.stdout)
        (attempt / "stderr").write_text(done.stderr)
        observed = [json.loads(line) for line in calls.read_text().splitlines()] if calls.exists() else []
        (attempt / "result.json").write_text(json.dumps({
            "exit": done.returncode, "accepted": accepted.exists(), "calls": observed,
        }, indent=2) + "\n")
        return done, accepted.exists(), observed

    def assert_refused(self, mode, target=BINARIES[0]):
        done, accepted, calls = self.smoke(mode, target)
        self.assertNotEqual(done.returncode, 0, done.stdout + done.stderr)
        self.assertFalse(accepted, "failed inspection must stop before publication")
        self.assertEqual([call[1] for call in calls],
                         [str(self.staged / binary) for binary in BINARIES[:BINARIES.index(target) + 1]])
        return done

    def test_failed_inspector_with_plausible_output_stops_every_binary(self):
        for target in BINARIES:
            with self.subTest(target=target):
                done = self.assert_refused("failed_valid", target)
                self.assertIn("17", done.stderr)

    def test_failed_empty_inspector_is_refused(self):
        self.assert_refused("failed_empty")

    def test_successful_but_unproven_listings_are_refused(self):
        for mode in ("empty", "header_only", "wrong_header", "truncated", "bare_path", "garbage",
                     "short_version", "long_version", "bad_current_version", "unknown_flag"):
            with self.subTest(mode=mode):
                self.assert_refused(mode)

    def test_non_system_dependencies_are_refused(self):
        for mode in ("external", "rpath", "traversal", "parent"):
            for target in BINARIES:
                with self.subTest(mode=mode, target=target):
                    self.assert_refused(mode, target)

    def test_complete_system_listings_accept_every_binary_with_path_characters(self):
        done, accepted, calls = self.smoke()
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertTrue(accepted)
        self.assertEqual(calls, [["-L", str(self.staged / binary)] for binary in BINARIES])
        self.assertEqual(done.stdout.count("system libraries only"), len(BINARIES))

    def test_complete_system_dependency_variants_are_accepted(self):
        for mode in ("weak", "reexport", "upward", "lazy", "system_path_chars"):
            with self.subTest(mode=mode):
                done, accepted, calls = self.smoke(mode)
                self.assertEqual(done.returncode, 0, done.stderr)
                self.assertTrue(accepted)
                self.assertEqual(len(calls), len(BINARIES))

    def test_signature_failure_stops_before_library_inspection(self):
        done, accepted, calls = self.smoke(codesign_rc=19)
        self.assertNotEqual(done.returncode, 0)
        self.assertFalse(accepted)
        self.assertEqual(calls, [])

    def test_file_failure_stops_before_library_inspection(self):
        done, accepted, calls = self.smoke(file_rc=23)
        self.assertEqual(done.returncode, 23)
        self.assertFalse(accepted)
        self.assertEqual(calls, [])


if __name__ == "__main__":
    unittest.main()

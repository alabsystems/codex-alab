# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Proxy unit runs must never create managed CA state in the owner's home."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


UNIT_TESTS = Path(__file__).resolve().parents[1] / 'unit-tests.sh'


class ProxyHomeTests(unittest.TestCase):
    def run_fixture(self, package, exit_code):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            tree = root / 'tree'
            (tree / 'codex-rs').mkdir(parents=True)
            target = root / 'target with spaces'
            target.mkdir()
            owner = root / 'owner'
            owner.mkdir()
            receipt = root / 'observed-home'
            cargo = root / 'fixture-cargo'
            cargo.write_text('''#!/bin/bash
set -eu
printf '%s' "$CODEX_HOME" > "$PROXY_HOME_RECEIPT"
test -d "$CODEX_HOME"
exit "$PROXY_HOME_EXIT"
''')
            cargo.chmod(0o755)
            env = dict(os.environ, CODEX_HOME=str(owner),
                       PROXY_HOME_RECEIPT=str(receipt), PROXY_HOME_EXIT=str(exit_code))
            result = subprocess.run([
                '/bin/bash', '-c', '''
source "$1"
alab_build_command() { "$@"; }
cargo="$2"
packages=(-p "$5")
unit_tests "$3" "$4"
''', 'proxy-home-fixture', str(UNIT_TESTS), str(cargo), str(tree), str(target), package,
            ], env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, exit_code, result.stderr)
            observed = Path(receipt.read_text())
            self.assertEqual(list(owner.iterdir()), [])
            if package == 'codex-network-proxy':
                self.assertEqual(observed.parent, target)
                self.assertFalse(observed.exists(), 'test home leaked after unit run')
            else:
                self.assertEqual(observed, owner)

    def test_proxy_home_is_isolated_and_removed_on_success_and_failure(self):
        for status in (0, 101):
            with self.subTest(status=status):
                self.run_fixture('codex-network-proxy', status)

    def test_other_package_keeps_its_requested_environment(self):
        self.run_fixture('codex-protocol', 0)


if __name__ == '__main__':
    unittest.main()

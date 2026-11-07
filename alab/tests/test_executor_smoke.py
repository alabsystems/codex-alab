#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise the package smoke over actual private child stdio, without native builds."""

import asyncio
import importlib.util
import json
import os
from pathlib import Path
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
CLIENT = "0.166.0-dev.1+gaaaaaaaaa"
PRODUCT = "0.1.0-dev.1+codex.0.165.0.gaaaaaaaaa"
PYTHON_ENTRY = "#!/bin/sh\n'''exec' " + shlex.quote(sys.executable) + ' -I "$0" "$@"\n\' \'\'\'\n'


class ExecutorSmokeTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="alab-executor-smoke-")).resolve()
        print(f"retained executor-smoke fixture: {self.root}", file=sys.stderr)
        self.package = self.root / "package with spaces"
        self.home = self.root / "private home"
        (self.package / "bin").mkdir(parents=True)
        self.home.mkdir()
        (self.package / "codex-package.json").write_text(json.dumps(
            {"version": PRODUCT, "execServer": "bin/codex-exec-server"}))
        executable = self.package / "bin/codex-exec-server"
        executable.write_text(PYTHON_ENTRY + r'''
import json, os, sys
from pathlib import Path
home = Path.home()
(home / "pid").write_text(str(os.getpid()))
assert sys.argv[1:] == ["--listen", "stdio"]
assert os.environ["TMPDIR"] == str(home)
assert os.environ["CODEX_HOME"] == str(home / "codex")
assert not any(name.startswith(("ANTHROPIC_", "OPENAI_", "CODEX_ALAB_")) for name in os.environ)
scenario = (home / "scenario").read_text()
client = "0.166.0-dev.1+gaaaaaaaaa"
environment = {"executorVersion": client, "capabilities": {"fixture": True}}
for line in sys.stdin:
    request = json.loads(line)
    method = request["method"]
    if method == "initialized":
        continue
    if method == "initialize":
        result = {"sessionId": "private-session", "environmentInfo": environment}
        if scenario == "product-in-protocol":
            result["environmentInfo"]["executorVersion"] = "0.1.0-dev.1+codex.0.165.0.gaaaaaaaaa"
        if scenario == "missing-session":
            result.pop("sessionId")
    elif method == "environment/info":
        result = dict(environment)
        if scenario == "identity-changed":
            result["executorVersion"] = "0.167.0"
    else:
        raise AssertionError(method)
    if scenario == "invalid-json":
        print("not json", flush=True)
    elif scenario == "refused":
        print(json.dumps({"id": request["id"], "error": {"code": -1}}), flush=True)
    else:
        print(json.dumps({"id": request["id"], "result": result}), flush=True)
''')
        executable.chmod(0o755)
        spec = importlib.util.spec_from_file_location("executor_smoke", ROOT / "alab/executor_smoke.py")
        self.smoke = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.smoke)

    def run_smoke(self, scenario):
        (self.home / "scenario").write_text(scenario)
        return asyncio.run(self.smoke.check(self.package, self.home, CLIENT))

    def assert_reaped(self):
        pid = int((self.home / "pid").read_text())
        with self.assertRaises(ProcessLookupError):
            os.kill(pid, 0)

    def test_client_identity_can_differ_from_product_and_survive_refresh(self):
        self.run_smoke("valid")
        self.assert_reaped()

    def test_bad_identity_or_response_is_refused_and_child_reaped(self):
        for scenario in ("product-in-protocol", "identity-changed", "missing-session",
                         "invalid-json", "refused"):
            with self.subTest(scenario=scenario):
                with self.assertRaises(RuntimeError):
                    self.run_smoke(scenario)
                self.assert_reaped()

    def test_missing_executor_metadata_never_starts_a_child(self):
        (self.package / "codex-package.json").write_text(json.dumps({"version": PRODUCT}))
        with self.assertRaises(ValueError):
            self.run_smoke("valid")
        self.assertFalse((self.home / "pid").exists())

    def test_deadline_failure_kills_and_reaps_an_actual_child(self):
        create = asyncio.create_subprocess_exec
        children = []

        async def capture(*args, **kwargs):
            child = await create(*args, **kwargs)
            children.append(child)
            return child

        # Inject the deadline fault after actual spawn, without a shorter timer.
        with (mock.patch.object(self.smoke.asyncio, "create_subprocess_exec", side_effect=capture),
              mock.patch.object(self.smoke.asyncio, "timeout", side_effect=TimeoutError)):
            with self.assertRaisesRegex(RuntimeError, "one-minute hang detector"):
                self.run_smoke("valid")
        self.assertEqual(len(children), 1)
        self.assertIsNotNone(children[0].returncode)
        with self.assertRaises(ProcessLookupError):
            os.kill(children[0].pid, 0)

    @unittest.skipUnless(Path("/usr/bin/sandbox-exec").is_file(), "requires macOS Seatbelt")
    def test_canonical_driver_smoke_is_explicitly_readable_from_separate_target(self):
        driver = self.root / "canonical driver"
        (driver / "alab").mkdir(parents=True)
        shutil.copyfile(ROOT / "alab/executor_smoke.py", driver / "alab/executor_smoke.py")
        target = self.root / "target checkout"
        target.mkdir()
        for repo in (driver, target):
            subprocess.run(["git", "init", "--quiet", str(repo)], check=True, timeout=60)
        cargo_home = self.root / "cargo-home"
        cargo_home.mkdir()
        (cargo_home / ".alab-seeded").touch()
        (self.home / "scenario").write_text("valid")
        environment = {key: value for key, value in os.environ.items() if not key.startswith("ALAB_")}
        # The profile denies reads beneath its declared home. OS temporary paths
        # otherwise remain readable, so the missing-read control needs this home
        # to contain both checkouts rather than depending on tempfile's location.
        environment.update(HOME=str(self.root), ALAB_CONTAIN="1",
                           ALAB_CARGO_HOME=str(cargo_home), ALAB_PYTHON=sys.executable)
        command = r'''set -euo pipefail
. "$1/alab/package.sh"
repo_root="$2"
wt="$3"
staged="$4"
codex_home="$5"
python="$6"
client_version="$7"
fixture_drop_read="$8"
package_contained() {
  if [[ "$fixture_drop_read" == 1 ]]; then
    [[ "$1" == --read && "$2" == "$repo_root/alab" ]] || return 92
    shift 2
  fi
  alab_contained "$@"
}
package_executor_smoke
'''
        for drop_read in ("1", "0"):
            with self.subTest(drop_read=drop_read):
                child = subprocess.Popen(
                    ["/bin/bash", "-c", command, "canonical-driver-smoke", str(ROOT), str(driver),
                     str(target), str(self.package), str(self.home), sys.executable, CLIENT, drop_read],
                    env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                    text=True, start_new_session=True)
                try:
                    stdout, stderr = child.communicate(timeout=60)
                except subprocess.TimeoutExpired:
                    try:
                        os.killpg(child.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    child.communicate(timeout=60)
                    self.fail("contained canonical-driver smoke exceeded its one-minute hang detector")
                if drop_read == "1":
                    self.assertNotEqual(child.returncode, 0, stdout + stderr)
                    self.assertIn(str(driver / "alab/executor_smoke.py"), stderr)
                    self.assertFalse((self.home / "pid").exists())
                else:
                    self.assertEqual(child.returncode, 0, stdout + stderr)
                    self.assertIn("protocol client version: " + CLIENT, stdout)
                    self.assert_reaped()


if __name__ == "__main__":
    unittest.main()

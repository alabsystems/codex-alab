#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Real gate shell functions, with retained fake Cargo and resolver boundaries."""

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
PYTHON_ENTRY = "#!/bin/sh\n'''exec' " + shlex.quote(sys.executable) + ' -I "$0" "$@"\n\' \'\'\'\n'
FAKE_CARGO = r'''import json, os, sys
from pathlib import Path
args = sys.argv[1:]
tree = str(Path.cwd().parent)
stage = ("query" if args == ["--version", "--verbose"] else
         "resource-refusal" if args == ["-vV"] else
         next((x for x in ("metadata", "tippy", "build", "test") if x in args), "unknown"))
record = {"args": args, "tree": tree, "stage": stage,
          "archive": os.environ.get("RUSTY_V8_ARCHIVE"),
          "binding": os.environ.get("RUSTY_V8_SRC_BINDING_PATH"),
          "codex_home": os.environ.get("CODEX_HOME")}
with open(os.environ["ALAB_FAKE_CARGO_LOG"], "a") as stream:
    stream.write(json.dumps(record) + "\n")
if stage == "query":
    print("cargo fixture\nhost: fixture-native-target")
    raise SystemExit(0)
if stage == "resource-refusal":
    print("fake atpkg: unrecognized query needs an owner record in a read-only sandbox", file=sys.stderr)
    raise SystemExit(75)
if stage not in ("metadata", "tippy", "build", "test"):
    raise SystemExit("unexpected fake Cargo command")
for key in ("RUSTY_V8_ARCHIVE", "RUSTY_V8_SRC_BINDING_PATH"):
    path = Path(os.environ.get(key, "/missing"))
    expected = Path(os.environ["ALAB_CARGO_HOME"]) / "alab-v8-inputs"
    if (not path.is_file() or path.parent.parent != expected or
            json.loads(path.read_text()) != {"repo": tree, "target": "fixture-native-target"}):
        print("fake Cargo: missing verified V8 pair: " + key, file=sys.stderr)
        raise SystemExit(61)
providers = {
    "test_stdio_server": ("codex-rmcp-client", "rmcp-client", "src/bin/test_stdio_server.rs"),
    "codex-code-mode-host": ("codex-code-mode-host", "code-mode-host", "src/main.rs"),
}
def package(name, directory, targets):
    root = Path(tree) / "codex-rs" / directory
    return {"id": "path+" + root.as_uri() + "#" + name + "@0.0.0", "name": name,
            "version": "0.0.0", "manifest_path": str(root / "Cargo.toml"), "targets": targets}
def binary_target(binary, directory, source):
    return {"kind": ["bin"], "name": binary,
            "src_path": str(Path(tree) / "codex-rs" / directory / source), "test": True}
if stage == "metadata":
    if os.environ.get("ALAB_FAKE_HELPER_FAILURE") == "metadata":
        print("fixture helper metadata unavailable", file=sys.stderr)
        raise SystemExit(73)
    packages = [package(name, directory, [binary_target(binary, directory, source)])
                for binary, (name, directory, source) in providers.items()]
    for directory, name in (("core", "codex-core"), ("network-proxy", "codex-network-proxy")):
        packages.append(package(name, directory, [{"kind": ["lib"],
            "name": name.replace("-", "_"),
            "src_path": str(Path(tree) / "codex-rs" / directory / "src/lib.rs"), "test": True}]))
    print(json.dumps({"workspace_root": str(Path.cwd()),
                      "target_directory": os.environ["CARGO_TARGET_DIR"],
                      "workspace_members": [item["id"] for item in packages], "packages": packages}))
if stage == "build":
    target = Path(os.environ["CARGO_TARGET_DIR"])
    selected = [args[index + 1] for index, arg in enumerate(args) if arg == "--bin"]
    selected_packages = [args[index + 1] for index, arg in enumerate(args) if arg == "-p"]
    for binary in selected:
        name, directory, source = providers[binary]
        if name not in selected_packages:
            raise SystemExit("helper binary has no selected provider: " + binary)
        executable = target / "fake helpers" / binary
        executable.parent.mkdir(parents=True, exist_ok=True)
        executable.write_text("#!/bin/sh\nexit 0\n")
        executable.chmod(0o755)
        provider = package(name, directory, [binary_target(binary, directory, source)])
        test_profile = (os.environ.get("ALAB_FAKE_HELPER_FAILURE") == "selection"
                        and binary == "test_stdio_server")
        print(json.dumps({"reason": "compiler-artifact", "package_id": provider["id"],
                          "manifest_path": provider["manifest_path"], "target": provider["targets"][0],
                          "profile": {"test": test_profile}, "executable": str(executable)}))
if stage == "test":
    for key in ("CARGO_BIN_EXE_test_stdio_server", "CARGO_BIN_EXE_codex-code-mode-host",
                "CARGO_BIN_EXE_codex_code_mode_host"):
        if not Path(os.environ.get(key, "/missing")).is_file():
            raise SystemExit("missing native helper override: " + key)
    print("test result: ok. 1 passed; 0 failed; 0 ignored")
'''
FAKE_V8 = r'''import argparse, json, os, sys
from pathlib import Path
parser = argparse.ArgumentParser()
for name in ("repo", "target", "cache"):
    parser.add_argument("--" + name, required=True)
args = parser.parse_args()
with open(os.environ["ALAB_FAKE_V8_LOG"], "a") as stream:
    stream.write(json.dumps(vars(args)) + "\n")
if os.environ.get("ALAB_FAKE_V8_FAILURE") == "resolver":
    raise SystemExit(42)
cache = Path(args.cache) / Path(args.repo).name
cache.mkdir(parents=True, exist_ok=True)
result = {"version": "fixture", "target": args.target}
for key, name in (("RUSTY_V8_ARCHIVE", "archive"), ("RUSTY_V8_SRC_BINDING_PATH", "binding")):
    if os.environ.get("ALAB_FAKE_V8_FAILURE") == "missing-binding" and name == "binding":
        continue
    path = cache / name
    path.write_text(json.dumps({"repo": args.repo, "target": args.target}))
    result[key] = str(path)
print(json.dumps(result))
'''


@unittest.skipUnless(shutil.which("jq") and shutil.which("git"), "requires jq and git")
class GateV8InputTests(unittest.TestCase):
    def setUp(self):
        # Retain every tiny fixture; there is deliberately no cleanup callback.
        self.base = Path(tempfile.mkdtemp(prefix="gate-v8-wiring-",
                         dir=os.environ.get("ALAB_TEST_EVIDENCE_ROOT"))).resolve()
        self.tooling = self.base / "tooling"
        self.tooling.mkdir()
        source_root = Path(os.environ.get("ALAB_TEST_GATE_SOURCE", str(ROOT)))
        shutil.copyfile(source_root / "alab/unit-tests.sh", self.tooling / "unit-tests.sh")
        (self.tooling / "v8.py").write_text(FAKE_V8)
        self.cargo = self.tooling / "fake-cargo"
        self.cargo.write_text(PYTHON_ENTRY + FAKE_CARGO)
        self.cargo.chmod(0o755)
        self.trees = [self.base / name for name in ("candidate", "pristine")]
        for tree in self.trees:
            (tree / "codex-rs").mkdir(parents=True)
            (tree / "target").mkdir()
            for directory, name, source in (
                    ("rmcp-client", "codex-rmcp-client", "src/bin/test_stdio_server.rs"),
                    ("code-mode-host", "codex-code-mode-host", "src/main.rs"),
                    ("core", "codex-core", "src/lib.rs"),
                    ("network-proxy", "codex-network-proxy", "src/lib.rs")):
                package = tree / "codex-rs" / directory
                (package / source).parent.mkdir(parents=True)
                (package / source).write_text("fn main() {}\n")
                (package / "Cargo.toml").write_text(f'[package]\nname = "{name}"\nversion = "0.0.0"\n')
            # The real build guardian observes the checkout's Git volume too.
            subprocess.run(["git", "init", "-q", str(tree)], check=True,
                           env={"PATH": os.environ["PATH"], "GIT_CONFIG_GLOBAL": "/dev/null",
                                "GIT_CONFIG_NOSYSTEM": "1"}, capture_output=True, timeout=20)
        self.home = self.base / "home"
        self.home.mkdir()
        self.cargo_home = self.base / "private-cargo-home"
        self.cargo_home.mkdir()
        git_program = shutil.which("git")
        self.assertIsNotNone(git_program, "Git is required for V8 gate-wiring fixtures")
        self.environment = {"HOME": str(self.home),
            "CODEX_HOME": str(self.home),
            "PATH": str(Path(git_program).parent) + ":/usr/bin:/bin:/usr/sbin:/sbin",
            "ALAB_CARGO_HOME": str(self.cargo_home), "ALAB_PYTHON": sys.executable,
            "ALAB_FAKE_CARGO_LOG": str(self.base / "cargo.jsonl"),
            "ALAB_FAKE_V8_LOG": str(self.base / "resolver.jsonl")}
        source = (source_root / "alab/check.sh").read_text()
        self.lint = source[source.index("lint() {\n"):source.index("# lint_verdict <json>")]
        print("retained V8 wiring fixture:", self.base, file=sys.stderr)

    def run_functions(self, unit=False, failure=None, helper_failure=None, proxy=False):
        self.run = Path(tempfile.mkdtemp(prefix="run-", dir=self.base))
        script = ('set -euo pipefail\n. "$1/alab/lib.sh"\n. "$2/unit-tests.sh"\n'
                  'cargo="$3"\n' + self.lint + '\npackages=(-p codex-core'
                  + (' -p codex-network-proxy' if proxy else '') + ')\n')
        if helper_failure:
            script += '''mktemp() {
  local allocated
  allocated="$(command mktemp "$@")" || return $?
  printf '%s\\n' "$allocated" >> "$ALAB_FAKE_TEMP_LOG"
  printf '%s\\n' "$allocated"
}
'''
        for tree in self.trees:
            command = "unit_tests" if unit else "lint"
            script += f'{command} "{tree}" "{tree}/target"' + ("\n" if unit else ' -p codex-code-mode-host\n')
        environment = dict(self.environment,
                           ALAB_FAKE_CARGO_LOG=str(self.run / "cargo.jsonl"),
                           ALAB_FAKE_V8_LOG=str(self.run / "resolver.jsonl"),
                           ALAB_FAKE_TEMP_LOG=str(self.run / "temporaries"),
                           RUSTY_V8_ARCHIVE="/untrusted-ambient-archive",
                           RUSTY_V8_SRC_BINDING_PATH="/untrusted-ambient-binding")
        if failure:
            environment["ALAB_FAKE_V8_FAILURE"] = failure
        if helper_failure:
            environment["ALAB_FAKE_HELPER_FAILURE"] = helper_failure
        (self.run / "command.sh").write_text(script)
        process = subprocess.run(["/bin/bash", "-c", script, "gate-v8-wiring", str(ROOT),
                                  str(self.tooling), str(self.cargo)], env=environment,
                                 stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=20)
        (self.run / "stdout").write_text(process.stdout)
        (self.run / "stderr").write_text(process.stderr)
        (self.run / "status").write_text(str(process.returncode) + "\n")
        return process

    def records(self, name):
        path = self.run / name
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def test_lint_passes_actual_candidate_and_pristine_pairs(self):
        process = self.run_functions()
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertEqual([row["args"] for row in self.records("cargo.jsonl")
                          if row["stage"] == "query"], [["--version", "--verbose"]] * 2)
        calls = [row for row in self.records("cargo.jsonl") if row["stage"] == "tippy"]
        self.assertEqual([row["tree"] for row in calls], [str(tree) for tree in self.trees])
        self.assertEqual([row["repo"] for row in self.records("resolver.jsonl")], [str(tree) for tree in self.trees])
        for row in calls:
            self.assertTrue(row["archive"].endswith("/" + Path(row["tree"]).name + "/archive"))
            self.assertTrue(row["binding"].endswith("/" + Path(row["tree"]).name + "/binding"))

    def test_final_library_test_keeps_both_verified_overrides(self):
        process = self.run_functions(unit=True)
        self.assertEqual(process.returncode, 0, process.stderr)
        calls = self.records("cargo.jsonl")
        self.assertEqual([row["stage"] for row in calls], ["query", "metadata", "build", "test"] * 2)
        for build, test in ((calls[2], calls[3]), (calls[6], calls[7])):
            self.assertEqual((build["archive"], build["binding"]), (test["archive"], test["binding"]))
        for tree in self.trees:
            self.assertEqual(list((tree / "target").glob("alab-helpers.*")), [])

    def test_plain_helper_failure_cleans_metadata_and_proxy_scratch(self):
        sentinel = self.home / "keep"
        sentinel.write_text("owner data")
        for failure in ("metadata", "selection"):
            for proxy in (False, True):
                with self.subTest(failure=failure, proxy=proxy):
                    process = self.run_functions(unit=True, helper_failure=failure, proxy=proxy)
                    self.assertEqual(process.returncode, 73 if failure == "metadata" else 1,
                                     process.stderr)
                    calls = self.records("cargo.jsonl")
                    expected = ["query", "metadata"] + (["build"] if failure == "selection" else [])
                    self.assertEqual([row["stage"] for row in calls], expected)
                    self.assertTrue(all(row["tree"] == str(self.trees[0]) for row in calls))
                    if failure == "selection":
                        self.assertIn("no unique runtime helper artifact", process.stderr)
                    self.assertEqual(Path(calls[-1]["codex_home"]), self.home)
                    allocated = [Path(path) for path in (self.run / "temporaries").read_text().splitlines()]
                    helper_dirs = [path for path in allocated if path.name.startswith("alab-helpers.")]
                    proxy_dirs = [path for path in allocated
                                  if path.name.startswith("alab-network-proxy-home.")]
                    self.assertEqual(len(helper_dirs), 1)
                    self.assertEqual(len(proxy_dirs), int(proxy))
                    for path in helper_dirs + proxy_dirs:
                        self.assertEqual(path.parent, self.trees[0] / "target")
                        self.assertFalse(path.exists(), "temporary scratch survived helper failure")
                    for tree in self.trees:
                        self.assertEqual(list((tree / "target").glob("alab-helpers.*")), [])
                        self.assertEqual(list((tree / "target").glob("alab-network-proxy-home.*")), [])
                    self.assertEqual(sentinel.read_text(), "owner data")

    def test_failed_or_incomplete_resolution_never_reaches_native_command(self):
        for unit, failure in ((False, "resolver"), (True, "resolver"),
                              (False, "missing-binding"), (True, "missing-binding")):
            with self.subTest(unit=unit, failure=failure):
                process = self.run_functions(unit=unit, failure=failure)
                self.assertNotEqual(process.returncode, 0)
                self.assertEqual([row["stage"] for row in self.records("cargo.jsonl")], ["query"])

    def test_unrecognized_short_query_refuses_without_resolver_or_native_work(self):
        process = subprocess.run([str(self.cargo), "-vV"], cwd=self.trees[0] / "codex-rs",
                                 env=self.environment, stdin=subprocess.DEVNULL,
                                 capture_output=True, text=True, timeout=20)
        self.assertEqual(process.returncode, 75, process.stderr)
        self.assertIn("owner record", process.stderr)
        self.assertFalse(Path(self.environment["ALAB_FAKE_V8_LOG"]).exists())
        records = Path(self.environment["ALAB_FAKE_CARGO_LOG"]).read_text().splitlines()
        self.assertEqual([json.loads(line)["stage"] for line in records], ["resource-refusal"])


if __name__ == "__main__":
    unittest.main()

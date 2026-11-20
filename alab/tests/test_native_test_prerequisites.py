#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise real prerequisite shell code with generated, non-Rust Cargo/helper mocks."""

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
MOCK_CARGO = r'''import json, os, shlex, sys
from pathlib import Path

root = Path(os.environ["MOCK_TREE"])
target = Path(os.environ["CARGO_TARGET_DIR"])
args = sys.argv[1:]
with Path(os.environ["MOCK_CALLS"]).open("a") as log:
    log.write(json.dumps({"args": args, "cwd": str(Path.cwd()),
                          "helpers": {k: v for k, v in os.environ.items()
                                      if k.startswith("CARGO_BIN_EXE_")}}) + "\n")
providers = {"codex": ("cli", "codex-cli", "src/main.rs"),
             "test_stdio_server": ("rmcp-client", "codex-rmcp-client", "src/bin/test_stdio_server.rs"),
             "codex-code-mode-host": ("code-mode-host", "codex-code-mode-host", "src/main.rs")}
def identifier(directory, package):
    return "path+" + (root / "codex-rs" / directory).as_uri() + "#" + package + "@0.0.0"

if args[:1] == ["metadata"]:
    packages = []
    for name, (directory, package, source) in providers.items():
        base = root / "codex-rs" / directory
        packages.append({"name": package, "id": identifier(directory, package),
                         "manifest_path": str(base / "Cargo.toml"),
                         "targets": [{"name": name, "kind": ["bin"], "src_path": str(base / source)}]})
    print(json.dumps({"workspace_root": str(root / "codex-rs"),
                      "target_directory": str(target), "packages": packages,
                      "workspace_members": [p["id"] for p in packages]}))
elif "build" in args:
    selected_packages = {args[i + 1] for i, a in enumerate(args) if a == "-p"}
    bins = [args[i + 1] for i, a in enumerate(args) if a == "--bin"]
    fault = os.environ.get("MOCK_FAULT", "")
    bad_helper = os.environ.get("MOCK_BAD_HELPER", "codex")
    for name in bins:
        directory, package, source = providers[name]
        assert package in selected_packages
        base = root / "codex-rs" / directory
        output = target / "selected helpers" / name
        output.parent.mkdir(parents=True, exist_ok=True)
        # Only the CLI's existing --version warmup executes this generated mock.
        output.write_text("#!/bin/sh\n" + "printf '%s:%s\\n' " + shlex.quote(name) + ' "$*"' +
                          " >> " + shlex.quote(os.environ["MOCK_WARMUPS"]) + "\n" +
                          ("exit 17\n" if fault == "warmup" and name == bad_helper else "exit 0\n"))
        output.chmod(0o755)
        record = {"reason": "compiler-artifact", "package_id": identifier(directory, package),
                  "manifest_path": str(base / "Cargo.toml"),
                  "target": {"kind": ["bin"], "name": name, "src_path": str(base / source)},
                  "profile": {"test": False}, "executable": str(output)}
        if name == bad_helper:
            if fault == "missing":
                continue
            if fault == "wrong_package":
                record["package_id"] += "-foreign"
            if fault == "wrong_source":
                record["target"]["src_path"] = str(base / "src/foreign.rs")
            if fault == "harness":
                record["profile"]["test"] = True
            if fault == "outside":
                record["executable"] = sys.executable
            if fault == "ambiguous":
                other = output.with_name(name + "-other")
                other.write_bytes(output.read_bytes())
                other.chmod(0o755)
                print(json.dumps({**record, "executable": str(other)}))
        print(json.dumps(record))
elif "test" in args:
    assert ("--lib" in args) == (os.environ.get("MOCK_SCOPE", "lib") == "lib")
    if "--test" in args and args[args.index("--test") + 1] == "alab_fleet_model":
        print("running 1 test\ntest fleet_model ... ok")
        print("test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;")
else:
    raise SystemExit("unexpected mock Cargo call: " + repr(args))
'''


@unittest.skipUnless(shutil.which("jq"), "requires jq")
class NativeTestPrerequisitesTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="alab-prerequisites-")
        self.scratch = Path(self.directory.name).resolve()
        self.tree = self.scratch / "source"
        self.target = self.scratch / "target"
        self.target.mkdir()
        self.helper = self.scratch / "helper evidence"
        self.calls = self.scratch / "calls.jsonl"
        self.warmups = self.scratch / "warmups"
        self.mock = self.scratch / "mock-cargo"
        self.mock.write_text("#!/bin/sh\nexec " + shlex.quote(sys.executable) + " -I -B \"$0.py\" \"$@\"\n")
        self.mock.chmod(0o755)
        self.mock.with_suffix(".py").write_text(MOCK_CARGO)
        for directory, package, source in (
                ("cli", "codex-cli", "src/main.rs"),
                ("rmcp-client", "codex-rmcp-client", "src/bin/test_stdio_server.rs"),
                ("code-mode-host", "codex-code-mode-host", "src/main.rs")):
            base = self.tree / "codex-rs" / directory
            (base / source).parent.mkdir(parents=True)
            (base / source).write_text("// fixture source; never compiled\n")
            (base / "Cargo.toml").write_text('[package]\nname = "' + package + '"\n')
        self.environment = {k: v for k, v in os.environ.items()
                            if not k.startswith(("ALAB_", "CARGO_", "ATPKG_PROJECT_", "RUSTY_V8_"))}
        self.environment.update(MOCK_TREE=str(self.tree), MOCK_TARGET=str(self.target),
                                MOCK_HELPER=str(self.helper), MOCK_CALLS=str(self.calls),
                                MOCK_WARMUPS=str(self.warmups), MOCK_CARGO=str(self.mock),
                                MOCK_LIBRARY=str(ROOT), ALAB_PYTHON=sys.executable,
                                CARGO_BIN_EXE_codex="/foreign/codex",
                                CARGO_BIN_EXE_test_stdio_server="/foreign/test_stdio_server",
                                **{"CARGO_BIN_EXE_codex-code-mode-host": "/foreign/host-dash",
                                   "CARGO_BIN_EXE_codex_code_mode_host": "/foreign/host-underscore"})

    def tearDown(self):
        self.directory.cleanup()

    def test_integration_test_selects_the_workspace_test_harness(self):
        workspace = self.tree / "codex-rs"
        package = workspace / "code-mode-host"
        test_source = package / "tests/stdio.rs"
        test_source.parent.mkdir()
        test_source.touch()
        target = {"name": "stdio", "kind": ["test"], "src_path": str(test_source)}
        manifest = str(package / "Cargo.toml")
        package_id = "fixture-host"
        metadata = self.scratch / "metadata.json"
        metadata.write_text(json.dumps({"workspace_root": str(workspace), "target_directory": str(self.target),
            "workspace_members": [package_id], "packages": [{"id": package_id, "name": "codex-code-mode-host",
            "manifest_path": manifest, "targets": [target]}]}))
        harness = self.target / "stdio-harness"
        harness.touch(mode=0o755)
        artifact = {"reason": "compiler-artifact", "package_id": package_id, "target": target,
                    "profile": {"test": True}, "manifest_path": manifest, "executable": str(harness)}
        command = [sys.executable, "-I", "-B", str(ROOT / "alab/native_helpers.py"),
                   "--repo", str(self.tree), "--target-dir", str(self.target), "--metadata", str(metadata),
                   "--integration-test", "codex-code-mode-host/stdio"]
        result = subprocess.run(command, input=json.dumps(artifact), text=True, capture_output=True, check=True)
        self.assertEqual(json.loads(result.stdout), {"stdio": str(harness)})
        for change in ({"profile": {"test": False}}, {"package_id": "other"},
                       {"target": dict(target, kind=["bin"])}, {"target": dict(target, src_path=manifest)}):
            with self.subTest(change=change):
                result = subprocess.run(command, input=json.dumps({**artifact, **change}),
                                        text=True, capture_output=True)
                self.assertEqual(result.returncode, 1)
                self.assertIn("no unique runtime helper artifact", result.stderr)

    def shell(self, body, *args, **environment):
        setup = '''set -euo pipefail
. "$MOCK_LIBRARY/alab/unit-tests.sh"
alab_library_root="$MOCK_LIBRARY"
cargo="$MOCK_CARGO"
export CARGO_TARGET_DIR="$MOCK_TARGET"
tree_config=(--config 'paths = []')
test_command=(/usr/bin/env)
alab_build_command() { "$@"; }
native_v8_env() { printf '%s\\n' '{"RUSTY_V8_ARCHIVE":"/mock/archive","RUSTY_V8_SRC_BINDING_PATH":"/mock/binding"}'; }
'''
        return subprocess.run(["/bin/bash", "-c", setup + body, "prerequisite-fixture", *args],
                              cwd=self.tree / "codex-rs", env={**self.environment, **environment},
                              capture_output=True, text=True, timeout=30)

    def observations(self):
        return [json.loads(line) for line in self.calls.read_text().splitlines()] if self.calls.exists() else []

    def plan(self, scope, packages):
        return self.shell('''count="$1"; shift
scope=()
for ((i = 0; i < count; i++)); do scope+=("$1"); shift; done
packages=("$@")
native_test_requirements "${scope[@]}"
printf '%s %s %s %s %s\\n' "$need_metadata" "$need_v8" "$need_stdio" "$need_host" "$need_cli"
''', str(len(scope)), *scope, *packages)

    def prepare(self, **environment):
        return self.shell('''packages=(-p codex-tui -p codex-tui)
native_test_requirements test all
[[ "$need_metadata" == 1 ]] && mkdir "$MOCK_HELPER"
native_test_prerequisites "$MOCK_TREE" "$MOCK_TARGET" "$MOCK_HELPER" 0 1
printf '%s\\n' "${test_command[@]}"
''', **environment)

    def test_target_and_package_plan_is_pure_and_validated(self):
        cases = ((["lib"], ["-p", "codex-tui"], [0, 0, 0, 0, 0]),
                 (["test", "all"], ["-p", "codex-tui"], [1, 0, 1, 0, 1]),
                 (["default"], ["-p", "codex-tui"], [1, 0, 1, 0, 1]),
                 (["lib"], ["-p", "codex-mcp"], [1, 0, 0, 0, 1]),
                 (["lib"], ["-p", "codex-core", "-p", "codex-mcp"], [1, 1, 1, 1, 1]),
                 (["default"], ["-p", "codex-alab-autopilot"], [1, 1, 0, 1, 0]))
        for scope, packages, expected in cases:
            with self.subTest(scope=scope, packages=packages):
                result = self.plan(scope, packages)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual([int(x) for x in result.stdout.split()], expected)
        for scope, packages in (([], ["-p", "codex-tui"]),
                                (["lib", "all"], ["-p", "codex-tui"]),
                                (["test"], ["-p", "codex-tui"]),
                                (["test", "other"], ["-p", "codex-tui"]),
                                (["test", "all"], ["-p", "codex-mcp"]),
                                (["test", "all"], ["-p", "codex-tui", "-p", "codex-mcp"]),
                                (["lib"], []), (["lib"], ["--workspace"]),
                                (["lib"], ["-p"]), (["lib"], ["-p", "bad/package"]),
                                (["lib"], ["-p", "-option"]), (["lib"], ["-p", ""])):
            with self.subTest(invalid=(scope, packages)):
                result = self.plan(scope, packages)
                self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.observations(), [])
        self.assertFalse(self.helper.exists())

    def test_selected_tui_target_prepares_separate_exact_helpers_once(self):
        result = self.prepare()
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.observations()
        self.assertEqual(len(calls), 3)
        self.assertEqual(calls[0]["args"], ["metadata", "--locked", "--config", "paths = []",
                                           "--no-deps", "--format-version", "1"])
        builds = calls[1:]
        for call, package, binary in zip(builds, ("codex-rmcp-client", "codex-cli"),
                                          ("test_stdio_server", "codex")):
            self.assertEqual(call["args"], ["--unverified", "build", "--locked", "--config", "paths = []",
                                            "-p", package, "--bin", binary,
                                            "--message-format=json-render-diagnostics"])
        for binary in ("codex", "test_stdio_server"):
            self.assertIn("CARGO_BIN_EXE_" + binary + "=" + str(self.target / "selected helpers" / binary),
                          result.stdout.splitlines())
        self.assertEqual(len((self.helper / "helper-native.jsonl").read_text().splitlines()), 1)
        self.assertEqual(len((self.helper / "helper-cli.jsonl").read_text().splitlines()), 1)
        self.assertEqual(self.warmups.read_text().splitlines(), ["codex:--version"])

    def test_wire_and_fleet_recipes_build_only_the_cli_they_launch(self):
        candidate = self.tree / "alab"
        candidate.mkdir()
        for script in ("unit-tests.sh", "native_helpers.py", "focused-tests.sh", "focused_results.py",
                       "python_test_results.py", "cli_status.py"):
            (candidate / script).symlink_to(ROOT / "alab" / script)
        (candidate / "lib.sh").write_text('''alab_library_root="$MOCK_TREE"
alab_project_selected() { :; }
alab_build_command() {
    if [[ "$1" == --result-file ]]; then shift 2; fi
    "$@"
}
''')
        recipes = self.scratch / "feature_recipes"
        recipes.mkdir()
        for script in ("python_test_results.py", "cli_status.py"):
            (recipes.parent / script).symlink_to(ROOT / "alab" / script)
        runner = recipes / "run.sh"
        shutil.copyfile(ROOT / "alab/feature_recipes/run.sh", runner)
        fixtures = {"wire": "wire_controls", "model-wire": "model_wire_controls",
                    "product-wire": "product_wire_controls", "skill-wire": "skill_wire_controls",
                    "native-ui-wire": "native_ui_controls", "execution-wire": "execution_wire_controls"}
        for module in fixtures.values():
            (recipes / (module + ".py")).write_text('''import os, unittest
class Wire(unittest.TestCase):
    def test_cli(self):
        self.assertEqual(os.environ["ALAB_FEATURE_BINARY"], os.environ["MOCK_EXPECTED_CLI"])
''')
        with (recipes / "model_wire_controls.py").open("a") as extra:
            extra.write('    def test_unrelated(self): self.fail("unrequested control ran")\n')
        binaries = self.target / "selected helpers"
        for stage in (*fixtures, "fleet-cli"):
            with self.subTest(stage=stage):
                out = self.scratch / stage
                out.mkdir()
                calls = out / "cargo.jsonl"
                names = ["model_wire_controls.Wire.test_cli"] if stage == "model-wire" else []
                result = subprocess.run(
                    ["/bin/bash", str(runner), "contained", str(self.tree), "a" * 40, str(out), stage, *names],
                    env={**self.environment, "ALAB_CARGO": str(self.mock), "TMPDIR": str(self.scratch),
                         "CARGO_TARGET_DIR": str(self.target), "MOCK_CALLS": str(calls),
                         "MOCK_SCOPE": "default", "MOCK_EXPECTED_CLI": str(binaries / "codex")},
                    capture_output=True, text=True, timeout=30)
                self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
                observed = [json.loads(line) for line in calls.read_text().splitlines()]
                commands = [call["args"] for call in observed]
                self.assertEqual(sum(args[0] == "metadata" for args in commands), 1)
                built = [args[i + 1] for args in commands for i, flag in enumerate(args) if flag == "--bin"]
                self.assertEqual(built, ["codex"])
                if stage == "fleet-cli":
                    last = observed[-1]
                    self.assertIn("test", last["args"])
                    self.assertEqual(last["args"][last["args"].index("-p") + 1], "codex-cli")
                    self.assertEqual(last["args"][last["args"].index("--test") + 1], "alab_fleet_model")
                    self.assertEqual(last["helpers"]["CARGO_BIN_EXE_codex"], str(binaries / "codex"))
                else:
                    tests = json.loads((out / "tests.json").read_text())["tests"]
                    self.assertEqual([test["id"] for test in tests], [fixtures[stage] + ".Wire.test_cli"])

    def test_artifact_refusal_precedes_cli_warmup(self):
        for fault in ("missing", "ambiguous", "wrong_package", "wrong_source", "harness", "outside"):
            with self.subTest(fault=fault):
                shutil.rmtree(self.helper, ignore_errors=True)
                self.calls.unlink(missing_ok=True)
                self.warmups.unlink(missing_ok=True)
                result = self.prepare(MOCK_FAULT=fault)
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn("helper", result.stderr)
                self.assertFalse(self.warmups.exists())

    def test_selected_helpers_bind_each_sources_own_target(self):
        ours = self.prepare()
        self.assertEqual(ours.returncode, 0, ours.stderr)
        other_tree = self.scratch / "comparison source"
        other_target = self.scratch / "comparison target"
        other_target.mkdir()
        shutil.copytree(self.tree, other_tree)
        other_helper = self.scratch / "comparison evidence"
        other = self.prepare(MOCK_TREE=str(other_tree), MOCK_TARGET=str(other_target),
                             MOCK_HELPER=str(other_helper))
        self.assertEqual(other.returncode, 0, other.stderr)
        for result, target in ((ours, self.target), (other, other_target)):
            self.assertIn("CARGO_BIN_EXE_codex=" + str(target / "selected helpers/codex"),
                          result.stdout.splitlines())
        selected = json.loads((other_helper / "helper-cli.jsonl").read_text())
        self.assertEqual(selected["manifest_path"], str(other_tree / "codex-rs/cli/Cargo.toml"))
        self.assertEqual(selected["executable"], str(other_target / "selected helpers/codex"))

    def test_shared_preparation_preserves_direct_callers_trap_and_cwd(self):
        marker = self.scratch / "direct-exit"
        for fault in ("", "missing"):
            with self.subTest(fault=fault):
                shutil.rmtree(self.helper, ignore_errors=True)
                result = self.shell('''packages=(-p codex-tui)
native_test_requirements test all
mkdir "$MOCK_HELPER"
trap 'printf done > "$MOCK_EXIT"' EXIT
before="$(trap -p EXIT)"; before_cwd="$(pwd -P)"
rc=0
native_test_prerequisites "$MOCK_TREE" "$MOCK_TARGET" "$MOCK_HELPER" 0 1 || rc=$?
[[ "$before" == "$(trap -p EXIT)" && "$before_cwd" == "$(pwd -P)" ]] || exit 99
exit "$rc"
''', MOCK_FAULT=fault, MOCK_EXIT=str(marker))
                self.assertEqual(result.returncode == 0, not fault, result.stderr)
                self.assertNotEqual(result.returncode, 99)
                self.assertEqual(marker.read_text(), "done")

    def test_library_tui_keeps_library_argv_and_allocates_no_helper_scratch(self):
        result = self.shell('''packages=(-p codex-tui)
mktemp() { printf 'unexpected helper allocation\\n' >&2; return 97; }
unit_tests "$MOCK_TREE" "$MOCK_TARGET" --config 'paths = []' --exact declared::control
''')
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.observations()
        self.assertEqual([c["args"] for c in calls], [["--unverified", "test", "--locked", "--config",
                          "paths = []", "--no-fail-fast", "--lib", "-p", "codex-tui",
                          "--", "--exact", "declared::control"]])
        self.assertFalse(self.warmups.exists())
        self.assertEqual(list(self.target.glob("alab-helpers.*")), [])

    def test_mcp_library_preserves_warmup_alias_and_cleanup(self):
        result = self.shell('''packages=(-p codex-mcp)
unit_tests "$MOCK_TREE" "$MOCK_TARGET" --config 'paths = []'
''')
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.observations()
        self.assertEqual(len(calls), 3)
        self.assertIn("--bin", calls[1]["args"])
        self.assertEqual(calls[1]["args"][calls[1]["args"].index("--bin") + 1], "codex")
        self.assertIn("--lib", calls[2]["args"])
        self.assertEqual(calls[2]["helpers"]["CARGO_BIN_EXE_codex"],
                         str(self.target / "selected helpers/codex"))
        self.assertEqual(self.warmups.read_text().splitlines(), ["codex:--version"])
        self.assertEqual(list(self.target.glob("alab-helpers.*")), [])
        self.assertFalse(any("test_stdio_server" in c["args"] for c in calls))

    def test_mixed_library_plan_keeps_native_pair_and_cli_graphs_and_aliases(self):
        result = self.shell('''packages=(-p codex-core -p codex-mcp)
unit_tests "$MOCK_TREE" "$MOCK_TARGET" --config 'paths = []'
''')
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.observations()
        self.assertEqual(len(calls), 4)
        self.assertEqual([calls[1]["args"][i + 1] for i, a in enumerate(calls[1]["args"])
                          if a == "--bin"],
                         ["test_stdio_server", "codex-code-mode-host"])
        self.assertEqual(calls[2]["args"][calls[2]["args"].index("--bin") + 1], "codex")
        for alias, binary in (("test_stdio_server", "test_stdio_server"),
                              ("codex-code-mode-host", "codex-code-mode-host"),
                              ("codex_code_mode_host", "codex-code-mode-host"), ("codex", "codex")):
            self.assertEqual(calls[3]["helpers"]["CARGO_BIN_EXE_" + alias],
                             str(self.target / "selected helpers" / binary))
        self.assertIn("--lib", calls[3]["args"])
        self.assertEqual(list(self.target.glob("alab-helpers.*")), [])

    def test_active_alab_default_caller_keeps_full_target_scope(self):
        result = self.shell('''all_alab_tests "$MOCK_TREE" "$MOCK_TARGET" "$MOCK_HELPER" -p codex-alab-autopilot
''', MOCK_SCOPE="default")
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.observations()
        self.assertEqual(len(calls), 3)
        self.assertEqual(calls[1]["args"][calls[1]["args"].index("--bin") + 1], "codex-code-mode-host")
        self.assertEqual(calls[2]["args"], ["--unverified", "test", "--locked", "--no-fail-fast",
                                           "-p", "codex-alab-autopilot"])
        self.assertFalse(self.warmups.exists())
        for alias in ("codex-code-mode-host", "codex_code_mode_host"):
            self.assertEqual(calls[2]["helpers"]["CARGO_BIN_EXE_" + alias],
                             str(self.target / "selected helpers/codex-code-mode-host"))

    def test_invalid_active_selection_refuses_before_result_allocation(self):
        state = self.scratch / "recorded result"
        result = self.shell('''packages=(--workspace)
unit_tests "$MOCK_TREE" "$MOCK_TARGET" --record-results "$MOCK_STATE"
''', MOCK_STATE=str(state))
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(state.exists())
        self.assertEqual(self.observations(), [])

    def test_failed_shared_cli_preparation_keeps_caller_trap_and_cleans_unit_scratch(self):
        marker = self.scratch / "caller-exit"
        for fault in ("warmup", "missing"):
            with self.subTest(fault=fault):
                self.calls.unlink(missing_ok=True)
                result = self.shell('''packages=(-p codex-mcp)
trap 'printf done > "$MOCK_EXIT"' EXIT
before="$(trap -p EXIT)"; before_cwd="$(pwd -P)"
rc=0
unit_tests "$MOCK_TREE" "$MOCK_TARGET" || rc=$?
[[ "$before" == "$(trap -p EXIT)" && "$before_cwd" == "$(pwd -P)" ]] || exit 99
exit "$rc"
''', MOCK_FAULT=fault, MOCK_EXIT=str(marker))
                self.assertNotEqual(result.returncode, 0)
                self.assertNotEqual(result.returncode, 99)
                self.assertIn("failed to start" if fault == "warmup" else "helper", result.stderr)
                self.assertEqual(marker.read_text(), "done")
                self.assertEqual(list(self.target.glob("alab-helpers.*")), [])
                self.assertFalse(any("test" in c["args"] for c in self.observations()))


if __name__ == "__main__":
    unittest.main()

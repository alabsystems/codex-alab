#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""alab/check.sh's step 4 as it runs: the real check.sh, lib.sh and lint.jq in a scratch clone
shaped like codex-alab (an upstream commit, ALab's commit with hooks on top), with ALAB_CARGO
naming a fake cargo. The fake builds and tests nothing: it answers `metadata` from the
manifests in front of it, and `tippy` with the JSON lines a test gives for each tree (ALab's,
or the pristine upstream tree check.sh checks out in the clone's git dir), so what is judged
is check.sh's reading of cargo's records and the exit it takes on them. One test runs the step
under alab_contained as alab/sync.sh's run_check does, since what the sandbox refuses (a
here-document's temp file, under macOS's bash 3.2) breaks only there.
"""

import importlib.util
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "lint_step_fixtures", Path(__file__).with_name("test_unit_results.py"))
FIXTURES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FIXTURES)
RUNNER = FIXTURES.RUNNER


ROOT = Path(__file__).resolve().parents[2]

FAKE_CARGO = """#!/bin/sh
'''exec' {python} -I "$0" "$@"
' '''
import json, os, shlex, subprocess, sys
from pathlib import Path

if Path(__file__).parent.name == "deps":
    if os.environ.get("ALAB_FAKE_BIND_SOCKET"):
        import socket, stat, tempfile
        with tempfile.TemporaryDirectory(prefix="alab-control-") as directory:
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
                listener.bind(str(Path(directory) / "updater-control.sock"))
            root = Path(os.environ["TMPDIR"])
            with (Path(os.environ["ALAB_FAKE_CARGO_DIR"]) / "runtime-temp.jsonl").open("a") as log:
                log.write(json.dumps({{"path": str(root), "private": stat.S_IMODE(root.stat().st_mode) == 0o700}}) + "\\n")
    test = os.environ.get("ALAB_FAKE_LIBTEST_NAME", "")
    selected = bool(test) and ("--exact" not in sys.argv or test in sys.argv[1:])
    if "--list" in sys.argv:
        if selected:
            print(test + ": test")
        sys.exit(0)
    if not selected:
        print("test result: ok. 0 passed; 0 failed; 0 ignored")
        sys.exit(0)
    failed = os.environ.get("ALAB_FAKE_LIBTEST_FAILED") == "1"
    print("test " + test + " ... " + ("FAILED" if failed else "ok"))
    print("test result: " + ("FAILED. 0 passed; 1 failed" if failed else "ok. 1 passed; 0 failed") + "; 0 ignored")
    sys.exit(101 if failed else 0)

fake = Path(os.environ["ALAB_FAKE_CARGO_DIR"])
# A path override (lib.sh, alab_pristine_config) is configuration, as cargo itself takes it:
# logged as given, then set aside before the call is answered.
logged = sys.argv[1:]
args = []
words = iter(logged)
for word in words:
    if word == "--config":
        value = next(words)
        if value.startswith("paths"):
            continue
        args += [word, value]
    else:
        args.append(word)
here = Path.cwd()
for parent in (here, *here.parents):
    manifest = parent / "Cargo.toml"
    if manifest.is_file() and "[workspace]" in manifest.read_text():
        here = parent
        break
tree = str(here.parent)
side = "pristine" if ("/alab-pristine/" in tree + "/" or "/alab-pristine-" in tree) else "ours"
with open(fake / "calls", "a") as log:
    log.write(side + " " + " ".join(logged) + "\\n")
with open(fake / "environments.jsonl", "a") as log:
    log.write(json.dumps({{"args": logged, "CARGO_BUILD_JOBS": os.environ.get("CARGO_BUILD_JOBS"),
                          "RUST_TEST_THREADS": os.environ.get("RUST_TEST_THREADS"),
                          "side": side, "target": os.environ.get("CARGO_TARGET_DIR"),
                          "build": os.environ.get("CARGO_BUILD_BUILD_DIR"),
                          "v8": {{key: os.environ.get(key) for key in
                                  ("RUSTY_V8_ARCHIVE", "RUSTY_V8_SRC_BINDING_PATH")}},
                          "helpers": {{key: value for key, value in os.environ.items()
                                      if key.startswith("CARGO_BIN_EXE_")}}}}) + "\\n")
mode_file = fake / "unit-mode.json"
mode = json.loads(mode_file.read_text()) if mode_file.exists() else {{}}
effective_target = Path(os.environ.get("CARGO_TARGET_DIR") or
                        os.environ.get("CARGO_BUILD_TARGET_DIR") or
                        mode.get("configured_target") or str(here / "target"))
if mode.get("cache_ownership") and any(word in args for word in ("run", "build", "test", "tippy")):
    marker = effective_target / "fixture-owner"
    if os.environ.get("CARGO_TARGET_DIR"):
        # Native admission owns the gate target and its lint child before compilation.
        owner_targets = [effective_target]
        if "tippy" in args and effective_target.name == "tippy":
            owner_targets.insert(0, effective_target.parent)
        for owner_target in owner_targets:
            owner_marker = owner_target / "fixture-owner"
            if owner_target.exists() and (not owner_marker.is_file() or owner_marker.read_text() != "owned"):
                print("fixture cache ownership refused before compilation", file=sys.stderr)
                sys.exit(75)
            owner_target.mkdir(parents=True, exist_ok=True)
            owner_marker.write_text("owned")
    else:
        effective_target.mkdir(parents=True, exist_ok=True)
        if not marker.exists():
            marker.write_text("unmanaged")
    with (fake / "compiled.jsonl").open("a") as log:
        log.write(json.dumps({{"side": side, "args": args, "target": str(effective_target)}}) + "\\n")
def require_v8():
    for key, filename in (("RUSTY_V8_ARCHIVE", "archive"), ("RUSTY_V8_SRC_BINDING_PATH", "binding")):
        path = Path(os.environ.get(key, "/missing"))
        expected = Path(os.environ["ALAB_CARGO_HOME"]) / "alab-v8-inputs" / filename
        if path != expected or not path.is_file() or json.loads(path.read_text()) != {{
                "repo": tree, "target": "fixture-native-target"}}:
            sys.exit("fake cargo: missing pinned V8 prerequisite " + key)

if args == ["-vV"]:
    print("fake atpkg: unrecognized query needs an owner record in a read-only sandbox", file=sys.stderr)
    sys.exit(75)
elif args == ["--version", "--verbose"]:
    print("cargo 0.0.0\\nhost: fixture-native-target")
elif args[:1] == ["metadata"]:
    if "--offline" in args and side in mode.get("metadata_failures", {{}}):
        print("fixture missing locked dependency: " + side, file=sys.stderr)
        sys.exit(mode["metadata_failures"][side])
    packages = []
    for manifest in sorted(here.glob("*/Cargo.toml")):
        name = next(line.split('"')[1] for line in manifest.read_text().splitlines()
                    if line.startswith("name"))
        targets = [{{"kind": ["lib"], "name": name.replace("-", "_"),
                    "src_path": str(manifest.parent / "src/lib.rs"), "test": True}}]
        if (manifest.parent / "build.rs").exists():
            targets.append({{"kind": ["custom-build"], "name": "build-script-build",
                            "src_path": str(manifest.parent / "build.rs"), "test": False}})
        identifier = "path+" + manifest.parent.as_uri() + "#" + name + "@0.0.0"
        packages.append({{"name": name, "id": identifier, "version": "0.0.0",
                         "manifest_path": str(manifest), "targets": targets}})
    print(json.dumps({{"packages": packages, "workspace_root": str(here),
                      "target_directory": str(effective_target),
                      "workspace_members": [package["id"] for package in packages]}}))
elif "tippy" in args:
    require_v8()
    lines = fake / (side + ".jsonl")
    sys.stdout.write(lines.read_text().replace("@TREE@", tree) if lines.exists() else "")
    code = fake / (side + ".rc")
    sys.exit(int(code.read_text()) if code.exists() else 0)
elif "run" in args:
    print("codex-cli 0.0.0")
elif "build" in args:
    if mode.get("build_failure") == side:
        sys.exit(101)
    providers = {{"test_stdio_server": "codex-rmcp-client",
                  "codex-code-mode-host": "codex-code-mode-host", "codex": "codex-cli"}}
    packages = {{args[index + 1] for index, arg in enumerate(args) if arg == "-p"}}
    binaries = {{args[index + 1] for index, arg in enumerate(args) if arg == "--bin"}}
    if binaries == {{"alab-unit-runner"}}:
        expected = ["--unverified", "build", "--locked", "--manifest-path",
                    os.environ["ALAB_FAKE_LIBRARY_MANIFEST"], "-p", "codex-alab-unit-runner",
                    "--bin", "alab-unit-runner", "--message-format=json-render-diagnostics"]
        if args != expected or packages != {{"codex-alab-unit-runner"}}:
            sys.exit("fake cargo: unexpected native runtime build " + " ".join(args))
        print(json.dumps({{"reason": "compiler-artifact", "target": {{"kind": ["bin"], "name": "alab-unit-runner"}},
                          "executable": os.environ["ALAB_FAKE_UNIT_RUNNER"]}}))
        sys.exit(0)
    if (not binaries or binaries - providers.keys() or packages - set(providers.values())
            or any(providers[name] not in packages for name in binaries)
            or "--bins" in args or set(args[args.index("build") + 1:]) - {{
            "--locked", "--message-format=json-render-diagnostics", "-p", "--bin",
            "codex-rmcp-client", "test_stdio_server", "codex-code-mode-host", "codex-cli", "codex"}}):
        sys.exit("fake cargo: unexpected helper build " + " ".join(args))
    if "codex-code-mode-host" in binaries:
        require_v8()
    for name in sorted(binaries):
        output = Path(os.environ["CARGO_TARGET_DIR"]) / "native helper output" / name
        output.parent.mkdir(parents=True, exist_ok=True)
        if mode.get("artifacts") == "directory" and name == "test_stdio_server":
            output.mkdir(exist_ok=True)
        else:
            startup = ""
            if name == "codex":
                startup = ('if [ "$1" = --version ]; then\\n' +
                           ("exit 17\\n" if mode.get("cli_failure") == side else
                            "touch " + shlex.quote(str(output) + ".ready") + "\\n") + "fi\\n")
            output.write_text("#!/bin/sh\\n" + startup + "printf '%s\\n' '" + name + "'\\n")
            output.chmod(0o755)
        if mode.get("artifacts") == "missing" and name == "test_stdio_server":
            continue
        record = {{"reason": "compiler-artifact", "target": {{"kind": ["bin"], "name": name}},
                   "executable": str(output)}}
        print(json.dumps(record))
        if mode.get("artifacts") == "ambiguous" and name == "test_stdio_server":
            print(json.dumps(dict(record, executable=str(output) + "-other")))
elif "test" in args:
    if "--lib" in args:
        target = Path(os.environ.get("CARGO_TARGET_DIR", str(here / "target")))
        selected_packages = [args[index + 1] for index, arg in enumerate(args) if arg == "-p"]
        ready = True
        names = []
        if mode and any(name in selected_packages for name in ("codex-core", "codex-app-server")):
            require_v8()
            names += ["test_stdio_server", "codex-code-mode-host"]
        if mode and "codex-mcp" in selected_packages:
            names += ["codex"]
            ready = (target / "native helper output/codex.ready").is_file()
        for name in names:
            # Match the real cargo_bin resolver's original-name-before-alias order.
            path = os.environ.get("CARGO_BIN_EXE_" + name,
                                  os.environ.get("CARGO_BIN_EXE_" + name.replace("-", "_")))
            expected = target / "native helper output" / name
            if path != str(expected) or not expected.is_file():
                ready = False
            elif subprocess.run([path], capture_output=True, text=True, check=True).stdout.strip() != name:
                ready = False
        test = "native::behavior" if ready else "native::uses_helpers"
        failed = not ready or mode.get(side) == "fail"
        owner = ("codex-core" if "codex-core" in selected_packages else
                 "codex-mcp" if "codex-mcp" in selected_packages else None)
        runner = []
        for index, argument in enumerate(args):
            config = (args[index + 1] if argument == "--config" else
                      argument.partition("=")[2] if argument.startswith("--config=") else "")
            if ".runner=" in config:
                runner = json.loads(config.split(".runner=", 1)[1])
        libtest_args = args[args.index("--") + 1:] if "--" in args else []
        manifests = {{next(line.split('"')[1] for line in manifest.read_text().splitlines()
                          if line.startswith("name")): manifest
                     for manifest in here.glob("*/Cargo.toml")}}
        any_failed = False
        for package in selected_packages:
            manifest = manifests[package]
            crate = package.replace("-", "_")
            executable = target / "debug/deps" / (crate + "-0123456789abcdef")
            if "--no-run" in args or not runner:
                executable.parent.mkdir(parents=True, exist_ok=True)
                executable.write_text(Path(__file__).read_text())
                executable.chmod(0o755)
            if not executable.is_file():
                sys.exit("fake cargo: no prepared library test artifact " + str(executable))
            if "--no-run" in args:
                print(json.dumps({{"reason": "compiler-artifact",
                                  "package_id": "path+" + manifest.parent.as_uri() + "#" + package + "@0.0.0",
                                  "manifest_path": str(manifest),
                                  "target": {{"kind": ["lib"], "name": crate,
                                             "src_path": str(manifest.parent / "src/lib.rs"), "test": True}},
                                  "profile": {{"test": True}}, "executable": str(executable),
                                  "fresh": True}}))
                continue
            environment = dict(os.environ, CARGO_MANIFEST_DIR=str(manifest.parent),
                               CARGO_PKG_NAME=package, CARGO_PKG_VERSION="0.0.0",
                               CARGO_CRATE_NAME=crate,
                               ALAB_FAKE_LIBTEST_NAME=test if mode and package == owner else "",
                               ALAB_FAKE_LIBTEST_FAILED="1" if failed else "0")
            print("     Running unittests src/lib.rs (" + str(executable) + ")", flush=True)
            result = subprocess.run([*runner, str(executable), *libtest_args],
                                    cwd=manifest.parent, env=environment, check=False)
            if result.returncode:
                print("error: test failed, to rerun pass `-p " + package + " --lib`", file=sys.stderr)
            any_failed = any_failed or result.returncode != 0
        sys.exit(101 if any_failed else 0)
    print("test result: ok. 0 passed; 0 failed; 0 ignored")
else:
    sys.exit("fake cargo: unexpected " + " ".join(args))
"""


FAKE_V8 = """import argparse, json, os, sys
from pathlib import Path

parser = argparse.ArgumentParser()
for key in ("repo", "target", "cache"):
    parser.add_argument("--" + key, required=True)
args = parser.parse_args()
side = "pristine" if ("/alab-pristine/" in args.repo + "/" or "/alab-pristine-" in args.repo) else "ours"
fake = Path(os.environ["ALAB_FAKE_CARGO_DIR"])
mode_file = fake / "unit-mode.json"
mode = json.loads(mode_file.read_text()) if mode_file.exists() else {}
if mode.get("v8_failure") == side:
    sys.exit(42)
cache = Path(args.cache)
cache.mkdir(parents=True, exist_ok=True)
result = {"version": "fixture", "target": args.target}
for key, filename in (("RUSTY_V8_ARCHIVE", "archive"), ("RUSTY_V8_SRC_BINDING_PATH", "binding")):
    path = cache / filename
    path.write_text(json.dumps({"repo": args.repo, "target": args.target}))
    result[key] = str(path)
print(json.dumps(result))
"""


def package_id(name, path):
    return f"path+file://@TREE@/codex-rs/{path}#{name}@0.0.0"


def built_script(name, path):
    """cargo's record of a package's build script that compiled."""
    return json.dumps({
        "reason": "compiler-artifact", "package_id": package_id(name, path),
        "manifest_path": f"@TREE@/codex-rs/{path}/Cargo.toml",
        "target": {"kind": ["custom-build"], "name": "build-script-build",
                   "src_path": f"@TREE@/codex-rs/{path}/build.rs", "test": False},
        "filenames": [], "fresh": True,
    })


def ran_script(name, path):
    """cargo's record of a build script that ran to completion."""
    return json.dumps({"reason": "build-script-executed", "package_id": package_id(name, path),
                       "linked_libs": [], "linked_paths": [], "cfgs": [], "env": [],
                       "out_dir": "/out"})


def bench_error(path="core", name="codex-core"):
    """An error in a bench target of codex-core, in upstream's code."""
    return json.dumps({
        "reason": "compiler-message", "package_id": package_id(name, path),
        "manifest_path": f"@TREE@/codex-rs/{path}/Cargo.toml",
        "target": {"kind": ["bench"], "name": "b", "src_path": "b.rs", "test": False},
        "message": {"level": "error", "message": "mismatched types", "code": {"code": "E0308"},
                    "rendered": "error[E0308]: mismatched types in core/benches/b.rs\n",
                    "spans": [{"file_name": f"{path}/benches/b.rs", "line_start": 4,
                               "line_end": 4, "column_start": 5, "is_primary": True,
                               "expansion": None}]},
    })


CLEAN = [built_script("codex-cli", "cli"), ran_script("codex-cli", "cli"),
         built_script("codex-alab-core", "alab-core"), ran_script("codex-alab-core", "alab-core")]


@unittest.skipUnless(shutil.which("jq") and shutil.which("git"), "requires jq and git")
class CheckLintStepTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not RUNNER.is_file():
            raise RuntimeError("Build codex-alab-unit-runner first; set ALAB_TEST_UNIT_RUNNER to its actual artifact")

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="alab-check-")
        scratch = Path(os.path.realpath(self.directory.name))
        self.repo = scratch / "repo"
        self.fake = scratch / "fake"
        self.fake.mkdir()
        # The contained step admits the repo and the fake's directory. TMPDIR may
        # live under the account home, where the rest of scratch is unreadable.
        cargo = self.fake / "targo"
        cargo.write_text(FAKE_CARGO.format(python=shlex.quote(sys.executable)))
        cargo.chmod(0o755)
        self.environment = {key: value for key, value in os.environ.items()
                            if not key.startswith(("ALAB_", "CARGO_", "GIT_"))}
        cargo_home = scratch / "cargo-home"
        cargo_home.mkdir()
        (cargo_home / ".alab-seeded").touch()
        self.environment.update(ALAB_CARGO_HOME=str(cargo_home),
                                ALAB_CARGO=str(cargo), ALAB_FAKE_CARGO_DIR=str(self.fake),
                                ALAB_FAKE_UNIT_RUNNER=str(RUNNER),
                                ALAB_FAKE_LIBRARY_MANIFEST=str(self.repo / "codex-rs/Cargo.toml"),
                                ALAB_PYTHON=sys.executable,
                                GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_NOSYSTEM="1",
                                GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@example.invalid",
                                GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@example.invalid")
        rs = self.repo / "codex-rs"
        self.write(rs / ".gitignore", "/target/\n")
        self.write(rs / "Cargo.toml",
                   '[workspace]\nmembers = ["cli", "core", "code-mode-host", "codex-mcp", "alab-*"]\n')
        for path, name in (("cli", "codex-cli"), ("core", "codex-core"),
                           ("code-mode-host", "codex-code-mode-host"), ("codex-mcp", "codex-mcp")):
            self.write(rs / path / "Cargo.toml", f'[package]\nname = "{name}"\nversion = "0.0.0"\n')
            self.write(rs / path / "src/lib.rs", "pub fn upstream() {}\n")
        self.write(rs / "cli/build.rs", "fn main() {}\n")
        self.git("init", "-q", "-b", "main")
        self.commit("upstream")
        self.git("update-ref", "refs/remotes/upstream/main", "HEAD")
        for name in ("check.sh", "lib.sh", "lint.jq", "retention.py", "unit-tests.sh", "unit_results.py", "command_guard.py"):
            self.write(self.repo / "alab" / name, (ROOT / "alab" / name).read_text())
        # The shared resolver's own tests validate upstream code and pinned hashes.
        # This boundary proves check.sh supplies the actual tree/host/cache and
        # delivers its validated pair to the helper-build child in both trees.
        self.write(self.repo / "alab/v8.py", FAKE_V8)
        (self.repo / "alab/check.sh").chmod(0o755)
        self.write(rs / "alab-core/Cargo.toml",
                   '[package]\nname = "codex-alab-core"\nversion = "0.0.0"\n')
        self.write(rs / "alab-core/src/lib.rs", "pub fn alab() {}\n")
        self.write(rs / "alab-core/build.rs", "fn main() {}\n")
        for path in ("cli", "core"):
            self.write(rs / path / "src/lib.rs",
                       f"pub fn upstream() {{}}\n// ALAB({path}): a hook\n")
        self.commit("alab")
        self.git("update-ref", "refs/remotes/origin/main", "HEAD")

    def tearDown(self):
        self.directory.cleanup()

    @staticmethod
    def write(path, text):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def git(self, *args):
        subprocess.run(["git", "-C", str(self.repo), *args], env=self.environment, check=True,
                       capture_output=True, timeout=60)

    def commit(self, message):
        self.git("add", "-A")
        self.git("commit", "-q", "-m", message)

    def check(self, ours, ours_rc=0, pristine=None, contained=False):
        """check.sh's exit and stderr, with tippy giving these lines in each tree. Contained,
        it runs as alab/sync.sh's run_check runs it: the pristine tree put at the merge base
        outside, then check.sh under alab_contained with the two target dirs writable (and
        the fake's own directory, for its log)."""
        (self.fake / "ours.jsonl").write_text("".join(line + "\n" for line in ours))
        (self.fake / "ours.rc").write_text(str(ours_rc))
        if pristine is not None:
            (self.fake / "pristine.jsonl").write_text("".join(line + "\n" for line in pristine))
            (self.fake / "pristine.rc").write_text("101")
        if not contained:
            process = subprocess.run(["/bin/bash", "-c", '. "$1"; check_main', "check-body", str(self.repo / "alab/check.sh")], cwd=self.repo,
                                     env=self.environment, capture_output=True, text=True,
                                     timeout=60)
            return process.returncode, process.stderr + ("\n" + process.stdout if process.returncode else "")
        cargo_home = self.fake.parent / "cargo-home"
        cargo_home.mkdir(exist_ok=True)
        (cargo_home / ".alab-seeded").touch()
        command = ('set -euo pipefail; . alab/lib.sh; '
                   'base="$(git merge-base HEAD upstream/main)"; alab_pristine_at "$PWD" "$base"; '
                   'pristine="$(alab_pristine "$PWD" "$base")"; '
                   'mkdir -p codex-rs/target "${pristine}/codex-rs/target"; '
                   'alab_contained --pristine "${pristine}" --read "$2" "$PWD/codex-rs/target" "${pristine}/codex-rs/target" "$1" '
                   '-- /bin/bash -c \'. ./alab/check.sh; check_main\'')
        process = subprocess.run(["/bin/bash", "-c", command, "contained-check", str(self.fake), str(RUNNER)],
                                 cwd=self.repo, env=dict(self.environment,
                                                         ALAB_CARGO_HOME=str(cargo_home)),
                                 capture_output=True, text=True, timeout=60)
        return process.returncode, process.stderr + ("\n" + process.stdout if process.returncode else "")

    def calls(self, side):
        path = self.fake / "calls"
        lines = path.read_text().splitlines() if path.exists() else []
        return [line for line in lines if line.startswith(side + " ")]

    def compiler_calls(self, side):
        return [call for call in self.calls(side)
                if set(call.split()) & {"run", "build", "test", "tippy"}]

    def test_a_clean_run_passes(self):
        code, stderr = self.check(CLEAN)
        self.assertEqual(code, 0, stderr)
        self.assertIn("tippy finds nothing in ALab's code", stderr)
        self.assertEqual(self.compiler_calls("pristine"), [])
        calls = self.compiler_calls("ours")
        lint = next(index for index, call in enumerate(calls) if " tippy " in call)
        cli = next(index for index, call in enumerate(calls) if " run " in call)
        alab = next(index for index, call in enumerate(calls)
                    if " test " in call and "--lib" not in call)
        units = next(index for index, call in enumerate(calls)
                     if " test " in call and "--lib" in call)
        self.assertLess(lint, cli)
        self.assertLess(cli, alab)
        self.assertLess(alab, units)

    def test_both_full_locked_graphs_resolve_before_the_first_compiler(self):
        code, stderr = self.check(CLEAN)
        self.assertEqual(code, 0, stderr)
        calls = (self.fake / "calls").read_text().splitlines()
        full = [call for call in calls if " metadata --offline --locked --format-version 1" in call]
        self.assertEqual(full, ["ours metadata --offline --locked --format-version 1",
                                "pristine metadata --offline --locked --format-version 1"])
        first_compiler = next(index for index, call in enumerate(calls)
                              if set(call.split()) & {"run", "build", "test", "tippy"})
        self.assertTrue(all(calls.index(call) < first_compiler for call in full))
        self.assertTrue(all("--no-deps" not in call for call in full))

    def test_locked_dependency_failures_aggregate_without_starting_a_compiler(self):
        for failures in ({"ours": 37}, {"pristine": 41}, {"ours": 37, "pristine": 41}):
            with self.subTest(failures=failures):
                (self.fake / "unit-mode.json").write_text(json.dumps({"metadata_failures": failures}))
                (self.fake / "calls").unlink(missing_ok=True)
                code, stderr = self.check(CLEAN)
                self.assertEqual(code, failures.get("ours", failures.get("pristine")), stderr)
                for side in failures:
                    self.assertIn("fixture missing locked dependency: " + side, stderr)
                self.assertTrue(any(" metadata --offline --locked " in call
                                    for call in self.calls("ours")))
                self.assertTrue(any(" metadata --offline --locked " in call
                                    for call in self.calls("pristine")))
                self.assertEqual(self.compiler_calls("ours"), [])
                self.assertEqual(self.compiler_calls("pristine"), [])

    def test_dependency_preflight_stops_on_resource_refusal_or_interruption(self):
        for status in (75, 130):
            with self.subTest(status=status):
                (self.fake / "unit-mode.json").write_text(json.dumps({
                    "metadata_failures": {"ours": status, "pristine": 41},
                }))
                (self.fake / "calls").unlink(missing_ok=True)
                code, stderr = self.check(CLEAN)
                self.assertEqual(code, status, stderr)
                self.assertEqual(self.calls("pristine"), [])
                self.assertEqual(self.compiler_calls("ours"), [])

    def test_default_target_is_owned_before_any_compilation_and_stays_consistent(self):
        (self.fake / "unit-mode.json").write_text(json.dumps({"cache_ownership": True}))
        code, stderr = self.check(CLEAN)
        self.assertEqual(code, 0, stderr)
        compiled = [json.loads(line) for line in (self.fake / "compiled.jsonl").read_text().splitlines()]
        target = str(self.repo / "codex-rs/target")
        self.assertIn("tippy", compiled[0]["args"])
        self.assertEqual(compiled[0]["target"], target + "/tippy")
        self.assertTrue(all(item["target"] == target for item in compiled if "tippy" not in item["args"]))
        self.assertEqual((Path(target) / "fixture-owner").read_text(), "owned")

    def test_unit_preparation_and_execution_keep_one_build_directory(self):
        code, stderr = self.check(CLEAN)
        self.assertEqual(code, 0, stderr)
        calls = [json.loads(line) for line in (self.fake / "environments.jsonl").read_text().splitlines()]
        units = [call for call in calls if "test" in call["args"] and "--lib" in call["args"]]
        self.assertTrue(any("--no-run" in call["args"] for call in units))
        self.assertTrue(any("--config" in call["args"] for call in units))
        expected = str(self.repo / "codex-rs/target/build.noindex")
        self.assertTrue(all(call["build"] == expected for call in units))

    def test_unknown_target_is_refused_before_compilation_without_changing_its_contents(self):
        target = self.repo / "codex-rs/target"
        target.mkdir()
        (target / "keep").write_text("earlier unowned output")
        (self.fake / "unit-mode.json").write_text(json.dumps({"cache_ownership": True}))
        code, stderr = self.check(CLEAN)
        self.assertEqual(code, 75, stderr)
        self.assertFalse((self.fake / "compiled.jsonl").exists())
        self.assertEqual(list(target.iterdir()), [target / "keep"])
        self.assertEqual((target / "keep").read_text(), "earlier unowned output")

    def test_metadata_target_keeps_config_and_environment_precedence(self):
        configured = self.repo / "configured output"
        build_env = self.repo / "build environment output"
        explicit = self.repo / "explicit output"
        for variables, expected in (({}, configured),
                                    ({"CARGO_BUILD_TARGET_DIR": str(build_env)}, build_env),
                                    ({"CARGO_BUILD_TARGET_DIR": str(build_env),
                                      "CARGO_TARGET_DIR": str(explicit)}, explicit)):
            with self.subTest(variables=variables):
                (self.fake / "compiled.jsonl").unlink(missing_ok=True)
                self.environment.update(variables)
                (self.fake / "unit-mode.json").write_text(json.dumps({
                    "cache_ownership": True, "configured_target": str(configured),
                }))
                code, stderr = self.check(CLEAN)
                self.assertEqual(code, 0, stderr)
                compiled = [json.loads(line) for line in (self.fake / "compiled.jsonl").read_text().splitlines()]
                self.assertTrue(all(item["target"] == str(expected) for item in compiled
                                    if "tippy" not in item["args"]))

    def test_unknown_external_target_refuses_for_config_and_explicit_environment(self):
        target = self.repo.parent / "external output"
        target.mkdir()
        keep = target / "keep"
        keep.write_text("unowned external output")
        for variables in ({}, {"CARGO_TARGET_DIR": str(target)}):
            with self.subTest(variables=variables):
                self.environment.update(variables)
                (self.fake / "unit-mode.json").write_text(json.dumps({
                    "cache_ownership": True, "configured_target": str(target),
                }))
                code, stderr = self.check(CLEAN)
                self.assertEqual(code, 75, stderr)
                self.assertFalse((self.fake / "compiled.jsonl").exists())
                self.assertEqual(list(target.iterdir()), [keep])
                self.assertEqual(keep.read_text(), "unowned external output")

    @unittest.skipUnless(Path("/usr/bin/sandbox-exec").is_file(), "requires macOS sandbox-exec")
    def test_configured_external_target_does_not_expand_containment(self):
        target = self.repo.parent / "external output"
        target.mkdir()
        keep = target / "keep"
        keep.write_text("outside the admitted roots")
        (self.fake / "unit-mode.json").write_text(json.dumps({"configured_target": str(target)}))
        code, _stderr = self.check(CLEAN, contained=True)
        self.assertNotEqual(code, 0)
        self.assertEqual(list(target.iterdir()), [keep])
        self.assertEqual(keep.read_text(), "outside the admitted roots")
        self.assertEqual(self.unit_children("ours"), [])

    @unittest.skipUnless(os.name == "posix", "requires Unix sockets")
    def test_long_inherited_tmpdir_cannot_break_native_socket_fixtures(self):
        inherited = self.repo.parent / ("inherited-" + "x" * 110)
        inherited.mkdir()
        keep = inherited / "keep"
        keep.write_text("caller-owned temporary input")
        self.environment.update(TMPDIR=str(inherited), ALAB_FAKE_BIND_SOCKET="1")
        code, stderr = self.check(CLEAN)
        self.assertEqual(code, 0, stderr)
        observed = [json.loads(line) for line in (self.fake / "runtime-temp.jsonl").read_text().splitlines()]
        self.assertTrue(observed)
        self.assertTrue(all(item["private"] for item in observed))
        self.assertTrue(all(Path(item["path"]) != inherited for item in observed))
        self.assertEqual(keep.read_text(), "caller-owned temporary input")

    @unittest.skipUnless(Path("/usr/bin/sandbox-exec").is_file(), "requires macOS sandbox-exec")
    def test_native_socket_temp_stays_inside_the_existing_containment(self):
        self.environment["ALAB_FAKE_BIND_SOCKET"] = "1"
        code, stderr = self.check(CLEAN, contained=True)
        self.assertEqual(code, 0, stderr)
        observed = [json.loads(line) for line in (self.fake / "runtime-temp.jsonl").read_text().splitlines()]
        self.assertTrue(observed)
        self.assertTrue(all(item["private"] for item in observed))

    def test_disk_resource_stop_preserves_no_verdict_without_pristine_retry(self):
        guard = self.repo / "alab/command_guard.py"
        original = guard.read_text()
        guard.write_text(original.replace(
            'sys.exit(main())',
            'os.statvfs = lambda path: type("Space", (), {"f_bavail": 19 * 2**30, "f_frsize": 1})()\n    sys.exit(main())'))
        code, stderr = self.check(CLEAN)
        self.assertEqual(code, 75, stderr)
        self.assertIn("disk resource stop", stderr)
        self.assertEqual(self.calls("ours"), [], "disk refusal precedes the first compiler")
        self.assertEqual(self.calls("pristine"), [], "a resource stop is no compiler comparison")

    def test_disk_resource_stop_at_detached_unit_boundary_skips_incomplete_collection(self):
        guard = self.repo / "alab/command_guard.py"
        original = guard.read_text()
        guard.write_text(original.replace(
            'sys.exit(main())',
            'if "test" in sys.argv and "--lib" in sys.argv and "--no-run" not in sys.argv:\n'
            '        os.statvfs = lambda path: type("Space", (), {"f_bavail": 19 * 2**30, "f_frsize": 1})()\n'
            '    sys.exit(main())'))
        evidence = self.fake / "disk-stop-results"
        self.environment["ALAB_UNIT_RESULTS_DIR"] = str(evidence)
        code, stderr = self.check(CLEAN)
        self.assertEqual(code, 75, stderr)
        self.assertIn("disk resource stop", stderr)
        self.assertTrue((evidence / "unit-main/artifacts.jsonl").is_file(), "compilation prepared the graph")
        self.assertFalse((evidence / "unit-main.json").exists(), "no verdict from incomplete runtime")
        self.assertEqual(self.compiler_calls("pristine"), [])

    def test_explicit_unit_evidence_survives_cleanup_and_refuses_existing_output(self):
        self.unit_mode(ours="fail", pristine="fail")
        evidence = self.fake / "retained-unit-results"
        self.environment["ALAB_UNIT_RESULTS_DIR"] = str(evidence)
        code, stderr = self.check(CLEAN, contained=Path("/usr/bin/sandbox-exec").is_file())
        self.assertEqual(code, 0, stderr)
        self.assertEqual(evidence.stat().st_mode & 0o777, 0o700)
        for phase in ("main", "alone", "pristine"):
            self.assertTrue(list((evidence / ("unit-" + phase)).glob("library-*.json")))
            self.assertEqual(len(json.loads((evidence / ("unit-" + phase + ".json")).read_text())["failed"]), 1)
        original = {str(path.relative_to(evidence)): path.read_bytes() for path in evidence.rglob("*") if path.is_file()}
        existing_file = self.fake / "existing-output"
        existing_file.write_text("preserve this file")
        existing_link = self.fake / "existing-output-link"
        existing_link.symlink_to(evidence, target_is_directory=True)
        for existing in (evidence, existing_file, existing_link):
            with self.subTest(existing=existing.name):
                self.environment["ALAB_UNIT_RESULTS_DIR"] = str(existing)
                code, stderr = self.check(CLEAN)
                self.assertEqual(code, 1, stderr)
                self.assertIn("cannot create fresh unit-results directory", stderr)
                self.assertEqual(original, {str(path.relative_to(evidence)): path.read_bytes()
                                            for path in evidence.rglob("*") if path.is_file()})
        self.assertEqual(existing_file.read_text(), "preserve this file")
        self.assertTrue(existing_link.is_symlink())

    def check_limits(self, allocated, expected):
        for key in ("CARGO_BUILD_JOBS", "RUST_TEST_THREADS"):
            self.environment.pop(key, None)
        self.environment.update(allocated)
        recorded = self.fake / "environments.jsonl"
        recorded.unlink(missing_ok=True)
        code, stderr = self.check(CLEAN)
        self.assertEqual(code, 0, stderr)
        children = [json.loads(line) for line in recorded.read_text().splitlines()]
        # Observe both the ALab and hooked upstream test commands, rather than a
        # helper's settings: every cargo subprocess must receive the allocation.
        tests = [child for child in children if "test" in child["args"]]
        self.assertTrue(any("--lib" not in child["args"] for child in tests), tests)
        self.assertTrue(any("--lib" in child["args"] for child in tests), tests)
        for child in children:
            self.assertEqual({key: child[key] for key in expected}, expected, child)

    def test_native_checks_bound_parallelism_without_allocated_limits(self):
        self.check_limits({}, {"CARGO_BUILD_JOBS": "4", "RUST_TEST_THREADS": "4"})

    def test_native_checks_keep_independent_allocated_limits(self):
        for allocated, expected in (
                ({"CARGO_BUILD_JOBS": "2"}, {"CARGO_BUILD_JOBS": "2", "RUST_TEST_THREADS": "4"}),
                ({"RUST_TEST_THREADS": "3"}, {"CARGO_BUILD_JOBS": "4", "RUST_TEST_THREADS": "3"}),
                ({"CARGO_BUILD_JOBS": "6", "RUST_TEST_THREADS": "2"},
                 {"CARGO_BUILD_JOBS": "6", "RUST_TEST_THREADS": "2"}),
                ({"CARGO_BUILD_JOBS": "", "RUST_TEST_THREADS": ""},
                 {"CARGO_BUILD_JOBS": "4", "RUST_TEST_THREADS": "4"})):
            with self.subTest(allocated=allocated):
                self.check_limits(allocated, expected)

    def unit_mode(self, **mode):
        (self.fake / "unit-mode.json").write_text(json.dumps(mode))
        # Conflicting inherited aliases must not choose another tree's helper.
        for name in ("test_stdio_server", "codex-code-mode-host", "codex_code_mode_host"):
            self.environment["CARGO_BIN_EXE_" + name] = str(self.fake / "wrong helper")

    def unit_children(self, side):
        children = [json.loads(line) for line in (self.fake / "environments.jsonl").read_text().splitlines()]
        return [child for child in children if child["side"] == side and "--lib" in child["args"]]

    def assert_prepared(self, side):
        children = self.unit_children(side)
        self.assertTrue(children)
        for child in children:
            for name in ("test_stdio_server", "codex-code-mode-host", "codex_code_mode_host"):
                filename = name.replace("codex_code_mode_host", "codex-code-mode-host")
                expected = Path(child["target"]) / "native helper output" / filename
                self.assertEqual(child["helpers"]["CARGO_BIN_EXE_" + name], str(expected))
        builds = [call for call in self.calls(side) if " build " in call]
        self.assertTrue(builds, self.calls(side))
        for build in builds:
            self.assertNotIn("--bins", build)

    def test_native_helpers_are_prepared_before_libraries(self):
        self.unit_mode(ours="ok", pristine="ok")
        code, stderr = self.check(CLEAN)
        self.assertEqual(code, 0, stderr)
        self.assert_prepared("ours")
        self.assertEqual(self.compiler_calls("pristine"), [])

    def test_mcp_cli_is_built_in_each_compared_tree(self):
        self.write(self.repo / "codex-rs/core/src/lib.rs", "pub fn upstream() {}\n")
        self.write(self.repo / "codex-rs/codex-mcp/src/lib.rs",
                   "pub fn upstream() {}\n// ALAB(version): request identity\n")
        self.commit("hook MCP without core")
        self.git("update-ref", "refs/remotes/origin/main", "HEAD")
        self.environment["CARGO_BIN_EXE_codex"] = str(self.fake / "wrong codex")
        self.unit_mode(ours="fail", pristine="ok")
        code, stderr = self.check(CLEAN)
        self.assertEqual(code, 1, stderr)
        for side in ("ours", "pristine"):
            children = self.unit_children(side)
            self.assertTrue(children)
            for child in children:
                expected = Path(child["target"]) / "native helper output/codex"
                self.assertEqual(child["helpers"]["CARGO_BIN_EXE_codex"], str(expected))
            builds = [call for call in self.calls(side) if " build " in call]
            self.assertTrue(builds)
            for build in builds:
                self.assertTrue("-p codex-cli --bin codex" in build or
                                "-p codex-alab-unit-runner --bin alab-unit-runner" in build, build)
                self.assertNotIn("codex-code-mode-host", build)
                self.assertNotIn("--bins", build)

    def test_mcp_cli_startup_failure_stops_before_tests(self):
        self.write(self.repo / "codex-rs/codex-mcp/src/lib.rs",
                   "pub fn upstream() {}\n// ALAB(version): request identity\n")
        self.commit("hook MCP")
        self.git("update-ref", "refs/remotes/origin/main", "HEAD")
        for side in ("ours", "pristine"):
            with self.subTest(side=side):
                self.unit_mode(ours="fail" if side == "pristine" else "ok",
                               pristine="ok", cli_failure=side)
                (self.fake / "environments.jsonl").unlink(missing_ok=True)
                code, stderr = self.check(CLEAN)
                self.assertEqual(code, 1, stderr)
                self.assertEqual(self.unit_children(side), [])

    def hook_code_mode_host(self):
        self.write(self.repo / "codex-rs/code-mode-host/src/lib.rs",
                   "pub fn upstream() {}\n// ALAB(version): running identity\n")
        self.commit("hook code mode host")
        self.git("update-ref", "refs/remotes/origin/main", "HEAD")

    def check_lint_v8(self, contained=False):
        self.hook_code_mode_host()
        self.environment.update(RUSTY_V8_ARCHIVE="/untrusted-archive",
                                RUSTY_V8_SRC_BINDING_PATH="/untrusted-binding")
        failure = bench_error("code-mode-host", "codex-code-mode-host")
        code, stderr = self.check(CLEAN + [failure], ours_rc=101,
                                  pristine=CLEAN + [failure], contained=contained)
        self.assertEqual(code, 0, stderr)
        children = [json.loads(line) for line in
                    (self.fake / "environments.jsonl").read_text().splitlines()]
        queries = [child for child in children if child["args"] == ["--version", "--verbose"]]
        self.assertEqual({child["side"] for child in queries}, {"ours", "pristine"})
        lint = [child for child in children if "tippy" in child["args"]]
        self.assertEqual({child["side"] for child in lint}, {"ours", "pristine"})
        for child in lint:
            for key, filename in (("RUSTY_V8_ARCHIVE", "archive"),
                                  ("RUSTY_V8_SRC_BINDING_PATH", "binding")):
                path = Path(self.environment["ALAB_CARGO_HOME"]) / "alab-v8-inputs" / filename
                self.assertEqual(child["v8"][key], str(path), child)
                self.assertEqual(json.loads(path.read_text())["target"],
                                 "fixture-native-target")

    def test_lint_uses_verified_v8_for_current_and_pristine_trees(self):
        self.check_lint_v8()

    @unittest.skipUnless(Path("/usr/bin/sandbox-exec").is_file(), "requires macOS sandbox-exec")
    def test_lint_v8_is_prepared_in_contained_comparisons(self):
        self.check_lint_v8(contained=True)

    def test_code_mode_host_only_unit_checks_keep_verified_v8(self):
        self.hook_code_mode_host()
        self.write(self.repo / "codex-rs/core/src/lib.rs", "pub fn upstream() {}\n")
        self.commit("only cli and code mode host remain hooked")
        self.git("update-ref", "refs/remotes/origin/main", "HEAD")
        code, stderr = self.check(CLEAN)
        self.assertEqual(code, 0, stderr)
        children = self.unit_children("ours")
        self.assertTrue(children)
        self.assertFalse(any(" build " in call and "--bin alab-unit-runner" not in call
                             for call in self.calls("ours")))
        for child in children:
            for key, filename in (("RUSTY_V8_ARCHIVE", "archive"),
                                  ("RUSTY_V8_SRC_BINDING_PATH", "binding")):
                self.assertEqual(child["v8"][key],
                                 str(Path(self.environment["ALAB_CARGO_HOME"]) / "alab-v8-inputs" / filename))

    def test_lint_v8_resolution_failure_stops_before_tippy(self):
        self.hook_code_mode_host()
        self.unit_mode(v8_failure="ours")
        code, stderr = self.check(CLEAN)
        self.assertEqual(code, 1, stderr)
        self.assertFalse(any(" tippy " in call for call in self.calls("ours")))

    def test_native_helpers_keep_the_pristine_comparison_independent(self):
        self.unit_mode(ours="fail", pristine="ok")
        code, stderr = self.check(CLEAN)
        self.assertEqual(code, 1, stderr)
        self.assert_prepared("ours")
        self.assert_prepared("pristine")
        self.assertNotEqual(self.unit_children("ours")[0]["target"],
                            self.unit_children("pristine")[0]["target"])

    @unittest.skipUnless(Path("/usr/bin/sandbox-exec").is_file(), "requires macOS sandbox-exec")
    def test_native_helpers_are_prepared_in_contained_comparisons(self):
        self.unit_mode(ours="fail", pristine="ok")
        code, stderr = self.check(CLEAN, contained=True)
        self.assertEqual(code, 1, stderr)
        self.assert_prepared("ours")
        self.assert_prepared("pristine")

    def vendor_toolchain_source(self):
        """ALab's commit carries a crate in place of upstream's, as alab/vendor/allocative does."""
        vendored = self.repo / "alab/vendor/allocative"
        self.write(vendored / "ALAB-SOURCE.json", json.dumps({
            "crate": "allocative", "version": "0.3.6", "crate_sha256": "d8" * 32}))
        self.write(vendored / "Cargo.toml", '[package]\nname = "allocative"\nversion = "0.3.6"\n')
        self.commit("carry a toolchain source")
        self.git("update-ref", "refs/remotes/origin/main", "HEAD")
        return f'paths = ["{vendored}"]'

    def compared(self, contained=False):
        """A run whose lint and unit tests are both compared with pristine upstream."""
        self.unit_mode(ours="fail", pristine="ok")
        failure = bench_error("code-mode-host", "codex-code-mode-host")
        code, stderr = self.check(CLEAN + [failure], ours_rc=101, pristine=CLEAN + [failure],
                                  contained=contained)
        self.assertEqual(code, 1, stderr)
        self.assertIn("fails with ALab's layer, passes on upstream", stderr)
        self.assertIn("does not build on upstream", stderr)
        calls = [json.loads(line) for line in (self.fake / "environments.jsonl").read_text().splitlines()]
        compiling = ("tippy", "build", "test", "run", "metadata")
        return [(call["side"], call["args"]) for call in calls
                if any(word in call["args"] for word in compiling)]

    def overrides(self, args):
        return [args[index + 1] for index, word in enumerate(args)
                if word == "--config" and args[index + 1].startswith("paths")]

    def test_pristine_upstream_builds_with_the_toolchain_sources_alab_carries(self):
        """PRISTINE UPSTREAM BUILDS WITH THE TOOLCHAIN SOURCES ALAB CARRIES: upstream alone
        does not build under Trust (allocative 0.3.6: E0119), so step 5's comparison, and step
        4's, never compiled and every full check exited 1 there. Each cargo call in the
        pristine tree, the lint, the helper builds, the metadata read and every unit round,
        takes the crates alab/vendor carries as path overrides, which keep upstream's lockfile
        (--locked) and its tree untouched. NEGATIVE CONTROL: no call in ALab's tree takes them
        (its own codex-rs/Cargo.toml patches them in), and with nothing carried no call does."""
        self.hook_code_mode_host()
        for side, args in self.compared():
            self.assertEqual(self.overrides(args), [], (side, args))
        override = self.vendor_toolchain_source()
        modes = [False] + ([True] if Path("/usr/bin/sandbox-exec").is_file() else [])
        for contained in modes:
            with self.subTest(contained=contained):
                # Each run checks the pristine tree out afresh: the last one's untracked
                # target/ leaves it unclean, which check.sh rightly refuses to reuse.
                for tree in (self.repo / ".git").glob("alab-pristine-*"):
                    self.git("worktree", "remove", "--force", "--force", str(tree))
                (self.fake / "environments.jsonl").unlink(missing_ok=True)
                calls = self.compared(contained=contained)
                pristine = [args for side, args in calls if side == "pristine"]
                self.assertTrue(any("tippy" in args for args in pristine), pristine)
                self.assertTrue(any("--lib" in args for args in pristine), pristine)
                self.assertTrue(any("build" in args for args in pristine), pristine)
                for args in pristine:
                    if "alab-unit-runner" in args:
                        # ALab's own runner, built from ALab's manifest.
                        self.assertEqual(self.overrides(args), [], args)
                    else:
                        self.assertEqual(self.overrides(args), [override], args)
                for side, args in calls:
                    if side == "ours":
                        self.assertEqual(self.overrides(args), [], args)

    def test_native_helper_build_failures_are_preconditions(self):
        for side in ("ours", "pristine"):
            with self.subTest(side=side):
                self.unit_mode(ours="fail" if side == "pristine" else "ok",
                               pristine="ok", build_failure=side)
                (self.fake / "calls").unlink(missing_ok=True)
                (self.fake / "environments.jsonl").unlink(missing_ok=True)
                code, stderr = self.check(CLEAN)
                self.assertEqual(code, 1, stderr)
                self.assertEqual(self.unit_children(side), [])

    def test_native_helper_v8_failures_are_preconditions(self):
        for side in ("ours", "pristine"):
            with self.subTest(side=side):
                self.unit_mode(ours="fail" if side == "pristine" else "ok",
                               pristine="ok", v8_failure=side)
                (self.fake / "calls").unlink(missing_ok=True)
                (self.fake / "environments.jsonl").unlink(missing_ok=True)
                code, stderr = self.check(CLEAN)
                self.assertEqual(code, 1, stderr)
                self.assertEqual(self.unit_children(side), [])
                self.assertFalse(any(" build " in call for call in self.calls(side)))

    def test_native_helper_artifacts_must_identify_each_executable(self):
        for artifacts in ("missing", "ambiguous"):
            with self.subTest(artifacts=artifacts):
                self.unit_mode(ours="ok", pristine="ok", artifacts=artifacts)
                (self.fake / "environments.jsonl").unlink(missing_ok=True)
                code, stderr = self.check(CLEAN)
                self.assertEqual(code, 1, stderr)
                self.assertEqual(self.unit_children("ours"), [])

    def test_native_helper_artifact_directories_are_not_executables(self):
        self.unit_mode(ours="ok", pristine="ok", artifacts="directory")
        code, stderr = self.check(CLEAN)
        self.assertEqual(code, 1, stderr)
        self.assertEqual(self.unit_children("ours"), [])

    def test_helper_fixture_obeys_requested_targets_and_providers(self):
        cargo = self.fake / "targo"
        target = self.repo / "codex-rs/target"
        environment = dict(self.environment, CARGO_TARGET_DIR=str(target))
        cache = Path(self.environment["ALAB_CARGO_HOME"]) / "alab-v8-inputs"
        cache.mkdir(parents=True)
        for key, filename in (("RUSTY_V8_ARCHIVE", "archive"), ("RUSTY_V8_SRC_BINDING_PATH", "binding")):
            path = cache / filename
            path.write_text(json.dumps({"repo": str(self.repo), "target": "fixture-native-target"}))
            environment[key] = str(path)

        def build(*args):
            return subprocess.run([str(cargo), "--unverified", "build", "--locked", *args,
                                   "--message-format=json-render-diagnostics"],
                                  cwd=self.repo / "codex-rs", env=environment,
                                  capture_output=True, text=True, timeout=60)

        # A single selected target cannot fabricate readiness for the other one.
        one = build("--bin", "test_stdio_server", "-p", "codex-rmcp-client")
        self.assertEqual(one.returncode, 0, one.stderr)
        self.assertEqual([json.loads(line)["target"]["name"] for line in one.stdout.splitlines()],
                         ["test_stdio_server"])
        self.assertFalse((target / "native helper output/codex-code-mode-host").exists())
        for args in (("-p", "codex-rmcp-client", "--bin", "codex-code-mode-host"),
                     ("-p", "codex-rmcp-client", "--bins"),
                     ("-p", "codex-rmcp-client", "--bin", "other")):
            with self.subTest(args=args):
                result = build(*args)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")

    def test_unrelated_library_suites_do_not_prepare_native_helpers(self):
        self.write(self.repo / "codex-rs/core/src/lib.rs", "pub fn upstream() {}\n")
        self.commit("only cli remains hooked")
        self.git("update-ref", "refs/remotes/origin/main", "HEAD")
        code, stderr = self.check(CLEAN)
        self.assertEqual(code, 0, stderr)
        self.assertFalse(any(" build " in call and "--bin alab-unit-runner" not in call
                             for call in self.calls("ours")))

    def test_a_hooked_build_script_that_fails_gates_beside_a_target_upstream_breaks_too(self):
        # The gap: every failing target also fails on upstream, so the comparison set them
        # all aside, and nothing said that codex-cli's build script never ran.
        ours = [built_script("codex-cli", "cli"), bench_error()] + CLEAN[2:]
        pristine = [built_script("codex-cli", "cli"), ran_script("codex-cli", "cli"),
                    bench_error()]
        code, stderr = self.check(ours, ours_rc=101, pristine=pristine)
        self.assertEqual(code, 1, stderr)
        self.assertIn("bench b (codex-rs/core/Cargo.toml) does not build on upstream", stderr)
        self.assertIn("the build script of codex-rs/cli/Cargo.toml was built and did not run "
                      "to completion", stderr)
        self.assertIn(f"and upstream {self.upstream()[:9]} alone shows no such failure", stderr)
        # Pristine upstream was linted for the script's package as well as the bench's.
        tippy = [call for call in self.calls("pristine") if "tippy" in call]
        self.assertEqual(len(tippy), 1, self.calls("pristine"))
        self.assertIn("-p codex-cli", tippy[0])
        self.assertIn("-p codex-core", tippy[0])
        self.assertTrue(self.compiler_calls("ours"))
        self.assertTrue(all(" tippy " in call for call in self.compiler_calls("ours")),
                        "the full lint verdict refuses before CLI or test codegen")

    def test_a_build_script_that_fails_on_upstream_too_does_not_gate(self):
        ours = [built_script("codex-cli", "cli"), bench_error()] + CLEAN[2:]
        pristine = [built_script("codex-cli", "cli"), bench_error()]
        code, stderr = self.check(ours, ours_rc=101, pristine=pristine)
        self.assertEqual(code, 0, stderr)
        self.assertIn("the build script of codex-rs/cli/Cargo.toml was built and did not run "
                      "to completion (cargo's error above names it), and does not run on "
                      f"upstream {self.upstream()[:9]} alone either (not gating)", stderr)

    def test_a_failed_build_script_alone_is_named(self):
        code, stderr = self.check([built_script("codex-cli", "cli")] + CLEAN[2:], ours_rc=101,
                                  pristine=[built_script("codex-cli", "cli"),
                                            ran_script("codex-cli", "cli")])
        self.assertEqual(code, 1, stderr)
        self.assertIn("the build script of codex-rs/cli/Cargo.toml was built and did not run",
                      stderr)
        self.assertNotIn("with no error in any target", stderr)

    def test_an_alab_build_script_that_never_built_gates(self):
        # alab-core has no upstream counterpart, so nothing there can excuse it.
        ours = CLEAN[:2] + [bench_error()]
        code, stderr = self.check(ours, ours_rc=101, pristine=[bench_error()])
        self.assertEqual(code, 1, stderr)
        self.assertIn("the build script of codex-rs/alab-core/Cargo.toml was never built", stderr)

    def test_an_unlinted_packages_build_script_is_not_a_finding(self):
        # A workspace package with no hook is not linted; its script is cargo's to report.
        rs = self.repo / "codex-rs"
        self.write(rs / "other/Cargo.toml", '[package]\nname = "codex-other"\nversion = "0.0.0"\n')
        self.write(rs / "other/src/lib.rs", "pub fn other() {}\n")
        self.write(rs / "other/build.rs", "fn main() {}\n")
        self.commit("an unhooked package")
        self.git("update-ref", "refs/remotes/origin/main", "HEAD")
        code, stderr = self.check(CLEAN + [built_script("codex-other", "other")])
        self.assertEqual(code, 0, stderr)
        self.assertEqual(self.compiler_calls("pristine"), [])

    @unittest.skipUnless(Path("/usr/bin/sandbox-exec").is_file(), "requires macOS sandbox-exec")
    def test_the_step_runs_contained_as_alab_sync_runs_it(self):
        code, stderr = self.check(CLEAN, contained=True)
        self.assertEqual(code, 0, stderr)
        self.assertIn("tippy finds nothing in ALab's code", stderr)
        ours = [built_script("codex-cli", "cli"), bench_error()] + CLEAN[2:]
        pristine = [built_script("codex-cli", "cli"), ran_script("codex-cli", "cli"),
                    bench_error()]
        code, stderr = self.check(ours, ours_rc=101, pristine=pristine, contained=True)
        self.assertEqual(code, 1, stderr)
        self.assertIn("the build script of codex-rs/cli/Cargo.toml was built and did not run "
                      "to completion", stderr)
        self.assertIn("-p codex-cli", [c for c in self.calls("pristine") if "tippy" in c][0])

    def upstream(self):
        return subprocess.run(["git", "-C", str(self.repo), "rev-parse", "upstream/main"],
                              env=self.environment, capture_output=True, text=True,
                              check=True, timeout=60).stdout.strip()


if __name__ == "__main__":
    unittest.main()

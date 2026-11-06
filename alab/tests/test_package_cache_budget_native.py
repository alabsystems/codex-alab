#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Opt-in real native cache API and production package frontend integration.

Set ALAB_PACKAGE_CACHE_NATIVE_ATPKG to a frozen development atpkg executable and
ALAB_PACKAGE_CACHE_NATIVE_TRUST to its existing physical Trust sysroot. Fixtures,
including every compiled executable, are retained under .ubd/*.noindex.
"""

import hashlib
import json
import os
from pathlib import Path
import secrets
import runpy
import select
import signal
import subprocess
import sys
import tempfile
import textwrap
import unittest


ROOT = Path(__file__).resolve().parents[2]
NATIVE = os.environ.get("ALAB_PACKAGE_CACHE_NATIVE_ATPKG")
TRUST = os.environ.get("ALAB_PACKAGE_CACHE_NATIVE_TRUST")
VERB = "__build_cache_package"
INDEX = "Library/Caches/aterm/.ubd/registered-builds.noindex"
COMPILER = "0123456789abcdef"


CAPTURE = r'''
import json
import os
from pathlib import Path

target = Path(os.environ["CARGO_TARGET_DIR"])
records = [json.loads(p.read_text()) for p in
           (Path(os.environ["HOME"]) / "Library/Caches/aterm/.ubd/registered-builds.noindex").glob("*.json")]
records = [record for record in records if record["root"] == str(target)]
assert len(records) == 1, records
record = records[0]
assert record["schema"] == 3 and record["package"] is not None, record
assert record["package"]["declaration"]["preserve_if_present"]["path"] == os.environ["ALAB_PACKAGE_CACHE_PRESERVE"]
source = Path(os.environ["ALAB_PACKAGE_SOURCE_GUARD"])
snapshot = {"receipt": record, "source_guard": str(source),
            "source": json.loads(source.read_text()), "group": os.getpgrp(),
            "attempt": os.environ["ALAB_PACKAGE_CACHE_ATTEMPT"],
            "guardian": int(os.environ["ALAB_PACKAGE_GUARD_PID"])}
with Path(os.environ["BRIDGE_ADMITTED"]).open("x") as output:
    json.dump(snapshot, output, indent=2)
'''


WORKER = r'''
set -euo pipefail
. "$BRIDGE_ROOT/alab/lib.sh"
. "$BRIDGE_ROOT/alab/package.sh"
python="$ALAB_PYTHON" cache="$ALAB_PACKAGE_CACHE" wt="$BRIDGE_REPO"
compiler_id=0123456789abcdef target_triple="$BRIDGE_TRIPLE" cargo="$BRIDGE_CARGO"
if [[ "${BRIDGE_CONTAINED:-0}" == 1 ]]; then
  [[ "$(alab_account_home)" != "$HOME" ]] || { printf '%s\n' 'real account-home protection was replaced' >&2; exit 1; }
else
  alab_account_home() { printf '%s\n' "$HOME"; }
fi
CARGO_TARGET_DIR="$(package_rust_target)"
export CARGO_TARGET_DIR
"$python" -I -B "$BRIDGE_CAPTURE"
build=("$cargo" --unverified build --locked --offline --jobs 1 --message-format=json
  --manifest-path "$wt/codex-rs/Cargo.toml" --target-dir "$CARGO_TARGET_DIR")
if [[ "${BRIDGE_CONTAINED:-0}" == 1 ]]; then
  alab_contained "$CARGO_TARGET_DIR" -- "${build[@]}"
  alab_contained --read "$CARGO_TARGET_DIR" -- "$CARGO_TARGET_DIR/debug/native-budget-bridge"
else
  "${build[@]}"
  "$CARGO_TARGET_DIR/debug/native-budget-bridge"
fi
'''


GUARDIAN = r'''
import fcntl
import json
import os
from pathlib import Path
import signal
import subprocess
import sys

cache = Path(os.environ["BRIDGE_CACHE"])
native = os.environ["BRIDGE_NATIVE"]
target = os.environ["BRIDGE_TARGET"]
attempt = os.environ["BRIDGE_TOKEN"]
child = None

def cancelled(signum, frame):
    raise SystemExit(128 + signum)

for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
    signal.signal(sig, cancelled)

worker = r"""
import os
import sys
args = [os.environ['BRIDGE_NATIVE'], '__build_cache_package', 'admit',
        '--cache', os.environ['BRIDGE_CACHE'], '--target-dir', os.environ['BRIDGE_TARGET'],
        '--manifest-path', os.environ['BRIDGE_MANIFEST'], '--budget-bytes', str(64 << 30),
        '--attempt', os.environ['BRIDGE_TOKEN'], '--producer-pid', sys.argv[1],
        '--producer-group', str(os.getpgrp()), '--compiler-driver', os.environ['BRIDGE_DRIVER'],
        '--registrar', os.environ['BRIDGE_NATIVE'],
        '--preserve-if-present', os.environ['BRIDGE_PRESERVE']]
os.execv(args[0], args)
"""
descriptor = os.open(cache / '.alab-package.lockf', os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
try:
    fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
    child = subprocess.Popen([sys.executable, '-I', '-B', '-c', worker, str(os.getpid())],
                             start_new_session=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    group = child.pid
    out, err = child.communicate(timeout=60)
    code = child.returncode
    try:
        os.killpg(group, 0)
        gone = False
    except ProcessLookupError:
        gone = True
    reply = json.loads(out) if code == 0 else None
    print(json.dumps({'phase': 'ready', 'guardian': os.getpid(), 'worker_group': group,
                      'group_gone': gone, 'code': code, 'reply': reply, 'stderr': err}), flush=True)
    action = sys.stdin.readline().strip()
    if action == 'settle':
        done = subprocess.run([native, '__build_cache_package', 'settled', '--cache', str(cache),
                               '--attempt', attempt, '--success'], capture_output=True, text=True, timeout=60)
        print(json.dumps({'phase': 'settled', 'code': done.returncode,
                          'stdout': done.stdout, 'stderr': done.stderr}), flush=True)
        raise SystemExit(done.returncode)
    print(json.dumps({'phase': 'finished'}), flush=True)
finally:
    # A non-reaped Popen still leases its PID/group. Never signal a reaped group.
    if child is not None and child.returncode is None:
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        child.communicate(timeout=60)
    os.close(descriptor)
'''


def sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as source:
        for block in iter(lambda: source.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


@unittest.skipUnless(NATIVE and TRUST and os.name == "posix",
                     "set ALAB_PACKAGE_CACHE_NATIVE_ATPKG and ALAB_PACKAGE_CACHE_NATIVE_TRUST for the real native bridge")
class PackageCacheNativeTests(unittest.TestCase):
    def setUp(self):
        self.native = Path(NATIVE).resolve(strict=True)
        self.trust = Path(TRUST).resolve(strict=True)
        self.native_sha = sha256(self.native)
        self.children = []
        parent = Path(os.environ.get("ALAB_PACKAGE_CACHE_FIXTURE_ROOT", tempfile.gettempdir())) / ".ubd"
        parent.mkdir(parents=True, exist_ok=True)
        self.fixture = Path(tempfile.mkdtemp(prefix="native-cache-budget-", suffix=".noindex", dir=parent)).resolve()
        print("preserved native package cache fixture: " + str(self.fixture), flush=True)
        self.home = self.fixture / "home"
        self.home.mkdir(mode=0o700)
        self.repo = self.home / "Developer/source"
        (self.repo / "codex-rs/src").mkdir(parents=True)
        prefix = self.home / ("Library/Application Support/aterm/pkg" if sys.platform == "darwin" else ".local/share/aterm/pkg")
        (prefix / "store/trust").mkdir(parents=True)
        (prefix / "store/trust/current").symlink_to(self.trust, target_is_directory=True)
        (prefix / "bin").mkdir()
        self.host = prefix / "bin/atpkg"
        self.host.symlink_to(self.native)
        self.cargo = self.trust / "bin/targo"
        compiler = self.trust / "bin/trustc"
        for name in ("cargo-home", "rustup-home", ".ubd/tmp.noindex", ".config", ".cache", ".run"):
            (self.home / name).mkdir(mode=0o700, parents=True, exist_ok=True)
        self.env = {"HOME": str(self.home), "PATH": str(self.trust / "bin") + ":" + str(prefix / "bin") + ":/usr/bin:/bin:/usr/sbin:/sbin",
                    "ATPKG_BIN": str(prefix / "bin"), "ALAB_PYTHON": sys.executable,
                    "ALAB_CARGO_HOME": str(self.home / "cargo-home"),
                    "CARGO_HOME": str(self.home / "cargo-home"), "RUSTUP_HOME": str(self.home / "rustup-home"),
                    "CARGO_BUILD_JOBS": "1", "CARGO_INCREMENTAL": "0", "RUSTC": str(compiler),
                    "TMPDIR": str(self.home / ".ubd/tmp.noindex"),
                    "XDG_CONFIG_HOME": str(self.home / ".config"), "XDG_CACHE_HOME": str(self.home / ".cache"),
                    "XDG_RUNTIME_DIR": str(self.home / ".run"), "GIT_CONFIG_GLOBAL": os.devnull,
                    "GIT_CONFIG_NOSYSTEM": "1", "GIT_OPTIONAL_LOCKS": "0",
                    "BRIDGE_ROOT": str(ROOT), "BRIDGE_REPO": str(self.repo), "BRIDGE_CARGO": str(self.cargo)}
        info = self.run_command([str(compiler), "-vV"], "compiler-version")
        self.assertEqual(info.returncode, 0, info.stderr)
        self.triple = next(line[6:] for line in info.stdout.splitlines() if line.startswith("host: "))
        self.env["BRIDGE_TRIPLE"] = self.triple
        (self.repo / "codex-rs/Cargo.toml").write_text('[package]\nname = "native-budget-bridge"\nversion = "0.1.0"\nedition = "2021"\n[workspace]\n')
        (self.repo / "codex-rs/Cargo.lock").write_text('version = 4\n\n[[package]]\nname = "native-budget-bridge"\nversion = "0.1.0"\n')
        (self.repo / "codex-rs/src/main.rs").write_text(
            'fn main() { println!("PACKAGE_CACHE_WITNESS {}", '
            'std::env::current_exe().unwrap().display()); println!("13"); }\n')
        self.git("init", "--quiet")
        self.git("config", "user.name", "Native cache fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.commit("first inert source")
        self.capture = self.fixture / "capture.py"
        self.capture.write_text(textwrap.dedent(CAPTURE))
        self.worker = self.fixture / "worker.bash"
        self.worker.write_text(textwrap.dedent(WORKER))
        self.guardian_script = self.fixture / "guardian.py"
        self.guardian_script.write_text(textwrap.dedent(GUARDIAN))
        self.cache = self.home / "cache.noindex"
        self.cache.mkdir(mode=0o700)
        self.target = self.target_in(self.cache)
        (self.fixture / "inputs.json").write_text(json.dumps({"native": str(self.native), "native_sha256": self.native_sha,
                                                            "trust": str(self.trust), "python": sys.version}, indent=2))

    def tearDown(self):
        for child in self.children:
            if child.returncode is None:
                if child.stdin is not None and not child.stdin.closed:
                    child.stdin.close()
                    child.stdin = None
                try:
                    child.communicate(timeout=60)
                except subprocess.TimeoutExpired:
                    if child.returncode is None:
                        os.killpg(child.pid, signal.SIGTERM)
                    child.communicate(timeout=60)
            self.assertIsNotNone(child.returncode)
        self.assertEqual(sha256(self.native), self.native_sha, "the explicitly frozen native CLI changed")

    def run_command(self, argv, name, env=None):
        child = subprocess.Popen(argv, cwd=self.repo, env=env or self.env,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, start_new_session=True)
        self.children.append(child)
        try:
            out, err = child.communicate(timeout=60)
        except BaseException:
            if child.returncode is None:
                os.killpg(child.pid, signal.SIGTERM)
            out, err = child.communicate(timeout=60)
            raise
        (self.fixture / (name + ".stdout")).write_text(out)
        (self.fixture / (name + ".stderr")).write_text(err)
        return subprocess.CompletedProcess(argv, child.returncode, out, err)

    def git(self, *args):
        done = self.run_command(["git", "-C", str(self.repo), "-c", "commit.gpgsign=false", *args], "git-last")
        self.assertEqual(done.returncode, 0, done.stderr)
        return done.stdout.strip()

    def commit(self, message):
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", message)
        return self.git("rev-parse", "HEAD")

    def target_in(self, cache):
        legacy = cache / ("rust-release-" + self.triple + "-" + COMPILER + ".noindex")
        native = runpy.run_path(str(ROOT / "alab/package_cache.py"))
        return native["budgeted_target"](cache, legacy, self.repo / ".git/alab-preserve-builds")

    def receipt(self, target):
        matches = [json.loads(path.read_text()) for path in (self.home / INDEX).glob("*.json")]
        matches = [record for record in matches if record["root"] == str(target)]
        self.assertEqual(len(matches), 1, matches)
        return matches[0]

    def package(self, number, *, contained=False):
        env = {**self.env, "ALAB_PACKAGE_CACHE": str(self.cache), "BRIDGE_CAPTURE": str(self.capture),
               "BRIDGE_ADMITTED": str(self.fixture / ("admitted-" + str(number) + ".json")),
               "BRIDGE_WORKER": str(self.worker), "BRIDGE_OUTPUT": str(self.home / "output.noindex"),
               "BRIDGE_CONTAINED": "1" if contained else "0"}
        body = 'set -euo pipefail; . "$BRIDGE_ROOT/alab/lib.sh"; '
        if not contained:
            body += 'alab_account_home() { printf "%s\\n" "$HOME"; }; '
        body += 'alab_package_cache_locked "$ALAB_PACKAGE_CACHE" --build-disk "$BRIDGE_OUTPUT" "$BRIDGE_REPO" /bin/bash "$BRIDGE_WORKER"'
        done = self.run_command(["/bin/bash", "-c", body], "package-" + str(number), env)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(done.stdout.splitlines()[-1], "13", done.stdout)
        admitted = json.loads(Path(env["BRIDGE_ADMITTED"]).read_text())
        artifact = [json.loads(line) for line in done.stdout.splitlines() if line.startswith("{")]
        artifact = [row for row in artifact if row.get("reason") == "compiler-artifact" and row["target"]["name"] == "native-budget-bridge"]
        self.assertEqual(len(artifact), 1, done.stdout)
        final = self.receipt(self.target)
        self.assertEqual(final["schema"], 3)
        self.assertIsNone(final["package"]["active"])
        self.assertEqual(final["package"]["learned_budget"], 64 << 30)
        self.assertEqual(final["package"]["last_settled"], admitted["attempt"])
        self.assertEqual(json.loads(Path(admitted["source_guard"]).read_text())["phase"], "closed")
        active = admitted["receipt"]["package"]["active"]
        self.assertEqual(active["token"], admitted["attempt"])
        self.assertEqual(active["producer"]["pid"], admitted["guardian"])
        self.assertEqual(active["producer"]["group"], admitted["group"])
        with self.assertRaises(ProcessLookupError):
            os.killpg(admitted["group"], 0)
        return admitted, artifact[0], final

    def start_guardian(self, name, driver=None, cache=None, token=None):
        cache = cache or self.home / (name + ".noindex")
        cache.mkdir(mode=0o700, exist_ok=True)
        env = {**self.env, "BRIDGE_CACHE": str(cache), "BRIDGE_NATIVE": str(self.host),
               "BRIDGE_TARGET": str(self.target_in(cache)), "BRIDGE_TOKEN": token or secrets.token_hex(16),
               "BRIDGE_MANIFEST": str(self.repo / "codex-rs/Cargo.toml"), "BRIDGE_DRIVER": str(driver or self.cargo),
               "BRIDGE_PRESERVE": str(self.repo / ".git/alab-preserve-builds")}
        child = subprocess.Popen([sys.executable, "-I", "-B", str(self.guardian_script)], env=env, cwd=self.repo,
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                 text=True, start_new_session=True)
        self.children.append(child)
        self.assertTrue(select.select([child.stdout], [], [], 60)[0], "guardian readiness timed out")
        line = child.stdout.readline()
        self.assertTrue(line, "guardian exited before readiness")
        ready = json.loads(line)
        (self.fixture / (name + "-ready.json")).write_text(json.dumps(ready, indent=2))
        self.assertEqual(ready["phase"], "ready")
        self.assertTrue(ready["group_gone"], ready)
        with self.assertRaises(ProcessLookupError):
            os.killpg(ready["worker_group"], 0)
        return child, ready, env

    def finish_guardian(self, child, name, action):
        out, err = child.communicate(input=action + "\n", timeout=60)
        (self.fixture / (name + "-finish.stdout")).write_text(out)
        (self.fixture / (name + "-finish.stderr")).write_text(err)
        self.assertEqual(child.returncode, 0, err + out)
        reply = json.loads(out)
        if action == "settle":
            self.assertEqual(reply["phase"], "settled")
            self.assertEqual(reply["code"], 0, reply)
        return reply

    def test_real_package_build_reuses_native_policy_target_across_commits(self):
        first, built, before = self.package(1)
        identity = (self.target.stat().st_dev, self.target.stat().st_ino, before["nonce"])
        self.assertTrue(first["receipt"]["package"]["active"]["fresh"])
        self.assertFalse(built["fresh"])
        (self.repo / "uncompiled-note").write_text("only Git history changes; Rust inputs stay identical\n")
        self.commit("second commit changes no compiler input")
        second, cached, after = self.package(2)
        self.assertFalse(second["receipt"]["package"]["active"]["fresh"])
        self.assertTrue(cached["fresh"], cached)
        self.assertNotEqual(first["attempt"], second["attempt"])
        self.assertEqual(identity, (self.target.stat().st_dev, self.target.stat().st_ino, after["nonce"]))

    def test_real_v2_package_build_keeps_default_containment_and_settles(self):
        self.package(1, contained=True)
        output = (self.fixture / "package-1.stdout").read_text()
        self.assertIn("PACKAGE_CACHE_WITNESS " + str(self.target / "debug/native-budget-bridge"), output)

    def test_unrelated_settlement_is_refused_after_real_worker_group_retirement(self):
        import fcntl
        child, ready, env = self.start_guardian("settlement-authority")
        self.assertEqual(ready["code"], 0, ready)
        self.assertTrue(ready["reply"]["budgeted"], ready)
        target = Path(env["BRIDGE_TARGET"])
        before = self.receipt(target)
        self.assertEqual(before["package"]["active"]["token"], env["BRIDGE_TOKEN"])
        with (Path(env["BRIDGE_CACHE"]) / ".alab-package.lockf").open("r+") as lock:
            with self.assertRaises(BlockingIOError):
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        rejected = self.run_command([str(self.host), VERB, "settled", "--cache", env["BRIDGE_CACHE"],
                                     "--attempt", env["BRIDGE_TOKEN"], "--success"], "unrelated-settlement")
        self.assertEqual(rejected.returncode, 75, rejected.stderr)
        self.assertEqual(self.receipt(target), before)
        self.finish_guardian(child, "legitimate-settlement", "settle")
        self.assertIsNone(self.receipt(target)["package"]["active"])

    def test_real_registrar_compatibility_does_not_upgrade_an_old_or_ambiguous_shim(self):
        cases = (("new-physical-helper", [str(self.native)], True), ("new-symlink-helper", [str(self.host)], True),
                 ("old-helper", ["/usr/bin/true"], False),
                 ("missing-helper", [], False), ("ambiguous-helper", [str(self.host), str(self.host)], False))
        policy_cache = None
        for name, helpers, budgeted in cases:
            with self.subTest(shim=name):
                driver = self.fixture / (name + ".sh")
                driver.write_text("#!/bin/sh\n" + "".join("# atpkg cache helper " + json.dumps(helper) + "\n" for helper in helpers) + "exit 99\n")
                driver.chmod(0o700)
                child, ready, env = self.start_guardian(name, driver=driver)
                self.assertEqual(ready["code"], 0, ready)
                self.assertEqual(ready["reply"]["budgeted"], budgeted, ready)
                self.assertEqual(self.receipt(Path(env["BRIDGE_TARGET"]))["schema"], 3 if budgeted else 1)
                self.finish_guardian(child, name, "settle" if budgeted else "finish")
                if budgeted:
                    policy_cache = Path(env["BRIDGE_CACHE"])
        old = self.fixture / "old-helper.sh"
        child, ready, env = self.start_guardian("policy-old-helper", driver=old, cache=policy_cache)
        self.assertEqual(ready["code"], 75, ready)
        self.assertIsNone(self.receipt(Path(env["BRIDGE_TARGET"]))["package"]["active"])
        self.finish_guardian(child, "policy-old-helper", "finish")


if __name__ == "__main__":
    unittest.main()

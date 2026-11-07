#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Real package/guardian orchestration with an inert native cache authority."""

import hashlib
import fcntl
import json
import os
from pathlib import Path
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import textwrap
import time
import unittest


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ENTRY = "#!/bin/sh\n'''exec' " + shlex.quote(sys.executable) + ' -I "$0" "$@"\n\' \'\'\'\n'
COMPILER = "0123456789abcdef"
TARGET = "aarch64-apple-darwin"


HOST = r'''
import fcntl
import json
import os
from pathlib import Path
import re
import sys

args = sys.argv[1:]
prefix = args[:1] == ["pkg"]
if prefix:
    args = args[1:]
mode = os.environ.get("FIXTURE_MODE", "supported")
cache = Path(os.environ["FIXTURE_CACHE"])

def options(words):
    result = {}
    while words:
        word, *words = words
        assert word.startswith("--"), word
        if "=" in word:
            key, value = word[2:].split("=", 1)
        elif word in ("--success", "--require-owned"):
            key, value = word[2:], True
        else:
            key, value = word[2:], words[0]
            words = words[1:]
        result[key] = value
    return result

def lock_held():
    with (cache / ".alab-package.lockf").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return True
        fcntl.flock(lock, fcntl.LOCK_UN)
        return False

def record(kind, **fields):
    row = {"kind": kind, "locked": lock_held(), "pkg_prefix": prefix,
           "group": os.getpgrp(), **fields}
    with Path(os.environ["FIXTURE_EVENTS"]).open("a") as output:
        output.write(json.dumps(row) + "\n")
    return row

if args[0] == "__build_cache_register":
    if args[1:] == ["--capabilities"]:
        print("aterm-build-cache-register-v1")
        raise SystemExit(0)
    opts = options([word for word in args[1:]
                    if word.startswith(("--target-dir=", "--manifest-path="))
                    or word == "--require-owned"])
    target = Path(opts["target-dir"])
    record("register", target=str(target), existed=target.exists(),
           strict=bool(opts.get("require-owned")))
    if not target.exists():
        target.mkdir()
        (target / ".aterm-build-cache.json").write_text("inert v1 receipt\n")
    raise SystemExit(0)

assert args[0] == "__build_cache_package", args
if args[1:] == ["--capabilities"]:
    if mode == "old":
        raise SystemExit(2)
    print("aterm-package-build-cache-v1" if mode == "v1" else "aterm-package-build-cache-v2")
    raise SystemExit(0)
verb = args[1]
opts = options(args[2:])
assert Path(opts["cache"]) == cache, opts
if verb == "compact":
    record("compact")
    if mode == "compact-unavailable":
        print("fixture native compaction unavailable", file=sys.stderr)
        raise SystemExit(69)
    if mode == "compact-busy":
        print("fixture native compaction busy", file=sys.stderr)
        raise SystemExit(75)
    print(json.dumps({"status": "deferred", "reason": "inert fixture"}))
    raise SystemExit(0)

attempt = opts["attempt"]
assert re.fullmatch(r"[0-9a-f]{32}", attempt), attempt
state_path = cache / ("fixture-attempt-" + attempt + ".json")
if verb == "admit":
    target = Path(opts["target-dir"])
    guard = int(opts["producer-pid"])
    group = int(opts["producer-group"])
    source = Path(os.environ["ALAB_PACKAGE_SOURCE_GUARD"])
    source_record = json.loads(source.read_text())
    state = {"attempt": attempt, "guard": guard, "worker_group": group,
             "source": str(source), "preserve": opts["preserve-if-present"]}
    state_path.write_text(json.dumps(state))
    record("admit", **state, token_env=os.environ.get("ALAB_PACKAGE_CACHE_ATTEMPT"),
           guard_env=os.environ.get("ALAB_PACKAGE_GUARD_PID"),
           source_phase=source_record["phase"], source_pid=source_record["pid"],
           target=str(target), existed=target.exists(),
           driver=opts.get("compiler-driver"), registrar=opts.get("registrar"),
           manifest=opts.get("manifest-path"), budget=opts.get("budget-bytes"))
    if mode == "refuse":
        raise SystemExit(75)
    existed = target.exists()
    managed = (target / ".aterm-build-cache.json").is_file()
    budgeted = not existed or managed
    if not existed:
        target.mkdir()
        (target / ".aterm-build-cache.json").write_text("inert native receipt\n")
    reply = {"path": str(target), "budgeted": budgeted, "created": not existed,
             "attempt": attempt if budgeted else None}
    if mode == "bad-json":
        print("{broken")
        raise SystemExit(0)
    if mode == "wrong-path":
        reply["path"] = str(cache / "unrequested.noindex")
    if mode == "relative-path":
        reply["path"] = target.name
    if mode == "wrong-token":
        reply["attempt"] = ("1" if attempt[0] == "0" else "0") + attempt[1:]
    if mode == "missing-token":
        reply["attempt"] = None
    if mode == "bad-budgeted-type":
        reply["budgeted"] = 1
    if mode == "bad-created-type":
        reply["created"] = "yes"
    print(json.dumps(reply))
    raise SystemExit(0)

assert verb == "settled", verb
state = json.loads(state_path.read_text()) if state_path.exists() else {}
if not state:
    print("no generation admitted for this attempt", file=sys.stderr)
    raise SystemExit(75)
source = Path(state["source"]) if state else None
phase = json.loads(source.read_text())["phase"] if source else None
group_gone = False
if state:
    try:
        os.killpg(state["worker_group"], 0)
    except ProcessLookupError:
        group_gone = True
record("settled", attempt=attempt, success=bool(opts.get("success")),
       source_phase=phase, group_gone=group_gone,
       guardian_group=os.getpgid(state["guard"]) if state else None)
assert phase == "closed" and group_gone, (phase, group_gone)
if Path(state["preserve"]).exists():
    print("clone preservation holds settlement", file=sys.stderr)
    raise SystemExit(75)
print(json.dumps({"status": "settled"}))
'''


DRIVER = r'''
import fcntl
import json
import os
from pathlib import Path
import sys

cache = Path(os.environ["FIXTURE_CACHE"])
with (cache / ".alab-package.lockf").open("a") as lock:
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        held = False
    except BlockingIOError:
        held = True
source = os.environ.get("ALAB_PACKAGE_SOURCE_GUARD")
row = {"kind": "compiler", "locked": held, "group": os.getpgrp(),
       "target": os.environ.get("CARGO_TARGET_DIR"),
       "attempt": os.environ.get("ALAB_PACKAGE_CACHE_ATTEMPT"),
       "guard": os.environ.get("ALAB_PACKAGE_GUARD_PID"),
       "preserve": os.environ.get("ALAB_PACKAGE_CACHE_PRESERVE"), "source": source}
with Path(os.environ["FIXTURE_EVENTS"]).open("a") as output:
    output.write(json.dumps(row) + "\n")
Path(os.environ["FIXTURE_COMPILER_MARKER"]).write_text("inert compiler ran\n")
if os.environ.get("FIXTURE_LATE_HOLD"):
    Path(os.environ["ALAB_PACKAGE_CACHE_PRESERVE"]).write_text("preserve this clone's builds\n")
if os.environ.get("FIXTURE_TAMPER_SOURCE") == "1":
    path = Path(source)
    record = json.loads(path.read_text())
    record["phase"] = "fixture-changed"
    path.write_text(json.dumps(record))
raise SystemExit(int(os.environ.get("FIXTURE_CODE", "0")))
'''


WORKER = r'''
set -euo pipefail
. "$FIXTURE_ROOT/alab/lib.sh"
. "$FIXTURE_ROOT/alab/package.sh"
python="$FIXTURE_PYTHON" cache="$FIXTURE_CACHE" wt="$FIXTURE_REPO"
compiler_id=0123456789abcdef target_triple=aarch64-apple-darwin
cargo="$FIXTURE_DRIVER"
alab_account_home() { printf '%s\n' "$HOME"; }
if [[ "${FIXTURE_UNSAFE_PARENT:-0}" == 1 ]]; then
  "$python" -I -B -c 'import os; from pathlib import Path; os.chmod(Path(os.environ["ALAB_PACKAGE_CACHE_PRESERVE"]).parent, 0o777)'
fi
CARGO_TARGET_DIR="$(package_rust_target)"
export CARGO_TARGET_DIR
"$cargo"
'''


class PackageCacheBudgetTests(unittest.TestCase):
    def setUp(self):
        parent = Path(os.environ.get("ALAB_PACKAGE_CACHE_FIXTURE_ROOT", tempfile.gettempdir())) / ".ubd"
        parent.mkdir(parents=True, exist_ok=True)
        self.fixture = Path(tempfile.mkdtemp(prefix="package-cache-budget-", suffix=".noindex", dir=parent)).resolve()
        print(f"preserved package-cache-budget fixture: {self.fixture}", flush=True)
        self.home = self.fixture / "home"
        (self.home / "Developer").mkdir(parents=True)
        self.repo = self.fixture / "repo"
        (self.repo / "codex-rs").mkdir(parents=True)
        (self.repo / "codex-rs/Cargo.toml").write_text("[workspace]\nmembers = []\n")
        # Keep this fixture independent of Apple's optional Git and expose no
        # unrelated tools alongside the caller's selected executable.
        real_git = shutil.which("git")
        self.assertIsNotNone(real_git, "the cache fixtures require Git")
        tools = self.fixture / "tools"
        tools.mkdir()
        git = tools / "git"
        git.write_text("#!/bin/sh\nexec " + shlex.quote(real_git) + ' "$@"\n')
        git.chmod(0o700)
        self.base_env = {"HOME": str(self.home), "PATH": str(tools) + ":/usr/bin:/bin:/usr/sbin:/sbin",
                         "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1",
                         "GIT_OPTIONAL_LOCKS": "0", "ALAB_PYTHON": sys.executable,
                         "FIXTURE_ROOT": str(ROOT), "FIXTURE_REPO": str(self.repo),
                         "FIXTURE_PYTHON": sys.executable}
        for args in (("init", "--quiet"), ("config", "user.name", "Budget fixture"),
                     ("config", "user.email", "fixture@example.invalid"), ("add", "."),
                     ("-c", "commit.gpgsign=false", "commit", "--quiet", "-m", "inert fixture")):
            result = subprocess.run(["git", "-C", str(self.repo), *args], env=self.base_env,
                                    capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
        self.host = self.fixture / "aterm"
        self.prefix = self.fixture / "prefix"
        self.prefix.mkdir()
        self.atpkg = self.prefix / "atpkg"
        self.driver = self.fixture / "compiler-driver"
        for path, body in ((self.host, HOST), (self.atpkg, HOST), (self.driver, DRIVER)):
            path.write_text(PYTHON_ENTRY + textwrap.dedent(body))
            path.chmod(0o700)
        self.worker = self.fixture / "worker.bash"
        self.worker.write_text(textwrap.dedent(WORKER))

    def case(self, name, *, mode="supported", frontend="aterm", repo=None, cache=None):
        repo = repo or self.repo
        cache = cache or self.home / (name + ".noindex")
        cache.mkdir(mode=0o700, exist_ok=True)
        legacy = cache / f"rust-release-{TARGET}-{COMPILER}.noindex"
        common = Path(subprocess.check_output([
            "git", "-C", str(repo), "rev-parse", "--path-format=absolute", "--git-common-dir"],
            env=self.base_env, timeout=60).decode().rstrip("\n"))
        preserve = common / "alab-preserve-builds"
        info = common.stat()
        family = hashlib.sha256(os.fsencode(preserve) + b"\0" +
                                f"{info.st_dev}:{info.st_ino}".encode()).hexdigest()[:32]
        budgeted = cache / f"rust-budgeted-release-{TARGET}-{COMPILER}-{family}.noindex"
        target = legacy if mode in ("old", "v1") or frontend == "missing" else budgeted
        events, marker = cache / (name + "-events.jsonl"), cache / (name + "-compiler-ran")
        environment = {**self.base_env, "FIXTURE_MODE": mode, "FIXTURE_CACHE": str(cache),
                       "FIXTURE_EVENTS": str(events), "FIXTURE_COMPILER_MARKER": str(marker),
                       "FIXTURE_DRIVER": str(self.driver), "FIXTURE_WORKER": str(self.worker),
                       "FIXTURE_OUTPUT": str(self.home / "output.noindex"), "FIXTURE_REPO": str(repo)}
        if frontend == "aterm":
            environment["ALAB_ATERM"] = str(self.host)
        elif frontend == "atpkg":
            environment["ATPKG_BIN"] = str(self.prefix)
        else:
            environment["ALAB_ATERM"] = str(self.fixture / "missing-host")
        return {"cache": cache, "target": target, "legacy": legacy, "preserve": preserve,
                "events": events, "marker": marker,
                "env": environment, "frontend": frontend}

    def run_job(self, case, *, code=0, worker=None, plain=False, exec_root=False):
        environment = {**case["env"], "FIXTURE_CODE": str(code)}
        command = '. "$FIXTURE_ROOT/alab/lib.sh"; '
        command += 'alab_account_home() { printf "%s\\n" "$HOME"; }; '
        if plain:
            command += 'alab_package_cache_locked "$FIXTURE_CACHE" "$FIXTURE_DRIVER"'
        else:
            command += ('alab_package_cache_exec' if exec_root else 'alab_package_cache_locked')
            command += ' "$FIXTURE_CACHE" --build-disk "$FIXTURE_OUTPUT" "$FIXTURE_REPO" '
            command += '"$FIXTURE_MISSING"' if worker == "missing" else '/bin/bash "$FIXTURE_WORKER"'
            environment["FIXTURE_MISSING"] = str(self.fixture / "missing-worker")
        return subprocess.run(["/bin/bash", "-c", "set -euo pipefail; " + command],
                              cwd=self.fixture, env=environment,
                              capture_output=True, text=True, timeout=60)

    def events(self, case):
        return [json.loads(line) for line in case["events"].read_text().splitlines()] if case["events"].exists() else []

    def assert_lifecycle(self, case, rows, *, success):
        self.assertEqual([row["kind"] for row in rows], ["compact", "admit", "compiler", "settled", "compact"])
        before, admitted, compiled, settled, after = rows
        self.assertFalse(before["locked"])
        self.assertFalse(after["locked"])
        self.assertEqual(before["group"], os.getpgrp())
        self.assertEqual(after["group"], os.getpgrp())
        self.assertTrue(all(row["locked"] for row in (admitted, compiled, settled)))
        self.assertRegex(admitted["attempt"], r"^[0-9a-f]{32}$")
        self.assertEqual(admitted["attempt"], admitted["token_env"])
        self.assertEqual(str(admitted["guard"]), admitted["guard_env"])
        self.assertEqual(admitted["guard"], admitted["source_pid"])
        self.assertEqual(admitted["worker_group"], admitted["group"])
        self.assertEqual(compiled["group"], admitted["worker_group"])
        self.assertEqual(compiled["attempt"], admitted["attempt"])
        self.assertEqual(compiled["guard"], str(admitted["guard"]))
        self.assertEqual(compiled["target"], str(case["target"]))
        self.assertEqual(admitted["target"], str(case["target"]))
        self.assertEqual(admitted["manifest"], str(self.repo / "codex-rs/Cargo.toml"))
        self.assertEqual(admitted["driver"], str(self.driver))
        self.assertEqual(admitted["registrar"], str(self.host if case["frontend"] == "aterm" else self.atpkg))
        self.assertEqual(admitted["budget"], str(64 * 1024**3))
        self.assertEqual(admitted["preserve"], str(case["preserve"]))
        self.assertEqual(admitted["source_phase"], "active")
        self.assertEqual(settled["attempt"], admitted["attempt"])
        self.assertEqual(settled["success"], success)
        self.assertEqual(settled["source_phase"], "closed")
        self.assertTrue(settled["group_gone"])
        self.assertEqual(settled["group"], settled["guardian_group"])
        self.assertTrue(all(row["pkg_prefix"] == (case["frontend"] == "aterm")
                            for row in (before, admitted, settled, after)))

    def test_real_package_lifecycle_owns_locks_and_retires_each_fresh_attempt(self):
        for frontend in ("aterm", "atpkg"):
            with self.subTest(frontend=frontend):
                case = self.case(frontend, frontend=frontend)
                case["env"].update(ALAB_PACKAGE_CACHE_ATTEMPT="stale-caller-attempt", ALAB_PACKAGE_GUARD_PID="1",
                                    ALAB_PACKAGE_CACHE_PRESERVE="/unowned-stale-marker")
                first = self.run_job(case)
                self.assertEqual(first.returncode, 0, first.stderr)
                rows = self.events(case)
                self.assert_lifecycle(case, rows, success=True)
                identity = (case["target"].stat().st_dev, case["target"].stat().st_ino)
                second = self.run_job(case)
                self.assertEqual(second.returncode, 0, second.stderr)
                later = self.events(case)[len(rows):]
                self.assert_lifecycle(case, later, success=True)
                self.assertNotEqual(rows[1]["attempt"], later[1]["attempt"])
                self.assertEqual(identity, (case["target"].stat().st_dev, case["target"].stat().st_ino))

    def test_package_controller_cancellation_retires_writers_before_root_exit(self):
        for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
            with self.subTest(signal=signum):
                self.check_package_controller_cancellation(signum)

    def check_package_controller_cancellation(self, signum):
        case = self.case(f"cancel-root-{signum}")
        ready, release = case["cache"] / "ready", case["cache"] / "release"
        writer = self.fixture / "cancel-writer.py"
        writer.write_text(
            "import os,time\nfrom pathlib import Path\n"
            "root=Path(os.environ['FIXTURE_CACHE'])\n(root/'ready').touch()\n"
            "while not (root/'release').exists(): time.sleep(.01)\n"
            "(root/'released-writer').touch()\n")
        scripts = self.repo / "alab"
        scripts.mkdir(exist_ok=True)
        (scripts / "package.sh").write_text(
            'package_main() { exec "$FIXTURE_PYTHON" -I -B "$FIXTURE_CANCEL_WRITER"; }\n')
        # Exercise the actual sourceable selected-entry dispatch with an inert product.
        # Native authority authentication is outside this process-lifetime fixture.
        text = (ROOT / "alab/package.sh").read_text()
        function = "package_selected_entry() {" + text.split("package_selected_entry() {", 1)[1].split("\n}\n", 1)[0] + "\n}\n"
        body = '. "$FIXTURE_ROOT/alab/lib.sh"; repo_root="$FIXTURE_REPO"; '
        body += 'alab_project_selected() { return 0; }; alab_account_home() { printf "%s\\n" "$HOME"; }; '
        body += function + '\npackage_selected_entry --out "$FIXTURE_OUTPUT"'
        lock = case["cache"] / ".alab-package.lockf"
        def available():
            with lock.open("r+b") as stream:
                try:
                    fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    return True
                except BlockingIOError:
                    return False
        with (self.fixture / "cancel-stdout").open("wb") as out, (self.fixture / "cancel-stderr").open("wb") as err:
            child = subprocess.Popen(["/bin/bash", "-c", body], cwd=self.fixture,
                                     env={**case["env"], "FIXTURE_CANCEL_WRITER": str(writer),
                                          "ALAB_PACKAGE_CACHE": str(case["cache"])},
                                     stdout=out, stderr=err, start_new_session=True)
            try:
                deadline = time.monotonic() + 10
                while not ready.exists():
                    self.assertIsNone(child.poll(), "controller exited before writer startup")
                    self.assertLess(time.monotonic(), deadline, "writer startup hung")
                    time.sleep(.01)
                child.send_signal(signum)  # Only our own unreaped root, as the native front does.
                code = child.wait(timeout=10)
                unlocked_at_exit = available()
            finally:
                # Also retire the deliberately broken baseline without signalling a descendant PID.
                release.touch()
                if child.poll() is None:
                    child.send_signal(signal.SIGTERM)
                    child.wait(timeout=10)
                deadline = time.monotonic() + 10
                while lock.exists() and not available():
                    self.assertLess(time.monotonic(), deadline, "fixture custody did not retire")
                    time.sleep(.01)
        self.assertTrue(unlocked_at_exit, "root exit must not precede writer retirement and lock release")
        self.assertEqual(code, 128 + signum)
        self.assertFalse((case["cache"] / "released-writer").exists(), "cancellation stopped the real writer")

    def test_executable_guard_keeps_maintenance_order_and_product_exit_status(self):
        for code in (0, 23):
            with self.subTest(code=code):
                case = self.case(f"exec-root-{code}")
                done = self.run_job(case, code=code, exec_root=True)
                self.assertEqual(done.returncode, code, done.stderr)
                self.assert_lifecycle(case, self.events(case), success=code == 0)

    def test_failed_build_keeps_its_status_through_uncertain_maintenance(self):
        for mode in ("supported", "compact-unavailable", "compact-busy"):
            with self.subTest(mode=mode):
                case = self.case(mode, mode=mode)
                done = self.run_job(case, code=23)
                self.assertEqual(done.returncode, 23, done.stderr)
                self.assert_lifecycle(case, self.events(case), success=False)
                if mode != "supported":
                    self.assertRegex(done.stderr.lower(), r"cache|compact")

    def test_old_or_missing_hosts_use_registration_and_never_adopt_legacy_outputs(self):
        for host in ("old", "v1", "missing", "supported"):
            for legacy in (False, True):
                if host == "supported" and not legacy:
                    continue
                with self.subTest(host=host, legacy=legacy):
                    case = self.case(f"compat-{host}-{legacy}", mode=host if host in ("old", "v1") else "supported",
                                     frontend="missing" if host == "missing" else "aterm")
                    if legacy:
                        case["legacy"].mkdir()
                        (case["legacy"] / "reusable").write_bytes(b"owner-free inert old output")
                        identity = case["legacy"].stat().st_ino
                    done = self.run_job(case)
                    self.assertEqual(done.returncode, 0, done.stderr)
                    rows = self.events(case)
                    if host == "supported":
                        self.assert_lifecycle(case, rows, success=True)
                        self.assertFalse(any(row["kind"] == "register" for row in rows))
                    else:
                        self.assertFalse(any(row["kind"] in ("admit", "settled") for row in rows))
                        registered = [row for row in rows if row["kind"] == "register"]
                        self.assertEqual(len(registered), int(host != "missing"))
                        if registered:
                            self.assertEqual(registered[0]["existed"], legacy)
                            self.assertTrue(registered[0]["locked"])
                            self.assertLess(rows.index(registered[0]), next(i for i, row in enumerate(rows) if row["kind"] == "compiler"))
                    if legacy:
                        self.assertEqual(case["legacy"].stat().st_ino, identity)
                        self.assertEqual((case["legacy"] / "reusable").read_bytes(), b"owner-free inert old output")
                        self.assertFalse((case["legacy"] / ".aterm-build-cache.json").exists())
                    elif host == "missing":
                        self.assertFalse((case["target"] / ".aterm-build-cache.json").exists())

    def test_unadopted_budgeted_name_keeps_its_contents_and_uses_legacy_target(self):
        case = self.case("unadopted-budgeted")
        case["target"].mkdir()
        kept = case["target"] / "unknown-output"
        kept.write_bytes(b"unadopted output must survive\n")
        before = case["target"].stat().st_ino
        done = self.run_job(case)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(case["target"].stat().st_ino, before)
        self.assertEqual(kept.read_bytes(), b"unadopted output must survive\n")
        rows = self.events(case)
        compiler = next(row for row in rows if row["kind"] == "compiler")
        self.assertEqual(compiler["target"], str(case["legacy"]))

    def test_independent_clones_are_distinct_while_linked_worktrees_share_one_family(self):
        clone, linked = self.fixture / "independent-clone", self.fixture / "linked-worktree"
        for args in (("clone", "--quiet", str(self.repo), str(clone)),
                     ("-C", str(self.repo), "worktree", "add", "--quiet", "--detach", str(linked), "HEAD")):
            done = subprocess.run(["git", *args], env=self.base_env, capture_output=True, text=True, timeout=60)
            self.assertEqual(done.returncode, 0, done.stderr)
        first = self.case("family-first")
        cases = (first, self.case("family-clone", repo=clone, cache=first["cache"]),
                 self.case("family-linked", repo=linked, cache=first["cache"]))
        for case in cases:
            done = self.run_job(case)
            self.assertEqual(done.returncode, 0, done.stderr)
            rows = self.events(case)
            self.assertEqual(next(row for row in rows if row["kind"] == "compiler")["target"], str(case["target"]))
            self.assertEqual(next(row for row in rows if row["kind"] == "admit")["preserve"], str(case["preserve"]))
        self.assertNotEqual(first["target"], cases[1]["target"])
        self.assertEqual(first["target"], cases[2]["target"])
        self.assertEqual(first["target"].stat().st_ino, cases[2]["target"].stat().st_ino)

    def test_initial_and_late_preservation_keep_outputs_without_failing_the_build(self):
        initial = self.case("initial-preservation")
        initial["preserve"].write_text("keep this clone's builds\n")
        try:
            done = self.run_job(initial)
            self.assertEqual(done.returncode, 0, done.stderr)
            self.assertFalse(initial["target"].exists())
            self.assertFalse(any(row["kind"] == "admit" for row in self.events(initial)))
            self.assertTrue(initial["legacy"].is_dir())
        finally:
            initial["preserve"].unlink()
        kept = self.case("keep-worktree-policy")
        kept["env"]["ALAB_PKG_KEEP_WORKTREE"] = "1"
        done = self.run_job(kept)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertFalse(kept["target"].exists())
        self.assertFalse(any(row["kind"] == "admit" for row in self.events(kept)))
        self.assertTrue(kept["legacy"].is_dir())
        late = self.case("late-preservation")
        late["env"]["FIXTURE_LATE_HOLD"] = "1"
        done = self.run_job(late)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn("package cache settlement deferred", done.stderr)
        self.assertTrue(late["preserve"].is_file())
        self.assertTrue(late["target"].is_dir())

    def test_native_admission_refusal_or_invalid_reply_starts_no_compiler_or_fallback(self):
        for mode in ("refuse", "bad-json", "wrong-path", "relative-path", "wrong-token",
                     "missing-token", "bad-budgeted-type", "bad-created-type"):
            with self.subTest(mode=mode):
                case = self.case(mode, mode=mode)
                done = self.run_job(case)
                self.assertEqual(done.returncode, 75, done.stderr)
                self.assertFalse(case["marker"].exists())
                rows = self.events(case)
                self.assertEqual(sum(row["kind"] == "admit" for row in rows), 1)
                self.assertFalse(any(row["kind"] in ("compiler", "register") for row in rows))

    def test_untrusted_guardian_parent_refuses_before_native_admission_or_fallback(self):
        case = self.case("untrusted-parent")
        case["env"]["FIXTURE_UNSAFE_PARENT"] = "1"
        done = self.run_job(case)
        self.assertEqual(done.returncode, 75, done.stderr)
        self.assertIn("package preservation parent is uncertain", done.stderr)
        self.assertFalse(case["marker"].exists())
        self.assertFalse(any(row["kind"] in ("admit", "compiler", "register") for row in self.events(case)))

    def test_unfinished_guardian_or_refused_source_return_never_settles(self):
        unavailable = self.case("unavailable-source-guard")
        unknown = unavailable["cache"] / ".ubd/package-source-guards.noindex"
        unknown.parent.mkdir()
        unknown.write_bytes(b"inert unknown metadata must survive\n")
        unavailable["env"].update(ALAB_PACKAGE_CACHE_ATTEMPT="f" * 32, ALAB_PACKAGE_GUARD_PID="1",
                                  ALAB_PACKAGE_CACHE_PRESERVE="/unowned-stale-marker")
        done = self.run_job(unavailable)
        self.assertEqual(done.returncode, 0, done.stderr)
        rows = self.events(unavailable)
        self.assertEqual([row["kind"] for row in rows], ["compact", "register", "compiler", "compact"])
        self.assertIsNone(rows[2]["attempt"])
        self.assertIsNone(rows[2]["guard"])
        self.assertIsNone(rows[2]["preserve"])
        self.assertIsNone(rows[2]["source"])
        self.assertEqual(unknown.read_bytes(), b"inert unknown metadata must survive\n")
        missing = self.case("missing-worker")
        done = self.run_job(missing, worker="missing")
        self.assertNotEqual(done.returncode, 0, done.stderr)
        self.assertFalse(any(row["kind"] == "settled" for row in self.events(missing)))
        records = list((missing["cache"] / ".ubd/package-source-guards.noindex").glob("guard-*/guard.json"))
        self.assertEqual(len(records), 1)
        self.assertEqual(json.loads(records[0].read_text())["phase"], "active")
        changed = self.case("changed-source-guard")
        changed["env"]["FIXTURE_TAMPER_SOURCE"] = "1"
        done = self.run_job(changed)
        self.assertTrue(changed["marker"].exists())
        self.assertFalse(any(row["kind"] == "settled" for row in self.events(changed)))
        self.assertIn("source", done.stderr.lower())
        admitted = next(row for row in self.events(changed) if row["kind"] == "admit")
        self.assertEqual(json.loads(Path(admitted["source"]).read_text())["phase"], "fixture-changed")
        with self.assertRaises(ProcessLookupError):
            os.killpg(admitted["worker_group"], 0)

    def test_nonpackage_guard_clients_keep_their_existing_behavior(self):
        case = self.case("ordinary-guard")
        done = self.run_job(case, code=23, plain=True)
        self.assertEqual(done.returncode, 23, done.stderr)
        rows = self.events(case)
        self.assertEqual([row["kind"] for row in rows], ["compiler"])
        self.assertTrue(rows[0]["locked"])
        self.assertIsNone(rows[0]["attempt"])
        self.assertIsNone(rows[0]["guard"])
        self.assertIsNone(rows[0]["source"])
        self.assertFalse((case["cache"] / ".ubd/package-source-guards.noindex").exists())


if __name__ == "__main__":
    unittest.main()

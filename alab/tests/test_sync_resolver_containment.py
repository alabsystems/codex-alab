#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""A real contained sync resolver may fill its private cache without changing code."""

import errno
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest

from test_github import disk_fixture_cli


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ENTRY = "#!/bin/sh\n'''exec' " + shlex.quote(sys.executable) + ' -I "$0" "$@"\n\' \'\'\'\n'


@unittest.skipUnless(Path("/usr/bin/sandbox-exec").is_file(), "requires macOS containment")
class SyncResolverContainmentTests(unittest.TestCase):
    def test_nested_private_cache_stays_writable_and_tooling_and_source_stay_readonly(self):
        directory = Path(tempfile.mkdtemp(prefix="alab-resolver-", suffix=".noindex")).resolve()
        print(f"preserved resolver fixture: {directory}", flush=True)
        upstream, repo, home = (directory / name for name in ("upstream", "repo", "home"))
        home.mkdir()
        environment = {key: value for key, value in os.environ.items()
                       if not key.startswith(("ALAB_", "GIT_"))}
        for key in ("CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR", "CARGO_BUILD_BUILD_DIR"):
            environment.pop(key, None)
        environment.update(HOME=str(home), GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1")

        def git(folder, *args):
            return subprocess.run(["git", "-C", str(folder), *args], env=environment,
                                  capture_output=True, text=True, check=True).stdout.strip()

        upstream.mkdir()
        git(upstream, "init", "-q", "-b", "main")
        for folder in (upstream,):
            git(folder, "config", "user.name", "Fixture")
            git(folder, "config", "user.email", "fixture@invalid")
        (upstream / "codex-rs").mkdir()
        (upstream / "codex-rs/Cargo.toml").write_text('[workspace]\nmembers = []\n')
        (upstream / "codex-rs/Cargo.lock").write_text('version = 4\n')
        git(upstream, "add", ".")
        git(upstream, "commit", "-qm", "upstream base")
        subprocess.run(["git", "clone", "-q", str(upstream), str(repo)],
                       env=environment, check=True)
        git(repo, "config", "user.name", "Fixture")
        git(repo, "config", "user.email", "fixture@invalid")
        git(repo, "config", "rerere.enabled", "true")
        git(repo, "remote", "set-url", "origin", str(repo))
        git(repo, "remote", "add", "upstream", str(upstream))
        scripts = repo / "alab"
        scripts.mkdir()
        for name in ("sync.sh", "lib.sh", "sync_worktree.py", "github.py", "github_push.py", "cli_status.py", "command_guard.py", "retention.py", "merge-attributes",
                     "removed-paths", "owned-paths", "watched-paths", "project-targo.sh"):
            shutil.copy2(ROOT / "alab" / name, scripts / name)
        merged = repo / ".git/alab-sync"
        witness = directory / "selected-worktree"
        native = directory / "native-selector"
        front = directory / "native-front"
        cargo = scripts / "fake-cargo"
        front.write_text(PYTHON_ENTRY + '''import os, sys
assert sys.argv[1:3] == ["__build_admit", os.environ["ATPKG_PROJECT_TARGO"]], sys.argv
assert os.environ["CARGO_TARGET_DIR"] == os.getcwd() + "/target"
os.execv(sys.argv[2], sys.argv[2:])
''')
        native.write_text(PYTHON_ENTRY + f'''import os, sys
from pathlib import Path
assert sys.argv[1:6] == ["toolchain", "run", "--repo", {str(merged)!r}, "--"], sys.argv
Path({str(witness)!r}).write_text(sys.argv[4])
os.environ.update(ATPKG_PROJECT_FRONT={str(front)!r}, ATPKG_PROJECT_TARGO={str(cargo)!r},
                  ATPKG_PROJECT_TRUSTC="/usr/bin/true", ATPKG_PROJECT_TRUSTDOC="/usr/bin/true",
                  ATPKG_PROJECT_PAYLOAD={str(scripts)!r}, ATPKG_PROJECT_EXECUTION={str(scripts)!r})
os.execv(sys.argv[6], sys.argv[6:])
''')
        front.chmod(0o755)
        native.chmod(0o755)
        cargo.write_text(PYTHON_ENTRY + '''import errno, json, os, sys
from pathlib import Path
assert sys.argv[1:] == ["metadata", "--format-version", "1"], sys.argv
cache = Path(os.environ["CARGO_HOME"])
(cache / "registry/cache/new-dependency.crate").write_bytes(b"new public dependency archive")
protected = [Path(__file__).with_name("lib.sh"), Path.cwd() / "Cargo.toml"]
denials = []
for path in protected:
    try:
        descriptor = os.open(path, os.O_WRONLY)
    except OSError as error:
        denials.append(error.errno)
    else:
        os.close(descriptor)
        denials.append(0)
(Path.cwd() / "Cargo.lock").write_text("version = 4\\n# contained resolver wrote its lock\\n")
(cache / "witness.json").write_text(json.dumps(denials))
print(json.dumps({"packages": [], "workspace_members": [],
                  "workspace_root": str(Path.cwd()), "target_directory": str(Path.cwd() / "target")}))
''')
        cargo.chmod(0o755)
        # Only these two fixture-local tools measure capacity. Run their real
        # entry points with a fixed reading while preserving sandbox and Git behavior.
        python = scripts / "fixture-python"
        interpreter = shlex.quote(sys.executable)
        dispatch = []
        for tool in (scripts / "github.py", scripts / "command_guard.py"):
            fixture = shlex.quote(disk_fixture_cli(tool, 21))
            dispatch.append(f'if [ "$1" = -I ] && [ "$2" = -B ] && [ "$3" = {shlex.quote(str(tool))} ]; then\n'
                            f'  shift 3\n  exec {interpreter} -I -B -c {fixture} "$@"\nfi\n')
        python.write_text("#!/bin/sh\n" + "".join(dispatch) + f'exec {interpreter} "$@"\n')
        python.chmod(0o755)
        git(repo, "add", ".")
        git(repo, "commit", "-qm", "ALab tooling")
        (upstream / "new-upstream-file").write_text("new upstream behavior\n")
        git(upstream, "add", ".")
        git(upstream, "commit", "-qm", "upstream change")
        desired = git(upstream, "rev-parse", "HEAD")
        private = repo / ".git/alab-cargo-home"
        (private / "registry/cache").mkdir(parents=True)
        (private / ".alab-seeded").touch()
        environment.update(ALAB_CONTAIN="1", ALAB_ATPKG_PUBLISH=str(native),
                           ALAB_CARGO_HOME=str(private), ALAB_PYTHON=str(python),
                           ALAB_ATERM="/usr/bin/false")
        done = subprocess.run(
            ["/bin/bash", str(scripts / "sync.sh"), "--ref", desired, "--no-check", "--no-push"],
            cwd=repo, env=environment, capture_output=True, text=True, timeout=60,
        )
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        self.assertEqual(witness.read_text(), str(merged))
        self.assertEqual((private / "registry/cache/new-dependency.crate").read_bytes(),
                         b"new public dependency archive")
        self.assertTrue(all(code in (errno.EPERM, errno.EACCES)
                            for code in json.loads((private / "witness.json").read_text())))
        self.assertEqual((scripts / "lib.sh").read_bytes(), (ROOT / "alab/lib.sh").read_bytes())
        self.assertIn("contained resolver wrote its lock", (merged / "codex-rs/Cargo.lock").read_text())
        self.assertEqual(git(merged, "show", "HEAD:new-upstream-file"), "new upstream behavior")


if __name__ == "__main__":
    unittest.main()

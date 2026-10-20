#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""alab/prune.py against a scratch package root and a scratch atpkg prefix (never the real
one, never a real package): which packages past the newest <n> it keeps, and why.

  marker    a package atpkg's link marker (<prefix>/links/codex) names is kept.
  symlink   a package the bin/codex shim symlinks into is kept, and the shim is never read
            through: its target is a FIFO, so a read through it would block (the run is
            bounded: a hang fails the test).
  twin      a package the agents/codex twin script execs is kept, its path read whole
            (the package's name has a space in it).
  which     a package only `aterm pkg which codex` names (a stand-in aterm on PATH, in the
            real output's form, its paths under a directory with a space) is kept.
  owed      a package only a standing codex-alab session record names (its resumeArgv[0]:
            a crash relaunch still owed, its process dead) is kept.
  running   a package a live process executes from (an ad hoc signed copy of /bin/sleep
            as its bin/codex) is kept.
  pruned    a package nothing uses, past the newest <n>, is removed.
  homes     a record under a home named only in ALAB_CODEX_HOMES keeps its package (a
            dry run), and is unseen without it.
  controls  with the marker, the shims, aterm, the process and the record gone, the same
            packages are pruned; a marker or a record that stands but cannot be read prunes
            nothing (fail closed).

usage: prune_test.py     Exit 0 when every check passes, 1 otherwise.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
PRUNE = os.path.join(os.path.dirname(HERE), "prune.py")
# A wait for what must happen is a one-minute hang detector, never a latency budget.
HANG = 60

failures = []


def check(what, ok, detail=""):
    print(f"{'ok  ' if ok else 'FAIL'} {what}{(': ' + detail) if detail and not ok else ''}")
    if not ok:
        failures.append(what)


def package(root, name, built_at):
    d = os.path.join(root, name)
    os.makedirs(os.path.join(d, "bin"))
    with open(os.path.join(d, "alab-build.json"), "w") as f:
        json.dump({"version": name, "built_at": built_at}, f)
    return d


def prune(root, prefix, keep, codex_home, aterm_dir=None):
    # No aterm on PATH unless a stand-in is given: only the prefix's files, the session
    # records and the process table speak.
    path = "/usr/bin:/bin" if aterm_dir is None else f"{aterm_dir}:/usr/bin:/bin"
    env = {"PATH": path, "HOME": os.path.dirname(prefix)}
    done = subprocess.run(
        [
            sys.executable,
            "-I",
            PRUNE,
            "--root",
            root,
            "--keep",
            str(keep),
            "--prefix",
            prefix,
            "--codex-home",
            codex_home,
        ],
        capture_output=True,
        text=True,
        timeout=HANG,
        env=env,
    )
    return done.returncode, done.stderr


def dry_run_default_homes(root, prefix, homes):
    """A --dry-run with the homes left to their default, ALAB_CODEX_HOMES set to `homes`
    (HOME is the scratch dir, so ~/.codex names nothing real)."""
    env = {"PATH": "/usr/bin:/bin", "HOME": os.path.dirname(prefix)}
    if homes is not None:
        env["ALAB_CODEX_HOMES"] = homes
    done = subprocess.run(
        [sys.executable, "-I", PRUNE, "--root", root, "--keep", "1", "--prefix", prefix, "--dry-run"],
        capture_output=True,
        text=True,
        timeout=HANG,
        env=env,
    )
    return done.stderr


def left(root):
    return sorted(n for n in os.listdir(root) if not n.startswith("."))


def main():
    scratch = tempfile.mkdtemp(prefix="alab-prune-test.")
    sleeper = None
    try:
        root = os.path.join(scratch, "artifacts", "codex")
        prefix = os.path.join(scratch, "pkg")
        for sub in ("links", "bin", "agents"):
            os.makedirs(os.path.join(prefix, sub))
        names = ["p1", "p2", "p 3", "p4", "p5", "p6"]
        dirs = {n: package(root, n, f"2026-10-16T0{i}:00:00Z") for i, n in enumerate(names)}
        # Older than all of them: candidates whatever --keep is.
        dirs["p 7"] = package(root, "p 7", "2026-10-15T22:00:00Z")
        dirs["p8"] = package(root, "p8", "2026-10-15T23:00:00Z")
        codex_home = os.path.join(scratch, "codex home")
        sessions = os.path.join(codex_home, "alab", "sessions")
        os.makedirs(sessions)

        # marker: atpkg's linkmode TOML, naming p1 as the checkout.
        with open(os.path.join(prefix, "links", "codex"), "w") as f:
            f.write(
                'schema = 1\nprogram = "codex"\n'
                f'checkout = "{dirs["p1"]}"\nbins = ["bin/codex"]\n'
                'linked_at = "2026-10-16T00:00:00Z"\n'
            )
        # symlink: bin/codex -> p2/bin/codex, a FIFO nothing writes.
        os.mkfifo(os.path.join(dirs["p2"], "bin", "codex"))
        os.symlink(os.path.join(dirs["p2"], "bin", "codex"), os.path.join(prefix, "bin", "codex"))
        # twin: agents/codex execs the codex of "p 3" (a space in its path).
        twin = os.path.join(prefix, "agents", "codex")
        with open(twin, "w") as f:
            f.write(f"#!/bin/sh\nexec '{dirs['p 3']}/bin/codex' \"$@\"\n")
        os.chmod(twin, 0o755)
        # which: a stand-in aterm whose `pkg which codex` names "p 7", as the real one prints.
        aterm_dir = os.path.join(scratch, "aterm bin")
        os.makedirs(aterm_dir)
        with open(os.path.join(aterm_dir, "aterm"), "w") as f:
            f.write(
                '#!/bin/sh\n[ "$1 $2 $3" = "pkg which codex" ] || exit 1\n'
                f"echo 'codex → {prefix}/agents/codex → {dirs['p 7']}/bin/codex — managed "
                "0.1.0 — `codex update` here runs `aterm pkg update codex`'\n"
            )
        os.chmod(os.path.join(aterm_dir, "aterm"), 0o755)
        # owed: a dead process's record whose resume line runs p8's codex.
        record = os.path.join(sessions, "999999.json")
        with open(record, "w") as f:
            json.dump(
                {
                    "v": 1,
                    "pid": 999999,
                    "resumeArgv": [os.path.join(dirs["p8"], "bin", "codex"), "resume", "019a-id"],
                    "status": "busy",
                    "exit": "signal:1",
                },
                f,
            )
        # running: a copy of /bin/sleep as p4's bin/codex, signed ad hoc, left running.
        exe = os.path.join(dirs["p4"], "bin", "codex")
        shutil.copyfile("/bin/sleep", exe)
        os.chmod(exe, 0o755)
        subprocess.run(["codesign", "--force", "--sign", "-", exe], check=True, capture_output=True)
        sleeper = subprocess.Popen([exe, "600"])

        try:
            code, said = prune(root, prefix, 1, codex_home, aterm_dir)
        except subprocess.TimeoutExpired:
            check("the bin/codex symlink is never read through", False, "the run hung on the FIFO")
            return 1
        check("the bin/codex symlink is never read through", True)
        check("prune exits 0", code == 0, said)
        kept = left(root)
        check("marker: the linked package is kept", "p1" in kept, said)
        check("symlink: the package the shim names is kept", "p2" in kept, said)
        check("twin: the package the twin execs is kept, a space in its path", "p 3" in kept, said)
        check("which: the package aterm pkg which names is kept", "p 7" in kept, said)
        check("owed: the package a standing record's resume line runs is kept", "p8" in kept, said)
        check("running: the package a process runs from is kept", "p4" in kept, said)
        check("pruned: the unused package past the newest is removed", "p5" not in kept, said)
        check("the newest is kept", "p6" in kept, said)
        check("it says why it kept each", said.count("keeping ") == 6, said)
        before = left(root)

        # A record that stands but cannot be read: fail closed, nothing pruned (without aterm,
        # "p 7" is otherwise unused).
        bad = os.path.join(sessions, "999998.json")
        with open(bad, "w") as f:
            f.write("{not json")
        code, said = prune(root, prefix, 1, codex_home)
        check("control: an unreadable record prunes nothing", left(root) == before, said)
        os.remove(bad)
        # A writer's temporary is no record.
        with open(os.path.join(sessions, "999998.json.1a2b.tmp"), "w") as f:
            f.write("{half")

        # Controls: nothing uses p1..p4 any more.
        sleeper.kill()
        sleeper.wait()
        sleeper = None
        for f in ("links/codex", "bin/codex", "agents/codex"):
            os.remove(os.path.join(prefix, f))
        # A marker that stands but cannot be read: fail closed, nothing pruned.
        os.symlink("/nonexistent", os.path.join(prefix, "links", "codex"))
        code, said = prune(root, prefix, 1, codex_home)
        check("control: an unreadable marker prunes nothing", left(root) == before, said)
        os.remove(os.path.join(prefix, "links", "codex"))
        # No link source at all: nothing can be told, nothing pruned.
        code, said = prune(root, prefix, 1, codex_home)
        check("control: with no link source nothing is pruned", left(root) == before, said)
        # A shim that names none of them: the unused are pruned, the owed one kept.
        os.symlink("/usr/bin/true", os.path.join(prefix, "bin", "codex"))
        code, said = prune(root, prefix, 1, codex_home)
        check("control: unused, the same packages are pruned", left(root) == ["p6", "p8"], said)
        # homes: a CODEX_HOME named only in ALAB_CODEX_HOMES is read by default too.
        said = dry_run_default_homes(root, prefix, f"/nonexistent:{codex_home}")
        check("homes: ALAB_CODEX_HOMES's record keeps its package", "/p8: a codex-alab session record names it" in said, said)
        said = dry_run_default_homes(root, prefix, None)
        check("control: without it the record is unseen", "/p8 (newest 1 kept)" in said and "would prune" in said, said)
        # The record gone (swept, or its relaunch done): its package is pruned too.
        os.remove(record)
        code, said = prune(root, prefix, 1, codex_home)
        check("control: with no record naming it, the owed package is pruned", left(root) == ["p6"], said)
    finally:
        if sleeper is not None:
            sleeper.kill()
            sleeper.wait()
        shutil.rmtree(scratch, ignore_errors=True)
    print("prune_test: " + ("ok" if not failures else f"{len(failures)} failed"))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())

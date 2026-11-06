#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Poll origin/main for the already selected local artifact channel.

The dispatcher fetches once; the fetched public entry owns the transaction's cache lock.
Fetched tooling runs in a private source worktree; the supplied checkout stays untouched.
It never merges, pushes, or installs its optional LaunchAgent.
"""

import argparse
from contextlib import contextmanager
from datetime import datetime, timezone
import fcntl
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import pwd
import runpy
import shutil
import stat
import subprocess
import sys
import tempfile


# Loading a controller or helper must not leave ignored bytecode in either checkout.
sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("alab_promote", ROOT / "alab/promote.py")
PROMOTE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PROMOTE)
LABEL = "systems.alab.codex-alab-local-update"
GUARDIAN_CONTRACT = "cache-lock-v2"
CHANNEL_STATUS = ".alab-local-update-status.json"
WORK_PHASES = frozenset(("fetching", "checking", "building", "selecting"))


def git(repo, *args):
    done = subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True,
                          timeout=60)
    if done.returncode:
        raise RuntimeError(f"git {args[0]} failed: {done.stderr.strip() or done.returncode}")
    return done.stdout.strip()


def published_source(root, source):
    """Reuse a complete exact-source artifact, never rebuilding a corrupt published one."""
    matches = []
    failure = None
    entries = 0
    for path in root.iterdir():
        # The one display sidecar is not an artifact-directory entry for this guard.
        if path.name == CHANNEL_STATUS:
            continue
        if entries >= 4096:
            raise RuntimeError("artifact directory exceeds 4096 entries")
        entries += 1
        if path.name.startswith(".") or path.is_symlink() or not path.is_dir():
            continue
        try:
            record = json.loads(PROMOTE.regular_bytes(path / "alab-build.json"))
        except (OSError, ValueError, TypeError):
            continue
        if not isinstance(record, dict) or record.get("source") != source:
            continue
        try:
            actual, version = PROMOTE.package(path, root)
        except (OSError, ValueError, TypeError, AttributeError) as error:
            # A previously published copy can be damaged while a valid rebuilt copy
            # is already available. Validate the whole source set before refusing it;
            # never rebuild or change the damaged artifact as part of recovery.
            if failure is None:
                failure = error
            continue
        if actual == source:
            matches.append((version, path.name, path))
    if matches:
        return max(matches)[2]
    if failure is not None:
        raise failure
    return None


def bootstrap_observation(checkout):
    """Best-effort journal hints, never package admission or version parsing."""
    try:
        record = json.loads(PROMOTE.regular_bytes(Path(checkout) / "alab-build.json"))
        if not isinstance(record, dict):
            return {}
        source = record.get("source")
        version = record.get("version")
        observed = {}
        if isinstance(source, str) and PROMOTE.SOURCE.fullmatch(source):
            observed["initial_selected_source"] = source
        if (isinstance(version, str) and 0 < len(version) <= 128
                and all(ord(char) >= 0x20 and ord(char) != 0x7f for char in version)):
            try:
                version.encode("utf-8")
            except UnicodeError:
                pass
            else:
                observed["initial_selected_version"] = version
        return observed
    except (OSError, ValueError, TypeError):
        return {}


def update_once(repo, out, prefix, aterm, *, run=subprocess.run, build=None, keep=3,
                progress=None, source=None, dispatch=None, expected_checkout=None,
                expected_prefix=None):
    """Inspect and dispatch, or transact under the fixed controller's cache guardian."""
    if source is not None and not all(
            isinstance(value, str) and Path(value).is_absolute()
            for value in (expected_checkout, expected_prefix)):
        raise RuntimeError("controller handoff requires the original checkout and prefix")
    repo = Path(repo).resolve(strict=True)
    out = Path(out).absolute()
    root = (out / "codex").resolve()
    prefix = Path(prefix).resolve()
    if expected_prefix is not None and str(prefix) != expected_prefix:
        return False, "the prefix changed during controller handoff; leaving the selection in place"
    bootstrap = source is None and dispatch is not None
    try:
        if bootstrap:
            selected = PROMOTE.selected_checkout(root, prefix)
            observed = bootstrap_observation(selected)
        else:
            selected, old_source, old_version = PROMOTE.selected_package(root, prefix)
            observed = {"initial_selected_source": old_source,
                        "initial_selected_version": old_version.text}
    except (OSError, ValueError, TypeError, AttributeError) as error:
        return False, f"no selected local artifact channel ({error})"
    if expected_checkout is not None and selected != expected_checkout:
        return False, "the selection changed during controller handoff; leaving it in place"
    report = progress or (lambda phase, **details: None)
    report("fetching", initial_selected_checkout=selected, **observed)
    # Only the bootstrap fetches. Its internal handoff supplies that immutable commit,
    # so fresh tooling neither fetches a different tip nor recursively dispatches.
    if source is None:
        git(repo, "fetch", "--quiet", "--no-tags", "origin",
            "refs/heads/main:refs/remotes/origin/main")
        source = git(repo, "rev-parse", "--verify", "refs/remotes/origin/main^{commit}")
    if not PROMOTE.SOURCE.fullmatch(source):
        raise RuntimeError("origin/main did not resolve to an exact source commit")
    report("checking", target_source=source)
    if bootstrap:
        # A no-op needs no package authority. Any advancing source is admitted only
        # by its fresh controller: old package/version grammar must not strand it.
        if source == observed.get("initial_selected_source"):
            return False, "the selection observed at start already matches origin/main"
        return dispatch(source, selected, str(prefix))
    if PROMOTE.git(repo, "merge-base", "--is-ancestor", old_source, source)[0] != 0:
        return False, "origin/main does not advance the selected source; leaving it in place"
    if source == old_source:
        return False, "the selection observed at start already matches origin/main"
    if not aterm or not Path(aterm).is_absolute():
        raise RuntimeError("ALAB_ATERM must name the absolute durable aterm executable")
    PROMOTE.require_selector(str(aterm), run)
    try:
        if PROMOTE.selected_package(root, prefix)[0] != selected:
            return False, "the selection changed while checking origin/main; leaving it in place"
    except (OSError, ValueError, TypeError, AttributeError):
        return False, "the local artifact channel was released; leaving it in place"
    candidate = published_source(root, source)
    if candidate is None:
        report("building")
        if build is None:
            # The outer public entry already owns the package cache. Call the existing
            # function, not package_entry, so this child cannot reacquire its parent's lock.
            def build(commit):
                return subprocess.run(
                    ["/bin/bash", "-c", '. "$1/alab/package.sh"; shift; package_locked_entry "$@"',
                     "alab-local-build", str(ROOT), commit, "--out", str(out),
                     "--keep", str(keep), "--no-promote"], check=False)
        result = build(source)
        if result.returncode:
            raise RuntimeError(f"package build failed (exit {result.returncode}); selection kept")
        candidate = published_source(root, source)
        if candidate is None:
            raise RuntimeError("the successful build published no valid exact-source artifact")
    report("selecting")
    return PROMOTE.promote(repo, root, prefix, candidate, source, aterm=str(aterm), run=run,
                           expected_checkout=selected)


def parser():
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--repo", type=Path, default=ROOT)
    result.add_argument("--out", type=Path,
                        default=Path.home() / ".local/share/aterm/dev-builds/artifacts")
    result.add_argument("--prefix", type=Path, default=Path(os.environ.get(
        "ATPKG_BIN", str(Path.home() / "Library/Application Support/aterm/pkg/bin"))).parent)
    result.add_argument("--aterm", default=os.environ.get("ALAB_ATERM"))
    result.add_argument("--keep", type=int, default=3)
    result.add_argument("--interval", type=int, default=900)
    result.add_argument("--launchd-plist", action="store_true")
    result.add_argument("--status", action="store_true")
    result.add_argument("--fixed-source", help="run this exact source with its own cache guardian")
    result.add_argument("--expected-checkout", help="original verbatim selected checkout")
    result.add_argument("--expected-prefix", help="original resolved selection prefix")
    # Observations only: neither value grants selection custody.
    result.add_argument("--initial-selected-source", help=argparse.SUPPRESS)
    result.add_argument("--initial-selected-version", help=argparse.SUPPRESS)
    return result


def cache_busy(cache):
    """A no-create hint only; the fetched guardian must claim the lock for itself."""
    cache = Path(cache)
    if not cache.is_absolute():
        raise ValueError("ALAB_PACKAGE_CACHE must be an absolute path")
    cache = cache.resolve()
    for home in (Path.home().resolve(), Path(pwd.getpwuid(os.getuid()).pw_dir).resolve()):
        if cache == home or cache in home.parents:
            raise ValueError("the package cache must not be an account home or above it")
    try:
        descriptor = os.open(cache / ".alab-package.lockf",
                             os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    except FileNotFoundError:
        return False
    try:
        if not stat.S_ISREG(os.fstat(descriptor).st_mode):
            raise ValueError("the package cache lock is not a regular file")
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return True
        return False
    finally:
        os.close(descriptor)


def common_dir(repo):
    result = subprocess.run(
        ["git", "-C", str(repo), "rev-parse", "--path-format=absolute", "--git-common-dir"],
        capture_output=True, timeout=60,
    )
    if result.returncode or not result.stdout.endswith(b"\n"):
        raise RuntimeError("cannot locate the clone's common directory")
    # Only the final newline frames Git's output; other whitespace belongs to the path.
    # Binary output also keeps carriage returns that text mode would translate.
    directory = Path(os.fsdecode(result.stdout[:-1]))
    if not directory.is_absolute() or not directory.is_dir():
        raise RuntimeError("the clone's common directory is unavailable")
    return directory


def check_controller_disk(repo, out):
    """Refuse known resource shortages before allocating a retained source tree."""
    common = common_dir(repo)
    out = Path(out).absolute()
    home = Path.home()
    # Match lib.sh's build-disk paths, including a separately routed artifact child.
    # This read-only preflight grants no lock or reservation; the fetched guardian
    # still checks them before launch and throughout its cache-owning transaction.
    paths = [os.environ.get("ALAB_PACKAGE_CACHE", str(home / ".cache/codex-alab")),
             out, out / "codex", home / "Developer", common,
             os.environ.get("ALAB_CARGO_HOME") or common / "alab-cargo-home",
             "/tmp", os.environ.get("TMPDIR") or "/tmp"]
    # Bootstrap helpers can be stale or locally edited; do not execute them just
    # to measure space before handing off to the fetched source's own guardian.
    for requested in paths:
        path = Path(requested)
        if not path.is_absolute():
            raise ValueError("disk guard paths must be absolute")
        path = path.resolve()
        while True:
            try:
                space = os.statvfs(path)
                break
            except FileNotFoundError:
                if path == path.parent:
                    raise
                path = path.parent
        free_gib = space.f_bavail * space.f_frsize / 2**30
        if free_gib < 20:
            raise RuntimeError(f"disk: {free_gib:.2f} GiB free for {requested}; "
                               "20 GiB controller floor; not allocating source")


@contextmanager
def controller_checkout(repo, source, *, handoff_complete=None):
    """Own one regenerable source tree, preserving any unexpected output or work."""
    controller = Path(tempfile.mkdtemp(prefix="alab-local-controller-", dir=common_dir(repo)))
    registered = False
    try:
        git(repo, "worktree", "add", "--quiet", "--detach", str(controller), source)
        registered = True
        if git(controller, "rev-parse", "HEAD") != source or git(
                controller, "status", "--porcelain=v1", "--untracked-files=all", "--ignored"):
            raise RuntimeError("the fresh controller worktree is not exact and clean")
        yield controller
    finally:
        try:
            if handoff_complete is not None and handoff_complete() is not True:
                raise RuntimeError("the public handoff did not confirm successful completion")
            if registered and (git(controller, "rev-parse", "HEAD") != source or git(
                    controller, "status", "--porcelain=v1", "--untracked-files=all", "--ignored")):
                raise RuntimeError("unexpected controller work or output")
            # Registered controllers use their exact-source retention policy too.
            # No freshly fetched code is trusted after unexpected edits or output.
            policy_root = controller if registered else ROOT
            allowed = runpy.run_path(str(policy_root / "alab/retention.py"))["cleanup_allowed"](repo) is True
        except Exception as error:
            allowed = False
            print(f"alab: cannot establish controller cleanup policy ({error}); preserving", file=sys.stderr)
        if not allowed:
            print(f"alab: kept controller source {controller}: cleanup policy preserves worktrees",
                  file=sys.stderr)
        elif registered:
            try:
                # Even without a preservation policy, unexpected output or work
                # makes this source tree one to retain rather than discard.
                if git(controller, "rev-parse", "HEAD") != source or git(
                        controller, "status", "--porcelain=v1", "--untracked-files=all",
                        "--ignored"):
                    raise RuntimeError("unexpected work or output")
                git(repo, "worktree", "remove", str(controller))
            except (OSError, RuntimeError, subprocess.SubprocessError) as error:
                print(f"alab: kept controller source {controller}: {error}", file=sys.stderr)
        else:
            try:
                if controller.is_dir() and not any(controller.iterdir()):
                    controller.rmdir()
            except OSError as error:
                print(f"alab: kept controller source {controller}: {error}", file=sys.stderr)


def launchd_plist(args):
    """A concrete job bound to the supplied host; printing never installs or starts it."""
    if not args.aterm or not Path(args.aterm).is_absolute():
        raise RuntimeError("--aterm/ALAB_ATERM must name the durable absolute aterm executable")
    if args.interval <= 0 or args.keep <= 0:
        raise ValueError("interval and keep must be positive")
    PROMOTE.require_selector(args.aterm)
    repo = args.repo.resolve(strict=True)
    prefix = args.prefix.resolve(strict=True)
    python = str(Path(sys.executable).absolute())
    cache = str(Path(os.environ.get("ALAB_PACKAGE_CACHE",
                                   str(Path.home() / ".cache/codex-alab"))).absolute())
    environment = {"ALAB_ATERM": args.aterm,
                   "ATPKG_BIN": str(prefix / "bin"), "ALAB_PACKAGE_CACHE": cache,
                   "HOME": str(Path.home()), "PATH": os.environ.get("PATH", "/usr/bin:/bin")}
    for name in ("CARGO_BUILD_JOBS", "RUST_TEST_THREADS", "ALAB_CONTAIN", "ALAB_CARGO_HOME",
                 "ALAB_CARGO", "ALAB_ATPKG_PUBLISH"):
        if name in os.environ:
            environment[name] = os.environ[name]
    builder = os.environ.get("ALAB_PYTHON")
    if builder:
        executable = shutil.which(builder)
        if executable is None or subprocess.run(
                [executable, "-I", "-c", 'import sys; sys.exit(sys.version_info < (3, 11))'],
                capture_output=True, timeout=60).returncode:
            raise ValueError("ALAB_PYTHON must name Python >=3.11; leave it unset for discovery")
        environment["ALAB_PYTHON"] = str(Path(executable).absolute())
    elif sys.version_info >= (3, 11):
        # Keep a capable controller even off PATH. Older controllers leave the
        # builder unset so package.sh can use its existing modern-Python discovery.
        environment["ALAB_PYTHON"] = python
    log = str(common_dir(repo) / "alab-local-update.launchd.log")
    return plistlib.dumps({
        "Label": LABEL, "ProgramArguments": [python, "-I", str(repo / "alab/local_update.py"),
            "--repo", str(repo), "--out", str(args.out.absolute()), "--prefix", str(prefix),
            "--keep", str(args.keep)],
        "EnvironmentVariables": environment, "WorkingDirectory": str(repo),
        "RunAtLoad": True, "StartInterval": args.interval, "Nice": 10, "LowPriorityIO": True,
        "StandardOutPath": log, "StandardErrorPath": log,
    }, sort_keys=False)


def record_status(repo, code, reason, *, phase="completed", **details):
    directory = common_dir(repo)
    record = {"when": datetime.now(timezone.utc).isoformat(), "complete": code is not None,
              "phase": phase, "exit": code, "reason": reason, **details}
    with tempfile.NamedTemporaryFile(mode="w", dir=directory, prefix=".alab-local-update-",
                                     delete=False) as stream:
        json.dump(record, stream)
        path = Path(stream.name)
    try:
        path.replace(directory / "alab-local-update.status.json")
    finally:
        path.unlink(missing_ok=True)


def channel_context(out, details):
    """An optional display destination, admitted only by the selected package check."""
    root = (Path(out).absolute() / "codex").resolve(strict=True)
    selected = Path(details["initial_selected_checkout"])
    source, version = PROMOTE.package(selected, root)
    label = version.text
    name = selected.name
    if (not selected.is_absolute() or len(name) > 255 or len(label) > 128
            or name in (".", "..")
            or any(ord(c) < 32 or ord(c) == 127 or c in "/\\" for c in name)
            or source != details["initial_selected_source"]
            or label != details["initial_selected_version"]):
        raise ValueError("the display context does not match the selected package")
    return root, {"initial_package": name, "initial_source": source,
                  "initial_version": label}


def record_channel_status(context, code, phase, last_phase, details):
    """Best-effort, atomic, nonsecret observation; never update or selection authority."""
    if context is None:
        return False
    path = None
    try:
        root, initial = context
        if (last_phase not in WORK_PHASES
                or (code is None and phase != last_phase)
                or (code is not None and (phase != "completed" or type(code) is not int
                                          or not 0 <= code <= 255))):
            return False
        record = {"schema": 1, "when": datetime.now(timezone.utc).isoformat(),
                  "phase": phase, "last_phase": last_phase, "complete": code is not None,
                  "exit": code, "initial_package": initial["initial_package"],
                  "initial_source": initial["initial_source"],
                  "initial_version": initial["initial_version"]}
        target = details.get("target_source")
        if target is not None:
            if not isinstance(target, str) or not PROMOTE.SOURCE.fullmatch(target):
                return False
            record["target_source"] = target
        # Deliberately omit reason, controller paths, argv, environment and logs.
        data = (json.dumps(record, ensure_ascii=True) + "\n").encode("utf-8")
        if len(data) > 64 * 1024:
            return False
        with tempfile.NamedTemporaryFile(mode="wb", dir=root,
                                         prefix=".alab-local-update-display-",
                                         delete=False) as stream:
            path = Path(stream.name)
            stream.write(data)
        path.replace(root / CHANNEL_STATUS)
        return True
    except Exception:
        # The existing journal and transaction retain their original failure semantics.
        return False
    finally:
        if path is not None:
            try:
                path.unlink(missing_ok=True)
            except OSError:
                pass


def run_locked(argv, *, source=None, expected_checkout=None, expected_prefix=None,
               initial_details=None, guardian_contract=False):
    """Dispatch inspection, or the fixed-source guardian's private transaction."""
    args = parser().parse_args(argv)
    code = 0
    # These copied observations are journal data only. The explicit checkout and
    # prefix arguments carry selection custody; status never grants authority.
    details = dict(initial_details or {})
    details.update({"controller_source": source, "controller_path": str(ROOT),
                    "controller_python": sys.executable} if source is not None else {})
    if expected_checkout is not None:
        details["initial_selected_checkout"] = expected_checkout
    handed_off = None
    display = None
    last_phase = "checking"

    def progress(phase, **observed):
        nonlocal display, last_phase
        details.update(observed)
        last_phase = phase
        # This is a journal snapshot, not process liveness or selection authority.
        # A killed updater leaves an explicitly incomplete last report behind.
        record_status(args.repo, None,
                      f"last reported {phase}; completion not recorded; current activity unverified",
                      phase=phase, **details)
        # The bootstrap has only a managed-link snapshot; channel_context itself
        # requires full package admission. The fresh controller supplies that context.
        if phase == "fetching":
            try:
                display = channel_context(args.out, details)
            except Exception:
                display = None
        record_channel_status(display, None, phase, last_phase, details)

    def dispatch(commit, checkout, prefix):
        nonlocal handed_off
        check_controller_disk(args.repo, args.out)
        with controller_checkout(args.repo, commit,
                                 handoff_complete=lambda: handed_off == 0) as controller:
            # A stable public mode chooses its own same-version guardian/private
            # entry. The dispatcher never passes a private guardian ABI forward.
            # Keep this source tree until that entry and guardian have both exited.
            observations = []
            for name in ("source", "version"):
                value = details.get("initial_selected_" + name)
                if value is not None:
                    observations.append("--initial-selected-" + name + "=" + value)
            done = subprocess.run([
                sys.executable, "-I", "-B", str(controller / "alab/local_update.py"),
                *argv, "--repo", str(args.repo.resolve(strict=True)),
                "--fixed-source", commit, "--expected-checkout", checkout,
                "--expected-prefix", prefix, *observations], check=False)
            handed_off = done.returncode
        return False, "controller handoff completed"

    try:
        progress("checking")
        if source is not None and guardian_contract != GUARDIAN_CONTRACT:
            raise RuntimeError("controller handoff requires the cache-owning guardian; refresh the bootstrap")
        _, reason = update_once(args.repo, args.out, args.prefix, args.aterm, keep=args.keep,
                                progress=progress, source=source,
                                dispatch=dispatch if source is None else None,
                                expected_checkout=expected_checkout, expected_prefix=expected_prefix)
        if handed_off is not None:
            return handed_off
    except (OSError, ValueError, TypeError, RuntimeError, ImportError, SyntaxError,
            subprocess.SubprocessError) as error:
        code, reason = 1, str(error)
    print(f"alab: local update: {reason}", flush=True)
    record_status(args.repo, code, reason, **details)
    record_channel_status(display, code, "completed", last_phase, details)
    return code


def fixed_source_args(argv):
    """Validate public fixed-source scope; journal observations grant no authority."""
    args = parser().parse_args(argv)
    if not args.fixed_source or not PROMOTE.SOURCE.fullmatch(args.fixed_source):
        raise ValueError("--fixed-source must name an exact source commit")
    if not all(isinstance(value, str) and Path(value).is_absolute()
               for value in (args.expected_checkout, args.expected_prefix)):
        raise ValueError("fixed source requires the original absolute checkout and prefix")
    if git(ROOT, "rev-parse", "HEAD") != args.fixed_source or git(
            ROOT, "status", "--porcelain=v1", "--untracked-files=all", "--ignored"):
        raise RuntimeError("the fixed-source controller is not exact and clean")
    return args


def run_fixed_locked(argv):
    """Private same-version join, invoked only by this source's public guardian."""
    args = fixed_source_args(argv)
    details = {key: value for key, value in (
        ("initial_selected_source", args.initial_selected_source),
        ("initial_selected_version", args.initial_selected_version)) if value is not None}
    return run_locked(argv, source=args.fixed_source,
                      expected_checkout=args.expected_checkout, expected_prefix=args.expected_prefix,
                      initial_details=details, guardian_contract=GUARDIAN_CONTRACT)


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    args = parser().parse_args(argv)
    try:
        if args.status:
            path = common_dir(args.repo) / "alab-local-update.status.json"
            try:
                status = PROMOTE.regular_bytes(path).decode("utf-8")
            except FileNotFoundError:
                status = "no local update run recorded yet"
            print(status)
            return 0
        if args.launchd_plist:
            sys.stdout.buffer.write(launchd_plist(args))
            return 0
        if args.keep <= 0:
            raise ValueError("keep must be positive")
        cache = os.environ.get("ALAB_PACKAGE_CACHE", str(Path.home() / ".cache/codex-alab"))
        if args.fixed_source is None:
            if any(value is not None for value in (args.expected_checkout, args.expected_prefix,
                    args.initial_selected_source, args.initial_selected_version)):
                raise ValueError("selection custody arguments require --fixed-source")
            if cache_busy(cache):
                print("alab: local update: another packager owns the cache; next poll will look again",
                      file=sys.stderr)
                return 75
            return run_locked(argv)
        fixed_source_args(argv)
        # Like package_entry: the guardian owns the lock through child-group cleanup.
        # There is no environment flag that lets a public invocation skip the lock.
        done = subprocess.run([
            "/bin/bash", "-c", '. "$1/alab/lib.sh"; shift; cache="$1"; shift; '
            'output="$1"; repo="$2"; shift 2; '
            'alab_package_cache_locked "$cache" --build-disk "$output" "$repo" "$@"',
            "alab-local-update", str(ROOT), cache, str(args.out.absolute()), str(args.repo.absolute()),
            sys.executable, "-I", "-B", "-c",
            'import runpy,sys; ns=runpy.run_path(sys.argv[1]); '
            'sys.exit(ns["run_fixed_locked"](sys.argv[2:]))',
            str(Path(__file__).resolve()), *argv],
            check=False)
        if done.returncode == 75:
            print("alab: local update: another packager owns the cache; next poll will look again",
                  file=sys.stderr)
        return done.returncode
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"alab: local update: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())

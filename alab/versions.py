#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""codex-alab's version ledger, alab/versions.toml (alab/README.md, Decision 8).

One row per codex-alab version recorded for release, mapping it to the upstream Codex
baseline it is built on: the newest upstream stable rust-vX.Y.Z whose branch point the
release's source has merged. Many versions may share one baseline. The ledger is append-only:
a row once recorded never changes and is never removed, rows run in strictly increasing
version order, and a baseline never moves backward.

The file is TOML, read here by a strict line grammar (no TOML library: this runs under the
macOS /usr/bin/python3, 3.9, as package.sh's other version steps do) that refuses anything
it does not name:

    [[release]]
    version = "0.1.0"
    baseline = "0.164.0"
    baseline_commit = "<the 40-hex commit rust-v0.164.0 names>"
    date = "2026-10-24"

Blank lines and lines starting with '#' are free anywhere between rows; a row is exactly those
four lines, in that order, after its header.

Commands (each reads git objects only, never the working tree; exit 1 with the reason on
stderr when it refuses):

    versions.py parse <file>                         validate a ledger file, print its rows
    versions.py check --repo R --commit C --version V --baseline B --baseline-commit H
                      --main REF --released URL       the release gate: print V+codex.B
    versions.py row   --repo R --commit C --version V --baseline B --baseline-commit H
                      --date D --main REF --released URL
                                                      print the row a release of C appends
    versions.py baseline --repo R --commit C --version V
                                                      print the baseline C's ledger records for V

`check` and `row` read the ledger's whole history and refuse it unless git holds all of it: a
shallow clone, a grafts file or a replacement object hides or alters history, so the gate
refuses the first two and reads around the third (--no-replace-objects). The history is that
of C and of REF (origin/main, which package.sh has just fetched fast-forward only), and REF must
resolve, and every commit this clone's reflog says REF once named must still be in its
history (a rewritten main is never checked along). URL is the public release repository: each release it carries (main and every
v<X.Y.Z> tag) is a snapshot whose message names the dev commit it was cut from, which must be
on REF, with the snapshot's ledger byte for byte that commit's. A rewritten origin/main that
dropped a released commit is refused there, however its own history reads. Along that history
(alab/README.md, Decision 8):
  - a row once recorded never changes, and every recorded row is still in C's ledger;
  - the ledger is append-only: a row first recorded at a commit is above every version that
    commit's ancestors ever recorded (whatever they later removed), names the VERSION that
    commit carries, and names a baseline whose branch point that commit has merged;
  - every row's baseline_commit is the commit refs/upstream-tags/rust-v<baseline> names;
  - `check`: C is on REF, V's row is C's last, on the baseline B C is built on.
"""

import argparse
import datetime
import importlib.util
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile


# -I deliberately excludes the script directory from sys.path. Load the
# adjacent ALab-owned adapter explicitly, never a private publication module.
_GITHUB_SPEC = importlib.util.spec_from_file_location("alab_github", Path(__file__).with_name("github.py"))
GITHUB = importlib.util.module_from_spec(_GITHUB_SPEC)
_GITHUB_SPEC.loader.exec_module(GITHUB)


LEDGER = "alab/versions.toml"
NUMBER = r"(?:0|[1-9][0-9]{0,8})"
VERSION = re.compile(rf"({NUMBER})\.({NUMBER})\.0")
BASELINE = re.compile(rf"({NUMBER})\.({NUMBER})\.({NUMBER})")
COMMIT = re.compile(r"[0-9a-f]{40}")
DATE = re.compile(r"[0-9]{4}-[0-9]{2}-[0-9]{2}")
FIELDS = ("version", "baseline", "baseline_commit", "date")
FIELD = re.compile(r'([a-z_]+) = "([^"\\]*)"')
LIMIT = 1024 * 1024
UPSTREAM_MAIN = "refs/remotes/upstream/main"
UPSTREAM_TAGS = "refs/upstream-tags/"
STABLE_TAG = re.compile(rf"refs/upstream-tags/rust-v({NUMBER})\.({NUMBER})\.({NUMBER})")
# What the publication engine's mirror-head promote names a snapshot (~/publication bin/pub,
# cmd_mirror_head): "snapshot: <source commit date> from codex-alab@<dev origin/main>".
SNAPSHOT = re.compile(r"snapshot: [0-9]{4}-[0-9]{2}-[0-9]{2} from codex-alab@([0-9a-f]{40})\n")
RELEASED_REF = re.compile(rf"refs/heads/main|refs/tags/v{NUMBER}\.{NUMBER}\.{NUMBER}")


class Refused(Exception):
    """The ledger, or what is asked of it, does not hold."""


def version_key(text):
    match = VERSION.fullmatch(text)
    if not match:
        raise Refused(f"version {text!r} is not MAJOR.MINOR.0 (codex-alab's own VERSION)")
    return int(match[1]), int(match[2])


def baseline_key(text):
    match = BASELINE.fullmatch(text)
    if not match:
        raise Refused(f"baseline {text!r} is not an upstream stable X.Y.Z")
    return int(match[1]), int(match[2]), int(match[3])


def check_date(text):
    if not DATE.fullmatch(text):
        raise Refused(f"date {text!r} is not YYYY-MM-DD")
    try:
        datetime.date.fromisoformat(text)
    except ValueError:
        raise Refused(f"date {text!r} is no calendar day") from None


def check_row(row):
    version_key(row["version"])
    baseline_key(row["baseline"])
    if not COMMIT.fullmatch(row["baseline_commit"]):
        raise Refused(f"baseline_commit {row['baseline_commit']!r} is not a full commit sha")
    check_date(row["date"])


def parse(text, where=LEDGER):
    """The rows of one ledger text, in order, each a dict of FIELDS; Refused otherwise."""
    rows = []
    lines = text.split("\n")
    if text and not text.endswith("\n"):
        raise Refused(f"{where}: the last line has no newline")
    if lines and lines[-1] == "":
        lines.pop()
    i = 0
    while i < len(lines):
        line = lines[i]
        if line == "" or line.startswith("#"):
            i += 1
            continue
        if line != "[[release]]":
            raise Refused(f"{where}:{i + 1}: {line!r} is not a row header ([[release]]), a "
                          "comment or a blank line")
        row = {}
        for field in FIELDS:
            i += 1
            if i >= len(lines):
                raise Refused(f"{where}: the row ends before its {field}")
            match = FIELD.fullmatch(lines[i])
            if not match or match[1] != field:
                raise Refused(f"{where}:{i + 1}: {lines[i]!r} is not '{field} = \"...\"' (a row "
                              f"is exactly {', '.join(FIELDS)}, in that order)")
            row[field] = match[2]
        try:
            check_row(row)
        except Refused as error:
            raise Refused(f"{where}: the row for {row['version']!r}: {error}") from None
        rows.append(row)
        i += 1
    for before, after in zip(rows, rows[1:]):
        if version_key(after["version"]) <= version_key(before["version"]):
            raise Refused(f"{where}: {after['version']} follows {before['version']}: rows run in "
                          "strictly increasing version order, one row per version")
        if baseline_key(after["baseline"]) < baseline_key(before["baseline"]):
            raise Refused(f"{where}: {after['version']}'s baseline {after['baseline']} is older "
                          f"than {before['version']}'s {before['baseline']}: a baseline never "
                          "moves backward (upstream is only ever merged forward)")
        if after["date"] < before["date"]:
            raise Refused(f"{where}: {after['version']} is dated {after['date']}, before "
                          f"{before['version']}'s {before['date']}")
    return rows


def row_text(row):
    return "[[release]]\n" + "".join(f'{field} = "{row[field]}"\n' for field in FIELDS)


def git(repo, *args, timeout=120, stdin=b""):
    """git in `repo`, replacement objects ignored (they would let a local ref rewrite the
    history read here) and never prompting (a release repository that asks for credentials
    refuses rather than waits); (exit code, stdout bytes). A timeout refuses."""
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    env.update(GIT_TERMINAL_PROMPT="0", GIT_NO_LAZY_FETCH="1", GIT_OPTIONAL_LOCKS="0",
               GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1")
    try:
        process = subprocess.run(["git", "--no-replace-objects", "-c", "core.hooksPath=" + os.devnull,
                                  "-c", "core.fsmonitor=false", "-c", "protocol.allow=never",
                                  "-c", "protocol.file.allow=always", "-C", repo, *args],
                                 input=stdin, capture_output=True, timeout=timeout,
                                 env=env)
    except subprocess.TimeoutExpired:
        raise Refused(f"git {' '.join(args[:2])} in {repo} did not finish in {timeout}s") from None
    return process.returncode, process.stdout


def git_path(repo, name):
    code, out = git(repo, "rev-parse", "--git-path", name)
    path = out.decode().strip()
    if code != 0 or not path:
        raise Refused(f"cannot locate {repo}'s {name}")
    return path if os.path.isabs(path) else os.path.join(repo, path)


def whole_history(repo):
    """Refuse a clone whose history git cannot read whole: a shallow clone cut below a commit
    with parents (an edit before the cut reads as the original), or a grafts file (it rewires
    history). A shallow clone whose boundary commits have no parents (a published snapshot,
    one root commit) cuts nothing."""
    code, out = git(repo, "rev-parse", "--is-shallow-repository")
    if code != 0 or out.strip() not in (b"true", b"false"):
        raise Refused(f"cannot tell whether {repo} is a shallow clone")
    if out.strip() == b"true":
        try:
            with open(git_path(repo, "shallow"), "rb") as stream:
                cuts = stream.read(LIMIT).split()
        except OSError as error:
            raise Refused(f"cannot read {repo}'s shallow file: {error}") from None
        for cut in cuts:
            code, body = git(repo, "cat-file", "commit", cut.decode("ascii", "replace"))
            if code != 0:
                raise Refused(f"cannot read the shallow boundary {cut[:12]!r} in {repo}")
            if b"\nparent " in body.split(b"\n\n", 1)[0]:
                raise Refused(f"{repo} is a shallow clone, cut at {cut[:12].decode()}, whose "
                              "parents are not here: the ledger's history before the cut would "
                              "read as the original. git fetch --unshallow origin, then rerun")
    grafts = git_path(repo, "info/grafts")
    if os.path.lexists(grafts):
        raise Refused(f"{repo} has a grafts file ({grafts}): it rewires the history the ledger "
                      "is read from. Remove it (git replace --convert-graft-file), then rerun")


def ledger_at(repo, commit):
    """The ledger text at `commit`, or None when the commit has none."""
    code, out = git(repo, "cat-file", "-e", f"{commit}:{LEDGER}")
    if code != 0:
        return None
    code, out = git(repo, "show", f"{commit}:{LEDGER}")
    if code != 0:
        raise Refused(f"cannot read {LEDGER} at {commit[:12]}")
    if len(out) > LIMIT:
        raise Refused(f"{LEDGER} at {commit[:12]} is over {LIMIT} bytes")
    try:
        return out.decode("utf-8")
    except UnicodeDecodeError:
        raise Refused(f"{LEDGER} at {commit[:12]} is not UTF-8") from None


def resolve(repo, rev):
    code, out = git(repo, "rev-parse", "-q", "--verify", f"{rev}^{{commit}}")
    if code != 0:
        raise Refused(f"no commit {rev!r} in {repo}")
    return out.decode().strip()


def ancestor(repo, older, newer):
    code, _ = git(repo, "merge-base", "--is-ancestor", older, newer)
    if code not in (0, 1):
        raise Refused(f"cannot tell whether {older[:12]} is in the history of {newer[:12]}")
    return code == 0


def entry(repo, commit):
    """The ledger's tree entry at `commit` (mode, type, object), or None when it has none."""
    code, out = git(repo, "ls-tree", "-z", commit, "--", LEDGER)
    if code != 0:
        raise Refused(f"cannot read the tree of {commit[:12]} in {repo}")
    return out.split(b"\t", 1)[0].decode() if out else None


def rewound(repo, ref, main):
    """Refuse when this clone once saw `ref` (origin/main) at a commit its current value
    `main` does not hold (its reflog): origin's main was rewritten, and a row recorded only in
    the history it dropped (released or not) would read as never recorded."""
    code, out = git(repo, "reflog", "show", "--format=%H", ref, "--")
    if code != 0:
        raise Refused(f"cannot read {ref}'s reflog")
    seen = sorted(set(out.decode().split()))
    if not seen:
        return
    code, out = git(repo, "rev-list", "--stdin", "-n", "1",
                    stdin=("\n".join(seen) + f"\n^{main}\n").encode())
    if code != 0:
        raise Refused(f"cannot read the commits {ref}'s reflog names")
    if not out.strip():
        return
    lost = next((commit for commit in seen if not ancestor(repo, commit, main)), seen[0])
    raise Refused(f"this clone saw {ref} at {lost[:12]}, which {ref} ({main[:12]}) no longer "
                  "holds: origin's main was rewritten, and the ledger is never checked along a "
                  "rewritten main. A person decides (once decided, `git reflog expire "
                  f"--expire=now {ref}` forgets what this clone saw)")


def released(repo, url, main):
    """Every release the public release repository `url` carries (its main and each
    v<X.Y.Z> tag), checked against this clone: each is a snapshot of a dev commit on `main`
    whose ledger is the snapshot's. {dev commit: [public refs]}. A release whose dev commit
    is gone from `main` means origin/main was rewritten after it: refused."""
    with tempfile.TemporaryDirectory(prefix="alab-released-") as scratch:
        code, _ = git(scratch, "init", "-q", "--bare")
        if code != 0:
            raise Refused(f"cannot make a scratch repository to read {url}")
        try:
            observed = GITHUB.list_refs(scratch, url, ["refs/heads/main", "refs/tags/v*"], anonymous=True)
        except (GITHUB.Refused, OSError, subprocess.TimeoutExpired) as error:
            raise Refused(f"cannot list the releases at {url} (the public release repository): "
                          f"a release is checked against every release before it: {error}") from None
        refs = sorted(ref for ref in observed if RELEASED_REF.fullmatch(ref))
        sources = {}
        if not refs:
            return sources
        # Preserve each enumerated raw tag/commit OID. Fetches are explicitly
        # anonymous through Agh; later local tree reads cannot lazily network.
        try:
            GITHUB.fetch(scratch, url, [f"{ref}:{ref}" for ref in refs], anonymous=True,
                         snapshot=observed, snapshot_refs=True)
        except (GITHUB.Refused, OSError, subprocess.TimeoutExpired) as error:
            raise Refused(f"cannot fetch the releases at {url}: {error}") from None
        for ref in refs:
            code, out = git(scratch, "rev-parse", "-q", "--verify", f"{ref}^{{commit}}")
            if code != 0:
                raise Refused(f"{ref} at {url} names no commit")
            snapshot = out.decode().strip()
            code, out = git(scratch, "cat-file", "commit", snapshot)
            message = out.split(b"\n\n", 1)[1] if code == 0 and b"\n\n" in out else b""
            match = SNAPSHOT.fullmatch(message.decode("utf-8", "replace"))
            if not match:
                raise Refused(f"{ref} at {url} is {snapshot[:12]}, which does not name the dev "
                              "commit it was cut from ('snapshot: <date> from codex-alab@<commit>', "
                              "the publication engine's mirror-head form): the gate cannot tell "
                              "what was released. A person decides")
            source = match[1]
            code, _ = git(repo, "cat-file", "-e", f"{source}^{{commit}}")
            if code != 0 or not ancestor(repo, source, main):
                raise Refused(f"{ref} at {url} was released from {source[:12]}, which is not in "
                              f"the history of origin/main ({main[:12]}): origin's main was "
                              "rewritten after that release (the ledger it released can no longer "
                              "be read along main), or this clone is stale (git fetch origin). "
                              "A rewritten main is never released from: a person decides")
            if entry(scratch, snapshot) != entry(repo, source):
                raise Refused(f"{ref} at {url} carries a {LEDGER} other than its dev commit "
                              f"{source[:12]}'s: what was released is not what main recorded")
            sources.setdefault(source, []).append(ref)
    return sources


def history(repo, heads):
    """The ledger's history reachable from `heads`, as git simplifies it to the commits where
    the ledger changed while keeping the ancestry between them (--full-history
    --simplify-merges): [(commit, parents, rows or None)], parents first. A historical ledger
    that does not parse is refused: its rows cannot be told apart from a change."""
    code, out = git(repo, "rev-list", "--full-history", "--simplify-merges", "--parents",
                    "--topo-order", "--reverse", *heads, "--", LEDGER)
    if code != 0:
        raise Refused(f"cannot read the history of {LEDGER} from {', '.join(heads)}")
    walk = []
    for line in out.decode().splitlines():
        commit, *parents = line.split()
        text = ledger_at(repo, commit)
        rows = parse(text, f"{LEDGER} at {commit[:12]}") if text is not None else None
        walk.append((commit, parents, rows))
    return walk


def recorded(walk):
    """Every row any version of the ledger in `walk` ever held: {version: (row, commit that
    held it)}. A version that ever read two ways is refused for good."""
    seen = {}
    for commit, _, rows in walk:
        for row in rows or []:
            held = seen.get(row["version"])
            if held is not None and held[0] != row:
                raise Refused(
                    f"{LEDGER}: the row for {row['version']} reads {held[0]} at {held[1][:12]} "
                    f"but {row} at {commit[:12]}: a row once recorded never changes")
            seen.setdefault(row["version"], (row, commit))
    return seen


class Upstream:
    """Upstream's stable tags and the branch points they name, read once per run."""

    def __init__(self, repo):
        self.repo = repo
        self.points = {}

    def tag_commit(self, baseline):
        code, out = git(self.repo, "rev-parse", "-q", "--verify",
                        f"{UPSTREAM_TAGS}rust-v{baseline}^{{commit}}")
        return out.decode().strip() if code == 0 else None

    def branch_point(self, baseline):
        if baseline not in self.points:
            code, out = git(self.repo, "merge-base", f"{UPSTREAM_TAGS}rust-v{baseline}",
                            UPSTREAM_MAIN)
            self.points[baseline] = out.decode().strip() if code == 0 else None
        return self.points[baseline]


def version_at(repo, commit):
    code, out = git(repo, "show", f"{commit}:VERSION")
    return "".join(out.decode("utf-8", "replace").split()) if code == 0 else None


def append_only(repo, walk, upstream):
    """Along `walk`, each commit's rows that one of its parents never knew (that parent's
    ledger or any before it, whatever was later removed) are above every version that parent
    knew: a row is only ever appended. A row no parent knew is new at that commit: it names
    the commit's own VERSION (what `package.sh --ledger-row` prints there) and a baseline whose
    branch point the commit has merged."""
    known = {}
    for commit, parents, rows in walk:
        rows = rows or []
        for parent in parents:
            if parent not in known:
                raise Refused(f"cannot read {LEDGER}'s history in order at {commit[:12]}")
        everything = set().union(*(known[parent] for parent in parents))
        for parent in parents:
            before = known[parent]
            if not before:
                continue
            top = max(before, key=version_key)
            for row in rows:
                if row["version"] not in before and version_key(row["version"]) < version_key(top):
                    raise Refused(
                        f"{LEDGER} at {commit[:12]} records {row['version']}, which its parent "
                        f"{parent[:12]} never recorded, below {top}, which it had: the ledger is "
                        "append-only, a row is only ever added above every row recorded before "
                        "it")
        for row in rows:
            if row["version"] in everything:
                continue
            carried = version_at(repo, commit)
            if carried != row["version"]:
                raise Refused(
                    f"{LEDGER} at {commit[:12]} first records {row['version']}, but that commit's "
                    f"VERSION is {carried or 'missing'}: a row is recorded from a commit that "
                    f"carries its version (alab/package.sh --ledger-row >> {LEDGER})")
            point = upstream.branch_point(row["baseline"])
            if point is None or not ancestor(repo, point, commit):
                raise Refused(
                    f"{LEDGER} at {commit[:12]} first records {row['version']} on the upstream "
                    f"baseline {row['baseline']}, whose branch point "
                    f"{'is unknown here' if point is None else point[:9]} that commit has not "
                    "merged: a version is built on an upstream release its source holds")
        known[commit] = everything | {row["version"] for row in rows}


def checked(args, appending):
    """C's ledger, after the history check; with V's row present (release) or absent (row)."""
    repo = args.repo
    whole_history(repo)
    commit = resolve(repo, args.commit)
    version_key(args.version)
    baseline_key(args.baseline)
    if not COMMIT.fullmatch(args.baseline_commit):
        raise Refused(f"baseline commit {args.baseline_commit!r} is not a full commit sha")
    text = ledger_at(repo, commit)
    if text is None:
        raise Refused(f"{commit[:9]} has no {LEDGER}: a release of codex-alab's own line is "
                      "recorded there (alab/README.md, Decision 8)")
    rows = parse(text, f"{LEDGER} at {commit[:12]}")
    code, out = git(repo, "rev-parse", "-q", "--verify", f"{args.main}^{{commit}}")
    if code != 0:
        raise Refused(f"no {args.main} here: the ledger is checked along origin/main as origin "
                      "has it now (git fetch origin), never along this commit's history alone")
    main = out.decode().strip()
    rewound(repo, args.main, main)
    if not appending and not ancestor(repo, commit, main):
        raise Refused(f"{commit[:9]} is not on origin/main ({main[:9]}): a release is cut from "
                      "origin/main (alab/README.md, Decision 4). Push it, or git fetch origin")
    released(repo, args.released, main)
    walk = history(repo, [commit, main])
    by_version = {row["version"]: row for row in rows}
    for version, (row, at) in sorted(recorded(walk).items(),
                                     key=lambda item: version_key(item[0])):
        if by_version.get(version) != row:
            now = by_version.get(version)
            what = f"reads {now}" if now else "is gone"
            raise Refused(f"{LEDGER} at {commit[:9]}: the row for {version}, recorded at "
                          f"{at[:12]} as {row}, {what}: a row once recorded never changes and is "
                          "never removed (an origin/main that recorded a later row is released "
                          "from, never an older commit)")
    mine = by_version.get(args.version)
    last = rows[-1] if rows else None
    if appending:
        if mine is not None:
            raise Refused(f"{args.version} already has its row at {commit[:9]}: {mine}")
        if last is not None:
            if version_key(args.version) <= version_key(last["version"]):
                raise Refused(f"{args.version} is not above the last recorded version "
                              f"{last['version']}: `pub bump codex-alab` first")
            if baseline_key(args.baseline) < baseline_key(last["baseline"]):
                raise Refused(f"the baseline {args.baseline} is older than {last['version']}'s "
                              f"{last['baseline']}: a baseline never moves backward")
    else:
        if mine is None:
            raise Refused(f"{LEDGER} at {commit[:9]} has no row for {args.version}: append it in "
                          f"the commit released (alab/package.sh --ledger-row >> {LEDGER}), push, "
                          "then release")
        if mine is not last:
            raise Refused(f"{args.version} is not the newest recorded version at {commit[:9]} "
                          f"({last['version']} is): a release is above every version recorded "
                          "before it")
        frozen = False
        if mine["baseline"] != args.baseline:
            # Tags can appear after a source was frozen: a patch of the recorded line, or
            # the next minor's stable. A newly visible stable does not change a recorded
            # baseline when its branch point was already integrated in EVERY historical
            # carrier of that row: the claim already held that upstream code, and only its
            # name arrived later. A later source merge that brought the branch point in, an
            # older or unknown stable, or a moved tag still needs a new product version.
            # Recorded tag identities are checked below.
            upstream = Upstream(repo)
            before, observed = baseline_key(mine["baseline"]), baseline_key(args.baseline)
            point = upstream.branch_point(args.baseline)
            carriers = [at for at, _, past in walk
                        if any(row["version"] == args.version for row in past or [])]
            frozen = (before < observed
                      and upstream.tag_commit(args.baseline) == args.baseline_commit
                      and point is not None and bool(carriers)
                      and all(ancestor(repo, point, at) for at in carriers))
        if mine["baseline"] != args.baseline and not frozen:
            raise Refused(f"{args.version} is recorded on the upstream baseline "
                          f"{mine['baseline']}, but {commit[:9]} is built on {args.baseline}: that "
                          "mapping never changes. `pub bump codex-alab` and append the new "
                          "version's row instead")
        if not frozen and mine["baseline_commit"] != args.baseline_commit:
            raise Refused(f"{args.version}'s row names rust-v{mine['baseline']} at "
                          f"{mine['baseline_commit'][:12]}, but that tag names "
                          f"{args.baseline_commit[:12]} here")
    upstream = Upstream(repo)
    for row in rows:
        named = upstream.tag_commit(row["baseline"])
        if named != row["baseline_commit"]:
            raise Refused(f"{LEDGER} at {commit[:9]}: the row for {row['version']} names "
                          f"rust-v{row['baseline']} at {row['baseline_commit'][:12]}, but "
                          f"{'no such tag is here' if named is None else 'that tag names ' + named[:12]}"
                          f" ({UPSTREAM_TAGS}): every row names the commit its baseline's tag "
                          "names. If upstream moved or deleted the tag, a person decides")
    append_only(repo, walk, upstream)
    return rows


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    sub = parser.add_subparsers(dest="command", required=True)
    one = sub.add_parser("parse")
    one.add_argument("file")
    for name in ("check", "row"):
        command = sub.add_parser(name)
        for flag in ("--repo", "--commit", "--version", "--baseline", "--baseline-commit",
                     "--main", "--released"):
            command.add_argument(flag, required=True)
        if name == "row":
            command.add_argument("--date", required=True)
    base = sub.add_parser("baseline")
    for flag in ("--repo", "--commit", "--version"):
        base.add_argument(flag, required=True)
    args = parser.parse_args(argv)
    try:
        if args.command == "parse":
            with open(args.file, "rb") as stream:
                data = stream.read(LIMIT + 1)
            if len(data) > LIMIT:
                raise Refused(f"{args.file} is over {LIMIT} bytes")
            for row in parse(data.decode("utf-8"), args.file):
                print(" ".join(f"{field}={row[field]}" for field in FIELDS))
        elif args.command == "check":
            rows = checked(args, appending=False)
            mine = next(row for row in rows if row["version"] == args.version)
            print(f"{args.version}+codex.{mine['baseline']}")
        elif args.command == "row":
            check_date(args.date)
            rows = checked(args, appending=True)
            row = {"version": args.version, "baseline": args.baseline,
                   "baseline_commit": args.baseline_commit, "date": args.date}
            if rows and args.date < rows[-1]["date"]:
                raise Refused(f"the date {args.date} is before the last row's {rows[-1]['date']}")
            sys.stdout.write("\n" + row_text(row))
        else:
            commit = resolve(args.repo, args.commit)
            version_key(args.version)
            text = ledger_at(args.repo, commit)
            rows = parse(text, f"{LEDGER} at {commit[:12]}") if text is not None else []
            mine = [row for row in rows if row["version"] == args.version]
            if not mine:
                raise Refused(f"{LEDGER} at {commit[:9]} records no baseline for {args.version}")
            print(mine[0]["baseline"])
    except (Refused, UnicodeDecodeError, OSError) as error:
        print(f"alab: versions: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

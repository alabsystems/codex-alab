<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# codex-alab: trailing upstream with a thick layer on top

codex-alab is ALab's build of OpenAI's Codex CLI. It follows
[openai/codex](https://github.com/openai/codex) daily and carries ALab's changes on top.
This directory holds the process and tooling that keep both true at once.

[Native controls](NATIVE-CONTROLS.md) describes the version, update and Autopilot UI
owned by this binary. aterm supplies terminal services; it draws no Codex overlay.

The optional interface's Ctrl+S fallback parks a full prompt and editor state per
conversation; history search and configured keys keep priority. This ALab behavior
is documented here so the upstream composer module remains byte-identical where
it needs no functional hook.

| file | what it is for |
| --- | --- |
| `sync.sh` | merge the newest upstream into `main`: run it at least daily |
| `daily.sh` | the unattended wrapper: stepwise sync, status, journal, LaunchAgent |
| `CONTRACT.md` | the native harness: what codex-alab does itself in aterm, and the wire between them |
| `check.sh` | the gate a sync passes before it lands |
| `delta.sh` | the ALab layer, measured: where the next conflict will come from |
| `package.sh` | build a codex package and advance its already selected local artifact channel |
| `local_update.py` | poll origin/main for that selected local channel; prepare its separate LaunchAgent |
| `promote.py` | conditionally select a forward local package through atpkg after smoke passes |
| `prune.py` | keep the newest packages and every one in use (linked by atpkg, running, or owed a crash relaunch) |
| `removed-paths` | upstream paths codex-alab does not carry; every sync re-applies them |
| `owned-paths` | paths ALab owns that upstream might also ship (root `AGENTS.md` and `README.md`) |
| `watched-paths` | upstream paths whose changes a sync calls out (release recipe, version logic, the vendor daemon's package rules) |
| `merge-attributes` | the lockfile merge rule, applied to sync merges only |
| `setup.sh` | make a clone sync-ready: upstream remote and refs, rerere |
| `smoke/dropin.py` | codex-alab against OpenAI's build: still a drop-in `codex` ("The drop-in check") |
| `smoke/capacity_exec.py` | the smokes' shared loopback Responses model, command-guardian runner and package reader; its capacity and quota cases run in aterm's jailed lane ("Headless capacity and quota recovery") |
| `smoke/parallel_logins.py` | concurrent real exec processes with distinct credentials and shared conversation storage; no build |
| `smoke/quota_resume.py` | quota recovery across persisted thread origins using existing binaries |
| `smoke/quota_wait.py` | repeated stale-reset backoff and native cancellation during a distant reset |
| `smoke/prune_test.py` | `prune.py` against a scratch package root and atpkg prefix |

## What upstream is like (measured 2026-10-15)

- **A one-way export.** Every upstream commit in the last 90 days carries a
  `GitOrigin-RevId` line. The export began at dc5ae3789 "Initialize Copyberry export
  baseline" (2026-07-10), an empty commit stacked on the existing history, which was not
  rewritten. History is linear: there is not one merge commit in it. 271 authors
  contributed in 90 days.
- **No pull requests.** Upstream's `docs/contributing.md`: "We do not accept external code
  contributions or pull requests." Plan to carry every ALab change for as long as ALab
  wants it.
- **Busy.** 3,981 commits in 90 days: about 44 a day on average, and up to 84 on a single
  day (Pacific dates). In the
  same window, 3,842 commits touched `codex-rs/`, and the TUI alone saw 11,973 file
  touches.
- **Releases are cut off `main`.** Each release is a branch point, plus a release commit
  that changes only `[workspace.package] version` in `codex-rs/Cargo.toml` (its message
  is the release notes; the annotated tag says `Release X`), plus up to 16 cherry-picks
  (most minors have 0 to 4). A release tag is therefore never an ancestor of
  `upstream/main`. Stable minors ship about every 5 days (19 in 90 days, plus 17 patch
  releases), with alphas on most days, often several (308 alpha tags in 90 days). A build
  from `main` says `codex-cli 0.0.0`.

## The shape

```
upstream/main  o---o---o---o---o---o---o---o---o     openai/codex; fetched, never pushed to
                \               \               \
main             S---a---b-------M1---c---d------M2  ALab; fast-forward only, never rewritten

S   the fork point: upstream's tree plus this directory
a-d ALab commits ("alab(<topic>): ...")
M   "alab sync: merge upstream openai/codex@<sha>", one per sync
```

- `upstream/main` is openai/codex `main`, fetched fast-forward only. If upstream
  rewrites its history, the fetch refuses and the sync stops for a person.
- `upstream-tags/rust-v*` are upstream's release tags. They live outside `refs/tags`,
  so they never mix with ALab's tags and never get pushed.
- The `upstream` remote's push URL is a refusal.
- `main` on `origin`, ALab's private dev repository, is the product. The public
  `alabsystems/codex-alab` is a snapshot of it that the publication engine cuts ("Publication").

## Why merge forward instead of rebasing

The goal is to keep ALab on the latest upstream. A literal daily `git rebase` is the
wrong tool for a large layer:

1. **A rebase replays every ALab commit.** An upstream file that 20 ALab commits touched
   can conflict 20 times in one day. A merge meets each upstream change once.
2. **A rebase gives every ALab commit a new sha every day.** ALab identifies a shipped
   build by its commit, so a rebased `main` would orphan the provenance of everything
   installed. A merge keeps every shipped commit reachable from `main` forever.
3. **A rebase needs a force-push.** That breaks every clone, worktree and agent branch
   built on `main`, and many agents work here at once.

What merge gives up is a linear, per-topic patch series. `delta.sh` gives back the view
that matters: what ALab changes in upstream's files, ranked by conflict risk, with
`--topics` grouping every hook by topic. To review a sync merge, use
`git log --first-parent --remerge-diff`; to bisect, use `git bisect --first-parent`.
If a clean series is ever needed for an audit, cut it as a side branch from
`git diff upstream/main...main`. It is a view, never a replacement for `main`.

rerere is on for the abort-and-redo cycle, not for repeated daily conflicts. In
merge-forward a resolved conflict rarely comes back, because the next merge's base
already contains the resolution. Where rerere earns its keep is an `--abort` followed
by a re-run, or a retry after `origin/main` moved: there it replays every hand
resolution.

## The daily sync

```sh
alab/sync.sh              # merge upstream/main into main, check, push
alab/sync.sh --stepwise   # land what merges unattended, stop at the first change that needs a person
alab/sync.sh --status     # what is pending or in progress
```

The sync never touches anyone's checkout. It works in one persistent sync worktree
(`.git/alab-sync`, or `ALAB_SYNC_DIR`), whose `codex-rs/target/` survives between runs
while the disk has room (`daily.sh` removes it otherwise, "Running unattended"), so the daily
build reuses every unchanged crate. A pristine upstream tree beside it
(`alab-pristine-<full-upstream-commit>` inside the sync worktree's git dir) is built, in its own `target/`, when a
failure has to be told apart from upstream's own (below). One sync runs at a time per
clone: a kernel lock (`lockf`, or `flock` on Linux) that is released however the run ends
(within about a second of a run killed by `kill -9`), so no lock outlives a killed run. Each
run:

1. Fetches `upstream` and `origin`, and stops if `origin/main` already contains the
   target. It also stops if upstream's history was rewritten.
2. Merges the target into `origin/main` with `--no-ff`, resolving the mechanical
   conflicts itself (below).
3. On a conflict that needs judgment, exits 3 and names each file, with the upstream
   and ALab commits that touched it.
4. Commits `alab sync: merge upstream openai/codex@<sha>`. The message records the
   upstream commit, how many upstream commits came in, the paths rerere and
   `owned-paths` resolved and those resolved by hand, how many files `removed-paths`
   dropped, whether the lockfile was re-resolved, and any upstream change to a watched
   path.
5. Runs `check.sh`, then pushes to `origin main` as a fast-forward. If `origin/main`
   moved meanwhile, the sync merges the new ALab commits in, checks again, and pushes.
   Repeated races stay in this run, with backoff from 1 s up to 60 s. A retry requires
   Git's per-ref fast-forward rejection and a successful fetch proving that main advanced
   without rewriting history and has commits the sync lacks. Authentication, transport and
   remote-hook failures still stop, as do conflicts and failed checks.

A sync that stops for a person (exit 3, 4 or 6) records why, and so do a `--no-push`
merge and a person's `--continue` that failed: each waits (exit 5) for `--continue` or
`--abort`. An unattended sync that stopped on an error instead (the network dropped during
the push, a run was killed mid-check) has no such record, and the next run resumes it: it
re-applies what the sync resolves by itself, checks again and lands it before syncing
further.

`--stepwise` is what the unattended runner uses. It probes the upstream range without
touching a worktree or the clone's index (`git merge-tree`, a binary search, a few
probes). It lands, as one merge, a prefix that
merges unattended (the longest one, unless an upstream revert hides a longer one: two
reverts in 90 days), then merges up to the first upstream commit that needs a resolver
and stops there. `main` stays current while it waits, and the resolver faces one
upstream change, with its commit message and PR, instead of a day's worth.

| exit | meaning | next step |
| --- | --- | --- |
| 0 | landed, or already current; with `--no-push`, merged and checked (and with `--no-check` too, only merged) | none (`--continue` checks and lands a `--no-push` merge) |
| 3 | conflicts need a resolver | resolve in the worktree, `git add`, `alab/sync.sh --continue` |
| 4 | `check.sh` failed, or a fix was left uncommitted | fix it in the worktree (see below), `alab/sync.sh --continue` |
| 5 | an earlier sync still waits: for its resolver, or for a `--no-push` merge to land | `alab/sync.sh --status` says why and for how long; resolve and `--continue`, or `--abort` |
| 6 | upstream itself does not build and run with Trust | a lane fault: fix it in Trust, then `--continue` |
| 1 | anything else, named on stderr (a failing git command exits with its own status, such as 128) | read it; the next unattended run resumes a sync that stopped this way (after a person's `--continue`, rerun it), and `alab/sync.sh --abort` starts over |

`--continue` lands the merge in progress only; upstream commits beyond it wait for the
next run.

### What the sync resolves by itself

The sync resolves only what is mechanical and safe:

- **rerere** replays a resolution recorded for the exact same conflict.
- **`removed-paths`**: a conflict inside a removed path resolves to the removal, and a
  file upstream adds under one is dropped.
- **`owned-paths`**: a conflict keeps ALab's side, and upstream's version is saved as
  `alab/upstream/<path>`.
- **`codex-rs/Cargo.lock`** is derived data. When both sides changed it, upstream's side
  wins (`merge-attributes`, scoped to the sync's own merges; a merge of main's concurrent
  commits keeps the sync's side instead). At commit time, `targo metadata` re-resolves
  it, which puts ALab's crates back. Before any sync lands, however its merges were
  committed, every package upstream locked must still be there at the same version,
  source and checksum; if any moved, the sync stops (exit 3) with the list. An ALab crate
  can add dependencies, enable features or add a second version of a crate, but it cannot
  move one of upstream's. ALab's own entries are re-resolved at nearly every sync, so a
  version an ALab crate needs exactly is pinned in its `Cargo.toml` (`= x.y.z`), never in
  the lockfile alone.

Anything else goes to a resolver, including conflicts in other lockfiles (ALab should
not edit those) and in the members list.

### Resolving a conflict

1. Read both intents: upstream's commit (and its PR), and ALab's.
2. Keep upstream's change, and re-apply ALab's intent on top of it. Upstream's code is
   the moving part; ALab's patch adapts to it, never the reverse.
3. If upstream now does what the ALab change did, drop ALab's change. The layer shrinks.
   If that removes an `ALAB(<topic>)` marker, `check.sh` stops on the lost hook; rerun
   with `ALAB_ALLOW_HOOK_LOSS=1` to say it was meant.
4. If the same file needed you last week too, the real fix is a refactor, not another
   re-patch (see "The occasional refactor"), done as its own commit after the sync lands.
5. `git add` the files, then run `alab/sync.sh --continue`.

When `check.sh` fails (exit 4), fix it in one of two ways:

- **A wrong resolution:** fix it inside the merge, so neither history nor rerere keeps
  the bad version. Run `git rerere forget <file>`, fix the file, then
  `git commit --amend`; the merge is not pushed yet.
- **An adaptation to an upstream API change:** make it its own commit on top.

Then `--continue`.

### The gate

`check.sh` takes no options other than `--help`, and refuses any other argument.
Codex and its tests run with a scratch `CODEX_HOME`, so nothing writes into the
`~/.codex` that holds this machine's login. The gate resolves Cargo's effective
target from locked metadata before compiling and keeps it explicit through every
phase, preserving configuration and environment precedence. Native cache ownership
therefore refuses an existing unknown target before compilation; choose a fresh
`CARGO_TARGET_DIR` when that happens, rather than adopting or deleting old outputs.
Direct runs allocate private short scratch under `/tmp/.ubd`; contained runs keep
scratch inside their admitted temporary root. Real file-event fixtures allocate
their own private directory outside `.ubd`, where macOS can deliver the events.
Its steps:

1. **No hook went missing.** Each `ALAB(<topic>):` marker in an upstream file on
   `origin/main` (or `ALAB_CHECK_BASE`, which must exist) must still be there (markers
   inside `alab/`, `AGENTS.md` and `codex-rs/alab-*` do not count).
2. **Codex builds from the committed lockfile and runs** (`codex --version`). On failure
   (`check.sh` exit 2) the sync builds and runs pristine upstream in the pristine tree:
   if that fails too, the fault is the toolchain's (exit 6), else ALab's (exit 4).
3. **Every `codex-rs/alab-*` crate passes its tests.**
   Capsule ownership checks run in an exact child with a bounded wait and a named
   helper result. A closed descriptor number can be reused immediately by another
   parallel test, so checking its absence in the shared test process is unreliable.
   Reuse the existing helper; retain all rejection, consumption and real-exec checks.
4. **`targo tippy` finds nothing in ALab's code.** It lints the `alab-*` crates and every
   upstream package with a hook, all targets (the glue in `codex-rs/alab-*/glue` compiles
   inside codex-tui, codex-core and codex-cli), in a target dir of its own,
   `<target>/tippy`: tippy's rustc fingerprint differs from the build's, so sharing one
   would rebuild every build script, and everything built on them, each time steps 2, 3 or
   5 and this one followed each other. A warning or error gates when cargo's JSON message
   (read by `alab/lint.jq`) locates it in a file under `codex-rs/alab-*` or `alab/` (the
   vendored sources included), or starts it on an upstream line that carries an `ALAB`
   marker; one anywhere else in upstream's code is upstream's and is set aside, and
   `--cap-lints warn` keeps a lint upstream denies from stopping the crates built on it.
   Three cases are judged against pristine upstream, linting the packages they are in there
   (in the pristine tree's `target/tippy`): a diagnostic whose span takes in a hook line it
   does not start on (clippy reports a method chain from its receiver, so a hook joining
   one midway is named from upstream's line), which gates unless upstream alone has as many
   of it in that file; a target that does not build with no error in ALab's code, which
   gates unless it fails to build on upstream alone too; and the build script of a linted
   package (an `alab-*` crate or a hooked one, with a `custom-build` target in `targo
   metadata`) that does not run to completion, which gates unless pristine upstream compiled
   that package's script and it did not run there either. Cargo's JSON records each build
   script that ran (`build-script-executed`, fresh or not) and says only in prose that one
   failed; none of the package's other targets is then built, so no diagnostic names it, and
   with some other target failing on upstream too, nothing else in the step would gate (step
   5 would, as a build error, and only for a package with a library). A failure that names
   no target gates. Linting Code Mode's host or runtime first resolves the same
   independently verified V8 archive and binding as packaging, in the private Cargo
   home's cache; pristine comparisons resolve their own tree's pins. The pair also stays
   available for direct host/runtime unit tests and isolated retries.
   It needs `jq` (`/usr/bin/jq` on macOS 15 and later); `python3
   alab/tests/test_lint_owned.py` checks `alab/lint.jq`'s reading of cargo's messages, and
   `python3 alab/tests/test_check_lint_step.py` runs the step itself, the pristine
   comparison included, in a scratch clone against a fake cargo, with contained cases.
   First build its native adapter from `codex-rs/` with
   `targo --unverified build --locked -p codex-alab-unit-runner --bin alab-unit-runner --message-format=json-render-diagnostics`
   and set `ALAB_TEST_UNIT_RUNNER` to the executable path Cargo reports. The fixtures
   use that actual adapter while their fake Cargo supplies test artifacts and outcomes.
   `test_unit_results.py` checks package/library/copy attribution and process custody;
   `test_unit_runner_native.py` additionally needs Python 3.11 or newer and real Trust
   Cargo, and checks inherited context before a native harness initializes Rust, plus
   existing Cargo runner configuration and a Python signal-change negative control.
   Measured contained on this Mac (2026-10-21, `alab_contained` as
   `run_check` calls it): the whole gate passed, jq, `<target>/tippy` and the step's metadata
   read included (17 upstream diagnostics set aside), and with a build-script fault planted
   in codex-cli the step linted codex-cli in the pristine tree's `target/tippy` and gated.
5. **Every upstream package ALab hooks into passes its unit tests (`--lib`), where the
   layer is to blame.** Upstream runs these under nextest, a process per test; under
   libtest some fail on pristine upstream (a stack overflow at the default stack, timing
   under load, insta's one-assertion-per-process rule). Before a core or app-server
   unit suite runs, the step builds its declared `test_stdio_server` and
   `codex-code-mode-host` runtime helpers in that tree's own target. Cargo's artifact
   paths bind the test environment, overriding inherited helper aliases. The step
   also builds and binds that tree's full `codex` executable for the MCP suite's
   sandbox-capability checks, first running that exact artifact with `--version`
   so first-launch loading is outside the capability probe's timeout. A failed
   helper build or missing/ambiguous executable is a prerequisite failure, never an
   excused test assertion. The native host uses packaging's independently verified
   upstream V8 pair, cached in the private Cargo home; resolution requires Python 3.11
   or newer (`ALAB_PYTHON` selects the interpreter). The step runs the suite with an
   8 MiB stack and `--no-fail-fast`. Cargo metadata and compiled artifacts bind each
   failure to its package/version, manifest, library target/source and exact test name.
   The step reruns each original copy alone, then what still fails on pristine upstream.
   Compilation retains the complete selected package/feature graph in every phase:
   narrowing to the failing packages can change their dependencies' enabled features.
   Runtime comparisons select one exact case per library in each Cargo round, skipping
   unrelated harnesses; helpers and artifact preparation run once per phase. The native
   adapter execs within Cargo's child without changing its PID, parent, session/group,
   environment, inherited descriptors or signal context. Python preparation/collection
   stays outside that child, and the adapter bypasses Rust startup's own signal/FD edits.
   A test gates unless its original package/library copy fails there too; a missing,
   crashed or zero-match copy cannot borrow another library's outcome. A zero-match
   diagnostic names the comparison tree, package, library and test, and says that the
   original failure remains gating. Check the original source failure when an ALab-only
   test has no counterpart in pristine upstream. Existing matching
   cfg runners conflict instead of being replaced; a target-triple runner takes precedence
   and absent adapter receipts refuse attribution. A crash,
   a build error, more than 60 failures, or a failure count that disagrees with libtest's
   own fails the step outright. Contained (below), upstream tests can fail where they pass
   uncontained (they write outside the scratch dir or bind or reach sockets elsewhere);
   the pristine reruns are contained too, so they fail there as well and
   do not gate, and the cap sits well above them. Earlier measurements with hooks in
   codex-core and codex-tui recorded 9 to 18 minutes per sync, plus the pristine tree's
   own build, and caught a planted one-line fault. Those timings describe that earlier
   gate. A hung unit test holds the sync lock;
   `daily.sh` reports it after two refused runs.

Set `ALAB_UNIT_RESULTS_DIR` to a new absolute directory to retain the three unit
phases' metadata, actual library/helper artifacts, runtime receipts, logs and round
statuses after ordinary scratch cleanup. The gate creates that directory with mode `0700`
and refuses any existing directory, file or symlink. Its parent must already exist and
be writable; a contained caller must explicitly admit that parent too. Without this
setting the evidence uses the gate's existing temporary scratch policy.

Pristine upstream is built in a tree of its own, one per checkout and resolved upstream
commit (`alab-pristine-<full-commit>` in the checkout's git dir), with its own `target/`: cargo judges a workspace crate fresh by file
times, so a target shared with the ALab tree could hand either tree the other's builds,
and a check must never move an existing pristine tree. A person who runs `check.sh` in
their own checkout keeps these trees and their targets under `.git`. A clean exact
revision is reused without Git writes; occupied dirty, mismatched, broken or symlink
revision paths are refused without repair. New upstream revisions use ordinary worktree
addition, so a preservation marker does not prevent progress. Legacy `alab-pristine`
trees remain untouched. Daily target cleanup includes the legacy path and registered,
clean revision trees, subject to its existing use checks and common preservation policy;
unregistered or dirty revision paths stay kept. Removing a linked worktree deletes its pristine tree's files
but leaves that tree's registration behind (`git worktree list` shows it `prunable`): drop it with
`git worktree remove --force --force <its path>` rather than `git worktree prune`, which would also
drop a person's worktree on a disk that is not mounted.

The sync runs `check.sh`, the pristine build that tells an ALab fault from a lane fault,
and the lock re-resolve (`targo metadata`, which reads the `.cargo/config.toml` upstream
just merged) **contained** (`lib.sh`, `alab_contained`): they build and run code upstream
landed hours ago, build scripts of newly added crates included, as the account that holds
`~/.codex/auth.json`, git and `gh` credentials, the SSH agent and the release key. Under
a macOS sandbox (`sandbox-exec`) with a scrubbed environment and a scratch `TMPDIR`, that
code cannot read files under `HOME` other than the clone, the Rust toolchains (the
sysroot PATH's `trustc` runs from among them: atpkg's shims exec wherever the toolchain is
linked, a Trust checkout's dev sysroot included, so the profile asks `trustc --print sysroot`
from `/`, where no `rust-toolchain` file applies, and refuses one at a home or above), PATH's
tools, the private cargo home (below; not its credentials), the `.cargo/config.toml` of
the directories above the build, and git's config (it can list the directories above
those, the home included, for targo's check of the toolchain path; it cannot read what
they hold); cannot read the keychain or the
clipboard, open apps, send Apple events or run `launchctl`; cannot connect to a Unix
socket but its own (not the SSH agent's, nor aterm's); and can write only the directories
its caller names (the two target dirs, for the gate), its scratch dir and the private cargo
home, so it plants no source, `.git`, `.cargo` or `rust-toolchain` file that git or cargo
read outside the sandbox. The one shared accounting exception is the account's
`Library/Caches/aterm/capacity` directory: native Rust admission reads and settles
its cooperative claims, CPU slots and lock files there, even with a scratch
`HOME`. Aliased ledger roots are refused. No neighboring aterm cache or state
directory becomes readable or writable, and explicit read-only fences still
take precedence. It keeps the network: builds fetch crates and V8.

Before those contained commands create a target, sync and packaging ask aterm's Rust cache
registrar to record their real workspace. Packaging validates its compiler-keyed location
without creating it first. Fresh package targets can also opt into aterm's disposable-cache
budget, described under "Packaging through atpkg" below. The native registrar checks the
actual compiler driver's compatibility before admitting that policy. Existing unmarked outputs remain
unmanaged and reusable, and an older host keeps the established registration flow. A marked
target refuses to build if its owner gate is unavailable or publication fails. The contained
compiler can read the fixed cache-receipt index to validate an already published owner, but
cannot write that index, even when an output directory broadly contains it. Package selection
and the package-cache guardian keep their existing policies.

`alab_check_targets_ready` declares the entire gate inventory before containment:
source and pristine build targets, their dedicated lint targets, and ALab's
attribution runner sharing the pristine build target. The gate and preparation
share the lint-path helper. A refused declaration stops before any compiler, so
an unregistered later lint target cannot waste a completed build and test run.

Before its first compiler, the canonical gate also resolves the full locked
dependency graphs of both source and pristine upstream with offline metadata.
The shared helper checks both trees and preserves their errors. A source-only
cache is insufficient: missing pristine dependencies otherwise appear only after
the source tests finish and attribution starts. Prepare both exact lockfile
closures before freezing a costly run; this observation never fetches or relaxes
containment, registry ownership, or test attribution.
Resource stops and interruptions return immediately without a dependency verdict.

For a cold private Cargo home, run this from the fixed checkout before the gate.
First run `alab/setup.sh` if `upstream/main` is absent. Set `ALAB_ATPKG_PUBLISH`
to the real producer when it is not on PATH, and `ALAB_PYTHON` to the selected
Python executable when needed. This preparation can fetch dependencies; the gate
itself keeps its offline preflight. It uses the same native lease, target
inventory and containment as the gate, and does not compile or use `~/.cargo`.

```bash
set -euo pipefail
repo="$(pwd -P)"
. "$repo/alab/lib.sh"
alab_project_run "$repo" "" /bin/bash -c '
  set -euo pipefail
  repo="$1"
  . "$repo/alab/lib.sh"
  alab_project_selected
  cd "$repo"
  upstream_base="$(git merge-base HEAD upstream/main)"
  pristine="$(alab_pristine "$repo" "$upstream_base")"
  alab_pristine_at "$repo" "$upstream_base"
  build_target="$(alab_check_targets_ready "$repo" "$pristine" "$repo/codex-rs/target")"
  pristine_target="$(alab_real "$pristine/codex-rs/target")"
  export CARGO_TARGET_DIR="$build_target"
  alab_contained --pristine "$pristine" --read "$ATPKG_PROJECT_FRONT" \
    --read "$ATPKG_PROJECT_EXECUTION" "$build_target" "$pristine_target" -- \
    /bin/bash -c '\''
      set -euo pipefail
      . "$1/alab/lib.sh"
      (cd "$1/codex-rs"; export CARGO_TARGET_DIR="$3"
       alab_build_command "$ALAB_CARGO" metadata --locked --format-version 1 >/dev/null)
      (cd "$2/codex-rs"; export CARGO_TARGET_DIR="$4"
       alab_build_command "$ALAB_CARGO" metadata --locked --format-version 1 >/dev/null)
    '\'' alab-prepare "$repo" "$pristine" "$build_target" "$pristine_target"
' alab-prepare "$repo"
```

After it succeeds, leave the source fixed and run `bash alab/check.sh` normally.
The [native gate review](reviews/native-gate-provisioning-2026-11-06.md)
records a successful protected preparation followed by a passing full gate.

When macOS's `/bin/ps` has set-id bits, the wrapper supplies an ordinary ad-hoc signed
copy on PATH. The sandbox refuses to exec the privileged system image; the copy can
observe owned children for cleanup assertions. Its mode is 0755 and its path is
read-only inside containment. A successful first exec prints the wrapper's PID before
the scratch path becomes eligible for cleanup; a preparation failure retains that path.
The original system image and the sandbox's account/file/IPC restrictions stay intact.

Contained commands build with a **private cargo home**, `<git-common-dir>/alab-cargo-home`
(`ALAB_CARGO_HOME` names another; outside a git checkout it must be set), as both
`CARGO_HOME` and `TARGO_HOME`. `~/.cargo` is not writable from inside, and its registry
and git caches are not even readable: every build the owner runs outside the sandbox, aterm
included, compiles crate sources from those caches, so a build script that could write them
could plant code that later runs uncontained. `~/.trust/cache` is read-only for the same
reason. The private home is made outside the sandbox, and the first time it gets a copy of
`~/.cargo/config.toml` when that names no token or credential provider; its config is
read-only from inside, so one contained step cannot reconfigure the next. It is about as
large as `~/.cargo/registry` (3 to 4 GB), downloaded once per clone; nothing caps it.
`ALAB_CONTAIN=0` turns containment off, and with it the private home (the caller's own
cargo home is used); a system without `sandbox-exec` (Linux) runs uncontained and says so.
Outside a git checkout nothing of the working directory is readable but what the caller
names, and a working directory (or a checkout) at `HOME` or above it is refused. Because
`check.sh` cannot check out the pristine tree from inside, the sync puts it at the merge
base first.

Contained builds retain the caller's `CARGO_BUILD_JOBS` and `RUST_TEST_THREADS`, so
machine admission's compile share and the test concurrency limit survive the
environment scrub. Containment itself leaves an unset limit unset; `check.sh`
defaults missing or empty limits to four build jobs and four test threads, independently,
so a fresh scheduled job has bounded build and test parallelism. Unrelated
environment variables remain scrubbed. Run `python3 alab/tests/test_contained_budget.py` outside a
contained command to check this through the actual macOS sandbox boundary.

What containment does not stop:

- **A process that outlives its step.** A contained command can leave a process behind
  (a daemon, or anything it forks into the background) that keeps running after
  `sandbox-exec` returns. It stays sandboxed, but it can go on writing everything that
  step could write: its target dir, the private cargo home, and anything the step was
  given. So nothing outside the sandbox runs a program from, or writes into, a directory a
  contained step was given (`package.sh` copies the package out before adding to it), and
  what a contained build records about itself (`target/.rustc_info.json`) is data,
  compared as text, never a path to run. One contained step can still poison what a later
  one reads (the private cargo home), which stays inside the sandbox.
- **launchd.** `launchctl` itself cannot run inside, but a program that talks to launchd
  directly (submitting a job over its Mach service, which the profile leaves open because
  builds need launchd for much else) gets a process launchd starts, outside the sandbox.
  A separate macOS user with a deploy key that pushes only to codex-alab closes that, and
  the other gaps of running as the owner.
- **What it builds.** The binaries a contained build makes are upstream's code. Running
  them as the owner (a linked package, `cargo run` outside this tooling) is trusting
  upstream; containment covers building and testing them here, not using them.
- **The network.** It is open: a build script can send what it can read (the clone, the
  toolchains) anywhere.

Only crates under `codex-rs/` with a library target get step 5; a hook in a binary-only
crate (bwrap, config-schema, thread-manager-sample, voice-host) is covered only by the
lint in step 4 and, if codex depends on it, the build in step 2.

### Running unattended

`daily.sh` is the scheduled entry point. Each run is one stepwise sync. It appends the
output to `.git/alab-daily.log` and rewrites `.git/alab-daily.status` with one line:
when, exit status, the two tips and how many upstream commits are pending.
The whole run holds a kernel lock; an overlapping daily invocation exits 75 before
syncing or changing status. Failures stay in the journal for an agent to revisit.
The scheduler sends no notifications or callbacks and keeps no alert delivery state.

Before each run, holding the sync lock, and only while no sync is merging or holds commits
`main` lacks (a resolver may be building in the sync worktree), a `target/` of the sync
worktree or its pristine tree over `ALAB_TARGET_CAP_GB` (default 40, each) is deleted. This
Mac's disk ran 96 to 100% full while this was built; a codex build plus one crate's unit
tests take 12 to 15 GB of `target/`, and the gate's test builds add more.

After each run, landed or stopped, holding the sync lock again, `daily.sh` removes the
pristine tree's `target/` always, and the sync worktree's unless at least
`ALAB_KEEP_TARGET_GB` (default 100) is free on its volume once the pristine one is gone; the
daily log says what it removed, how big it was, and what is free after. Measured on this
Mac (2026-10-16): the two trees stayed on disk between runs at about 11 and 14 GB, and with
other builds running they took the data volume, 97% full, down to 13 GB free. A `target/`
a cargo build holds is kept whatever the space: cargo flocks `<profile>/.cargo-lock` for a
whole build, and a person's build in the sync worktree by hand holds no sync lock (with the
store's Trust: the lock probes busy while a build runs and free after it). When a sync holds
the lock, nothing is removed and the log says so. The next check rebuilds what went: a
cold codex `target/` is about 11 minutes here.

Both cleanup passes honor the clone-wide `alab-preserve-builds` policy before removing a
target. Any marker kind in the shared Git directory preserves the builds across linked
worktrees; an unavailable or unreadable policy helper also keeps them. Without that
policy, the normal size, free-space and active-use checks still govern cleanup.

```sh
PATH="${HOME}/.local/bin:${PATH}" alab/daily.sh --launchd-plist \
  >~/Library/LaunchAgents/systems.alab.codex-alab-sync.plist
plutil -lint ~/Library/LaunchAgents/systems.alab.codex-alab-sync.plist
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/systems.alab.codex-alab-sync.plist
```

The plist starts a run when it is loaded (at bootstrap and at every login), then every
4 hours (`StartInterval`, `ALAB_INTERVAL_SECONDS`); launchd skips an interval that falls
during sleep. It carries the PATH and HOME of the shell that printed it, so `targo`
resolves. Include the durable `aterm` CLI directory too (`~/.local/bin` for the standard
install, as above). The generator preserves the supplied PATH without adding executable
directories. The plist also carries `ALAB_SYNC_DIR`, `ALAB_CARGO`, `ALAB_ATERM`, `ATPKG_BIN`,
`ALAB_TARGET_CAP_GB`, `ALAB_KEEP_TARGET_GB`, `ALAB_CONTAIN`, `ALAB_CARGO_HOME`,
`ALAB_PYTHON`, `CARGO_BUILD_JOBS` and `RUST_TEST_THREADS` if they were set. Select
Python 3.11 or newer through `ALAB_PYTHON` for the native V8 artifact resolver when
the default interpreter is older. Smaller ranges conflict less, so more often is
cheaper. Name one sync host: the lock is per clone, and two hosts would race to push.

Install this source-sync job separately for the chosen clone and account; neither a
checkout nor the local package updater installs it. Generating its plist does not load
the job. Check installation with
`launchctl print gui/$(id -u)/systems.alab.codex-alab-sync` and its latest outcome with
`alab/daily.sh --status`. Run it as the owner's account with the gate contained (above);
its git credentials stay outside the sandbox. Bootstrapping starts the first sync
immediately, including fetches, gated merges and pushes to `origin/main`.

### If upstream rewrites history

Upstream has never rewritten `main`, but it is an export from an internal repository, and
an export can be re-based. If that happens, the fetch refuses and the sync stops. Moving
ALab's layer is then a one-off job for a person:

1. Fetch the new history beside the old, without the configured refspec (which would
   refuse): `git fetch --refmap= upstream +main:refs/alab/upstream-new`.
2. Take the layer as a patch, `git diff --binary <last merged upstream> origin/main` (the
   layer deletes binary files, which a plain diff cannot carry). `<last merged upstream>`
   is the second parent of the newest `alab sync:` merge on `main`'s first-parent line.
   Apply it to `refs/alab/upstream-new` in a scratch worktree, and resolve.
3. Join the histories without a rewrite, and push the join to `origin main` (a
   fast-forward): `git commit-tree <resolved tree> -p origin/main -p refs/alab/upstream-new
   -m "alab sync: merge upstream openai/codex@<new tip> (re-join after upstream rewrote
   main)"`. The subject must start with `alab sync:`: the sync takes the second parent of
   the newest such merge as the upstream commit `main` merged last. Every shipped commit
   stays reachable.
4. Move `upstream/main` once, by hand (the fetch refuses to, by design):
   `git update-ref refs/remotes/upstream/main refs/alab/upstream-new`, then
   `git update-ref -d refs/alab/upstream-new` and re-run `alab/setup.sh`.

## The layering law: customizing without paying daily

Every line ALab changes in a file upstream owns can conflict at a sync. A line in a file
only ALab owns never does. How much a layer costs depends on where its lines sit; their
number matters far less. Replaying 90 days of daily merges against simulated ALab edits:

| ALab edit | upstream changed that file on | conflicts |
| --- | --- | --- |
| one-line `"alab-*"` member, last in the `codex-rs/Cargo.toml` members list | 52 days | 0 |
| one-line `install(&mut builder)` in `app-server/src/extensions.rs` | 14 days | 0 |
| one-line default change in `core/src/config/mod.rs` | 65 days | 0 |
| 15-line patches inside random functions of `tui/src/app.rs` | — | 19 of 26 patches, 35 conflicts |
| the same in `tui/src/chatwidget.rs` | — | 18 of 30 patches, 34 conflicts |
| the same in `core/src/config/mod.rs` | — | 16 of 30 patches, 31 conflicts |

That is about 0.01 conflicts per in-function patch per day. A layer of 100 such patches
would need a hand resolution about every day, while a layer of well-placed one-line
hooks needs almost none. So:

1. **New behavior lives in ALab-owned paths.** Code goes in `codex-rs/alab-*` crates. A
   single `"alab-*"` glob line, last in the workspace members list, makes every one of
   them a member; add it with the first crate. Process, tooling and docs go in `alab/`;
   agent rules in the root `AGENTS.md`.
2. **Upstream files get hooks, not features.** A hook is a few lines: a call into an
   ALab crate, a match arm, a list entry, a default. Each carries an `ALAB(<topic>):`
   comment saying why. `delta.sh --topics` lists them, and `check.sh` fails if one
   disappears.
3. **Use the extension points upstream already has before writing a hook.**
   - `ExtensionRegistryBuilder` in `app-server/src/extensions.rs`.
   - Config `notify` and `hooks`.
   - `model_providers` and `profiles`.
   - For aterm integration, drive the `codex app-server` protocol or `exec --json` from
     the aterm side instead of patching the TUI.
4. **Config before code.** ALab's defaults go in `codex-rs/config/defaults.toml`. It is
   compiled into the binary as the lowest config layer ("packaged defaults"), so it
   reaches every install with no file on disk, and upstream has changed it twice in 90
   days. Two placement rules:
   - Top-level keys go in a block right after its first line. The file ends inside
     `[history]`, so a key appended at the end is silently read as `history.<key>`.
   - New tables go at the end.
5. **Never reformat, reorder, rename or re-indent upstream code.** Format only what you
   wrote, with `trustfmt <file>`. A workspace-wide `targo fmt` rewrites upstream's files.
6. **Remove only whole paths, through `removed-paths`.** A hand-deleted upstream file
   conflicts every time upstream edits it; a listed one resolves itself. A workspace
   crate cannot be removed that way: the members list, `[workspace.dependencies]` and
   its dependents would all need hooks.
7. **No upstreaming.** OpenAI takes no pull requests. File an issue with a root-cause
   analysis if it helps, and plan to carry the change until OpenAI ships an equivalent.
   When it does, the resolver drops ALab's copy (step 3 of "Resolving a conflict").
8. **One topic per commit, with the subject `alab(<topic>): ...`.**

## The occasional refactor

`delta.sh` ranks every upstream file ALab modifies by risk: upstream commits touching
it in the last 90 days, times the lines ALab changed in it. `--patch` shows just those
hunks, and `--topics` the hooks grouped by topic.

Refactor when:

- the top of that list grows;
- a file needs a hand resolution twice in a month;
- or once a month, whichever comes first. Upstream minors land every ~5 days, too often
  to be the trigger.

A refactor does one or more of these:

- moves logic out of upstream files into ALab crates behind smaller hooks, or onto an
  extension point;
- replaces code patches with `defaults.toml` entries;
- drops changes upstream has made redundant.

Measure with `delta.sh` before and after. The modified-upstream line count and the top
risk score should both drop; nothing else is the point.

## Releases

`main` tracks `upstream/main` only, and `sync.sh` refuses a target that is not on it.
Merging a release tag into `main` would keep its version line and cherry-picks through
every later sync, and every later release would then conflict on that line.

To ship something aligned with upstream release X:

1. Find upstream's branch point, `BP=$(git merge-base upstream-tags/rust-vX upstream/main)`.
   A tag appears 8 hours to 7 days after its branch point, so `main` has normally merged
   past it by then.
2. Cut a leaf branch `release/X` from the newest `alab sync:` merge on `main`'s
   first-parent line whose second parent is an ancestor of BP. Cutting from `main`'s tip
   instead would give X plus everything upstream landed since BP.
3. On the leaf, merge BP the way a sync does: with the lockfile rule (the `-c` options
   `sync.sh` passes, naming `alab/merge-attributes`), re-apply `alab/removed-paths`
   (`git rm -r -q -f --ignore-unmatch` each entry: without `-f`, git refuses a file the merge
   just added), then `targo metadata` in `codex-rs/` to
   re-resolve the lock, and commit. Then merge `upstream-tags/rust-vX`. That brings in the
   rest of the branch point, the version bump and the release's cherry-picks.
4. Package from the leaf. Nothing on it ever merges back into `main`.

Whether ALab ships from `main` (alpha-equivalent, daily) or from release leaves
(stable-equivalent, weekly) is an owner decision. Trailing latest points to `main`.

## Packaging through atpkg

On Apple-silicon macOS, atpkg's signed index distributes codex-alab as the managed `codex`:
`[programs.codex]` names `repo = "codex-alab"`, and the channel pins its production build.
`aterm pkg install codex` installs it; `aterm pkg update codex` follows the channel, and
`aterm pkg which codex` reports the installed source and build. Public `vX.Y.0` releases
are source snapshots of codex-alab's own line (`v0.1.0` on, Decision 8); production binaries
still carry the dev version described below, and tell OpenAI's servers the client version.
The ALab lane orders builds by signed build number, never by comparing that version with
OpenAI's (`aterm`'s `crates/atpkg/src/alab.rs` and `cli_alab_tests.rs`). Other targets keep
the vendor lane.

`aterm pkg update codex --vendor` explicitly chooses OpenAI's builds on this machine;
`aterm pkg update codex --alab` returns to codex-alab's channel. Without that choice,
an installed ALab build is kept if the index no longer pins one, rather than replaced by
OpenAI's build.

`alab/package.sh` builds the same package layout locally. A development link takes
priority over managed updates; unlinking restores the previously installed managed build
and preserves any pin. If there was no managed build, install one after unlinking. Once
an ALab package under `<out>/codex` is selected, successful packages in that same output
automatically advance it. The source must descend from the selected source on
`origin/main`, and its version must advance. Other checkout links and sources stay put.
`--no-promote` builds without changing selection. `ALAB_ATERM` names the aterm executable
when it is not the one on PATH; promotion requires its guarded `pkg link --if-checkout
--if-prefix --wait-lock` capability and never falls back to an unconditional link. It
waits up to 30 minutes for the store lock, then rechecks the selection under that lock.
A refused selector leaves the completed package available and reports the failed promotion.
Hooks on `main`: none.

`local_update.py` closes the scheduled local-production path. It fetches only `origin/main`,
without changing the supplied checkout or pushing anything. Only a selected, admitted package directly
under this output's `codex/` directory opts into this channel. An unchanged source does no
build; a forward source reuses its complete published artifact or runs the existing packager
for that exact fetched commit. The original selected package is checked again after the
build and under atpkg's lock, preserving a manual checkout, unlink or package change.
Before fetching, the dispatcher probes an existing regular package-cache lock without
creating it. A busy cache exits 75 and the next scheduled poll looks again. This is only
a hint: the fetched entry must claim the same canonical lock for the build/promote
transaction, and a race to a busy cache also returns 75. A failed build or refused
selection reports failure without an unconditional retry.

The packager's upstream fetch has a 120-second deadline inside the already owned
transaction group. A timeout stops and reaps Git, fails the transaction, and lets
the existing guardian stop any remaining helpers before cache unlock. The next
scheduled poll can try again; compilation has no added deadline. Read-only
package queries also own a command group, without acquiring a package-cache lock.

The command guardian's surviving custodian inherits the same package-cache BSD
flock and owns the real command as an unreaped direct child in a separate process
group. It stops remaining group members after command exit. Guardian exit, SIGTERM
or abrupt death closes a private lifeline and requests the same cleanup. The
custodian stops and reaps that group before releasing its lock lease.
The guardian waits for retirement proof before returning or settling cache/source
receipts. Package children stay in the command group. Deliberate group escape and
SIGKILL of the surviving custodian remain outside this boundary; missing retirement
proof fails the transaction without guessing an unowned process group to signal.

When the source advances, the dispatcher makes one public handoff to the exact fetched
commit's controller in a private detached source worktree. Its controller, guardian,
promotion code, packager and ALab helpers advance together even when the supplied checkout
stays behind or holds owner edits. Git operations and the status journal retain the
original `--repo`. The public `--fixed-source COMMIT --expected-checkout CHECKOUT
--expected-prefix PREFIX` mode requires an exact clean controller checkout, uses the
dispatcher's interpreter, fetches no second tip and creates no second source worktree.
It chooses its own same-version cache guardian and private transaction entry; private
guardian contracts no longer cross source versions. The fixed entry rechecks the original
verbatim selected checkout and resolved prefix under that guardian's cache lock, preserving
a manual selection made during dispatch. Optional initial-selection source/version
arguments are journal observations only, never selection authority.
The bootstrap validates the bounded managed-link marker and its direct, nonlinked artifact
directory, but leaves package records and version syntax to the fetched controller. An old
version parser must not strand the channel after a newer controller selects a new package
format. Bounded build-record source/version hints are best-effort journal observations;
missing or unfamiliar records still reach the controller, which refuses any invalid package
before building or selecting. Older bootstraps need one source refresh for this boundary.
A private source-only call without original custody or the current owning-guardian
contract still fails closed. Older bootstraps that use the private handoff need one
source refresh to adopt the public dispatcher; the new public mode then lets the
guardian and its private caller advance together. Journal snapshots never grant authority.
An unchanged source hint ends a no-op poll without creating a controller worktree or granting
any package authority. Before allocating a controller for newer source, the dispatcher
checks the same eight build-disk paths against the 20 GiB floor. A known shortage or
unreadable measurement refuses the poll without allocating source trees; repeated
low-disk polls cannot accumulate controllers before reaching the fetched guardian.
This preflight reserves no space or cache lock; the guardian still checks throughout
its transaction. Older bootstraps need a source refresh for this early check.
Clone-local preservation policy from `retention.py` also applies
to controller cleanup: a marker or unknown policy keeps both registered source trees
and empty paths left by failed registration. Older bootstraps need refreshing for this
cleanup behavior. The dispatcher keeps the controller source until the fetched public
entry and its guardian exit. Without preservation policy, exact clean source-only
controller worktrees are then removed normally, using their own retention helper;
this source-only Git cleanup occurs after the build guardian releases the cache.
Every nonzero or interrupted public handoff preserves its whole source worktree,
even if it looks clean: a failed wrapper does not establish that its guardian or
children have stopped. Successful cleanup relies on the fetched public mode's contract
that exit 0 follows guardian cleanup; the dispatcher does not independently observe
another guardian's process group.
Tracked changes, untracked files, ignored output or an unreadable state preserve the whole
path for inspection. Build/artifact retention policy still applies to the packager's own
worktrees and outputs.

The controller supports Python 3.9 or newer; native package builds require Python 3.11 or
newer. Use an explicit capable, durable aterm executable; a development app bundle kept
in a permanent artifact directory is suitable. The selected
prefix is still checked by atpkg, and the host must advertise both selection guards and
bounded lock waiting before any build starts. For example, prepare a job for review:

```sh
ALAB_ATERM='/permanent/path/aterm (dev).app/Contents/MacOS/aterm' \
  /opt/homebrew/bin/python3.14 -I alab/local_update.py --launchd-plist \
  >/tmp/codex-alab-local-update.plist
```

Printing the plist does not install or start it. It binds the actual Python interpreter,
the supplied host, package prefix, output and cache paths, runs at load and every 900 seconds
(`--interval`), and lowers its CPU and I/O priority. `--repo`, `--out`, `--prefix` and `--keep`
can specify the concrete job.
Explicit `CARGO_BUILD_JOBS`, `RUST_TEST_THREADS`, `ALAB_CONTAIN`, `ALAB_CARGO_HOME`,
`ALAB_CARGO` and `ALAB_ATPKG_PUBLISH` values are carried into the job; leaving one unset keeps the builder's existing default
for that control. A custom private Cargo home therefore survives job regeneration
instead of selecting another clone-local cache.

An explicit `ALAB_PYTHON` builder is checked for Python 3.11 or newer before the plist is
emitted. Otherwise a capable controller also serves as the builder, even off `PATH`;
an older controller leaves builder discovery to `package.sh`. Without `tomllib`, marker
reads accept only the flat generated keys, single-line strings and string arrays;
unsupported syntax or malformed values leave the selected channel unchanged.

Its log is `<git-common-dir>/alab-local-update.launchd.log`;
`python3.14 -I alab/local_update.py --status` prints the last reported transaction phase,
the selection observed at its start (`initial_selected_*`) and the fetched target source.
`controller_source`, `controller_path` and `controller_python` identify the tooling used;
the source-only path can have been retired after a completed run.
The initial selection remains historical after completion; `codex --version` observes the
build selected for a new invocation. A build reports before
it starts, so a long compile does not leave an older completed result on display. `complete: false` means
completion has not been recorded; it does not prove the updater is still alive. Only a
completed record has an exit code; target versions are not predicted before a build.
Once the selected local package is validated, the updater also best-effort mirrors a
small display record to `codex/.alab-local-update-status.json` in that artifact channel.
It contains only the reported phase and UTC time, initial package/source/version,
target source when known, and a completed exit code with the last work phase. The
Git journal keeps its existing format and diagnostics; raw reasons, paths, environment
and log contents are not mirrored. Atomic replacement preserves the previous complete
record if publication fails or is interrupted. A display failure cannot change the
update result or selection, and older updaters simply leave this observation unavailable.
This job is separate from `daily.sh`: upstream source synchronization and its existing
notifications keep their own schedule and error semantics. The local job needs no public
release or release credential.

`aterm pkg which codex` and a shell's `codex --version` describe what a new invocation
runs. An already running TUI keeps its existing image until native live upgrade can
resume its loaded threads safely (CONTRACT §5). The October 17 build (`07a25d86a`)
rejects pending continuation timers; builds through `e8ed46aae` can still start an
immediate Cruise continuation before considering upgrade after a long turn. Either
can defer an upgrade indefinitely. Restart and resume those old sessions at a safe
stopping point to load the selected package. Current code carries both paced timers
and newly decided immediate continuations across re-exec before starting another turn.

```sh
alab/package.sh --commit main      # package main (the run fetches upstream; fetch origin first)
alab/package.sh                    # package HEAD of this checkout (clean, and on origin/main)
alab/package.sh --version-only     # print the version a build of HEAD would carry, and why
alab/package.sh --client-version-only   # print the client version (what OpenAI's servers see)
alab/package.sh --upstream-base    # print the upstream base line HEAD's release page names
alab/package.sh --any-commit       # package a commit that is not on origin/main
alab/package.sh --rebuild          # package a commit again (a fixed packager, a new compiler)
alab/package.sh --no-promote       # build without advancing the selected artifact channel
aterm pkg link codex <pkgdir> bin/codex   # choose a local artifact channel initially
aterm pkg unlink codex                    # restore the previously installed managed build
```

`--out <dir>` and `--keep <n>` say where packages go and how many stay. A commit that is
not on `origin/main` is refused (Decision 4) unless `--any-commit` is given. A commit
that a package in `<out>/codex` already holds (the `source` in its `alab-build.json`, not
its version, whose line moves) is refused unless `--rebuild` is given; the new package is
then staged beside the old one, as `<version>.rebuilt-<UTC time>` when the version is the
same.

What a run does, in order (`--help` says the same):

1. **The two versions** (Decision 8), both `MAJOR.MINOR.0-dev.<count>+g<sha9>`, the dev
   form of `~/publication/VERSIONING.md`. `<count>` is `git rev-list --count`, for ordering
   only; the sha is the identity.

   - **The version**, codex-alab's own: `MAJOR.MINOR.0` is the commit's root `VERSION`,
     codex-alab's release line (restarted at `0.1.0`, moved only by `pub bump codex-alab`).
     It is what the package says it is: `codex --version`, `codex-package.json`,
     `alab-build.json`'s `version`, the package directory, atpkg's pkg record, the banner and
     `/alab version`.
   - **The client version**, upstream's numbering: `MAJOR.MINOR.0` is the higher of the
     commit's `alab/client-version` and the floor: the next minor above the newest upstream
     stable tag (`upstream-tags/rust-vX.Y.Z`, no pre-release) whose branch point (its
     merge-base with `upstream/main`) the commit has merged. It is what upstream's code and
     OpenAI's servers see (`client_version`, the user agent, telemetry, the rollout's
     `cli_version`), compiled in as `CODEX_ALAB_BUILD_VERSION` and recorded as
     `alab-build.json`'s `client_version`. So a package never tells the server it is older
     than the upstream code it holds, and packaging never waits on an edit (upstream cuts a
     stable every two to seven days). When `alab/client-version` is behind the floor the run
     says so and that the file is due an edit before the next release: `--release-check`
     (what `publish/pre-promote` runs) refuses until it is, because a published snapshot (one
     commit, no history) has no branch point to read and takes its `alab/client-version` as
     cut.

   **The compound version** (Decision 8, amended 2026-10-24): from the version ledger
   (`alab/versions.toml`) on, the version carries the upstream Codex baseline in its SemVer
   build metadata, `<VERSION>-dev.<count>+codex.<baseline>.g<sha9>` (a release names
   `<VERSION>+codex.<baseline>`), the baseline being the newest upstream stable
   `rust-vX.Y.Z` whose branch point the commit has merged, its patch number kept
   (`0.1.0-dev.13600+codex.0.164.0.g<sha9>`). Build metadata never orders: SemVer, Cargo and
   every aterm reader order by the core and the count alone. `alab-build.json` records the
   baseline again as `baseline`. A published snapshot has no history to read a baseline from,
   so it takes its ledger's row for its `VERSION` (the release that cut it carried that row),
   and with no row it is refused. When the ledger already maps `VERSION` to an older baseline
   (upstream cut a stable and `main` merged it after the release), the run names the
   commit's own baseline, says `pub bump codex-alab` is due, and `--release-check` refuses
   until then. A commit with `alab/client-version` but no ledger keeps the version it was
   packaged with (`<VERSION>-dev.<count>+g<sha9>`) and releases nothing.
   `--release-version` prints the compound release version once the release check passes,
   and `--ledger-row` the ledger row a release of the commit appends. Both, and
   `--release-check`, fetch origin's main fast-forward only and check the ledger along it and
   against the public release repository's releases (Decision 8, "The gate"). Every mode
   refuses a clone whose history git does not hold whole (a shallow clone cut below a commit
   with parents, which would count too few commits and hide ledger edits, or a grafts file)
   and ignores replacement objects.

   A commit from before `alab/client-version` has one version, its client version, as it
   was packaged then (its `VERSION` was upstream's numbering, held to the floor, and
   `--release-check` holds it to the floor as before); a commit from before `VERSION`
   existed takes the floor. `--version-only` and `--client-version-only` print each. The
   run fetches upstream's release tags and `main` first, by the refspecs
   `alab/setup.sh` configured (fetch only; `main` has no `+`, so a rewrite stops the run), so
   a stale tag ref cannot pass an old line, and prints how it got each part, with the
   upstream base. A dirty checkout is refused.

   **The upstream base**, beside the version and never in it: the newest upstream stable
   whose branch point the commit has merged (the floor's stable) and the upstream commit
   it merged last (its merge-base with `upstream/main`), read once for three readers. The
   release build gets them as `CODEX_ALAB_UPSTREAM_VERSION` (`0.163.0`) and
   `CODEX_ALAB_UPSTREAM_COMMIT` (40 hex), which the final executable captures through
   `alab-core/src/version.rs` for
   the session header, the launch card and `/status`; `alab-build.json` records them as
   `upstream`, with the count and the date below (step 5); and `--upstream-base` prints the
   line a release's page names it with (`publish/release-body` runs it for the source
   release's), counting the upstream commits past that stable's branch point and dating the
   merged commit by its committer date in UTC. Measured at `origin/main` f692cf474 (2026-10-21):
   `Based on upstream openai/codex rust-v0.163.0 + 241 commits (e8aa7c1678ca, 2026-10-20)`.
   A published snapshot has no history to read a base from: its build says `unreleased`, its
   `alab-build.json` has no `upstream`, and `--upstream-base` refuses it.
2. **A private scratch worktree** at the commit, `~/Developer/codex-alab-pkg-<sha9>.<nonce>`.
   Sources with `initialize_running_build!` retain their manifest and lock versions:
   `CODEX_ALAB_BUILD_VERSION` supplies the package version at the two final executable
   call sites. Libraries read that immutable running identity without compiling the
   per-build input themselves. A fallback read never initializes it, and conflicting
   initialization refuses. Historical `--commit` sources without this capability keep
   the legacy workspace-version stamp and contained `targo metadata` refresh; that
   refresh may change only workspace versions in the lock. Neither path commits a stamp. The
   worktree goes when the run ends, however it ends
   (`ALAB_PKG_KEEP_WORKTREE=1` keeps it); the run removes its own worktree only and
   never prunes the clone's other registrations.
3. **The compiler, then codex and codex-code-mode-host.** The package entry resolves
   its requested revision to one full commit using local metadata, then invokes
   `atpkg-publish toolchain run --repo DIR --source FULL_SHA -- COMMAND` before
   touching a compiler-keyed cache. The native selector authenticates that exact
   source's committed `trust.lock`; a genuinely unlocked source uses the freshly
   authenticated managed default. `ALAB_ATPKG_PUBLISH` may name a qualified native
   producer. Caller compiler overrides, including `ALAB_CARGO`, refuse.
   The canonical gate and package controllers refuse `ALAB_CONTAIN=0`.

   `atpkg-publish` is a development producer, not a shipped client tool. If it is
   absent from PATH, build it in an owned aterm worktree, following that repository's
   toolchain and submodule instructions:
   `targo --unverified build --profile test --locked -p atpkg-publish`.
   Cargo's `metadata --no-deps --format-version=1` names the target directory; use
   the real built executable through an absolute `ALAB_ATPKG_PUBLISH` path.
   Its `toolchain run --repo DIR -- /usr/bin/true` can check compiler admission
   before a costly gate. That preflight authenticates and leases the compiler;
   the normal protected gate still has to run. Keep its private Cargo home and
   prepare both locked dependency graphs when its offline preflight finds a cold
   cache; do not substitute the owner's Cargo home or inject selection fields.

   The selector leases the signed payload and validates its execution root through
   controller settlement. Every compiler call uses its explicit native admission
   hop, including calls from pristine upstream, without rediscovering a cwd pin.
   A one-file probe records the selected compiler's version, root and binary digest
   for cache identity. This observation must match the native selected execution
   root. It grants no authority itself; `pack-package` independently authenticates
   the same full source lock and compiler record before signing. The local artifact
   selector retains `compiler.admitted=true` only after the observation matches the
   held execution root; this status field does not authorize native publication.
   Then targo builds the probe crate again, contained, in `codex-rs/` (whose
   `.cargo/config.toml` it reads) and in the neutral `~/.cache/codex-alab`: each record
   must name that compiler, so a stock toolchain stops the run here, not after the build.
   A record a contained step wrote is only compared, as text, with the trusted `-vV` and
   sysroot: nothing named in it is run (the record is the build's own claim, so this
   catches a misconfigured toolchain, not a hostile build that forges it). Then one
   release build: `targo --unverified build --release --locked -p
   codex-cli --bin codex -p codex-code-mode-host --bin codex-code-mode-host`, adding
   `-p codex-exec-server-cli --bin codex-exec-server` when the selected commit's packager
   accepts `--exec-server-bin`. Historical `--commit` targets keep their own input set.
   Every supported binary is supplied as a prebuilt input to that target's packager,
   so its Python never starts a second Cargo build. The build runs with
   `STABLE_GIT_COMMIT=<commit>`, the package's `CODEX_ALAB_BUILD_VERSION` and upstream
   provenance inputs, and the `RUSTY_V8_ARCHIVE` pair from upstream's
   `scripts/codex_package/v8.py` (`resolve_codex_v8_cargo_env`, cached in
   `~/.cache/codex-alab/v8`). The script checks the pair again itself: the release's
   checksum manifest against `third_party/v8/*_release_manifests.sha256`, each file against
   the manifest. No `--target`, so the output is `<release-target>/release/`. The build runs
   contained (`alab_contained`, "The gate"), writing only the private package cache's
   selected compiler-keyed target; the V8 cache is
   read-only to it (`--read`). Its `.rustc_info.json` must record the trusted
   compiler. Every native package target, the three compiler probes and rg's install
   target included, ends in `.noindex`, also when the resolved store
   targo runs without atpkg's shim.
   The package guardian serializes this target through child cleanup. Its root must be
   an owned real directory without group/world write permission, and cannot be a home
   or above one. Successive sources keep their own running stamps and source worktrees
   while making unchanged dependency outputs available to Cargo; a compiler change selects
   another target.
   A host advertising `aterm-package-build-cache-v2` can admit a fresh clone-scoped
   `rust-budgeted-release-<target>-<compiler id>-<clone id>.noindex` as a wholly
   disposable cache. Its native receipt binds the directory, coordinator lock, current
   guardian attempt and this clone's `alab-preserve-builds` path plus its parent identity.
   The clone id hashes that canonical Git common-directory identity: linked worktrees
   share outputs, while independent clones sharing a package cache use separate targets.
   Existing compiler-only targets are never adopted into disposal. A marker created
   later, an unreadable policy or a missing/replaced parent holds all retirement,
   including partially completed retirement. New schema-3 receipts make older native
   helpers preserve rather than silently ignore that guard. The initial budget is
   64 GiB. A successful first build of a fresh
   generation can raise it once to twice that build's footprint; that learned limit survives
   cache generations. Failed builds and later warm builds never raise it.
   Maintenance can retire the whole target when its allocated size exceeds the limit or
   its volume has less than 20 GiB available. Published packages and source worktrees are
   outside that disposable target. The guardian reports settlement only after its
   process group is gone and its source handoff succeeds. Native maintenance runs before
   and after the guarded transaction, outside its coordinator lock, and acquires that lock
   itself before considering retirement. Cargo locks and process-use checks protect live
   outputs, including mapped files whose descriptors have closed. Uncertain maintenance
   preserves the cache and the build's original exit status. An unfinished retirement must
   settle before another policy build can start, so deferred deletion cannot accumulate
   more generations. Each clone family keeps one materialized policy target across
   compiler keys under the shared coordinator. Other families' active or preserved
   outputs do not force this family onto an unbounded fallback. Selecting a different
   key requests the old target's retirement; native
   admission returns exit 75. Maintenance runs after the guardian returns, and the next poll
   can admit the new target once the old one is gone. Switching compilers can therefore
   require a cold build. An older host (v1 included), initial preservation policy,
   `ALAB_PKG_KEEP_WORKTREE=1`,
   or an unadopted target retains the previous
   registration and reuse behavior; admission refusal
   from a capable host stops the build rather than silently dropping its policy.
   An untrusted or noncanonical preservation parent in the guardian context also
   refuses before admission rather than silently dropping that context.
   This is target reuse, not a claim that every unchanged local library avoids recompilation.
   New-capable sources
   avoid per-build Cargo package-version changes, but fresh source paths and mtimes
   remain, including vendored path dependencies. The first build with stable Cargo
   identities can rebuild local crates once; this does not measure a release speedup.
   Private packaging fixtures prove allocation, routing, compiler partitioning and
   refusal semantics; they do not measure Cargo cache hits or a build speedup.
4. **rg, built with Trust** (atpkg's lineage check refuses upstream's stock-built one), at
   the version `scripts/codex_package/rg` pins, with PCRE2 like the vendor rg:
   `PCRE2_SYS_STATIC=1 targo --unverified install --locked --features pcre2
   ripgrep@<ver>`, from `~/.cache/codex-alab` and with its own cargo home there
   (`~/.cache/codex-alab/cargo-home`), so no repo's `.cargo/config.toml` applies: `cargo
   install` of a registry crate reads the config above its cargo home, not above the
   working directory, and the clone's private cargo home sits under the main checkout's
   `.cargo/config.toml` (the sandbox could not read it: "could not load Cargo
   configuration"; measured with the store's targo, 2026-10-16). It builds into a fresh
   hidden directory there (the only one it can write; it goes when the run
   ends, however it ends). PCRE2 is built from its vendored source and linked statically,
   never a Homebrew `libpcre2` that pkg-config finds. Its compiler is checked as in step
   3, then it moves to `~/.cache/codex-alab/rg-<ver>-<compiler id>`: a new ripgrep
   version or a new compiler builds it again. A cached rg is used only when its
   `--version` (run contained, read-only) says that version with `+pcre2` and PCRE2
   available; what it said is kept and shown when it is refused (a binary a contained step
   built never runs outside the sandbox, not even for a message).
5. **The package**: upstream's `scripts/build_codex_package.py --target aarch64-apple-darwin
   --variant codex --package-version <v> --entrypoint-bin … --code-mode-host-bin …
   --rg-bin … --strip all` stages it in a new hidden `<out>/codex/.<v>.partial.*`
   (`<out>` defaults to `~/.local/share/aterm/dev-builds/artifacts`, as aterm's
   `tools/atpkg-build-local.sh`). That directory is the only one it can write: the two
   binaries and the cached rg it reads are read-only to it, and so is every published
   package. With prebuilt bins it only stages; `xcrun strip -S -x` keeps the ad-hoc
   signature. A process the packager leaves behind could go on writing that directory (a
   symlink named `LICENSE`, say, that a later write outside the sandbox would follow), so
   nothing more is written there and it is never published: `copy_package.py` copies
   regular-file bytes and modes, opening each input relative to a held directory
   descriptor without following links, into another new hidden
   `<out>/codex/.<v>.staged.*` that no contained step is ever given. Inputs other than
   directories and regular files, unsafe modes, and files observed changing during
   copying are refused. Failed copies keep both trees; a successful copy removes the
   source only when the clone's cleanup policy permits it. The run writes into the copy
   `LICENSE` and `NOTICE` (Apache-2.0, from the commit's own objects, refused unless each is a regular file
   there) and `alab-build.json`: `source` (the commit), `version`, `client_version` (step 1;
   absent for a commit from before `alab/client-version`), `baseline` (the compound
   version's upstream baseline, `X.Y.Z`, which must be the version's own `+codex.<X.Y.Z>`
   and `upstream`'s release; absent with a version that names none), `upstream` (step 1's
   base: `release`, the stable's tag such as `rust-v0.163.0`; `commit`, the 40-hex upstream
   commit merged; `commit_date`, its committer date as UTC ISO-8601), `target`, `compiler`
   (the trusted compiler of step 3: `rustc`, the full `rustc_vV`, `sysroot`,
   `binaries_sha256` and `id`; native source-lock admission is checked independently), `ripgrep` and `ripgrep_features`, `rusty_v8`, `packager`
   (the commit of the `alab/package.sh` that ran) and `built_at`. A package without
   `upstream` names no base: a snapshot's, or one packaged before 2026-10-21; a reader
   takes the block as optional. The writer refuses one of the two values without the other.
   `alab/tests/test_upstream_base.py` checks the block and its absence, the line
   against a scratch upstream, and step 6's hold on `bin/codex`.
6. **A smoke** of the copy, with a scratch `CODEX_HOME`, each step contained, the copy
   read-only to it (`--read`) and able to write only that `CODEX_HOME`: `bin/codex --version` says `codex-cli <v>`;
   new-capable sources also require each shipped helper's `--version` to say its binary
   name and the same product version before any transport or telemetry starts. A shipped
   standalone executor additionally completes a bounded stdio initialize and
   environment-info exchange with a private home, retaining the separately stamped client
   version in both protocol replies. This ALab smoke preserves the client/product split;
   upstream's standard-package smoke keeps its existing version equality. These are
   independently compiled identities, not values read from `alab-build.json`. `bin/codex
   debug packaged-defaults` prints the compiled packaged-defaults layer as TOML, which the
   smoke parses to check `[features] daemon_auto_start` and `plugins` are both boolean
   `false`. This diagnostic calls the same compiled-layer selector as the config loader;
   it loads no user, project, managed or cloud configuration and resolves no authentication.
   `bin/codex` carries the upstream commit (read as bytes, so its header can name the
   base) when the commit's ALab code reads `CODEX_ALAB_UPSTREAM_COMMIT`; a commit from
   before `alab-core/src/version.rs` (packaged again with `--commit` or `--rebuild`)
   compiles none in and is held to none. `codex-path/rg --version` says `+pcre2` and
   PCRE2 available; `codesign --verify --strict` passes, `file` says arm64 and `otool -L`
   names only `/usr/lib` and `/System/Library` libraries on each Mach-O; upstream's layout
   validator passes. Only then is the copy renamed into place (mode 0755, as a package
   made before was).
7. **It advances the selected local artifact channel**, unless `--no-promote`. The
   candidate appears only after smoke and the final rename; `promote.py` reads only it
   and the package the exact `links/codex` marker selects. Both must be admitted ALab
   packages directly under this output's `<out>/codex`, the candidate's source must be
   a strict descendant on `origin/main`, and its version must advance. The existing
   atpkg link operation waits up to 30 minutes for its store lock, then checks the expected
   checkout and prefix, so an unrelated or concurrently changed choice is preserved.
   Unsupported conditional selection fails loudly with the complete artifact kept; no legacy fallback runs.
   `alab/tests/test_promote.py` checks the producer boundary and negative controls.
   It also prints the initial link command and aterm's measure command for the build
   (`lab codex-measure --package <pkg>`, run in an aterm worktree and landed there: aterm
   reads a codex build's session record only once it has measured that client version, so
   until an aterm that did ships, the build's tabs log no turns). It keeps the newest
   `--keep` (3) packages in `<out>/codex` (`alab/prune.py`). A package in use is never
   deleted: one atpkg links as
   `codex` (its link marker `<prefix>/links/codex`, the `bin/codex` shim and the
   `agents/codex` twin, a symlink resolved and never read through, a regular file read
   only when it is a small text shim; `aterm pkg which codex`), one a running process
   executes from (`proc_pidpath` of every process, and `ps -o comm`), and one a standing
   codex-alab session record names (`resumeArgv[0]`, `exe`, `launchArgv0`, including recoverable
   `<pid>.json.<nonce>.tmp` writes and quarantines, under
   `$CODEX_HOME` and `~/.codex`: a dead process's record is a crash relaunch still owed). Paths
   are read whole, spaces and all (`~/Library/Application Support/...`). When no link source
   can be read, a marker or a record stands but cannot be (an existing empty record included),
   or no process can be listed, nothing is pruned. A truly absent sessions directory is
   ignored; a listed record disappearing before its read defers pruning for that pass,
   since a custody rename may have moved it outside the captured filenames.
   The entire census and deletion hold the same `<prefix>/store.lock` BSD flock as
   atpkg's writers, so a manual selection either precedes the census or waits until
   collection ends. A missing, unreadable, special or contended lock defers collection;
   pruning creates no store or holder record. `aterm pkg which` is an offline read-only
   verb and does not acquire that lock.
   `alab/tests/test_prune.py` checks custody and unsafe-record refusal with plain
   scratch payloads; `alab/smoke/prune_test.py` also checks actual process use against a scratch
   root, prefix and `CODEX_HOME`.

Create `<git-common-dir>/alab-preserve-builds` to preserve this clone's builds. Its
presence is the explicit policy; its contents can record the reason. All linked
worktrees share it. Package pruning then keeps every artifact, and newly started
packagers keep their worktree, scratch directories, staging evidence and rg build
intermediates. An existing rg cache is kept intact when a new private result is needed.
Contained commands also keep their private `TMPDIR` and report its path, including
when they fail or run from a neutral directory outside the driving clone.
The scheduled updater uses this policy without replacing its LaunchAgent. Unknown
policy state also preserves data. `prune.py --repo <clone> --dry-run ...` previews
normal collection when that clone has no preservation policy.

Current version-capable sources use a stable private worktree under the package cache's
`.ubd/package-sources.noindex`, keyed by the repository, compiler and target. An ordinary
clean commit update retains unchanged file times and absolute vendored paths, allowing
the shared Cargo target to reuse those inputs. The existing exclusive cache guardian
binds each generation to its directory and Git-registration identities, detached commit
and clean tracked, untracked and ignored state. A worker can request its return; only the
guardian, after its child group has retired, marks that source reusable. Unknown, dirty,
locked, replaced or crashed generations stay intact and a fresh source is used instead.
Unreceipted reservations and unknown pool entries consume the generation limit too;
failed registration cannot grow the pool indefinitely. A source overlapping the active
driver worktree is never reused, and a cache nested inside that driver falls back to
an independent fresh worktree.

Registration happens without checkout first, so its exact initial Git state is checked
before the first checkout. Later checkout accepts only its own HEAD and reflog transition;
hook-created notes, refs and audit records remain unowned.
Older receipts without this creation proof stay intact and receive a new generation.
Before compilation and return, validation checks the actual staged index and reads file
content against a fresh committed-tree index. Command-local Git settings disable
fsmonitor callbacks and untracked caches during these custody queries and removal;
normal checkout and reference hooks remain enabled. A failed admission stops compilation.
Repository-local inherited path and index redirects are removed while caller Git
configuration remains effective. The final ordinary removal also suppresses configured
clean/process filter callbacks after the last metadata check. If converted content then
looks dirty without its filter, Git refuses removal and preserves the source.

Historical sources needing manifest/lock version stamps, explicit worktree retention,
and clone preservation still use fresh private worktrees. Historical tool-only stamps
can be restored after child retirement only while their exact post-write identities and
contents match the recorded writes and their original committed blobs remain available.
Then ordinary clean Git removal reclaims that attempt. A later same-file edit, replacement
or uncertain ownership keeps the source and its registration. No forced checkout, reset,
stash, broad worktree prune or recursive removal repairs an uncertain package source.

An already running older packager can read the new pruner at its final pruning step,
but its cleanup function was loaded when it started. This policy cannot retrofit that
old function. A separately labeled copy outside its cleanup paths can preserve a
snapshot; it does not guarantee that the old run's original paths survive or that a
concurrently copied compiler cache is a consistent reusable build.

Every step that runs upstream's code, its Python included, runs under `alab_contained`
and can write only the directories named above and the clone's private cargo home ("The
gate"); no step outside the sandbox runs a binary a contained step built, or a program a
contained step's output names, or writes into a directory a contained step was given.
`alab_contained [--read <dir>]... <writable-dir>... -- <cmd>` takes read-only roots: a
`--read` directory (or file) is readable and never writable, even inside a writable one.
Outside a git checkout (the rg build runs in `~/.cache/codex-alab`) it reads no clone and
nothing of the working directory but what is named, and the `.cargo/config.toml` of every
directory above it; `package.sh` names the private cargo home for it (`ALAB_CARGO_HOME`).
In or out of one, it also reads the `.cargo/config.toml` of each directory above the
cargo home, which a contained `cargo install` reads.

Builds wait for `ALAB_MIN_FREE_GB` (35) free on the volume, checking every 5 minutes for
up to `ALAB_DISK_WAIT_MINUTES` (120), then stop. A failure (no `~/Developer`, a missing
manifest, `df` failing) stops with an `alab:` message. Packaging and scheduled local updates
also check their cache, output, worktree, Cargo-home and temporary volumes once a second.
If any has less than 20 GiB free, or its free space cannot be measured, the guardian stops
its owned process group and returns nonzero before releasing the cache lock. Below 20 GiB
it refuses to start rather than entering the 35 GiB admission wait. This does not remove
the selected package or change cleanup policy.

Sync and check commands use that same 20 GiB running floor at each Cargo
boundary, including the lockfile resolver, native helpers, lint, main tests and
pristine comparisons. They check the actual target (and Trust build child),
source/lockfile, Git metadata, Cargo/Trust homes and temporary volumes. Unit
Cargo starts in a detached command group owned by the surviving custodian, with
stdin disconnected and caller-inherited descriptors retained. Lock and lifeline
descriptors stay out of the command. A known shortage or unreadable disk
measurement returns 75 as a resource stop before result collection or a pristine
compiler comparison; the sync can continue after room is available. This does
not delete or adopt existing targets, nor guard arbitrary shell builds outside
these entry points. Descendants that deliberately escape their Cargo process
group remain outside the guardian's boundary.
The target follows `CARGO_TARGET_DIR` before `CARGO_BUILD_TARGET_DIR`;
`CARGO_BUILD_BUILD_DIR` is checked separately: relative paths follow the caller's
working directory; `{workspace-root}` follows an explicit workspace manifest
even when the caller stands in pristine; `{cargo-cache-home}` covers the actual
Cargo/Trust homes. A build-directory template this guard cannot resolve stops
before launch. With no target/build override, the macOS shim's shared intermediate
root is checked too. These checks retain the caller's output routing and caches.

Measured on the owner's Mac (2026-10-16),
packaging `main` at ccd947b69 as `0.164.0-dev.12321+gccd947b69` (`--rebuild`, beside the
first package of that commit): the whole run took 11 min 29 s, of it the release build
11 min 2 s from a cold `target/` and rg with PCRE2 14 s; the package is 312 MB (`codex`
239 MB, `codex-code-mode-host` 65 MB, `rg` 6.9 MB, no voice). The compiler was the
Trust checkout's own stage2 (`commit-hash: unknown`); `aterm pkg lineage scan` against the store's
Trust finds nothing (3 objects), but refuses that stage2 itself as a `--compiler`,
because its `-vV` names no commit.

**Why it is built this way** (settled by evidence)

- **The client version** (the final executable's compiled `CODEX_ALAB_BUILD_VERSION`, which
  every upstream reader takes; new-capable libraries keep stable Cargo versions) must be a
  pre-release above the last vendor version installed, unique per build. `0.0.0` stalled
  aterm's Codex live upgrade (retired since, aterm fc74be92fe), and the server filters
  models by `client_version`. **The version** (`codex --version`, `codex-package.json` and
  `alab-build.json`, independently checked as identical) is codex-alab's own line
  (Decision 8), in the same dev form: a bare stable
  `X.Y.Z` switches on OpenAI's daemon updater (`app-server-daemon/src/prepare_install.rs`),
  which would swap the daemon back to vendor bytes and refuses a second build with the same
  stable version. Upstream's alphas already use the `X.Y.Z-<tag>.N` shape.
- **The binary stays `codex`, in layout v1.** aterm reads a program's phase, finds the
  harness's target and lays the `agents/` twin by that name; codex, its daemon and aterm's
  harness all read `codex-package.json`, `bin/codex-code-mode-host` and `codex-path/rg`.
  Never link a bare `target/release/codex`.
- **No voice.** codex-voice-host needs gstreamer (it fails on this Mac) and refuses a
  helper stamped with another commit; the package lays no `codex-resources/voice`.
- **aarch64-apple-darwin only**, the one target Trust ships for. Other targets stay on the
  vendor lane.
- **`targo --unverified`**: the verified lane is not realistic for a 1,294-crate closure;
  run it on ALab's own `alab-*` crates.

**What aterm has for a linked build** (aterm main, 2026-10-16): a dev-linked agent program
gets its `agents/` twin (atpkg 071ebc86e2), so `aterm pkg link codex <pkgdir> bin/codex`
wins at the front of PATH inside aterm and the harness finds the linked build as the managed
`codex`; and aterm knows an ALab build by its package, not its version (`package_entrypoint`,
aterm-agent f34be7adc9: the layout and entrypoint only), so the pre-release version no
longer keeps the claim from being proven. The readers that order builds (`parse_package`,
`package_version`) still refuse a pre-release, so no aterm decision orders an ALab build
against a vendor one (CONTRACT §7). **The published lane has since shipped**: atpkg admits
the ALab package through its signed index, with build ids above all legacy vendor pins
and below the vendor build namespace. The source rule in `crates/atpkg/src/alab.rs`
keeps local links first, respects an explicit vendor choice, and otherwise follows the
ALab pin or keeps an installed ALab build.

The continuous update path has two owners. aterm's package manager discovers and verifies
published packages, installs a complete immutable package, and switches the managed launch
path atomically. A dev link remains a local choice: a selected artifact channel advances
automatically when its producer finishes a forward package in the same output root;
other development checkouts keep their chosen targets. The native codex-alab harness then notices
that its launch path names another executable. It checks at startup and every minute,
with file notifications providing earlier hints; a failed or closed watcher leaves the
minute checks running. A delayed check skips missed intervals rather than replaying them.
No updater modifies the running executable in place.

The existing idle gate decides the handoff: every loaded thread must be idle, no background
terminal or active goal may remain, the exact conversation must be resumable, and the
claim, person-input and attention checks must still permit the act. The process writes its
resumption record, including a paced action it owes, leaves through the TUI's teardown and
re-executes the managed launch path. The next image resumes the same conversation and
restores that action once. A held gate keeps the old image running. `/alab update` (also
`/alab version`) shows the running version, the installed launch target's own package
version, and the pending reason or degraded watcher. It works with the ALab interface off.
For a verified local artifact package, these commands also read the channel's fixed,
bounded regular display record without following its final symlink or waiting on a
FIFO. They show its phase, UTC report time, target source and completed exit separately
from the running image and launch target. A changed selection is labelled a historical
channel attempt, including a same-source rebuilt package. The report separately says
whether the recorded target source matches the current launch source; a match does not
establish that this attempt installed it or that the running session adopted it.
An incomplete record always
says completion was not recorded and current activity is unverified; an old timestamp
is not evidence of an alive, stuck or latest updater. Missing, malformed or unsupported
records are unavailable. This read changes no update, policy, selection or handoff state.

The Claude Code harness lives in aterm's image, so its implementation updates through
aterm's own seamless handoff. Claude Code's vendor executable has its separate package
watch and guarded idle restart in aterm; it does not use codex-alab's re-exec protocol.

**Decisions** (the owner, 2026-10-16: "you decide")

These record the initial rollout. The managed atpkg lane described above has superseded
the local-only packaging and future published-lane plans in Decisions 3 and 6.

1. Today's request supersedes the 2026-09-30 retirement of the fork RFC
   (`docs/RFC-embedded-agents-2026-08-15.md`, "acx"): for an agent ALab builds from source,
   the agent's own source is part of aterm's layer. aterm records it in its own docs.
2. Versions follow the publication scheme (`~/publication/VERSIONING.md`): a dev build is
   `MAJOR.MINOR.0-dev.<count>+g<short-sha>` (from the version ledger on,
   `MAJOR.MINOR.0-dev.<count>+codex.<baseline>.g<short-sha>`, Decision 8). Its client version's line is at least the next
   minor above the newest upstream stable `main` has merged, so it sorts above every vendor
   build the server or atpkg has seen, and codex's update check cannot parse it (it never
   offers OpenAI's build over it). Both versions are compiled into the final executables at
   packaging, never stamped on `main` (Decision 8 says where each line lives).
3. Publication: the owner made the public `alabsystems/codex-alab` (2026-10-16), so
   codex-alab is published through the publication engine ("Publication" below). Packaging
   stays local (`aterm pkg link codex <pkgdir>`, no credential).
4. Ship from `main` (alpha-equivalent, daily).
5. The binary stays named `codex` (aterm identifies agents by name). The launch banner
   says "ALab build of OpenAI Codex"; the build is for the owner's own machines.
   (2026-10-21, owner: "in the codex-alab banner, advertise bloatware free, nag free,
   optimized for aterm", and "can we advertise any metrics? faster? less memory?": the
   banner now carries that tagline, the upstream release it is based on ("based on OpenAI
   Codex <release>"), and only the claims the 2026-10-20 head-to-head against
   vendor 0.163.0 backed, from one table, `codex-rs/alab-tui/src/banner/claims.rs`, whose
   rows hold both numbers and the date. "Faster" is not one: cold starts measured the same.)
   The composer uses the same readable build numbers and a small ALab toggle on its rainbow
   rule; exact source identities remain in `/alab version`. Its former kitty harness badge
   renderer has no callers and is removed. OpenAI attribution stays plain and secondary to
   codex-alab, following [OpenAI's design guidelines](https://openai.com/brand/); the rainbow
   treatment belongs to ALab, not an altered OpenAI logo.
6. Updates: the local link first; a roster-signed head beside the vendor lane later
   (owner rule R1), which needs the owner's release credential to publish.
7. No `rust-toolchain.toml` hook: `targo` ignores the stock pin.
8. **Two versions: codex-alab's own line, and the client line** (amended 2026-10-24; the
   original ruling of 2026-10-16 follows, superseded where this says so). **codex-alab's
   version is compound** (amended again 2026-10-24, below first).

   **The compound version.** The owner, 2026-10-24, after the ruling that restarted the line
   at `0.1.0`: "codex-alab does need to be associated with the baseline upstream codex
   version though"; "so it's a compound number."; "that mapping wont change but it's possible
   that there are multiple codex-alab versions based on the same version of codex". Decided
   from that (by the agent the owner delegated to, fail-closed and reversible):

   - **The form.** codex-alab's own core with the upstream Codex baseline in SemVer build
     metadata: a release `<VERSION>+codex.<baseline>` (`0.1.0+codex.0.164.0`), a dev build
     `<VERSION>-dev.<count>+codex.<baseline>.g<sha9>`. The baseline is the newest upstream
     stable tag `rust-vX.Y.Z` (no pre-release) whose branch point the commit has merged, with
     its real patch number: the same stable step 1 names as the upstream base and the build
     compiles in as `CODEX_ALAB_UPSTREAM_VERSION`. It is what codex-alab says it is
     (`codex --version`, `/alab version`, the session record, `codex-package.json`,
     `alab-build.json`'s `version`, the package directory, atpkg's pkg record); the banner
     and the composer show the readable `0.1.0-dev.<count>` beside "based on OpenAI Codex
     <baseline>", as they did. `alab-build.json` records `baseline` too. The client version,
     the user agent and every other server-facing reader are unchanged (still
     `<client line>-dev.<count>+g<sha9>`), and so is the `codex-alab@<pid>` claim.
   - **Why build metadata**, not a pre-release or the patch slot. SemVer and Cargo ignore
     build metadata for precedence and for matching (`=0.1.0` matches `0.1.0+codex.0.164.0`),
     so the version still orders by codex-alab's own line and `pub version`/`pub bump` keep
     reading and writing a plain root `VERSION` (`0.1.0`). A pre-release (`0.1.0-codex.164`)
     would sort below `0.1.0` and make every requirement opt in; a patch-slot encoding
     (`0.1.164`) breaks the publication scheme's always-`0` patch and would read a new
     baseline as a newer codex-alab. Measured against aterm's readers (aterm
     `atpkg::alab::AlabVersion`, its tests `a_codex_alab_version_orders_by_its_own_line_never_by_its_baseline`
     and `codex_alab_s_compound_version_packs_naming_its_upstream_baseline`): atpkg orders
     builds by signed build number and shows the version verbatim, `atpkg-publish
     pack-package` reads the compound dev form and refuses a package whose
     `alab-build.json` names another baseline, and aterm-agent's `Version::parse` refuses
     it as it refused the dev form (no order guessed). The dev form keeps its `-dev` for the
     reason below: a bare stable switches on OpenAI's daemon updater.
   - **Landing order.** aterm accepted the compound form first: the installed aterm 0.133.0
     (d05c8ca8869d) already reads it in every client reader (verbatim, or not ordered at
     all), and the one reader that refused it, the producer `atpkg-publish pack-package`
     (run from an aterm tree on a signing-roster Mac), was fixed on aterm main (aterm
     67663b9361, landed 2026-10-24) before this packager could emit it. An `atpkg-publish`
     built from an older tree refuses a compound package (nothing is signed): rebuild it from
     aterm main.
   - **The ledger**, `alab/versions.toml`: one row per codex-alab version recorded for
     release (`version`, `baseline`, `baseline_commit` the commit `rust-v<baseline>` names,
     `date` the UTC day recorded), in a strict TOML grammar `alab/versions.py` reads (no TOML
     library: it runs under macOS's `/usr/bin/python3`). Several versions may share one
     baseline; one version never has two. It began empty: no release of codex-alab's own
     line (`0.1.0` on) had been published, and public `v0.164.0` (upstream's numbering, the
     2026-10-16 ruling) predates it and has no row; no history is invented. A release
     appends its row in the commit it releases: `alab/package.sh --ledger-row >>
     alab/versions.toml`, commit, push, then `pub promote`.
   - **The gate** (`package.sh --release-check` and `--release-version`, which
     `publish/pre-promote` runs before any public write, and `--ledger-row`;
     `alab/versions.py check` and `row`) refuses:
     (a) a recorded row that changed or was removed, read across every version of the ledger
     in the history of the release commit and of `origin/main`. Three records keep that
     history honest, since the dev repo carries no release tags (a mirror-head promote tags
     only the public snapshot; measured 2026-10-24: dev origin has no `v*` tag after public
     `v0.164.0`). First, `origin/main` is fetched fast-forward only before the check, a
     missing `origin` refuses, and the release commit must be on it. Second, every commit
     this clone's reflog says `origin/main` once named must still be in its history. Third,
     the public release repository: each release on it (its `main` and every `v<X.Y.Z>`
     tag) is a parentless snapshot whose message names the dev commit it was cut from
     (`snapshot: <date> from codex-alab@<commit>`, the engine's mirror-head form). That
     commit must be on `origin/main`, and the snapshot's `alab/versions.toml` must be byte
     for byte that commit's. A rewritten `main` that dropped a released commit is refused
     however its own history reads, in any clone, fresh ones included. Git must hold the
     history whole: a shallow clone cut below a commit with parents, or a grafts file,
     refuses, and replacement objects are ignored.
     (b) a version whose row names another baseline than the commit is built on.
     (c) a version that is not above every other recorded version (its row must be the last).
     (d) a missing or unparseable baseline: no row for the version, no ledger at a commit of
     codex-alab's own line, a snapshot with no history, a malformed ledger.
     (e) a baseline older than the previous row's (rows run in strictly increasing version
     order with non-decreasing baselines and dates).
     The ledger is append-only along that history, not only at the release commit. A row
     that a commit's parent never recorded (in that parent's ledger or any before it,
     whatever was later removed or deleted) is above every version that parent recorded, so
     no row is inserted below a recorded one, directly, across a deleted ledger, or through
     a merged branch. A row first recorded at a commit names that commit's own `VERSION` and
     a baseline whose branch point that commit has merged: what `--ledger-row` prints there.
     Every row's `baseline_commit` must be the commit `rust-v<baseline>` names here, the rows
     of other versions included.
   - **Refinements, each the fail-closed choice an owner ruling could relax.** A historical
     ledger that does not parse refuses every later release (its rows cannot be told apart
     from a change). A row once changed refuses for good, even restored, because that
     version once read two ways; a row removed and restored exactly is accepted, because
     its mapping never had another value. A row recorded but never released (the baseline
     moved before the promote) stays: that version is never reused, and the next release
     takes the next version (`pub bump`). (e) is ours, not the owner's: upstream is only
     ever merged forward, so a backward baseline means a release leaf off an older stable,
     which this refuses until a ruling says otherwise. A dev package whose baseline moved past
     its `VERSION`'s row is still packaged, naming its own baseline (packaging never waits on
     a bump), and the run says the bump is due. Amended 2026-10-24 after an adversarial
     review of the first version of this gate:
     - The release record is the public repository, not dev tags: `ALAB_RELEASED_REPO`
       (default `https://github.com/alabsystems/codex-alab.git`) names it for a person's
       run, and `publish/pre-promote` sets it from the engine's `PUB_REPO_SLUG`, which
       outranks it. A release record that cannot be read refuses (network included), as
       does a release on it whose message names no dev commit, or names one this clone does
       not have. A changed snapshot format or publication mode refuses until this gate
       learns it.
     - Upstream moving or deleting a stable tag a row names refuses every later release
       (every row is re-checked each time), and a person decides, as for any upstream
       rewrite.
     - A new row is checked against the branch points its recording commit has merged, never
       against "the newest stable" as of that commit: upstream tags a branch point 8 hours to
       7 days after it, so the newest stable a commit has merged can rise after the fact, and
       an exact check would refuse a truthful row later.
     - A clone that once saw `origin/main` at a commit main no longer holds refuses every
       release (a rewrite before any release can drop a recorded but unreleased row that no
       published record holds). Once a person has decided, `git reflog expire --expire=now
       refs/remotes/origin/main` forgets it. What stays open: a fresh clone after such a
       pre-release rewrite cannot see the dropped row. That case rests on `main` never being
       rewritten (AGENTS.md), which GitHub enforces since 2026-10-24: ruleset 25803575
       ("main: no force-push, no deletion") refuses a non-fast-forward push to `main` and
       its deletion. Reversible with `gh api -X DELETE
       repos/<dev-owner>/codex-alab/rulesets/25803575`.
     - Releases are cut from `origin/main` only (mirror-head promotes its head), so a
       release commit off `main` is refused even where its ledger would otherwise pass;
       `--ledger-row` may run on an unpushed commit (its row is checked again at release).
     - A shallow clone of a published snapshot (one parentless commit) cuts nothing, so it
       is still read; a depth-1 clone of the dev `main` counts one commit and would read as
       a snapshot, so it refuses.
     - `publish/pre-promote` refuses a checkout whose gate files differ from the source
       commit's, so the gate that decides a release is always the one that release carries.
   - **Reversible.** Deleting `alab/versions.toml` returns packaging to
     `<VERSION>-dev.<count>+g<sha9>`; releases of codex-alab's own line are then refused
     until it returns.

   The first amendment, 2026-10-24:

   The owner, 2026-10-24: "we need every harness (like codex-alab) to be in their own repos
   with their own versions starting from v0.1.0 written in Rust and distributed via atpkg".
   The 2026-10-16 ruling below made the root `VERSION` upstream's numbering (0.164.0) so a
   build would never call itself older than the upstream code it holds, because OpenAI's
   server picks models by `client_version`. Both needs hold at once once they are two
   versions with two names (decided by the agent the owner delegated to, fail-closed and
   reversible):

   - **The version** is codex-alab's own release line: the root `VERSION`, restarted at
     `0.1.0` (`MAJOR.MINOR.0`, the patch slot always `0`) and changed only by `pub bump
     codex-alab`. It is what codex-alab reports as itself: `codex --version` (and each
     subcommand's), the session record, the banner and `/alab version`, `codex exec`'s
     opening line (`codex-alab v<version> · based on OpenAI Codex <release>`; until the review
     of 2026-10-24 it still said `OpenAI Codex v<client version>`), `codex-package.json`
     and `alab-build.json`'s `version`, the package directory, the atpkg pkg record's
     `version`, and the public release tag `v<VERSION>` from `v0.1.0` on. A package's dev
     form is `<VERSION>-dev.<count>+g<sha9>`: the `-dev.<count>` the publication scheme
     gives builds that must sort, which atpkg-publish's `pack-package` requires
     (`dev_version`, aterm `crates/atpkg-publish/src/artifact/package.rs`) and checks against
     the source's count, and which `promote.py` orders by. Chosen over the bare
     `0.1.0+g<sha9>` because that would break the released pair (an atpkg-publish that
     refuses it is what signs packs today) and lose the order.
   - **The client version** is upstream's numbering, separately named: the committed
     `alab/client-version` (`MAJOR.MINOR.0`, set to the floor of the day it was introduced,
     `0.165.0`), held at or above the floor (step 1). It is compiled in as
     `CODEX_ALAB_BUILD_VERSION` (`codex_alab_core::version::running_version`), so every
     reader upstream's code has (`client_version` and the models URL, the user agent and its
     `version` header, telemetry, MCP client info, the rollout's `cli_version`, the update
     check, the daemon's app-server version) is byte for byte what it was: model selection
     is unchanged. `codex doctor`'s runtime row keeps it too, because its update rows compare
     it with OpenAI's latest. `alab-build.json` records it as `client_version`; `/alab version` shows
     it as "Codex client version". A release is refused while `alab/client-version` is below
     the floor (`publish/pre-promote`, `package.sh --release-check`), because a published
     snapshot has no history to read the floor from and builds with the committed line.
     `alab/client-version` is edited by a plain commit, never by `pub bump`, which moves
     codex-alab's own `VERSION` only.
   - **Why the old name keeps the old meaning.** `CODEX_ALAB_BUILD_VERSION` stays the client
     version and `CODEX_ALAB_PRODUCT_VERSION` is new: a historical `--commit` source given
     the new packager reads only the first, so it still tells the server what it told it
     then, and a source built without the second reports its client version as itself (one
     identity, as before). A commit with `alab/client-version` whose source cannot report
     codex-alab's version is refused before its build; a package whose `bin/codex` does not
     carry its client version is refused at the smoke.
   - **What sorts below what.** Public `v0.164.0` (2026-10-16, upstream's numbering) sorts
     above `v0.1.0`; nothing orders codex-alab by its release tag. atpkg orders builds by
     the signed build number (`YYYYMMDDNN`), `promote.py` by `<count>` along `origin/main`,
     and aterm reads a codex-alab version verbatim (atpkg) or not at all (aterm's
     `Version::parse` refuses a pre-release, CONTRACT §7). The aterm side was read for every
     codex version reader (`atpkg-publish` `programs.rs` and `artifact/package.rs`, `atpkg`
     `alab.rs`, `aterm-agent` `codex_alab.rs`, `codex_steps.rs` and `codex_tap.rs`,
     `aterm-phase` `harness_version.rs`): each accepts `0.1.0-dev.<count>+g<sha9>` as it is,
     `harness_version.rs`'s two-part parser reads only aterm's compiled harness versions,
     and the rollout builds aterm's tap measures are client versions, which this leaves
     unchanged. [Corrected after the review of 2026-10-24: the tap also compared those
     builds with the build the running package names, which aterm's `alab_build` read from
     `codex-package.json`'s `version`, codex-alab's own line, so no build of the line could
     ever be tapped. aterm 70c9edf195 names that build by `alab-build.json`'s
     `client_version` where a package records one, and by the manifest's `version` where it
     records none (a package from before the line), and refuses a record whose `version`
     is not the manifest's (`codex_alab_s_own_line_names_its_build_by_its_client_version`,
     `a_codex_alab_own_line_package_is_tapped_at_its_client_version`).] aterm pins that
     acceptance in a regression test
     (`crates/atpkg-publish/tests/artifact/package.rs`,
     `codex_alab_s_own_line_packs_with_its_client_version_beside_it`, and the `0.1.0` rows of
     `the_version_form_and_the_origin_are_read_exactly`).
   - **Reversible.** Deleting `alab/client-version` and setting `VERSION` back to upstream's
     numbering restores the 2026-10-16 behaviour exactly (the one-version path is kept for
     historical commits).

   The 2026-10-16 ruling, as amended above:
   The release line is the root `VERSION` (`MAJOR.MINOR.0`), changed only by `pub bump
   codex-alab`. It is upstream's numbering, not a line of ALab's own the way orca-alab
   restarted at `0.1.0`: a codex-alab binary must never call itself older than the upstream
   code it holds (the server picks models by `client_version`): a package takes the floor
   when `VERSION` is behind it (step 1 above), and a release is refused below it
   (`publish/pre-promote`, `package.sh --release-check`, run by the engine before any public
   write). [Superseded 2026-10-24: `VERSION` is codex-alab's own line from `0.1.0`; the
   floor holds `alab/client-version` instead.] Run `pub bump` with `PUBLISH_REPO_PATH` naming a clean checkout of `origin/main`,
   as every engine verb is run, and push its commit. A public release `vX.Y.0` is a source snapshot; a package
   built from it still carries `X.Y.0-dev.<count>+g<sha>`, because a bare stable version
   switches on OpenAI's daemon updater ("Why it is built this way"). The upstream base is
   a separate fact, never the version: the stable and the openai/codex commit the release
   merged last (step 1, "The upstream base"; `package.sh --upstream-base` prints the line).
   A public snapshot has no history, so it cannot be read from the published tree: a
   package's `alab-build.json` and the running build's header carry it. The annotated tag
   `openai-codex/<upstream sha12>` this decision once named was never made (the public
   repository's tags on 2026-10-21: `v0.164.0` and `atpkg-codex-2026102001`); the release
   pages name the base instead (`publish/release-body`, "Publication").
   What keeps a codex-alab install from being "upgraded" back to OpenAI's build is not the
   version, so a line that sorts below OpenAI's latest is safe: atpkg's source rule leaves
   a dev-linked program alone and follows the signed ALab pin for a managed install. It
   keeps an installed ALab build if that pin is absent; only an explicit `--vendor`
   choice returns it to OpenAI's lane. aterm's older Codex upgrade lane (`upgrade_codex`,
   which moved only vendor bytes) is retired (aterm fc74be92fe, CONTRACT §7).
9. **The front page is ALab's.** The root `README.md` is an owned path (`owned-paths`): an
   upstream change to it never stops a sync, and upstream's copy waits in
   `alab/upstream/README.md`. Chosen over a merge attribute (a `merge=ours` driver drops
   upstream's change without saving it) and over a stage-time transform (the engine runs
   none for a mirrored repo, and the dev repo's front page should say the same thing).
10. **Toolchain and gate.** Executing `alab/check.sh` enters the shared native
   working-lock runner before the single source/pristine target inventory and
   existing protected profile. Its inner sourceable `check_main` is the testable
   gate body, not a compiler selector. The same executable is
   `TOOLCHAIN_ACCEPT_CMD_DEFAULT`, so a native adoption's working lock is the
   authority judged by acceptance. No global current or shim change is required.
   A root `rust-toolchain.toml` says `channel = "trust"` (an ALab
   path, so a sync never meets it); `codex-rs/rust-toolchain.toml` stays upstream's stock pin
   (Decision 7). The publication engine runs no gate for a mirrored repo (orca-alab's
   precedent: its `CHECK_CMD_DEFAULT` is inert), and the gate it admits for Rust needs a
   verified `targo trust check|build` step that passes. Measured 2026-10-16: `targo trust
   check -p codex-alab-core`, ALab's smallest crate (one dependency, `toml`), is
   INCONCLUSIVE on both the strict and the advisory lane (184/184 bodies covered, 0 failed,
   7 unknown from unsupported MIR); the policy's strict gate over the whole 1,294-crate
   workspace also builds voice-host, which needs gstreamer. A gate that cannot pass is not a
   gate, so `publish/config.sh` carries a presence check, labelled inert, and the evidence
   that the snapshot builds with Trust is `check.sh` (every sync builds and runs `codex`,
   `targo --unverified`) and `package.sh`'s release build: no proof claim. When alab-core's
   verified check passes, the gate becomes that check plus `targo --unverified test` of the
   `alab-*` crates and of `codex-cli --bin codex --no-run`.
11. **aterm's Codex lane is retired** (owner, 2026-10-16: "retire the codex harness"; "we
   still need codex-alab to work like codex for our purposes"; aterm main
   `28e13d46ba..83bd43c206`, CONTRACT §9 step 4). A vendor `codex` gets display only (its OSC
   7501 report, the stall row, the disk tick); aterm admits a Codex only as codex-alab and
   watches beside it, and its crash relaunch, stall remedy and memory remedy need codex-alab's
   standing proven claim. The sign-in screen of a signed-out first launch, which no aterm lane
   answers any more, raises codex-alab's keyed attention so a person is told, until the sign-in
   completes (CONTRACT §2 (a)).
12. **An off is met by its kind** (decided 2026-10-17 after the review of
   `8e834c8f6..4cdc96090`; CONTRACT §0, §2). It replaces b24e535df ("passive for any reason
   keeps the proven claim"), whose premise was false: aterm's host gives the watch-only
   worker's remedies only where its host stands, and under `agent-off` and Autopilot Off it
   runs no worker and relaunches nothing, under `disabled`, `headless` and a suspension its
   host is off, and `unhosted` there is no host. The kept claim bought nothing there and had
   side effects (`status` showed it proven under Autopilot Off, so `aterm drive task` typed
   mail into a hands-off session; other supervisors met `ERR busy`; mail receipts and reports
   ran while passive). So a transient off (a hold, `why=suspended`, a policy read that failed)
   keeps the proven claim and acts on nothing; a person's or the config's off
   (`autopilot-off`, `agent-off`, `disabled`, `headless`, `unhosted`) gives it back and takes
   it again, by the ordinary proven claim, once the policy reads `effective=on`, seen at the
   `harness-policy`, `hold` and `meta` events, after lost events, and on a 30 s clock while
   passive (aterm announces a suspension with no event, and a session may start off).
   codex-alab's own passive reasons (a point it cannot answer, a wedged loop) never gave the
   claim back and still do not. Mail receipts and reports are sent, and descriptions written,
   only while codex-alab acts; a mail turn that ends passive owes its receipt and report until
   it acts again, typed mail is read on standby too, and act records go while the claim is held
   (amended after the review of `fea034d20`).

13. **Sleep and wake** (decided 2026-10-24). A usage-limit continuation in another harness
   fired inside a 19 s dark wake with the network down. Its request then sat in backoff
   across the following sleeps until the person pressed Esc. codex-alab's waits had the
   same exposure, because they all ran on the `Instant` clock, which stops while the host
   sleeps. The fix is in `alab/CAPACITY_RECOVERY.md`, "Sleep and wake": codex-alab measures
   the kernel's count of time asleep. Waits and the deadlines that name real time count
   that sleep. After a wake the host settles for 30 s plus the session's jitter share
   before any act. A due usage resume waits for its first reach measure and never types
   into an API measured down. The choices, each reversible by reverting the commit:
   * The settle covers every typed act, not only the resume, because a dark wake with the
     network up would otherwise start a turn that the next sleep cuts. Attentions are not
     held.
   * A measure that stays down holds the resume for at most the network wall's 15 min.
     Unknown holds nothing, because a slow network is not an outage.
   * Waits are cut short and their doubling restarted on every wake, dark wakes included.
     This affects only retries inside a turn, never a new act. A retry that fails in a
     dark wake costs one refused connection.
   * A turn that was streaming when the lid closed keeps upstream's stream idle timeout.
     Cutting it at a wake would discard a live stream after a short sleep. This is the
     one open case. [Superseded the same day: the timeout counts the sleep, below.]

   **Amended 2026-10-24, after the review of `952bff3204..e7521f5010`** (decided by the
   agent the owner delegated to, fail-closed and reversible by reverting the commit):
   * **A wake is dated when the host woke**, by the kernel's record of it (macOS
     `kern.wake_abs_time`), not when a reading noticed it. Nothing reads while a turn
     streams or the session stands idle, so dating the wake at the first reading held every
     act up to 90 s after a wake many minutes old, saying "the host just woke". A record
     from before the reading before is not believed (that reading found the host awake),
     and where the kernel keeps none (Linux) the reading's time stands in: the latest the
     wake can have been, so the settle errs long, never short. The module's claim of a
     per-thread poll, which never existed, is corrected.
   * **Every retry wait inside a turn counts the sleep**, not only the connection wait:
     core's bounded backoff and the `Retry-After` a transport switch honours go through the
     same wait (`wait_until`, two `ALAB(wake)` hooks in `core/src/responses_retry.rs`, which
     `4ba19cf61` landed the same day; this review's identical hooks gave way to it). A
     server's `Retry-After` is still never cut short in real time.
   * **A stream's idle timeout counts the sleep** (`wake::timeout`, two `ALAB(wake)` hooks in
     `codex-api`): the timeout is upstream's 300 s, measured in real time. The objection
     above held only for cutting a stream at every wake; counting the sleep toward the
     timeout gives up only a stream idle for longer than its timeout in real time, which on
     a host that never slept upstream gives up too. A short sleep shortens the timeout by the
     sleep and discards nothing; a sleep longer than the rest of it gives the stream up once
     the host has stayed awake `WAKE_SETTLE` (30 s) after the wake, each wake settling
     again, and the turn's ordinary retry, now counting the sleep as well, takes over. The
     settle and not the retries' 2 s, because giving a stream up sends a new request, which
     in a dark wake the next sleep would cut, to be sent again at every dark wake through
     the night, spending the usage the person's limits count; a refused retry costs nothing.
     No wait inside a turn is left that a person must press Esc to end.

### Publication

`~/publication` publishes codex-alab as `mode = "mirror-head"`, orca-alab's mode: the
public `alabsystems/codex-alab` is one snapshot commit of the dev repo's `origin/main` per
release, minus the paths no ALab repo exports (`docs/`, `publish/`, `.github/`), with the
dev org rewritten and the engine's guards run on exactly the bytes it pushes. `pub stage
codex-alab` puts the snapshot in the private staging repo; only `pub promote`, on the owner's
direction, writes the public one. That rule is for the source snapshot. A package's
`atpkg-codex-<build>` prerelease on the public repository is a different act: aterm's daily
publish writes it with `atpkg-publish mirror`, just before the index that pins it. The agent
running that publish performs the step on a rostered Mac that holds the release credential
(aterm `docs/RUNBOOK-atpkg-republish.md`, "codex-alab", step 4). `publish/` holds the repo's
policy for the snapshot.

`publish/forbidden-extra.txt` adds the owner's account, the second machine's account, the
name run together and the machines' host names to the central patterns (none is in the tree,
upstream's files included); `publish/pre-promote` refuses a release whose
`alab/client-version` is below the upstream floor, or whose `VERSION` (codex-alab's own
line, the tag the engine cuts) is not the one the engine releases, or whose version ledger
(`alab/versions.toml`) does not map that `VERSION`, unchanged and above every other row, to
the upstream baseline the commit is built on. It checks the ledger along `origin/main`
(fetched fast-forward only) and against every release the public repository the engine
names (`PUB_REPO_SLUG`) already holds, so a `main` rewritten after a release is never
released from. It refuses unless the checkout it runs in carries the source commit's own
gate (`alab/package.sh`, `lib.sh`, `command_guard.py`, `github.py`, `versions.py`): the engine runs the
hook from the attested commit but in the operator's checkout, which it never checks out, so
an older checkout would otherwise judge the release with an older gate. It prints the
compound version the release is (`0.1.0+codex.0.164.0`) (Decision 8).

**Release transport, 2026-11-06.** `alab/github.py` owns the release/version gates'
GitHub reads. Setup and the package gate call it explicitly for upstream, and
the package gate for origin;
`versions.py` uses it for published snapshots. It invokes native Agh, preserving
its `agh` argv[0], binding the selected executable for the operation, and requiring
exact anonymous batch fetch support. `ALAB_AGH=/absolute/path/to/qualified/agh` selects a retained
client during rollout. A missing capability refuses before transport; no token,
credential helper, PATH shim or private publication-module dependency is added.
Upstream and public snapshot reads are explicitly anonymous; a quota refusal is
retained without changing identity. Origin uses Agh's registered owner route.

The adapter enumerates matching refs with native `api --all` and fetches one
explicit `{source, into, expected_oid}` batch into Agh's namespace. Native Agh
checks every raw OID before atomically updating its cache refs; the adapter then
checks that same snapshot before
updating local metadata. GitHub's [matching-ref contract](https://docs.github.com/en/rest/git/refs#list-matching-references)
defines all prefix matches; the native qualification observed 1,489 upstream
`rust-v` refs (225 decimal version shapes, 193 eligible under the release floor's
bounded version grammar), no pagination Link, and identical included/all-page
OID sets. Historical alpha tags remain included. A cold batch may be prepared
outside the package gate's existing 120-second transport deadline. Every gate
still enumerates again; only unchanged exact cached OIDs avoid re-fetching the
same graph. Origin/upstream main remain fast-forward only, local
ref updates compare old OIDs atomically and retain reflogs, and the configured
`+` for `refs/upstream-tags/` remains local metadata policy. Public snapshot tag
OIDs are immutable and a tag changed during fetch refuses. Local fixture remotes
remain explicit file transfers, with hooks/helpers/network protocols disabled.
The reader admits at most 2,048 snapshot refs, matching the native batch contract,
and 8,192 local refs. Larger catalogs refuse explicitly. Shared local refs are
read as bounded snapshots rather than launching Git repeatedly per identity;
physical ref/reflog admission and final old-OID CAS still apply in linked
worktrees and with packed refs. Qualification caught and corrected an initial
120-second failure caused by those repeated local probes, without changing the
guardian's deadline.
The 1,490-tag source-signing retry exposed another per-ref cost: launching
`check-ref-format` separately for each observed and mapped name. Full names now
go through Git's own parser in NUL-delimited `update-ref --stdin -z` batches
that end with `abort`, without preparing any update. This retains Git's syntax
rules without a second parser or per-tag processes. The large warm-snapshot
regression bounds total Git calls, while malformed-name, command-injection,
unchanged-ref/reflog and lock-cleanup checks exercise the aborted transaction.
The existing floor, ledger, complete-history and source-identity checks still
decide release eligibility; transport readiness alone is not release readiness.

A release's GitHub page names its upstream base. The engine writes a source release's
body itself (`bin/pub-sign-release`, `release_body`): the released tree's
`release-notes/v<version>.md`, which a mirror-head release does not have (the engine writes
`release-notes/` into a staged export only, and the path is the engine's), then what the
source commit's own `publish/release-body` prints, then its verification paragraph
(~/publication RELEASE.md, "The repo's own line"). That hook prints `alab/package.sh
--upstream-base --commit <source>` (`Based on upstream openai/codex rust-v0.163.0 + 241
commits (e8aa7c1678ca, 2026-10-20)` for `origin/main` on 2026-10-21); the engine runs it
only when it creates the release, screens what it prints with its guards, and refuses the
create when it fails, so a release is never made without the line its commit asks for. A
package's own release (`atpkg-codex-<build>`) carries the same line, which aterm's
`atpkg-publish` reads off the package's `alab-build.json`: on the staging page that
`pack-package --upload` writes, and on the public page the mirror creates, read off the
archive as a client stages it. A package that records no count says the release and the
commit without it, and one that records no `upstream` (or a malformed one) says none.
`v0.164.0` and `atpkg-codex-2026102001` were published before this and name none: the
engine never rewrites a published page.

**The first compound package on the channel.** `atpkg-codex-2026102501` is
`0.1.0-dev.13633+codex.0.164.0.g9fef22351`: source `9fef22351`, built by the store's Trust
15698 (`20565c325`). Its page names `rust-v0.164.0 + 77 commits`. It is the first package
of codex-alab's own `0.1.0` line and the first with a compound version. atpkg index 77
pinned it on 2026-10-25, signed by roster machine `incumbent-head` on m21.
`atpkg-codex-2026102201` (`0.164.0-dev.12972`) was staged on 2026-10-22 and never pinned.
The package went through aterm's daily publish (`docs/RUNBOOK-atpkg-republish.md`,
"codex-alab"), which now has two more steps:

* `alab/check.sh` runs on the packaged commit before `pack-package`. On `9fef22351` it
  exited 0: 48 libtest suites, 13,349 tests passed, 0 failed, and tippy found nothing in
  ALab's code (2 upstream diagnostics set aside).
* A canary installs the staged pack on the packing Mac before the index publishes.

The RUNBOOK records the signing choices. It also records why step 4, the index, is the
agent's act rather than a wait for the owner, citing the owner's rulings of 2026-10-02 and
2026-10-16. Choices made for this package:

* The package and the gate name one commit: the `origin/main` tip when the work began.
  `main` landed 48 commits before both finished, and none of them was packaged.
* The build id is the UTC date.
* The first release build stopped with `trustc ... (signal: 5, SIGTRAP)` and no diagnostic.
  It was run again once, at the same commit, and passed. The crash report
  (`~/Library/Logs/DiagnosticReports/targo-*.ips`) showed the trap in the child that
  `targo` forked to exec `trustc`, before the exec: macOS 27.0.1's libmalloc
  (`_malloc_fork_child`, then `_xzm_foreach_lock`). The compiler never ran.

The guards refuse upstream's tree as it ships (measured 2026-10-16 on `origin/main`, the
engine's guards as of this change, which now also scan the plaintext of the two `.json.zst`
schema bundles and run the Cargo closure check from `codex-rs/` and
`tools/argument-comment-lint/`, both clean): the central forbidden-content baseline matches
233 lines in 59 upstream files, and Gitleaks reports 19 findings in 16 files, 18 in 15 after
the engine clears a lockfile dependency name (17 test strings and an FNV hash constant).
Of the 59, 45 carry machine paths such as `/Users/<name>/...` in test data and snapshots:
the baseline has no exception path for those, by design. The other 14 carry only
credential-shaped test strings (`ghp_…`, `AKIA…`, `sk-…`, PEM private keys), which the
inline `gitleaks:allow` tag clears for both scanners, line by line (the engine honours it in
the baseline's credential section and in Gitleaks; nowhere else). Either way it means
editing upstream's test data, the kind of layer the layering law forbids on `main`: it waits
for the owner (export-time transforms for a mirrored repo, which the engine does not run
today, or carrying the rewrite and the tags here). ALab's own files are clean.

### Parallel login smoke

Run a canonical package with independent credentials and shared conversation storage:

```sh
python3 alab/smoke/parallel_logins.py \
  --package /absolute/path/to/package \
  --output /new/retained-output --count 40
```

The loopback HTTPS server holds responses until every account has a request in
flight. It checks each request's credential, account header and original task;
the runner checks distinct conversation IDs and unchanged credential files.
All credentials are synthetic. Each exec process uses the existing command
guardian, and the retained result names the package source and executable hash.

### Headless capacity and quota recovery

For retry behavior and public, selected and running build identities, see
[`CAPACITY_RECOVERY.md`](CAPACITY_RECOVERY.md).

aterm's lab is the off-network proof of a package's headless capacity and account-quota
recovery: `targo --unverified run -p aterm-lab -- codex-alab-proof --capacity-only --package
/absolute/path/to/package` in an aterm checkout (aterm-lab's jailed capacity lane,
`crates/aterm-lab/src/codex_alab_capacity_proof.rs`, which the full `codex-alab-proof` also
runs as its last leg). It builds nothing. It runs the package's `codex exec` under the lab's
`sandbox-exec` profile, which allows one loopback port (the lane's own Responses model) and
no other network, DNS included, and fences the owner's homes (no writes there, their
credential files unreadable). Each case has fresh homes, a scrubbed environment and a config
whose only provider is that model, with `requires_openai_auth = false` and codex's own
request and stream retries off, so the retry it times is codex-alab's recovery. No case uses
an account or spends quota. It checks the package's records (version, target, exact source
commit, `bin/codex` inside the package), that `exec --help` names the flags it passes and that
`--version` says the package's version, then runs ten cases and keeps each one's command,
output, requests, socket log and verdict:

- `capacity-recovery`: a completed `update_plan` call survives a 503
  `server_is_overloaded` with a two-second `Retry-After`, and the same turn recovers no
  sooner than `Retry-After` after the answer's headers began, the plan run once and its output
  kept unchanged in the retry;
- `quota-reset-recovery`: the same across a 429 `usage_limit_reached` naming `resets_at`
  (rounded up to the second, a second or more past `Retry-After`, so a retry that kept
  `Retry-After` alone would come before it), the retry no sooner than that reset either;
- `quota-no-reset-recovery`: the same across that 429 with no `resets_at`, the retry no
  sooner than 30 seconds after the answer's headers;
- `permanent-refusal`, `quota-permanent-refusal` and `quota-no-reset-refusal`: a 403 with
  the overloaded body, or the quota body with or without its reset, fails the turn after one
  request, with a non-zero exit and no signal;
- `quota-terminal-<kind>`: a 429 with the quota body, a reset and
  `x-codex-rate-limit-reached-type` one of the four credit-depletion and spending-cap types
  fails the same way, the misleading reset notwithstanding.

`codex exec --json` can emit a nonfatal `error` retry notice while the same turn is still
running, so the lane reads each outcome from `turn.completed` or `turn.failed` and the exit
status. It witnesses egress: before each case a jailed `nc` reaches the model's port and is
refused a loopback canary port, whose listener takes nothing, and while `codex exec` runs its
process tree's sockets are sampled with `lsof` on a clock and at each request the model takes
(codex waits on that connection then): no socket off loopback, the sample at each request
seeing `codex` connected to the model's port, and the canary untouched after the run. `codex`
leads a process group of its own, which the lane kills, before it reaps the leader, once
`codex` exits, at a hang bound one minute past what the case waits by design, or at an
interrupt of the lab, as `command_guard.py` stops a command's group.

That lane replaced `smoke/capacity_exec.py`'s standalone run, which ran the same ten cases
with the Internet open. `capacity_exec.py` stays as the loopback Responses model, command
guardian runner and package reader the smokes below import.

`alab/smoke/quota_resume.py --codex /path/to/codex --app-server
/path/to/codex-app-server --output /new/retained-output` seeds real persisted
threads under CLI and VSCode origins, then resumes them through `exec` across
quota errors. Its reverse control resumes an Exec-origin thread on a non-Exec
host and keeps the normal terminal policy. It verifies the saved origin is not
rewritten; the seed, resume and final read all use the production app-server API
inside `capacity_exec.py`'s bounded command guardians.

`alab/smoke/quota_wait.py --codex /path/to/codex --output /new/retained-output`
checks four consecutive stale reset responses before success, retained request
input and growing positive backoff. It also sends native SIGINT during a
one-hour quota wait and checks a handled exit within ten seconds, no additional
HTTP request, and no guardian timeout. It builds nothing and uses isolated homes
and a local server, with raw process evidence retained alongside each result.

For the account boundary, run each of these two smokes through `alab_contained` from a
neutral directory, granting the package, script and private Cargo home read-only, with only
the new output directory and the helper's private scratch writable (`lib.sh`). That policy
denies account files and services but permits Internet access; each smoke configures only
the loopback provider. Retain the exact containment invocation alongside its results.

### The drop-in check

codex-alab must stay a drop-in `codex`: the same binary name, CLI, `CODEX_HOME`, config and
auth files, `exec`, `app-server`, MCP and SDK wire. `alab/smoke/dropin.py` measures that
against OpenAI's build, both run with a scratch `HOME`, `CODEX_HOME` and `XDG_RUNTIME_DIR` and
no aterm in the environment, and exits non-zero on any difference it cannot attribute. It
runs two packages and builds nothing (defaults: the newest `package.sh` output and the newest
vendor build in atpkg's store, `codex-old`'s). Each flag is compared with its short form,
whether it takes a value and its possible values, and each page's positional arguments as
written; a short flag, value or argument OpenAI's build has and codex-alab lacks fails unless
upstream's source no longer names it, and a clone with no `upstream/main` (before
`alab/setup.sh`) fails rather than excuse every removal. The files `exec` leaves in `CODEX_HOME` compare codex-alab's own
names there (`alab-core/src/home.rs`: `skills/.alab-system`, `models_cache.alab.*`) as the
vendor's (`skills/.system`, `models_cache.*`), and a file codex-alab writes under one of the
vendor's own names fails. Run 2026-10-23, codex-alab `0.165.0-dev.13344+gf02eae4fc` against
OpenAI's `0.164.0` (`codex-old`):

| check | result |
| --- | --- |
| binary | both `bin/codex` in layout v1, `codex-cli <version>` |
| `--help` tree | 76 pages; the one difference, `debug packaged-defaults`, is ALab's (a marked clap command declaration) |
| `exec --experimental-json` (the TypeScript SDK's argv) | both: a turn against a local mock model, `thread.started` to `turn.completed`; the same 20 event key paths |
| `app-server --listen stdio://` (the Python SDK's argv) | both answer `initialize` |
| `app-server generate-json-schema --experimental` | 22,116 definitions, properties and enum values in both; 50 only in codex-alab, every one in upstream's protocol crate at the merge-base (398500106); none only in OpenAI's build |
| effective config (`config/read`), outside aterm and with `TERM_PROGRAM=aterm` | codex-alab with no `config.toml` reads what OpenAI's build reads with ALab's defaults as its `config.toml`: 19 ALab defaults outside aterm, 22 inside; the only other differences, `tui.alab_interface` (and inside aterm `tui.alab_progress`), are ALab's own keys, read from the layers |
| ALab defaults overridable | the six ALab feature defaults each flip back by `config.toml`, `-c features.<f>=…` and `--enable`/`--disable` |
| `mcp list --json` | both list the server `config.toml` declares |
| `CODEX_HOME` after one exec | 23 paths in both, codex-alab's own names read as the vendor's; codex-alab writes nothing OpenAI's build does not, and nothing under its names; it skips the plugin marketplace sync (2 paths) and the three removed bundled skills (37 files) |
| auth | `login --with-api-key` writes `auth.json`, `login status` reads it, in both |

Negative controls, each a wrapped package the check must fail: `-m` removed from every page,
`--model` removed in a clone with no `upstream/main`, and `--sandbox`'s `workspace-write`
renamed with `[PROMPT]` made `<PROMPT>`. Hidden subcommands and flags are not compared (`--help`
does not list them, and no SDK passes one).

### Runtime behavior: what codex-alab changes, and how

Every ALab behavior holds only in the `codex` binary: its `main` calls
`codex_alab_core::activate()` first (`ALAB(core)`), and every hook asks
`codex_alab_core::active()`. Upstream's unit tests never run that `main`, in any crate, so
they keep testing upstream's behavior and the gate keeps gating on them. ALab's own tests
switch it on themselves. (A first cut that edited `config/defaults.toml` and keyed hooks on
`cfg(test)` failed the gate: the defaults file is compiled into every crate, so upstream's
tests in `codex-tui` and `codex-app-server` saw ALab's values.)

**Config.** `codex-rs/alab-core/defaults.toml` is merged, table by table, over upstream's
`config/defaults.toml` in the packaged-defaults layer (`ALAB(defaults)` in the loader), the
lowest layer: any user, profile, project or `-c` value overrides each key. It turns off the
update check, product analytics and Statsig metrics, `/feedback` uploads, the shared daemon,
ChatGPT apps and the plugin marketplace, the skill-search shadow experiment, the image tool,
tool-install suggestions, tips (and the desktop-app ad), the server-version notice and
automatic recaps and desktop notifications; it caps `history.jsonl` at 8 MiB, and picks a status line with context and
both usage windows. Inside aterm only (`TERM_PROGRAM=aterm`), `alab-core/aterm-defaults.toml`
is merged over that: no approval prompts or execution sandbox by default, OSC 9 for explicitly enabled notifications,
no paste-burst detection (aterm's typed input arrives as a burst and must submit), and `tui.alab_progress`,
the OSC 9;4 progress bar (ALab's own key: upstream's typed config, and so `config/read`, carries
no `alab_*` key, and `smoke/dropin.py` names it ALab's own; vendor codex's `--strict-config`
refuses it where a person writes it). The permission defaults are native to codex-alab,
so direct launches and older resume/fork wrappers get them too; explicit user, profile,
project and CLI permission settings still win. Local aterm resumes send this effective
permission configuration rather than retaining a saved session's older permission mode.
Native defaults do not count as explicit launch overrides for remote resume or fork.
In another terminal none of these defaults is
set, so codex behaves there as upstream's does: it picks the notification method
the terminal supports, holds an unbracketed multi-line paste as one paste, writes no pet
sheet or frames into `~/.codex` for a pet nothing draws, and writes no progress bar. `alab-tui`'s tests
check each key through `ConfigBuilder` (an upstream rename would otherwise turn it into a
silent no-op) and that a user's config still wins.

The composer keeps its spectrum rule and identifies `codex-alab` and its OpenAI Codex
source separately. Packaging supplies the included upstream release baseline and exact
source commit before capturing ALab's package version in the final executables; an ordinary unstamped build says
`dev` / `unreleased`. Launch and `/status` cards also show both identities. The names use
their normal spelling and neutral terminal text, following [OpenAI's brand guidance](https://openai.com/brand/).
The footer omits routine activity and default permission labels. Its weekly meter shows
reported usage, the reset time, earned reset-credit dots and an inline warning during a
known credit's last 24 hours. Click the account or quota to inspect reset details and the
larger usage graph. The graph keeps account-scoped observations under `CODEX_HOME/alab/quota`;
unobserved periods remain gaps. A projection is labelled as an estimate, needs at least
three readings spanning 30 minutes, and disappears when its latest reading is over
30 minutes old. No pet consumes prompt space until the person selects one in `/pets`.

Accepted observations stay in the account generation's memory during cache contention or
storage errors and are saved when storage recovers. Reset and downward-correction boundaries
remain separate; conflicting historical corrections within the same second discard the
ambiguous anchor and older forecast segment. Exiting before a successful save can lose
unwritten observations.

**Hooks** (one line each; what config cannot reach):

| hook | what it stops |
| --- | --- |
| `tui/src/tooltips.rs` | the announcement feed GET at every launch (made even with tips hidden) |
| `tui/src/lib.rs` (default-socket probe) | attaching to any running shared daemon, a vendor one included (TUI, `codex archive/delete/queue`) |
| `tui/src/startup_orchestration.rs` | the vendor daemon's superseded package copies (`$CODEX_HOME/packages/app-server-daemon/releases`, about 316 MB each, pruned only when the vendor's own daemon or installer runs, which codex-alab never does): with the daemon off by the effective config, a TUI start removes them on a thread of its own (`alab-tui/src/daemon_packages.rs`). It keeps the release `current` names (a vendor codex still finds its daemon) and the ones `selection-history` names (the vendor's rollback target), anything not shaped like an installer's release, anything with a mount inside it, and everything while a daemon may run (a pid file names a live process, a process runs from the package directory, its socket answers), while either daemon lock is held or a migration is under way. It goes further than the vendor's own pruning in one case: without a `selection-history` matching `current` the vendor prunes nothing, so its copies stay forever, where codex-alab keeps only `current`'s. Whether anything is superseded is asked first, with no lock or probe, and the socket is asked last, so a start never connects to a vendor daemon it can see otherwise. The upstream files it copies its rules from are in `alab/watched-paths` |
| `tui/src/backend_banners/actions.rs` | plan-ad buttons (buying credits or a reset stays: at a hard limit it is the only way to keep working) |
| `tui/src/app/startup_prompts.rs` | the optional "try the new model" screen (forced migrations still show) |
| `core/src/turn_metadata.rs` | per-turn `git status`, HEAD and remote URLs in OpenAI's request metadata |
| `app-server/src/extensions.rs` | the git-attribution extension (a settings fetch per thread and a "Co-authored-by: Codex" instruction) |
| `feedback/src/lib.rs`, `state/src/log_db.rs` | the 4 MiB TRACE ring and TRACE rows in the SQLite log (INFO and up stay), and /feedback's per-request `feedback_tags` rows there (about 4 KB each, the whole feature map: 29% of a real 18.6 MB log database). Those rows are also the log's only record of each model request's and response's id and of a 401 recovery's request id, cf-ray and error, so they go too; the filter ignores config, so `[feedback] enabled = true` does not bring them back |
| `app-server/src/models_refresh_worker.rs` | the model catalog every 4.5 min per tab (hourly instead) |
| `tui/src/pets/mod.rs`, `tui/src/tui.rs`, `tui/src/custom_terminal.rs`, `tui/src/tui/event_stream.rs` | inside aterm, the ambient pet's whole PNG on every draw (a delete and about 3.9 KB of base64 for each frame codex paints: about one a second at rest, 31 a second in a turn). Each frame is stored once under an id of its own (`alab-tui/src/pets/kitty.rs`, through `alab-tui/glue/pet_kitty.rs`) and then only placed; it is sent again after a full clear or a resize, and when aterm answers a placement `ENOENT` (its restore after a seamless update carries no kitty images), an answer `alab-tui/glue/kitty_replies.rs` takes off the input. At most 16 frames stay stored, and the pet's end frees them. An aterm older than 0.100.0 (its `TERM_PROGRAM_VERSION`), whose puts ignore their own `c=`/`r=` and never move, keeps upstream's draw |
| `tui/src/pets/ambient.rs`, `tui/src/status_indicator_widget.rs`, `tui/src/motion.rs`, `tui/src/tui/event_stream.rs` | inside aterm, decorative frames while the terminal reports focus out (`alab-tui/src/motion.rs`): the pet holds its frame, and the status row's shimmer and the loading spinner tick with the row's 1 s clock. Focus in resumes them |

Hooks that add rather than stop:

| hook | what it adds |
| --- | --- |
| `tui/src/pets/image_protocol.rs` | `/pets` inside aterm (`TERM_PROGRAM=aterm`): codex's kitty graphics path on the direct medium (`Kitty`, never `KittyLocalFile`: aterm's `t=f` is off by default and `q=2` would hide the refusal). The same verdict turns on the fullscreen transcript's image previews |
| `tui/src/pets/catalog.rs`, `picker.rs`, `asset_pack.rs` | Rainbow Kitty (`alab-kitty`), available through `/pets`: aterm's codex calico on the spectrum ribbon, generated by `alab-tui/src/pets/pack.rs` into codex's built-in cache, never downloaded. No pet is selected by default, leaving the full prompt width available |
| `tui/src/onboarding/directory_trust.rs`, `tui/src/startup_draft.rs` | folder trust inside aterm (`alab-tui/src/trust.rs`, CONTRACT §5): codex-alab answers its own launch folder's trust screen when aterm's `harness policy` for the session says `effective=on` and either `approve=all` (aterm's default: any launch folder, as aterm's full power trusts any folder) or `approve=safe` with the folder and the folder codex's trust is written for (a Git subdirectory's repository root) both under a `trust_roots` root, as written and symlink-resolved. It writes codex's own trust entry, as "Trust and continue" does, and the launch session card names the folder and why (the root, or full power) (an info line through the banner's glue, no hook of its own). Not for a project marked untrusted, a `-C` or resumed folder, or a connected app-server. A read unanswered within 2 s, one aterm answers `busy`, or one that finds the host suspended for a seamless update, is made again for up to 15 s while the startup draft draws its composer (one read anywhere else; a person who quits the draft meanwhile quits); an older aterm, no socket, a hold or a read still unanswered then shows the screen, and the screen raises codex-alab's `trust:` attention naming the folder and why while it waits (`alab-tui/src/waiting.rs`) |
| `tui/src/onboarding/onboarding_screen.rs` | the sign-in screen inside aterm (`alab-tui/src/login.rs`, through `alab-tui/glue/login_screen.rs`, CONTRACT §2 (a)): while a signed-out first launch shows codex's login step, before any claim, codex-alab raises its keyed attention (`meta set attention owner=codex-alab login: …`, at most 200 B, on a connection of its own, never delaying the screen), and clears it as soon as the sign-in completes, or when the onboarding screen ends without it (the person gone). One a killed screen left standing is cleared by the next start past the sign-in. Outside aterm, an older aterm or a stalled one raises nothing, and the screen is upstream's |
| `tui/src/app/startup_prompts.rs`, `tui/src/startup_hooks_review.rs` | the other startup screens a person answers before any thread, inside aterm (`alab-tui/src/waiting.rs`, CONTRACT §2 (a)): a forced move off a retired model and the hooks review raise codex-alab's keyed attention (`migration: …`, `hooks: …`, at most 200 B whole) while they wait, cleared when they end, as the sign-in's is; a killed screen's is cleared by the next start past the screens. Outside aterm, an older aterm or a stalled one raises nothing |
| `tui/src/terminal_program_status.rs`, `config/src/strict_config.rs` | the OSC 9;4 progress bar inside aterm, with no multiplexer between (`alab-tui/src/progress.rs`, written by the frame glue `alab-tui/glue/session_observe.rs` beside OSC 7501, CONTRACT §6): `9;4;3` while the thread works with no plan this turn, `9;4;1;<pct>` from this turn's `update_plan` steps while that turn runs, `9;4;2;<pct>` when it fails (100 with no step done), `9;4;0` at rest. aterm draws no bar on macOS: its pet turns to look at a value bar's change, which with upstream's defaults is only a failed turn, since a plan needs `[tools.update_plan] enabled = true` (opt-in upstream since a9519cbcd; ALab leaves it so). The first hook takes a showing bar down wherever the TUI clears its 7501 report (the exit, the panic hook, the signal watchdog, Ctrl+Z, an external program). `[tui] alab_progress = false` (in `~/.codex/config.toml`, or `-c`) turns it off; the second hook lets codex-alab's `--strict-config` take that key (`codex_alab_core::own_tui_key`), while vendor codex's `--strict-config` refuses it, so it stays out of a config vendor codex reads strictly, such as a checked-in project config |
| `tui/src/chatwidget.rs` (`refresh_terminal_program_status`), `tui/src/chatwidget/protocol.rs` (the live `TurnStarted` arm), `tui/src/terminal_program_status.rs` (`alab_batch`, `set_terminal_program_status_with_prefix`, a failed write's cache reset), `tui/src/app/app_server_events.rs` (the routed notification, the event stream) | the OSC 7501 report inside aterm as aterm counts turns (`alab-tui/src/turns.rs`, glue in `alab-tui/glue/chat_hand.rs` and `alab-tui/glue/program_status_batch.rs`, CONTRACT §6): MCP startup with no turn is no `working` (the report says where the thread stands), and a turn the TUI submitted while the report stands in a turn (a queued message sent as the last turn ends, a message under a background terminal or an active goal) writes `idle`, then `working`, as it starts, so aterm's `ProgramStateReport::turns` counts it and a proven self-claim's landing reopens; a goal's continuation or an autopilot act under standing work writes nothing. An edge's two words go as one batch under upstream's status-cache lock, the order the panic hook's `clear` takes, and one flush. While a turn of a thread the TUI does not show runs (the autopilot's root threads, and every turn the TUI routes the app-server's notifications for: a sub-agent's, a side thread's, any under a remote app-server; glue in `alab-tui/glue/session_observe.rs`), a frame at rest says `working`, so a thread switch away from a running turn and back begins none. Only in the codex binary inside aterm; elsewhere the report is upstream's |
| `tui/src/security_setup.rs` | inside aterm, OpenAI's account security-setup notice as ONE entry of aterm's exceptions digest (`notice later`: the menu-bar item and Settings ▸ Messages) instead of a banner in every tab and launch (`alab-tui/src/notices.rs`, through `alab-tui/glue/security_setup.rs`, CONTRACT §6; the owner's ruling, 2026-10-21: "for the nag, we need to manage those from a central location, not just interrupt users randomly"). It runs after upstream's fetch, its 3 s deadline and its checks, under the key `codex.security-setup.<8 hex>` (an FNV-1a hash of the account and user ids, so no id reaches aterm's `messages.log`), with fixed words, the server's title and description as detail lines, and the entry's one button, `Open in browser`, on the server's chatgpt.com page. The owner's dismissal there holds for every tab (`how=quiet` is taken too). Not the codex binary, outside aterm, an aterm without `notice later` (`ERR usage: notice <post|…`), a refusal, or no answer within 750 ms: upstream's banner, exactly as before. Usage limits, the sign-in and hook consent stay inline, and nothing here touches a refusal or an access decision |
| `tui/src/chatwidget/ide_context.rs` | `/ide` inside aterm under a proven claim: each message carries the terminal's context, read from aterm over the control socket on a connection of its own (`alab-aterm/src/context.rs`: the selection, this tab's commands from before codex started, a split pane's last failed command and the tail of its output), fenced as data, with an info line naming what was sent. Nothing is written into `~/.codex`; without the claim, `/ide` is upstream's |
| `login/src/auth/manager.rs` (`refresh_and_persist_chatgpt_token`) | the cross-process refresh lock aterm takes on a Codex login (`alab-core/src/auth_lock.rs`, through `alab-core/glue/auth_refresh_lock.rs`): a ChatGPT refresh of a login kept in a file (`File` storage, a `CODEX_AUTH_PATH` file, an `Auto` login's fallback file) holds an exclusive `flock` on `auth.json.lock` beside it (`<file>.lock`, made `0600`, never through a link) from a read of the login under it to the write of the answer, so codex-alab and aterm, or two codex processes, never both send its single-use refresh token. Under the lock it sends only the refresh token the file still holds: one another process rotated meanwhile is reloaded and nothing is sent, and another login, or none, gets `persist_tokens`' error with no request. A lock still held after 30 s (`auth_lock::WAIT`, three of aterm's 10 s usage reads), or refused, returns a transient error without sending the token or changing the cached login. A later attempt can retry when the lock is available. Keyring and ephemeral logins keep upstream's in-process ordering; vanilla mode keeps the lock. The same glue holds the reload codex-alab's lost-login watch makes (`AuthManager::alab_reload_if`, `alab-autopilot/src/login.rs`, CONTRACT §5's login wall row), which no upstream line calls: upstream's `reload` that takes the login the store holds only where it is the walled identity's, as the 401 recovery's reload takes only the expected account's |

**Inline pets need aterm's history-aware Kitty deletion.** Current aterm source applies
image-ID and placement-ID deletes (`a=d,d=I,i=<id>`) to ring, staged and hot history as well
as the screen, so retiring a pet frame also removes its scrolled placements. Visible-only
deletes preserve history and the image data it still owns. Older aterm builds can leave
inline pet sprites behind until their image horizon drops them. The defaults select no pet;
`tui.alternate_screen = "auto"` and the fullscreen transcript use the alternate screen,
which has no scrollback. Update aterm for inline pets under `alternate_screen = "never"`,
raw output mode or a forced inline view.

Kept on purpose: the security-setup banner fetch (it carries account-security deadlines; inside
aterm its notice goes to aterm's digest instead of a banner, by the `tui/src/security_setup.rs` hook above),
the usage poll that feeds the status line, automatic thread titles (they name aterm tabs),
the Responses WebSocket prewarm at session start (model traffic: the first turn's
connection), the workspace cloud-config bundle (business, edu and enterprise plans only: their
admins' policy), the queue's 10 s SQLite poll (`codex queue`, local), and `codex doctor`/`codex
app` (on demand only). Removed files: the bundled `imagegen`, `openai-docs` and
`skill-installer` skills (`alab/removed-paths`).

**Measured, 2026-10-17.** Each of `codex exec` (one turn), `codex app-server` (initialize and a
thread) and the TUI (one turn, 75 s) ran against a mock ChatGPT backend with a scratch
`CODEX_HOME` and a ChatGPT login, behind a logging proxy, with every other connection denied by
a sandbox. Under codex-alab's defaults they reached only the model catalog, the account check
and the Responses endpoint; the TUI also polled usage and reset credits and fetched the
security-setup banner. Nothing reached a third party or OpenAI's analytics. With upstream's
values put back in `config.toml` the same `exec` and `app-server` runs also sent analytics events
and fetched the plugin catalog and the ChatGPT apps MCP server, so the harness sees what the
defaults stop. An idle TUI used 0.01 s of CPU in 35 s.

**Re-read, 2026-10-24.** The 247 upstream commits merged since (7b44ac1661..3985001067) were
read, not run, through four lenses: outbound requests, telemetry, local disk and CPU use,
and new screens or prompts. Nothing new runs under codex-alab's defaults. The analytics,
OTel and feedback switches gate every new event, and the new cloud skill and plugin catalogs
have no live provider. The new prompts are the Bedrock sign-in row, shown only when signed
out, and the daemon mismatch menu, which needs the shared daemon codex-alab does not attach.
The only leftovers were two tests of removed skills, now in `alab/removed-paths`. The next
audit starts at 3985001067.

## What was removed, and what was kept

Six auditors, each checked by an adversarial verifier, produced the removal list. The
combined set was then checked by building the whole workspace without it
(`alab(removed)` commits; every entry is in `removed-paths`):

- **all of `.github`**: OpenAI's CI (it would have run on every push here), its cron
  job, Dependabot, CODEOWNERS, issue forms, release and signing scripts;
- **the Bazel build**: every `*.bazel`/`*.bzl`, `.bazel*`, `MODULE.bazel.lock`, `bazel/`,
  `patches/`, Bazel helper scripts, Bazel-only sources and V8 inputs;
- **npm and installer distribution**: `codex-cli/`, `responses-api-proxy/npm`,
  `scripts/stage_npm_packages.py`, `scripts/install`;
- **Nix**, **CI-only linters**, `.vscode`, Windows dev setup, `docs/CLA.md`,
  `CHANGELOG.md`, the repo copy of `announcement_tip.toml`, and an unused 838 KB fixture;
- `SECURITY.md` (owner, 2026-10-16): it sends reports to OpenAI's Bugcrowd program.

That is 426 files. ALab's front page (`README.md`, now an owned path) removed two more,
`docs/contributing.md` and `docs/open-source-fund.md`. Kept on purpose, for the owner to decide:

| path | why it stays for now |
| --- | --- |
| `sdk/typescript`, `sdk/python`, `sdk/python-runtime` | kept (owner, 2026-10-16): for driving codex-alab from TS or Python; the packager's smoke tests use `sdk/python` |
| `package.json`, `pnpm-*`, `.npmrc`, `.prettier*` | kept with `sdk/typescript` (the MCP conformance runner installs through them) |
| `justfile`, `scripts/format.py`, `scripts/just-shell.py` | remove as a set if ALab does not use `just`; never piecemeal |
| `tools/` (argument-comment-lint, buildifier) | upstream's lint for `/*param*/` comments; ALab hooks should follow that convention anyway |
| `third_party/voice`, `codex-rs/voice-host` | depends on whether ALab ships voice; the crate needs gstreamer to build (it fails on this Mac) |
| `scripts/codex_package`, `scripts/build_codex_package.py`, `third_party/v8/*_release_manifests.sha256` | `alab/package.sh` uses them |
| `LICENSE`, `NOTICE` | Apache-2.0 requires them, and the package must ship them too |

## Setting up a clone

```sh
git clone <codex-alab's dev remote> ~/codex-alab
~/codex-alab/alab/setup.sh --train-rerere
```

The dev remote is ALab's private repository, the one `sync.sh` pushes to. The public
[alabsystems/codex-alab](https://github.com/alabsystems/codex-alab) is a snapshot of it, one
commit per release with no history: enough to build and package codex-alab, not to sync from.

`setup.sh` is idempotent. It configures the fetch-only `upstream` remote and its refs,
enables rerere with auto-staging and zdiff3 conflict markers, and with
`--train-rerere` replays the resolutions recorded in `origin/main`'s 30 newest sync
merges.

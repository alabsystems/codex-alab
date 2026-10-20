<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# codex-alab: trailing upstream with a thick layer on top

codex-alab is ALab's build of OpenAI's Codex CLI. It follows
[openai/codex](https://github.com/openai/codex) daily and carries ALab's changes on top.
This directory holds the process and tooling that keep both true at once.

| file | what it is for |
| --- | --- |
| `sync.sh` | merge the newest upstream into `main`: run it at least daily |
| `daily.sh` | the unattended wrapper: stepwise sync, status, log, notification, LaunchAgent |
| `CONTRACT.md` | the native harness: what codex-alab does itself in aterm, and the wire between them |
| `check.sh` | the gate a sync passes before it lands |
| `delta.sh` | the ALab layer, measured: where the next conflict will come from |
| `package.sh` | build a codex package directory from a clean commit, for `aterm pkg link` |
| `prune.py` | keep the newest packages and every one in use (linked by atpkg, running, or owed a crash relaunch) |
| `removed-paths` | upstream paths codex-alab does not carry; every sync re-applies them |
| `owned-paths` | paths ALab owns that upstream might also ship (root `AGENTS.md` and `README.md`) |
| `watched-paths` | upstream paths whose changes a sync calls out (release recipe, version logic, the vendor daemon's package rules) |
| `merge-attributes` | the lockfile merge rule, applied to sync merges only |
| `setup.sh` | make a clone sync-ready: upstream remote and refs, rerere |
| `smoke/dropin.py` | codex-alab against OpenAI's build: still a drop-in `codex` ("The drop-in check") |
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
(`alab-pristine` inside the sync worktree's git dir) is built, in its own `target/`, when a
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
`~/.codex` that holds this machine's login. Its steps:

1. **No hook went missing.** Each `ALAB(<topic>):` marker in an upstream file on
   `origin/main` (or `ALAB_CHECK_BASE`, which must exist) must still be there (markers
   inside `alab/`, `AGENTS.md` and `codex-rs/alab-*` do not count).
2. **Codex builds from the committed lockfile and runs** (`codex --version`). On failure
   (`check.sh` exit 2) the sync builds and runs pristine upstream in the pristine tree:
   if that fails too, the fault is the toolchain's (exit 6), else ALab's (exit 4).
3. **Every `codex-rs/alab-*` crate passes its tests.**
4. **Every upstream package ALab hooks into passes its unit tests (`--lib`), where the
   layer is to blame.** Upstream runs these under nextest, a process per test; under
   libtest some fail on pristine upstream (a stack overflow at the default stack, timing
   under load, insta's one-assertion-per-process rule, tests that need a binary `--lib`
   does not build). So the step runs them with an 8 MiB stack and `--no-fail-fast`,
   reruns each failure alone, and reruns what still fails on pristine upstream. A test
   gates unless it fails there too (a name two packages share: unless each package's copy
   that fails here fails there); one that does not run there gates as well. A crash,
   a build error, more than 60 failures, or a failure count that disagrees with libtest's
   own fails the step outright. Contained (below), about 20 upstream tests fail that pass
   uncontained (they write outside the scratch dir, bind or reach sockets elsewhere, or run
   the setuid `ps`); the pristine reruns are contained too, so they fail there as well and
   do not gate, and the cap sits well above them. Measured with hooks in codex-core and codex-tui: 9 to 18
   minutes per sync, plus the pristine tree's own build of the failing tests' packages,
   and a planted one-line fault was caught. A hung unit test holds the sync lock;
   `daily.sh` reports it after two refused runs.

Pristine upstream is built in a tree of its own, one per checkout (`alab-pristine` in the
checkout's git dir), with its own `target/`: cargo judges a workspace crate fresh by file
times, so a target shared with the ALab tree could hand either tree the other's builds,
and a check in one checkout must never move the tree another is testing. A person who runs
`check.sh` in their own checkout keeps that checkout's pristine tree and its `target/`
(about 3 GB per hooked crate) under `.git`; `git worktree remove --force "$(git rev-parse
--git-path alab-pristine)"` deletes it. Removing a linked worktree deletes its pristine tree's files
but leaves that tree's registration behind (`git worktree list` shows it `prunable`): drop it with
`git worktree remove --force --force <its path>` rather than `git worktree prune`, which would also
drop a person's worktree on a disk that is not mounted.

The sync runs `check.sh`, the pristine build that tells an ALab fault from a lane fault,
and the lock re-resolve (`targo metadata`, which reads the `.cargo/config.toml` upstream
just merged) **contained** (`lib.sh`, `alab_contained`): they build and run code upstream
landed hours ago, build scripts of newly added crates included, as the account that holds
`~/.codex/auth.json`, git and `gh` credentials, the SSH agent and the release key. Under
a macOS sandbox (`sandbox-exec`) with a scrubbed environment and a scratch `TMPDIR`, that
code cannot read files under `HOME` other than the clone, the Rust toolchains, PATH's
tools, the private cargo home (below; not its credentials), the `.cargo/config.toml` of
the directories above the build, and git's config; cannot read the keychain or the
clipboard, open apps, send Apple events or run `launchctl`; cannot connect to a Unix
socket but its own (not the SSH agent's, nor aterm's); and can write only the directories
its caller names (the two target dirs, for the gate), its scratch dir and the private cargo
home, so it plants no source, `.git`, `.cargo` or `rust-toolchain` file that git or cargo
read outside the sandbox. It keeps the network: builds fetch crates and V8.

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

Only crates under `codex-rs/` with a library target get step 4; a hook in a binary-only
crate (bwrap, config-schema, thread-manager-sample, voice-host) is covered only by the
build in step 2 if codex depends on it.

### Running unattended

`daily.sh` is the scheduled entry point. Each run is one stepwise sync. It appends the
output to `.git/alab-daily.log` and rewrites `.git/alab-daily.status` with one line:
when, exit status, the two tips, how many upstream commits are pending, and since when
the sync has needed someone.

Notification goes through `ALAB_NOTIFY`, a shell command that gets the exit status as
`$1` and the run's output on stdin. Unset, it is a row on aterm's message band (`aterm ctl
notice post`, key `codex-alab-sync`, the stop's reason as its detail), or a macOS
notification when no aterm answers; the row comes down when a later run lands. It fires:

- on every exit 3, 4 or 6: each is a new stop, since a sync still waiting exits 5;
- on exit 1 twice in a row (a lock held that long is a hung sync);
- once on exit 5, when the wait passes `ALAB_ESCALATE_HOURS` (default 48).

Point it at whatever reaches a resolver: an aterm session's inbox, or a headless agent
started in the sync worktree with the report as its task.

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

```sh
alab/daily.sh --launchd-plist >~/Library/LaunchAgents/systems.alab.codex-alab-sync.plist
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/systems.alab.codex-alab-sync.plist
```

The plist starts a run when it is loaded (at bootstrap and at every login), then every
4 hours (`StartInterval`, `ALAB_INTERVAL_SECONDS`); launchd skips an interval that falls
during sleep. It carries the PATH and HOME of the shell that printed it, so `targo`
resolves, plus `ALAB_NOTIFY`, `ALAB_SYNC_DIR`, `ALAB_CARGO`, `ALAB_ESCALATE_HOURS`,
`ALAB_TARGET_CAP_GB`, `ALAB_KEEP_TARGET_GB`, `ALAB_CONTAIN` and `ALAB_CARGO_HOME` if they were set. Smaller ranges conflict less, so more often is
cheaper. Name one sync host: the lock is per clone, and two hosts would race to push.

The owner decided (2026-10-16) to run it on this Mac as the owner's account, with the
gate contained (above). It is installed on the owner's Mac; it pushes with the account's own git
credentials, which stay outside the sandbox.

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

Today atpkg installs `codex` straight from OpenAI, through aterm's vendor lane
(`crates/atpkg/src/vendor_direct/`: a two-host digest anchor and Apple team `2DC432GLL2`).
`alab/package.sh` builds the same package layout from codex-alab, and `aterm pkg link`
puts it in that lane's place on one machine. Hooks on `main`: none.

```sh
alab/package.sh --commit main      # package main (the run fetches upstream; fetch origin first)
alab/package.sh                    # package HEAD of this checkout (clean, and on origin/main)
alab/package.sh --version-only     # print the version a build of HEAD would carry, and why
alab/package.sh --any-commit       # package a commit that is not on origin/main
alab/package.sh --rebuild          # package a commit again (a fixed packager, a new compiler)
aterm pkg link codex <pkgdir> bin/codex   # the command the script prints; a person runs it
aterm pkg unlink codex                    # back to the vendor build
```

`--out <dir>` and `--keep <n>` say where packages go and how many stay. A commit that is
not on `origin/main` is refused (Decision 4) unless `--any-commit` is given. A commit
that a package in `<out>/codex` already holds (the `source` in its `alab-build.json`, not
its version, whose line moves) is refused unless `--rebuild` is given; the new package is
then staged beside the old one, as `<version>.rebuilt-<UTC time>` when the version is the
same.

What a run does, in order (`--help` says the same):

1. **The version.** `MAJOR.MINOR.0-dev.<count>+g<sha9>`, the dev form of
   `~/publication/VERSIONING.md`. `MAJOR.MINOR.0` is the higher of the commit's root
   `VERSION`, the public release line (Decision 8), and the floor: the next minor above the
   newest upstream stable tag (`upstream-tags/rust-vX.Y.Z`, no pre-release) whose branch
   point (its merge-base with `upstream/main`) the commit has merged. So a package never
   calls itself older than the upstream code it holds, and packaging never waits on a
   release bump (upstream cuts a stable every two to seven days). When `VERSION` is behind
   the floor the run says so and that `pub bump codex-alab` is due before the next release:
   `--release-check` (what `publish/pre-promote` runs) refuses until it is. A commit from
   before `VERSION` existed takes the floor; a published snapshot (one commit, no history)
   has no branch point to read and takes its `VERSION` as cut. `<count>` is `git rev-list --count`, for ordering only; the sha is the
   identity. The run fetches upstream's release tags and `main` first, by the refspecs
   `alab/setup.sh` configured (fetch only; `main` has no `+`, so a rewrite stops the run), so
   a stale tag ref cannot pass an old line, and prints how it got each part, with the
   upstream commit the source merged last. A dirty checkout is refused.
2. **A scratch worktree** at the commit, `~/Developer/codex-alab-pkg-<sha9>`. The version is
   stamped in `[workspace.package]` of `codex-rs/Cargo.toml` and `targo metadata`
   refreshes the lock, contained and able to write only the lock; the run stops if that
   moved anything in the lock but workspace versions. The stamp is never committed. The
   worktree and its `target/` go when the run ends, however it ends
   (`ALAB_PKG_KEEP_WORKTREE=1` keeps them); the run removes its own worktree only and
   never prunes the clone's other registrations.
3. **The compiler, then codex and codex-code-mode-host.** The toolchain is the atpkg
   store's admitted Trust: the `targo` in `<prefix>/store/trust/current/bin` (`<prefix>` is
   `$ATPKG_BIN`'s parent, else `~/Library/Application Support/aterm/pkg`), resolved
   physically to its build-numbered directory, as `atpkg-publish pack-package` resolves the
   compiler it holds a package to (aterm `crates/atpkg-publish/src/artifact/toolchain.rs`;
   there is no PATH rung). The `targo` on PATH is often rustup's `trust` link to a Trust
   checkout's stage2, whose `-vV` says `commit-hash: unknown`: pack-package refuses its
   package. `ALAB_CARGO=<targo>` names another one explicitly. Before any contained step, the
   compiler is taken from the toolchain itself: targo builds a one-file probe crate (no
   dependency, no build script, so only the toolchain's own code runs) outside the
   sandbox, in a new directory under `~/.cache/codex-alab` that no contained step is ever
   given. Its `target/.rustc_info.json` names the `-vV` and the sysroot; that sysroot's own
   `trustc -vV` must say the same, and it must say `binary: trustc`, else the run stops.
   The compiler's id is a digest of its `-vV`, its sysroot and its binaries (the `trustc`
   executable and `librustc_driver`), so a rebuilt compiler at the same path is a new id.
   Then it is held to the store, as pack-package's `check_compiler` holds the package: the
   sysroot must be the store build's root (physically) and the `-vV` that root's own
   `trustc -vV`, byte for byte (the digest and id are then pack-package's, taken over the
   same root). The store's targo building with anything else stops the run. A compiler
   `ALAB_CARGO` names that is not the store's is built with, but labelled: a banner at the
   start, a line at the end, and `compiler.admitted` `false` (with `admitted_why`) in
   `alab-build.json`; pack-package refuses that package.
   Then targo builds the probe crate again, contained, in `codex-rs/` (whose
   `.cargo/config.toml` it reads) and in the neutral `~/.cache/codex-alab`: each record
   must name that compiler, so a stock toolchain stops the run here, not after the build.
   A record a contained step wrote is only compared, as text, with the trusted `-vV` and
   sysroot: nothing named in it is run (the record is the build's own claim, so this
   catches a misconfigured toolchain, not a hostile build that forges it). Then one
   release build: `targo --unverified build --release --locked -p
   codex-cli --bin codex -p codex-code-mode-host --bin codex-code-mode-host`, with
   `STABLE_GIT_COMMIT=<commit>` and the `RUSTY_V8_ARCHIVE` pair from upstream's
   `scripts/codex_package/v8.py` (`resolve_codex_v8_cargo_env`, cached in
   `~/.cache/codex-alab/v8`). The script checks the pair again itself: the release's
   checksum manifest against `third_party/v8/*_release_manifests.sha256`, each file against
   the manifest. No `--target`, so the output is `target/release/`. The build runs
   contained (`alab_contained`, "The gate"), writing only its `target/`; the V8 cache is
   read-only to it (`--read`). Its `target/.rustc_info.json` must record the trusted
   compiler.
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
   nothing more is written there and it is never published: the run copies it with
   `cp -RP` (a symlink is copied, never followed) into another new hidden
   `<out>/codex/.<v>.staged.*` that no contained step is ever given, and removes it. The
   copy is refused if it holds a symlink, a special file, or a setuid, setgid, sticky,
   group- or world-writable file. The run writes into the copy `LICENSE` and `NOTICE`
   (Apache-2.0, from the commit's own objects, refused unless each is a regular file
   there) and `alab-build.json`: `source` (the commit), `version`, `target`, `compiler`
   (the trusted compiler of step 3: `rustc`, the full `rustc_vV`, `sysroot`,
   `binaries_sha256`, `id`, and `admitted`/`admitted_why`: whether it is the store's
   admitted Trust), `ripgrep` and `ripgrep_features`, `rusty_v8`, `packager`
   (the commit of the `alab/package.sh` that ran) and `built_at`.
6. **A smoke** of the copy, with a scratch `CODEX_HOME`, each step contained, the copy
   read-only to it (`--read`) and able to write only that `CODEX_HOME`: `bin/codex --version` says `codex-cli <v>`; `bin/codex
   features list` shows ALab's defaults (`daemon_auto_start` and `plugins` false);
   `codex-path/rg --version` says `+pcre2` and PCRE2 available; `codesign --verify
   --strict` passes, `file` says arm64 and `otool -L` names only `/usr/lib` and
   `/System/Library` libraries on each Mach-O; upstream's layout validator passes. Only
   then is the copy renamed into place (mode 0755, as a package made before was).
7. **It prints the link command** and keeps the newest `--keep` (3) packages in
   `<out>/codex` (`alab/prune.py`). A package in use is never deleted: one atpkg links as
   `codex` (its link marker `<prefix>/links/codex`, the `bin/codex` shim and the
   `agents/codex` twin, a symlink resolved and never read through, a regular file read
   only when it is a small text shim; `aterm pkg which codex`), one a running process
   executes from (`proc_pidpath` of every process, and `ps -o comm`), and one a standing
   codex-alab session record names (`resumeArgv[0]`, `exe`, `launchArgv0` under
   `$CODEX_HOME` and `~/.codex`: a dead process's record is a crash relaunch still owed). Paths
   are read whole, spaces and all (`~/Library/Application Support/...`). When no link source
   can be read, a marker or a record stands but cannot be, or no process can be listed,
   nothing is pruned. `alab/smoke/prune_test.py` checks each against a scratch root, prefix
   and `CODEX_HOME`.

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
manifest, `df` failing) stops with an `alab:` message. Measured on the owner's Mac (2026-10-16),
packaging `main` at ccd947b69 as `0.164.0-dev.12321+gccd947b69` (`--rebuild`, beside the
first package of that commit): the whole run took 11 min 29 s, of it the release build
11 min 2 s from a cold `target/` and rg with PCRE2 14 s; the package is 312 MB (`codex`
239 MB, `codex-code-mode-host` 65 MB, `rg` 6.9 MB, no voice). The compiler was the
Trust checkout's own stage2 (`commit-hash: unknown`); `aterm pkg lineage scan` against the store's
Trust finds nothing (3 objects), but refuses that stage2 itself as a `--compiler`,
because its `-vV` names no commit.

**Why it is built this way** (settled by evidence)

- **The version** (`CARGO_PKG_VERSION` and `codex-package.json`, identical) must be a
  pre-release above the last vendor version installed, unique per build. `0.0.0` stalled
  aterm's Codex live upgrade (retired since, aterm fc74be92fe), and the server filters
  models by `client_version`. A bare stable
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
against a vendor one (CONTRACT §7). **What aterm still needs**, for a published lane later:
an ALab anchor beside the vendor one, ad-hoc signatures accepted, and a store build id above
`LEGACY_CEILING` (2 026 092 401) and below 10^18.

**Decisions** (the owner, 2026-10-16: "you decide")

1. Today's request supersedes the 2026-09-30 retirement of the fork RFC
   (`docs/RFC-embedded-agents-2026-08-15.md`, "acx"): for an agent ALab builds from source,
   the agent's own source is part of aterm's layer. aterm records it in its own docs.
2. Versions follow the publication scheme (`~/publication/VERSIONING.md`): a dev build is
   `MAJOR.MINOR.0-dev.<count>+g<short-sha>`. The line is at least the next minor above the
   newest upstream stable `main` has merged, so it sorts above every vendor build the server
   or atpkg has seen, and codex's update check cannot parse it (it never offers OpenAI's
   build over it). It is stamped on a throwaway leaf at packaging, never on `main`
   (Decision 8 says where the line itself lives).
3. Publication: the owner made the public `alabsystems/codex-alab` (2026-10-16), so
   codex-alab is published through the publication engine ("Publication" below). Packaging
   stays local (`aterm pkg link codex <pkgdir>`, no credential).
4. Ship from `main` (alpha-equivalent, daily).
5. The binary stays named `codex` (aterm identifies agents by name). The launch banner
   says "ALab build of OpenAI Codex"; the build is for the owner's own machines.
6. Updates: the local link first; a roster-signed head beside the vendor lane later
   (owner rule R1), which needs the owner's release credential to publish.
7. No `rust-toolchain.toml` hook: `targo` ignores the stock pin.
8. **The release line is the root `VERSION`** (`MAJOR.MINOR.0`), changed only by `pub bump
   codex-alab`. It is upstream's numbering, not a line of ALab's own the way orca-alab
   restarted at `0.1.0`: a codex-alab binary must never call itself older than the upstream
   code it holds (the server picks models by `client_version`): a package takes the floor
   when `VERSION` is behind it (step 1 above), and a release is refused below it
   (`publish/pre-promote`, `package.sh --release-check`, run by the engine before any public
   write). Run `pub bump` with `PUBLISH_REPO_PATH` naming a clean checkout of `origin/main`,
   as every engine verb is run, and push its commit. A public release `vX.Y.0` is a source snapshot; a package
   built from it still carries `X.Y.0-dev.<count>+g<sha>`, because a bare stable version
   switches on OpenAI's daemon updater ("Why it is built this way"). The upstream alignment
   is a separate fact, never the version: the openai/codex commit the release merged last
   (`package.sh --version-only` prints it). The fork-provenance record of a public release
   is an annotated tag `openai-codex/<upstream sha12>` on its public snapshot, beside
   `v<version>`.
   What keeps a codex-alab install from being "upgraded" back to OpenAI's build is not the
   version, so a line that sorts below OpenAI's latest is safe: atpkg leaves a dev-linked
   program alone (its vendor lane answers `Linked` before it reads any version), and aterm
   moves no Codex at all: its Codex upgrade lane (`upgrade_codex`, which moved only vendor
   bytes) is retired (aterm fc74be92fe, CONTRACT §7), codex-alab upgrades itself, and no aterm
   reader parses a pre-release version to order builds.
9. **The front page is ALab's.** The root `README.md` is an owned path (`owned-paths`): an
   upstream change to it never stops a sync, and upstream's copy waits in
   `alab/upstream/README.md`. Chosen over a merge attribute (a `merge=ours` driver drops
   upstream's change without saving it) and over a stage-time transform (the engine runs
   none for a mirrored repo, and the dev repo's front page should say the same thing).
10. **Toolchain and gate.** A root `rust-toolchain.toml` says `channel = "trust"` (an ALab
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

### Publication

`~/publication` publishes codex-alab as `mode = "mirror-head"`, orca-alab's mode: the
public `alabsystems/codex-alab` is one snapshot commit of the dev repo's `origin/main` per
release, minus the paths no ALab repo exports (`docs/`, `publish/`, `.github/`), with the
dev org rewritten and the engine's guards run on exactly the bytes it pushes. `pub stage
codex-alab` puts the snapshot in the private staging repo; only `pub promote`, on the owner's
direction, writes the public one. `publish/` holds the repo's policy for it.

`publish/forbidden-extra.txt` adds the owner's account, the second machine's account, the
name run together and the machines' host names to the central patterns (none is in the tree,
upstream's files included); `publish/pre-promote` refuses a release below the upstream
floor (Decision 8).

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

### The drop-in check

codex-alab must stay a drop-in `codex`: the same binary name, CLI, `CODEX_HOME`, config and
auth files, `exec`, `app-server`, MCP and SDK wire. `alab/smoke/dropin.py` measures that
against OpenAI's build, both run with a scratch `HOME`, `CODEX_HOME` and `XDG_RUNTIME_DIR` and
no aterm in the environment, and exits non-zero on any difference it cannot attribute. It
runs two packages and builds nothing (defaults: the newest `package.sh` output and the newest
vendor build in atpkg's store). Each flag is compared with its short form, whether it takes a
value and its possible values, and each page's positional arguments as written; a short
flag, value or argument OpenAI's build has and codex-alab lacks fails unless upstream's source
no longer names it, and a clone with no `upstream/main` (before `alab/setup.sh`) fails rather
than excuse every removal. Run 2026-10-16, codex-alab `0.164.0-dev.12410+g4d44820e9` against
OpenAI's `0.163.0`:

| check | result |
| --- | --- |
| binary | both `bin/codex` in layout v1, `codex-cli <version>` |
| `--help` tree | 74 pages; the only differences, `exec-server` and `exec-server forward` `--executor-credential-stdin`, are upstream's own (in upstream at 835d3049f, after 0.163.0) |
| `exec --experimental-json` (the TypeScript SDK's argv) | both: a turn against a local mock model, `thread.started` to `turn.completed`; the same 20 event key paths |
| `app-server --listen stdio://` (the Python SDK's argv) | both answer `initialize` |
| `app-server generate-json-schema --experimental` | 21,848 definitions, properties and enum values in both; 131 only in codex-alab, every one in upstream's protocol crate at the merge-base |
| effective config (`config/read`), outside aterm and with `TERM_PROGRAM=aterm` | codex-alab with no `config.toml` reads what OpenAI's build reads with ALab's defaults as its `config.toml`: 18 ALab defaults outside aterm, 21 inside (the pet, OSC 9, no paste-burst detection); the one other difference, `tui.presets`, is a key upstream added after 0.163.0 |
| ALab defaults overridable | the six ALab feature defaults each flip back by `config.toml`, `-c features.<f>=…` and `--enable`/`--disable` |
| `mcp list --json` | both list the server `config.toml` declares |
| `CODEX_HOME` after one exec | 23 paths in both; codex-alab writes nothing OpenAI's build does not; it skips the plugin marketplace sync (20 paths) and the three removed bundled skills (37 files) |
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
automatic recaps; it caps `history.jsonl` at 8 MiB, and picks a status line with context and
both usage windows. Inside aterm only (`TERM_PROGRAM=aterm`), `alab-core/aterm-defaults.toml`
is merged over that: Rainbow Kitty as the pet, OSC 9 notifications, no paste-burst
detection (aterm's typed input arrives as a burst and must submit), and `tui.alab_progress`,
the OSC 9;4 progress bar (ALab's own key: upstream's typed config, and so `config/read`, carries
no `alab_*` key, and `smoke/dropin.py` names it ALab's own; vendor codex's `--strict-config`
refuses it where a person writes it). In another terminal none of
the four is set, so codex behaves there as upstream's does: it picks the notification method
the terminal supports, holds an unbracketed multi-line paste as one paste, writes no pet
sheet or frames into `~/.codex` for a pet nothing draws, and writes no progress bar. `alab-tui`'s tests
pin each key through `ConfigBuilder` (an upstream rename would otherwise turn it into a
silent no-op) and that a user's config still wins.

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

Hooks that add rather than stop:

| hook | what it adds |
| --- | --- |
| `tui/src/pets/image_protocol.rs` | `/pets` inside aterm (`TERM_PROGRAM=aterm`): codex's kitty graphics path on the direct medium (`Kitty`, never `KittyLocalFile`: aterm's `t=f` is off by default and `q=2` would hide the refusal). The same verdict turns on the fullscreen transcript's image previews |
| `tui/src/pets/catalog.rs`, `picker.rs`, `asset_pack.rs` | Rainbow Kitty (`alab-kitty`), ALab's built-in pet and the default inside aterm (`[tui] pet` in `alab-core/aterm-defaults.toml`): aterm's codex calico on the spectrum ribbon, generated by `alab-tui/src/pets/pack.rs` into codex's built-in cache, never downloaded |
| `tui/src/onboarding/directory_trust.rs` | folder trust inside aterm (`alab-tui/src/trust.rs`, CONTRACT §5): codex-alab answers its own launch folder's trust screen when aterm's `harness policy` for the session says `effective=on` and either `approve=all` (aterm's default: any launch folder, as aterm's full power trusts any folder) or `approve=safe` with the folder and the folder codex's trust is written for (a Git subdirectory's repository root) both under a `trust_roots` root, as written and symlink-resolved. It writes codex's own trust entry, as "Trust and continue" does, and the launch session card names the folder and why (the root, or full power) (an info line through the banner's glue, no hook of its own). Not for a project marked untrusted, a `-C` or resumed folder, or a connected app-server; an older aterm, no socket or a 2 s timeout shows the screen |
| `tui/src/onboarding/onboarding_screen.rs` | the sign-in screen inside aterm (`alab-tui/src/login.rs`, through `alab-tui/glue/login_screen.rs`, CONTRACT §2 (a)): while a signed-out first launch shows codex's login step, before any claim, codex-alab raises its keyed attention (`meta set attention owner=codex-alab login: …`, at most 200 B, on a connection of its own, never delaying the screen), and clears it as soon as the sign-in completes, or when the onboarding screen ends without it (the person gone). One a killed screen left standing is cleared by the next start past the sign-in. Outside aterm, an older aterm or a stalled one raises nothing, and the screen is upstream's |
| `tui/src/terminal_program_status.rs`, `config/src/strict_config.rs` | the OSC 9;4 progress bar inside aterm, with no multiplexer between (`alab-tui/src/progress.rs`, written by the frame glue `alab-tui/glue/session_observe.rs` beside OSC 7501, CONTRACT §6): `9;4;3` while the thread works with no plan this turn, `9;4;1;<pct>` from this turn's `update_plan` steps while that turn runs, `9;4;2;<pct>` when it fails (100 with no step done), `9;4;0` at rest. aterm draws no bar on macOS: its pet turns to look at a value bar's change, which with upstream's defaults is only a failed turn, since a plan needs `[tools.update_plan] enabled = true` (opt-in upstream since a9519cbcd; ALab leaves it so). The first hook takes a showing bar down wherever the TUI clears its 7501 report (the exit, the panic hook, the signal watchdog, Ctrl+Z, an external program). `[tui] alab_progress = false` (in `~/.codex/config.toml`, or `-c`) turns it off; the second hook lets codex-alab's `--strict-config` take that key (`codex_alab_core::own_tui_key`), while vendor codex's `--strict-config` refuses it, so it stays out of a config vendor codex reads strictly, such as a checked-in project config |
| `tui/src/chatwidget/ide_context.rs` | `/ide` inside aterm under a proven claim: each message carries the terminal's context, read from aterm over the control socket on a connection of its own (`alab-aterm/src/context.rs`: the selection, this tab's commands from before codex started, a split pane's last failed command and the tail of its output), fenced as data, with an info line naming what was sent. Nothing is written into `~/.codex`; without the claim, `/ide` is upstream's |

**Pets in inline mode leave stale sprites in aterm's scrollback.** aterm's kitty deletes
(`a=d,d=I,i=<id>`) reach placements on the visible screen only; one that has scrolled into
history keeps its cells until aterm's image horizon drops it. The defaults are safe:
`tui.alternate_screen = "auto"` and the fullscreen transcript draw on the alternate screen,
which has no scrollback. Under `alternate_screen = "never"`, raw output mode or a forced
inline view, a pet frame pushed off the top by history insertion stays behind as a ghost.
The fix belongs in aterm (id-addressed deletes reaching history rows, as kitty does), not in
a codex-alab gate.

Kept on purpose: the security-setup banner fetch (it carries account-security deadlines),
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

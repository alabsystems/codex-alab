<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# AGENTS.md — rules for agents working on codex-alab

codex-alab is ALab's build of [openai/codex](https://github.com/openai/codex): upstream's
tree and history, merged in at least daily, with ALab's changes layered on top.
`alab/README.md` is the process; this file is its short form. Upstream's own conventions
(code style, crate layout, tests, `/*param*/` argument comments) hold in the code upstream
owns. The nested `AGENTS.md` files are upstream's.

## Continuous autonomous work
- Decide, act, record the decision and its reason, and keep moving. Another AI can revisit it.
- Do not raise routine alerts, nags, FYIs or `NEEDS YOU` requests. An unavailable credential,
  permission, service or machine blocks that operation; record it and advance other useful work.
- Respect explicit human holds and permission boundaries. Repeated failures call for a different
  approach, not another request for the owner to decide.
- Prefer rigorous architecture, delete obsolete behavior, and preserve compatibility only when
  explicitly required. An expedient stopgap requires an explicit request.

## Keep the layer thin
Every line ALab changes in a file upstream owns can conflict at a sync; a line in a file
only ALab owns never does. Where a change sits decides its cost: a one-line hook at a
stable spot almost never conflicts, but a patch inside a busy upstream function
conflicts about once a quarter. Upstream lands about 44 commits a day.
- New behavior lives in ALab-owned paths: `codex-rs/alab-*` crates for code, `alab/` for
  process and tooling. A single `"alab-*"` glob line, last in the workspace members list,
  makes every ALab crate a member.
- An upstream file gets only a hook (a call into an ALab crate, a match arm, a list
  entry, a default), marked `ALAB(<topic>): <why>`. Use an extension point upstream
  already has where one fits (`ExtensionRegistryBuilder`, config `notify`/`hooks`,
  `model_providers`, the app-server protocol).
- Never edit upstream's entries in `codex-rs/Cargo.lock`: a sync takes upstream's lock and
  re-resolves ALab's entries, so pin a version an ALab crate needs exactly in its own
  `Cargo.toml` (`= x.y.z`), never in the lockfile alone.
- Defaults go in `codex-rs/alab-core/defaults.toml`, merged over upstream's packaged
  defaults in the ALab binary. Terminal-specific defaults go in
  `codex-rs/alab-core/aterm-defaults.toml`; leave upstream's defaults unchanged.
- Never reformat, reorder, rename or re-indent upstream code. Format only what you wrote
  (`trustfmt <file>`), never a workspace-wide `targo fmt`.
- Remove upstream content only as a whole path listed in `alab/removed-paths`, in the same
  commit as the `git rm`. Never hand-delete an upstream file.
- OpenAI accepts no pull requests: plan to carry every change. Drop ALab's copy when
  upstream ships an equivalent.
- One topic per commit, with the subject `alab(<topic>): ...`. `alab/delta.sh` ranks the
  upstream files ALab modifies by conflict risk, and `--topics` lists the hooks. Look at it
  before and after touching an upstream file.

## Syncing with upstream
- `alab/sync.sh` merges upstream into `main`. `--stepwise` lands what merges unattended as
  one merge, then stops at the first upstream change that needs a resolver. `alab/daily.sh`
  is the unattended form. Never rebase `main` and never force-push it: every build ALab
  ships is identified by a commit that must stay reachable. `main` tracks `upstream/main`
  only. Release-aligned builds come from leaf branches (`alab/README.md`, "Releases").
- `alab/sync.sh --status` says what is in progress and why it stopped. `--continue` lands
  the merge in progress only; `--abort` discards it (rerere keeps what it learned).
- Exit 3: conflicts. Resolve them in the sync worktree the sync names: keep upstream's
  change and re-apply ALab's intent on top. Then `git add` and run
  `alab/sync.sh --continue`.
- Exit 4: `alab/check.sh` failed, or a fix was left uncommitted. If a resolution was
  wrong, fix it in the merge (`git rerere forget <file>`, fix, `git commit --amend`). If
  ALab has to adapt to an upstream change, commit the adaptation on top. If upstream made a
  hook redundant and you dropped it, rerun with `ALAB_ALLOW_HOOK_LOSS=1`. Then `--continue`.
- Exit 5: an earlier sync still waits, for its resolver or for a `--no-push` merge to land:
  `--continue` it, or `--abort`.
- Exit 6: upstream itself does not build and run with Trust. That is a lane fault, fixed in
  Trust; then `--continue`.
- Exit 1 (or a git status such as 128): an error, named on stderr. The next unattended run
  resumes an unattended sync that stopped this way; after your own `--continue`, rerun it.
- A new clone runs `alab/setup.sh --train-rerere` once. Never push to `upstream`.

## Toolchain and gate
- Build with the Trust toolchain, in your own worktree (each `target/` is 12 to 15 GB):
  `targo --unverified build -p codex-cli --bin codex` in `codex-rs/`, which is what the
  gate builds. A bare workspace build also builds voice-host, which needs gstreamer and
  fails on this Mac. `codex-rs/rust-toolchain.toml` pins stock Rust for rustup's proxies;
  targo runs Trust regardless (`aterm help rust`).
- `alab/check.sh` is the gate a sync must pass. Its executable entry uses a qualified
  `atpkg-publish` (`ALAB_ATPKG_PUBLISH` may name it) to authenticate and lease the working
  `trust.lock`, then runs the source/pristine checks under the existing protected profile.
  `alab/package.sh` selects from its exact requested source commit before using compiler-keyed
  caches. Both use native explicit-target admission and refuse compiler overrides or
  `ALAB_CONTAIN=0`; neither selects from the global current shim. The gate takes no options
  (`--help` aside):
  - no `ALAB` hook in an upstream file went missing;
  - the codex binary builds from the committed lockfile and runs;
  - every `codex-rs/alab-*` crate passes its tests;
  - `targo tippy` finds no warning or error in ALab's code (the `alab-*` crates, the glue
    upstream crates compile, the sources `alab/` carries, and each upstream line with an
    `ALAB` marker); what it finds in upstream's code is set aside, and a diagnostic over a
    hook line, a target that does not build, or a linted crate's build script that does not
    run is judged against pristine upstream;
  - every upstream library crate under `codex-rs/` with a hook passes its unit tests
    (`--lib`), judged against pristine upstream: a test that fails there too does not gate.

  Upstream's CI does not run here.

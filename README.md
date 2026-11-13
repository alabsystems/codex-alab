<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# codex-alab

codex-alab is ALab's edition of OpenAI's [Codex CLI](https://github.com/openai/codex): the
same `codex`, built from OpenAI's open-source code with the Trust toolchain, merged with
upstream's `main` at least daily, and carrying a thin layer of ALab changes on top.

**Unofficial.** codex-alab is not made by, affiliated with, or endorsed by OpenAI. Codex is
OpenAI's product; report a codex-alab problem here, not to OpenAI. For OpenAI's own build, see
[openai/codex](https://github.com/openai/codex).

## A drop-in `codex`

- The binary is `codex`, with upstream's subcommands and flags. You sign in with your own
  ChatGPT account or API key, and it talks to OpenAI's service as `codex` does.
- It reads and writes the same `~/.codex` (or `$CODEX_HOME`): `config.toml`, `auth.json`,
  sessions, logs. `codex exec`, `codex app-server` and MCP server configuration work
  unchanged, and so do the TypeScript and Python SDKs (`sdk/`), which drive codex through
  `exec --experimental-json` and `app-server`.
- Every ALab choice is a packaged default, the lowest config layer: your `config.toml`, a
  profile, a project config or `-c` wins over each one.

[`alab/smoke/dropin.py`](alab/smoke/dropin.py) checks this against OpenAI's own build of
codex, with a scratch `CODEX_HOME`: the `--help` tree (74 pages on 2026-10-16, where the only
two differences were flags upstream itself added since that vendor version), `codex exec
--experimental-json` against a local mock model, `app-server` over stdio, `mcp list`, a
`config.toml` override of an ALab default, the files left in `CODEX_HOME`, and `auth.json`.

## Which OpenAI Codex it is built from

Each build names its upstream base apart from its own version: the newest OpenAI Codex
stable release whose branch point it has merged, and the exact
[openai/codex](https://github.com/openai/codex) `main` commit it merged last. On
2026-10-21 that is `rust-v0.163.0` and `e8aa7c1678ca`, 241 upstream commits past the
release's branch point.

- **In codex:** the launch card, the session header and `/status` read `codex-alab
  v0.164.0-dev.12814 · based on OpenAI Codex 0.163.0`, for example. Primary labels keep the
  release and build number, leaving the opaque source hash to version details. The launch
  banner names the readable ALab version and says `based on OpenAI Codex 0.163.0` where
  it has room. `/alab version` (also `/alab update`) shows the full running and installed
  package versions, update state, and full ALab and OpenAI source commits for the running
  build. `codex --version` still prints the full `codex-cli <version>`, as upstream's does.
- **In a package:** `alab-build.json` at the package root (`aterm pkg which codex` names
  the package's `bin/codex`) records it:

  ```json
  "upstream": {
    "release": "rust-v0.163.0",
    "commits_since_release": 241,
    "commit": "e8aa7c1678caa9a368243d72bf0e9b7859d97f15",
    "commit_date": "2026-10-20T13:50:13Z"
  }
  ```

  A package built before 2026-10-21 has no `upstream` block, and one built on 2026-10-21
  has no `commits_since_release`.
- **On the release pages:** `alab/package.sh --upstream-base [--commit <rev>]` prints the
  line that names it, `Based on upstream openai/codex rust-v0.163.0 + 241 commits
  (e8aa7c1678ca, 2026-10-20)`. A source release's GitHub page carries that line for its
  source commit (`publish/release-body`, which the publication engine runs when it creates
  the release), and a package release's page (`atpkg-codex-<build>`) the same line read off
  the package's `alab-build.json`, without the count for a package that records none. The
  releases published before the pages named it (`v0.164.0`, `atpkg-codex-2026102001`) name
  none, and a published page is never rewritten ([`alab/README.md`](alab/README.md),
  "Publication").

## What ALab changes

Defaults ([`codex-rs/alab-core/defaults.toml`](codex-rs/alab-core/defaults.toml)) turn off:

- the update check (it polls GitHub and Homebrew every 20 hours; a codex-alab install moves
  to a new package, never to OpenAI's build);
- product analytics, Statsig metrics and `/feedback` uploads to OpenAI's Sentry;
- the shared app-server daemon (1.5 GB of package copies, a process that never exits, and
  OpenAI's own updater): each TUI embeds its app-server;
- ChatGPT apps and the plugin marketplace (a sync at every start and about 142 MB of caches),
  the skill-search shadow experiment, the image tool and tool-install suggestions;
- tips (and with them the desktop-app ad), the server-version notice and automatic recaps.

They also cap `history.jsonl` at 8 MiB. One-line hooks in upstream files stop what config
cannot reach: the announcement feed fetched at every launch, `git status`, HEAD and remote
URLs in each turn's request metadata, the "Co-authored-by: Codex" git-attribution extension,
the 4 MiB TRACE log ring, plan-ad buttons, the optional "try the new model" screen, and a
background model-catalog refresh every 4.5 minutes per tab (hourly instead; a launch and a
`codex exec` still read the catalog online once). Measured by the drop-in check on
2026-10-23: on its first `codex exec` in a fresh `CODEX_HOME`, OpenAI's build cloned the
plugin marketplace and unpacked 37 bundled skill files; codex-alab wrote neither, and wrote
nothing that OpenAI's build does not. Since 2026-10-27 codex-alab carries the same bundled
`imagegen`, `openai-docs` and `skill-installer` skills again (Vanilla mode must match
OpenAI's build) and unpacks them under `skills/.alab-system`; the marketplace is still
never cloned.

Removed from the tree: OpenAI's CI, Bazel build and npm/installer distribution
([`alab/removed-paths`](alab/removed-paths)).

Added:

- **The ALab interface.** An account, subscription and usage footer; `/alab account` for
  allowance details; `/tasks` for local work; and `Ctrl+S` / `/stash` for parking a prompt.
  It is on by default; `/alab on` and `/alab off` select the presentation. The
  [interface guide](alab/interface.md) covers the controls and task checklist setup.
- **Capacity recovery.** Model-at-capacity failures retry automatically with backoff,
  retaining the selected model, current input and completed work during responses and
  foreground context compaction. Malformed or timer-unsafe retry advice falls back to
  normal backoff. Interrupt the turn to stop waiting.
- **The rainbow kitty.** A launch banner: aterm's codex kitty riding aterm's spectrum, with
  "bloatware-free · nag-free · optimized for aterm" beside it, what was measured against
  OpenAI's Codex 0.164 as shipped on 2026-10-23 (about 6× less memory, no background daemon, 1 HTTPS
  connection open at idle instead of 5; `codex-rs/alab-tui/src/banner/claims.rs` holds the
  numbers and the date), and the upstream source the build is based on. Inside [aterm](https://github.com/alabsystems/aterm),
  Rainbow Kitty is also available through `/pets`, drawn from the binary and never downloaded.
  No pet is selected by default, leaving the full prompt width available.
- **A native aterm harness.** Inside aterm, codex-alab claims its own session over aterm's
  control socket and does itself what aterm otherwise does by reading the screen
  ([`alab/CONTRACT.md`](alab/CONTRACT.md)): it reports its state, answers its launch folder's
  trust screen under aterm's harness policy, and sends the terminal's context with `/ide`.
  Outside aterm none of this runs.

Every ALab line in an upstream file is marked `ALAB(<topic>)`; `alab/delta.sh --topics`
lists them. [`alab/README.md`](alab/README.md) is the process: the daily sync, the gate, the
layering rules and every decision behind the list above.

## Install

On Apple-silicon macOS, aterm's package manager, atpkg, distributes codex-alab as its managed
`codex`. The signed index selects the production build; install or update it with:

```sh
aterm pkg install codex
aterm pkg update codex
aterm pkg which codex             # the installed source and build
codex --version
```

Public `vX.Y.0` releases are source snapshots. Installed binaries carry a version such as
`X.Y.0-dev.<count>+g<sha>` even in the production channel; atpkg selects them by the signed
build number, not by comparing that version with OpenAI's. Other targets use OpenAI's build.
To choose OpenAI's build on this machine, run `aterm pkg update codex --vendor`; use
`aterm pkg update codex --alab` to return to codex-alab's channel.

For a local development build:

```sh
alab/setup.sh                    # once per clone: the upstream remote and its release tags
alab/package.sh                  # build the package (Trust toolchain; ~11 minutes, ~15 GB of target/)
aterm pkg link codex <pkgdir> bin/codex   # the command package.sh prints
aterm pkg unlink codex                    # restore the previously installed managed build
```

`package.sh` builds `codex`, its code-mode host and ripgrep with Trust, stages them with
upstream's own package layout, smokes the result and prints the link command. A local link
takes priority over managed updates. If no managed build was installed before linking,
run `aterm pkg install codex` after unlinking.
[`alab/README.md`](alab/README.md) ("Packaging through atpkg") has every step. aterm
installs the Trust toolchain.

New launches use an update immediately. Existing sessions adopt it when native live
upgrade can resume every thread safely. Older builds can defer that upgrade indefinitely
under Cruise; restart and resume them at a safe stopping point if they keep showing the
old interface.

## Turning an ALab default back on

Set the key in `~/.codex/config.toml`, as for any codex default. For example:

```toml
[analytics]
enabled = true

[features]
plugins = true
apps = true

[tui]
show_tooltips = true
```

Or for one run: `codex -c features.plugins=true`. `codex features list` shows what is on.

## License

Apache-2.0, as upstream: [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE) are OpenAI's, kept as
they ship. ALab's additions carry their own Apache-2.0 headers.

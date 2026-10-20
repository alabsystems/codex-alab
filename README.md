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
model-catalog refresh every 4.5 minutes per tab (hourly instead). Measured by the drop-in
check: on its first `codex exec` in a fresh `CODEX_HOME`, OpenAI's build cloned the plugin
marketplace and unpacked 37 bundled skill files; codex-alab wrote neither, and wrote nothing
that OpenAI's build does not.

Removed from the tree: the bundled `imagegen`, `openai-docs` and `skill-installer` skills,
and OpenAI's CI, Bazel build and npm/installer distribution
([`alab/removed-paths`](alab/removed-paths)).

Added:

- **The rainbow kitty.** A launch banner: aterm's codex kitty riding aterm's spectrum, with
  "ALab build of OpenAI Codex" under it. Inside [aterm](https://github.com/alabsystems/aterm),
  Rainbow Kitty is also the default `/pets` pet, drawn from the binary and never downloaded.
- **A native aterm harness.** Inside aterm, codex-alab claims its own session over aterm's
  control socket and does itself what aterm otherwise does by reading the screen
  ([`alab/CONTRACT.md`](alab/CONTRACT.md)): it reports its state, answers its launch folder's
  trust screen under aterm's harness policy, and sends the terminal's context with `/ide`.
  Outside aterm none of this runs.

Every ALab line in an upstream file is marked `ALAB(<topic>)`; `alab/delta.sh --topics`
lists them. [`alab/README.md`](alab/README.md) is the process: the daily sync, the gate, the
layering rules and every decision behind the list above.

## Install

codex-alab is built for Apple-silicon macOS (the one target the Trust toolchain ships), and
installs through aterm's package manager, atpkg, in place of OpenAI's build:

```sh
alab/setup.sh                    # once per clone: the upstream remote and its release tags
alab/package.sh                  # build the package (Trust toolchain; ~11 minutes, ~15 GB of target/)
aterm pkg link codex <pkgdir> bin/codex   # the command package.sh prints
aterm pkg unlink codex                    # back to OpenAI's build
```

`package.sh` builds `codex`, its code-mode host and ripgrep with Trust, stages them with
upstream's own package layout, smokes the result and prints the link command;
[`alab/README.md`](alab/README.md) ("Packaging through atpkg") has every step. aterm
installs the Trust toolchain.

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

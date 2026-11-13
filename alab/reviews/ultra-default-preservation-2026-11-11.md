<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Preserve an explicit Ultra default

The owner requested Ultra as codex-alab's startup default. The packaged
default was set in `a03d547f18`. A subsequent read of the installed binary's
`config/read` returned `medium`; the user's saved configuration also contained
`medium` again. The author of that later write is not established by the
available evidence, so the investigation did not overwrite it.

## Reproducible default replacement

The source has a path that replaces a configured Ultra default with Medium:

1. Selecting Ultra in the model picker emits `ApplyAdvancedReasoning`.
2. `on_apply_advanced_reasoning` asks
   `default_reasoning_effort_for_conversation_model` for the default to retain.
3. That method unconditionally excludes a configured Ultra and chooses the
   model's ordinary default instead.
4. The returned value replaces the in-memory default and the event handler
   persists it to the config file. The active conversation still uses Ultra.

Upstream's `conversation_reasoning_uses_compatible_default_for_new_threads`
test explicitly expects configured Ultra to become Medium for the same
`gpt-5.5` model. This proves the source behavior; it does not prove which
process wrote the particular user's file.

## Decision and implementation

In active codex-alab, preserve an already configured Ultra default when the
selected conversation model equals the configured default model. A
conversation-only choice must not silently lower that existing startup
default. Keep the original compatibility selection for a different model,
other configured efforts, Vanilla mode, and upstream binaries.

The policy lives in `codex-rs/alab-tui/glue/reasoning_defaults.rs`. The
upstream-owned `tui/src/app/config_persistence.rs` has a module hook and a
single call into that policy. The existing event handler persists the
returned value; no second config writer or configuration format is added.

The isolated tests exercise the actual `on_apply_advanced_reasoning`
boundary and its return value for persistence, the new-thread configuration,
and the active conversation. Separate processes contain ALab activation so
upstream's tests retain their original behavior. Cases cover same-model
Ultra, a changed model, Low, Vanilla, and an inactive upstream process.

`alab/delta.sh --topics` could not run because this clone has no `upstream`
remote or tracking ref. The hook diff was inspected directly against the
latest committed sync's upstream parent. No upstream formatting or existing
test expectations were changed.

## Validation

Trust's unverified test lane compiled the TUI and passed the two
`alab_ultra_default` harness tests, including the five isolated cases above.
The three existing `conversation_reasoning_` tests also passed unchanged.
The new files passed `trustfmt --check` and the diff passed `git diff --check`.

The first dependency fetch stopped making progress on a public Chromium
source connection. After ending only that owned test process, retrying with
`CARGO_NET_GIT_FETCH_WITH_CLI=true` completed the fetch and build. Logs are
`codex-ultra-default-tests-cli-fetch-20261111.log` and
`codex-ultra-default-upstream-tests-20261111.log` under `$HOME/.aterm/.ubd`.
The compiler was the installed Trust build selected by the `targo` shim
(`targo 0.11.0+g20565c325`); these are test results, not a verified-lane proof.

After identifying and fixing the reset path, the saved setting was restored
to the owner's requested `ultra`. A fresh installed app-server's `config/read`
confirmed `gpt-6.1-sol` and `model_reasoning_effort = "ultra"`. This verifies
current startup configuration; it does not claim that the installed package
already contains the new TUI hook. The source fix takes effect when the
updated package is adopted. The metadata-only startup result is retained as
`codex-m3-registry-proof-20261111.noindex/ultra-startup-restored.json` under
the same private evidence directory.

Publication initially raced main advancing to `75f737dac6`. That commit
changes documentation and comments only, with no overlapping hook changes.
The topic was rebased onto it; the existing test executable passed both
filters again. The executable was not rebuilt for those comment-only edits.

## Persistence and installed adoption follow-up

The initial regression covered the configuration value returned to the
existing writer. Extend its isolated harness with a complete round trip:
write Ultra into a temporary home, dispatch `ApplyAdvancedReasoning` through
the real app event handler and embedded server, read the saved TOML, shut
down, reconstruct configuration from disk, and start a fresh server/thread.
The test must still observe Ultra at each boundary. It never uses the
person's home or credentials.

Pair that case with an inactive-upstream control using the same initial
Ultra file. The control must save Medium and restart with Medium while the
active conversation stays on Ultra. A handler that silently did nothing
would leave the initial file untouched and fail this control. Both cases
use the 8 MiB test-thread stack already specified by `alab/unit-tests.sh`;
the first direct invocation's smaller default stack overflowed in the full
embedded-server fixture, and the same executable passed with the standard
stack size.

`aterm pkg update codex` reported that managed build `2026111101` is current
under index 128. `alab/local_update.py --status` had no local update run
recorded. Do not replace the managed channel with a development link merely
to make the source fix appear installed. Package publication and adoption
remain separate from the source and test results recorded here.

The final formatted source passed the two `alab_ultra_default` harness tests,
now covering seven isolated cases, including both save/restart cases. The
control saved and restarted with Medium; the ALab case retained Ultra, and
both active conversations remained on Ultra. Validation used Trust's
unverified lane with `--locked --offline`. The retained log is
`$HOME/.aterm/.ubd/codex-ultra-persist-restart-final-20261111.log`.

Publication then raced the input-custody changes through `9606493229`.
The test was rebased without conflicts and rebuilt against that integrated
source. Both harness tests and all seven isolated cases passed again;
`codex-ultra-persist-restart-integrated-20261111.log` records that run.

<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Target-aware native test prerequisites

The reviewed Unix TUI statusline integration control needs the actual `codex`
CLI and `test_stdio_server`. The maintained package-only planner did not derive
either for `codex-tui`; CLI preparation was embedded in the MCP library runner.
The five newer upstream commits also select 28 TUI library declarations, whose
exact bodies and traced fixtures establish no new CLI/stdio helper requirement.
They must not gain integration prerequisites merely because they share a crate.

This isolated change starts at published
`bef9ffc4faaf418899e21c838b9581be11e9f3c4` (tree
`3b0293479e710df3d601ab2cb01d4a2a38061b07`). It changes ALab-owned tooling only.
The prior source proposal is frozen separately in
`codex-tui-prerequisite-obligations-267-20261117/index.json`, SHA-256
`aad922cef7a0bb1eca0db36e7c93cfb25b7d7a140f362463d299fdfdf49e49ff`.

## Planning and preparation

`native_test_requirements` requires an explicit `lib` or `default` scope, or
`test all`. It validates nonempty `-p package` pairs, package-name syntax and
scope arity. Named integration selection is deliberately limited to
`codex-tui` / `test all`; unsupported package/target combinations refuse.
Package existence and actual target/artifact identity remain separate native
Cargo/metadata responsibilities; strings in this pure plan grant no authority.

The existing callers use the plan directly: `unit_tests` selects `lib` before
allocating result/helper state, while `all_alab_tests` selects `default` before
allocating helper state. The same five caller-local flags, including new
`need_cli`, control allocation and shared preparation. TUI library planning
leaves all helper flags off. Explicit TUI `test all`, and default targets that
include it, plan metadata, CLI and stdio; existing V8/host derivation is unchanged.
Repeated packages union these flags without duplicate helper builds.

The MCP CLI build, actual artifact selection and exact `--version` warmup now
live in shared preparation. The CLI build remains separate from the existing
stdio/host build, preserving the feature-graph distinction documented in
`native-helper-artifact-identity-2026-11-12.md`. Recorded native and CLI artifact
streams remain separate. Plain library scratch and its metadata are still
owned by the existing library caller's EXIT trap.

`native_helpers.py` is unchanged: selection still requires the actual source
workspace, target, unique package/bin metadata and matching non-test executable
artifact. Existing `tree_config`, build admission, normal compiler/profile,
target containment and helper environment aliases remain in place. The selected
CLI warmup precedes publication of its path to the test command. A failed
selection or startup prevents the library test command.

## Preserved execution boundaries

This adds no integration runner. `unit_tests` still uses `--lib` for preparation
and execution, with the exact original result/round suffix. `unit_results.py`
still accepts library artifacts only; `check.sh`, its candidate/pristine
selection, `native_helpers.py`, defaults, manifests, locks, pins and upstream
files are unchanged. `all_alab_tests` keeps its full default Cargo target scope.
No statusline feature, remote account, held route or permission is activated.

The integration control itself remains reviewed upstream source, absent from
this source comparison. Its future execution needs a separately reviewed
integration runner/result contract and actual selected-scope dependency
preparation. This change does not establish that CLI/stdio are every transitive
prerequisite, that compilation needs no V8/host inputs, or that an application
is qualified.

## Validation

Eleven new Python unit declarations exercise the real shell planner and
preparation functions with generated Cargo/helper mocks and scratch source
trees. They cover validated scope/package shapes, TUI library versus integration
plans, mixed MCP/native plans, separate build argv and logs, source/target-bound
selection, missing/ambiguous/foreign/harness/escaping artifacts, exact mock CLI
warmup and aliases, library/default caller argv, refusal before result
allocation, and caller trap/cwd plus scratch cleanup on success/failure.

The mocks compile nothing. Only generated mock CLI programs receive the
existing `--version` warmup; no project Cargo, compiler, native helper, metadata,
version, V8 resolver, Rust test, or application is executed. The existing two
planning fixture methods receive the explicit `lib` scope and fifth flag; their
larger gate suite is unrun here because it requires the actual native unit
runner. Shell syntax, Python parsing and diff whitespace checks pass. Full native
compatibility, library receipts and integration execution remain unrun.

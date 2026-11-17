<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->

# Status observation CLI completion

Decision: guard only the already-parsed upstream status CLI operation with the
existing `cli_status.run_operation`. Preserve raw APIs and completed observation
semantics. The implementation base is published
`497a194a0a0ad1110a9756a988641a60564d8a4f`; this three-path topic uses one
private worktree, handed to root for composition after review and commit.

## Contract and proof

This command observes local source/intake state; it is not an admission gate.
A completed report with `pending_count > 0`, missing intake or
`intake_complete: false` legitimately returns 0 when both `result.errors` and
`assessment.errors` are empty. Either error list makes the final status 1.
Caught assessment failures retain completed source JSON and add an assessment
error. `--intake` without `--base` is a handled operation error returning 1
with no JSON, not a parser error. Help remains 0 and parser errors remain 2.

The initial audit ran 24 finite owned Python children through pipes during the
disk refusal: 19 actual original entries and five direct raw-main calls. Eight
entries exited 0 without main completing or JSON appearing: report exits 0/256,
assessment exit 256, stderr exit 0 after a report error, stderr exit 256 after an
assessment error, healthy-report stdout exit 256, source-error stdout exit 0 and
formatter exit 256. Root separately corroborated healthy pending status 0 and
an incomplete report exit 256. Those are separate samples, not added test counts.

The real original parser, main, output and `sys.exit(main())` AST ran. Report and
observe boundaries were intentionally synthetic. Reaching the actual assessment
branch used exactly two fixed in-memory Git-return seams, followed by real pure
TOML/catalog parsing. Actual Git processes, native/application/guardian execution,
remote operations, threads, sockets and raw PID/group signaling were forbidden
and independently counted zero. The observations establish process completion
status, not native qualification, source authentication or adoption.

The observed old source SHA-256 is
`9b3fb1541f0d2917f3499699ea74d2dec2259dd9a5f7a2e88088b6e294e73b92`.
It matches the literal implementation base object. The audit context was
candidate 6f75acd30f7f5d7bcb694dbb799e254b8d7b8a05; source-content bookends
match, without claiming a frozen whole mutable checkout. Its original-entry
AST hash is
`bd8e2b32f65cc70966b2cd03bb3d697f6c65dc9380580bb8b3f639cf19fcda1a`:
SHA-256 of UTF-8 `ast.dump(ast.Module(body=[original_guard], type_ignores=[]))`
with Python 3.14 defaults, including the Module wrapper, no attributes or indent.
The initial disk-blocked observations remain in the conversation tool transcript;
no temp script, artifact, cache or bytecode was created for that audit.

## Implementation and compatibility

`_arguments()` retains parsing before the CLI guard. Raw `main()` delegates to
`_run(_arguments())` without guarding cancellation. `cli()` parses once and
passes only `_run(args)` to the exact sibling shared helper; the original entry
calls `cli()`. The existing successful-exit classification and diagnostics
policy are reused without modification.

Literal-base AST checks confirm unchanged complete `report`, `observe` and
`contains` functions, parser body and operation body. Global report/observe
lookups remain late. The operation still owns `result = None`, the internal
intake/base check, original handled exception tuple, `upstream status: ` prefix,
partial JSON retention and the exact errors-OR-assessment-errors formula.
Genuine nonzero/keyboard cancellation and uncaught raw RuntimeError retain exact
objects. No schema, receipt, cap, timestamp, protocol, qualification or native
policy changes.

The existing shared child harness adds status as its fourth consumer instead
of duplicating the runner. All prior 223 control signatures remain. The extended
20-method module has 343 finite child controls, including report and assessment
cancellation, ordinary/formatting/sink faults, source and nested assessment
errors, legitimate incomplete observations and raw API behavior. New source
and assessment output-failure cases use separate progress/error witnesses.
The child boundary and forbidden-operation counts are inspected independently.

The maintained literal-base replay produces 45 expected upstream-status zero-exit
failed subcontrols across six methods, with no errors or skips. All prior 223
feature/review cases and compatibility controls pass. This 343-control replay
is distinct from the earlier 24/8 audit and root's two-control corroboration.
Source-bound replay and result artifacts are under
`.codex/.ubd/upstream-status-cli-implementation-20261116`.

The direct sibling helper source was already loaded through qualification and
included in the relevant protected/copy closures; no copied-input update is
needed. No library/import compatibility fallback is introduced.

## Verification

Managed Python 3.14 passes 67 unique selected methods in 27.721 seconds under
`-I -B`, with zero failures, errors or skips. Standard loader patterns are
`test_feature_cli_status.py`, `test_upstream_status.py`,
`test_catalog_feature_ids.py` and `test_artifact_diagnostics.py`, plus exactly
three `QualificationTests`: qualification-to-status fields, invalid schema
refusal, and retained source status on unavailable snapshot capabilities. IDs
are checked for uniqueness. The actual self-signal qualification test is excluded.

The new negative child controls execute no actual Git/native/application work.
Existing selected component fixtures use finite local Git and Python/shell
commands; the three qualification fixtures use a tiny owned shell gate, not the
native gate/compiler/application. No full Python workflow, native prerequisite
waiver or native image qualification is claimed. Existing Python 3.11 TOML
support is required; no Python 3.9 result or shim is added. Root reuses this same
worktree after its clean handoff, without another validation checkout.

Both independent reviewers approved all three frozen paths, including the full
shared 20-method/343-control harness and note/closure. Neither authored or edited
this topic. Each independently passed the same 67 unique managed Python 3.14
methods with zero failures, errors or skips: 27.768 seconds and 27.720 seconds.
All three frozen hashes matched before and after each run, AST equality checks
passed and diff checks were clean. Root reviewed all three paths and independently
verified the 343-observation baseline, 45 incomplete CLI zero exits and preserved
223 prior controls. Only this attribution was appended after those checks.

## Combined verification

After the clean handoff, root reused the same worktree and ran the fixed
combined selection at committed source
`d72d3cd6d0e3ad15893d2532e3f3a114a9d93c9f`. All 808 unique managed Python
3.14 methods passed in 195.371 seconds under `-I -B`, with zero failures,
errors or skips, identical HEAD bookends and a clean worktree. This retains
the prior 768-method scope, adds five shared CLI methods and the full
27-method upstream-status and eight-method artifact-diagnostics modules.
The catalog and three qualification/status methods were already included;
no IDs were duplicated. The actual self-SIGTERM qualification method and
four capacity-process guardian methods remain outside this selection.

The private log is
`~/.codex/.ubd/upstream-status-cli-composition-tests-20261116.log`.
A fresh maintained fetch still resolved remote main to the exact base
`497a194a0a0ad1110a9756a988641a60564d8a4f`; no incoming source needed
composition. This result establishes selected component behavior. The native
frontend, unit-runner and CPU-authority prerequisites remain unavailable;
no full workflow, application certificate, package or installed adoption
is claimed.

The first guarded publication refused because main advanced. Root fetched the
new main `d70f61698a3e7c63e43d59f0dfe4acca359ee454` and preserved its four
request-timeline paths in merge
`15a3c219e991a3711c0660c1b48193329bb21868`. Those changes leave the selected
808-method inputs unchanged; the standalone timeline module has no other
Python importer. Root separately passed all 37 timeline methods at the clean
merge in 0.796 seconds, zero failures, errors or skips, with matching HEAD
bookends. The 808-method result remains attributed to `d72d3cd6`. The two
results do not imply a full native workflow run.

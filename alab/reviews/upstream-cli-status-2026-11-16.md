<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->

# Upstream review CLI completion status

Decision: use the existing parsed-operation CLI guard for upstream review
admission while preserving its raw library and `main()` interfaces. The base
is `1023d7db0ff45c5ca08f1bece9e1b4284e9a31be`. This is a three-path topic
using one private worktree; that same worktree is handed to root for composition
and final checks rather than creating a separate validation checkout.

## Contract and bounded proof

`check.sh` observes the upstream review CLI status as a prerequisite. Normal
JSON results return 1 for either `errors` or `missing`, and 0 only when both are
empty. The existing isolated CLI regression separately requires missing reviews
to produce status 1 and a JSON `missing` list. Help remains 0 and parser errors 2.

The read-only audit used 25 finite owned Python child controls: 17 original
entry executions and eight raw-main calls. Only the admission boundary was
intentionally synthetic; the actual parser, operation, diagnostic/JSON output
and original `sys.exit(main())` AST executed. Eight original entries exited 0
without main completing: operation exits 0/256, stderr exits 0/256 after an
ordinary admission failure, healthy-result stdout exit 0, returned-errors stdout
exit 256, returned-missing stdout exit 0 and formatter exit 256. The last two
returned refusals emitted no JSON. Healthy, errors-only and missing-only results,
help/parser errors, genuine nonzero/keyboard controls and raw API identities
were observed separately. Child actor counters were independently all zero.

The actual audited source SHA-256 was
`544ef08d48c48764c0e4984411a8ef63bf0d07d8742e60c6260c1e696d99a8a2`;
its original entry AST SHA-256 was
`bd8e2b32f65cc70966b2cd03bb3d697f6c65dc9380580bb8b3f639cf19fcda1a`.
The source matched the literal base Git object and file-content bookends across
all children, without asserting a frozen mutable checkout. Private audit
artifacts are under `.codex/.ubd/upstream-gate-cli-audit-20261116`.

This is an upstream CLI completion defect, not proof of full-gate bypass or
native qualification. The immediately following feature admission stage calls
raw review admission again and refuses errors or missing reviews. Feature
snapshot with a base also repeats review admission before scoping. Those checks
and the actual admission algorithm remain intact.

## Change and compatibility

`_arguments()` retains the parser outside the status guard. Raw `main()` calls
`_run(_arguments())` without catching cancellation. A separate `cli()` parses
first and guards only `_run(args)` through exact sibling
`cli_status.run_operation`. The original entry invokes `cli()`. No admission
algorithm, helper policy, schema, receipt, cap, timestamp or budget changes.

A private literal-base AST comparison confirms unchanged admission, parser and
operation bodies. The operation still looks up global `admission` at call time,
uses the original handled exception tuple and `upstream review admission: `
prefix, prints the same JSON, then returns 1 for `errors` OR `missing`. Ordinary
uncaught RuntimeError and all raw API cancellation objects retain identity.
The existing native success-shaped exit classifier is used unchanged; genuine
nonzero and keyboard cancellation retain their original behavior. Parser help
and errors are evaluated before operation observation.

The extended shared test harness covers all three consumers rather than adding
a second child runner. All 145 existing feature control signatures remain; the
combined module has 15 methods and 223 finite child controls. New upstream
controls separately cover errors-only and missing-only JSON/status refusals,
interrupted missing-only output, success-shaped operation/diagnostic/formatting
exits, genuine controls, raw API identity and the ordinary RuntimeError boundary.
It executes actual original entry ASTs and records real OS statuses, progress,
identity, unchanged source/input files and independent forbidden actor counts.
The original feature snapshot/bundle and semantic parser controls remain.

Against literal-base source files, the extended module produces 30 expected
failed upstream zero-exit subcontrols across five methods, with no errors or
skips; compatibility controls and all existing feature cases pass. The old
25/8 audit and new 223-control replay are distinct counts. Raw API zero-shaped
SystemExit observations remain intentionally separate from CLI completion
failures. Private replay/results are under
`.codex/.ubd/upstream-cli-status-implementation-20261116`.

The new direct sibling helper load is already covered by the protected input
and copied gate fixture closures. No fixture copy or protected-input addition
is needed. The sole worktree is used for review and handed off clean after the
three-path commit. No native runner prerequisite is bypassed.

## Verification

Managed Python 3.14 passes 154 unique selected methods in 61.965 seconds under
`-I -B`, with zero failures, errors or skips. Standard loader patterns are
`test_feature_cli_status.py`, `test_upstream_gate.py`, `test_upstream_intake.py`,
`test_catalog_feature_ids.py`, `test_feature_metadata_snapshot.py`,
`test_feature_log_snapshot.py`, `test_feature_evidence.py`, `test_feature_gate.py`,
`test_feature_bundle_custody.py`, `test_feature_lifecycle_contract.py`,
`test_feature_scenario_contract.py`, `test_feature_snapshot_ids.py` and
`test_focused_results.py`. All selected IDs are checked for uniqueness.

The feature/upstream tools retain
their existing Python 3.11 TOML prerequisite; no Python 3.9 shim or result is
claimed. New child controls run no Git, native application/compiler/guardian,
remote operation, thread, socket or raw PID/group signal. Existing adjacent
fixtures may use finite local Git and Python/shell commands, without native
compilation/application/guardian or remote qualification. These are selected
component checks, not the full Python workflow or native gate.

Both independent reviewers approved all three frozen paths, including the
complete shared 15-method/223-control harness and note. Neither authored this
topic or created a separate validation worktree. Each independently passed the
same 154 unique managed Python 3.14 methods with zero failures, errors or skips:
66.143 seconds and 64.053 seconds. All three file hashes matched before and after
each run, AST extraction assertions passed and diff checks were clean. Root
reviewed and approved all three paths. Only this review attribution was appended
after those checks; the same worktree is handed to root for composition.

## Composed validation

After the producer's clean handoff, root reused the same worktree and tested
committed source `af097c6ef0a7ce4e652a05f12d56f11b143c9a47` on managed Python
3.14 under `-I -B`. All 768 selected methods passed in 134.655 seconds, with
zero failures, errors or skips, identical HEAD bookends and a clean worktree.
This includes the prior 748-method component scope, the two added shared CLI
methods, and the complete upstream-intake and catalog-ID modules. Existing
feature CLI cases and release-helper binding controls remain included. The
actual self-SIGTERM method and four capacity-process guardian controls remain
outside this selection. The private log is
`~/.codex/.ubd/upstream-cli-composition-tests-20261116.log`.

Separate discovery at that source found 2,140 unique IDs and zero import
errors; no test bodies or native prerequisites ran during discovery. A fresh
maintained fetch still resolved main to the exact implementation base
`1023d7db0ff45c5ca08f1bece9e1b4284e9a31be`. The 06:04 UTC resource query
still reports `CPU slot file identity changed`, and neither the qualified
frontend nor native unit runner resolves through PATH or configured environment.
No complete Python workflow, native gate, application certificate, package or
installed adoption is claimed by this selected component run.

Publication initially stopped during local transport preparation while the data
volume rejected writes. Root retired an unlocked, clean, fully published old
validation checkout through ordinary Git. The native cleaner then independently
reclaimed eligible build output; no cache custody or resource guard was bypassed.

A later maintained fetch resolved main to
`5a6c49b39db8093349bf6ce1580063ce0114e201`. Root preserved those four incoming
request-timeline source/test/documentation paths in merge
`b82874ff128da6315d8fc114b29b12eb70f97da5`. They do not change the selected
768-method inputs, and no other Python module imports that standalone reader.
The prior component result remains attributed to `af097c6e`; it is not relabelled
as a new full run. Root separately passed all 34 incoming timeline methods at
the clean merge, in 0.737 seconds, with zero failures, errors or skips and
identical HEAD bookends. No additional native qualification follows from this
composition.

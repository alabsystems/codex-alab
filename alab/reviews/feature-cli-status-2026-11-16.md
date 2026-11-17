<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->

# Feature CLI completion status

Decision: guard only each already-parsed command operation with the existing
`cli_status.run_operation`. Preserve the programmatic `main()` interface and
its exact cancellation objects. The implementation base is
`2be3f27af9de70d9a1f0bc684d7aa6550c7c51b3`.

## Observed contract and proof

`check.sh` invokes `feature_gate.py` as a prerequisite before compilation and
uses its process status to refuse failed affected-feature admission. The feature
evidence CLI likewise returns 0 for completed snapshot, validation or bundle
work, 1 for handled refusals and 2 for parser errors. An interruption that happens
to request an OS-success exit must not make incomplete work appear complete.
These are CLI completion and prerequisite-status contracts, not a new evidence
schema or a claim of native qualification.

Three bounded audit sets are distinct:

- The feature evidence audit made 13 original-entry Python child invocations.
  Five exited 0 without finishing the operation or its diagnostics: operation
  `SystemExit(0)`, `SystemExit(None)` and `SystemExit(256)`, a stderr write exit
  and a diagnostic formatter exit. Healthy validation/snapshot/help, parser
  errors, handled refusal/error and genuine nonzero exits provided controls.
- The independent gate audit made 37 original-entry child invocations, finding
  14 incomplete zero exits. These include interrupted JSON output after
  admission had already returned a refusal. The root's separate two-control
  sample corroborated the gate status gap; it is not another full matrix.
- The maintained new module replays 145 controls against the literal base
  source: 45 subcontrols fail across four of its 13 methods, with no errors or
  skips. All 45 failures are actual old zero OS exits where the test requires
  failure. The other compatibility methods pass. This replay is separate from
  the earlier 13/5 and 37/14 audit counts.

Both audits execute the actual original `__main__` AST with the actual parser,
operation and `sys.exit`, intentionally controlling only the snapshot or
admission boundary. Evidence validation uses real owned closed JSON/log files
with explicitly synthetic declarations; no declared test command runs. The
new replay additionally exercises real exclusive snapshot and bundle writes.
Owned Python child processes are finite. Independent actor counters exclude
Git, native application/compiler/guardian execution, remote access, threads,
sockets and raw PID/group signals inside each new child.

The audited Python file SHA-256 values, unchanged between requested source
4232d6a7 and observed base 2be3f27, are:

- `feature_evidence.py`:
  `105583520a8e94d307733a3145fe4586fbb463bff4ea313834c9e5229e27b2e4`.
- `feature_gate.py`:
  `a68a950d8417fce4101190e9d15a0a11ab6e06923c95ffd85ddd277a460cfa0e`.

Each old final entry AST hashes to
`bd8e2b32f65cc70966b2cd03bb3d697f6c65dc9380580bb8b3f639cf19fcda1a`.
The audit and baseline use file-content bookends, not a claim that the mutable
main checkout was frozen. Private artifacts are under
`.codex/.ubd/feature-cli-status-audit-20261116`,
`.codex/.ubd/feature-gate-cli-audit-20261116` and
`.codex/.ubd/feature-cli-status-implementation-20261116`.

## Change and limits

Each `_arguments()` retains the old parser and semantic parser checks outside
the status guard. `main()` calls `_run(_arguments())` directly; raw API ordinary
behavior and cancellation identities remain intact. `cli()` parses first, then
passes only `_run(args)` to the exact sibling shared status helper. The original
entry now invokes `cli()`. The operation bodies, diagnostic prefixes, JSON and
normal return codes are unchanged. A private root AST comparison against the
literal base confirms both parser and operation extractions are identical.

The shared helper turns incomplete no-code or native success-shaped integer
exits into CLI status 1, including native POSIX wrap cases. Genuine nonzero,
noninteger exit payloads and keyboard cancellation keep their existing
exception/OS behavior. Failure reporting cannot restore success if its sink
also requests a successful exit. Help remains 0, syntax and semantic parser
errors remain 2, and both occur before operation observation. This does not
change how evidence is admitted, source binding, schemas, budgets, log formats,
metadata text decoding or any native command.

The existing metadata resolve-cause observer now follows the actual `_run`
frame. It retains its exact cause assertion rather than observing an inactive
wrapper. The new direct sibling CLI helper loads use a source already in the
relevant protected input lists and copied gate fixture closure. No protected
input or copied-source fixture additions are needed.

## Verification

The new 13-method module passes all 145 finite child controls on managed Python
3.14. It checks actual original-entry OS exits, operation progress, raw API
identity, persistent diagnostic sinks, formatting and output failures, returned
gate refusal, healthy JSON/announcements and exclusive snapshot/bundle creation.
The fixtures independently inspect forbidden actor counters and original
record/source bytes. They do not infer success from a mocked exception alone.

Managed Python 3.14 passes 116 unique selected methods in 51.603 seconds, with
zero failures, errors or skips under `-I -B`. The standard loader selects
`test_feature_cli_status.py`, `test_feature_metadata_snapshot.py`,
`test_feature_log_snapshot.py`, `test_feature_evidence.py`, `test_feature_gate.py`,
`test_feature_bundle_custody.py`, `test_feature_lifecycle_contract.py`,
`test_feature_scenario_contract.py`, `test_feature_snapshot_ids.py` and
`test_focused_results.py`; selected IDs are checked for uniqueness.

Python 3.11 TOML support is an existing prerequisite; an accidental
system Python 3.9 invocation stopped at the existing `tomllib` import and is not
a product regression result. No compatibility shim or Python 3.9 claim is
added. Existing selected feature fixtures may execute finite local Git and
Python/shell component commands. These runs do not execute the native gate,
application/compiler or guardian, and do not qualify a native image or an
entire source checkout.

Both independent reviewers approved all five frozen paths, including the full
new 13-method/145-control module, both tools, metadata observer and this note.
Neither authored this topic. Each independently passed the same 116 unique
managed Python 3.14 methods with zero failures, errors or skips: 50.645 seconds
and 45.793 seconds. All five frozen file hashes matched before and after each
run, and diff checks were clean. Root also reviewed and approved all five
paths. Only this review attribution was appended after those frozen checks.

## Composed validation

Root tested clean committed source
`2718353f126a4d8757a365dac852ec4c68869646` on managed Python 3.14 under
`-I -B`. All 748 selected methods passed in 124.741 seconds with zero failures,
errors or skips. Source HEAD bookends matched and the worktree remained clean.
This adds the new 13-method CLI module and two release-hook CLI-helper binding
controls to the prior 733-method component scope. The actual self-SIGTERM method
and four capacity-process guardian controls remain outside this selection.
The retained private log is
`~/.codex/.ubd/feature-cli-composition-tests-20261116.log`.

Separate discovery at that exact source found 2,138 unique IDs and zero import
errors; no test bodies or native prerequisites ran during discovery. Fresh main
`13fc8a15f9a7e872facf7ccba275548bb6808b49` adds one Rust test path and two
review notes. All peer work is preserved, with selected Python inputs unchanged.
Its separate 340-method reader-verification record retains its own 26b source
and is not added to this count; the Rust change receives no qualification from
this Python run. The actual 05:38 UTC native resource check still reports
`CPU slot file identity changed`; no qualified frontend or native unit runner
resolves through the current PATH or configured environment. No complete Python
workflow, application certificate, package or installed adoption was performed
for this topic.

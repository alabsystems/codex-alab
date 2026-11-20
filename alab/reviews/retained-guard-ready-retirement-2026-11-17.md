<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Check normal completion before destroying the retained future

The real retained Side owner controls previously checked tracker retirement only
after destroying their pinned future. That can conceal a completed future which
still owns its tracking token. The helper now checks the normal finished case
immediately after awaiting a mutable reference to the future to Ready, while the
future remains retained. It neither yields nor closes/waits on the tracker before
the assertion.

The initial change is six lines in the ALab-owned test helper. The three test
methods, five modes per method, production owner implementations and capture
reads remain unchanged. Cancellation, early return and observer-pending behavior
keep their previous destruction-based and pending-token checks. The new assertion
applies only to known normal completion; it introduces no stronger policy for an
unfinished early return.

Both production and the fixtures already clone the owner's non-Copy resource
inside their async blocks. Source review rejected an initial capture-mismatch
hypothesis. The earlier original-write baseline remains valid for its old
destruction-based contract; it is not rewritten as a reproduced production bug.

Decision: measure the strengthened current fixtures once before changing them.
If they already satisfy synchronous retirement, retain the minimal assertion.
Only a natural failure would justify aligning their normal-end drops with the
production drops already present. Do not predeclare a failure count, fabricate
a legacy defect, or substitute a different owner or Native authority.

Validation uses the existing source-qualified Trust unverified test lane,
protected profile, genuine runtime prerequisites and unchanged three-package
feature graph. Only the existing three actual-owner methods are selected, with
all 15 source-defined scenarios retained. Source/tool/script bookends, real test,
collector and guardian statuses remain separate. These are lower owner-lifetime
controls, not successful Native Side operations or application certification.

The actual run on `dc15e21bcc935b985d80fa3e9964b7f181fa97b7` passed. All three exact
methods completed successfully across the unchanged 15 source-defined modes.
Test, collector, coverage, source and guardian statuses were all zero; the
completed harnesses and exact method rows contained no failure or ignore.
The new normal-completion assertion passed in each method while the completed
future remained retained. No fixture-end drops or production changes were needed.

Decision: retain only the six-line assertion. The measurement establishes
stronger regression coverage of existing behavior, not a newly repaired
production defect. There was no RED result, no capture mismatch, no new Native
acceptance proof and no reason to repeat broader completed suites.

The qualified source-selected route used Trust 15698, the existing protected
profile and genuine V8, stdio, code-mode-host and recorded-runner prerequisites.
Source, tools, scripts and plan remained byte-identical before and after, with
12,406 immutable Git bindings. The test package was app-server within the same
three-host feature graph; the client and TUI siblings selected zero tests and
are not counted as positive coverage.

The original command was launched once and actually reaped with exit 0.
Closed summary: `codex-guard-ready-retirement-dc15e21b-closed-summary.json`,
SHA `c695283d56252fa715820693126e28009060ab290c07c6e961509ffd3d2cfa38`.
The per-run source/tool/script map SHA is
`3124d9c8512ac0c7b3b2d94b1363a190ee060b375f6e6ec1105aff2cdff10457`.
The earlier broader gate failure and source-specific unavailable upgrade coverage
remain unchanged; this focused unverified run grants no application certificate.

The closed result packet is `codex-guard-ready-retirement-dc15e21b-results.md`,
SHA `9ea13ddd2fffb1fe2f6d0ecb6db6f40462f6abdea0b87cb219df7cb65478cbeb`.
It preserves the original process capture and immutable source bindings; later
checkout changes do not alter that completed observation.

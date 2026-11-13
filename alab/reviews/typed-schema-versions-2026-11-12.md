<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Require integer schema versions at catalog and record boundaries

## Finding and decision

Python considers `True`, `1.0` and `1` equal. Equality-only schema checks accepted
boolean and floating-point values in TOML feature inventories and JSON intake
or evidence records. Valid review/evidence contents with those malformed headers
could still be reported as complete. This is a format-validation defect; it
does not mean the other source, review or execution requirements were bypassed.

Require the actual integer type and supported version at every boundary that
consumes these schema headers. The five catalog readers are feature inventory,
feature evidence snapshots, upstream admission, intake validation and the
status tool's optional review assessment. The latter two previously omitted
the catalog header check altogether.

Also require integer schemas on incoming intake and evidence records. Admission
and status compare whole-record immutable metadata before handling individual
commits, and need the type check there too. They deliberately select per-commit
validation findings; adding only a global error to the intake validator would
leave malformed headers accepted by those consumers.
Admission checks a relevant record's schema before processing supersession,
so a valid replacement cannot conceal an unsupported header on its predecessor.
Unrelated historical batches retain their existing skip behavior.

Keep each reader's existing payload requirements and source-selection behavior.
Status and review lookup need feature IDs; evidence snapshots need active
scenarios; inventory checks full ownership contracts. Do not make all readers
run full inventory validation. Paths that do not consume a catalog remain
unchanged: intake snapshot, status without a review assessment, and admission
with no new upstream commits. Generated schema headers remain integer `1`.
The committed catalog and all 20 committed intake records already use that
integer version; no stored-record migration is required.

## Validation

Independent scratch probes confirmed that boolean and floating-point schemas
were accepted by both JSON validators and their CLIs. Committed intake records
also produced a reviewed commit with no admission errors and a complete status
assessment. Injecting only the intake validator's global schema error did not
stop either consumer, confirming the need for their immutable-header guards.

Regression groups exposed 33 expected assertion failures before their
respective fixes, including malformed superseded records with a valid
replacement. The actual `check.sh` fixture admitted boolean/float evidence
headers to its first build-command stand-in before the fix and refuses them
after it. Integer `1`, reader-specific partial catalogs, and all three
no-catalog paths pass their positive controls.

All 167 selected workflow tests passed. The final supersession placement fix
then passed all 18 upstream admission tests, including the additional
supersession regression. The committed catalog and 20 historical intake
headers retain their existing valid format. No Rust code, native runner,
installed binary or qualification claim changed; the previously unavailable
17 native-runner tests remain outside these Python/Bash checks.

Those 17 deferred tests subsequently passed after the local helper was built;
see [the native runner recovery](native-unit-runner-recovery-2026-11-12.md).

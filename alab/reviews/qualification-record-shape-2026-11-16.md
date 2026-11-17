<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Require a literal clean after-snapshot

The observation reader required `before.clean is True` and equality between
the before and after dictionaries. Python treats numeric `1` and `1.0` as
equal to `True`, so either numeric after.clean value could pass that check and
populate the status report's qualified source despite a malformed snapshot.

Decision: independently require an after dictionary with `after.clean is True`,
while retaining snapshot equality and the other record, source, tree and log
checks. Invalid records leave qualified_sha unset and report a qualification
error; independently observed Git/source status remains available. This adds
no authentication, schema version, native gate or proof claim. The proposed
JSON-depth failure was not reproduced under the managed interpreter and was
explicitly withdrawn; no depth-handling change is included.

The same final 38 qualification and 16 status methods run from coherent exact
`794ff49328c17a6f14bae5adca06e051dc47e3e6` source with only the selected reader
changed in the fixed bundle. Baseline: each module exits 1 with two failed
subtest assertions in one parent method, exclusively numeric after.clean
values. Fixed: all 54 methods pass, both actual exits 0, no errors or skips.
True accepts; False, missing clean, null/list/missing snapshots refuse.
The producer control runs an owned temporary shell fixture. The report control
uses an explicitly synthetic local record to test reader behavior and source
status independence; it is not evidence of a native application gate.

The complete combined run uses clean committed source
`80adcc3dd308f966ed24889d11ee38732ba27fdf`, tree
`ffcf5140a793983c930a1a767fe8765a36b5d46b`: 38 qualification, 41 cleanup/CLI
and 16 status methods, 95 distinct methods total, actual exit 0 in 56.865s.
There are no failures, errors, skips, expected failures or unexpected successes.
Head, tree, clean state, captured ALab Python/scripts/tests and Git/Python/sh/bash
entry-point hashes remain identical before and after. These module runs overlap
the counterfactual runs and are not additive. Existing qualification fixtures
run only owned temporary shell gates, including a self-TERM fixture; the real
repository Rust gate and native application certification were not run here.

After that run, main `9e1039ada00b314959aa6a6ab671e7de14a2a19f` was integrated.
The only captured input changed was sync.sh outside the identical 1,111-byte
run_check function exercised by the fixture (SHA-256
`198ed82d79d976f4f167e8e6642596ddcf7ad8a2d8550a39f15c337e8b26d5fb`).
The 95-method result remains bound to its tested commit; it does not validate
the incoming native changes. The combined qualifier SHA-256 is
`50bfe9f35a5e370f1cd5df5c6017817b9e6e7f72d9d57984caf4c623cb7ab910`.

Independent review approved the strict reader, diagnostic composition and
controls. The shared recursive evidence archive and index are recorded in the
[CLI decision](qualification-cli-diagnostics-2026-11-16.md); the final raw union
log SHA-256 is
`12b2792f557df86334cf2ac8309bd4cb1a4642ee64a3027b23b679e1592527ea`.

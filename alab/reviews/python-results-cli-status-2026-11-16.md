<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Fail interrupted Python test discovery at the CLI boundary

The hostile review of `334bcfc0824c230ac555b96810132a5070ef2571` found no
new updater or collector defect. It checked guardian dispatch, unchanged API
and admission bodies, cancellation identity, incomplete journals, all seven
collector Refusal producers, the status consumer and shell propagation. The
existing 145-pass result remains applicable; no redundant run was needed.

The next bounded investigation found a separate natural failure in
`alab/python_test_results.py` at baseline
`b862d9138de9e1c8bf4726179d6b597433503973`. A selected module's `load_tests`
hook can raise SystemExit with code 0, None, False, 256 or -256 after the exclusive
result-file claim. Each real standalone CLI process exits zero with an empty
claimed record, a called hook and no executed test body. An import-time
SystemExit hypothesis was withdrawn: unittest already captures that path as
a completed failed test. The hook call has a different exception boundary.

Decision: reuse the unchanged adjacent stdlib-only `cli_status.py` helper.
Argument parsing stays outside the operation guard. The guard covers exclusive
creation, discovery, execution, serialization and output; successful SystemExit
interruptions now return failure. The ordinary OSError/ImportError reporter is
protected too. The final entry simply exits with the returned status, preserving
intentional healthy zero. NamedResult, execute and passed_tests retain their
baseline ASTs and API cancellation behavior.

Interrupted records remain claimed and incomplete, preserving existing-file
custody. The change does not invent a completed record or overwrite earlier
evidence. Existing consumers already reject incomplete records; this fixes the
misleading process status. The sole physical script-copy fixture already copies
cli_status.py, and the shared helper is already source-bound.

Eleven original standalone proof cases distinguish five false successes from
healthy, ordinary hook error, import error, nonzero cancellation, KeyboardInterrupt
and help controls. Ordinary hook errors and import errors produce completed failed records;
the interrupted hooks leave actual empty files.

The coherent baseline/fixed pair uses identical final tests and the existing
baseline helper. Baseline actual exit is 1: ten completed methods, twelve
assertion events across two failed methods and eight passes, no errors or skips.
Fixed actual exit is 0: all ten pass. Each arm records 24 original-entry children:
13 natural owned-module/argument/custody cases and 11 diagnostic cases triggered
by a real existing-file refusal. Ten false zero exits become 1; six already
failing statuses, healthy/help zero, argument error 2, nonzero 47,
KeyboardInterrupt and native-word overflow retain their statuses. Two ordinary
diagnostic exceptions are now handled without escaping. Inode, mode and bytes
of preexisting records stay unchanged, with no discovery side effects. A direct
execute API check preserves the exact cancellation objects.

The final broader run uses clean committed source
`a1433bcdd162cbd1cb20a3ee629f052c73c247ae`, tree
`ca9861c979a4f23af95ccf5c1911b5c6bf6eb0fc`: actual exit 0, 52 distinct
methods all passing, no failures, errors, skips, expected failures or unexpected
successes. Unittest reports 34.126 seconds. Four complete modules and one positive
upstream-gate fixture cover result production, evidence validation, admission
and the shared helper copy. Focused counts overlap this run and are not added.

All 89 captured inputs, controller, tool identities, Head/tree and clean state
match before/after. Independent reviews approved source semantics, test custody,
input closure and bounded execution scope. Results record CPython 3.14.8 and
Apple Git 2.54.0 on this POSIX Mac. Fixtures use owned local Git and Python;
sourceable gate bodies stop at inert build hooks, and the profile-selection
fixture only captures arguments. No actual native gate, compiler, profile,
entitlement, installed selection or certification is exercised. Previously
unavailable native certification/producer work remains separately deferred.

The recursive common-Git archive is
`alab-validation/a1433bcdd162cbd1cb20a3ee629f052c73c247ae/python-results-cli-k_mbfdj0`:
196 payloads, 2,350,679 bytes and 96 immutable Git bindings. It includes the
standalone proof, paired runs, source final-entry identities, raw records and
witnesses, CPython unittest sources, root captures/logs and all controllers.
Root verifies exact inventory, every size/hash and every declared Git binding.
Index SHA-256:
`12067db8eb3f0fae4b00bad37fd9a377c7a8a73502a83b10739ed530febe947d`.
Root log SHA-256:
`b610968cbee7ceadfcfc59318a253d952c9ee37d89d04a59b37d2e09d8937844`.

<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Observe physical package sources without external filter callbacks

## Findings and decision

At `d42eab260098d5357355a2ec82182d2df27d4e1b`, a configured clean filter
could return committed bytes while the package worktree held an owner edit.
The fresh comparison index reported no change and `validate_binding` accepted
the source. The same check admitted a modified controller `lib.sh`, which then
executed an inert witness command through the fixed-source public entry.
Final worktree removal refused that changed source and preserved it. A separate
cross-commit checkout control also refused to overwrite the edited file and
used a different generation; no edited source bytes were lost in those cases.

A distinct environment mismatch defeated the removal safeguard. Inherited
scalar `GIT_CONFIG` selected an empty file for `git config --list`, while
ordinary status/removal still used the repository's filter configuration.
Enumeration therefore missed the filter. A callback ran during removal,
wrote an owner note into the worktree registration, and Git removed that
registration. The control without scalar `GIT_CONFIG` suppressed the removal
callback and created no note.

Factor the existing bounded, NUL-framed filter-key enumeration into one
standalone package-source helper. Use its command-local empty clean/process
commands and `required=false` for both fresh source observation and final
removal. Reject empty or ambiguous filter-driver names. No external callback
may manufacture clean validation evidence; source changed by external
conversion is preserved and refused. Built-in Git text/CRLF conversion remains active.

Drop inherited scalar `GIT_CONFIG`, which applies only to the config command.
Keep command-wide configuration inputs, including `GIT_CONFIG_COUNT`, its
key/value entries, `GIT_CONFIG_PARAMETERS`, and global/system selectors.
Enumeration and execution must observe the same effective configuration.
The helper stays self-contained because package-source tooling runs from
standalone copied source trees too.

No custody-schema bump is needed: every existing receipt is revalidated
against the live source before reuse or cleanup. Old receipts cannot replace
the new physical-source check with an earlier clean result.

## Validation and limits

Six focused methods produced nine expected failures against the old source
in 12.874 seconds. The fixed implementation passed all six in 15.362 seconds.
The cases cover clean/process dirty-source refusal, zero validation/removal
callbacks, normal removal, converted-checkout preservation, scalar config
isolation with effective registered-filter and COUNT checks, built-in CRLF
acceptance, and empty-driver refusal. Independent production review found no
blocker, and `git diff --check` passed.
The same six methods passed under system Python 3.9 in 9.336 seconds.
The full package-source module ran 35 tests in 148.246 seconds: 34 passed and
the native compilation opt-in was intentionally skipped. Production and test
sources remained unchanged throughout that run.

The source-admission and controller-execution audit remains under
`$HOME/.aterm/.ubd/package-filter-audit-df_sqy8w.noindex`.
The independent scalar-config reproduction remains under
`/private/tmp/alab-config-custody-audit-tkcmwdp5.noindex`.
Selected observations, raw test logs, commands and source hashes are retained
under `$HOME/.aterm/.ubd/package-filter-validation-20261112-e11zzy_u.noindex`.

All checks use private local Git repositories and inert fixture programs.
No native build, real package selection, provider call or live cache was used.
These remain local source-custody observations, not authenticated attestation
or a fence against unrelated concurrent writers.

## Controller integration and landing checks

A separate regression exercises the real fixed-source controller subprocess
with an edited `lib.sh` and a clean filter that returns its original bytes.
Against the original implementation, the command exited successfully and
selected the fixture package. With the fix, it refuses before the filter,
edited shell code, builder or selector runs. Cleanup preserves the source,
its index bytes and its Git registration. The focused fixed run passed in
2.387 seconds.

The complete local-update suites passed 68 tests in 146.618 seconds, package
cache-budget tests passed 12 in 28.402 seconds, and package-version tests
passed seven in 1.783 seconds. Combined with the 34 passing package-source
tests, this is 121 distinct passing tests; one explicit native-build opt-in
was skipped. The additional focused and Python 3.9 runs repeat relevant
cases rather than increase that distinct count. Independent review found no
blocker in the runtime patch or controller regression. All runs used the
same runtime SHA-256:
`c7cea67187c48fbe1dedc79ebec20bd2412cfa476ad6528526e8e6ee87343770`.

Controller baseline/fixed logs, the other integration logs and their manifest
are retained under
`$HOME/.aterm/.ubd/package-filter-integration-20261112-9pc92mtx.noindex`.
The manifest SHA-256 is
`a13afe6f27cfb1e8c66bfa79ff27792631eb8145a5071c789ee6dd8eaa488464`.

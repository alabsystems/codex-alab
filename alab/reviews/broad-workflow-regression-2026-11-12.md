<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Broader local workflow regression check

## Scope and decision

Check the ALab Python workflows beyond the recent inventory, schema and guardian
changes. The tested source stayed clean at
`ca11cee78a002696113340661a4f4f302b3a2e97`. Independent review found no new fault
in the exit-notification change or the review-admission boundaries. The broader
regression run also found no failure, so leave the implementation unchanged.

Run each test module in a fresh ordinary unittest process, sequentially. This
avoids relying on restoration of module-global state and lets fixtures apply
their own containment. Do not add an outer sandbox to the harness. Use the
retained native unit runner explicitly, and keep the four optional native
package/controller experiment switches unset.

## Results

All 66 selected module processes exited successfully, with no timeouts:

| Observation | Passed | Skipped |
| --- | ---: | ---: |
| Main sweep: 727 distinct test identities | 718 | 9 |
| Two portable-runtime cases replayed with a suitable runtime | 2 | 0 |
| Three native-context cases run separately | 3 | 0 |

The portable-runtime replay covers two identities already counted as skips in
the main sweep. The native-context cases are three additional identities.
Combined coverage is **723 distinct passed tests and seven unexecuted cases**,
with no failed tests, errors, expected failures or unexpected successes.

The main sweep used Python 3.14.5. Its runtime is outside the protected account
home, so two tests correctly skipped that environment. A private copy of the
managed CPython 3.14.8 runtime exercised those tests: runtime imports work while
runtime writes and reads of sibling/account canaries are denied. Both passed
in 3.893 seconds. The installed runtime was not modified, and the temporary
copy was removed after verification.

The native-context module requires an actual compiler invocation. Run it
separately under source-selected Trust 15698 and the existing containment
profile. All three cases passed in 0.990 seconds. The retained adapter's hash
remained `eb83cda1b09cd68e5c10fae21a27ee094565560c0d4e33a61d6e23e0ea982287`;
its Rust sources, Cargo lockfile and compiler pin were unchanged since its
[recorded build](native-unit-runner-recovery-2026-11-12.md).

The remaining seven cases are explicit opt-ins: four native package-cache
cases, two native project-controller cases, and one native package-source
case. Their required experiment switches were deliberately unset. They are
unexecuted coverage, not successful tests.

The [native follow-up](native-optin-completion-2026-11-12.md) subsequently ran
all seven cases successfully, after correcting the source fixture's premature
target creation. That separate record identifies its source and tool versions;
it does not change the original sweep's counts above.

## Retained evidence and limits

Logs and per-module status records are retained under
`$HOME/.aterm/.ubd/codex-python-sweep-20261112-jf2rn73x`. Each module has its own
raw log, with separate raw logs for the two supplemental runs. Independent
inspection confirmed unique test identities and the skip reconciliation above.
The 72-file evidence manifest has SHA-256
`7f38a78dd3e30fdebaf9d242964a77259037c7a54a96a0f2e4b38360c8becbad`.

These are local regression observations. They are not an authenticated
attestation, a feature-admission bundle, a full native release gate, or a claim
about installed adoption. No product source or installed executable changed.

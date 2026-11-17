<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Read completed follow-ups as completed in Cruise reports

Cruise treated `Available follow-ups were already complete.` as open work merely
because it contained `follow-ups`. The observed report also said unavailable
native work remained deferred. Repeating a demand to finish the completed
follow-ups did not advance the available work.

The regression preserves that report verbatim:

> Nothing left: available follow-ups were already complete; unavailable native work remains deferred. Fixed workflow false-success exits and landed at `610696ddc1`.

Decision: narrowly recognize completed `follow-up` and `follow-ups` status rows
inside Cruise's existing reader. A private helper accepts a known whole subject
or past worker action with an explicit completion predicate. It does not use the
broader settling vocabulary: a green or passing check does not establish that
follow-up work is complete. Unknown subjects, partial counts, conditions,
exceptions and predicate tails continue to read open.

The helper rejects completion questions. It examines the complete worker prefix
without discarding a condition before `and` or `then`; for example, `If CI passed
and I completed the follow-up.` remains open. Compound worker prefixes are
intentionally conservative, including a benign `I then ...` report. Bare `both`
may name a whole subject, while counted modifiers such as `both of the three`
remain open. Completion in another clause or another noun's predicate cannot
clear a follow-up, and genuine open work after a completed follow-up remains
visible.

This changes one Cruise status predicate and adds one private helper in the
ALab-owned reader. Removing those additions from final source `14fcd6e0de`
restores the entire baseline `open_work.rs` byte-for-byte. Land's implementation,
head handling, needs reader and native admission are unchanged. The conservative
reader is not a general English completion recognizer.

The tests contain five new methods and 61 final table rows. They cover the exact
report, singular/plural forms and auxiliaries, completion predicates, past
after-CI completion, deferred and partial work, future conditions, distinct
subjects/clauses, questions, conditional worker prefixes and counted `both`.
Table rows are not additional independently counted libtest methods.

Native validation history remains source-specific:

- The first RED at `5cd4f0ca0e45ea765ff4d2e471d83ce724ed237e` used the
  48-row test version. Its original failing run is retained separately.
- The coherent later baseline at
  `57b599c78245b1e20c173709951a576f89d0da3d` used 54 rows. Its native run
  selected 23 parser-related methods: 20 passed, the three new completion
  methods failed, none were ignored, and 1,114 were filtered. Actual status was
  101 in the compiler/controller log; the original outer tool result was lost
  during context truncation, so an outer receipt is not inferred. The owning
  library and dependency build succeeded.
- The exploratory 54-row fixed run and the later
  `6b8d94af96b24d65addae49bd2eac4f4bca89c3e` 59-row fixed run each passed
  their 23 selected methods. They remain separate observations; neither is the
  final 61-row validation.
- Final source `14fcd6e0ded38e65581e364dfa8383ae63c3f803` contains the
  61-row version. Original combined controller 40504 reached completion
  `6010cb` with actual exit 0. The `open_work` filter passed 23 methods, with
  zero failures or ignored methods and 1,114 filtered, in 2.79 seconds.
  `decide_tests` passed 184 methods, with zero failures or ignored methods and
  953 filtered, in 10.07 seconds. These are 207 passing observations of 205
  distinct methods; two identities overlap the filters. Table rows and earlier
  runs are not added to that distinct count. The durations are ordinary
  libtest output, not a performance comparison.

The manual ignored Land holdout measurement is excluded from these ordinary
unit filters; it is not a failed unit check.

All 212 captured source inputs and three recorded tool identities match
before/after. The source remained clean at the final commit. Earlier RED and
exploratory GREEN runs stay attributed to their own source/test versions.

These are supported owning-library policy tests in the named-unverified lane,
using the source-selected Trust build 15698 and compiler source
`20565c3255adf151f9eb920e89cdf83c38f84abe`. An existing qualified publisher
selected the committed compiler through the existing protected project route.
No compiler, profile or verification override was introduced. This is native
unit execution, not application certification or an application proof claim.
Held application operations and the unavailable producer-channel work remain
deferred; this repair does not retry or authorize them.

The read-only recursive archive is
`alab-validation/14fcd6e0ded38e65581e364dfa8383ae63c3f803/cruise-completed-followups-pk1qye65`:
452 payloads, 17,458,529 bytes and 437 immutable Git bindings. It retains raw
original controller output, source/input snapshots, frontend selection and
controller status records, and the separate baseline/exploratory histories. These source-bound
unit observations are not application certification receipts. Index SHA-256:
`6383ad5692da01ff6dbfa94b3dbae0cd957cdfe1650a3b047c8bce82a7525a15`.

Publication composes remote main
`32b4f1583905c1e34b134d4fda86a0d4fc9271bf` through the ordinary merge
`3fb2c36d5da4df2c3ba4aec19f9bb418c4bea477`. All 212 captured native inputs
and all 88 captured fixed bundle-consumer inputs remain byte-identical to their
respective tested versions. Incoming changes are Python custody code/tests,
review/intake records and `alab-tui/glue/startup_account_read.rs`. That glue is
included by `codex-tui`'s `app_server_session.rs`; it is outside the selected
Autopilot library unit build. Its behavior is not claimed as executed here.
No selected compiled source, compiler lock or project-route input changed, so
repeating the same selected tests would duplicate the recorded observations.

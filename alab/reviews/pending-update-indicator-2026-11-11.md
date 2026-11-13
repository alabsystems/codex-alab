<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Keep a blocked update visible

## Finding and decision

At `e9d966b26b`, the update evaluator could confirm a replacement build and
then report `Update pending` while clearing the cached pending flag. The
composer reads that flag to draw `↑ update`. Three post-admission paths used
the nonpending publisher: a harness that became passive, failure to save a
carried act, and refusal of the final TUI handoff.

Use the pending publisher for those three paths. The replacement remains
available even when this attempt cannot leave safely. Keep the existing
explicit disabled-policy and not-due behavior. This is a presentation-state
correction: it grants no admission, changes no handoff decision, and does
not override draft, input, record, or native-work checks.

The regression must exercise the real native evaluator, observe its cached
indicator after refusal, and preserve the existing continuation/input
assertions. Its update cache is process-global, so the regression runs in a
bounded isolated child rather than racing other library tests.

## Before-fix observation

With the new regression but the original three status calls, the explicit
refusal case failed at the pending-bit assertion. Its report simultaneously
said `Update pending: the TUI could not leave safely; the current session
keeps running`. This reproduces the text/badge mismatch, rather than a
setup or admission failure. The retained log is
`$HOME/.aterm/.ubd/codex-pending-indicator-before-20261111.log`.

The three status calls were then restored to the pending publisher for
the final checks. The regression runs each of the original four refusal
cases in its own process and requires its completion marker, preserving
the existing native input, queue, and continuation assertions.

The focused regression passes with the corrected publishers, including all
four isolated cases. Its log is
`$HOME/.aterm/.ubd/codex-pending-indicator-after-20261111.log`.

## Integration

While these checks ran, remote main advanced to `54659b32de` with native
capture-before-teardown work. The indicator change was rebased onto that
source without conflicts. Its three production call sites were unchanged
by that work; the broader native handoff group is checked again on the
combined source.

All 32 tests under `extension::upgrade_handoff_tests` passed on the combined
source, with no failures or ignored cases. This includes the four isolated
indicator cases and the existing cancellation, policy, record, native-input,
inventory, and upgrade-readiness checks. The log is
`$HOME/.aterm/.ubd/codex-pending-indicator-integrated-20261111.log`.

The builds used Trust's unverified lane with `--locked --offline` and the
repository's 8 MiB test stack. The final group ran with four test threads;
each indicator case still ran in its own exact-test child. Independent
review confirmed that the original assertions remain intact. The
post-admission passive branch has the same publisher correction but is
not directly exercised by these four cases. All changes are in ALab-owned
paths; no upstream hook or lockfile entry changed.

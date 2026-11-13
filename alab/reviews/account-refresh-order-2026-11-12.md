<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Keep account refresh results in request order

## Finding and decision

At `01f3fdb5ad`, each account refresh starts an independent native snapshot
request. Its completion retains the pane and thread identity, but nothing
identifies which refresh it answers. An older response can therefore replace
newer account data or overwrite a successful refresh with an old error.

Store an opaque snapshot token in each ALab-owned account view. Begin a new
token synchronously before scheduling a refresh, and accept a completion only
while its token is still current. Consume the token when accepted so duplicate
replies cannot apply again. Keep the current text while a refresh is pending;
do not recreate the pane or reset its navigation state.

Assignment observations are explicit updates rather than snapshots. They
invalidate outstanding snapshot tokens before changing the text, so a late
inspection response cannot overwrite assignment progress. A later explicit
refresh receives a fresh token and can succeed. Existing weak-pane and thread
checks still apply. These tokens correlate presentation updates; they provide
no authority to inspect, select, enroll, or authenticate an account.

## Layer boundary

The state and correlation rules belong in `alab-tui`. Upstream's two existing
marked account event declarations carry the ALab-owned view/update types;
their dispatcher continues to forward them. No new upstream control flow is
needed. The delta helper was attempted before editing but is unavailable
because this checkout has no upstream remote; inspect the exact two-line
upstream diff directly and preserve its markers.

## Regression check

The tests use the production snapshot scheduler with controlled asynchronous
fetch results and apply the resulting events to a real account pane. They
exercise both completion orders, old success and failure responses, duplicate
delivery, assignment-progress invalidation, a later explicit refresh, and
the existing pane/thread/draft guards.

With snapshot acceptance deliberately made permissive, the newer snapshot was
first displayed, then the older completion replaced it and failed the expected
newer-text assertion. This reproduces the original display-ordering failure.
The guard was restored before the final run. The counterexample log is
`$HOME/.aterm/.ubd/codex-account-refresh-permissive-20261112.log`.

With correlation restored, all 29 account-details tests passed with no failures
or ignored cases, including the recent reconciliation observer cases. The
final log is `$HOME/.aterm/.ubd/codex-account-refresh-ordered-20261112.log`.
The run used Trust's unverified lane with `--locked --offline`, the repository's
8 MiB test stack, and four test threads. Formatting and whitespace checks pass.

Independent review confirmed that the scheduler retains only a weak pane
reference, mismatches preserve the newer token, accepted replies consume their
token, and assignment updates invalidate pending snapshots. Tests use controlled
read results and do not change or authenticate real accounts.

The final upstream diff changes only the types in the two existing marked
account event declarations. The dispatcher and all other upstream source remain
unchanged. The delta helper is still unavailable for the recorded missing-remote
reason; manual diff inspection confirms both markers remain. No lockfile entry
changed.

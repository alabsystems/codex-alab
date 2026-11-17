<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Keep configured runtime custody continuously closed

Decision: give configuration-only reconciliation its own opaque runtime resource
family, without exposing generic Core replay or a scheduling-release method.
An actual accepted replacement may also move the same initial or replacement
Hold while Source still owns an unchanged Empty selection. That transfer does
not require inventing an earlier UI commit: it requires the private accepted H1,
the original owner and deadline, and the real Source guard after SDK mutation
serialization. Native independently determines whether H1 may exist at all.

The move preserves the exact Hold Arc and every independent Source ticket,
marks matching old runtime resources transferred before the worker can ACK,
and leaves physical ACK ownership with that original worker. The mutation
semaphore is the same one used by release. A failed or canceled observer cannot
open admission, replace a Hold, create Native permission or claim completion.
The configured wrappers still require the actual original Native observations;
their commit does not release the Source scheduling Hold.

The two reviewed SDK drafts and the strengthened overlap control are composed
at aggregate SHA-256
`5a36c95dc48bc75304a292315e52f3bd15814d8e59f11ec23a6b2939ee31b922`.
Four overlapping export statements retain both resource families. Only the
eight changed ALab-owned Rust files were formatted. Source-selected Trust 15698
ran the protected project's explicit unverified lane: all 333 SDK methods pass,
four existing helper cases remain ignored, the native-source-proof feature
check exits zero, and scoped Tippy reports no owned, spanning, failing, script
or set-aside diagnostic. Both lockfiles remain unchanged.

Four new private ledger controls cover the same Hold and ticket ownership,
invalid/expired/foreign or changed-Empty denial, canceled semaphore waiting,
and a real competing release while transfer is pending on the same semaphore.
They exercise local custody and serialization; they fabricate no public SDK
receipt and do not qualify a successful Native/UI transition.

The exact formatted input SHA-256 is
`7a97c47ba02147d135b17c9b7f7a59ff946476179228a877b6aec599e8a38fb5`.
Outside evidence under `~/codex-live-update-evidence` includes
`configured-closed-custody-e8ef-first.json`, its matching patch and raw logs,
controller results, lint verdict, and `configured-closed-custody-validation.json`.
This record is the only addition after those checks.

The concrete Source registry/configured consumer, admission-close/drain ordering
correction and actual frontend UI composition remain separate work. This change
does not enable ordinary activation, qualify pending views or editor transfer,
certify an application, install a package, or complete the live-update goal.

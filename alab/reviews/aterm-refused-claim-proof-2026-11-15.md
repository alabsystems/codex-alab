<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Forget a previous local proof when the native claim is refused

Decision: invalidate the shared local claim cache when a native claim request
receives a non-OK acknowledgement. A person's clear followed by another holder's
take can precede watch delivery. Previously, the next claim reported Busy while
every client clone still reported holds_claim and could send claim-gated acts.
Reconfirm already invalidates that cache on refusal; claim must do the same.

The fix changes only the cached proof. It releases no native claim, changes no
other holder and does not infer that a refused act succeeded. A later successful
claim plus its existing proven status check is required to restore the cache.
Policy NotPermitted keeps its existing behavior: a transient policy off retains
the native claim under the contract's standby custody. The production native
adoption path starts on a new, unclaimed client, so no earlier proof is present
when its native check returns an error.

Tests-only source `772dcfd608` reproduces the Busy case through FakeAterm's actual
claim rule after a clear and a foreign take. Its focused test exits 101 at the
stale-clone-proof assertion. The generic refusal branch is not reached in RED.
An intermediate GREEN run passes the Busy branch but finds a missing LF in my
raw generic-refusal fixture; it times out instead of reading the refusal. Retain
that failed capture. The corrected fixture supplies LF and the final run covers
both cases, verifies no gated request is sent with stale proof, preserves the
foreign holder, and requires fresh native proof before another act is recorded.

Final source base `6554574fc6` plus the reproduction and fix passes the full
alab-aterm library suite: 242 passed, zero failed, three existing subprocess helper
entrypoints ignored, zero filtered. Separate-target all-target Tippy with
`--no-deps -- -D warnings` exits zero. Both use direct source-selected Trust 15698
(`20565c3255adf151f9eb920e89cdf83c38f84abe`), with tests in the explicit unverified
runtime lane. These are ordinary library/lint observations, not native gate,
application certificate, packaging or installed-adoption evidence.

The exact final client/tests sources and four relevant raw logs are archived
under the shared Git directory at
`alab-validation/claim-refusal-20261115/ordinary-library-7sj8g4wy`.
Index SHA-256:
`a75283b0ae01bac957ebf6a78f3660756ebc487706511dfe2f6e55652c1d8ffd`.

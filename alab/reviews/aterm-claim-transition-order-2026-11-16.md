<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Keep native claim transitions and local proof in the same order

`Client::claim` serialized claim attempts, but `release` and `reconfirm` did
not join that semaphore. The connection worker can receive a proving status
reply while its caller is not scheduled. Another clone can then release the
actual native claim. Resuming the old claim future overwrote that release's
local `claimed = false` with the earlier proof's `claimed = true`.
Synchronous `note_claim_lost` could be overwritten by the same buffered proof.
An admitted reconfirmation could also report success after local revocation
and could retake a native claim cleared before the set actually executed.

The existing Supervisor tests protect its own refresh/publication transactions
through `policy_refresh`; they do not serialize every public Client clone.
The exit cleanup also calls Client release independently of that Supervisor
mutex. The Client's documented shared proof must therefore enforce its own
ordering.

All three asynchronous native claim transitions now retain the existing
semaphore. One guarded boolean records revocation: only a new claim holding
that semaphore resets it, while loss, close and release invalidate immediately.
Both ordinary proving status and native adoption publish a held claim only
after checking revocation and connection liveness under the state lock. No
other claim can reset the flag while this proof is pending. Cleanup uses a
private already-locked release method, including unproven and malformed status
paths, so it cannot recursively acquire the semaphore.

Reconfirmation never resets the flag or publishes `claimed = true`. An OK
acknowledgement after revocation returns false and conditionally releases its
native retake, using the same holder-restricted unset. Another holder remains
untouched. A subsequent explicit claim still obtains a fresh native proof.
There are no new allocations, counters, claim grants or native protocol fields.

Five `codex-alab-aterm` library tests in `tests::claim_order_tests` manually poll
callers independently of the actual connection worker. A later `whoami` reply
on the same connection proves the earlier proof or acknowledgement is ready
without resuming its caller. They cover:

- `release_cannot_be_overwritten_by_an_already_answered_claim_proof`;
- `a_local_loss_invalidates_an_already_answered_claim_proof`, including a foreign
  holder and successful subsequent fresh claim;
- `release_revokes_an_in_flight_reconfirmation_before_it_can_authorize`;
- `a_reconfirmation_that_retakes_a_locally_lost_claim_is_given_back`;
- `failed_proving_status_releases_without_reentering_the_transition_gate`.

The first four assert behavior the original implementation violates; the last
checks error-path cleanup after adding serialization. Real native state, shared
local proof and unsent harness acts are checked; there are no sleeps or
elapsed-time absence oracles. Existing unproven-claim and Supervisor ordering
tests remain relevant controls.

The initial implementation had source review and `git diff --check` only.
Subsequent ordinary execution used native-selected Trust 27009, compiler
`5062eed55c0bc9df8a79eba1368d88ffde76c741`, in the existing protected profile.
The native adoption controller's generated working lock was not committed;
its SHA-256 was `7c4c9f1a8f7f4950089d431ccab4d2e3e8b27baccf5e42d9242b36cc3de892f8`.

At source `1c46e84927951137dabee226e63eeb3dcfcd689a`, the original Client blob
`e6f2cb4ede417c6a14cae58bf31daaf7dea5d822` fails the four race controls at
runtime. The malformed-status cleanup control passes: one pass, four failures,
zero ignored, exit 101. A prior fixture compile failure was corrected by an
explicit assertion-macro import and is not counted as this runtime result.

At source `4e3d23242d90d29e49e54a859c8b39555bed1466`, the fixed Client blob
`95a6d74f16810bc35d63070077bb4624761684e4` passes all five controls. The full
Aterm library suite passes 247 tests, fails none and retains three existing
ignored child entry points; no execution is claimed for those ignored entries.
The ordinary command exits zero. Root independently checked the retained file
hashes, named results and identical Client/control source bytes before composing
the fix with newer main.

The same source also passes native-protected, canonical-profile all-target Aterm
Tippy with `--no-deps --all-targets -- -D warnings`, exit zero in 1m01s.
Its separate `aterm-strict-lint` index SHA-256 is
`ab5d7a45b507901615095b03602ddda70cc353c31b9d86d04efbb6df56b48d49`.

Evidence lives under the shared Git directory's
`alab-validation/<source>/trust27009-adoption/claim-controls-{red,green}`.
The respective index SHA-256 values are
`f9608801a68c9163e5631857c035aaa7f40344051f8d29c235c081e324608396` and
`853817a95c5ed3a09e767c47174d808992f5c3c69ffe7ad058cd1ebf694e29c8`.
These source-bound ordinary results do not qualify the final upstream integration,
commit the compiler pin, certify an application build or select an installed image.

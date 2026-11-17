<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Policy reads retain their original cache transaction

`Client::harness_policy` used to await its control reply and then replace the
shared policy cache without checking another reader or a local invalidation.
The connection worker orders requests, but each caller resumes independently.
An old reply already delivered to its oneshot could therefore overwrite a newer
reply after that newer caller had returned.

This is reachable through current callers. First,
`renewal_follow::first_recovery_host` reads through the claim connection outside
`policy_refresh`, and its old no-five-hour-bound reply becomes buffered. Second,
`Supervisor::refresh` reads the newer bound under `policy_refresh` while handling
the policy event, updates its own generation and finishes that event. Last, the
older renewal caller resumes and overwrites only the shared Client cache. The
Supervisor's generation records do not order that independent cache write. The
five-hour recovery check in `renewal_follow` treats
that remembered omission as a reason to return without reading the host again.
An active Supervisor also has no periodic policy refresh, so another successful
policy read or event is needed to repair the stale cache. This is a recovery
liveness bug; this review does not establish a human-hold bypass.

The same publication race violated two Client lifecycle rules. `forget_policy`
could clear the cache and then have an already buffered reply restore it, and
`close` cleared the claim without clearing the policy. `forget_policy` has no
production caller in this source; its regression protects the public API's
explicit invalidation contract separately from the reachable concurrent-read
bug.

## Change

One shared semaphore retains the entire policy request, classification and cache
publication transaction. Only a new read holding that semaphore resets a guarded
invalidation boolean. `forget_policy` clears the cache and marks the current
transaction invalid. A buffered answer from that transaction returns the typed
local `Error::PolicyInvalidated`, rather than a stale supported policy or an
invented server refusal. A queued reader resets invalidation only after the old
transaction has settled and it can issue a fresh request.

`close` first closes the connection, then clears policy and claim state under
the state lock. The closed transport prevents later proof or cache publication;
if a transaction was already publishing under the mutex, the subsequent clear
removes it before close returns. After releasing the mutex, close shuts both
semaphores. Their caller-owned wakers may inspect Client state synchronously,
so waking them while holding that mutex could deadlock. Queued policy, claim,
reconfirm and release calls wake with `Error::Closed` without waiting for an
older caller to resume. Subsequent reads also refuse. Ordinary connection
errors retain their original classification, and `note_claim_lost` still
preserves a useful supported policy.

The change adds no per-read allocation, version counter or new native grant.
It serializes only policy reads. The native connection, current policy,
source admission and explicit human-hold checks retain their existing behavior.

The exhaustive native-member transport conversion maps the new local error to
`io::ErrorKind::Interrupted`. Other inspected Aterm error classifiers have
fallback arms: the Supervisor stays passive on an error, recovery treats it as
unread, and folder trust does not mistake it for unsupported protocol or an
unanswered host.

## Regression source

Test-only commit `44278dba3d` adds five ordinary library tests in
`codex-rs/alab-aterm/src/policy_order_tests.rs` before the production fix. It
references only errors present in the original implementation, so its intended
RED result is a behavioral assertion rather than a missing-variant compile
error. The production commit additionally checks the precise new error variant.

- `concurrent_policy_reads_retain_the_latest_five_hour_recovery_bound` buffers
  the real old no-bound reply without resuming its caller, reads a new bound on
  a clone, then exercises either permitted caller schedule and requires the
  latest actual policy to remain cached by both clients.
- `forgetting_policy_invalidates_a_buffered_read_before_a_waiting_fresh_read`
  checks synchronous invalidation, refusal to claim from an empty cache, a
  queued reader that cannot validate the older reply, and successful fresh read
  and claim afterward.
- `closing_keeps_policy_empty_when_a_buffered_read_and_a_waiter_finish` starts
  with a real proven fake-host claim, closes from a clone, and checks typed
  invalidation, closed-connection refusal and an empty cache after all waiters
  finish.
- `dropping_a_policy_read_does_not_block_the_next_fresh_policy` drops an actual
  gated caller and requires the next read to complete with the new bound.
- `losing_a_claim_keeps_its_supported_policy_available` prevents policy
  invalidation from being broadened to ordinary claim loss.

The tests poll callers independently of the real connection worker. A later
`whoami` reply on that same connection proves the earlier policy reply reached
its oneshot; no sleep or scheduler timing is used as that evidence.

The close-order followup adds two controls. The private Client test
`close_wakes_external_readers_without_holding_the_state_lock` queues public
reads behind retained permits and registers a legal custom waker. The waker
checks the state mutex with nonblocking `try_lock`, so an incorrect close fails
without deadlocking, spawning threads or leaving a hung fixture. The actual
fake-host test
`close_wakes_queued_claim_transitions_before_a_buffered_proof_resumes` queues
claim, reconfirm and release behind an already answered claim proof. Each must
return `Closed` on its next poll while the older caller remains unpolled; the
older proof then remains unproven and the policy cache stays empty.

The initial commits had static source checks only. Subsequent ordinary execution
used native-selected Trust 27009, compiler
`5062eed55c0bc9df8a79eba1368d88ffde76c741`, inside the existing protected
profile. The native adoption controller's generated working lock was not
committed; its SHA-256 remained
`7c4c9f1a8f7f4950089d431ccab4d2e3e8b27baccf5e42d9242b36cc3de892f8`.

At `44f97f47d40a55d4d2890059740bf63618d98e32`, the original Client blob
`95a6d74f16810bc35d63070077bb4624761684e4` produced three named runtime
failures and two passing controls, zero ignored, exit 101. This is actual
behavioral reproduction, not a fixture compile failure.

At `34d983ee18907155e2188b3e060e886016c66dcf`, all five policy controls and
both close controls pass. The full library suite passes 254 tests, fails none
and retains three existing ignored child entry points; doc tests select zero.
The runtime and strict all-target Aterm Tippy commands both exit zero, with
`--no-deps --all-targets -- -D warnings` for lint. An earlier runtime pass had
six test-only unnecessary Box-pin findings; those queued local futures were
pinned in place and both runtime and strict lint were rerun. The earlier lint
failure is not presented as a passing check.

The shared Git directory retains the two source-bound observations under
`alab-validation/<source>/trust27009-adoption/policy-controls-{red,green}`.
Their index SHA-256 values are
`cba8f8359b9317593d784933ba684b6b34f669c14fbade83ae3c7306af00875f` and
`bd4ddc3f8f5a3eab17f6c165ed2215fd9d01cb2c8752e17da59aebd7ce7e17f9`.
Root checked every retained file hash and exact changed Rust-file bytes against
the observed GREEN source before composing the fix with current main.

These ordinary source controls do not commit the compiler pin, certify an
application, qualify the separate upstream integration or exercise an ignored
native fixture. They do not confer package or native runtime authority.

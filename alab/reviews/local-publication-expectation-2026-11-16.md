<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Honor the expected ref for local publication

The explicit local publication route checked candidate ancestry but then used
an ordinary fast-forward push, ignoring the supplied receiver preimage. With
expected A, remote B, and candidate C extending B, it advanced B to C while the
native route correctly refused the stale expectation.

Decision: use the existing fresh complete bare transport for both routes and
verify actual candidate ancestry there before any remote request. Source grafts
must not authorize publication. Local writes use an exact expected-ref lease
with a fixed receive-pack command that enforces fast-forwards at the receiver,
ignores replacement/graft views, and preserves receive hooks and checked-out
branch policy. Receiver configuration files are never changed. GitHub writes
continue exclusively through Agh's native atomic-ref request.

The lease flag provides comparison, not permission to rewrite history: the
receiver independently refuses every non-fast-forward, including a proposal
made from misleading source ancestry. Official Git documents both the
[expected-ref lease](https://git-scm.com/docs/git-push) and
[receiver fast-forward enforcement](https://git-scm.com/docs/git-config#Documentation/git-config.txt-receivedenyNonFastForwards).
Tests showed a client-only config flag does not reach the local receiver, so
that weaker design was rejected.

Git may report an already-current destination without performing the lease
comparison. Parse its single exact-target porcelain record: a fast-forward
succeeds; an up-to-date result succeeds only when expected equals candidate.
Otherwise report a confirmed advance to the canonical candidate. Other local
failures stay refusals, without guessing an automatic retry classification.
A no-op with expected equal to candidate can linearize at its advertisement.
All validated source identities remain immutable when caller HEAD moves.

Independent review approved the implementation and public controls. The same
final eight methods run from coherent exact `d55214dc36573b2589260748080b046dc619cb69` and fixed
`b854e1424237788492f11649569d3b3b418e1978` source: baseline actual exit 1, six assertion failures across four
parent methods; fixed actual exit 0, all eight pass, zero errors/skips. Valid FF,
valid no-op, receive-hook and checked-out-branch guards pass both versions. The
first classification draft expected Advanced for a plain stale-lease failure;
its fixed-run error is retained separately. The corrected expectation keeps
that refusal and only requires Advanced for the confirmed already-candidate
case. Public tests use real local Git, preloaded competing objects, source and
post-writer receiver ref/reflog/HEAD snapshots, and source config guards.

A separate twelve-control prototype verifies receiver policy with on-disk
non-FF protection false, source/receiver graft and replacement cases, an actual
pre-receive synchronization gate plus an independent writer outside quarantine,
no-op/stale behavior, and hook/nonbare policy. All final refs/outcomes/config
bytes match expectations; no project repository or GitHub ref was written.
Three earlier private controllers stopped on fixture preparation errors; their
raw artifacts remain separate and are not counted as completed proof. Git's
quarantine protections were preserved, not disabled for the concurrent writer.

The initial fetch investigation required no production change: both an exact
competing target and a newer competing target correctly fail the reader's old
OID transaction, with all tracking refs/reflogs/HEAD unchanged from the complete
post-writer snapshot. A fresh snapshot recovers an equal target and still
refuses rollback of a newer one. That strict CAS behavior remains intact.

Local-only publisher SHA-256:
`9b517e3cbbde3ed35813abeab8cb3861941b96fc03653b2dc6b5c8b34d0e7f50`.
Combined publisher SHA-256:
`a8dfadeca00c7e7a1cbf11f70ed61e6d52f5bcba69b7e18b479bac107434a09c`.
Final local-test SHA-256:
`06aae94685cafd8c0112e713d42dab3a58b31ec7b43b94b21feab9763a142364`.

The final complete four-module run uses clean committed source
`bbd646de3fd49d7b1aa7fbb8a4d0f1fd7cf3de6f`, tree
`70a79472fce68680480063467419fd267ff94c15`: 48 distinct methods, actual exit 0 in 38.633s,
zero failures/errors/skips/expected failures/unexpected successes. Head, tree,
clean state, captured source/tests/scripts and Git/Python/sh/bash entry identities
remain unchanged before/after. This combines the preexisting reader/publisher
controls with nine native-error and eight local-publication methods. Earlier
40-method validation and focused pairs overlap this 48 and are not additive.
Tests use owned local repositories and module-local native-response fixtures;
no real Agh request, GitHub network, project ref write, Rust/native build or
application certificate is exercised by these tests. Platform observations
cover this Mac/Git environment; unsupported receiver commands fail closed.

The recursive common-Git archive is
`alab-validation/bbd646de3fd49d7b1aa7fbb8a4d0f1fd7cf3de6f/github-contracts-e1edkzu0`:
417 payloads, 4,916,833 bytes. Root verified exact recursive inventory, all payload
sizes/SHA and selected immutable Git source bindings. It retains both coherent
counterfactuals, complete root snapshots/raw logs/drivers, the intentional fetch-CAS
audit, original local expectation proof, rejected shortcut evidence, final 12-case
receiver prototype, and separately labeled preparation/classification drafts.
Mutable prototype repositories are omitted; regular artifacts, source copies
and original drivers are retained. Index SHA-256:
`b029cb3db4a781bae376d7e6f2e7d42d2bd6aab48443e394a127a4248e37c1de`.
Final root log SHA-256:
`5858dfde195d7291beb3ff7ae1d67ea6a94058314bf2f804e4ad6b9b3fdcf188`.

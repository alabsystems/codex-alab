<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Require canonical numeric native failure metadata

The GitHub reader used Python equality to recognize version, exit and HTTP
status fields in a native error envelope. Boolean true and floating-point
aliases could therefore be interpreted as confirmed absence. The publisher
also treated malformed conflict metadata as a resumable race after main moved.

Decision: ALab requires exact JSON integers for version 1, matching exit status,
and each non-null HTTP status before relying on native failure metadata.
Missing/null optional status remains supported. Only canonical integer 404/409
absence signals produce None, and only canonical no-effect conflict 9 metadata
can start the existing fresh authoritative-main race check. Unknown/malformed
metadata remains the original Refused error; it does not gain absence or race
authority. This is an ALab confirmation policy; no external JSON schema is
asserted. Valid native replies, authentication/effect checks and all GitHub
network routing remain unchanged.

Independent source/test review approved the change. Exact `d55214dc36573b2589260748080b046dc619cb69` coherent source
and fixed f338801fb00b6fac24f0f7b09de6f8512a86ddcd use the same final nine public
controls: baseline actual exit 1, 13 assertion events plus four error events
across four failed parent methods; fixed actual exit 0, all nine pass, zero
errors/skips. Canonical absence, optional-status, permission and unchanged-main
controls remain valid. Source and test hashes before/after match. Tests call
public Agh.api/call through a module-local subprocess response fixture and
public publication through an owned local Native fixture; no predicate-only
substitute or manufactured result classification is used. The publisher must
retain the original malformed failure and avoid an extra authoritative query.

Frozen github.py SHA-256:
`fe28fc42bf6e5d3a9074f4a0ce5de2b3f3c2894c86cd5ac1a3bfe9d605ae7d63`.
Typed-only github_push.py SHA-256:
`0342f1a1a69af361698756cbe228e0856d68aaf8afda096d31ed6cb1922e307c`.
The combined publisher also incorporates the separately reviewed
[local expectation fix](local-publication-expectation-2026-11-16.md).

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

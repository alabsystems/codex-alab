<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Reject ambiguous feature catalogs before counting review

The intake and source-status readers converted catalog IDs directly into a
set. Two otherwise valid feature tables with the same ID and distinct topics
therefore produced successful review-coverage/intake-complete reports, even
though the ownership validator rejected that duplicate identity. The committed
admission reader used the same unchecked projection.

Decision: share catalog_feature_ids in the existing upstream_intake validation
module, which all three readers already import. Require exact integer schema 1,
a features list, table entries and unique nonblank string IDs before returning
a set. Preserve IDs verbatim, empty-list catalogs and minimal ID tables. Full
ownership/lifecycle/path/topic validation remains a separate contract. Keep
each reader's existing catalog provenance: candidate Git tree for status,
source Git tree for admission, local file for intake validation. The admission
reader's no-new-upstream path still returns before reading a catalog.

Malformed or ambiguous catalogs now fail with ValueError before review can
be counted. The status CLI retains its already completed source observations;
intake and admission refuse cleanly. This validates review declarations and
catalog identity only; it does not establish semantic review, execution proof
or artifact adoption. A separate bounded 24-control malformed-JSON audit found
no new defect and required no production change.

Independent review approved the frozen implementation and nine public-reader
controls. The same final module runs from coherent exact baseline
`83bea1d82e027dd9e557e87e53a3619f2b9f268a` and fixed source with identical tests
and helper bytes. Baseline: actual exit 1, 20 assertion events plus five error
events across five failed parent methods; four compatibility/bypass methods
pass. Fixed: actual exit 0, all nine pass, zero failures/errors/skips/expected
failures/unexpected successes. Original driver exits 0 and all before/after
input hashes match. Only the three changed production files differ across
selected source bundles. Tests exercise actual intake/status CLI subprocesses
and public committed admission, never the new helper directly; they validate
malformed/blank IDs, duplicate rejection and literal/minimal/empty compatibility.

Root runs six complete related modules on clean committed source
`6a95cd3649d7b8b9d36fed29abd0ae849a567dab`, tree
`a1ec1451e206b6f7f43d98518faf76a3b330ab97`: 112 distinct methods (nine intake, 27 status,
18 admission, nine catalog IDs, 27 inventory, 22 feature evidence), actual exit 0
in 72.485s, zero failures/errors/skips/expected failures/unexpected successes.
Head, tree, clean state, captured ALab sources/tests/scripts and Git/Python/sh/bash
entry identities are unchanged before/after. The nine counterfactual methods
are included in 112; the separate 24-control JSON audit is not added to that
count. Existing admission integration fixtures source a temporary copied
check_main body and stub the first build command with status 71; they assert
review boundaries and start no compiler or native executable gate entry.
Other checks use local Git/TOML/JSON/owned fixtures. This is no native application
build, Rust test, certificate, installed-adoption or semantic-review claim.

After validation main `d08ea03de8b880ec1d841588ac22f5102ce0f950` was integrated.
Its review document and native autopilot changes leave every captured input of
the 112-method run identical. The result remains bound to its tested commit and
does not assert execution of the incoming native changes.

Frozen source SHA-256 values: intake
`3729e46a7501ff96c32a0693587ed40f0cb2619e48073cf85845891bc7aa3f52`, status
`9b3fb1541f0d2917f3499699ea74d2dec2259dd9a5f7a2e88088b6e294e73b92`, admission
`544ef08d48c48764c0e4984411a8ef63bf0d07d8742e60c6260c1e696d99a8a2`.
New test SHA-256:
`9c5a65ec4926120f6c7c0c9d44d801eff7ba932917333a7b0d75a094f77c16cf`.

The recursive common-Git archive is
`alab-validation/6a95cd3649d7b8b9d36fed29abd0ae849a567dab/catalog-identities-6n3a5ele`:
372 payloads, 3,008,430 bytes. All sizes/hashes, exact inventory and selected source
Git bindings were verified. It retains coherent counterfactuals, raw logs, drivers,
root snapshots, the original duplicate-ID proof and no-change JSON audit (with
its initial private-expectation failure retained separately). The proof's exact
catalogs were extracted from its owned commits and matched to recorded hashes;
its mutable Git repository is excluded. Index SHA-256:
`3a5d407a9a4aedd55bbdf0d77d6bbdcf96cace486bd3e3cbc5d32406d874c24a`.
Final root log SHA-256:
`81ba5c629ad7b950e24fbc8da904aac8f073c27398f9ac39dfc12bbf69fec9ab`.

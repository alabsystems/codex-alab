<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Refuse malformed scenario descriptions before snapshot output

The next-issue read through the normal GitHub route returned HTTP 403 with
`issues=read` unavailable, actual command status 5. That does not establish that
there are no open issues. The refusal was recorded without another credential
route or retry. A nearby TODO/FIXME scan found no actionable item beside the
open-work reader and evidence exporter, so source review selected a separate
portable contract mismatch.

On baseline `260aa8bb015a6ed969dd8a681e6d6bc4eeb97b02`, feature_evidence.snapshot
does not enforce the scenario shape already required by feature_inventory.
The source iterates a string or table as character/key descriptions, accepts
non-string or blank entries, and can raise TypeError or KeyError for malformed
input instead of the public ValueError refusal. Some actual baseline CLI cases
return 0 and report pending-record creation for inputs that should refuse.
Those ephemeral fixture JSON files are not retained evidence artifacts; the
observations establish the return status and message, while character/key
reinterpretation is also visible in the source.

Decision: require a nonempty list of nonblank strings before exact duplicate
checking. This is a local input-contract guard, not an import of the complete
inventory validator and its unrelated ownership requirements. Invalid scenarios
refuse before a snapshot claims a new output. Description literals and order,
exact duplicate semantics, retired-feature omission, committed catalog selection
and exclusive existing-output behavior remain intact.

The identical nine new methods cover 16 malformed variants through the public
snapshot API and real isolated snapshot CLI: scalar/string/table/missing
containers, numeric/mixed/boolean/nested/table entries, blank descriptions, empty
lists and duplicates. Positive controls preserve whitespace and order, existing
output device/inode/bytes, exclusive success, malformed retired omission and
both directions of committed-catalog versus workcopy selection. Subtests are
not counted as distinct methods. The fixture reuses only an existing Git helper;
it inherits no other test methods and replaces no runner or native adapter.

The actual maintained baseline runner executes clean committed source
`f7cc432f6354653c0c1ee16014725f82889426c7`, with production bytes equal to
the selection baseline above. Original controller 8063 exits 1: nine distinct
methods complete, five pass and four do not. Unittest records 17 assertion
failure events and six error events. The errors are the intended wrong public
API TypeError/KeyError behavior, not fixture or import failures.

The fixed run executes clean committed source
`8877dde2ac4911c371721402c341d647a8b341bd`. Original controller 60467 exits
0, and the real maintained result runner reports all 79 distinct methods passed
in 44.235 seconds, with no errors, skips, expected failures or unexpected
successes: custody 9, evidence 22, gate 12, inventory 27 and new contract 9.
The focused nine are included in 79 and are not added again. These commands
execute the maintained checkout; archived source copies document their inputs
and are not presented as the executed private source trees.

All 90 captured source inputs match their exact Git commit and remain unchanged
before/after each run, with empty recorded tracked Git status and unchanged
Head. The two input maps differ only in feature_evidence.py. The controller and
new test are identical across observations. CPython 3.14.8 and Git 2.56.0
executable identities are captured before/after; these are the only two recorded
tool identities. Existing gate fixtures also invoke /bin/bash, whose pre-run
identity was not captured, so no complete tool-identity claim is made. Their
build hooks and argument-capture frontend remain inert. No native runner,
compiler, actual gate/profile, installed selector, certification or held
validation is exercised by these tests.

The common-Git archive is
`alab-validation/8877dde2ac4911c371721402c341d647a8b341bd/feature-scenario-contract-z2daruap`:
208 payloads, 4,234,409 bytes and 180 exact Git source bindings. It preserves
both original packets and indexes, raw stdout/stderr, result records, the
selection record, controller, comparison and original outer observations/logs.
Index SHA-256:
`1763b9057abdfa394b9aee183657cc50451c447015e7435d07dca48ce3622c3e`.
Original baseline index SHA-256:
`a83ea2ad7c7dc2a91c51f0d43fe6ac135094960176efe581bfb4016dfdde076b`.
Original fixed index SHA-256:
`c83fe7ad70673daef985878e330179b2d8b3ed6d3bf009aea00bb6edc87a630c`.
The identical new test SHA-256 is
`727e10c09e4aa312b0f1c46d6b4f47c4bce2a73ee4fc7d037216e0fa51a01eb9`;
controller SHA-256 is
`bdd56602b910a224055de26bbbd1c2929953cf6e0504ae385724b3e986dad36b`.
Fixed feature_evidence.py SHA-256 is
`c7415f0d9a8d90ae5185bbfbcef5334a2bcb2c5eafa79a3b356564749b1acd2a`.

A separate actual snapshot CLI invocation at the fixed commit exits 0
(original completion 406d31). Its retained pending record matches all 14 active
features and all 79 scenario descriptions in committed catalog order, with
every checks list empty. These are 79 pending scenario declarations, a separate
count from the 79 passed unit methods. No scenario coverage is qualified by
snapshot creation. The later observation is separately sealed at
`alab-validation/8877dde2ac4911c371721402c341d647a8b341bd/feature-scenario-real-catalog-r35j48hb`:
six payloads, 58,327 bytes and one catalog Git binding, index SHA-256
`dd9a64717a4b6e65417c23af782d7f81a3159ed8e884c712f0498fa08ebbce9a`.
The primary archive remains unchanged. Its metadata establishes no additional
pre-run tool capture or qualified native coverage.

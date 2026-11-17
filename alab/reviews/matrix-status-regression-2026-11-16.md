<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# CLI regression measurement and matrix status validation

The completed CLI guard change was measured against its starting main
`1074b5ff72b1f567490f2dfa1a8a0f0fea5e2c6c` and published composition
`7b13ecfd6905809a55203f01408a03c4d10755d7`. The unchanged frozen observer ran
22 controlled Python entry processes and 14 direct API identity controls. Ten
baseline false-success exits now fail; all twelve other child statuses and all
fourteen API identities are preserved. The final status vector matches the
previously measured fix. Comparing all fields in the CLI and API observation
records with that fix finds only sixteen owned executable-path changes, retained
explicitly. No introduced
regression was found.

Original tool session 19254/completion d0e993 exited 0. Its private packet is
`codex-smoke-cli-final-measurement-7b13-a6t3xi6p`, index
`838117401728944e889ebc78432bdabaf044bf08588d24e378163e10a6d251dc`.
All five imported production files match Git at the final source; their baseline
copies match the starting main. The observer's copied initial tool fields do
not establish a second observation: separate post-run rehashes establish stable
Python, driver and child bytes, in
`codex-smoke-cli-final-measurement-7b13-post-run-tools.json`, SHA-256
`17ef9c055afdbe6b335f5e23709f644baba449d3be23a102bca0f66f239ab48f`.
This controlled Python comparison ran no native host, Codex or live provider.

The next independently reproduced gap is in `renewal_matrix.case_passed`:
Python equates `False` and `0.0` with zero. The reader accepted these malformed
controller, guardian and raw-host statuses. Four controlled public-main runs
actually returned 0 and wrote `qualification.passed: true` for the guardian/host
aliases. Truthy malformed guardian objects also raised in the predicate; the
public entry already caught that error and returned 1.

The decision is to require integer zero for each exit status and reject a
guardian that is not an object. Boolean success, timeout and case-specific
nonempty proof requirements remain separate checks. This closes the reader's
false-success path without changing the case producer or native qualification.
The original reproduction is tool completion f25032, exit 0, packet
`codex-renewal-matrix-types-proof-8ly7lbic`, index
`7fbc274065ce5f9fc18e533f7196ea4cdb7a0fc8f7a9fcf74d7d7400e79ab450`.
It contains 19 predicate and eight public-main observations, with owned inert
inputs and explicit native admission/subprocess seams. It makes no genuine
native qualification claim. Matrix source is unchanged between the measured
7b13 composition and the new topic's starting main `5f8aa27052699540c0d4fc089ebfb565ff988ba4`.

Nine new methods were frozen before the production repair. On the test-only
source `3024a39126c7fa903aa27939d316b7da8ecf452d`, original tool session
1383/completion 89aab1 exits 1: four methods pass and five expose the reader's
status/shape defects. After the narrow repair at
`812c48f2afe3d0515cf1b4c1da871984aaf20c8c`, original tool session 55220/completion
7dda0c exits 0: the same nine methods and all 26 existing rotation-controller
methods pass, with no skips or errors. Tests exercise public failure stopping,
the retained passing prefix, the failed case's original result hash and the
unchanged pinned inputs. Independent source and fixture review found no blocker.

The maintained named-result captures preserve 901 Git-bound inputs and stable
executable maps. The red packet is `codex-wide-composition-python-a8bfrqxj`,
index `79156bad6887342c95e3f2c5382cae651f8cf0a9f603afa010171df83b4b5a86`;
the green packet is `codex-wide-composition-python-7547f77d`, index
`ec29798c20467792f4e4b2e21c2fad147c087adfb494bc00c2fbc5f3710adb3d`.

Main subsequently advanced to `cc1dbc8930427cdb89baf364da26c68acbfbb52c`.
A normal merge produced `de9278c7219fd13a905719307bb78a355362d773`, bringing
the shared bounded hash helper and listener-acquisition changes into this
composition. The matrix repair and frozen tests are unchanged. Original
63735/completion 0bea4b ran ten affected test modules on that clean merge:
159 distinct methods, 157 passed, two skipped, no failures or errors. Both skips
are the same certified `pre-promote` methods already held in the earlier full
workflow. Their module's maintained named-result runner, and therefore the combined
selection, returns 1. It is not a successful complete gate. The other nine
module children return 0, including all nine new matrix methods and all 26
existing rotation methods. The packet `codex-wide-composition-python-lkr5qvgy`
retains 907 Git input bindings and stable tools, index
`62892b4e21922ad56802ce5f1fe33c23ec19d723a9517218537b9c7a22bd3cc4`.
No test was removed or replayed to hide those held skips.

The earlier full Python workflow's skips have been inventoried: eight optional
native experiments, two certified-hook methods under the existing application
certification hold, and one filesystem subtest this platform cannot represent.
The real Trust package-source cache test was selected separately because its
Python source session and four small unverified builds do not use the held
external native producer protocol. Its actual exact-method attempt on the clean
7b13 source selected Trust 15698, then the existing protected profile denied
`/bin/ps` at `PackageSource.Session.process_start`, before any Cargo build.
Original tool session 55282/completion d29aa6 exits 1; its completed guardian
receipt records supervised-command status 1. The single named method records an
error. All 898 captured source inputs,
six tool maps, both private scripts and the selected PATH entries stayed stable.
This is unavailable permission evidence, not a passing cache experiment.
The decision is to preserve that attempt and its fixture under
`codex-package-source-native-7b13-1.noindex`, without an unprotected retry or
profile relaxation; its index is
`8f372659d1f959d46f3c121a54665d14364c0313f2b333eee3de0d751de673d8`.
The producer/cache-bridge and certified-hook operations
retain their human holds. The old native timeout cause remains
conditional on a new failing diagnostic trace; later passing checks do not
identify its cause. No held operation is retried.

The remaining diagnostics opt-in uses a different maintained route:
`native_diagnostics_probe.sh` starts outside containment, freshly selects the
source's compiler, and owns its protected native children. Its helper graph
does not use `PackageSource.Session`, the held external producer protocol,
application certification or release/adoption. The actual retention-policy
preflight returned 0. This makes it eligible for a separate check; it does not
make the denied cache experiment available.

The first diagnostics preparation, original tool session 63990/completion
2911f3, exits 1 before creating a native fixture state. Its outer selector had
exported the maintained `ALAB_CARGO` alias, which the probe's fresh selector
explicitly refused as an inherited override. The one named Python method
records failure. All 11,522 source inputs and the tool/script maps match before
and after. The retained V1 packet is `codex-native-diagnostics-7b13-1.noindex`,
index `db25493b6f5ef353b6d9aff2513b00495bb6940c59aaed763d1a2d2c8f862d6d`.
V2 requires and removes only that exact maintained alias from the child
environment. The unchanged probe must then select the pinned compiler itself;
the observer's qualified tools, source pin and the maintained protected profile
remain in force. Independent review approved this correction to the selector
contract. V1 remains a failed preparation, and the denied cache test is not
retried.

The actual V2 run, original tool session 96762/completion 2ed960, exits 0.
Its completed guardian receipt records command status 0, and the exact single
named diagnostics method passes. Both intended fixture phases and their Cargo
rounds retain status 101; the outer protected-command status remains 101 too.
The maintained verifier confirms the exact expected failed native case per
phase, retained bounded witnesses, an omitted outside-root symlink, and removal
of the allocated protected scratch. All 11,522 source inputs, seven physical
tool maps and both frozen scripts match before and after. This closes one
optional diagnostics check on the clean 7b13 source, separately from the earlier
full workflow and the unavailable cache experiment. It certifies no application
and identifies no cause for the earlier unrelated timeout.

The completed native evidence is retained outside the evictable target under
the shared Git directory's `alab-validation/7b13ecfd6905809a55203f01408a03c4d10755d7/`
in `native-failure-diagnostics-v2-private-0n2o58cp`. Original capture tool
session 62575/completion f70b3b exits 0. The sealed archive has 11,710 payloads,
149,445,171 bytes and 11,522 Git source bindings, index
`9fd0043271243be741f973f9b7375b73dfbe07299d57f44381f3407300012497`.
It preserves the whole observer and probe states, both declared diagnostics
roots, raw runtime/descriptor/environment receipts, metadata and scripts.
Opaque evidence was copied and hashed without projecting its values; compiled
helper/test binaries and the compiler execution directory are not copied.

After the composed Python check, main advanced to
`e3aa26f8761934e04407b8eafc1585d7154c4228`. A normal merge produced
`b584774aee44f84bf6f3e1051c9da070838270bf`. Its additions concern Rust startup
routing, SSE test fixtures and their reviews. Within `alab`, `publish`,
`trust.lock` and `VERSION`, only two review documents differ from the checked
de9278 composition. All executable Python inputs and frozen matrix tests are
unchanged. The recorded checks keep their exact source scopes; this merge
adds no new Rust, application-certification or full-workflow claim.

Main then advanced to `89b3209b7324d314e44ca06f0f6b1ab97e5a0190`, adding
cached image snapshots, the shared listener's preparation hook and the matrix's
hash-helper input pin. A normal merge produced
`00de639b8b89aa42cc26707bd1949fd56139b755`. Those executable changes justified
a new affected check. Original tool session 54944/completion 56f42a passes all
108 methods across ten matrix/listener/image/hash modules. Original 35928/
completion ae9ad8 passes all 84 renewal-model methods. All 192 IDs are distinct,
all eleven runner children exit 0, with no skipped, failed or errored methods.
Both captures retain 912 Git-bound inputs and stable executable maps. Their
private packets are `codex-wide-composition-python-mm6kph9e`, index
`9897aad7b2248b1bab689c1b91d348f0dfe5b944928cbdad79730da65b2aff7c`, and
`codex-wide-composition-python-aqurm2b_`, index
`f862f4a21e18d0e3a1890415516eec27e83547646b69ea19518fe2993286ac62`.
This new affected selection does not replace the earlier 159-method result
with its two held skips, the 7b13 native diagnostics result, or the earlier full
workflow. Publication uses a literal fresh main lease and retained guardian
completion, followed by remote ancestry and owned-branch cleanup checks.

Final alignment also retains main's journal-lock owner correction at
`1d8f34bca6a012a2cbc03680f7fa72659e323a5f`. Its one-line Rust change leaves
all checked Python inputs and the matrix repair unchanged. The 192-method
result and native diagnostics evidence retain their recorded source scopes.

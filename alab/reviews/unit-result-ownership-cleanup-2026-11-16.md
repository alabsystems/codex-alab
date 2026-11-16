<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Preserve failures across unit-result preparation and collection cleanup

The follow-up review of the manifest cancellation fix finds the same exception
replacement in the public `prepare` and `collect` functions. Their raw `finally`
closes can replace a pending receipt/identity refusal or interruption with a
secondary close error. Preparation rollback can do the same through `rmdir`.
The first preparation entropy call also occurs after acquiring the target
file descriptor but before its cleanup protection, leaking it if entropy fails.

Decision: apply one owned-directory context manager to the preparation target
and child, collection root and leaf, and existing manifest child. It delegates
error cleanup to the existing cancellation-selection policy, generalized to
cover rollback removal. Generate the diagnostic name before acquiring the
preparation target. This closes the remaining ownership scopes consistently
without duplicating the exception policy at each call site.

A pending cancellation survives subsequent ordinary or cancellation errors.
Cleanup cancellation supersedes an ordinary primary error with its cause and
note; an ordinary secondary error becomes a note on the primary. Successful
body completion still propagates a close error. Every adopted descriptor is
attempted once; borrowed parents are left to their caller. Rollback happens
only after successful directory creation while the target is still owned.
Successful target-close failure remains visible after state save without
removing the committed diagnostics root. Schema, path/identity checks, flags,
permissions, exclusive save and qualification decisions remain unchanged.

The counterfactual uses the exact `unit_results.py` Git blob from
`0e551869d42b414fe8df577864c4631be8ba9227`, SHA-256
`db540dbfc1bee29955e9e548e1cd9feb0887da134ba05c59ca3218e6c9c1d994`.
The same maintained collector controls have 12 assertion failures and four
passes there, actual exit 1; all 16 pass with the fix, actual exit 0. The
preparation controls run 19 parent methods: 11 parents have 23 failed assertion
events, eight pass fully, actual exit 1. All 19 pass with the fix, actual exit 0.
Neither baseline has errors or skips. Subtest assertion events are not counted
as additional methods. All faulted directory resources are module-local
synthetic descriptors; small metadata, artifact and round-status fixtures are
owned temporary files. No synthetic descriptor reaches the real OS.

Root independently verifies the old blob, frozen driver/source/test hashes,
case inventories and baseline/fixed outcomes. Final ordinary unittest discovery
runs the four complete affected modules: 36 directory cleanup methods, 19
preparation methods, 16 collection methods and 26 existing native collector
fixtures. All 97 distinct methods pass in 11.649 seconds, actual exit 0, with
zero failures, errors, skips, expected failures or unexpected successes. Source,
all four test files and the reused native adapter have identical before/after
hashes. Earlier subset executions overlap this final union and add no coverage
count. Independent review approves the ownership implementation and public
function controls; they do not mock or call the new private helpers.

A read-only recollection with the new reader also accepts the retained actual
twelve-library result packet from runtime source
`599ed29083688a6b937b7d96750eca2ec3b13ba3`. The original Cargo status is zero;
the new collector exits zero, names 12 libraries and no failed copies, and
produces byte-identical JSON, SHA-256
`6491e48b24254e8b9ff6725b53ceb797e77aec6e6ca45d1129621b0776e05ffa`.
Original state/log/status/result files remain byte-identical. This exercises
the updated collection path against real retained receipts; it replays no Rust
tests and does not relabel the original native run as this source revision.

Frozen final SHA-256 identities:

- Reader: `5e8f7292c29edc41a9a098ebee610832a382a2d18ed31704ee0eeaeeef958be1`.
- Preparation controls: `3948ab0e94c8a98e645f6584a8f402f13d6ce4b46d5c7cb567f3c9cbc0fa577e`.
- Collection controls: `021dd0b494a2853cab4aa6c38373689f37fecfee7e0548a3c06522c205b30976`.
- Reused native adapter: `b3ba555c1b547cf89fc7931e15fc70b5e92ffe803f417b973e451f4bb0998d0c`.
- Final union log: `7b1dcf646b07c6b820183d5276b0ff49062ab37a1de64fb97b4bce7fca21a79a`.

The common Git directory retains 49 payloads (4,810,555 bytes), including exact
sources, both counterfactuals, frozen drivers, final test records and original
native state/log/results, at
`alab-validation/0e551869d42b414fe8df577864c4631be8ba9227/unit-result-ownership-cl968rdw/`.
Index SHA-256:
`8d2cb53e6eb6242bbcffd8efc2b25677062a67d9807c304be5f3b02f1241fef9`.
Root verifies every payload size/hash and the exact regular-file inventory.
The base-keyed archive identifies the working fix by its file hashes; it does
not claim the unchanged base commit contains the fix or qualified application.

This packet changes only ALab-owned Python code, tests and this decision record.
No compiler, protected profile, application build or native helper changes.
The previously recorded pinned Trust application-certification failure remains
separate; this focused packet neither reruns nor claims the full gate or whole
Python workflow. Fresh main at `a8ece33d62e94c6b9bcebea33839f6af08dc31d8` adds
sync, intake and aterm work in disjoint paths. Preserve it during publication;
the results above remain bound to the recorded code and test hashes.

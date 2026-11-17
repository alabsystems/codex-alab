<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Require valid included feature IDs before snapshot output

feature_evidence.snapshot accepted numeric, boolean, empty and blank IDs for
included contracts. Actual API/CLI observations retained pending JSON carrying
those IDs, while numeric-ID validation later raised TypeError. Missing IDs raised
KeyError, and list/table IDs raised TypeError, instead of the public ValueError
refusal. feature_inventory already requires nonblank string identities.
The minimal guard now enforces that same local contract before snapshot output;
all 95 affected feature-suite methods pass.

Decision: read the ID with get and require a nonblank string after the existing
retired omission. Preserve exact whitespace, case, Unicode, source order and
duplicate semantics for valid IDs. Retired entries still omit invalid or missing
IDs without reserving an included ID. No complete inventory-validator dependency,
string coercion or normalization is added.

The independent original proof executes twelve committed owned-catalog cases
through the actual public API and isolated snapshot/validate CLI at
`f810fd98e874d60aa1f11e5e496e013bdb36cf02`. Original controller 6586 exits
0, terminal a55abb. It retains actual malformed pending records for integer,
boolean, float, empty and blank IDs; each snapshot CLI returns 0 and creates the
output. The numeric cases subsequently produce API TypeError and CLI 1 during
pending validation. Missing/list/table cases have wrong API exception types;
literal, case-distinct, exact-duplicate and retired-omission controls bound the
change. These twelve observations are not unittest methods or qualifying
scenario evidence; their checks remain empty.

The same eight maintained regression methods cover ten malformed ID variants
under both active and retiring lifecycles. They require API ValueError and an
actual CLI (status, output-exists) tuple of (1, false), so baseline output claims
are observed rather than inferred after a short-circuited status assertion.
Controls retain existing output device/inode/bytes; preserve literal whitespace,
case, Unicode and order; keep exact duplicate refusal across included lifecycles;
omit retired malformed IDs/scenarios without reservation; preserve committed
selection in both directions; and keep valid output exclusive. The fixture reuses
only an existing Git method, with no inherited test cases, native adapter or
mocked runner. Subtests are not counted as distinct methods.

The baseline maintained runner executes clean committed source
`f5a49238ed7e44c2408d81e76ff705a22a31286c`, whose production matches f810.
Original controller 88445, start 6bc220 and terminal 4c1b42, exits 1. All eight
methods finish: five pass and three have failed subtests, with 30 assertion
events and six wrong-API-exception errors. These are the intended public contract
failures, not fixture/import errors.

The fixed maintained run at
`009579dba45c15fd16785f50cecd4c8d437514ba`, original controller 23047,
start aaeb8e and terminal eb61c8, exits 0. All 95 distinct methods pass in
74.799 seconds without errors, skips, expected failures or unexpected successes:
custody 9, evidence 22, gate 12, inventory 27, scenario contract 9, lifecycle
contract 8 and new IDs 8. The focused eight are inside 95 and are not added
again. All 93 captured source inputs match their Git commit and remain unchanged
before/after, with clean recorded tracked status and unchanged Head. The two maps
differ only in feature_evidence.py; the test and controller are identical.
The three recorded Python/Git/Bash executable maps are stable before/after and
across runs. Existing gate consumers retain their inert hook/argv fixtures.

A separate actual snapshot CLI at the fixed source, original completion 29c71d,
exits 0 and retains all fourteen actual catalog features and 79 pending scenario
descriptions in exact order with empty checks. The actual catalog is all active;
retiring behavior is covered by unit fixtures. They are pending declarations,
not 79 more unit methods or qualified coverage. The original proof and real
probe remain separate from the eight-method counterfactual and 95-method suite.

The sealed private common-Git archive is
`alab-validation/009579dba45c15fd16785f50cecd4c8d437514ba/feature-snapshot-id-contract-u1h253ue`:
387 payloads, 4,604,078 bytes and 193 exact Git bindings. These comprise 93 inputs
at each unit source, six original-proof inputs at f810 and the fixed real
catalog. Both original unit indexes are retained. The original twelve-case proof
and five-file catalog probe were unindexed; their explicitly derived post-run
inventories preserve every original artifact without changing it. Selection,
comparison, raw logs, source/driver copies, retained malformed records and all
original outer observations remain present. Directories are 0500 and files 0400.
Index SHA-256:
`df284115db5878e13ba586b23f876a4f22d5840e3a3e48370508baf2c0d0f979`.
Original baseline index SHA-256:
`adacf65ae44f359c62ceebbea3dbc0caf89d190432d7b7b1546a9a48cc8ed51e`.
Original fixed index SHA-256:
`0b12e81ac78d9f946714377ffca3c72047506afb7b49d212504d5ffeca97b20c`.
Fixed source SHA-256:
`b1d98afe5b622c94eaab000047821da57252e6e2321653a42dffa103a8537d0b`.
Identical new test SHA-256:
`ec05a178eab741c6e0cb2633f2372485ccfeec4f1c2e5499ca5a4b0d132e76c3`.

The broad [full Python workflow](merged-wide-python-2026-11-16.md) is separately
executed on frozen f810. This repair stays in the owned second worktree while
the broad Python and Rust baselines retain their original source. Full-sweep
results are not relabeled as execution of the ID fix or summed into a single
tested tree. No application certification, installed adoption, held producer or
rejected release action is attempted; the historical issue-read 403 is not retried.

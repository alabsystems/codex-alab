<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Merged-tree gate findings and ownership repairs

Decision: run the normal executable `alab/check.sh` through `qualification.py`,
retain its actual failure, and repair source findings without suppressing lint,
widening visibility, or presenting component tests as complete scene support.
Each gate attempt used source-selected Trust 15698 and the existing protected
profile. None reached application certification, application runtime checks,
or the subsequent complete ALab and hooked-upstream library suites.

The first attempt on merged main
`634808da1717cf7acc8a82dd92791e1bb851382c` failed at the full all-target Tippy
policy with 955 located warnings and no source errors. A complete closed-log
inventory contains 932 dead-code warnings, 20 concrete expectation/enum-size/
argument-count/type-complexity findings, and three unused imports. Main advanced
during this run; its record correctly leaves `qualified_source` null.

Commits `349837ca80272a95d1857b69a09106c45d9d9a9a` and
`7c9153d66ee138cff4876ed04f39f3f976478d61` repair those 20 findings. Mailbox
publication retains the same lock and detached map through refusal and unwind;
only a consuming seal transfers the original messages. Late publication now
rejects missing, claimed, or foreign delivery ownership before charging counters.
Queue preparation validates the actual rows before moving them. Large state
payloads remain owned behind boxes; the callback observer receives a typed
context instead of a long parameter list. The only upstream edit is the existing
marked construction hook for the boxed start context.

Two source-bound native library invocations on 7c9153 passed 51 distinct tests
(19 Core and 32 TUI), with no failures or ignored tests. Both used the maintained
unit runner and authenticated three-library graph. Actual test, collector,
coverage, source-observation and guardian statuses are zero. Before/after source
and tool books are identical, SHA-256
`567e6a37f8f37ecf76c53bc46453c84649d7e54b58347f5635f2761eb3ead0d5`.
The finite saved evidence contains 60 payloads and 24,166,469 bytes; independent
verification rehashed every payload and checked private modes. Index SHA-256:
`f8b1a66e1565d07007a9bd9ee6a6f1f87389a9831e4d659550524ab99d8c0739`.
These are unverified test executions, not an application certificate or a
successful retained Native E1 transition.

The full gate rerun on 7c9153 also failed at lint policy. Before/after source,
tree and clean status are identical; both origin observations are dd83242c73.
The supported explicit hook-comparison base is the original 634808da main;
normal upstream intake admission and proof requirements remain unchanged.
Closed log SHA-256:
`10e0910b780f4c949e0a818d18030b26ae95dcc79fe8a231736863a58345aaed`.
The complete inventory has 938 warnings: 935 dead-code and the same three unused
imports. A multiplicity-preserving comparison by normalized path, family and
message proves all 20 targeted events disappeared and three dead type-alias
events appeared in the still-unconsumed scene graph. This is not a green gate.

Review also found that the consuming image installer could drop its prepared
resources or a successful callback result after an uncertain attempt. Commit
`72dc4c9294fbfaff172556276af163cb5def4217` replaces that shortcut with a caller-held
installation transaction. Preflight refusal preserves the exact `FnOnce` and
permits retry; entering the callback makes the outcome nonretryable `Unknown`
until all postconditions and real renderer handoff succeed. Failure and unwind
retain prepared, displaced and current image resources and invalidate the real
renderer watch. Both callback result and original `io::Error` are destroyed
before their custody owners. Independent source review found and corrected the
error-destructor ordering before application. Six local-resource tests cover
these paths without constructing a Native grant or whole-scene acknowledgement.

The code merged cleanly with main a356d8e527 at source
`2ccb3b497eba02d0d0af3b2e407540e0ea3fe670`. One maintained native invocation
passes 62 distinct tests: the earlier 51, all six image-installation controls,
and five existing image-alias controls. Core passes 19 and TUI passes 43;
the authenticated app-server library has zero matching tests. No test fails or
is ignored, and test, collector, coverage, source and guardian statuses are zero.
Original process 23876/completion dff4ef returns zero. The exact per-library ID
Counters reconcile independently of Cargo output order. Before/after books
cover 12,497 source inputs and 11 tools and are identical, SHA-256
`cd89f612415376fa410c4b36eb1ced5ee8347ccad98db606dc70e8296544371c`.
Raw unit log SHA-256:
`2b2ea936dafcb2d69264413e48dc36472d08c1e08a60e6127c73a9b0d339ceb0`.
The saved packet `codex-image-install-62-evidence-4ht7llxq.noindex` contains
54 payloads and 12,907,584 bytes, including identical source/tool books,
20 selected immutable Git source blobs, scripts, actual results and opaque
native evidence metadata. Its index SHA-256 is
`3930551012d8ae44fc9c0a11210fccea6faaba22a089a1909a30346a4b6ec390`.
Independent verification rehashes every saved payload, reconciles the exact
recursive inventory and checks private file/directory modes; it executes no
archived program and interprets no opaque native context.
The native peak-memory advisory could not keep its record because of an
ordinary permission refusal; execution succeeded without a bypass. No memory
measurement, certificate or complete-scene result is inferred.

The canonical full gate on 2ccb3b49 also fails at lint policy, with no source
errors: 951 dead-code events, the three unused imports, and four concrete lint
events from the newly merged main. The image transaction contributes 17 new
dead-code events and removes one old unused-field event; its aggregate remains
unconsumed by the missing whole-scene assembler. The four additional concrete
findings are three mutex guards held across awaits in shell-output retirement
and its test, plus a range loop in a banner coordinate test. The closed gate
log SHA-256 is
`16859c80c8f534c30840522a12966a6427fd5f0762aaf4d5fc8520ac453c119e`.
Source/tree/cleanliness remain unchanged while origin legitimately advances;
the execution record leaves qualification null.

Fresh main 0ee39bc116 already contains
`197b3bb4c8e32631b107e16e4ca04042d38448c4`, which fixes all three lock scopes.
Independent review confirms that identity checks and original task admission
stay atomic, while owned waits move outside registry locks; the cancellation
test still suspends after protecting the real handoff. Commit
`5d80ebc0d9be1b245ea2784c140a9a2ac14193dd` fixes the remaining banner test loop
by iterating the strict checked coordinate slice. It preserves out-of-range
failure and every row/cell assertion. The existing exact coordinate test passes
on 5d80ebc0, with all result-accounting statuses zero and identical before/after
books covering 12,498 source inputs and 11 tools. Book SHA-256:
`4b89e11d48f26b8bafb3fde4424ea31feac93038129eded30f07d5bb8d4ede67`.
This single owned-crate test is separate from the 62 tests on 2ccb3b49.
Its private saved packet contains 33 payloads and 10,735,754 bytes, index
`31e98b793dae85afc2f0d2a173d0b1b292f5dd039faed7e5f67e4ab60440cfd7`.
Independent saved-only audit verifies the exact inventory, source/tree bindings,
private modes and sole passing test; opaque native metadata is only hashed.

The full gate on 5d80ebc0 confirms exact removal of those four concrete lint
events with no new diagnostic identities. It still fails at lint policy with
951 dead-code warnings and the three unused imports, no source errors, and
six separate tool notices. Before/after source/tree/cleanliness are identical;
origin advances during execution and qualification remains null. Closed log:
`723335dddedc55904d9a8f1087fe5d914233a0edb47dc1e3a1a3ba0e1d312af2`.
Decision: land the tested ownership and lint repairs with this failed-gate
record, then continue real scene integration. These source repairs do not
promote or certify an application build, and the complete gate is not green.

The retained-scene work remains incomplete. Its source must include the actual
outer startup actor's event FIFO, terminal stream, backend collection, reconnect
and pending operations as well as App/widget/transcript/image state. Capture
and installation roots are still uncalled, and thread projection alone cannot
establish complete scene installation. The required work is genuine H1 capture,
one shared graph, original operation checkpoints, complete staged installation,
and a private completion witness before input, draw or ACK. No empty scene,
recreated task, hidden lint exemption or fake Native permission substitutes for
those consumers. Separate external component proposals remain unapplied until
their real aggregate caller exists. The widest-check task remains active.

<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Report incomplete operations and select artifacts by canonical ancestry

Exact source `1b28ce62d57bd18a039f5a1699ad93e31916fda8` had three adjacent
failure paths: caller grafts could authorize unrelated artifact promotion,
interrupted CLI operations could exit successfully, and natural scratch-cleanup
notes disappeared from version diagnostics. Owned local fixtures reproduce
each problem. These changes retain the existing conditional selector, GitHub
route, expected-ref checks and release-history admission rules.

Decision: pin the empty graft file beside `GIT_NO_REPLACE_OBJECTS` in
`promote.git_environment`. Both promotion ancestry checks then use actual
parents: selected source to candidate, and candidate to origin/main. A forged
parent cannot authorize either check; a hidden real parent cannot block a valid
forward update. The existing local-update caller inherits this environment
through its current helper dependency. Physical graft/config/marker/package
contents remain intact. No new transport dependency or installed channel change
is introduced.

Decision: distinguish a returned CLI operation from an exception requesting a
successful process exit. Git read, publication and version CLIs parse arguments
outside their operation guards, so help remains successful without invoking an
operation. After entry, no-code SystemExit and success-shaped native POSIX integer
statuses become status 1 with an incomplete-operation diagnostic. False is an
integer zero; integer 256 also actually exits zero on this Mac. Floating 0.0
already exits nonzero and remains an unchanged exception payload.

The shared classifier reads accepted integer payloads with `int.__index__`,
avoiding their overloaded operators. POSIX low-byte classification is bounded
to the signed native-word range; oversized integer payloads retain the existing
interpreter behavior. Other platforms only classify integer zero/no code, and
are not described as executed here. Nonzero 47, KeyboardInterrupt, float/string
payloads and out-of-range integer controls retain exact exception identity.
Public fetch, publish and release-reader APIs retain their cancellation policy;
the conversion belongs to the CLI boundary. The reader's old module-level
failure handler is consolidated into `main`, which now returns its CLI status.

Decision: preserve natural exception notes in both release transport wrappers
and all three CLI diagnostics. The shared reporter prints the available primary
header before reading advisory string notes. Ordinary note/sink failures cannot
make the command successful; a success-shaped reporting SystemExit also returns
1. Nonzero reporting exits and KeyboardInterrupt still propagate. Publication's
Advanced branch retains its exact machine-readable fetch-first row inside this
guarded reporter. No-note primary headers and healthy output remain unchanged.

Focused controls use coherent exact baseline files and identical final tests
against frozen fixes, with source/helper/test/tool identities unchanged:

- Five promotion methods: baseline actual exit 1, four intended assertion
  failures and one replacement-object guard pass; fixed actual exit 0, five
  passes. Real Git/raw-parent and ordinary fake-view controls isolate both
  forged ancestry checks and both hidden-parent cases. Fake selector callbacks
  check exact conditional arguments while configuration, grafts, marker,
  package bytes/modes, HEAD, refs and raw reflogs remain unchanged.
- Ten CLI methods: baseline actual exit 1, 23 assertion events plus 34 error
  events across six failed methods; fixed actual exit 0, ten passes. Each side
  records 37 actual child process observations executing the original entry AST,
  with called/returned witnesses and exact escaping exception identity. Raw exit
  counts change from `{0:24, 1:6, 47:3, -2:3, 255:1}` to
  `{0:6, 1:24, 47:3, -2:3, 255:1}`. All 18 observed incomplete operation/report
  false successes become failures; six healthy/help controls remain successful.
- Eight version methods: baseline actual exit 1, ten assertion events across
  four failed methods, four compatibility passes, zero errors; fixed actual
  exit 0, eight passes. Real owned malformed-release and nested Agh batch
  cleanup faults create natural notes before list/fetch wrapping or CLI output.
  No note is fabricated. Both transport wrappers retain them, while direct
  release-reader zero/None cancellation objects remain unchanged.

Fault namespaces are module-local. Real owned directories are cleaned before
cleanup errors are injected. Entry tests execute the parsed original module
entry, rather than adding a manufactured exit wrapper. The separate baseline
proofs and every measured child stream/witness/entry-AST identity are retained.
Focused controls overlap the final run and add no separate coverage count.

The final fourteen-module run uses clean committed source
`6d991100c6cad0c2034bc46df1c3e2ade00e4bee`, tree
`4726dcf92e18371afdde9de7ad39de0d1384a27a`: 188 distinct methods, 186 passes,
two existing certification-dependent pre-promote skips, zero failures, errors,
expected failures or unexpected successes. Actual exit is 0; unittest reports
323.067 seconds. This includes the existing Git suites, all new controls, full
promotion/local-update tests, ledger and version-line tests.

All 56 captured sources/scripts/tests, the controller itself, Git/Python/sh/bash
entry identities, Head/tree and clean state match before/after. The capture list
was independently audited before launch and includes `setup.sh` and the version
ledger. The run records CPython 3.14.8 and Apple Git 2.54.0 on this Mac. Signal
and wrapped-exit observations are scoped to that POSIX environment.

Tests use owned repositories, artifact directories and markers, fake builders
and selectors, module-local responses and read-only shell version fixtures.
No real installed selection, persistent development channel, Rust compilation,
production certification gate or shipping certificate is exercised. The two
existing skips and committed pre-promote certification note remain unchanged;
their required compiler support remains explicitly deferred.

Final SHA-256 identities: promotion
`babbedea2da5bf67286bf7429be3fd19970d3fb3d53ea1e292fa05f106db208f`,
reader `8a31cb4d6cc2ae0e6c68bff2b9729966c39d3386e3e994c26acf08366d1cd87c`,
publisher `ac9f47ef5e7c550ec30824930dcfb2f1050f341dd66e0882cdce21c7c2701927`,
ledger `7b648eb57cfce0e08b7caf803db0bb63ef29519b8259b637b8e4ea0048e4d4e1`.

The recursive common-Git archive is
`alab-validation/6d991100c6cad0c2034bc46df1c3e2ade00e4bee/operation-completion-huc4m6ae`:
692 payloads, 4,589,168 bytes and 262 immutable Git bindings. Root verifies exact
inventory, all payload sizes/hashes and selected Git source bindings. It retains
all focused pairs, original proofs, measured children, complete root snapshots
and raw logs, with their controllers. Index SHA-256:
`8d5112602fd54475729e34a225537c4c7ee614b2a62cfcdfbc5da55a124aa5b2`.
Final root log SHA-256:
`414de527ee695f4d1321cb7903f0e6cf35b0c119a6c6dfab52b7e8f676cdb66b`.

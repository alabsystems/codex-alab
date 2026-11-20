<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Sequential libtest output collection

Decision: land the independently verified collector change separately from the
Rust component repairs that are still being verified. This candidate carries only
`alab/unit_results.py`, its existing test module and this record over main
`d8dd5e00e2b8930ee444b83d6ea994ff5d7ea34e`. Peer Rust, defaults, Cargo inputs and
unrelated tooling retain that main's exact Git entries.

A real sequential component run exposed the trigger: fixture stderr appeared
between a named libtest header and its standalone `FAILED` outcome. The old
inline-only parser missed that failure and refused the inconsistent summary.
The component failures remain failures; collecting their names does not repair
or qualify the tested application behavior.

An explicit `collect --sequential-output` caller may now attribute one standalone
`ok`, `FAILED` or `ignored` token to one pending named header. Terminal cursor
controls separate records; SGR styling does not split a test name. Duplicate or
ambiguous results, orphan outcomes and unfinished headers refuse collection.
The maintained `ResultOutput(sequential=True)` interface shares this logic with
private evidence readers. Callers must actually use sequential execution, such as
`--test-threads=1`; the option is a caller contract, not native argv attestation.
Default collection preserves inline-only attribution for ordinary parallel output
and does not claim a complete passing-row inventory. A regression test preserves
that default when cursor controls interrupt a passing header.

The exact two Python blobs were exercised at
`cb022bac7628cae60c8f31045066371afc9fc465`. The maintained qualified host workflow
ran three real native-context prerequisite tests first, then all 34 methods in
`test_unit_results`; all passed, with zero failures, errors or skips. Independent
qualification is `91564440ececfac3a68b0f46177307f48a0ade738ef3f8699511fcaeec1dd45e`.
These are separate prerequisite and module counts, not a broad Rust-suite result.
The split candidate requires an independent equality review of the test, helper,
runner and fixture dependency closure; this record transfers no operational
native admission or application authority to the new main-based checkout.

Landing the bounded Python result-reader improvement avoids tying it to unrelated
component runtime failures or pending memory admission. No test is skipped, no
failure becomes a pass and no gate/profile requirement changes. This is not a
passing widest gate, a new full Python workflow, application certification,
installation or adoption. Explicit human holds remain, and the original opaque
Bad Request cause is still unknown.

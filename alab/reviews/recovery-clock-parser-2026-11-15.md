<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Clock decoding preserves the original parser — 2026-11-15

Decision: audit clock keys while the original JSON parser consumes the value.
Keep unusable clocks optional without weakening the former Value reader's
numeric, Unicode, nesting or reserved-key validation.

Hostile review of `90248de15e03313565d65b1bcecf3edb0fb85ca6` found that
RawValue's scanner can skip checks performed by Value, and from_str reparsing
starts a fresh nesting budget. Swallowing the later typed clock error therefore
accepted records the former reader rejected. This was a codec-domain defect;
no shortened native timer, unauthorized action or original HTTP400 cause was
demonstrated.

RED `0c22b5cf610b68d7959cab596e7fab23866b8870` selected all six record
contracts in the default JSON numeric graph. Four passed and both new methods
failed, without ignores, measured methods or filters. Native unit/controller
exits were 101. Actual raw serde and regular-file readers produced 36 clock
mismatches from overflow, lone surrogates and excessive nesting, plus 12 parent
mismatches from the exact nesting boundary and a first reserved RawValue key.
Positive controls exercised valid clocks, bounded invalid clock shapes,
125 nested unknown-field arrays and 126 nested unknown-parent arrays. The
15-file archive record hash is
`725300d807a3bf8b2c33e9512585342d805c8f44a93dd5365bb3273a9294b9c3`.

The replacement calls deserialize_any on the original reader. Map and sequence
access delegate to public Value deserialization, preserving its validation and
feature-dependent numeric domain. A map adapter inspects decoded keys before
normalization: repeated parent clocks and duplicate or unknown clock fields
still clear only the clock. Existing typed wait validation, bounds, conversions,
schema and recovery authority remain unchanged. No dependency or feature was
added. Scalar handling follows the locked serde_json 1.0.149 Value visitor.

Review also caught a private Value encoding that expands an entire wait from a
first `$serde_json::private::RawValue` string. That expansion can collapse clock
duplicates before the adapter sees them. Such decoded wait facts are retained,
but their unsupported timing encoding supplies no clock. The same key after an
ordinary first key remains an unknown field and preserves an audited canonical
clock. A seventh method checks eight encoded payloads and two later-key positive
controls, each through both record readers; payload quoting preserves duplicate
keys until the actual decoder runs.

The intermediate `fe91ca74522e19d26782e71a21ea03e5ad2cb511` did not compile:
visit_unit called the Visitor trait's visit_none without its generic error bound.
No runtime method or graph guard executed. Its failed runtime archive has ten
files and record hash
`179f677b5f1bfebd7b85bfe38438ba9b744b587af0f443f6a7d8d9b8dca7a337`.
The corresponding Tippy attempt also failed with E0277, retaining twelve files
and record hash
`e7c29d09b0515394c6f404d8b43e87af31af5aa7a97e73c410a652cbd28ba115`.
An earlier private lint launcher omitted the helper import and exited 127 before
any Tippy compiler ran; its original log and status remain separate. These
failures are not accepted runtime or lint observations.

At `b79af3004f8ed454712e9bbbcedd017909f0dd75`, direct construction of Value::Null
fixes that bound. All seven default-graph contracts pass in 0.03 seconds, with
zero failures, ignores, measured methods or filters; both exits are zero. The
runtime graph guard observes no arbitrary precision. Its 15-file record hash is
`90b57f34c0513667322f2b3506674bb42098c3e5c6ff893c8fd3a5d6bdd76aa9`.

The full selected TUI and record-contract libraries also passed at `b79af3004f`:
370 TUI methods and seven contracts, zero failures, five ignores, no measured
methods or filters, with the arbitrary-precision runtime graph guard enabled.
TUI's 375 methods here reflect this selected dependency graph; the separately
recorded earlier four-library graph enabled additional test-support methods.
Both native exits were zero. The sixteen-file record hash is
`dc0d1890155e2e131f534d3f4e2efdadca45d39e5dd3f49fdfcd1b0370e20d48`.

Tippy at that source compiled the two selected packages but classification
refused one owned collapsible_if warning; two upstream diagnostics were set
aside. This remains a failed lint observation. Its twelve-file record hash is
`9a8145cfd640018143596f745bea15ee43d2b5b620baa87547952ec166a1167d`.
The final `02227aded7d51a5e6d366e81c96953cb68bf75a4` changes only that nested
conditional into an equivalent let-chain; all contract source bytes remain
unchanged. The final-source default graph again passed all seven contracts,
without ignores, measured methods or filters, with both exits zero. Its
15-file record hash is
`b488e02d1059575180bef7811b5a0804dd98e4c7162578311471c7930a915793`.
Final-source arbitrary-precision verification passes all ten exact selected
methods: seven record contracts and three TUI writer/record regressions. There
are zero failures, ignores or measured methods; 372 other TUI methods are
filtered and supply no final-source runtime pass claim. Both exits are zero,
all ten required identities pass once, and the runtime numeric guard observes
arbitrary precision. The sixteen-file record hash is
`e5ab399074e22e386fb7bef61b29073312b195a3c4a8b0aee8ea343b8caa6479`.
Final-source all-target Tippy for TUI and record contracts passes with zero
owned, spanning, failing or build-script diagnostics; two upstream diagnostics
are set aside. Both library and test profiles are present for each selected
package, with 1360 canonical artifacts observed. Native compiler and
controller exits are zero. Its thirteen-file record hash is
`257496993389048bfced463b72525297eb40f2ba2a66e7ab665f7f7b3ef9969b`.
Independent review verified every indexed regular file, exact method identities,
source attribution, native statuses and lint profiles. The final three archives
contain 44 regular files; none needs an external symlink target to validate the
retained evidence. Earlier failures remain failed.
Fresh main advanced to `71b2e148377ae4af0e04a4c2220960d59564d889` with
separately reviewed Autopilot tests and fixture seams, plus their review record.
Composition `5f5c932610f3afbafdfc15e254f5f6cec995125b` imports those changes
without production, dependency or parser overlap. Both owned parser and contract
files are byte-identical to checked source `02227aded7`. The runtime and lint
observations retain that source attribution; this composition and documentation
are not a new whole-tree qualification. No additional runtime permutation was
justified by the disjoint test-only import.

All tests use synthetic codec metadata. Library runs use the explicit unverified
test lane through source-selected Trust 15698 and the maintained native context
and receipt machinery. Raw receipts stay private; archives index literal links
without retaining external executable targets. These observations supply no
application certificate or installed validation. The prior certified application
gate's unaudited procedural-macro boundary is unchanged and was not retried;
delivery remains deferred without a compiler or policy override.

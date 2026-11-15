<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Ambiguous persisted clocks retain their waits — 2026-11-15

Later hostile review found a validation gap in this raw-scanner approach. The
[original-parser review](recovery-clock-parser-2026-11-15.md) records its actual
regressions and replacement. The source-bound results below remain historical.

Decision: validate raw clock members before any JSON Value normalization, keep
known waits when their clocks are unusable, and preserve the numeric behavior of
non-clock fields in each supported dependency graph.

The previous optional readers first decoded Recovery and InFlightRetry as Value.
That representation collapsed duplicate clock members before RecoveryClock's
strict typed decoder could see them. At `cec73952c2618589ab0e9910db6ce2d995a045f6`,
17 existing record methods passed and both new methods failed. Twelve nested
clock-field mutants and four repeated parent-clock mutants each exercised the
actual serde Record decoder and regular-file record::read: 24 and 8 reader
mismatches, respectively. Other libraries selected zero methods and supply no
runtime pass claim. Native unit/controller exits were 101, without ignores or
measured tests, and 1,764 methods were filtered. The 22-file record hash is
`17bd602c6d7c45a9e796d22a379487daef9ab7d95743498cc6ebfc703ae70eb1`.

ClockField now forwards the live JSON reader into a RawValue and strict
RecoveryClock decoding. ClockedFields handles the enclosing object before a Value
can collapse clock keys. A sticky repeated-clock flag clears only that clock;
other values keep their recursive last-value treatment. Existing wait validation,
detail limits, conversions and the wire schema remain in place. An unknown
parent shape receives the Value validation the former reader performed before
its optional wait is discarded. No timing, boot sampling or automatic action is
introduced by the codec.

The first full attempt at `385b9d976368f0552e1ffbf66e38f6c31f34b009` retained
1,774 passes, one failure and nine ignores. Both duplicate-clock regressions
passed. The sole failure was a new test's incorrect assumption that 1e400 must
always be rejected. The selected application dependency graph enables JSON
arbitrary precision. This failed attempt remains failed, with no Tippy phase;
its 26-file record hash is
`3586464b008b7e0f1243be1eacb6ccdbcf432b6f88572339ebd5eea74cf0d6e1`.
The corrected oracle follows the former Value reader's representability and
compares complete records against explicit expectations.

A separate ALab-owned record-contracts test package can select TUI as a library
without TUI's upstream Core development dependencies. It enables no arbitrary
precision feature. Standalone selection observes the default numeric graph;
selection with the application test libraries observes their unified graph.
An optional 0/1 runtime guard verifies the selected graph without changing it.
The four methods always check valid records, duplicate clocks, non-clock fields
and unknown shapes through both readers. The workspace's existing alab-* member
glob admits the package. Metadata added only its nine ALab-owned lockfile lines;
no upstream package version or entry was changed.

At `be5f17f166651d6247359bd761031e83b1b5f7c4`, actual default-graph artifacts
contained only default/raw_value/std JSON features and the zero graph guard
passed. Three contracts passed; unknown shapes failed with eight mismatches:
two overflow-bearing arrays in each wait field, through both readers. The
16-file record hash is
`74e407d3c139eb92bacd33bcfcac3cb8bc8749d6f28b4a7cf06d7175a5d19ad3`.
After validating Value inside the shape-error branch, all four contracts passed
at `631c47eb23bb25fa671356a4a70aebdb31a17e7c`, without ignores, measured or
filtered methods. Its 16-file record hash is
`2267567eb816a10b16d65278c8df05b1325699b306b5780ca8a9cdba9f829e21`.

At `84a50cfebbe466cca4500785f3710f7097b37578`, the final full application-graph
run passed 1,801 methods: TUI 390, Core 344, Autopilot 1,063 and record contracts
four. There were zero failures, nine ignores, no measured methods and zero
filters. All seven required regression and contract identities passed; the
runtime graph guard observed arbitrary precision. Native unit and controller
exits were zero. The 26-entry archive indexes retained regular files and literal
symlink metadata; post-runtime executable hashes identify observed images but
do not retain them or supply a during-run process sample. Its record hash is
`bc66217f2533e042408b841ea9a93709fa2a3309faa72c12dc0ddb18b258e18d`.
The unchanged widest `alab/check.sh` also ran on that clean source under the
schema-5 observer. Full all-target Tippy completed with zero ALab diagnostics
and 41 upstream diagnostics set aside. The certified CLI phase waited for its
original 16 GiB native admission, then source-selected Trust 15698 refused the
unaudited in-process procedural-macro graph before proof measurement. Gate and
controller exited two; qualified_source is null. Missing proof transport follows
that refusal and provides no application proof count. The later application
runtime and ALab/hooked-library unit gate phases were not reached. Before/after
source observations were identical and clean. Its eleven-file archive includes
the observer, log and four certification reports, with record hash
`b341eeae30e5072aa47c60812cd79e3e66410d94bc23b77d2b5a3c8c72c6f8c8`;
the gate-log hash is
`d671db285be45c2770594c8c925ed248323aa24506858cb9b6d20b05d89751d1`.
Separate successful library tests do not change that failed certification result.
Independent review verified the final records, all retained regular-file hashes,
literal symlink metadata, exact method outcomes and the gate refusal. Before
publication, main advanced to `945cd2ab37d44ba46c0aa9d156a14726fff64ae1`.
The clean composition `cdc58dfec13ad4da9596ffdff8d74af28ab0128e` imports its
separately reviewed goal-completion tests, fixture observers and development
thread-store dependency. All owned codec and contract source bytes match the
checked `84a50cf` tree. No production overlap or conflict required another
runtime attempt; the imported tests keep their own documented native results.
The full-runtime and widest-gate observations above remain attributed to
`84a50cf`, not to an unexecuted composition or this documentation commit.

Historical archives retain their original indexes. The 22-entry RED index has
20 matching regular files and two helper symlinks; the 26-entry initial full
attempt has 22 matching regular files and four helper symlinks. Those six old
entries hashed external executable targets that later builds replaced. They
do not retain historical executable bytes. A separate accounting record binds
the original indexes and result records, verifies every regular file, and
records the literal links without opening their targets. Both 16-file default
numeric-graph archives contain only regular files and match throughout. New
archives index symlinks as link metadata and hash only retained regular files.
The separate accounting record has SHA-256
`0a9b3dc93b2001dd9f6328b76b84e84356193192e050a2765e68ca19c2fbc4b9`.

All anchors here are synthetic codec metadata. These tests do not measure a
native boot, shorten a real provider wait, or demonstrate an application crash
and relaunch. Raw execution receipts stay private. The original backend HTTP400
cause remains unknown. Application certification keeps its existing live-proof
requirement; runtime or lint observations cannot substitute for it.

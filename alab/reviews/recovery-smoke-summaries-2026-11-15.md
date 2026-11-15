<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Incomplete recovery-smoke evidence — 2026-11-15

Decision: establish the overall report immediately after admitting a fresh
output directory. Binary selection and initial hashing, case execution and
case-result persistence now share a protected reporting boundary. Failed
identification never runs a case and supplies no invented binary hash.
The active case remains named until its result is saved; a setup failure's
original case error survives a subsequent case-record write failure.

Ordinary case exceptions still become failed cases and allow later cases when
their evidence can be written. Failure to persist a case record stops the suite
and retains the partial overall report. Existing final binary identity checks
remain intact. KeyboardInterrupt and SystemExit retain the same exception
object and exit semantics, including SystemExit(0), after attempting the partial
summary. A finally block preserves that interruption even if summary writing or
printing also fails; the secondary output error remains in the exception chain.
No report can be guaranteed when its destination cannot be written, or when
the process is killed without unwinding.

RED `2fc547e50806aeed1f8ff9d13ea186f799fdfc18` ran fourteen tests: eleven
controls passed, two whole-test assertions and three interruption subtests
failed only because the overall summary did not exist. Unit exit was one;
the original interruption object checks had already passed. Record SHA-256:
`2cedf596fccbb61fcc233cbbb698cc995e343ec93c3300990cb9517f78236b84`.

Candidate `cc75ce9b7e49dfa053b97fc5915ba6cef0c56f78` remained nonqualifying:
one whole-test and six subtest assertions compared macOS's `/var` fixture paths
with resolved `/private/var` paths. The actual summaries and original exception
identity checks passed first. Only the four new fixture roots were subsequently
canonicalized; production code was unchanged by that correction.

At `2697103b388cbc7f5e14dad9c40a225b49bf8d79`, all fifteen tests passed in
0.028 seconds, exit zero, without errors or skips. Controls cover initial
identity refusal, original setup-error retention, interruption after an earlier
completed case, and summary write/print failures under all three interruption
variants. Existing malformed-history and retry-timing controls still pass.
Record SHA-256:
`96c01ee060af526f2b678adf49f052dc2d0117da5fc87fd9982380d9cb6b06fd`.
Nine indexed archive files across these three sources were independently
verified. Archives are `.git/alab-validation/<source>/request-recovery-summary`,
with `request-recovery-summary-red` for the first source.

The separate listener-startup repair closes the bound server if constructing or
starting its serving thread fails, before ownership reaches the caller. It
preserves the original exception even if close fails, and avoids shutdown/join
on a loop that may never have started. RED
`124671f3b207257111c072ec7ed367f1b71dba71` had three passing controls and
twelve failed close-count subtests. At
`736e2459d647b075334dcd7cdc1d7a4af6cee0f2`, all five methods passed, with no
errors or skips. Those tests use listener/thread mocks; fourteen indexed RED and
GREEN files were verified independently by the integrating agent.

A real-listener probe at `7d60aa2eb8716bf9e556ead30db8da01f05e1f28` failed
only because the bound socket remained open after an injected thread-construction
failure; its original exception check passed. The fixture's finally block closed
the listener. Later probe cases were not reached. Four files were indexed;
record SHA-256:
`b452b3d4214eb63685b723e8759a8dc4c1054928b00543f1662a3d1731d8856e`.

Combined source `dc933f6078421a26b6786e9b6148c86fabab468d` passed all twenty
Python tests: fifteen summary/oracle tests in 0.027 seconds and five shared
fixture tests in 0.015 seconds, both exits zero without skips. The same bounded
loopback probe then passed all three checks: a real listener closed after each
injected construction/start failure, and an actual normal HTTP SSE request
completed before its serving worker joined and listener closed. Eight files were
indexed; record SHA-256:
`796acd397bb6d1edfa44e7a8e8c2d5802f1d9f902dc074354ecffd81c6eb3cc2`.
Archive: `.git/alab-validation/dc933f6078421a26b6786e9b6148c86fabab468d/recovery-smoke-combined`.
The failed-start probes use mocked thread startup with real sockets; they do not
prove retirement after an indeterminate real thread start.

All commands use managed Python 3.14 `-I -B`. These tests execute no Codex binary
and establish no native application qualification. The original backend rejection
cause remains unknown; certified
packaging, installed recovery validation and installation retain their recorded
compiler prerequisite. No running agent or shipping gate was changed.

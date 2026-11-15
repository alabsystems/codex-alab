<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Native processes retain persisted provider waits — 2026-11-15

Decision: exercise the previously excluded process boundary with actual library
test processes and native timers, while leaving application certification and
retry authority to their existing gates.

Existing recovery tests already read the native boot clock and import waits with
supplied death/lease inputs. Another sampling-only or codec-only test would not
close the remaining boundary. The new TUI fixture uses a real writer child,
abrupt process exit, and three fresh reader children. It is enabled on macOS and
Linux; the observations here were made on macOS. It changes no production
code, dependency, compiler pin, unsafe operation or upstream hook.

The writer uses its actual PID, kernel process identity and sleep-inclusive boot
sample, then the production Shared/Writer path persists Recovery and InFlightRetry.
Independent controls save kernel deadlines at ten and twelve seconds from that
sample. A private materialized rollout permits the ordinary resume arguments;
the fixture explicitly checks those arguments before proceeding. Thread, rollout
contents and failure description remain synthetic.

The writer flushes its ready marker and exits 86 without Rust Drop or a writer
ending. The parent requires that exact status and marker, both pipe EOFs, a Busy
record without an orderly exit, and an actually dead kernel identity. Every
reader has a different live native birth and an unobserved Shared before
Writer.prepare selects the exact dead predecessor. The reader must retain both
wait facts and original bounds. A real timestamp gate precedes the reader;
the fixture invents no newer birth time.

For matching clocks, bracketed Instant/native samples bound reconstruction from
the independently saved kernel deadlines. The upper bound rejects restarting
the entire wait. Both timers must initially be Pending on an unpaused runtime;
their actual wake samples must reach their respective kernel deadlines. Missing
clocks and syntactically valid wrong-boot clocks preserve the same wait facts
while supplying no usable clock. Autopilot remains Off, with no owed act,
launch witness, probe or drive lease.

The maintained child collector bounds source/readers at 10/30/15 seconds plus
its existing five-second cleanup. The source uses an intentional-exit gate;
each reader requires OS success, exactly one passed method, a marker and both
EOFs. After every child is reaped, the parent drops its runtime and closes its
private directory before emitting one JSON observation under eight KiB. It
contains only verified native identities, deadlines and reader outcomes; child
harness output, raw records, environment and paths are not relayed.

At `16c3fc59dc83c18496ae2bd16f4a6adc680965a4`, the one exact parent method
passed, with 375 other TUI methods filtered and no failures/ignores/measured
methods. Its fifteen-file record hash is
`999e05321546a6e492a4799bf96d15ab7ed067240341341315b9d03ddd50dc5f`.
Tippy compiled that source but classification rejected one owned redundant clone.
That failed lint observation has twelve files and record hash
`f079b2ff326b436c5ea3b8a1575f3396e0799ba09be42ef569d71c0df7bf16b3`.

At `104a0ca98fc949569108baba142262056b464587`, moving the consumed fields
removes the copies. The exact native parent passes again, with the same 375
filtered methods and zero failures/ignores/measured methods; both exits are zero.
Its observation records three distinct reader births and actual matching wakes
3,648,458 and 3,755,167 nanoseconds past the original ten/twelve-second kernel
bounds. These are observed correctness margins, not latency benchmarks. Its
fifteen-file record hash is
`0d2b4cf950d7e3d2827c0f6d7bc66d1c84b0a07a4bccb887f77cf316c33c999c`.
Scoped all-target TUI Tippy has zero owned/spanning/failing/script diagnostics,
two upstream diagnostics set aside, and both library/test profiles. The
thirteen-file record hash is
`509b4ceb9ad099c2a3d4941c675694c0cc3a6acce0b25e0fb64cd34f192863cd`.

Independent review verified the exact source104a observations, all 28 runtime
and lint archive files, the three native births and both real wake samples.

Main's native diagnostic producer/context changes at
`c769ed91b987f6769d50dbe767b6a031272a5f5a` affect this fixture's execution
machinery. After composition `c5322022b41660536c3dbe818383bfb5014a7e15`, the
unchanged fixture passes through the new producer: one method passed, 375
filtered, zero failures/ignores/measured methods, both exits zero. Three actual
reader births and both refusal controls remain verified; actual wake samples
are 3,614,292 and 2,669,292 nanoseconds past their original kernel bounds. The
fifteen-file record hash is
`6be7b4692d9b95028d26750d14b6148d14ab8ecb3ef91c899c89434b8af5b108`.
Current-source all-target TUI Tippy has zero owned/spanning/failing/script
findings, two upstream diagnostics set aside, 1,358 canonical artifacts and
both library/test profiles. The thirteen-file record hash is
`c65a8219a16aa118b76acd1cea0ed45b2d7b6b8ad0812a37d85395e6d789bee3`.
Independent review verified all 28 current-source runtime/lint archive files,
the exact observation, all three births and timer samples. Prepared diagnostic
leaves, runtime receipt identities and collected diagnostics agree for the
actual selected library and round; the new producer interface was exercised.
Own fixture bytes remain those checked at source104a. These source-bound
observations are not a new qualification of later documentation or composition.

Execution uses the source-selected Trust 15698 explicit unverified library-test
lane, actual Cargo context/receipts and the admitted native cache. Image hashes
identify post-runtime library images; their executable bytes are not archived
and no during-run PID image sample is claimed. The test proves process custody,
record import and live timer bounds, not a real Codex application crash, accepted
external actor, provider retry, physical host sleep or wall-clock step. The
original backend400 cause remains unknown; the unchanged procedural-macro
certification boundary still prevents application delivery.

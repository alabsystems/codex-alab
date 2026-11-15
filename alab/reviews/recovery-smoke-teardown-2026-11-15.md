<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve recovery-smoke errors through teardown — 2026-11-15

Decision: retain the case's primary outcome while collecting secondary cleanup
diagnostics. The old sequential finally could replace a body interruption with
an ordinary cleanup OSError; main then continued through later cases and lost
the interrupted marker. An early cleanup error also skipped later cleanup and
request evidence. These are smoke-controller defects, not observations of a
Codex application failure.

Case cleanup now attempts shutdown, joins only after successful shutdown, and
independently attempts listener closure and request persistence. Failed shutdown
records a skipped join; it cannot introduce a new unbounded join. A returned
case result becomes failed on cleanup errors and retains its verifier error,
request count and process return code. Primary exceptions are captured explicitly,
so an outer caller's already handled exception cannot become this case's error.

The first pending control exception wins over ordinary failures. An existing
body control is preserved; a new cleanup control supersedes an ordinary body
error and retains that body's diagnostic. Standard exception notes carry every
secondary cleanup diagnostic. A failed verifier's diagnostic also survives when
a cleanup cancellation abandons its pending result. Main serializes errorNotes
for ordinary case errors and interrupted summaries without changing the original
control object or exit semantics. This includes SystemExit(0) and preserves
other non-Exception controls rather than swallowing them.

This deliberately makes this smoke require Python 3.11 or newer. Standard notes
preserve identity without inventing an exception metadata protocol or altering
causal chains. No prior Python 3.9 promise for this smoke was found; the version
minimum is a new scoped decision, documented in the CLI and README. The CLI
refuses older interpreters before output-directory ownership or case work.

Committed RED `1c52e34f3aefb2d33b1aab9ff21250a91eed9621` ran seven methods:
the normal-success control passed, while fifty subcases and one whole-test
assertion failed. There were no framework errors or skips; actual exit was one.
The 117-file archive preserves fake case evidence; record SHA-256:
`78469013fb05f1e6c605cab0e2490476a45b10bb420d496ef8203328dabc5a40`.

Review found the additional failed-verifier/cleanup-cancellation composition.
RED `3300c2184411cb87c314551d115ca305346548d1` passed ten methods and failed
six subcases only at the missing verifier-note check, after confirming the
original cancellation identity. The 160-file archive has record SHA-256:
`def92bdce26afed25c30fb86e20b6c82a10cb05a57588fbf2534420b277a015c`.

An intermediate controller at `85d2323a891c4af24b26e32390f92e9803193580`
passed thirty tests and three real case checks, but remained nonqualifying:
its old-Python probe used -I, which removed sibling imports before version
admission. Its failure is retained. The corrected CLI probe uses -E -s -B,
preserving normal script imports while ignoring environment/user-site settings.

Final source `12a7dacd45c47091b6ce16ba38eef742ef3b9c9b` passed 31 tests:
eleven cleanup methods, fifteen existing recovery methods and five shared
fixture methods. All unit and controller exits were zero, with no failures,
errors or skips. Four actual local HTTP case checks passed: normal completion,
cleanup failure, body interruption plus cleanup failure, and verifier failure
plus cleanup cancellation. Each preserves both request records and closes its
real listener. The actual Python 3.9.6 CLI exited two with the clear minimum
version message and created no output directory. All 181 final indexed files
were retained; record SHA-256:
`b8530be06c60572090adccfc5ab61d5f84b4a7ebfe3a6fc966bb0531c2abf66c`.
Archive: `.git/alab-validation/12a7dacd45c47091b6ce16ba38eef742ef3b9c9b/request-recovery-teardown-final`.

Unit tests and the real-case controller use managed Python 3.14 -I -B with
explicit fixture imports. The real cases mock command execution and inject
errors after an actual successful server shutdown. Code skips failure-mode joins;
the fixture owner subsequently joins its workers. This proves no retirement
under genuinely failed shutdown, real Codex recovery, external provider behavior
or application certification. The original backend rejection cause remains
unknown. Certified packaging and installed validation retain their recorded
compiler prerequisite; no package, running agent or application gate changed.

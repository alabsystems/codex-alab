<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Weekly-reset report executable identity — 2026-11-16

Decision: a successful standalone weekly-reset report must finish with the same
executable digest it records before the cases. Re-read it before setting
`passed` and `fullSuite`; changed, removed or unreadable bytes leave a failed
summary with the completed cases retained. Existing case errors and original
cancellations retain their own outcomes.

The hash reader opens with `O_NOFOLLOW | O_NONBLOCK`, requires the held
descriptor to be regular, hashes in bounded chunks and refuses changes to its
identity and mutation fields during the read. Its descriptor closes on every
outcome. An independent review identified that a pathname-only final hash could
otherwise wait for a writer after the case replaces the executable with a FIFO.
The bounded subprocess control now refuses that replacement promptly, preserving
the successful case evidence in a failed overall report.

Read progress is also bounded by the descriptor's initial size plus one byte,
with no arbitrary executable-size ceiling. An actual byte count different from
that initial size refuses truncation and growth before the final stat check.
A real-file control appends after every read: the earlier EOF-driven reader
keeps following those appends, while the repaired reader refuses after exactly
the original size plus one byte. The test's three-call guard bounds the old
negative control independently, which fails one assertion without hanging.

At source `e8320f61c4`, `main()` recorded `binarySha256` once and never checked
it again. A self-modifying inert Python stand-in demonstrated the false pass
through the actual main, loopback fixture and unchanged command guardians: it
completed the authenticated preflight case, changed its own file before exit,
and left `passed: true` under its original digest. The repaired main rejects
the same run after its case passes; an unchanged stand-in still passes.

Thirteen new test methods cover unchanged full coverage, changed and removed
bytes, a refused final read, an existing case failure, cancellation after file
removal, FIFO replacement, regular streaming digests, symlinks and directories,
mutation and continuous appends while reading, truncation, and closed
descriptors after read failure or cancellation. Three initial assertions fail
against the old main. All 44 weekly-reset fixture methods pass on managed
Python 3.14 with `-I -B` and macOS
Python 3.9 with `-B`; `git diff --check` passes.

This checks byte equality at the beginning and end. It does not prove source,
certification or which image the operating system executed. The maintained
runtime gate already makes its own final executable check; this closes the
standalone report gap without changing that gate or package statement policy.
No compiler, native Codex binary or real provider account was exercised.

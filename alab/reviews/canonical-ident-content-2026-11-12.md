<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Compare ident source against exact allowed byte forms

## Finding and decision

The source observers at `8fd4bc595c` delegated built-in `ident` normalization
to Git. That check can erase arbitrary content between `$Id:` and the closing
`$` before comparing a working file with its committed blob. An inert shell
fixture used that space for extra commands while the fresh-index comparison
reported clean. The behavior follows Git's documented
[ident check-in conversion](https://git-scm.com/docs/gitattributes#_ident),
but normalized equality alone cannot authorize execution of those bytes.

Add one shared, Python-3.9-compatible helper for qualification, pristine and
package-source callers. It selects effective ident-attributed regular files
from the exact committed tree, using the caller's fresh-index environment and
the same worktree, info and global attribute context as its status operation.
It accepts either the exact raw committed blob or Git's exact canonical
checkout representation. Raw form remains valid for newly committed `$Id$`
files that have not been checked out again. Canonical form supports normal
expanded identifiers, line endings and working-tree encoding without
reimplementing Git's conversion grammar.

Canonical bytes come from
[`git cat-file --filters --path=<path> <blob>`](https://git-scm.com/docs/git-cat-file#Documentation/git-cat-file.txt---filters).
The caller must supply complete external-filter suppression, including
smudge-only drivers. No external conversion program may run while producing
the expected form. The helper additionally disables fsmonitor, lazy fetching,
replacement objects and optional Git writes. It changes neither source nor
the actual or comparison index.

Open each source once as a no-follow regular-file descriptor. The raw check
hashes that descriptor with `hash-object --no-filters`; if needed, rewind the
same descriptor and compare canonical output in 64 KiB chunks. There is no
small-file cutoff or full canonical blob capture. Each Git subprocess has a
60-second lifetime bound. File and parent identities are checked before and
after observation; nonregular leaves and symlinked parent components refuse.

These are two deliberately exact accepted representations, not every form
that Git's lossy reverse conversion considers equivalent. For example, an
expanded identifier with a different hash or a mixture of noncanonical forms
does not acquire execution authority merely because Git strips it. Existing
callers retain their other source, staged-index and recursive-child checks.

## Validation and limits

Real-Git tests cover raw and canonical forms with LF and CRLF; forged
identifier fields including NUL bytes; multiple identifiers; Unicode, colon
and newline paths; non-UTF8 file content; smudge-only callback suppression;
nonregular leaves and symlinked parent directories; and files larger than
one MiB with a forged identifier beyond that boundary. They compare both
real and temporary index bytes before and after the helper.

This macOS filesystem refuses a raw non-UTF8 filename with `EILSEQ`; that
specific filename subcase is explicitly skipped, while Unicode/newline path
and non-UTF8 content cases run. The test remains enabled on filesystems that
can represent the filename. Independent implementation review found no
concrete blocker. No native build, provider call or live source/cache mutation
is part of this helper validation.

The focused suite completed six methods under the modern interpreter in
4.737 seconds and under system Python 3.9 in 4.370 seconds. Both runs skipped
that one unsupported filename subcase. `git diff --check` passed.
The retained Python 3.9 log, result summaries and source hashes are under
`$HOME/.aterm/.ubd/ident-content-validation-20261112-mbdw33nu.noindex`.

The earlier child-index audit found no false admission: dirty sparse,
split-index, inherited-index and stale-stat cases all refused while preserving
the real child index. Its evidence is retained under
`$HOME/.aterm/.ubd/gitlink-index-audit-1tb3u8zb.noindex`.
Canonical-conversion research is retained under
`$HOME/.aterm/.ubd/ident-canonical-probe-e_ni6lhb.noindex` and
`/private/var/folders/bn/vl51wh2n77v_7lgll0j3l2tm0000gn/T/alab-ident-byte-audit-x12p78l2.noindex`.

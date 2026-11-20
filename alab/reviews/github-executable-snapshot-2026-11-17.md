<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Refuse non-regular Agh executables before capability probing

The requested landing inventory found no session-owned work left to publish.
A fresh admitted Agh read observed main afd870cf5e9e7ba280d5cbd224e9ac34196b3f83.
Both owned worktrees were clean and detached at already-contained commit
74445ab051d89319db3cae50276d2295da4317d3. The sole local branch was another
actor's main, no stash ref existed, and the retained pristine baseline was also
contained. No foreign branch, worktree, stash or working file was changed.
The inventory and decision are recorded in codex-owned-landing-audit.json.

The next independently identified gap is executable acquisition. Agh accepted
an X_OK path, then used Path.read_bytes before its subprocess timeout existed.
An executable FIFO without a writer can therefore block construction before
capability probing. The existing shared descriptor reader already provides
nonblocking open, regular-file admission, bounded streaming reads and stable
before/after descriptor metadata.

Decision: load that adjacent reader only when constructing a native client, after
the existing missing/non-executable refusal. Bind the helper's absolute location
when the adapter loads, so a later cwd change cannot redirect lazy loading.
Resolve installed executable symlinks as before; derive both the digest and
initial five-field identity from the same descriptor snapshot. Preserve the
existing per-call identity guard, capability probe and Agh-only transport.
This closes special-file acquisition blocking; it is not a universal filesystem
wall-clock deadline, pathname custody or proof of which image executed.

Only the updater's staged native-client fixture needs to copy file_hash.py.
Other partial-copy fixtures are local-only or stop before native transport and
retain their existing dependency boundary. Production callers use complete
source checkouts, and the version ledger fixture already includes the helper.
The shared reader's implementation remains unchanged.

The new branch starts at pinned full main
afd870cf5e9e7ba280d5cbd224e9ac34196b3f83. Tests-only commit
b20abccee44f3b9190cf21a247b7450154cea13e pins four methods before production.
The baseline completed two passing controls and two assertion failures, with
zero errors or skips. The real no-writer FIFO child timed out after four seconds,
recording before-constructor with no help or process call; subprocess.run killed
and reaped that direct child before the assertion failure. The directory case
separately returned IsADirectoryError instead of the required typed Refused;
it is not another hang. Regular symlink/digest/identity/cwd binding and later
mutation guard, plus missing/non-executable selection controls, already passed.
Original process, guardian and collector statuses1 are retained, source status0,
with identical source/tool bookends and exact named result/raw output agreement.

The independently approved fix is committed at
f78173c6eff7dc0f56bea52fb3b2127886ee2372. A real installed-Agh read using the new
constructor completed successfully before merging observed main
f423070576b5d6163ddbcf114777edadef136f9a by its fixed OID. Merged source
8a3e84a0acf2776b85b08dcd5df88ee241ab3a1b retains that disjoint incoming work.
The complete GitHub transport selection passed all 80 methods, and the complete
local updater selection passed all 59 methods. All 139 distinct AST/named result
identities finished, with zero failures, errors or skips. Each original process,
guardian, collector and source status was zero; raw output exactly matched its
named result record and source/tool bookends were identical. The four baseline
methods are included in those 139, not four additional distinct tests. The
updater run exercises the genuine staged-helper dependency; all old transport,
publication, snapshot and cleanup controls remain included. No additional test
execution was warranted after these passing selections and unchanged inputs.

Production github.py SHA-256:
2cd71137b899f4700fe7fb21f4b14e6841dcc55d16861a41f872b21add5e2dc0.
Frozen constructor module SHA-256:
7b1f6ffd835d2b391d21dc8557e840632cc93f0db4c5ba0b37f8b1b9333eff66.
Baseline summary SHA-256:
578c3e5aa0c028bef928e1a977d6680b5bea27bfd0b24ed7ca34cc9defef6525.
Passing summary SHA-256:
ef69095818a666fc3065c5655d42e68fe00717c09830f066a4875281baa9fab6.

The finite saved-only evidence packet is
codex-github-executable-evidence-azqzvhed.noindex: 25 payload files,
1,235,272 bytes, index SHA-256
3269c275bf09f00932e807fab715e52d33279f831ddd890a1561a9607ce8e20e.
It retains both original outcomes, exact raw/named records, typed completion
receipts, original process handles, plans and unchanged source/tool bookends.
Payloads are read-only with no binary, full source-copy or live cache dependency.
Original nonzero baseline statuses and the two distinct failure reasons remain
retained. Root independently rehashed the complete payload set, sizes and modes.

Independent source and fixture review approved the descriptor policy, minimal
copy dependency and distinction between the actual FIFO block and the directory
error-type normalization. No application build, certification, package promotion,
installed change or new native runtime authority is claimed. Existing application
certification prerequisites remain unchanged; these local observations cannot
qualify those unavailable cases.

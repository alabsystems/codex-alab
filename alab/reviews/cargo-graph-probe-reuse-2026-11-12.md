# One Cargo graph observation per landing probe

Land repeatedly held an unchanged nested rusty_v8 Cargo checkout on `.cargo-ok`
and inherited Chromium history. Its HEAD still matched its parent's gitlink, both
Cargo databases retained their exact pins, and its sole initial-reset reflog and
empty marker were unchanged. These are dependency copies, not work to push.

The preserved session scope contains five members of one 20-repository graph.
Previously every candidate independently proved that entire graph under one
60-second deadline. The unchanged-source full-scope diagnostic passed narrowly
in 55.935 seconds; this was not a timing reproduction. A deterministic five-member
read-budget regression did reproduce the false hold on unchanged production code
(`d505f702a0`, actual runtime exit 101).

Reuse only a completed positive graph observation within one `session_reads`
invocation, keyed by canonical anchor, exact effective ownership names and proven
candidate membership. Bound all retained observations by the existing state-file
allowance. Preserve the deadline, explicit whole-repository ownership, complete
provenance checks and fresh observations on the next probe. Classify before
ordinary Git counts to avoid expensive duplicate history reads and a mixture of
old proof and later generic counts. Git reads are sequential observations; this
does not claim an atomic snapshot across concurrent external writes.

Moving classification earlier also exposes quiet missing-provenance queries.
Normalize the fixed `--no-replace-objects` prefix when recognizing Git's existing
quiet absence answers; retain strict exit and stderr handling for real failures.

Validation is scoped to this ALab-only change: affected all-target Tippy through
the existing diagnostic classifier, the complete Autopilot library suite, ownership
and fresh-probe regressions, and the exact saved live scope. This does not transfer
the earlier F31 full-canonical qualification to a new source. Package the checked
repair source only after it is an ancestor of remote main, then verify actual
binary adoption with the original session scope intact. Never mutate or publish
the third-party dependency checkout to clear a landing hold.

Actual scoped results on repair source `712088d96158de6c24ad41dbc23a2d66a4c2d322`:
894 library tests passed, zero failed, four host diagnostics ignored. All-target
Tippy completed with zero owned, spanning, failing or build-script findings in
the existing classifier. The explicit ignored saved-scope diagnostic separately
ran exactly once and returned `Read(None)` in 9.971 seconds, versus the unchanged
source's 55.935 seconds. The deterministic read-budget case that failed before
the fix passed in the full suite, together with the ownership, fresh-probe and
quiet-absence/refusal cases. These are unverified Trust execution results, not a
formal proof or a new full-canonical qualification.

Before publication, private commit subjects were corrected to the repository's
`alab(<topic>): ...` convention. Every rewritten commit retains its exact tree,
author, body and mapped merge parents; published ancestry is unchanged. The final
package leaf is `963164b213d9835ffb77dbf97f2c8c3fdaf4b714`, with the same tree
`ba392c1ff31f83eca76847dfdf885ef8a11232db` as the earlier repair source.
Fresh checks on that final leaf again passed all 894 library tests and affected
all-target lint. Its saved-scope diagnostic separately returned `Read(None)` in
16.124 seconds. The different timings are individual runs on a shared machine,
not a controlled benchmark.

The corrected merge `d4e7f4648199e1052f0a39150856b529080dd751` has exactly the
same complete tree, `609abfdba47b3178f74ee6313ccc65604d6c419b`, as the checked
merge `d0847ce5f28508be666fca10745b70a020db6094`. The latter actually passed
affected all-target lint and all five synthetic Cargo-copy tests after composing
with main `f376ccc57fc6dc269527522e1b176c58a07de992`. Those receipts retain
their original commit identity. Tree equality, clean source and native project
authentication justify publication without repeating identical merged runtime
checks for message changes. This final receipt changes documentation only.

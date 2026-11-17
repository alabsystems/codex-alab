# Distinguish owned-path deletion from an unreadable source tree

Decision: use checked literal tree listings to determine whether an owned path
exists in HEAD and MERGE_HEAD. A failed cat-file existence probe cannot distinguish
absence from an object-read error; its former else branch removed files. Inspect
both source trees for the path before mutating the archive or resolving that path.
Read errors terminate even in the boolean context used by the resolver.

Evidence: four injected failure cases failed before the change, covering either
source side and empty or partial failed output. Both now stop with a read-error
diagnosis and no archive reads, staging, checkout or deletion. All four known
presence/deletion combinations retain their existing resolution. An additional
real-Git test confirms ordinary and wildcard-shaped names are matched literally,
while a wildcard string cannot borrow another file's presence.

Validation: 74 sync tests passed, followed by the final three-test owned-path suite
including the subsequently added real-Git case (75 distinct sync tests overall).
All 30 delta tests, Bash syntax and diff whitespace checks passed. No application
build or native runtime qualification is claimed.

Publication composition: merged remote 8fc6d97b52, which changes only the existing
HTTPS replay review record. The tested tooling source is unchanged by that merge.

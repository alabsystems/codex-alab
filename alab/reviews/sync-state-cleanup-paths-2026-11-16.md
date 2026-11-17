# Resolve recovery paths before cleanup

Decision: collect and validate the complete absolute recovery-path set before
removing any file. Derive the final-message path from the single checked message
lookup. A read failure must stop cleanup even in boolean contexts; returning a
nonzero status alone would still permit partial deletion with errexit disabled.

Evidence: before the change, all 20 injected failure cases deleted files despite
an unsuccessful path lookup. Coverage includes all five state names, empty and
partial failed output, and direct versus boolean calls. An empty message lookup
also turned its derived final-message path into the unrelated relative .final
file. The fixed tests verify every original byte survives a failed lookup, and
successful cleanup removes precisely the recovery set while preserving unrelated
.final and partial files. Paths containing spaces are included in the fixtures.

Validation: all 70 sync tests and 30 delta tests passed, plus Bash syntax and diff
whitespace checks. No certified application build or runtime qualification is
claimed.

Publication composition: merged remote faafc8bcd5, whose additional changes are
intake JSON and its review record. No tooling source changed during composition;
all nine upstream intake tests passed on the combined tree. Preserve both histories.

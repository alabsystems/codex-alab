# Require successful unresolved-path reads

Decision: check unresolved-path reads before treating their output as a complete
conflict set. An unreadable Git index must stop the sync before marker checks,
commit, or the qualification gate. Reuse the validated set when writing resolver
state and the stop summary. Also check reads for owned-path resolution and the
conflict report instead of discarding process-substitution failures.

Evidence: the new finish regression failed before the change in both injected
cases. An empty failed read reached commit and gate; a partial failed read took
the conflict branch instead of reporting the read error. Both now stop with the
read-error diagnosis and reach neither marker checks nor commit nor gate.
Existing successful empty-conflict cases still pass.

Validation: Bash syntax and git diff whitespace checks passed. All 56 selected
sync tests and 30 delta tests passed. These are tooling checks; no application
build or runtime qualification is claimed.

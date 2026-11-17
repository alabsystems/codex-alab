# Pending sync state requires a successful Git read

Decision: merge-state reads distinguish Git's documented quiet missing-reference
status (1) from execution errors. Unlanded-commit reads must succeed before their
output determines whether the sync is idle. Both predicates terminate on unknown
state even when called in an if, negation, or OR-list; merely returning failure
would still authorize the idle branch and potentially discard recovery state.

Evidence: before the change, 36 injected failure cases reached an idle or pending
action instead of stopping. Coverage includes empty and partial output, statuses
2/128/137, and the three boolean contexts. After the change all stop with a read
error. Successful absence and presence classifications remain intact. A real
empty Git repository additionally confirms absent MERGE_HEAD remains ordinary
absence, while an unreadable origin/main..HEAD range stops instead of becoming idle.

Validation: 59 sync tests and 30 delta tests passed. The final focused suite of
four tests also passed, including the subsequently added real-repository case
(60 distinct sync tests covered overall). Bash syntax and diff whitespace checks
passed. No certified application build or runtime qualification is claimed.

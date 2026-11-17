# Preserve unavailable sync worktrees when history cannot be read

Decision: before removing a registered but missing sync worktree, require a
successful read of origin/main..alab-sync. Only a proven empty range authorizes
removal. Unknown history must preserve registration, refs, indexes and recovery
files, and must not offer advice based on an unproven unique-commit diagnosis.

Evidence: two real-repository regressions inject empty and partial failed history
reads while the unavailable worktree's branch holds unique commits. Before the
change the empty failure removed its registration, confirmed by the existing
full repository observation assertion. The partial failure falsely advised that
unique commits had been found. Both now stop with the read-error diagnosis and
leave observed state unchanged. The existing successful unique-commit and live
holder checks continue to pass.

Validation: all 62 sync tests and 30 delta tests passed; Bash syntax and diff
whitespace checks passed. This is a tooling correction, with no application
build or runtime qualification claim.

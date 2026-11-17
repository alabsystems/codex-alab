# Resolve merge metadata before preparing the worktree

Decision: capture and check merge base, commit count, abbreviated target identity,
upstream summary and progress summary before cleaning/resetting the sync worktree
or rewriting recovery records. Reuse these complete values in the merge message
and progress output. An echo containing failed command substitution must not turn
that failure into a successful partial history record.

Evidence: ten before-failing cases cover each metadata read with empty or partial
failed output. Earlier reads failed only after worktree cleanup; nested echo reads
could continue into merging with incomplete metadata. All now stop with an explicit
read-error diagnosis, no cleanup/checkout/merge/finish calls and byte-identical
previous state records. The successful control retains the exact message format,
target record and merge sequence. Existing ancestry tests admit the new read-only
preflight while preserving their mutation-boundary assertions.

Validation: all 93 sync tests and 30 delta tests passed, plus Bash syntax and diff
whitespace checks. No certified application build or runtime qualification is
claimed.

Publication composition: merged remote 19ab3f8e52, whose additional change only
updates the existing HTTPS replay review. The tested tooling code is unchanged.

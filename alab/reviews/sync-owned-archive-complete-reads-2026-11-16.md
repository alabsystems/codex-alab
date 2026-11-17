# Replace owned-path archives only after complete upstream reads

Decision: stage upstream archive content beside its destination, then rename it
into place only after git show succeeds. Preserve existing regular-file modes,
reject directory destinations, and clean staging on copy, read or replacement
failure. Use a fixed temporary prefix so long archive basenames do not consume
the filesystem's per-name limit. Stage the archive in Git only after replacement.

Evidence: before the change a partial failed git show overwrote the previous
complete archive. The regression now verifies its original bytes and executable
mode survive, with no Git add or owned-file checkout and no temporary files left.
Additional injected mode-copy and replacement failures verify the same guarantees.
Successful resolution and all known presence/deletion combinations still pass.

Validation: all 77 sync tests and 30 delta tests passed, plus Bash syntax and diff
whitespace checks. No certified application build or native runtime qualification
is claimed. This guarantees complete per-file replacement; it does not claim a
transaction spanning every path in a sync.

# Check merge-reference reads before recovery decisions

Decision: require a successful MERGE_HEAD read before classifying an open merge,
and retain that resolved head for ancestry checks and conflict reporting. For a
committed merge, distinguish the documented quiet missing-parent status (1) from
Git execution errors. Unknown parent state must not become reset or abort advice.

Evidence: 18 injected cases failed before the change. Coverage crosses open and
committed merges, statuses 2/128/137, and empty, partial or apparently matching
output. A failed read that printed the expected target could reach commit and
the gate; other failures produced recovery advice without knowing the reference.
All now stop before ancestry, status checks, marker reads, commit and gate. A
known missing second parent retains the existing non-HEAD merge recovery path;
existing matching-reference paths still pass.

Validation: all 72 sync tests and 30 delta tests passed, plus Bash syntax and diff
whitespace checks. This is tooling validation, with no certified application build
or runtime qualification claim.

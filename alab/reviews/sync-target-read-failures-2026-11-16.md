# Distinguish unavailable target records from missing recovery state

Decision: check the target-path lookup, then read and validate the complete target
record before returning it. Only an actual missing-file result may enter the
existing lost-state recovery path. I/O errors, empty or malformed records and
dangling links stop as read errors, without abort advice. Use isolated Python's
file-error distinctions, as for the stop record; emit no target on failure.

Evidence: four failure cases failed before the change: empty or partial failed
path lookups, a directory instead of a readable target, and an empty record. The
fixed tests additionally reject newline-only, multiline, binary malformed and
dangling records. Successful complete reads and known missing-state diagnosis
retain their existing behavior.

Validation: 84 sync tests passed, followed by the final five-test target suite
including the added malformed/dangling cases (85 distinct sync tests overall).
All 30 delta tests, Bash syntax and diff whitespace checks passed. No certified
application build or native runtime qualification is claimed.

Publication composition: merged remote 2b895cac26 with qualification diagnostic
and snapshot-validation changes. The combined tree passed all 85 sync tests,
79 qualification tooling tests and 16 upstream-status tests. Preserve both
histories; these tests remain distinct from full application qualification.

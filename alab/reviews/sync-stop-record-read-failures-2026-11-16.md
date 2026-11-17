# Preserve stop decisions when their records cannot be read

Decision: distinguish an absent stop record from an unreadable, empty, dangling
or invalid one. Only absence authorizes unattended error recovery. Read the first
line with the existing isolated Python interpreter convention so missing-file
errors can be distinguished from other I/O errors; validate bytes before emitting
a code. Both startup and status check the read result, and status checks it before
printing recovery advice. Read each decision once rather than silently retrying it.

Evidence: real-repository status regressions failed before the change for an
unreadable record (a directory) and an unknown hold value: both were reported as
eligible for unattended resumption. They now fail without recovery advice and
without repository changes. Focused startup tests reject empty, unknown and
dangling records, preserve all six written stop codes, and resume when no record
exists. Binary malformed codes cannot become valid through shell substitution.

Validation: all 68 sync tests and 30 delta tests passed, plus Bash syntax and diff
whitespace checks. No certified application build or runtime qualification is
claimed. This change preserves existing explicit --continue and --abort behavior;
it changes unattended eligibility and status reporting when the record is unknown.

Publication composition: remote main advanced to 794ff49328 with independent
qualification-observer cleanup changes. Merged that history without rewriting;
the combined tree passed all 68 sync tests and 34 qualification cleanup tests.
The earlier 30 delta tests passed on the stop-record source. These selected
checks do not claim the full native qualification gate passed.

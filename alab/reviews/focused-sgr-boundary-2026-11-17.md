<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# SGR styling cannot create a passed-test record boundary

On source `fdbcce2890dbb95428ddc00c3c8d200d866f7e0b`, both maintained public
CLIs accepted a log whose only apparent passed-test name followed ordinary
visible text on the same line, separated by an SGR color sequence. The focused
result CLI and feature-evidence validator both returned 0 for that styled
prefix. Both correctly returned 1 for the uncolored equivalent. Healthy named
output and a cursor-terminated TUI frame also passed their controls. The owned
producer only formatted inert fixture logs; no Rust test execution or native
qualification was claimed.

`libtest_output` inserted a newline before any ANSI control immediately followed
by `test `, including SGR controls. Color styling therefore manufactured a line
boundary that the raw visible text did not contain. The resulting passed name
then satisfied exact-name checks and the feature consumer's declared scenario.

Decision: remove SGR styling first, then apply the existing boundary handling to
remaining controls. Styling cannot create a record boundary or hide a preceding
cursor command. The two linear passes retain colored names/statuses, the
uncolored prefix refusal and cursor-plus-color output. Merely suppressing SGR's
own newline insertion would lose the real preceding cursor boundary when
styling intervenes. Suite counts, exact selectors, command status, skip checks,
feature validation and the parser's existing scope remain unchanged.

Two maintained regression methods exercise the actual focused CLI and the
feature consumer's API/CLI. Each includes a plain prefix refusal, styled prefix
refusal, a valid colored line and a valid cursor-plus-color line. With those
same final test bytes on the original production code, managed Python 3.14.8
reported two assertion failures across two distinct methods, actual exit 1,
zero errors/skips. HEAD and all 240 ALab Python source hashes matched before and
after that baseline; its owned fixture scratch was empty afterward.

With the fix, five complete affected modules passed all 75 distinct methods:
focused results, feature evidence, feature gate, upstream gate and focused
capture. Actual exit 0 in 22.714 seconds, with zero failures, errors, skips,
expected failures or unexpected successes. The two new methods are included in
the 75. All 75 actual verbose OK records match the frozen unique selected IDs.
HEAD and all 240 ALab Python source hashes matched before and after the run;
its owned fixture scratch was empty afterward. The candidate was frozen with
the separate runtime-retention topic's two Python edits present in the source
bookends. The SGR regression test bytes were identical to the baseline's;
`focused_results.py` is this topic's only production change. An independent
subagent approved the source/test diff without executing further tests.
`git diff --check` passed.

Original public commands, raw logs, statuses, the local Git inventory fixture
and source bookends are retained under
`~/.codex/.ubd/focused-sgr-review-sd5l_rm2/`, alongside the maintained
`baseline.*` and `candidate.*` artifacts. Production SHA-256:
`e6acf154bff8840fb7a799de30b180338a8fc1b54f6b047794927a81cbd6e655`.
The two final test hashes are
`1dc6be410f8140158f2216eb26240fdb29f1ab00865b869a7ecae2f69eb39e58`
and `e621244c1e14f40eb6f1e9866ee464c313fc6b85259769a534a08a4c1e888718`.
Full-run log SHA-256:
`e33f42c5495e613fda3103aaf16d71e8d89dead67712953b527980a29fe32052`.
Source/test verification here does not replace the unavailable native gate,
Rust build or package publication.

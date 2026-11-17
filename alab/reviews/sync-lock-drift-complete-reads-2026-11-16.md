# Require complete lock package sets before drift decisions

Decision: replace the ambiguous upstream lock existence probe with a checked tree
listing. Capture and check upstream and merged package pipelines before comparison,
and check vendor identity reads and the final comparison explicitly. Process
substitutions now carry only complete validated package sets, including genuinely
empty sets. Unknown inputs must produce an error, not a pass or repair advice.

Evidence: eight injected failure cases failed before the change: upstream tree,
empty/partial upstream blob reads, either package parser, vendor identities,
comparison, and a missing merged lock. All now stop with a read-error diagnosis.
Controls preserve unchanged lock acceptance, genuinely absent upstream lock and
empty registry package sets, real drift repair advice, and matching vendor-row
exemptions. The production package parser is exercised through the test wrapper.

Validation: all 89 sync tests and 30 delta tests passed. The final four-test lock
suite also passed with expanded empty-set controls. Bash syntax and diff whitespace
checks passed. These tooling tests do not claim a certified application build or
native runtime qualification.

# Stage resolved locks only after a proven diff

Decision: distinguish git diff --quiet's clean (0), changed (1) and error results.
Only changed locks are staged and recorded as re-resolved. Unknown diff state must
stop before staging, writing a success record or checking package drift, including
when shell errexit is disabled by a boolean caller.

Evidence: six injected error cases failed before the change: statuses 2/128/137 in
direct and boolean calls. They staged the lock and recorded a successful resolution.
All now stop with the original diagnostic plus an explicit read-error diagnosis,
and preserve the prior auto-resolution log. Known clean and changed controls keep
their staging, log and drift-check behavior. Resolver execution is a recording stub
in these focused tests; the suite's existing contained resolver fixture also passed.

Validation: all 91 sync tests and 30 delta tests passed, plus Bash syntax and diff
whitespace checks. No certified application build or native runtime qualification
is claimed.

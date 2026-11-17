# Publish stop decisions without truncating existing records

Decision: route resolver stops, explicit continuation and no-push decisions through
one checked writer. Resolve the absolute record path, write a complete temporary
record beside it, then replace it. On lookup, write or replacement failure, retain
the previous decision and report an error instead of silently claiming a saved
resolver stop. Temporary records are removed on write or replacement failure.

Evidence: before the change all three injected failure cases failed the regression;
failed writes could destroy the previous decision and were ignored. Tests now
check the exact error for path, partial-write and replacement failures, unchanged
previous bytes, no leftover temporary files, and the original exit code plus full
new record after a successful write. Existing finish tests use the same writer.

Validation: all 79 sync tests and 30 delta tests passed, along with the strengthened
two-test writer suite, Bash syntax and diff whitespace checks. No certified
application build or native runtime qualification is claimed. Atomic replacement
preserves a prior decision; it does not guarantee a new stop can be persisted on
an unavailable filesystem.

Publication composition: merged remote 9bf2a99dcd with independent SSE recovery
code/tests and request-diagnostics documentation. Their runtime qualification is
separate. The final combined tree passed all 80 sync tests, including an added
control rejecting directory destinations before mv can treat them as containers.
The directory and its contents remain unchanged on that failure.

A publication race then required composing remote 9e1039ada0, which adds aterm
claim-transition changes and updates its feature evidence. The new combined tree
passed 80 sync tests and 27 feature-inventory tests. Incoming native execution
claims stay with their own source-bound review; these tooling checks do not
requalify that runtime behavior.

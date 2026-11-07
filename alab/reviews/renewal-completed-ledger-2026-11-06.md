# Renewal oracle requires completed native evidence

Review of the remaining renewal smoke oracles found that consumed_receipt
accepted its active prepare recipe in either native_prepare or
native_completed_prepare. The former is an in-flight plan, not completion.
A ledger with coordination and observation fields could therefore be accepted
when the completed recipe was absent or still named an older operation.

Decision: require the exact active recipe in native_completed_prepare, retaining
the operation-specific ledger path, attempt ancestry, native coordination,
no-rollback and nonempty observation checks. No prepared-plan compatibility
fallback remains. This follows the native completed_restart reader, which reads
native_completed_prepare before accepting a completed operation. This is a
smoke-evidence correction; no native renewal authority changed.

The receipt-ledger test file also lacked unittest.main, so running it directly
returned success without executing a method. Its executable entry now runs the
suite. The new regression failed both missing-completion and older-completion
cases before the oracle correction, despite a matching prepared recipe. The
same operation's exact completed recipe remains accepted.

Verification: receipt ledger 4/4 passed in 0.009 seconds; successor rest 2/2,
durable plan custody 2/2, request attribution 2/2 passed. Existing controls still
reject a child using its root ledger, mismatched ancestry/plan, rollback,
uncoordinated or unobserved completion, replaced tool evidence, and invalid
successor rest or request shapes. Diff whitespace checks passed. These are
private fixture/oracle tests, not a new provider or native end-to-end run. No
installed package was selected and the separate m157 proof remains deferred.

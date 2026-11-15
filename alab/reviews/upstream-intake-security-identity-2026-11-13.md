# Seccomp, workload identity and retained delegation review

Extend the reviewed prefix from 22 to 25 commits through upstream
`f562a7b3e23302c36964baf2770c98e1e3b0091f`, against ALab base
`4575be74ad380c331475c5a1bc6b636314b0e9e5`. Preserve the prior 22 decisions
verbatim and point both earlier batches directly at the new active record.
Of the 96 commits observed at upstream `f941a5fb927b429e2575cf32da8b27472255683a`,
71 remain outside this review. No upstream source is merged or qualified here.

## `1bda886113`: adopt the x32 seccomp restrictions

The x86-64 rule builder now adds x32-flagged syscall numbers and the separate
x32/legacy aliases for the existing denied operations. Socket and socketpair
argument conditions go through the same builder. Non-x86-64 construction is
unchanged. The three network policy modes retain their previous allowed
families and exemptions; the change closes an ABI route around those rules.

The new disposable-thread probes cover denied calls, allowed-call behavior,
and socket families in all three modes. They compare allowed results before
and after installation, including ENOSYS on kernels without x32. Require these
tests on Linux x86-64; this Mac cannot establish their runtime result. No ALab
hook in the changed files requires adaptation.

## `6242064da0`: keep SPIFFE authentication explicitly selected

File assertions remain the default. SPIFFE requires the explicit source,
federation rule, Unix endpoint and audience; mixed file/SPIFFE configuration
is rejected, and a socket environment variable alone does not activate it.
The session fingerprint includes the source settings and rejects changes
while the session is active. No new default or socket permission is granted.

The acquisition code reconnects per exchange, limits acquisition to ten
seconds, retains assertions only in memory, and reduces agent failures to
their gRPC status code. It rejects multiple returned identities and checks
the JWT subject and audience against the configured selection. Signature and
expiry validation remain the token endpoint's job. The existing token-exchange
destination-policy permit continues to enclose assertion acquisition and the
exchange. OpenAI-specific source settings are stripped from model-launched
environments; the general SPIFFE socket setting remains available to other
workloads under their existing filesystem policy.

The wire field numbers and unary RPC match the [SPIFFE Workload API](https://spiffe.io/docs/latest/spiffe-specs/spiffe_workload_api/).
The code supplies the required metadata and accepts only an absolute Unix
endpoint, a narrower choice than the [endpoint specification](https://spiffe.io/docs/latest/spiffe-specs/spiffe_workload_endpoint/).
Accepting JWTs without a key ID is consistent with the optional `kid` field
in [JWT-SVID](https://spiffe.io/docs/latest/spiffe-specs/jwt-svid/); this client
does not claim to perform signature validation locally.

No ALab caller directly constructs the changed assertion-source field. Keep
the workload-auth source membership hooks and test them on the merged tree.
Required tests include agent restart with concurrent refresh, a missing key ID,
an ambiguous or mismatched identity, redacted failures, stalled acquisition,
explicit configuration, active-session conflicts and non-inheritable environment
fields.
New upstream dependencies and their lock entries remain upstream-owned; do not
pin or rewrite them in ALab's lock policy.

## `f562a7b3e2`: adopt retained TUI delegation prompts

Dynamic-tool turn startup now sets the existing `TurnToolOutput.retain` flag
for both create-thread and send-message-to-thread. It retains the same escaped
delegation envelope as a tool output, preserving its source identity instead
of converting it to ordinary user input. Permission-profile projection and
the no-legacy-fallback boundary are unchanged. The updated lifecycle test
asserts retention on both operations. Exercise native retained-history and
exec handoff/compaction tests when integrating; this flag is not authority to
replay an already completed delegated action.

## Validation boundary

The source and test diffs were read; none of these upstream tests was run here.
Intake validation, exact preservation of the earlier decisions and unique
active coverage are checked separately. Linux execution, merged-tree native
tests, certified build and installed adoption remain required future stages.

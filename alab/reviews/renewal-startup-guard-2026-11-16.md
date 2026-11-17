<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Own renewal listeners before completing startup

Decision: extend the existing renewal body guard over report metadata completion
and listener acquisition. Reuse the shared `cleanup_resources` tail rather than
adding a second cleanup or exception-selection policy.

At `5f8aa27052`, public-main controls with inert listener pairs showed that an
issuer startup failure left the already-returned model listener untouched and
wrote no traces or result. Failure while constructing the report likewise
escaped after both listeners had returned. API cancellation retained its exact
identity but skipped all cleanup and evidence. Ordinary acquisition errors also
escaped the API instead of producing the body's failed observation.

The run now has a failed report seed and empty listener ownership slots before
its guard. Metadata completion runs inside the guard and before either listener
starts. Each returned listener/worker pair is stored as one owned value before
its server is extracted for the body. Final cleanup reads those pair slots, so
cancellation immediately after a returned pair cannot lose its worker handle.
A later acquisition refusal cleans only the pairs already returned, attempts
stop and every trace independently, and saves the final failed verdict after
that work. The existing API cancellation identity, final-save precedence and
CLI status rules continue to apply.

Six new methods use the actual `main` and original entry against the existing
inert renewal fixture. They cover first/second acquisition refusal, ordinary and
cancellation outcomes, actual caller-frame interruption immediately after each
pair publication, report-summary refusal before acquisition, failed cleanup and
final save, CLI exit policy, and successful two-listener handoff into the
existing body. Five methods exposed 21 failing subtests on the original source;
the successful handoff control already passed. All six pass on managed Python
3.14 and system Python 3.9 with `-I -B`. All 13 existing tail/CLI methods pass on
managed Python 3.14. `git diff --check` passes.

The six new methods bind no sockets, start no threads or child processes, and
invoke no application, native host, PTY, compiler or provider account. Stand-in
package and host records remain unqualified fixture data. Handler bodies,
native command construction, case proofs, budget start and budget values are
unchanged. The report keeps its existing complete fields when metadata succeeds;
a metadata refusal now saves its failed seed and the available error evidence.

This is caller ownership after a factory returns. Renewal's existing plain/TLS
`serve` still owns its internal bind, TLS wrapping and thread startup until it
returns; this change makes no claim about resources lost inside that factory.
Root/environment/model failures before the guard also remain outside this
listener-acquisition scope. No native application qualification or certified
build was attempted.

Root composition `6c7584a431ee549a908031b4f9487d0cb676369e` includes the
independently reviewed shutdown-worker refusal control and passes all 350
selected Python methods in 26.771 seconds, without failures, errors or skips.
The exact source stays clean and unchanged. Separate discovery-only observation
finds 1,898 unique IDs, no discovery errors and all six new renewal identities.
This is selected Python validation; no native prerequisite or application
qualification is inferred from the discovery or test count.

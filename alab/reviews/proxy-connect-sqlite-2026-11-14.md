<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Proxy CONNECT diagnostic exclusion — 2026-11-14

Decision: cover the boundary between a proxy refusing CONNECT and the target
rejecting a WebSocket Upgrade through the installed SQLite diagnostic sink.
Qualified source `660105af1f0319d5c883d2a1a7085281b5bb9f88`, based on
`cc8c682c58037c1a5a6bf158efe1d82135ac4fef`, adds one test to the existing
ALab-owned state-contract module. Its original 517-line Upgrade matrix remains
byte-identical. Independent static review accepted the change. This is an
expected-green coverage addition; no production defect or new RED is claimed.

The parent binds a loopback proxy and a separate loopback target listener. Only
the isolated child's environment selects the proxy; inherited HTTP/HTTPS/ALL
and NO_PROXY aliases are cleared first. No process-global environment changes
or external service are involved. The actual public Responses WebSocket client
uses the ordinary ReqwestDefault factory, without system-proxy fallback.

The proxy must observe the exact CONNECT authority and Host, with no target
path, Upgrade or request Authorization header. It replies with HTTP 400,
opaque-looking JSON, Retry-After and synthetic correlation/private values. The
fixture requires the exchange to finish, rejects subsequent target bytes and
proves the target listener received no connection. An arbitrary Network error
cannot satisfy the test. EOF and a peer reset/abort are both valid after the
client stops at the rejected CONNECT headers and leaves the entity unread.

The public result must retain its Network class and status-only CONNECT
message. API mapping must supply no provider HTTP status, retry advice or opaque
provider-400 recovery prefix. Ordinary Network/Stream retry policy remains
valid; this test does not require generic Network errors to become terminal.

An active enhanced-logging child installs the real state filter and SQLite
writer. A persisted INFO sentinel proves the sink is live. Flush, writer-loss
state and failure reporter are checked before querying every row. There must
be no endpoint diagnostic or HTTP body-capture row, including threadless or
wrong-thread rows. All stored messages exclude the proxy's synthetic IDs,
private markers and fabricated Upgrade/provider recovery fields. No diagnostic
event is manually emitted to simulate the result.

The exact child guard precedes its separate unpaused runtime. The parent awaits
the shared bounded collector directly, retaining its internal cleanup and
strict exit/one-pass/both-EOF checks. It owns the proxy through child assertions,
then joins/reaps it before checking no direct target connection and the child
report. Server operations have five-second bounds; no outer timeout cancels
collector cleanup. Temporary SQLite data is queried and then normally removed
after close; raw databases and rows are not retained as evidence.

Source-selected Trust 15698 unverified runtime session `57156` completed with
exit zero. The exact new test passed in 0.14 seconds with six filtered tests;
the full state-contract library passed all seven tests in 1.02 seconds with
none filtered. Both selections had zero failures, ignored or measured tests.
These are seven distinct tests across eight outer executions. Native runner
and library attribution are recorded independently of parsed pass lines.
Scoped all-target state-contract Tippy completed successfully, with empty
owned, spanning, failing and scripts lists and no upstream diagnostics set
aside. Independent checks found 809 existing canonical artifacts under the
separate lint target, both normal and test profiles, and successful build
completion. Feature ownership passed with 88 hook topics and 14 features.

The private source-labelled archive and publication receipt are under
`.git/alab-validation/660105af1f0319d5c883d2a1a7085281b5bb9f88/proxy-connect-sqlite/`.
It contains 41 indexed files, including 28 recorded-run files and two binary
identities. `record.json` SHA-256 is
`81996b36c72c14f2692411a1ad3a324054dac90ef9d78ce37836fd2a7c782cb7`;
`source.diff` SHA-256 is
`bd0aa3b152386754cdbbc76c3177a1d5954d13d5563cbeb4530729a8f029cc20`.
The checks establish a synthetic local transport/persistence exclusion. They
identify no original backend cause, grant no recovery authority, and qualify
no application build or installed release.

A subsequent bounded comparison found maintained contracts for the remaining
request-diagnostics guide promises and no concrete new counterexample.
Additional status/SQLite combinations would duplicate shared behavior. The
next useful incident evidence is a source-identified unexpected outcome or
server-correlated observation; synthetic tests cannot recover the original
missing wire request. Keep that backend cause unknown and reopen against new
evidence. The compiler prerequisite for a dependency-inclusive certified
application remains separate from these passing runtime contracts.

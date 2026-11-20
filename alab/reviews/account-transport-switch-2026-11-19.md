<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Account changes during budget checks and WebSocket reuse

The first account-isolation scenario uses seven named tests from
`codex-core --test all`. The other six scenarios retain their original
descriptions and have no recipe in this review. The feature campaign's
`collect-part --feature account-isolation --scenario 1` selects this row for
a candidate with an applicable recorded gate request.

The runtime fixtures use synthetic external credentials and a loopback
registration service. The HTTP fixture holds the generation budget callback
at a barrier, reloads a different actor, and releases the callback. The server
expects two registrations and one generation request. Dual-header mode
checks the destination bearer. Assertion-only mode checks the assertion
format and absence of the destination bearer from the Authorization header.
Matching that HTTP assertion's instance ID to the destination registration
remains unverified in the budget race. See
[`client_websockets_runtime_auth.rs`](../../codex-rs/core/tests/suite/client_websockets_runtime_auth.rs).

The WebSocket fixture holds its budget callback after the connection opens,
then changes the runtime actor. Each of the four auth and gateway combinations
checks five handshakes, runtime instance changes and request counts of
`1, 1, 1, 0, 1`. The retired fourth connection receives zero requests and the
new runtime receives the final request. Core revalidates the HTTP owner after
the budget callback and the WebSocket owner after the post-connect callback.
See [the request and connection paths](../../codex-rs/core/src/client.rs).

The ordinary ChatGPT control exercises a connection eligible for reuse.
It keeps the old peer open, changes the synthetic `auth.json` and reads the
account ID and bearer from each actual handshake. Account A gets two requests
on one connection. Account B gets three on its own connection. Its first
request carries the full prompt, with no A response ID or turn state. The
fixture repeats the switch within one turn and across a new-turn preconnect.
See [the WebSocket integration suite](../../codex-rs/core/tests/suite/client_websockets.rs).

The source inventory requires the seven exact test names for this scenario.
The protected runner rejects a libtest log reporting a network skip even if
libtest counted the early return as a pass. The frozen runtime under review is
`a98666774c6d3e5f22e6269538fa719284e3b902`. It predates the source inventory
declaration added by this review.

On that exact source, the protected Core integration stage selected Trust
15698 and ran all seven named cases. Seven passed in 3.61 seconds, with zero
failures or ignored tests and 3,296 unrelated tests filtered out. The raw
combined log's SHA-256 is
`941eee6872b07998aa498550cb8acfa6a42d8603d15b885476d4fb31305dcd05`.
The separate unchanged-owner trace control also passed on this source. The
host ran 85 campaign, scenario collection, gate, inventory and focused-result
tests. The full contract continues to require all seven scenarios.

<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Repeated SSE response-header correlation — 2026-11-16

## Decision and reason

Exercise repeated physical HTTP response headers through the real HTTP200 SSE
transport. Existing product code already selects the first safe whole already-decoded value from
each repeated field, gives valid `x-request-id` values priority over the
`x-oai-request-id` alias, and retains request and ray IDs independently. Earlier
real HTTP400 tests and scripted SSE tests covered adjacent behavior, but did not
establish this opening-HTTP200-to-selected-SSE-error integration.

This is GREEN-only coverage of existing behavior. No production parser,
diagnostic selector, retry policy, compiler pin, manifest, lockfile or
upstream-owned source changes. The original opaque HTTP400 backend cause remains
unknown; a passing integration test does not identify its rejected field.

## Observable contract

Two isolated native child methods each declare six ordered raw-header matrices:
invalid early values followed by valid primary values; first-safe priority;
later alias fallback; entirely invalid whole values including commas;
request-only retention; and ray-only retention. Mixed-case names test the HTTP
parser's case-insensitive field handling. Each exchange uses a direct loopback
TCP response with repeated physical header lines, rather than inserting or
comma-joining values in a synthetic HeaderMap.

One method requires the original typed `InvalidRequest`, its exact message and
terminal/no-advice/no-fallback mapping, followed by channel closure while EOF is
still withheld. The other releases EOF and requires the original buffered
`Retryable` with no advice, preserving its ordinary retryable stream mapping.
Exactly one opening RateLimits category is accepted before the selected error;
arbitrary successes and completion cannot be skipped. Both paths verify channel
closure and bounded joined fixture cleanup before diagnostic assertions.

The selected diagnostic retains the original request subscriber and thread/turn
ancestors after caller guards have exited. Shape and failure share one positive
local attempt, the expected transport and fixed failure kind. Opening HTTP200
does not become an error status; invalid-request advice is absent and the
unadvised Retryable slot records false. Private input/provider/body/header data,
invalid whole values and unselected safe tokens remain absent. Independent
optional request/ray equalities run last.

The fixture has separate event and EOF gates. A 15-second live EOF receiver
outlasts the five-second selected-outcome deadline, preventing an expired fixture
gate from masquerading as immediate selection. Server event-written observation
plus a Pending receive does not establish client decoder progress. The eventual
exact Retryable after EOF establishes parsing without a pre-EOF scheduling claim.
Default fixture bytes, original body deadline and incomplete-body Drop/cancel/
finish checks remain. Expected peer-close errors are tolerated only during the
scripted fixture's final shutdown; all task ownership remains bounded.

The test capture filters exact diagnostic target and INFO level before visiting
event fields. This limits this harness's capture boundary; it is not a global
logging privacy proof.

## Qualification

Source `b07ec257172a98a0883c48235fb6a82ef539459e` passed both complete libraries:
103 public-contract methods and 283 API methods, totaling 386. Failed, ignored,
measured and filtered counts were zero. Both required methods passed once in the
outer results, with their isolated children completing their declared loops.

The prescribed project frontend selected committed Trust 15698. Maintained
record-results execution used the explicit unverified library-test lane, ordinary
runtime target, one round and one test thread. Both `codex-api` and
`codex-alab-api-tests` remained selected in the shared graph. Scoped all-target
Tippy used the separate canonical `target/tippy` cache and completed with zero
owned, spanning, failing, missing-script or set-aside findings. Unit, lint and
outer controller statuses were zero.

Source and target stayed frozen through live capture of two library images and
the native unit runner. Prepared/current file identities, compiler digests,
launch/CWD receipts and diagnostic directory identities matched. Both normal and
test lint profiles and 657 canonical artifact paths were observed before the
subsequent documentation-only change. The private
`sse-repeated-headers-20261116` stage retains raw results and independent reviews;
historical executable paths are not rehashed after later rebuilds.

The first source `ddc0bc0fe6843161b4e3dcfde0353583f594bd40` did not compile:
Futures requires an Unpin future for a manual poll, while Tokio's async receive
future needs explicit pinning. This was missed in static review. The scoped
stack-pin correction preserves the one-poll oracle and ends its receiver borrow
before the next phase. The failed controller, compiler diagnostics and capture
refusal remain separate: no prepared state or selected library execution existed,
and no stale image or failing-baseline comparison was accepted. The corrected
source's fresh passing phase supplies the runtime evidence.

Native resource reservations were waited out without changing claims, pools,
compiler selection or targets. All owned verification jobs completed.

## Limits and publication

Two methods each declare six source-loop rows. Strict child success and an
end-of-loop marker support the declared exchange coverage; per-case output is
not forwarded as an independently observed wire inventory. No complete physical
request/frame count, connection-reuse proof, full Core/TUI actor, scheduler bound,
application certificate, package, installation or delivery is claimed.

Qualification remains attached to its exact source. Disjoint already-main peer
work may be preserved byte-for-byte under conservative selected-graph, compiler,
workspace, Git-control and helper guards, without executing or qualifying it.
The unavailable application-certification prerequisite and human holds remain.

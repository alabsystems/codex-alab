<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# HTTP rejection body read progress — 2026-11-16

Decision: add numeric rejection_body_bytes_read to the existing HTTP capture
observation. Complete-only rejection_body_bytes intentionally omits unavailable
entities; it cannot distinguish a zero-data stall from a partly delivered body.
The new independent observation helps diagnose that phase without retaining or
formatting provider payloads. This is a logging enhancement, not a new diagnosis
of the original opaque HTTP 400 or a change in recovery policy.

The count means bytes returned by successful reqwest chunk() calls, after
transfer/content decoding and before charset/BOM conversion. It starts Some(0)
only after the declared-size precheck enters collection. It includes the whole
last successful chunk that exceeds the retention limit. Every preceding chunk
was retained, so body.len().checked_add(chunk.len()) is the cumulative total;
an unrepresentable sum stays unavailable. None also covers an unpolled body
collector, including pre-read denial, a precheck rejection or a deadline already
ready before its first poll. Source review establishes the last timing condition;
this topic does not simulate native credential-admission queues.

The local Option lives outside the original permit and inner timer race, borrowed
by collection until the operation finishes or is dropped. Timeout and revocation
can therefore retain successful-read progress. The counter uses no atomics or separate
heap allocation. It adds no body copies, public interfaces, dependencies or
upstream edits; a subscriber can allocate when recording the extra field.
The old complete-only field, body decoding, 64KiB retention limit, five-second
budget, status/header/advice ownership and original permit priorities remain.
Dropping the owning future still emits no completed capture phase.

Two test methods use synthetic reqwest Body streams to exercise the actual
private capture function and real NetworkPolicy permits. A held stream acknowledges its next pending
poll only after the preceding chunk returned through collection. This proves
consumer delivery, instead of inferring it from a TCP write. Before late count
assertions, tests require old phase/outcome/status/advice observations and body
owner retirement. Controls cover zero data, complete and empty bodies, read
failure, unknown-length overflow, size precheck, pre/post-progress revocation,
and owning-future cancellation. They never activate a global ALab mode. Native
credential admission and an already-ready precollection deadline are unmeasured.

Two real HTTP integration methods retain Responses/Images semantics, original
status, headers, retry advice, actual POST content, privacy and socket/task
cleanup. Responses uses an exactly 64KiB+1 unknown-length entity: selecting the
size rejection requires all 65537 returned bytes regardless of segmentation.
Read-error cases use honest bounds up to the sent prefix rather than claiming
all sent bytes reached the collector. Complete/empty, declared oversize and
buffered strict-UTF8 controls retain their original classifications. The matrix
has six Responses and five buffered exchanges if both wrappers complete.
Field-name allowlists reject unexpected fields before formatting their values.

Initial RED 149031c5671a5bdf92645fc01f66bf3c4fe48525 executes 939 outer methods:
935 pass and four fail, zero ignored/measured/filtered. Only three failures are
metric oracles: two synthetic None-versus 23 counts and buffered None-versus 85.
Responses first fails an inherited http.method expectation at line 833: its
literal string is rendered quoted, whereas Images uses display formatting.
That initial attempt remains explicitly unqualified as a four-metric RED, with
all four artifact/receipt observations preserved under red-original paths.

The sole fixture correction quotes that one Responses expected value. Corrected
RED 5a128384319da172468ce56aef28f804f35fe9b8 again runs 939 outer methods:
935 pass and all four fail at genuine late metric comparisons. Responses gets
None versus 65537 at line 887; buffered gets None versus 85 at line 793; synthetic
timeout and revocation get None versus 23 at line 94 after healthy controls and
body retirement. Suites are 93/2 public, 424 owned core, 281 API, 137/2 HTTP.
Two exact-child failure summaries are excluded from the outer total. Required
method identities are checked from each outer harness prefix, excluding nested
failure/success details rather than counting duplicated child lines.

Eight disjoint incoming main paths are byte-preserved at 98ca741f4. Actual GREEN
reruns on merged 0dcdc3237e51a166a26b8e430b739b56e05e24c1, with all three test
files byte-identical to corrected RED. All 939 methods pass: 95+424+281+139,
zero failed/ignored/measured/filtered. Four required outer methods pass once.
Controller, unit and Tippy compiler exit 0; owned/spanning/failing/script and
set-aside lint categories are all zero. Independent observations retain all four
binary hashes, preparation snapshots, native execution receipts and diagnostic
identities for each attempt. The helper and guide remain byte-identical to the
reviewed dc467bea56c9007a50b6a2575b6517aaf031dbb3 production commit.

Both final phases use source-selected Trust 15698 and the native frontend/runner
in the explicit unverified test lane. The 12 controlled body cases include nine
new-metric assertions, two legacy healthy controls and one cancellation/no-phase
control. Both integration wrappers complete all 11 real HTTP exchanges. Incoming
Python workflow CLI guards pass 10 controlled failure-path methods with actual
exit 0; no native prerequisites or broad Python workflow are substituted. Other
incoming TUI, compiler-refusal and intake records are preserved without claiming
their unselected suites. This topic adds only this decision record after the
final Rust execution.

Native build/lint admission waited behind other machine work. Source changes and
reviews proceeded in an isolated worktree while RED stayed frozen. A read-only
fleet capacity check led to one plain-shell spawn refusal on m71, whose existing
policy permits no new tabs. No worker was created or access changed; all accepted
verification ran locally. Only this topic's implementation worktree/branch was
removed, and its commits remain in the topic history.

All first positive rows require healthy legacy controls before their late new
metric assertion. RED stops at its first missing metric in each method; later
rows are GREEN-only. Successful child output is drained, so completed row reach
follows passing wrappers and source flow rather than emitted markers alone.

Evidence is limited to these selected library/transport fixtures and scoped
lint. The read count is not wire traffic, retained allocation or proof of a
complete body. Actual Core/Autopilot retry execution, native credential admission,
physical host sleep and application delivery are not qualified. No package or
running binary is replaced. The recorded application-certification prerequisite
and unknown original backend rejection reason remain unchanged.

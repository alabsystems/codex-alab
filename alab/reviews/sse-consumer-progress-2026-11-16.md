<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Observe SSE consumer progress before EOF — 2026-11-16

## Decision and reason

Replace the deferred correlation test's manual Pending receive with an ordered
public event. Hostile review found no defect within the prior test's explicitly
limited claim: server emission plus a Pending receive did not establish client
decoder progress. An exact public event from a later frame provides stronger
evidence without adding a dependency, clock, sleep, fixture API or outer method.

Only the owned context-test file changes. There is no production, upstream,
manifest, lockfile, compiler or package change, and no failing baseline is
claimed. Remove the obsolete manual poll and its pinning rather than retaining
two competing progress probes.

## Observable contract

Each of the six deferred header rows now sends the unknown `response.failed`
frame followed by one minimal `response.created` frame with a fixed private ID.
Before releasing the live EOF gate, the test must receive exactly that public
Created event and still observe only the shape diagnostic. It then releases EOF
and requires the same original typed Retryable, message and absent advice.

The current SSE consumer processes frames sequentially, forwards Created as a
nonterminal event, and leaves its pending error intact. The ordered public
witness establishes progress past the earlier failed frame; the eventual exact
original error establishes preservation through that event. Treating the failed frame as immediately
terminal, or losing its buffered error, cannot satisfy these two outcomes.
This exercises the current consumer contract, not a provider protocol guarantee
or arbitrary scheduling bound.

The immediate-error rows are unchanged. Both paths retain exact prelude, original
and mapped policy, channel closure, bounded joined fixture cleanup, original
request context, fixed classification and privacy checks. The new private marker
is explicitly excluded from diagnostics. Independent optional request/ray
correlation checks remain last and still refer to opening HTTP headers.

The same two isolated methods each declare six header rows: twelve exchanges and
six public witnesses. There are eighteen scripted SSE events in the fixture
source. These are source-loop declarations, not independently measured physical
request/frame counts or a complete wire inventory. The server's write
acknowledgment continues to mean emission only; the client event supplies the
new progress evidence.

## Qualification

Source `a96b490d714c4e16fee2ed5ba802803e583c0383` passed both complete libraries:
103 public-contract methods and 283 API methods, totaling 386. Failed, ignored,
measured and filtered counts were zero. Both required outer methods passed once;
their strict isolated children completed the declared rows, including all six
exact public Created witnesses.

The prescribed project frontend selected committed Trust 15698. Maintained
record-results execution used the explicit unverified library-test lane, ordinary
runtime target, one round and one test thread. Both API packages remained in the
shared graph. Scoped all-target Tippy used canonical `target/tippy` and completed
with zero owned, spanning, failing, missing-script or set-aside findings. Unit,
lint and outer controller statuses were zero.

Source and target remained frozen through live capture of two library images
and the native unit runner. Prepared/current identities, compiler digests,
launch/CWD receipts and diagnostic directory identities matched. Both normal and
test lint profiles and 657 canonical artifact paths were observed before the
subsequent documentation-only change. The private `sse-consumer-progress-20261116`
stage retains raw outcomes and independent reviews; historical executable paths
are not rehashed after subsequent cache rebuilds. All owned verification jobs
completed.

## Composed-source qualification

Main advanced to `5f8aa27052699540c0d4fc089ebfb565ff988ba4`, changing the protected
`cli_status.py` import used by verification and publication. The helper retained
its original function bytes and appended one operation wrapper. Its delta
received independent static review; the existing guard was retained and the
exact incoming bytes were manually composed. The initial API result was not
transferred to the new helper source.

Composed source `2ec7e00a2096c2620b6c3c3407da441d95dbb781` freshly passed both
complete libraries: 103 public-contract and 283 API methods, totaling 386, with
all other scalar counts zero. Scoped all-target Tippy again had zero findings
or set-aside diagnostics. A separate live capture bound the two libraries and
runner, both lint profiles, 657 canonical artifact paths and the native
receipts/identities before this documentation update. The overlapping initial
and composed passes are not added into a larger distinct-test count.

Three controlled Python suites also ran on that exact composed source: smoke CLI
10, GitHub CLI 10 and qualification CLI 8. All 28 methods passed, with no skips,
failures or errors. Their raw statuses/summaries and sixteen source/helper input
hashes were guarded before and after execution. These are inert case callbacks,
controlled child entries and temporary local Git fixtures; no actual Codex,
compiler, remote operation or complete Python workflow was qualified by them.

Initial source/control/evidence records remain separate and immutable. The
closing controller now resolves the recorded evidence phase and verifies its
source/status and the helper qualification before changing checkouts or refs;
the earlier controller is explicitly historical. This corrects a record-assembly
assumption without changing the application or weakening qualification guards.

## Limits and publication

This is GREEN-only test coverage of existing behavior. It does not identify the
original opaque HTTP400 backend cause, prove a global logging privacy property,
or qualify a full Core/TUI actor or packaged application. The existing live
certified application-build requirement and human holds remain unchanged.

Qualification stays attached to its recorded source. Disjoint already-main peer
work may be preserved byte-for-byte under the conservative native input,
compiler, workspace, Git-control and helper guards, without executing or
qualifying those unrelated paths. All owned verification jobs must finish before
closing cleanup changes source checkouts or refs.

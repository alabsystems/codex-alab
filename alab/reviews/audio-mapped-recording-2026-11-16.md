<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve mapped audio recording scope explicitly — 2026-11-16

## Decision and reason

Close the previously recorded mapped-audio boolean coverage gap in the existing
public SSE and real WebSocket fixtures. They already checked the original
`ApiError` fields. Their mapped checks examined display, protocol category and
retry policy; one expected error passed through the same mapper as the actual
error. Those checks could miss a mapper that forced `per_recording` to a
constant. The current bridge correctly forwards it; no production defect or
failing baseline is claimed.

The downstream audio recovery consumer distinguishes an individual recording
rejection from a combined-upload size rejection. The tests now borrow
`mapped.details()`, require the typed `InvalidAudioInput` variant, and compare
its message and boolean directly against the fixture's expected values. Remove
the obsolete same-mapper expected-error oracle from the dedicated SSE fixture.
Original error fields, timing, terminal/retry policy, context, safe IDs,
diagnostic privacy and bounded fixture cleanup remain checked.

Add a direct workspace `codex-protocol` dev dependency to the ALab test crate so
it can name this public typed result. That package was already transitive. The
lockfile changes by one dependency line inside `codex-alab-api-tests`; every
other byte remains unchanged. No package version, production source, upstream
hook or fixture row/envelope changes. Hook inventories before and after match.

## Coverage and qualification

The same five existing outer methods declare 78 fixture rows. Dedicated SSE
uses six codes, two envelopes and three contradictory tails: 36 typed checks,
30 true and six false. The two WebSocket methods retain `response.failed`
envelopes, 40 typed checks and two completion controls: 20 true and 20 false.
Together they declare 76 mapped boolean checks, 50 true and 26 false. These are
source-loop/control-flow counts, not new test methods or independently measured
physical request/frame totals. No WebSocket error-envelope coverage is added.

Source `01e5b69871f7ac40418fc50ac1b54befcdf79db6` passed both complete API
libraries: 103 public-contract methods and 283 API methods, totaling 386. Failed,
ignored, measured and filtered counts were zero. Each required outer method
passed exactly once, with its strict isolated child executing the existing
fixture rows. Scoped all-target Tippy had no owned, spanning, failing,
missing-script or set-aside findings. Unit, lint and controller statuses were 0.

The prescribed qualified frontend selected committed Trust 15698. Tests ran in
the explicit unverified library lane, ordinary runtime graph, one round and one
test thread. Source and target remained frozen through live capture of both
test images and the native runner. Compiler digests, prepared/current images,
launch/CWD receipts and diagnostic identities matched. Both selected lint
profiles and 657 canonical artifact paths were observed. The private
`audio-mapped-recording-20261116` stage retains raw outcomes, capture and reviews.
The capture is an observation; independent qualification is a separate record.

## Composed-source qualification

Main advanced to `37ce487652b090925f9b508cc4549289db2f77af`, including a protected
`unit-tests.sh` change. Publication stopped before merge or write. Independent
review established that the delta adds an 8 MiB stack export to
`all_alab_tests`; the selected `unit_tests` path was unchanged and already used
that stack setting. Exact incoming documentation, catalog and unexecuted smoke
expectations received separate static admission. Preserve the guard and carry
already-main peer bytes exactly; do not transfer the initial runtime result.

Composed source `41e6107d72961638b9b8bcb868c21a60790b17f5` freshly passed both
complete libraries: the same 103 public-contract and 283 API methods, totaling
386, with all other scalar counts zero. Scoped all-target Tippy again had zero
findings or set-aside diagnostics. A separate live capture bound both libraries,
the runner, receipts, diagnostic identities, both lint profiles and 657 artifact
paths. Independent qualification accepted this new evidence before the source
hold was released. Static feature-catalog/hook inventory also passed, without a
semantic qualification claim for unrelated peer behavior.

The initial source, capture and reviews remain immutable historical records;
counts are not added across phases. Closing controls now require the explicit
`composed-37ce` child phase and its source to match the proof and qualified
runtime. Its qualification, checks, controller and capture must each appear once
in the hash index before cleanup changes source or refs. Initial root-level
records cannot satisfy that active-phase gate.

## Limits and publication

This is GREEN-only coverage of existing public error mapping. Scripted SSE and
a real local WebSocket peer do not qualify actual provider behavior, the full
Core audio actor or a packaged application. The original opaque HTTP400 cause
remains unknown. The unchanged certified application-build prerequisite and
human hold remain; no installation, link, account/model change or peer restart.

The initial maintained origin fetch was refused by the background core-read
reserve before effects. Honor its reset time, then obtain a fresh expected-main
view before publication. Preserve disjoint already-main work exactly without
qualifying its behavior; reject changes to owned/compiled inputs or protected
controls until appropriately reviewed and requalified. Bind original runtime
attribution through any composition, finish owned checks, preserve other
branches and the pre-existing peer, and retain private evidence and the handoff.

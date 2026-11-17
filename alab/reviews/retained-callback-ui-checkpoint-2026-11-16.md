<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Whole UI and callback checkpoint

This checkpoint adds retained UI data transfer for rich messages, user input
forms, pending views, startup draft state and managed readers. Original queued
answer values and their reply identities remain owned across preparation.
Private Source callback records retain observed outcomes, enforce bounded
reservation before publication and separate live connection, configured
no-primary and selected Core acknowledgement routes. The actual connection gate
closes before asynchronous bookkeeping, and the retained backend dispatcher
registers the same gate. Endpoint closure or callback delivery alone does not
authorize Core work or prove that an answer was published.

The SDK additionally has a deferred UI-data capture window after the genuine
Native H1 acknowledgement and authenticated settings connection, before I1/G1.
The pure editor checkpoint codec validates closed data; it does not restore an
editor child, TTY, input ownership or completion. Actual full UI capture, earliest
no-primary installation and queued answer publication callers remain unfinished.

This checkpoint merges observed main 290ce46852 without changing any of the 106
in-progress source paths during the merge. The incoming 44 paths do not overlap
this component. Independent review found no retained protocol/authority conflict;
the four incoming Rust paths change tests or fixtures. Focused discovery of ten
incoming Python suites passes 87 tests, with zero errors, failures or skips.

## Component evidence

The CLI, TUI, app server and app-server client all-target check passes on
input 36cef5581bb2aec371fb36edbf997ecde0e541755c4efde9673faefb70b5edec.
Subsequent runtime input
a4960b0a166c660dc9cc8fae113390ccbfa4a288c3db228227215a60bc4366ae
passes 373 scoped tests: SDK 86, core 72, ALab TUI 11, backend 85, client 17
and TUI 102. Three existing tests are ignored. These are source-selected
Trust 15698 explicit unverified checks/tests, not shipping certificates.

The runtime run found two genuine integration problems before passing:
existing lower approval fixtures lacked the newly required actual connection
gates, and an internally tagged enum could not deserialize its own RawValue
payload through serde's buffer. Fixtures now register actual Source gates
without changing their original assertions. The view codec directly decodes
its original closed wire object, preserves raw JSON, and has an actual wire
round-trip assertion. A follow-up restricts its discriminator to the original
string syntax and checks malformed kind values and unknown inner fields.

The official owned lint classifier on the 373-test input reports 140 owned
diagnostics, no spanning diagnostics, no failing targets and no missing build
scripts; 14 upstream diagnostics are set aside. Lint is not clean. Unfinished
callers and representation/style findings remain visible without suppression.
Independent reviews of the callback fixtures, codec and upstream hooks found
no remaining critical issue for this development checkpoint.

The final discriminator change affects only the view codec and its identity
control. Exact input
1728b12202e66a933e9c9b2fec2344e09b0c74a3f09d8cfcf9a43fc25987b79b
passes all 102 scoped TUI tests again, including actual wire decoding and
rejection of object/numeric/unknown discriminator values and unknown view
fields. Other files are byte-identical to the 373-test input.

A fresh remote main advanced to 2bb2974a598806bea5846b482eafa290c8080912.
Its 17 incoming paths are disjoint from all 106 component paths. Independent
review confirms the production Rust change only corrects journal-lock error
wording; the SSE test change adds an actual ordered consumer progress witness.
Five affected Python suites pass 50 tests on exact main 2bb, with no failures,
errors or skips. Prior Rust test scopes remain exact; that main merge is not
represented as a new certified or full Rust test run.

## Qualification limits

The ordinary retained CLI selector remains inactive. This is a development
checkpoint; it does not provide a seamless production live update. Source
publication of original human values, actual full C1 capture and early C2
installation, active editor continuation, complete Side creation history cuts,
repeated signed Native/Source/UI execution, clean owned lint, the certified
application gate, main landing and installation remain required. Drafts of
those vertical paths remain outside the source tree and are not qualified by
component tests. No shipping certificate or application activation is claimed.

Final scoped input 1728b122 passes the same official lint classification: 140 owned diagnostics, 0 spanning, 0 failing targets, 0 missing scripts; 14 upstream diagnostics set aside. The shipping lint gate remains unmet.

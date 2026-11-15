# Accepted input and session continuity

Review the next three upstream commits through
`1111483b4836b7c220ac2f08bbaa5b50e3d0052e`, extending the existing 28 decisions
without changing them. This is source review only; it neither merges upstream
code nor qualifies a binary. The observed upstream snapshot has 65 further
commits after this prefix.

## Dictation attribution (`006911a313`): adopt, retain analytics off

The input boundary removes `codex_app_used_dictation` from both replacement and
patch metadata before forwarding Responses metadata. The patch wins even when
its value is malformed: parsing then produces unknown attribution instead of
falling back to an older replacement. Attribution is recorded only after a
user-origin input enters history. Synthetic/agent input does not count as user
dictation.

The reducer reports true after any true input, false only after known false
inputs, and otherwise omits the field. No-input and unknown are distinct
internal states. ALab's packaged `analytics.enabled = false` remains unchanged;
this review does not authorize enabling telemetry.

ALab's private `ExecPendingInputs` stores and round-trips the whole
`UserInputMetadata`, so the added optional, defaulted field needs no parallel
handwritten encoding. Its existing struct literal uses `..Default::default()`.
Require a populated-field exec round trip on integration, plus extraction,
patch precedence, malformed values, unknown/true aggregation and the app-server
steering test that checks the key is absent from Responses headers. Upstream's
new tests cover precedence, header stripping and representative aggregation;
the missing-value and invalid-value permutations should also be checked.

## Session resume (`5183e1dcb2`): adopt with continuity checks

The shared resume path pauses submission with a scoped guard, drains outgoing
events before capturing input, and preserves the outgoing session if terminal
clearing fails. Backend switches park input before activation. Restored input
reconciles exact client IDs against both resumed history and buffered events.
Missing receipts keep submitted input uncertain; they do not authorize replay.
Writable active turns retain their running state. Eligible idle queues resume
after activation, while read-only views do not submit.

Interrupted steers encountered during the handoff are queued on their original
thread rather than emitted against the destination. Drafts retain whether a
leading bang was literal or shell syntax. A Cloud shell rejection clears the
pending submission because no completion event will arrive, allowing the next
ordinary prompt to proceed without sending the rejected shell request.

Read the production diff and changed tests, including live mock-server session
round trips, buffered completion/interruption, read-only to writable receipt
reconciliation, literal bang preservation and Cloud rejection. Require those
tests on the integrated candidate. Also require ALab safety-recovery and
exec-input tests: `alab-tui/glue/app_safety_recovery.rs` restores this same
`ThreadInputState` after failed submission, while the native recovery fix must
retain accepted input and completed side effects. The added composer field is
internal; no direct ALab struct literal was found. Do not infer compatibility
solely from that search.

## Attached sessions (`1111483b48`): adopt

A shared visibility predicate excludes ephemeral and attached side chats;
root eligibility additionally excludes parented/spawned threads. History
discovery, pinned sessions and refill accounting use the same root predicate.
This changes overview membership, not storage or thread access. The added
saved-side-chat case in `shared_overview_shows_only_root_sessions` must pass,
alongside ALab native-session navigation checks.

## Validation boundary

Validate exact 31-commit coverage, byte-equivalent prior decision objects and
one active record for each covered commit. Runtime tests remain pending with
the certified integration gate. The separate operational recovery at 22:56 UTC
resumed the stalled peer through `/goal resume` and restored its unsent `8`;
it is evidence that manual recovery works in the installed older binary, not
evidence that the uninstalled automatic recovery fix has shipped.

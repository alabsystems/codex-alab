<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Explicit history retention, Guardian allocation and child CA defaults

A successful maintained Agh fetch still observes upstream main at
`0184dfa6a48f0110dd9c6835522492908e5abcf3`. The clean review worktree advanced
to current ALab main `6554574fc64134b48c694ed9505f7016883fa6a8` before this
review. Extend the reviewed prefix from 61 to 66 commits through
`d87330d91e2309aea3b82286961340181d145ab4`, preserving prior decisions unchanged.
112 commits remain beyond that prefix. This report integrates no upstream source
and does not supply runtime qualification.

## Explicit retention: adapt `c33002259e`

The injection endpoint accepts raw response items or wrapped items whose policy
is explicitly `default` or `always`. Raw input remains unspecified; new policy
is persisted separately from legacy `client_authored`. The explicit false value
keeps ordinary history rules even when developer-message retention is enabled.
Retention is host history metadata, not provider wire configuration or execution
authority.

Local/remote compaction and fresh context windows share newest-first selection
with original-position output order. Retained tool exchanges must be adjacent,
matching call/output kinds and call IDs, with both items marked always. Whole
structured exchanges pay their complete group cost and cannot be partially
truncated. Resize notices stay with their source, while client-authored or
explicit-policy developer messages are treated as independent records. Local
user-message retention uses source positions to avoid duplicating an occurrence
that already qualified under the explicit budget. Pending mail still uses its
final compactor input and appears once.

Fresh child creation inherits explicitly retained records under the retention
budget. It keeps retention/replay metadata while discarding parent checkpoints
and marks inherited records as inherited authorization. Full-history forks
already carry them and also preserve that provenance. ALab's native handoff
codec stores the complete metadata, but source restoration must continue to
apply current inherited-authorization restrictions; retention cannot upgrade a
parent's evidence into a new local grant.

### Required correction: validate consumed paired outputs

Source review finds a defect in the new `record_injected_items` loop, still
present at fetched upstream main. The loop validates its current item, then
consumes the next item to verify an always-retained call/output pair. That output
never becomes the loop's current item, so it skips
`validate_injected_response_item`. The skipped validator rejects uploaded audio
file references in both messages and function/custom tool outputs, because those
files require typed turn-input admission. The endpoint's separate image URL
validation does not enforce that audio rule.

An always-retained function/custom call followed by its matching always-retained
output containing `input_audio` with `file_id` therefore skips this source-level
refusal. This is a control-flow finding, not a runtime exploit or successful
native reproduction claim. Integration must validate every converted item in an
independent pass before scanning pairs. Keep pair adjacency/identity checks and
the original refusal; do not resolve file audio or weaken admission in this
history-only path.

Require regressions for function and custom tool-output file audio in retained
pairs, plus unpaired/raw and ordinary valid-text controls. Rejection must precede
history injection, initial-context capture and checkpoint work, with no uploaded
file lookup or network request. Also cover empty input, unsupported/missing
wrapper policy, mismatched IDs/kinds, output-first and separated/mixed-policy
pairs. Run the existing forged-configuration tests and
`raw_injection_rejects_uploaded_audio_before_modifying_history`, whose message,
function-output and custom-output cases currently exercise raw/untyped inputs,
not the new consumed retained-pair path.

### Required startup reconciliation

Upstream adds a non-inlined `spawn_internal` factory returning a boxed
`initialize` future. Current ALab already wraps startup construction and splits
preparation from initialization through `alab_assignment::phased_startup`; its
continuation explicitly owns source leases and discarded factory providers.
Reconcile the outer factory at integration while retaining that deeper phase
boundary and synchronous cancellation. A box alone is not evidence that
preparation's poll frame ends before `Session::new`, and it does not replace the
additional boxed submission-loop construction. Avoid carrying duplicate outer
wrappers where one ALab factory supplies the upstream intent.

Require original capture-tree/startup controls, task-local identity, cancellation
in each phase and lease lifetime. Preserve checkpoint/input/ephemeral-history,
compaction-provenance and effective-model recovery hooks in the changed Core
files. Require explicit retention across repeated local/remote compaction,
restart, fresh/forked children and native private handoff, including pending mail,
retained image replay sources and restricted inherited authorization.

The same commit updates Quinn's lock entries and adds three matching
Hickory DNSSEC advisory exceptions. Carry upstream lock resolution, never a
manual lockfile pin. Before integration, inspect the resolved all-workspace
feature graph and require DNSSEC to remain disabled. The comments' stated
advisory scope is not independently certified by this review. Reassess exceptions
before enabling DNSSEC and remove them when the stated fixed dependency is used.
Keep removed Bazel paths absent.

## Async Guardian allocation: keep `a655b50de9` opt-in

`async_elastic_budgeting` and its section policy are independent of synchronous
budgeting and disabled by default. Config validation rejects the sync-only
selected-exec-results section for async requests. Model-default resolution keeps
the configured async policy rather than replacing it with model defaults.

Only new classifier evidence is allocated after charging setup and immutable
conversation history. History is not taken until the complete candidate input
fits the request limit. Failure leaves committed history and cursor unchanged.
Permission context stays protected and whole even when a section's ordinary
text cap is small. Failed image preparation records an omission; duplicate
already-delivered images stay distinct from failed preparations. The shared
delivery method is renamed `prepare_delivery`; no ALab direct caller or
explicit affected config literal was found.

Require opt-in/config-error and independent fixed-capture tests, full classifier
request coverage, model-default retention, fixed/elastic committed-prefix and
cursor refusal controls, protected permission/action tests and image omission
accounting. Native Guardian policy and current-account/source custody still
decide execution. No default policy, reviewer choice or feature activation is
changed by intake.

## Typed evidence caps: adopt `6919e2cd8e`

For opted-in async allocation, cap typed evidence before rendering it as trusted
developer messages. Oversized tool attestations and previous-review fragments
are omitted whole. Skill paths are kept only as complete JSON-rendered entries
that fit both existing rendering and configured entry limits. Previous-review
IDs are retained with the delivered fragments. Omission observations feed the
request notice; host provenance remains the source of trust. Fixed budgeting
retains its existing rendering path.

Require `classifier_entry_caps_preserve_whole_evidence_and_only_sent_review_ids`
and fixed/elastic orchestrator-connector scenarios, including omission notices,
oversized escaped paths and no metadata for omitted reviews. Existing
ALab inherited-history and recorded-command nonexecution controls remain required.

## Reserve real ID costs: adopt `76cf0f45ec`

Before allocation, reserve actual IDs for the largest possible new suffix and
charge their serialized string escaping and field framing to the global budget.
After selecting evidence, assign those same reserved IDs only where missing.
Existing item IDs and committed prefix remain intact. Reservation is conservative:
discarding a native item can merge adjacent user messages, and unused reserved
IDs may remain consumed. These ordinal gaps must remain harmless in native
checkpoint/handoff; reservation does not authorize alias reuse or ID rewriting.

Require `reserves_escaped_ids_for_user_messages_and_native_call_outputs`,
`elastic_fits_ids_without_resetting_committed_history`, fixed exact-boundary and
elastic evidence-reduction cases, plus ALab ID allocation capture/restore controls.
Keep the final complete-input bound and failure-before-history-consumption checks.

## Child CA variables: adopt `d87330d91e`

When a managed MITM bundle exists, supply Deno and Google Cloud SDK CA variables
only if absent. Explicit values, including an empty string, win. These two
child-only variables do not contribute roots to the proxy's own trust bundle.
Native restricted execution grants read access to the recognized generated
bundle; it does not grant reads to an arbitrary overridden CA file.

Snapshot capture removes recognized generated defaults so replay regenerates
them, while preserving a shell startup override. Exec-server also avoids letting
an already-filled generated request default override that startup value. Managed
proxy stripping recognizes these variables only for a generated bundle path.
Nix's existing behavior stays separate. No ALab consumer of the CA variable list
was found; retain the shell-snapshot source-admission hooks.

Require default/explicit/empty override tests, proxy trust-root separation,
generated-versus-custom snapshot replay and restricted generated-bundle reads
with arbitrary-file refusal. Preserve native destination admission and normal
TLS verification; CA defaults do not bypass policy or establish network access.

## Review admission

Validate exact 66-commit coverage, unchanged first 61 decisions, one active
record per commit and direct historical supersession. The next commit,
`f8580db472`, is outside this review and must fail complete coverage. All named
runtime regressions and integration adaptations remain pending. The complete
native gate, certified application build and installed adoption are separate;
the retained procedural-macro certification refusal is unchanged.

Observed validation: the maintained intake validator accepts all 66 exact
commits. Source inventory finds one active decision per covered commit and
valid direct supersession; the first 61 decision objects equal their committed
originals. Extending the range to
`f8580db4725c6a51a5ee2de0980a8b8b0659e143` is rejected as incomplete.
All 58 upstream workflow Python tests passed in 27.243 seconds with managed
Python 3.14, and `git diff --check` passed. No Rust test, application build or
runtime reproduction was run for this intake review.

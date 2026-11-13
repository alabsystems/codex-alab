<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# New frontier preparation — unqualified

Exact base: `c4b13b947e` (full target064b2d9bca review candidate).
New upstream: `39b42aaacdbb55b86528b4294443696985c348ee` (nine more commits).
This separate worktree prepares the new range while the earlier native gate keeps
its source fixed. Both protocol archives have now been canonically regenerated with the leased
15698 compiler. This source is not qualified for publication.

Removal policy was reapplied (52 paths). Contained offline Targo metadata
re-resolved ALab entries. A package/checksum audit retains all1308 upstream sourced
package identities with the existing exact allocative vendor receipt exception.
Metadata is not compilation or qualification.

Semantic adaptations prepared:
- Preserve restored_through_sequence exactly in exec model-context snapshots;
  reject a cutoff past captured sampling progress. Do not treat newly recorded
  calls as inherited/restored. Extend state round-trip and negative-boundary tests.
- Classify InferenceHookDenied as fatal for automatic continuation. Preserve its
  immediate SSE outcome; a capacity-looking provider message cannot turn a typed
  denial into capacity recovery. Extend the retry/stream/classifier matrices.
- Surface post-turn compaction inference-hook refusal through the real error
  lifecycle, retaining the completed answer while stopping automatic fallback.
  Extend isolated steering coverage so queued input cannot erase this refusal.
- Supply RuntimeAuthChanged when destination account assignment marks MCP dirty;
  preserve the destination-generation checks and actual caller admission.

Follow-up review covered the shared Guardian preparation entry point, sandbox
manager/terminal policy, legacy permission-label helpers, MCP invalidation and
cancellation, plugin metadata diagnostics, request-local tracing, and Cargo
resource/schema lookup changes. All nine source decisions are now recorded in
`39b42aaacd-intake.json`; exact range/coverage validation passed. This records
source intake, not successful runtime qualification or review of the older395
commits. Initial bulk diff output was truncated; the decisions use subsequent
focused source inspection rather than treating that truncation as evidence.

Owned Rust files parsed/formatted with Trustfmt skip_children; whitespace checks
passed. Python SDK artifacts were regenerated from the merged JSON schema bundle
with the existing locked codegen environment; the generated file had no additional
drift. Both contract-generation and RPC-method suites passed, and manual typed
validation retained inferenceHookDenied plus optional ALab observation metadata.
This does not validate the JSON bundle against Rust source. Native full-candidate
checks, Rust schema fixtures, focused runtime regressions and source adoption
remain pending.

Next: regenerate stable and experimental archives from merged protocol source,
check fixtures and recheck SDK output if the bundle changes, finish nine-commit intake, commit the merge and
adaptations, then compose into the main review worktree after its live gate ends.
The earlier cloud-config recursion-depth failure still requires pristine
attribution and repair. Keep all non-conflicted owned adapters, not only rerere.

Canonical stable and experimental writers each passed under native15698 compiler
selection in the explicit unverified test lane. Their output changed only the two
precomputed archives; merged text fixtures had no extra drift and no generated
paths were removed. Four ordinary Rust fixture checks still need to run against
the copied archives. The initial395-commit intake and full qualification remain
incomplete.


## Provider boundary qualification

Candidate `8dfe7e9db8` passed the focused native `codex-cloud-config` library
Tippy check with Trust 15698. The concrete reusable-client authentication-selection
future is erased at the model-provider boundary; cancellation, source validation,
telemetry and selection logic remain upstream's. Boxing native preparation and
direct HTTP execution did not resolve the failure and those experiments were
removed. No recursion limit was raised. This focused result does not qualify the
whole candidate. Full gate results remain required.

Both canonical Rust protocol archive writers completed successfully on the
integrated source, followed by fixture checks: four passed, zero failed, one
ignored writer (already invoked explicitly). SDK contract generation and RPC tests
also passed. Later provider and recovery adapters do not change protocol sources.

The 395-commit intake snapshot is now persisted beside this note. It has 63
recorded source decisions and 332 pending entries; coverage validation is expected
to refuse closure until every decision is recorded. This is deliberately a partial
review record, not an all-commit approval or test receipt.


## Reviewer and bootstrap follow-up

The full gate reached app-server and found one remaining owned initializer missing
upstream's new optional `user_id`. Rejected bootstrap login now returns no account
and no user identity; the existing sign-in regression checks both. This adaptation
still requires qualification on the next candidate.

Recovery must retain the requested reviewer separately from a policy-forced effective
reviewer. The serialized thread-settings snapshot already preserves both. Reapplying
the effective reviewer as an explicit turn override would replace the original
preference and break later model switches. The owned recovery adapter now leaves
that thread-owned setting with the restored snapshot. A new runtime regression
covers both requested reviewers across durable and ephemeral handoffs and switches
back to an ordinary model. Before-fix execution reproduced requested `User` becoming `AutoReview`.
After the preference fix, durable scenarios passed, while the ephemeral scenarios
exposed a separate idle-child memory-history ownership problem.

Native diagnostics located the missing history in an idle Guardian child's
preflight. Isolated internal sessions bypass the host lifecycle contributor.
The owned helper inherits only memory-history bookkeeping from the actual parent;
it does not grant root identity or recovery admission. Both production constructors
must call it: the general internal-session factory and Guardian's direct startup
path. Positive and non-exec negative unit controls cover the ownership distinction.
The first hook alone did not resolve the regression; the second constructor was
identified by inspecting every production `InternalSessionParent` construction.

On candidate `c307a95ae98586a34a24d281ef90de11561e4465`, the native Trust 15698
app-server library filter `recovery_options_tests::` passed all five tests,
zero failed (482 filtered out). The new regression completed all four combinations
of durable/ephemeral history and requested User/AutoReview, including switching
back to an ordinary model. Existing settings/admission, compacted history,
ephemeral no-storage, and tree-release regressions also passed.
Execution log: `codex-alignment-recovery-suite-guardian-parent-native.log` in the
session's external evidence directory. This is focused runtime evidence; it does
not substitute for the full candidate gate or qualify newer upstream commits.

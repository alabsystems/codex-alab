<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Systematic upstream alignment

Status: operating design with partial implementation. The merge, daily runner,
delta report, native check and package mechanisms already exist. The intake
snapshot/coverage validator is implemented in `alab/upstream_intake.py`; feature
semantic coverage and gate integration remain. The initial `features.toml`
inventory groups all current hook topics into behavior contracts;
`feature_inventory.py` (Python 3.11+) checks topic ownership and implementation
paths. Protocol/config surfaces and executable per-feature evidence still need
expansion before this can enforce semantic coverage. `upstream_status.py`
reports locally observed source identities and pending commit count/age. It does
not fetch; fetch time, qualification and artifact adoption remain unknown until
evidence from those stages is connected.

Snapshot a fetched range without overwriting an existing review:

```sh
python3 alab/upstream_intake.py snapshot --base origin/main --target upstream/main --record /absolute/path/intake.json
python3 alab/upstream_intake.py validate --base origin/main --target upstream/main --record /absolute/path/intake.json
```

Fill each commit's classes, affected feature IDs, decision, reason and review
evidence; deferred activation also needs a revisit trigger. Validation resolves
base/target again, checks complete ordered coverage and source paths, and rejects
stale decisions. Feature IDs must name contracts in `alab/features.toml`; unknown
IDs are rejected. It does not prove the quality of a review or run qualification.
Keep a completed record in `alab/upstream/` with the integration's evidence.

## Outcome and invariants

Follow official `openai/codex` main continuously, retain intentional ALab behavior,
and make each addition or retirement an evidenced decision. Stable release tags
are reference points, not the source frontier. A new upstream feature is part of
the source merge even when ALab deliberately leaves its activation opt-in.

- Main advances through merges, never rebases or force pushes. Never push upstream.
- Every integration names an exact fetched upstream SHA and exact ALab base SHA.
- Preserve upstream behavior, APIs and fixes; adapt ALab intent to their new shape.
- Preserve ALab features by observable contracts, not marker counts alone.
- Keep discovery, source integration, qualification and installed adoption separate.
  A fetched commit, a passing unit test or a packaged binary proves only its stage.
- No silent omitted commits, disabled tests, weakened permission boundaries or
  claims that a toolchain failure is a successful product check.

## One integration pipeline

| Stage | Operation | Evidence required to advance |
| --- | --- | --- |
| Discover | Fetch main and release tags through `alab/github.py`/Agh; identify commits since integrated frontier | Immutable target, base, ancestry and complete commit range |
| Assess | Classify the range; map changed paths to ALab features, defaults and removed paths | Intake record for every commit; semantic review for affected behavior |
| Integrate | Run `sync.sh --stepwise`; resolve in its worktree; preserve both intents | Merge, adaptations, regenerated artifacts and decision records |
| Qualify | Run existing `check.sh` plus affected feature and upstream-change scenarios | Exact source/compiler identity, commands, results and attributable failures |
| Land | Let `sync.sh --continue` validate and fast-forward origin main | Remote ancestry includes target; candidate rechecked if main moved |
| Adopt | Package exact qualified source, smoke it, promote through existing package tooling | Artifact/source identity, smoke result and selected/running version |
| Reconcile | Compare retained features with new upstream equivalents and report lag | Feature retirements, residual debt and next concrete action |

Continue using `daily.sh` every four hours and its existing lock/journal. A single
integration owner holds the sync lock. An assessment can read objects concurrently;
resolvers must not edit the same candidate concurrently. Persist decisions before
handing off. Resume a live process or recorded sync; never start duplicate builds
because an observation timed out.

A stepwise prefix is useful progress but does not close intake for the full target.
Record the unresolved next commit and continue until the chosen frontier lands.
Do not bypass a conflict by cherry-picking only easy later improvements. An urgent
fix can be backported separately with its upstream SHA recorded, then reconciled
when the ordinary merge reaches it.

## Deliberate intake

Add an ALab-owned intake tool and records under `alab/upstream/`. Each batch record
contains base, fetched target, complete commit list, changed paths, classifications,
affected feature IDs, decisions/reasons, evidence paths and final remote merge SHA.
The tool must detect gaps in coverage and stale decisions after either base changes.
A commit may have several classifications; path matching suggests review, never
proves a semantic decision. Read commit diffs and relevant tests, including changes
outside watched paths. Review release notes for fixes carried only on release branches;
record any applicable backport instead of merging a release leaf into main.

| Change | Default decision | Additional review |
| --- | --- | --- |
| Bug/security fix | Integrate promptly | Regression reproducer; effect on ALab hooks and boundaries |
| New model or capabilities | Make usable promptly for eligible accounts | Catalog, provider, picker, request and execution contract below |
| Upstream feature | Carry source; respect upstream activation and ALab product policy | UX, network/disk/CPU effects, consent and defaults |
| Refactor/API/schema change | Adapt ALab consumers | Protocol fixtures, lifecycle and generated outputs |
| New default, background task, telemetry or prompt | Explicit behavior decision | Effective ALab defaults and real runtime effects |
| Equivalent to an ALab feature | Prefer upstream after parity is proved | Remove redundant hook, implementation and tests tied only to old internals |
| Removed or watched path | Reapply removal policy; inspect upstream change | Dependencies and useful behavior hidden by removal |

Decisions are `adopt`, `adapt`, `keep-opt-in`, `superseded`, or `defer-activation`,
with reasons and validation. Source integration itself is not optional per feature.
Deferred activation names a concrete dependency and revisit trigger. It must not
become an undocumented permanent fork or a pretext for holding unrelated fixes.
Routine reversible decisions are autonomous; explicit human holds remain binding.

## New-model readiness

Treat model availability as both a service event and a source event. A new model
may arrive in the server catalog before a new CLI release; merging main alone is
not proof it is usable. Conversely, source support does not grant account access.

At every intake, and when a catalog change is observed:

1. Compare a fresh catalog for the intended provider/account with the previous
   observation. Record model IDs and capability metadata without credentials.
2. Inspect upstream catalog/fallback changes, model migrations, provider routing,
   reasoning options, context/compaction limits, tools, modalities and pricing
   metadata where the client uses it. Avoid a duplicate hardcoded ALab catalog.
3. Check that ALab config filters, aliases, cached catalog state, defaults and picker
   logic expose the model and preserve a person's explicit model selection.
4. Verify discovery and selection through the actual CLI/app-server/TUI paths;
   verify a request uses the exact selected ID and supported settings. Exercise
   the capabilities that require changed client behavior and relevant recovery.
5. Use deterministic catalog/request fixtures for repeatability and a bounded live
   smoke for an eligible account when available. Record service access failures as
   unverified availability, not client regressions or permission to use another ID.

Availability and default selection are separate decisions. Expose supported models
promptly; change ALab's default only after capability, quality, latency, cost and
recovery evidence supports it. Catalog reads use existing authentication and refresh
machinery; do not add an independent polling service or copy secrets into records.

## Feature ownership and modularity

Introduce `alab/features.toml`, one entry per intentional behavior. Each entry has
an ID, purpose, observable invariants, ALab implementation paths, upstream hook or
extension paths, config/default keys, protocol surface, test commands, upstream
equivalence criteria and lifecycle (`active`, `retiring`, `retired`). Include current
features, not just new work: account isolation, quota/capacity recovery, autopilot,
aterm integration, auth locking, defaults, native update/package behavior and UX.

Prefer upstream configuration/protocol/extensions, then a small marked adapter,
then implementation in an `alab-*` crate. Keep upstream-specific types in glue;
ALab state machines and policy consume small owned inputs and return explicit
results. Avoid copying an upstream subsystem merely to remove a hook: duplicated
behavior drifts even when it has no textual merge conflicts. Never create a broad
plugin framework without concrete consumers.

Use `delta.sh` before and after changes. Rank extraction by repeated semantic
conflicts, high-churn patched functions and duplicated policy, rather than line
count alone. Two manual resolutions in the same file in a month trigger a design
review. Extraction is a separate topic commit with unchanged feature contracts.
Upstream equivalence triggers deletion after parity tests, including failure and
cancellation behavior, prove the replacement. Record intentional hook loss before
using `ALAB_ALLOW_HOOK_LOSS=1`; marker survival is only a structural check.

Generated protocol archives must represent merged source, not an arbitrary choice
of ours/theirs. Regenerate with the canonical generator and verify fixtures. If
archives cannot be regenerated in the admitted lane, keep the integration pending.
Pin ALab dependency requirements in owned manifests; retain upstream lock entries.

## Validation and reporting

Keep `check.sh` authoritative and fail closed. Add feature coverage around it,
rather than replacing it with a smaller gate. For every affected feature, run its
contract scenarios, including permission refusal, cancellation, restart and account
isolation where relevant. Run focused upstream regression/integration scenarios
for the actual change even though upstream's full CI is not run here. Compare
failures with pristine upstream using the same compiler and scenario; record
shared failures and their effect on confidence. A shared failure is not a pass.

Add a status report with fetched/integrated/qualified/packaged/selected/running
SHAs, fetch time, pending commit count and oldest pending age, current blocker,
feature coverage gaps, and links to intake and qualification evidence. Distinguish
"unknown" from "zero". Check installed identity explicitly; main landing does not
mean a running session adopted it. Retain last known good package for rollback;
rollback selects that artifact and never rewrites source history.

Target: discover within four hours while the runner is available and integrate
within a day when qualification permits. Measure actual lag and conflict causes;
these targets never authorize skipping a gate. Record unavailable resources and
advance independent assessment or modularization work without routine owner nags.

## Implementation order and acceptance

1. Inventory existing ALab feature contracts and map hooks to feature IDs. Acceptance:
   every current hook/default/removal has an owning feature or documented policy.
2. Build intake ledger and status reporting on existing sync state. Acceptance:
   fixture ranges detect missing decisions, stale bases and partially landed batches.
3. Add model readiness scenarios and high-risk feature contracts. Acceptance:
   catalog-only launch, source-dependent launch, stale cache, explicit selection and
   unavailable account cases produce distinct correct results.
4. Connect intake/coverage checks to sync qualification; preserve current gate.
   Acceptance: a test sync cannot land with an unreviewed range or missing affected
   contract evidence; interrupted work resumes without losing decisions.
5. Complete current upstream backlog, measure conflict hotspots, extract the worst
   adapters, and prove a second consecutive sync with the same workflow. Acceptance:
   remote main contains the assessed upstream frontier, retained features pass,
   artifacts identify their source, and no selected behavior is silently lost.

Current observation (2026-11-09 Pacific): setup restored the fetch-only upstream
remote and trained rerere. Target `064b2d9bca2b5ba2f9f399ffefd06a75d97852f0`
is 395 commits beyond origin main `1ad8b4f8ec`. The stepwise sync is pending at
`053e5b1ca9`, with two precomputed schema archives conflicted. No source merge or
qualification has landed for this batch. Re-query these facts before execution.

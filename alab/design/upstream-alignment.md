<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Systematic upstream alignment

Status: operating design with partial implementation. The merge, daily runner,
delta report, native check and package mechanisms already exist. The intake
snapshot/coverage validator is implemented in `alab/upstream_intake.py`.
`alab/upstream_gate.py` admits committed review coverage in `check.sh`, before
compiler commands, against the integrated frontier and current `origin/main`.
`feature_evidence.py` checks explicit scenario-to-executed-test declarations.
`feature_gate.py` requires complete exact-source bundles for the affected features
declared in committed reviews before `check.sh` starts compiler commands.
Semantic review and complete scenario observations remain. The initial `features.toml`
inventory groups all current hook topics into behavior contracts;
`feature_inventory.py` (Python 3.11+) checks topic ownership and implementation
paths at one resolved Git commit. `--source REV` selects it (default `HEAD`);
the catalog, hooks and owned paths all come from that commit, whose ID is printed
in the result. Working-tree and staged edits do not enter this report.
The shared `hook_topics.py` extractor supplies exact topic identities to the
validator and formats `delta.sh --topics` separately. Whitespace and binary
attributes cannot hide a marker; control characters are escaped for display.
Inventory and intake/evidence record headers require an integer schema version;
boolean and floating-point values are not alternate spellings of version `1`.
Protocol/config surfaces and executable per-feature evidence still need
expansion; declared impact scope is not automatic semantic coverage. `upstream_status.py`
reports locally observed source identities and pending commit count/age. Optional
explicit artifact/executable/PID inputs distinguish packaged, selected and running
manifest identities. `qualification.py` retains the existing native gate's output and
unchanged clean source identities; an explicit record lets the status report identify
that qualified source. It does not fetch; fetch time remains unknown without an
independent observation.

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

Admission reads records and feature IDs from the candidate's Git objects, so an
uncommitted review cannot authorize a sync. Stepwise prefixes may use a larger
review batch, while a newly integrated unreviewed commit still fails. Duplicate
active coverage is refused. A historical partial record can name an active
committed `superseded_by` path and a `supersession_reason`; that replacement must
cover every relevant commit and independently validate. A pending or incomplete
replacement never fills a missing review. Hook-loss exceptions and
`ALAB_CHECK_BASE` do not change this admission base or bypass it.

The gate and packager share TOML-capable Python discovery before native project
selection. An explicit `ALAB_PYTHON` must be Python 3.11 or newer; otherwise the
existing managed/uv/PATH candidates are probed. This avoids using macOS's older
system Python for a scheduled sync's intake check.

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

Expose supported models promptly. Sol is ALab's default family. A conversation
following Sol, Astra or another versioned first-party family automatically selects
its newest eligible release on startup, resume or catalog refresh. Verify the next
turn uses that release with the saved provider and reasoning effort. Explicit model
pins stay exact.
Bedrock's built-in catalogs and Vertex's built-in catalog can also advance a
versioned OpenAI model. Select from the saved provider's catalog and retain
the full route, including Bedrock Runtime's routing prefix. For custom
providers and explicitly configured catalogs, follow their declared successors. Review
capability, quality, latency, cost and recovery when changing the default family.
Use existing catalog authentication and refresh machinery. Keep credentials out
of intake records.

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

The `d4b19eca27` integration puts core session startup, auth manager and TUI
event dispatch at the top of `delta.sh`'s source conflict ranking. Prioritize
those seams when the next change touches them. For startup, keep the upstream
startup sequence in place and put ALab's phase construction and credential
lifetime handling in its assignment glue. Check that deferred initialization
keeps both its source lease and its bounded polling stack. For auth, keep the
factory bound to the credential read that selected it. Move policy decisions
into the existing ALab owner while retaining the native auth manager's lock
and invalidation boundaries. For event dispatch, keep one arm per native event
and route ALab state transitions through the existing session and widget
adapters. Check request identity before accepting asynchronous replies.

Regenerate protocol fixtures from the merged source whenever the merge changes
schema inputs or conflicts in generated exports. In the admitted native profile,
run the upstream fixture writer from `codex-rs/` with a fresh, writable scratch
directory. The stable run writes the JSON and TypeScript trees and the stable
archive. The experimental run writes the experimental archive to the same root.

```sh
export CODEX_APP_SERVER_SCHEMA_ROOT="$schema_scratch"
for schema_mode in 0 1; do
    CODEX_APP_SERVER_SCHEMA_EXPERIMENTAL="$schema_mode" \
        alab_build_command "$ALAB_CARGO" --unverified test --locked \
        -p codex-app-server-protocol --lib \
        schema_fixtures_tests::write_schema_fixtures_from_env -- --exact --ignored
done
```

For a conflict in either precomputed archive, compare the regenerated file
with the merge base and both parents while Git still has all three index
stages. Run this from the conflicted worktree, using the affected archive name.

```sh
python3 -IB alab/protocol_export_merge.py "$schema_scratch/precomputed/app-server-exports-experimental.json.zst"
```

The report compares TypeScript text and both sets of JSON schemas. If both
parents changed the same value, review the reported path against the merged
Rust source. Compare and stage the changed generated fixtures, including deletions.
Regenerate the Python SDK from the stable JSON tree using the same frozen
`uv` test group and `generate-types` command as
`codex-rs/app-server-protocol/scripts/write_schema_fixtures.py`.
Verify `schema_fixtures_tests::` against the staged files and record the
generator source and outputs with the integration. Keep the integration pending
if generation or fixture parity fails. Pin ALab dependency requirements in owned
manifests and retain upstream lock entries.

## Validation and reporting

A `collect-part --scenario` invocation runs the exact Python wire tests named
in those scenarios. The collector checks the protected process arguments and
both the discovered and executed test identities. This lets independently
declared scenarios finish when a different wire test fails. A scenario that
requires the failed test remains open. Running the full feature still runs
the entire fixture module.

Focused native controllers should source `alab/focused-tests.sh` inside the
already-admitted native profile, then call the shared capture helper:

```sh
alab_run_focused 1 alab_build_command "$ALAB_CARGO" --unverified test --locked -p <package> --lib -- --show-output
```

The caller supplies an existing absolute `CARGO_TARGET_DIR` and managed
`ALAB_PYTHON`. The helper captures stdout and stderr together in a unique target
file, preserves command or capture-pipeline failures, then invokes the checker.
It grants no execution lane and does not replace native admission. Reported
cloud-fixture skips must fail qualification even when libtest prints a pass.
Use `--show-output` to retain successful tests' diagnostics, including self-skip
messages, while reducing interleaving with libtest's completion lines.
Default capture hides successful self-skips; parallel `--nocapture` output can
splice a completion line into another test's diagnostics. Keep strict named
result validation when output remains ambiguous rather than inferring a pass.
For older stdout-only captures, recheck the entire completed combined outer log
with the real terminal status before crediting its results.

When combining filters, require each one to match a passed test so a misspelled
module cannot borrow another filter's successful result:

```sh
alab_run_focused 1 --require-filter cloud_backend::tests --require-filter cloud_backend::idle -- \
    alab_build_command "$ALAB_CARGO" --unverified test --locked -p codex-app-server-client --lib -- \
    cloud_backend::tests cloud_backend::idle --show-output
```

Guard options and their nonempty, non-option values must precede `--`; only the
command and its arguments follow that boundary. Repeat `--require-test <full-emitted-name>` when particular tests must
pass exactly once, including any generated case suffix. Both guards check only
named passes within completed suites and reject inconsistent named counts or
duplicate identities. The original count-only invocation remains available.

Focused native runs can also check captured combined libtest output with
`python3 alab/focused_results.py --log <combined-output> --status <actual-exit-status>`.
The checker accepts the same repeatable `--require-filter` and `--require-test`
options without running another command.
Use `--minimum-suites` when selecting several targets. This rejects empty,
incomplete, failed, ignored or explicitly skipped selections. Capture the actual
test/pipeline status before invoking it; a checker success cannot replace that
status. Keep the checker in the admitted repository when running under the native
protected profile. This count check supplies neither source provenance nor
semantic contract coverage and does not replace `check.sh` or `unit_results.py`.

Snapshot all active and retiring contracts from the exact committed source:

```sh
python3 alab/feature_evidence.py snapshot --source <candidate-sha> --upstream <upstream-sha> --record /absolute/path/features.json
python3 alab/feature_evidence.py validate --source <candidate-sha> --upstream <upstream-sha> --record /absolute/path/features.json
```

A snapshot starts pending and refuses to overwrite an existing record. Each
scenario needs one or more `checks`, with the exact `source`, actual integer
command `status`, `command` argument list, absolute captured `log` path,
`log_sha256`, fully qualified executed `tests`, and a `reason` explaining the
semantic mapping. A single test may cover several scenarios when each mapping
is justified. Broad scenarios may require several checks; never fill a broad
contract with an unrelated passing test merely to satisfy the schema.

Rust checks use `format = "libtest"` (the default). Python workflow tests can
produce named results directly through the owned unittest runner:

```sh
python3 -I alab/python_test_results.py --start-directory alab/tests --pattern test_upstream_gate.py --record /absolute/path/workflow-results.json
```

Capture its actual exit status and set the observation's `format` to
`python-unittest`, `log` to that JSON file, and `tests` to the executed unittest
IDs in its `tests` array. The record retains Python-level test output; native
child processes may also write to the invoking command's captured streams.
Retain those command streams with the execution evidence when relevant.
The runner refuses an existing record before executing tests. Its result is
unsuccessful for empty selections, duplicate IDs, skips (including subtests and
class fixtures), expected failures, unexpected successes and failed tests.
Validation checks named completion and counts. A Rust-looking console summary
cannot substitute for a Python record; an unknown result format is refused.
The runner and format provide checked execution declarations, not authenticated
source provenance or proof that a test covers the declared scenario.

Validation binds the ordered feature/scenario inventory and its digest to the
source, requires that source to contain the declared upstream, checks log bytes,
and rejects missing, duplicate, failed, ignored, skipped or incomplete selected
tests. It reads the inventory from Git, not an uncommitted working copy. These
are checked declarations: the caller must capture the real command status and
source during execution. A record is not an authenticated execution receipt, and
the tool cannot infer semantic coverage or live entitlement from test names.
Existing focused observations at a different source cannot simply be relabeled.
An affected feature can opt in to mandatory Rust test groups through
`required_libtest_groups` in its committed inventory. Final validation uses
that candidate's recipes to resolve the exact test names for each scenario
that selects those groups. For the specific-refusal scenario, provider recovery
requires the live refusal test and all four replay tests. This check applies
to cached and imported evidence as well as a fresh run. Independent captures
can supply different required names for the same scenario. Partial records
can contribute a subset, and their union must contain every required name
with a passing libtest result before the complete bundle qualifies.
Historical candidates are judged against the contract in their own source.

The collector maps affected scenarios to named tests in
[`feature_recipes/scenarios.json`](../feature_recipes/scenarios.json). From the
checkout that owns a pending sync, inspect the requested features and run their tests.

```sh
python3 alab/feature_campaign.py request --repo .git/alab-sync
python3 alab/feature_campaign.py run --repo .git/alab-sync
```

For an upstream transport review, run either protected check directly. Set
`candidate_worktree` to the absolute path of the clean candidate checkout and
`output` to a fresh absolute directory for these receipts.

```sh
candidate=$(git -C "$candidate_worktree" rev-parse HEAD)
mkdir -p "$output/code-mode/artifacts" "$output/connector/artifacts"
alab/feature_recipes/run.sh outer "$candidate_worktree" "$candidate" \
  "$output/code-mode/artifacts" code-mode-stdio >"$output/code-mode/run.log" 2>&1
alab/feature_recipes/run.sh outer "$candidate_worktree" "$candidate" \
  "$output/connector/artifacts" connector-logging \
  connector_results_reach_websocket_without_payload_logs >"$output/connector/run.log" 2>&1
```

`code-mode-stdio` runs the process-owned host's gRPC session test and retains
the matching CLI and host under `artifacts/bin`, with their Cargo metadata
and artifact messages under `artifacts/helpers`. It selects the compiled
integration test from Cargo's messages and launches it with that retained host.
`connector-logging` requires
one exact test name from the `codex-api` WebSocket logging integration target.
Both checks run on request and leave frozen campaign coverage to its declared
scenario recipes.

The scheduled sync runs the native gate first. When its recorded failure requests
a missing feature bundle, the sync runs the matching recipes. If every scenario
has a recipe and the checks pass, it submits the same candidate to the full
native gate once more. Both gate runs retain independent records. If a feature
has no recipe or a scenario has an empty group list, the sync collects the
runnable scenarios across all affected features, reports the missing scenario
numbers and exits 1. The resulting part can be imported once coverage is
complete.

The collector verifies the native gate record and its log digest, freezes the
recorded source, base and integrated upstream, and checks the scenario text
against its recipes. A changed scenario contract stops collection before any
build. The normal source-selected compiler, cache registrar and guarded runtime
execute the tests. Candidate Python runs under the same protected profile as
the native tests. It writes its named results to the declared artifact directory
and uses the profile's temporary scratch for fixtures. The outer command guardian
records its completion outside the writable artifact directory. One package
test owns a narrower native profile. The reviewed host fixture only accepts
identical package and profile controllers, then runs the candidate's dispatcher
and policy inside that profile and checks the kernel's read and write decisions.
A changed controller requires review. A managed detached worktree keeps its
native target warm.
`--workspace` can reuse a dedicated, clean detached worktree and fast-forward
it to the requested source. Pass an independently collected `--part` to
either command to validate its original scope and subtract its exact named
tests from the work still needed. A name shared across stages requires a
fresh execution. The existing part's declarations stay on their original
scenarios.

When one affected feature is ready before the others, collect its evidence
against the recorded native failure. For example, run the skill-discovery
recipe and retain the resulting part path.

```sh
python3 alab/feature_campaign.py collect-part --repo "$candidate_worktree" \
  --record "$native_record" --feature skill-discovery
```

The JSON result names the file in `part`. The file keeps the entire frozen
inventory and fills only the selected feature's checks. Receipts and captured
logs remain in the local campaign directory. A later complete `run --part`
against the same source, upstream and base can reuse those executions. The
scheduled sync still requires every affected feature before compilation.

A stopped single-feature run may have finished every test for some of its
scenarios. Recover those rows using the original clean producer and candidate
checkouts, the native gate record, and the exact campaign directory reported
on stderr.

```sh
python3 alab/feature_campaign.py recover-part --repo "$candidate_worktree" \
  --record "$native_record" --from-run "$stopped_run" --producer "$collector_worktree" \
  --output "$evidence_dir/recovered-part.json"
```

The result lists the recovered and unresolved scenario numbers and returns the
stopped stage's nonzero exit status. The output is created only when an entire
scenario satisfies the candidate and producer selections. The candidate and its
original execution workspace must retain the recorded source. The original
collector path may be in a separate clean clone. It must contain the exact
recorded recipe at the candidate commit or a revision published to private main.
Recovery compares each prospective row's full stage and test selection with the
candidate's registration. Rows with a different or missing registration remain
unresolved while other complete rows can be exported. Recovery checks the recorded commands, completion receipts
and named test logs without executing a stage. Use the recovered file with the
usual `--part` or portable-progress commands. A missing completion receipt or a
signal requires fresh execution.

A partially configured feature can collect individual runnable scenarios.
Number them from one in their frozen `features.toml` order. For example, to
collect the first and fourth scenarios when their recipes are ready, run

```sh
python3 alab/feature_campaign.py collect-part --repo "$candidate_worktree" \
  --record "$native_record" --feature "$feature_id" --scenario 1 --scenario 4
```

The recipe must list every scenario in source order. An empty `groups` list
marks a pending scenario. Selecting one fails before execution, and `request`
lists the feature as unsupported for a complete run. Each selection gets its
own receipt directory and fills only its numbered rows in the part. Repeating
the same selection validates those receipts. Once every affected feature has
complete recipes, repeat `--part` on the full run to import independent
observations and execute the remaining checks.

Use the same cross-feature selection as the scheduled sync to collect a single
part directly. Shared stages run once for all selected scenarios. The request
reports unimplemented rows under `scenarios_without_recipes`, using the same
one-based numbers as `--scenario`. Running the command again validates the
previous stage receipts against the same source, base and recipes.

```sh
python3 alab/feature_campaign.py collect-ready --repo "$candidate_worktree" \
  --record "$native_record"
```

To count the scenarios covered by local campaign parts, use the source,
upstream frontier and base from their recorded gate.

```sh
python3 alab/feature_evidence.py progress --repo "$candidate_worktree" \
  --source "$candidate" --upstream "$upstream" --base "$base" \
  --record "$first_part" --part "$second_part"
```

Add `--summary` to see the candidate, how many ALab release scenarios have
verified evidence, and which scenario numbers still need a result.
The JSON output gives the total and per-feature counts, plus the number and
description of every pending scenario. A row needs a verified observation and
every typed test name required by the frozen inventory. Repeated parts count
each row once. Invalid captures fail the command. For a portable primary record,
pass its bundle directory with `--log-root`. Additional `--part` records use
their own absolute log paths. Omit `--base` for an unscoped inventory.
This read-only report grants no release qualification.

To preserve a partial run for review on a different checkout or machine,
use `export-progress` with the same records and a fresh destination.
It copies the verified captures under their content hashes and writes
`progress-record.json` after it rechecks them. Move the whole directory
together. A checkout containing the three commits can reproduce the
original counts after the old log paths disappear.

```sh
python3 alab/feature_evidence.py export-progress --source "$candidate" \
  --upstream "$upstream" --base "$base" --record "$first_part" \
  --part "$second_part" --destination "$archive"

python3 alab/feature_evidence.py progress --source "$candidate" \
  --upstream "$upstream" --base "$base" --log-root "$archive" \
  --record "$archive/progress-record.json"
```

A later `export-progress` can take this portable record first with its
`--log-root` and add new local parts for the same source. Release bundle
publication continues to require complete coverage.

Results live under the common Git directory in `alab-feature-campaign/`.
Rerunning validates completed stage receipts and named test logs. The runner
records each precompiler cache refusal and can retry it twice. A test failure
stops the run with its actual status. After changing an execution condition,
`--new-run` creates a separate run and preserves earlier attempts. A completed
campaign atomically installs a validated bundle and invokes the ordinary
feature gate. Its receipt covers the frozen upstream review. Native
certification, signing, publication and running-binary adoption still have
their own gates.

A feature-stage leader becomes the authenticated `atpkg-publish toolchain run`
process. On cancellation the publisher signals its original controller and
waits for its native cohort to drain. The surviving campaign custodian holds
the workspace lock through that wait, including when the collector dies.
A cooperating stage may finish work already in flight before it returns 143.

A forced loss of the native publisher itself removes the original cohort
observer. The public toolchain interface exposes no way to reattach to that
run. Retain the attempted log and receipt, keep that worktree at its recorded
source until its original owners settle, and use a fresh detached worktree
for independent qualification. A changed log digest invalidates the attempt.
[Cancellation decision and evidence](../reviews/feature-stage-cancellation-2026-11-18.md)

For portable retention, export a completed local record and its log bytes:

```sh
python3 alab/feature_evidence.py bundle --source <candidate-sha> --upstream <upstream-sha> --record /absolute/path/features.json --destination /absolute/path/new-bundle
python3 alab/feature_evidence.py validate --source <candidate-sha> --upstream <upstream-sha> --record /absolute/path/new-bundle/record.json --log-root /absolute/path/new-bundle
```

Export validates the local record, copies logs by SHA-256, and validates the
exported record before completing. It requires a new destination and leaves the
input record unchanged. Identical logs share one file. The bundle can move as a
unit without its original logs; validation requires exact `logs/<sha256>.log`
paths and refuses symlinks, escaping paths, missing files and changed bytes.
Export runs no tests and provides no new execution or semantic authority.

Independent workers can fill in copies of the same snapshot. Pass the first
copy as `--record` and each additional copy as `--part` to `bundle`. All copies
use the same candidate, upstream, inventory and review base, and leave unexecuted
scenarios with an empty `checks` list. Export verifies each observation against
its captured log, retains distinct checks for the same scenario and writes the
bundle once the combined record covers every required scenario.

```sh
python3 alab/feature_evidence.py bundle --source "$candidate" --upstream "$upstream" \
  --base "$base" --record "$build_checks" --part "$provider_checks" --destination "$bundle"
```

For an upstream integration, `--base <pre-sync-main-sha>` derives the affected
feature set from complete committed intake reviews. Use the same base for
snapshot, validation and export. The record binds that resolved base as well as
the candidate, upstream and full inventory digest. Every scenario of each affected
active or retiring feature remains required; callers cannot select individual
features or scenarios. Missing, duplicate or invalid reviews refuse scoping, and
an affected retired contract requires reconciliation rather than silent omission.
An already integrated prefix contributes no new affected features. This scope
describes declared upstream impacts; it does not assess independent ALab changes
or prove that a review identified every behavioral interaction.

Shared pristine failures are retained separately by `check.sh`; they cannot fill
a scenario as passed.

### Sync admission and resuming evidence collection

For newly integrated upstream commits, `check.sh` first admits committed review
coverage and then checks every scenario of the declared affected features. It
refuses missing, partial, changed or stale evidence before its first compiler
command. A review that declares no affected features requires no scenario bundle;
reviewers must still assess behavioral interactions. All existing native build,
lint, owned-crate and attributed upstream unit checks run after admission.

The default bundle is `<absolute-git-common-dir>/alab-feature-evidence/<candidate-sha>`.
This is shared by linked worktrees and stays outside the source commit, avoiding
a receipt commit changing the very source the tests observed. Keep the candidate
unchanged while executing checks. Snapshot with the exact pre-sync main as `--base`,
record actual commands/statuses/logs and complete all affected scenarios, then
export with `feature_evidence.py bundle` to that default directory. Run
`alab/sync.sh --continue` to retry qualification. Admission reads and rechecks the
same bundle after interruption; a partial export never authorizes compilation.
An adaptation or a changed integration base requires new matching evidence.

If a retained bundle lacks a required case, keep it for review and capture the
missing tests at the same candidate, upstream and base. Use the `bundle` command
above with the original local observations, any additional `--part` records,
and a fresh absolute destination in `$bundle`. Select that completed bundle
when resuming the sync.

```sh
ALAB_FEATURE_BUNDLE="$bundle" alab/sync.sh --continue
```

`ALAB_FEATURE_BUNDLE` can name an absolute retained bundle elsewhere; the native
gate adds that directory as read-only access in its existing protected profile.
An override selects storage, never the feature set, expected source, inventory
or base. Direct inspection uses `feature_gate.py --base <pre-sync-main-sha>` with
the same `--repo`, `--source` and `--upstream` arguments as qualification, plus
`--bundle-root` when inspecting an explicitly relocated bundle. Bundle root and
record symlinks are refused. Nothing here authenticates a caller's execution
declaration, proves semantic mapping or bypasses native source qualification.

Keep `check.sh` authoritative and fail closed. Add feature coverage around it,
rather than replacing it with a smaller gate. For every affected feature, run its
contract scenarios, including permission refusal, cancellation, restart and account
isolation where relevant. Run focused upstream regression/integration scenarios
for the actual change even though upstream's full CI is not run here. Compare
failures with pristine upstream using the same compiler and scenario; record
shared failures and their effect on confidence. A shared failure is not a pass.

`upstream_status.py` extends the existing lag/oldest-pending-age report with
optional source/intake observations under its `assessment` field. Without
`--base`, the original local lag report remains available.
Pass explicit batch records rather than globbing archival or overlapping ledgers:

```sh
python3 -I alab/upstream_status.py --base <integration-base> --upstream upstream/main --candidate HEAD --main origin/main --intake <first-batch.json> --intake <next-batch.json>
```

It re-derives each batch from Git, rejects stale immutable coverage, unknown feature
decisions and overlapping records, and reports missing commits against the complete
requested range. Pending decisions stay pending. A successful observation can report
incomplete work; its exit status is not qualification admission. Local origin/upstream
tracking refs are explicitly cached observations, not a fresh remote check. Candidate
ancestry, review completion, qualification and installed adoption remain distinct.
The `assessment` field never infers qualification or adoption from a branch or
JSON decision record.

Inspect explicit local artifact and process inputs independently:

```sh
python3 -I alab/upstream_status.py --package /absolute/path/artifact --selected-executable /absolute/registered-executable-pointer --running-pid <supervised-pid>
```

The artifact's `alab-build.json` identifies `packaged_sha`; the resolved executable
pointer independently identifies `selected_sha`. On macOS, the running observation
checks the PID's start time around inspection and matches the loaded Codex image's
inode to its artifact before reading that artifact's manifest for `running_sha`.
A newly selected package never supplies an older process's identity. The observer
executes no artifact, selects no package and changes no process. Source identities
come from packaged manifests, not authenticated source qualification. An absent,
unsupported, ambiguous, changed or unreadable input stays unknown with an error;
the remaining source/lag and review observations still run. Stages without explicit
inputs remain unknown. Fetch freshness, qualification, promotion and live model
entitlement are not inferred from any of these inputs.
The observer resolves the supplied package and executable pointers again after
reading their manifests. If atpkg switches a pointer during that read, the
affected stage reports the change. A fresh read can then identify the new build.
Use the package manager's registered executable pointer, which may differ from
its dispatcher on `PATH`. An opaque command shim without a directly associated
artifact manifest stays unknown; the observer never runs it to guess its target.

Extend this report with fetched/integrated/qualified/packaged/selected/running
SHAs, fetch time, pending commit count and oldest pending age, current blocker,
feature coverage gaps, and links to intake and qualification evidence. Distinguish
"unknown" from "zero". To qualify a live update, start a conversation on the
previous signed build, observe it adopt the new signed executable, and verify the
next request succeeds on the same thread with its earlier history. Retain the
last known good package for rollback, which selects that artifact and preserves
source history.

`sync.sh` runs `check.sh` through `qualification.py` and retains each attempt under
`<git-common-dir>/alab-qualification/<source>/run-*`. The recorder preserves the
native gate and its exit status, captures its combined output, and compares source,
tree, origin base, integrated upstream and clean Git state before and after execution.
Only a successful unchanged-source attempt supplies a qualified source. Pass its
`record.json` through `upstream_status.py --qualification-record`; the reader checks
the source tree and sibling log hash. Failed attempts remain available for diagnosis.
These are local execution observations, not authenticated attestations or evidence
of artifact selection, running adoption or live account entitlement.

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

Inspect current source lag with `python3 alab/upstream_status.py --repo <clone>`
and interrupted sync work with `alab/sync.sh --status`. These are local observations;
refresh upstream through the existing Agh transport before choosing a new target.
Read exact-source intake records in `alab/upstream/` and qualification receipts
separately. A completed focused selection, reviewed prefix or integrated source
frontier does not imply the full gate passed or an installed binary adopted it.

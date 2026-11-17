<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Terminal WebSocket provider correlation — 2026-11-16

## Decision and reason

Complete one deliberate family of selected provider-error correlation instead of
adding another isolated type to the existing three-type allowlist. For example,
`response.failed` with `inference_hook_denied` and valid nested request IDs
already returned the correct terminal error but omitted those available IDs.

The owned selector now makes an exhaustive choice for all 26 current `ApiError`
variants: 15 terminal provider-error variants are eligible, and 11 other variants
remain excluded. `ModelNotFound` and `ContentPolicyViolation` additionally require
no HTTP status; their status-bearing forms remain excluded. Future enum additions
require another explicit metadata-ownership decision at compile time.

The provenance argument is specific to this family: the WebSocket loop observes
the current text before decoding and returns parser errors immediately, while
the observer replaces or clears its candidate on every text frame. This does not
establish ownership of a late provider frame. Wrapped HTTP errors retain their
own metadata; generic stream/socket, incomplete-response and advice-slot errors
do not gain this fallback. `ResponseIncomplete` is also protocol-Never but is
outside the provider-error-frame family, so eligibility is not all Never errors.

The predicate only matches types and optional status. It neither parses input
nor calls a bridge, retry policy or clock. Existing error values, advice,
classification, pacing, standalone admission and recovery policy are preserved.
The incident guide now states the complete scope, including absent-advice
exclusions. No upstream-owned source, dependency, lockfile or compiler pin changed.

## Reproduction and passing checks

Tests-only source `43bf0497b6d638a12a677bccb81fe2c7e37b68c0` retained the old
product code. Two exact public WebSocket methods failed at their first Nested
request-ID assertion: `inference_hook_denied` and `context_length_exceeded`.
Each child first established the exact typed error and relevant payload, mapped
message and Never/no-advice/no-fallback policy, channel closure and joined fixture,
caller context, failure kind and privacy. Both static pre-ID markers were observed,
then `None` differed from the available request ID. Public-library results were zero passed,
two failed and 99 filtered, with actual unit/controller status 101. The two child
failures, each filtering 100 methods, are not added to the outer count. Later rows
and the following ray assertion were unreached in this failing phase.

Fixed source `a0233d680103ced6d6126f123ab177f7957714d4` passed both complete
libraries: `codex-alab-api-tests` 101 and `codex-api` 283, totaling 384 methods.
Failed, ignored, measured and filtered counts were zero. Scoped all-target Tippy
passed with zero owned, spanning, failing, missing-script and set-aside findings.
Unit, lint and outer controller statuses were zero.

The public source is byte-identical between reproduction and passing execution.
The two new methods enumerate 12 newly eligible categories across 10 rows each:
108 typed refusals and 12 completion controls. Original payloads, status omission,
terminal policy, privacy and selected-frame context remain asserted. Six added
independent-token cases strengthen the older request/prompt/audio matrices:
request-only, ray-only, split precedence and one-invalid/other-valid expectations
are now independently expressible. This closes a coverage gap without a separate
production-defect claim for those older paths.

Two observable unit methods add 12 advice rows and two status-bearing rows.
They preserve returned variant, message, advice value and status, and verify
candidate IDs/private fields are omitted. Existing Stream, HTTP-priority and
completion controls also pass. These rows do not newly exercise every excluded
variant, and value equality alone is not a runtime proof of no clock reads.

## Lane, custody and limits

The prescribed native project frontend selected committed Trust 15698. Maintained
`unit_tests --record-results` used the explicit unverified library-test lane, one
round and one test thread; reproduction selected exactly two full identities,
then passing execution selected both complete libraries. Canonical `target/tippy`
covered normal/test profiles with 657 artifact paths observed. Native GUI memory
and timed reservations were waited out without changing claims, pools or targets.

Source and target remained frozen through live capture. The corrected comparison
captures two libraries plus the native unit runner in both phases.
Compiler digests, prepared/current file identities, launch/CWD receipts and
diagnostic directory identities are retained. Historical executable paths are
not rehashed after rebuilding. Strict child completion and full-library success
cover the declared loops; these counts are not a complete wire-frame inventory.

This qualifies two API libraries with local fixtures, not a full Core/Autopilot/TUI
actor, every feature context or a packaged application. No application was
certified, packaged, installed or adopted. The unavailable application-certification
prerequisite and existing holds remain unchanged. The original opaque HTTP400
backend cause remains unknown. Native evidence and independent source reviews
are retained in the private `ws-terminal-error-ids-20261116` validation stage.

## Composed-tree verification

Remote main advanced to `08d0e3934a7c50bc172630ffa1f1b4fd12e1aa25` during review.
All 20 incoming paths were merged byte-for-byte, without overlap with this topic.
Two Core-included glue paths changed inside the conservatively protected
`alab-core` directory. They are outside the selected API compilation graph, but
the directory guard was retained and the API checks were rerun on the composition.
The initial source's result was not transferred to the moving tree.

Composed source `022ad53db86d4856672f56360a81a1894875bcd2` again passed both complete
libraries: 101 public tests and 283 API tests, totaling 384, with all other scalar
counts zero. Scoped lint again had no findings or set-aside diagnostics; native
unit, lint and controller statuses were zero. Its separate live capture bound
the two library images and runner before any further build, including both lint
profiles and 657 canonical artifact paths. Public test bytes still matched RED.

The six changed Python tooling/test paths received exact-blob static review.
Seven controlled suites then ran on that same composed source: feature bundle
custody 9, feature evidence 22, scenario contracts 9, lifecycle contracts 8,
snapshot IDs 8, request-recovery smoke controls 18 and weekly-reset controls 29.
All 103 methods passed with no skips, failures or errors. These are focused Python
fixtures, not a packaged-binary smoke or semantic qualification of every feature.
The incoming Core, Auto and TUI changes retain their own review/execution scope;
the selected API replay does not qualify those actors.

The native phases retain separate source, executable, receipt and raw-summary
bindings. Initial base/checks/control/quiescence inputs are preserved so later
records cannot rewrite their meaning through mutable aliases. All owned checks
completed before the final source record; application certification remains
separate and unavailable under the unchanged prerequisite.

## Shared-graph correction

Final review found an existing requirement missed by the first focused baseline:
`alab/README.md:351` requires both `codex-api` and `codex-alab-api-tests` in focused
RED and final runtime, because the API dev dependencies enable shared Hyper server
features. The initial public-only baseline retains its real missing-ID observations
but does not satisfy that comparison protocol. Its records remain unchanged and
are superseded for the final comparison by `corrected-protocol` evidence.

The same tests-only source `43bf0497b6d638a12a677bccb81fe2c7e37b68c0` was rerun
with both packages and the two exact test-name filters. Public results again were
zero passed, two expected missing-ID failures and 99 filtered; the API library was
compiled in the shared graph and ran zero matching tests with 281 filtered.
Total filtered count was 380, not another set of executed tests. Both libraries
and the native runner were captured before changing source.

Full source `022ad53db86d4856672f56360a81a1894875bcd2` was then rerun with the same
package pair: all 384 methods and scoped lint passed again, with the same zero
failure/ignore/measure/filter and lint counts. The actual emitted feature lists
matched for all 556 common Cargo artifact keys, including Hyper and hyper-util.
Both corrected phases use the same controller bytes, package pair, runtime build
flags, ordinary target and source-selected compiler. Baseline harness filters
narrow execution; the final phase also runs lint. This closes the
documented graph-protocol gap without reinterpreting the earlier record.

## Publication scope

Repeated peer smoke-only updates exposed a separate publication design problem:
optional discovery of unrelated tests made publication acquire new execution and
qualification work after this API candidate had already been reviewed. Publication
now preserves disjoint smoke/test Python source as opaque Git data, like already
permitted unselected Rust source. It runs no incoming unittest discovery and makes
no validation claim about those later peer helpers. Their owners' work is preserved
byte-for-byte; native 384 and Python 103 remain attributed to source `022ad53`.

Actual input protection is strengthened: all existing owned/package/compiler/
workspace/vendor/native-helper guards remain, and the Git publication load chain
`github_push.py` → `github.py` → `cli_status.py` is protected. The retained catalog
admission protects `feature_inventory.py` and `hook_topics.py`. Every Cargo manifest,
the sole `alab-*` member-entry inventory including mode/type, Git attributes/modules/
ignore controls, and AGENTS changes require a new explicit input review. Forward
ancestry, exact peer/source equivalence and compare-and-swap publication remain.

Source audits found no semantic smoke/test-source import in the selected API/runtime
or fixed publication helper graph. No executable commit/merge/checkout Git hook was
configured. This separation supersedes the private, unexecuted 57-fixture condition;
it waives no operator hold, application gate or actual feature-input check. It is a
scope boundary, not qualification of a moving whole tree.

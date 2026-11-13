<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Full-range resolution and qualification status

Upstream target: `064b2d9bca2b5ba2f9f399ffefd06a75d97852f0`.
Review branch: `alab/alignment-full-sid0` in
`${HOME}/.ubd/codex-alignment-full-sid0.noindex`.
All 37 conflicted paths are resolved and the merged source is committed. This
candidate includes concurrent ALab main changes through `ac0ec10156`. It is **not
qualified or landed**. Native gate logs below are diagnostic evidence, not a pass.

## Retained behavior and upstream adaptations

- Keep upstream's extracted auth/provider tests, relocated message-board installer,
  terminal SSE errors, WebSocket telemetry, catalog notifications and pet state.
  Retain ALab source provenance, autopilot, bounded error decoding and pet cleanup.
- Remove obsolete MCP auto-deny copying after upstream removed its workaround;
  retain a fresh destination router without inherited full-access permission.
- Use upstream runtime auth and centralized request resolution. Capture the actual
  credential source before factories/analytics; bind source policy at the resolved
  provider factory. Opaque AuthOwner currency checks do not invent bearer proof
  from assertion-only authentication.
- Keep upstream compaction retry eligibility and limits while retaining verified
  earned-reset recovery and absolute provider deadlines through host sleep.
- Admit recovered trees before upstream host input admission. Suspension records
  unclaimed input, freezes accepted queues and waits for End ACK and persistence
  closure before success; errors remain errors.
- Restore model context in two stages: validate portable data, then install the
  real destination item-ID generator. Carry sampling counters, trusted audio
  durations/rejections, review budgeting and additional-context policy. Renew
  process-local guardian revision and runtime authority. Exhaustive struct matches
  force review when upstream adds state.
- Preserve accepted input publication identity and actual lifecycle ownership.
  Child/review registration establishes custody before awaiting tier acceptance.
- Carry prompt-cache lifetime in exec settings. Same-process recovery uses the
  exact started turn's observation; a fresh process cannot infer an absent value
  from rollout. Copy policy values out of mutex guards before awaiting recovery.
- Keep upstream server-owned startup model defaults, explicit selections and
  destination ownership. Drop the obsolete unsupported-config-read fallback test.
- Keep ALab cache-aware polling while refreshing retained originator catalogs on
  both online strategies. Offline reads remain local. The new regression checks
  retained Work refreshes without refetching a fresh default catalog.
- Keep upstream app-server security fetching and runtime auth. Optional ALab
  identity metadata proves successful observation even when no reminder exists;
  errors, timeout and identity changes provide no absence proof. The TUI observes
  before RPC and still requires current request and saved identity to settle.

Intentional hook retirements: guardian fixed-stack fixture (upstream now inherits
runner budget), old recovered-task wrapper import (replaced by pre-admission gate),
and unsupported-config-read fallback fixture. `ALAB_ALLOW_HOOK_LOSS=1` records
these reviewed retirements; it does not waive behavioral qualification.

See `high-risk-review-064b2d9bca.md` and `defaults-review-064b2d9bca.md` for
partial semantic intake and pending scenarios. Full 395-commit review coverage,
per-feature evidence and gate integration are still incomplete.

## Evidence and remaining checks

Both protocol archives and stable/experimental exports were regenerated with the
canonical Rust generators. Four protocol fixture checks passed; ignored generator
steps were run separately. Python RPC models were regenerated with locked SDK
codegen versions and their test suite passed. These used the explicit Trust
unverified lane, not full native source qualification.

The dependency audit found no unaccounted upstream package/checksum drift. The
existing allocative 0.3.6 vendored exact-archive receipt remains the sole registry
row substitution in this comparison.

Native qualification uses signed Trust15698, compiler tree
`9e3ecca1051a2231f7a9ed4fce5f5904f77e5f43b0f9dd18da59204efe3040bb`, with
`codex-rs/target-native-15698.noindex`. Earlier ordinary outputs were refused by
the native controller; they were left untouched and a fresh native target used.

Full candidate gate logs live under `${HOME}/.ubd/` with prefix
`codex-alignment-full-native-gate-`. Runs through `originators.log` failed: owned
API/fixture and mutex lifetime findings have fixes committed on top; cloud-config
still has a compiler recursion-depth failure whose pristine attribution is pending.
The new model polling, context and security scenarios do not yet have a full native
pass. Do not publish this candidate as qualified.

The separate first-prefix sync gate passed CLI build/version and 1838 ALab tests
before comparing shared failures to pristine upstream. After composing concurrent
main it is checking again; its result is separate from full-range qualification.
Preserve both worktrees and their logs. Rerere carries conflict resolutions only;
this branch also contains non-conflicted adapters, generated artifacts and owned
implementation changes that canonical integration must retain.

## Native attribution and HTTP adaptation

`codex-alignment-full-native-gate-fixtures-complete.log` compared the cloud-config
recursion-depth failure with pristine upstream064b2d9bca under the same15698
compiler. Pristine's library builds; ALab's layer does not. This is an ALab
adaptation fault, not an upstream or compiler-lane exemption.

The owned native HTTP operation wrapper now boxes its future to bound the outer
request's layout. It retains lazy preparation, source admission before operation
polling, the same permit borrow and cancellation/drop ownership. It adds no Send
restriction or new upstream-file patch. The next native gate must prove the
compiler failure is removed and existing source-order/cancellation tests pass;
this paragraph records a candidate fix, not a qualification result.

The first boxed-operation attempt did not remove the recursion error; the
http-layout gate reproduced it and pristine still passed. That allocation has
been removed. The next candidate erases the concrete native-preparation future
itself behind the existing Send boundary, as revocation already does. The generic
caller operation retains its original bounds and polling order. Qualification
remains pending; no compiler-limit or upstream-file override is introduced.

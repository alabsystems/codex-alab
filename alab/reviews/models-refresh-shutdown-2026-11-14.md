<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Model refresh observer shutdown — 2026-11-14

Decision: make the ALab-owned live refresh worker observe shutdown while its
current catalog request is pending. Race that observer future against the
worker's existing cancellation token, with shutdown selected first. Keep the
change in `codex-rs/alab-core/glue/models_refresh_live.rs`.

Regression source `86236b322813123f91575af15ed35d00a25289fe` reproduces the
retained observer. Fixed source `d9f3ff984a7b6776e5b945d02f3c9a8c46473175`
contains the seven-line production change, model-discovery invariant and test
cleanup. Actual selected-Trust execution passed all six owned worker tests and
scoped all-target Tippy. Runtime tests used the explicit unverified lane; they
are not application certification. Exact evidence and limits follow.

## Why this is the next change

The preceding explicit-response-limit audit at
`1db2ab7cf33332b50be03c9c0b3d0b4f0365c1db` found no production caller selecting
`Some` for `Request::response_body_limit_bytes`. Concrete model fetches already
have a five-second encompassing deadline
(`model-provider/src/models_endpoint.rs:142–154`); gateway OAuth's separate byte
collectors have a 20-second request timeout and device login exposes caller
cancellation. Ordinary body deadlines do not grant a native source-admission
deadline. That negative audit supports preserving explicit-limit semantics and
introducing no global body guard, authority fallback, or relaxed admission.

The RED source had a separate observer-lifetime gap. At
`alab-core/glue/models_refresh_live.rs:40`, it upgrades a weak catalog reference
and awaits `model_catalog.list_models(strategy)`. Its shutdown token is checked
before the request and during the subsequent sleep, but is not polled while the
request is pending. `app-server/src/models_refresh_worker.rs:19–26` makes both
explicit shutdown and `Drop` cancel that token; dropping the stored JoinHandle
does not itself stop the task. A pending observer can therefore retain the
catalog and manager futures after its owner has requested shutdown.

`ModelCatalog::list_models` includes configuration/provider checks, manager
preflight, cache work and retained originator refreshes
(`app-server/src/model_catalog.rs:116–151`). Some of these awaits precede or lie
outside the model endpoint's existing HTTP fetch deadline. Shutdown should stop
the worker's observation wherever that one future is waiting, without needing a
new global timeout or waiting for source readiness.

## Scope and mode contract

`app-server/src/models_refresh_worker.rs:31` already selects the owned live
adapter by `is_alab_binary()`. It remains alive when enhanced mode is false so
its watch receiver can observe a later experience change. Within that adapter,
enhanced mode chooses `OnlineIfUncached` and the enhanced interval; Vanilla mode
chooses `Online` and the original interval. `active()` combines binary identity
and the enhanced switch (`alab-core/src/lib.rs:52–58`).

Lifecycle shutdown applies in both modes. Restricting cancellation to
`active()` would leave the same owned observer alive in Vanilla mode. This does
not require changing the identity routing, upstream `spawn_polling`, refresh
strategy, cadence, live receiver, or when mode changes wake the sleeping worker.
Mode changes continue to affect the existing next-iteration behavior; only
shutdown cancels the in-flight observer.

The committed replacement for the direct await is:

```rust
let refresh = tokio::select! {
    biased;
    _ = worker_shutdown.cancelled() => break,
    refresh = model_catalog.list_models(strategy) => refresh,
};
```

The existing error handling follows `refresh`. A shutdown drops this observer
future and its retained catalog reference. It introduces no source-ready
substitute, cache fallback, new deadline, direct credential/native actor abort,
or authority bypass. Actors that independently own credential persistence or
native source work retain their existing lifecycle contracts.

## Regression design present in the frozen source

`alab-core/glue/models_refresh_worker_tests.rs:320–562` in the RED source (now
lines 314–556 after removal of the draft comment) adds two test functions
with six cases in total:

| Test | Cases |
| --- | --- |
| `live_worker_shutdown_releases_pending_preflight_in_both_modes` | Explicit shutdown before first task poll and during acknowledged pending preflight, each with enhanced true/false: four cases. |
| `live_worker_drop_releases_pending_preflight_in_both_modes` | Real worker `Drop` during acknowledged pending preflight, each with enhanced true/false: two cases. |

The real owned worker is spawned with the existing catalog/manager fixture.
`PendingPreflightEndpoint::uses_codex_backend` records entry, retains a drop
observer and waits on a closed gate. The test keeps that gate closed while
observing shutdown, catalog release, no cache/fetch calls, and worker completion.
The before-start cases run on a current-thread runtime without an intervening
await between spawning and cancellation.

Tests retain the actual worker JoinHandle by replacing its private field with
an already completed empty-task placeholder. This preserves real `Drop`
behavior and permits joining the actual task; it does not copy worker logic or
use abort as successful cancellation evidence. Observations are saved before
the gate is released. Cleanup then releases the gate and joins before contract
assertions. An abnormal abort/reap fallback is bounded, recorded, and always
fails the contract. Two-second observation and five-second cleanup bounds are
test custody bounds, not production refresh deadlines.

## Validation and remaining lint

The completed native execution `87192` exited 101. Its test summary is
`0 passed; 2 failed; 0 ignored; 0 measured; 505 filtered out`, in 4.17 seconds.
Both functions failed at the intended final observer-drop assertion, with
`dropped_while_held=0` instead of `1`. The emitted first failing case for each
function was enhanced mode during acknowledged preflight, once for real `Drop`
and once for explicit shutdown. Each recorded `entered=1`, `gate_held=true`,
`catalog_released=false`, `task_finished=false`, no cache/fetch calls,
`clean_join=true`, `forced_cleanup=false`, and `final_observer_drops=1`.

Each function collects and cleans all its cases before contract assertions.
The six-case execution/cleanup therefore follows that source control flow;
the failure output contains only the first failing observation per function,
not six separately logged observations. The result demonstrates that the old
observer stayed alive while readiness was withheld and then dropped after the
test released the gate, with clean worker joins and no forced cleanup in either
emitted failure observation.

| Evidence | Result |
| --- | --- |
| RED source | `86236b322813123f91575af15ed35d00a25289fe` |
| Actual RED execution | Native execution `87192`; exit 101; both intended regressions failed as detailed above. |
| Complete RED log | Retained at the root repository's `.git/alab-validation/86236b322813123f91575af15ed35d00a25289fe/models-refresh-shutdown-red/checks.log`; 40,442 bytes; SHA-256 `17ff9a5790d49ed2a864f21a652f966d630bde820e67ad9d2995190709149a97`. Actual native execution exit was 101. |
| Fixed source | `d9f3ff984a7b6776e5b945d02f3c9a8c46473175`; observer cancellation changes only the owned live glue; model-discovery invariant/scenarios updated in `alab/features.toml:54–55`. |
| Fixed owned glue hash | `codex-rs/alab-core/glue/models_refresh_live.rs`: SHA-256 `6c203bf0169ea654596b85d5f1d7ed28e2fb6b156c05530db555b27be0436576`. |
| Fixed regression source hash | `codex-rs/alab-core/glue/models_refresh_worker_tests.rs`: SHA-256 `808df3f96a11bc30e75641b1fc78ebedc5134f2671f12bdc7c936346d0519412`. |
| Fixed feature inventory hash | `alab/features.toml`: SHA-256 `2e32d77f919025d86ee6f90d7f89bd996de7cd8eeb0c0849a6008e869c633c24`. |
| Actual GREEN focused tests and existing mode/cadence controls | Root execution `63523`, fixed source `d9f3ff984a7b6776e5b945d02f3c9a8c46473175`: 6 passed; 0 failed; 0 ignored; 0 measured; 501 filtered out; finished in 0.11 seconds. Four existing owned controls and two new test functions passed. |
| Actual scoped all-target lint | Same execution `63523`, selected Trust 15698; exit 0. `codex-app-server --no-deps --all-targets`, separate canonical lint child target; no owned, spanning, failing or build-script diagnostics. Six upstream diagnostics set aside; successful build-finished record and 1,434 artifact paths within that target verified. |
| Complete GREEN tests/lint archive and hash | Root repository `.git/alab-validation/d9f3ff984a7b6776e5b945d02f3c9a8c46473175/models-refresh-shutdown/`; `checks.log` SHA-256 `b7a8e1b3b85e42dfba1d28f16532819502b543c9ce91d7cd662e92aa4293adab`; `lint.json` SHA-256 `728ce052cec443fcd9c7f59acaae672224ece5792efc042210120d6539d3c38f`. Source diff, inventory, classifier verdict and hashes retained with the record. |
| Publication custody | Publication receipts belong in the separate private validation ledger. |

These regressions exercise the actual owned worker and catalog/manager path
with a controlled pending preflight endpoint. They do not exercise real OAuth
credentials, native authorization/source admission, HTTP body timeouts, or
application qualification. Runtime evidence covers the old source's intended
RED failure/recorded cleanup and the fixed source's six passing owned tests.
Scoped lint passed. No native authorization proof is claimed here. The selected
Trust 15698 certified-build macro-admission prerequisite remains unchanged;
application qualification, packaging and installation remain deferred as recorded
in `targo-macro-admission-2026-11-14.md`.

The next debugging improvement is an actual SQLite persistence contract for the
new HTTP rejection-body capture phase. Existing durable endpoint fixtures bypass
the real HTTP transport. Source review found no persistence defect, so this is a
coverage task: exercise complete/truncated bodies, disabled mode, terminal status,
typed advice, context and privacy through the installed filter and writer. Its
outside draft is reviewed but unformatted and unexecuted; it is not part of this
validation result.

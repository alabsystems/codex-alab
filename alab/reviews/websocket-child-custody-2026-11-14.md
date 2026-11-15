<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# WebSocket child custody, 2026-11-14

Decision: migrate three related WebSocket test-parent adapters to the existing
shared bounded child runner, while retaining their certificate, environment and
parent-owned proxy contracts. This is expected-GREEN test maintenance, not a
new production WebSocket defect or runtime RED claim.

The composed code source is `ee411f3d89e4e09967d5bdc258833802d20ea7ae`.
Native execution `88002` passed standalone `codex-websocket-client --lib` and
canonical scoped all-target lint with exit zero. Initial source `608d1c2c7e`
also passed before an independent storage-recovery change reached main; its
actual evidence remains separately scoped. Independent source review accepted
the WebSocket changes. Root ran the checks recorded below.

## Three adapters and preserved child proof

All paths below are in `codex-rs/alab-core/glue/`. They are `cfg(test)` children
of the owned `websocket_rejection.rs`, loaded by the WebSocket client dialer.

| Adapter | Old collection | Shared-runner budget |
| --- | --- | --- |
| `websocket_rejection_tls_tests.rs::native_default_wss_honors_ssl_cert_file_and_ignores_codex_ca` | Unbounded blocking `Command::output` inside a current-thread Tokio test | New 60 s |
| `websocket_proxy_config_error_tests.rs::isolated` | 60 s wait, but `Take(64 KiB)` stopped reading both pipes | Existing 60 s |
| `websocket_proxy_ipv6_tests.rs::isolated` | 60 s wait with unbounded retained output and a separate live proxy | Existing 60 s |

The earlier real noisy-child RED establishes the capped-drainage mechanism;
it is not relabeled as a new observed failure of these three fixture families.
TLS and IPv6 had different custody gaps. All adapters remain async and await
the existing shared runner directly; no new runtime is constructed.

Exact names, `--exact`/`--nocapture`, child marker values and return conventions
are retained. No test-thread flag or clock pause is added. Both proxy helpers
keep their proxy/CA environment-removal lists, case guards and Default/Explicit
route choices. Config-error cases keep their typed errors and private-proxy
Display/Debug exclusions. IPv6 keeps its generated proxy, target order/count,
optional Basic auth, real Upgrade/text/echo exchange, CONNECT authority/Host
brackets and SOCKS greeting/address-byte assertions.

The TLS parent's `Scratch` owner remains across the awaited child and internal
cleanup, retaining the same root/certificate/key/invalid-CA files and cleanup
on Drop. `SSL_CERT_FILE`, the invalid `CODEX_CA_CERTIFICATE`, material-directory
and child markers, and removal of only `SSL_CERT_DIR` are unchanged. Its child
keeps the split rejection, status/body/IDs/advice and ALPN assertions. The new
60-second parent ceiling follows existing WebSocket conventions; five-second
fixture/phase guards are unchanged. No stdout COMPLETE marker is substituted
for these semantic child or wire assertions.

## Separate proxy-server custody

The IPv6 parent retains `child_result`, then collects nonpanicking
`server_result` before asserting either. Child failure and secondary server
failure remain available together; strict child success still precedes wire
observation assertions, but both cleanup paths have already been collected.

`ProxyTask::finish` keeps its JoinHandle in the owner through the first
five-second wait and the second five-second abort/join wait. Cancellation of
finish therefore retains Drop's abort fallback. An initial join timeout always
returns an error, even if the task completes during cleanup. A second timeout
keeps the handle for the abort fallback and reports the cleanup deadline;
successful reaping is not invented. Late cleanup cannot make the test pass.

The shared child runner separately drains true EOFs with bounded prefix/tail
retention, a strict last-summary gate, and internal bounded kill/drain/reap.
External cancellation only requests `kill_on_drop`, not asynchronous reaping.
These owners cover the exact test child and local task, not descendant-process
or native actor custody.

## Thin surface and standalone features

The WebSocket migration changes five files: three owned test modules, one owned `cfg(test)`/path module
registration beside them, and `websocket-client/Cargo.toml`'s existing Tokio dev
feature entry. Adding explicit `io-util` lets standalone WebSocket tests read
the shared pipes without depending on API-package feature unification. No
upstream `lib.rs` hook, production code/feature, dependency version, lockfile or
compiler-pin change is introduced. The shared runner is unchanged.

## Actual validation and custody

The initial `608d1c2c7e4c3b216c5f5ebbc3d81c8de794f59b` source passed
53 tests in 0.56 seconds and scoped lint, execution `80229`. Main then gained
storage recovery at `4a9e7300879869f8e03dc89e884d36017a4192e5`. Actual
compiler-artifact metadata identified four local compiled roots, with changes
inside `alab-core`. That dependency change required a fresh runtime/lint run.

The merged source `ee411f3d89e4e09967d5bdc258833802d20ea7ae` passed all
53 standalone tests in 0.73 seconds, execution `88002`, with zero failed,
ignored, measured and filtered tests. This includes the native-default WSS
root control, proxy-configuration privacy cases and IPv6 wire exchanges.
The standalone selection executes the actual test parents and explicit crate
features; building it as an API dependency does not execute them.

Scoped all-target Tippy exited zero. Its `owned`, `spanning`, `failing` and
`scripts` classifier lists are empty, `set_aside=0`, and build completion is
successful. All 241 unique compiler artifact paths lie under the canonical
runtime target's `tippy` child. Both runs respected native resource admission.
The shared child collector and the five WebSocket topic files are identical
across composition. Other API/state results are not relabeled as new runs.

The storage merge also exposed two unowned hook topics in the feature catalog.
A separate metadata commit, `dfd83a3c038ca557ac61eb9486d9b7c7dacce006`,
registers `storage` and `storage-status` under `goal-storage-recovery` with
narrow owned paths and explicit invariants and retirement criteria. Existing
thirteen feature tables remain byte-identical. Committed-source inventory
then passes: 88 hook topics and 14 features. This is ownership metadata,
not proof of storage behavior or application qualification. The entire tested
Rust tree, lockfile, compiler pin and validation support are unchanged by that
catalog correction and the independent documentation merge.

Evidence remains in the root repository's `.git/alab-validation/`:

| Evidence | Path below `.git/alab-validation/` |
| --- | --- |
| Initial runtime/lint | `608d1c2c7e4c3b216c5f5ebbc3d81c8de794f59b/websocket-child-custody/` |
| Composed runtime/lint | `ee411f3d89e4e09967d5bdc258833802d20ea7ae/websocket-child-custody/` |
| Catalog correction | `dfd83a3c038ca557ac61eb9486d9b7c7dacce006/goal-storage-feature-inventory/` |

Both runtime exit markers contain `0`. The failed pre-correction inventory is
retained alongside the passing metadata check. Audit and migration notes are
retained at `.git/alab-validation/websocket-child-custody-next-20261114/`.

| Composed artifact | SHA-256 |
| --- | --- |
| `checks.log` | `8e380ff04165b29b86121cddf366357f23165a94b6a895894f73b8184a579667` |
| `lint.json` | `b8ea3f23d4fce7badee8e615576d3e315efa1bd2907441422e4cbbce548d25a6` |
| `verdict.json` | `7f390f2bfe1312792d12d567794e90b8e44e1c16eebb0e6678be1d29ffa58300` |
| `source.diff` | `647faadfcab511ca70c25c749badcb4510dc089f3274358d5ad69536ac502d9d` |
| `feature-inventory-failure.txt` | `9ea32d5f9e375deb84230bf66b4b6cadbaa2170b63d2f39d2b0d9e4d4b09e1a7` |
| IPv6 fixture source | `7df7c671efd60c4eabee737186dea054eb1d97464e0f47436b332069f6e2bdad` |
| Owned registration | `0cdb5cc5af4b4d15cf667e8c06549cdde659d6bcd8a8d78f191d6e32dc55db81` |
| Crate manifest | `c93d7f8ad562bb102974d95513543794378d1900ad17ce3f12c04bce08a0253e` |
| Unchanged collector | `5b4dcd42ca23f1c99a827a10e7a3703180c442bfb8c8f2142539b9e40e8cee81` |

Independent review accepted the actual WebSocket source, including retained
proxy-task ownership and both result contexts before assertions. These checks
establish fixture completion and output custody. The TLS test name describes
default trust-root behavior, not native credential admission or accepted-actor
lifetime. No application certification, package or installed-update claim
follows; the selected-compiler macro-admission blocker is unchanged.
Publication identity and receipt remain in the private validation ledger.

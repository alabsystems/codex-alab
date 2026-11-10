<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Upstream branch composition candidate

Merge `alab/sync-cce9d3228b5d-preserved` at `bc4271966c` into the coherent
release candidate. This incorporates upstream through `cce9d3228b` and the
branch's accumulated source adaptations. It is an **unqualified candidate**,
not permission to ship or a claim that its earlier stack failure is fixed.

Source conflict decisions:

- Keep current native target-policy glue and the branch's boxed native-I/O
  helper together.
- Retain upstream outcome-handoff close/drop alongside current configuration
  admission shutdown and exact suspension cleanup.
- Use upstream ProviderAuthContext and request setup, capturing its actual
  credential manager rather than the mutable host. Keep captured analytics
  credentials. Bind prepared HTTP factories through the existing native source
  policy seam before upstream runtime binding.
- Keep current immutable provider snapshot wrappers and move their module hook
  below upstream's split test modules. Preserve upstream's new provider/auth
  implementation and test-module layout.
- Combine upstream workload-identity/redacted-debug changes with native epoch
  provenance. Preserve real manager registration in constructors and the
  upstream network-policy ownership flag.
- Preserve atomic cached credential/factory capture, context-host dispatch,
  deferred constructor admission and native storage selection while accepting
  upstream refresh locking, reload errors and forced-workspace policy fencing.
- Select the branch's precomputed schema archives provisionally; regenerate and
  compare them with the merged protocol source before qualification.

The earlier source review forbids continuing its refused binary-inspection
investigation. This work uses source review and ordinary native build/test
qualification; it does not inspect that binary or peer terminal history.

`alab/delta.sh` was read before the upstream merge; retained output is
`/tmp/codex-release-delta-before-s0.txt`. Repeat it after final adaptation.
The lockfile is the automatic branch merge, never hand-edited upstream entries.
Check locked resolution and resolve ALab entries through the admitted toolchain
if necessary. Source gate, focused source-isolation tests, protocol exports,
package smoke and distribution/adoption verification remain required.

## Hook-count reconciliation

The first combined gate stopped at the hook-count audit, before compilation.
Compare against remote main `9a2fb340cc`. Accept only these reviewed reductions
with the gate's documented `ALAB_ALLOW_HOOK_LOSS=1` control:

- `accounts`, initially 331 to 330 before the follow-up constructor hooks: upstream removes the inline realtime sideband-header
  helper and the legacy ModelClient constructor/setup. ProviderAuthContext now
  captures the actual manager; prepared HTTP factories and auxiliary pools bind
  native source policy in ResolvedProviderAuth. RuntimeRequestAuth already uses
  `runtime_auth_source.rs` for typed source admission and original header
  replacement semantics; do not wrap that path redundantly. Realtime carries auth and factory to
  its sideband client rather than materializing headers in the removed helper.
- `core-test-fixtures`, 7 to 6: upstream removes its fixed 4 MiB explicit worker
  stack and inherits the test runner's stack budget. The ALab override on that
  deleted call is redundant; no replacement stack override is introduced.
- `notices`, 7 to 6: HTTP-result handling, pre-fetch observation and guarded App
  delivery moved together into `alab-tui/glue/security_setup.rs`. The upstream
  module reexports `prefetch_using`; account matching, captured revision,
  conditional settlement and banner fallback remain in the owned glue.
- `startup-test-fixture`, 11 to 7: the branch replaces the old socket-proxy
  drain adapter with upstream's recorder shutdown path. StartupWitness retains
  the actual attachment and normal-exit observations with hang detection;
  obsolete proxy-loop/drain callbacks are removed. The affected startup tests
  still require native qualification, including the prior stack regression.

This authorizes the known source-level reductions, not failures or skipped
native tests. Do not add dummy markers to defeat the count audit.

The first native lint run exposed upstream's new `owns_network_policy` field
in the ALab host facade constructor. Set it false: the host is an inert selector,
not a credential epoch, and cannot invalidate the actual manager's policy.
Keep the real epoch's upstream ownership flag. This run stopped at compilation;
it supplied no test or release qualification.

The next lint run compared the remaining login error against pristine upstream.
Pristine `cce9d3228b` builds; the composed layer failed because the new workload
identity ApiKeyAuth initializer omitted `alab_epoch`. Initialize it unbound, as
other fresh credentials do; real cache publication attaches the actual epoch.
A source scan also found five moved/new Bedrock test initializers needing the
same fixture default. Preserve this attributed red control and rerun the gate;
no native tests have yet passed on this composed source.

The following native lint run reaches executor startup but overflows the compiler
query depth (130) when laying out ExecServerCommand::run. Pristine upstream
builds the same target. The host dispatch macros used Pin<Box<concrete future>>,
which retained the recursive concrete AuthManager future in downstream types.
Use the existing owned boxed_native boundary (a dyn Future) for host delegation
while preserving same-task lifetime and cancellation. Add async dispatch coverage
for A-to-B selection and closed-host Option/boolean/Result refusal. Keep compiler
recursion and runtime stack limits unchanged. This is a candidate correction;
its result remains unproven until the native gate and regression run complete.

The typed-host experiment did not change the executor's query-depth failure;
remove it rather than carrying a failed correction. Retain the async host
selection/refusal regression. The existing constructor/shared factory wrappers
boxed their input but returned an opaque concrete outer future, exposing that
large type through all startup callers. Erase the outer owned factory futures
at these existing ownership boundaries, with Send and the caller's borrow
lifetime explicit. Factory admission, task-local scope, anchoring, cancellation
and completion stay inside the same operation. No upstream signature or limit
changes. This narrower correction still requires the actual native gate.

Erasing the constructor wrappers also left the executor at the same depth130
failure; restore those wrappers. The final candidate now gives the executor's
configuration and remote-authentication preparation a typed owned startup
interface, with only module/import hooks upstream. The ordinary config functions
stay upstream's; their borrowed inputs, output, cancellation and task remain
unchanged. This isolates preparation from the command's transport/shutdown
future instead of changing the recursive host APIs or increasing limits.

The full tooling run on frozen6b5b42ca4c completed550tests with three failures
and seven skips. Two failures are the same pristine-preflight mismatch: offline
metadata omitted the vendored toolchain override that actual builds already
used. Carry the override through preflight too; retain the regression assertions.
The third concerns the controlled signal-mask setup before Cargo's native front;
direct and adapter observations matched, but SIGUSR1 was already unblocked.
Investigate and qualify that fixture boundary separately; no pass is claimed.

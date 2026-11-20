<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Trust 27009 adoption: required build-script execution remains unsupported

The unchanged native adoption gate reached certified compilation and refused required
verification of `anyhow`'s build-script execution. **No compiler pin, application proof,
or application release was accepted.** The native-generated working `trust.lock` remains
uncommitted. The conditional pre-promote restoration remains held because its qualified-pin
condition has not been met.

The executed sources below precede this documentation commit. Their observations are not
qualification of this document's HEAD, moving main, or the separate upstream integration candidate.

## Actual observations

- Source `b9b99b3bd9294159caae99d5aa7e383b8a3588a5`: protected, named-unverified
  runtime controller 1696 passed all 16 selected outer tests: shared large-stack helper 3,
  actual cloud destination source/policy/cancellation 3, WebSocket return 4, native model and
  code-mode overlap 5, and scenario-factory runtime context 1. The maintained prerequisite
  functions built and artifact-selected the genuine code-mode host. Child test executions
  are not counted again as outer tests.
- The same source's four-package all-target Tippy controller 10192 returned zero owned,
  spanning, failing, and missing-build-script findings; 16 upstream findings were set aside.
  Its extra upstream finding is the previously masked warning in
  `cloud-config/src/service_tests.rs:1742`. No pristine attribution was claimed for that row.
- Source `f0d9efffbff07cdc31f9b03bad04821a3e78e1f9` merged freshly fetched main
  `33eb7462bd464c4fd27587d09337cdf5113202e1` without conflicts. The six helper/cloud
  source, test, and manifest files were unchanged from the runtime-tested source; incoming
  diagnostic/recovery changes were preserved. Native adoption attempt 5 ran the unchanged
  `./alab/check.sh`. Its full canonical all-target classifier found nothing in ALab's code,
  setting aside 146 upstream diagnostics. This passed the lint stage, not the whole gate.
- Attempt 5 then failed its first certified build. Native `adopt` exited 1; its project
  controller/gate exited 2. The later application-runtime, full ALab-test, and hooked
  upstream-unit stages were not reached.

The committed compiler remains build 15698, source
`20565c3255adf151f9eb920e89cdf83c38f84abe`. Native adoption selected build 27009,
Trust 0.14.0/Rust 1.101.0-dev, source
`5062eed55c0bc9df8a79eba1368d88ffde76c741`, signed tree
`b253bdba29ba8afca37ac844ee4f767e0a2fe06b1a7d8509e1ad492ba8157be0`.
The pending native-generated lock's SHA256 remained
`7c4c9f1a8f7f4950089d431ccab4d2e3e8b27baccf5e42d9242b36cc3de892f8`
through the checks and fresh-main merge. It was neither edited nor committed manually.

## The first certification refusal

The compiler controller adopted `policy=certify`, dependency inclusion, and required
verification, selected the expected native 27009 compiler, then reported:

```text
cannot satisfy -Ztrust-verify=on assertion for anyhow v1.0.103
 target "build-script-build" mode run-custom-build
required verification is unavailable: build-script-execution
```

The subsequent missing `TRUST_JSON`, coverage summary, and terminal target inventory are
consequences of that early refusal. They do not establish a missing-session-flag or compiler
selection defect. The retained report explicitly labels itself
`untrusted_observational_only_no_proof_credit`; its single unknown row is a synthetic
transport failure, not a measured program obligation. No actual proof was established.
The original reports remain under `codex-rs/target/alab-certified.q55U0E/report` and in the
immutable refusal archive below.

## Source and published-capability check

In selected source 5062, `trust_non_proof_mode_exclusion_reason` maps `RunCustomBuild`
to `build-script-execution`. This is a Cargo control action, distinct from compiling
`build.rs`. The dependency policy includes compiling the script and its support closure;
it does not provide a compiler proof protocol for executing the script.
`require_trust_unit_verification_assertion` validates the active protocol, asserted flag,
and session before rejecting that exclusion. Adding session flags cannot supply the missing
execution capability.

The same exclusion and refusal remain in observed private main
`157424a8c1f575f8960b118654869f5091dae663`, public main/v0.14.0
`d23057518df44c462c24106005ce43f0f42c331f`, and local staging
`aab333e337946d946f6255d0a5621698cca09ae8`.
Trust already documents this boundary as
[BUILD-SCRIPT-ASSERTION-REFUSAL-1](https://github.com/alabsystems/trust/blob/157424a8c1f575f8960b118654869f5091dae663/docs/OPEN_ISSUES.md#L1085),
with an explicit semantic decision and `DECISION_QUEUE` entry required to close it.
Its TCB documentation also records the separate co-resident script/proof-store authority
hazard; no exploit was attempted here.

Fresh official release inventories observed on 2026-11-16 contain 24 private and 22 public
releases and no numeric native compiler newer than 27009. The later
[v0.14.0 publication](https://github.com/alabsystems/trust/releases/tag/v0.14.0) is not a newer
native compiler package. D21's isolated proc macros and newer unit/byte receipts do not
implement build-script execution verification. No newer admitted repair was established.

## Source changes retained for review

The earlier SDK checks exposed a recursive context-storage type graph and long finite
async `Send` proofs. Safe erased `Arc<dyn Any + Send + Sync>` storage breaks the former,
with typed access, invariant `PhantomData<fn(M) -> M>`, checked downcasts, and exact pointer
CAS retained. Owned constructor/reader interfaces reuse existing boxes and preserve the
same lazy operations, task-local scopes, ownership, and cancellation.

The final test correction removes only the unnecessary `Send` bound on a root future
constructed and polled on one dedicated child thread. Its factory and result remain
`Send + 'static`; the two workers, 8 MiB stacks, joined errors, and panic failures remain.
The native compatibility wrapper still invokes the scenario factory inside Tokio on first
poll. The `Rc` and runtime-handle controls exercise those actual contracts. Production
spawned-task `Send` requirements were unchanged.

The cloud correction places a borrowed `Send` future at the existing generic
`destination_snapshot<S: BundleSource>` I/O operation. It uses the source trait's existing
explicit future contracts and leaves the operation body, policy publication, timeout, and
source lifetime unchanged. The unsuccessful concrete-getter constructor/hook was removed
completely; config and loader have zero net changes from the pre-attempt baseline.

Allocation costs are explicit: ManagerLoad, BoundAuth, RefreshAdmission, and the cloud
observation each add one operation allocation. Other retained coercions reuse existing
boxes; the paired-auth cleanup removes one redundant box for a real manager and two on
its host path. No runtime speedup or net allocation improvement is claimed.
Earlier retained login evidence includes 88 passing owned controls and an empty library
classifier at `6eda0c593241d3d754ab945a4ad0b73b92806943`; it remains attributed to that
source and limited scope.

## Retained evidence and next action

Archives live under `.git/alab-validation/<executed-source>/trust27009-adoption/`.
Their SHA256 indices bind raw controller output and inputs; ordinary observation indices
and read-only source reviews are not native proof receipts.

| Source / archive | Index SHA256 |
|---|---|
| b9b99 / `generic-destination-all-targets` | `ce1cc754f726b35862fb97ae2a1afc7a58c674ad65c05f8815518f627b8e6cb0` |
| b9b99 / `final-test-and-destination-controls` | `880637508e1c694d4f3d204632e0046f33f4445847f0ce3c2103cb47ef22d829` |
| f0d9 / `native-attempt5-build-script-refusal` | `7bbd815ade53619ce40e64e9904d68f9be30e18b3214c3cc84a0196436ca652b` |
| f0d9 / `build-script-capability-audit` | `5f7b72693edbe6ae98923afb6d3d149e7a8833638feb6198ce8e3c353d54c87a` |

The full gate automatically removed its temporary lint JSON. Its original successful
rendered canonical lint verdict is retained in the native log; no reconstructed JSON is
presented as that original verdict. All six failed certification reports were preserved.

The next unblock belongs in Trust: resolve the documented execution-scope decision,
implement and qualify the required execution/completeness contract while preserving
ALab's required verification and compiler-capable dependency coverage, then publish an
authenticated native compiler carrying it. Only changed, admitted capability inputs
justify another unchanged native adoption attempt. The compiler pin, prescribed release
hook restoration, and upstream integration remain held. No policy, compiler, SDK, session,
cache-integrity, or permission workaround was introduced.

## A two-package reproduction on macOS arm64

On 2026-11-19, Trust build 27009 certified a constant supplied by a local
library and refused the same constant supplied through a Cargo build script.
The two runs used the same consumer and the same dependency-inclusive
`alab/proof-policy.toml` from `c1028949a53fae09ef66a8d5c5f1316049c0d379`.
This small comparison can be run offline before allocating an application
build when evaluating a compiler with changed execution support.

Use two fresh workspaces with resolver 2. Each has two Rust 2021 packages at
version 0.1.0, a binary named `consumer` and a library named `emitter`.
In both, the consumer depends on `emitter = { path = "../emitter" }` and its
`src/main.rs` contains `fn main() { assert!(emitter::NUMBER == 42); }`.

| File | Static workspace | Generated workspace |
| --- | --- | --- |
| `emitter/src/lib.rs` | `pub const NUMBER: u8 = 42;` | `include!(concat!(env!("OUT_DIR"), "/answer.rs"));` |
| `emitter/build.rs` | Absent | The script below |

```rust
use std::{env, fs, path::PathBuf};
fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out.join("answer.rs"), "pub const NUMBER: u8 = 42;").unwrap();
    println!("cargo:rerun-if-changed=build.rs");
}
```

In each workspace, generate its lockfile with
`targo generate-lockfile --offline`. Set `CARGO_TARGET_DIR` to a
separate fresh target per run, `CARGO_BUILD_JOBS=1`, `CARGO_INCREMENTAL=0` and
`CARGO_PROFILE_DEV_DEBUG=0`. Supply the absolute path to the policy and a fresh
absolute report path.

```sh
targo trust build --certify --report-dir "$report" --config "$policy" \
  --offline --locked -p consumer --bin consumer
```

The static case exited 0 and reported two proved obligations. The generated
case exited 2 at `emitter`'s `run-custom-build` unit with
`required verification is unavailable: build-script-execution`. These results
cover this fixture. Application adoption still requires the full source-selected
gate and its own live proof.

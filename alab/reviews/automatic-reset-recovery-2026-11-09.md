<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Automatic reset recovery: qualification and adoption

The original Trust session is working again on its existing compiler goal, and
both it and this session load the qualified reset-recovery runtime. The behavior
and subsequent hardening are on remote main. Detailed decisions and historical
attempts are in `alab/continuous-earned-resets.md`.

## Behavior and decision

Keep the original request and its completed work while checking the authenticated
subscription allowance and earned reset credits. Recheck availability on a paced
30-second loop; bound each backend attempt to 45 seconds and release account
custody between attempts. Do not infer availability from model text.

An earned reset has durable, account-bound custody and an idempotency key. A lost
acknowledgment, cancellation, crash, or deadline expiry retains that key for
reconciliation. A consumed credit does not authorize repeated retries without
fresh allowance evidence, and distinct credits remain independently eligible.

The final deadline correction (`faf104c113`) fixes another starvation path:
backend errors and slow requests could wait beyond the model provider's known
natural reset. Bound every attempt and polling delay by that original deadline,
then return scheduling to the native quota policy. Keep ordinary polling when
there is no known deadline. Tests first reproduced the persistent-outage and
in-flight-request failures against the old loop, then passed after the fix.

## Verification

- All 29 quota library tests pass, including cancellation, uncertain redemption,
  reconciliation, and natural-deadline custody controls.
- Quota all-target lint passes with warnings denied.
- All 27 native reset cases pass against the debug executable and independently
  against the exact selected release package. Both reports declare full coverage.
- The deadline outage cases retain the completed plan and original input, spend
  no credits, reject early retries, and recover before another polling interval.
- Native guarded packaging builds, smokes, and selects the exact committed source
  with its admitted Trust compiler. Builds use the explicit unverified lane;
  these results make no formal proof claim.

The reset probes use isolated fake ChatGPT authentication and loopback services.
They do not redeem live subscription credits. Broader integration qualification is tracked separately; an interrupted or
pending gate is not a pass.

## Runtime identity and adoption

- Package: `0.2.0-dev.15257+codex.0.167.0.gfaf104c11`.
- Source: `faf104c11323e30daf127ecda02716d08e25a519`.
- Compiler: native-admitted Trust 15698, identity `010b4e686f3b1ea7`.
- Binary SHA256: `e2fa26244ea65cfb60e1afac5436966ca2e6bb1ddc5052b06422123b1309f18a`.
- Loaded image UUID: `19DFE9BD-1151-3C1C-AAB0-2DFE0F0CFADD`.

Controlled restarts occurred at stopping points, after saving handoffs and checking
holds, pending input, and native job state. Independent process samples confirm
both actual loaded images match this UUID. The Trust tab resumed its existing
"FINISH THE COMPILER PIPELINE AND ITS PROOF CHAIN for Trust!" goal immediately,
with fresh compiler tool activity. This upgrade needed no injected task prompt or
manual goal-resume command.

Evidence retained locally under the owned worktree's ignored `codex-rs/target/`:
`deadline-native-full`, `deadline-package-full`, `deadline-adopted-self.sample`,
and `deadline-adopted-trust.sample`. The exact-package report records the executable
and probe hashes, requested cases, completion, cancellation controls, and guardian
results. No guardian timed out.

## Follow-up improvement

The smoke runner previously read the entire 1.04 GB debug executable to hash it.
`245d1b1e11` streams 1 MiB chunks instead. The digest is unchanged; the measured
process used 28,557,312 bytes maximum resident memory and 0.81 seconds. The six
report/fixture tests pass. This changes evidence collection, not reset behavior.

## Follow-up qualification, November 10

The native completion-race fix and banner test optimization are on main through
`460073cbc0`. Six child-suspension tests and all nine public recovery-tree tests
pass. The original failure is reproduced before the fix and passes afterward.
All 21 banner tests pass; selecting Mono before rendering preserves its 14,460
cases and reduces the measured test runtime from 92.75 to 0.76 seconds.

Two tooling classes that previously failed setup now pass: 38 lint-step fixtures
and 17 result-attribution fixtures. The runner artifact comes from the earlier
native qualification; its crate, workspace manifest and lockfile are unchanged.
Its copied binary SHA256 is
`f565d7e731b1de42345ecbd845176faff2d8142090662d54dd95ac35761d0d2c`.
The lint fixtures completed in four named batches of 10, 10, 10 and 8 tests.
An earlier interrupted run without a final summary is not counted as a pass.
The compiler-backed native-context class remains a separate pending check.

The combined gate previously passed ALab lint and binary startup, then stopped
at the native 20 GiB disk floor during test compilation. Capacity subsequently
returned. Reusing its caches is refused because the recorded device identity
no longer matches the directories' identity. Preserve those receipts and bytes;
do not rewrite custody markers. A new qualification worktree admits fresh native
own/pristine caches and is running the full gate on immutable `460073cbc0`.
A running or resource-stopped gate is not a pass. Follow-up runtime packaging
remains pending; the installed reset-recovery package above remains selected.

## Native-context prerequisite closed

All three real-Cargo execution-context tests now pass in 4.672 seconds under
native-admitted Trust 15698, with no skipped methods. An initial invocation used
the shell-based project launcher as Cargo and failed an inherited-mask assertion.
Direct execution and adapter execution had identical signal state: the mask had
already changed before the adapter ran. Keep this failed invocation as a control.

The test driver now accepts `ALAB_NATIVE_CARGO_COMMAND`, a nonempty JSON argument
list, so it can invoke the native admission front end, `__build_admit`, and the
selected Cargo executable directly. Keep the existing single-executable setting.
This removes the intervening shell while retaining native admission. Paths with
spaces remain separate arguments. The corrected run preserves blocked SIGUSR1
and SIGUSR2, descriptor state and process context; the Python-startup negative
control still detects its expected signal change. Existing-runner conflicts and
missing-receipt refusal also pass. No production runtime or policy changed.

These scoped runs resolve all three setup errors from the 490-test tooling run:
38 lint fixtures, 17 result fixtures and three native-context tests now complete.
They do not turn the still-running combined Rust gate into a pass. Evidence is in
`retained-native-context.log` (initial control),
`retained-native-context-direct-front.log`, and the corresponding evidence folders.

## Completed ALab phase and delivery boundary

The native gate on `9101bfdcdd` passes ALab lint, builds and starts the CLI, and
passes all 25 ALab harnesses: 1,843 tests, zero failures and four existing ignores.
The 868-test autopilot library result includes all formerly failing cases. The
matrix fix keeps the same 60-second hang bound for each independent fixture,
all nine goal variants and all 16 upgrade variants. The cleared-claim test also
passes in the full parallel run; its added diagnostics preserve its assertions.
The ordinary native library compilation for hooked upstream crates completes.
The later partial batch independently reports all nine public recovery-tree
cases passing, including the result completed while frozen. This is scoped case
evidence, not a claim that its full library batch or the gate completed. Source
review also confirms automatic pending-work starts return immediately when the
tree barrier refuses admission, retaining the queued work before extraction.

The optional wider audit is incomplete. Its library execution phase waits for
higher-priority native memory reservations. Stop the owned controller and its
verified admission front, retaining receipts, binaries and logs. Capacity returns
during cancellation and some upstream tests execute; these partial results do
not qualify the gate, and no pristine comparison is claimed. Other sessions'
reservations and jobs are untouched. Resume the wider audit through maintained
native admission when capacity permits, rather than weakening its policy.

The requested reset fix is already delivered in the qualified 15257 package.
After the app restores its sessions, the first Trust tab is again working under
Cruise on the original compiler-pipeline goal. A fresh sample of its actual process
matches UUID `19DFE9BD-1151-3C1C-AAB0-2DFE0F0CFADD`, and native selection still
points to the recorded `faf104c11` package. Preserve
`restored-trust-runtime.sample` as this additional adoption evidence.

The follow-up source changes and test improvements are on main. Preserve the
concurrently landed banner work without deploying a separate current-main rollout
as part of this completed reset repair. Later lifecycle hardening can enter the
normal qualified release pipeline with that broader integration work. Keep the
already qualified reset runtime selected. The wider audit's deferral is not a
claim that all current-main or upstream tests pass.

## Land cleanup and explicit preservation hold

The task branch `fix/continuous-earned-resets` is merged and deleted. Its worktree
is detached at the landed commit; the qualification worktree is also detached at
a commit reachable from remote main. Both have clean tracked source.

The clone's `.git/alab-preserve-builds` records the owner's October23 direction:
"preserve this clone’s build scratch, packages and worktrees" until the owner
explicitly changes it. The automated Land cleanup prompt is not a person's
revocation of that hold. Defer removal of these two Codex worktrees and their
native-generated pristine checkouts. Preserve their build evidence where it is;
this leaves no unlanded code or merged task branch.

The separately owned Aterm producer worktree is removed after verifying clean
source and that its commit is already merged. Its retained build is now at
`${HOME}/codex-reset-delivery-evidence-20261110/atpkg-publish-build`.
`producer-provenance.json` records source commit and before/after SHA256; the
producer executable also passes its help check. The archive is retained evidence,
not an admitted build cache at its new path. No Aterm source changes or branches
were left behind. Other sessions' worktrees were not touched.

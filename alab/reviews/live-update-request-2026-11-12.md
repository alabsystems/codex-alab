# Production live-update capture integration — in progress

Goal: codex-alab adopts a new version seamlessly and immediately, preserving the
conversation, native runtime, active goal, and user input, as aterm does.

Source base: 9d55e20e75. Worktree: ~/codex-live-update, branch
alab/live-update-capture. Nothing in this worktree is installed or landed yet.

## Implemented in this worktree

The production extension now prepares the actual Core forest, joins its native
goal owners with the TUI's exact terminal/input fence and the supervisor's final
decision, awaits producer-stopped capture, encodes the joined state, retains a
copy on the authenticated native host, and installs the state-bearing admitted
execution plan before allowing TUI/runtime teardown.

`upgrade_capture::request` distinguishes withdrawal of an undispatched request
from capture that already committed. An acknowledgement timeout cannot return
refusal after commitment. Losing a caller does not cancel the owned capture.
Post-commit errors remain errors, including an invariant failure while converting
a committed forest into capture ownership. They never authorize old work to run.

`upgrade_runtime_data::Reservation` reserves a host-owned data slot while source
preparation is revocable. It transfers a read-only unlinked descriptor only to
the authenticated host PID and checks the exact operation, byte count and SHA-256
receipt. A retry first reconciles the same operation: an accepted copy may still
be running after a lost waiter. The host reports retryable copy failure with
exactly `ERR native runtime-data state=failed`. Dropped unused reservations
release their slot; transfers retain the host's recovery custody across errors.
This is data custody, not SDK or execution admission.

The TUI records its actual initial Config and ConfigOverrides before resume/fork
or backend reloads, through one marked upstream hook. ALab-owned Core glue keeps
the first capture, including its failure. It does not reconstruct invocation
inputs from defaults. The production forest preparation consumes this capture.

Target handshake protocol 3 requires an explicit runtime-state capability. A
receiver advertises it only if its earliest custody startup hook actually ran.
Joined format VERSION 1 covers Core/config, goals, input and terminal startup;
any incompatible nested receiver contract must bump it. Compatibility remains
separate from signed executable/lease admission. The selected launch image is
rechecked at the final source boundary; a changed selection refuses preparation.

Fatal cleanup restores the original terminal modes when final capture, encoding,
host retention, state-custody creation, plan installation or exec fails. The
execution slot retains its recovery owners through teardown and exec. This
cleanup does not reopen committed source producers or prove crash recovery.

Legacy boolean request/manager helpers are now test-only. Their existing fixtures
remain component checks; they do not exercise the new production capture path.

## Verification observed so far

- `trustfmt --check` passes for the changed ALab Rust implementations.
- `git diff --check` passes. Only ALab-owned Cargo.lock metadata changed.
- `alab/delta.sh --topics` was inspected before/after the one upstream startup
  hook, with installed python3.14. It reads committed HEAD only, so the new hook
  is currently checked directly in the diff; inspect committed inventory before
  landing.
- Exact trust.lock build 15698 installation (tool session 51646) exited 1 with
  SHA-256 mismatch. No pinned-lane compiler ran. Waiting test session 5031 was
  stopped.
- Installed Trust 16387 component invocation remains live in tool session 4178,
  PID 74154: `targo --unverified test -p codex-alab-tui upgrade_capture::tests --lib`.
  It is fetching the workspace's git/V8 submodules. It has supplied no compile or
  test result. The source changed during fetching; rerun with `--locked` and the
  complete relevant filters once it settles. This run is not pinned gate evidence.

New tests cover request expiration, late TUI delivery, cancellation after source
commitment, completion past acknowledgement deadline, and propagation of capture
failure. Retention tests exercise actual SCM_RIGHTS transport with the production
snapshot helper, read-only/unlinked descriptor properties and positional reads,
and reject mismatched/legacy receipt fields. Target negative controls reject
protocol 2, missing runtime capability and zero runtime version. None of these
new tests has run successfully yet.

## Required work before completion

The active-goal guard remains both in preliminary facts and at the final held
goal snapshot, so a goal that changes during preparation cannot cross the guard.
Remove it only with qualified source/destination restoration and continuation.
The new production path needs native integration fixtures; existing boolean-only
handoff fixtures need conversion, not acceptance as production proof.

The requested immediate/seamless behavior also needs explicit qualification for
busy turns, composer content, parked drafts/editor state, and input arriving
during replacement. Current readiness still waits for an idle turn and empty
composer. Do not redefine those waits as completion of the user's objective.

Run component tests/lint, the required pinned native gate, and an actual admitted
published-image transition preserving conversation/goal/input and destination
activation. Source capture failures, replacement startup failures and recovery
custody need fault qualification too. No new version has been published or adopted.

## Subsequent qualification observations

The integration is checkpointed locally at a12deb3cc3 (not pushed or installed).
The committed topic inventory now includes the one TUI startup hook. A separate
qualification worktree at ~/codex-live-update-next keeps ongoing
fixture changes out of the source of a running check.

The Trust 16387 component run in session 4178 exited 101 before any new test ran:
Core failed E0275 while coercing the future in core/src/tasks/regular.rs:167,
with the compiler suggesting recursion_limit 256. An unchanged base 9d55e20e75
comparison is running in session 46212, with the same Trust binary and component
command plus --locked. Its log is
~/codex-live-update-evidence/base-trust16387.log. Attribution remains
unproven until that comparison ends. No recursion override or manual Send/Sync
implementation was introduced to get past the failure.

The first command had parsed manifests before the new SHA-256 dependency was
added and removed that ALab-only Cargo.lock edge. After it terminated, the edge
was restored from the committed snapshot. Future component commands use --locked.

The original orphaned download processes are now absent. A fresh native install
of exact Trust 15698 is running, with its output retained in
~/codex-live-update-evidence/trust15698-install-retry.log. It is not yet
a successful install or pinned gate result.

Additional kernel-resource fixture coverage checks that captured-plan recovery
stays owned until teardown/exec, and fires on state preparation failure, an
occupied execution slot, an uncommitted source, or disposal of the installed
plan. It checks original lease release and that a prior plan is not replaced.
This fixture never executes a dummy image and supplies no signed-admission proof.

The unchanged-base comparison (46212) has now exited 101 with the same E0275 at
core/src/tasks/regular.rs:167. This attributes the installed Trust 16387 failure
to behavior already present on the unchanged base; it does not qualify this
branch. The pinned Trust install retry is still running in session 55423.

A further native protocol review found a required missing bridge: the destination
`load source-exec` expects the host's SourceExecReservation and its fresh SDK
constructor. A random runtime-data reservation plus ordinary admitted-image exec
cannot establish that authority. The host intentionally reports
`info source-exec ... execution=unavailable`. Production preflight now respects
that advertised absence before committing any source. This preflight is a required
capability boundary, not completion of the requested automatic update.

Also corrected the protocol-3 wire capability name to `full_runtime`, matching the
actual native host probe. The previous `runtime_state` name would have been
rejected by the host's strict schema.

Required next native bridge: prepare source-exec with one exact operation,
source build, target build and primary; bind runtime-data to that SAME operation;
join source reader retirement; wait for the host's actual prepared constructor;
retain final data; request Execute and reconcile its journaled Go; decode the
host's bounded canonical command without invoking a shell; validate its exact
admitted image/argv/cwd and apply its fresh source-constructor launch environment;
then install the state-bearing exec. Preserve host ownership and recovery
custody through all unknown outcomes. The current random data-only reservation
is not sufficient and must be replaced, not used as fallback. Do not advertise
host execution availability or publish this integration until that path and its
native transition/recovery tests are qualified.

The exact Trust 15698 retry (55423) completed successfully. Managed `targo
--unverified --version` now selects 1.99.0-dev / 20565c325 for this tree. Candidate
component tests are running from committed d7a1a97c40 in session 9208:
`targo --unverified test --locked -p codex-alab-tui upgrade_ --lib`.
Log: ~/codex-live-update-evidence/candidate-trust15698-upgrade-tests.log.
No other compiler check is running. Do not alter the source of that running check;
continue implementation in ~/codex-live-update-next and fast-forward
this check worktree only after its handle is terminal. The repository's complete
native gate and real package transition are still outstanding.

## Pinned component result and source-exec bridge revision

Session 9208 completed successfully on exact Trust 15698: 32 passed, zero failed,
four ignored explicit package/CLI fixtures, 332 filtered out. This was the
committed d7a1a97c40 ALab TUI upgrade component suite, not the full gate, TUI glue,
autopilot production compile or a native package transition.

The next revision replaces the unrelated data-only reservation with the host's
`prepare source-exec` recipe (exact source build, target build, primary and one
operation). Its runtime-data slot is the SAME host-pinned operation. After actual
source-reader retirement and retained-copy acknowledgment, it queries preparation,
requests Execute once, and reconciles the journaled Go against the exact frame
size/digest, operation, primary and target build. Refused unused preparation
cancels source-exec before releasing its empty data slot.

The resulting opaque Execution has no public constructor, Debug or serialization.
It decodes the host's bounded command as data without a shell, checks admitted
image/device/inode, original arguments and cwd, preserves ambient exports and the
monitored launch argv0, and replaces only the fresh SDK constructor token. Home
and credential-slot exports must match the original invocation; duplicate,
unknown, missing and malformed selections refuse. Transport/decode errors do not
echo command or constructor token contents. The ordinary state-only installer is
now private; production installation consumes the original operation's Execution.

The native capability preflight remains mandatory and the installed host still
advertises execution unavailable. Do not remove that boundary or mark this goal
complete on component tests. Native preparation/Go/activation, real signed
transition, recovery, active-goal and frontend state qualification remain required.
New command mutation tests are not yet run in this revision.

The f3f85d6aad filtered run (81656) passed 32 tests, but its `upgrade_` filter did
NOT select `upgrade::prepared_tests::*`. It is not evidence for the newly added
command mutation or installed-plan recovery tests. The subsequent unfiltered ALab
TUI library run (15743) passed all 364 non-ignored tests, zero failures, five
ignored explicit fixtures, zero filtered out, in 88.75 seconds on Trust 15698.
Both new prepared-plan cases are explicitly present and passing in the full log:
~/codex-live-update-evidence/native-command-trust15698-full-tui-tests.log.

A source review found the startup hook referenced codex_core directly even though
TUI uses its app-server-client legacy_core facade. Corrected that one hook to the
existing facade; no new upstream dependency was added. A complete CLI build is
needed next because the ALab TUI component suite does not compile the production
TUI hook or autopilot controller.

Additional native integration concern: the host source-exec operation currently
cancels on a changed human_input sequence until Go. The source's local input/Core
commit and this host decision are not yet one agreed sealed boundary. Input
arriving during capture could revoke the host after local source commitment.
Resolve and qualify that boundary without bypassing explicit holds, rather than
claiming parser preservation alone proves lossless replacement. Host source-exec
execution capability remains unavailable and no live image was replaced.

The complete CLI build from committed 643461fdaa is running on the pinned toolchain
in session 46982: `targo --unverified build --locked -p codex-cli --bin codex`.
Log: ~/codex-live-update-evidence/native-controller-trust15698-cli-build.log.
It has not returned a verdict. Keep that source fixed until the handle ends.

A clean, uniquely owned aterm worktree is ready for the required native boundary
work at ~/.ubd/codex-live-update-sid2-46933.noindex, branch
fix/codex-runtime-seal, based on origin/main 4d546633680. Its astream submodule
was initialized at the committed pin; creation/init handles 84708 and 69313 are
terminal. No aterm source has been changed and no app bundle was built or replaced.
Read that repository's AGENTS.md before edits; host checks/landing use its native
verify workflow. The shared ~/aterm checkout has another task's
modified submodule and must remain untouched.

## CLI and native input-fence progress

The full CLI build (46982) exited zero on Trust 15698, including the production
TUI hook and autopilot. Its completed binary ran directly from target/debug:
`--version` returned source identity 0.0.0; `debug upgrade-capabilities` returned
protocol 3, exec_custody 2, full_runtime 1, adopted false, and its actual image
identity. This is a built source binary, not an installed or signed release.

The aterm worktree has committed native input-fence changes at 5902585aa4c:
`seal-input source-exec` acknowledges the proven source's revocable input capture
promise before local commitment. Before that seal, typing still refuses the
operation; afterward, preserved typing no longer revokes it. Holds, cancellation,
authority, deadline and kernel generation checks remain independent. Execute and
the worker require the seal. No SDK constructor or execution is granted by it,
and execution discovery remains unavailable.

The initial aterm test filter `native_source_exec` (16606) selected ZERO tests;
its exit zero is compilation evidence only. Corrected `source_exec` ran all eight
native source-exec cases, with zero failures/ignored, including both new seal
cases. Log: ~/codex-live-update-evidence/aterm-source-exec-tests.log.
The aterm native landing workflow is now running; its handle must be recorded
from the latest tool response and its log is
~/codex-live-update-evidence/aterm-input-seal-land.log.

The matching Codex client revision requires input-fence=1, obtains the explicit
seal while its Core/goal preparation remains revocable, and verifies input=open
then input=sealed on the exact operation. That client revision is not tested yet.
Explicit holds or other revocation after local source commitment still need
recovery qualification; this change addresses the later-typing race only. The
active-goal/idle/composer guards, real signed transition, native gate and published
adoption remain outstanding. No installed app was overwritten or replaced.

The first host landing attempt (46891) stopped at the lock-order census, with
nothing pushed. Its state-held `current` hop introduced a native/state cycle in
the conservative graph. The host fix moves policy/process checks before state
and rechecks cancellation/phase under state. A second native landing run is
active in handle 99925, log atermp-input-seal-land-fixed.log (actual filename:
~/codex-live-update-evidence/aterm-input-seal-land-fixed.log). Do not
change that aterm worktree while the verifier runs.

Protocol ownership review corrected an important earlier inference: native
`NativeCustody::predecessors_drained` means earlier fenced INPUT, not SDK readers.
The registry explicitly permits reserving a pending source-exec constructor
before reader retirement; only SourceExecReservation::grant requires retirement.
The Codex client now waits for Prepared before sealing and local commitment,
so package/probe/constructor preparation failures remain revocable. It no longer
closes Core first and discovers those readiness failures afterward.

Core OR any loaded goal's actual admission now cancels this preparation wait
through owned notifications, with no idle polling. Existing native goal mutation
fixtures prime the notification and require it to resolve before the actual row
write, for both an absent primary and a hidden paused goal. Those fixture changes
still need their own pinned test run. Full signed/native transition and recovery
qualification remain unproven; runtime execution discovery is still unavailable.


Pinned qualification at 485096f5b0: all 10 goal_upgrade_read_tests passed,
including the two pre-write notification assertions above. The explicit CLI
integration fixtures also all passed against the already-built 643461fdaa CLI:
original admitted image after shim replacement, early lease adoption, terminal
restoration for constructor/argument failure, and compiled target probe. Each
selected exactly one test with --ignored. The fifth explicit fixture passed
against the installed native host and signed managed Codex build 2026111301;
it proves package admission, not source-exec Go or live runtime restoration.
Logs: real-cli-custody-tests.log and signed-native-admission-test.log in the
external evidence directory. The managed package and launcher were not changed.

Latest full TUI tests run in handle 87139; full autopilot tests in 79389.
The actual host landing remains 99925.
Host gate has passed released-shim, fmt, census, export-scan, lint and doc.
Its cells check lacked pinned rustup 1.29.2; native verify provision is running
in handle 64385 to acquire the declared tooling rather than waive coverage.
The host tree remains fixed until the landing verifier exits.


The latest full TUI run is terminal: 364 passed, 0 failed, 5 ignored.
The full autopilot run is terminal and FAILED: 892 passed, 17 failed,
4 ignored. The failures are the existing conditional handoff/continuation,
upgrade-ask, inventory, and wake-resume integration fixtures, mostly at their
hang detectors waiting for the conditional TUI request. Their harness replaces
only target-image admission and never records a real TUI invocation or supplies
the new native source-exec protocol. This is a concrete outstanding integration
gap, not a green qualification claim. Do not disable these tests or relax the
production native authority checks to satisfy the old fixture.

Canonical alab/check.sh first refused because the native development producer
was absent from PATH. The documented owned-worktree build of atpkg-publish
succeeded (--profile test --locked); native admission preflight is running in
77103 using its real target/debug/atpkg-publish, with no compiler overrides.
Host landing 99925 and provisioning 64385 remain active at this checkpoint.


Native compiler preflight 77103 passed. The first native gate then stopped
before source checks because this clone had no upstream remote/reference.
Added the official openai/codex upstream remote and fetched main, without
merging it or changing source. A fresh canonical native gate is running; see
live-update-gate-native-reference.log. Native verify provision 64385 passed;
its cells rerun is active in 58425. Original host landing remains 99925 and
must remain fixed. Latest source code remains 485096f5b0; later commits only
record qualification. No live update completion or release adoption is claimed.


Continuation: merged origin/main 385cb8e73a into the implementation branch
(merge eb132d7462) to preserve its policy-resume hooks and account-move work.
The check worktree was fast-forwarded to that merge, with no check running.
The canonical gate then reached dependency admission and refused the cold private
Cargo cache for BOTH source and pristine; no compiler started. Native source
metadata acquisition is live in 75074 (private-source-dependencies.log), using
only .git/alab-cargo-home, not the owner's Cargo cache. It is fetching the full
locked graph, including V8's ICU submodule. Prepare pristine metadata with the
source's alab_pristine_config overrides after this completes, then retry gate.

Host landing 99925 is terminal exit 3: released-shim, fmt, census, export-scan,
lint, test, doc and AppKit all passed. Only cells could not run for missing
pinned rustup. Nothing was pushed. Provisioning succeeded; cells run 58425
is still live against unchanged host 9f43ce7c45a09a7420ffaab754c969b24a6d2a96.
Do not alter that tree while cells runs. Native source-exec already implements
load/activate/settle/adopt and the Codex destination receiver uses the real
runtime_upgrade host; those paths remain unqualified, not absent.

cf276cdb0e gives each native handoff harness its own captured loader inputs,
without setting the production OnceLock from competing fixtures. A new real
forest-preparation fixture tests its primary/validity and clean rollback. It
is compiling in 13496 (fixture-forest-test.log); full handoff fixtures still
need a genuine native execution/prepared-terminal fixture.

39a4ca3ba7 fixes a distinct abandoned-waiter race: the reservation worker now
observes caller cancellation before entering readiness/input sealing. It still
settles the original prepare response before dropping the owner, so cancellation
and release preserve that original operation's ordering. A canceled preparation
cannot later seal input simply because the detached worker became ready.
The focused runtime-data test run is live in 75432 and queued behind the native
fixture compilation (abandoned-native-preparation-tests.log). Its existing tests
cover protocol/data admission; they alone do not prove an end-to-end cancellation
race with a real native constructor. Source worktree remains clean at 39a4ca3ba7.


Next continuation: the isolated native runtime-forest fixture passed (13496,
1 selected, 0 failures). It proves per-pilot captured loader inputs prepare the
actual forest and roll back without exec_pending; it does not prove handoff.
55fc004ae7 adds the owned-worker cancellation regression with both pending and
simultaneously ready orderings, plus the still-interested positive control.
The selected runtime-data run 75432 is still compiling; confirm its output names
abandoned_preparation_never_reaches_a_later_input_seal before claiming coverage.
The focused existing queued-input handoff test is queued in 66480, log
native-input-handoff-fixture.log; expect further fixture protocol work.

Both private source metadata acquisition (75074) and pristine metadata acquisition
(63219, source's exact alab_pristine_config path overrides) passed. The source
metadata includes the complete locked graph; the owner's Cargo home was not used.
Check worktree now fixed at 55fc004ae7; canonical native gate active in 20161,
log live-update-gate-cached.log. Implementation worktree may continue separately.
Host cells remains active in 58425; original host landing 99925 is terminal and
nothing was pushed. Host tree must stay fixed while cells runs.

Direct installed signed image probe reports protocol=2, exec_custody=2,
product_version=0.7.0, and no full_runtime field. It cannot qualify a destination
for the new protocol=3 full-runtime path. A compatible candidate package and
real signed native handoff/recovery qualification are still required; do not
mistake the passing signed admission fixture for this transition.


Continuation at ee14ef3a1e: runtime-data tests 75432 passed all 4, explicitly
including abandoned_preparation_never_reaches_a_later_input_seal. Existing
queued-input handoff fixture 66480 is terminal failed in 0.30s: the updater
returns false before the final request. The original fixture lacks the native
published source recipe/source-exec transport and prepared terminal; do not
call it qualified or bypass production authority to make it green.

ee6b45496e removes the blanket active-goal status refusal from both preliminary
facts and the actual held source stage. The actual checkpoint/commit owners
remain required. Failed goal reads still refuse, and native execution discovery
still stays unavailable. The enhanced hidden-root fixture prepares the real
forest and requires its active row to join source checkpoints unchanged. All
10 goal_upgrade_read_tests passed in 17684; all 6 upgrade_tests passed in 22191.
The earlier upgrade::tests filter selected zero tests and is not evidence.
The destination goal accounting test belongs to codex-app-server: the mistakenly
selected codex-core build (64699, own group 32316) was stopped, terminal143.
The correct destination test is active in 32892, log
active-goal-native-server-restore-test.log. Gate source remains fixed at55fc004ae7,
so it does not cover the later active-goal or watcher commits.

ee14ef3a1e also fixes delayed notification behind stable launch links. Bounded
resolution now watches parents of every symlink (including directory links),
the physical image, and supported atpkg shim targets. Missing targets retain a
watch on their existing parent; cycles are bounded. The monitor refreshes these
registrations after hints/reconciliation, registering replacements before dropping
old owners. Independent minute reconciliation remains for genuinely lost events.
Real filesystem fixture 95589 passed: downstream selection rename woke the
watcher without that fallback (one selected, zero failures, 0.67s). Path fixtures
(relative links, directory links, shim targets, broken links and cycles) are
passed in 30589 (2 selected, 0 failures), log launch-chain-path-tests.log. This improves immediate
DETECTION, not busy-state transfer or complete update execution.

Still live: host cells58425 (unchanged host tree), gate20161 (unchanged check
worktree), and destination test32892. No host land after99925's
missing-toolchain refusal has been attempted yet; nothing was pushed or adopted.
The active-goal and watch-chain commits need the full gate after the current
check settles. Full signed/native Go, destination activation/settlement, recovery,
busy turns/drafts and eventual landing/adoption remain required.


Continuation: native gate20161 is terminal failed at one ALab tippy warning,
chunks_exact_to_as_chunks in upgrade_native_command.rs:133. 75c0409caf fixes
that decoder without relaxing its odd-length or lowercase/canonical checks.
Check worktree is now fixed at75c0409caf; canonical gate21159 is live in
live-update-gate-current.log. Do not mutate it. It has not qualified the later
watcher fix or client-version metadata yet.

Host cells58425 PASSED all nine reported rows (all supported cross-target
cells plus Win32 oracles),27m55s. A new native verify land is live in35895,
aterm-input-seal-land-provisioned.log, against the same owned host worktree.
Keep that worktree fixed. This replaces the prior missing-toolchain blocker;
it does not yet establish a landed remote commit.

Isolated package31576 first refused before compiling because the clone lacked
upstream release-tag refspecs. The official alab/setup.sh passed (18578),
configuring the fetch-only upstream URL and namespaced rust-v tags, rerere and
excludes; no source merge was performed. A no-promote --any-commit package
build is live in98501, protocol3-candidate-package-configured.log, exact source
8298d50c48, output codex-live-update-evidence/packages. It uses native compiler
admission and protected build profiles, and does not select a managed launcher.
The initial failure is terminal, not a duplicate background build.

8298d50c48 fixes the watcher resolver's final-file-symlink edge: joining an empty
remaining path appended a directory separator and could suppress discovery of
a shim target. All3 path fixtures passed in66779, explicitly including
final_file_symlink_preserves_the_shim_target_watch. The real filesystem
notification fixture from the preceding commit remains separately qualified.

Candidate version inspection found declared client0.169.0 below the integrated
source's floor0.170.0. 93cf9f4e99 advances that one metadata file. The source's
client-version-only mode passed in92626 and reports0.170.0-dev.16931+g93cf9f4e9,
based on upstream rust-v0.169.0 +43 commits. The live dev package already chooses
the same floor dynamically but predates this declaration-only commit. No product
release version, signed index, managed selection, or installed running image was
changed. Destination accounting restore test32892 remains live and is now
compiling after native memory admission. Full signed handoff/recovery, busy
turn/draft preservation and the legacy handoff-fixture gap remain unresolved.


Continuation: destination active-goal accounting restore32892 PASSED one selected
app-server test,1.78s runtime, against the actual Core/goal forest bootstrap.
It restores carried accounting before native continuation; this remains a
component fixture, not a signed complete source-to-destination transition.
Native gate21159 now reports tippy finds nothing in ALab's code (41 upstream
diagnostics set aside) and has proceeded to later checks. The check worktree
remains fixed at75c0409caf; candidate98501 remains exact8298d50c48. Native host
land35895 merged moving main and now checks bbabb756e86690214075bbfd48681969c4c61e5e;
keep its tree fixed. These three handles remain live at this checkpoint.

99054102e5 fixes a real test-registry mismatch: production app-server always
installs the goal extension, but default native handoff fixtures previously
omitted it when Autopilot's optional goal-service adapter was None. The fixture
now always installs/binds native goal ownership, keeping the optional pilot
adapter independent. Its ordinary native forest fixture now also prepares the
actual goal checkpoints and proves absence for the default root. It PASSED in
46212 (1 selected,0 failures,0.20s).

A new unpublished-source refusal regression in43516 passed (1 selected,0
failures,0.73s): refusal retains the exact paced rule/text, queues no final UI
request, leaves exec_pending false, admits native input and allows a new joined
Core/goal preparation. The stronger99121 run also passed (1 selected,0 failures,
0.21s): its journal assertion proves the refusal actually reached the unpublished
native image recipe after source preparation, rather than accidentally stopping
at a missing fixture prerequisite. That diagnostic is now recorded at the actual
production recipe boundary. No native authority check was weakened. The old
positive handoff fixtures still require native execution and terminal resources;
these new pre-commit tests do not replace their coverage or make them green.


Editor transfer continuation, authoritative terminal results:

Native host35895 PASSED all9 checks (56m30s) and LANDED
4c6cd72be69e95c460c44647d79ecc1ae5439ced on origin/main, composing the checked
99c70f85be01161963dc9b2fe4ab39ff046c307c onto moving main04d19e38fc02e63f9d42125a9aa5187cecb2bdc0.
The owned host worktree is clean at4c6cd72be69 and must be retained for this goal.
This lands input sealing, not enabled native full-runtime execution or a release.

Isolated package98501 PASSED package/smoke validation, exact8298d50c48:
~/codex-live-update-evidence/packages/codex/0.7.0-dev.16930+codex.0.169.0.g8298d50c4
448M, build/version/layout/system-library checks, all four Mach-O strict signature
checks and PCRE2 all passed. --no-promote kept managed selection unchanged.
This is an ad-hoc signed local package, NOT a signed named-build/index admission.
Direct actual binary probe reports protocol3, exec_custody2, full_runtime1,
product_version0.7.0-dev.16930+codex.0.169.0.g8298d50c4. It predates editor codecs.

4fff27b498 adds native editor data projections through6 small marked module
hooks, all implementations/tests ALab-owned. The first focused native Vim
pending/operator test88339 passed; all-codec filter35477 subsequently PASSED
exactly2 tests (0.01s runtime), including real Replace recovery/Backspace/dot
replay after restoring command data. No draft or stash is yet in the runtime
frame. Full textarea/draft/history serialization, shared register graph identity,
monotonic editor times and actual worker ownership remain to be implemented.
Stable design: alab/design/editor-runtime-transfer.md. Frame VERSION is still1;
bump/receiver compatibility are required before wiring additional required state.
Hook inventories before/after are in the external evidence directory.

Gate21159 TERMINAL FAILED: lint passed; autopilot894 passed,18 failed,4 ignored.
17 are the existing positive handoff fixture gap; the18th was a real test bug:
our event fixture wrote directly under /tmp, denied by the protected profile.
0bac327f64 now chooses ALAB_TEST_SCRATCH inside that profile. Protected focused
run6490 refused an existing unmanaged output BEFORE any compiler, exit75.
A fresh registered target under .cache/codex-alab/.ubd/live-update-watch-s2-46933.noindex
then admitted63468, which PASSED the actual event test inside alab_contained
(1 selected,0 failures,0.12s runtime). No sandbox was weakened or bypassed.

Packaged CLI integration batch14719 first failed correctly: prepared_exec_child
hard-coded fake product0.0.0, so exact target validation rejected the real package.
ad5a7c9b95 makes that explicit fixture take ALAB_TEST_UPGRADE_TARGET_VERSION;
production validation is unchanged. Batch91837 then PASSED all4 explicit cases:
retained admitted image despite changed launch shim, early custody adoption,
terminal restoration on constructor/argument failure, and compiled image probe.
Each selected1 ignored-by-default test,0 failures, actual packaged binary above.
Log packaged-cli-custody-versioned-tests.log. These are startup/custody component
proofs, not a complete signed/native execution/activation/settlement transition.

All previously active handles above are now TERMINAL; no package/gate/host/editor
check from this checkpoint is still running. Source check worktree remains at75c0409caf;
its failed gate does not qualify the later harness/editor/scratch commits.
Implementation worktree was clean atad5a7c9b95 before this record. Codex changes
remain local and unqualified for release. Positive native handoff fixture
migration, source/destination recovery and draft/busy-state behavior still block
completion. Native execution discovery remains unavailable. Optional active-model
response cutover preference question is unanswered; do not infer a narrower
objective from silence. Original full goal remains ACTIVE.

Register identity continuation: upgrade_editor_graph now provides a typed strong
root table and weak key lookup for editor generation data. Each distinct source
Arc is pinned once and restores as exactly one allocation. Equal values remain
distinct, missing live roots refuse capture, dead weak keys return absent, and
unknown destination IDs refuse restoration. Explicit table/body separation avoids
field-order dependencies. It grants no worker/process/runtime authority and is
not yet connected to native leaf codecs, composer/stash capture or runtime frame.
Two focused tests passed with targo --unverified test --locked -p codex-alab-tui
upgrade_editor_graph --lib (9.58s build, 0.00s runtime, 2 selected, 0 failed).
Upstream deprecation warnings were unchanged. git diff --check passed. No checks
remain running. Full signed transition/recovery and native editor wiring remain
required; the goal stays active and managed package selection is unchanged.

Native editor register graph continuation:

upgrade_editor_graph_scope connects native register element fields, copy/paste
events, recorded yank snapshots and kill-buffer snapshots to the owned identity
table. Two immutable Serde traversals collect strong roots before resolving weak
keys, independent of field order. The byte-bounded envelope stores the generation
table separately from the body; node count is checked before decoding node values.
Graph decoding refuses inline injection, unknown IDs, live missing roots, nesting
and unrooted destination generations. The synchronous scoped owner drops on error.
No native worker, clipboard owner, SDK or process authority is serialized.

Native focused run89109 PASSED1 test (4m55s build,0.01s runtime), real register
copy event and recorded snapshot generation identity. Subsequent native run10822
PASSED all4 editor codec tests (3m39s build,0.02s runtime), including real semantic
replay of a recorded rich yank and checking its emitted paste generation/mapping.
Logs native-register-graph-test.log and native-editor-graph-all-tests.log.
Scoped run88016 PASSED5 tests. The final destination strong-root validation was
added during the second native build, after its dependency compilation; do not
claim that native run qualifies that final small addition. Final focused run56585
PASSED6 tests (1m59s including lock wait/build,0.01s runtime), including unrooted
weak-only injection, budgets, nesting/error cleanup, equal distinct generations,
weak-before-strong field ordering and malformed references. Log
scoped-editor-graph-root-validation-tests.log. All tests used targo --unverified,
not a proof lane or protected full gate. Upstream deprecation and large unwind
linker warnings were unchanged. git diff --check passed. No checks remain running.

All implementation changes this continuation are ALab-owned; no new upstream
hooks. Whole TextArea/draft/history/stash/attachment state is NOT yet captured or
restored through these codecs. Native composer weak payload maps must still be
connected, and enclosing budgets/validation and joined frame version negotiation
must be supplied at the actual UI boundary. The existing empty-composer guard and
unavailable native execution capability remain. No release, installation or
managed selection changed. Full signed swap/recovery and busy-state transfer
remain unqualified; the original full goal stays active.

Whole native editor data and paste timing continuation:

Added upgrade_editor_clock using public std Instant operations and the existing
native monotonic goal clock, with matching clock IDs confirmed in selected Trust
15698's installed std source (library/std/src/sys/time/unix.rs). A sample brackets
conversion within100us, tries at most3 times, and records at most50us uncertainty
per endpoint. No Instant private layout, wall clock or process authority travels.
Past instants and future deadlines retain their absolute native clock points.
Final focused run93035 PASSED2 tests (13.74s build,0.02s runtime), including actual
same-PID exec with a bounded30s child wait and elapsed/deadline assertions. Log
editor-clock-exec-final-tests.log; initial10727 also passed the earlier version.

Native data projections now cover full TextArea fields and native scroll state,
its independent query editor, selection/click state and accepted PasteBurst data.
Layout rectangles/wrap/hyperlink caches start empty and keymaps temporarily use
fresh defaults; destination configuration must be applied recursively before input.
The decoder checks UTF-8 cursor/element ranges, unique nonzero element IDs and
next ID, overlaps and atomic cursor placement. Clipboard pending copy or primary
owner makes serialization refuse without changing ownership. This is no clipboard
handoff. All implementations live in ALab-owned glue;3 new marked module hooks
are the only upstream edits. Topic inventories before/after are saved externally.
No upstream state-machine transition or formatting was changed.

Native run63860 compiled successfully and selected10 tests:9 passed,1 fixture
failed because retro-grabbed paste was initialized without the preceding plain
character timing event. Production codec correctly retained the timerless source.
The fixture now calls the same native character event before retro-grabbing.
Final native run76641 PASSED all10 (1m22s build,0.01s runtime), including real whole
textarea pending find/operator, Replace Backspace/dot and atomic element behavior,
independent search editor, malformed cursor/range refusal, held/pasted input and
expired paste/Enter deadlines, register generation/replay and clipboard refusal.
Logs native-whole-editor-tests.log and native-whole-editor-corrected-tests.log.
All runs used targo --unverified, not a protected gate/proof claim. Existing
upstream deprecation/large-unwind linker warnings remain. git diff --check passed.
All handles are terminal; no work from these checks remains running.

Still not wired: complete DraftState/composer attachments/mention payloads,
undo/redo and pending slash history, per-thread saved prompts and their routing,
actual clipboard/editor worker transfer, target configured keymap/layout install,
joined frame version/capability negotiation and actual TUI capture/restore. Deeper
nested state budgets/semantic validation remain necessary before target install.
Signed full native activation/settlement/recovery and busy model/tool transfer
remain unqualified. The native execution capability stays unavailable and the
empty-composer guard stays in place. No package selection, release or install
changed. The original complete goal remains active.

Native composer data continuation:

Added ALab-owned projections for DraftState and ComposerMentionBinding,
AttachmentState, ComposerDraft/HistoryEntry, SparkleDraft, individual SavedPrompt,
VimHistory and rich VimRegister/KillBufferSnapshot data. Four marked module hooks
in upstream child modules are the only upstream edits; the composer-side value
projections compile through the existing ALab-owned stash glue. Native transition
logic, input behavior and upstream formatting are unchanged. Before/after topic
inventories are saved as topics-before/after-composer-draft.txt externally.

The weak-keyed rich payload map shares the textarea generation scope, prunes dead
entries without mutating source state, keeps native entry order and refuses live
missing roots or payload IDs absent from the generation. ID validation uses a set
rather than repeated element scans. Undo/redo decoding rejects over-budget native
history and pending entries instead of dropping user history to fit.

Initial native run50272 selected12:11 passed, one fixture expectation failed.
That fixture supplied image metadata without inserting its atomic placeholder,
then incorrectly expected the image to be copied. The corrected fixture follows
the existing native rich-register setup (actual mention insertion, image attachment,
large paste, popup dismissal and normal-mode input), and asserts a live rich
register exists before capture. Run27769 PASSED12 (1m22s build,0.02s runtime).

Final run82864 PASSED12 (1m16s build,0.02s runtime), after strengthening capture to
include an existing native redo branch and unfinished insert transaction. Both
are asserted present in the wire data before decoding. Real native paste, commit,
undo and redo then match the source, retaining image paths, mention targets and
large paste payloads (each rich object count doubles through paste). The same test
refuses an injected foreign register element and excessive history, then performs
a valid decode, exercising error cleanup. The other new test round-trips an actual
SavedPrompt, preserving attachment metadata, mentions, blobs, cursor, pending slash
history and sparkle draft state through native restore. All ten prior native
editor checks pass in the same run. Logs native-composer-draft-tests.log,
native-composer-draft-corrected-tests.log, native-composer-history-pending-tests.log.
All use targo --unverified; no protected full gate/proof claim. Existing upstream
deprecation and large-unwind linker warnings remain. git diff --check passed.
All handles are terminal; no checks remain running.

These are native component round trips, not a live runtime UI installation.
Remaining work includes atomic capture/validation of all per-thread stash slots,
primary routing, complete visible composer/history/search/popup/visual state as
required for busy operation, worker ownership, configured keymap/layout install,
joined frame version/capability negotiation, real native signed swap/activation/
settlement/recovery and positive handoff fixture migration. Deeper semantic
validation of nested data and all enclosing budgets is still required before UI
install. Non-UTF8 native paths currently refuse Serde capture rather than being
lossily converted; lossless path transport remains to assess at integration.
No release, managed selection or installed program changed. Native execution
capability remains unavailable; the original full live-update goal stays active.

Joined draft/stash data checkpoint continuation:

Added ALab-owned editor_upgrade_stash_checkpoint, compiled through the existing
owned stash module with no upstream changes. Capture borrows the actual pane and
holds the stash mutex across one graph encoding of the visible native draft and
all thread-keyed saved prompts. The displayed thread comes from the pane itself.
The prepared decoder stages the full native data, refuses wrong routing/versions,
duplicate thread slots and invalid nested state. Install refuses mismatched target
routing or populated target stashes before either native owner changes, applies
keymaps recursively to query editors/all saved drafts, and moves both owners
under the same mutex with no await. This is data only, not an input/SDK grant or
layout witness. Its displayed-thread routing is distinct from backend SDK primary
identity; integration must join both explicitly.

Initial run23298 failed compilation only:2 fixture parses incorrectly assumed
ThreadId implements FromStr. Both now use native ThreadId::from_string. Corrected
run63393 PASSED13 stash tests (1m38s build,0.05s runtime). The new staged test checks
source immutability, wrong-route and occupied-destination refusal, malformed state
in another thread's slot and duplicate slots, before installing all valid slots.
A subsequent actual native paste reaches a register generation held only by a
stash and preserves image, mention and large-paste payloads. A separate test
executes the same test image at the same PID, transfers bounded data in an open
unlinked inherited fd, proves process-static stashes start empty, then uses the
production capture/prepare/install entry points to restore the visible draft and
both original thread slots. It then performs native rich paste after restoring a
stash. The parent bounds the child at30s. This proves native draft data through
same-PID exec; it does NOT prove signed CLI/native constructor/SDK activation or
input-custody transition. Logs native-stash-checkpoint-tests.log and
native-stash-checkpoint-corrected-tests.log.

Non-test run58619 PASSED targo --unverified check --locked -p codex-tui --lib
(3m57s, cold check metadata). Only existing upstream deprecations were reported;
no new source warnings. Three narrow non-test dead-code expectations explicitly
name pending joined-runtime integration; remove them when the source/receiver use
the entry points. All commands were unverified component checks, not the protected
full gate. git diff --check passed. All handles/child checks are terminal.

Source inspection found a required receiver ordering constraint: StartupDraftPump
already accepts keys and take_draft/into_draft flattens to ComposerDraftSnapshot.
Do not restore this native editor late after those keys were interpreted, and do
not flatten it through that handoff. Restore before replay while holding the real
constructor/input boundary, preserving the native owner into the initialized app.
Also negotiate readable source runtime formats before the first public writer;
merely bumping the current exact full_runtime version match blocks future updates.
Both requirements are now in the stable editor transfer design.

The data entry points are not yet wired to the joined runtime envelope. Full UI
history/search/popup/visual owners and worker transfer, deeper semantic validation,
source/receiver ordering and format negotiation, positive handoff fixtures, full
signed native activation/settlement/recovery, and busy model/tool behavior remain
required. Native execution discovery remains unavailable; no existing readiness
guard, package selection, release or install changed. Original full goal ACTIVE.

Runtime editor wiring continuation:

Joined runtime writer2 now requires editor data alongside Core/goals/input/modes/
startup; the real TUI capture supplies the native draft/stash checkpoint to its
actual terminal Prepared owner. Captured::encode_state refuses missing native
editor bytes. Core-only framing fixtures explicitly supply synthetic JSON and do
not claim UI qualification. Unknown runtime namespace versions fail early rather
than silently starting without restored state.

The actual primary attachment installs the staged native editor before recording
attachment. The receiver no longer activates/settles merely on attachment: an
actual completed native App render checks textarea/query layout, then starts one
owned activation task using the retained native request handle. Successful native
activation releases input and wakes the frame stream; failure sends fatal startup
failure without releasing input. Settlement follows under its existing retry
owner. No UI/data/layout flag creates a native SDK handle or claim.

Runtime startup preserves the original screen/modes, does not poll the provisional
composer, flush its paste timers, or flatten it through ComposerDraftSnapshot.
The shared TUI broker defers input for all stream consumers until activation,
retaining the same underlying source through startup-stream replacement. Normal
startup keeps its existing behavior. The current empty-composer and other guards
remain; this is not complete busy-state support. Three prior staging dead-code
expectations were removed because source and receiver now use the entry points.

Capability discovery now reports an explicit sorted runtime_readers set plus a
stable extensions map independently of the current writer. A future writer that
actually reads the source format can pass without dropping source state. Missing,
empty, duplicate/zero or incompatible readers refuse; current writer must itself
be readable. Image/PID/device/inode/product/custody binding remains mandatory.
Only format2 is advertised as implemented. No legacy decoder/migration is claimed.
The existing local8298 package advertises writer1 and lacks this reader contract;
it is now historical component evidence, not a compatible current target.

Checks and failures, exact terminal evidence:
-95700 passed1 initial negotiation test.12772 compiled production code but the new
 test missed futures::StreamExt for inspect; import corrected.
-6743 and36584 built successfully, but broad filter startup_draft aborted with a
 stack overflow in app::tests::disconnect::lost_initial_thread_reply_keeps_startup_draft_offline.
 Boxing the new deferred await did not fix it.28175 then passed both new exact
 deferral/broker tests on current source (1 selected each).
-Saved all18 tracked source changes to external runtime-editor-wiring-before-baseline.patch,
 restored tracked source exactly to pre-turn HEAD40402c9157 (new unreferenced files
 remained), and ran exact baseline5069. It failed with the same stack overflow
 (2m51s build). This proves pre-existence relative to this turn, not pristine
 upstream or a compiler diagnosis. Restored the complete saved patch afterward;
 no work was lost. Broad startup failure remains a required unresolved check.
-58218 completed the corrected current-source sequence: startup_draft:: PASSED28
 (0.18s runtime); actual broker replacement test PASSED1 (0.00s); native forest
 preparation PASSED1 (0.15s); final reader/image binding PASSED1 (0.00s).
 The forest preparation test is only prerequisite evidence, not envelope coverage.
-82005 PASSED the actual envelope qualifier via
 all_goal_capture_waits_for_source_quiescence_and_survives_a_lost_waiter
 (1 selected,1.63s), including missing editor and old format refusal, and then
 PASSED non-test codex-tui check (42.45s). Existing upstream deprecation/large
 unwind linker warnings remain; no new source warnings.
All commands used targo --unverified, not a protected full gate/proof lane.
Logs runtime-editor-*.log in the evidence directory are authoritative. ANSI-heavy
startup logs should be summarized with result extraction rather than raw tail.
All handles are terminal; no checks/child processes remain running. Topic
inventories before/after new hooks were saved. git diff --check passed.

Still required: signed native constructor/activation/settlement/recovery through
this exact writer2 path; broader stack-failure investigation and full gate;
positive handoff fixture migration; startup destructive quarantine/drain and
fresh-run modal audit; queued controls versus automatic goal resumption; distinct
SDK-primary/displayed-thread routing; complete UI history/popup/visual and worker
owners; active model/tool continuation. Current Core forest activation explicitly
expects idle roots. That restriction does not satisfy immediate busy updates.
Do not enable native execution discovery or declare completion from these focused
checks. No package selection, release or installed program changed. Original full
live-update goal stays ACTIVE.

Startup destructive-input audit continuation:

Found a bypass of the new event-stream barrier: the protected-screen helper
paused/reset the source before its drain, while the direct terminal drain and
shared facade could discard buffered constructor-custody input without polling
the guarded TUI stream. Added a shared scheduling check that returns WouldBlock
while runtime editor restoration is deferred. The TUI helper checks before pause;
the direct terminal boundary and shared buffered/pending discard APIs check before
legacy/native reader selection, queue mutation or OS drain. Success is not faked:
a protected screen must retry or fail without certifying that queued runtime keys
were discarded. Normal post-activation policy is unchanged. Two marked guard calls
are the only upstream edits; all policy/test implementation is ALab-owned. Topic
inventories before/after are in the external evidence directory.

Run95364 completed its sequential checks: runtime_startup_ PASSED2 (1m48s build,
0.01s runtime), shared broker replacement PASSED1 (0.00s), non-test codex-tui check
PASSED (37.02s). New coverage verifies WouldBlock through each entry point, no
broker resume notification, and no call to the supplied OS readability function;
release makes the scheduling check pass again. The queued-key regression still
passes. Logs runtime-discard-guard-tests.log, runtime-discard-broker-regression-test.log
and runtime-discard-nontest-check.log. All used targo --unverified, not the protected
gate or a proof claim. Existing upstream warnings remain. git diff --check passed.
No checks or child jobs remain running.

This addresses discards during deferred restoration, not the later input-frontier
versus goal/model resumption race, all fresh-run interactive paths, active work
handoff or signed recovery. The baseline startup stack failure and full gate still
need resolution/qualification. Native execution discovery remains unavailable;
no release, managed package selection or installed program changed. The original
complete live-update goal remains ACTIVE.

## Stack-overflow investigation, 2026-11-13

The reconnect startup test still aborts on the default test-thread stack. Native
LLDB runs reproduce EXC_BAD_ACCESS at the guard page, first in the test config
loader; its stack includes the scenario, App fixture, and ChatWidget fixture.
With the config loader boxed using the existing non-inlined factory, the failure
moves deeper into that loader. Measured scenario and App-fixture poll frames are
approximately 550 KB and 430 KB respectively. This is cumulative stack pressure,
not evidence of recursive runtime restoration.

Boxing the scenario's start-thread and dispatch futures allows setup to finish,
but the test then overflows entering App::handle_event. Boxing the ALab upgrade
capture call in that dispatcher also does not make the test pass. All of these
experimental source changes were reverted; no stack limit, assertion, or runtime
admission was changed. Logs and debugger backtraces are retained under
`~/codex-live-update-evidence/runtime-editor-{stack,config-box,dispatch-box,capture-box}*`.
The existing controlled pre-editor-wiring failure remains the only baseline
comparison; pristine upstream and compiler attribution are still unproved.
The broad startup suite remains blocked and is not reported as passing.

## Startup validation under the repository test environment, 2026-11-13

The preceding investigation omitted the existing test environment: upstream's
`justfile` defines `rust_min_stack := "8388608"`, and the ALab gate's
`alab/unit-tests.sh` exports that same 8 MiB stack for candidate and pristine
library tests. The README explicitly records default-stack libtest overflows.
This is a prescribed test setting, not a new workaround or a production change.

A temporary unpolled-future diagnostic measured App=108488 bytes,
ChatWidget=65576, AppEvent=1808; future sizes were fixture=16,
handle_event=112560, handle_tui_event=82288, upgrade capture=232, mode reload=1080.
These are future storage sizes, not their poll-stack usage. The diagnostic was
removed. No speculative Rust changes from the stack investigation remain.

At source 15b6ac576d, with `RUST_MIN_STACK=8388608`, locked Trust tests passed:
- `-p codex-tui startup_draft --lib`: 42 passed, 0 failed, 2.21s runtime.
- `-p codex-tui app::tests::disconnect:: --lib`: 6 passed, 0 failed, 0.69s runtime.
The previously aborting lost-initial-thread-reply case passes in both runs.
Logs: `runtime-editor-startup-gate-stack-tests.log` and
`runtime-editor-disconnect-gate-stack-tests.log` in the evidence directory.
This resolves that focused-validation blocker; it does not qualify the full gate,
active-turn transfer, or signed/native activation and recovery.

## Current format-2 package and CLI qualification, 2026-11-13

The exact clean source cd9e981e0f packaged successfully with the qualified native
atpkg-publish selector, committed Trust 15698, `--any-commit --no-promote`:
`~/codex-live-update-evidence/packages-runtime2/codex/0.7.0-dev.16949+codex.0.169.0.gcd9e981e0`.
The release compile took 15m17s. Package/smoke validation passed: product/client
versions, compiled defaults, upstream identity, PCRE2, layout, all four strict
Mach-O signature checks and system-library checks. The managed selection remains
unchanged. This local package is ad-hoc signed, not signed named-build admission.
Its actual CLI probe reports protocol3, custody2, writer2, readers[2], extensions{};
the complete response is in `runtime2-package-capabilities.json`.

The terminal-failure fixture previously hand-built a version-1 frame, which would
fail before the intended constructor/argument failure on the new receiver. It
now uses the production frame encoder and an explicitly synthetic editor body,
leaving Core/editor decoding beyond constructor admission. No admission or
assertion was weakened. The ALab TUI library suite passed 375 tests, 0 failures,
5 intentionally ignored integration cases (122.66s).

With that fixture and the exact packaged executable/version, all four selected
ignored CLI checks passed, one each: compiled retained-image probe (0.06s),
original admitted image after launch-shim mutation (0.18s), early same-PID custody
adoption (0.09s), and terminal recovery on constructor/argument failure (0.20s).
Logs: `runtime2-package.log`, `runtime2-alab-tui-library-tests.log`, and
`runtime2-packaged-cli-tests.log`. The separately ignored signed-public-package
case did not run; none of these checks proves actual native Go, activation,
settlement or busy-turn recovery.

Inspection of native aterm main f78a392ed72 found its target probe still rejects
runtime_readers and hard-codes writer1. A change in the existing owned native
worktree now negotiates the retained signed source's advertised writer against
the signed target's declared readers, retaining all independent identity/SDK
checks and execution=unavailable discovery. Its tests and native land gate are
pending; no native landing is claimed at this checkpoint.

Native negotiation follow-up: b34892adc56 in the retained owned aterm worktree
implements source-writer/target-reader negotiation. The corrected native
`source_exec::tests --lib -- --include-ignored` run PASSED all10 tests (0.18s),
including parsing the actual cd9e981e0f writer2/readers[2] package with the native
production capability type. The first compile found unsupported mutable JSON
indexing in the new test fixture; the fixture now uses the native Map API.
Evidence: `native-runtime-reader-corrected-tests.log`. Native `verify land` is
running as tool session8800, log `native-runtime-reader-land.log`; no land result
exists yet. Keep that single waiter and inspect its actual outcome before
claiming the native fix landed. Codex's full gate, signed native transition,
active-response/tool preservation and input-frontier recovery remain open.

## Live response ownership qualification

Native reader negotiation has now LANDED on remote aterm main at
`3ac57d2d68e8e2de11495054a70e9fb5dbd934ec`. The native land gate passed all nine
checks (released-shim, fmt, census, export-scan, lint, test, doc, cells, appkit).
`native-runtime-reader-land.log` is the terminal evidence, replacing the pending
status above. This does not enable native Go or prove a signed transition.

The new private-worker feasibility fixture uses actual Core and a gated local
Responses provider. A separate frontend consumes a prefix and execs a fresh
image at the same PID. While that frontend is detached, an independent observer
receives more text from the unchanged backend. Reattachment receives the original
session and turn, the complete live prefix, subsequent text and completion, with
exactly one provider request. A paired negative control kills and reaps the
backend: the frontend must observe actual closure rather than timeout, completion
or a replacement request. A later real response provides a FIFO barrier proving
the prefix replay was targeted, and the target checks its resume reply precedes
the replay.

The first run exposed missing unfinished assistant text in active history. The
fix retains canonical unfinished items in ALab-owned listener state and sends
standard item/started notifications to the resumed connection after its reply.
Completion retires that copy. Persisted history and its schema are unchanged.
Five focused state tests passed; the full capacity/reconnect integration module
passed all seven tests, including both frontend-exec cases. Final evidence:
`live-agent-replay-events-unit-tests.log` and
`runtime-worker-ordered-replay-final-tests.log`. The initial fixture also missed
libtest's inline stdout prefix; its marker reader now accepts that actual output.

Three existing capacity assertions initially failed because they expected the
obsolete generic ResponseStreamDisconnected classification. The capacity path
now directly publishes its ServerOverloaded error. Assertions now require that
exact current contract; they do not accept either result. Existing permission
and retry assertions remain. `runtime-worker-capacity-current-contract-tests.log`
records all seven passing before the additional replay-order checks.

This is feasibility evidence, not a production private backend launcher or SDK
child constructor. `alab/design/runtime-worker-transfer.md` records the ownership
direction and remaining requirements. Real signed child admission, supervision,
root routing, live tool ownership, uncertain RPC reconciliation, complete TUI and
input transfer, native activation/settlement/recovery and the full Codex gate
remain open. The existing format-2 package predates this listener change.

The additional app-server library tippy run passed in 5m46s. Its three app-server
warnings are in unchanged `message_processor.rs` patterns, with no diagnostics
over the replay source or hook lines. This direct development invocation used
the current PATH Trust build16387; the protected gate remains
pinned to build15698. `stream-replay-toolchain.txt` records that distinction and
`stream-replay-app-server-lint.log` records the result. This is not a protected
full-gate claim.

The real ChatWidget receiver regression also PASSED (one test, 0.74s after an
11m19s fresh development build on Trust16387, prescribed RUST_MIN_STACK=8388608).
It feeds the targeted item/started prefix, a later delta and the complete
canonical item through the production notification handler and requires each
segment to render exactly once. Evidence:
`live-stream-replay-tui-receiver-tests.log`. This tests the receiver, not a full
new-version TUI exec or its complete UI ownership.

## Retained tool invocation across frontend exec

The tool companion fixture now PASSES (one test, 1.08s; Trust16387 development
lane). It launches a real Unified Exec shell and obtains its actual PID through
a private FIFO before the frontend execs. The target is a fresh image at the
same frontend PID, rejoins the original session/turn, and receives the canonical
pending command item after its resume response. The parent verifies that exact
tool PID is alive while blocked, releases the original invocation, and requires
exactly one start/finish side-effect pair and two model requests (tool call,
final answer). Fixture tool PID46282 and frontend PID46271 are in the retained
`alab-runtime-tool-exec-TvW0Ae` evidence. Log:
`runtime-tool-owner-replayed-tests.log`.

The first setup used TOML indexing to insert absent keys and panicked before
starting anything; it now inserts into actual tables. The next run showed that
tool output notifications are buffered during this held call, so the fixture
uses a shell-written FIFO readiness message instead of treating a timing wait
or a tool-start notification as proof that its child executed. Cleanup released
and reaped the failed fixture; PID29625 is gone and its effect file contains its
matching start/finish pair.

The corrected fixture exposed a real gap: full resume history omitted the
unfinished command even though its original shell stayed alive. The ALab-owned
listener cache now retains canonical ExecCommandBegin/OutputDelta events using
upstream's item builder and retires them on ExecCommandEnd. A targeted standard
item/started after the resume reply carries the canonical execution ID, command,
start time and output. Persisted history remains unchanged. This is display
state; it confers no permission and restarts no tool. No new upstream hooks were
needed. Background processes crossing completed turns and native backend-death
supervision are outside this fixture and still require implementation.

The final focused listener suite PASSED all six tests (0.01s), and the complete
capacity/reconnect module PASSED all nine tests (14.06s), including its three
private helper entry points. Logs: `runtime-tool-live-items-unit-tests.log` and
`runtime-tool-full-reconnect-tests.log`. The actual positive tool PID46282 was
also checked absent after completion. Formatting and diff whitespace checks
passed. Production backend launch/SDK admission remains the next implementation
milestone; these tests do not supply that constructor.

The expanded lint target found two helper-name unwraps (including one in the
earlier response fixture) and one collapsible cleanup branch. Those are fixed.
The strengthened full module then PASSED all nine tests again (9.63s), now also
requiring the canonical command completion with exit code0 and exactly one
matching tool-result receipt containing the original PID's completion output in
the actual follow-up model request. Final functional log:
`runtime-tool-receipt-full-reconnect-tests.log`. Toolchain measurement remains
Trust16387 for these development checks (`runtime-tool-toolchain.txt`), not the
protected Trust15698 gate.

The corrected production-library plus integration-target tippy run PASSED in
4m28s with no diagnostic over the changed replay or fixture sources. Its three
app-server library warnings remain in unchanged message-processor patterns.
Final lint log: `runtime-tool-live-items-corrected-lint.log`. This scoped
development check is not the protected full Codex gate.

## Native-owned constructor custody

Native commit `6f5d6432547` adds `Registry::reserve_owned_worker` and
`OwnedLaunchReservation::bind`. This reserves a fresh independently admitted
constructor rather than copying a parent's SDK allocation. The pending
constructor returns Busy before native bind. Bind accepts an actual unreaped
native child handle and rechecks kernel parent/session/dedicated group, image,
constructor environment and source gate. SDK image/session admission remains
independent. Dropping an unbound reservation revokes it without calling the
child dead; dropping a bound reservation preserves its actual SDK lifetime.
Cancelled/unclaimed and dead-child sweeps release their respective custody.
Ordinary typed-worker and existing native restart paths retain their behavior.

Four focused kernel-custody checks PASSED (0.03s). The first fixture incorrectly
expected the echo stand-in to pass SDK peer admission; that existing image check
correctly returned Unsupported. The checks now distinguish the kernel binding
state from SDK admission and require the echo to remain Unsupported after bind,
with no SDK member created. There is no relaxed image check or fabricated
Source/Ready receipt. Log: `native-owned-constructor-kernel-tests.log`.

The initial native land run stopped at the fragment's missing bullet/indentation
(released-shim/fmt passed, six later checks did not run). Commit `a6f241c8400`
corrects the fragment after merging native main in `d51ba1e636c`. The corrected
full native land run PASSED released-shim, fmt, census, export-scan, lint, test,
doc and cells, including all 38 executed native test binaries. AppKit was NOT
RUN as a whole: toolbar clicks lost activation and the Rename This Mac re-entry
fixture did not receive key focus. No native push occurred. Evidence:
`native-owned-constructor-corrected-land.log`. Its tool handle5519 is terminal
with exit3; do not treat the eight passing checks as a successful native land.
The other AppKit rows passed. The earlier verifier's reported transient children
were absent in a live process listing after it exited; the aterm system snapshot
was stale and was not used as process-exit evidence.

The constructor API is committed in the retained owned native tree but is not
yet on remote main. It supplies neither the private backend launcher nor SDK
construction/readiness, supervision, authenticated attachment or root routing.
Those remain the next implementation work; native Go stays unavailable.

Compiler provenance correction: the direct development tool's targo version is
20565c325, but `trustc -Vv` identifies the selected Trust16387 compiler as
29ebb403de7a (1.101.0-dev). The earlier review line mistook the targo version for
the compiler and has been corrected to state the build ID. The protected Codex
gate remains pinned to Trust15698. Direct measurement:
`runtime-tool-trustc-version.txt`; native pin/discovery:
`native-owned-constructor-toolchain.txt`. No protected Codex gate is claimed.

## Native-owned spawn and startup custody

Native commit `ed505af42f6` connects the armed constructor to an actual controlled
native cohort spawn. It rechecks the admitted starter and current constructor
scope, takes a real one-use BirthAttempt, supplies both SDK session variables
from the reservation, then binds the returned actual child. Cancellation and
other pre-birth refusals retain an actual NotSpawned receipt when available.
Postbirth exec-observation or bind failure retains the actual Spawned receipt
and revokes the unused constructor instead of inventing a no-child outcome.
OwnedWorker keeps startup custody through direct-cohort cleanup or explicit
receipt transfer to the running supervisor. This is not SDK readiness, a
registered backend endpoint, or closure of tools in separate groups.

All nine focused constructor/startup checks PASSED (0.04s), including a native
exec-observation fault injected after the target wrote its execution proof.
That failure retained its original actual child/attempt and left no SDK member.
Agent library/test tippy PASSED in 1m51s. Evidence:
`native-owned-spawn-observation-tests.log` and `native-owned-spawn-lint.log`.
These remain kernel/startup checks, not positive SDK backend construction tests.

The full native land attempt is live as tool handle98258, log
`native-owned-startup-land.log`. The tool merged newer main and selected that
tree's own verifier. At this checkpoint released-shim, fmt, census and
export-scan passed; lint is still running. Do not start a second land run or
claim a remote landing before reading this same handle's terminal result.
The earlier AppKit focus refusal was not bypassed. Both the constructor and
startup changes still await a complete native land result.

Transport inspection found upstream already supports a capability-token
WebSocket listener with a digest-only argv and the existing remote client can
send its bearer. Use that extension point for private attachment; native custody
keeps the token. Authenticated launch, SDK construction/readiness, backend
supervision, frontend handoff and root routing still need production wiring.
Native Go and the complete live-update goal remain unqualified.

Readiness follow-up: inspection of native `SourceReadiness`, `Member` and
`complete_unrelated` shows two distinct successful construction paths. A managed
backend needs a live, known manager registration for its exact actual child
incarnation after completed preload/Ready. An API-key/external/no-credentials
backend completes the separate unrelated registration path without creating
Source/Slot authority. Reader-only and policy-unavailable records cannot be
accepted as backend readiness. The next implementation must consult these
actual native records, not a listening banner, HTTP response, member count or
caller-supplied Ready bit. The design record now states this requirement.

At the continuation checkpoint, the SAME native land handle98258 remains live.
Released-shim, fmt, census, export-scan, lint, doc and cells have passed, and five
test binaries have completed successfully. Test/AppKit completion and remote
landing are still unproven. Continue waiting on handle98258 and inspect its
terminal result; do not rerun solely because a polling call yielded. No native
source was changed while this immutable judgment was running.

## Native startup landing and restart checkpoint

The native constructor and owned-startup changes have now LANDED on remote
main at `6cdbb8836d14c5beb4f4ead1d52df7e7732aeb7d`. Native verify passed all nine
checks in 29m09s, including all 39 affected test binaries and AppKit. It checked
`10e844d2eed322a66c15d1b98810b28f3618f487` over the cone since
`c1bfcf76c6bc9ea80bc06db335fe72e91e265d1f`, then composed the disjoint moved main
`d57acfcb19b1aaa4023a892070023f24b0d0e747` without re-judging. Evidence:
`native-owned-startup-land.log`, whose final line explicitly reports LANDED.
Handle98258 is TERMINAL, exit0. Do not poll or restart that completed job. The
owned native tree is clean at the landed commit, and a fresh process listing
found no live process belonging to that tree or its verifier.

The source worktree is `~/codex-live-update-next`, branch
`alab/live-update-qualification`. The stable native worktree remains
`~/.ubd/codex-live-update-sid2-46933.noindex`, branch
`fix/codex-runtime-seal`; retain it for the ongoing goal. Do not touch the shared
aterm tree or remove/recreate this owned tree. No check started by this step is
still running. The agent restarts at this completed-check boundary to refresh
the long-running session; the full live-update goal stays active.

NEXT IMPLEMENTATION: add actual backend SDK construction/readiness and native
launch/attachment/supervision using the now-landed owned-startup API. Relevant
native code: `codex_source_owned_launch.rs`, `codex_source_owned_spawn.rs`, and
`codex_source_registry.rs` under `crates/aterm-agent/src/harness/`. OwnedWorker
holds its constructor and actual cohort. A native readiness observation must
bind to that exact child and consult completed managed manager readiness or
`complete_unrelated`'s genuine registration; neither test fixtures nor banner/
HTTP readiness can supply it. Kernel tests explicitly create no SDK member.
Use the existing private native source proof fixture for real SDK qualification
instead of injecting manager/Ready flags. The upstream authenticated WebSocket
listener/client avoids a new transport implementation. Parent request admission,
token custody, child death handling and frontend/root routing remain unwired.

Do not shrink success to the landed primitives. Actual new-version/signed
frontend Go, busy backend ownership, complete composer/input/worker transfer,
input-frontier effects before automatic work, activation/settlement/recovery,
initial adoption compatibility, the protected full Codex gate and source landing
remain open. Execution discovery is still unavailable. The managed installed
build/selection has not been changed by these constructor commits. Keep the goal
active until the full requested behavior is true and verified.

### Owned-child SDK construction observation

The interrupted restart succeeded: the resumed native status reports supervisor
`codex-alab@55416`, with the same live goal and no human hold. Both owned trees
were clean at the previous checkpoint.

Native commit `1162a590b7e` adds `OwnedWorker::check_sdk_construction`. It observes
the original retained, current birth receipt and child incarnation, then matches
its exact bound native constructor to an authenticated SDK actor's issued
operation and live connection. Accepted manager construction must be known,
without a pending preload or revoked source. Genuine `complete_unrelated`
registrations also qualify; reader-only and policy-unavailable roles do not.
This method supplies a current observation, not source or attachment authority.
Actual attachment still needs its own admission and construction recheck.

The nine focused native custody tests pass on the final edits (0.04s;
`native-owned-sdk-construction-settled-tests.log`). The owned echo child really
emits a WebSocket listener banner, and remains construction-pending afterward.
These kernel fixtures create no SDK member and provide no positive SDK proof.
Positive admitted Codex qualification remains required during service integration.
Native full landing is running as handle8204, evidence
`native-owned-sdk-construction-land.log`; do not duplicate its waiter or edit the
native tree until it exits. It merged moving main to `323df472b9e` initially.

The latest supplied AGENTS instructions strengthen Codex application builds to a
live source-selected `targo trust build --certify` with dependencies included.
Unsupported, unknown and runtime-only bounds cannot count as proof. Existing
package/gate evidence predates this requirement. Merge `e993fafeb6` incorporates
40 commits through `origin/main` at `91f18dcf9f`, including its complete certified
build boundary, dependency policy, native verifier containment, retained reports,
package-byte binding and exact-source runtime smoke. The merge was clean; its
only lockfile change adds serde/serde_json to ALab's core dependency list.
The attempted local overlapping implementation was discarded in favor of that
stronger existing implementation, before committing any of its code. Its
obsolete fixture run handle58650 was interrupted and is TERMINAL130, not a
passing suite. Do not poll it. Native handle8204 remains live; released-shim,
fmt, census, export-scan and lint have passed, and test compilation is active.

The merged certification, verifier-memory, runtime-smoke, package-entry and
project-toolchain fixture modules passed all 42 tests in 4.082s, evidence
`merged-certified-build-fixture-tests.log` (handle81346 TERMINAL0). These are
orchestration fixtures, not application proof. A real unit-runner test driver
was built with native source-selected Trust15698 in 5.28s, under the protected
profile, at `codex-rs/.ubd/certified-gate.noindex/debug/alab-unit-runner` (handle50907
TERMINAL0). The legacy `codex-rs/target` was refused as unmanaged and left intact;
the fresh target was admitted by the native cache owner. The new target's
artifact directory is excluded locally in git's info/exclude. No source or
compiler ownership was bypassed. The protected full gate should use this fresh
managed target, ALAB_PYTHON's actual python3.14 path, and the qualified native
`target/debug/atpkg-publish` in the stable owned native tree. Retain the existing
protected entry point; do not substitute a direct unverified application build.
No new Codex application package has been produced or promoted.

The full protected gate at `ef38905e26` is TERMINAL1 (handle83047), evidence
`live-update-certified-full-gate.log`. It selected Trust15698, admitted the fresh
managed target, waited for capacity, then completed tippy compilation in 11m27s.
Its verdict found two ALab warnings: `items_after_test_module` in `chat_hand.rs`
and `type_complexity` in `editor_upgrade_textarea.rs`. Certification and runtime
tests were not reached. The fixes move the unchanged runtime-editor impl before
the test module and name the existing native click tuple; they alter no runtime
behavior or wire representation. The gate must rerun on that committed fix.
Do not poll or duplicate the completed handle83047.

The native landing run handle8204 is TERMINAL0: all nine checks passed in 37m28s,
including all 39 affected test binaries and AppKit. It explicitly LANDED
`25781c004ed7463f9cff6832beccbfc853762207` on origin/main, checking
`03f1c649040548cf050c5f45f0fa9de9a45aad13` and composing disjoint main
`e9eadeb530cc605c0273d884114641ecd56ebf72` with zero re-judges. Evidence remains
`native-owned-sdk-construction-land.log`. Do not poll the completed handle.

To keep implementation moving while both original trees were frozen, backend
admission work has its own retained worktree:
`~/.ubd/codex-backend-sid2-55416.noindex`, branch
`fix/codex-backend-sid2`, now merged through `25781c004ed`. Its astream submodule is at the
committed `8497b9710d83d9be4253ae96e7035e84004ea220` pin. No original worktree was
removed or recreated. The uncommitted native files add an opaque
`ConstructedMember` observation and `Registry::constructed_member` for the
frontend's exact process, issued operation, registration and live connection.
It accepts completed known managers or genuine no-native-reader SDK
registrations, supplies no Source/provider/attachment grant, and invalidates on
EOF, retirement, re-preload or process replacement. Accepted target registrations
can identify their actual frontend without transferring any target/source rights.
A borrowed server PTY must actually name that member as its foreground process;
the proof does not duplicate a PTY or require a task/supervisor claim.
The source's existing NativeMember retains its transport for NoNativeReader, so
the future frontend factory can use its authenticated channel without inventing
an OAuth source or requiring an autopilot task.

`OwnedLaunchReservation::spawn_backend` now starts a separately armed Codex
app-server under that actual foreground member. Native memory retains a random
bearer; argv receives only its digest, a private loopback listener and bounded
configuration values. Refusal retains the genuine birth attempt outcome. The
startup owner releases its credential only after actual child SDK construction
and rechecking the original full frontend process identity; a replacement image
still needs native handoff admission. These helpers passed the final aterm-agent
library check (15.25s, `backend-native-codex-row-checked.log`, handle3968
TERMINAL0). This is compile evidence, not SDK/PTY integration qualification.
Root service supervision, endpoint observation, descendant/tool ownership,
request/event receipts, frontend routing and exec handoff remain unwired.

The protected gate rerun at `a038930904` is TERMINAL2 (handle85539), evidence
`live-update-certified-editor-lint-fixed-gate.log`. ALab tippy is clean (41
upstream diagnostics set aside). It reached `targo trust build --certify` with
dependency inclusion and verification required, but Trust15698 refused its
procedural-macro TCB boundary before compiler measurement. Its report explicitly
has unknown coverage and no authenticated unit inventory. No application proof,
successful certified build or package exists from this run. Do not poll it.

Native `atpkg-publish adopt trust=16387` created its own
`~/codex-alab-adopt-trust-16387` worktree on `adopt/trust-16387`.
Its first key run refused newer-compiler recursion-depth warnings in ALab's
context-host graph (`live-update-trust16387-adoption.log`, handle98371 TERMINAL1).
The tool initially resolved its `HEAD` base in main (`0f5d353337`); merge
`a8215c95fc` brings this goal's complete `a038930904` source onto the trial branch.
Commit `3526a4e282` then hides context-table storage behind a checked Send+Sync
interface, preserving atomic pointer publication, capture, wake and retirement.
The four real context-host tests passed under native-selected Trust16387
(`trust16387-context-host-tests.log`, handle9308 TERMINAL0). No recursion budget
or unsafe auto-trait assertion was added. The generated trust.lock is still
uncommitted; only native adoption may commit it after its acceptance key passes.
The retry is TERMINAL1 (handle45425), evidence
`live-update-trust16387-context-fixed-adoption.log`. Its 4m54 tippy run removed
the original context-host recursion warnings. Two remaining ALab findings were
the deeply nested direct-reader worker future and a redundant credential clone
in an epoch test. Commit `efeb0b10ab` on the trial branch gives the already boxed
worker an explicit Send future interface before its outer task-local scopes,
and moves the snapshot instead of redundantly cloning it. All nine real
credential-epoch tests passed under native-selected Trust16387 in 0.01s,
`trust16387-reader-epoch-tests.log` (handle85571 TERMINAL0). The generated
trust.lock is still uncommitted. NEXT: rerun the native adoption command on the
existing trial branch, using its full source revision rather than ambiguous
`HEAD`, and inspect the full acceptance key. No compiler override or advisory
proof lane is permitted.

Agent refresh checkpoint: every check started in this session is terminal;
no background check should be restarted or polled. Native backend draft files
remain uncommitted in the separate stable backend worktree and have a private
backup at `backend-startup-before-agent-refresh.tar.gz`. Its final library
check and tippy both passed (15.25s and 1m17s respectively; handles3968 and43018
TERMINAL0). Positive SDK/PTY tests and the actual native root service still need
implementation; do not land this draft merely because it compiles. Preserve all
three existing owned worktrees and the trial tree, and leave shared aterm/main
untouched. The agent refreshes only at this stopped-check boundary. Verify its
new supervised PID after resuming; the previous PID was55416. Keep the complete
live-update goal active, including busy work, full input/editor/worker ownership,
native Go/activation/settlement/recovery, initial adoption, certified package
qualification and source landing. The installed managed selection is unchanged.


### Continued native endpoint and compiler qualification

The native backend draft now observes the actual owned child stderr for the
bounded loopback WebSocket announcement, preserves pipe custody, strips terminal
color, and permanently rejects malformed/duplicate endpoint observations. Five
endpoint tests passed; final library/tests tippy completed in 2m02s. These are
startup primitives, still without GUI root-service dispatch or a positive native
SDK/PTY integration test. They remain uncommitted in the existing backend tree.

Trust16387 adoption remains unaccepted. Trial commits3526a4e282, efeb0b10ab and
8b5f02a429 bound recursive host/reader/destination future expansion; the last
full login run passed336 tests. The subsequent acceptance key exposed another
constructor-wrapper expansion. An uncommitted checked-Send future interface in
auth_manager_catalog.rs now carries an explicit borrowed lifetime; its first
attempt failed E0310 and was corrected. The new protected login run is recorded
in trust16387-catalog-lifetime-login-tests.log (check its final outcome before
claiming acceptance). Never stage the trial generated trust.lock manually.
The full live-update goal and all previously recorded runtime qualification
requirements remain open. No installed package selection changed.


The corrected constructor interface passed all336 login tests (50.30s;
protected run13922 TERMINAL0), then committed as765f7dc40b in the adoption tree.
The full adoption retry21662 TERMINAL1 stopped before compilation because
origin/main had gained39 commits/new hooks. Those changes merged cleanly into
the trial without waiving hook checks or staging its generated lock. A fresh
adoption key is running as unified shell session12619; evidence file
live-update-trust16387-main-integrated-adoption.log. Keep the trial frozen until
that exact job terminates; do not start another key beside it. This is not a
successful adoption or a shipping package claim.


The main-integrated adoption key12619 ended TERMINAL1 after tippy5m27s.
Constructor-interface warnings are gone. Its remaining two ALab warnings trace
through AuthManager::auth into the refresh graph, at paired-auth boxing and a
native-member recovery test spawn. The trial now has an uncommitted checked
Send trait-object boundary in auth_context_host's recursive async forwarding
macros. Existing captures, forwarding targets and cancellation stay unchanged;
no recursion-budget increase, unsafe Send implementation or warning waiver is
used. Protected login tests are running as session31412, evidence
trust16387-host-forwarding-login-tests.log. Do not change the trial until that
job ends; its generated trust.lock remains uncommitted.


### Auth interface fix qualified by login tests

The generic host-forwarding experiment did not remove the depth warning and was
discarded. Its corrected experiment run65542 passed336 tests, but was not the
final implementation. Focused compiler traces showed the separate recursive
host-auth, initialization, guarded managed-load and refresh-operation expansions.
The retained fix uses concrete checked Send future interfaces at these owned
boundaries, preserving existing async bodies, scopes and error mapping. Two
small ALAB(accounts) calls in the upstream manager delegate reload/managed-load
to the owned adapters; before/after delta topic output is in the evidence folder.
No workspace formatting, recursion-limit changes or unsafe auto-trait claims
were introduced.

The combined focused login library check42940 passed with no login warning
(4.06s). Final formatted code then passed336 login tests in47.91s; run26269
TERMINAL0, trust16387-auth-interfaces-login-tests.log. Committed the five code
files only as7e61163c7ae404a039c132b6f9e1d90107d691f6 in the existing adoption tree;
trust.lock remains generated/uncommitted. The new full native adoption key is
running; evidence live-update-trust16387-auth-interfaces-adoption.log. Keep its
trial HEAD and files fixed until it terminates. This does not yet establish a
certified application build, package, deployment or completed live update.
The native backend draft has an updated private backup containing all seven
files at backend-startup-and-endpoint-draft.tar.gz and remains uncommitted.


The full auth-interface key24811 ended TERMINAL1: login warnings are gone;
remaining ALab diagnostics were model-provider client_auth.rs:79 and the owned
executor_startup.rs:32 adapter (lib/test variants). Adding explicit checked
Send return bounds to constructor interfaces compiled but did not alone remove
those sites (focused run14638 TERMINAL0,2m11s). The retained follow-up exposes
the existing typed auth adapter as AuthManager::alab_auth for three executor
calls, and bounds runtime binding selection in the owned runtime_auth_pair
adapter through two marked hooks. Runtime state/refresh/registration futures
remain the original bodies; no new grant, retry or fallback is introduced.

Focused login/model-provider/executor library check74968 passed in56.73s,
trust16387-provider-executor-interfaces-check.log. Both previously named sites
are absent; model-provider still reports an upstream request_setup.rs:38 warning
through agent-identity auth, so this is not a claim that all compiler warnings
are gone. The complete gate must judge that and all test variants. Code committed
as9726493f98efbdf1cd17672670d695aacb1e1b95 (five files, generated lock excluded).
A fresh native full adoption key is running; evidence
live-update-trust16387-provider-executor-adoption.log. Keep the trial fixed until
it terminates. No application proof or deployment has completed.


The provider/executor full adoption key48023 ended TERMINAL2. ALab tippy is
clean (3m10s;139 upstream diagnostics set aside). Certified application build
then refused before verification: Trust16387 enforces no in-process proc-macro
TCB and enumerated the CLI's procedural-macro dependencies as unsafe for its
authenticated proof transport/artifact boundary. The report has unknown coverage,
no TRUST_JSON inventory and no proved application obligations. This reproduces
the actual15698 compiler admission limitation on16387 after resolving the new
compiler lint issues; neither is a successful certified package/build. No policy
relaxation or warm-cache/unverified substitute is authorized. Native adoption
correctly left its generated trust.lock uncommitted. Report directory:
codex-rs/target/alab-certified.aWPDV0/report in the existing trial. The installed
managed targo remains16387 (index134). Next inspect qualified compiler options;
source backend integration and runtime qualification remain open independently.


The managed signed update check93713 ended TERMINAL0: Trust and its companion
units are up to date at16387. There is no newer qualified compiler selected by
that index. Do not rerun this same adoption key absent a relevant compiler or
macro-isolation/admission change. Do not mark procedural macros audited by hand
or relax the proof policy. No own checks remain running from this qualification
sequence;48023 and93713 are terminal. Trial has only generated trust.lock dirty.
Native feature tree remains its seven-file uncommitted startup/endpoint draft.
Existing source proof CLI constructs an SDK member only after its driver command;
positive native-owned SDK qualification must not await construction before
allowing that command. Full native root service, routing, handoff and recovery
implementation remain required; the overall live-update goal stays active.


### Native terminal ownership for background startup

Background startup must retain the actual terminal owner while it borrows a
foreground proof; copying an i32 or duplicating the PTY is not that ownership.
Added SinkWriter::owned_master in the existing primary native worktree. It
returns a BorrowedFd only from the sink's actual OwnedFd, with the existing
fd_lifecycle UseFd anchor; retaining the sink preserves descriptor allocation,
but terminal retirement/foreground must still be independently checked.
The real descriptor/EOF lifecycle test passed (native run91484 TERMINAL0,
1 test selected). The unowned-sink refusal/EOF test also passed (1 selected).
These are descriptor-ownership tests and establish no SDK or root grant.

Committed native topic38e5e9e2165 with its changelog fragment. The required full
native landing gate is live as unified session17768, evidence
native-owned-pty-borrow-land.log. It merged current main at2d34dd31727; keep the
primary worktree HEAD/files fixed until that exact run ends. The separate backend
worktree fast-forwarded to38e5e9e2165 with its original seven-file draft preserved.
It can retain Arc<SinkWriter> in the future startup worker and then borrow this
owned master locally, avoiding an unowned raw descriptor or a PTY duplicate.
The actual GUI root service and complete root/tool lifetime remain unfinished.


### Original-cohort retirement must remain owned through kernel absence

Inspection of Source tool startup confirms non-Linux parent-death signaling is
a no-op; separate tool sessions therefore still require native broker custody.
Even the original startup cohort had a narrower cleanup gap: OwnedWorker::stop
used kill_group_until_empty, whose documented cap proves no drain, then reaped
only its leader and dropped its receipt. The backend draft now waits for genuine
member-ended observations before the owning reap, then retains the actual
Spawned receipt until the same scope's atomic kernel group query is Absent.
After reaping it never signals the numeric group again. Pending observations
return WouldBlock; off-GUI Drop retries that pending retirement. This remains
original-cohort coverage only, not a claim about tools in other sessions/groups.

Added a real two-child same-group regression: keep the second child's owning
wait pending, require retirement to stay WouldBlock after the original leader
is absent, then reap the second child and require kernel group absence and
successful idempotent cleanup. The initial compile11674 TERMINAL101 used private
fields in its test; corrected assertions use public behavior and actual kernel
observations. Corrected run37806 is live (memory admission wait behind the native
landing gate), evidence backend-native-cohort-retirement-observation-test.log.
Do not change the backend draft until this exact run ends. All eight draft files
have a private backup at backend-startup-endpoint-retirement-draft.tar.gz.

Primary native gate17768 is still live: fmt/census/export-scan/lint passed;
cross-cell and test compilation are running. Keep its primary tree fixed.
Source feature worktree merged9726493f98 cleanly atb229b80efcafed422debcfa5f7f9b7edc0e46dc3;
a diff against the trial proves only this review log differs, so compiler fixes
are no longer parked solely on the adoption branch. The generated16387 lock was
not imported. Application certification remains refused at the proc-macro TCB
boundary; do not retry the unchanged adoption key or substitute unverified proof.


The corrected cohort regression37806 ended TERMINAL0:1 selected,0 failures,
0.06s. A genuine old-code negative control88420 then used HEAD's previous stop
body while preserving the same test: it ended expected TERMINAL101 because old
stop returned Ok with an actual unreaped companion still in the group (0.11s).
Restored the fixed source immediately from the private fixed-source backup.
The complete owned-launch/startup/endpoint set5468 then passed15 tests,0 failures,
0.04s. These cover original native cohort custody and endpoint observations;
they still establish neither child SDK-positive construction nor whole-root
coverage of tools in separate groups.

The restored final draft's lint job1642 is live; evidence
backend-native-cohort-retirement-final-lint.log. It is waiting for the primary
native gate's timed-control redraw reservation, not a compiler failure. Keep the
backend draft fixed until that job terminates. Primary native gate17768 remains
live:7 checks have passed (released-shim,fmt,census,export-scan,lint,doc,cells),
and24 test binaries had completed at the last parsed event. Do not duplicate
either job or waive the native timed-work reservation. The latest entire draft
is backed up in backend-startup-endpoint-retirement-draft.tar.gz.


### Native terminal borrowing landed; control-role separation in progress

Native gate17768 ended TERMINAL0 and LANDED
e3dc0a08305041b210d883bc79639e72791f767b on origin/main. All9 checks passed;
checked3dd88a89cf22a7821cab32db6834dd2ba7fced4e,1655s,0 re-judges, composed
onto6b882b48a4c0e0eadcb853655780ce1428e1511e. The primary tree was clean and
origin/main matched that landing at inspection. Backend final lint1642 also
ended TERMINAL0 (1m58s). Ported only the tested original-cohort cleanup and its
real regression to the primary tree asabcfbff5dd6 plus its changelog fragment;
its required full landing gate is live as session83197, evidence
native-cohort-retirement-land.log. Keep primary HEAD/files fixed.

Saved the eight-file backend prototype as local draftcc3a98e5410 (not landed),
then mergedabcfbff5dd6/current native main cleanly at4869647faccac5b1c8040b6da08681baf6315d8e.
Read the updated AGENTS delta and initialized the pinned astream submodule.
The main merge added metadata-only source-proof observations, which are not
controller/provider authority and do not replace the actual SDK-positive test.

Frontend control must not require a successful credential-manager load. In the
backend draft, replaced unused ConstructedMember/constructed_member with
FrontendController/frontend_controller in codex_source_frontend_control.rs.
This observes the same live native-issued SDK allocation/connection and actual
process, then independently requires a real foreground PTY. It has no Source,
provider, construction or root-attachment grant; backend SDK readiness remains
unchanged. FrontendIdentity now also binds the original member, operation and
connection, so same-PID or same-image replacement/reconnect cannot implicitly
inherit its attachment. This is a host observation API only: controller wire/
client lifetime, source-census interaction and genuine SDK/PTY qualification
still require implementation. Do not claim signed-out live updates are wired.

Added a negative allocation/control test (no signed SDK or foreground grant).
Its first run53134 ended TERMINAL101: manually binding the old fixture child
correctly refused missing required session environment. Corrected the fixture
to use actual controlled native spawn; no admission was weakened. Run5565 is
live, waiting for memory admission behind the primary gate; evidence
backend-native-frontend-control-owned-test.log. Keep backend HEAD/files fixed
until it terminates. The previous test log is
backend-native-frontend-control-separation-test.log. Existing15 startup tests
passed before this role refactor; they do not qualify the new role yet.


The replacement v65 AGENTS instructions are in force. Corrected control test5565
ended TERMINAL101 at reserved_peer: the echo executable is intentionally not a
consumer-capable signed SDK image. The earlier explanation about missing session
environment was wrong; the rejection is the actual image-capability boundary.
Removed the unsuitable test/module and its test-helper visibility change instead
of manufacturing a peer or weakening admission. Both failed fixture runs are
not qualification evidence. Genuine SDK/PTY coverage remains required.

Current controller-refactor library check10773 passed TERMINAL0 in1m31s,
backend-native-frontend-control-library-check.log. Current draft lint is running,
evidence backend-native-frontend-control-lint.log; keep backend files fixed until
it terminates. Primary cohort landing gate83197 is still live. No new Codex
certified application build or package has become available; its proc-macro TCB
refusal remains an operation-specific blocker, not a waiver.


### Check-free agent refresh boundary

Native cohort gate83197 ended TERMINAL0 and LANDED
955013fd65c824cf1124649a58d3a5fd6d3bafa8 on origin/main:9 checks passed,
checkedf378a884faca707a86b962f59c7d8bf35e67f442,1909s,0 re-judges,
composed onto537b13cd926803c59d73d6efc8985038d6430a83. Primary tree clean;
origin/main matched that landing at inspection. Backend controller lint32818
passed TERMINAL0 (2m15s); library10773 had passed1m31s. Committed the compiled/
linted controller API refactor locally as6d846dc7cf6f3068a4935487f016a08da45232ad;
backend tree is clean, still an unlanded prototype requiring genuine SDK/PTY
qualification and actual service wiring. All own checks are terminal; no old
handles should be repolled or restarted.

Operational handoff is ~/codex-live-update-evidence/restart-handoff-current.md.
The agent refreshes itself at this stopping point under the supplied restart
policy. Current status names supervised PID90340, sid1 adopted (use @self, not
old sid2); help restart read. After resume verify new supervised PID and retain
all existing owned trees. Complete original goal remains active and unbounded;
no application proof, new Codex package or deployment has completed.


### Real SDK executable qualification

Added `codex-alab-native-source-probe`, an explicitly selected, feature-gated
`harness = false` debug test target. It compiles the existing production SDK
proof implementation, installs the actual transport/credential adapters, and
enters the real native constructor before any config/credential read. It does
not build or qualify the application, advertise a runtime schema, synthesize
native members/Ready receipts, or bypass the certified application gate.
Source-selected Trust 15698 built this test successfully. A no-constructor run
was refused. Targeted tippy on source9d9abb2c9f passed with no probe diagnostics;
subsequent closed phase labels and async auth admission compiled successfully
but have not received a final lint pass yet.

The joined isolated native host admitted the actual test image and its SDK
member. Real native PRELOAD returned Waiting then Unavailable; no Ready receipt
or credential was exposed. Config and manager construction succeeded, but both
cached-only and normal async auth snapshots were unavailable. Native debug
fixture diagnostics originally logged only Peer/Open; drafta21e4dd56c1 now also
logs closed construction reply classes, without identifiers or payload data.
Its explicit native_host test build passed. This diagnosis supersedes the
inference that no PRELOAD request reached native code.

Evidence: `native-source-probe-auth-build.{jsonl,stderr}` (exit0),
`native-source-probe-lint.{jsonl,stderr}` (exit0 on9d9abb2c9f),
`backend-native-host-phases-build.{jsonl,stderr}` (exit0), and
`real-sdk-source-proof-phases.log` (exit1). Retained isolated world
`/tmp/.ubd/atsp82476_e872fd4ed079f660_0.noindex/source-host.log`
contains the closed PRELOAD reply diagnostics. Investigate the actual native
canonical source/Slot initialization refusal; do not manufacture NoCredentials,
a Source or Ready, or loosen image/constructor admission to make the probe pass.


### Published-grant diagnosis and isolated OS qualification

Native draft958b328a4ac locates constructor failures without private error data;
0d5150fb9c8 distinguishes canonical publication and lineage admission. Both
explicit native_host test builds passed. The joined host proof now establishes
that PRELOAD fails specifically in `lineage_consumers_capable`, before the
constructor's launch-selection and Slot initialization callback. The hypothesis
of a failed Slot write is superseded. The existing `mac codex-source-proof`
lane explicitly requires a clean OS process universe and retains both IP and
real-home guards, rather than waiving the process-wide consumer census.

Started that existing lane with the exact genuine SDK test package and native
host artifact. Its native runner is still LIVE: unified exec session67267,
lab PID27119 at last direct process observation. Log
`~/codex-live-update-evidence/real-sdk-source-proof-vm.log`.
Record directory
`~/codex-live-update-evidence/sdk-source-vm-28184.noindex`.
The 31.7GB compressed disposable macOS base image reached1% after3m21s;
preparation and qualification have not completed. Do not start a second pull,
relaunch the lane, edit its native checked tree, or classify it terminal from a
quiet log. Poll session67267 and inspect the retained record/log. Its copied
inputs are hashed in inputs.json. The source package is explicitly an
unverified SDK test executable, not a certified application build.

Final source probe tippy on source8471685fe2 (codebdd2e13e51) exited0, with
zero probe warnings/errors; unchanged upstream dependency warnings remain.
Evidence `native-source-probe-auth-lint.{jsonl,stderr}`. Handle50687 terminal,
consumed. Host proof6777 exited1; world
`/tmp/.ubd/atsp18764_c449e28577bf21fc_0.noindex/source-host.log` contains
closed lineage-consumers phase evidence. Host build25813 exited0, consumed.
Goal remains active. A passing component cannot qualify production backend
routing, authenticated handoff, tool custody, Stop effects, or application
certification; those full-goal requirements still remain.


### Independent native controller protocol implemented

Review of the first controller qualification draft found a real lifecycle error:
it observed a credential member's physical connection. Credential reconnection
could withdraw frontend control even when the frontend remained alive. That
coupling has been removed, rather than preserved as compatibility behavior.

Native primary worktree `~/.ubd/codex-live-update-sid2-46933.noindex`
was clean955013fd65c, fast-forwarded to the backend prototype0d5150fb9c8, and
its branch renamed to `fix/codex-controller-proof-sid1`. It is now08852de3f3b.
The other native backend tree remains frozen at0d5150fb9c8 for the original VM
qualification. Do not confuse these trees or replace the installed aterm.

Native now issues separate controller allocations, on separate authenticated
connections, under the actual App scope and observed peer. A bounded controller
table is excluded from Source member/census accounting. Original process,
image, connection and operation remain mandatory; Source member IDs no longer
satisfy frontend_controller(). Physical EOF, explicit controller-close, or
process/image replacement withdraws control. Controller close is release-only;
it acknowledges no backend/tool completion. Live controller connections are
protected from idle eviction, and closed/departed entries are retired. Unix
implementation is cfg-guarded in the cross-platform registry.

New native `codex_frontend_control.rs` serves closed-schema controller-open and
controller-close messages before credential-job scheduling. SDK sourceab4f61651f
adds an independent authenticated transport/client and TUI opener. The actual
SDK proof opens control before config/manager construction, retires all actual
credential readers first, then explicitly closes control. The native fixture
requires before-Ready control, actual kernel foreground denial, survival across
credential retirement, and withdrawal while the same SDK process remains alive.
No Source, NoCredentials, Ready, process or foreground proof is synthesized.
Production per-root backend routing and authenticated transfer are still unwired.

Both updated builds passed: native host75363 exit0 (1m04s), source SDK probe54256
exit0 (4m18s). Native agent+GUI tippy8428 exit0 (1m54s, -D warnings). Source
probe tippy69295 exit0 with zero diagnostics in changed ALab packages. All four
handles terminal/consumed. Evidence uses `native-sdk-independent-controller-`
file prefixes in the evidence folder. The earlier qualification-draft host19387
also compiled, but its Source-coupled assumptions were superseded by08852de3f3b.

The genuine updated SDK opened controller-open successfully against the new
native host on this Mac. Credential PRELOAD still correctly refused at the
known process-wide lineage census. Evidence
`native-sdk-independent-controller-host-proof.log` exit1, world
`/tmp/.ubd/atsp30991_0c705ee04d4f4473_0.noindex/source-host.log`.
This proves the real control handshake, NOT full controller lifetime or Source
qualification. New probe package
`~/.ubd/codex-sdk-control-probe-sid1-28184.noindex` records actual
sourceab4f61651f, compiler15698, binary hash and explicit unverified-test status.
It is not an application/backend executable and must not be used for app-server
qualification or shipping. The original package remains unchanged for VM1.

ONE OWN JOB STILL LIVE: unified exec67267, original VM1, old backend host0d515
and old SDK packagebdd2e13e51. Base download recovered from network interruptions
and reached20% at last observation. Keep polling the same handle; no second VM
or image pull has been started. Its log and record paths remain as recorded
above. Once this run finishes, use the cached base for a NEW qualification with
primary host08852de3f3b and the new SDK-control probe package. Do not mistake VM1
success for qualification of the new controller protocol. No agent restart while
this own job runs. Goal active, not complete. Source design updated9e99acf2d6.


### Withdrawal evidence strengthened (native6a244a07a3e)

Native primary now9c720e9fcf1 +6a244a07a3e, on the same stable worktree/branch.
A failed current() check can mean metadata contention, so it is NOT evidence of
retirement. Added opaque ControllerWithdrawal<'_>, constructible only after
explicit withdrawal of the exact allocation or revocation of its original
connection. Binding mismatches, live metadata, contention and unavailable
metadata yield no witness. This says nothing about backend/root/tool completion.
The fixture's retired-while-live check now requires that witness AND the original
actual Process (including image) still present. Critical credential-retirement
and controller-close observations keep the real reply unsent while boundedly
retrying unknown/Busy metadata, so process exit cannot substitute for explicit
retirement and credential closure cannot race past the survival observation.

Native host builds54022 and10978 both terminal0; final host build36.57s.
Final native agent+GUI tippy24745 terminal0 (1m30s, -D warnings). Logs:
`native-controller-withdrawal-final-host-build.{jsonl,stderr}` and
`native-controller-withdrawal-final-lint.log`. All consumed; do not repoll.
No source SDK/protocol change in this turn. Source SDK packagecontrol-probe
stillab4f61651f and is paired with latest primary native host6a244a07a3e for
NEXT VM qualification, not the older08852 host. Source remains clean.

Only own live job remains ORIGINAL VM1 session67267. Last exact poll confirmed
live; its download reached26% after automatic network retries. Continue that
same handle/log/record; do not restart, duplicate its pull or alter backend
worktree0d515 while it runs. VM1 still uses OLD SDKbdd2 and OLD host0d515, so
it cannot qualify the newer independent-controller or withdrawal checks. No
full application certification, new runtime schema, production backend routing,
handoff, tool broker or input/Stop acknowledgment qualification has been obtained.
Goal remains active. Avoid long speculative architecture loops; act from these
specific check handles and the current files.


### Joined SDK/native controller qualification passed

Latest source functional commit7435cd77b1 fixes actual source Lifetime retention
after native use End. An EndObservation retains State for acknowledged history;
State previously kept the Lifetime even after End, preventing the original manager
from retiring. Both authenticated Closed/proven-Unsent paths now release the actual
Lifetime before publishing ended. A strengthened ownership regression verifies
retention before native End and release afterward while the observer remains alive.
All3 focused native_source_use tests pass; Core tippy --lib --no-deps -D warnings
passes. Diagnostic-only source9094192171 supplies private-data-free Stop phase labels.

Nativec137a05fa2d fixes a separate metadata observer error: accepted derived ancestry
is historical issuance metadata, not an assertion that the original parent use is
still active. Actual current-use, Scope and custody validation remain independent.
Its focused regression, host build and native agent/GUI tippy all pass.

VM5 session18038 TERMINAL0 with capture_complete=true, in a SIP-enabled disposable
macOS guest. Evidence: ~/codex-live-update-evidence/sdk-use-owner-vm-28184.noindex.
Actual SDK startup, contextual reader, derived manager, resample and rebind through
auth/source generation4 passed. All3 retirement observers obtained Closed. Native
controller checks: before_ready=true, foreground_denied=true, survived_credentials=true,
retired_while_live=true; actual controller-close reply delivered before SDK exit.
Source test package codex-sdk-use-owner-probe-sid1-28184.noindex records the exact
7435cd77b1 source and actual binary SHA. Native host is compiler artifact from
native-derived-ancestry-host-build.jsonl, paired with that SDK test executable.
VM1-4 are terminal failures retained as diagnostic evidence, not qualification.
No VM/download remains running.

This is a synthetic-issuer joined real SDK/native component qualification, NOT a
production SourceV1 activation, application/shipping certificate, automatic update,
work-preservation, positive frontend foreground or complete runtime handoff proof.
The result manifest explicitly marks automatic_update_proven=false, runtime_enabled=false,
shipping_certified=false, work_preservation_proven=false. The full goal remains active.
Native full verify land was started after this qualification; consult its real handle
and native-controller-qualified-land.log, not this checkpoint, for the outcome.


### Native support landed after full gate

Full verify land session43024 exited0. All9 checks passed in26m31s;
released-shim, fmt, export-scan, census, lint, cells, doc, test and appkit allok.
The tool composed only clean disjoint main changes, with0rejudges, then landed
be2b2778490edb748cebd09872dbc3c9bb09d3b8 on origin/main. A subsequent fetch
and merge-base --is-ancestor against origin/main exited0; native worktree clean.
Gate log native-controller-qualified-land-after-merge.log; checked tree
6e9073ef799064c72d8e19e2919a069a7c1a9128; landing trailers retain exact rows.
Merge conflict resolution retained main's owned-child source environment and
private descriptor control support alongside backend/controller support.

No own build/test/VM/landing job remains live. Full goal is still incomplete:
production per-root backend service/routing, actual tool-tree lifetime broker,
authenticated handoff and request/event receipts, complete UI ownership/input
frontier, new runtime schema/certified application and real immediate-update
qualification are still required. Native landing is not source application landing.


### Native-owned bootstrap joined qualification passed

SDK commitb10c34e18d adds default-Unsupported Member control begin/ack methods
and a concrete original-transport implementation. native_owned_control::bind
consumes a physical Unix socket, receives exactly ALABCTL1+32hex challenge bytes
under one async deadline, acknowledges on the original constructor connection,
and retains that actual socket/member. Cancellation closes the endpoint; no
blocking worker or public socket locator is left behind. Early startup binds
only the unconsumed original allocation, refusing a target/derived substitute.
Three real socket tests (fragmentation with subsequent bytes preserved, malformed/
truncated refusal, and partial-read cancellation) pass; Core+aterm tippy passes.
SDK fixture8c316ca931 binds actual stdin before config and uses the resulting
async socket for its command stream, avoiding blocking-stdin/nonblocking aliasing.

Native3eb91baf956 starts backend ownership with spawn_with_control and requires
both actual SDK construction and native control binding before credential access.
Nativeaa570193360 exposes post-birth failure settlement using the same strict
original-cohort retirement as OwnedWorker, preserving Unbound history;21 focused
startup tests pass, including actual post-exec failure and kernel group absence.
Nativefixture5ac61267df1 captures actual process environment with explicit private
overrides, independently reserves/arms/spawns a native-owned child, and waits for
OwnedWorker.check_control_binding before transferring input to the guardian.
This ordering prevents JSON driver bytes racing ahead of the native challenge.
Only this leaf debug fixture transfers its genuine child into its existing guard;
production backend ownership retains the complete OwnedWorker.

VM6 session67912 TERMINAL0 with capture_complete=true, SIP enabled, exact native
5ac host and SDK8c316ca artifact SHA recorded by inputs.json in
~/codex-live-update-evidence/sdk-owned-bootstrap-vm-28184.noindex.
The driver completed every source/contextual/derived/resample/rebind step through
auth/source4 and controller lifecycle checks alltrue. Native check_control_binding
succeeded before driver descriptor transfer. The host's SDK stderr is now a
retained pipe rather than inherited output, so absence of SDK phase labels in
source-host.log does not substitute for the actual owning native check.

Initial native tippy69324 failed only because the old fixture-only
Preparation::spawned PID-based helper became unused; native00c4d54e643 deletes
that obsolete helper. Final tippy is running; no full native gate for this new
bootstrap delta has passed yet. Result remains explicitly qualification_only,
synthetic issuer, runtime_enabled=false, automatic_update_proven=false,
shipping_certified=false, work_preservation_proven=false. No application build,
production root service/tool broker, positive foreground attachment, frontend
update or work-preserving runtime handoff has been qualified by VM6.


### Native owned-bootstrap support landed

The complete native `targo --unverified verify land` gate passed all nine
checks in 26m40s and landed commit
`1a7e2ace09433254c07b262f10199e0378948e92` on native origin/main.
It checked `83539b71e58069389acaaa9f11c736ed8609cea8`, then composed
onto `260ff77fddea37635ec33e77dc673dccb88ac4b6` with no re-judges
because the concurrent commits touched none of this branch's files.
A subsequent fetch and ancestry check confirmed the landed commit on
origin/main; the native worktree is clean. Evidence:
`native-owned-bootstrap-land-final.log` in the external evidence directory.
The first gate's disallowed busy-yield test was replaced with a paced retry
before this successful full gate; no production admission was loosened.

This lands physical child/socket binding and strict post-birth failure cleanup,
not a production backend supervisor or seamless update. Source still needs
production startup dispatch, per-root routing, native tool-tree ownership,
authenticated frontend attachment and full UI/work preservation. The joined
VM remains a test-only SDK qualification; application certification remains
blocked at the compiler's in-process proc-macro TCB boundary.


### Production CLI bootstrap dispatch

The production CLI now retains native_backend_startup::Startup through dispatch,
before configuration consumes its original constructor. The private first-argv
--alab-owned-backend request accepts only the native launcher's fixed app-server
command shape: separate config overrides, private loopback listener, capability
authentication and a valid token digest. It strips that request before actual
Clap parsing. Ordinary user input containing the marker is not intercepted.
On Unix it binds the actual stdin socket through the original SDK member;
failed binding never falls through to ordinary app-server startup. The socket
owner remains on the main stack until dispatch finishes. No attachment, Source
readiness or root/tool closure is inferred from this request.

Two focused request-validation tests passed (source-owned-backend-startup-tests-
final.log); Core library tippy with -D warnings passed in 6.07s. The actual
codex-cli codex test target compiled with --no-run in 6m14s, terminal success,
including both modified upstream CLI hooks (source-owned-backend-cli-test-build
.jsonl/.stderr). These are unverified test artifacts, not application proof.
Delta topics and diff checks passed. Native counterpart 3bc802b1e8e landed
on native origin/main as 8fe40cb2d60343d22a2e641f93d5399a87c1d7c4 after
all nine gate checks passed in 47m53s (tests 18m19s, AppKit 1m47s, including
resource and quiet-machine waits). The first push received GitHub Internal
Server Error. A fresh fetch proved the attempted landing was absent and both
agh open-intent queries were empty. The resumed landing reused judgment
da0a3f8a9cf09ceaa61f614cd144ea97e977542e with zero re-judges, composing
onto 306a9788dbe4b3e2beeba12ab4f52784f8d7e6a7. A subsequent fetch and
ancestry check confirmed the landing on remote main; the native tree is clean.
Evidence: native-backend-dispatch-land.log and
native-backend-dispatch-land-resume.log in the external evidence directory.
The existing joined VM6 validates the physical SDK binding protocol, not this
new production dispatch, full backend service or automatic updates.


### Required native backend parent-death routing landed

Native commit cac5f746709 adds Command::require_tether and uses it for private
backend control startup. Missing watcher routing refuses before birth instead
of silently using an untethered child or Linux's signal-only fallback. The
actual selected route still arms its watcher before executing the requested
program; calling optional tether afterward cannot weaken the requirement.
Generic constructor and control fixtures retain their separate existing APIs.
The backend still needs physical child/socket binding and independent SDK
construction; requiring a watcher grants no frontend or Source permission.

The actual missing-helper command test preserved its no-birth receipt and
proved the shell marker was never written. The backend constructor regression
preserved its exact attempt receipt, revoked its constructor and left the
bystander alive. All ten owned-spawn tests passed; the existing macOS retained-
image qualification now requires the route and passed its ten replacement,
symlink, bundle and parent-death cases. UDS/agent library and test tippy passed
with warnings denied. These checks qualified the original backend cohort,
not separately launched tools or an application-level update.

The first full gate ended COULD NOT RUN after 47m44s: nine checks passed, but
zstd_suite and publish_suite lacked the independent zstd CLI. Installed the
external Homebrew zstd 1.5.7 bottle through native brew compatibility and
verified its executable. The rerun used the supported ATERM_TEST_ZSTD path;
both previously missing suites passed. The full gate then passed all ten
checks in 41m59s and landed native commit
c9e386687bc3c7eba70ce2b6e205276e41b4e902 on origin/main, checking
caec004cd6341439e10f6bfd12c1dc69898b5f72 with zero re-judges and composing
onto 5c9ce77d2fed75457b7454478781eb48245d4784. A subsequent fetch and
ancestry check confirmed remote landing; the native worktree is clean. Logs:
native-required-tether-land.log and native-required-tether-land-with-reference.log.
Reference path and SHA are retained in native-zstd-reference.txt externally.

This does not complete root/tool supervision, authenticated frontend attachment,
production per-root routing or UI/work handoff. Source tool spawning still has
its own process groups and non-Linux parent-death gap; backend group retirement
cannot assert their closure. The new private production CLI dispatch remains
unqualified by a joined application run. Application certification remains
blocked at the compiler's in-process proc-macro TCB boundary.


### Private backend owner withdrawal enters app-server cleanup

Private startup now retains its bound physical control socket in an owned
async monitor. A silent live owner keeps the backend running; EOF, IO failure
or unexpected post-bootstrap bytes withdraw it. The sticky notification is
only cancellation input. It supplies no frontend, Source or root authority,
and ordinary invocations have no owner notification. Main-stack startup
custody aborts the monitor on drop rather than detaching a socket owner.

One marked app-server select branch stops turn admission, cancels transport
and enters existing connection/thread cleanup on this actual owner loss.
It does not wait for an active turn to finish naturally. Existing bounded
thread cleanup is not proof that separately spawned tool descendants ended;
complete native tool custody and Stop acknowledgement remain outstanding.

Three core tests passed for an actual silent socket, EOF/unexpected-byte
withdrawal, sticky observation and monitor-sender loss. Core library tippy
passed with warnings denied (Trust15698, 5.92s). Strict combined app-server
lint stopped in unchanged diagnostics/src/lib.rs:35 on upstream's deprecated
Atomic::fetch_update; no upstream source or lint policy was changed. The
actual codex CLI test-target build passed in 8m00s, compiling the production
app-server hook and startup ownership together. This is an integration
compile, not application certification or production owner-loss qualification.
Evidence: source-native-owner-tests.log, source-native-owner-core-lint.log,
source-native-owner-lint.log and source-native-owner-cli-test-build.log in
the external evidence directory. Full seamless updating remains incomplete.


### Actual native custody before requested tool execution landed

Native functional commit b527ab1fb86 adds PreparedCohort, EntryPermit and
ExecObservation. A private inherited socket holds the native helper after
its parent-death watcher is armed and before requested target exec. The
broker can retain the actual Spawned child first, then release only a permit
matching that original unreaped custody. Exec observation retains its status
pipe separately, so pending observation cannot borrow away child control.
Cancellation transfers the actual born child; withdrawal never invents a
no-birth result. This is a broker primitive, not Root or tool-tree authority.

Real native fixture checks passed for hold/release, cancellation/drop, wrong
cohort and reaped-child denial, failed-exec Unbound custody, and group stop
while exec observation remained pending. Barrier socket units and birth
regressions passed, as did UDS library/test lint with warnings denied.

The first full gate stopped at the inline-test-module census rule; identical
tests moved into their separate file in 7abbf3d. The complete rerun, using the
external ATERM_TEST_ZSTD reference, passed all ten checks in 49m05s, including
339 test-binary results, five cross-platform cells, doctests, browser smoke
and live AppKit drives. The new prepared_cohort fixture and all 977 UDS
library cases passed within this full run.

The gate landed 8bf579cbf23647838d0805bad6f8a7bcc42a6e58 on native origin/main,
checking 5fffe08d1d23b4403c2cb72c268e1d878c98f565 with zero re-judges and
composing onto disjoint 03e3e4d67d07a673e699c5ef5c63d2aa6c627e07. A fresh
fetch and ancestry check confirmed remote landing; the native worktree is
clean and sys left reports no validation jobs left running. Evidence:
native-prepared-cohort-land.log and native-prepared-cohort-land-2.log in the
external evidence directory.

Production Root/tool routing, authenticated frontend attachment, input
frontier acknowledgement and complete UI/work preservation remain required.
This qualification does not supply application certification or a production
new-version transition.

### Retained pipe-tool route and original descriptor channel

The private backend now retains its original native socket and SDK constructor
for tool delivery as well as owner-loss detection. SDK prepare binds the exact
command payload digest and length; the physical socket carries three original
stdio descriptions and the bounded command bytes. Writes are serialized across
the whole frame. A cancelled or failed partial frame withdraws the socket's
write side; startup withdrawal closes both directions, including active senders.

The Source child adapter exposes output pipes and authenticated observations,
without a locally signalable/reapable PID. Core shell-tool spawning now selects
this adapter in a private owned backend, with ALab-owned spawn/output glue and
small marked upstream hooks. Timeout and cancellation await the native captured
cohort stop receipt. Ordinary local children retain the existing output loop.
Explicit argv[0], Unix argument bytes, environment filtering, network environment
and working directory are carried. Inherited stdio and PTY routes remain
unsupported for this adapter; an owned constructor never falls back to local
birth when it is withdrawn or the route refuses a request.

The native protocol separately reports pending reservation, retained birth,
helper pipe closure without reported exec error, and actual reported exec error.
`no_exec_error` is not proof that the target ran or became ready. A failed native
attempt remains recorded; only an actual unused-attempt proof permits the
`not_spawned` observation. An unresolved refusal remains `refused`. Retrying the
same operation cannot create a second child. These observations grant neither
frontend attachment nor all-Source-work/next-goal completion authority.

Validation in the working component: native agent/GUI strict lib-and-tests lint
passed; eight native Root tests passed, including actual descriptor transfer,
argv[0], owning wait status and descendant group closure. Source core/aterm
strict lib-and-tests lint passed for the adapter before the final small core
hook. The actual core library and unit-test target compiled (6m01s); the existing
spawn trace test and ten full-buffer output/cleanup tests passed. Sixteen focused
Root tests across Source core/aterm passed, including closed response codecs,
raw command data, actual descriptor transfer and partial-frame shutdown.

This is component validation, not a shipping certificate or a complete live
update demonstration. Production native Root creation, a real positive SDK
Root/tool fixture, other tool/MCP/helper/PTY routes, frontend attachment and full
UI/input handoff remain unfinished. The application certificate remains blocked
by the compiler's in-process proc-macro dependency TCB refusal. No package was
shipped, installed or linked, and ordinary frontend Core ownership has not yet
been moved into the retained backend.

The subsequent core-library lint completed successfully (2m52s). It reported
four diagnostics in files unchanged by this component: the existing
`executed_tool_calls.rs` atomic deprecation and three `realtime_conversation.rs`
collection-drain suggestions. It reported none in the new spawn/output glue.
This was the workspace lint policy, not a claim of warning-free upstream or a
replacement for the source-selected certified application gate.

### Frontend-created roots and physical listener publication

The new native startup handler reserves one actual startup on the original
foreground controller allocation. It uses the normal program store, native
Source admission, captured process environment and actual App scope. Repeated
requests reconcile that same startup; a wire root ID, reported PID or claimed
readiness cannot select or construct its owner. Source has a matching client on
the original controller connection, with a closed response schema, exact original
session validation, private loopback endpoint checks and wiped credential text.
This client is not yet called by normal TUI startup.

Native supervision now starts after actual SDK/control binding and before
backend configuration. The private backend waits for that native owner before
configuration can launch more helpers. Listener discovery uses the actual TCP
listener: the original SDK member reserves a correlation nonce, then transfers
one descriptor on its original bound socket. Native code validates the kernel
listener and address before returning RPC transport data to the original live
foreground controller. The old diagnostic-banner address parser is removed.
The app-server transport change is one marked call immediately after listener
bind. Ordinary app-server processes retain their existing behavior.

The retained backend's stdout and stderr are continuously drained into discard
sinks under clones of its actual counted operation; EOF releases those clones.
The original bootstrap stdin is replaced with null after retaining its
close-on-exec socket clone, so unrelated helpers do not inherit the private
channel. Root startup without listener publication expires and retains native
cleanup custody. Cleanup now retries kernel-observation errors rather than
surrendering an unresolved original cohort.

Component evidence: ten native Root tests passed, including actual listener and
stdio descriptor transfer, two-megabyte diagnostic draining with scope retention
until EOF, and owning cohort completion. Three Source descriptor tests passed,
including actual listener publication. The kernel listener tests first exposed
macOS rejecting SO_ACCEPTCONN through getsockopt; the implementation now uses the
existing libproc socket-state decoder on macOS and the socket option elsewhere.
All three kernel tests then passed, including refusal to treat a real socket as
an SDK binding. Changed Source core/aterm strict lint passed. The actual app-server
transport hook passed its workspace lint check; broad strict lint stopped on an
unchanged diagnostics-crate atomic deprecation, and the workspace-policy run
reported upstream diagnostics rather than suppressing them.

These checks do not establish positive SDK Root startup or new-version handoff.
A real foreground SDK fixture, normal TUI connection with complete configuration
forwarding, explicit ordinary-exit cleanup with a reliable actual-owner receipt,
all remaining process routes, backend/frontend native handoff gating, full
UI/input preservation and a certified application build remain required. RPC
transport credentials alone do not establish a NativeGo/replacement-frontend
admission or an all-Source-work/next-goal completion receipt. No package was
shipped, installed or linked by this component.

### Captured-root settlement remains observable after queue closure

Native Root stop no longer relies on a one-shot message reply. It cancels the
actual operation and returns an observer of that exact owner's settlement. A
private custody wrapper holds the actual OwnedRoot and publishes the sticky
observation only after dropping it returns; that drop retains and retries the
original backend/tool cohorts until their cleanup succeeds. Queue closure,
request cancellation and generic App-scope retirement never publish a receipt.
This remains a captured-native-root observation, not all Source effects drained
or permission to advance a goal.

The original foreground controller now has a separate stop operation. It selects
only its existing native startup allocation, cancels startup if still pending,
and reports stopping until the actual owner's captured-cohort receipt exists.
It admits no new App work and accepts no wire root ID/PID. The matching Source
client validates the original controller connection/session and the exact
captured-native-root receipt scope. Ordinary exit must use it before withdrawing
its controller; NativeGo/handoff remains a distinct lifecycle operation.

Eleven focused native Root tests passed, including queue/scope-retirement denial,
actual cohort waits and existing diagnostic/descriptor checks. Two Source
frontend transport/stop codec tests and strict Source lint passed. No positive
SDK Root was fabricated for a unit test: the real foreground SDK start/stop
fixture remains required. Normal TUI startup/exit wiring, full handoff and the
certified application gate are still unfinished.

## Current integration boundary after main alignment (2026-11-15)

Qualification branch `alab/live-update-qualification` merged current main
`779eb1cd32` in `7e14aeb1a3`; both commits are on origin. Conflict review preserved
native backend exports alongside current model-goal modules, tool observers,
accounting, event sinks and resumed goal restoration. Four timeout-evidence Python
controls pass with Python 3.14; they do not qualify the Rust runtime or application.

The earlier real SDK VM capture
`~/codex-live-update-evidence/sdk-root-live-stack-vm-28184.noindex`
proves original foreground admission, physical backend control binding, native
root capture and real credential-manager construction. Root startup still timed
out before a published ready endpoint. The aligned diagnostic build subsequently
compiled actual Core and app-server successfully (explicit unverified test lane,
5m17s). Its VM capture `sdk-root-phase-checkpoints-vm-52006.noindex` completed with
exit 1 before the root case: the existing committed-Ready-loss control did not
report same-report recovery, while census observations repeatedly returned Busy.
That result alone does not establish whether the fault fired or whether retry
regressed. Temporary upstream checkpoints were removed afterward. No completed
frontend replacement or readiness permission is inferred from these observations.

The normal TUI needs an owned-local backend path, not a generic remote target.
`tui/src/lib.rs::start_embedded_app_server_with` supplies the complete live
`InProcessClientStartArgs`, including cloud loader, environment manager, policy,
state/log databases and feedback. These handles are built in startup orchestration
before the present client-choice boundary. The current native root request carries
only raw string overrides, which cannot preserve that constructor. A complete typed
invocation must move construction into the retained backend before the frontend
creates those live owners. Preserve non-UTF8 paths and use exhaustive field mapping;
serialized bytes remain data, with admission supplied by actual native owners.

`alab-tui/glue/upgrade_receiver.rs` currently requires a fresh in-process handle
and rebuilds a forest. Retained-backend recovery instead needs reattachment and
request/event reconciliation against the advancing backend. It must not replay
confirmed mutations, issue ordinary ThreadResume over the surviving runtime, or
release goal continuation before editor/input/layout restoration is acknowledged.
Remote workspace/auth behavior must not accidentally replace local behavior.

Native review found that current failed postbirth spawns are retained in
OwnedRoot.tools before return; the rejected-map type alone is not evidence of a
settlement leak. Listener publication does have a transient-admission ownership
issue under repair: keep the actual received descriptor across Busy rather than
cancel the root. Full native gates, certified application/package qualification,
busy-turn/input/editor recovery and actual admitted new-version adoption remain
required. The goal is incomplete.

Compiler availability was rechecked against the configured official release source:
remote `alabsystems/aterm` index `atpkg-index-142` matches the local cached signed
index asset (SHA-256 `c3e2f26f4783a8f09dd3b10b813faa002d08943cdebae27cc87fc289c55d1b50`).
Its ARM macOS Trust pin and newest visible `alabsystems/trust` release both remain
20882. That build's compiler `82bb867ed` and Targo `b532bb2f` do not include Trust
`a213b2ec20` (November 14), which enables isolated proc-macro workers in verified
macOS lanes. The old application refusal covers the broad dependency closure,
including Serde, Tokio, Clap and SQLx macros; replacing a local ALab macro is not
a remedy. No compiler adoption or gate rerun was launched on an unchanged lane.
A qualified published compiler containing the isolation change is still required;
verification policy, source selection and the working lock remain unchanged.

Native main was fetched to `bf03ea6986e`. It already supplies per-session retained
service custody (`codex_retained_service.rs`, `codex_source_owned_service.rs`),
actual foreground/SDK/control checks, fenced direct-descriptor attachments and
complete byte-preserving arguments/environment over ALABSET1. These supersede
this branch's listener endpoint startup and kernel environment-count workaround.
Native main still lacks the branch's distinct per-tool/MCP cohort inventory, so
that custody must be deliberately integrated rather than deleted with old startup.
The native prototype is preserved and pushed as `b72c1fd6941` on
`fix/codex-controller-proof-sid1`; it has not passed full verify land or landed main.
Its root service tests passed 6/6, listener checks 3/3, and diagnostic host compiled
on source-selected Trust 16387. This is component evidence only.

Source now implements the ALABSET1 startup-settings codec in `0babf321a8`.
Seven protected source-selected Trust 15698 tests passed, including native-format
vectors, non-UTF8 bytes, bounds, invalid data, redaction and a real Unix-stream
write-side half-close. These are codec/transport controls, not real native SDK
attachment qualification. Complete runtime construction and product activation
remain required; raw arguments/environment alone do not serialize live handles.

### Versioned callback lifetime and descriptor exclusion

Real Source socket controls exposed a native callback lifetime defect: an
unversioned F/FD send followed by immediate close can finish before Source
accepts; Darwin kernel peer observation then fails with ENOTCONN. The new native
protocol is F1 plus a 32-character nonce and one Unix stream descriptor. Source
retains actual endpoint custody, writes the exact nonce receipt and half-closes
its write side. Native requires the receipt and EOF, retaining the original
callback/endpoint through receipt or terminal uncertainty and rechecking actual
SDK/controller/foreground admission. Lost/invalid receipts never resend the FD.

The independent native main-based worktree is
`~/.ubd/codex-retained-protocol-sid1-52006.noindex`. Protocol commit
`531b1f6aaa0` and separate GUI lint cleanup `ccdd4cb7ed0` landed on native
`origin/main` in `1a37b64bf1147ae5ca52e1d96593c06f2e28d132`. All eight required
`verify land` checks passed, including cross cells, full tests and AppKit. The
verifier owned moving-main composition and rejudgment; no force push or manual
gate substitution was used. The native worktree was clean with no remaining
check processes. A later safe pull advanced it to `2be755c029c2`, retaining both
commits. A subsequent safe pull aligned the native worktree to `62b1b160f4e`,
clean with no outgoing commits. The diagnostic host/lab pair was rebuilt from
exact `2be755c029c` with selected Trust 16387 and retains that provenance; it is
not relabeled as the newer main. Read-only review found no mismatch between the
Source proof and native's base retained fixture. This qualifies the native
callback fix, not the full live update goal.

Source's retained client is restricted to component tests and the explicit debug
`native-source-proof` feature. It correlates one immutable
startup operation and authenticates callbacks against its original kernel peer;
receives F1 and rights without discarding later ancillary data; retains uncertain
rights before awaiting a receipt; and joins cancelled delivery waiters instead
of resending. Selected Trust 15698 controls verified default Command and pre_exec
spawns exclude intentionally non-CLOEXEC rights. That evidence does not cover
raw exec by itself.

A shared ALab Core exclusion guard now covers synchronous rights adoption/CLOEXEC
and the two ALab raw exec paths. Actual capsule/custody owners drop before the
guard unlocks on failed exec. Existing private control/admission/custody/recovery
socket creation and polling also use finite guard scopes, without holding it
across async waits. Four guard tests passed; all 55 PTY tests passed with explicit
inheritance as the default. The default-exclusion controls retain a real forced
Native-inherit negative control and explicit preserve_fds coverage. Twenty-five
TUI prepared/admission/custody/runtime-data checks passed; five explicit
CLI/native-required cases remain ignored. Final connection/exec/retained-client checks passed 22 tests with one helper
case ignored. The full observational lint completed; its one own byte-literal
warning was fixed, and focused no-deps test lint then passed without code
diagnostics. Strict lint still fails on unchanged upstream deprecation; this is
not a strict-lint or application-certificate pass. Package-daemon liveness probes and input wake/data sockets
are not covered by a claim that every process FD is guarded; complete NativeGo
input/producer restoration remains unqualified.

These changes fix transport and custody prerequisites. Ordinary TUI retained
startup, full constructor preservation, backend Core/tool/MCP ownership,
request/event reconciliation, editor/input frontier acknowledgement, certified
application/package admission and actual new-version adoption still require
implementation and verification. The full objective remains open.

Source descriptor-custody changes are committed in `98fd259b6a`; the quarantined
F1 client and its controls are committed separately in `6a17e2cee3`.

The independent-controller/backend A-channel slice is committed in `d7267e33fc`.
It binds the
actual private stdin challenge, retains controller/channel ownership and queues
actual received endpoints before nonce acknowledgement. Full queue and nonce/ACK
waits have one transaction deadline and observe owner withdrawal. It does not
install the old Root monitor or replace startup inputs with defaults. Eight
backend controls passed, including the normally ignored subprocess helper when
explicitly invoked by its parent; 13 F1 controls and focused test lint also
passed on protected source-selected Trust 15698. These are component results;
actual native SDK integration remains unqualified.

### Persistent owned input and explicit SDK proof

The next Source slice supplies an opaque owned incoming-stream provider to one
existing app-server runtime. It preserves the caller's constructor arguments,
requires local transport `Off`, and uses the existing WebSocket request/event
implementation and remote client's initialize worker. Owner withdrawal shuts
down actual socket fences and awaits connection cleanup through `Closed`.
No pathname, marker, PID, nonce or readiness value supplies native permission.

An explicit debug-only fixture recognizes native's existing
`alab-native-retained-proof` / `--alab-retained-service` contract. The frontend
opens independent controllers, starts the native-owned backend, performs a real
typed `ConfigRead`, withdraws its first connection, then attaches a second
controller to the same service and repeats the request. The backend binds the
physical native control challenge before credential seeding and runs the real
app-server with the owned provider. Its explicit fixture constructor defaults
do not preserve an original TUI invocation and cannot qualify product startup.
Unsupported fixture variants are rejected rather than routed to another proof.

Ten focused incoming-stream controls passed on protected source-selected Trust
15698: three Core, one runtime, two client and four transport tests. They cover
successive connections, existing initialize inputs, cancellation custody,
compression/frame negotiation, stalled handshake and actual owner withdrawal
through `ConnectionClosed` with writer release. The initial compile found a
test-only `AbsolutePathBuf` constructor mismatch; the checked constructor was
fixed before the passing run. Existing upstream deprecations and the large
app-server test binary's unwind-table linker warning remain separate findings.

Feature-aware lint found a queue guard held across an async wait. The fixtures
and real proof adapter now poll their receiver synchronously under a standard
mutex, releasing the guard before `Pending`; no lint was suppressed. The corrected
implementation passed the same ten tests, plus eight backend controls with the
proof feature enabled and the inherited-stdin helper explicitly exercised.

The explicit proof executable built successfully on the same protected selected
15698 test lane. The first feature build caught a typed request-field mismatch
(`request_id` was required); it was corrected before the successful build. Its
SHA-256 is `3cadcd158b0fc7efac9d84d63700e16d9693906fd7b223f33984994faa74fd3d`.
Actual-binary frontend/backend markers with unsupported extra arguments both
exited 1 with the fixed error and no stdout. These are rejection controls, not
positive SDK permission or runtime qualification. This executable became an
intermediate artifact when feature-aware lint exposed two panic paths and an
unused custody field. Failed F1 workers now return an unresolved result while
retaining the same operation and any actual descriptor. Backend owner drop
explicitly shuts down a pending endpoint while retaining descriptor custody
through cancellation; poison recovery is restricted to cleanup.

All 24 current F1/backend controls passed after these fixes, including two new
failed-worker cases and a pending-descriptor control that retains the pending
attachment Arc after owner drop and requires actual peer EOF. The inherited-stdin child helper
was explicitly exercised by its passing parent. A new test's immediate Tokio
read initially encountered `WouldBlock`; the test now awaits the actual bytes
outside its mutex and restores retained custody. Combined feature-aware lint then
passed for six selected packages and all their targets with zero ALab
diagnostics and zero errors. Nine warnings were upstream-only (diagnostics,
Core, Core test support and app-server message processing). This is a focused
lint result, not a strict application gate or shipping certificate. The final
executable rebuild and actual SDK round trip remain pending.

The selected protected resolver admitted only the exact lockfile and build
target, adding three dependencies to ALab's probe entry without changing upstream
lock entries. No application certificate, ordinary TUI integration, installation
or completed goal is claimed from these component controls.

Read-only startup review identified the actual TUI constructor boundary:
`start_embedded_app_server_with` builds complete `InProcessClientStartArgs`, which
become `InProcessStartArgs` for `start_uninitialized`. They retain real config,
CLI/loader/cloud inputs, executable paths, network policy, environment manager,
feedback and state/log DB handles. The current factory returns only an in-process
client. An owned-local route must cover initial connection, reconnect and
config-driven restart while preserving local auth/cwd/thread semantics; generic
`Remote` clears local auth restrictions. The standalone `run_main` proof fixture
does not provide this integration.

The requested Git alignment pulled Source main to `71b2e14837`, merged it into
the qualification branch as `b38d37a6b2`, and pushed that branch. Main and the
branch each matched their remote tips after the push, with main an ancestor of
the branch. Uncommitted runtime/proof work was preserved for validation, then
committed and pushed separately as `20bbbe5fe3` (persistent input/runtime/client)
and `1a6f7e50d9` (explicit SDK proof and cleanup controls). A final main pull found
no further change. Delta inspection confirms the upstream changes are marked
hooks: app-server's conflict surface changed from +12/-3 to +19/-3, plus nine
new mount/export lines across the client and transport files. Implementation
and tests live in ALab-owned files.

A fresh official release check found index 144, published November 15 at
18:52:03Z, with unchanged Trust pins (ARM macOS 20882, generic 16387). The
remote asset matches the cached signed index; the newest Trust release remains
20882. Its compiler `82bb867ed` and Targo `b532bb2f` still exclude
`a213b2ec20`. No newly published compiler that fixes the verified proc-macro
blocker was found, and no pin, channel or verification policy was changed.

### Real retained SDK qualification and fixture bootstrap correction

The initial real retained fixture reached native SDK readiness, physical A/F1
attachment and WebSocket initialization, then lost the transport on its first
`config/read`. Bounded proof diagnostics excluded JSON-RPC parsing, response
deserialization and client shutdown. Native observation through the actual
unreaped child established SIGABRT before that service's stop, without assigning
the sender or claiming Source effects had settled.

A debug-only native stderr tee then classified complete, untruncated stderr as
`stack-overflow`; raw messages, paths, thread names and backtraces were not
exported. Static inspection of the frozen Source executable found a 4,071,408-byte
initialized-request polling frame. The fixture had bypassed the ordinary CLI
and app-server `arg0_dispatch_or_else` bootstrap, whose shared policy provides
16 MiB stacks for the main runtime thread and Tokio workers.

`acab3923ab` corrects the proof to use that existing production bootstrap and
passes its complete executable paths to both backend routes. It introduces no
new production stack value or environment override. The private argument checks
remain; physical backend binding still precedes SDK/auth setup. Optional dotenv
loading, helper filesystem setup and PATH changes are the existing wrapper's
documented startup behavior.

Protected selected-15698 build 25467 and explicit proof-target lint 84074 passed;
the latter reported no ALab diagnostics and two unchanged upstream deprecations.
Entry negatives passed in an isolated home. Earlier `--all-targets` lint did not
include this `test=false` executable; the explicit `--test native-source-proof`
invocation corrects that coverage gap. Tippy now uses its documented, separately
admitted child cache instead of invalidating the executable-build cache.

The corrected test package SHA-256 is
`56905bcddb2dc90876ecb45e05c35beb2080918c4ae31da612639afd4cb323f0`.
Its exact-build provenance records the original a7 source plus the reviewed
patch, not a clean committed build. Real VM 74105 passed with complete capture:
two physical attachments and actual typed config reads, a fresh second frontend
on the same surviving backend, current SDK admission, exactly one backend image,
then Native-confirmed App drain and backend cohort absence. The backend was
still running before cleanup. Its partial stderr capture does not prove an
absence of errors.

This is a genuine retained-SDK component result using a synthetic qualification
issuer. It does not qualify ordinary TUI startup, active work preservation,
production activation, certified shipping or actual new-version adoption. Those
requirements and the full goal remain open.

### Complete initialization inputs and requested Git alignment

The native-stream client now accepts the caller's complete `InitializeParams`
and queue capacity. It passes those values to the existing WebSocket client
worker, preserving title, attestation, gateway OAuth, extensions and optional
capabilities. The `RemoteAppServerConnectArgs` convenience entry delegates to
this path. Capacity zero follows the existing embedded client's minimum of one;
positive capacities are preserved. The change touches two ALab-owned files and
adds no endpoint lookup, native permission or production activation.

All six actual Unix/WebSocket wire controls passed under protected selected
Trust 15698 (clean command 29716). Focused client `--lib --tests --no-deps` Tippy
61795 passed, including both production and unit-test library targets, with no
ALab diagnostics or errors and two existing upstream deprecations. The first
test execution passed its controls but its outer runner returned a parse error;
the unchanged source was rerun with a fixed runner and exited zero. Exact source
hashes, original logs and scope limits are retained in
`source-exact-initialize-client-validation.json` outside the source tree. These
are transport controls, not another SDK qualification or shipping certificate.

The user's renewed alignment request pulled shared main from `a5bd9052c1` to
`73d34a6698`, matching `origin/main`. The qualification branch is being merged
forward with that main and published with an exact expected remote ref. The
unfinished live-update implementation remains on its qualification branch;
neither this alignment nor the passed component checks completes the goal.

### Complete-input retained local host

The retained host now consumes every `InProcessStartArgs` field and uses the
existing embedded managed-policy bootstrap. It retains the real config,
CLI/loader/cloud inputs, executable paths, network policy, thread loader,
environment, feedback and database handles. One processor serves successive
native-delivered WebSocket connections; frontend EOF performs connection cleanup
without shutting down that processor. The original initialize declarations stay
available as frontend metadata and do not initialize a global client session.

The owner closes input admission synchronously when dropped or shut down. A
cancelled waiter cannot discard accepted startup or its stored result. Withdrawal
before startup skips it; withdrawal during bootstrap retains the same pending
operation until its actual outcome. Boundary checks then avoid constructing a
new processor after observed withdrawal. Dispatcher supervision stops admission
and actual transports while retaining in-flight dispatch, records child failure,
and joins listener, connection and transport cleanup. Provider failures retain
their original error rather than becoming successful withdrawal. Host completion
still supplies no native cohort-settlement receipt.

Implementation and tests are ALab-owned. The only upstream mount/export adds
five marked lines in `app-server/src/in_process.rs`. Actual WebSocket origins
remain WebSocket origins; the adapter does not claim embedded verification
authority from client declarations or a transport label.

Protected selected-15698 checks passed: four Core input controls and 22
app-server ownership/processor/wire controls. The latter create an ephemeral
thread on the first frontend, require no persisted rollout, disconnect it, and
find the same loaded thread through a second frontend. Actual config reads also
observe a changed file while retaining the caller's CLI provider. Negative
controls cover input errors, an actual router panic, malformed connection shapes,
full writer queues, joined listener cancellation and cancelled startup observers.

The first app-server compilation found one private token access; existing
upstream connection cleanup already cancels it, so the direct access was removed.
Lint found one redundant test-provider closure, replaced with the method
reference. Final app check 67383 passed all 22 controls (50 s build, 0.20 s run).
Final focused Core/app-server library and test lint 78603 passed in 29.40 s.
The existing attribution filter found zero owned, spanning, failing or missing
script findings and set aside six distinct upstream warnings. Both production
and unit-test library targets were included. These checks use the established
test stack profile and emit no shipping proof.

Exact working-source patch `9065096600762bf1a6a7f3090f19a4eb9ef68a8cc9525517275491042c46a6e1`,
file hashes, failed attempts and final logs are retained outside Source in
`source-owned-local-runtime-validation.json`. Native observability separately
landed as `df1132c1805` after all eight native checks, including 343 runtime
binaries, documentation tests and AppKit. Historical SDK qualification artifacts
retain their original source identities.

This host is not yet selected by ordinary TUI startup. A client adapter and
headless bootstrap reuse are still pending, followed by active work, approval,
MCP and UI/input continuity and actual admitted new-version activation. Existing
local-daemon behavior shows that actual UI-local config/environment/feedback
objects need not all become wire projections. Native admission permits a
credential member alongside an independent controller, but simultaneous real
frontend/backend SDK use and retirement still require qualification. The fresh
official release check found unchanged Trust 20882/index 144; no new qualified
compiler fix or certified application build is claimed. The full goal remains open.

### Original client-argument conversion for a retained backend

`InProcessClientStartArgs::start_retained_native_host` now consumes the actual
existing client arguments and delegates their original private conversion to
the retained host constructor. It adds no duplicate field mapping, worker,
endpoint or admission rule. The backend main retains the returned owner;
frontends attach their own physical streams. The ALab-owned implementation is
mounted below an existing ALab-owned client module. The sole upstream change
adds the existing ALab Core path as a normal Unix dependency; its dev dependency
remains and the lockfile is unchanged.

Protected selected-15698 client check 92954 passed all six existing wire controls
(5m48s build, 0.01s tests). Focused library/test lint 15652 passed in 3m50s,
including both actual production and unit-test library targets. The existing
attribution filter found zero owned, spanning, failing or script findings and
set aside two upstream warnings. Both commands accepted the committed lockfile.
Exact patch/file hashes and logs are in the outside
`source-owned-local-client-validation.json`. These controls compile the new
entry and regress its shared transport; they do not qualify a new SDK service
or ordinary TUI activation. The optional generic TUI factory change remains
unapplied until the actual backend bootstrap and target selection use it.

### Human-command approvals created during frontend absence

The retained constructor now installs a private wait policy tied to its exact
`Incoming` allocation. A thread-scoped `HumanCommand` request with no eligible
frontend is registered as pending, with no recipient invented. Existing warm
resume replay registers an eligible recipient before delivery and carries the
same request ID and body. Existing response and error authorization checks stay
in force. Ordinary constructors, threadless requests and owner-bound user
verification retain their existing behavior.

Registration and terminal cancellation use the original callback-map mutex.
Retained cancellation closes registration under that mutex before draining;
registration checks the sticky close and the input's actual ended state under
the same lock. A callback queued behind cancellation therefore cannot appear
after the drain. Per-turn cancellation remains separate. This is input/callback
lifetime management, with no new human or native permission grant.

A regression through the actual retained preparation failed first: the approval
receiver was closed while the other 22 host controls passed. With the fix, check
80998 passed all 31 focused controls, including the constructor regression and
eight callback/replay controls. They reject both success and error replies from
ineligible or never-delivered recipients, require the same ID on eligible replay,
observe one completion and no subsequent replay, and cover both cancellation
orders and distinct owners. The existing outgoing suite also passed all 48 tests
(eight overlap the focused set), including verification and request-context tests.

Lint found a test timeout helper's `expect`; it now handles timeout explicitly.
Final outgoing check 33798 passed all 48 tests (37.44 s build, 0.07 s execution).
Final focused Core/app-server library and test lint 28044 passed in 25.15 s; the
existing ownership filter found zero owned, spanning, failing or missing-script
findings and set aside six upstream warnings. Exact patch
`ad6bad324a24fb5e1271bb28004202ae6552c134af7ed337ef627587159bca6e`,
hashes, RED/GREEN and final logs are retained outside Source in
`source-retained-approval-gap-validation.json`.

The source audit also confirms that callback replay alone cannot preserve TUI
dynamic-tool execution: reconnect aborts its frontend tasks, and a new App has a
new deduplication map. Those executors and the frontend-owned MCP service still
need retained ownership. Actual SDK attachment, TUI replacement, active work and
certified new-version adoption remain unqualified; this fix does not complete
the live-update goal.

### Backend-owned task creation

The retained constructor now owns the task MCP listener and accepted create-thread operations independently of frontend connections. START and initial-turn submission use the existing full app-server policy bodies with typed client/backend targets. The complete operation preserves prompt validation, live source metadata, model/provider/workspace/permission inheritance, naming and the initial delegated tool output before returning the child ID. Shared pure helpers keep the TUI implementation on the same policy.

Each private route is bound to the exact created Core runtime; endpoint JSON and thread labels grant no ownership. Actual managed MCP constraints and namespace ownership are checked before advertisement. Catalog discovery can finish during Core startup, while mutation admission waits for binding. Ordinary client warning, subscription, response ordering and tracing remain on their original paths. Backend execution adds no client connection, fabricated request context, UI acknowledgement or human approval.

The accepted operation owns its entire recipe. A synchronous work lease returns the original work to its record on cancellation, and terminal guards preserve an uncertain observation on coordinator loss. Source-only startup custody distinguishes an unentered Core-start path, an exact captured runtime and an unknown outcome. Captured failed-start cleanup uses the existing bounded exact-runtime shutdown helper. Mutex guards do not cross awaits; final record destruction occurs outside the registry mutex. None of these observations claims Native effects settlement.

The actual HTTP test starts a real inspectable root, obtains its private MCP endpoint from the loaded Core config, blocks a required child MCP process at initialize, closes C1 and initializes C2 while CREATE is still pending, then releases the process. It requires one child, one delegated model request, the same source/runtime identities and no UI acknowledgement. Negative controls reject a mismatched parent and bearer without a child or model request. Other controls cover policy errors, post-birth cancellation, lost waiters, runtime teardown, late captured ownership and aborted work leases. Protected source-selected Trust 15698 checks passed all 16 START/turn/HTTP/work-lease controls, 12 existing TUI task controls, 14 Daybreak controls and 31 retained-host controls. The combined fixture run uses four test threads for the measured 256-file process limit. Final production/test-library lint for app-server, ALab Core and TUI has zero owned, spanning, failing or script findings; 11 upstream warnings are set aside. These are explicit unverified tests, not a shipping certificate. Exact 25-file working patch `400a15d75dac037f6662aff4af151578ab59df837e508876d282fbfb3552de7b`, source hashes and logs are retained outside Source in `source-retained-task-create-validation.json`. The checks ran on the committed base plus that dirty patch, not a subsequently named clean commit.

This is not ordinary TUI activation. The retained target still needs to select the backend service and preserve the remaining task/worktree operations, resume/fork/config refresh, UI state and input frontier acknowledgement. Actual Native SDK replacement and certified adoption of a new application version remain unqualified. The full live-update goal remains open; no managed install or update channel changed.

### Backend-owned task reads and waits

The retained task MCP service now executes the complete `list_threads`, `list_archived_threads`, `read_thread` and `wait_threads` operations through its actual ThreadRequestProcessor. The issuing parent comes from the existing private runtime binding; the operation uses the same Incoming, cancellation token and original typed read helpers. It checks the actual parent's current registration before work and publication. Read cancellation or owner withdrawal does not return a stale successful snapshot.

The backend and TUI share the existing pure schemas, limits, account/archive filters, history fallback, cursors, formatting and wait predicates. Embedded pure policy moves into ALab glue because the remaining TUI module needs frontend transport and AppEvent types; the already standalone original JSON serializer is reused unchanged by path. Wait subscriptions are hints obtained before each snapshot. The existing one-second refresh and total/per-target deadline budgets remain. An atomic tracked-only watch helper prevents arbitrary unknown UUIDs from leaving permanent watcher entries, and fresh receivers handle removal/recreation.

The actual HTTP controls use one genuine StateRuntime allocation shared with normal Core/store construction. A fresh metadata-only RPC succeeds, while full history before the first user message preserves the actual typed RPC error. After a real gated model turn completes, HTTP read requires the same completed turn and assistant text; active-list IDs must match the initialized client's account-filtered typed response. Controls also cover positive timeout expiry, unchanged cursors, invalid/self/duplicate targets, cancellation, owner withdrawal, private binding/bearer checks, output bounds and watcher retention. Initial failures exposed the test fixture's missing database and its unsupported pre-message history expectation; production read policy was preserved.

Protected source-selected Trust 15698 checks passed 23 backend task controls, 12 existing TUI task controls, the original history-compatibility control and 31 retained-host controls. Final app-server/ALab Core/TUI production and test-library lint has zero owned, spanning, failing or script findings; 11 upstream warnings are set aside. These are explicit unverified component checks. Exact 13-file working patch `9519af55743971181f16df01081d2fc937df81efa254fb6b608a6c50e44f329b`, source hashes, failed attempts and final logs are retained outside Source in `source-retained-task-read-validation.json`. TUI checks preceded app-server-only fixture changes; all affected production file hashes are identical. No shipping certificate, Native SDK replacement, ordinary TUI activation or completed live-update goal is claimed. Remaining task/worktree mutations, frontend selection/state/input preservation and certified actual new-version adoption remain open.

### Backend-owned task title and archive operations

The retained service now implements the original `set_thread_title` and `set_thread_archived` tools. Their schemas, strict arguments, default target, raw-title result, canonical self-archive rejection and result formatting are shared with the TUI. The backend calls the complete existing name/archive/unarchive helpers, retaining the original archive list-state permit through notification enqueue. Name changes use the helper's own permit. Neither path invents a client, UI acknowledgement, permission or Native role.

Accepted mutations are stored and started under the same mutex that closes admission, then owned by the existing host task tracker independently of HTTP observation. Owner cancellation is checked before entry to the complete helper; title helper entry includes its internal permit wait, so this does not promise cancellation before the first storage effect. Once entered, the helper is retained through shutdown drainage. A normally joined result retires its record outside the registry lock. An unpolled lost worker or polling/destruction panic retains its actual request and parent Arc with an unobserved outcome, preventing a clean host-stop result. The operation is not replayed. Source helper completion and the archive helper's bounded shutdown policy do not establish Native effects settlement.

Actual HTTP controls rename the real root, observe normal persisted-name normalization, reject invalid/self-archive arguments and require real archived-state changes through archive and restore. An accepted title request waits behind the actual policy permit while its HTTP observer and C1 close; C2 sees the same actual runtime, and the title completes when the permit is released. This control keeps the host input live and proves acceptance continuity, not a particular private helper-entry phase. Fault controls use the same production future observer, retain the exact parent ID/Arc for all three loss cases and require the specific unobserved-management cleanup error. All failure paths release the permit before draining and close the actual shared database.

Protected source-selected Trust 15698 checks passed 26 backend task controls, 12 existing TUI task controls and 31 retained-host controls. Final production/test-library lint has zero owned, spanning, failing or script findings, with 11 upstream warnings set aside. One test-only redundant closure was corrected before final validation. Exact 11-file working patch `aac508c72cd0a8e63f55b4add09656bd711040c7dcd9db63dc4cb02f111c4853`, source hashes and phase-specific logs are retained outside Source in `source-retained-task-management-validation.json`. TUI runtime checks preceded a private backend future-observer refactor and test additions; the TUI and shared pure-policy file hashes are unchanged, and final backend controls exercise the refactored observer. These explicit unverified component checks do not certify an application or qualify ordinary TUI replacement. SEND, fork, worktree operations, frontend/bootstrap selection, UI/input continuity and actual certified new-version adoption remain open. Fresh official ARM Trust 27009 metadata includes the previously missing proof-worker fix; adoption and the normal certified application gate have not yet run.

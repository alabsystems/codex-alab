<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Coherent release: native qualification follow-up

This records component evidence and candidate corrections, not a release verdict.
The frozen combined source is `ed67548040`; the tooling correction is separately
`601faacc45`. Neither main publication nor a new signed package is claimed here.

## Why the installed package lagged

The managed selection was build `2026110901`, signed index 125, snapshot
`0.2.0-dev.15239+codex.0.167.0.g022ae520c`. Its source commit was recorded on
2026-11-09. Compared with private main `d20a5d89c9`, it lacked 150 commits across
75 files (5751 insertions,275 deletions). Graph counts are not counts of distinct
features. The `0.2` prefix names the product release family; the dev count and
source hash identify an iteration.

`daily.sh` synchronizes upstream source. `local_update.py` advances only an
opted-in local artifact channel. This installation used the managed store,
which follows the signed package pin. A branch push or a main push does not
publish that pin. The observed atpkg selector served its published pin correctly.
The current release candidate's product line is 0.4.0; its publication remains pending.
The original 27 branch heads and 54 preserved variants have explicit composition
reviews in the adjacent inventory and preserved-variant review. Ancestry alone
is not a claim that every experimental implementation should be activated.

## Completed components

On frozen `ed67548040`, the native Trust 15698 gate completed all-target Tippy
with zero ALab diagnostics and 40 upstream diagnostics set aside. The CLI built
and started. All ALab targets passed 1,852 tests, with four existing ignored.
The 32-package hooked-library graph explicitly passed
`app::agents_overview::tests::actions::archiving_selects_the_next_displayed_task`
at the standard 8 MiB stack. No recursion or runtime stack limit was increased.

The protocol suite passed its four stable/experimental comparisons. Both actual
schema writers then regenerated scratch archives byte-identical to the selected
checked-in stable and experimental exports. No provisional archive replacement
remains necessary for this source.

The complete Python tooling suite on `601faacc45` passed 550 tests in 1,026.234 seconds,
with seven skips. The pristine dependency preflight now carries the same vendor
overrides as actual pristine builds. Native context setup happens at the tested
runner boundary after Cargo's launcher, retaining the descriptor, signal and
Python negative-control assertions. The focused native context suite passed 3 tests.

The built CLI passed both quota-wait cases, including native SIGINT cancellation
in about 0.026 seconds, and all 27 earned-reset cases (`fullSuite: true`). These
used isolated fake credentials, loopback servers and the maintained sandbox.
The native harness probe printed `SMOKE OK`: proven claim, policy answer reaching
Core, landing, same-PID re-execution, reclaimed claim and no post-landing typing.
Its outer evidence-copy operation exited 1 on the leftover fake Unix socket; that
outer command is not recorded as exit 0. The packaged-binary probe must exclude
only ephemeral sockets from its archive and record the actual test status and
executable hashes before and after.

## Corrections requiring native retest

The hooked-library run found 30 failures; all 30 persisted in isolated reruns.
Pristine attribution remains pending. Retain those red controls and fix actual
ALab failures; do not waive them or call the full gate passed.

- Realtime initially bound the saved login's native source policy even when an
  explicit provider key won selection. Upstream then selected an independent
  runtime credential owner and correctly rejected pairing that owner with the
  old transport. Bind the saved source only when its API key is actually used.
  Explicit and environment keys retain their own runtime selection. The strict
  HTTP policy joining and independent-application-controller checks remain.
  The existing upstream saved-login/explicit-key regression supplies the control.
- A login fixture used `Arc::get_mut` after publishing a weak catalog handle.
  Use the existing before-publication fixture configurator, preserving its policy,
  rotation and revocation assertions. Do not remove the real weak catalog handle.
- Reminder custody fixtures still served only `getAuthStatus`, although production
  now requests `account/securitySetup/read`. Use the actual embedded app-server
  account handler against the gated loopback HTTP fixture. Preserve every native
  observation, ordering, identity, restatement, timeout and settlement assertion.
  A successful empty response with no observed native entry needs no App event;
  it cannot settle anything. An observed entry still requires current-request and
  identity admission before conditional settlement. This also preserves upstream's
  no-notice-event contract when nothing was observed.
- The pet preview fixture called a frame-consuming draw getter after the actual
  resize had already drawn that frame. Observe the existing placement-visible
  flag without consuming it. Keep the real resize, overlay clear and restoration
  assertions, with no production draw-policy change.

The plugin runtime actor-credential failure after network-policy invalidation
still requires its pristine control; no speculative correction is included.
Other daemon, socket, sandbox, path and deadline failures also await attribution.

Run the affected cases with the complete hooked feature graph and actual native
adapter, then the full canonical gate on the final committed source. Finish the
fixed-commit main publication, release check, immutable package, staged canary,
signed index publication, public readback, normal installation and running-image
adoption. SourceV1 remains unadvertised; this release work does not grant its
separate native controller/consumer qualification.

## Pristine controls and positive outcome boundary

The original gate ended exit 1, correctly refusing to excuse an ALab-only pet
case that matched zero tests on pristine upstream. The actual pristine runs
passed the realtime explicit-key, plugin runtime, primary provider policy,
trusted-access deadline and upstream null-reminder cases that failed in the
composed layer. Seventeen other copies failed there too. The eight ALab-only
pet/custody cases have no pristine counterpart; their source failures remain
gating. The retained gate log and original/isolated result records preserve
these controls; this is not a passing attribution verdict.

The real app-server fixture exposed an API ambiguity: upstream returns a null
notice for successful empty HTTP as well as HTTP refusal, timeout, unavailable
credentials and other failed reads. Require optional, positive server evidence
for the successful empty result, including the actual fetched account and user.
The owned server implementation retains upstream eligibility, source generation,
HTTP policy and post-fetch identity checks, applying them to empty successes too.
The client verifies that identity against its observed identity before publishing
an empty settlement candidate. Missing evidence never authorizes settlement.
A real independent-server-identity negative control joins the existing success,
refusal, timeout, restatement, supersession and identity-change controls.
This adds a typed protocol extension; regenerate all protocol exports and the
SDK artifacts on the composed source before qualification.

The plugin failure traced to a cached credential snapshot retaining an old
application policy generation after that policy was invalidated and republished.
A fresh read under the actual manager's source/cache/member guards may recapture
that manager's current policy using the existing epoch-checking publication
helper. A retained credential clone keeps its old policy, another manager cannot
relabel it, and strict policy joining stays unchanged. The new causal control
requires a fresh pair to work while both the old permit and retained credentials
remain refused after policy republication.

The trusted-access fixture assumed one poll reached its recording HTTP client.
Native credential preparation can suspend before HTTP. Drive the original
lookup until its real HTTP gate is entered, retaining the same paused clock,
request-count assertion and 2499+1 millisecond deadline checks. No production
lookup deadline or refusal behavior is changed. All corrections still require
native retesting and the final gate.

## Native repair controls after composition

On `2de0fc363b`, the full hooked feature graph passed the primary application
policy control, both realtime selection cases, plugin runtime registration and
cross-origin stripping, all five selected epoch/source cases, the MCP deadline,
pet overlay/cache controls and the standard-stack archive regression. Reminder
HTTP fixtures failed before reaching their response gate: the actual embedded
server first requests plugin inventory and account routing. The fixture now
serves empty startup inventory and routing independently, then gates the actual
security-setup request. Its native settlement counters, request ordering,
identity-change, refused/timeout and independent-server assertions remain.
The full reminder matrix still requires a passing run and final qualification.

The provider-policy test now obtains the real application-policy factory through
an owned test fixture. Its original assertions concern that policy's shared
restrictions and invalidation, rather than a credential epoch that is separately
revoked by the test's initial key publication. Production credential binding is
unchanged. Existing configure/TLS fixture helpers moved to a test-only owned
file so test-only fixture edits no longer rebuild production dependencies.

The affected native control run on `9451c804ea` completed exit 0 with the full
33-package hooked feature graph, including every reminder custody case and the
standard-stack archive regression. Its record is
`/tmp/.ubd/codex-release-repair-controls-r7-s0` on the qualification machine.
The actual stable and experimental protocol writers passed; their regenerated
exports and SDK types are committed in that source. The SDK contract and artifact
suite then passed all 92 cases with frozen dependencies. These are component
results; the final canonical gate, publication and adoption remain outstanding.


## Fresh release metadata and final source

A fresh native package run fetched upstream stable `rust-v0.167.1`, whose branch
point is already included. The recorded 0.3.0 row maps to 0.167.0 and is immutable.
The canonical publication `pub bump codex-alab --write` therefore advanced VERSION
to 0.4.0, and `package.sh --ledger-row` generated the append-only 0.4.0 row on
0.167.1 (`0a94398fa60c4fac0e7659e4213a0a36e613dd46`). No 0.3 package was promoted
or published. The provisional 69c87ef3d5 package build was cancelled with exit 143;
its source/build evidence remains preserved.

The 69c87ef3d5 gate passed all-target lint (zero ALab diagnostics, 40 upstream
classified), built and ran the CLI, passed 1,852 ALab tests with four existing
ignores, and passed the protocol's 323 tests, including schema comparisons, with
one ignored writer. Its remaining hooked-library run was cancelled with exit 143
to run the consolidated release source directly. Five app-server-transport failures
were present before cancellation and remain subject to native pristine attribution;
this interrupted run is not a passing release gate.

The consolidated candidate retains remote main's two documentation-only banner
owner-decision/evidence commits. The maintained native smoke now supports distinct
old/next package executables, checks immutable hashes and different version strings,
and records actual loaded executable, same PID, same displayed conversation,
exec generation, renewed claim and absence of post-landing duplicate work. Syntax
and help checks pass; the actual distinct-version runtime test awaits the package.

## Qualified package and actual automatic transition

The complete canonical gate on `e12b641e20bf2e6c5da2ac7c173ed947b601b45c`
finished exit 0. It passed 1,852 ALab tests with four existing ignores and all-target
lint with zero ALab diagnostics (40 upstream diagnostics classified separately).
All 17 remaining hooked-library failures persisted alone and failed on pristine
`cce9d3228b5d` too; the gate found no missing pristine match. Native result records
are retained in `codex-rs/target/coherent-release-final-unit-results-r2-s0` on the
qualification worktree.

The immutable 0.4 package built exit 0, passed its layout/helper/signature/defaults
checks, both quota-wait cases and all 27 earned-reset cases (`fullSuite: true`).
The current-image native harness probe passed its full Land-answer, landing,
same-PID re-exec, renewed claim and no-duplicate-work assertions. The distinct
published 0.2 -> 0.4 transition then passed with the same PID and displayed
conversation, exec generation 1, the new immutable executable loaded, renewed
claim and no post-landing typing. Both package hashes remained unchanged.
Its isolated receipt is
`/tmp/.ubd/codex-release-native-transition-e12-r3-s0/evidence/probe-receipt.json`.

The cross-version test names the published starting image's exact recommended
answer and `answer-recommended@v1` action. The current-image control retains the
new Land-specific answer/action assertions. Initial cross-version fixture attempts
failed those new-feature/action-name assumptions before attempting a handoff; they
are preserved and are not recorded as runtime-upgrade failures or passing probes.
This follow-up changes only the smoke test and this historical evidence. The
qualified release remains pinned to e12b641e20; publication, installation and this
session's running-image adoption remain separate delivery evidence.

## Source-export follow-up

Public export of e12b641e20 was refused before publication: ALab evidence documents
contained machine home paths, and two modified upstream files retained public,
synthetic credential fixtures. The central policy explicitly supports inline
`gitleaks:allow` for credential shapes only; all other content rules remain active.
The follow-up makes evidence paths portable and marks those unchanged upstream
fixtures. It changes no Rust tokens, fixture key bytes, production behavior or
scanner policy. The staged signed build 2026111001 remains unpublished. Qualify
and build the adjusted source before selecting the next immutable staging build.

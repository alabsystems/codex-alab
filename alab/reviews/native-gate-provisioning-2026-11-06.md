# Provisioning the native compiler selector for the Codex gate

The earlier checks deferred the full gate because `atpkg-publish` was not on
PATH. Source review found a supported way to obtain the real development
producer: build it from an owned aterm checkout, then let its native project
selector authenticate the signed compiler payload and hold the lease. No fake
front, exported authority fields or containment exception is needed.

On fleet m157, built aterm source `08d1cd2d3b` in the owned detached worktree
`~/aterm-check-s0-setup`, with its required astream submodule:

```text
CARGO_BUILD_JOBS=4 targo --unverified build --profile test --locked -p atpkg-publish
```

The build completed exit 0 in 28.71s (owned shell `s-a845a19f856134e74492`, block 0).
The checkout pins Trust 16387 / compiler `29ebb403de7a`. Ordinary Cargo metadata
named its target directory; the executable is `target/debug/atpkg-publish`.
It was run in place, without installing a tool or replacing the aterm app.

Its official `toolchain run --repo ~/codex-alab-check-f0a4e96f --
/usr/bin/true` admitted Codex's pinned Trust 15698, signed tree
`9e3ecca1051a2231f7a9ed4fce5f5904f77e5f43b0f9dd18da59204efe3040bb`, with payload and
execution both at `trust/15698.noindex`. Block 3 completed exit 0. This is native
compiler admission, not a full gate result or a Rust verification proof.

Prepared Codex's exact source `766acd06bb1f31dcca073b7eaf87f8bb04b01cf8`. The clone
lacked `upstream/main`; maintained `alab/setup.sh` configured a fetch-only remote
and fetched the public baseline through its guarded anonymous helper. It completed
exit 0, with upstream `7de8a0afc0` (block 5).

The first normal protected gate authenticated the compiler but stopped at the
locked offline metadata preflight for both source and pristine trees (block 6,
exit 101): the clone's private Cargo home lacked the tungstenite Git closure.
No compiler started in that attempt. Its refusal remains in `full-gate.log`.

Prepared both exact locked graphs under the same native lease and sandbox, using
the existing helpers in order: `alab_project_run`, `alab_project_selected`,
`alab_check_targets_ready`, `alab_contained`, then `alab_build_command metadata
--locked --format-version 1` for source and pristine. That preparation completed
exit 0 (block 7). The private Cargo home was retained; the owner's cache was not
substituted, and the gate's offline preflight was not changed.

The unchanged normal `alab/check.sh` then ran on that fixed source with the real
producer path, managed Python 3.14, four build jobs and four test threads. The
CLI built and ran (`codex-cli 0.0.0`). The ALab phase reported 1,733 passed, one
failed and four ignored over 25 target reports. The failure was the old
`cruise_continues_a_persons_own_exchange` integration case: it waited for the
continuation that the greeting fix intentionally prevents. Its diagnostic
journal shows a successful `left-to-person@v1` publication. The whole gate exited
1 (block 8), before lint and hooked-upstream unit phases.

Fixture-only `ef59796294` replaces that contradictory expectation with the
reported `sup` / `Hey—what are we working on?` exchange: one model request,
quiet publication and no pending continuation timer. It then submits a later
task and requires the actual `keep going` continuation. Two stale comments were
also corrected. Through the same native controller and sandbox with a scratch
CODEX_HOME, this end-to-end test passed in 0.43s (block 11, exit 0). Neither its
model nor its aterm is real: the test uses a real core thread with local mock
responses and a private fake host.

The normal full gate completed on that exact candidate with the same producer
and containment settings: retained block 12 is complete with exit **0**. Both
checkouts stayed fixed throughout. The complete tee log is 2,653,109 bytes;
terminal history alone retained only its tail, so totals below come from the
complete saved log rather than the displayed screen.

- The CLI built and ran (`codex-cli 0.0.0`).
- All ALab targets passed: **1,734 passed, zero failed, four ignored**, over
  25 target reports. The end-to-end autopilot binary passed all 93 tests.
- Tippy found no diagnostics in ALab's code and set aside 41 upstream diagnostics
  under the gate's existing attribution rules.
- The hooked upstream unit phase completed. Seventeen failing test copies
  persisted in isolated reruns; every one failed on pristine upstream
  `3026a8e54e0f48c4d8074cd700f35b0697abc329` too, making them non-gating under
  the maintained gate. They cover three daemon-update cases, five local-socket
  transport cases, two core permission cases, three exec-server cases and four
  protocol path-alias permission cases. This is a passing attributed gate,
  not a claim that every upstream test is green.

This verdict qualifies source `ef59796294131b249bc3abe00b9913fafd176cc1`.
Later main merge `734a854c86` incorporated independently qualified quota changes;
that merged source does not automatically inherit this fixed candidate's full
verdict. Source landing also does not establish package publication or adoption
by this session. The currently selected signed build 2026110601/index 99 still
predates the greeting fix; no restart into that same binary was performed.
Logs on m157 under `/tmp/.ubd/codex-native-f0a4e96f.8bYgIz`:
`publisher-build.log`, `compiler-admission.log`, `gate-setup.log`,
`dependency-prepare.log`, `full-gate-after-prepare.log`,
`opening-integration-protected.log` and `full-gate-after-integration-fix.log`.

## Reproducible preparation follow-up

Decision: document the protected cold-cache preparation directly beside the
README's offline-preflight description. The earlier wording required both
closures but left the operator to reconstruct the native-selector and sandbox
orchestration. The example uses existing maintained helpers and leaves gate
policy unchanged; no new executable entry point or compiler bypass is added.

Extracted the exact README Bash block and checked it with `/bin/bash -n`, then
ran it on the same fixed fleet checkout with the real producer and selected
Python. Retained block 16 completed exit 0. The selector again authenticated
Trust 15698 and the same signed tree, and both locked metadata commands
completed. This follow-up reused the populated private Cargo cache; the initial
cold-cache recovery is recorded above. No compilation or new full-suite claim
is attached to this documentation change.

The signed index advanced to 100 but `aterm pkg update codex` still reports old
build 2026110601 current. The quota report's newer package is absent from its
default artifact path on both this Mac and the owned fleet checkout host.
Adoption remains deferred. GitHub issue discovery was also deferred after the
configured identity received 403 `issues=read`; no alternate identity or access
path was tried. Work continued on the reproducible preparation instructions.

## Merged-main compaction interaction

The full native gate on merged-main `df7ebf94ac` completed exit 1 (block 18).
Its ALab test phase and Tippy passed, but the real foreground compaction
integration timed out at 15 seconds in both the main and isolated unit passes.
Attribution correctly refused to set aside this ALab-only case: pristine
upstream has no matching test. The failure is retained in
`full-gate-merged-main-df7ebf94ac.log`; it is not an upstream-only failure.

The fixture began with exhausted usage. The new quota preflight redeemed its
only credit before the first model request, but the fixture still forced that
request to return a quota wall. Decision: correct the event sequence and cover
both intended behaviors, rather than weaken timeout, redemption or input checks.
Fixture-only `f17975bed6` runs local and remote compaction with both proactive
and reactive redemption. Reactive cases begin with a healthy preflight and
exhaust usage before the first model response; proactive cases begin under
pressure and succeed on the first model request. Each requires exactly one
consume, the expected usage reads, and preserved original conversation input.
The reactive retry also requires byte-equivalent JSON input across requests.

Through the real native selector, existing containment and maintained
`unit-tests.sh` prerequisites, the exact parent integration passed all four
scenarios in 0.86s (block 21, exit 0). Trust formatting and diff checks passed.
The full canonical gate was rerun on this candidate and passed; its complete
result follows. No runtime quota implementation changed in this correction.

## Complete composed-source verdict

The unchanged canonical `bash alab/check.sh` on
`f17975bed6fe70f36590ee0493920a40a4ccb105` completed **exit 0** (retained block
22). This candidate includes the incoming proactive quota changes and the
corrected foreground compaction matrix, closing the earlier composition gap.
Main `73e9c9ebc2` differs only by the review text above; no unqualified runtime
change was added after the tested candidate.

The CLI built and ran. The ALab phase passed **1,736 tests, zero failed, four
ignored**, over 25 target reports. Tippy reported no diagnostics in ALab's code,
setting aside 41 upstream diagnostics. The full hooked-library pass explicitly
reported the revised compaction integration `ok`, as well as the focused check
recorded above. Seventeen other failures persisted in isolation and all failed
on pristine upstream `3026a8e54e0f48c4d8074cd700f35b0697abc329` too; the existing
attribution rules therefore classified them as non-gating. No missing-result
attribution refusal remains. This is a passing attributed gate, not an all-green
upstream test claim or a Trust verification proof.

The complete tee log is `full-gate-compaction-matrix-f17975bed6.log` in the same
m157 scratch directory, 2,412,617 bytes, SHA-256
`07d46a570268f9968803be5d67d5a3579afd1f6bb2d4220f920c68d108496d96`.
Block 23 independently read the complete log for these counts and its checksum.
All gate and focused-check processes have completed. Signed index 101 still
selects old build 2026110601 on this Mac, so runtime adoption remains deferred;
restarting that binary would not adopt the source fix.

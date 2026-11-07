# Explicit zero-test attribution failures

The composed native gate's compaction fixture failed in the source tree and had
no counterpart on pristine upstream. The collector correctly refused attribution,
but its generic “no unique explicit result” message did not distinguish this
zero-match case from conflicting output. The fixture itself was subsequently
corrected and the composed gate passed, as recorded in
[native gate evidence](native-gate-provisioning-2026-11-06.md).

Decision: identify the exact comparison tree, package, library and test when an
expected copy reports a zero-test summary, and explicitly retain the original
failure as gating. This changes only a rejection diagnostic: zero result lines
already fail the existing acceptance condition. Crashes, missing receipts,
ambiguous output, ignored cases and contradictory summaries retain their existing
refusals. No test is treated as passing because upstream lacks it.

Preparing its regression checks exposed another fixture issue. With selected
managed Python under a path containing spaces, generated shebangs split the
interpreter path before the fixture ran. Native protected baseline block 24
reported 17 tests with 13 failures, mostly missing harness output. Fixture-only
`c95225b3cc` uses the same quoted shell `exec` entry as the package/update fixtures.
The same qualification then passed all 17 tests in 3.084s (block 26, exit 0),
including process-context, child-reaping and artifact-provenance checks.

Candidate `a86b4ef053` adds the clearer zero-match refusal and assertions to the
existing zero-match regression. Through the real native selector and existing
containment with the actual native unit runner, all 17 attribution tests passed
in 3.006s. The test verifies the tree/package/library/test diagnostic, nonzero
status and absence of an accepted result file.

The companion gate-step suite was also attempted under that same outer protected
profile. It completed 38 cases in 417.185s: 31 passed, five failed and two errored
(block 28, overall exit 1). The five failures explicitly report denied allocation
of new `/tmp/alab.*` or `/tmp/.ubd/alab.*` scratch roots. The two errors read missing
runtime records in contained-comparison cases that did not reach execution.
This outer profile does not admit the fresh roots those cases allocate for their
own containment. These seven cases remain unavailable in this qualification
context; the companion suite is not claimed green. No profile was relaxed and
no failed check was rerun outside its boundary. The focused collector suite and
31 completed gate-step cases provide the validation for this small diagnostic
and fixture change; the earlier full Rust gate remains scoped to `f17975bed6`.

Logs on m157 in `/tmp/.ubd/codex-native-f0a4e96f.8bYgIz`:

- `unit-results-managed-python-baseline.log`
- `unit-results-managed-python-launcher-fixed.log`
- `unit-results-zero-comparison-qualified.log`

The source stayed fixed throughout each run; native Trust 15698 admission,
private Cargo home and the existing sandbox were retained. Diff whitespace
checks passed. No runtime Rust source, package selection or deployment changed.

## V8 gate fixture follow-up

The audit found the same unquoted interpreter shebang in
`alab/tests/test_gate_v8_inputs.py`. Protected managed-Python baseline block 29
completed exit 1: four test methods reported six failures and one error across
their subcases. The captured shell error explicitly names
`~/Library/Application: bad interpreter`; fake Cargo never reached
the V8 boundary checks.

Fixture-only `b3386bdca1` uses the established quoted shell `exec` launcher while
retaining isolated Python. The complete four-test suite then passed in 7.075s
(block 31, exit 0), through the same real native selector and outer protected
profile. It checks actual candidate/pristine shell wiring, propagation of both
verified-input paths to final library tests, refusal before native work for
failed/incomplete resolution, and the unrecognized short-query refusal. Cargo
and the V8 resolver inside these fixtures are stand-ins; this does not claim
that the suite compiled V8 or independently authenticated a real V8 archive.
No production shell or compiler-selection code changed. The logs alongside the
previous qualification logs are `gate-v8-managed-python-baseline.log` and
`gate-v8-managed-python-fixed.log`.

The seven companion gate-step cases remain deferred in the earlier outer
qualification context. Source review confirms `alab_contained_scratch_path`
requires a short immediate child of the physical `/tmp`, and `alab_contained`
allocates that namespace deliberately for Unix socket limits and cleanup.
Supporting nested scratch is a separate production containment design change,
not a fixture-launcher correction. Decision: preserve that contract and its
refusals rather than expand writable roots or skip the controls to obtain a
green companion report. The independently executable V8 boundary suite above
provides additional completed coverage within the existing profile.

## Executor smoke launcher and boundary control

The same interpreter-path audit found an unquoted shebang in the executor smoke
fixture. The standard standalone suite on m157 needs no Rust build and uses
fresh private Python executor scripts. Its canonical-driver case establishes its
own per-child containment. Baseline block 32 completed exit 1: five methods
reported two failures and seven errors across subcases, with child spawn failing
before protocol checks because of the spaced managed-Python interpreter path.

Launcher-only `e4939a7765` corrected startup. Four methods then passed, but the
canonical-driver missing-read control failed (block 34, exit 1): its smoke
succeeded even after dropping the explicit read grant. The fixture placed the
driver under the OS temporary directory, outside the declared home. The real
profile permits ordinary reads there by default, so that placement did not
exercise the intended home-data restriction.

Decision: make the fixture root the declared HOME for this canonical-driver
case. Both separate checkouts now sit beneath the denied home, and only the
explicit driver read grant admits the smoke helper. Fixture-only `94c26add86`
adds this precondition while retaining the original negative and positive
assertions. The production profile and its read/write rules are unchanged.

All five methods then passed in 1.732s (retained block 36, complete exit 0), with
no skips. Coverage includes distinct product/protocol identities, refreshed
identity, malformed or refused replies, missing session identity, missing
metadata before spawn, and an injected deadline after real child creation.
Protocol-error and deadline cases verify actual child reaping. The canonical
driver's missing-read case now denies the expected helper path before executor
spawn, and its admitted-read case completes the smoke and reaps its child.
No provider call was made. Diff whitespace checks passed.

The peer source stayed fixed for each run. Logs in the same m157 scratch folder:

- `executor-smoke-managed-python-baseline.log`
- `executor-smoke-managed-python-launcher-only.log`
- `executor-smoke-managed-python-qualified.log`

## Cache-registration fixtures

The managed-Python baseline on m157 (block 40) ran 14 methods with eight
failures and two errors. Quoting the registrar interpreter with the existing
shell/Python launcher pattern restored 13 methods (block 42); the remaining
failure exposed an obsolete assertion that sync itself prepared gate targets.

Decision: test the real `alab_check_targets_ready` inventory directly, retaining
all five ownership declarations and refusal-before-followup assertions. Test
real `run_check` separately with an inert executable gate: it delegates once,
preserves set and unset target environment, and propagates 0, 1 and 75.
No production admission or containment behavior changed. The resulting 15
methods passed locally under managed Python 3.14 in 5.381s; whitespace checks
passed. These are Python/shell fixtures, without a Rust build or provider call.

## Push-race fixture launcher

The next interpreter audit reproduced 15 errors across five push-race methods
under managed Python 3.14: the fake Git never started, so its recording file
was absent. Decision: use the same quoted shell/Python launcher in this inert
fixture, retaining isolated mode. Removing the f-string requires ordinary
single dictionary braces in its generated script; all protocol logic is unchanged.
All five methods then passed in 4.517s, including repeated gated fast-forward
races, capped backoff, refusal to retry authentication/transport/hook/rewrite
failures, and stopping after merge or gate failure. The fixture performs no
real Git push, checkout, build or cleanup. Whitespace checks passed.

## Network-fetch fixture launchers

Managed Python reproduced a `bad interpreter` failure in the network-fetch
fixture before its stalled writer started (exit 126 instead of the expected
transaction failure). Decision: quote the selected interpreter in both generated
executables with the established isolated shell/Python launcher. Remove the now
unused interpreter instance field. Production fetch and deadline code is unchanged.
The suite passed in 4.526s: the injected deadline stops the inherited process
group, the cache lock is released without replacing its inode, a subsequent
ordinary attempt succeeds, and the read-only entry does not create the cache.
Git transfers are between local private fixture repositories; no external fetch,
Rust build or provider request is involved. Managed Git was selected explicitly
because this Mac lacks Apple's Command Line Tools. Whitespace checks passed.

## Daily-cleanup recording helper

Managed Python reproduced nine failures across 13 daily-cleanup methods because
the recording `rm` helper could not start from its spaced interpreter shebang.
Decision: use the existing quoted isolated shell/Python launcher for this helper.
The helper still only records requested removals; production cleanup is unchanged.
All 13 methods passed in 17.298s with managed Git and Python, retaining real
Cargo-flock checks, common preservation markers, missing/unreadable policy-helper
refusal, exact newline-path boundaries, registered versus lookalike revisions,
launchd environment behavior and sync outcome journaling. Whitespace checks passed.
Baseline and passing logs are retained in the session's private handoff directory.

## Version-ledger read-only queries

The ledger scratch environment stripped the caller's ALAB_PYTHON, falling back
to this Mac's unavailable /usr/bin/python3. Select sys.executable explicitly in
the fixture. That exposed the generated release-repository reader's invalid
spaced shebang; quote its isolated interpreter using the existing launcher.
The focused repository-selection case then passed within the broader run.

The first full run still reported 19 failures across 37 methods (94.643s).
A retained diagnostic fixture proved that one successful version query creates
an untracked alab/__pycache__/ directory through the imported GitHub transport;
a subsequent query refuses that now-dirty source. The real repo ignores Python
bytecode, but read-only ledger queries should not write it at all. Decision:
add -B to package.sh's versions.py invocation rather than hide the side effect
with a fixture ignore rule. Add an explicit assertion that the source stays clean.
No history, release-identity or dirty-tree acceptance rule is relaxed.

The full 37-method suite then passed in 111.557s under managed Python and Git,
including engine-named release repository selection, append-only history,
rewritten main and released snapshots, baseline identity and manifest records.
All transfers use owned local scratch repositories. No publication, compiler or
provider operation occurred. Bash syntax and whitespace checks passed. Full
failing/passing logs are retained in the private session handoff directory.

## Contained sync-resolver launcher

The real sync-resolver containment test reproduced a guardian spawn failure at
the generated fake Cargo interpreter before resolution. Decision: quote the
selected isolated interpreter using the established launcher, preserving the
existing per-child containment profile. Removing the f-string also restores
ordinary dictionary braces in the generated metadata reply. No resolver or
permission behavior changed.
The test then passed in 2.983s with managed Python and Git: the real contained
child populates its nested private dependency cache and writes Cargo.lock,
receives EPERM/EACCES for tooling and source writes, and the real sync merges
the expected upstream fixture commit. The fake Cargo is an inert Python resolver;
no compiler, network provider or publication is involved. Whitespace checks pass.

## Companion version-line fixture

The caller audit found no additional production shell entry bypassing the fixed
ledger invocation. The companion version-line fixture also removed ALAB_PYTHON
from its environment without replacing it: managed-Python baseline ran nine
methods with 15 failures, falling back to unavailable /usr/bin/python3.
Decision: explicitly select sys.executable in this fixture, matching the ledger
suite while retaining isolation from other caller ALAB settings. All nine
methods passed in 24.423s: product/client version separation, client floor,
legacy source and snapshot behavior, malformed lines, pre-promote refusal of a
missing ledger, and manifest identity. No production code changed. Managed Git
uses private local repositories; whitespace checks passed. Logs are retained in
the private handoff directory as version-lines-baseline/qualified-2026-11-06.log.

## Transport and guardian companion verification

After the launcher and read-only ledger corrections, the unchanged GitHub
transport suite passed all 17 methods in 7.918s under managed Python and Git.
It covers exact raw tag identities, bounded batches, packed/shared refs,
concurrent tracking updates, immutable published tags, explicit repository and
capability selection, typed native failures and private batch-input cleanup.
The tests use local repositories and inert native-client fixtures.

The unchanged command-guardian suite passed all 21 methods in 1.312s, covering
real leader/descendant cleanup and cache locks, inherited group deadlines,
resource stops before and after spawn, exact exit status and descriptor policy.
The post-fork companion suite also passed. Decision: retain the current adapter
and guardian implementations; these checks found no defect requiring a change.
Full outputs are retained privately as github-qualification-2026-11-06.log,
command-guard-qualification-2026-11-06.log and
command-guard-postfork-qualification-2026-11-06.log. This is Python/shell
verification, not native Rust gate or installed-image adoption evidence.

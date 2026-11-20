<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Widest merged-tree check and authority correction

The maintained gate exposed source integration debt and two reproducible TUI
regressions. The regressions are repaired: placements retain the original image
file owner, and settings requests fit the unchanged bounded-stack test. Mechanical
cleanup removed 61 baseline warnings. The latest completed gate still fails at
Tippy with 961 warnings, including seven additional image reachability warnings;
application certification was not reached. The failed results remain visible.

## Executed checks

All Rust execution used source-selected Trust 15698, tree
`9e3ecca1051a2231f7a9ed4fce5f5904f77e5f43b0f9dd18da59204efe3040bb`,
through maintained native selection, target admission and the protected profile.
Tests used the explicit unverified lane. Test success confers no application
certificate or Native runtime authority. Results below belong to their exact
source; concurrent main changes do not retroactively change them.

| Actual source | Check | Actual outcome |
| --- | --- | --- |
| `081b4a0d72` | Canonical `alab/check.sh` | Exit 1 at Tippy: 1,015 warnings in 183 files; certification not reached |
| `081b4a0d72` | Complete maintained Python workflow | Exit 0; 2,229 unique cases started and finished, 2,218 successful callbacks, no failures/errors; required three native cases passed first |
| `081b4a0d72` | Separate syntax matrix and prune smoke | 237 AST, 13 Bash and three TOML parses; separate copied-process prune check passed 19 checks |
| `35760220c6` | Actual private Guard/Worker baseline | All 15 lifetime scenarios passed across three methods with original production writes |
| `ff77c7d953` | All 15 ALab packages, complete default scope | Exit 101; 43 completed suites, 2,974 passed, 18 failed, 17 ignored |
| `107067031c` | Earlier canonical gate | Exit 1; 954 warnings in 164 files, exactly 61 baseline findings removed and zero additions; no source errors |
| `107067031c` | Complete release and joint API packages | Release: 17 passed, two existing ignores; joint API: 426 passed across two libraries and six integration targets |
| `107067031c` | Complete app-server, client and TUI libraries | Exit 101; app-server 717 passed, client 103 passed; TUI aborted before its footer; collector 134 refuses incomplete attribution |
| `107067031c` | Focused CLI controls | Exit 0; three selected cases passed, 319 filtered; not complete CLI default scope |
| `384f1a7d2c` | Four incoming fleet Python modules | Exit 0; all 31 unique cases passed with stable source/tool bookends; no live fleet writes |
| `ebe0308e42` | Corrected-runtime image controls | All nine cases passed; test, collector, observer, source and outer statuses 0 |
| `0086fdb4e3` | Original exact Astra stack regression | Passed with unchanged four-MiB worker, eight-MiB server and one worker; all test/collector/observer/source/outer statuses 0 |
| `8d523847b9` | Complete merged app-server, client and TUI libraries | All three harnesses completed without abort; 7,694 passed, two existing upgrade parents failed, six ignored; test/outer 101, collector/source 0 |
| `8d523847b9` | Changed sync/status/ancestry Python scope | All 31 selected cases passed with stable source/tool bookends |
| `a902c5d750` | Complete ALab Core library and focused family controls | Core 472 passed; Core family/transfer and models-manager controls added six passes; all actual test/collector/source/outer statuses 0 |
| `a902c5d750` | Earlier post-repair canonical gate | Exit 1 at Tippy: 961 warnings in 164 files, no source errors; four separate locationless messages; qualified source null, certification not reached |
| `9ed7871a23` | Incoming direct-collector fixtures before correction | Exit 1; 40 cases finished, 25 passed and 15 failed; caught missing `sequential_output` attribute, no errors/skips |
| `eff853217f` | Fifteen complete affected Python modules | Exit 0; all 246 unique cases finished and passed, no failures/errors/skips; all 15 guarded children and collectors 0, exact stable input/tool bookends |
| `eff853217f` | Original exact Astra and details regressions | Both passed once; test, collector, coverage, source and guardian statuses 0; original bounds retained |
| `eff853217f` | Nine image controls | Test/collector/source 0 and completed footer nine passed; private coverage/outer 1 because only eight individual passing rows survived rendering; original refusal preserved |
| `eff853217f` | Complete isolated app-server-client library | All 103 cases passed with intact rows and footer, including direct request drop, roundtrip, JSON-RPC errors and error sources; all test/collector/coverage/source/guardian statuses 0 |
| `eff853217f` | Both Kimi profile controls | Both exact cases passed with intact rows; all test/collector/coverage/source/guardian statuses 0 |
| `eff853217f` | Complete runner-dependent Python module | All 34 methods passed; source/tool/Cargo/real runner bytes and maintained selector results matched before/after; guardian/collector 0 |
| `9702cade3b` | Seven complete affected Python modules after main alignment | All 86 methods passed with no failures/errors/skips; exact IDs and source/tool bookends matched; seven guardian/collector results 0 |
| `9702cade3b` | Final canonical gate | Exit 1 at owned Tippy findings: 961 located warnings/164 files, no source errors; six locationless messages separate; all qualification snapshots and extra source/tool hashes equal; qualified source null, certification not reached |
| `9702cade3b` | Original Astra after incoming Core layout changes | Exact original case passed once with four-MiB worker unchanged; all test/collector/coverage/source/guardian statuses 0 and full source/tool maps equal |
| `9702cade3b` | Focused incoming family, formatter and output-collection controls | All 11 exact cases passed across four independent library scopes; test/collector/coverage/source/guardian statuses 0, full source/tool maps equal |
| `9702cade3b` | Named shell-output integration | Actual exact case and assertions passed, one-pass footer; native prerequisites, Cargo receipt, observation, source and outer statuses 0; network early-return gate false and no skip message; LIB collector not applicable |
| `7bf599660e` | Direct final-merge checks | Six exact methods passed: three guard methods covering 15 modes, one parent covering three owned-PTY cases, and two changed WebSocket cases; all test/collector/coverage/source/guardian statuses 0 with equal source/tool maps |
| `9e88a52d3d` | Python-only publication-race alignment | Prune 13, daily 14 and retention 24: all 51 methods passed without skips/errors; exact IDs, guardian/collector statuses 0 and source/tool bookends equal |

Python's eleven skip events comprise ten whole methods and one filesystem
subtest: eight optional native experiments without opt-ins, two certified-gate
notes, and a non-UTF8 filename subtest. Skipped parents were not counted as
passes. Native and remaining phases both completed with exact coverage. The
separate syntax and prune checks are outside discovery. Protected context,
environment and descriptor evidence was retained as opaque bytes.

The failed 15-package run retained default library, integration, binary and
doctest scope with `--no-fail-fast`. Its 3,009 outer cases reconcile to suite
footers; twelve quoted child summaries were excluded. Cargo identified the
Autopilot library's 17 failures and the release library's one failure. The
release failure is repaired and rechecked; its original failure is preserved.

## Repairs and their limits

The source cleanup preserves ownership and cleanup order while simplifying
conditions, copies and method calls. Backend methods have the same `app`
visibility as their owners. Upstream changes are limited to marked image type
hooks and moving the existing pager reexport before its test module. No upstream
reformatting or new stack hook is carried.

Three private completion methods replace writes at the original lifecycle
points. Tests exercise actual private owners and the real Core/Incoming fixture
through unpolled loss, pending cancellation, early return, completion, and
cancellation during observer publication after completion. The original writes
passed too; no lifetime defect was reproduced. These are local owner controls,
not Native acceptance or successful Side-operation proofs.

The release test fixture inherited `0700` directories under umask `077`, while
the strict layout reader requires `0755`. Production already sets that mode.
The fixture now does likewise, retaining permission, replacement and symlink
controls. Its repaired test passed under the same restrictive umask. Two existing
kernel checks remain ignored because macOS refuses nested sandbox application.

The image repair aliases the SAME `Arc<OriginalFile>` held by the TUI book.
It creates no duplicate descriptor, file or unlink owner. Capture validates
physical owner identity before offering resources. Prepared placements reuse
the supplied owner; installation compares file identity as well as upstream's
raster/geometry equality. Registration and composer caches retain their no-file
contracts, and failed or partial writes retain custody. Five new controls cover
completed real drawing, retirement, substitution, non-file transport and restore
preparation. Their first run exposed absent Tokio setup; only the five test
attributes and async signatures changed. All nine controls subsequently passed.

The original details-image and Astra tests both passed on pristine upstream
`ce3031018ef130e653ed4d9c5bb971652de97161` with the same toolchain, real
prerequisites, three-package feature graph and original bounds. This established
two layer regressions. A private observer missed ANSI-prefixed success rows;
actual passing rows, complete footers and the maintained collector establish
the passes, while the original false observer refusals remain retained.

Several initial producer-boxing experiments failed the unchanged Astra test and
were reverted. Temporary layout tests, phase hooks and dispatcher/notice wrappers
were removed. Inert type sizes did not establish polling-stack bounds. Bounded
pre-repair disassembly found the event handler's polling frame at 3,933,760 bytes
versus pristine's 3,210,160; a known simultaneous subset left only 55,296 bytes
of the four-MiB worker for other frames. This was not a full dynamic high-water
measurement or attribution of every added byte.

The owned process's macOS crash report was matched by Mach-O UUID. Only ordinary
exception and crashed-thread symbolic frames were extracted; register, memory,
environment, descriptor and protocol fields were not extracted or displayed. The chain reaches settings
updates and `request_typed<ThreadSettingsUpdateResponse>`, then ALab's configured
request future. Its reported return site alone does not prove a faulting branch.

The repair lives entirely in `retained_configured_original.rs`: a private,
non-inlined factory boxes the complete original async request body at its
producer. Task-local selection, admission, channel checks, serialization, sending
and receipt mutation still occur only when polled. The same errors and
cancellation path remain; no task is spawned and no dynamic trait, `Send` or
lifetime restriction is added. It adds one heap allocation when the polled handle
path constructs this configured-request future; constructing an unpolled outer
`request_typed` future does not itself allocate this box. The original bounded Astra test passed. In the `0086fdb4e3` binary, measured
frames fell to 2,086,352 bytes for the event handler and 1,360 for typed requests,
reductions of 1,847,408 and 19,408 bytes respectively. This proves neither a
global stack bound nor application certification.

The complete three-library run on `8d523847b9` subsequently finished. App-server
passed 717 cases, client 103 and TUI 6,874; TUI's two upgrade parents failed and
six cases remained ignored. Astra, details, all nine image and all three actual
side-guard cases have intact passing rows. Terminal output obscured 101 other
passing TUI identity rows; their counts come from the complete outer footer,
not the collector's empty passing-copy list. Failed/ignored identities reconcile.

The newly merged collector requires argparse's explicit `sequential_output`
field. Three internal fixtures hand-build those arguments and omitted it; an
actual 40-case check produced 15 failed assertions over the caught attribute
error. Only those three Namespace constructors now supply `False`, matching
argparse's default. No assertion or production behavior changed. All 40 and
206 additional affected cases then passed on `eff853217f`. This is affected
module coverage, not a repeat of the older complete Python workflow.

Final affected Rust checks on `eff853217f` preserve the original graph and
execution bounds. Astra and details have intact exact passing rows. The image
harness completed nine passed, zero failed/ignored, but terminal output erased
the nested-clear case's individual row. Its private observer correctly refused
complete identity attribution, and its original coverage/outer exit 1 remains.
This is a footer-level nine-pass result, not nine individually observed passing
rows. The earlier complete host run retains intact rows for all nine.

## Findings that still require genuine enclosing integration

The latest gate's 961 findings comprise 932 dead-code, 15 unused-import, six
large-enum, four `expect`, three type-complexity and one argument-count warnings.
Six locationless messages are separate: two tool lane/propagation notes and four
duplicate-module notes. Two duplicate-module messages are new versus A902,
for the app-server and TUI execution-process test helpers; no primary source
location was retained for these messages. Compared with `107067031c`, all 954 existing diagnostic
identities are unchanged and seven image dead-code identities are added. The
final `9702cade3b` source Counter exactly matches A902, with no additions or
removals. Compared with `081b4a0d72`, 61 are removed and seven added; the net decrease is 54.

Source inspection independently confirmed there is no production caller of
`Book::capture`, `Data::prepare` or `Prepared::install_with`. The existing gate
already reported their enclosing retained APIs as dead. The new alias guards
are under those callers; the newly diagnosed `offer` and `keys` fields and
renderer-facing prepare method already existed before this repair. The plain gate
log has no Cargo target labels, so this normal/test reachability explanation is
source-based, not direct target attribution from diagnostics.

Removing `ImageFile.actual` would drop essential strong unlink custody; removing
alias checks would weaken capture/install protection. Fake reads, new public
visibility solely for lint, suppression, or manufactured renderer grants would
conceal required work. The decision is to preserve the real owner and guards and
defer the larger whole-TUI integration. Closure needs a genuine entered renderer
Binding, Offer retention through capture/handoff, original terminal/placement
installation before painting and the existing identity postcondition. The
[retained widget review](retained-native-widget-streams-2026-11-17.md) records
unfinished enclosing callers. No certificate or gate exception is claimed.

The 17 Autopilot positive fixtures similarly lack genuine published images,
independent admitted host identity and real terminal/input context. Existing
unpublished-source and unavailable-admission refusal controls passed. Copying
images into plausible paths, forging replies, extending timeouts or changing
ignores would not establish positive coverage.

The two TUI upgrade parents still fail. Error-only projection localized one
child to its zero-successful-preparations assertion and the other to absent
expected target-refusal text. Zero successes does not prove zero attempts.
Source inspection finds an earlier native source-exec/input-fence check that
their fake terminal cannot answer; if reached, it refuses before candidate
preparation. The independent published-running-image requirement remains too.
This does not individually trace every child's earlier stop. All 19 failures
remain failed and deferred pending genuine resources; no human hold is inferred.

## Alignment and authority provenance

Main through `d8dd5e00e2b8930ee444b83d6ea994ff5d7ea34e` removed the fleet
tools tested earlier and temporarily restored an explicit Sol 6.1 Ultra pin.
Two Kimi expectations were aligned with that policy, and all 472 Core cases
passed on `a902c5d750`. Later main `941c55f2762ff042613947a59e433ee2e29c2e67`
returned to an unpinned model while retaining Ultra. Commit `9ed7871a23` retires
only those two intermediate expectations; the Kimi file now exactly matches
that main. Production defaults are preserved. Older default policies and test
outcomes remain historical, not authorization to overwrite the latest main.

The canonical A902 gate ran the executable `alab/check.sh` without options through
the maintained qualification entry. Source/tree/upstream/clean state and extra
source/tool/plan hashes stayed unchanged. Its whole qualification snapshots
DIFFER: another actor advanced the shared `origin/main` reference from `fdbcce28`
to `941c55f2`. Its actual status is 1 and qualified source is null. Tippy stopped
before certification and later checks. Empty base-to-upstream ranges mean no
conditional intake execution is claimed. After closure, a fresh maintained Agh
fetch and ordinary merge preserved incoming main; no rebase or force was used.

Earlier [named-receipt](named-receipt-cleanup-2026-11-17.md),
[matrix](matrix-result-snapshot-2026-11-17.md) and
[Cruise comparison](cruise-main-comparison-and-pending-status-2026-11-17.md)
reports assigned several exclusions to human holds without a person-message
source. The primary audit checks 65 user-role records: 57 harness prompts, five
instruction/environment records and three work/recovery messages. Two are task
requests; the API-recovery message's authorship is not attributed to a person.
Conditional instructions to respect actual holds do not create category-wide
holds. Native admission and producer/proof contracts remain binding. A distinct
saved automatic review rejection has an unavailable original delegated payload;
its unknown scope establishes neither a blanket ban nor clearance of that action.

Final alignment fetched main `0630ece4f1b091f707241cafba385e77c93f280b`
through maintained Agh and merged it normally into `9702cade3b`. The incoming
39 paths preserve the latest defaults and our request/image repair blobs, while
adding family policy, shell-output capture, sync leasing and runtime retention.
All 86 cases in the seven affected Python modules passed. This includes the
resolver-containment fixture deferred for resource status 75 in the incoming
[release-helper review](release-helper-integration-resource-stop-2026-11-17.md).
Its old failed result remains; the fixture executed successfully here with
available capacity. Controlled fixture coverage is not native compiler proof.

The final canonical gate ran once on this merged source through the same
maintained qualification entry and executable check without options. Its entire
before/after record is equal: source, tree, base `0630ece4`, upstream `ce303101`,
and clean state. Extra source/tool/plan maps also match. Actual gate and guardian
statuses are 1, qualified source is null, and Tippy stopped execution before
certification or later checks. There is no conditional intake execution claim
for the empty base-to-upstream range. This newer equal observation does not
rewrite the earlier A902 base-reference change.

## Bound local evidence

The final gate audit is
`2718dc3a4bf3800c3d72b7cfbbe9fe552b6baff81e40395b6cb295a02d283910`;
its final closed summary is
`02089d9b8c7d8f34a5edbaad1dc319b9761d0f585513c1eb7af13e7d5808bb54`;
the earlier A902 audit is
`7016ea6be86c6b39b0bd190f5fba0f5f7feeb4c24ff3313cd45c2e2722a26bfd`;
its independent closed summary is
`a2b15e55f891c0cdaa4ca74e00cdd4078930a7fd58b4a19aeb5ed5de4499b342`.
The repaired Astra report is
`889c1416488b1d689f0668ae54cf723486334d011d5c6f8e787156870d150b7e`;
complete merged host-library report
`0a20b418389dffc371856525f803d36e58d64296a5f358826ab530d984161bfe`;
and outside-host controls report
`3dadf074786aecdfe89a1d403f75a21acc538604ace5a1b0df00d355d06a0ec3`.
The primary authority audit is
`08bc804ff4966c52a76c1f75544748863773ae82d6a3a8c00ba04a74d6f883c1`.
The independently checked complete Python packet contains 24,781 payload files,
index `df13ee73316ccf92449da41c38b4f64e74a29693ea25b85bc8826aca1daee9c5`.
Local evidence records execution and retention; it confers no artifact promotion
or live entitlement.

The final Eff Rust evidence is `codex-widest-final-check-eff85321-results.md`,
SHA `fb32e0bede8656dd8304c3a87843a8ed3be02295fd9bdecf1a4c8a718b1d32c2`:
116 completed-footer passes, 115 intact individual rows, and the one original
image observer refusal preserved. The closed runner-dependent Python index is
`f1077aba58f43dbfb7854390de3164366620446377e950177b0beaa841a8e630`;
the final 86-case affected Python packet index is
`23ffcbc65852b87a972ea2050f77103a1a3c01a5769a3abab6fab4350face58e`.
These packets are retained locally under `.ubd`, with source bindings and actual
process ledgers; none is application certification.

The realigned original Astra summary is `codex-widest-realigned-astra-9702cade-closed-summary.json`,
SHA `b2126e56cef53edaebce1669a046bec82016d5e53df1941084901eb7bbb42dde`.
The changed Core graph justifies this one additional unchanged regression check;
image and client checks were not broadly replayed for unrelated incoming code.

The finite final Rust scope on the merged source completed 13 passed, zero
failed or ignored: the unchanged bounded Astra case, 11 focused library controls,
and one named shell-output integration. The library controls cover explicit pins,
five catalog-family cases, three formatter cases and two bounded-drain cases.
The integration exercised actual owned local SSE, PTY, temporary-file and V8
resources through both tool surfaces. Its actual source-defined network
early-return gate was false, its existing skip message absent, and its exact
ordinary row and one-pass footer intact. This establishes assertion execution
rather than an early-return harness success. No test or production source was
changed to produce these results.

The named integration used maintained native prerequisites and a separately
reviewed named Cargo target, with an actual Cargo guardian receipt. The public
LIB collector was explicitly inapplicable. Every original was launched once,
reaped before the next, and kept separate actual statuses and stable source/tool
bookends. The earlier Eff image observer refusal remains unchanged. No full
client, image, host or blanket Core suite was replayed for unrelated incoming
code. These final controls confer neither application certification nor
successful Native upgrade coverage.

Final named-integration summary:
`codex-widest-incoming-shell-integration-9702cade-closed-summary.json`,
SHA `52fbf266474e59e48bcb1ffa3f15265eb1cadf35c8217b357ee31f39ab61eebc`.

The complete postgate aggregate is `codex-widest-postgate-9702cade-results.md`,
SHA `4dd41f6409f2b14404c853c44b754cbb2e36794d5ca0b8f82fb3867695373eca`.

A publication-time fetch found incoming main `1326a21034df59d6294ce5bea28f6fe888902b3e`,
including explicit completion-owner drops in the same three files. Normal
merge `7bf599660e` preserves both changes: the existing completion methods
borrow their whole owner mutably, then the incoming explicit drop retires it
at the original normal end. The injection worker drops only after its observer
publication loop, so tracking custody spans that await. Early return and
unfinished cancellation retain the existing Unknown path. No method signature,
visibility, ownership field, test body or Native policy changed in resolution.
Independent source review approved the three exact conflict hunks.

Incoming terminal raw-mode restoration, WebSocket test polling and intake
decisions were preserved. Two incoming mechanical cleanups already matched
this branch. The last canonical gate remains the explicitly recorded failed
`9702cade3b` run; it is not relabeled as a gate on this later merge.
Only directly affected checks are selected after this alignment.

All four direct-merge check invocations completed successfully on `7bf599660e`.
The same three guard methods passed all 15 unchanged lifetime scenarios; the
terminal-handoff parent passed ordinary, held and failed owned-PTY cases,
including raw/cooked termios restoration. Both changed standalone WebSocket
cases passed. These are six exact harness methods, with zero failed or ignored
methods and all test, collector, coverage, source and guardian statuses zero.
Their full maps match before and after across 12,396 Git-bound source entries.
No broader suite or canonical gate is claimed for this final alignment.

The final WebSocket timeout summary is
`codex-widest-final-merge-delta-websocket-timeout-7bf59966-closed-summary.json`,
SHA `ff6ac3e03c5683bf0687730cf54b1a11b4433cc4f10fe9b1f8edafee8e984834`.

The final merge-delta aggregate is `codex-widest-final-merge-delta-7bf59966-results.md`,
SHA `7066b10ae024cc0502ef3fbef0b4494d87994c5344f496e3744323c697d78179`.

The first publication attempt safely refused before writing because main
advanced. A fresh maintained fetch and normal merge preserved main `a27fb81de5`
in `9e88a52d3d`. This ten-path delta changes Python prune selection custody,
test cleanup oracles, reviews and intake records; Rust, lockfiles and build
helpers are unchanged. Three complete affected modules passed all 51 methods:
prune 13, daily 14 and retention 24, with no skips, failures or errors. Every
guardian and collector status was zero, exact method identities and raw output
matched their records, and source/tool bookends remained equal.

These tests use owned temporary registries, packages, repositories and scratch,
recording cleanup commands and fake Cargo metadata refusal before compilation.
Only owned shell/filesystem controls use actual macOS containment. No real
cache prune, application action, compiler proof or native qualification is
claimed. The previous Rust and canonical-gate results retain their exact source
scopes; the Python-only delta does not justify broad Rust replays.

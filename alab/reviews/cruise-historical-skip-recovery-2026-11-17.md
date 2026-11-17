<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Explicitly recovered historical skips in Cruise

Decision: finish the previously deferred historical-skip candidate using the
maintained native test route. On the old reader, a report such as
`The previously skipped probe now runs and passes.` still named `skipped` as
open work. The report describes a recovered check, so its next Cruise prompt
should carry the ordinary continuation rather than a stale skip warning.
Current unfinished work, nonzero generic skip tallies and human holds remain
unchanged.

The correction adds one private predicate and one Cruise-only call site. It
requires the complete subject `the` plus an existing historical marker, an
immediate check/test/probe subject with number agreement, and an exact current
passing predicate. Conditions, questions, partial subjects, unrelated success,
exceptions and unknown tails within that sentence retain the conservative open
reading. Land, heads, NEEDS YOU handling and the continuation scheduler are unchanged. The
module header identifies the local corrections carried beside the port.

All native runs used source-selected Trust build 15698, compiler
`20565c3255adf151f9eb920e89cdf83c38f84abe`, through the qualified project controller
and existing protected profile. The global PATH toolchain was not substituted.
`unit_tests` prepared the actual pinned V8 pair, Code Mode host and runtime
adapter, retained the full selected feature graph, and ran the whole ordinary
`open_work_tests` module with its native adapter and collector. Default capture,
four build jobs, four test threads and the 8 MiB stack were preserved. The broad
`open_work` spelling includes an intentionally ignored Land measurement; that
selector issue was caught before execution and the ordinary module was selected.
No ignored case was enabled or filtered after a failing run.

| Phase and committed source | Passed | Failed | Ignored | Actual command status |
| --- | ---: | ---: | ---: | ---: |
| Baseline `5a6c49b39db8093349bf6ce1580063ce0114e201` | 13 | 0 | 0 | 0 |
| Test-only RED `c9260aa8359158b5e3f18e980f1d8cfe57a340b7` | 14 | 2 | 0 | 101 |
| Fixed GREEN `25517bad7751931e62d0e18670da13790a15a8d5` | 16 | 0 | 0 | 0 |

Original executions are 22052/ffb9f0, 50920/5378e3 and 88442/5373a9 respectively.
Each typed guardian retains its actual status. Helper authentication, collection,
source checks and the independent observation all return zero in every phase;
RED's two failures are native test failures, not missing prerequisites.
The collector's empty record-mode `passed` list is never used as a passing count:
exact raw ASCII case rows, round/library markers and footers establish the counts.

The three new test functions define 92 table scenarios: 72 historical
marker/subject/predicate forms, 17 incomplete or conditional controls and three
independent-work cases. All 72 positive forms call both actual public readers
before the aggregate assertions. RED's controlled panic shows all 72 Cruise
results as only `skipped`; the Land assertion passes. RED's first mixed case
returns `[skipped, not verified]` instead of `[not verified]`, and the other two
mixed rows are not reached in that failing function. GREEN passes all 16 native
functions and therefore all 92 new scenario rows. The same test bytes are frozen
across RED/GREEN, SHA-256
`6c980b2e2dbb37b6aeeee0eb362c6dd99ee35b905626360029ce4435031b07b8`.
The malformed brace boundary in an earlier private draft was caught by the
pinned formatter and corrected before any native RED execution.

After GREEN, `84ed2aeed2185d18da2d1cb65f4e04f0545235d5` adds only two module
provenance comment lines; the executable source body and tests are unchanged.
On that clean source, original 88301/bfdd3f returns zero for focused
`codex-alab-autopilot` all-target Tippy in the separate `target/tippy` cache.
Actual Cargo records include the normal library, test-profile library and 12
integration targets. The maintained classifier finds no owned, spanning,
failing-target or missing-script result; three upstream diagnostics are set
aside. Source/tool/script and independently verified V8 input bookends agree.

The previously unexecuted native-adapter lint-fixture scope was also completed.
Original 78776/aee0eb returns zero on 84ed: all 57 methods of
`test_check_lint_step.py` finish once and pass, without skips or errors. The
maintained fixture module uses its fake Cargo cases with the real adapter freshly
selected from GREEN's genuine metadata/artifacts. The selected runner's bytes,
source inputs and both recorded bookends agree; no stale fixed binary hash,
fake adapter or replacement test runner is used. Fixtures retain their own
protected children. This is adapter-fixture coverage, separate from actual
Tippy, native parser tests and a complete Python workflow.

Durable private seals under the common Git directory's `alab-validation` retain
raw outcomes, actual owner process evidence, typed receipts, source snapshots,
controllers and opaque native receipts. Each native phase binds 11,585 Git inputs.
Compiled helpers, test binaries and whole build targets are excluded:

- Baseline `cruise-open-work-baseline-private-os1c6edk`, index
  `fc39b229871e3e387d60ccf26a05534f49c0cb71c20308c176f4463ebbcc0a51`.
- RED `cruise-open-work-red-private-jjdjqcek`, index
  `ce1e12af84141844164cb7fa8567c4043c751745f5295a368979eb94616edadb`.
- GREEN `cruise-open-work-green-private-kpm2pwus`, index
  `c3a98730742333db80d6d12e04b6a428bafed898505aba50121ebc2e4cf87045`.
- Focused Tippy `cruise-focused-tippy-private-_qjv2aby`, index
  `ca884ac41a0b69250d7816dbc9061e4d438f2e8325a6dea6f1d233f1b69bb7c7`.
- Native-adapter fixture `cruise-native-lint-fixture-private-y2fgrq76`, index
  `c6a110f3e3a58cbe3724962f2e5b17fb3ff1a9e9afba62c464b3d904b65a87f9`.

Independent source review approved the narrow grammar and unchanged Land path.
The native and Tippy seals were independently checked against actual source,
outcomes and tool identities. These are focused test/lint results, not a full
application certificate, package, installed activation or compiler adoption.
Existing application-certification, adoption and external-producer holds stand.
Publication uses a fresh literal main lease through the maintained Agh publisher;
only session-owned branches and worktrees are changed.

After the earlier native parser, Tippy and 57-method adapter-fixture checks,
main `9ee12e330bb9a41a1355aa8772b9808e4342b804` was normally merged into
`8f8f12d1e7f346e69ab4a1d1cfdccc137adec868`. A separate source-qualified selected
Python check covers 12 complete maintained control-plane fixture modules, with
243 distinct declared methods, in sequential fresh managed-Python children.
The existing genuine GREEN runner is freshly authenticated for lint fixtures;
fixture Cargo/compiler/front controls remain inert. The helper Rust, Cargo,
lock and source Trust inputs are unchanged. This check is separate from actual
Rust compilation, application certification and a complete Python workflow.

Original 85347/02b6c6 returns one, as does its typed guardian. All 243 methods
finish: 242 pass and one fails through five failed subtests, without skips.
Eleven complete modules account for 238 passing methods. The verdict-protocol
module has four passing methods and one failed method: its public-entry cases
for statuses 82, 83, 75, 130 and 37 fail while checking inherited fixed
descriptors, before verdict interpretation. The original failed run is retained
in `cruise-merged-fixtures-failed-private-4_ukhmp5` (index `682ecb666fc68af9a4151e609dcb82c2717756802c18a940169af837276d1ae5`).
It is not relabeled as a successful 243-method run.

A separate guarded owned-descriptor observation confirms the fixture's
hardcoded `/usr/bin/python3` enters Apple's Python 3.9.6 and raises EBADF at
`fstat(18)`; the same shell-owned fresh unlinked inode and descriptors 18/19
survive under managed Python 3.14.8. Actual child statuses are one and zero;
the observation driver and guardian return zero, with source/tool/driver
bookends unchanged. This establishes the launcher/interpreter boundary without
claiming which launcher internal operation closes the descriptor. Its private
18-payload seal `codex-verdict-fd-probe-zldfw645` has index
`2c7408236be92d2b725ca4e00c73dbba62ac1d127176c8ffd5928e1e0770a3cb`.

Commit `91620313b7346dd410443483813476a9acf72a50` changes only the five-method
verdict fixture to use its current interpreter. Its generated short-read
wrapper quotes interpreter and payload paths, preserves the original arguments,
and requires a valid exact record to map to two before requiring a forged suffix
to fail. Both three- and 14-byte read sizes are covered. This also removes the
old negative-only check's ability to pass vacuously on EBADF. No production
transport or held producer protocol changes.

The focused full five-method module on 916 returns zero, original
51970/de0ce4, with typed guardian, child, fixture and source statuses all zero.
All five methods finish and pass without skips or errors. Its source/tool/script
bookends agree, SHA-256
`2ce2acab3bcaa4c40a95aa6b171471e4ee4ef710a45764d25515baeea6294cd5`.
The durable `cruise-verdict-green-private-0h1gebxb` seal has index
`f081d5557fc8a817b6ab1e48bedf2dfcfc84dfc74d5b748022b3c841f48af7f1`.
This focused check needs no native adapter or compiler. The earlier failed
243-method scope and this later five-method result remain separate; the other
11 passing modules are unchanged by the fixture-only repair. No further native
build is warranted by this merge: the executable parser body, test bytes and
Rust dependency graph remain identical to their checked sources. All human
holds and earlier native/lint seals are unchanged.

A later normal merge brings main
`a4c6d583e9f0cd68e2168e2c364ee45e03e959dc` into
`e10a8595c2e185c9a83ad4d0d3e6e4166303772c`. The parser and its 16 ordinary
module methods are unchanged, as are the native helper implementations and
source Trust 15698 pin. The incoming Core, Autopilot and linked Rust changes,
including Cargo.lock, nevertheless require fresh selected native and lint
validation. This merge supplies no application certificate, adoption or live
update qualification.

On exact e10, original 85518/cb3df9 returns zero: all 16 selected native methods
pass, with no failures or ignored tests and 1,127 filtered out. The actual unit,
tee, collector, host authentication, runner authentication, observation and
source statuses are all zero, with typed guardian zero. Source/tool/script
bookends have SHA-256
`7ec7a313ce1439468b594d0262e8c25db89ca1ef5a66772293cf93e8d698a0a5`.
The independently audited `cruise-aligned-native-private-m3kezrm5` seal has index
`4ce7c03eeed6e17301ac30b2ddb1c83d6619119f9ccdda6269a2126a3d25c233`:
12,272 payloads, 149,716,223 bytes and 12,235 Git bindings. Native receipt bodies
remain opaque. The first copy invocation used a mistyped capsule checksum and
refused before creating an archive leaf; its raw failure is retained separately.
Correcting only the supplied checksum allowed the completed evidence to seal.
This is selected-module evidence, not a full-library result. The separate
feature inventory 67426/48af05 accounts for 104 hook topics across 14 features.

The first aligned all-target Tippy, original 92714/4a2f7e, returns one, with typed
guardian one. Actual Cargo/Tippy returns zero and all 14 fresh metadata-declared
target/profile variants are present. The maintained classifier reports exactly
three owned findings, zero spanning/build/script findings and three upstream
diagnostics set aside. The owned findings are two unused methods and one unused
import from the incoming source. The failed
`cruise-aligned-tippy-private-cbfhn_fb` seal retains index
`f5361a9398d2831893e0b095f8671cd7f06c64158e8161e493e9ed4fa39798f7`.
Cargo success does not turn those owned findings into a passing lint result.

Commit `e54d2b81ed126f3ae203b830438f9a274290bdd1` deletes exactly 30 unused
lines: the uncalled restricted EditorCheckpoint constructor, an uncalled Harness
idle-lifecycle helper, and the unused Path import. Whole-tree uses and included
configuration-specific tests were reviewed independently. Serde validation,
checkpoint accessors, the separately used retry_idle_goal helper and all actual
resource/producer guards remain unchanged. There is no lint suppression or
protected-producer implementation.

The final selected all-target Tippy on exact e54, original 88210/72ad19,
returns zero with typed guardian and all seven component statuses zero. All 14
fresh metadata-derived target/profile variants occur exactly once, with no
coverage errors. The classifier finds zero owned, spanning, build or script
issues; three upstream diagnostics remain set aside. Source/tool/script
bookends agree, SHA-256
`8a3c3c8dfcb1be072ca683e2888a1130b040bffdcbdf305685efec579c0f1989`,
and independently verified V8 inputs agree before and after. The
`cruise-clean-tippy-private-qacjfdcp` seal has index
`5da288dd609acb7916e28bfbd469747f86a6217e412aec58c87c44e116ea33a8`.
The original failed lint result remains preserved separately.

The incoming batch changes no ALab Python or shell implementation/test files;
prior selected Python fixture results remain
bound to their recorded sources. No second Python run or native runtime rerun
is used to imply a broader current-source pass. The deletion-only cleanup is
appropriately checked by fresh all-target compilation/lint, while the parser
runtime result stays explicitly bound to e10. All existing human holds remain.

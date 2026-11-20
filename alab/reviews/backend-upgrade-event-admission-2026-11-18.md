<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve original backend event queues before conditional upgrade

Production source `196608d01c1dad83be8f4b3fd23efae1286a8da1`, based on main
`a5613d4651fa08ea1bfecf2a53ebf44972774084`, adds a non-consuming event-backlog
precondition to conditional leave. Nineteen selected methods passed there. Its
canonical gate failed at Tippy with 942 warnings. The following test-only repair
passed its two scripted fixture controls; its post gate still failed with 940
warnings. All results below are closed and retain their exact source scope.

Current job joins do not cover events already queued in physical owners.
The actor checks those buffers before Book observation and again after physical
currentness, beside its final original App/persistent-inbox checks. It never
receives, requeues, filters, clears or serializes original events to observe them.
Ordinary events retain their route. Refusal resolves the SAME conditional request
without dispatching or rebasing its Native scan; the original pump remains live.

[Client](../../codex-rs/alab-core/glue/upgrade_client_events.rs) checks InProcess's
event receiver, Remote's startup deque plus event receiver, and Cloud's event
receiver plus actual queued-loss map. The worker records losses BEFORE send and
marks supersession; only the ordinary reader filters/retires those deliveries.
Busy/poisoned or inconsistent accounting is unavailable and refuses the attempt.
[Session](../../codex-rs/alab-tui/glue/session_upgrade_events.rs) requires its
physical owner and adds its Unix configured FIFO. [BackendSessions](../../codex-rs/alab-tui/glue/backend_upgrade_events.rs)
checks every entry, including hidden/paused/non-listening owners, parked requests
and deferred side-startup events. The prior Book guard and old eleven controls
remain byte unchanged.

We rejected the earlier liveness veto. A closed receiver may hold unread messages;
after ordinary consumption, closed-empty is known empty. A closed command endpoint
or finished worker says nothing about backlog. The query does not inspect either
state, preserves EOF behavior, and never turns emptiness into transport readiness.
The committed diff is 16 paths/706 additions/two deletions: eleven owned files and
five three-line marked upstream mounts; only owned files were formatted. Original
consumers/producers, filtering, reconnect and shutdown bodies are unchanged.

| Actual check and source | Outcome |
| --- | --- |
| Client4 at 196608, core/app-server/client; two authenticated zero siblings | 4 passes; original/guardian and all four component statuses0 |
| TUI14 at 196608, core/app-server/TUI; two authenticated zero siblings | 14 passes; original22803/5d63c8→af2d40 and guardian/component statuses0 |
| Existing local Cloud loss producer at 196608, same client graph | 1 pass; original19562→de7ca7 and guardian/component statuses0 |
| Canonical196608, original65363/67ef57→bf4f46 | Gate1 at full Tippy: 942 source warnings = 937 dead_code + 3 unused_imports + 2 await_holding_invalid_type; zero source errors; tool messages separate; qualified_source null |
| Two existing scripted settlement controls at bb302a9 | 2 passes in one authenticated codex-alab-aterm library; original68209/aa9e30→6c9015 and guardian/component statuses0 |
| Post-repair canonical bb302a9, original43177/cccd41→22a410 | Gate/guardian/qualification1 at full Tippy: 940 source warnings = 937 dead_code + 3 unused_imports, zero source errors; tool messages separate; qualified_source null |

The first three scopes contain 19 DISTINCT selected IDs: seven new controls and
twelve unchanged existing methods, not whole-library coverage. TUI books match
`e6c9b3a2da5b44b179539e2de2fd6e80a482d0ca9fea4e9307b696c5db7f165e` over 12,564
inputs/11 tools/32 selected bindings; exact rows, Source collector and IDs agree.
TUI raw: `7018586488707af17f46b9589e50ee3cffd2422b1928ebe9fe52be2ad7f9fbd5`.
The producer method executes actual local Replies.replace/forward_next→report_loss
with the SAME Cloud loss map and original reader. It does not call the new query,
pause between record/send, run HTTP/remote Cloud, or grant Native authority.

The lower facade fixtures use real owned channels and a lifetime-only worker,
not embedded/WebSocket execution. The new ledger control directly records map
entries and manually queues a typed loss; separate producer coverage above is
not attributed to it. Busy/poison behavior is measured; malformed-counter refusal
is source-reviewed only. The configured-FIFO control manually retains a physically
received event, without executing the full configured-tail protocol or its receipt.
Outer controls preserve parked request/side-startup routing and known-empty EOF.

Canonical196608 kept source/tree `2b97e27b…`/actual base `8b5869ac…`/upstream
ce303/clean=true ALL unchanged, but failed before certification, application
runtime or full-test stages. Its log SHA-256 is
`9dfdf4ff5bcb4288e5bab561cc1b7b76202fc2acea321ec30949fab530b0eaa8`.
The two new warnings identify existing settlement TEST guards, not query production.
Repair `bb302a9225df51ac26c653d41083b82a306ca341` changes only their private
async Tokio unit mutex to Semaphore::const_new(1), with permits acquired at the
SAME two positions through the SAME lexical scopes. Production bytes are identical;
reversing those three lines recovers the entire parent file. No test bodies,
assertions, protocol, deadlines or APIs changed. Its real module prefix is
`source_member::runtime_upgrade::tests`, not the source filename. The selected
FakeAterm transport tests do not provide genuine Native admission or cancellation proof.

The scripted settlement run's before/after books match
`fbea5ff068102f3db6225a9c4df0b8aec8e42b9a5fbfa3f081c56f0c3b1a3187` over 12,564
inputs/11 tools; raw `f6eebcc24b251423eaac18f58582476d588277edd707619eba6bff69f135d717`.
Across the two SOURCE-SPECIFIC sets, 21 distinct selected IDs passed: nineteen
at 196608 plus two existing scripted fixtures at bb302a9 (seven new/fourteen
existing overall). No old result is relabeled bb302a9; all 21 were not run there.

The post gate kept its ENTIRE qualification snapshot equal: bb302a9/tree
`f6af1926…`, actual base `ccf2fd26…`, upstream ce303 and clean=true. It nevertheless
failed before application certification, application runtime and full-test stages.
Post log: `13b932effad2c3d2cdd05d36f3f7aa62362495de2035a4d902800410be6ff012`.
Complete static audit `cf52cc51…` and multiplicity-preserving path/family/message
Counter remove ONLY the two test-mutex diagnostics, add zero (`68fa96ce…`), and
match the earlier 59111 warning multiset exactly (`30f2293d…`). The remaining
940 warnings are unresolved; no counts were hidden or promoted into a gate pass.

Earlier [job-admission results](backend-upgrade-job-admission-2026-11-18.md) remain
source-specific. Stored-buffer observations are not a global producer fence,
atomic H1, future migration or whole-scene installation. Scripted transport/receipt
fixtures do not grant real SDK authority. Finite unit success does not imply a
complete gate, source qualification or certificate.

The normal merge of main `771ecc3479a9b1c5784673db34237317bce0b762`
preserves its saved-history refusal/retry before our queue checks and returns
the same original upgrade request. Main changed actor admission to `&mut self`
and retained the actual `Arc<Job>` ownership contract. Adaptation
`3c8c034607fe287e204c478416c88bc1218361af` changes only two new test bindings
to mutable actors. Independent immutable-source review found no integration
blocker; it did not supply a runtime result.

One maintained native invocation on exact 3c8c0346 passed 15 distinct TUI
methods: the prior fourteen plus the existing detached saved-opening/history
control. Core and app-server are authenticated zero-test siblings. No method
failed or was ignored; original14252/83697→7bd9a9, guardian, test, collector,
coverage and source statuses are all zero. Entire before/after source, tool,
script and plan records match over 12,572 inputs and eleven tools, SHA-256
`da4747b5ef073ea155cc65085a57702649db45503b59e306dec77bcbe00c70d4`.
Raw unit log: `1800e0bf4babb146a1bf561776de1dea8e1406d3ba7ae4eab755295fdc50c475`.
Independent closed-evidence audit `fa90ba20…` verifies the exact method rows,
module mounts, collector joins and source bindings. Earlier client, producer
and scripted-settlement results retain their original source scope.

The canonical gate on exact 3c8c0346 also closed, original82330/11007→c9a91a,
with gate and guardian status1 at owned Tippy policy. Source, tree `dfd080bf…`,
upstream ce303 and clean=true remain fixed. The entire qualification identity
does not match: actual origin base advances from `49a3969e…` to `3ee6338a…`.
Qualification remains null, before application certification, runtime checks
and subsequent full test suites. Closed gate log SHA-256:
`a9c1087fd8ae7834c1760e8fa145ec16f2022de1912764572d50f711236053a1`.

The closed log contains 940 located source warnings: 937 dead-code and three
unused imports, with zero source errors. Multiplicity-preserving path/family/
message and severity Counters against bb302a9 add and remove zero findings.
Earlier wording called two tool warnings “notes”; that severity label was
incorrect. Both bb302a9 and this merged log contain 942 warning headings
(940 source plus two locationless tool warnings), zero error headings, and
eight separate compiler note annotations. No tool message or source finding
is discarded by this clarification.

Decision: land these tested source repairs with the failed-gate record, then
continue integration. Source landing does not certify, package or install the
application. The 940 unfinished integration findings remain open. Fresh main
changes require review of their actual contracts and justified regression
checks; no earlier result qualifies a later merge.

The next authoritative read confirmed main `3ee6338aef489efc72a500d7ba3c949d3016d01f`.
Normal merge `7f6446299ccc31cfa0e7c247156a6e386d5a3724` preserves all seventeen
topic source paths byte-for-byte from tested 3c8c0346. Incoming main changes the
saved-opening scheduler and its ordinary startup poll, so we reviewed those
contracts and ran the prior fifteen controls plus all five existing deadline
tests. Timed reads remain on the original Session Book; history refusal/retry
still precedes the queue and Book admission checks.

That exact 7f644629 invocation passes twenty distinct TUI methods with zero
failures or ignored tests and two authenticated zero-test sibling libraries.
Original18615/63495→078606, guardian, test, collector, coverage and source
statuses are zero. Full before/after records match over 12,575 inputs and eleven
tools, SHA-256 `a02a39a85e3a6b0e193883de2342a77e0df11eb46ae83313271856efff43ffa3`.
Raw unit log: `c8d6d2751140bc510121bdb65c9ce69ba973d422cf08db3e23b584643fb3884f`.
The twenty include the updated detached-history control and bounded retry,
original-reader, capacity, failure/terminal-disarm and immediate-rearm cases.

Decision: check the changed history contract with these actual regressions;
do not repeat the failed full gate merely to follow unrelated concurrent main
edits. The canonical result above remains tied to 3c8c0346 and does not qualify
7f644629. Its complete closed audit (`24a89130…`) retains all 940 source findings,
both tool warnings, every note annotation and the observed base drift. Source
landing retains that unresolved integration work and supplies no application
certificate, Native grant, package promotion or installation.

Follow-up `f747409af815d17c7d2506638a73f2a9203dc398` closes the earlier
source-review-only malformed-accounting test gap. It appends 104 lines to the
owned Cloud test file; the entire existing 144-line/4,791-byte prefix and all
other tracked production bytes and modes remain unchanged from landed 42dbaa81.
Only that owned file was formatted through source-selected Trust 15698.

The fixture deliberately corrupts private accounting to `(0, 0)` and `(1, 2)`.
Each case observes `InvalidData` twice through the public facade, preserving the
entire original map, including another thread. It then explicitly repairs only
the fixture counters before ordinary delivery and checks the same boxed errors
arrive in FIFO order, accounting retires and the closed stream reaches EOF.
This measures refusal and original ownership; the deliberate corruption and
fixture repair establish neither normal producer reachability nor a runtime
repair policy. Independent committed-source review `65f5d956…` confirms the
guards retire before awaits and production visibility and behavior are unchanged.

One maintained invocation on exact f747409a passes six distinct client methods:
the new case, the four original queue controls and the existing local loss
producer control. Core and app-server are authenticated zero-test siblings.
No method fails or is ignored. Original11824/6542→536243, guardian, test,
collector, coverage and source statuses are zero. Entire before/after records
match over 12,575 inputs and eleven tools, SHA-256
`ea2a07f96a1dbe79f3257eb84bb2efaf542f5db79ae6c2dbaff057b5126905f7`.
Raw unit log: `aad9320997298bdda20eff2315c3f9d99482db01782d72055154dca61de9de40`.

Decision: land the test-only closure after these actual regressions. Unchanged
production code supplies no reason to repeat the known failed full gate here.
That earlier gate remains source-specific with 940 unresolved source findings;
this finite library result does not qualify f747409a or an application build.

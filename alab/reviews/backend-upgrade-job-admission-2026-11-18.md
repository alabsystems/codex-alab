<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Conditional upgrade admission joins every physical backend's jobs

Source `ff51a8c06a32eda359f3055374b668b89b7ec7ee` adds a real production
precondition: an upgrade request is refused while an original physical backend's
model/work join is busy, invalid or stale. Its selected 11-method invocation
passed. The canonical widest gate failed at Tippy with 940 warnings and no errors;
it produced neither a source qualification nor an application certificate.

Previously, empty App/actor inboxes could coexist with an original worker awaiting
its RPC. Waiting inside admission while pumping ordinary completions is not a
valid way to preserve that request: [event_dispatch.rs](../../codex-rs/tui/src/app/event_dispatch.rs)
calls `upgrade::native_event()` for ordinary events, invalidating the original
Native scan. This explicitly conditional attempt instead refuses promptly, keeps
the original pump/jobs alive, and requires a fresh request after they settle.
It never rebases the existing scan or migrates an arbitrary future.

[backend_upgrade_work.rs](../../codex-rs/alab-tui/glue/backend_upgrade_work.rs)
captures BOTH Books for every physical backend, including inactive/non-displayed
entries, and polls the owned aggregate once. Cancelling a pending observation
restores the original join leases without cancelling or resubmitting the workers.
A Ready guard retains exact ordered BackendIds, displayed owner and both physical
Book witnesses; replacement, request views and changed generations are rejected.
A second generation pass follows model DATA snapshot cloning.

[lifecycle_actor_events.rs](../../codex-rs/alab-tui/glue/lifecycle_actor_events.rs)
keeps the old actor checks FIRST, obtains/revalidates the actual job witnesses,
then checks BOTH original inboxes LAST without dequeuing anything. Non-upgrade
events keep their existing route. Refusal resolves the SAME UpgradeRequest false;
admission returns it unchanged for the existing downstream Native validation.
No await, spawn or new pending-request state was added to admission.

The six-file source diff is `602` additions / `7` deletions. Four files are owned
ALab behavior/fixtures; upstream receives a three-line backend module mount and
one extra argument in its existing startup hook (`+1/-1`), without reformatting.
The old actor tests only rename their original helper, plus owned formatting.
Maintained delta: startup stays risk `25500`, churn `125`, `+138/-66`;
backend_sessions moves risk `44→77`, churn `11`, `+4/-0→+7/-0`.

| Actual check on ff51 | Result |
| --- | --- |
| Original unit process `59974` (`17487d→8952eb`) | Original/guardian 0; exact 11 TUI passes, no failures/ignored; two authenticated zero-match sibling libraries; test/collector/coverage/source all 0 |
| Canonical gate `7528` (`d8f2e4→d4f683`) | Original/guardian/qualification 1 at full Tippy; 940 warnings = 937 dead_code + 3 unused_imports, zero source errors, two separate locationless notes; qualified_source null |

The 11 methods are three new actual Cloud/actor controls, four existing actor
controls and four lower Work/Model cancellation/join/loss controls. The new tests
hold actual picker and prefetch ModelList RPCs, including a real EOF-inactive
second backend. Refusal preserves RPC/response ownership, ModelsLoaded UUID/result
and App FIFO order; the independent persistent actor inbox is also preserved.
Public bootstrap consumes the SAME retained model cache without rediscovery.
Other controls invalidate old cuts by new work, displayed-owner change, inventory
reordering and same-ID physical replacement, then check original cancellation.
All explicit Result exits service and shut down original/retired physical owners.
These ordinary decision callbacks test custody, not genuine Native acceptance.

Unit books are byte-identical (`cd4e9ab71afbcaf452f8cc6169e48943f2373d4dc5fb23eeb0ddc218da245865`),
covering 12,538 source inputs and 11 tools. Ordinary rows, exact-ID coverage and
source collector agree; raw log SHA-256 is `65585a3231a30db47c593a5ad5c8578a1bfe2bcea6aee5f35dba96d9531cc2bd`.
Unit plan `be600d62…` binds 17 selected Git source files and the unchanged protected
three-package/source-selected Trust15698/native-prerequisite/result route.

The no-option canonical guardian→qualification→executable check used the supported
literal hook baseline `6ce1c0…`; intake still read actual origin/main. Source/tree
`ff51`/`88bf17a1…`, upstream `ce303…` and clean=true stayed unchanged; origin/main
advanced `683df620…→a944ba33…`, so the whole qualification snapshots are NOT equal.
The gate stopped before certification, application runtime and full-test stages.
Its log hash is `0e65b07ee8c4e3ec0649193bc5c6684305f046ed5aed05b10e0fc6453a75c228`.

Static audit `d1aeb70e…` and delta `b0363c84…` preserve multiplicity using normalized
path/family/message, ignoring line/column. Compared with c43's 952 warnings,
14 diagnostic rows disappeared and two were regrouped: existing Work `changes`
and `spawn_original` methods remain unused while capture/observe became used.
Their source bytes did not change; these are not new unused definitions. The net
reduction is 12 warnings. The remaining 940 findings are not resolved by this work.

This is a join/currentness precondition, not an atomic transport/H1 cut, weak
request-identity claim, Native grant or whole-scene capture/install. Whole-App
scene assembly and staged input/editor/resource installation still need their own
production integration. Independent selected-unit success does not turn the failed
canonical gate into certification or complete repository test coverage.

## Integrated source and catalog repair

A normal merge produced `5d84daf3f5d934a8ce3bc4f6259bcaa386d1ace4`, integrating
fresh main `a944ba33e6ee73a959fc3ec1dc9b6f3f69c74660`. All six topic code/test file blobs are
unchanged from ff51. Two of the 17 selected bindings changed: sidebar_loading
and app_server_session. Their unsubscribe receipt/correlation changes retain the
original Work Book, physical handle, actor sender and terminal completion route.
The merged source graph was checked independently; ff51 results stay unchanged.

Merged Native11 `96580` (`a470fd→d1a627`) and guardian closed 0: the SAME 11 IDs
passed, no failure/ignore, two zero-match sibling libraries, all four component
statuses 0. This repeats the prior 11 controls; it is not 22 distinct tests.
Books match byte-for-byte (`439095e0278a08b2c2d437b1fb10fcd16e231ad50e96cfd8799a6f06151044a9`),
with 12,546 source inputs and 11 tools; raw log is
`98f5fc4712a7b40f2da6ba9040998711c5497d395f29fe7533acc26035d77d49`.

Merged canonical gate `36490` (`1d2a7b→94b46e`) closed 1 at Tippy: 942 warnings,
zero source errors, two locationless notes, qualified_source null. It retained
937 dead_code and three unused_imports findings, and exposed two incoming Mantle
lints: an async Tokio unit mutex held across region resolution, and cloning an
already-owned region. Source/tree/clean stayed unchanged; actual origin/main
advanced `03d61546…→7d9e3c9e…`. No certification/runtime/full-test phase was reached.
Log SHA-256: `534dc533cfbce0c528d78f373bdc8eda4b95e7d1d6003fe8725f2aeaf18dc9a6`.

Repair `59111ca5a57f221317cbf4f18f7da750ab4c6ee1` changes three owned files only.
It moves the owned region and replaces that async `Mutex<()>` with a private
one-permit Semaphore, held from source sampling through actual resolution and
currentness-checked publication. The observed-data RwLock remains unchanged.
The existing metadata test holds the first real GET, polls a second original
preparation before release, then joins both SAME calls and still requires exactly
one GET and the original cache/source/origin checks. No primitive-mirror test,
production resolver substitution or cancellation/negative-control result is claimed.

Provider Native2 `31612` (`14e82a→f50b48`) and guardian closed 0 at 59111: both
selected methods passed in one authenticated provider library, no failures/ignores;
test/collector/coverage/source statuses were all 0. Before/after books match
`a26a7fcfef069cd22f2a4821c33525267d0c787b9088d06179d0068f47ba3e40` over 12,546
inputs/11 tools; raw `25dff2d237482b93aaf126cdca58dfc24bb40674521664b7487b6d89f1b7ee38`.
The merged Native11 remains measured at 5d84, not relabeled as a 59111 run.

Post-repair canonical gate `36572` (`a0c5ef→1941aa`) closed 1 at full Tippy:
940 warnings = 937 dead_code + 3 unused_imports, zero source errors and two
separate locationless notes; guardian/qualification 1, qualified_source null.
Source 59111/tree `308b65b7…`/clean=true and upstream ce303 stayed unchanged;
actual origin/main advanced `11102339…→f9d0c85f…`, so whole snapshots are not equal.
The supported a944 hook comparison did not replace actual origin/main intake or
qualification. No certification, application runtime or full-test stage was reached.
Log SHA-256: `cab189ae7a205a0918a91b8c03878a53198fa40679d8a55783cfe2a62a1f4020`.

Complete static audit `e3263ede…`, using the same multiplicity-preserving
path/family/message Counter, removes ONLY the two incoming Clippy diagnostics
from 5d84 and adds zero (`b97fbbe6…`). The remaining multiset is exactly ff51's
940 (`8c66b352…`); those findings are not fixed. Frozen manifest wording was
clarified separately (`b34adc6a…`): the replaced lock was an async Tokio unit
mutex, not a synchronous-lock deadlock. Frozen inputs were not rewritten.
Earlier failed gates remain retained. Finite source-specific unit passes imply
neither a complete gate/certificate nor Native grant or whole-scene installation.

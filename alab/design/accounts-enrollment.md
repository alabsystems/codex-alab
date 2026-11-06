<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Native account enrollment operations

Status: implementation design, derived from native origin/main
`91a7cb18812af55a4e9fc9c28b453e7b98477b05`; no live sign-in or credential
inspection was performed for this audit. This extends `accounts.md` and does not
claim that the snapshot-only implementation supports enrollment.

## Existing behavior and gaps

The GUI's `control.rs::AccountServices` owns an `account_work::Lifetime` and
routes `accounts-renew` into `remote_renew::serve_scoped`. That service already
scopes request IDs to callers, retains running jobs, bounds output and cancels
owned work with its lifetime. However, its book is volatile, permits only one
working request globally, and its fixed child command is
`harness accounts renew --due --here` (`remote_renew.rs:218–231`). That is a due
Claude renewal pass, not enrollment of a named Codex row. Restart loses the
operation book; an unknown result cannot establish completion. Do not expose
this existing command as the implementation of `/accounts add`.

Codex's actual enrollment body exists in
`harness/cli.rs::renew_one_for` (15908 onward). It prepares a fresh staging home
through `sign_in_plan`, rechecks the current workflow and policy, runs the native
sign-in driver, verifies the expected account report, and only then binds the
result. Wrong-account results preserve staging and leave live bindings alone.
The account store already has renewal checkpoints, workflow admission and
renewal/success generations. Reuse those records and transactions rather than
building a second login ledger.

`accounts import --cli codex` is explicitly refused by `run_accounts_import`
(12142): pending-source import is Kimi-only. Codex rows are added through the
existing source-aware add transaction, or discovered by synchronization. An
explicit known lab account label resolves through the authoritative lab roster;
an unknown arbitrary label must not become an authenticated account identity.

## Typed native operation

Extend the authenticated session-bound account protocol with an enrollment
operation and status/cancel requests. No shell text, command path, credential,
URL or mailbox content belongs in a request. A start carries a bounded operation
ID, expected snapshot revision, and an authoritative existing account reference
or a typed source-aware row-add proposal. Creation and enrollment are distinct
transactions: a listed row is not an enrolled login. The native owner checks the
peer's proven process birth and session, current login policy and source holds,
then resolves the proposal using the same add/source code as the CLI.

Extract the named Codex renewal orchestration behind a typed service entry shared
by the CLI and control worker. It invokes `renew_one_for`'s staged workflow under
`account_work::Lifetime`; it must not run a shell or reinterpret user text as
arguments. Internal typed driver subprocesses remain part of the existing native
login workflow. Preserve pinned-host/device-code checks and mailbox ownership.
The control reply maps typed outcomes into waiting, authentication-required,
ready, applied, cancelled or refused, with bounded sanitized reason codes.

Associate the operation ID and immutable request digest with the existing
admitted workflow/checkpoint before launching the provider. Polling the same ID
returns that workflow; reusing an ID with another row or revision is refused.
Following a host restart, reconcile its durable checkpoint and committed binding
generation before starting anything. An uncertain provider attempt remains
uncertain; an empty volatile book is never permission to duplicate it. Reuse
existing uncertain-renewal recovery instead of inventing a retry rule.

`applied` requires the identity-verified binding transaction and its committed
success generation. The account view can refresh afterward. Enrollment must
never change this session's assignment or the global pool pin as a side effect.
Cancellation retires the operation's owned browser/CLI group before releasing its
lease; a timeout or disconnect alone does not cancel or restart a provider flow.
A security challenge retains one quiet pending operation and never authorizes
bypassing the challenge.

## Enrollment control interface

Account management uses the proven native process/session and the normal Signal
hold gates. Manual enrollment does not require a supervisor's Walls or Relaunch
grant. Automatic wall recovery retains its existing stronger admission checks.
Automatic rotation being off does not by itself decide permission for an explicit
manual account-management request; the native owner checks that permission.

The read namespace remains `codex-accounts`. Its independent enrollment discovery
request is `enrollment-status schema=1 request=<32 lowercase hex>`, answered as
`OK codex-accounts enrollment-status <JSON>`. The bounded response carries:

- `schema`, `request`, and the proven `session` (SID, process PID, canonical birth,
  and current host epoch).
- A stable authority `revision` (64 lowercase hex), captured with the targets.
- Enrollment capabilities `enroll` and `create`, independent of the read-only
  snapshot's capabilities.
- At most 128 targets with opaque `account_id` and display `label`.
- At most 256 retained operations belonging to this proven caller, with
  `operation`, `account_id`, admitted `revision`, `phase`, and `generation`.

Discovery runs on a worker. While it loads, it returns
`OK codex-accounts state=waiting request=<same request ID>`; polling retains
that ID and does not start another worker.

Mutations use the separate Signal namespace `codex-accounts-change`:
`enroll schema=1 operation=<32 hex> revision=<64 hex> account=<opaque reference>`,
`enrollment-poll schema=1 operation=<32 hex>`, and
`enrollment-cancel schema=1 operation=<32 hex>`. The native owner resolves poll
and cancel targets from retained custody; a caller cannot retarget them.

Before durable admission, a worker may return
`OK codex-accounts-change waiting <JSON>` carrying only schema, operation and
session. A cold poll cannot know its account before the worker reads custody;
this transport wait claims neither a target nor the durable Running phase.
`OK codex-accounts-change enrollment <JSON>` additionally carries phase and
the resolved account reference and generation; only Applied carries an actual
committed binding generation.
Replies fit within 64 KiB and expose no raw driver diagnostics or credential data.

Closing an account pane only stops observation. Reopening discovers and follows
the existing operation ID rather than starting another login. A successor Codex
process requires the native custody receipt to recover the original caller;
matching a SID alone never transfers operation authority. An existing-target-only
implementation advertises `create=false`: enrolling a configured account must
not claim that an arbitrary new subscription account was created or added.

## Independent-account concurrency

The current implementation does not provide the concurrency promised by
`accounts.md`. `accounts.rs::Store::signin_within` (10684 onward) takes one
machine-wide `signin.lock` across every CLI. Browser profiles require exclusive
ownership. Codex's native login currently tries the registered loopback callback
ports 1455 and 1457, retaining the successfully bound listener. Merely launching multiple
control workers leaves them serialized or busy; removing the lock is unsafe.

Separate three ownership domains explicitly: credential lineage, browser profile,
and callback listener. Different account lineages may progress concurrently only
when their browser profiles and callback endpoints are independent. Same-lineage
refresh and sign-in publication must retain one owner and revalidate generations.
A shared browser profile remains exclusive regardless of labels or vendors.
PKCE flows must retain their exclusively bound registered callback listener;
the two current ports bound their independent listener capacity. An arbitrary
unregistered redirect port is not an alternative. Device flows can avoid a callback
listener but still require profile and lineage ownership; do not assume device
flow alone proves safe concurrency. Preserve lock ordering and held-resource
retirement through process exit and uncertain replies. Update the existing
AccountSigninLock model and conformance coverage with the new resource scopes.

Until that refactor is qualified, expose waiting-on-existing-sign-in truthfully;
do not claim independent-account parallel enrollment works. This is an explicit
remaining requirement, not a reason to weaken locking or silently narrow scope.

## Required verification

Exercise the real typed orchestration with a controlled native driver: duplicate
start and lost replies; same ID/different request; stale source revision; process
replacement and cross-session polling; restart during admission, provider work
and binding publication; wrong identity; removed or held source row; policy
revocation; cancellation before and after provider completion; existing active
credentials remaining usable; and no assignment/global-pin change on enrollment.

For concurrency, prove two independent lineage/profile/endpoint operations overlap,
while two aliases of one lineage cannot refresh or publish concurrently. Include
a shared-profile negative control and fixed-port collision control. Verify owned
process retirement before releasing each lease and recovery of uncertain attempts
without duplicate provider work. Test sanitization using canary credentials,
URLs, codes, paths and mailbox bodies in driver output; none may enter account
protocol responses or conversation history.

Fixture success proves orchestration only. Final acceptance requires a real
isolated native/Codex pair, a verified login binding, and subsequent authenticated
usage under that binding. Workspace PAT enrollment remains a separate credential
kind and must not be forced through OAuth refresh or email-code machinery.

## Retention over indefinite operation

The current bounded operation book refuses new admission at 256 records and
never silently evicts an uncertain request or a completed ID. This is a safe
foundation limit, not completion of indefinite unattended recovery. Discovery
also needs pagination: the 64 KiB frame can fill before the record-count limit.

The follow-up design is a durable operation epoch, stored and advanced in the
existing account transaction. Start, poll and cancel references must include
their epoch. A retired epoch must reject an old start before any provider work;
an absent compacted record must never mean that its request is new. Compaction
and epoch advancement must be one publication, with restart tests at both sides
of that publication.

Scope epochs by proven credential lineage so one unresolved account cannot
prevent completed requests for other accounts from being compacted. Labels and
browser profiles are not lineage identities. Preserve active, uncertain and
authentication-required custody until its native resolution is proved. Merely
waiting for a deadline, losing a connection or observing a process exit does not
authorize eviction or replay.

A retired reference needs an explicit stale result; it cannot claim a lost
completion was observed. The client then inspects current verified account
state without automatically creating another provider attempt. Qualify stale
request rejection, alias lineage fencing, lost completion replies, concurrent
compaction and admission, and bounded multi-page discovery before enabling
compaction. Until then, keep the limit explicit and retain this requirement.

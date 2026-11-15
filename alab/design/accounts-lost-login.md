# Lost login and early move: the move is built, the early move deferred

Status on 2026-11-14: the lost-login move is built on both sides; the early
move is a deferred design. codex-alab's side of the move is on main since `8bd759e3b6`
(the refusal veto, the `LoginRotation` purpose, the `codex-login-rotation`
client and its ask; `alab/CONTRACT.md` §3, §4 and the §5 login-wall row). It
does nothing until a host advertises `codex_rotation_login=1`. aterm's verb
and decision are on aterm main since `7f1450f9dda` (2026-11-13). No released
aterm carries them (0.201.0, installed here by 2026-11-14, does not: its
`harness policy` reply has no `codex_rotation_login`), so no installed aterm
advertises the word, and no move has run live. The smoke's cases are in
`alab/smoke/renewal_follow.py` since `169a6be055` (`alab/PARALLEL-LOGINS.md`,
"Trial runs of the lost-login move"): the cases that must not move pass; the
standard case that must move does not yet, because aterm's fixture presents
the refresh token under the platform trust store and not the run's test CA.
A case that serves the issuer on plain loopback HTTP instead moved, which is
trial evidence of the move's steps only. The reading behind this page was
done on codex-alab `cdd8038411` and aterm `98f59e72528`; that aterm ref
contains `c1bfcf76c6b` and `04d19e38fc0` but not `2020f39c770`. An independent
critique of a first draft found it unsound, and this page records the corrected
plan. Recheck file and line references against current code before building.

## What happens today

A session's own login can be lost for good: the refresh token expired, was
reused or invalidated, or the account was revoked. The session then stops on
`Wall::Login`, even when other enrolled accounts are signed in on this Mac.
codex-alab raises "sign in to Codex again" (`alab-autopilot` `decide.rs`). It
resumes only when the same identity's login comes back: `decide.rs` decides and
types login-back once (`login_back` and `decide_wall`'s `Wall::Login` arm;
`alab-autopilot/src/login.rs` only reads whether the login is back), or aterm's
same-row renewal attaches a successor. For Codex that renewal mostly cannot
happen unattended. Sign-in stops at the provider's bot check, which nothing
here evades, and a bot check leaves the row alone for 6 h. A lost login is
therefore a session stopped for hours, or until a person signs in again. Only a
person's `/accounts use` moves it sooner.

Nothing moves a codex-alab session before its wall. aterm's own early rule
(`[harness] accounts = early`) never advises a self-supervised codex-alab
(`advise_session` answers `Stay(OtherSupervisor)`).

## Decision: a lost login may move a session, only for a token's ordinary end

A session whose login is lost moves to another signed-in account only when
the refusal shows the token simply ended: `refresh_token_expired`.

`refresh_token_reused` does not qualify. A race between processes of one
login lineage never settles as a lost login (one of them holds the renewed
token), so a settled `reused` is a token spent outside that lineage, which is
no ordinary end.

Every other loss stays a stop for a person, as today: `refresh_token_reused`,
`refresh_token_invalidated`, a 400 `invalid_grant`, an account the provider
deactivated or banned, an unknown or unrecorded reason, a plain HTTP 401 while
a usage read still works, and a login in aterm's own custody (a managed
slot). The last is a stop for more than one reason; see "A login in aterm's
custody" below.

Why so narrow: a revocation or deactivation can be the provider's own action
on that account. Carrying the work on under another of the owner's accounts
would get around that action rather than recover from an outage.

Both sides can tell these cases apart. Upstream keeps the reason in
`CodexErrorDetails::RefreshTokenFailed` and drops it only from the public
category (`protocol/src/error.rs`, `Unauthorized`). It reaches ALab, coarsely
(`Expired`, `Exhausted`, `Revoked`, `Other`), through `TurnErrorInput.error_details`
(`ext/extension-api/src/contributors/turn_lifecycle.rs`) and
`AuthManager::refresh_failure_for_auth` (`login/src/auth/manager.rs`). aterm's
`RefreshJournal::Rejected` keeps a closed `RefusalReason`
(`codex_app_login.rs`).

Nothing in this move signs anyone in or copies a token. The destination is a
row already signed in on this Mac, and the restart changes only
`CODEX_AUTH_PATH` to that row's own login (`alab/AUTH-BOOTSTRAP.md`).

## Plan for the lost-login move

aterm decides the whole recovery order in one place. codex-alab detects the
loss and asks once, then follows the purpose of the receipt it is given. It
never guesses from a `waiting` answer, which aterm uses for many states: a
renewal in progress, a hold, the person's grace.

1. The refusal's reason is recorded on both sides.

   aterm: done on aterm main `c1bfcf76c6b` (2026-11-13), for legacy-file
   logins. The reason lives in the login's `.refreshed` journal
   (`RefreshJournal::Rejected` keeps a closed `RefusalReason`: expired, reused,
   invalidated, invalid_grant, another RFC 6749/6750 code, or unknown; no
   provider text) and is read through one shared reader (`refusal_standing`,
   `row_refusal`). `aterm harness accounts --cli codex`, `table` and
   `observed` show `signed-out (<reason>)` once the refusal is settled, and
   only for the row's own account. Only native custody gets a row field
   (`Account.auth_refusal`, A2).

   codex-alab: it also keeps its own reading, as a veto only. Any reason but
   `Expired` that a turn error gives, and codex's held refusal for the cached
   login confirms, is kept as a create-new file named by a domain-separated
   SHA-256 of the refused refresh token under `<CODEX_HOME>/alab/login-refusals/`,
   holding the reason word and no token byte, pruned after 30 days
   (`alab-autopilot/src/login_refusal.rs`). A move needs aterm's settled
   `Expired` and no codex-alab veto. Where codex-alab never got an answer,
   aterm's record decides.

   Native custody: done on aterm main `3a8282b2de9` (2026-11-13). A login in
   aterm's own custody keeps its settled refusal's reason on its row
   (`Account.auth_refusal`, the code word only), recorded only while the
   login file still holds the token that was refused, and cleared by any
   later news of that login.

   Still open on aterm's side:
   - The codex-accounts snapshot does not carry the reason, and no report
     shows a custody row's.
   - The move does not read a custody row's record: its decision passes no
     custody record to `row_refusal`, so such a row reads Unknown and stays
     a stop ("A login in aterm's custody" below).
2. A real `LoginRotation` purpose on both sides, chosen by the verb, not by a
   parsed field (aterm's `control.rs`, `codex_renewal_control.rs`). A third
   verb, `codex-login-rotation`, takes the usage wall's rotation's four keys
   (`thread`, `turn`, `source_account`, `source_user`) and answers with its own
   `OK codex-login-rotation` prefix in the rotation's grammar. Its record marker
   is the existing `rotation` field, told apart by rule `login-rotate@v1`
   (aterm checks `owed.rule`, `codex_renewal.rs`). The quota rotation's receipt
   cannot be reused as is: its history must be a usage-limit error, so a
   lost-login rollout would be refused.
3. One `prepare` decides every step.
   - A committed same-row successor comes first: a renewed row's login dir is
     no longer the session's, and such a row is not SignedOut, so
     `codex-login-rotation` answers `not-needed` and codex-alab's next pass
     follows today's `codex-renewal` path.
   - Another row is taken only when all of these hold: the source is SignedOut
     on record and the token its file holds now has a settled `Expired`
     refusal; `LoginActivity` is not active for the source row (a held renewal
     is not running, and no reader separates the holds); and a destination
     passes a fresh `usable()` read for a different account.
   - SignedOut is reached by aterm's existing two-refusal rule: when aterm
     presents the token itself, the refusal settles 60 s later, then one
     usage-only confirm read records SignedOut. A fresh SignedOut never
     qualifies on its own, and a refusal record alone does not either, because
     aterm's proactive refresh can write `Rejected` while the access token
     still serves.
   - Selection is the pure `accounts::candidates` ranking at
     `Point::LoginEnded`, walked with fresh `usable()` reads. (`Point::LostLogin`
     already exists and means a lost login right after the session's own move.)
     There is no advise, `Advice`, `SessionPlan` or `commit_native`.
4. The move is session-scoped. Unlike `/accounts use`, which has no receipt, it
   rides the sealed receipt through a session-scoped selection writer. That
   writer skips `active == target`, `receipt_binding` and `native_rotations`,
   and calls no `begin_rotation`, canary or `prove_completed_rotation`. It
   re-checks `lease_verdict` on the target, as `begin_rotation` does for the
   quota path. It leaves `active` unchanged and is not charged to the hourly
   breaker. That breaker is machine-wide (2 × rows per hour), and spending it
   here would make a later usage-limit handoff wait. Thrashing is already
   excluded: the source stays SignedOut until renewed, and each session moves
   at most once per lost-login episode.
5. Read bounds, so nothing reads a provider on a timer. Per process, wall turn
   and source refresh token, aterm presents the refresh token at most once, and
   only while no journal names it. It makes at most one usage-only confirm read
   after the refusal settles, and only while SignedOut is not on record. It
   reads each candidate once. A failed publish retries only the publish and
   ends as `not-needed` after `OBSERVATION_MS`.
6. codex-alab mirrors the purpose in `alab-core` `account_handoff.rs`
   (`Purpose::LoginRotation`) and persists it by rule in `alab-tui`
   `record.rs` and `session.rs`, which before inferred the purpose from the
   field name and would have dropped the owed continuation. Nothing is typed
   for the move: the successor reruns the original turn id with
   `recover_turn_if_idle`. The tests prove that the intent and its text
   ("Continue the original task.") survive the record and that the original
   turn is recovered once.
7. Scope:
   - The pin: the move requires SignedOut on record, and staging SignedOut
     already ends the source's pin with `Unpin(signed-out)`. That event plus
     the move's log line is how the move says so. A pin set after the sign-out,
     or a pin on any other row, stops the move, as the quota writer refuses any
     pin.
   - Autopilot off or passive is out of scope: aterm's record match requires
     an active Cruise or Land, and codex-alab asks only under a proven claim.
   - `codex exec` is out of scope; it has no login variant of its handoff.
   - The policy reply advertises the move as `codex_rotation_login=1`. A host
     without the word (aterm 0.199.0 among them) keeps today's stop, and
     codex-alab sends it nothing new.

codex-alab's side of the ask (`renewal_follow.rs` `follow_renewal_point`): a
login wall with no handoff intent asks once where the host advertises the word
and no veto stands, and follows the ask every 5 s. `waiting`, every refusal
that is no definite no, and every transport failure are asked again. A definite
no declines the move until the next native start: for an ask no receipt binds,
`not-needed`, an unknown verb, `invalid request`, `native wall changed`,
`wrong recovery purpose`, an unreadable reply, a policy that no longer permits
it, a veto that now stands, or 240 s of this process's awake time since its
first ask; for a bound receipt, whose `commit` may have gone, only `retired`.
On any definite decline the intent comes off the record (the goal's sign-in
recovery stays) and the next pass, scheduled at once, takes today's Login path.

Build order: after aterm's renewal fix (`s51fb4f/renewal-bounded-resume`)
lands. It changes the same renewal files, and every move rides a successor
attach that fix repairs. Live use of the receipt's commit and `Seal::admits`
also needs aterm `2020f39c770` (a same-holder in-place claim renewal keeps
`supervisor_revision`).

## A login in aterm's custody stays a stop

Decided 2026-11-13, after a design and an independent critique, both from
reading the code and neither from a run
(`~/Developer/.ubd/lost-login-custody-move-design.json`). Letting
the move read a custody row's recorded reason is not the small step it looked
like, and it is not built. Three things stand in the way:

- A session on a managed login never asks. Its refused refresh comes back
  from the host as `ERR codex-auth unavailable`, which codex-alab carries as
  a transient I/O error and classifies as a server wall, not a login wall.
  `follow_renewal_point` asks `codex-login-rotation` only from a login wall.
  Reading the record in aterm would therefore move no real session. To ask,
  codex-alab needs a distinct, terminal `codex-auth` reply that carries the
  reason word, mapped to a permanent refresh failure
  (`alab-core/glue/auth_refresh_lock.rs`, both `managed_auth.rs` files,
  `alab/CONTRACT.md`'s managed-slot paragraph).
- The restart lane has never carried a native-source member. Such a session
  holds a slot, an assignment and source-registry state that belong to its
  old row, and today the decision's stop is the only thing that keeps it out
  of the lane. No test seats a real member through a move.
- A rollback has nowhere to go. The settlement that would qualify the move
  also retires the grant's catalogue entry, and a retired entry refuses the
  startup admission a rolled-back session needs.

Two differences from a legacy login would remain even then. Nothing but
native custody may read a custody token, so there is no usage-only
confirmation behind the refusal. And the row holds nothing until the refusal
settles 60 s on, so an ask inside that minute is a definite no rather than a
wait.

A row in custody is also never chosen as a destination: the candidate read
goes through the legacy reader, which is fenced off a custody home. A pool
whose other rows are all in custody has no target.

Order, if this is taken up: prove the member relaunch and decide the
rollback in aterm first, with a test that seats a real member; then the
aterm decision, as a function that is given no reader; then the client
contract.

## Decision: the early move is deferred

Moving a session before its wall, at a quiet turn end, to an account with
more room would spare a cut turn and a 429. It is deferred until live
usage-limit handoffs with two or more accounts have been measured. Reasons:

- Its value is unmeasured. A relaunch costs a cold prefill, and no handoff
  has yet run live on this Mac.
- Its guards are not yet sound:
  - Under Cruise every turn end owes a continuation, so "codex-alab owes the
    next turn" rules almost nothing out.
  - The preflight's seconds-per-point bound is a staleness bound, not a burn
    projection.
  - aterm's rotation probe answers `waiting` first, so either every early ask
    falls back to the same account or every qualifying turn end waits up to
    60 s.
  - Sharing the breaker could starve later usage-limit handoffs.

If it is built, it needs:
- a refusal under any pin and for a session a person moved since its last
  wall;
- its own budget: one early move per session per window episode, and only
  while the shared breaker keeps `rows` of headroom;
- a bounded wait for the probe, at most 60 s, before the continuation is
  typed on the same account;
- a host-observed history predicate: a clean completed turn plus a fresh
  `leave_early` read;
- a background-terminal check;
- credits first, through the redemption window rule.

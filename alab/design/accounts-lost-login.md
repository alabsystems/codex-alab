# Lost login and early move: decided, not built

Status on 2026-11-13: both are designs. Nothing here is implemented. The
reading behind them was done on codex-alab main `a79804068f` and aterm
origin/main `04d19e38fc0`. An independent critique of a first draft found it
unsound, and this page records the corrected plan. Recheck file and line
references against current code before building.

## What happens today

A session's own login can be lost for good: the refresh token expired, was
reused or invalidated, or the account was revoked. The session then stops on
`Wall::Login`, even when other enrolled accounts are signed in on this Mac.
codex-alab raises "sign in to Codex again" (`alab-autopilot` `decide.rs`). It
resumes only when the same identity's login comes back: it types login-back
once (`alab-autopilot/src/login.rs`), or aterm's same-row renewal attaches a
successor. For Codex that renewal mostly cannot happen unattended. Sign-in
stops at the provider's bot check, which nothing here evades, and a bot check
leaves the row alone for 6 h. A lost login is therefore a session stopped for
hours, or until a person signs in again. Only a person's `/accounts use` moves
it sooner.

Nothing moves a codex-alab session before its wall. aterm's own early rule
(`[harness] accounts = early`) never advises a self-supervised codex-alab
(`advise_session` answers `Stay(OtherSupervisor)`).

## Decision: a lost login may move a session, only for a token's ordinary end

A session whose login is lost moves to another signed-in account only when
the refusal shows the token simply ended:

- `refresh_token_expired`;
- `refresh_token_reused`, settled as a race between processes of one login
  lineage.

Every other loss stays a stop for a person, as today: `refresh_token_invalidated`,
an account the provider deactivated or banned, an unknown or unrecorded
reason, a plain HTTP 401 while a usage read still works, and a managed slot
answering `unavailable`.

Why so narrow: a revocation or deactivation can be the provider's own action
on that account. Carrying the work on under another of the owner's accounts
would get around that action rather than recover from an outage. Today
neither side can tell these cases apart. Upstream reduces a permanent refresh
failure to `CodexErrorInfo::Unauthorized` without its reason, and aterm's
`RefreshJournal::Rejected` keeps no issuer code. So the first step of the
build is to record the reason. Until it is recorded, nothing moves.

Nothing in this move signs anyone in or copies a token. The destination is a
row already signed in on this Mac, and the restart changes only
`CODEX_AUTH_PATH` to that row's own login (`alab/AUTH-BOOTSTRAP.md`).

## Plan for the lost-login move

aterm decides the whole recovery order in one place. codex-alab detects the
loss and asks once, then follows the purpose of the receipt it is given. It
never guesses from a `waiting` answer, which aterm uses for many states: a
renewal in progress, a hold, the person's grace.

1. aterm records the settled refusal's issuer code on the row and in
   `RefreshJournal::Rejected`. codex-alab carries the reason past
   `Unauthorized` in its own crates where it can see it. If it cannot, aterm's
   record decides.

   Done on aterm main `c1bfcf76c6b` (2026-11-13), for legacy-file logins:
   - `RefreshJournal::Rejected` keeps a closed `RefusalReason`: expired,
     reused, invalidated, invalid_grant, another RFC 6749/6750 code, or
     unknown. No provider text is kept.
   - `aterm harness accounts --cli codex`, `table` and `observed` show
     `signed-out (<reason>)` once the refusal is settled, and only for the
     row's own account.

   Still open:
   - A `reused` record does not say whether the token was spent by the same
     login lineage, which the move requires.
   - The codex-accounts snapshot does not carry the reason.
   - Logins in aterm's native custody record no reason.
2. aterm adds a real `Purpose::LoginRotation`: auth-wall history, its own
   marker and rule, a target identity, a selected writer and a
   purpose-carrying `prepare` parse. The current parse takes exactly four
   fields, so `wall=login` cannot simply be appended. The rotation receipt
   cannot be reused as is: its history must be a usage-limit error, so a
   lost-login rollout would be refused.
3. One `prepare` decides every step. A committed same-row successor comes
   first. Another row is taken only when all of these hold: the source is
   SignedOut on record with a qualifying reason; its renewal is not running,
   or is held by the person backoff; and a destination passes a fresh
   `usable()` read for a different account. Selection is a native login plan
   at `Point::LostLogin`; today's quota plan answers `NothingDue` for anything
   other than a wall.
4. The move is session-scoped, like `/accounts use`. It sets `rotates:false`,
   leaves `active` unchanged and is not charged to the hourly breaker. That
   breaker is machine-wide (2 × rows per hour), and spending it here would
   make a later usage-limit handoff wait. Thrashing is already excluded: the
   source stays SignedOut until renewed, and each session moves at most once
   per lost-login episode.
5. A retry never re-reads a source that is SignedOut on record. "No target" is
   decided from the records before any provider read, so nothing reads a
   provider on a timer.
6. codex-alab mirrors the purpose in `alab-core` `account_handoff.rs`. It
   persists it by rule in `alab-tui` `record.rs` and `session.rs`, which today
   infer the purpose from the field name and would drop the owed
   continuation. A round-trip test proves "Continue the original task." is
   typed after the successor's relaunch.
7. Scope:
   - A person's pin on the lost row ends with the move, and the move says so.
   - Autopilot off or passive is out of scope: aterm's record match requires
     an active Cruise or Land.
   - `codex exec` is out of scope; it has no login variant of its handoff.
   - The policy reply advertises the move as `codex_rotation_login=1`. A host
     without the word keeps today's stop.

Build order: after aterm's renewal fix (`s51fb4f/renewal-bounded-resume`)
lands. It changes the same renewal files, and every move rides a successor
attach that fix repairs.

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

<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Moving a session to another account

On 2026-11-18, signed Codex 0.11.0 and aterm 0.207.0 on m159 resumed a
conversation on another enrolled subscription and completed a provider turn
under the new identity. The trial also covered an exhausted destination.
[Interactive switch on m159](../PARALLEL-LOGINS.md#interactive-switch-on-m159-2026-11-18)

The Accounts pane keeps Enter for refresh. The development client shows a
keyboard picker when aterm supplies a verified reference for an eligible
subscription. On confirmation, Codex refreshes the roster and checks the
revision, slot and reference before requesting the switch. The explicit
`/accounts use <label>` command resolves the label supplied by the user.
It also works with hosts that only support named switches.

Refreshing the pane withdraws the picker destinations, the per-row switch
commands and the automatic-choice command together. The account labels and
last observed usage stay visible if the read fails. A later successful read
restores the commands offered by that roster revision. A typed command resolves
the label at the time it is entered, so a command from an older roster could
select a reassigned label.

On m159, a terminal replay with a synthetic host first offered a selectable
account, then refused the next roster read. The previous CLI kept the old
switch command beside the stale-data warning. At `61a4fcf0f4`, the warning
and account row stayed visible and the switch command disappeared. Neither
terminal requested a restart.

The roster uses eligible rows for the typed-switch hint. The picker also
requires a subscription reference for each destination. A host can support
named switches while the only other login is at its weekly limit. The pane
shows that row's recorded limit and
keeps the host's automatic-choice capability independent. If multiple
addresses share the same mask, the enhanced pane can show the connected
server's full email above the roster. The selected local server's ChatGPT
identity supplies that line. The host supplies the launch slot and masked
roster rows independently. Reading the email at render time removes it on a
disconnect or identity change, including while a cached roster is still on
screen. The roster records whether two rows share a mask. For those rows the
pane suggests `/status` for account details when the server is connected and the
pane has no email to show. During a disconnect it shows the transport state.
The picker displays the host's labels and masks. The full email appears only
in the local view.

Both paths ask the application to check for unsent input before opening
the native connection and again after writing the session record. A blank
composer can coexist with an unsent draft on another thread or backend.
Only the application owns those parked drafts.

An earlier terminal test on m159 ran the label-only development CLI against
a simulated aterm and a loopback provider. After opening the picker, Esc returned to the roster.
Reopening it and pressing Enter sent exactly one restart for the selected
label. In a second run the destination reached its usage limit while the
picker was open. Enter displayed the limit and sent zero restart requests.
The provider received only the setup turn in each run. The signed-client
trial above verified the host relaunch and the conversation ID separately.

### Show both identities in the local account picker

Two subscriptions can have the same masked address and different remaining
allowances. On m159 the two signed Codex tabs each showed their own full
connected email. The signed `/accounts` roster could distinguish the slots
by label; it could not identify both full addresses in one view. The local
shell lookup joins the slot label and expected provider email. Enrollment
can also show the configured sign-in address when those addresses differ.

The development Codex client offers **Show account addresses** in the account
list when masks collide. The user's left navigation key asks the native host
to draw a temporary, read-only address list over the requesting session. Opening it
from Codex does not require an Account chip in the aterm window. Escape
closes the overlay and preserves the Codex selection. The underlying pane
continues to identify the launch slot and the connected server account.
If the host reports that the displayed roster changed, Codex closes its switch
picker and keeps the observed accounts and quota visible. Refresh makes the
current roster's address and switch actions available again.

The host candidate accepts a session-scoped `codex-accounts-display show`
request with a request ID and the displayed pool revision. Showing content
on the person's screen requires write authority and a foreground Codex
process. The host reads the local roster, checks its revision and draws each
label beside the configured sign-in address. It shows a distinct expected
provider address separately. A missing address stays unknown. Only an
acknowledgement with the request ID and revision goes back to Codex. The
candidate withdraws the view on loss of focus or authority, a changed roster,
or expiry. It has no action that changes a pool preference or restarts a
conversation.

Signed aterm 0.209.0 lacks this control. The development client gives the
local lookup command on that host. New enrollment still requires the
Registry binding described in the [browser design](accounts-browser-enrollment.md).

### Preserve the confirmed picker destination through restart

On November 19, the signed CLI sent a synthetic host one named restart for
`work`. The request carried the pid and label. The host returned a quota
refusal and the original process stayed alive. The earlier keyboard-picker
trial sent the same label-only request. Native aterm derives the visible row
ID from the label, stores the label in its restart record and resolves the
label again at relaunch. If `work` changes subscriptions after the picker's
final roster read, a newly eligible account can inherit that name. The trials
measured the wire and the refusal. A relaunch on the wrong subscription has
not been observed.
[Codex move request](../../codex-rs/alab-aterm/src/account_move.rs) ·
[Native row ID](https://github.com/alabsystems/aterm/blob/376345c209b66b4eb11d3696620e59e308e2f606/crates/aterm-agent/src/harness/codex_enrollment.rs#L78) ·
[Native restart record](https://github.com/alabsystems/aterm/blob/376345c209b66b4eb11d3696620e59e308e2f606/crates/aterm-agent/src/harness/restart_stamp.rs#L42)

For a confirmed picker move, the host should advertise that it can honor the
specific selection. Codex should send the selected row and its snapshot
revision together with an opaque reference to the provider identity the
host verified when it built the displayed row. The existing pool revision
covers roster and binding metadata. Replacing credentials in the same
directory can leave those inputs unchanged. Aterm can use the account and
user identity it already checks for automatic quota moves. For a
Registry-managed row, the Registry supplies the assigned account key and
login address.
[Pool revision inputs](https://github.com/alabsystems/aterm/blob/2976ace03a322e7ea083e703a8b96cf77c306dc9/crates/aterm-agent/src/harness/accounts.rs#L1323) ·
[Native provider identity](https://github.com/alabsystems/aterm/blob/2976ace03a322e7ea083e703a8b96cf77c306dc9/crates/aterm-agent/src/harness/codex_renewal.rs#L14)

At restart admission the host must validate the revision, resolve the
reference and match the current login to the displayed identity. It must
carry that identity in the restart record and verify the prepared login
against it again before resuming the thread. An ordinary credential renewal
for the same subscription can pass this check. A changed binding detected
before restart leaves the original process running. If detected after
shutdown, recover the original
thread under its verified source account when possible and report the
refusal. A host offering only named moves can still serve explicit
`/accounts use <label>` and `/accounts auto`.

The bound selection must survive a host replacement during restart. The
current restart reader keeps a recognized process record even when it cannot
decode the destination. Keep the confirmed selection in a versioned record
that a successor can validate before starting a conversation under another
login. If the successor cannot read it, recover the verified source or leave
the conversation pending. Test replacement with an older host after admission
and before relaunch, including a newly reassigned destination label.

Run `python3.13 alab/smoke/account_switch_legacy.py --codex /path/to/codex --output /new/result-directory`
to check the client against a simulated label-only host. Signed Codex
2026111701 kept typed switching available and presented no keyboard picker.
The development picker executable (SHA-256 starting `9b0319b6f397`) sent a
label-only restart after the fixture reassigned its selected label. The host
refused every restart and each terminal exited normally.

Qualify both boundaries with two eligible, identically masked fixture
subscriptions. First reassign the label after Codex confirms the picker and
before native admission. Then keep it stable through admission and reassign
it before native relaunch. In each case verify that the replacement receives
no conversation request. With stable bindings, verify that the chosen
subscription receives the next turn on the original thread. Repeat with an
in-place credential replacement that keeps the label, directory, roster
revision and visible email fixed. Changing only the provider workspace or
only the provider user must refuse the move. Refreshing credentials for
the same workspace and user must preserve it. These are release qualification
criteria for the client and host together.

The development protocol adds `capabilities.select_bound` and a nullable
`selection_ref` per row in schema 4. A confirmed switch sends
`restart pid=<pid> account=<label> selection=<revision>:<reference> request=<nonce>`.
The nonce identifies one confirmation. Before sending, Codex withholds its
ordinary resume command and retains the original conversation for this request.
If the reply is lost, reopening the Accounts pane can ask aterm to cancel
that exact request. A pending outcome keeps the conversation reserved.
The signed 0.209.0 host supports explicit named switches.

During a confirmed picker resume, Codex defers an in-place binary upgrade
and keeps the conversation on the selected subscription. The upgrade
protocol requires credentials supplied by a new native backend. The picker
verified a file-backed login, and the upgrade protocol cannot carry that
identity check into the new backend. A pending picker request also holds an
upgrade until the switch or its cancellation settles.

## Decision

`/accounts use <label>` and `/accounts auto` move the running session to another
Codex account by asking aterm to restart it on that account: aterm stops the
process and its relaunch types the session's own resume line with only
`CODEX_AUTH_PATH` changed, so the same conversation continues in a new process
under the other login. codex-alab never switches credentials inside a running
process for this command.

The in-process assignment request grammar this file used to specify
(`codex-accounts-change assignment-prepare|commit|poll|ack|cancel` and the
`assignment-target` preview) is withdrawn, and codex-alab's client for it is
deleted with its receipt in the session record. The reason: an in-process switch
has to retire and rebuild every account-scoped resource in the process (sockets,
model and tool catalogs, Apps and MCP grants, background work) and prove each
step, and no process host for it was ever installed (`install_process_host` had
no caller). From 2026-11-12 (the capability gate) every request was refused;
between 2026-11-06 and then a request was accepted, persisted an unacknowledged
`assignment` locator into the session record before it asked for a host, and
waited in reconciliation. Current builds read such a record, ignore the key and
drop it at the next write. aterm keeps no custody for it: in aterm's source
read (branch `s51fb4f/codex-account-move`) `codex-accounts-change` serves only
enrollment and `assignment-target`. A restart builds all of that fresh under the new
login through the ordinary startup path, which aterm's crash relaunch already
does when a row stops serving. Nobody is signed in by a move: a row that is not
enrolled or is signed out is refused.

The process-wide activity domain (`codex_alab_core::assignment::Domain`) stays.
Its admission and activity API (`admit_start`, `admit_work`, `admit_resource`,
`admit_publication`, the activity leases) and the manager catalog's retirement
(`CatalogProof`, `manager_catalog::Participant::retire`, the mailbox's `Retire`)
are live: native source, destination-epoch, exec, MCP and quota recovery code use
them.

**Dormant, kept on purpose.** The rest has had no production caller since the
deleted coordinator, its only one (found by `git grep` over `codex-rs/alab-*`,
`core/src`, `login/src` and `app-server/src`, excluding `#[cfg(test)]` modules and
`*_tests.rs`):

- `Domain::request`, `hold_unresolved`, `cancel_pending`, `try_retire`, `retire` and
  `recover_retirement`, so no `RetirementPermit` or `BoundaryPermit` is made in
  production and `Intent` and `Selection` travel nowhere. Only `request` and
  `hold_unresolved` move a domain out of `Phase::Open`, so in production it never
  leaves it: the waits on other phases and `DispatchGate`'s `Pending` deferral are
  inert.
- The `AuthManager` native-source boundary methods in
  `alab-core/glue/auth_native_source.rs`: `alab_assignment_source_admissible`,
  `alab_native_source_fence`, `alab_rebind_native_source_fenced`,
  `alab_pin_current_native_source`, `alab_restore_unchanged_native_source` and
  `alab_rebind_native_source` with its install helpers. Each takes a
  `BoundaryPermit`.
- `assignment_service::Status`, `Phase`, `Mode` and `Reason`, which only
  `alab_restore_unchanged_native_source` takes.

They are kept, and marked in the code, because they are the only in-process
rebinding codex-alab has: a move by restart keeps no draft, shell or sub-agent (it
is refused while either of the first two exists, and ends the last), and whether
the automatic rotation still to come needs to keep them is not decided.
If it moves sessions by restart too, they should be deleted. (The 2026-11-12
deletion commit called them "still called"; only their own tests call them.)

## What a move is

- **Session-scoped.** It changes the auth file of one process. aterm writes
  nothing to the pool's active row, its pin or its rotation history.
- **Uncharged.** It is not a rotation and does not count toward the rotation
  breaker.
- **Same conversation, new process, from rest.** It is asked, and aterm carries it,
  only while the session record stands at its idle point, so no turn, act, draft,
  background shell or waiting box is under way (see "In the TUI" and "At aterm's
  signal"). Sub-agents at rest and anything else held only in memory end with the
  old process. A session that left rest in the moment of the signal gets its one
  carry-on (CONTRACT §5); an idle one comes back idle. One waiting at a recorded
  usage wall comes back at that wall, bound to the login moved to: that login's
  next accepted usage read that permits the wall's bucket resumes it at once, the
  retry rung the old login's tries reached dropped at the move (no rung is kept per
  login, so a move back starts that login's ladder over too), and the deadline its
  record carried stays the latest it waits (CONTRACT §5, the quota wake; before
  2026-11-14 it waited out the old login's reset, and until `1d171802e3` the old
  login's rung still paced that first resume).
- **Background terminals stop it.** A background terminal (a dev server, a watch
  build) keeps the record `busy` for as long as it runs, and the restart would end
  it, so the move is refused up front, by codex-alab and by aterm alike (an outside
  `restart account=` too), until the person stops it or it ends. Nothing offers to
  end it for them: decided 2026-11-13, because a move that silently ends a running
  server is the turn-ending loss the rest fence exists to prevent, and a person who
  wants it gone stops it in one step. Before that date such a move was carried and
  ended the terminal.
- **Needs Autopilot on and a shell under the session.** aterm restarts only a
  session it supervises and relaunches only where a shell would type the line.
- **Proven again at relaunch.** The ask is judged on record, without a provider
  read. At relaunch the destination must answer a usage read, or hold a sample
  read within 300 s, and still serve. When the requested account cannot serve,
  aterm tries the previous account and can select another pool account. It posts
  a notice when the fallback lands. After resume, `/status` shows the active login.
- **Under a pin.** A session moved off the row the pool is pinned to does not
  rotate at its usage limit while the pin holds; it waits at its wall until the
  pin ends or someone moves it again.
- **`auto`** is a one-shot move to the row a new launch would start on now. When
  that is the session's own row, nothing restarts.

## Wire

Both lines go on codex-alab's own accounts control connection.

**Snapshot.** `codex-accounts snapshot schema=2 request=<32 lowercase hex>`.
Schema 2 is schema 1 plus:

- `assignment: {account_id}`: the row aterm judges from the process's launch
  environment, or null. There is no `mode` key.
- `pool: {choice_id, pinned, rotation}`: the row a new launch would get (or
  null), whether the pool is pinned, and `off`, `wall` or `early`.
- per-row `eligible`: a move to that row would not be refused on record.
- `capabilities.select`: aterm would carry a move of this session now.
  `capabilities.auto`: `select` holds and the pool's choice is another row.
  `add` is always false.

`account_id` and `choice_id` always name a listed row when present. The
development Accounts pane first requests schema 4. Schema 3 adds a required nullable
`signed_out_reason` field to each schema 2 row. Aterm may supply a settled
refresh refusal for a signed-out login. The reader accepts only the host's
enumerated codes, including the explicit `unknown` value. A null field
means the host has no recorded reason to show.

The client checks the requested schema and rejects a reply of another schema, any
`add: true`, an `auto: true` without `select` or without a choice other than the
session's own row, and unknown fields.

**Move.** `restart pid=<own pid> account=<label>` or
`restart pid=<own pid> account-auto`. A label is `[a-z0-9][a-z0-9._-]{0,63}` and
never `help`; codex-alab refuses any other label before sending anything.

**Replies**, read by key in any order:

- `OK restarting pid <n> cause=<stall|asked>[ discarded=<bytes>] account=<label>`
- the same line ending in the bare word `account-auto` for the pool's choice
  (`account=auto` names a row labelled `auto`)
- `OK restart gone pid <n>`: this process no longer leads its tab; also
  `ERR pid <n> is not the foreground process group (<m>)` and
  `ERR no foreground process group`, answered before the act
- `ERR restart unavailable: <word>`, where the word is one of aterm's fence words
  (`not-the-hosts-agent`, `not-relaunchable`, `not-supervised`, `driver-lease`,
  `driver-turn`, `halted`, `start-unread`, `no-state`, `exit-owner-not-ready`) or
  one of the move's:
  `account-unread`, `account-unmanaged`, `account-not-resumable`,
  `account-recovery-pending`, `account-off`, `account-unknown`,
  `account-no-choice`, `account-current`, `account-same`,
  `account-not-enrolled`, `account-signed-out`, `account-held`,
  `account-limited`, `account-identity`, `account-restart-pending`, and last
  `account-busy`: aterm did not find the session at its own rest when it came to
  restart it (see "At aterm's signal" below)
- `ERR denied native operation authority` or `ERR denied`, before the act
- `ERR restart failed: stamp (<why>)` (nothing written or sent) or
  `ERR restart failed: killpg pid <n>` (no signal sent by this act)

A refusal leaves no stamp and sends no signal. A refusal word codex-alab does not
know and a stamp that could not be written read as "nothing restarted". codex-alab
accepts `Restarting` only for its own pid and the exact account token it asked
for; any other line is an unknown answer, after which the session may or may not
restart. `killpg` is one: where the act joined a standing stamp for the same
account, aterm keeps that stamp and an earlier ask's restart may still follow. No
reply text is shown or logged. codex-alab waits up to 15 s for the answer, not the
connection's usual 5 s: aterm judges the account for up to 5 s before it takes the
act's lock, and reads the session record under that lock for up to 1 s more.

## In the TUI

`/accounts use|auto` refuses, sending nothing, when there is no conversation;
while the displayed widget runs a turn, holds queued input or a submitted turn
that has not started, or the TUI sees a turn on any thread it routes (a
sub-agent's, a side thread's, a remote one); while the session record would not
stand at its idle point (`codex_alab_tui::session::idle_point`, the writer's own
words: `status:"idle"` needs a frame that reads idle, done or error and no
`Shared::unrest`, that is no turn on any thread, shown or not, no act standing or
waiting for its turn, no box answer, no owed carry-on, no person's turn on its way
or queued input and no live upgrade); when the snapshot is schema 1 or a
capability is false (rotation off says so; otherwise, with Autopilot off, the
sentence asks to turn it on); when the label names no row, the session's own row
or a row that is not eligible. The record's other ways out of rest with no turn
running have their own sentences, so the person learns why before anything is
asked: a background terminal keeps the frame `working` ("Background terminals run
in this session, and the restart would end them. …"); a box waiting for an
answer keeps it `blocked` ("This session waits for an answer (an approval, a
question or a sign-in). …"); an active goal between its turns, or tools still
starting, keep it `working` with no terminal ("This session is working with no
turn running (an active goal between its turns, or its tools still starting), so
it is not at rest. …"); and a session still starting has no frame yet, or writes
the busy initializer of a relaunch whose launch probe has not answered ("This
session is still starting (its first screen, or aterm's confirmation of its
relaunch, has not come yet), so it is not at rest. …"). `idle_point` reads that
initializer from the writer without its lock, which a write holds through its
sync. The command's own Enter is not read as a draft.

The pane shows the requested label or the pool's current choice while the client
checks whether it can switch. A pin on another row is shown here. The client
writes the session record and then sends the `restart` request. Only aterm's
acceptance changes the message to “Restarting this conversation.” Both messages
point to `/status` on resume because destination eligibility can change at relaunch.
If the record cannot be written, nothing is sent. The pool read can take up to 30 s, and a turn, an act, queued input, a background terminal, a box or a draft
(a key since the last frame counts) may begin meanwhile, so the same check runs
again, draft included, both before the record's write and after it. Before, so
the record aterm reads is written at the idle point: leaving rest is written
through but coming back to it is coalesced (CONTRACT §4), so a record written busy
could still say so when aterm reads it. After, for what began during the write.
The command's own Enter reads as a draft until the next frame shows the composer
empty, and the pool's read can answer sooner, so before the first of those checks
the move waits for that frame, at most 500 ms, while no key, paste or click came
since the command; one that did is a draft, and refuses at once.
Each refusal word has a fixed sentence ending "The session is unchanged."

The pane shows what the move says. Closing it does not stop the move: aterm may
still be weighing it, and the key that closes it reaches the program, so aterm can
refuse the move for that very key (`account-busy`). The move's last word (every
refusal, and an answer that could not be read) is therefore said once in history
where its pane is gone, closed, replaced or left for another thread; it is one of
the fixed sentences, which name no account. The pending sentence, which names the
row, stays in the pane, and so does a carried restart's, since the process then
ends.

## At aterm's signal

The client's checks end at the ask, and aterm then judges the account for up to
5 s and may wait for another tab's restart: a turn that Autopilot or the person
starts in those seconds would be ended by the signal. So aterm fences the move
itself, for an account ask that writes its own stamp only (a plain restart, the
stall and memory remedies, is unchanged, and so is the schema 2 snapshot's
`capabilities.select`, which still means "carried at rest"):

- Under the act's lock and before it stamps, it reads this session's record
  through the home of the self-claim copy it judged, within 1 s, and needs its
  idle point for this pid: `status:"idle"`, `draftEmpty:true`, no `exit`. A
  record that is busy, waiting, missing, unreadable, late or another process's
  refuses, with no stamp written. Its own fences (a driver's lease or turn, a
  hold, its claim) are read again after that read, so a hand that came during
  it answers its own word.
- Just before its signal, and before it discards unread input, it needs no input
  left unread in the pty and no person's gesture since the ask arrived; it takes
  its stamp back to refuse. Where the process left its tab under that stamp, aterm
  answers `OK restart gone pid <n>` instead (its exit may already be read as the
  act's), which codex-alab never reads, being that process.

Either refusal is `account-busy`. The /accounts pane says: "aterm did not find
this session at rest when it came to restart it (a turn, an act, typing or unread
input, or a session record it could not read), so it restarted nothing. The
session is unchanged." Because the client asks only at the same idle point, that
sentence means the session left rest meanwhile (the key that closed the pane
among it: the sentence is then said in history), or its record could not be read.

What remains: a turn's start or an act writes `busy` through before it proceeds,
with an atomic write that syncs its file (tens of ms, at most 1 s); one whose
write lands after aterm's read is ended by the signal, before or just after its
turn reaches the model, and its record (`busy`, `signal:15`) gives the relaunch
its one carry-on (CONTRACT §5) or keeps its paced act owed. A turn the TUI only
sees (a sub-agent's, a side thread's, a remote one) is in no record, so aterm
cannot see it; codex-alab's own check sees it only up to the ask. Autopilot's
turns already wait while the /accounts pane is open, and all its acts for the
human grace after the person's keys, so the remaining window is mostly a person's
or mail's. An older
codex-alab reads `account-busy` as a refusal word it does not know ("aterm refused
to restart this session now. The session is unchanged."), which is safe.

The `/accounts` view uses the row and pool information from schemas 2
through 4 to show which switches are available. It renders the recorded sign-out
reason in the local account pane. Restart preflight and automatic handoff
continue to ask for schema 2 because their decisions use eligibility.

## Older aterm

An aterm without schema 4 or 3 answers an unsupported snapshot request with exactly
`ERR codex-accounts invalid request`. Only this reply triggers an older
schema request. A denial or malformed snapshot ends the read.
An aterm without the move answers schema 2 with exactly
`ERR codex-accounts invalid request`. codex-alab then asks schema 1 once, finds
every capability false, and answers "Account changes are unavailable on this
host." Should it reach the restart anyway, the old aterm answers
`ERR bad restart argument: <word>`, which codex-alab reads as unsupported and
answers with the same sentence. Nothing is typed or restarted on an old aterm.
Both cases are tested against alab-aterm's fake host only.

## Handoff at a usage wall

The same schema 2 snapshot sharpens the automatic handoff's pool gate
(`Snapshot::another_account_can_run`, asked by `rotation_target_in_sight` in
`alab-autopilot`): schema 2 names the session's own row, so one other able row is
enough. Against a schema 1 aterm, which names no row, two able rows are still
required, as before.

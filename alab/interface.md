<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# codex-alab interface

Codex and Claude Code share one experience contract: Vanilla retains the vendor's
rendering, interactions, defaults and newly shipped features. Enhanced adds ALab's
interface and Autopilot controls. Codex implements these additions in its compiled
fork; aterm does not rewrite Codex's terminal output.

`/alab off` selects Vanilla and `/alab on` selects Enhanced. Vanilla keeps a small
`[ALab off]` switch on the existing composer border so Enhanced can be restored
with a click. The rest of the native interface stays unchanged, with no branded
launch card or automatic harness action. `/alab` also restores Enhanced. The saved choice also selects the
packaged defaults on the next launch, so disabling the interface cannot silently
keep ALab's feature overrides. Explicit user configuration retains its normal
precedence. Managed-package identity, explicit account isolation and restart-safety
bookkeeping remain available in both modes; they do not authorize automatic turns.
Switching modes preserves the current account, thread, selected
model, permissions, prompt, attachments and queued input. Turns that run in
Vanilla are the person's, unseen by the Autopilot: back in Enhanced it decides
nothing over the point that stood before the switch, counts the session past its
opening message, reads the thread's history and acts again at its last turn end,
with the NEEDS YOU lines it leaves standing (`alab/CONTRACT.md` §5, person's own
exchange).

In Enhanced, the composer has a three-way Autopilot switch. Click **Cruise** to
continue working, **Land** to finish and land the current work, or **Off** to wait
for the next message. Each segment updates the session through aterm's owner-only
mode control. The filled segment shows the mode reported by aterm. Square
brackets also mark it in plain text. Land stays selected when the status
changes from `⤓ land` to `✓ landed`. On light
16-color terminals all three labels use the configured text and background colors,
with the selected segment shown in reverse video.

Beside the switch, aterm's status glyph shows Cruise (`∞`), a scheduled landing
(`∞ ⤓18:00`, local time), Land (`⤓ land`), a completed landing (`✓ landed`), a
point left to a person (`⤓ quiet`), or Off (`○ off`). Click the status to cycle
modes. At the scheduled time, Cruise switches to Land and the agent finishes
the current work. Clicking a scheduled landing switches to Land immediately.
When aterm's harness or its switch for codex-alab sessions is disabled, or when
aterm has no harness host, the composer shows `⊘ off`. Click it for the corresponding Settings location.

Right-click or control-click a segment or status glyph to open the Autopilot
picker. When the terminal reports pointer motion, the choice under the pointer
is underlined. The arrow stays on the keyboard selection. Click a mode or timer
preset to select it. You can also open the picker
with `/alab menu` and choose a row with the keyboard. Choose **Cruise until…** to enter
a local 24-hour time such as `18:00`, or a duration from one minute to 24 hours
such as `+1h30m`. If the entry is invalid, the editor shows the accepted format
and keeps the cursor in place. A queued mode selection still takes effect.
Choosing **Cruise** clears a scheduled landing.

The composer reserves space for the release pair, then fits the three mode names.
As space runs short it removes their padding and shortens the separate ALab
button to `[on]`, then uses the space before the release and beside the button.
For `0.12 on Codex 0.169.1`, all three names fit at 48 columns.
At 61 columns the switch also shows the scheduled landing time or full Land
status. The targets regain their padding at 67 columns and the full
`[ALab on]` label returns at 72. With a pending update, the time and
`↑ update` fit together from 70 columns.
At 43–47 columns, the short edition button leaves room for the full status,
including a scheduled landing such as `∞ ⤓18:00`. In smaller panes the glyph
remains available when it fits. Right-click the status or use
`/alab menu` from the keyboard. The ALab button selects the
Vanilla or Enhanced interface.

The ALab interface builds on Codex's native terminal UI.
The rainbow composer overline shows the running ALab release and its Codex
baseline, for example `0.11 on Codex 0.169`. A nonzero patch stays visible. Clicking the
label opens the exact build identities and update report. A pending update adds
`↑ update` when space permits. The colors adapt to the composer's background and
the terminal palette. In 16-color mode, the release, update hint and edition
button use the terminal's configured text color. The rule keeps its rainbow.
The compact footer leads with one account unit: the account email, subscription
plan, weekly usage bar with its reset time and available reset credits, and, where
there is room, the five-hour allowance. Its parts are joined by `/`, and a `│` sets
the unit apart from the workspace fields (activity, mode, branch, model, context),
which keep `·` between them. The full account unit is clickable through its last
painted cell, including for Arabic and halfwidth Japanese text. The usage trend and
estimated consumption at reset appear when they fit beside the full email, plan
and available five-hour allowance. On light terminals the account, quota and
workspace text use contrast-adjusted colors in RGB and 256-color mode, or the
configured foreground in 16-color and monochrome modes.
When the email username is too long for its share of the row, the footer keeps
its beginning and end so a numbered alias stays visible.
The account and weekly reading retain their bold emphasis. Routine Working, Thinking, Ready and Default / Full access
labels are omitted; permissions and task state retain their native behavior.

While the displayed conversation has an active goal, a separate `Goal:` row
shows its objective above the account and quota row. Long objectives shorten
with `…` to fit the terminal; `/goal` opens the full objective and controls.
The row follows the displayed conversation and disappears when its goal is
paused, stopped, completed, cleared, or disabled. It remains visible while the
agent works and between goal turns, including in Plan mode.

An example of the information hierarchy, with illustrative values:

```text
   0.11 on Codex 0.169 ───────────── ⤓ land ─▏ Cruise [ Land ] Off ▕─[ALab on]
› Ask Codex to work on this project
──────────────────────────────────────────────────────────────────────
  you@example.com / Pro / weekly [██░░░░░░] 30% used ↻ Fri 16:00 · resets ●●● │ main
```

The names remain on the composer's rule without adding
a dashboard above the conversation. The status row answers which account is active,
which allowance applies, and where the work is happening. The two rows have
separate jobs; account statistics are not squeezed into the brand overline.

The overline button toggles the interface immediately. `/alab` also toggles it;
`/alab on` and `/alab off` select a state, and `/alab help` lists the controls.
The choice is saved as `[tui] alab_interface` in the user's config. ALab's packaged
default is on; ordinary config-layer precedence determines the initial value,
and a live explicit choice persists through subsequent display refreshes.

Off restores the native composer and configured status presentation, retaining
only the `[ALab off]` recovery switch. Click it or use `/alab on` to restore Enhanced.
Changing presentation preserves the draft, selected model, permissions,
conversation, queued messages, attachments, and configured status-line items.

The launch card places the Codex mark at the upper left and the rainbow kitty
beside the release pair and session details. Narrow panes use a
six-column, three-row Codex mark when the release pair and all visible facts
fit beside it at their current size. Direct truecolor aterm draws the PNG. Other
terminals and multiplexers use a text rendering of the same ring and prompt.
If the details need their own column, direct truecolor aterm can place a
four-column mark beside the title and complete release pair alone.
The Autopilot row follows the
session's reported mode. Outside aterm the card still draws the session details.
The `[ALab on]` button selects
Vanilla; Cruise, Land and Off select Autopilot policy while Enhanced stays on.
aterm never draws controls over either interface. [Dark and light launch-card previews](design/codex-opening-banner.md#preview)

Use `/alab account` to inspect the full selected email, subscription plan, weekly
and five-hour allowances, reset times, and the usage snapshot's checked time.
Where the native renderer forwards mouse input, clicking anywhere in the footer's
account unit, the joins between its parts included, opens the same view; the `│`
boundary and the workspace fields are not targets, and dragging footer text retains
native text selection. The details use the connected server's account data and
request a native usage refresh for signed-in ChatGPT accounts. Existing account
and usage updates appear while the view is open; closing it preserves the draft.
In a local aterm session, the inspector points to `/accounts` for the other
configured subscriptions and their switch commands.
Switching ALab off closes the view, and the command stays inactive until ALab is
enabled again. `/usage` retains Codex's
analytics and reset actions.
The inspector includes the current week's usage history, cycle dates and an estimated
pace. History starts with observed account usage and survives restarts when the backend
supplies an account identifier. Older responses keep history only in the current native
account session, including while optional email metadata is loading. Gaps represent
periods without observations. Estimates require three
readings spanning 30 minutes and are withheld when the data is stale. Account changes
and resets keep histories separate.

The footer shows the most recent account credit balance. For local personal
accounts, it also shows credits charged to this chat when task accounting
reports them. The inspector gives the billing report's cutoff time when available,
including when the provider returns no entry for this chat. An empty report
shows `Chat spending unreported`.
When billing identifies both plan and credit use and supplies a net amount,
the spending line shows the exact amount followed by `plan also used`.
An explicit zero net debit also appears in the inspector. If billing attributes it
to the included plan, the inspector shows `Covered by plan`.
When the provider identifies the plan, credits, or both but omits the debit,
the inspector names the source and shows `credit amount unreported`. Reports
without credit activity leave the footer available for the balance.
The inspector also shows how this chat's usage compares with the current 5-hour
and weekly limits when the billing service supplies those percentages. A chat
can exceed 100% of a current limit across multiple windows. Credit spending
comes from the separate debit field or explicit billing source.
Opening the inspector requests chat spending for a local personal account even
when plan allowance remains and no credits are available. Account refreshes
share the same limit of one billing request per minute for the displayed chat.
While waiting for a first report, the inspector shows `Checking chat spending…`.
If billing is temporarily unavailable, the inspector says so. An existing
report stays visible during the next check.
When the footer shows chat credit activity, a confirmed empty earned-reset
inventory stays in the inspector, even if the account balance is unreported.
If the detailed weekly allowance is too wide to fit beside the credit figures,
the footer shows the short weekly reading when it fits. A fresh earned reset
expiring within 24 hours takes priority over that weekly reading when only
one fits beside the credit figures. With incomplete expiration data, the
compact count shows the known minimum, for example `≥1 reset expires soon`.
If a quota refresh omits the plan for the same account, the current chat’s
report and any pending billing request are preserved. A refresh identifying
a workspace or explicitly reporting an unknown plan clears personal chat spending.
When a billing refresh times out, returns an older cutoff, or reports that
usage is unavailable, the display labels the previous figure `Last report`
and keeps its original cutoff. While that report is retained, a current
account balance takes the first position in the footer. A fresh billing
report puts chat spending first again. The inspector shows both figures.
When the account balance itself goes stale, the footer and inspector label
the last reported amount `Stale balance`, including an explicitly reported
zero or unlimited balance. A missing amount stays unknown.
A report at the same or a later cutoff can correct the total, including a refund
or a cleared charge. An undated report can update the display without inheriting
a cutoff. Switching chats or accounts clears the previous chat’s figure.
If a billing check detects a different local sign-in for the same account ID,
it clears the previous figure. The inspector says `Chat spending unavailable`
when the check cannot establish a report for the current sign-in. The next
eligible check verifies the new sign-in and fetches that user’s report.
Partial reports show `Partial` before the amount so the qualifier stays visible
when the footer is clipped. A negative net debit appears as `Net refund` followed
by the amount returned. For a local personal account,
open `/usage` → **View analytics** → **Chats** for the breakdown. For a local
workspace account, the inspector points to **View analytics** → **Credits** for
workspace spending. It shows these directions once the account's plan is known.
When the provider supplies an individual monthly credit limit, the inspector
shows its remaining percentage and reported reset time. An exhausted limit
blocks spending even if either spending total is unreadable. At the reset time,
the inspector marks the monthly limit as needing a refresh and retains the last
reported balance. A subsequent account reading supplies the new percentage.
A workspace spending restriction or an account marked unavailable to spend
appears before earlier chat charges in the footer. A reported zero shows
`0 credits left`. If the amount is missing or unreadable, the inspector shows
`Unavailable to spend · balance unreported`. Once the provider allows spending
again, the footer shows the chat's current reported charges first. When the
provider identifies a spending restriction, the footer and account inspector
name the cause alongside the reported balance. They distinguish an empty
workspace balance from workspace, member, account and monthly spending limits.
When only the plan allowance runs out, the footer keeps showing the
account’s reported credit availability. The account inspector shows the figures at narrow
terminal widths and identifies missing or stale balance data. A change to the
shared account balance cannot identify which chat incurred a charge.

Reset-credit dots use the backend's available count. Known credits expiring within
24 hours appear in bold with an inline warning. Color terminals use a warm
highlight where the palette permits it. On light 16-color terminals the meter
uses the configured text color. In monochrome and light 16-color mode,
an expiring credit is marked `◉` beside the other available credits `●`. The inspector shows the
known expiry time. A current empty inventory shows just the count. Routine
account polls keep checking that count and omit expiration details while zero
is fresh. A new positive count makes the next poll eligible for details.
Opening the inspector explicitly requests details. Available
resets with missing dates are identified explicitly. Expiring and expired
counts share one direction to review resets in `/usage`. A stale inventory
is marked as needing a refresh. Reviewing
or redeeming a reset remains an explicit native `/usage` action.
Passing a known expiry removes that credit even after detail metadata ages out;
stale details hide future-expiry warnings without restoring expired credits.
After a known expiry, a fresh count supersedes cached credit details until an updated
detail read identifies the currently available credits.
In short windows, the details scroll with the configured list navigation keys;
the view displays the resolved shortcuts for scrolling and closing.

During a server reconnect, the account row starts with `Reconnecting`. If the
connection fails, it starts with `Offline`. The label stays visible as the pane
narrows and before the first account read. Cached account and allowance details
follow when space permits. The account inspector keeps the connection label in
its fixed title and identifies those details as cached.
Scrolling the details cannot hide the connection label. A restored connection
returns to the current native task activity and account-refresh flow.

The Rainbow Kitty uses evenly timed frames and continuous loops: 20 frames per
second while working and 12.5 while idle or waiting. Reduced motion keeps the
first frame; an unfocused aterm stops requesting animation redraws.

No pet is selected by default. A pet explicitly selected in `/pets` has its own dock
beside the entire composer, including its status and
shortcut rows. Text, the cursor, mouse hit targets and the image use the same
reserved geometry. The image never rises into conversation history. When the
dock would leave fewer than 72 text columns, or the viewport is too short for
it, the kitty hides so the account and weekly allowance retain the space.
Hidden pets stop scheduling animation redraws. Showing the dock again resumes
the animation, including after a resize or popup closes. Full-screen overlays
hide the pet and picker preview images without disabling the selected pet,
including when the session already owns the alternate screen. Disabling the
pet also releases the image and its cached frames from the terminal.
Exiting clears both pet images explicitly, including an open picker preview
in an inline session. Switching threads clears the previous widget's pet images
before restoring the selected conversation.

## Sign-in and separate accounts

`/alab account` inspects the account used by the connected server. For ALab's
default embedded server, manage sign-in from a shell with `codex login`,
`codex login status` and `codex logout`; `/logout` also signs out from the TUI.
A remote app-server manages its own credentials.

**To switch an idle conversation on signed ALab 0.11.0 in aterm**, open
`/status` to see its connected email and thread ID. `/accounts` lists
the local slot labels and masked emails. With Autopilot Off, signed
aterm 0.209.0 and Codex 0.11.0 can show “Account changes are unavailable
on this host.” Enable Land in the conversation you intend to switch so
aterm can supervise its restart.

If the two masks are identical, run the label lookup from the codex-alab
checkout. It selects an installed Python 3.11 or newer and reads the local
roster. Each label appears beside its configured provider email and any
distinct sign-in address.

```sh
bash alab/account-labels.sh
```

When the sign-in address differs from the provider email, it appears in `loginEmail`.
To check whether aRegistry has saved an explicit alias link for each sign-in
address assigned to this Mac, run this from the checkout.

```sh
python3 alab/account_registry_probe.py --here --show-alias-declarations
```

Read `alias_declaration` beside each `registered_login`. A saved link is
`declared` and an unset link is `absent`. A `disputed` link has competing
values that the typed address record and the Registry email view both show.
The targets stay private. This check also works when the mailbox view is unavailable.

Exit 0 means the requested count was met and every declaration is settled.
Exit 1 means too few assignments matched or a login has a `disputed` or
`unconfirmed` declaration. Exit 2 means the lookup itself could not be
verified. For example, if the two Registry views disagree about the competing
alias values, the report withholds the logins and exits 2.

To see which inbox aRegistry lists for each sign-in address, run this command.

```sh
python3 alab/account_registry_probe.py --here --show-mail-routes
```

This report puts the saved-link status in `mail_route.alias_declaration`.
When `mail_route.status` is `reported`, `mail_route.mailbox_key`
identifies the recorded destination. A provider observation
(`mail_route.evidence` of `observed`) can establish that inbox even when the
alias is `absent`. A status of `no-effective-route` means aRegistry returned
no destination for the login. To see the routing metadata, run `aterm db alab mail`.

An alias or mail-route check can confirm its result despite a disputed
subscription status when the typed account record and the complete Registry
account view show the same competing values. Plain login discovery and
inventory require a settled status. Setup checks each requested account's
eligibility separately.

Both reports include a `check` object. The value of `check.name` is
`alias-declarations` or `mail-routes`. A top-level `proven` value of
`true` confirms the requested account associations. In that case,
`check.status` is `complete` when every alias declaration is settled
or, for mail routes, every login has a confirmed inbox.
Otherwise it is `unresolved`. The mail-route check can exit 0 with no
saved aliases if every inbox is confirmed. It can exit 1 with a saved
alias if its inbox is unresolved.

When the associations cannot be confirmed, `check.status` carries the
top-level association status, such as `insufficient-accounts` or
`unavailable`. The numeric `check.exit_code` records the lookup result.
Use `--output <new-file>` to save a private copy before the command writes
the JSON to stdout. If stdout fails afterward, the command exits 2 and
reports that the private file was saved. That file contains the complete lookup.

The two local slots checked by a plain quota overview can differ from this
Mac's Registry assignments. [Choose which subscriptions to check](#live-overview-account-selection).

When the target row shows **Request switch**, close the account pane with Esc
and enter its `/accounts use <label>` command in the conversation. After the restart, open
`/status` again to confirm the connected email and the original thread ID.
[Signed switch verification](PARALLEL-LOGINS.md#interactive-switch-on-m159-2026-11-18)

If you already have a conversation open under each login, selecting its aterm
tab takes you directly to that conversation. The enhanced Codex footer shows
the connected email when it fits. Use `/status` in a narrow tab. Each tab
retains its conversation and model selection.

If the session has closed, keep the same Codex home. Look up the destination
email in `aterm harness accounts list --cli codex --json`. Use `auth.json`
in that account's current `dir`, or `$HOME/.codex/auth.json` when aterm
omits `dir`. For example, with the default account as the destination and
the saved thread ID in `thread_id`, reopen it from the shell.

```sh
CODEX_AUTH_PATH="$HOME/.codex/auth.json" codex --no-daemon resume "$thread_id"
```

Check the email and thread ID in `/status` before sending a follow-up.
After a quota failure, the follow-up starts a new turn with the saved prompt
and tool results. Signed Codex 0.11.0 can display **Queued follow-up inputs**
while the destination checks usage on reopening such a thread. In the
verified case, the destination had capacity and sent the queued prompt
when the read completed. [Signed recovery checks](PARALLEL-LOGINS.md#reopening-an-interrupted-turn-on-m159-2026-11-19)

In the enhanced interface on the ALab 0.170 review build, sign in to the account you want to use
and run `codex resume`. The picker starts with all active conversations
in this device’s shared Codex home, across logins and project directories,
newest first. To find an older conversation, type a word from its title
or preview. Select the conversation and press Enter. If the picker asks
where to work, choose **Use session directory** to reopen its original
project. If the project has moved, launch the picker from its new location
and choose **Use current directory**. New requests use the current login.
This also works on a login’s first conversation. [Cross-login resume tests](reviews/managed-daemon-account-resume-2026-11-19.md)

Inside aterm, `/accounts` shows each configured label, masked email, login-file
presence, cached pool status, and 5-hour and weekly usage. Press Enter to
refresh the list. The slot marked **current session** comes from aterm's
launch record.

In the development build's enhanced interface, **Connected server account**
shows the full email reported by the local Codex server above the roster.
The line updates when the login changes
and disappears if the connection or identity becomes unavailable.
A signed-out row can also show the reason aterm recorded for a failed login
renewal, such as an expired token. The pane explains when account rotation or
this session’s Autopilot is off. Refresh it after changing a setting. When
aterm can verify the destination subscription and carry that choice through
restart, press → to open the account picker. On a host that supports only
named switches, use `/accounts use <label>`. Each row shows its slot
label and the masked email supplied by the host. Use ↑ or ↓ to highlight a
destination, and press Enter to switch.
Esc returns to the account list. The arrows follow your list navigation key
bindings. When two addresses have the same mask, press ← in the account list
to see the local slot labels and configured sign-in addresses together on a
host with the address view. A distinct expected provider email appears beside
the sign-in address. Esc returns to the account list. With a signed 0.209.0 host,
run `bash alab/account-labels.sh` from the checkout.
If the server is connected and the pane has no email to show, `/status` reports
the session's available account details. An API-key session shows its authentication
method, and a ChatGPT session can show its plan when the email is unavailable.
During an outage the pane shows the connection state alongside the slot labels.
Run `aterm harness accounts list --cli codex` in a shell for the full
addresses. An addressed row can omit its local label from that listing.
The label lookup also accepts two specific provider addresses as arguments
and pairs each with the label used by `/accounts add` and `/accounts use`.

To find the sign-in addresses assigned to this Mac, run the Registry check
on the Mac. This works before you create its local slots.

```sh
python3 alab/account_registry_probe.py --here --show-logins
```

The report lists registered login addresses for current preferences and
placements. It withholds them when the Registry cannot confirm the machine
or the address binding. A shared receiving mailbox can serve both logins.
Use `--machine <label-or-key>` to look up a named machine in the local replica.

### Live overview account selection

With exactly two configured subscriptions, `--overview` finds both addresses.
Add `--registry-here` to check every subscription currently assigned to this
Mac when the Registry lists two or more. Other configured slots are skipped.

```sh
account_python="$(bash -c 'source alab/lib.sh && alab_python_311')"
"$account_python" alab/smoke/live_account_quota.py --overview --registry-here
```

The report matches each registered sign-in address and account key to a
local slot and verifies its provider identity. To check for a saved alias
link, use the [`--show-alias-declarations` lookup](#sign-in-and-separate-accounts).
A change to the machine binding or assignments during the overview withholds
the report. You can also supply two provider addresses explicitly.
If an assigned subscription has no local slot, the check can show the native
setup command. Alongside the assignment and local roster checks, it reads each
missing account's typed Registry record twice. Both reads must confirm a live
account with the same eligible status (unset, `pending`, or `active`). An
unreadable or disputed status withholds the setup suggestion.
Sync loads preferred accounts and can recognize the existing default login.
For a placement still missing after sync, use the account key shown in the
`aterm harness accounts add` command.
If aterm confirms local sign-ins and identifies empty slots among the
remaining assignments, the check gives their labels. If aterm has sign-in
instructions for an empty slot, the check also gives its Registry key. In
`aterm harness accounts list --cli codex --json`, match that key to the
row's `ac` field and read `sign_in_needed`. The native report can also say
to wait for the Registry. For the other empty slots, open `/accounts add`.
The check stops before contacting the provider.
For a verified login with a confirmed slot label, `Request switch` shows
the `/accounts use` command to enter in the conversation you want to move.
If the provider blocks ordinary requests, reports a reached spend control or
rate limit, or a host sample reports a hold, usage limit, sign-out or missing
enrollment, the report shows the slot label and the reason the switch is
unavailable. If aterm reports a lease on the account held by another Mac,
the overview shows its expiry in UTC while that time is still ahead.
The host checks current eligibility
again when you request a switch. When aterm reports that account switching is
off, the overview withholds switch commands for all slots. A pin can still
select the account for the next launch. If the configured Codex launcher can
use only the default login, the overview directs you to `/accounts` in the
running session. That session can be using a different Codex build.
The report checks each label against the credential directory aterm currently
selects, including a directory assigned by renewal. If a destination moves,
its switch command disappears. If aterm refuses the roster, all switch commands
disappear. The provider results still show which credential files passed verification.
Each row shows the sign-in check separately from provider usage. A slot at
its weekly limit can show `Sign-in Verified` and `Provider usage Blocked`.
`Unverified` means the check could not confirm that slot's current login.
`Blocked` means the provider denies ordinary requests for that login.
`Unknown` means the provider's permission could not be established. The
report includes the check time and local reset times for measured windows.
`Next launch  Selected by aterm` marks the subscription the host would
choose for a new ordinary launch at the time of the check. It appears when
the host reports the same choice across the check and its account key
matches the verified login. In a running conversation, `/status` shows its connected account.
When aterm forecasts a wait for the next launch, the overview shows its
local date and time after reconfirming the decision and its login bindings.
The host's launch policy can make that time differ from either provider
window's reset. An expired or changed forecast leaves the choice unconfirmed.

Add `--defaults` to the overview to see which model and reasoning effort a
fresh thread starts with under each login. The check uses the installed
Codex defaults and each subscription's own model catalog. An existing
conversation can keep its saved model and effort when you switch accounts.

Add `--registry-machine <machine>` to see whether the local Registry replica
associates each verified account with that machine. The report matches stable
account keys and lists the source of each association. A retained observation
means the Registry previously saw the account there. A Registry read failure
leaves the verified sign-in and live allowance visible.

If no alternate is selectable, the pane says so. A
row at its usage limit shows its cached usage and reset time when available.
The roster can still show a subscription as usable after its
allowance runs out. At relaunch aterm checks the destination identity and
refreshes stale usage before starting the conversation there.
`/accounts add [label]`
starts or follows aterm's enrollment of a label aterm already has configured.
To switch directly by label, use `/accounts use <label>`. Use `/accounts auto`
for the account a new launch would start on. The conversation resumes in a new
process.
If the requested account has exhausted its allowance, aterm can resume on the
current serving account. After the restart, `/status` shows the connected email,
thread ID and remaining allowance. Switches require an idle session. A running
turn, queued input, a draft in this or another conversation, a background
terminal or an unanswered prompt prevents the restart. Other work held only
in memory, including idle sub-agents, ends with the old process.
It needs Autopilot on and an aterm that carries `restart account=`; on any other
aterm it answers "Account changes are unavailable on this host." and nothing
restarts (`alab/design/accounts-assignment.md`).

To enroll a second subscription on a Mac managed by the lab registry, run
`aterm harness accounts sync alab --cli codex` to load this machine's preferred
accounts. For a specific registered subscription, run
`aterm harness accounts add <registry-account-key> --cli codex`. Aterm resolves
the account identity and creates its local roster row. In signed Codex 0.11.0,
`/accounts add` lists enrollment targets by slot label. If the label is
unfamiliar, run `bash alab/account-labels.sh` from the checkout before
selecting it. At **Account authentication required**, finish the provider's
sign-in on that Mac and choose **Refresh this operation**. Aterm verifies the
account identity before binding the login. Open `/accounts` to check the
enrolled accounts and their allowances.

When the host identifies a row, the development picker also shows its masked
email and whether the slot is signed out or has a login file. It shows the
local lookup when two targets share a mask and calls the authentication step
**Complete sign-in in aterm**. After **Enrollment refused**, it offers
**Open /accounts**. On signed 0.11.0, close the enrollment result and open
`/accounts`.
In a terminal on that Mac, run
`aterm harness accounts list --cli codex` and look for a `sign-in needed`
line under the account. That line gives aterm's current instructions.

Automatic quota rotation needs a running session managed by aterm and an eligible
signed-in destination. On aterm 0.208.0 and 0.209.0, a weekly limit can
interrupt a turn and leave the second subscription unused. A corrected host completed
the same turn on the destination in a full terminal test. The roster report
explains a refused selection, including a login that belongs to a different
account. [Host versions and switching results](PARALLEL-LOGINS.md#automatic-switching-in-the-full-aterm-host-on-m74-november-18)

To keep a separate work login while sharing `$CODEX_HOME`'s configuration,
skills, rules and session history, point `CODEX_AUTH_PATH` at an absolute
per-account `auth.json` file:

```sh
mkdir -p "$HOME/.config/codex-accounts/work"
CODEX_AUTH_PATH="$HOME/.config/codex-accounts/work/auth.json" codex login
CODEX_AUTH_PATH="$HOME/.config/codex-accounts/work/auth.json" codex
```

`codex-alab` stores and refreshes that account's credentials atomically in the
named file (even if `config.toml` selects keyring or auto storage for the
default home) and records `CODEX_AUTH_PATH` in its self-claimed session record
so crash and update relaunches keep the same account.

When you also want separate configuration, skills and session history, create
another Codex home and sign in there instead:

```sh
mkdir -p "$HOME/.codex-work"
CODEX_HOME="$HOME/.codex-work" codex login
CODEX_HOME="$HOME/.codex-work" codex
```

Use the same `CODEX_AUTH_PATH` or `CODEX_HOME` on each launch for that account.
Each home keeps its own configuration, session history and sign-in, including
when credentials are stored in the keyring. Either inline environment setting
above leaves an ordinary `codex` invocation using its existing home and login.

## Account and quota correctness

The subscription label prefers the provider's current account-usage plan over
the sign-in label; an older Pro sign-in must not hide a fresh Pro 500 (`promax`)
response. A newly observed provider plan requests a fresh model catalog through
the native account-scoped request path. Explicit model-catalog reads fetch current
capabilities, so upgrades can expose `/ultrafast` without restarting the conversation.
Switching experience also wakes the model poller: Vanilla immediately restores
upstream polling and cache freshness, including for a worker created in Enhanced.
The installed diagnostic filters also follow the live choice, restoring native
feedback and SQLite logging in Vanilla while retaining upstream payload exclusions.

Account identity comes from the connected app-server's account display, including
its asynchronous `account/read` refresh. It must not come from a second read of a
credential file: a connected server may use a different account. API-key sessions
are labeled as such. An unknown email is shown as unavailable rather than guessed.
Account labels discard display controls and outer whitespace before that fallback,
so an empty-looking value cannot erase the account field or inspector label.
While ALab is on, transient identity-read failures retry through the same connected
server with bounded attempts and backoff. Later usage refreshes can recover a
still-missing email. Account changes and disconnects cancel the retry, and native
request guards reject late results. Off retains the native one-shot reader.

The ordinary subscription allowance comes from the account usage snapshot for the
`codex` meter, using Codex's own window selection and percentage formatting. The
weekly window can appear as either the primary or secondary window; array position
alone does not identify it. Sparse inference updates can describe a model-specific
meter and are not authoritative replacements for the account usage display.

An account-change notification is an identity boundary, even if the plan name is
unchanged. Discard the old allowance before displaying the new account. Pending
email and usage responses retain native request/generation checks. Until a new
account usage read succeeds, show the allowance as unavailable. A failed read
must not invent a percentage or borrow another account's allowance.
Responses for replaced usage controls are discarded before they can change quota
history or reset-credit dots, even when the account itself has not changed.

Only the visible conversation publishes the compact status and account details;
updates from a parked voice session cannot replace the selected account. Changing
conversations requests a fresh native usage read immediately, coalescing with any
read already in flight. The replacement does not inherit the previous widget's
quota, billing or recovery cache.

When the native `/status` freshness threshold passes, label recognized quota
windows as stale instead of showing cached percentages. The account details also
withhold stale reset claims while retaining identity, plan and the last checked
time. A successful usage read restores the current allowances. This changes only
presentation; the native usage cache and recovery decisions remain authoritative.
A local redraw deadline also expires the footer and an open inspector when a
disconnected server has stopped polling, including while the session is idle.

Branch/model/mode/activity data comes from the same native state used by `/statusline`.
Routine activity is tracked without repeating it in the footer; context percentage
does not determine whether a session is active or fresh.
Use native `/status` for available context usage, tokens used and window capacity;
`/alab help` points to it alongside the account and usage controls.
Keep generated conversation titles out of the compact status line: a title is
neither an input suggestion nor the current task's authoritative status.

## Interaction contract

All existing command, completion, editor, permission, transcript, side-question,
voice, and session controls remain reachable. Advertise the resolved runtime
shortcuts, including user remaps. Do not apply Claude Code's keys to Codex silently:
Codex's default `Ctrl+T` opens the transcript and `Ctrl+O` copies a response;
Claude Code uses those two keys for a checklist and transcript, respectively.
`/alab help` resolves the current shortcut or chord for native keyboard help and
explains that it opens from an empty prompt. It omits disabled shortcuts and
advertises the stash shortcut only when the current keymap leaves it available.

As the account row narrows, it drops context, mode, model, branch and task
activity, then the five-hour allowance. It shortens the email beside the plan
and drops the plan when fewer than eight columns remain for the name.
For a connected local subscription with fresh usage, the footer can also drop
the reset details, then the bar, to keep the account name beside the full
weekly percentage. Warnings about stale or expiring credits retain their
usual priority. A 31-column terminal can show `team…ops / weekly 100% used`.
When the pair cannot fit, the weekly allowance takes precedence. The inspector
keeps the full email, quota and reset details.
The build names shorten before dropping either version
number on an 80-column prompt, and the button adapts independently. Use explicit labels and
display-cell-aware truncation. Color supports the text; it never supplies the
only distinction between normal, unavailable, and exhausted allowance.

## When a chat stops for a misalignment finding

A misalignment finding ends the request. In Enhanced mode, Cruise and Land wait
for a person. If you see **Review findings**, open it to read what the server
flagged and the exact continuation it proposes. Only **Acknowledge findings and
continue** submits that continuation. Use **Back** to leave it unsubmitted.
Repeating your original prompt can return the same finding before any tool runs.

A shell command that was running when the finding arrived can still finish.
On signed 0.11.0, its live row can show `(exit 1)` while it runs and keep that
label even after it succeeds. Before repeating a command that changes files,
check its saved result. In a reopened session, press `Ctrl+T` (the default)
to see the command's output and completion indicator in the detailed transcript.

An app-server client can call `thread/read` with `includeTurns` set to `true`.
Match the `commandExecution` item by ID and check its `status` and `exitCode`.
A failed turn can contain a successful command. If the command's completion
has yet to appear, its exit code is still unknown. On builds with the display
fix, the live row stays pending until the command reports its outcome.

On signed 0.11.0, the reopened transcript can show `✗ (1)` for a saved
`declined` or `failed` command whose `exitCode` is `null`. In that case
the record contains no process exit code. Canceling at the approval prompt
can also leave an interrupted turn with no `commandExecution` item.

On signed 0.11.0 for macOS, a command requested with `tty=true` can run in the
account’s home directory if its requested working directory is missing. The
saved item still shows the requested path. For cleanup that depends on a
specific directory, use `tty=false` and an explicit `workdir`. In that mode,
a missing directory stops the command before it starts.

If you see **Server-suggested continuation (not submitted)**, interactive review
was off. Signed 0.11.0 can automatically send a follow-up queued before a finding.
For a new session on that version, launch `codex --enable misalignment_review`.
A finding then clears the queued input and pauses the chat for review. Keep a
copy of any queued text you want to reuse.

On builds that show **automatic sending paused**, a queued follow-up stays
available while you send a fresh request. Use the displayed edit shortcut to
recall it, then press Enter to send it. To enable interactive review for future
launches, run `codex features enable misalignment_review`. The setting takes
effect at launch.

Codex omits the explanation, continuation,
and review identifier from saved history, so resuming a thread cannot reconstruct
its earlier review prompt. A fresh, reviewable finding can offer the action again.

## Local work and prompt stash

`/tasks` (also `/alab tasks`) opens one live, searchable list of the current
checklist, local agents and background terminals. Select an agent to use native
agent navigation, or select a checklist step or terminal to inspect its current
details. The view works with ALab's normal embedded server and refreshes from
native state without starting a shared daemon or polling the network. Search and
selection survive refreshes, including checklist reordering. A delayed refresh
cannot undo newer typing or reopen a closed view.

Checklist and terminal details scroll with the configured list navigation keys,
including in short windows. The full step or command and the captured recent
terminal output remain reachable. Accepting `Back to Tasks` returns to the list;
cancelling closes the details and preserves the draft. A queued Back action
cannot reopen Tasks after the conversation changes or ALab is turned off.
An open Tasks view keeps its local inspection, refresh and Back navigation when
the connected server disconnects. Agent rows show `Reconnect to open` and stay
open when selected until navigation is available again. Search and local
selection survive the connection change.

The checklist is the latest native plan update retained by the conversation view.
Checklist rows require Codex's opt-in checklist tool:

```toml
[tools.update_plan]
enabled = true
```

With the embedded server, when the tool is disabled and no native checklist has
arrived, `/tasks` labels the disabled tool above the list and points to `/alab help`
for the setting, including when agents or terminals are present. This guidance
does not enable the tool or count as a task.
A connected server's checklist capability is not inferred from client settings.

An explicit empty plan clears older steps. Live compaction keeps it; a restart or
transcript rebuild can have no structured checklist to show. This is not a new
persistent or shared task backend, and proposed-plan prose is not treated as a
checklist.

With the ALab interface on, `Ctrl+S` parks the current prompt. Use the empty
composer for a different question, then press `Ctrl+S` again or use `/stash`
(also `/alab stash`) to restore it. A second occupied prompt never replaces a
saved one. Text, image attachments, mentions, large paste payloads, cursor and
Vim edit state stay together. The stash is private to this conversation and CLI
process; switching conversations cannot consume another conversation's prompt.

Native history-search `Ctrl+S`, explicit custom bindings and chord prefixes keep
their existing meanings. The off state adds no stash shortcut or task commands,
and turning the interface off retains a saved prompt for later recovery.

## Harness comparison

This is a capability comparison, not a claim that the interface implements every
Claude Code service or reproduces its exact interaction semantics. Existing
commands can also depend on the selected backend, account, configuration, or
feature availability.

| Capability | Existing Codex entry point | Parity assessment |
| --- | --- | --- |
| Model and reasoning selection | `/model`, reasoning shortcuts | Existing capability |
| Permissions and planning | `/permissions`, `/plan` | Existing capability; permission cycling has no default key binding |
| Prompt completion and editing | `/`, `@`, `/vim`, `/keymap`, `Ctrl+G`, `Ctrl+R` | Existing capability; preserve Codex semantics |
| Detailed transcript and search | `Ctrl+T`, `F3`, `F4` | Existing capability with different default keys |
| Queue and redirect work | `Tab` while working; queued-message editing | Existing capability with different queue semantics |
| Side questions | `/btw`, `/side` | Existing capability; Codex uses an ephemeral fork |
| Session navigation | `/resume`, `/fork`, `/worktree`, `/rename` | Existing capability; fork/background behavior differs |
| Agents and background shells | `/tasks`, `/subagents`, `/ps`, `/stop` | Unified local view; `/agents` remains the native shared-server dashboard |
| Task progress | `/tasks`, native plan events and `task-progress` status item | Inspectable live checklist; not Claude's persistent/shared task backend |
| Review and diff | `/review`, `/diff` | Existing capability; review workflows and diff viewers differ |
| Sign-in and account selection | `codex login`, `codex login status`, `codex logout`, `/accounts`, `CODEX_AUTH_PATH`, `CODEX_HOME` | Shell sign-in and per-account auth files. In aterm, the account picker can resume the same thread on another signed-in account. |
| Usage and status customization | `/alab account`, `/usage`, `/status`, `/statusline` | Inspect selected email, plan, quota and reset times; compact footer groups email, plan and allowances into one account unit that opens it |
| Skills, hooks, MCP, plugins | `/skills`, `/hooks`, `/mcp`, `/plugins` | Existing entry points; not a claim of ecosystem compatibility |
| Voice | `/voice`, `F8`, `F9` | Existing capability; backend availability and controls differ |
| Conversation rewind | Double `Esc` and prompt backtracking | Partial: do not claim code-only checkpoint restoration |
| Renderer and output controls | `/tui`, `/raw`, `/theme` | Existing capability; independent of ALab presentation toggle |
| Prompt stash | `Ctrl+S`, `/stash`, `/alab stash` | Structured per-conversation prompt stash; native search and remaps are preserved |
| Detailed context breakdown | `/status` for native context usage and capacity | Native figures are available; category breakdown remains a gap and the compact context cue remains secondary |

Claude reference checked on 2026-10-20 against its official documentation:
[interactive mode](https://code.claude.com/docs/en/interactive-mode),
[commands](https://code.claude.com/docs/en/commands),
[status line](https://code.claude.com/docs/en/statusline),
[checkpointing](https://code.claude.com/docs/en/checkpointing), and
[fullscreen rendering](https://code.claude.com/docs/en/fullscreen).

ALab-specific code belongs in `codex-rs/alab-tui/`. Upstream files contain only
marked hooks. The ordinary Codex presentation path remains the off-state path,
so upstream behavior and user settings remain the reference for compatibility.

## Launch artwork and compact controls

To use a text header in Enhanced, start Codex with `ALAB_BANNER=off codex`.
The header still shows the release pair, such as `0.11 on Codex 0.169`, and
the composer keeps its Autopilot controls. For future launches, leave
`ALAB_BANNER` unset and set `welcome = false` under `[tui.effects]` in
`config.toml`.

When a narrow pane shows `∞▾`, clicking `∞` changes Cruise to Land. Clicking
`▾` opens the picker with the current mode selected. Choose **Cruise**,
**Land** or **Off**, or press Escape to return to your draft.
[See the compact switch and its picker](design/assets/codex-compact-mode-picker.png).

From the keyboard, submit `/alab menu` to open the picker. To open it during a
turn or while keeping a draft in place, assign **Open Autopilot picker** in
`/keymap`. With the composer empty, press `?` (the default shortcut) and look
under **Session** for **Open Autopilot** and your assigned key or chord.

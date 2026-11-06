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

In Enhanced, Cruise, Land and Off buttons select the session's Autopilot through
aterm's existing owner-only mode control. Cruise continues work; Land finishes
and lands it; Off disables automatic continuation while retaining the enhanced
interface. The active highlight follows the shared Autopilot policy.
Left of the buttons the line wears aterm's Autopilot face: `∞` (cruise),
`∞ ⤓18:00` (cruise, landing at 18:00 local time), `⤓ land`, `✓ landed`,
`⤓ quiet` (a point left to a person) or `○ off`, in aterm's tones and readable on
its ground. Clicking it does what clicking aterm's chip does (land, land now, off,
cruise). Where one of aterm's Settings switches holds the harness off (the
harness itself, or watching codex-alab sessions), or aterm runs no harness host,
the face is `⊘ off` with no buttons, and clicking it says why, and that only
aterm's Settings ▸ Harness turns it on. A right-click or control-click on the face
or a button opens Codex's native Autopilot picker, as `/alab menu` does from the
keyboard. It offers Cruise, Land, Off, timed Cruise presets, a custom deadline
editor, and cancellation of a scheduled landing. From 54 to 60 columns only the glyph
shows, and narrower still none. (Amended 2026-10-29 after the parity sweep, gap
12, and its review: `⊘ off`.) These controls are distinct from the Vanilla/Enhanced choice. At narrow
widths use `/alab cruise [until=HH:MM|+DURATION]`, `/alab land`,
`/alab autopilot-off` and `/alab autopilot`.

The ALab interface builds on Codex's native terminal UI.
The existing rainbow composer overline identifies the codex-alab build (codex-alab's own
version, `0.1.0` on: alab/README.md, Decision 8) and its OpenAI Codex source version
separately, with an on/off button on the right. Clicking the product/version text
opens the native build details and update report. A known pending update adds
`↑ update` when it fits; the mode controls keep their space at narrow widths.
The spectrum continues through the labels, with readable colors on light and dark
backgrounds, so long build labels cannot hide it. Contrast is measured against the
actual composer fill, including any rounding to the terminal's color palette.
The compact footer leads with one account unit: the account email, subscription
plan, weekly usage bar with its reset time and available reset credits, and, where
there is room, the five-hour allowance. Its parts are joined by `/`, and a `│` sets
the unit apart from the workspace fields (activity, mode, branch, model, context),
which keep `·` between them. Wider terminals also show the usage trend and estimated
consumption at reset. Routine Working, Thinking, Ready and Default / Full access
labels are omitted; permissions and task state retain their native behavior.

While the displayed conversation has an active goal, a separate `Goal:` row
shows its objective above the account and quota row. Long objectives shorten
with `…` to fit the terminal; `/goal` opens the full objective and controls.
The row follows the displayed conversation and disappears when its goal is
paused, stopped, completed, cleared, or disabled. It remains visible while the
agent works and between goal turns, including in Plan mode.

An example of the information hierarchy, with illustrative values:

```text
── codex-alab · based on Codex <source> ── ✓ landed ─[Cruise]─[Land]─[Off]───[ALab on] ──
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

When the shared policy supports the session, the launch banner adds a compact
row such as `✦ Autopilot ∞ cruise`. It names the real mode: `∞ cruise`,
`⤓ land`, or `○ off`. With Autopilot off the
banner's trail, wordmark and row wear aterm's white moonbow (white to steel on a dark
background, pearl to graphite on a light one), as aterm's own card does; cruise and
land keep the spectrum. When policy does not support the session, the plain banner
shows without the row; outside aterm nothing changes. There is one codex-alab
product version and no separate harness version. The `[ALab on]` button selects
Vanilla; Cruise, Land and Off select Autopilot policy while Enhanced stays on.
aterm never draws controls over either interface.

Use `/alab account` to inspect the full selected email, subscription plan, weekly
and five-hour allowances, reset times, and the usage snapshot's checked time.
Where the native renderer forwards mouse input, clicking anywhere in the footer's
account unit, the joins between its parts included, opens the same view; the `│`
boundary and the workspace fields are not targets, and dragging footer text retains
native text selection. The details use the connected server's account data and
request a native usage refresh for signed-in ChatGPT accounts. Existing account
and usage updates appear while the view is open; closing it preserves the draft.
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

Reset-credit dots use the backend's available count. Known credits expiring within
24 hours receive a warm highlight and an inline warning. The inspector shows the
known expiry time; missing or stale expiry details are identified explicitly. Reviewing
or redeeming a reset remains an explicit native `/usage` action.
Passing a known expiry removes that credit even after detail metadata ages out;
stale details hide future-expiry warnings without restoring expired credits.
After a known expiry, a fresh count supersedes cached credit details until an updated
detail read identifies the currently available credits.
In short windows, the details scroll with the configured list navigation keys;
the view displays the resolved shortcuts for scrolling and closing.

During a server reconnect, the activity label says `Reconnecting`; a failed
connection says `Offline`. The account inspector keeps that connection label in
its fixed title and identifies the displayed account and allowance as cached.
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
The inspector has no sign-in or saved-account picker. A remote app-server manages
its own credentials.

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

At narrow widths, remove context first, then mode, model, branch and activity. Then
the account unit shrinks as one: the five-hour allowance goes, the email is cut with
`…` to the room beside the plan (never below 8 columns), the plan goes and the email
takes its room back, and then the email goes; the weekly allowance goes last. Full identity remains in the
inspector. At the smallest widths only the weekly field may fit. The build names shorten before dropping either version
number on an 80-column prompt, and the button adapts independently. Use explicit labels and
display-cell-aware truncation. Color supports the text; it never supplies the
only distinction between normal, unavailable, and exhausted allowance.

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
| Sign-in and account selection | `codex login`, `codex login status`, `codex logout`, `CODEX_AUTH_PATH`, `CODEX_HOME` | Shell sign-in, per-account auth files and isolated homes; no in-session `/login` or saved-account picker |
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

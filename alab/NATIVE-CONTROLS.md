# Native codex-alab controls

codex-alab is the agent bundled with aterm. Its Rust TUI owns its entire composer,
including the version, update indicator, Autopilot controls and menus. aterm does
not draw a second product mark, version label, switch, goal or mode over it.

The `/alab` suggestion says whether Enter will turn ALab on or off.
Both editions put `/alab version` immediately after that action so it stays
near the start of a wrapped hint. Enhanced also lists `/alab menu` for Autopilot.

The composer reserves the running ALab release and Codex baseline before fitting
the Autopilot switch. Previously, widening from 51 to 52 columns could hide the
baseline when the padded switch appeared. Measuring each layout against the
actual release text keeps both numbers visible across that resize. A known
pending update adds `↑ update` when it fits. At widths where only the ALab
release and current mode fit, the presentation button stays at `[on]` so
widening the pane preserves the mode's mouse target. At 10 columns, click the
mode glyph to cycle. From 11 columns, the adjacent `▾` opens the picker. When
the short release first fits at 15 columns, the row reads `0.11 ⤓▾[on]` in
Land. The version, mode, arrow and edition each have their own click target.
While the supervisor has yet to report a mode, the compact edition button
makes room for the Codex baseline.
Clicking the version text opens the native build and update report. The report
shows the last update check. When aterm explicitly reports that live session
handoff is unavailable, the update stays pending and the report gives that
reason. An unrecognized reply leaves support unconfirmed. A timeout means
the capability check exhausted its wait budget. The async waits use the time
remaining on the native update deadline so time spent in the queue counts
toward the same limit. Rendering reads cached state.

In the enhanced interface, Windows Terminal's `WT_SESSION` hint respects a
monochrome result from `NO_COLOR` or `TERM=dumb`. The launch card and composer
then use text emphasis for the active state.

Use `/alab menu` to open the Autopilot picker from the keyboard, including in
native scrollback when the terminal handles the mouse. With mouse capture
(the full-screen default), click Cruise, Land or Off to select that mode.
In full screen, pointing at a named segment underlines its name. The brackets
stay on the mode reported by aterm until a selection takes effect.
When the names are hidden, click the mode glyph to cycle modes. If the
adjacent `▾` fits, click it to open the picker. Right-click or Control-click
the status or switch to open the same picker. It offers the three modes, timed
Cruise presets (one, two or four hours) and a custom deadline editor. Choosing Cruise
explicitly clears any scheduled landing. The host protocol always treats that
write as a mode change, even if the timer already fired and the session landed.
The picker therefore offers it once, under its mode name. Escape in the deadline
editor returns to the same picker row. Escape in the picker returns to the draft.
Opening the picker is local and leaves the session policy and draft intact.
For keyboard access during a turn, search for **Open Autopilot picker** in
`/keymap` and assign a shortcut. A single key or a two-key chord opens the picker immediately
and keeps the draft, caret and pending follow-ups in place. While Codex reconnects
to its server and aterm remains connected, you can use the shortcut or enter
`/alab menu` as the entire draft and press your submit key. The shortcut starts
unbound. To set it in `config.toml`, for example, use

```toml
[tui.keymap.chat]
open_autopilot = "leader 3"
```

The example uses your configured leader followed by `3`. Enhanced `/alab help`
shows the physical shortcut. An explicit binding replaces any implicit default
on the same keys across both editions.
Use a primary click to toggle the edition or open build and account details.
`/alab autopilot` reports the current mode and points to `/alab menu` when a mode
can be selected. This keyboard route also works in panes too narrow to show a
mode button. When the harness is disabled, the report points to aterm's Settings.
The deadline editor sends its request directly to aterm. If the Codex server
disconnects while the picker or editor is open, keyboard input stays with that
view. You can finish typing or pasting a deadline and submit it, or press
Escape to return with your conversation draft intact.
`/alab cruise until=HH:MM|+DURATION` also sets a custom deadline.

The picker checks aterm's latest report when you activate a choice or open the
deadline editor. If aterm turns the harness off or stops reporting a selectable
mode while you edit a deadline, Enter returns to the picker with its current
status. The deadline stays unscheduled and the conversation draft is preserved.
This check runs before creating a host request so an unavailable choice cannot
supersede an earlier valid selection.

The switch, picker and slash commands share one writer. If another selection
arrives during aterm authentication, only the newer choice goes to the host.
A request already sent finishes before the next one. A refusal names the
current selection, and a lost reply says that aterm did not confirm it because
the host may already have applied it. An unrecognized reply also leaves the
selection unconfirmed because only an explicit `ERR` establishes refusal.
A late failure for an earlier selection
stays out of the conversation. The highlight follows the mode aterm reports.

An open picker follows the mode and landing time that aterm reports. Before
the first arrow or shortcut, the cursor follows the current mode. After
navigation, the cursor stays on the chosen row while the current-mode marker
updates. This lets a timer expire while someone reads the menu without
redirecting their next Enter key. If the numbered mode and `(current)` would
wrap, a checkmark uses the space before the number. At 11 columns, `✓1. Cruise`
marks the active mode even when `› 2. Land` has keyboard focus.

In Vanilla, an Autopilot mode or timer command explains how to enable the
controls with `/alab on`. This gives keyboard users feedback while preserving
the session's current policy.

Switching between Vanilla and Enhanced retires the previous composer's click
targets. The next rendered row publishes the positions of its version label,
Autopilot controls and ALab switch. This also covers switching off and on between
frames, when the composer may have moved.

A mouse press records the action under the pointer in the last painted frame.
The composer checks it again after refreshing the layout. This keeps a
multiline paste from moving the edition switch under a click on blank space.
The footer uses the same check when a stationary release opens account details.

Each redraw fits the release label and Autopilot controls before painting the
row. This avoids drawing and erasing a provisional release label on every
interactive frame. The version, switch and mode click targets are published
together from that same layout.

The renderer at `c906bbe515` matched all 34,848 layouts and click targets
from `08ffc64a95`.
In seven alternating debug-build runs, 9,000 true-color composer redraws with
Land selected at 24, 35, 48, 60, 80 and 120 columns took a median 0.403 seconds
of user CPU, down from 0.667 seconds.

Rerunning the same layout comparison after the compact-button change restored
the mode glyph and its click targets in 1,296 frames and the full release pair
in 360 frames awaiting a mode. The other 33,192 frames matched byte for byte,
including all 17,424 with an effective width of at least 35 columns.

Account, model, context and activity stay in the existing native status row.
Checklists, agents and terminals stay in `/tasks`; goals use Codex's native goal
lifecycle. Launch art can name the Autopilot mode but does not advertise another
harness or harness version. The plain interface setting remains available.

Cruise leaves a brief opening conversation, such as `sup` followed by
`Hey—what are we working on?`, for the person to answer. This applies only to the
session's first exchange, answered in less than ten minutes, without tool work
or a question box. A service-error retry shares that time budget; if it starts
tool work or opens a question box, Cruise continues. Later interruptions and
completed work still receive Cruise's continuation. Restarting preserves the
opening's tool-work and question-box evidence; missing tool evidence in an older
record does not establish a taskless opening.

When the provider reports a misalignment finding, Cruise and Land stop sending
continuations on that thread. Messages queued before the finding stay in the
queue, and incoming agent messages wait. After reviewing the finding, the
person can explicitly start a queued message or send a new one.

aterm supplies terminal I/O, context reads, shared settings, process recovery and
fleet services. Claude Code and other vendor CLIs retain their separate aterm
harnesses. Tests assert that Codex's terminal cells are unchanged by aterm's footer
pass, including a blank pad, and that native controls preserve their hit targets.


## Fleet model settings

The aterm operator starts a model change with `aterm fleet codex model set
--all --family sol --latest --effort ultra --scope defaults,sessions` and reads
the results with `aterm fleet codex model status <operation-id>`. Each Codex
login selects the newest released Sol that supports Ultra in its own model
catalog. Its launch default follows that family. An explicit model pin or a
later personal edit appears as `overridden` in the receipt.

A running native TUI confirms the operation through its thread settings API.
A turn already in progress finishes with its existing settings. The next turn
on the same thread uses the confirmed model and effort. The person's draft
stays in the composer. A hold, unsupported provider, replaced thread, or
unreachable login has its own outcome. Codex returns settings receipts to
aterm, which owns fleet membership, authorization and delivery.

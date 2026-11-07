# Native codex-alab controls

codex-alab is the agent bundled with aterm. Its Rust TUI owns its entire composer,
including the version, update indicator, Autopilot controls and menus. aterm does
not draw a second product mark, version label, switch, goal or mode over it.

The composer header has one product identity and one running build. It shortens
before the mode controls do. A known pending update adds `↑ update`; clicking the
product/version text opens the native build and update report. The render path
reads cached update state, never the filesystem or network.

Cruise, Land and Off remain adjacent to the current mode. A secondary click there,
or `/alab menu`, opens Codex's native keyboard-accessible picker: the three modes
and timed Cruise choices (one, two or four hours), a custom deadline editor,
and cancellation of a scheduled landing. It uses the existing shared policy service to apply the
choice. Opening a menu does not change policy, submit a prompt or require a host
window. Escape returns to the draft. `/alab cruise until=HH:MM|+DURATION` retains
custom deadlines.

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

aterm supplies terminal I/O, context reads, shared settings, process recovery and
fleet services. Claude Code and other vendor CLIs retain their separate aterm
harnesses. Tests assert that Codex's terminal cells are unchanged by aterm's footer
pass, including a blank pad, and that native controls preserve their hit targets.

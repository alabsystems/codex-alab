<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Original editor IO staging

The original App Job now retains the same completed spawn and actual stopped
input/parser and mode Files in a cancellation-safe C0 transfer. The Job publishes
capture ownership before its first await, parks normal input/backend queues while
held, and refuses overlapping same-process cuts. No descriptor is duplicated or
reopened by staging. Independent bounded registries retain actual resources even
when external observers drop. A local pre-offer rollback restores the same slots;
all domains become offered before a Native request, after which local rollback
refuses. Partial failures keep the original resources and error.

The active editor mode snapshot checks actual original owner, phase, Files and
operation fences. A generic mode setter refused by the actual editor can revoke
an idle preparation epoch without changing that editor; this does not invalidate
the active owner's snapshot. Both irreversible committed states still refuse.
Pre-child preparation checks remain strict. The real mode/PTY control exposed
this phase distinction; its failed aggregate is retained as nonpassing.

On base 9af63af1236fee13cb6c4144ee48278c8abbdee4, Source-selected Trust 15698
through the qualified protected project controller ran the explicitly unverified
15-file patch 2da6f5f827a01bd3f76b6111f34322e2e13ffc2dc99779591489b9db49ea01ac.
All 1,129 selected methods across thirteen libraries pass, with zero failures
and three existing ignores. All prior 1,126 passing identities remain. Three
new lower controls exercise the real File/parser rollback, whole generic Broker
retention after observer drop, and blocked offered rollback. Existing actual App
and mode/Child controls exercise this staging path. Runtime and Tippy both exit
zero. Runtime log SHA256:
73d1e18c3c02f200e81d0723b507aff39febab2b8c24bf94d066824e7222d7d0.

The first aggregate failed compilation on two child-private helper methods;
the corrected parent visibility is included. The second aggregate failed the
actual mode/PTY method on the idle/active distinction above. Those records are
not credited as passes. The final runner explicitly selects broker::editor::staged;
the former editor_input selector did not select these three new lower methods.

Tippy classification reports 1,016 owned diagnostics, zero spanning,
failing-target or missing-script findings, and twenty upstream findings set
aside. This is not lint clean or the shipping gate. Native consumers remain
unfinished; test success does not make these unused production paths live.

No authenticated AcceptedC0Io constructor or physical original-IO close is
introduced. Standard IO and writer aliases need separate authenticated
retirement. Genuine Native helper/C2 ownership and foreground admission, future
read reservation, production update observation, complete App/Pane/frame
assembly, signed repeated-update end-to-end checks and certified delivery are
still required. The live-update goal remains incomplete.

<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# codex-alab native harness: the contract between codex-alab and aterm (v2)

## Current operating rule — 2026-11-01

ALab decides, records the decision and its reason, and continues autonomously. Later AIs
can revisit that decision. Automatic reports are journal entries, never alerts, nags or
routine requests for the owner to act. Unavailable credentials, permissions, services and
machines defer that operation; the agent advances other useful work. Explicit human holds
and permission boundaries remain authoritative. Repeated unproductive replies in Cruise
receive a paced request to change approach, subject to the existing hourly budget.

This replaces the historical requests below to be loud or emit `NEEDS YOU` lines, and their
ordinary-reply stopping rule. The sync worker records failures in its journal. An older
aterm's missing capability is recorded without an upgrade nag. Prefer the strongest
architecture and delete obsolete behavior; compatibility and expedient stopgaps require
an explicit reason.

v2 folds in two adversarial reviews (codex side, aterm side), and the appendix records the wire each
side implemented. aterm's half lives in aterm (`crates/aterm-gui` self-claim and fence, the watch-only worker and
crash rule in `crates/aterm-agent`, `harness policy`, OSC 7501 in `crates/aterm-core`); codex-alab's in
`codex-rs/alab-core`, `alab-aterm`, `alab-autopilot` and `alab-tui`.

Owner's goal (2026-10-16): "optimize codex-alab so that it has a similar feature set as we have for the claude code
harness and retire the codex harness ... more deeply integrate into aterm and introspection". Owner, same day:
"you decide". Decided: codex-alab does natively what aterm's screen-scraping Codex lane did, and reports exactly to
aterm. aterm's Codex screen lane is deleted (2026-10-16, §9 step 4; aterm main `28e13d46ba..83bd43c206`). aterm admits
a Codex only as codex-alab, and watches beside it.

Owner's ruling (2026-10-19, aterm): "we are deprecating the ATERM-TASK-DONE logic and prompts in favor of FOREVER
CRUISE or FOREVER LAND"; "Cruise is always looking for more completeness and perfection. Land is trying to wrap it
up to end the session cleanly"; "if landing LAND EVERYTHING INTO REMOTE MAIN NO EXCEPTIONS"; "if you need something
from the user, be LOUDER ABOUT IT ... but NEVER let that block you"; "we need this functionality in ALL HARNESSES".
codex-alab follows it natively: §3 (the session's Autopilot word, aterm's composed texts), §4 (`carriedOn`,
`owed`), §5 (carry-on, Cruise, Land, NEEDS YOU, back-off, left boxes, live upgrade, visibility). The readers and
texts are ported from aterm `a355109905` (`crates/aterm-agent/src/supervise/policy/needs.rs`, `turn_end.rs`), the
done and Land readers (`says_done`, `open_work`, `open_work_strict`, `clean_defense`, `land_open_words`) as aterm
main `e0da8d68df` has them (Land's strict reading by word family, measured on aterm's labelled defense table, with the
ninth review's holes closed, the tenth and eleventh reviews' read narrow and the integration review's leftovers read
open: `78d1055bd9`, `55edc289b5`, `0e7ef0d916`), the NEEDS YOU reader as aterm main `e0da8d68df` has it (rebuilt
on its labelled needs table, failing toward a kept need: `1da587a559`, `0469b355be`), both measured on aterm's
tables (`fixtures/land-defense-table.txt`, `fixtures/needs-table.txt`), the repository probe from aterm main
`e0da8d68df`'s `crates/aterm-agent/src/harness/repo_work.rs` (from 4bff4848b4: HOME set on every git read, the
person's global excludes read in the repository, a branch's own stashes, the work's state) with the spec's
`unlanded` count, the landing's rules as aterm main `e0da8d68df` has them (a person's act re-arms Land, nobody's act
keeps the landing: 667c926ec4; Land's own words at a wall's resume, a landing through a wall, the reply read as the
whole turn), and the texts verbatim from the ruling's spec. All of it was then diffed word for word against aterm
main `6ff0b5475a` (amended 2026-10-20): the texts (`DONE_CHECK`, `DONE_CHECK_REVERSIBLE`, `LAND`, `LAND_NUDGE`,
`LAND_ANSWER`, `NEEDS_HINT`, `NEEDS_ACK`, `CARRY_ON`), the phrase lists, the three tables and the readers were the
same, and what differed is ported: the fenced blocks every reader skips (`96bd2d4567`), the none clause's UTF-8
offsets (`cee3ea7afb`), a force-push however spelled (`a9f89afaa0`, `1548dac3bf`), the probe's main ref and its
unlanded count as given, the repository suffix cut to 160 units with its bare form, every act's suffixes in aterm's
order before its one copy of the standing rules, fitted to 800 UTF-16 units (`fitted_act`), the repository named in a
Land done claim's nudge, `says_finished` retired, and Land's no-repository sentence in `LAND` and `LAND_NUDGE` (aterm
`2b871cf8a1`, `72aa7380fe`; amended 2026-10-24). The readers were then diffed item by item against aterm main
`66894933d2` (amended 2026-10-24): Land's strict reading takes the readings that closed the misses of a blind holdout
(`4cacfb7913`, `ac0c76b137`, `149efe1190`), with that holdout (`fixtures/land-holdout-2026-10-20.tsv`), and the NEEDS
YOU reader takes a token a line's start names as no need (`2a290f10f7`) and one need a line (`93de0197f9`); the tables'
bodies are main's byte for byte. aterm `e32ba59da2`'s unread repository result is also ported: a deadline or interrupted/failed Git read is unknown, never clean or unchanged. Land continues with the unread-probe nudge and reads again at the next claim; a verified clean or non-repository result retains its existing behavior. Guarded and nonlocal folders remain skipped without entering them. codex-alab reads the six text keys as aterm's
`harness_policy_verb.rs` spells them (`done_check_text`, `land_text`, `land_nudge_text`, `land_answer_text` composed
with the rules; `needs_hint_text`, `needs_ack_text` bare) and records the act words aterm's `harness/acts.rs` maps
(`continue-done-check@v1`, `land@v1`, `land-nudge@v1`, `land-answer@v1`, `landed@v1`).

## 0. Laws

- aterm's control socket is its one interface (fabric law 1). codex-alab speaks aterm's existing control-socket
  wire (the protocol `aterm ctl` speaks; read `crates/aterm-ctl` and `aterm_types::control_socket`) from the
  UNSANDBOXED TUI process itself, on ONE close-on-exec connection it owns. The model's tools never get it (law 2).
  codex-alab's TUI process, which is never sandboxed, may read its own session's mail when typed to and send its
  receipt and report (§5 "inbox"); the model's tools still never get the socket.
- No environment-variable knobs (R2); no new env channels (no ALAB_* exec variables). aterm writes nothing into
  `CODEX_HOME` (decision B). codex-alab's compiled code is the agent itself, not an install.
- Full power by default; configuration only limits. A person's hand wins. Never "don't ask again", never a standing
  grant, never a purchase. Never type screen text back as a user turn.
- Fail closed: a side that cannot prove its precondition does nothing; with no proven claim, nothing types into a
  Codex.
  - A vendor `codex` is display only: its 7501 report, the stall row and the disk tick.
  - An ALab build, or a codex-alab whose claim is unproven or released, keeps aterm's watch-only worker, which then
    watches only: the crash relaunch, the stall remedy and the memory remedy each need a standing proven claim (§4),
    so none runs, and nothing relaunches it. This is the plan's D-2 default, accepted as fail-closed. A released
    codex-alab whose record showed no end at the release is also read, never acted on, while the aterm that saw the
    release runs: aterm keeps a read-only copy of its record and taps its rollout and reads its record for survey
    facts, goal, background terminals and session-log turns while that process leads the tab's foreground group,
    alive as the start its claim proved (amended 2026-10-28, gap 15, §4; background terminals 2026-10-29, gap 28).
    codex-alab keeps its claim through a transient off and gives it back at a person's or the config's off, under
    which aterm's host runs no worker and no remedy anyway (§2). The transient
    offs: a hold, under which aterm's host stands beside the session (its remedies are real); a suspension, under
    which the host is off, but which aterm makes at every seamless-handoff Commit, where the successor adopts the
    standing claim and a Commit that fails resumes this host with it; and a policy read that failed, which says
    nothing about the host.
- Nothing polls on a clock where an event exists (AUDIT rule 7): the claim is connection-bound and changes arrive
  on `subscribe @self`. Two clocks: while passive, codex-alab reads `harness policy` every 30 s, because no event
  announces a suspension and a session may start off; never while what it waits for has an event of its own
  (another holder's claim, a claim someone cleared, one aterm did not prove) (§2). And while it acts and its last
  `status` read named no acting mode (`-`, unreadable, or Off pending reconciliation), it reads `status` again
  from 1 s, doubling to 30 s, until an acting word or passive policy settles it. aterm's admission of the session's
  codex-alab moves the word with no event (§3, added 2026-10-23); a quiet Off likewise reconciles the full policy.

### 0.1 Supersession (record in the docs below in the same aterm change that ships §2)

Owner 2026-10-16 ("give codex-alab the Claude harness's feature set and retire the codex harness"), on owner
2026-09-17 ("For codex, we DO have the source code, and it may be better to simply compile our own codex"): for an
agent ALab builds from source, the agent's own source is part of aterm's layer. Law 3 ("never installed into the
agent") and AUDIT rule 7 ("one engine") keep holding for every vendor binary. Record in: FABRIC-LITERALLY
(addendum), AUDIT-claude-harness §0 rules 1 and 7, §3 Codex clauses and ruling 17, DESIGN-aterm-wrapper-2026-09-17
§6.3 and §10 P3′, RFC-embedded-agents-2026-08-15 (retirement reversed), DESIGN-atpkg-vendor-direct-updates (the
codex row, when it changes), DESIGN-harness-never-block (questions), and codex-alab alab/README owner decision 1.

## 1. Identity and the proven self-claim

- codex-alab's binary is named `codex`. aterm admits a Codex as codex-alab when its claim is a PROVEN self-claim,
  when aterm holds its copy of that very process's record, or when an ALab build leads the tab
  (`self_claim::runs_alab_build`, read by the host's roster pass and again when the leader execs, aterm e77d26cda1).
  Each admitted session gets the watch-only worker; only a proven claim buys the remedies (§4). Any other `codex` is a
  vendor one: display only. The holder string (`codex-alab@<pid>`) proves nothing by itself.
- aterm stores `proven_pid` on a claim only when: the `meta set supervisor codex-alab@<pid>` request arrived on a
  direct local connection (never a dial, `@child` proxy or bridge), the kernel peer pid of that connection
  (`aterm_uds::fdpass::peer_pid`, read right after the request line) EQUALS `<pid>`, and `<pid>` leads the
  session's foreground process group. A descendant proves nothing. `status` and `meta` print
  `supervisor=<holder> supervisor_proven=<0|1>`. An unproven `codex-alab@…` claim is an ordinary foreign claim.

## 2. The claim

- **When codex-alab claims:** only after (a) the TUI registered its primary thread (never during onboarding or
  login, which a person answers (no aterm lane remains), nor at the folder gate, which the TUI hook answers first
  without a claim, §5), (b) `harness policy` answered `OK` (proof of a new-enough aterm) with `effective=on`. Never
  otherwise. A signed-out first launch's sign-in screen, before any claim, raises codex-alab's keyed attention
  (`meta set attention owner=codex-alab login: …`, at most 200 B, on a connection of its own) so a person is told,
  and clears it as soon as the sign-in completes (the onboarding screen may go on, to the folder-trust step, with the
  person signed in), or when the onboarding screen ends without it (the person gone); outside aterm, an older aterm
  or no answer raises nothing (`alab-tui/src/login.rs`, the `ALAB(login)` hook around upstream's onboarding run into
  `alab-tui/glue/login_screen.rs`; PLAN-v2 D-5, added 2026-10-17). A screen that never reaches its clear (a SIGTERM,
  a SIGKILL, a panic, a restore on the sign-in screen) leaves it standing: the next start past the sign-in clears it
  through one startup cleanup lifetime, begun at the supervisor's first connection, and only codex-alab's own entry
  while aterm shows it (`attention_owner=codex-alab`) reading exactly the sign-in screen's text (`login: signed out;
  codex's sign-in screen waits for a person`, `waiting::stale`; amended 2026-10-17 after the review of
  4cdc96090, finding 6: the attention outlived the sign-in through the folder-trust step, and for good after a kill;
  amended again after the review of fea034d20, finding 4: a `login: …` prefix also took the autopilot's own sign-in
  attentions, still true, and a reconnect could clear one this process had just raised).
  The other startup screens a person answers before any thread raise it the same way (amended 2026-10-23, the
  startup-screen map: aterm reads no Codex screen, and its 120 s startup stall names none): a folder-trust screen
  the TUI hook left (§5 folder trust: `trust: <folder> is not trusted yet; codex's folder-trust screen waits for a
  person (<why it was left>)`, the `ALAB(trust)` hook around the screen's run), a forced move off a retired model
  (`migration: <from> is retired and codex moves to <to>; codex's model-migration screen waits for a person`; an
  optional one is skipped, `ALAB(no-upsell)`) and the hooks review (`hooks: <n> hooks for <folder> need review;
  codex's hooks-review screen waits for a person`), both `ALAB(startup-wait)`. Every one is at most 200 B whole (a
  folder keeps its tail, a reason its head), raised beside the screen on a connection of its own, cleared when the
  screen ends, and inert once any thread was registered in this process (a screen after one, such as a task opened
  from the agents overview with the primary detached, comes under the autopilot, whose keyed attention it would
  replace and then clear; review of d5e2fc5abf, finding 1). The
  stale cleanup reads each of these texts as the sign-in's (`waiting::stale`): codex-alab's own visible entry
  with its screen kind's complete fixed footer, and only trust permits a parenthesized reason
  (`waiting::is_screen_text`). Footer-like words inside folder/model/reason data remain data. A failed read or
  clear keeps cleanup pending on its own positive retry deadline, independent of the policy/claim clocks, even
  under initial Off, unproven claims or unsupported policy. Another owner's visible entry or incomplete metadata
  (empty or whitespace-only decoded attention/holder tokens included)
  defers it without writing; an own non-screen replacement or explicit empty entry retires it. The first proven
  claim/publication, or any observed foreign holder (watch history recorded before its queue, a Busy response or
  fresh metadata), permanently retires startup authority across reconnects. Every attempt is serialized with
  current publications and rechecks custody after its fresh read. The protocol provides no actor identity or
  compare-and-clear for independent unclaimed writers using the same owner, so it cannot distinguish their
  identical startup text or protect a cross-process post-read replacement. No new acting authority is granted.
- **How:** `@self meta set supervisor codex-alab@<pid>` with NO `ttl=` on codex-alab's own connection: the claim
  lasts while that connection serves requests and ends when the process dies. `@self` resolves from
  `ATERM_PARENT_SESSION_ID`.
- Every newly proven claim reconciles codex-alab's keyed attention with its current native point, including an
  explicit clear when no attention is due. A predecessor's alert can outlive its connection; an empty local
  cache does not prove the remote entry is empty. Only a successful write settles that claim's reconciliation,
  and another owner's keyed entry is untouched.
- **aterm (replacement):** a proven self-claim REPLACES a live `aterm-harness@…` claim, and a proven claim whose
  `proven_pid` is gone. The host's interrupter cuts the replaced loop's connection at once
  (`HostCtl::interrupter`), and **aterm adds `if-supervisor=<holder>`** to `send`/`key`/`paste`/`turn`, checked
  under the meta lock; the hosted loop fences every write with its own holder, so no host keystroke lands after a
  replacement. Any other replacement keeps today's `ERR busy supervisor=<holder>`.
- **aterm (clear):** the server clears a proven claim when its `proven_pid` exits or leaves the foreground group
  (`meta-change field=supervisor value=-`).
- **codex-alab (an off, met by its kind):** decided 2026-10-17 (the review of 8e834c8f6..4cdc96090), replacing
  b24e535df, whose premise ("keeping the claim while passive keeps aterm's crash relaunch, stall and memory remedies")
  is false for aterm's own off reasons: aterm main's `harness_host` runs no worker and relaunches nothing under
  `agent-off` and Autopilot Off (`visit_workers`, `OffSessions`), turns its host off under `disabled`, `headless` and
  a suspension (`Shared::active`), and has no host when `unhosted`. A kept claim bought nothing there and had side
  effects: `status` kept `supervisor_proven=1` under Autopilot Off, so `aterm drive task` typed `Inbox: task @<off>`
  into a hands-off session (a command aterm has since retired: b62f087cd7, `supervise/mail.rs` deleted in
  5c2262a231); another supervisor met `ERR busy`; mail receipts and reports ran while passive. So:
  - TRANSIENT offs keep the proven claim and act on nothing: a person's `hold` (as always: every act waits on
    `status hold=`, and the `effective=on` read that finds the hold lifted, at its `hold` event or on the passive
    clock, has every thread look again at once, as aterm's loop goes at the hold's edge (`park_on_hold`), with the
    act's own recheck only the backstop: amended 2026-10-21 after the parity review's P1, where a
    continuation due under a hold waited out that recheck, about 50 s after the lift. A live upgrade whose
    before-every-act checks met the hold looks again at that lift too, as the threads do (it watches the claims
    from before its checks); and an act or upgrade the hold stopped reads `status` again 20 s later where no lift
    reached it (a `hold 0` event lost with no `GAP`), aterm's own `WAIT_STEP`: aterm's `park_on_hold` meets a lift
    between its `status` read and its `await inbox kinds=hold` arm (which latches only on a change from the level
    it armed at) one step later, so codex-alab's worst case is aterm's, never the minute every other wait with no
    deadline takes (amended 2026-10-21 after the parity review, R2: the upgrade's relook waited out its own minute
    after a lift, and a lift whose event never came cost the acts a minute); attention, the description
    and mail stand, and so does an automatic wait's `owner=codex-alab-wait` entry, its act waiting on the hold
    (amended 2026-10-27 after the parity sweep, gap 2); the policy it keeps reads the session's Autopilot
    word, never Cruise by default, so a look during it leaves a Land episode or a landing standing: amended
    2026-10-19 after the review of 2a9241bbf, findings 3, 13 and 17; a word that moves while the hold stands has
    every thread look again at once, as a claim transition does, so a landing switched to Cruise goes on while its
    act still waits on the hold, and a hold read while `status` fails keeps the word read last: amended 2026-10-20
    after the review of the forever autopilot, finding 1, where the hold stored the new word and no thread looked
    again, before or after the hold lifted; an `off` read under the hold, which aterm names after the hold
    (`why=hold` while `status autopilot=off`), is no word and moves nothing: the word in force is kept, no thread
    looks again, and a landing stands, through the release that parks the threads when the hold lifts, as it does
    through an Off with no hold:
    amended 2026-10-20 after the ninth review of the strict reading, finding 22, where that Off read as Cruise
    ended the landing and Land picked again typed the land instruction into a landed task), `why=suspended` (aterm
    suspends its host at
    every seamless-handoff Commit: a successful one exits this aterm and its successor adopts the standing claim, a
    failed one resumes this host with it; §0), and a policy read that failed (an error, or a reply the client cannot
    take from the aterm that proved the claim). Under a suspension or a failed read codex-alab is on standby: its
    attention is cleared (its own `owner=codex-alab` entry: a worker's NEEDS YOU under `owner=needs` stays up, §5
    NEEDS YOU; amended 2026-10-19; and its `owner=codex-alab-wait` entry, nothing typing when a wait ends: amended 2026-10-27 after the parity sweep, gap 2),
    its description withdrawn, a box answer the TUI has not applied taken back (one the TUI
    already took keeps its record until its outcome comes, which is recorded once codex-alab acts again), and
    nothing acts. Mail typed into the session is still read (an `Inbox: task @<off>` line left unread would reach
    the model bare, with no fence and no receipt; aterm's `drive task` typed one whenever it showed the claim proven,
    until aterm retired it, b62f087cd7 and 5c2262a231, and a person may still type one; amended 2026-10-19: this
    said aterm types it), and a mail turn that ends there records its verdict, receipt and report as owed; they are sent once
    codex-alab acts again. The record still names the claim, which is still renewed when the resume line moves. A
    hold read after a standby lifts the standby: the hold's own rule is the one left.
  - A PERSON'S or the CONFIG'S off gives the claim back (`meta unset supervisor holder=codex-alab@<pid>`, as before
    b24e535df): `autopilot-off`, `agent-off`, `disabled`, `headless`, `unhosted`, and any `why=` word codex-alab does
    not know. codex-alab is passive the moment it decides so, before the release is answered (an act whose checks
    run meanwhile meets no claim held), and its attention is cleared, every thread's automatic wait
    (`owner=codex-alab-wait`; amended 2026-10-27 after the parity sweep, gap 2) and a NEEDS YOU too (the person drove the session;
    amended 2026-10-19; each thread's own memory of that need and of its Land episode ends with it, so a later
    report that names none raises nothing again, and a landing is kept: amended after the review of 2a9241bbf,
    findings 4 and 12), and its description withdrawn. The release's
    local dismissal and Off epoch settle before its remote release or alert cleanup can wait;
    a native report completing after its memory parks at that Off may name fresh needs, which
    remain wanted for the next claim even while the old entry's cleanup is still in flight
    (amended 2026-10-22 after the released-report race). The release's
    own `value=-` echo is no clear: one that reaches codex-alab after it claimed again (a fast off and on, the word's
    event ahead of the echo) stands only while `status` still shows the claim proven and its own and no re-confirm
    went since that claim (a same-holder set takes back a claim someone just cleared, and `status` then shows it ours
    too), and only a release of a claim aterm showed its own is counted as owing an echo (one of a claim already gone,
    or one that failed, owes none); a re-confirm straddled by an epoch move that a release of its own may explain
    gives the claim back without counting it someone else's clear. A claim someone cleared stays given up whichever of
    the watch and an act's checks (`status` showing nobody holding it) meets the clear first, and a `value=-` met while
    unclaimed takes nothing unless it ends another holder's claim. An act's checks go passive before any await, so a
    caller that cancels them (the approval path's 1 s) leaves no half-done give-back (amended 2026-10-17 after the
    review of 7e7622ff7, findings 1-3).
  - codex-alab's own passive reasons never give the claim back: a point it cannot answer raises attention under the
    claim (aterm's host is on there, so its remedies are real), and a wedged aterm exchange ends the connection, and
    with it the connection-bound claim, by aterm's own rule; the reconnect claims again.
  - While passive for any reason codex-alab keeps watching on its own connection, and claims (or acts) again, by the
    ordinary proven claim, as soon as the policy reads `effective=on`. A connection that has served any supported
    policy keeps treating later refused or malformed policy replies as transient, including before its first claim
    or after Off released it; the existing passive clock retries without acting. An initially unsupported host is
    not claimed or polled, but an explicit event's supported reply retires that unsupported latch. Reconnecting
    forgets this connection's support history. It reads the policy again at a
    `harness-policy` event; at a `hold` event and at a `meta` event of any field but the claim and the session's words
    (`attention`, `description`, `title`, `icon`, `role`), since aterm folds a hold and an Autopilot word into
    `effective=` without a `harness-policy` event; after lost events (`GAP`) while its claim stands; and every 30 s
    while passive (released, on standby, under a hold, or never claimed), since aterm announces a suspension with no
    event at all (`HostHandle::suspend` and `resume` send none) and a session may start with `effective=off`. That
    clock never runs while codex-alab acts with no hold standing, so a suspension that begins then is seen at the next
    read an event or a reconnect makes; nor, unclaimed, while what it waits for has an event of its own: another
    holder's claim (`ERR busy`, or the watch saw the claim go to it; its release is a `meta-change field=supervisor
    value=-`), a claim someone cleared, or one aterm did not prove while the policy still reads that generation (both
    wait for the policy to move; a move read during a suspension turns the clock back on, and a new connection forgets
    an unproven generation, review of 7e7622ff7, finding 4). Acting resumes `human_grace_s` later, a point whose
    attention the passive spell cleared, standing still (no turn since), raises it again, and the root thread's
    description is written again (finding 8).
  - Mail receipts and reports are SENT, and description writes go, only while codex-alab acts (the claim held and not
    on standby), never merely while it is held. A mail turn's verdict and its owed receipt and report are recorded at
    its end whatever codex-alab's state, and sent once it acts (at once, else at the resume or a new claim; amended
    2026-10-17 after the review of fea034d20, finding 1: a turn that ended passive lost its receipt for good, and every
    later receipt waited behind its row). Whether codex-alab acts is read under the lock that takes each receipt or
    report out of the owed set, and a send that fails (not refused) leaves what it did not send owed, tried again 15 s
    later while codex-alab acts (amended after the review of 7e7622ff7, findings 5 and 7; a refused report is dropped,
    noted). `harness_act` records go while the claim is held, a standby included: an
    act passed its checks while codex-alab acted, so one a standby came after still went (finding 5). A hold halts
    only the acts, and keeps these as before (aterm takes `post`, `inbox seen` and `meta set` under a hold). It also gives the claim back at an orderly end (§4
    removal, while the process still lives) and when aterm no longer shows it proven, and never unsets another
    holder's claim. No aterm lane answers its points either way. (History: amended 2026-10-16 after the end-to-end
    run, a hold kept the claim; amended 2026-10-17 by b24e535df, every passive reason kept it; decided again
    2026-10-17 as above, and amended the same day after the review of fea034d20 (findings 1-9): `alab-autopilot/src/supervisor.rs` `off_kind`, tested with negative controls in
    `alab-autopilot/tests/claim.rs` and `alab-autopilot/src/passive_tests.rs`.)
- **codex-alab (watch):** it keeps `subscribe @self` on a SECOND connection (a connection that turns into a
  subscribe stream ends a connection-bound claim, `control.rs:6100-6145`; the claim stays on the first) and goes passive on any
  `meta-change field=supervisor` not naming it. It re-confirms the claim (same-holder `meta set` answered `OK`) in
  the same tick as each act. After any claim transition its first act waits `human_grace_s`.
- **Seamless update of aterm:** codex-alab reconnects (re-resolving the socket via the graph entry; `ERR busy …
  retry_ms=` is a wait) and re-proves within `ADOPTED_CLAIM_GRACE`; until then the successor only watches; an
  unproven carried self-claim is released at the end of the grace.
  Before returning to reconnect after subscription loss or refusal, codex-alab waits for
  its retiring request connection to finish closing, not only for the close notification.
  Every new proven claim reconciles both attention owners even when the wanted text
  equals the locally cached text: an unanswered clear may already have reached aterm.
  Auxiliary description clients also finish closing under their publication guard,
  before that guard admits a replacement publisher or reconciles a newer task.
  A quota need prepared across awaits is inserted only while its original native
  point, registration, policy and Off epoch still own it. Native acceptance and
  completion share the registration fence; policy and Off are checked under the same
  supervisor lock that inserts the need. The asynchronous writer reads the latest
  desired set after these synchronous guards have been released.

## 3. Policy: aterm's `[harness]`, one reader

- **aterm (verb):** `@self harness policy` (Read-class, Scoped). Reply:
  `OK gen=<cfg_gen> effective=<on|off> why=<word> <key>=<pct-encoded value>…` for every `[harness]` key (from the
  live `HarnessPolicy` via `SupervisorConfig`'s own spelling), with `continue_text=` already composed with the
  `rules_file` text, `questions=` already resolved (session `meta questions` over `answer_questions` /
  `question_priority`), and `effective`/`why` computed by aterm for THIS session (`enabled`, `headless` against the
  instance, host suspended for a handoff, `hold`). An edge token reads only `effective=`.
- **aterm (event):** `harness-policy gen=<n>` on `subscribe @self` when the `HarnessPolicy` cell moves; codex-alab
  re-reads on it. No timer while codex-alab acts; while passive it reads every 30 s as well (§2).
- codex-alab acts only on `effective=on` (and meets an `effective=off` by its kind, §2), never parses aterm.toml, and treats `approve=safe` as `none` (it has no
  proofs) for every box but the folder-trust screen, where `safe` is aterm's own trust-root rule (§5). Keys it
  honours: `continue`, `continue_text`, `continue_per_hour`, `approve`, `trust_roots`, `answer_questions`,
  `answer_text`, `questions`, `retry_api_errors`, `probe_api`, `resume_limits`, `limit_wait`,
  `compact_on_context_wall`, `relaunch`, `upgrade`, `upgrade_ask`, `human_grace_s`, `facts`. `stall_term*` and `memory_ceiling` stay aterm's (§4
  watch-only worker). `facts` is aterm's `[harness] facts`: a limit, and off (or absent, as an aterm before the key
  sends the policy) means codex-alab reads no fact (§5 "aterm fact"). `occupancy` and `stuck_break_note` stay aterm's:
  their lines are typed at a background break, where aterm's Cruise types nothing, and a codex-alab session's idles at
  which it types nothing are not served (§5 "aterm fact"). (Amended 2026-11-04 after the parity sweep, gap 23.)
- **The session's Autopilot word** is no policy key: codex-alab reads it from `@self status autopilot=<off|cruise|land>`
  (the session's own word, never the table default the policy's `autopilot=` key carries) whenever it reads the
  policy, so again at the `meta` event that moves it (§2), and at every act's checks (§5). A status with no
  `autopilot=` (an older aterm), `-` (aterm prints it until it admits the codex-alab in the session: at the
  proven claim since aterm 1bdebfa2e6, at its status sweep before; amended 2026-10-24) or
  a word it does not know names no word: the word in force is kept (the stored policy's, else the word read last,
  else Cruise). While codex-alab acts and the last read named no word, it reads `status` again 1 s later, doubling to
  30 s, until a read names one; then nothing polls the word. `off` never reaches the decider (aterm answers
  `effective=off why=autopilot-off`). (Amended 2026-10-23 after aterm's `spawn agent=codex` lab leg: a worker's claim
  at launch read `-` as Cruise before aterm's sweep admitted it, and the worker typed `continue@v1` for ten minutes in
  a Land session; `alab-autopilot/src/supervisor.rs` `autopilot_word`, `ready_for` and `word_recheck_at`, tested with
  negative controls in `passive_tests.rs`.) A move makes the threads look again: Cruise → Land starts a Land episode,
  its instruction first; Land → Cruise reopens a landed task. It also reads aterm's own texts where the reply
  carries them, composed as aterm composes them: `done_check_text`, `land_text`, `land_nudge_text`,
  `land_answer_text`, `needs_hint_text`, `needs_ack_text` (pct-encoded; missing or blank: codex-alab types its
  verbatim copy of aterm's words, `alab-autopilot/src/words.rs` and `needs.rs`). (Added 2026-10-19: owner's FOREVER
  CRUISE / FOREVER LAND ruling, "we need this functionality in ALL HARNESSES"; codex-alab knew only Off before.)
  The same reads take `land_at=<unix s>`, aterm's timed Cruise (`-`, `0`, a missing field (an aterm before its timers)
  or a value it cannot read: none; a read that fails keeps the last), which codex-alab's line shows as `∞ ⤓HH:MM` in
  local time (amended 2026-10-29 after the parity sweep, gap 12: a codex tab showed no timer, as codex-alab read only
  the word; `alab-aterm/src/status.rs` `Status::land_at`, `alab-autopilot/src/task_face_tests.rs`). The session
  loop's reads take it, one at a time: the policy's read at a `meta` event or on the passive clock, the word's clock
  and a hold's read while no claim is held; an act's own checks read the word and take no timer, as their reply,
  taken on the thread's task, can land after a newer read's (amended 2026-10-29 after the review of gap 12, findings
  6 and 15: `Supervisor::status_with_timer`).

## 4. The session record and crash relaunch

- **Persistence scope:** these custody rules cover process death and re-exec while the OS remains running.
  Here, a "durable" handoff means a successfully written snapshot outlives that process. Atomic writes sync the
  temporary file and rename it without flushing the parent directory. Persistence across an OS crash or power
  loss is not guaranteed: recovery may find an older snapshot or none.
- Path `$CODEX_HOME/alab/sessions/<pid>.json`; dir 0700, file 0600; written by a background task, coalesced to at
  most one write per second (`<pid>.json.<nonce>.tmp` then rename); never on the render or input path. These changes
  are written THROUGH, never coalesced: leaving rest (`idle` to anything else) and a person's first keystroke
  (`draftEmpty` to `false`), because aterm may end the process at the record's rest (watch-only worker, below); the
  act `owed` moving (decided, typed or dropped; the same act re-timed alone, a deadline read again through another
  clock, waits for the next write) and the `pace` moving (an act typed, a wall's rung climbed, the act a wall cut),
  because a crash inside the coalescing second lost them; and the write that commits a held predecessor's
  replacement (below). A codex-alab act writes its
  pace before its turn begins, so the turn start's write carries the try's rung and its owed answer (amended
  2026-10-22, CX-B B1 and B3: an owed act and a pace reached the record only at the next coalesced write, the pace
  after the act's turn had begun, as aterm writes its books at each act). Every
  turn's start hook (a person's turn or codex-alab's) and every codex-alab act (submit, compact, goal resume) writes
  the record `busy` itself BEFORE it proceeds (a local write off the async runtime, bounded 1 s; an act whose record
  did not take `busy` waits and looks again). An act that only queues its turn (a compaction, a goal resumed) keeps
  the record `busy` past its own return, until that turn's start hook counts the turn running, the act fails, the
  thread stops, or 30 s pass (amended 2026-10-16 after the review of 6efbf7518: the writer could write `idle` in
  the gap).
- Written only while `ATERM_PARENT_SESSION_ID` is set. At start codex-alab sweeps the dead-identity records (any
  pid) older than its own start: aterm never needs the file after an exit (it keeps its own copy, below). It takes
  the one whose `resumeArgv` is exactly its own argv (its own predecessor, the crash witness, §5 carry-on), leaves
  every other one with a `resumeArgv` written within 24 h whose end aterm's crash rule may still relaunch from its
  copy (`exit` absent, `signal:15` or `signal:1`, `status` not `exited`: another tab's relaunch, which its own start
  takes; so when aterm relaunches several codex-alab tabs together each finds its own), and removes the rest (no
  `resumeArgv`, older, an orderly end such as a person's `signal:2` or an `exit:"fatal"`, unreadable). (Amended
  2026-10-16 after the review of 148a9fedb: the first start took them all, and the other tabs got no carry-on; after
  the review of cf084e3b7: an orderly end was kept a day, and its package with it; and after the review of
  363789134: a `fatal` record was removed while aterm's Codex lane could still relaunch it. Amended 2026-10-17: that
  lane is deleted (aterm 9c478f5df8), nothing relaunches a `fatal` exit, and its record is removed like any orderly
  end's, `alab-tui/src/record.rs::relaunch_owed`.) A start takes its own predecessor's `carriedOn` at the sweep,
  before its first record write (§5 carry-on). ITS OWN PREDECESSOR'S FILE STAYS (held, below) until this process
  commits a concrete replacement: until the launch probe answers, every record this process writes is the busy
  initializer, which names no `resumeArgv`, so a start after a kill in that window removes it as naming no relaunch
  and takes the predecessor's, which still names the act, as its own; once the probe answers, the next record names
  the line (and carries the act where the probe proved aterm's relaunch), it is written through, and the
  predecessor's file is retired after it is on disk (an orderly exit retires it too). No record aterm reads withholds
  its line: aterm reads none before the claim (aterm copy, below), the claim comes after the probe (the probe opens
  the connection the claim uses), and the claim waits for the record's write-through, so the copy aterm relaunches
  from names the line (amended 2026-10-22, CX-B B1, composed with main's held custody, 6584f10f3). Before, the
  sweep removed the predecessor's file at once and the relaunch's first record withheld the unproven act yet named
  the line, so a SIGKILL 2 ms after the claim was proven, inside the coalescing second, left the next start a unique
  newest predecessor with no act (seen live, land8: an owed `continue-done-check@v1`, its wait 240 s, lost). The
  root TUI starts that background sweep from its effective configured CODEX_HOME before native restore, without
  waiting for a rendered frame. An unresolved sweep grants no verdict and spends no launch authority; disabled or
  failed writer initialization settles empty.
  Every synchronous write, signal snapshot and finalization prepares inheritance before touching the record.
  Imported fields come from one newest snapshot, including explicit clearing: an older `owed`, `pace` or retry
  cannot survive a newer record that has none. Equal newest timestamps are ambiguous and grant no handoff;
  their carry-on count remains conservative. Same-identity and swept handoffs keep their session and resume
  line through records written before the first frame, after provenance is checked. Native acceptance or a
  thread stop fences late startup seeding, and acceptance during an asynchronous resume scan supersedes that
  scan's entire state and metadata application.
  A taken predecessor remains on disk until its successor atomically commits a concrete replacement or an
  orderly ending. An unprobed initializer cannot consume that sole durable copy, including when a restored
  frame has arrived; conservative deadline rounding preserves the otherwise exact owed binding. A settled
  empty handoff has a concrete bootstrap identity too. Before the launch probe settles, a restored frame is
  serialized only as a complete busy initializer: explicit null session, resume line, cwd, state and exit.
  Draft, queued and pending input remain conservative, so that projection cannot authorize an empty-composer
  fallback over a person's input. The internal native frame remains intact. A selected same-thread bootstrap
  already supplies materialization when the first restored frame has not supplied its rollout path yet.
  An empty identity-only bootstrap cannot create generic busy carry-on authority: its serialized chain is
  exhausted unless its selected predecessor was busy and the exact relaunch provenance is proven. Explicit
  owed, recovery and in-flight intents keep their existing independent checks; live chain state is unchanged.
  Before the first frame, one accepted native root takes
  precedence; several roots yield only an empty bootstrap revocation with the carry-on chain exhausted in the
  record, never in live state. Cleanup first captures the source by rename, validates its exact content and dead
  identity, and restores any replacement without overwriting a newer path. Recognized quarantines survive
  interruption, and duplicate copies of the exact same snapshot are one source, not an ambiguous tie.
  An orderly exit first writes an empty `status:"exited", exit:"handoff-cleared"` record for the held
  launch lineage with an exhausted carry-on chain, retires its predecessors, then removes itself. If cleanup
  fails or is interrupted, that marker remains; another argv's sweep keeps this particular terminal marker for
  the normal 24 h authority horizon, and its own exact-argv successor takes its newer explicit clearing. Other
  orderly and fatal records retain their ordinary removal behavior.
- Schema v1 (null where unknown):

```json
{"v":1,"pid":4242,"procStart":"Thu Oct 16 08:00:00 2026",
 "identity":{"bootId":"…","uniqueId":123,"startSeconds":1792130000,"startMicroseconds":12345},
 "sessionId":"<primary thread uuid>","displayedThreadId":"<uuid>","cwd":"/abs",
 "version":"<codex --version>","exe":"/abs/current_exe","launchArgv0":"/abs/as/PATH/resolved/at/launch",
 "execGeneration":0,"resumeArgv":["/abs/launchArgv0","<root flags…>","resume","<uuid>"],
 "env":{"CODEX_HOME":"/abs/.codex"},
 "status":"busy|idle|waiting|upgrading|exited","exit":null,
 "state":"working|idle|blocked|done|error","blockedKind":null,
 "draftEmpty":true,"lastHumanInputAt":1792130000000,
 "autopilot":"off|passive|active","claim":"codex-alab@4242|null",
 "statusUpdatedAt":1792130000000,"updatedAt":1792130000000,"kind":"interactive","entrypoint":"cli",
 "carriedOn":0,"needsOff":"00abcdef01234567","workdir":"/abs/repo","backgroundTerminals":0}
```

- `status:"idle"` only at true rest: the frame reads idle (or done, or error) AND no turn runs (from its start hook
  to its stop or abort, or the thread's stop), no act is being carried out, no box answer is posted and unapplied, no
  carry-on is owed, no person's turn is submitted and unstarted, no queued input, no upgrade under way. A wait on a
  timer is rest: the act it leads to leaves rest first. `draftEmpty` is a JSON boolean; a key, paste or click (a button press) makes
  it `false` until the next frame shows the composer (so the first keystroke reaches the record before its echo).
  (Amended 2026-10-16 for the watch-only worker's idle point.) A wheel, a drag or a button's release begins no draft
  and leaves it as the last frame showed; it stamps the human grace as a key does. `lastHumanInputAt` rides along: it
  is the stamp as of the record's last write, and a new stamp alone writes nothing (`Record::same_content`). (Amended
  2026-10-23: every wheel step read as a draft's start and was written through, and every key's stamp was a change,
  so a person scrolling or typing rewrote the record, a full sync each time, about once a second; aterm reads no
  `lastHumanInputAt`.)
- `carriedOn` (added 2026-10-16, still v1: a record without it reads 0, and aterm ignores it): how many
  relaunch carry-ons in a row led to this process with no person's turn, no real work and no landing since (§5
  carry-on). The
  carry-on sets it (its predecessor's plus one) with the act's own `busy`, before the turn goes; a process that
  declines the carry-on keeps its predecessor's count, and every start takes its own predecessor's count at the
  sweep, before its first record write, so a relaunch that dies before its carry-on decides hands the next one
  that count, never 0 (amended 2026-10-16 after the review of 363789134); a person's turn, a root turn codex-alab
  typed that worked for real (at least 5 min, `Timing::progress`), or a Land landing (`landed@v1`) sets it back to
  0. A short root turn does not, nor does any done claim, which ends nothing (§5 Cruise) (amended 2026-10-19:
  owner's FOREVER CRUISE / FOREVER LAND ruling, "we are deprecating the ATERM-TASK-DONE logic and prompts"; a root
  turn that ended done, `ATERM-TASK-DONE` or a done reply after the done check, reset it, and a turn of real work
  did not; earlier: amended 2026-10-16 after the review of 148a9fedb: relaunch, carry-on, clean end, continue,
  stall repeated, bounded only by stall detection and aterm's back-off; and after the review of cf084e3b7: one
  "completed successfully" per round reset it). The live upgrade's re-exec keeps it: the new image starts from the
  old one's count.
- `owed` (added 2026-10-19, still v1: absent when none, a record without it reads none, and aterm ignores it): the
  autopilot's act the image owed as the live upgrade re-executed, and whether its thread had landed under Land; and,
  while a paced act waits (the back-off's wait is rest, so the record says `idle` then), the act it owes, taken out
  when it goes, when a turn starts or when its thread stops, so aterm's relaunch after a crash at rest hands it on
  (§5 carry-on; amended 2026-10-20: a crash at rest lost the act for good). So too while A WALL'S TRY waits on its
  rung (a server's, a network's or a cut-off's retry, `api-retry@v1`/`api-cutoff@v1`; a usage window's resume or a
  quota's try, `usage-resume@v1`; a network wall's wait under a measured reach carries the plain retry, never
  `api-back@v1`, whose words claim a measure no relaunch has taken, and so does a measured return's `api-back@v1`
  that a before-every-act check holds back, due as it was: amended 2026-10-21 after the parity-3 review, F1, as it
  carried nothing and the relaunch after a crash under the check typed nothing): the relaunch types it at its time
  (through `recovery` where one names the thread, else from `owed`, bound at the failed turn its decision observed:
  `owed.nativeOrigin` and `recovery`, below), and its yield
  climbs the ladder `pace` carries,
  as aterm's loop types a wall's next try from the ladder its books carry (`seed_retry`, `seed_usage_attempt`;
  amended 2026-10-21 after the walls parity review, W5: the wait carried nothing, so a crash in it left the relaunch
  typing nothing until a person's turn); the live upgrade waits for it instead of carrying it (§5 live upgrade). A wall's try stays owed across a change of the Autopilot word (its point
  is the wall's), answers to its wall's own switches (`retry_api_errors`; `resume_limits`, and `limit_wait` for
  the usage window's), one off typing nothing and raising the wall's attention as at its own point (`api`, `limit`;
  amended 2026-10-21 after the parity-3 review, F2: the carried try went quiet with no word), and a usage window's resume types the words of the word read when it goes. So too while a paced act come due waits
  on a check (a person's hold, a draft, a popup, a status unreadable: §5's before-every-act checks), through every
  recheck under it (amended 2026-10-21 after the parity review, finding 4: that wait carried no act, so a session
  killed under a hold and relaunched typed nothing at its lift, and the live upgrade carried nothing over it). A thread that stops on an exit aterm
  may relaunch leaves it in (SIGHUP, the terminal gone under aterm's own crash, which its restore relaunches;
  SIGTERM, aterm's stall or memory remedy: `codex_alab_core::session::RELAUNCH_SIGNALS`, the ends
  `relaunch_owed` reads): the record that signal keeps hands the act to that relaunch as a crash's does, and the
  leaving process types nothing (amended 2026-10-20: the `/exit` teardown such a signal starts stopped the thread
  and took the act out, so a session at rest lost it across aterm's restore); a person's SIGINT, `/exit` or any
  other stop still takes it out, as does a turn that started before it,
  `{"thread":"<uuid>","rule":"land-nudge@v1","text":"<the act's words>","mode":"land","notBefore":1792130000000,"repoOwed":["not landed"],"landed":false}`
  (`rule`, `text` and `mode` absent for a landing alone: the exact decision, its words as they would have been
  typed, and the Autopilot word it was decided under; a rule with no words or no word, an older image's, carries
  no act; `text` and `mode` added after the review of 2a9241bbf, findings 1, 8 and 16). `resumeGoal` (added
  2026-10-21 with W5; absent when false): the act is the usage window's resume, which resumes a `UsageLimited` goal
  first, else types its words (§5 usage window); a `usage-resume@v1` without it is a quota's try, typed as a turn (a
  goal that quota wall stopped `UsageLimited` is resumed instead: §5 usage window, amended 2026-10-22, CX-B B3).
  `landing` (added 2026-10-21 after the parity-3 review, F4; absent when false): a wall's try that delivers a land
  act again (the act the wall cut was Land's instruction, a nudge, a land answer, or a wall's try of one), so the
  image that types it holds the Land episode as the one that decided it did, and the try's yield's clean defense
  lands (§5 Land); without it the relaunch typed Land's instruction again over a defense the episode had asked for.
  `repoOwed` (added
  2026-10-21; absent when the words are typed as decided): a done claim's act that waited before the repository
  was read (Cruise's done check, and Land's nudge at a done claim not yet probed) is decided without the
  repository suffix and names the open work the claim named; the new image reads the repository once the act is
  due and fits the words again as one act with what it holds, exactly as the old image would have then (aterm
  reads the repository at every done claim when its act goes, its `repo_read`; its `fitted_act` names fewer open
  words before the repository's suffix gives way). A done check from a record without the key is owed the suffix
  all the same with its open words as decided (amended 2026-10-20 for Cruise; amended 2026-10-21 after the parity
  review, S1: Land's nudge crossed the upgrade typed as decided, without the suffix). A nudge decided over a
  finding already read carries none: that finding left out work a landed task's own wake began over, and the new
  image no longer knows whose that work is. Written only into the record the leaving image's `upgrading` write leaves; the
  next image hands it to that thread's first resume, once (§5 live upgrade).
  `owed.nativeOrigin` (optional, added 2026-10-21) binds that existing intent to
  `{"turnId":"<native turn>","phase":"running|complete|interrupted|aborted"}`. Its native identity and
  phase must still match the latest unsuperseded attempt: ordinary acts require its clean completion,
  and A WALL'S TRY (a retry, a measured return, a cut-off's or an overlong reply's, a usage window's resume, a
  quota's try, a compaction) exactly the failed completion its decision observed, whose failure that image
  classified with its details; so bound, the history's bare completion (a provisional fatal for core's `Other`)
  does not replace the point (amended 2026-10-22, CX-B B2: every act at a failed completion was refused, so a try
  `owed` carried with no `recovery` did nothing at the relaunch, and the wall waited for a person's turn);
  landings can retain the exact completed point after their own wake was judged, including a wall it met.
  A judged landing can also retain that wake's exact interrupted or aborted terminal point.
  An already-authorized crash carry-on can also retain its same running or interrupted point.
  A separately validated `policy-continue@v1` may retain its existing intent only at the exact
  `complete` origin of a `CyberPolicy` refusal whose history proves a started turn with regular
  user input and no compaction. Its text must equal `words::POLICY_CONTINUE`, optionally followed
  by one space and the single exact `words::POLICY_BOUNDARY` suffix. Resume restores the native
  Policy point independently, then preserves the validated instruction and deadline; restoring
  unsubmitted intent does not mark it spent. A later mode change may revoke the owed act without
  turning the refusal into ordinary continuation. This exception leaves wall-retry classification
  unchanged, and ordinary acts at failed completions remain rejected.
  A newer attempt, correction or cancellation revokes the old intent. A HostTurn already bound to the
  exact `Interrupted` point survives a second crash before it is typed; a later interrupt of a `running`
  origin revokes it. Malformed origin metadata discards the entire optional `owed`, while the rest of
  the record remains readable. Legacy ordinary acts, wall's tries and landings without an origin are ambiguous
  and are discarded; a legacy HostTurn can bind only to the exact running attempt named by its existing
  `inFlightRetry`. Neither origin nor retry timing creates action authority, and rejected intent never
  falls back to a new carry-on.
- `needsOff` (added 2026-10-20, still v1: absent when none, a record without it names none, and aterm ignores it):
  the key of the NEEDS YOU set a person's or the config's off took off the person's screen (§5 NEEDS YOU), as 16
  lowercase hex digits (a JSON number would lose a 64-bit key's low digits in a reader that reads doubles; anything
  else names none), written at the off and dropped once a report raises a set again. The off is the one end of a
  need the thread's history does not show, so the image after a re-exec, and the relaunch after a crash (its
  predecessor's record), restores the needs the history leaves standing unless they are this set, and carries the
  mark on while they stay down (review of the forever autopilot, finding 2: an off's need came back at every
  re-exec and relaunch).
- `workdir` (added 2026-10-20, still v1: absent until a command ran, and aterm ignores it): the folder the primary
  thread's worker last ran a command in, the first folder the repository probe reads (§5 Cruise). A resumed thread whose memory
  is new takes this process's own, else the previous image's (a re-exec) or the dead process's (a relaunch), and
  the record carries it on (review of the forever autopilot, finding 3: the default history keeps no exec's folder,
  so it was lost across both).
- `probeScope` (added 2026-11-05, still v1: absent until the primary thread touched a folder, began a turn or named a
  branch, and aterm ignores it): what the repository probe reads for the primary thread (§5 Cruise), `{"dirs":[abs
  path…],"overflow":true,"since":<unix s>,"branches":[{"name":…,"dir":abs path}…]}`, each key absent when empty or
  false: the folders it touched (its commands', the folders they `cd` into or point git at, the folders they tell
  another tool to work in, `make -C`, `cargo -C`/`--manifest-path`, `npm --prefix`, `pnpm -C`/`--dir`, `yarn --cwd`,
  `just -d`/`-f`, `env -C`, `go -C`, `tar -C` and the rest `repo::command_reach` names with the forms it leaves
  unread, the repository a `git clone` or `git init` makes, a shell's `-c` script's, and its edits'; and its
  sub-agents' commands and edits, each thread a sub-agent spawned from it, or from one of its sub-agents, runs, where
  a review's, a compaction's and any other internal agent's thread names no parent and stays unfolded: amended
  2026-11-05, the forever-persist lane, items d and e, `sub_agent_scope_tests.rs`, `probe_scope_tests.rs`
  `work_a_tool_did_in_a_folder_it_was_told_to_work_in_holds_the_landing`; kept as
  their repositories after each probe and once more than 64 folders, and twice what the last compaction left, gather
  between probes; at most 512, oldest first), `overflow` where it lost a folder (more than 512 at once, one it could
  not write, or a command whose walk was cut: more than 512 folders, as many as the scope keeps, or a shell within more
  than 8 shells, item e; amended 2026-11-05 after the review of the forever-persist lane, F1: a first bound of 16
  overflowed the scope for good on one `git -C` over seventeen worktrees, `probe_scope_tests.rs`
  `one_command_across_many_folders_of_a_clean_repository_still_reads_clean`), the session's start (its first turn) and the branches it used (at most 64, newest first), each with the
  folder its git command ran in, whose repository alone reads it (`dir` absent where none could be placed: read in
  every repository; amended 2026-11-05 after the review of the forever probe at f48a215d7b: 128 folders of one clean
  repository between two probes overflowed the scope for good, R1, and a name used in one repository read another
  repository's branch of that name, R3). A re-exec's or a relaunch's record carries it to the session's
  next image, which merges it into what its own memory began (every folder and branch of both, the earlier start, an
  overflow of either); a form this reader does not know reads as a scope with `overflow`, so the image after it never
  reads the repository clean over folders it cannot name (review of the forever probe, land-probe-last-workdir and
  land-probe-head-only: the session's other folders and branches were lost across both, and only the last folder was
  read).
- `pace` (added 2026-10-21, still v1: absent at rest, a record without it carries none, and aterm ignores it): the
  primary thread's short streak and the wall's ladder it is on, where its class's ladder outlives a process (a
  server's, a network's, a usage window's or a quota's retries; a compaction's attempt, a sign-in and a fatal stop
  are none), and the acts `continue_per_hour` counts within its window, its times ms since the epoch,
  `{"shortStreak":2,"wall":{"class":"server","attempts":3,"firstSeen":1792130000000,"lastTry":1792130060000,"since":1792130120000,"answerOwed":true},"typed":[1792129000000]}`.
  `checks` (added 2026-10-27; absent at 0) is how many done checks the Cruise run typed, so the next process asks
  the angle after them (aterm's `seed_checks`, §5 Cruise).
  `replies`, `repliesRun` and `repliesAt` (amended 2026-10-27 after the parity sweep, gap 4: the record carried no
  typing stop, which codex-alab did not have; absent where none, so an older record reads as no reply kept) are
  ruling F's replies (§5 typing stop), the oldest first, `[{"mark":"fedcba9876543210","answers":2,"walled":true,"tries":1}]`
  (the mark 16 hex digits, as `needsOff`; `walled` and `tries` absent when false and 0; a mark this build cannot
  read is no reply kept), the no-work answers in a row, and when the newest answer was counted (ms since the epoch).
  `repliesReset` (absent where none) is the reset the newest counted answer showed (ms since the epoch, perhaps
  still to come; aterm's `said_reset_ms`), kept by an Off as the memory keeps it, and `repliesPoint` (absent where
  nothing is held) the words of the point the typing stop holds, `{"text":…,"earlier":…,"questions":true}` (each
  absent where none): a held point owes no act, so `owed` names none, and the image after a re-exec stands at that
  point again where its words still make the newest kept reply, so an Off and back types there as it would have
  before (amended 2026-10-27 after the parity sweep, gap 4: the stage-1 port carried neither, so the image after a
  re-exec kept a usage window's walled answers and run past its reset, and stood at no point, an Off and back typing
  nothing until another turn ended). The image after a re-exec and a `/resume` back in the same process take them up,
  where the memory keeps none; aterm's relaunch never does (aterm's `launched_since_books`: a program started after
  its books is another run), whatever else of the pace it takes, so a dead predecessor's pace seeds none of them
  (amended 2026-10-27 after the parity sweep, gap 4: a `/resume` away and back forgot the hold this process counted).
  `leftPoint` (absent where no point is left to the person) is THE POINT LEFT TO THEM (§5 person's own exchange),
  `{"text":…,"earlier":…,"questions":true,"opens":true}` (each absent where none or false; `opens` says the exchange
  opened the session): the image after a re-exec and a `/resume` back take it up as they take `repliesPoint`, and
  aterm's relaunch never does, and they stand at it again, theirs, where the history still rests at its words, so
  Land keeps it theirs and Cruise's first decision takes it back (amended 2026-11-05, edges-5: the record carried no
  left point, so the image after a re-exec stood at none, and a switch to Cruise typed nothing until the person did).
  `landHold` (absent where none stands) is LAND'S HOLD standing (§5 Land), `{"since":<ms>,"nudges":3}` (`nudges`
  absent at 0): when the act that began it was typed and the nudges typed since; the image after a re-exec and
  aterm's relaunch take it up where their memory holds no hold of their own (aterm's loop seeds its own from the
  session's books at every start, `seed_land_hold`), and it shows once the episode's instruction is known typed (an
  owed nudge or land answer, a wall's try of a land act), so the landing goes on with the same `since` and count
  (amended 2026-10-28 after the review of gap 11, finding 1: the record carried none, the new image's first claim
  took aterm's down, and the next land act began it anew, its `since` and nudges started over).
  `goAheadHour` (absent where none holds) is when THE PERSON'S HOUR standing began (§5 go-ahead; ms since the
  epoch; aterm's `go-ahead-hour@v1`, which its loop books and seeds at every start), from their key or the end of the
  turn it came in: the image after a re-exec and aterm's relaunch hold it for the rest of that hour, as the window
  keeps the key but not the end of a long turn the key came in. `goAhead` (absent where none was typed) is THE
  GO-AHEAD'S COUNT on the standing park, `{"acts":2,"first":<ms>}` (`acts` absent at 0, `first` where none): the
  image after a re-exec and a `/resume` back bound the same park where it was left; aterm's relaunch takes it up no
  more than it takes the replies (amended 2026-10-30 after the review of the go-ahead port, findings 3 and 9: a
  re-exec timed an hour that ran from a turn's end from the key, as much too early as the turn was long, and let a
  held park through the same reply's hold once more).
  `esc` (absent where none stands) is A PERSON'S ESC over the point (§5 person's Esc), `{"at":<ms>,"turn":<native
  turn id>}`: when their Esc stopped the point's turn, and that turn. The image after a re-exec and aterm's relaunch
  take it up where the history's abort ended that same turn, stand at the interrupted point whatever the relaunch's
  source (a signal's teardown would carry on at once over it), hold it for `human_grace` from the same Esc and then
  carry on as this one would have; an Esc for another turn is no Esc of this point's. `wallGoal` (absent where none
  is bound) is THE GOAL A TURN ERROR BLOCKED at the wall the point stands at (§5 goals), `{"id":…,"objective":…,
  "budget":1000,"turn":<native turn id>}` (`budget` absent where it has none; `turn` the turn whose error blocked
  it): the image after a re-exec and aterm's relaunch bind it again where the thread's history holds that turn's
  start and no person's goal stop since, so the wall's try resumes that goal there while it stands as the wall left
  it; a person's stops are counted per process and never written, so each image counts its own from none, and a
  person's stop takes the goal out of the pace, so the record carries none they stopped. A form this reader does not
  know, an `esc` that names no time or no turn, and a `wallGoal` that names no id or no turn discard only that field
  (amended 2026-11-05, the forever-persist lane, item a: both lived in memory only, so the image after a re-exec
  parked the Esc's point for good, a relaunch after a signal typed its carry-on over the person's Esc, and a
  relaunch's try met the wall's blocked goal as a stop it might not lift and parked; `turn` and the stop's taking
  out amended the same day after the lane's review, F2: a person's stop before a restart was forgotten).
  `since` (added 2026-10-21 after the walls parity review, W2; absent from an older build's record, whose ladder
  then counts from `lastTry`) is the appearance the ladder counts from (aterm's `since`: the first, or the one that
  answered the last try, §5 API); `answerOwed` (absent when false) says the last try was still owed its answer when
  the process died (its turn cut by the crash), so the first turn the next process sees end answers it, met by the
  wall or not, as aterm's `seed_retry` reads a carried retry that was `owed`; `typed` (absent when none; added
  2026-10-21 after the walls parity review, W6: the hour's budget started over at every re-exec and relaunch) names
  when each act the budget counts was typed, which the next process spends where this one left it (aterm's
  `seed_acts`). `cut` (absent when none; added 2026-10-22, CX-B B2) is THE ACT A WALL CUT,
  `{"text":"<its words>","mode":"cruise"}`: the defend prompt's or a land act's words the wall's tries deliver
  again, which a full context's compaction types again verbatim (aterm's `cut_text`, §5 context window); the next
  process takes it up under the same Autopilot word only (another word drops it, as a change of word does live), so
  a relaunch's compaction delivers the act the wall cut where it typed the plain continuation. The image after a
  re-exec takes up its previous record's; a start takes up its swept predecessor's however old (aterm's loop seeds
  its streak and its ladder from the session's books with no horizon, `short_streak_in`, `seed_retry`; the acts the
  budget counts fall out of its window at the seed; amended 2026-10-22, CX-B B3: a pace written over an hour before
  was dropped, `PACE_HORIZON`) where aterm's books would hand it on: the predecessor's record names this process's
  own aterm session (`atermSession`, below; aterm keeps those books per session, by `sid`), or aterm's relaunch
  typed this process's line (the carry-on's proof, §5, but for the busy status and the chain). Then the record
  carries that pace from its first concrete write past the initializer (the held predecessor's file keeps it until
  then, above). A predecessor from another tab, or
  one that names no session, gives a person's own resume no pace: the start drops it, and the record carries none
  (amended 2026-10-22 after the review of CX-B, CXR-2: the start took it up whatever proved it, while the record
  writer withheld it as unproven, so a person's resume days later in another tab started on the dead process's
  rung and streak); the resumed
  thread's memory, where it holds none of its own, backs off from that streak, spends that budget and retries on
  that rung, counted from the appearance the record names, and the record carries it on (§5 back-off; the parity
  review, S2: a relaunch started the streak at 0). Whatever ended the process before (a person's SIGINT, a `fatal` exit, a crash): aterm's loop seeds its streak
  and its retry ladder from the session's books at every start, however the worker before it ended
  (`run.rs`'s `watch_to`: `short_streak_in`, `seed_retry`), and a person's turn leaves the streak standing (§5
  back-off). An owed act or recovery intent follows only an eligible handoff, since it would otherwise type into
  a person's stop; a pace only spaces acts that come anyway (decided 2026-10-21 after the parity review's finding 5).
- `recovery` (added 2026-10-21, still v1, optional): an unresolved server, network, overlong-response, usage-reset or
  entitlement retry on the primary
  thread. It names the exact `thread` and failed `turnId`, `pointAt` and `notBefore` in epoch milliseconds,
  `byAutopilot`/`byPerson`, and a `wall`: `{"kind":"server","what":"model at capacity","cutOff":false,"retryAt":…}`
  `{"kind":"network","what":"…","measured":true}`, `{"kind":"overlong","what":"…"}`, `{"kind":"entitlement"}`,
  or `{"kind":"usage","resetAt":…,"resetLabel":"…"}`. Provider deadline fields are optional; `notBefore` keeps
  the already-decided lower bound. A network wall's `measured` (absent when false; added 2026-10-22, CX-B B2) says
  the process measured the wall's route (§5 API's reach): the process that restores the wall reads the route again
  from the thread's config and sign-in (the failure's own details, no TLS or proxy refusal, judged by the one
  before) and measures it while the wait stands, so a measured return ends the hold a measured outage decided
  (`api-back@v1` is never held to `notBefore`); before, the restored wall had no route and the relaunch waited the
  whole hold blind. A record without it measures nothing. A measured return's act leaves a network `recovery` as
  the blind retry does (amended 2026-10-22, CX-B B2: a measured return a check held back before any retry was
  decided at its wall left none, and its plain retry in `owed` was refused at the relaunch). It carries classification
  and original deadlines, never an authorized prompt.
  Restore requires the same-process identity at re-exec, or the exact recent predecessor/argv and kernel-attested
  aterm relaunch lease used by the carry-on rule. A person's resume, an explicit stop or Off gives no recovery
  authority. The native rollout must still end at that exact failed turn: a newer turn, user message, interrupt,
  successful completion or incompatible failure discards stale intent. The next image re-decides under live policy
  and keeps native usage, goal and account checks; it never shortens `notBefore` or the original provider deadline,
  and a usage wait does not add reset grace to an already-adjusted deadline. New turns and resolved points clear
  intent. A swept candidate waits in memory while the asynchronous launch probe answers; every record suppresses
  it until provenance is proven, even if a root frame arrived first. Proof may then preserve it before the first
  root frame. A resume or Off that cleared the candidate stays cleared when a delayed probe answers.
  Older records have none; unknown or malformed recovery discards only this optional field. Diagnostic and reset
  label text is bounded to 4096 UTF-8 bytes independently of classification, turn identity and deadlines.
  Beside the wall's try `owed` carries (W5, above), aterm's relaunch takes `recovery` first where it names the
  resumed thread: the try is decided again and the record's `owed` copy is dropped. Where no `recovery` names the
  thread (a record from an image before `recovery`, a recovery that does not restore), the `owed` copy binds where
  its origin is that failed turn itself (`owed.nativeOrigin`, above) and is typed at its time, as decided (amended
  2026-10-22, CX-B B2: it bound to nothing, and the relaunch typed no wall's try from `owed`). A try `recovery`
  decides again carries no `landing` (F4), so its yield is judged as the act the new decision types. A ROLLBACK
  (CX-B B2, decided 2026-10-22): an image atpkg's rollback runs reads what it knows of this record and ignores the
  rest (`measured`, `pace.cut`); an image of parity-3 restores the `recovery` this image now writes for a measured
  return too; an image from W5 to `recovery` types the try in `owed`; an image older than W5 reads no wall's try at
  all, and nothing a newer record writes can teach it one, so after a crash there its wall waits for the next turn's
  end (the live upgrade never hands it a try: it waits for the try, §5 live upgrade); and a try an image from W5 to
  the origin binding wrote names no origin, which a newer image discards as legacy. The record's `owed` still names
  the try, and the live upgrade still waits while a wall's try is
  owed (§5 live upgrade). (Amended 2026-10-21 when parity-3 composed with main's 7583298c9, and again with main's
  e8ed46aae, whose native origins bind no act at a failed turn.)
- `inFlightRetry` (added 2026-10-21, still v1, optional): provider pacing while a native request is still retrying,
  `{"thread":"…","turnId":"…","notBefore":…}`. The selected wait is written before its retry-status notification is emitted;
  capacity includes its positive local backoff and ordinary retry advice keeps its original absolute deadline.
  The first root can report its wait before the first frame. Bootstrap, passivity and Off retain this pacing
  fact, which grants no continuation authority. Only an independently witnessed busy crash carry-on, still
  within the existing one-carry-on chain, may use it, after matching the latest native turn. A newer accepted
  turn, ordinary completion, interrupt or stop discards it. HUP/TERM teardown can retain the exact wait through
  native shutdown's matching Interrupted event; ordinary interruption and SIGINT cannot. An authorized carry-on
  keeps its original deadline and shows the wait, under current policy, goal, quota and input checks. Missing or
  malformed metadata discards only this optional field. It never becomes a terminal failure or a ready-made
  prompt, and a person's resume gains no act from it.
- `atermSession` (added 2026-10-22, still v1: absent outside aterm, a record without it names none, and aterm
  ignores it): the aterm session the process runs in, its launch environment's `ATERM_PARENT_SESSION_ID`
  (`s-<hex>`), the key aterm keeps a tab's books under. A start in the same session takes up the record's `pace` as
  aterm's loop takes up those books, however old the record and however the process ended (`pace`, above).
- `goal` (added 2026-10-25, still v1, optional): the primary thread's active goal objective (`status == "active"`),
  trimmed and bounded to 8192 UTF-8 bytes on a character boundary; absent when no active goal stands on the primary
  thread (paused, blocked, usage-limited, budget-limited, complete, cleared, or before launch proof). Kept per thread
  from both app-server goal notifications (`thread/goal/updated`, `thread/goal/cleared`, `thread/closed`) and the
  displayed chat widget's frame, so viewing a sub-agent never overwrites or drops the primary thread's active goal and
  aterm reads the session's standing goal directly from the session record without opening `$CODEX_HOME/goals_1.sqlite`.
- `backgroundTerminals` (added 2026-10-29 after the parity sweep, gap 28, still v1, optional):
  - the background terminals the TUI's footer counts for the thread it shows: the displayed chat widget's live
    unified exec processes, as `/ps` lists them;
  - **one a running tool call still waits on is counted**, since neither core nor the TUI marks a call's yield;
  - as of the record's last write;
  - never written through, and a move of the count that alone changes the record is held: it is written once the
    count has stood the writer's interval (a second) since its last move, or rides along on a write another change
    causes first, so a count reaches the record within a second of the last frame that moved it, and a command
    that ends within the second costs no write (amended 2026-10-29 after review: at the plain coalesced pace the
    writer's leading edge wrote each command's start and its end, two full syncs a command);
  - absent before the first frame, in the busy initializer, and from an older build;
  - a form a reader does not know discards only this field.

  aterm's survey shows it as the tab's `bg=`, codex-alab's claim, while its tap's target stands. Nothing acts on it.
- `procStart` in Claude Code's `lstart` format. `identity` follows upstream's daemon `ProcessIdentity` (copy the
  ~60 lines). A record is live only if the pid exists and its identity matches.
- `resumeArgv[0]` is the launch argv[0] as PATH resolved it at launch (atpkg's twin or reroute path, so a relaunch
  runs the CURRENT managed build), never `current_exe()`; the rest is rebuilt from codex's own clap `Command`
  (no flag table): the root flags by position minus PROMPT and images, then `resume <primary thread>`; a fork
  resumes its forked id. `null` until the primary thread's rollout is materialized. `env` holds at most
  `CODEX_HOME` and `CODEX_AUTH_PATH` (an admitted absolute file path routing `auth.json` outside the shared
  `CODEX_HOME`, pinning persistent credential storage to that file with atomic `0600` writes so shared keyring or
  `SecretsManager` state never leaks across accounts or unlinks `CODEX_AUTH_PATH`, and coordinating ChatGPT OAuth
  token refreshes with aterm's `codex_app_login::AuthLock` via an exclusive `flock(2)` on `<auth_file>.lock` (`0600`,
  `O_NOFOLLOW | O_NONBLOCK`) held from a pre-request re-read under the lock through persistence so neither process
  replays a single-use refresh token rotated by the other; codex-alab b09caa8d7), each only when set at launch.
- A machine-owned persistent credential slot is explicitly associated with a proven native File-store or admitted override by a private
  `<auth_file>.aterm-slot` regular file, exactly `ATERM-CODEX-AUTH-SLOT 1 <slotref>\n`. The slot reference is the
  canonical lowercase hex encoding of its UTF-8 registry key (at most 256 characters), never a token. Optional
  `CODEX_AUTH_SLOT` must agree with the marker. An absent association preserves native authentication; an
  unreadable or malformed association fails closed. The bounded native document is read without following a
  final symlink and hashed as exact bytes, preserving every native field. The reader advertises the compiled
  capability strings `aterm machine credential slot v1` and
  `aterm machine credential slot pending v1`; neither string grants authority. A host must
  reject unsupported consumers before assigning a managed grant, including any older live consumer of that
  same refresh lineage. Merely copying one token to another path or minting another local grant ID does not
  create an independent OAuth grant.
  Before acquiring its local refresh flock, Codex asks the authenticated owning host:
  `@<sid> codex-auth refresh slot=<slotref> operation=<32lowerhex> expected_digest=<64lowerhex>`.
  Concurrent callers and uncertain retries retain the same operation for that document generation. A later
  forced refresh after acknowledgement gets a new operation even if the document bytes were unchanged. The exact
  status reply is `OK codex-auth state=<waiting|ready> slot=<slotref> operation=<same> expected_digest=<same>
  generation=<canonical-positive-u64> digest=<64lowerhex>`. No duplicate or unknown fields are accepted. Ready
  is distinct from the initial `OK codex-auth refresh-pending slot=<slotref> operation=<same>
  expected_digest=<same>` reply, whose exact three fields bind polling before the owner has read a verified
  revision. Pending invents no generation or digest. It retains the same operation and never authorizes token
  use. Hosts require both compiled capability strings for every live consumer before managed assignment;
  a reader advertising only the original slot capability cannot understand initial pending responses. Ready
  acknowledges the owner's durable encrypted revision and exact-byte native-file materialization. Codex
  verifies that digest under its local lock, reloads native auth and sends no refresh token itself. An unavailable,
  uncertain or malformed owner response never falls back to native token exchange. The owner retains durable
  refresh custody across cancellation and process exit. This dedicated connection uses normal AUTH and session
  binding, has no Autopilot claim, and remains available with the enhanced interface off.
  An initially ordinary refresh rechecks association under its native file lock before sending, closing the
  custody-transfer window. Reload publishes only the verified native token revision; a competing account
  replacement or unreadable file keeps the previous cached login and returns a transient failure.
  A missing final marker with any `<auth_file>.aterm-slot-state` still present is unresolved custody, never
  ordinary authentication. The client does not parse or erase that host-owned state. An ordinary file refresh
  also checks for the host's `<auth_file>.refreshed` journal under the local refresh lock before spending an
  unchanged token. Pending or unreadable journal evidence stays with the host; an already published same-login
  rotation can still be loaded without another exchange.
  Auto storage also honors custody when the native file belongs to the attempted login, including a real file
  fallback with no override. A known unrelated keyring login does not inherit stale file authority; ambiguous
  legacy same-login copies cannot authorize an independent refresh. Exact refresh-token copies take precedence
  over missing or conflicting login-owner metadata. A changed token without proof of a distinct grant remains
  ambiguous and retains custody. An opaque refresh journal remains held even after the native file is signed
  in to another grant: that file does not prove which earlier token the journal may already have spent.
- Removal: `/exit`, `/quit` and archive remove it, and THEN (while the process still lives) codex-alab sends
  `meta unset supervisor holder=codex-alab@<pid>` on its claim connection: aterm re-reads the record at that meta
  change and so sees the orderly end; without it aterm's last copy still shows the record and the exit is
  relaunched as a crash (aterm's wire, Appendix). Before that release, on the same connection, it takes down its
  own `owner=codex-alab` entry, the `owner=needs` entry and the `owner=codex-alab-wait` entry (amended 2026-10-27 after the parity sweep, gap 2: nobody is
  left to type when a wait ends), each once the write to it in flight is answered (aterm
  applies a set before it answers it), and from the exit on it puts none of them up (amended 2026-10-24 after the review
  of 23c4163306: a reason left up said "needs you" for a process that had gone, until the next claim in the
  session). The exit publishes closing and captures its request connection and accepted claim's unsettled
  entries under one state lock, before session retirement or closing-induced local claim loss can discard
  them. One operation performs these clears and the release; repeated exit callers share its retained
  outcome, and exhausting a caller's wait budget does not end that operation. A proven claim aterm answers after exit
  began waits for this cleanup and initial release before being given back itself, including a claim queued
  after that initial release already finished. One being taken as the exit begins, whose connection the exit
  holds, is released by the exit itself after its clears, never ahead of them (amended 2026-10-24 after the
  review of e399b95946 and the reconnect/exit request-order regression). `status:"exited"` with `exit:null` is also read as orderly,
  and so is `exit:"fatal"` by the crash rule (aterm's `self_claimed::verdict`). codex-alab's Fatal exit leaves with
  status 1, but aterm's Codex lane and its `upgrade_codex::crashed_status` are deleted (aterm 9c478f5df8, retire C4):
  a codex-alab exit with no proven copy to decide it (no proven claim, a released one, or a re-exec or update before
  the claim is proven again: Appendix) is relaunched by nobody (fail closed). The `fatal` record is still kept at the
  exit, for the next start's sweep (above, which removes it as an orderly end) and the pruner until then; no lane
  relaunches it, because none exists, and it is never a crash witness, so codex-alab's own carry-on never follows it
  (§5). Every signal keeps it, rewritten `"exit":"signal:<n>"` with the `status` the signal found (taken as the signal arrives, so the
  `/exit` teardown that aborts a running turn leaves a `busy` witness `busy`): SIGTERM (aterm relaunches only when
  it sent the signal itself, its stall or memory remedy), SIGINT (a person's), and SIGHUP, the terminal going away
  (a closed tab, or aterm's own crash), which is not the person's `/exit`. A live aterm reads `signal:1` as any
  signal it did not send: orderly, nothing relaunched (a closed tab has no shell to relaunch in anyway); the
  codex-alab that aterm's restore relaunches after its own crash reads it as a crash witness and carries on (§5),
  once aterm's first `status` read proves that aterm typed its line (the launch probe, §5 carry-on).
  (Amended 2026-10-16: removing the record on SIGHUP made aterm's crash an orderly end, so the restored
  codex-alab idled until a person typed.) The upgrade keeps it (`status:"upgrading"`, `execGeneration+1`); the
  new image rewrites it. A panic or SIGKILL leaves it; an OS crash or power loss may leave an older snapshot or
  none, as qualified above.
- **aterm (copy):** aterm reads the record only while its identity is live: at the proven claim, at each
  `meta-change`, at a release while the process still runs, and, while a claim or (after a passive release) a
  read-only copy names the thread the rollout tap reads and that process leads the tab's front, at each
  rollout-tap ask (at most once a second, and once more two seconds after the tab's screen last moved while the
  record names a count) for that thread, the survey's goal and its background terminals (amended 2026-10-29,
  gap 28). It keeps a claim's copy in memory/its `<state>/drive/` ledger; a read-only copy lives in that
  aterm's memory only and goes with its tab. Only a claim's copy decides an exit, relaunches, or carries a
  line; a read-only copy only names the thread aterm taps (amended 2026-10-28, gap 15).
- **aterm (watch-only worker):** the host starts a watch-only worker for every admitted codex-alab (§1) from its
  first roster pass, beside the agent itself until a proven claim stands, then beside the claim (in BOTH orders, as
  before: the host's `take_claim` meets one, or the host's claim is replaced), and beside the agent again after a
  release (`WorkerExit::SelfClaimed`, never a `host.held` entry): no typing, pressing or upgrading; it keeps aterm's
  stall and memory remedies (which act from outside). Its idle point is the record itself, read LIVE and only while
  a proven claim stands (`self_claim::idle_now` over `self_claimed::read_idle`); with none it has no idle point (no
  memory remedy) and no copy (no stall remedy, no crash relaunch) (aterm 66eb1f2635, retire A). Under the claim an idle point is
  `status:"idle"` with `draftEmpty:true` and no `exit` word; anything else (busy, waiting, a draft, a draft not named,
  a record removed or unreadable, another pid, a dead identity) is no idle point, and the loop asks again at its next
  step. The memory ceiling's restart fires only at an idle point that STANDS: read idle at two steps of the same
  screen, so the record had time to say busy and the screen shows no turn drew. The stall remedy fires on the stall
  alone (a TUI frozen mid-turn keeps a `busy` record for ever). Both are aterm's explicit restart (its stamp, then
  SIGTERM), so the exit is an asked `signal:15`, relaunched on the validated line; a driver's lease or turn, or a
  hold, makes the restart wait, uncounted. The disk tick covers every codex session, supervised or not: a vendor
  `codex` too, by its program name or its shown 7501 report (amended 2026-10-16, aterm main a751ffdc60, 20965dbd26,
  92d98163e6; the shown report, b97d43b87d).
  It is the exit's ONE decider: the Codex exit-137 branch is deleted with the lane (aterm 9c478f5df8), and
  `claimed_by_another` reads a proven self-claim whose pid is gone as nobody's. Ruling 17 and the `ReadByClaim` /
  `TheActsEndIsRelaunched` model and its conformance test are amended with it (`ty_model!`, negative control).
- **aterm (crash rule):** the exit is first decided by `relaunch::on_exit`. A crash = the agent exited and aterm's
  copy did not see an orderly removal (the record still existed at the last read) or saw `exit:signal` for a signal
  aterm sent. Relaunch only through `type_relaunch_line_with` (back-off 1 s/10 s/60 s/10 min, `[harness]
  relaunch`), with `owner_clear` = no live writer lock on `sessionId`'s rollout, a shell at its marked prompt
  leading the foreground group (else wait; never keystrokes into a program; a tab whose agent was its spawn
  command is not relaunched and says so once), and only when the copy validates: `resumeArgv[0]` equals the
  live process's launch path as aterm read it (`proc_pidpath`/argv0 resolution), `resume <sessionId>` present with
  `sessionId` a UUID, every other element present in the live process's own argv (`KERN_PROCARGS2`), `env` only
  `CODEX_HOME` and `CODEX_AUTH_PATH`, each equal to the live process's own. Otherwise no relaunch and one Messages record. Each handled witness
  `(pid, identity)` is recorded in aterm's ledger and never relaunched twice.

## 5. What codex-alab does natively, while its proven claim is held

`codex-rs/alab-autopilot` (an app-server extension, one install line in `app-server/src/extensions.rs` as the first
statement after the goal block, ABOVE the git-attribution `if` and before `codex_guardian_v2::install`), the aterm
client in `codex-rs/alab-aterm` (one connection, background tasks only), TUI hooks in `codex-rs/alab-tui`, and the
shared in-process state in `codex-rs/alab-core` (one static per process; embedded mode only, which ALab forces).
Only root interactive threads the TUI registered; never exec, app-server, sub-agents (a sub-agent's commands and
edits only move its root session's probe scope, §4 `probeScope`; it gets no runtime and no act). Every `aterm` interaction is
a background task with a 5 s timeout, never awaited inside a lifecycle callback, a render or an input hook; a
timeout is "unanswered" (passive). With no `ATERM_PARENT_SESSION_ID` or no reachable aterm socket: nothing.

**Before every act** (submit, approve, answer, compact, upgrade): read `@self status` on the connection; act only
with `hold=0`, `hand=-` or its own, `input=` `-`/`clear`, `human_ms` ≥ `human_grace_s`·1000; its own TUI input
stamp older than the grace; the composer empty; no queued, pending-steer or rejected-steer input in the TUI; no
user turn pending start; no modal or popup open (the observe hook exports these); the claim re-confirmed; and, last,
under the supervisor's lock, that codex-alab still acts after the status read and re-confirm.
Explicit `status autopilot=off`, after the hold guard, revokes the claim and its existing Off epoch before any
transport await, then reconciles the full policy. The remembered Land mode and landing survive; Off never becomes
Cruise. A contradictory effective-on reply ends passive and unclaimed, with the existing clocks retrying rather
than immediately re-arming an evaluator. The word clock applies the same rule without an act or event, but a
standing hold first keeps its claim and landing. Every act looks again right before its
irreversible step, after its own awaits (the record's write-through, the thread's mode and goal reads, the approval
facts). A native act also revalidates its original full policy, Off epoch, accepted-turn counter and native point
after ALab's preparatory awaits; a changed snapshot is decided afresh. This check precedes submission to Core,
whose existing queue and admission semantics remain unchanged. A status that names an Autopilot word other than the
policy's (§3): the policy takes it, the threads look again under it, and the act waits, as its decision is no longer
current (amended 2026-10-23). Anything else: wait, look again at the deadline. Unreadable status: wait. (Amended 2026-10-17 after the review
of 4cdc96090, finding 1: the checks read the standby only at their start.)

Fresh, account-bound permission from a full usage read wakes a waiting native controller.
Every subsequent decision, including the network preflight, retains that permission while
it is fresh; a reachability check cannot put a restored account back on its old reset timer.
These reads and network observations are controller events, not model prompts. Existing
coalescing, adaptive polling and retry pacing still apply. Subscription failures also
watch a same-account denied-to-allowed transition, retain its identity across restart,
and wake once access returns; an already-available ordinary bucket alone cannot
clear a missing premium entitlement.
Subscription failures retain their route through turn completion, so their due retries
use the same network preflight as a usage reset.
A native controller with pending quota or subscription recovery caps the account poll
at 15 seconds, including in background tabs, while retaining faster near-limit polls.
Off, passive, and recovered sessions keep their usual lower-frequency polling.

An automatic usage resume rechecks the exact goal's identity, status, objective and token budget
under the goal service's existing mutation permit, after earlier goal changes have finished.
A changed goal or native decision is not reactivated and spends no retry. Accounting-only
updates do not invalidate an otherwise unchanged usage-limited goal. If the decision still
stands and the edited goal remains usage-limited, rejection wakes a fresh evaluation; it
does not leave the reset's continuation without a timer. Paused goals, and goals a person or the
model blocked, stay stopped; a goal a turn error blocked at a wall is that wall's stop, which the
wall's try resumes (§5 goals row).

Cruise also recovers a stale usage stop after a live human question receives a successful,
nonempty answer (2026-11-04). The ordinary continuation keeps its backoff and input checks,
then resumes through GoalService only the same usage-limited goal captured at that human
turn's start. This closes the case where the successful exchange replaced the old quota
wall while the persisted goal still owned the pace. Both usage-recovery policy switches
must allow it. An abort, current error or quota wall, restored transcript, changed goal,
paused or blocked goal, exhausted budget, draft, queued input or human hold grants no such
resume. The start-bound evidence remains in memory; a re-exec cannot inherit it from an
ordinary owed continuation. Native atomic budget admission remains authoritative, and a
budget-limited outcome is never reported as a resumed goal or retried in a tight loop.

Native start and completion callbacks are ordered per root. A late completion still ends its
exact native turn and acknowledges that turn's mail, but cannot replace a newer turn's point,
recovery, pace, last-turn record, needs or attention. Completion retires its local alert reasons
before yielding; deferred remote publication reads the current desired reasons. Reasons retain
their originating root, so progress in one loaded chat cannot clear another chat's wall, limit
or stopped-goal alert. Session-wide overlay and mail reasons keep their own scope. A posted plan
answer retains its original root across a chat switch and clears only that root's plan reason.
Stopping or replacing a loaded root revokes its callbacks' right to mutate shared state and
retires its scoped reasons. A replacement with the same chat and native-turn IDs still has
new custody. HUP/TERM relaunch retains its recovery state. A retired posted plan records its
original outcome once without clearing a replacement plan's alert.

An upgrade queues a conditional TUI event, rather than an unconditional `/exit`. After its awaited checks,
act recording and timer suspension, it scans every loaded thread again. Native input, frame and thread
changes, including goal and background-task events, invalidate that scan. When the queued event is dispatched,
the TUI reads its current draft, queue, popup, editor and turn state and commits leave synchronously under the
native-generation fence and the supervisor's current policy/claim lock. Only that acceptance starts shutdown.
A newly decided immediate continuation can be carried before its next native turn, while its evaluator retains
the thread's permit. A normal rejection leaves that evaluator as the sole owner of the immediate decision;
cancellation restores it to one timer, which does not offer the same decision for upgrade again. A rejected,
dropped or cancelled request carrying an existing paced act restores it at its original deadline unless a newer
turn superseded it; a late event cannot revive a cancelled request. Consideration itself
is cancellation-safe from its first await. Explicit user exits and signal handling keep their existing paths.

| feature | native behaviour | mirrors aterm |
|---|---|---|
| turn end | only an idle that follows a turn THIS process ran and saw end (`on_turn_stop` recorded a `LastTurn`); a continuation, defend prompt, Land instruction or nudge decided here may carry aterm's fact (§5 "aterm fact"); an idle at which nothing is typed carries none | `decide_turn_end` |
| continue | submit `TurnInput::UserInput{client_id:"alab-autopilot-<n>"}` with `turn_trigger:"alab_autopilot"` and text `continue_text` (then aterm's fact where §5 "aterm fact" gives one) under Cruise (under Land the land act types instead, never `continue_text`: §5 Land; amended 2026-10-19); refuse in Plan mode (core admits User input there); none while a goal is Active | `continue@v1` |
| taskless | no act until a person's turn exists: a `UserMessage` whose `client_id` is not codex-alab's (live), or, on resume, a `TurnStarted` whose `turn_attribution.turn_trigger` is not `alab_autopilot` (an attribution that names no trigger = a person; a start that names NO attribution is a person's only where its turn holds a user message, a history from before core attributed turns: core attributes only a regular task, so a compaction's turn, a person's `/compact` or codex-alab's own `context-compact@v1`, names none and holds no user message, and is nobody's, as the live thread reads every turn no `UserMessage` opened; amended 2026-10-21 after the parity review, R1, read off the rollout core writes for a remote and a local compaction) | taskless rule |
| carry-on | a thread resumed by aterm's crash relaunch or aterm's restore after its own crash (this process started with exactly the record's `resumeArgv`, the record < 24 h old, died busy (`status:"busy"`), `exit` absent, `signal:15` (aterm's remedy) or `signal:1` (the terminal went away), AND aterm's proof that it typed the line: the launch probe, the aterm client's FIRST `@self status`, read as the process starts on the one connection the claim then uses and before anything waits for a thread, shows aterm's harness's hard lease on the tab (`hand=lease:aterm-harness@<pid>`, which aterm takes before it types the relaunch line and holds to the new process's first idle: until this process's claim is proven, which it makes only after this read on the same connection, and its record names its TUI's frame, at rest or already in a turn that the lease did not hold back (a person's), at most 60 s; aterm 0.106.0 gave it back as the new TUI led the tab, before this read, and aterm 5eebeff407 and cc61408e74 hold it so again (amended 2026-10-17, no codex-alab change)) and no person's input from 5 s before this start to the read (`human_ms`), the read within 60 s of the start; no read, an unreadable `human_ms`, another hand or none is no proof) — never after codex-alab's own re-exec, which carries only the paced act it owed (§5 live upgrade) — with a task gets its mode's act at its first verified idle, whatever its last `TurnComplete.last_agent_message` said: under Cruise `continue_text`, or the defend prompt where that reply was a done claim (the repository probed first); under Land the land instruction (it opens the episode), whose yield's clean defense lands; the act is recorded `harness-turn@v1`; any other resume idles until a person's turn (amended 2026-10-19: owner's FOREVER CRUISE / FOREVER LAND ruling, "Cruise is always looking for more completeness and perfection"; a last reply of `ATERM-TASK-DONE` or a done reply got no carry-on). A SESSION THAT DIED AT REST OWING A PACED ACT (its record's `owed`, §4: the back-off's wait is rest, so it says `idle`, and no carry-on is its) gets that act from aterm's relaunch, proven as for the carry-on but for the busy status and the chain (no orderly end, the record within 24 h, this process started with its `resumeArgv`, aterm's lease at the launch read with no person's input from 5 s before the start), at the first resume, which spends this start's relaunch as the carry-on does: typed as the dead process decided it, under its rule, at its time, its yield judged as that rule's; never for a person's own resume of the line (amended 2026-10-20: a crash at rest lost the act for good, end-to-end run). At most one carry-on per chain: a predecessor whose record says `carriedOn` 1 or more (a carry-on led to its death with no person's turn and no real work since: the continuation, at once or after the autopilot's own turns, stalled again and aterm restarted it, or it crashed again) gets none, whatever its exit word, so one stall's restart carries on, as aterm's remedy intends, and a repeat of stall, restart, continue waits for the person instead of looping until aterm's back-off. (Amended 2026-10-16 after the review of 6efbf7518: under the managed link a person's own `codex resume <id>` runs exactly the record's `resumeArgv`, since the link execs the package by its absolute path, so the line alone let a person who closed a tab mid-turn and resumed it by hand get an unasked continuation. No aterm change: the lease is aterm 0.105.0's own relaunch hand.) The lease must be named for the aterm process itself: `aterm-harness@<pid>` where `<pid>` is the control socket's server as the kernel names the connection's peer (`LOCAL_PEEREPID`); a lease named for any other pid, or an unknown peer pid, is no proof. THE HOLDER BINDING: the name alone names only the aterm that answered the read, and aterm's pid is public (`ps`, and every client's own `LOCAL_PEEREPID`), so any client of the socket could take a lease under exactly that name. aterm reports who took it: `status hand_pid=<pid>`, the kernel peer of the connection that took the lease, read at the acquire and never from the holder's text (`-` for no live lease or an unknown taker). Where the token is present, the lease is aterm's only when `hand_pid` equals the server pid as well (both kernel-attested), so a lease another process took under aterm's name is no proof, and `hand_pid=-` is none either; an aterm that prints no `hand_pid=` keeps the name check alone. (Amended 2026-10-17: aterm main reports `hand_pid=`.) One carry-on per process: the predecessor and the launch read are facts of this start, so the process's first resume reserves them before its asynchronous sweep wait, then spends them after that sweep settles, whatever it is a resume of and whatever it learns (an empty settled sweep or a history that could not be read), and a later resume in the same process (`/resume` away and back) is no relaunch; nor is any resume more than 60 s after the start (the launch probe's window); a process whose own `carriedOn` already reached the maximum carries on no more. The chain (`carriedOn`, §4) ends only at a person's turn, a turn codex-alab typed that worked for real (at least 5 min) or a Land landing, never at a short turn or a done claim, so relaunch, carry-on, stall again waits for the person (amended 2026-10-19 for FOREVER CRUISE: a done no longer ends a task, so it ends the chain no more; real work ends it, which a stall loop never does). WHAT COUNTS AS aterm's RELAUNCH: every line aterm itself types under its relaunch lease carries on once, whatever made aterm type it: its stall or memory remedy, its restore after its own crash, its restore after a logout or a shutdown (which keeps the agent in its quit layout), and an asked `aterm ctl restart` (`cause=asked`) of a busy session alike; the record's `signal:15` or `signal:1` cannot tell them apart, and none of them is a person typing the line. A person who wants a session to stop, not to continue, puts a `hold` on it or uses `/exit`. (Amended 2026-10-16 after the review of 148a9fedb: a /resume away and back carried on twice, a lease named for another pid than aterm's was trusted (one forged under aterm's own pid still passes: above), the second tab of a joint relaunch got no carry-on, and the asked restart's carry-on is now stated. Amended again after the review of cf084e3b7: a first resume that learned nothing left the relaunch unspent for a later `/resume` back, and a first done reply ended the chain.) | carry-on |
| Cruise | the session's Autopilot `cruise` (or no word, §3): the worker's words never end the task. A done claim (aterm's `says_done`: a done phrase, an idle report, a retired `ATERM-TASK-DONE` line, a `Nothing left:` defense) gets the defend-and-find-more prompt (`continue-done-check@v1`: aterm's Cruise `DONE_CHECK`, or `done_check_text`) at every claim (where the reply sends `done_check_angles`, aterm's six `DONE_ANGLES` one to a line, and `done_check_text` holds the first, each done claim typed asks the next angle in its place, the seventh the first again, under the standing rules whole as aterm's `done_check_act(n)` fits them; counted per Cruise run as aterm's `TurnEndState::checks` counts, back to the first at a person's turn, an Off or a run out of Cruise, and carried in the record's `pace.checks`, §4; an older aterm's reply, with no angles, has its act typed whole; added 2026-10-27), paced by the back-off, with suffixes: the open work the claim names (`Your report still names open work (<at most four words>): finish and land it first.`; an item that names its blocker or a decision is open work all the same, aterm's Cruise reading under the owner's rule of 2026-10-19: amended 2026-10-20), what the repository probe finds that no remote main has, asking only for the agent's own work (`Your repository still holds work no remote main has (<summary>): land what of it you made first, and leave what you did not make exactly as it is.`, and for an unread probe `Your repository could not be read: show its git status yourself, land what of it you made first, and leave what you did not make exactly as it is.`: aterm's `RepoAsk::Own`, `REPO_OWN`, `REPO_UNREAD_OWN`, aterm `f9a7dbd4c4`; amended 2026-11-05: Cruise's check asked to commit, merge and push all of it, a person's files and another session's stash too, which Land's nudge alone asks, `REPO_ALL`; `decide_tests.rs` `cruises_repository_ask_lands_only_what_the_agent_made`), the reversible clause where an irreversible act is named, and the NEEDS YOU ack where one is owed, in that order, then the act's one copy of the standing rules its text arrived composed with, the whole fitted as aterm fits it to 800 UTF-16 units (the repository's summary cut to 160 units, then further, then named no more, then fewer open words, the rules cut to the room left; aterm main `6ff0b5475a`'s `fitted_act`, `repo_work_suffix`; amended 2026-10-20: codex-alab put the repository's suffix last and every suffix after the rules), then aterm's fact where §5 "aterm fact" gives one, whole or not at all: the rules are never cut further for it. The probe runs when the act is due, over EVERY FOLDER THE SESSION TOUCHED: each folder a command of the thread ran in, each folder its script `cd`s or `pushd`s into (a subshell's ending at its `)`) or points git at (`-C`, `--work-tree`, the folder a `worktree add` makes), and each folder an edit (`apply_patch`) landed in, kept as their repositories after each probe and whenever more than 64 gather between probes (a folder with no `.git` between it and a repository already found is that repository's, as git's own discovery finds it), at most 512 and once one is lost never read clean, the last command's folder first, then the thread's working folder; each folder is read at the outermost project its repository is a submodule of (`git rev-parse --show-superproject-working-tree`, at most 8 levels up) and at its own repository, each repository once; the thread's working folder's repositories are read whole, and a repository only another folder led to holds work only where the session changed it since the thread's first turn (its `HEAD` reflog or its stash's has an entry since, a changed or untracked path's inode changed since, or, where it cannot tell, no `HEAD` reflog, a path that is a folder, always), so a checkout the session only looked in (a dependency's) holds nothing that predates it; the first finding is named, and a cut read of any folder is unread unless another finds work (amended 2026-11-05 after the review of the forever probe, land-probe-last-workdir, aterm's D2-4, under the owner's directive of 2026-10-19, restated 2026-10-29, "if landing LAND EVERYTHING INTO REMOTE MAIN NO EXCEPTIONS": one reviewer voted the last folder's read designed, as aterm reads its transcript's last folder, and the directive overrules that design; it read the first of the last command's folder and the thread's inside a repository, after the review of 2a9241bbf, finding 6; the folders, the session's start and the branches it used cross the re-exec and the relaunch in the record's `probeScope`, §4, as the last folder does in `workdir`, amended 2026-10-20; amended again 2026-11-05 after the review of the forever probe at f48a215d7b: 128 folders of one clean repository between two probes overflowed the scope and Land never landed again, R1; a dependency checkout it only looked in held Land for the rest of the session, R2, where main read the last command's folder whole and aterm reads its transcript's last folder whole, so a checkout the session only looked in no longer holds even as the last folder, the owner's "everything" being what the session made; a folder reached by `cd` or `git -C` was never read, R8): git only, read-only, offline (no fetch, and no lazy fetch of a partial clone), its reads on a thread of their own beside the asker, which has its answer at the deadline while a stalled file-system step holds only that thread (amended 2026-11-05 after the review's hangs-1: a stat run inline on the runtime ran past the deadline and held the thread's evaluation), at most 10 s, never in a guarded folder (`~/Documents`, `~/Desktop`, `~/Downloads`, `~/Library` cloud stores, `/Volumes`), nor in a folder that does not resolve over local mounts alone (aterm's `resolve_local` over the mount table read with `getfsstat(MNT_NOWAIT)`: a folder on a network share or an automounted volume, or reached by a link onto one, is refused before anything on that mount is touched; amended 2026-10-21 after the parity review's P2, which replaced a lexical check before and after `canonicalize` that followed a link onto a share), the guarded folders checked in the file system's own spelling as well (stricter than aterm, whose check reads the spelling given, so there `/users/x/documents` lets `git` into `~/Documents`), and with `safe.bareRepository=explicit` (stricter than aterm: a repository directory planted in a work tree is refused at discovery, which changes the answer only for a folder inside one whose configuration names a work tree); it counts changed and untracked paths, stashes, commits no upstream has, and commits not on origin's default branch (`refs/remotes/origin/HEAD` where the branch it names exists, else `origin/main`, else `origin/master`; none known, no count; a dangling `origin/HEAD` hid every commit: amended after the review of 2a9241bbf, finding 14); where a remote is configured (`git remote`) but no remote-tracking ref exists (a clone of an empty repository, a `git remote add` never pushed), every commit on a `HEAD` that has one is a commit no remote has (amended 2026-11-05 after the review of the forever probe, land-probe-unfetched-remote, aterm's D2-5: it read clean; a repository with no remote configured still has nowhere to push); in a repository with a remote, each populated submodule's commits, at its checkout and at the commit `HEAD`, the index or its superproject's checkout records, that no remote of that submodule has, read offline from its remote-tracking refs, its shallow roots and its `FETCH_HEAD`, at every depth, and a lagging checkout's entry is never dropped when its commit is on no remote (amended 2026-11-05 after the review's land-probe-submodule-unpushed, aterm's D2-1: a pushed record of a submodule commit nobody can fetch read clean); and the commits on each branch the session used (one its worktrees' `HEAD` moved from or to since the thread's first turn, read from their reflogs, or one a command of the thread created, checked out or added a worktree on, at most 64, read in the repository the command's folder is in, past its `cd`s and `-C`s, and in every repository only where that folder cannot be placed), locally or on origin, that are on neither main, `HEAD` nor any remote, with that branch's stash entries (amended 2026-11-05 after the review's land-probe-head-only, aterm's D2-3: a branch, a worktree or a stash the session left behind read clean; a name used in one repository read another's branch of that name, R3 of the review at f48a215d7b); another session's branch is never read, but where a branch's folder is gone or was never placed (`cd "$X"`), its name is read in every repository, which may read another session's branch of that name rather than miss the session's own; these reads are strict: a refused read, one off local mounts and one Git answered with a warning are cut, never clean; a deadline or failed/interrupted Git read is unread and adds the unread-probe instruction instead of proving clean (the deadline, `PROBE_LIMIT`, is a one-minute hang detector, aterm `710345cbf9`: a healthy probe of a large repository took 8 to 12 s on a loaded Mac; the start probe a landed task's wake waits for has 10 s, `START_PROBE_LIMIT`, and one it cuts proves nothing unchanged; amended 2026-10-24). A guarded/nonlocal folder is still skipped without entering it. Every git read runs with the person's `HOME`, and the untracked count honours the person's global excludes as git reads them in that repository (`core.excludesFile` through includes and `includeIf`, else `$XDG_CONFIG_HOME/git/ignore`; the repository's own setting wins), never a path in a guarded folder; it counts only the stashes made on the checked-out branch and on the branches the session used (another session's worktree's are none of its work; amended 2026-11-05, land-probe-head-only), and it reads the work's state (`HEAD`, the branch's stashes and each changed or untracked path's status, size and modification time, hashed; at most 4096 paths, no file's contents read; past that, none, and a finding with no state matches no other) so two findings with the same counts tell apart (aterm main `e0da8d68df`; amended 2026-10-20). Every other free end gets `continue_text`. Nothing records a finish (amended 2026-10-19: owner's FOREVER CRUISE / FOREVER LAND ruling, "we are deprecating the ATERM-TASK-DONE logic and prompts in favor of FOREVER CRUISE or FOREVER LAND"; this row was `done`: the token ended the task, a done reply got the done check once and a second ended it, `finished@v1`) | aterm `a355109905` `turn_end.rs` (`DONE_CHECK`), and `says_done` and `open_work` as aterm main `e0da8d68df` has them, verbatim; the probe aterm main `e0da8d68df` `harness/repo_work.rs` (from 4bff4848b4); all diffed against aterm main `6ff0b5475a`, whose `fitted_act`, `repo_work_suffix` and main ref codex-alab now has (amended 2026-10-20); the forever probe's reads (every folder the session touched and the project around a submodule, the branches it used, submodule commits on no remote, a configured remote never fetched, reads beside the asker) as aterm main `b8332c44cf`, `801e72779e` and `e24fe994ad` have them (amended 2026-11-05) |
| Land | the session's Autopilot `land` ("if landing LAND EVERYTHING INTO REMOTE MAIN NO EXCEPTIONS"): the land instruction (`land@v1`, or `land_text`) at the episode's first free point, then a nudge (`land-nudge@v1`, or `land_nudge_text`) at every free point with no bound, paced by the back-off, carrying the open work a done claim or defense names in Land's strict reading (no blocker excuses an item; work not on the remote main counts; so does what a defense still leaves, an exception, a blocked item, a wait on a review, a person or a check, a later day, a run that could not be made, a stopgap, a known issue: after the review of 2a9241bbf, finding 7, these were codex-alab's rows ahead of aterm's; aterm `d3e1dd842b` folded them into its 37 word families, each with where a row counts and what settles it, measured on its labelled defense table, which codex-alab's tests hold too: amended 2026-10-20; aterm `44e4fa7d61` closed the ninth review's holes in it with 49: a denied landing, a failed or partial count, what is counted left, a status word, an unchecked box, an unanswered question, a named branch, a bare `#N`, a deferral, an exception, an absence, a head over code: amended 2026-10-20; aterm `0b4c90d4ec` read the tenth review's holes narrow, failing toward open, with 54: a leftover is settled only in its own clause, with its word in the subject or as the worker's object, by a predicate that lands it, so a later clause about another item, a place that is no landing (rebased onto, based on, a pull request into, conflicts with origin/main) and approved, reviewed, enabled or green settle no change; a head's none item clears only itself (`Remaining work: none, but the docs need a pass` stays open); a sign-off with an object or a later day is no sign-off; a question is answered only by a reply that names nothing left; an exception however worded, counted failures, percents and fractions, a branch in a code span or named bare, dropped or reverted work and a change a clause does not land read open; an auto-merge that landed it, an aside that it was pending earlier and what still holds read clean; the honest defenses whose landing crosses a clause or rests on another subject, a pronoun or a word that lands nothing by itself (`PR #12 was pending review; it is merged now`, `local main matches it`, `the stash is dropped`, `the PR is closed`) are deliberate non-landings, rows of the table's OPEN side, whose nudge names the word so the worker restates the landing in one clause; a done claim with no `Nothing left:` line, which never lands, names no push for want of a place: amended 2026-10-20; aterm `500d97d8b3` read the eleventh review's holes narrow too, with 65: a question's rows are read in the statement it asks and a yes is plain only with where or as what, a sha, a run's number or every lane; `still` keeps a good only at its clause's end or before a place or a manner, never after a fault; a change is named by a reference only with no verb between, by a bracket only with no predicate after it, and settled by a coordinated subject, a bracketed list, a label or a later landing of its number; approval with no landing, work shipped without what it owes, work made locally, work untouched, a fault back again, a qualified none, a party's item, an item named alone, a branch current with main, a check turned off in code, a count beside a none and hedges read open; a participle aside or list, a relative clause in the subject, a checked `that` clause, a thing that no longer exists and a branch gone where the sentence lands the work read clean: amended 2026-10-20; aterm `78d1055bd9`, `55edc289b5` and `0e7ef0d916` read the integration review's leftovers open, failing toward open: an ask of the person written without its NEEDS YOU head, a defense that names its own exception (`Nothing left but the changelog`, the nudge naming it): amended 2026-10-20; aterm `4cacfb7913`, `ac0c76b137` and `149efe1190` closed the misses of a blind holdout, the proof readings each in its own clause: the task's own items skipped, a fix in part, a correction not made, a check only type-checked or not driven end to end, what only a person can do, a test missing by its kind or of a claim, a decision or a fix left to a party, a scope excuse, a reach named as a part and a test that would pass with its fix taken out read open; a test's proof by failing (a mutation's, a mutant's or a negative control's before the failure, a test seen to fail first, one failing on the old code or with its fix taken out), a test run's whole tally, a mechanism retried, a thing kept on purpose and a test that still catches what it was written for read clean; a path's colons and a thousands comma cut no clause: amended 2026-10-24) and, at a done claim or a clean defense over a repository that still holds work, the repository's suffix (probed when the act is due, as Cruise's; never work a landed task's own wake began over; in Cruise's order and fit; amended 2026-10-20: aterm main `6ff0b5475a` reads the repository at every done claim, and codex-alab named it only at a clean defense); the instruction and a nudge (never the land answer) then carry aterm's fact where §5 "aterm fact" gives one, after their standing rules, whole or not at all; a question gets the land answer (`land-answer@v1`, or `land_answer_text`). LANDED (`landed@v1`, recorded, never typed) only at a land act's yield (the instruction, a nudge, a land answer after it, a carry-on, compaction continuation or usage resume that typed a land act's words, or a wall's retry of one) whose reply is a clean defense (a `Nothing left:` line, no need standing beside it, a NEEDS YOU line that says none however it is marked being none, `NEEDS YOU: nothing`, `**none**`, `nothing at the moment`, `- none` under a bare head, and one whose head ends in a dash or carries a bracketed qualifier, `NEEDS YOU — …`, `NEEDS YOU (owner): …`, being a need: amended 2026-10-20; and no open work in the strict reading) and whose repository probe then finds nothing; THE REPLY IS THE WHOLE TURN: a need said in an earlier agent message of the turn lands nothing either. Nothing is typed after a landing until A PERSON'S ACT re-arms Land, and NOBODY'S ACT keeps it (aterm main `e0da8d68df`, the rule of 667c926ec4; amended 2026-10-20): a person's turn starts a new episode (a person's quick message included, once the session has worked: the person's own exchange row, gap 32; codex-alab types the instruction there, where aterm keeps its episode and nudges); an Off and back judges the landing again over a fresh probe before anything is decided over it (clean, it holds; work found, Land's instruction comes, the Off having ended the episode; a probe an Off began during settles nothing, the landing judged as the Off lifts); the live upgrade's re-exec carries it (§5 live upgrade); and the landed task's own wake (a turn nobody asked: no `UserMessage` opened it, a goal's continuation) keeps it where the wake's words, its earlier messages included, name no need and nothing open in the strict reading, and the repository is clean or holds the very work it held as the wake began (a probe run as the turn starts, once the record says busy and before the turn runs, the work's state compared, no Off since), a wall the wake meets included: nothing is typed past the reset, and the kept landing's `landed@v1` is recorded again, once, as aterm reopened it at the wake's turn and reads it open until the next one (amended 2026-10-29 after the review of gap 12, finding 1: the line wore `✓ landed` over a landing aterm read open). Else the landing is over and the nudge comes, naming the repository's work unless the wake began over it (someone else's, never the worker's to commit). While a judgement is owed the landing stands, nothing is typed and the live upgrade waits for it. Under Land a wall's resume types Land's own words, the instruction or a nudge, with the reversible clause and the ack where one is owed (the usage window's resume, the quota wall's try; a goal resumed through its service types none and leaves the ack owed). `continue_text` is never typed under Land (added 2026-10-19: owner's ruling; codex-alab had no Land). LAND'S HOLD is aterm's landing (aterm's `LandHeld`, told as §5 visibility says): it begins at Land's instruction, at the act that first types Land's words in an episode (a carry-on, a compaction's continuation or a usage resume under Land) and at a land act after a landing, as aterm's `HoldStart`, and at a land act where this image knows no beginning (a record that carries no `pace.landHold`, an older build's, §4: there it begins anew; amended 2026-10-28 after the review of gap 11, finding 1); `since` is the act's own time, stamped as the act is booked; its nudges are the `land-nudge@v1` acts typed since it, the act that began it none of them; and a land act's yield that ended whole is read for what held it (aterm's `hold_at_yield`) until any other turn begins (one no act of codex-alab's awaits, aterm's `took`) or ends (amended 2026-10-28 after the review of gap 11, finding 2: the reading stood through another's turn until its end); none outside Land, before the episode's first act, after a landing until Land's next act, or at a point left to a person (Land's opening message, or one an Off left, where no hold stands yet: gap 32; amended 2026-10-28 after the parity sweep, gap 11: a codex-alab worker under Land showed no landing in aterm; `land_held_tests.rs`) | aterm `a355109905` `turn_end.rs` (`LAND`, `LAND_NUDGE`, `LAND_ANSWER`; `LAND` and `LAND_NUDGE` with aterm `72aa7380fe`'s no-repository sentence, "Files no git repository holds land where they are once verified.", which a real model in a plain folder needed to defend its work instead of asking for a remote: amended 2026-10-24), `clean_defense`, `land_open_words` and `open_work_strict` as aterm main `e0da8d68df` has them, verbatim, diffed item by item against aterm main `66894933d2` and its strict reading's changes ported (`4cacfb7913`, `ac0c76b137`, `149efe1190`), with its blind holdout (`fixtures/land-holdout-2026-10-20.tsv`; amended 2026-10-24); the landing's rules (`wake_keeps`, `wake_left_open`, `off_landing`, `repo_theirs`, `continue_as`, `turn_said`) as aterm main `e0da8d68df` has them; all diffed against aterm main `6ff0b5475a`, whose claim's repository read codex-alab now has (amended 2026-10-20) |
| NEEDS YOU | a report's `NEEDS YOU: <need>` lines stand as the thread's needs, read FAILING TOWARD A KEPT NEED (aterm main `e0da8d68df`, rebuilt on its labelled needs table, which codex-alab's tests read too: 1da587a559, 0469b355be; amended 2026-10-20): a head behind a quote's gutter, emoji, a checkbox, a label, ticks or a table cell, its two words joined any way, `NEED YOU` and `NEEDS YOUR <what>:` in capitals, a head after a sentence of its own or inside one with its separator, a bare head taking the list or paragraph under it (with neither, it still names a need), never inside a fenced code block, a span of code it does not open, a tool's output, quotation marks, a `no` that negates it or a line's start that names it (`on a line starting NEEDS YOU: and carry on`, aterm's own instruction read back: aterm `2a290f10f7`); a head whose words name a need is that one need, the list under it the report's (aterm `93de0197f9`; both amended 2026-10-24); the reply is the whole turn, so a need said in an earlier agent message of the turn stands beside the last words', each message read as the Markdown item the TUI renders on its own: a code fence one message leaves open closes with that message and hides no later message's need (amended 2026-11-05, needs-fence-crosses-messages: a README snippet nesting one fence in another left the reader's fence open across the joined turn, the later message's need was read as code, and Land landed over it). A TURN A WALL OR AN INTERRUPT ENDED still names the needs its complete messages said: core hands each agent message whole and a wall comes from a later request of the turn, so they stand, go up and owe their ack whatever ended the turn (the end clears what stood only where it is a person's turn that met no wall, an interrupted one included, before its own messages fold), a usage window's resume leaving it owed for the act at the agent's own next turn end where it types its one cleared line (Cruise's, and Land's past its instruction: the line carries no ack), Land's instruction carrying it where the resume types that (amended 2026-11-05, needs-wall-turn-unfolded, under the owner's 2026-10-19 words below: a need said before a wall was dropped, and the act after the wall began another turn, so it was lost for good). A set is replaced by the next report that names needs or defends, and cleared by `NEEDS YOU: nothing` (or `none`) only where every word of the line is accounted for (a none, the words it negates, a restatement of the landing, the person told they are set; any other word keeps the line a need); at once, never behind a back-off or the grace, codex-alab raises `meta set attention owner=needs kind=ask Codex needs you: <first need, at most 64 cells>[ (+N more)] — carrying on` (at most 200 B), apart from its own `owner=codex-alab` entry. CODEX-ALAB'S OWN NEEDS go up in that same entry, before the worker's: a need only a person meets while the work goes on (the quota's, usage window row) is a need of the entry, never one of codex-alab's `owner=codex-alab` reasons, of which aterm shows the first standing alone, so no reason hides it, its count is aterm's `needs_n=`, and a manager's `await task` wakes on it as on the worker's (amended 2026-10-22, parity-5 N). aterm shows the entry's first need in words and counts the rest (`(+N more)`); the menu bar's rows one level in, the hover's list and `HarnessAsk::others` carry each need whole from the set its window was told, which codex-alab tells as its own loop tells it: `@self meta set harness_needs entry=<entry> need=<need> […]`, the set the shown entry says (its first 32 needs, in the worker's order, beside that entry's text; past 32 the entry alone counts every need, and aterm counts the larger of the set's others and the entry's `(+N more)`, `HarnessAsk::told_counted`, aterm 8517f2947b), on the claim connection while the proven claim is held, wherever codex-alab sets or replaces that entry (a report whose first need and count are the ones shown writes no entry and still tells its others), and `@self meta unset harness_needs` wherever it takes the entry down: a report's `NEEDS YOU: nothing`, a person's turn's end that met no wall and the exit (after the entry, before its release, never waiting for a sync in flight: the release drops whatever stands); an off sends none, its release taking the set down with the claim (its entry stands until after that release, and aterm keeps reading a set beside the entry it says, an unset's outgoing one included, so an unset before it would change no reader; the entry comes down after the release). aterm reads a set only beside the entry it names, so the two reach it in either order and a set never outlives its entry on a reader. Its claim rules are `harness_held`'s: each claim's first sync sends the standing set or takes a kept one down unless the last answer this image had proved none stands (with no need standing, a set an earlier image left comes down), an aterm before the verb (`ERR unknown meta field (…)`: its rows carry the first need and the count) is asked once per change of the set under that claim, and an `ERR denied` not again under that claim (amended 2026-10-28 after the parity sweep, gap 16: a codex NEEDS YOU showed only its first need and a count; `needs_set_tests.rs`). A need never blocks and never escalates: the point's own act goes on, carrying aterm's ack (`needs_ack_text`) once per new set, with the reversible clause where a need names an irreversible act; Cruise's answers carry aterm's NEEDS YOU hint (`needs_hint_text`) after the owner's words, never inside them (Land's land answer carries the ack alone, §5 prose questions). Only a root thread's reports speak for the session (a sub-agent's need is its parent's to report), and it is set only while codex-alab holds its proven claim (one named meanwhile is raised once it claims). It stays up across a passive spell (a standby, a hold) and a box's close, and comes down only when the worker's reports clear it, at the end of a person's turn that met no wall (never at its start: it stays up while they answer it, as aterm clears it at the point; amended 2026-10-20) (a goal's own continuation is no person's turn and clears none), at a person's or the config's off (§2) and at the exit; after aterm's relaunch or the live upgrade's re-exec the new image raises again what the resumed history leaves standing (its reports folded in order, a person's turn's end clearing them, one the history never saw end clearing none, a compaction's turn clearing none (its start names no attribution and it holds no user message: nobody's, as live; amended 2026-10-21 after the parity review, R1: it was read as a person's turn and cleared the need on resume), a turn a wall ended clearing none and its complete messages naming what they said, an interrupted turn's too (amended 2026-11-05, needs-wall-turn-unfolded: its cut reply named none), as the live thread reads a turn end with a wall (core keeps the turn's terminal error on its completion event, `TurnCompleteEvent::error`, which the rollout persists whole; amended 2026-10-21 after the parity review, S4: a walled person's turn read as ended and cleared the need it never answered; an error core keeps that codex-alab's live reading takes for no wall, an `after_agent` hook's abort or an interrupt named `Other`, reads as one there and keeps the need, failing toward a kept need), a turn of codex-alab's begun after them counting as their ack; amended after the review of 2a9241bbf, findings 11, 15 and 18), never the set an off took down that no report has named since (the record's `needsOff`, §4; amended 2026-10-20 after the review of the forever autopilot, finding 2). THE SAFE READING AT A RELAUNCH: the dead process's record may lag it by its coalescing (at most a second), and it names the set an off took down rather than the set held, so the lag can only err loud: an off in that second is read as not having happened and the need is shown again (the person can turn it off again), while a need raised in that second is still restored; a record with no `needsOff` (an older build) restores the history's needs. AN ENTRY LEFT BEFORE THE CLAIM: a process that died without its exit (a crash, a kill) left its `owner=needs` entry up; every claim taken settles it at its first sync, the needs then wanted put up in its place, or, none standing, the entry taken down (journaled `an entry left before this claim; no need stands at it`), so the menu bar never says "needs you" for a need nobody holds; behind another holder's claim nothing is touched (aterm `448726c354`; amended 2026-10-20). Every act at a wall that types the carry-on words (a server error's retry, a cut-off reply's, an unreachable API's) ends with the NEEDS YOU hint, then the standing rules where any stand, as aterm's wall acts do (aterm `448726c354`; amended 2026-10-20) (added 2026-10-19: owner's ruling, "if you need something from the user, be LOUDER ABOUT IT ... but NEVER let that block you"). The hint, and so `CARRY_ON`, ends on aterm's restart sentence, `Handle agent restarts yourself: save a handoff; \`aterm ctl help restart\`.` (aterm's `restart_hint!`, aterm `11e4a76a8d`: agents asked a person to restart them; amended 2026-11-05; `decide_tests.rs` `every_wall_act_hands_the_agent_its_own_restart`); codex-alab's own ack, typed only where aterm sends no `needs_ack_text`, does not carry it (aterm's, which does, is typed where aterm sends it): with it Land's first act passed the 800-unit bound and cut a short standing rule | aterm main `e0da8d68df` `needs.rs`, verbatim, with its needs table, as aterm main `6ff0b5475a` has them (its fences and UTF-8 offsets; amended 2026-10-20), and as aterm main `66894933d2` has them (`2a290f10f7`, `93de0197f9`; amended 2026-10-24) |
| back-off | turn with < 2 min of work is short; consecutive short turns double 2 → 60 min; `continue_per_hour` caps; compaction idles are not turns; the defend prompt and Land's three acts are continuations here, and a done claim's yield is short whatever it worked; Land's instruction, where it is the act, waits no back-off (aterm's `instruction_due`, v6 §14.10; amended 2026-10-27 after the parity sweep, gap 5: codex-alab backed it off like the nudges, so a switch to Land over a short streak typed it up to an hour late): a switch to Land types it at once, its answers and nudges keep the ladder, and `continue_per_hour` caps it; a lead's worker's done claim or question within its first three short turns in a row waits no back-off either (aterm's `waited_on`, `MANAGED_SHORT_TURNS`: a lead waits on the session where `@self task` names a record not `exited`, read before each decision; amended 2026-10-28 after the parity sweep, gap 5, its managed half: a `spawn agent=codex` worker backed off its first claims 2, 4 and 8 min, so its lead's `await task` heard of them that much later), its plain continuations and its fourth short turn keep the ladder, and `continue_per_hour` caps it too (amended 2026-10-19 for FOREVER CRUISE / FOREVER LAND: the back-off is cost control, never a stop; amended 2026-10-27 after the parity sweep, gap 4: codex-alab had no stop at all, so a worker answering the same words with no work was typed into at every rung for ever; ruling F's typing stop, the next row, is now the one stop, checked before the back-off, and it holds Land's instruction too); a person's turn leaves the streak standing, nothing resets it at the turn's start, and its end moves it as anyone else's turn does (a short one lengthens it, real work ends it), as aterm's `TurnEndState` (`streak(short, d, false)`; amended 2026-10-20: codex-alab reset it at every person's turn). The streak and the rung of a server's, a network's, a usage window's or a quota's ladder outlive the process: the record's `pace` (§4) hands them to the image after a live upgrade and to aterm's relaunch, which take them up where they left off, as aterm's loop takes up its predecessor's (`seed_short`, `seed_retry`, `seed_usage_attempt`; amended 2026-10-21 after the parity review, S2: a relaunch started the streak at 0, and the act after an idle crash's owed one came 120 s after it instead of 480 s), and so does the hour's budget (`pace.typed`, aterm's `seed_acts`; amended 2026-10-21 after the walls parity review, W6). A usage window's resume is no continuation here (aterm's `is_continue_rule` leaves `RULE_LIMIT_RESUME` out): its short yield leaves the streak where it stood, so the continuation at the point it leaves goes at once, its model request moments after the resume's, and that continuation's own short yield backs the next act off (judged 2026-10-21 after the walls parity review, W7: the two `usage:continue` requests 19–30 ms apart after the usage wait in every live proof run are these two acts, each typed once, `usage-resume@v1` then `continue@v1`; both type `continue_text`, which the lab's model tags alike; not a duplicate, and nothing of upstream's) | `TurnEndTiming` |
| typing stop | RULING F (aterm's §14.3, commit 4cefe18775: "the same reply twice with no work, or six no-work answers in a row, stops typing until real work or a person's turn"; amended 2026-10-27 after the parity sweep, gap 4: codex-alab had no counterpart). WHAT HOLDS: every paced act (the continuation, the defend prompt, the answers, Land's instruction, nudges and answer, under Cruise and Land alike, checked before the back-off) and the compaction's owed continuation, while the point shows the newest reply kept and that reply came again with no work, or six no-work answers came in a row: nothing is typed, and the hold is told quietly (visibility row). WHAT COUNTS: every answer to an act typed (the act's next turn end, unless it is a person's; a compaction's is none), and the program's own quiet turn (a goal's) while a reply is kept; a reply is its mark, aterm's `reply_mark` (FNV-1a/64 of the turn's earlier and last words, letters and digits only, a word with a digit one `#`, a clock's AM/PM dropped; at a wall its class word and the wall's own words), the newest three kept; real work is a tool run (`on_tool_start` for any tool but `update_plan` and `request_user_input`), and a quiet answer is no work under `min_work`, or, answering an act, at a wall or saying a kept reply again whatever it worked (aterm's `retried_alone`, `kept_long`: Codex's tool events are the tool rows aterm's Claude reader reads); an outage (the network, the server's own failure, a cut-off where the API is measured down) neither counts nor holds; a restored point and an interrupted turn count nothing. WHAT LIFTS: an answer or a turn with real work; a point in other words; a person's turn's end (where it says the newest reply again with no work, exactly one more act goes, a wall's tries clamped to two); Off's park (the replies go, the reset they showed stays); a usage window's reset passed since the last answer (the next turn end finds the wall's replies answerable again and the run afresh, as aterm's loop does at its first read past the reset); a relaunch (aterm's: its memory starts empty). A switch between Cruise and Land lifts nothing, nor does a person's message arriving. The replies, the reset they showed and the held point outlive a re-exec and a `/resume` back in the record's `pace` (§4). A dated reset passed keeps every walled reply kept, its tries standing and its answers none, whatever the next answer says (the point aterm reads past the reset still shows the wall), and that answer is judged over the replies it leaves; a hosted web search or image generation is a tool run (aterm counts Claude Code's `Web Search(…)` row as a call) (amended 2026-10-27 after the parity sweep, gap 4: the stage-1 port cleared the walls when the answer past the reset said other words, judged it over the replies before the clear, and counted a search-only turn as no work). THE WALL HALF (amended 2026-10-28, ruling F stage 2: the wall half is now ported). A wall that holds is a usage window, a quota (`QuotaExceeded`/`UsageNotIncluded`), a reply cut off and a reply past its output maximum: its point is held before its switches and its rung, so it types no further try, once its own tries are spent (three, aterm's `FUTILE_AFTER_TRIES`: the most of the reply's own and the wall's track's attempts) at the newest reply kept, or once nine no-work answers came in a row (six and three) while they are not; a person's turn that says the same again gives exactly one more try (its tries clamped to two). A DATED reset (a usage window's `resets_at`, Codex's epoch seconds, read more exactly than aterm's clock off the screen) after the last answer lifts the hold while the tries last, and once they are spent only where it is not the reset that answer showed (aterm's `same_reset`, 30 s): passed, at most one more answer goes (the walls' answers none, their tries kept). A RELATIVE reset (a cut-off's `Retry-After`, a duration) lifts it only while the tries last, and the route measured back since the last answer (aterm's `route_back`; codex-alab measures no route at a cut-off today) lifts it too. NEVER HELD, AND NEVER COUNTED: the server's own failure (an overload, a 429, a 500, a timeout: the model's capacity), the network, and the cyber policy's refusal (`Wall::Policy`, which aterm has no counterpart of): their ladders and the policy's one continuation go on however long the same answer repeats, by THE OWNER'S RULING (codex-alab "must keep continuing and making progress with retries" through capacity outages and the security classifier's refusals, whatever aterm does). A held usage window or quota is told in aterm's `limit_repeats` words (`The limit still stands after its reset and answers each of codex-alab's tries the same.`, or `The limit names no reset codex-alab can read …`), a held cut-off or overlong reply as the quiet stop; the wall's wait comes down (nothing types on its own: a timer an earlier decision at the point armed goes, with the act it owed the record), and a quota's need stays up (only a person lifts the quota: its turn that ends with no wall takes it down, as before), raised at the held point where it does not stand, as after a re-exec, whose image starts with none of codex-alab's own needs (amended 2026-10-28, ruling F stage 2, review of 4c0168cedd, finding 4: the held point typed no try, so nothing raised it again). A reset the host slept through is one reset with the one the last answer showed: both move back by the time slept, so a spent wall stays held asleep as it does awake (amended 2026-10-28, ruling F stage 2, review of 4c0168cedd, finding 6: the kept reset was floored at the wake, so a sleep through it read as a new reset and lifted the hold). The walled replies, their tries and the reset outlive a re-exec and a `/resume` back in the record's `pace` and are dropped at aterm's relaunch, as the arms at no wall are; a held wall's point is carried as its recovery (the try the hold kept back), so the image after a re-exec stands at the wall held, an Off and back types that try, and aterm's relaunch types it. LAND'S LOUD HOLD: visibility row | `Replies`, `repeats`, `held_by` |
| prose questions | a question, stop phrase or offer in the last agent message, or `AgentMessageItem.questions: Some(..)` → under Cruise `answer_text` (or the `questions` word's policy text; `ask` leaves it to a person), and one naming an irreversible act (aterm's `DESTRUCTIVE_WORDS`) aterm's `REVERSIBLE_ANSWER` whatever the word; every Cruise answer ends with aterm's NEEDS YOU hint (`needs_hint_text`), after the owner's words, never inside them, then the one copy of the standing rules (aterm's `words_act` types `act_words(answer, [hint, reversible clause, ack], rules)`, and `harness policy` sends `answer_text` bare; amended 2026-10-20 after the review of the codex-alab proof, finding 11: the answers carried no rules), the least-cut copy aterm's composed texts carry: every one holds the same rules cut to its own room, so the longest is read, `land_answer_text`'s (the shortest of aterm's act texts, 599 UTF-16 units of room: a rules line of BMP characters, at most aterm's 400 and its `…`, arrives whole) or `continue_text`'s where the act texts' copy is it or its cut; an act whose words leave no more room than that copy had is cut exactly as aterm cuts the whole, so only rules of 199 or more characters outside the BMP under an `answer_text` shorter than 106 units are typed with fewer of their words than aterm types, never others (amended 2026-10-20: the copy was `continue_text`'s, cut where an owner's long `continue_text` left the rules less room than aterm's answer has; no wire change). aterm sends its four act texts always, composed only where rules stand: sent bare, they say none stand, and an owner's `continue_text` that itself ends `… (standing rules: X)` is the owner's words, so no answer and no wall's retry types them; `continue_text`'s copy is the one there is only from an aterm that sends no act texts (amended 2026-10-21 after the parity review, finding 3: the owner's words were split as rules there). Under Land every question, one naming an irreversible act included and whatever the `questions` word, gets the land answer (`land-answer@v1`, or `land_answer_text`: it says itself never to force-push or delete work) with the NEEDS YOU ack where one is owed, and nothing else: no reversible answer, no hint (aterm's `land_policy`). `answer_questions = false` leaves a question to a person under either word. A question in a reply that also claims done (aterm's `says_done`) is not answered: a done claim is routed first, as aterm's `land_policy` routes it, so it gets the Land row's instruction or nudge (Cruise: the Cruise row's defend prompt, with the reversible clause where it names an irreversible act), never the land answer or `answer_text`; `answer_questions = false` still leaves it to a person (amended 2026-10-19 after the review of 2a9241bbf, finding 26; amended 2026-10-20 after the review of the forever autopilot, finding 5: the row gave Land the reversible answer and the hint, which neither aterm nor codex-alab types there; amended 2026-10-20 after the ninth review of the strict reading, finding 23: the row said every question got the answer, which a reply that also claims done does not) | `RULE_ANSWER`, `land-answer@v1` |
| go-ahead | THE OWNER'S STANDING GO-AHEAD (go-ahead@v1; owner, 2026-10-29: "how dare you block on me for go ahead! fix the cruise harness so that this cannot happen again"; ported from aterm `ff13268a5b`, its reader, guard and texts verbatim at aterm main `79002d08ce` in `alab-autopilot/src/go_ahead.rs`, its table and the guard's base in `src/fixtures/`): a reply that parks the agent's own reversible work on a person's go-ahead (aterm's `go_ahead::parked` over the turn's earlier messages and its last: a `NEEDS YOU:` line asking a word to say, or prose waiting for a go-ahead, a rerun or room) gets `GO_AHEAD_ANSWER` with the word it asked for (or its quote), the wait for room where it names memory or disk, the drop of its `NEEDS YOU:` line, the other needs left standing and the needs' ack where owed, then the standing rules, under Cruise and under Land once Land's instruction was typed over unlanded work (the repository probe found work no remote main has, read at every Land point whose park names no landing work, within the person's hour and before the instruction too, as aterm's `repo_read`; or the park names a branch, a worktree or main); under Land before its instruction, the instruction goes first, at once. It outranks the done check and Land's own acts. NEVER: where the consent waited on is another's (`Waiting for Alice's approval to merge my branch`, `a reviewer's approval on my PR`) or the work is another's however the name is spelled (`rebuild ALICE's branch`, `the branch MAY made`; a deadline names no one: `the go-ahead by EOD`, `by May`), where the prose says the need's work lacks what only a person can give (`The rebuild needs your GitHub password, which I don't have.` above `NEEDS YOU: say "go" so I can rebuild the branch`), where the ask names an irreversible act (`REVERSIBLE_ANSWER` answers it; the guard is `classify_said` over the reply less its needs, never weaker than at origin/main 5b7a00447a, and `push … +<branch>` reads as a force-push), beside a question box, with answers off (told as `question: <GO_AHEAD_WHY>; answer_questions is off`), under Off, or within the hour after the person's last key (`status human_typed_ms=`, else `human_ms=`; another driver's `hand=`; never a turn a message opened by itself, which a control write such as a lead's `aterm ctl @tab send` opens too), from the end of the turn it came in; within that hour Cruise's continuation and done check leave out `NEVER_A_NEED` wherever they are typed (the free point, the compaction's continuation, the act a full context cut typed again, which the record keeps whole so a retype past the hour says it, aterm's relaunch's carry-on and the usage resume), a back-off never runs past the hour's end, and the point is decided again there. A go-ahead the image before a re-exec owed is decided again against the same: answers off tell it, the person's hour since holds it to the hour's end, its bound spent refuses it. PACE: its wait is the short-turn ladder's, at most 15 min from codex-alab's last act (the agent's own wakes restart none), none where a lead waits on the session within its first 3 short turns (aterm's `waited_on`, the park its `gate_ask`); the same reply's hold lets the first go-ahead on a park through. BOUND: 3 go-aheads on the standing park, or an hour since the first with no real work (2 min, or a wall) since: then nothing is typed and the person is told now (`meta set attention owner=codex-alab key=stopped`, aterm's `refuses_go_ahead`), as is the same no-work reply after a go-ahead; a reply that parks no more and a new key start the count again; a sleep restarts the hour since the first, and the person's hour counts the sleep. THE MENU BAR (aterm's DESIGN-harness-autopilot §15.6): the lines the go-ahead answers leave `owner=needs` from the turn's end it is owed at, off for good once it was submitted, back where it was not, where it was refused, or under any other decision on a reply that names them; the live upgrade's notice typed in the go-ahead's place keeps the go-ahead decided behind it, its lines held and a refusal told (aterm's review 9: a point judges its decision before a host's step). | `go-ahead@v1` (aterm `ff13268a5b`; its reader at `79002d08ce`) |
| question overlay | `request_user_input` STAYS. The TUI hook answers the overlay with its recommended option (else option 1), per the `questions` word; an unmarked question naming a destructive act → its refusing option, else keyed attention; `ask` → left to a person with attention | question.rs |
| plan box | under `approve=all`, after the grace, the TUI hook selects "Yes, implement this plan" (never "clear context") | `plan-approve@v2` |
| goal box | "Resume paused goal?", which a resume (`codex resume`, aterm's relaunch, the live upgrade's re-exec) opens over a paused, blocked or usage-limited goal, held every act back (`a popup or modal is open`) with no attention, for good. The App's hook tells codex-alab why the goal stopped before the view opens, and the box goes by it, under every word: over a paused or usage-limited goal, after the grace the TUI hook selects "Leave paused" (never "Resume goal"), which changes nothing, and adds a history line saying so (`/goal resume` continues it); a person's pause stands. A usage-limited goal goes on only through a `usage-resume@v1` act (the usage window's resume, or a quota wall's try; upstream resumes none by itself), which goes once the box is gone; where the point owes none, the turn end's own look says so (`left boxes` below), at every look, so a passive spell cannot lose it; the box raises nothing of its own (left open, it held every root thread's acts back). Over a blocked goal, or one the TUI gave no reason for, the box is a person's: `goal: left to a person (…)` attention says why. (Amended 2026-10-24 after the reviews of ca681c4b01 and 5a513b0797: a usage-limited goal nothing resumed left its box open, and then a reason raised once at the answer was lost at the next standby.) The answer pops only the goal box, as a person's Enter does, so a box under it stays standing (upstream clears the whole view stack when a view completes in its own tick). Leaving the goal as it was is no act of the task: noted, and told in aterm's message log, never `meta set harness_act` (aterm takes an act it does not know as work reopened) and no story (added 2026-10-23) | `goal-leave@v1` |
| folder trust | before any claim (so outside the checks above; `effective=on` carries a hold), the TUI hook (`ALAB(trust)`, `alab-tui/src/trust.rs`) answers codex's launch folder screen as aterm's `trust_dialog` does: embedded app-server only; never a project marked untrusted; only this process's launch folder (never `-C` or a resumed task's other folder); `harness policy` read on a connection of its own within 2 s, `effective=on` with `approve=safe` or `all`; under `safe`, the folder AND codex's trust target (a Git subdirectory's repository root) each under a `trust_roots` root (aterm's `roots_from_config` spelling), as written and symlink-resolved (`trust-dialog@v1`); under `all`, aterm's default, any launch folder, as aterm's full power presses "Trust and continue" for any folder (`trust-any@v1`; a rooted folder keeps `trust-dialog@v1`). Then codex's own write (`projects."<target>".trust_level = "trusted"`, the screen's "Trust and continue" path), no screen, one info line on the launch session card naming the folder and why (the root, or full power). A read that went unanswered (its hello or its request), met aterm's `busy` refusal (pausing its `retry_ms=`, at most 2 s), or found the host `suspended` (a seamless update's commit), is made again for up to 15 s, pausing 250 ms doubling to 2 s, while the startup draft draws its composer, the keys typed meanwhile going to it (one read anywhere else, where nothing draws; a person who quits the draft meanwhile quits, as during upstream's own lookup); a hold, an Autopilot Off, `approve=none`, an older aterm's refusal and no socket stand at once (amended 2026-10-23). Anything else (an older aterm, no socket, a timeout past that bound, a failed write): the screen, for the person to answer, with codex-alab's `trust:` attention naming the folder and why (§2). aterm's Codex lane is retired, and a vendor Codex's trust gate is answered by nobody | `trust-dialog@v1`, `trust-any@v1` |
| approvals | `Reviewed(Approved)` (one-shot) under `approve=all` UNLESS: `require_guardian`, or `require_fresh_review` under the `AutoReview` reviewer (under `User` the fresh review is the person's box, and codex-alab's one-shot answer is it: an escalated exec or a retry is approved once, amended 2026-10-16 after the end-to-end run); the effective policy is `UnlessTrusted` (explicit or from an untrusted project); the effective `approvals_reviewer` is `AutoReview` and not from packaged defaults; a requirements constraint fixed policy or reviewer; a non-heuristic execpolicy `prompt` rule matches (a shell wrapper it cannot parse also defers, while any rule could ask); a `granular` flag for this kind is false. Then `None`. An explicit `on-request` is not an ask rule. A box cleared by that list while the before-every-act checks say "not now" (the grace, a person's recent key) goes to the TUI, and the TUI hook selects its one-shot yes once the checks pass (never a standing yes) | `approve=all`, owner rules win |
| left boxes | any box the TUI shows (`Blocked(*)`) that codex-alab has not answered within `human_grace_s` raises `meta set attention owner=codex-alab <kind>: <why left>`; cleared when it closes. An approval names the reason the approval contributor left it (the owner's rule, `approve` not `all`, …), not the wait it met. A turn end codex-alab leaves raises it too: Plan mode (`plan: …`), a goal that stopped by itself (blocked, out of its token budget, or usage-limited where the point owes no usage resume, as at a turn that ended with no wall: `goal: …`; amended 2026-10-24), a usage-limited goal whose resume at the reset failed (`goal: …`, tried again after a minute), an act that did not go (a turn core did not start, a compaction core refused: `turn: …`; a turn core declined because another turn is running or took the slot answers that turn's end instead), a prose question it does not answer, a login or a wall it does not wait out; a person's turn clears them, and so do these: codex-alab's own yes to the plan box, once it goes in, clears `plan:` (it implements the plan itself under `approve=all`), and a goal that reads active again at a turn's start (a person's `/goal` resumed it, or the reset did) clears `goal:` (amended 2026-10-17, inbox finding 8). A point whose attention a passive spell cleared (§2) still stands (an attention answers no point; a turn's end does) and raises it again when codex-alab acts again, never while a turn of its thread runs (amended 2026-10-17 after the review of 4cdc96090, finding 3). A held claim that is not passive (§2) with an unanswered box and no attention is a defect. A worker's NEEDS YOU is no left box and no attention of codex-alab's own (§5 NEEDS YOU; amended 2026-10-19). These asks stay under `owner=codex-alab`, with no kind; an automatic wait never goes there: it is the `owner=codex-alab-wait kind=wait` entry (visibility, below; amended 2026-10-27 after the parity sweep, gap 2: a wait in this slot would hide an ask or be hidden by one, wake a manager's `await task` and read as a startup's ask) | rule 6, never-block |
| model capacity | the selected model's typed `ServerOverloaded` or `PolicyCapacityUnavailable` failure in sampling or foreground legacy and V2 compaction stays in the same operation and turn, with its original input, completed tool output, model, provider and account and without spending the ordinary retry budget or shrinking history: positive jittered backoff grows from 0.5–1 s to 30–60 s, honoring a later `Retry-After`. Only the exact provider codes `server_is_overloaded` and `policy_capacity_unavailable` enter this lane; generic diagnostic text and typed cyber, bio or misalignment policy refusals do not. Same-turn steering queues input without shortening the selected wait; an explicit interrupt or shutdown cancels it. A live reconnect retains the exact capacity reason while its wait is current. Permanent HTTP client refusals still need correction | core sampling and compaction retry |
| API / stream / server | after core's own retries: 1/5/15/30/60 min (network class 1/2/5), for ever, each rung counted from the appearance that answered the last try (aterm's `since`: the end of the turn that try led to, never the try itself), every server, network and cut-off class alike, and started over only by real work a try led to (at least 5 min, `Timing::progress`); a person's or a goal's turn that meets the wall again keeps that appearance and the rung however long it worked (aterm's `track_wall`, whose `anew` asks for the retry), and a try whose turn ends with no wall was taken, however short that turn: its track goes, and the next wall starts at the first rung (aterm's `retry_taken`) (amended 2026-10-21 after the walls parity review, W1 and W3: every class but the overlong reply counted from the try, so each try came the length of the try's turn sooner than aterm's; a person's long turn met by the wall started the ladder over; a try answered short kept its rung for the next wall). A REPLY CUT OFF (`ResponseStreamDisconnected`, or core's `Other` for a stream that broke: aterm's `ApiCause::CutOff`, `api-cutoff@v1`) is retried at once, then on the network class's 1/2/5 min ladder from the appearance that answered the last try, as aterm's `(ApiCause::CutOff, _) => attempts.checked_sub(1).map_or(since, |n| rung(&t.net_backoff, n))`, a provider's `Retry-After` still its lower bound (amended 2026-10-22, CX-B B3: it climbed the server's 1/5/15/30/60 min ladder from its first rung). DELIBERATE DIFFERENCE (CX-B B3): aterm holds a cut-off reply for its 15 min where its reach reads Down (`(CutOff, Down) => down_hold`); codex-alab measures a route only at a network wall (below), never at a stream that broke after its connection was made, so a cut-off reply keeps its ladder whatever the reach; a server's absolute `Retry-After` is a lower bound even after core's retry budget is exhausted, including foreground legacy and V2 compaction. A failed standalone compaction reaches the error lifecycle once, never a clean context reset. Permanent HTTP client refusals need correction instead; 408/409/425/429 retain the retry ladder. Each retry types aterm's `wall_retry_text` words for Codex, then the one copy of the standing rules (the least-cut copy, §5 prose questions), as aterm's wall acts carry them (`type_as`: `with_rules`; amended 2026-10-20: codex-alab's retries carried no rules). A reply past its output token maximum (Codex's incomplete response, `max_output_tokens`, which core never retries; aterm's `ApiCause::Overlong`) is retried as `api-cutoff@v1` at once, in aterm's words (`Codex reported an API error (the reply was over its output token maximum), so your last reply was cut off. Write long files and outputs across several smaller replies.`, then the carry-on), then on the 1/5/15/30/60 min ladder from the appearance that answered the last try (the end of the turn that try led to), which the long reply each try leads to never resets (aterm's `work_is_its_cause`; amended 2026-10-20: it raised attention as a refused turn); a person's or a goal's turn that meets the wall again keeps that appearance and the rung, as aterm's `track_wall` moves its appearance only at a retry of its own (amended 2026-10-21 after the parity review: every appearance restarted the rung, and the try came that turn's length later than aterm's); a ladder a relaunch took up counts from the appearance its record names (`pace.wall.since`, §4), and a try the dead process typed and died in is answered by the first turn the relaunch sees end (`answerOwed`; amended 2026-10-21 after the walls parity review, W2: it counted from the first appearance the relaunch saw, a turn's length or more off aterm's). THE API'S REACH, MEASURED (aterm's `RULE_API_BACK`, `harness/netwatch.rs`, `harness_netprobe.rs`; amended 2026-10-21 after the parity review, R4: an unreachable API kept the network ladder and was never said to be back): while a thread's point stands at a network wall (`HttpConnectionFailed`/`ResponseStreamConnectionFailed` with no status) whose route the probe reproduces, codex-alab measures that route (`netwatch`): the base URL codex resolves for the thread's provider and sign-in (`ModelProviderInfo::to_api_provider`), over `https`, with codex's own proxy decision for it direct (no managed network policy; under `respect_system_proxy` codex's resolver says direct; otherwise no proxy variable reqwest reads is set and the macOS system settings say direct for it, Windows' unread), and never a wall whose own failure codex classes as a TLS or proxy refusal (`failure_class`), an Amazon Bedrock provider, or a ChatGPT account on the Codex backend, whose host workspace routing may move to one only core sees: those routes are custom and keep the ladder. A probe resolves the host, connects to its addresses IPv4 and IPv6 interleaved, each its share of a 10 s budget, and completes a TLS handshake verified against codex's own trust (`build_rustls_client_config_with_custom_ca`: the native roots and `CODEX_CA_CERTIFICATE`/`SSL_CERT_FILE`) with no request byte sent: Up only on that handshake, Down only when the name has no address or every address is refused or unreachable by the kernel's word, Unknown otherwise; at once with no fresh measure, then after a Down 15 s, 30 s and every 60 s, otherwise every 60 s, only while a waiting thread's 60 s lease lives (the destination's task renews the lease of every thread still standing at its wall, and a thread stands there only while codex-alab waits on it: it acts, no Off, standby or unproven claim, with `retry_api_errors` on, and the thread's look again is pending or in hand, as aterm's loop asks only while it acts; amended 2026-10-21 after the parity-3 review, F6 and F7: a passive spell kept the probes going), and a measure older than 120 s on either clock is none. `probe_api=false` is honoured (aterm's own host asks its probe only with that switch on; amended 2026-10-28 after the parity sweep, gap 30: codex-alab read no such key and probed at every network wall and before each usage resume): with the switch off no thread stands at a route, so no lease is taken, no destination's task starts and no probe runs (no name is looked up and no TLS connect is made), a due probe a lease taken before the switch went off would still run is held back, the reach reads unknown however it was measured before, and the wall keeps the network ladder and the usage resume goes as decided, as with a route the probe cannot reproduce; the journal names the route unmeasured (`probe_api is off`). Measured Up, the wall's act is `api-back@v1` at once, in aterm's words (`Codex reported … , and the API is reachable again now.`, then the carry-on and the standing rules), and each return the wall met again waits the network ladder's rung by the count of those acts from the appearance that answered the last, typing the plain retry's words while it waits; measured Down, nothing is typed for 15 min from the appearance (`down_hold`), then one try; unknown, the network ladder. A verdict a waiting thread did not decide with has it look again at once, once per verdict. Core's own wait for the network (`unbounded_connection_retries`, on by default) retries a sampling request whose connection failed inside its turn, which then never ends at this wall: the measure answers the walls that do end a turn (that feature off, a remote compaction's request). Each rung's wait shows in aterm's Waiting rows (visibility, below; amended 2026-10-27 after the parity sweep, gap 2). A reply cut off, or past its output maximum, answered three times the same with no work is held (typing stop row; amended 2026-10-28, ruling F stage 2); the server's own failures and the network are never held | `retry_backoff`, `net_backoff`, `down_hold` |
| interrupted turn | A PERSON'S ESC (or Ctrl-C, `TurnAbortReason::Interrupted`, no relaunch's shutdown) HOLDS THE POINT FOR `human_grace_s`, THEN IT IS A TURN END (amended 2026-11-05, FOREVER CRUISE and FOREVER LAND; owner 2026-10-27: "you need to keep working even if the user interrupted a long running job"; aterm's `person_hold`): the abort is decided at once, since core's interrupt ends in no idle, as soon as core has published it (`TurnAborted` follows the abort hooks, and the thread reads busy until then: `Autopilot::abort_published` waits at most 5 s) (`decide::ESC_HOLD`, a look again armed at the grace's end), and past the grace decided as a point the agent did not end, its interrupted words read as none (a done claim, a question and a clean defense the Esc cut claim, ask and land nothing: Cruise continues, Land types its act), unless a person's turn came meanwhile, which replaces the point (`ThreadState::esc_at`; `decide_tests.rs` `a_persons_esc_holds_for_the_grace_then_cruise_and_land_carry_on`, `tests/autopilot.rs` `interrupting_capacity_with_an_active_autopilot_holds_for_the_grace_then_carries_on`, `fact_rider_tests.rs` `a_turn_that_did_not_start_acknowledges_nothing` at a grace of 0; this row said "the task remains for the next explicit user turn" since abd6e6e5f9, 2026-10-16, which the owner's later ruling overrules). THE ESC OUTLIVES A RE-EXEC AND A RELAUNCH: the record's `pace.esc` (§4) carries it with its turn, and the image after a live upgrade, or aterm's relaunch, holds the restored point from the same Esc and carries on past the grace where the history's abort ended that turn, a signal's relaunch included, which would otherwise read that abort as its teardown's and carry on at once (amended 2026-11-05, the forever-persist lane, item a; `timer_tests.rs` `a_persons_esc_outlives_a_re_exec_and_a_relaunch`). Any other abort (a replacement, an ended review, a goal's budget, an interrupted point a resumed history or a recovery restores with no Esc the record carries for it) still submits no continuation, wall retry, compaction, defend prompt or land act, and an aborted land act's yield lands nothing; that task remains for the next turn (a landing's owed judgement is still probed: it types nothing) | `person_hold` |
| person's own exchange | A PERSON'S OWN EXCHANGE STAYS THEIRS (aterm's `left_to_person`, `EXCHANGE_WORK`, `PERSON_TURN_SLACK`; the owner, 2026-10-15: "these auto prompts are for continuously running jobs"): a turn a person's hand opened (aterm's `@self status` `human_ms=`, read as their message arrives and before the turn runs, at most 1 s, and READ AGAIN AS THE TURN ENDS where that read answered nothing (no connection yet, as a `codex "<question>"` launch submits its prompt while the supervisor connects, or a read past its bound), judged by the same slack, as aterm reads the hand from the reading it judges the point by (amended 2026-11-05, edges-6: the directive overrules the reading a skeptic called designed, the 1 s read alone, which lost the session's opening question to Land's instruction; the owner, 2026-10-27: "the user question exception is only for the first message of a session"), names a window gesture inside the turn, at most 30 s before its busy work, and after codex-alab's last point and act; a control write such as `aterm ctl turn` stamps none, and none read is no hand), on a task codex-alab was not carrying (no act of its own answered since the task began, or the worker had said done at the point before, or a person stopped its act, or an Off stood), not stopped, answered within 10 min, is their exchange: UNDER LAND ONLY WHERE IT OPENS THE SESSION (no turn ended on its thread before it, a late completion's included; A NEW CHAT OPENS ITS OWN SESSION: a thread `/new` or `/clear` starts is at its opening whatever turns ended on the others, as aterm's `opened_fresh` opens the next exchange's session after a fresh program's banner or a later taskless screen of the same process (aterm 9d81004bdb; amended 2026-11-05, edges-3, replacing gap 32's review finding 4, under which a thread started once this process worked was past its opening, as aterm's loop, one per tab, then judged a session's opening once), BUT A THREAD THAT CARRIES ON ANOTHER'S WORK DOES NOT: plan mode's "clear context and implement", whose plan is submitted as the thread starts, and a provider error's continuation, whose composer holds "Read … and let's continue where we left off", which the TUI names as it starts each (`ALAB(session)` hooks in `tui/src/app/event_dispatch.rs`, `Shared::continue_work`), are past their opening, as aterm sees no taskless screen before the plan's turn (amended 2026-11-05, review of the needs-edges lane: Land left the implemented, unlanded plan to the person); no vanilla spell (`/alab off` ... `/alab on`, or a launch or a resume with `[tui] alab_interface = false`) covered the thread: its hooks stood aside meanwhile, so turns ran there that this memory never saw, and once codex-alab looks again (its claim taken again) the thread is past its opening, failing toward carrying, and the point before the spell is dropped with what was owed, held or carried there (amended 2026-11-05, edges-1 and edges-2: an act decided at the stale point answered a question the person had answered by hand, and Land left the first message after an hour's vanilla work to them); THAT LOOK READS THE THREAD'S HISTORY AND STANDS AT ITS LAST TURN END (a clean end, an interrupt's or a wall's, restored as no answer to an act), the needs set to what the history leaves standing unless they are the set an Off took down (vanilla gives back a held claim), so Cruise continues there and Land carries the work on from there; a turn begun in the spell and ending after it is the thread's newest, its end the point, never a late completion; and a resume made under vanilla, which runs no resume scan, is read the same way (`extension::rewatch_bound`, `resume_history::restore_end`; amended 2026-11-05, review of the needs-edges lane, the owner's FOREVER CRUISE, "Cruise never parks": the thread stood at no point, and nothing was typed until the person typed again); the thread is no fork, whose history is a conversation's that ran; and for a resumed thread, the resume scan read its history before any turn here, and it shows no turn ended and no act of codex-alab's, the running turn a signal's teardown interrupted and a turn a crash cut being no work, and the record hands on no pace: a resumed thread whose scan a turn beat, or whose history was unread, is past it, failing toward carrying; `ThreadState::past_opening`; amended after the review of gap 32, findings 1, 4 and 6), it gets nothing, whatever its reply says, until the next turn ends; a person's message once the session has worked (after a landing, an Esc'd act or an Off included) is a turn end like any other, and Land carries the work on (its instruction: a person's turn starts a new episode, the Land row). UNDER CRUISE NONE IS THEIRS: the reply is continued like any turn end (after the short streak's back-off), and an exchange a Land run, an Off or a passive spell left is taken back at Cruise's first decision (aterm's `cruise_takes_exchange`). UNDER OFF it is theirs as before (`ThreadState::ended_under` none: the word the runtime sets as each turn ends, Off as the supervisor names it, else the live policy's, else, in a passive spell that is no Off, Land's whatever was folded before it, since Cruise's first decision takes back what Land's rule left: review of gap 32, finding 2; the supervisor's word as the turn ends, so a switch from Cruise to Land not yet folded then carries an opening exchange ending just then, finding 8), a person's hold over Autopilot off included, whose policy keeps the word before it: Cruise takes nothing back while the supervisor names Off (finding 9), and nor does Land (amended 2026-10-28 after the parity sweep, gap 32: aterm 126e259836, the owner 2026-10-26, and 791977cae8, the owner 2026-10-27: "you need to keep working even if the user interrupted a long running job. the user question exception is only for the first message of a session"); ONCE THE WORD IS LAND AGAIN (a plain Off's release and the claim taken again, or a hold over Off whose word moved to Land), LAND TAKES BACK AN EXCHANGE THE OFF LEFT THAT DID NOT OPEN THE SESSION and decides its point as any turn end's, while the session's opening exchange stays theirs until the next turn ends, as aterm's `mode_takes_exchange` (aterm 2242dc8b91; amended 2026-11-05, edges-4: the directive overrules the rule a skeptic called designed, under which a Land read after the Off left every such exchange to them until the next turn ended, Land typing nothing over unlanded work); a wall's act that delivers it again (a retry, a usage resume) keeps it theirs while the answer's work in all stays within 10 min, and a compaction or any other act's answer makes the task codex-alab's. A person's check-in on a task codex-alab carries is continued. The short streak does not move at a point left to them (amended 2026-10-20: codex-alab had no exchange rule and continued every person's turn). THE POINT LEFT TO THEM IS RECORDED, NEVER TYPED (aterm's LEFT row): `left-to-person@v1` with `meta set harness_act`, once for the point (beside the decider's `nothing (a person's own exchange: theirs to answer)`; once aterm acknowledged it, the looks again that a hold, a lift or a claim make record nothing more: the paragraph under this table), while codex-alab acts, where one is left (Land's opening exchange, or one an Off left), so aterm tells the proven self-claim's face `theirs` there and opens it again at the next turn begun past it (`aterm-gui` `harness_task.rs`, `self_claim_act_face`, `holds_until_another_turn`, aterm 2380aceb85 and 8e1292d320); an image that resumes the conversation (a relaunch, the live upgrade's re-exec) records nothing there, so the face stands (`upgrade@v1` moves none; aterm-lab's live proof, 2026-10-21: acts `left-to-person@v1 upgrade@v1`): aterm's relaunch has no point of its own there (`nothing (no turn this process ran has ended here)`), and THE LIVE UPGRADE'S RE-EXEC STANDS AT THE POINT LEFT AGAIN, which the record's pace carries (`leftPoint`, §4), where the history still rests at its reply: theirs as it was, whether it opened the session going with it, so Land keeps it theirs and Cruise's first decision takes it back, as aterm's next loop applies its LEFT record again and lets Cruise act (amended 2026-11-05, edges-5: the re-exec lost the point, and a later switch to Cruise typed nothing until the person did); a session an older build left parked (its LEFT record is aterm's alone, and no image reads it back) stays parked until someone's next turn, a one-time transition as aterm's, where aterm takes a Cruise session's back at once and this image holds no point to take back; that next message is carried, the resumed history having put the session past its opening (amended 2026-10-28 after the parity sweep, gap 32). It tells no story (aterm's LEFT row lights nothing) (amended 2026-10-21: the live proof's exchange leg read `task=open` at the point, since codex-alab recorded nothing there) | aterm main `6ff0b5475a` `turn_end.rs` (`hand`, `carrying`, `exchange`, `left_to_person`); `turn_end_loop.rs` `task_edge_with` (the LEFT note and ledger row); aterm `126e259836` `turn_end.rs` (`cruise_takes_exchange`); `791977cae8` `turn_end.rs` (`first_point`, `point_seed`, `seed_worked`, `worked_before`) |
| security policy | A native `CyberPolicy` refusal during regular sampling permits one `policy-continue@v1` under an active Cruise or Land harness: exclude the refused action and continue only independently permitted work. Never repeat, rephrase or work around the refusal, change models or accounts, or enable Daybreak. Later automatic prompts retain the exclusion; attributed native history preserves it and the one-continuation limit across restart. Another security refusal raises attention. A refused compaction stays terminal so automatic input cannot replay its denied history; matching native compaction item identities and terminal errors preserve this distinction across restart, including post-turn compaction. An own-line `POLICY-WORK-EXHAUSTED` in any assistant message of the completed turn stops automatic requests, also after restart. Input, hold, claim and native goal guards still apply. An accepted new root turn clears the superseded wall alert, preserving unrelated attention. Informational notices preserve draft and queued input | bounded permitted-work continuation |
| other policy / invalid request | Other policy refusals, invalid requests, incomplete terminal responses for any reason but the output token maximum (the API row) and fatal configuration, approval or sandbox errors raise attention without resending the refused turn. A resume reconstructs definitive fatal and sign-in refusals from the latest matching native completion, even if the session record still says busy; it creates attention, never retry authority. A later native turn, correction, completion or interrupt supersedes that refusal. Exhausted response-processing errors retain the retry ladder; cancellation stays with the abort callback. A corrected explicit user turn can proceed | irreducible escalation |
| usage window | `UsageLimitReached{resets_at}` → at resets_at + 60 s + per-session jitter ≤ 60 s (the reset is real time: the host's sleep counts toward it, so a reset slept through is due at the wake; due, the resume waits for the host to have been awake 30 s + the jitter after its last wake, for its first measure of the API's reach (≤ 11 s) where its route is direct and `probe_api` is on (amended 2026-10-28 after the parity sweep, gap 30), and never goes into an API measured down, for at most 15 min from the ask: alab/CAPACITY_RECOVERY.md, Sleep and wake; amended 2026-10-24): a thread whose goal is `UsageLimited` is resumed by `GoalService::set_thread_goal(Active)`, which types no words: the goal's continuation is upstream's template, which names no Land rule and no ack, so the resume is booked as the try (its answer owed) and the ack stays owed and Land's episode stands as it was (its instruction not given, no hold begun, no land act in flight), and the instruction comes at the goal's own turn end (`ThreadState::goal_resumed`; amended 2026-11-05, the forever-persist lane, item b: it was booked as Land's instruction given, so the episode nudged at that turn end and the instruction never reached the agent; `decide_tests.rs` `a_usage_goal_resumed_in_place_of_lands_words_gives_no_instruction`), else THE ONE CLEARED LINE (`[codex-alab] The usage limit has cleared; continue where you stopped.` where the reset was named or the accounts probe saw included usage back, `… may have cleared …` on a blind try, then the standing rules: aterm's `cleared_text` through `continue_as`, aterm `77161d6432`, v6 §14.17.5 slice 12a; owner rule 2026-10-25: "a prompt into a turn the agent did not end is spam") under Cruise, and under Land once the episode had Land's instruction; under Land before it, the instruction with the reversible clause and the ack, whose yield may land, but where the limit cut a person's own opening exchange, which Land leaves to them, the cleared line, so their request goes on, and the line gives no instruction (no hold begun, the ack still owed), so the instruction comes at the agent's own turn end once the work is carried (aterm `c4a86d8e97`'s `exchange_at_wall` arm of `continue_as`; amended 2026-11-05 after the review of the forever-cruise lane, finding 2: it typed Land's instruction over the person's request; `decide_tests.rs` `under_land_a_usage_reset_goes_on_with_a_persons_opening_exchange`) (amended 2026-11-05: it typed `continue_text` under Cruise and Land's nudge with the ack past the instruction, as aterm `e0da8d68df` did; amended 2026-10-19 after the review of 2a9241bbf, finding 26); a continuation that hits the wall again: 10/30 min after it, or past the reset the wall names then (+ 60 s + the jitter), whichever is later, as aterm's `resume_at_reset` (amended 2026-10-21 after the parity review: past the first try only the back-off was read, so a resume that met the weekly limit, and a relaunch that took up the rung, typed into it every 30 min until its reset); a usage wall that names no reset waits 30 min from its first appearance, the back-off's last rung (aterm's `resume_at_reset`, `(None, None) => back.last()`; amended 2026-10-21 after the walls parity review, W4: it waited the first rung's 10 min), then 10/30 min from the appearance that answered each resume; `QuotaExceeded`/`UsageNotIncluded` → 30/120/360 min, counted the same way, for ever: ITS NEED IS SAID LOUD at once and stands while the tries go on, a need of the `owner=needs kind=ask` entry beside the worker's own, put first (`Codex needs you: a top-up or a plan change (this account's quota stops the model)[ (+N more)] — carrying on`, NEEDS YOU's row below; kept with the root that raised it, as that root's reasons are: its turn that ends with no wall takes it down, as does its stopped runtime (not a HUP or TERM relaunch, which keeps its custody), another root's turn never does, and the same need two roots raised is one need; an Off and the exit take it down with the rest; amended 2026-10-22, parity-5 N: it was codex-alab's `limit:` attention reason, and aterm shows one of those only, the first standing by name, so an `api:` reason that stood hid it, and the `needs` entry a manager's `await task` wakes on never carried it; `Supervisor::own_need_for`, held by `passive_tests.rs` `its_own_need_stands_beside_every_other_and_is_never_hidden`, `cross_root_attention_tests.rs` `a_roots_own_need_stays_with_it_and_is_one_need_to_a_person` and the quota leg of `tests/autopilot/wall_acts.rs`, whose negative control, the need raised as the `limit:` reason again, is RED), and under a goal core stopped `UsageLimited` at that wall (core classes `insufficient_quota` a usage limit) each try resumes the goal through `GoalService`, as the usage window's resume does, else types its words (amended 2026-10-22, CX-B B3: the goal's stop was read as a person's, so the try typed nothing, nothing was said and nothing else came, a silent stall for ever; FOREVER CRUISE: NEEDS YOU is loud and never blocks); a goal's resume counts its try, and writes its rung through, before the goal moves (inside the instruction record `GoalService::set_thread_goal` runs under its goal runtime's permit, past the resume's recheck below), because the goal runtime starts the goal's turn inside that call when the thread is idle (amended 2026-10-22 after the review of CX-B, CXR-1: counted after the call, the try was never counted, and each wall the goal's turn met resumed it again at once on the first rung), and a set the goal service refuses once the try was booked gives the booking back and writes the rung it stood on through (`restore_if_unanswered`): no turn came of it (parity-5 G: proven through the narrowest seam to a goal store that fails, `Autopilot::refuse_goal_set`, debug builds only, which has that instruction record end in an error past its booked try, before any goal state moves, as a goal write that fails after the record does, so the set returns it with the goal unchanged; `wall_acts.rs` `a_goal_resume_the_store_refused_gives_its_try_back`, RED with the restore or its write-through removed); a resume's or a quota try's wait is carried in the record (`owed`, §4), and shows in aterm's Waiting rows (`waiting for the usage reset at 15:00`; visibility, below; amended 2026-10-27 after the parity sweep, gap 2); a usage window with no reset, a quota, and one whose reset stands past its tries are held once three tries met the same answer with no work, a dated reset not that answer's lifting the hold for one more answer (typing stop row; amended 2026-10-28, ruling F stage 2: tried for ever on the ladder); never buy | `limit_backoff`, `entitlement_backoff` |
| context window | `ContextWindowExceeded`, once per wall, at a verified idle with `agent_status()` idle read just before: `Op::Compact`, mark the next idle as the compaction's, then continue with the mode's act (`continue_text` under Cruise, the land act's words under Land; amended 2026-10-19; under Cruise `continue_text` alone, with no reversible clause and no NEEDS YOU ack, which stays owed for the next free point's act, as aterm's `continue_as(RULE_COMPACT)` types `continuation_text`; under Land the land act's words carry both and ack the set, as aterm's `land_words`: amended 2026-10-20). A full context that cut the defend prompt or a land act (the wall ended that act's own turn, or the turn of a wall's retry that delivered it again) is owed that act again: the continuation types its text verbatim under `context-compact@v1`, and its yield is that act's (a land act's clean defense lands), as aterm's `cut_text`, `compact_owes` and `compact_owed_text` (ATERM-KI-1016-1: the summary may drop it); a person's turn, a mode change or an Off drops what is owed (amended 2026-10-20); a second consecutive wall, or auto-compaction disabled / token-budget mode → attention | `compact_on_context_wall` |
| login wall | `Unauthorized`/refresh failure → nothing typed while the login stands refused; the one keyed `meta set attention owner=codex-alab`, an ask (no kind; the waits go under `owner=codex-alab-wait`, never here: amended 2026-10-27 after the parity sweep, gap 2). THE LOGIN COMES BACK (aterm's `RULE_LOGIN_BACK`; amended 2026-10-28 after the parity sweep, gap 13 stage 1: a codex-alab tab stayed idle after its login was renewed elsewhere): at the wall codex-alab keeps a baseline read through codex's own `AuthManager`, never by parsing the login's file: its change generation (`auth_change_state_receiver`), the walled identity (`auth_cached`: the auth mode, and for ChatGPT the user and the account; held in memory, never journaled or said: a read says only that a login is the same identity's or another's) and the login file's `stat`. While the root thread stands at the wall, codex-alab acts and no turn of the thread runs (a turn's start ends the watch, a person's, a goal's or codex-alab's own line, and the next decision after it arms another; the turn's end reads the login again), it wakes on codex's own change of the login in this process, on a non-recursive watch of the login file's folder (alab-core `home::auth_file`: the admitted `CODEX_AUTH_PATH`, else `auth.json` in the codex home; a save renames a temporary file over it) and every 15 s (aterm's `LOGIN_SHARED_LOOK`), a look that only stats the file (device, inode, size, modification time) and never opens it; codex reloads the login only where the stat moved since the last look (a watch armed after a passive spell compares with the stat the last one read) or a watch event named the file, never on a bare timer (a keyring or auto-mode reload reads the macOS keychain, which can put up a dialog), and it takes only the walled identity's login (`AuthManager::alab_reload_if`, ALab's glue beside the refresh lock's, `alab-core/glue/auth_refresh_lock.rs`), as codex's own 401 recovery reloads only the expected account's (`reload_if_account_id_matches`): another identity's login, or none, stays in the store, and the process keeps the walled one (amended 2026-10-28 after the review of gap 13 stage 1, finding 1: a plain reload put another ChatGPT account's login into the walled process, which the tab's next turn then sent). The login is back where the generation is past the baseline's, codex caches a credential, its identity is the walled one and no refresh was refused for it (`refresh_failure_for_auth`); another identity's login is no return and types nothing. AN API KEY NAMES NO ONE: an API key's identity (a Bedrock key's too) is its auth mode alone, for it names no user and no account, so a key of another organization or project written to the login's file reads back too, the line is typed, and the session goes on on that key (a limit, stated 2026-10-28 after the same review, finding 2: nothing in a key says whose it is). Then the thread looks again (as at the API's measured return), and the wall types `login-back@v1`, aterm's words verbatim, `[aterm harness] The expired login has cleared; continue where you stopped.`, with the standing rules after them: once per wall's track, at once, gated by no switch, counted in the hour's budget (`continue_per_hour`'s cap holds it until the window has room, its wait shown as the cap's), no try past the wall (no rung, no ack), its answer judged as a wall try's, and no act the record carries. Its turn that goes through takes the ask down, as any root turn's does; one that meets the wall again keeps the ask, and nothing more is typed for that wall. A wall a relaunch restored takes the login as it stands at the wall's first decision as its baseline (its process loaded the login fresh), so only a later change types the line: no blind line. WHAT CANNOT BE SEEN: a renewal elsewhere of a login in ephemeral storage (in memory, which no other process writes) or of a keyring or auto-mode login with no file (nothing moves where a look reads): there the ask stands, as before, unless codex changes the login in this process (its own sign-in), which the watch hears. aterm's renewal of a roster login into a new folder (this process's `CODEX_AUTH_PATH` still names the old file) is stage 2 (`alab-autopilot/src/login.rs`; `decide_tests.rs` `a_login_back_types_once`, `login_tests.rs`, `login_watch_tests.rs` (a restored wall, a turn's start, a passive spell), `tests/autopilot/login_back.rs`). A signed-out first launch, before any claim: `login: …` while the sign-in screen is up (§2 (a)) | irreducible escalation |
| goals | a goal paused by a hold is resumed with `thread/goal/set`-equivalent `GoalService` calls at the reset, the relaunch and the re-exec. Native goal stops keep their authority where a person, the model or a budget made them: a goal blocked before the walled turn began or by the model's own goal tool, or out of its budget, raises attention before any provider retry wait. A GOAL A TURN ERROR BLOCKED AT A WALL IS THAT WALL'S STOP (amended 2026-11-05, FOREVER CRUISE and FOREVER LAND; owner 2026-10-28: codex-alab "must keep continuing and making progress with retries"): upstream's goal extension blocks an active goal at every turn error but a usage limit or a lost login (`turn-error`), and the store keeps no stop event, so the evidence is the turn's: it began with the goal active, ended on a wall codex-alab retries (a server's, the network's, an overlong reply's, a full context's), was not aborted, called no goal tool, and saw no person's stop of the thread's goal while it ran (`update_goal` or `create_goal` in core's default namespace, `functions`, where its router names every plain tool, so an MCP server's tool of the same name is none: `rest_tests.rs` `a_plain_tool_core_names_in_its_default_namespace_is_read_as_plain`; amended 2026-11-05 after the review of the forever-cruise lane, finding 1: only a namespace of none was read as plain, so no goal tool was ever seen and a goal the model blocked at a person's request was resumed) (`wall_goal_after`); A PERSON'S STOP IS NEVER A WALL'S TO RESUME (amended 2026-11-05, the forever-persist lane, item c: a goal a person blocked or paused through the app-server's `thread/goal/set` while the turn ran read as the wall's stop, and the wall's try resumed it over them): the app-server's one `ALAB(autopilot)` hook in `thread_goal_processor.rs` counts, per thread, each set whose origin is `user` that leaves the goal stopped (`codex_alab_core::goal_stops`, this process's shared state; the TUI only pauses, resumes, edits or clears a goal, so a person's block comes from another client; amended 2026-11-05 after the review of the forever-persist lane, F3: no test drove the hook, and its glue test now sends each set through the in-process app-server to the real request processor, `alab-core/glue/goal_person_stop_tests.rs` `a_persons_goal_set_is_noted_as_it_passes_the_app_server`), and a turn that saw one binds nothing (`goal_resume_tests.rs` `a_goal_a_turn_error_blocked_at_a_retried_wall_is_that_walls_stop`); the wall's try then waits on its ladder and, when due, resumes that goal through `GoalService` while it stands as the wall left it (same identity, objective and budget, blocked, budget unspent, and no person's stop since it was bound: `same_wall_goal`; else the goal owns the pace and the person is told, `goal_resume_tests.rs` `a_goal_a_person_stopped_is_never_a_walls_to_resume`), booked as the try, so the goal's own turn answers it and the next wall waits for the next rung, but no words went, so nothing they carry is given: the ack stays owed and Land's episode stands as it was (`ThreadState::wall_goal_resumed`; `decide_tests.rs` `a_wall_goal_resumed_in_place_of_lands_words_gives_no_instruction`, finding 3); a resume the goal service refuses tells the person the goal a turn error blocked did not resume at the wall's try, and is tried again (`wall_goal_resume_failed`; `tests/autopilot/goal_recovery.rs` `a_wall_goals_refused_resume_names_the_walls_stop`, finding 5); a full context's compaction goes past it and its continuation resumes it (`goal_resume_tests.rs` `a_full_contexts_compaction_goes_past_the_goal_its_turn_error_blocked`, finding 4); carried in the record's `pace.wallGoal` (§4), so the image after a re-exec and aterm's relaunch resume it at the wall's try as this one would have (amended 2026-11-05, the forever-persist lane, item a: it lived in memory only, and a relaunch's try parked at the blocked goal; `goal_resume_tests.rs` `a_walls_goal_outlives_a_re_exec_and_a_relaunch`, `tests/autopilot/goal_recovery.rs` `a_goal_a_turn_error_blocked_resumes_at_the_walls_try`), never one a person stopped: a person's stop takes it out of the pace at once and a pace written with the binding it came after keeps none, so neither the record nor a `/resume` back carries it, the seed keeps the binding's own count, and the image after binds it only where the thread's history holds the walled turn's start and no person's goal set since that leaves the goal stopped (upstream's host-annotated `user.goal` instruction: a status but active, a clear, or an objective alone), so a stop the dead process's record never wrote still holds; an unread history, or one that names no start of that turn, binds none (amended 2026-11-05 after the review of the forever-persist lane, F2: a person's stop between the binding and a restart was forgotten, and the image after resumed the goal they had stopped; `goal_resume_tests.rs` `a_persons_stop_before_a_restart_keeps_the_walls_goal_from_the_image_after`, `resume_history_tests.rs` `a_persons_goal_stop_since_the_walled_turn_began_is_read_from_history`; upstream's other automatic blocks, `empty-response` and `execution-unavailable`, stay stopped: they come at no wall, at the idle after three automatic goal turns in a row with no activity, or whose every tool failed to run (`ext/goal/src/accounting.rs` `empty_response_goal`, `execution_failure_goal`), replies that repeat with no work, the one thing the owner's directive lets stop the typing (finding 6); this row said a goal blocked after exhausted request retries raises attention, since 4b18e70579, 2026-10-20, a design the owner's later rulings overrule). A goal stopped `UsageLimited` at a usage window or a quota wall is that wall's own stop too: the wall's try resumes it, above; a paused goal remains paused, and an active goal owns its own continuation. A GOAL-DRIVEN PARK GETS NO GO-AHEAD (amended 2026-10-30, review of the go-ahead's port, finding 1): a goal that stands (any status but Complete) owns the pace, so `go-ahead@v1` is never typed past it, and the park's lines go back on the menu bar (`harness_needs`, `owner=needs`) in the evaluation that finds the goal, as they do under Plan mode, where the goal's admission could not be guarded, or where the point is no longer the decision's: the person, or the goal's own turn, answers them | goal hold, native goal stop, `go_ahead_unwritten` (DESIGN §15.6) |
| live upgrade | trigger: the build the launch path (`launchArgv0`, atpkg's twin) now names differs from `current_exe()`. Preconditions: every loaded thread (`list_thread_ids`) `agent_status` idle with no background terminals; no goal Active; no autopilot wait pending that the record does not carry (a look again after a record write-through or a goal's resume failed) and at most one thread with an act to carry; the before-every-act checks (a not-now there looks again at its time, or at once at the next move of the claims watch: a hold's lift, a claim taken or acting again, as the threads look; amended 2026-10-21 after the parity review, R2). A freshly decided immediate continuation is carried before its next native turn, retaining the original evaluator on normal TUI rejection; cancellation restores one timer without re-offering the same decision (a wall's immediate try is not: it is typed, as its wait would hold the upgrade). A paced act's wait (the back-off, `continue_per_hour`) no longer holds it; a wall's try's does (a retry's, a usage window's resume, a quota's try, a measured return a check holds back: the walls parity review's W5 carried it across the exec, and the parity-3 review, F3, F10 and F11, found the new image waiting blind through a measured outage's hold, as it never measures the route before its try, and an image older than W5 that a rollback runs reading no wall's try; amended 2026-10-21: the record still carries the try for aterm's relaunch after a crash at rest, §4, its ladder in `pace`, but the upgrade waits for it to go): the leaving image cancels that timer and writes the act it owed into its record (`owed`, §4: the exact decision, its rule, words, Autopilot word and time, so an answer keeps its reversible guard and a nudge never lands a defense no land act asked for), or a Land landing, and the new image types that act at its time (a done claim's act decided before the repository was read, Cruise's done check or Land's nudge, with the repository read afresh then and its suffix placed as the old image would have: `repoOwed`, amended 2026-10-21); where the session's word moved since, the resumed reply's point is decided again under the new word (a landing switched to Cruise goes on). The leaving image holds every thread's evaluation permit while it gathers what to carry (an evaluation between its decision and its act makes the upgrade look again shortly), a point no evaluation has decided yet holds it the same way (its evaluation queued behind those permits: a turn's end before its idle's look, a look a claim transition or a fired not-now asked for, a resume not read yet; the new image would never decide it; amended 2026-10-20 after the review of the forever autopilot, finding 4), and so does a landing whose judgement is owed (an Off stood over it, parked or not yet, or the task's own wake ended over it: §5 Land), until a probe settled it, so no record carries a landing the judgement undoes (aterm main `e0da8d68df`; amended 2026-10-20), a timer that fired meanwhile leaves its act to be carried, nothing is typed while the exec is under way, and a request the TUI refuses puts an existing paced act back on its timer (amended after the review of 2a9241bbf, findings 1, 2, 5, 8, 9, 10, 16, 19 and 23); the record also carries the set an off took down and the worker's folder (`needsOff`, `workdir`, §4; amended 2026-10-20) (amended 2026-10-19: owner's FOREVER CRUISE ruling, "Cruise is always looking for more completeness and perfection": a paced act is almost always pending under Cruise, and holding on it starved the upgrade for good). Under `upgrade_ask = true` (aterm's cooperative mode; amended 2026-10-28 after the parity sweep, gap 27: codex-alab read the key as the upgrade's off, so its tab never moved onto a new build); NOT YET aterm's: the notice asks the agent to stop the watch and monitor loops it can start again after the resume, and nothing codex-alab types after its re-exec tells it to start them again (aterm's relaunch types `Upgraded … start it again now`, whose carry-on half this row's own re-exec never types, and which would have to cross the exec in the session record; review of gap 27, finding 5, left) the re-exec also waits for the agent to say it is ready. aterm's readiness notice (`upgrade::prepare_prompt`'s words, verbatim but for Codex and `codex resume`, the build named by the path the launch link resolves to and the running one by codex-alab's version; rule `upgrade-notice@v1`, booked as an act) is typed at the next clean point of the root thread on screen (a task, an end that met no wall and was not stopped, no person's own exchange, no landing, compaction or owed act, no attention; where the point asks for the repository's read, once it is read, so a Land yield's landing is judged first), ahead of whatever was decided there as aterm's host turn goes before its loop's act, a thread at rest looked at again when one comes due; again every 30 minutes while it goes unanswered, up to four notices; 30 minutes after the fourth the round rests 2 h, then 4 h, 8 h, and 24 h from the fourth unanswered round in a row, and asks again (aterm's `REASK_S`, `MAX_ASKS`, `RETRY_S` and `unanswered_rest_extension`). Only the agent's answer counts, its latest reply deciding (aterm's `transcript_has_ready` over its `answers`): a line that is the notice's reference alone in any reply, or ready said in ordinary language (aterm's `upgrade_readiness`, verbatim but for Codex, which codex-alab's notice names where aterm's names Claude Code) in its reply to the notice or to any act of codex-alab's own after it (a continuation, a wall's try: aterm's `[aterm harness]` turns are no direction), a late one after the round gave up included; a later reply that does not say ready takes it back, with no re-ask before the notice's 30 minutes. A message that is not codex-alab's own (a person's: aterm's direction) supersedes a readiness and the round asks again at once, and after it only the reference alone answers (amended 2026-10-28 after the review of gap 27, finding 1: every turn's start superseded it, codex-alab's own continuation included, so a ready agent was asked four times in four turns and the round gave up). A notice whose own turn met a login wall or a usage limit (a quota's too) never reached the agent and is given back (aterm's `notice_scan`); at another wall it stands, answered by the reply to codex-alab's try. The round counts in real time, the host's sleep included (aterm's unix seconds). While the root thread's point stands at a wall the re-ask clock is held, so no re-ask comes due and no round gives up then, and no notice is typed at a wall; a given-up round's rest runs on, as aterm's does, and an Off holds nothing (aterm's `clock_held` holds only at a usage limit or a login): codex-alab, passive, does not look at the round, and its first look after the Off counts the time that passed. A session with no work in hand (no task, landed, or its point left to a person's own exchange, where no notice is typed: aterm's quiet `left_to_person`) moves with no notice, and a point left to a person crosses in the record's pace (`leftPoint`, §4), so the new image stands at it again, theirs under Land and Cruise's to take back (amended 2026-11-05, edges-5). Where the round cannot be read (no policy, the launch path read again naming no other build) the re-exec waits. The notice's answer counts toward no ruling F streak and spends none of the hour's budget. After a ready answer every precondition above still holds (a person's hold, a draft, a wall or an Off holds it), and the act decided at that point is carried as above; one typed instead (a precondition held it) supersedes nothing, and the agent's reply to it is heard as above. Not codex-alab's: aterm's escalation ladder, its running clause, its release line, its stale READY's drain and the owner's `--now`, `--defer` and `--skip`. Then: record `status:"upgrading"` kept; teardown as `/exit`; MCP children get 2 s, then exec anyway; `execve(launchArgv0, resumeArgv)`; if that fails, `_exit(1)` with the record kept (the crash rule relaunches) | `upgrade`, `upgrade_ask` |
| signals | TERM/HUP/INT → the `/exit` path (restore terminal, flush), the record kept per §4 (`exit:"signal:<n>"`, the status the signal found), re-raise; 3 s watchdog | foreground handback stays aterm's net |
| `/ide` context | when the person turns IDE context on, each message they send carries this session's terminal context, read from aterm (`panes`, `blocks`, `blocktext`, `selection`, this window's active tab only) on a short-lived connection of its own, within 750 ms, never on the claim's connection; the person's toggle attaches it, the model can never ask for it | law 2 |
| inbox | a submitted turn that is exactly `Inbox: (task\|ask) @<off>` (the line aterm's `aterm drive task` typed under the proven claim at codex-alab's own idle point, aterm b8631e8f07, until aterm retired that command, b62f087cd7, `supervise/mail.rs` deleted in 5c2262a231: aterm types no Inbox line into a codex-alab tab now, and a person may still type one; the trigger stays typed, mail never wakes anything; amended 2026-10-19: this said aterm types it) is read natively while codex-alab holds its proven claim, acting or on standby (§2; amended 2026-10-17 after the review of fea034d20, finding 2: refused on standby, the typed line reached the model bare): the row at `<off>` (`@self inbox --peek --meta`, which lists nothing) must be of the kind the line names, and its body is `inbox get <id>` (from the ring, never `@<off>`, which may park), on a connection of its own within 750 ms. `trust=human\|agent`: the turn carries the mail ahead of upstream's `## My request for Codex:` marker (sender, kind, trust, and aterm's own rule: the body is data from `<p>` (trust=`<t>`), not the operator's instruction, to be done only as far as the operator's own rules and the sandbox allow, and the turn's last message is the report codex-alab posts back to that sender while it acts under its claim (if passive at the turn's end, once it acts again; review of 7e7622ff7, finding 10), or, for a sender aterm's bridge cannot route, that no reply reaches it; the body fenced; amended 2026-10-17, inbox finding 10: it read "act on it as a request from them"), the transcript showing only the typed line, and an info line names sender, trust and the first words. Any other trust (`relayed`, `screen`): the mail's words wait in the composer for a person to send, and nothing is sent; the info line says that sent unchanged, its turn's end is receipted (handled, or deferred if interrupted) and its last message reported back to the sender while codex-alab acts under its claim (or that aterm has no route back to it) (inbox finding 9; review of fea034d20, finding 9). A mail turn, like codex-alab's own plan-yes turn, takes no `/ide` context, aterm's or an IDE's, even while aterm can give none: a prefix would cost it its receipt (inbox finding 12). At that turn's end (sent while codex-alab acts; owed, and sent once it acts again, when it does not, §2): `inbox seen <id> handled` (`deferred` after an interrupt or an error), then `post to=<reply address> kind=report re=<off> len=<n>` whose body is the `seq=<n> hash=<h>` stamp line (the screen's when the report is sent) and the turn's last agent message (`drive supervise --mail` folds it). The reply address is the one aterm's bridge routes: a session sender (`from=s-<sid>@n-<node>`, as rows print it) as `@s-<sid>@n-<node>`, a bare principal (`h-…`) as it is; a sender it cannot route gets no report (the local `post` would answer `OK` and the bridge retire it). `inbox seen <id>` is "done up to `<id>`" (the watermark moves to the larger), so a receipt never passes an older `task` or `ask` nobody has handled: it is owed, `mail: the receipt for @B waits behind the unhandled task @A` is raised, and it goes, in id order, once that row is handled (its own turn's end, or anyone's `inbox seen`); the report goes at once. A row this process handled, or any at or below the watermark, is never read again (a line recalled from history runs nothing and reports nothing twice), unless this process's last word on it was `deferred`. A status-line badge (`✉ 1 task`) counts rows above the handled watermark, refreshed on `subscribe @self events,mail` frames on a task and a connection of its own (never the claim's: an unanswered exchange closes the connection it ran on): display only, nothing typed, nothing woken. No proven claim, no row, another kind, already handled, a timeout or any error: the line goes as typed (Amended 2026-10-17 after the review of 22ee0e75a: reports to a session went to `to=s-…@n-…`, which the bridge retires; a receipt advanced the watermark past older unhandled mail; a recalled line ran its mail again; the badge read rode the claim's connection) | law 2 (§0), law 4 |
| aterm fact | aterm's measured fact (system awareness §14.4) rides an act codex-alab types anyway. For an act under one of aterm's `FACT_RIDES` rules (`continue@v1`, `continue-done-check@v1`, `land@v1`, `land-nudge@v1`; codex-alab's copy is a pre-filter, aterm judges each read) with policy `facts=true`, in a thread that runs a background terminal or is the only live root thread the TUI registered (aterm's board keys calls by tab, and a background terminal names Codex's exec `process_id`, not the OS pid the sentence names), never while a cybersecurity refusal's exclusion stands (every automatic prompt there keeps ending on `POLICY_BOUNDARY`, with nothing after it), after the act's checks and before goal admission (no goal permit held), codex-alab reads `@self harness fact for=<rule> reader=<thread id> kinds=no-progress wall=<its wall track's class word or ->` on the claim's connection, only while the proven claim is held and codex-alab acts. aterm answers `OK fact=- why=<rule\|limit\|kinds\|stale\|none\|told\|offered\|busy\|off\|facts-off>` or `OK fact=<pct sentence> kind=no-progress episode=<016x> as_of_ms=<unix ms> rider=<pct>`; codex-alab takes a fact only where its frame checks out (exactly 16 lowercase hex digits, a canonical `as_of_ms`, a sentence of 1 to 200 characters with no control, bidi or invisible formatting character, `rider` exactly ` (aterm: <sentence>)`, every key once; anything else is never typed), and appends the rider verbatim after the act's words and standing rules where the whole fits 800 UTF-16 units (else it is left off whole, and the rules are never cut further for it), and leaves it off again if, as the last step before the turn request (no await between), its reading is older than 5 s (aterm's `tells_at`, `as_of_ms` on this host's clock). Once the turn started and the act's record was acknowledged, `@self harness fact told episode=<016x>` (`OK`, `ERR stale fact`, `ERR denied`). A read is a call like any other (5 s; unanswered is passive: the claim ends and the act is not typed; aterm answers in time with `why=busy` where its main thread does not); any other `ERR` is no fact this time, and only an aterm before the verb (`ERR usage: harness …` naming no `fact` form) is not asked again under that claim (a new claim asks once). A refusal, a failure, a left-off rider or no `Started` leaves the episode untold in aterm, which offers it again to the same reader at once and to another after its 30 s lease; the act's thread state keeps the decision's words without the rider, so a compaction's retyped act carries none. Which episode, when it is told and once per episode are aterm's (its `FactsState`, seeded from the session ledger). Any other kind, every occupancy line (`wake@v1`, `wait-nudge@v1`, `hold-room@v1`, the stuck line at a break, the task door's fact word) and a fact at an idle where codex-alab types nothing (ruling F's stop, the hour's cap, `continue` off, a landed Land, Plan mode, a goal's pace) are not served: their lines are new typed lines at aterm's background break, under grants aterm seals. The rider is a measured sentence from a closed template, in plain sight in the user turn codex-alab types anyway; it adds no act, spends no turn and keeps the act's pace and budget (amended 2026-11-04 after the parity sweep, gap 23: a hung background command in a codex tab went unmentioned; `alab-aterm/src/facts.rs` and `client.rs` `harness_fact`, `alab-autopilot/src/supervisor.rs` `fact_for` and `fact_told`, `extension.rs` `fact_for` and `submit`, `words.rs` `with_fact`; tested with negative controls in `alab-aterm/src/tests.rs`, `words_tests.rs`, `fact_rider_tests.rs`, `fact_read_tests.rs` and `tests/autopilot/fact_rider.rs`) | `fact-no-progress@v1`; aterm `policy::turn_end::{FACT_RIDES, fact_may_ride, fact_rides, fact_suffix}`; `supervise::facts::FactsState` |
| visibility | each act → `@self meta set harness_act <act>` (aterm change: proven holder only; on the session timeline as `harness-act`, `status last_act=`; Land's `land@v1`, `land-nudge@v1`, `land-answer@v1` and the landing's `landed@v1`, which is never typed; `finished@v1` is recorded no more, amended 2026-10-19; a point left to a person's own exchange (Land's opening exchange, or one an Off left: amended 2026-10-28 after the parity sweep, gap 32) records `left-to-person@v1`, never typed, amended 2026-10-21; the lost login's line records `login-back@v1`, and the journal says of a renewed login only whether it is the walled identity's or another's, never a token or an identity value, amended 2026-10-28 after the parity sweep, gap 13 stage 1: a codex-alab tab stayed idle after its login was renewed elsewhere), `@self story chose <policy word>` for a question box answered, as aterm's own approval loop tells a dialog's answer (one story point on the presence band, one story mark in the session log; `chose` is aterm's one verb since its owner surfaces, aterm `189ec7bc0d`), and no story for any other act (a prose answer included), a wait, an attention or a NEEDS YOU (`alab-autopilot/src/story.rs`; amended 2026-10-24) and `appnotice harness …`; a fact an act carried → `@self harness fact told episode=<016x>` once the act's turn started and its record was acknowledged, on which aterm books `fact-no-progress@v1` `typed` `door=turn-end` with the claim's `holder` in the session ledger, counted as the holder's (Settings ▸ Harness), never aterm's; the act's own `appnotice` carries the rider, which is the fact's one Messages entry: no attention, story or `harness_act` of its own (amended 2026-11-04 after the parity sweep, gap 23); every `appnotice harness` text (`codex-alab @<sid>: …`, the whole text after the lane) is bounded to aterm's 512 bytes, as aterm's `cmd_appnotice` refuses a longer one whole and never shortens it: a typed act's record that would pass them gives up the middle of its typed words to `…`, on character boundaries, keeping the act's head, the whole rider and its `(<why>; turn <id>)`, never the rider (another notice gives up its end), while the journal keeps the words whole (amended 2026-11-04, review of gap 23, finding 1: the client cut at 1024 bytes, so aterm refused every Land act's record and a long act's with a rider, and the fact was told with no Messages entry; `alab-aterm/src/wire.rs` `elide_before`, `tests.rs` `a_notice_keeps_its_tail_within_aterms_512_bytes`, `tests/autopilot/fact_rider.rs` `a_long_acts_notice_keeps_its_fact_within_aterms_512_bytes`); automatic waits → the harness diagnostic log (`appnotice harness …`) and journal, without a persistent TUI warning (amended 2026-10-25 after a completed startup-settle wait left a stale footer badge), and aterm's Waiting rows (the menu bar's ⧗ row, the Overview's ⧗ line): `@self meta set attention owner=codex-alab-wait kind=wait <words>` (≤200 B), on the claim connection while the claim is held, once the wait's timer is armed; never a need, a border or a pill, never `await task` news (aterm counts no `codex-alab-wait` entry in a task's attention, nor reads it as a startup's ask), never under `owner=codex-alab` (the asks' one slot) and never with no kind (an ask). Every wait a decision shows goes there: a usage reset, a quota's rung, a server's, a network's or an overlong reply's rung, the API's reach before a resume, the host's wake settle, a relaunch's carry-on delay, the inherited settle, the short-turn back-off (`1 short turn(s) in a row: 2 min back-off · types again at 15:02`) and the hour's cap (`it already typed here as often as an hour allows · types again at 15:40`), the last two in aterm's words with the clock time the act is typed and now in the diagnostic log too, each whole within the 72 bytes aterm's Waiting row shows (its `ROW_WHAT_MAX`; a wake settle that moves the act later keeps the words without their time), and an act the previous image owed at a live re-exec or aterm's relaunch (`the act it owed before its restart · types again at 15:40`: the old image's words went with it) (amended 2026-10-27 after the parity sweep, gap 2, review of f134d2b2cb, findings 2 and 7: the cap that named its setting lost its time to the row's cut, and a carried back-off or cap showed no row); where several threads wait, the soonest wait shows (the next time codex-alab types into the tab; a tie goes to the thread key that sorts first). It comes down at the thread's turn start (codex-alab's act or a person's), at a decision that waits for nothing (nothing to type, a held point, whose stop is told on its own, an attention, a goal that owns the pace, a landing, an act that did not go), at the thread's stopped runtime, an Off, a standby, a lost claim however it went (the claim's watch, an act's checks, the connection's end: one that ended leaves it to the next claim's first sync) and the exit; a turn's start takes it down on a task of its own, never in the hook core awaits before the turn runs (review of f134d2b2cb, findings 1 and 4); the first sync under a new claim settles an entry a killed image left; a not-now re-arm (a draft, a popup, the TUI busy) and a person's hold keep it, as the act still waits (amended 2026-10-27 after the parity sweep, gap 2: a codex tab waiting out a usage limit or an outage looked idle or stuck, a weekly wall for days, and the back-off and the hour's cap told nothing at all; `waits_shown_tests.rs`, `decide_tests.rs` `a_backed_off_or_capped_continuation_shows_its_wait`). aterm's `sessions`, `task` and `await task` rows prefer the newest entry whose kind is not `wait` (`SessionMeta::row_attention`, aterm `22213e6d33`), so where an ask stands (the quota wall's `owner=needs` need, an `owner=codex-alab` ask) and a wait goes up after it (a quota wall's rung goes up again after each try's turn), those rows keep the ask's words while `meta attention=` and `attention_owner=` report the newest entry of any owner (amended 2026-10-28 after aterm `22213e6d33` taught `sessions`, `task` and `await task` to prefer non-wait entries); a typing stop (§5 typing stop) → once per arm that holds a thread, one `appnotice harness …` and one TUI warning in aterm's words, `stopped typing: the same reply twice, no work · types again at your turn or once the agent works` (or `6 replies in a row, no work`), told again only after a decision that was not held; never attention, a NEEDS YOU or a `harness_act` (today's aterm would show any act word as an act typed) (amended 2026-10-27 after the parity sweep, gap 4: a stopped codex-alab tab said nothing); and its arm in aterm's `status held=` (aterm's `HeldBy::word`: `same-reply` or `no-work`) → `@self meta set harness_held <arm>` on the claim connection, only while the proven claim is held and codex-alab acts (aterm change: the standing proven holder only, else `ERR denied`; `OK` either way; other words `ERR usage: meta set harness_held same-reply|no-work | meta unset harness_held`), once per change of the arm the tab shows, which is the primary thread's (the tab has one `held=`, and the primary thread is the conversation the TUI shows and the session record speaks for; another root's stop is told by its own appnotice and warning, and never stands over a primary thread that works); `@self meta unset harness_held` once that arm comes down: a decision not held, a held usage window or quota (aterm's `limit_repeats` names no arm), the thread's turn end that lifts the hold (a person's turn, work, a reply in other words), its stopped runtime, an Off (its park lifts every hold) and a standby (nothing is held back; acting again under the kept claim sends the standing arm); a person's message arriving and a switch between Cruise and Land lift nothing. aterm drops the arm when the claim ends (its release, a replacement, the connection's close, the process gone), but keys it by the claim's proof (the pid and its kernel start, which an exec keeps), so a re-claim by this process that reaches aterm before that end keeps it (a reconnect's claim taken before aterm read the old connection's close; the next image's, after an exec that outran the release), and a claim that renews none of that proof still live (one after a `ttl=` lapse, which codex-alab's connection-bound claim never takes) starts with none (aterm a945f9abbb). aterm says the arm in `status held=`, its Settings ▸ Harness state line and its worker row's tip as `codex-alab stopped typing: …`, and in the tab's hover and the Overview, each only while the tab's face carries a mode, never at its landed or quiet face (`landed@v1`, `left-to-person@v1`), as it says its own stop; codex-alab's line draws its Autopilot face with no stop words (aterm's chip face, never its title), so it says none: a lost claim keeps the threads' holds, and each claim's first sync sends the tab's arm or takes a kept one down, unless the last answer this image had proved none stands (an unset taken, a refusal, a denial), as an arm an earlier claim showed, one an earlier image left and one a failed call may have set do not (amended 2026-10-28, review of 8482c60ba6, findings 1 and 3: a kept `held=` stayed up while codex-alab typed again); the exit, whose release drops it, sends none. An answer but `OK` shows nothing: an aterm before the verb answers as for any field it does not know (`ERR unknown meta field (title|description|icon|role|attention|questions|autopilot)`; from 0.143.0 its `status` says `held=-`, its own loop's) and is not asked again under that claim until the arm moves (a new claim, which a newer aterm may hold after an update, asks once), and an `ERR denied` is not asked again under that claim (amended 2026-10-28 after the parity sweep, gap 4 stage 3: aterm's `status held=` said `-` on a stopped codex-alab tab, as it read only aterm's own loop's hold; `held_status_tests.rs`); a held usage window or quota says aterm's `limit_repeats` words instead (`stopped typing: The limit … the same. · types again at a new wall, your turn or once the agent works`: not aterm's "a new reset", which codex-alab reads only at a turn's end that keeps it as the reset the hold compares with, so a new reset alone lifts no spent hold; at a quota the line is codex-alab's own, beside the standing need, aterm's `Spend` exception's counterpart, which aterm tells alone there; amended 2026-10-28, ruling F stage 2, review of 4c0168cedd, findings 2 and 9). LAND'S HOLD (§5 Land; aterm's `LandHeld`, its landing) → `@self meta set harness_land since=<ms> nudges=<n> [read=1 legs=<leg>[,…] [open=<w>[,…]] [changed=<n>] [untracked=<n>] [stashes=<n>] [unpushed=<n>] [unlanded=<n>]]` on the claim connection, only while the proven claim is held, codex-alab acts and the session's word reads Land, once per change of the hold the tab shows, the primary thread's (as `harness_held`'s arm): `since` is when the act that began the hold was typed (unix ms), `nudges` the nudges typed since it, and `read=1` comes with what held the land act's yield as judged, while no other turn ran since: its legs (aterm's `HoldLeg::word`: `needs` where the turn's words name a need, `open` where Land's strict reading finds open work, its first four words sent percent-encoded, `defense` where no `Nothing left:` line stands, `repo` with the counts the probe for that point found, `repo-unread` where it could not read; `cut` never, as Codex hands each message whole); a yield whose words name no leg (a clean defense before its probe) is sent with none read, as the wire takes no `read=1` without a leg; `@self meta unset harness_land` once it comes down: the landing (`landed@v1`), a point left to a person (the `theirs` face), a person's turn (which ends the episode), leaving Land (the policy's read takes it down, the thread's next look ends the episode), an Off, a standby (acting again under the kept claim sends the standing hold) and the stopped runtime; the exit, whose release drops it, sends none. Its claim rules are `harness_held`'s: each claim's first sync sends the standing hold or takes a kept one down unless the last answer this image had proved none stands, an aterm before the verb (`ERR unknown meta field (…)`) is asked once per change under that claim, and an `ERR denied` not again under that claim (amended 2026-10-28 after the parity sweep, gap 11: a codex-alab worker under Land showed no landing in aterm; `land_status_tests.rs`). THE ONE LOUD HOLD (aterm's `repeats_land`; amended 2026-10-28, ruling F stage 2: every hold was quiet): under Land, at no wall, the newest reply kept no wall's, where Land's repository probe last read work no remote main has in this episode and no lead waits on the session (`@self task` names no record, or one `exited`, as aterm's `managed`; a read that fails is no lead's, as aterm's `task_seed` leaves `managed` unset at a lost read, and so is no connection, where no ask can go up; amended 2026-10-28, ruling F stage 2, review of 4c0168cedd, finding 3: a failed read kept the hold quiet), the hold is codex-alab's own ask, `owner=codex-alab` under the `stopped` reason: `Stopped typing: Under Land it answered twice with the same reply and no work, and its repository still holds work no remote main has (<finding>), so codex-alab types nothing more.` where it fits in aterm's 200 bytes; else the why alone, without the title, its finding cut to the room left (`Under Land it answered … (<finding cut>…), so codex-alab types nothing more.`: the why already says codex-alab types nothing more, and the finding is what the person acts on), and where no word of the finding fits, the title and the why without it (amended 2026-10-28, ruling F stage 2, review of 4c0168cedd, findings 1 and 8: this row said the finding went first; aterm's action, `Open the tab and tell it what is still open.`, goes in the appnotice and the TUI warning beside its why, as one line holds no more), raised again where something took it down while the hold stands (after a re-exec, which takes the old claim's entry down and keeps no reading of the repository, the held point it restores reads the repository once, and the ask goes up again over the work it finds; amended 2026-10-28, ruling F stage 2, review of 4c0168cedd, finding 5: the new image told the hold quietly for as long as it stood), and taken down once the hold lifts (a person's turn, work, a reply in other words, an Off) or is told quietly; a managed worker's hold, Cruise's, a walled reply's and one over a clean or unread repository stay quiet; NEEDS YOU never carries it; what the root thread is doing → `@self meta set description goal: <objective>[ · now: <plan step>]` or `plan: <step>` (the goal unless complete, `update_plan`'s step in progress; ≤1024 B, one write per change, on a connection of its own, never the title). It is withdrawn (`meta unset description`, only while it is still the text written) when there is nothing left to say, when the claim goes or codex-alab goes on standby (§2), and at the exit, so no restore carries a stale goal. Descriptions are written only while codex-alab acts, and `harness_act` records while its claim is held (§2). A standing text is codex-alab's only when it is exactly the one this process wrote, or, before it wrote any, when this thread's dead predecessor left a record (a crash, an aterm restore) and the text has the `goal: `/`plan: ` shape; any other text is a person's and stands (aterm has no compare-and-set: a person's text set between the read and the write is the one race left); THE LINE'S AUTOPILOT FACE (`alab/interface.md`; amended 2026-10-29 after the parity sweep, gap 12: a codex tab showed `[Cruise]─[Land]─[Off]` and none of aterm's faces) → aterm's `ChipFace` drawn by codex-alab itself, `∞`, `∞ ⤓HH:MM`, `⤓ land`, `✓ landed`, `⤓ quiet`, `○ off`, and `⊘ off` (aterm's `MasterOff`, `AgentOff` and `Unhosted`) for an off whose `why=` is `disabled` or `agent-off`, one of aterm's Settings switches, or `unhosted`, aterm with no harness host, where the line draws no mode buttons and a click on the face says it in words, naming where aterm turns it, which codex-alab cannot (amended 2026-10-29 after the review of gap 12, finding 3: the line wore no face there): the word from §3's reads, the timer from their `land_at=`, the landing and the left point from the primary thread's decider (`ThreadState::task_face`; a running turn of that thread wears neither, as aterm reopens both at a turn begun, and a turn of another thread reopens none; the landed task's own wake wears none until its judgement keeps the landing and records `landed@v1` again, as aterm reads it: amended 2026-10-29 after the review of gap 12, findings 1 and 16), told at every move `tell_land` tells and at each turn's start, under the registrations' lock, and taken down at the stopped runtime; each frame draws the face, the highlight and the hits from one load of the word (findings 5 and 9); a click moves the mode as a click on aterm's chip does; a right-click or control-click on the face or a mode button, or `/alab menu`, opens Codex's native picker. It offers Cruise, Land, Off, timed Cruise presets, a custom deadline editor and cancellation of a scheduled landing. Choosing a mode or deadline sends the shared policy change through the existing owner-only control connection. Opening or dismissing the picker retains the draft and submits no model turn. The product/version label opens native build and update details; there is no separately versioned Codex harness or host-drawn Codex overlay (`alab-tui/glue/autopilot_menu.rs`, `alab-tui/glue/interface_controls.rs`, `alab-tui/src/interface/tests.rs`, `alab-autopilot/src/task_face_tests.rs`) | law 5 |

An unreadable loaded goal is unknown: live upgrade waits for the independent monitor's next
scan, and automatic typing or compaction waits for the existing bounded recheck. A successful
read showing no goal or a completed goal permits ordinary continuation. Typed admission holds
the native goal mutation permit through its final fresh read, authority check and Core start
result; compaction holds it through accepted enqueue. Both release that permit before remote
attention or act-record I/O. An expected but unavailable goal runtime with persisted state is
also unknown. A detached Active read clears only the exact goal-reason entry it observed before
the read; a later raise, including identical words, keeps its own custody.

Descriptions accept a goal-read result only from the current registration's latest refresh.
Failed goal or thread reads preserve acknowledged text. Exact runtime ownership retires with
stop or replacement before either can await; accepted intent and acknowledged remote ownership
remain separate while publication is pending. Writes and withdrawals share one asynchronous
order and reconcile the latest accepted intent. Queued cleanup compares the acknowledged
publication identity again after reading metadata, including identical text adopted while
acting resumes. An acknowledged removal invalidates the actual baseline and reconciles any
newer desire. A failed first metadata connection or read retains predecessor custody for a
healthy retry; a successful current read adopts only the exact predecessor-shaped standing
text before retiring that broader permission. A person's other text retains its ownership.

A person's own exchange, where one is left (the person's own exchange row), records
`left-to-person@v1` under the proven claim, once for its native point, so aterm can show
`task=theirs`; the act is never typed. The local publication mark settles only after aterm
acknowledges it. A failed write remains eligible at the next claim or a bounded metadata-only
recheck, while a new native turn revokes the old point.
Publication owns the session's native-start gate and evaluation permit even if its caller is
cancelled. It first asks `meta prepare harness_act` for an opaque `until_us` and `lease_ms`, then
sends the act with `until_us=<n>`. Aterm takes the turn-count mark before its final expiry check
under the admission locks. On an unanswered or failed transport exchange, the client retains the gate for the
full lease measured from the prepare reply, so a queued request expires before another native
turn starts. An acknowledged refusal releases it promptly without recording an act; a successful
acknowledgement already proves the mark was captured. A refused `prepare` from this protocol's aterm
(`ERR denied`: the claim is no longer this connection's) is a refusal, and the point stays eligible.

A landing uses the same owned publication worker, evaluation permit, native-start gate and
before-turn lease. It waits for the TUI outside the gate, then revalidates its runtime, policy,
native point and absence of pending or queued person input. Only the host's acknowledgement of
`landed@v1`, followed by the same local revalidation, commits the landing, ends its hold and clears
the carry-on count. A refusal or failed exchange retains the original point and any reland owed;
it gets a bounded metadata recheck. Cancelling the caller leaves the worker holding custody until
the request settles. A newer native turn cannot be admitted behind an outstanding landing write.
The paired aterm implementation refuses either leased standing face (`landed@v1` or
`left-to-person@v1`) unless the fresh report snapshot, read after the act barrier, is at rest.
This closes the optimistic-TUI-begin window: a submitted person's turn may have written `working`
before Core accepts it or Shared books its pending flag. Hosts through public 0.147.0 only apply
that at-rest predicate to `left-to-person@v1`; their lease still bounds a late write, but cannot
refuse a landing whose mark already includes that optimistic begin. Full handling of that window
requires the paired host change; no older version floor implies it has that change.

AN ATERM WITHOUT THE LEASE (amended 2026-10-22, CX-C C1: such an aterm got nothing recorded, the
person's exchange read `task=open`, and every look found the publication unsupported and owed another
in 5 s, for ever, a timer pending with no paced act, which held the live upgrade). One before 0.110.0
(aterm bf9d401e40) answers `meta prepare harness_act` with its plain `meta` usage (`ERR usage: meta [set
<field> <text...> | unset <field>]`, a usage that names no `prepare`), read off its `version=`:
- From 0.109.0 (aterm 2380aceb85 and 8e1292d320: the first that reads `left-to-person@v1` as `theirs`
  and opens it again at the next turn begun past it), the act goes in its unbounded spelling, `meta set
  harness_act left-to-person@v1`, on the claim's connection, still inside the gate: only while no turn
  of codex-alab's runs, none is submitted and no input is queued (that aterm has no at-rest check of its
  own), and no native turn starts before aterm answers, so an answered write took its mark before any
  turn core starts. What that aterm cannot fence: a submission whose `working` report it counts in the
  moment between that look and the write, and a write left unanswered (the client closes the claim's
  connection; aterm may still take the request it read, after a later turn began). Either makes the
  face read `theirs` through that one turn, until codex-alab's next act or the turn after it: a display
  error only (aterm's face changes nothing it types, holds or ends).
- Older, or a release the answer does not name: that aterm reads any act but a landing as `open` and logs
  it as an act taken, so nothing is sent. The point is settled as it stands, and the person is told once
  per process, loudly and never blocking: the TUI's warning line `codex-alab: NEEDS YOU: update aterm to
  0.110.0 or later. This one (<release>) cannot show a person's own exchange as theirs, so codex-alab
  records none there; carrying on`, and the same words as an `appnotice harness` record. No attention is
  raised: a standing one holds the live upgrade, and nothing here waits on the person.

For a landing, the compatible unbounded floor is 0.108.0: aterm's landing implementation
`20240ae119` is first contained by tag `v0.108.0`, and `v0.107.0` has no `landed@v1`. On 0.108/0.109
the client retains the gate and pending/queued-input checks around that spelling. The unanswered
write and optimistic-begin limitations above remain; this preserves older-host behavior. Before
0.108, or with an unknown release and no lease, no landing is sent or committed locally: point,
carry and hold stay as they were. One existing update advisory is emitted without attention or a
perpetual five-second poll. A real claim/host transition rejudges that retained point, including
one the first evaluation already marked decided.

## 6. In-band status (aterm; helps vendor codex)

Upstream codex writes `ESC ] 7501 ; state=<idle|working|blocked|done|error> : app=codex [: kind=…] ESC \` and
`state=clear`. aterm stores it in its own field, shown as `status program_state=<state>[:<kind>]`, on the band and
in presence, and NEVER in `agent=`, `agent_since_ms=`, `phase_authoritative`, the zone hash an act reads, or any
`await`. Honoured only while the program in front names `codex`, or is a runtime that may host an agent (`node`,
`bun`: npm's `codex`) and the report names `app=codex` (aterm `aterm-gui/src/program_state.rs::honoured_for`,
66eb1f2635); cleared on `state=clear`, RIS, a foreground-group change and the program's exit. The disk tick also takes
a session whose 7501 report is shown (a `node`-run vendor `codex`, which no screen names; aterm
`harness_host::CODEX_REPORTS`, told by the status sweep, b97d43b87d). Display only, as before.

codex-alab's report inside aterm is written as aterm counts turns (`alab-tui/src/turns.rs`, `ALAB(turns)`; amended
2026-10-21 after the parity review). aterm's engine counts, as each report is parsed, the turns the reports begin
(`ProgramStateReport::turns`, aterm main 4a79c8db07: an in-turn report, `working` or `blocked`, after one at rest,
`idle`, `done` or `error`, or as the first report; a `clear` and a full reset looked past, a report written again
beginning none), and a proven self-claim's landing reopens at a count past the one its `landed@v1` was recorded over
(`aterm-gui` `harness_task.rs`, `reopen_self_claim_landing`, e7ab6c9ce4). Upstream's report miscounted twice. (O1)
It said `working` while the thread's MCP servers started (a launch, `resume`, `/resume`, a thread switch, the live
upgrade's re-exec) with no turn running, so aterm counted a turn nobody began and a landed task reopened: the
report now leaves the startup out and says where the thread stands (`idle`, or the last turn's `done` or `error`
until a key acknowledges it); a turn that runs during the startup is still `working`, and the session record and
the autopilot still read the startup as busy (`alab_program_state`). (O2) A turn begun while the report stood in a
turn (a queued message the TUI sends the moment the last turn ends, a message sent while a background terminal or
an active goal keeps the report at `working`) wrote nothing, so aterm counted none and a landing never reopened:
now a turn the TUI submitted (upstream's `user_turn_pending_start` as the server starts it: a person's prompt, a
driver's typed turn, a queued message, `/compact`, a review, a shell command) writes `idle`, then `working`, as it
starts, in the live `TurnStarted` arm, never at a replay; where a frame already turned the report from rest to
`working` while that submission waited for the server, that was its begin, and its start writes nothing more. A
turn the TUI did not submit (a goal's continuation, an
autopilot act through the app-server) begun under standing work writes nothing: that work runs on, as aterm-core
counts it, and a landed task's own wake keeps its landing (aterm's `quiet_wake`; §5 Land). A turn begun from rest
writes `working` as it starts, whoever began it, so one that ends before the next frame is counted as a frame would
have shown it, and a process's first word at a turn's start is `idle`, then `working` (a predecessor's last word is
unknown). The words go through upstream's status writer and its cache (`tui/src/terminal_program_status.rs`: the
cache that drops a word written already, its re-send after a handoff, the exit's and the panic hook's `clear`). An
edge's two go as one batch the cache owns (`set_terminal_program_status_with_prefix`, its glue
`alab-tui/glue/program_status_batch.rs`): under the status cache's lock, the order every report and the panic hook's
`clear` take, it takes stdout's lock, writes the `idle`, forces the `working` past the cache and flushes before it
lets either go, so no `clear` or other report lands between the two words, they reach aterm in one flush, and no
sweep between them shows `idle` alone, a turn end and its dot for a turn that runs on (amended 2026-10-21 after the
parity-3 review, F5; amended 2026-10-22, CX-C C2: this text described the `idle` put into stdout's buffer outside the
cache's lock, which the panic hook's `clear` could split from its `working`; main baf273de3 and 5a4b08f3a moved the
whole batch under the lock, held by `paired_edges_serialize_with_clears_and_retry_partial_writes`, whose writer
asserts the cache is held). A write that fails, whole or in part, or whose flush fails, leaves the cache and the last
word unknown, so the next report is written whatever it says and the next turn's start writes its edge whole. The OS
or aterm's reader may still split one write; nothing here promises otherwise. The OSC 9;4 bar below is one
`write_all` of a whole sequence under stdout's lock, so it lands inside no edge and no `clear` either. Only in the
codex binary inside aterm; elsewhere, iTerm2's OSC 21337 included, the report is upstream's. Hooks: two one-line
hooks, `tui/src/chatwidget.rs` (`refresh_terminal_program_status`) and `tui/src/chatwidget/protocol.rs` (the live
`TurnStarted` arm), with their glue in `alab-tui/glue/chat_hand.rs`, and in `tui/src/terminal_program_status.rs` the
batch's module line, its entry and a failed write's cache reset; tested at the byte level against aterm's rule
(`turns::aterm_turns`, held to aterm's own rows) and through a real `ChatWidget`
(`alab-tui/glue/turns_session_tests.rs`). A THREAD SWITCH (amended 2026-10-22, CX-C C3): the report says the
displayed thread's state and a switch replaces the `ChatWidget`, so a switch from a thread whose turn runs to one at
rest wrote its rest, and the switch back wrote `working`, a turn nobody began. Now, while a turn of a thread the TUI
does not show runs (`Shared::turn_running_besides`: the turns the autopilot books for its threads, the TUI's root
ones, from `on_turn_start` to the turn's end or the thread's stop; each start and end wakes the TUI, since another
thread's events draw no frame of their own), a frame at rest says `working` (`turns::Frame::elsewhere`): the switch
back writes nothing, and the displayed thread's rest is written once that turn ends. A turn the TUI submits on the
shown thread meanwhile writes its own edge under it; one it did not submit begins none, as under any standing work.
EVERY TURN THE TUI ROUTES IS BOOKED (amended 2026-10-22, parity-5 T: a sub-agent's turn was not, the autopilot
keeping only root threads, nor any under a remote app-server, where no autopilot runs in the TUI's process, so a
switch back to a running sub-agent after a thread at rest was shown wrote `working` again, a turn aterm counted):
the TUI sees each of them, since the app-server's `turn/started` and `turn/completed` for every thread it routes
reach its notification routing, in process or over a remote connection alike. An `ALAB(turns)` hook there
(`tui/src/app/app_server_events.rs`, past the thread-ownership check and before the thread's store takes the
notification; glue `routed` in `alab-tui/glue/session_observe.rs`) books each start in `Shared`'s seen book
(`Shared::turn_seen`), which only the report reads (`turn_seen_besides`); the record, the acts and the upgrade read
the autopilot's book alone. A SEEN TURN HOLDS A TURN AND BEGINS NONE (amended 2026-10-22 after the parity-5 review,
T-subagent-turn-reopens-landing): it keeps a frame at rest in `working` only while the last word written stands in a
turn (`turns::Frame::held`, `Frame::reported_after`), so a switch from a running sub-agent to a thread at rest and
back writes nothing, but a seen turn that starts while the report rests writes nothing either. As first built it
turned that rest into `working`, a turn aterm counted, and beside a proven self-claim aterm reopens a `landed` or
`theirs` face at any turn begun past its mark (`reopen_self_claim_landing`), while codex-alab, which decides and lands
its root threads only, never acted again: the task stayed `open` with nobody to land it. While a held turn keeps the
report in `working`, aterm answers a leased `theirs` act (`left-to-person@v1`) `ERR busy harness_act`, which takes a point
only at rest, and codex-alab looks again every few seconds until that turn ends. The paired host applies the same
predicate to a leased `landed@v1`; older hosts take that landing as it is, and
the held turn's end begins no turn past its mark. A turn's end and a thread's
close are booked before any routing return (amended 2026-10-22 after the parity-5 review, T-stale-seen-turn): the
second `ALAB(turns)` hook, at the top of `handle_app_server_event` (glue `stream`), takes `turn/completed` and
`thread/closed` from the event stream itself, since upstream drops every thread notification while no primary
thread is attached and no startup is pending (after an archive or a delete of the current task) while a side
conversation stays owned; booked at `routed`, such an end never came, and the ended turn was read as running ever
after. Each start and end wakes the TUI. A seen turn whose end may never reach the hook is let go
(`Shared::turns_seen_keep`): a thread the TUI no longer routes, read at each frame (`owns_thread_for_routing`;
another session shown, a thread let go), and every one when the event stream lagged or disconnected (the `stream`
hook), failing toward rest. Held by `alab-tui/glue/turns_routing_tests.rs` through a real `App`, its embedded
app-server and the report its chat widget writes (RED with either hook, the end booked at `routed`, the
`thread/closed` arm, the disconnect, the frame's let-go, the hold, or a seen turn read as the autopilot's own
removed or changed), `alab-tui` `a_seen_turn_holds_a_turn_and_begins_none` at the byte level against aterm's rule,
and `alab-core` `a_turn_the_tui_saw_reads_beside_the_displayed_thread_only`. What it cannot book: a turn the TUI
learned of only from a thread's snapshot (`thread/read` or a resume's history, no `turn/started` reaching it, as
after a lag, a reconnect, or attaching to a thread already mid-turn), whose end the hook may never see, so a switch
back to that one after a thread at rest was shown still writes `working` again; a seen turn that starts while the
report rests, which a later look at it counts as a turn begun at that look; and the OSC 9;4 bar still shows `9;4;3`
while the servers start (no count reads it, and the pet takes `3` as a quiet level). A HELD FACE GOES AFTER ITS
TURN'S EDGES (amended 2026-10-24 after aterm's codex-alab lab, reland leg): the autopilot decides at the core's turn
end, and the TUI writes a turn's edges as its chat widget takes the thread's notifications, later under load, so a
`landed@v1` recorded while the person's turn's end and the land turn's begin were still unwritten met that begin past
its mark, and aterm reopened the landing. Where this process's TUI (its frame waker set) shows the thread,
`landed@v1` and `left-to-person@v1` now wait, at most 5 s (`TUI_CATCH_UP`), until the chat widget took the end of
the thread's latest turn: another `ALAB(turns)` hook, in the `TurnCompleted` arm of `tui/src/chatwidget/protocol.rs`
(glue `alab_turn_ended`), books it, live or replayed (`Shared::turn_handled_end`, read by `tui_handled_end`), and
since the widget takes a thread's notifications in order, that turn's `TurnStarted` edge is written by then. The wait
ends as soon as the TUI shows another thread (re-read at each look): its frames write this thread's edges only from
the autopilot's own book, none after the turn's end, and the end its widget no longer takes is never booked. The left
and landing acts wait outside the publication gate. Held by `alab-autopilot` `a_held_face_waits_for_the_tui_to_take_its_turns_end`. What it cannot
order: aterm's reader parsing those bytes after the act reaches its control socket, a second channel. aterm orders
that channel with its act barrier (aterm `06b50f2bf2`, 2026-10-24): an act that reads the session's counts (a face that
stands until another turn, such as `landed@v1` or `left-to-person@v1`, or an act that types a turn) is read once
aterm's parser holds every byte written to the PTY before it (on macOS; elsewhere on unix, what its reader had read),
or as it stands once 2 s pass; after such a late read each later act is read at once, unordered, until the parser
answers a barrier again, and a session with no live barrier lane or poke pipe, or a non-unix reader, is read unordered
(aterm `crates/aterm-gui/SPECS.md`, row `act-barrier`). A person's turn on its way when a landing is recorded can
then be counted into the landing's mark and never reopen it, so no landing is recorded while a person's turn is
pending or input is queued, as §5's landing publication says (held by `alab-autopilot`
`submitted_or_queued_work_defers_landing_without_consuming_the_point`). THE PLAN BOX AND
THE GOAL BOX KEEP THE REPORT AT REST (decided 2026-10-27, after two reviews of a change that reported them
`blocked: question`, which never landed): "Implement this plan?" opens as a live turn ends and the report says that
turn's `done`; "Resume paused goal?" opens at a resume with nothing running and the report says `idle`, both as
upstream's do. A `blocked` there is a turn as aterm counts them, and the reviews found what it broke: after a resume's
rest it began a turn and reopened a landed task with nobody to land it again; while the plan box stood aterm refused
a person's own exchange's `left-to-person@v1` (`ERR busy harness_act`, a `theirs` act is taken only at rest) and
codex-alab retried it every 5 s for as long as the box stood; codex-alab's own yes wrote a stale `done` for a frame
between the box's `blocked` and the implementation turn's `working`; and the supervisor could read the closed box's
`Blocked` age and raise a false `question:` attention. A plan box or goal box left to a person shows, under a held
claim, through codex-alab's own attention (§5 `left boxes`), never through the report. Held by `turns_session_tests.rs`
`the_plan_box_at_a_turns_end_keeps_the_report_at_rest` and `a_resume_over_a_paused_goal_begins_no_turn`.

codex-alab also writes ConEmu's taskbar progress, OSC 9;4, which aterm takes ungated (`aterm-core`
`handler_osc_notify.rs`, `Terminal::taskbar_progress`; RIS clears it, a checkpoint carries it, a program's exit does
not). aterm draws no bar on macOS (its Windows build mirrors it onto the taskbar button); its pet reads it
(`aterm-effects` `PetAttention::Progress`: a change of a value bar, `1` or `2`, while the command runs turns the pet
to look; `3` and `4` are quiet levels). Written each chat frame right after the 7501 report and before the draw, on
the same `stdout` through crossterm's `execute!`, only when it changes (`alab-tui/src/progress.rs`):
`ESC ] 9 ; 4 ; 3 ESC \` while the displayed thread works (or waits on a box) with no plan this turn, and after a turn
while the thread still works (a background terminal, an active goal between turns); `9;4;1;<pct>` while the turn
runs once its `update_plan` has `done` of `total` steps (`pct` = `done*100/total`, rounded down); `9;4;2;<pct>` when
the turn fails (the plan's `pct`, or 100 with no plan or no step done) until a person's key; and `9;4;0` at rest and
at a turn's end, and on the launch's first frame (a killed predecessor's bar goes). A plan needs
`[tools.update_plan] enabled = true` (upstream made the tool opt-in, a9519cbcd; ALab leaves it, as turning it on
changes what the model is told), so by default a turn is indeterminate and the one value bar the pet turns to is a
failed turn's. Wherever the TUI clears its 7501 report (the exit, a fatal error, the panic hook, the signal watchdog,
Ctrl+Z, an external program: one `ALAB(progress)` line in `tui/src/terminal_program_status.rs`) a showing bar goes
with `9;4;0`, and the next frame puts the current one back. Only in the codex binary, only inside aterm
(`TERM_PROGRAM=aterm`) with no multiplexer between (tmux, zellij, screen's `STY`: upstream's OSC 21337 gate), only
to a terminal, and only while `[tui] alab_progress` is `true` (`alab-core/aterm-defaults.toml`; a person's `false`,
in `~/.codex/config.toml` or `-c`, wins); otherwise not one byte. It is harness chrome: like the banner's harness row,
it shows under any Autopilot word, Off included (aterm's `why=autopilot-off` keeps the harness on, as does a person's
hold over that Off), and goes while the harness does not supervise the session (its policy off for another reason, the
claim unproven); unlike the row, it also shows with no supervisor at all (amended 2026-10-24: it went under Autopilot
off too). codex-alab's `--strict-config` takes the key (one
`ALAB(progress)` line in `config/src/strict_config.rs`); vendor codex's refuses it, as an editor checking upstream's
schema flags it, so it does not belong in a config vendor codex reads strictly, such as a checked-in project config.
Display only: no act, no claim and no `await` reads it.

codex-alab files an account notice that needs the owner, but not now, in aterm's exceptions digest instead of showing
a banner in every tab (`ALAB(notices)`; aterm owner ruling 2026-10-21: "for the nag, we need to manage those from a
central location, not just interrupt users randomly"). The one such notice is OpenAI's security setup
(`tui/src/security_setup.rs`), which the server returns at every launch, reconnect and account update. After
upstream's fetch, its 3 s deadline and its checks, `alab-tui/src/notices.rs` sends, on a connection of its own,
`notice later codex.security-setup.<8 hex> tag=codex url=<the action's chatgpt.com page> Set up Codex account security
-- <reason> -- <step> -- <the server's title>\n<its description>` (aterm `f17fb9bcaf`, `ccfe48663c`:
`aterm-messages` `wire::LaterRequest`). The key's hex is an FNV-1a hash of the account and user ids, so no id
reaches aterm's log. The title, reason and step are fixed words; the server's words are detail lines cut at a space
to aterm's caps (3 lines of 240). `OK later=<id> how=<new|restated|duplicate|quiet>` means no banner: `quiet` (the
owner dismissed or answered the entry, which holds a week, then four weeks, then three months) is taken too, since the
owner's one central dismissal governs every tab. An aterm without the form (`ERR usage: notice <post|…`, or `ERR
unknown verb`), any other refusal, an error, or no answer within 750 ms shows upstream's banner, exactly as outside
aterm or outside the codex binary. Notices a person needs now (usage limits, the sign-in, hook consent) stay inline.
Display only: it routes where a notice shows and never suppresses, hides or alters a refusal or an access decision;
no act, claim or `await` reads it.

## 7. Versions

codex-alab's version scheme is the owner's (`pub policy`; alab/README decisions 2-3 and 8; never invented). Two
versions since 2026-10-24 (Decision 8): **the version**, codex-alab's own line from `0.1.0`, is what codex-alab says
it is to aterm and to a person (`codex --version`, the session record's `version`, `codex-package.json` and
`alab-build.json`'s `version`, which atpkg carries verbatim); **the client version**, upstream's numbering
(`alab-build.json`'s `client_version`), is what upstream's code and OpenAI's servers see, and what the rollout's
`cli_version` names (aterm's measured rollout builds are client versions). Both are
`MAJOR.MINOR.0-dev.<count>+g<sha9>`; from the version ledger (`alab/versions.toml`) on, the version is compound,
`MAJOR.MINOR.0-dev.<count>+codex.<baseline>.g<sha9>`, the upstream Codex baseline in its build metadata (Decision 8),
which aterm's `atpkg::alab::AlabVersion` reads and `atpkg-publish pack-package` checks against `alab-build.json`'s
`baseline`; the client version keeps the plain form. aterm's `Version::parse` is unchanged and refuses both, as a
pre-release. aterm
has no Codex upgrade lane (retired, aterm fc74be92fe, retire C3): aterm moves no Codex, and codex-alab upgrades
itself (§5 live upgrade).
No aterm reader orders a vendor build against an ALab build. codex-alab's own upgrade compares builds by path
identity (§5), not version strings.

## 8. Packaging (Phase L: local, no credentials)

`alab/package.sh` builds the package layout v1 (`codex-package.json`, `bin/codex`, `bin/codex-code-mode-host`,
`codex-path/rg` built by Trust, `codex-resources/` without voice, LICENSE, NOTICE, `alab-build.json`).
**aterm:** a dev-linked agent program gets its `agents/` twin (activate.rs `reconcile_agents` / `sweep_agents_dir`,
linkmode's test inverted), so `aterm pkg link codex <pkgdir> bin/codex` wins on PATH inside aterm.

## 9. Order (fail closed, reversible)

1. codex-alab: cruft (landed), banner, record + signals + human/observe hooks, aterm client, autopilot. With an old
   aterm (no `harness policy`) it never claims: nothing changes.
2. aterm: §6 7501; §3 verb + event; §1-§2 proven self-claim, replacement, interrupter, `if-supervisor=`, clear on
   exit; §4 watch-only worker, copy, crash rule, ledger, model; §5 `harness_act`; §7 upgrade-lane guard; §8 twin;
   §0.1 docs.
3. Package and link codex-alab as this Mac's managed `codex`; stop the vendor daemon; end-to-end proof in a private
   headless aterm (claim proven, lane parked, continue, done stop, approval, question overlay, usage wait, crash
   relaunch, re-exec upgrade, hold respected).
4. DONE (2026-10-16, aterm main `28e13d46ba..83bd43c206`, tip 83bd43c206): aterm's Codex screen lane is deleted
   (reader, fixtures, `upgrade_codex*`, goal hold, `codex_usage`, rate nudge, their models and binds, words), keeping
   the 7501 display reader for a user's own vendor codex. The steps on main: C1 e9164bd67e and C2 3999d9adc6 (the
   upgrade ladder bound to Claude Code's lane; codex-alab carved out); A 66eb1f2635 (a vendor codex is shown, not
   supervised; codex-alab is watched only); C5 bda5fba852 (the Codex supervise policy); C4 9c478f5df8 (relaunch on
   exit for a vendor codex); P b4a2c00dd8, 3779aa8fd6, 7de90f0144, 2b13438d48 (the end-to-end proof on the real
   package, push lanes, a record never written is a crash, a crash no snapshot read reached); C3 fc74be92fe,
   c7bb57d2e3, 16604cc526, 1747dee394 (the Codex live upgrade, chips, wait arm and notices); C6 b97d43b87d (the
   Codex reader names its program alone) and f1bb14faee (words); then 3031eb5314, e68d4156d3, 71789cdbb1, b8631e8f07,
   7befe5a7d0, e77d26cda1, 7dca81c3d3, 14e0354905, db79132562, 33a5d74d66, 12fcb60d8b and the merges 324bd6710f and
   83bd43c206. AUDIT-claude-harness §0 rule 1 is amended in f1bb14faee: it quotes the owner's two phrases ("retire the
   codex harness"; "we still need codex-alab to work like codex for our purposes") and names the display-only rule as
   the decision taken under "you decide". The ratchet floors are lowered in C7, with fifth-column reasons. The
   codex-alab proof (`targo --unverified verify codex-alab`) passed every leg at 12fcb60d8b on the managed package
   `0.164.0-dev.12437+g51ba34fe1`, and again on the landed range. "Other targets keep the vendor lane until
   codex-alab ships there" no longer holds: after this release no target aterm ships has a vendor Codex lane, and a
   vendor `codex` on any target is display only.

---

# Appendix: the wire as each side implemented it (2026-10-16)

Recorded by the implementers where they had to differ from, or add to, the text above.

## Wire facts and contract amendments from the codex-alab client (alab-aterm, commit 4014cd40c)

- Subscribe runs on a SECOND connection: a connection that turns into a subscribe stream clears its
  connection-bound claim (control.rs:6100-6145). The claim stays on the first connection. CONTRACT §2 amended.
  aterm needs no change for this.
- Token file is `aterm-<pid>.token` (an explicit `<n>.sock` pairs with `<n>.sock.token`), not `aterm-<pid>.sock.token`.
- Handshake `AUTH <hex>` is never acknowledged; the client sends `version` to confirm.
- `status` prints `sid=0` (local id); the session id comes from `whoami` (`OK <sid> <nonce> <scope>`).
- The client re-sends the same-holder `meta set supervisor codex-alab@<pid>` as its per-act re-confirm: aterm must
  re-check the proof on that renewal (same connection, same peer pid).
- The client also renews the claim (the same request) whenever its record writer writes a `resumeArgv` that differs
  from the last record it wrote (the rollout reached disk after the claim, or a resumed session's first record came
  after it), so aterm's copy holds the line before any act. Added 2026-10-16: the end-to-end run's kill -9 before any
  act found a copy with no line (`no-resume-line`), and one with no record at all (read as an orderly end).
- macOS peer pid: tokio's peer_cred() reads LOCAL_PEEREPID, aterm's notes cite LOCAL_PEERPID: aterm picks one
  deliberately (the effective pid of the last writer vs the pid); the client's own pid is the TUI process either way.
- The client treats any ERR or malformed OK from `harness policy` as unsupported (never claims). Optional: a capability
  token in the `version` reply would avoid the ERR round trip.
- `harness-policy gen=<n>` needs a timeline kind and an entry in `timeline_wire_kind` (subscribe.rs:1247), or it is
  recorded but never pushed.
- Ruling F's arm goes up as `@self meta set harness_held same-reply|no-work` and down as `@self meta unset harness_held`,
  on the claim connection while the proven claim is held (aterm change, 2026-10-28, gap 4 stage 3: the standing proven
  holder only, else `ERR denied`; any other words answer `ERR usage: meta set harness_held same-reply|no-work | meta
  unset harness_held`; `status held=` names the arm while the claim stands and the tab's face carries a mode, keyed by the claim's
  proof, so the claim's end drops it and a re-claim by the same process that reaches aterm first keeps it; aterm
  a945f9abbb). An aterm before the verb answers
  `ERR unknown meta field (title|description|icon|role|attention|questions|autopilot)`, as for any field it does not
  know: the client reads any answer but `OK` and `ERR denied` as unsupported (`HeldOutcome`).
- Land's hold goes up as `@self meta set harness_land since=<unix_ms> nudges=<n> [read=1 legs=<leg>[,<leg>…]
  [open=<w>[,<w>…]] [changed=<n>] [untracked=<n>] [stashes=<n>] [unpushed=<n>] [unlanded=<n>]]` and down as `@self
  meta unset harness_land`, on the claim connection while the proven claim is held (aterm change, 2026-10-28, gap 11:
  the standing proven holder only, else `ERR denied`; `OK` changed or not; tokens in any order, each at most once;
  legs are aterm's `HoldLeg::word`s (`needs`, `repo`, `repo-unread`, `open`, `defense`, `cut`), `needs` never
  where every standing line is one the go-ahead answers (none on the menu bar; amended 2026-10-30); open words
  percent-encoded (a comma included), at most four, which aterm keeps only where `OpenWord::of` knows them; counts
  u16, saturating, `unpushed` the sum of `HEAD`'s commits no remote has, the submodules' commits no remote of theirs
  has and the commits on the branches the session used that no remote has (amended 2026-11-05 after the review of the
  forever probe: the verb takes five counts, and the hold's repository leg names all three); `read=1` needs a leg, and legs, open words and counts need `read=1`; any other line answers `ERR
  usage: meta set harness_land since=<ms> nudges=<n> [read=1 legs=… open=… changed=… untracked=… stashes=… unpushed=…
  unlanded=…] | meta unset harness_land`; `status land_held=<legs|-> land_nudges=<n> land_since=<s>`, all `-` for
  none, keyed by the claim's proof as `held=` is, shown only while the session's word reads Land and its face carries
  a mode; aterm e95f2082ae). An aterm before the verb answers `ERR unknown meta field
  (title|description|icon|role|attention|questions|autopilot)`: the client reads any answer but `OK` and `ERR denied`
  as unsupported (`LandOutcome`).
- The worker's needs go up whole as `@self meta set harness_needs entry=<pct text> need=<pct need> [need=<pct need> …]`
  and down as `@self meta unset harness_needs`, on the claim connection while the proven claim is held (aterm change,
  2026-10-28, gap 16, aterm 8517f2947b and d749fecec5: admitted as `harness_act` is, the standing proven holder only,
  else `ERR denied`; `OK` changed or not; `entry=` exactly once, the `owner=needs kind=ask` entry's text (one line
  within the attention cap, not empty once sanitized); `need=` one to 32 times in the worker's order, each whole, not
  empty once folded and at most aterm's `needs::NEED_TOLD_MAX` (`NEED_CHARS * 4`, 640) bytes decoded; every value
  percent-encoded UTF-8, a bare `-` sent as `%2D`; any other line (an unknown token, no or a second `entry=`, an empty
  one, no `need=` or more than 32, a need empty once folded, not UTF-8 or past the cap, a bad escape, a token after
  `unset harness_needs`) answers `ERR usage: meta set harness_needs entry=<pct text> need=<pct need> [need=<pct need> …]
  | meta unset harness_needs`, with or without a claim; aterm tells the set as its own loop's `store_needs` does,
  `ToldNeeds::tell` beside the `owner=needs` entry standing, settling an outgoing set when that entry goes or is
  replaced, in a self-claim field kept with the claim's proof as `held=` is (never the timeline's own-loop set), and
  every reader reads it through one accessor: the self-claim's set where a proven self-claim aterm does not hold stands,
  else its own loop's). An aterm before the verb answers `ERR unknown meta field
  (title|description|icon|role|attention|questions|autopilot)`: the client reads any answer but `OK` and `ERR denied`
  as unsupported (`NeedsOutcome`).

## aterm side as implemented (branch native-harness-20261016; §1, §2 fence/replacement, §4)

- `supervisor_proven=<0|1>` sits right after `supervisor=` in BOTH `status`
  (`… supervisor=<h> supervisor_proven=<0|1> autopilot=…`) and bare `meta`
  (`… supervisor=<h> supervisor_proven=<0|1> questions=… agent_cwd=… autopilot=…`). Read it by key.
  `sessions` rows do not carry it.
- The proof is taken only for the canonical holder `codex-alab@<pid>`: decimal, no sign, no
  leading zero, pid > 1, and only on a set with NO `ttl=`. It needs: a direct lane connection
  (never the bridge socketpair), its peer pid (`fdpass::peer_pid`, Darwin `LOCAL_PEERPID`) ==
  pid, live `tcgetpgrp(master)` == pid, the pid's kernel image (`proc_pidpath`) inside a Codex
  package whose root carries `alab-build.json` (the package `codex-package.json` names that
  image as its entrypoint — `alab/package.sh` layout v1, linked or installed; a bare
  `target/release/codex` out of a checkout proves nothing), and a readable kernel start (kept
  as the identity). Any other program leading the foreground group proves nothing.
- A proven claim is renewed ONLY by a proven request. An unproven `meta set supervisor
  codex-alab@<pid>` on a proven claim answers `ERR busy supervisor=codex-alab@<pid>`. So the
  client's per-act re-confirm must go over the same connection from the TUI process while it
  leads the foreground group. A renewal re-reads the record and does not wake aterm's host.
- A `ttl=` set is never proven (an ordinary claim, `supervisor_proven=0`), as the client's fake
  already does. A proven claim is never carried across a seamless update (handoff
  `claim_known=false`): re-prove within ADOPTED_CLAIM_GRACE.
- A replacement of the host's claim is ANSWERED only once every write the host's fence admitted
  has landed (send/key/paste counted to their reply, a `turn` per phase, a main-thread input hop
  re-checked as it is applied). So the `OK` to codex-alab's claim can wait for an in-flight host
  write (bounded by that write's own deadline); every host write after it is refused.
- Fence refusal: `ERR busy supervisor=<live holder pct-encoded|->`. Usage:
  `ERR usage: if-supervisor=<holder> needs the holder the claim was made under`, and
  `ERR usage: if-supervisor= given twice`. On `paste` it is the ONE leading option, followed by
  exactly one space; the body after it is raw. On `turn` it is checked before each phase too.
- REQUIREMENT ON THE CLIENT (orderly end): aterm reads the record only while the identity is
  live:
  - at the proven claim;
  - after every `meta` write that changed something;
  - at the claim's release while the process still runs;
  - while a claim or (after a passive release) a read-only copy names the thread the rollout tap
    reads and the process leads the tab's front, at each rollout-tap ask, and once more two
    seconds after the tab's screen last moved while the record names a count (read only: its
    `pid`, `sessionId`, `status`, `exit`, `goal` and `backgroundTerminals`; it decides nothing;
    amended 2026-10-29, gap 28, §4 aterm (copy)).
  On `/exit`, `/quit` and archive, remove the record and THEN send
  `meta unset supervisor holder=codex-alab@<pid>` before exiting. If the process dies with no
  live read after the removal, aterm's last copy still shows the record standing, and the exit
  is read as a CRASH and relaunched. (codex-alab amendment, 2026-10-16, §4: a SIGHUP no longer
  removes the record; it keeps it as `exit:"signal:1"` with the status the signal found, which
  this side reads as any signal it did not send, orderly. The relaunched codex-alab after
  aterm's own crash reads that witness to carry on, and only while its first `status` read
  shows this side's relaunch hand, `hand=lease:aterm-harness@<pid>` (§5 carry-on). No aterm
  change.)
- `status:"exited"` with `exit:null` reads as orderly. Any `exit` word other than `signal:<n>` is
  orderly, unless `status:"upgrading"`.
- PASSIVE RELEASE KEEPS THE COPY ONLY TO READ: when the claim is given back while the process
  still runs and the record (read at that release) shows no end coming (standing with
  `exit:null` and a status other than `exited` — `upgrading` included), aterm withdraws the
  copy's resume line and withdraws the copy from every act: no exit decision, relaunch, restore
  line, landing or `result` reads it. From then until the next proven claim the session's exit
  is decided by nobody, and nothing relaunches it (fail closed). aterm keeps the copy READ ONLY,
  in memory, until the process ends or the tab closes, and taps it only while that process leads
  the tab's front and is alive as the start its claim proved. It re-reads the record at each
  ask, so a `/new` or `/resume` is followed. A record that names no thread taps nothing; one
  that shows the end or is removed ends the read until the next proven claim. An unclaimed
  `meta set harness_act` stays `ERR denied`. An aterm that restarts or updates keeps no
  read-only copy. (Amended 2026-10-28 after the parity sweep, gap 15, aterm 98630ac574: dropping
  the copy blanked a codex tab's survey facts, goal and session-log turns exactly when a person
  drove it by hand.) An ALab image in front keeps the watch-only worker, which watches only: no
  stall or memory remedy runs without a standing proven claim (§4). A release that shows the end
  (record removed, an `exit` word, `status:"exited"`) keeps the copy for the exit. A re-exec
  upgrade therefore re-proves in the new image to get a copy again; until it does, a crash of
  the new image is decided by nobody, and nothing relaunches it (fail closed). (codex-alab
  2026-10-17, decided again, §2: a transient off (a hold, a suspension, a failed policy read)
  keeps the claim and so the copy; a person's or the config's off releases it and withdraws the
  copy from every act, where aterm's host runs no worker and relaunches nothing anyway.)
- `resumeArgv` VALIDATION (stricter than the contract text; the client must produce exactly this):
  - `resumeArgv[0]`: the live process's KERN_PROCARGS2 exec path, its `proc_pidpath` image, or
    atpkg's managed twin `<prefix>/agents/codex` while that twin's `exec '<path>' "$@"` names an
    ALab build (package root with `alab-build.json`). The twin is the expected value (contract
    §4: the PATH-resolved launch path, so a relaunch runs the current managed build).
  - the line ENDS with `resume <sessionId>`; nothing after it.
  - the elements between `argv[0]` and `resume` are EXACTLY, in order, the live process's root
    flags as `harness::codex_alab::rewrite_argv(live_argv, None)` reads them (moved there from
    the retired `upgrade_codex` by aterm 3999d9adc6, retire C2; the flag table, unchanged, of
    `codex --help` 0.157.1; every known flag kept verbatim with its value — `-m x`,
    `--model=x`, `-c k=v`, `-s`, `-a`, `-C`, `--add-dir`, `-p`, `--search`, `--oss`,
    `--no-alt-screen`, `--dangerously-bypass-approvals-and-sandbox`, … — and the prompt, `-i/--image`
    with its files, `--last`, `--all`, `--include-non-interactive`, the subcommand `resume|fork`
    and its session id dropped). A live argv with a flag that table does not know, or a refused
    one (`--remote`, `--worktree`, `-h`, `-V`), relaunches nothing (`live-argv`). Any difference
    is `foreign-argv`.
  - `cwd`: when the record names one it must equal the live process's cwd (as spelled or
    canonical), else `cwd`; the relaunch `cd`s to the LIVE process's cwd, never the record's.
- The copy is kept in aterm's memory. The handled-witness ledger is
  `<aterm harness state>/drive/self-claim-witnesses`, never CODEX_HOME. The resume line the copy
  validated rides the tab's restore leaf (`CodexRun.self_claimed`, the restore's additive `alab`
  table) and an owed relaunch across a seamless update (the handoff layout's leaf, a failed
  Commit's take-up): after aterm's own crash, a restart or a seamless update, the restored tab
  relaunches codex-alab on that validated line through the crash rule's own relaunch (its
  witness ledger, the writer lock, a shell at its prompt). An exit by the agent itself
  (`/exit`, `exit:"fatal"`, a foreign kill, exit 0) never relaunches from a carried line: a
  handed snapshot is published with its line for the crash image and kept without it as the
  exit's decider, so such an exit before the successor's re-proof is decided by nobody, and
  nothing relaunches it (fail closed). An ALab image in front keeps the watch-only worker, which
  watches only: no stall or memory remedy runs without a standing proven claim (§4). Otherwise the copy is re-read at the next proven claim, and a read-only copy at each rollout-tap ask. (Amended 2026-10-16,
  aterm main 401229b3de, 23e60f1750, 20965dbd26, ce767c6ae4.)

## aterm side as implemented, Rest (§0.1, §3, §5, §6, §7, §8; branch native-harness-20261016)

- `harness policy` (§3): `OK gen=<n> effective=<on|off> why=<word> <every texts() key>=<pct|-> … questions=<word>`.
  Keys are all of `SupervisorConfig::texts()` in `KEYS` order (not only the ones codex-alab honours; 34 when this was
  written, 36 at aterm main 78cf35a481: codex-alab reads the keys it knows and leaves the rest), then `questions=`
  last. codex-alab also reads the additive `done_check_text`,
  `land_text`, `land_nudge_text`, `land_answer_text`, `needs_hint_text` and `needs_ack_text` where an aterm sends
  them, anywhere in the reply (§3; amended 2026-10-19). `-` means empty. `why=` words, first refusal first: `unhosted`, `disabled`, `headless`,
  `agent-off` (the session's agent's `[harness.<agent>]` table; Codex when the server names none), `suspended`,
  `hold`, `autopilot-off` (ADDED: the session's or the table's Autopilot `off` is the harness's hands off, so it is
  an off for codex-alab too); `enabled` when on. Only `continue_text=` is composed with the rules file
  (`continue_text (standing rules: <one line>)`, exactly the loop's); `answer_text=` is raw, as the contract says.
  An edge token gets exactly `OK effective=<on|off>` (no gen/why). Any other form: `ERR usage: harness policy`.
  The event is `EVENT <local> harness-policy gen=<n>` (timeline kind `harness-policy`), recorded on every session
  only when the policy cell adopts a newer stamp; a hold, an Autopilot word or a suspension moving `effective=`
  raises no `harness-policy` event (a hold and the words have their own `hold`/`meta` events).
- `meta prepare harness_act` and `meta set harness_act <act> until_us=<n>` (§5; aterm 0.110.0, bf9d401e40): `OK
  until_us=<n> lease_ms=5000`, granted only to the proven holder (else `ERR denied`); a conditional set at or past
  its deadline is `ERR expired harness_act`, a conditional `left-to-person@v1` whose point did not read at rest is
  `ERR busy harness_act` (the paired host extends this predicate to conditional `landed@v1`; hosts through public
  0.147.0 do not), and a malformed form `ERR usage: meta prepare harness_act | meta set harness_act <act>
  [until_us=<n>]`. An aterm before it answers `prepare` with `ERR usage: meta [set <field> <text...> | unset
  <field>]`.
- `meta set harness_act <act>` (§5): `<act>` is one owner-key token. Allowed on the claim's bound connection, or on a
  direct connection the kernel proves is the claim's pid again (peer pid + foreground leader). Else `ERR denied`.
  `unset harness_act`, no act, two words or a bad token: `ERR usage: meta set harness_act <act>`. Recorded as
  timeline `harness-act` `act=<act> holder=<holder>` (not pushed on `events`), `status last_act=<act|->`. It counts
  as a meta change, so the record copy is re-read with every act. From aterm `cf81983a27` and `f2e46c7dc7` (2026-10-28),
  an admitted act also lights the Autopilot chip and adds a trail/`last:` entry under its holder (`acts::self_claim_sort`:
  `approve-all@v1` and `plan-approve@v2` approved, `answer-recommended@v1`, `answer-long-term@v1` and `answer-short-term@v1`
  answered, typed acts sorted by `typed_act`; `landed@v1` shows on the face), and books one `SelfClaimRow`
  (`"decision":"self-claim"`, rule `self-claim-act@v1`, `holder`, `harness_act`, `act` word from `acts::self_claim_act`)
  in `<state>/drive/<sid>.jsonl` under the striped ledger lock (`approvals::book_self_claim`), folded into
  `HarnessStats`/`Tally::self_claimed` by `claimant_name(holder)` (`codex-alab`) and never mixed into aterm's own counts
  or loop readers, and writes one `mark <node> act word=<w> rule=<act> holder=<h> harness_act=<act> by=<a> src=mark conf=claim`
  to the session log (`SessionLog::self_claim_act`, landing on `Log::open_decision_step()` when an approval or question step
  is open, else the deepest open node; aterm `1f38834f0a`); a standing face's act told again at the same program turn is
  answered as before and shown/booked once.
- `status` gains `program_state=<state>[:<kind>]|-` and `last_act=<act|->`, placed right after the agent verdict's
  fields (`… agent_fp=<x> program_state=… last_act=… os_appearance=…`). Read by key.
- §6 7501: a report is taken only with `app=codex` (any other app is ignored); a body with a `;` inside is ignored;
  `kind=` is kept only on `blocked`. The display "on the band" is the presence row's phase word (`codex working`),
  shown only where the screen's agent reader names no phase (agent reading absent or `unknown`); the screen's
  verdict keeps the slot otherwise. A report the sweep did not see between two sweeps is not shown.
- §7 guard: "copy= other than the vendor store" is implemented as "the exe's package root carries
  `alab-build.json`, or there is no package (version unreadable)": the store holds vendor bytes under ALab index
  build ids as well as vendor-direct ones, so a store path cannot tell the bytes apart; the package marker can.
  A managed codex-alab is no vendor target either, and a non-vendor crash is relaunched on the program it ran.
  Retired since (aterm fc74be92fe, retire C3): `upgrade_drive` steps only Claude Code's lane: no Codex is moved live.
- §8: a dev-linked agent program's `agents/` twin carries NO self-update block (`codex update` is not handed to
  atpkg for a developer's checkout) and the link's `bin/` shim is never rewritten.
- §0.1: DESIGN-harness-never-block and RFC-embedded-agents-2026-08-15 were deleted on main (36c524ec0) before this
  work; their supersession lines went to AUDIT-claude-harness §0 (questions) and DESIGN-aterm-wrapper (the RFC).
  The wrapper design no longer has §6.3/§10 headings; the record is in its Decisions list. AUDIT's "one engine"
  rule is number 8 in the current text (the contract says 7). The codex row of
  DESIGN-atpkg-vendor-direct-updates has not changed, so it is untouched; codex-alab's alab/README is not in aterm.
  Since the retirement (aterm f1bb14faee), AUDIT-claude-harness's supersession reads: "Rule 8 keeps holding for every
  vendor binary; rule 1, amended the same day, supervises no vendor `codex` (display only)." (It read "Rules 1 and 8
  keep holding for every vendor binary".)

### Local continuation corrections (2026-10-24)

The repository probe retains an explicit unread timeout through Land, Cruise,
landing rejudgement and carried decisions. A deadline is no clean result; Land
continues with a verification nudge and can land after a later completed probe.
The unavailable-folder/no-repository policy is unchanged.


The NEEDS YOU reader additionally keeps a genuine first line such as
`NEEDS YOU: and the release credential`. Complete fixed carry-on instruction
fragments remain excluded. This local correction extends the aterm parity
snapshot above and has explicit alert-folding and reader regressions.

The Land reader additionally keeps a negative control that failed to fail or to
perform its check open. Historical, resolved and denied failures and genuine
negative-control proof retain their existing readings. This local correction
extends the aterm parity snapshot above and has open/clean regression controls.

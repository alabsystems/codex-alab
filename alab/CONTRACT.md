<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# codex-alab native harness: the contract between codex-alab and aterm (v2)

v2 folds in two adversarial reviews (codex side, aterm side), and the appendix records the wire each
side implemented. aterm's half lives in aterm (`crates/aterm-gui` self-claim and fence, the watch-only worker and
crash rule in `crates/aterm-agent`, `harness policy`, OSC 7501 in `crates/aterm-core`); codex-alab's in
`codex-rs/alab-core`, `alab-aterm`, `alab-autopilot` and `alab-tui`.

Owner's goal (2026-10-16): "optimize codex-alab so that it has a similar feature set as we have for the claude code
harness and retire the codex harness ... more deeply integrate into aterm and introspection". Owner, same day:
"you decide". Decided: codex-alab does natively what aterm's screen-scraping Codex lane did, and reports exactly to
aterm. aterm's Codex screen lane is deleted (2026-10-16, §9 step 4; aterm main `28e13d46ba..83bd43c206`). aterm admits
a Codex only as codex-alab, and watches beside it.

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
    so none runs, and nothing relaunches it. This is the plan's D-2 default, accepted as fail-closed. codex-alab
    keeps its claim through a transient off and gives it back at a person's or the config's off, under which aterm's
    host runs no worker and no remedy anyway (§2). The transient offs: a hold, under which aterm's host stands beside
    the session (its remedies are real); a suspension, under which the host is off, but which aterm makes at every
    seamless-handoff Commit, where the successor adopts the standing claim and a Commit that fails resumes this host
    with it; and a policy read that failed, which says nothing about the host.
- Nothing polls on a clock where an event exists (AUDIT rule 7): the claim is connection-bound and changes arrive
  on `subscribe @self`. The one clock: while passive, codex-alab reads `harness policy` every 30 s, because no event
  announces a suspension and a session may start off; never while what it waits for has an event of its own
  (another holder's claim, a claim someone cleared, one aterm did not prove) (§2).

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
  once, at the supervisor's first connection (one look a process, whatever it met), and only codex-alab's own entry
  while aterm shows it (`attention_owner=codex-alab`) reading exactly the sign-in screen's text (`login: signed out;
  codex's sign-in screen waits for a person`, `login::clear_stale`; amended 2026-10-17 after the review of
  4cdc96090, finding 6: the attention outlived the sign-in through the folder-trust step, and for good after a kill;
  amended again after the review of fea034d20, finding 4: a `login: …` prefix also took the autopilot's own sign-in
  attentions, still true, and a reconnect could clear one this process had just raised).
- **How:** `@self meta set supervisor codex-alab@<pid>` with NO `ttl=` on codex-alab's own connection: the claim
  lasts while that connection serves requests and ends when the process dies. `@self` resolves from
  `ATERM_PARENT_SESSION_ID`.
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
  into a hands-off session; another supervisor met `ERR busy`; mail receipts and reports ran while passive. So:
  - TRANSIENT offs keep the proven claim and act on nothing: a person's `hold` (as always: every act waits on
    `status hold=`, and attention, the description and mail stand), `why=suspended` (aterm suspends its host at
    every seamless-handoff Commit: a successful one exits this aterm and its successor adopts the standing claim, a
    failed one resumes this host with it; §0), and a policy read that failed (an error, or a reply the client cannot
    take from the aterm that proved the claim). Under a suspension or a failed read codex-alab is on standby: its
    attention is cleared, its description withdrawn, a box answer the TUI has not applied taken back (one the TUI
    already took keeps its record until its outcome comes, which is recorded once codex-alab acts again), and
    nothing acts. Mail typed into the session is still read (aterm's `drive task` types `Inbox: task @<off>`
    whenever it shows the claim proven, and a line left unread would reach the model bare, with no fence and no
    receipt), and a mail turn that ends there records its verdict, receipt and report as owed; they are sent once
    codex-alab acts again. The record still names the claim, which is still renewed when the resume line moves. A
    hold read after a standby lifts the standby: the hold's own rule is the one left.
  - A PERSON'S or the CONFIG'S off gives the claim back (`meta unset supervisor holder=codex-alab@<pid>`, as before
    b24e535df): `autopilot-off`, `agent-off`, `disabled`, `headless`, `unhosted`, and any `why=` word codex-alab does
    not know. codex-alab is passive the moment it decides so, before the release is answered (an act whose checks
    run meanwhile meets no claim held), and its attention is cleared and its description withdrawn. The release's
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
    ordinary proven claim, as soon as the policy reads `effective=on`. It reads the policy again at a
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
  `answer_text`, `questions`, `retry_api_errors`, `resume_limits`, `limit_wait`, `compact_on_context_wall`,
  `relaunch`, `upgrade`, `upgrade_ask`, `human_grace_s`. `stall_term*` and `memory_ceiling` stay aterm's (§4
  watch-only worker).

## 4. The session record and crash relaunch

- Path `$CODEX_HOME/alab/sessions/<pid>.json`; dir 0700, file 0600; written by a background task, coalesced to at
  most one write per second (`<pid>.json.<nonce>.tmp` then rename); never on the render or input path. Two changes
  are written THROUGH, never coalesced, because aterm may end the process at the record's rest (watch-only worker,
  below): leaving rest (`idle` to anything else) and a person's first keystroke (`draftEmpty` to `false`). Every
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
  before its first record write (§5 carry-on).
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
 "carriedOn":0}
```

- `status:"idle"` only at true rest: the frame reads idle (or done, or error) AND no turn runs (from its start hook
  to its stop or abort, or the thread's stop), no act is being carried out, no box answer is posted and unapplied, no
  carry-on is owed, no person's turn is submitted and unstarted, no queued input, no upgrade under way. A wait on a
  timer is rest: the act it leads to leaves rest first. `draftEmpty` is a JSON boolean; a key, paste or click makes
  it `false` until the next frame shows the composer (so the first keystroke reaches the record before its echo).
  (Amended 2026-10-16 for the watch-only worker's idle point.)
- `carriedOn` (added 2026-10-16, still v1: a record without it reads 0, and aterm ignores it): how many
  relaunch carry-ons in a row led to this process with no person's turn and no done since (§5 carry-on). The
  carry-on sets it (its predecessor's plus one) with the act's own `busy`, before the turn goes; a process that
  declines the carry-on keeps its predecessor's count, and every start takes its own predecessor's count at the
  sweep, before its first record write, so a relaunch that dies before its carry-on decides hands the next one
  that count, never 0 (amended 2026-10-16 after the review of 363789134); a person's turn, or a root turn that ends done
  (`ATERM-TASK-DONE`, or a done reply after the done check: where the autopilot decides the task is finished),
  sets it back to 0. A root turn that merely ends cleanly does not, nor does a first done reply, which gets the
  done check ("...otherwise continue") (amended 2026-10-16 after the review of 148a9fedb: relaunch, carry-on,
  clean end, continue, stall repeated, bounded only by stall detection and aterm's back-off; and after the review
  of cf084e3b7: one "completed successfully" per round reset it). The live upgrade's re-exec keeps it: the new
  image starts from the old one's count.
- `procStart` in Claude Code's `lstart` format. `identity` follows upstream's daemon `ProcessIdentity` (copy the
  ~60 lines). A record is live only if the pid exists and its identity matches.
- `resumeArgv[0]` is the launch argv[0] as PATH resolved it at launch (atpkg's twin or reroute path, so a relaunch
  runs the CURRENT managed build), never `current_exe()`; the rest is rebuilt from codex's own clap `Command`
  (no flag table): the root flags by position minus PROMPT and images, then `resume <primary thread>`; a fork
  resumes its forked id. `null` until the primary thread's rollout is materialized. `env` holds at most
  `CODEX_HOME`, and only when set at launch.
- Removal: `/exit`, `/quit` and archive remove it, and THEN (while the process still lives) codex-alab sends
  `meta unset supervisor holder=codex-alab@<pid>` on its claim connection: aterm re-reads the record at that meta
  change and so sees the orderly end; without it aterm's last copy still shows the record and the exit is
  relaunched as a crash (aterm's wire, Appendix). `status:"exited"` with `exit:null` is also read as orderly, and
  so is `exit:"fatal"` by the crash rule (aterm's `self_claimed::verdict`). codex-alab's Fatal exit leaves with
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
  new image rewrites it. A panic, SIGKILL or power loss leaves it.
- **aterm (copy):** aterm reads the record only while its identity is live: at the proven claim and at each
  `meta-change`, and keeps its own copy in memory/its `<state>/drive/` ledger.
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
  `CODEX_HOME` equal to the live process's own. Otherwise no relaunch and one Messages record. Each handled witness
  `(pid, identity)` is recorded in aterm's ledger and never relaunched twice.

## 5. What codex-alab does natively, while its proven claim is held

`codex-rs/alab-autopilot` (an app-server extension, one install line in `app-server/src/extensions.rs` as the first
statement after the goal block, ABOVE the git-attribution `if` and before `codex_guardian_v2::install`), the aterm
client in `codex-rs/alab-aterm` (one connection, background tasks only), TUI hooks in `codex-rs/alab-tui`, and the
shared in-process state in `codex-rs/alab-core` (one static per process; embedded mode only, which ALab forces).
Only root interactive threads the TUI registered; never exec, app-server, sub-agents. Every `aterm` interaction is
a background task with a 5 s timeout, never awaited inside a lifecycle callback, a render or an input hook; a
timeout is "unanswered" (passive). With no `ATERM_PARENT_SESSION_ID` or no reachable aterm socket: nothing.

**Before every act** (submit, approve, answer, compact, upgrade): read `@self status` on the connection; act only
with `hold=0`, `hand=-` or its own, `input=` `-`/`clear`, `human_ms` ≥ `human_grace_s`·1000; its own TUI input
stamp older than the grace; the composer empty; no queued, pending-steer or rejected-steer input in the TUI; no
user turn pending start; no modal or popup open (the observe hook exports these); the claim re-confirmed; and, last,
under the supervisor's lock, that codex-alab still acts (an off read while the status read or the re-confirm was in
flight sends nothing and moves no epoch, so it is looked for there). Every act looks again right before its
irreversible step, after its own awaits (the record's write-through, the thread's mode and goal reads, the approval
facts). Anything else: wait, look again at the deadline. Unreadable status: wait. (Amended 2026-10-17 after the review
of 4cdc96090, finding 1: the checks read the standby only at their start.)

| feature | native behaviour | mirrors aterm |
|---|---|---|
| turn end | only an idle that follows a turn THIS process ran and saw end (`on_turn_stop` recorded a `LastTurn`) | `decide_turn_end` |
| continue | submit `TurnInput::UserInput{client_id:"alab-autopilot-<n>"}` with `turn_trigger:"alab_autopilot"` and text `continue_text`; refuse in Plan mode (core admits User input there); none while a goal is Active | `continue@v1` |
| taskless | no act until a person's turn exists: a `UserMessage` whose `client_id` is not codex-alab's (live), or, on resume, a `TurnStarted` whose `turn_attribution.turn_trigger` is not `alab_autopilot` (missing attribution = a person) | taskless rule |
| carry-on | a thread resumed by aterm's crash relaunch or aterm's restore after its own crash (this process started with exactly the record's `resumeArgv`, the record < 24 h old, died busy (`status:"busy"`), `exit` absent, `signal:15` (aterm's remedy) or `signal:1` (the terminal went away), AND aterm's proof that it typed the line: the launch probe, the aterm client's FIRST `@self status`, read as the process starts on the one connection the claim then uses and before anything waits for a thread, shows aterm's harness's hard lease on the tab (`hand=lease:aterm-harness@<pid>`, which aterm takes before it types the relaunch line and holds to the new process's first idle: until this process's claim is proven, which it makes only after this read on the same connection, and its record names its TUI's frame, at rest or already in a turn that the lease did not hold back (a person's), at most 60 s; aterm 0.106.0 gave it back as the new TUI led the tab, before this read, and aterm 5eebeff407 and cc61408e74 hold it so again (amended 2026-10-17, no codex-alab change)) and no person's input from 5 s before this start to the read (`human_ms`), the read within 60 s of the start; no read, an unreadable `human_ms`, another hand or none is no proof) — never after codex-alab's own re-exec, which happens only when nothing was owed — with a task and a last `TurnComplete.last_agent_message` that is neither `ATERM-TASK-DONE` nor a done reply gets `continue_text` at its first verified idle; any other resume idles until a person's turn. At most one carry-on per chain: a predecessor whose record says `carriedOn` 1 or more (a carry-on led to its death with no person's turn and no done since: the continuation, at once or after the autopilot's own turns, stalled again and aterm restarted it, or it crashed again) gets none, whatever its exit word, so one stall's restart carries on, as aterm's remedy intends, and a repeat of stall, restart, continue waits for the person instead of looping until aterm's back-off. (Amended 2026-10-16 after the review of 6efbf7518: under the managed link a person's own `codex resume <id>` runs exactly the record's `resumeArgv`, since the link execs the package by its absolute path, so the line alone let a person who closed a tab mid-turn and resumed it by hand get an unasked continuation. No aterm change: the lease is aterm 0.105.0's own relaunch hand.) The lease must be named for the aterm process itself: `aterm-harness@<pid>` where `<pid>` is the control socket's server as the kernel names the connection's peer (`LOCAL_PEEREPID`); a lease named for any other pid, or an unknown peer pid, is no proof. THE HOLDER BINDING: the name alone names only the aterm that answered the read, and aterm's pid is public (`ps`, and every client's own `LOCAL_PEEREPID`), so any client of the socket could take a lease under exactly that name. aterm reports who took it: `status hand_pid=<pid>`, the kernel peer of the connection that took the lease, read at the acquire and never from the holder's text (`-` for no live lease or an unknown taker). Where the token is present, the lease is aterm's only when `hand_pid` equals the server pid as well (both kernel-attested), so a lease another process took under aterm's name is no proof, and `hand_pid=-` is none either; an aterm that prints no `hand_pid=` keeps the name check alone. (Amended 2026-10-17: aterm main reports `hand_pid=`.) One carry-on per process: the predecessor and the launch read are facts of this start, so the process's first resume spends them, whatever it is a resume of and whatever it learns (a sweep not done in time, a history that could not be read), and a later resume in the same process (`/resume` away and back) is no relaunch; nor is any resume more than 60 s after the start (the launch probe's window); a process whose own `carriedOn` already reached the maximum carries on no more. The chain (`carriedOn`, §4) ends only at a person's turn or a turn the autopilot finishes the task at (`ATERM-TASK-DONE`, or a done reply after the done check), never at a turn that merely ended cleanly or a first done reply, so relaunch, carry-on, clean end, continue, stall again waits for the person. WHAT COUNTS AS aterm's RELAUNCH: every line aterm itself types under its relaunch lease carries on once, whatever made aterm type it: its stall or memory remedy, its restore after its own crash, its restore after a logout or a shutdown (which keeps the agent in its quit layout), and an asked `aterm ctl restart` (`cause=asked`) of a busy session alike; the record's `signal:15` or `signal:1` cannot tell them apart, and none of them is a person typing the line. A person who wants a session to stop, not to continue, puts a `hold` on it or uses `/exit`. (Amended 2026-10-16 after the review of 148a9fedb: a /resume away and back carried on twice, a lease named for another pid than aterm's was trusted (one forged under aterm's own pid still passes: above), the second tab of a joint relaunch got no carry-on, and the asked restart's carry-on is now stated. Amended again after the review of cf084e3b7: a first resume that learned nothing left the relaunch unspent for a later `/resume` back, and a first done reply ended the chain.) | carry-on |
| done | last agent line exactly `ATERM-TASK-DONE` (Markdown punctuation allowed) → done, nothing until a person's turn; a reply matching aterm's `DONE_PHRASES` → `DONE_CHECK` once; a second done reply → done | `DONE_CHECK`, `FINISHED` verbatim |
| back-off | turn with < 2 min of work is short; consecutive short turns double 2 → 60 min; `continue_per_hour` caps; compaction idles are not turns | `TurnEndTiming` |
| prose questions | a question, stop phrase or offer in the last agent message, or `AgentMessageItem.questions: Some(..)` → `answer_text` (or the `questions` word's policy text); one naming an irreversible act (aterm's `DESTRUCTIVE_WORDS`) → aterm's `REVERSIBLE_ANSWER` | `RULE_ANSWER` |
| question overlay | `request_user_input` STAYS. The TUI hook answers the overlay with its recommended option (else option 1), per the `questions` word; an unmarked question naming a destructive act → its refusing option, else keyed attention; `ask` → left to a person with attention | question.rs |
| plan box | under `approve=all`, after the grace, the TUI hook selects "Yes, implement this plan" (never "clear context") | `plan-approve@v2` |
| folder trust | before any claim (so outside the checks above; `effective=on` carries a hold), the TUI hook (`ALAB(trust)`, `alab-tui/src/trust.rs`) answers codex's launch folder screen as aterm's `trust_dialog` does: embedded app-server only; never a project marked untrusted; only this process's launch folder (never `-C` or a resumed task's other folder); `harness policy` read on a connection of its own within 2 s, `effective=on` with `approve=safe` or `all`; under `safe`, the folder AND codex's trust target (a Git subdirectory's repository root) each under a `trust_roots` root (aterm's `roots_from_config` spelling), as written and symlink-resolved (`trust-dialog@v1`); under `all`, aterm's default, any launch folder, as aterm's full power presses "Trust and continue" for any folder (`trust-any@v1`; a rooted folder keeps `trust-dialog@v1`). Then codex's own write (`projects."<target>".trust_level = "trusted"`, the screen's "Trust and continue" path), no screen, one info line on the launch session card naming the folder and why (the root, or full power). Anything else (an older aterm, no socket, a timeout, a failed write): the screen, for the person to answer. aterm's Codex lane is retired, and a vendor Codex's trust gate is answered by nobody | `trust-dialog@v1`, `trust-any@v1` |
| approvals | `Reviewed(Approved)` (one-shot) under `approve=all` UNLESS: `require_guardian`, or `require_fresh_review` under the `AutoReview` reviewer (under `User` the fresh review is the person's box, and codex-alab's one-shot answer is it: an escalated exec or a retry is approved once, amended 2026-10-16 after the end-to-end run); the effective policy is `UnlessTrusted` (explicit or from an untrusted project); the effective `approvals_reviewer` is `AutoReview` and not from packaged defaults; a requirements constraint fixed policy or reviewer; a non-heuristic execpolicy `prompt` rule matches (a shell wrapper it cannot parse also defers, while any rule could ask); a `granular` flag for this kind is false. Then `None`. An explicit `on-request` is not an ask rule. A box cleared by that list while the before-every-act checks say "not now" (the grace, a person's recent key) goes to the TUI, and the TUI hook selects its one-shot yes once the checks pass (never a standing yes) | `approve=all`, owner rules win |
| left boxes | any box the TUI shows (`Blocked(*)`) that codex-alab has not answered within `human_grace_s` raises `meta set attention owner=codex-alab <kind>: <why left>`; cleared when it closes. An approval names the reason the approval contributor left it (the owner's rule, `approve` not `all`, …), not the wait it met. A turn end codex-alab leaves raises it too: Plan mode (`plan: …`), a goal that stopped by itself (blocked, or out of its token budget: `goal: …`), a usage-limited goal whose resume at the reset failed (`goal: …`, tried again after a minute), an act that did not go (a turn core did not start, a compaction core refused: `turn: …`; a turn core declined because another turn is running or took the slot answers that turn's end instead), a prose question it does not answer, a login or a wall it does not wait out; a person's turn clears them, and so do these: codex-alab's own yes to the plan box, once it goes in, clears `plan:` (it implements the plan itself under `approve=all`), and a goal that reads active again at a turn's start (a person's `/goal` resumed it, or the reset did) clears `goal:` (amended 2026-10-17, inbox finding 8). A point whose attention a passive spell cleared (§2) still stands (an attention answers no point; a turn's end does) and raises it again when codex-alab acts again, never while a turn of its thread runs (amended 2026-10-17 after the review of 4cdc96090, finding 3). A held claim that is not passive (§2) with an unanswered box and no attention is a defect | rule 6, never-block |
| API / stream / server | after core's own retries: 1/5/15/30/60 min (network class 1/2/5), for ever, reset by a turn of real work | `retry_backoff`, `net_backoff` |
| usage window | `UsageLimitReached{resets_at}` → at resets_at + 60 s + per-session jitter ≤ 60 s: a thread whose goal is `UsageLimited` is resumed by `GoalService::set_thread_goal(Active)`, else `continue_text`; a continuation that hits the wall again: 10/30 min; `QuotaExceeded`/`UsageNotIncluded` → 30/120/360 min; never buy | `limit_backoff`, `entitlement_backoff` |
| context window | `ContextWindowExceeded`, once per wall, at a verified idle with `agent_status()` idle read just before: `Op::Compact`, mark the next idle as the compaction's, then continue; a second consecutive wall, or auto-compaction disabled / token-budget mode → attention | `compact_on_context_wall` |
| login wall | `Unauthorized`/refresh failure → nothing typed; the one keyed `meta set attention owner=codex-alab`. A signed-out first launch, before any claim: `login: …` while the sign-in screen is up (§2 (a)) | irreducible escalation |
| goals | a goal paused by a hold is resumed with `thread/goal/set`-equivalent `GoalService` calls at the reset, the relaunch and the re-exec | goal hold |
| live upgrade | trigger: the build the launch path (`launchArgv0`, atpkg's twin) now names differs from `current_exe()`. Preconditions: every loaded thread (`list_thread_ids`) `agent_status` idle with no background terminals; no goal Active; no autopilot timer pending; the before-every-act checks. Then: record `status:"upgrading"` kept; teardown as `/exit`; MCP children get 2 s, then exec anyway; `execve(launchArgv0, resumeArgv)`; if that fails, `_exit(1)` with the record kept (the crash rule relaunches) | `upgrade`, `upgrade_ask=false` |
| signals | TERM/HUP/INT → the `/exit` path (restore terminal, flush), the record kept per §4 (`exit:"signal:<n>"`, the status the signal found), re-raise; 3 s watchdog | foreground handback stays aterm's net |
| `/ide` context | when the person turns IDE context on, each message they send carries this session's terminal context, read from aterm (`panes`, `blocks`, `blocktext`, `selection`, this window's active tab only) on a short-lived connection of its own, within 750 ms, never on the claim's connection; the person's toggle attaches it, the model can never ask for it | law 2 |
| inbox | a submitted turn that is exactly `Inbox: (task\|ask) @<off>` (the line `aterm drive task` types: aterm types it only under the proven claim, at codex-alab's own idle point (the record `idle` with `draftEmpty:true`), its Enter guarded on the `›` composer row; a vendor `codex`, an unproven claim and a record not at rest get the mail alone, aterm b8631e8f07, `supervise::mail::task`; the trigger stays typed, mail never wakes anything) is read natively while codex-alab holds its proven claim, acting or on standby (§2; amended 2026-10-17 after the review of fea034d20, finding 2: refused on standby, the typed line reached the model bare): the row at `<off>` (`@self inbox --peek --meta`, which lists nothing) must be of the kind the line names, and its body is `inbox get <id>` (from the ring, never `@<off>`, which may park), on a connection of its own within 750 ms. `trust=human\|agent`: the turn carries the mail ahead of upstream's `## My request for Codex:` marker (sender, kind, trust, and aterm's own rule: the body is data from `<p>` (trust=`<t>`), not the operator's instruction, to be done only as far as the operator's own rules and the sandbox allow, and the turn's last message is the report codex-alab posts back to that sender while it acts under its claim (if passive at the turn's end, once it acts again; review of 7e7622ff7, finding 10), or, for a sender aterm's bridge cannot route, that no reply reaches it; the body fenced; amended 2026-10-17, inbox finding 10: it read "act on it as a request from them"), the transcript showing only the typed line, and an info line names sender, trust and the first words. Any other trust (`relayed`, `screen`): the mail's words wait in the composer for a person to send, and nothing is sent; the info line says that sent unchanged, its turn's end is receipted (handled, or deferred if interrupted) and its last message reported back to the sender while codex-alab acts under its claim (or that aterm has no route back to it) (inbox finding 9; review of fea034d20, finding 9). A mail turn, like codex-alab's own plan-yes turn, takes no `/ide` context, aterm's or an IDE's, even while aterm can give none: a prefix would cost it its receipt (inbox finding 12). At that turn's end (sent while codex-alab acts; owed, and sent once it acts again, when it does not, §2): `inbox seen <id> handled` (`deferred` after an interrupt or an error), then `post to=<reply address> kind=report re=<off> len=<n>` whose body is the `seq=<n> hash=<h>` stamp line (the screen's when the report is sent) and the turn's last agent message (`drive supervise --mail` folds it). The reply address is the one aterm's bridge routes: a session sender (`from=s-<sid>@n-<node>`, as rows print it) as `@s-<sid>@n-<node>`, a bare principal (`h-…`) as it is; a sender it cannot route gets no report (the local `post` would answer `OK` and the bridge retire it). `inbox seen <id>` is "done up to `<id>`" (the watermark moves to the larger), so a receipt never passes an older `task` or `ask` nobody has handled: it is owed, `mail: the receipt for @B waits behind the unhandled task @A` is raised, and it goes, in id order, once that row is handled (its own turn's end, or anyone's `inbox seen`); the report goes at once. A row this process handled, or any at or below the watermark, is never read again (a line recalled from history runs nothing and reports nothing twice), unless this process's last word on it was `deferred`. A status-line badge (`✉ 1 task`) counts rows above the handled watermark, refreshed on `subscribe @self events,mail` frames on a task and a connection of its own (never the claim's: an unanswered exchange closes the connection it ran on): display only, nothing typed, nothing woken. No proven claim, no row, another kind, already handled, a timeout or any error: the line goes as typed (Amended 2026-10-17 after the review of 22ee0e75a: reports to a session went to `to=s-…@n-…`, which the bridge retires; a receipt advanced the watermark past older unhandled mail; a recalled line ran its mail again; the badge read rode the claim's connection) | law 2 (§0), law 4 |
| visibility | each act → `@self meta set harness_act <act>` (aterm change: proven holder only; on the session timeline as `harness-act`, `status last_act=`), `@self story <verb> [<text>]` on the presence band as `drive watch` tells it (`alab-autopilot/src/story.rs` maps each act, wait and escalation onto aterm's closed verb set) and `appnotice harness …`; waits → a TUI warning line ("waiting for the usage reset at 14:05"); what the root thread is doing → `@self meta set description goal: <objective>[ · now: <plan step>]` or `plan: <step>` (the goal unless complete, `update_plan`'s step in progress; ≤1024 B, one write per change, on a connection of its own, never the title). It is withdrawn (`meta unset description`, only while it is still the text written) when there is nothing left to say, when the claim goes or codex-alab goes on standby (§2), and at the exit, so no restore carries a stale goal. Descriptions are written only while codex-alab acts, and `harness_act` records while its claim is held (§2). A standing text is codex-alab's only when it is exactly the one this process wrote, or, before it wrote any, when this thread's dead predecessor left a record (a crash, an aterm restore) and the text has the `goal: `/`plan: ` shape; any other text is a person's and stands (aterm has no compare-and-set: a person's text set between the read and the write is the one race left) | law 5 |

## 6. In-band status (aterm; helps vendor codex)

Upstream codex writes `ESC ] 7501 ; state=<idle|working|blocked|done|error> : app=codex [: kind=…] ESC \` and
`state=clear`. aterm stores it in its own field, shown as `status program_state=<state>[:<kind>]`, on the band and
in presence, and NEVER in `agent=`, `agent_since_ms=`, `phase_authoritative`, the zone hash an act reads, or any
`await`. Honoured only while the program in front names `codex`, or is a runtime that may host an agent (`node`,
`bun`: npm's `codex`) and the report names `app=codex` (aterm `aterm-gui/src/program_state.rs::honoured_for`,
66eb1f2635); cleared on `state=clear`, RIS, a foreground-group change and the program's exit. The disk tick also takes
a session whose 7501 report is shown (a `node`-run vendor `codex`, which no screen names; aterm
`harness_host::CODEX_REPORTS`, told by the status sweep, b97d43b87d). Display only, as before.

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
in `~/.codex/config.toml` or `-c`, wins); otherwise not one byte. codex-alab's `--strict-config` takes the key (one
`ALAB(progress)` line in `config/src/strict_config.rs`); vendor codex's refuses it, as an editor checking upstream's
schema flags it, so it does not belong in a config vendor codex reads strictly, such as a checked-in project config.
Display only: no act, no claim and no `await` reads it.

## 7. Versions

codex-alab's version scheme is the owner's (`pub policy`; alab/README decisions 2-3; never invented). aterm's
`Version::parse` is unchanged. aterm has no Codex upgrade lane (retired, aterm fc74be92fe, retire C3): aterm moves no
Codex, and codex-alab upgrades itself (§5 live upgrade).
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
  - at the claim's release while the process still runs.
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
- PASSIVE RELEASE DROPS THE COPY: when the claim is given back while the process still runs and
  the record (read at that release) shows no end coming (standing with `exit:null` and a status
  other than `exited` — `upgrading` included), aterm drops its copy. From then until the next
  proven claim the session's exit is decided by nobody, and nothing relaunches it (fail closed).
  An ALab image in front keeps the watch-only worker, which watches only: no stall or memory
  remedy runs without a standing proven claim (§4). A release that shows the end (record
  removed, an `exit` word, `status:"exited"`) keeps the copy for the exit. A re-exec upgrade
  therefore re-proves in the new image to get a copy again; until it does, a crash of the new
  image is decided by nobody, and nothing relaunches it (fail closed). (codex-alab 2026-10-17,
  decided again, §2: a transient off (a hold, a suspension, a failed policy read) keeps the
  claim and so the copy; a person's or the config's off releases it and drops the copy, where
  aterm's host runs no worker and relaunches nothing anyway.)
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
  watches only: no stall or memory remedy runs without a standing proven claim (§4). Otherwise the copy is re-read at the next proven claim. (Amended 2026-10-16,
  aterm main 401229b3de, 23e60f1750, 20965dbd26, ce767c6ae4.)

## aterm side as implemented, Rest (§0.1, §3, §5, §6, §7, §8; branch native-harness-20261016)

- `harness policy` (§3): `OK gen=<n> effective=<on|off> why=<word> <every texts() key>=<pct|-> … questions=<word>`.
  Keys are all 34 of `SupervisorConfig::texts()` in `KEYS` order (not only the ones codex-alab honours), then
  `questions=` last. `-` means empty. `why=` words, first refusal first: `unhosted`, `disabled`, `headless`,
  `agent-off` (the session's agent's `[harness.<agent>]` table; Codex when the server names none), `suspended`,
  `hold`, `autopilot-off` (ADDED: the session's or the table's Autopilot `off` is the harness's hands off, so it is
  an off for codex-alab too); `enabled` when on. Only `continue_text=` is composed with the rules file
  (`continue_text (standing rules: <one line>)`, exactly the loop's); `answer_text=` is raw, as the contract says.
  An edge token gets exactly `OK effective=<on|off>` (no gen/why). Any other form: `ERR usage: harness policy`.
  The event is `EVENT <local> harness-policy gen=<n>` (timeline kind `harness-policy`), recorded on every session
  only when the policy cell adopts a newer stamp; a hold, an Autopilot word or a suspension moving `effective=`
  raises no `harness-policy` event (a hold and the words have their own `hold`/`meta` events).
- `meta set harness_act <act>` (§5): `<act>` is one owner-key token. Allowed on the claim's bound connection, or on a
  direct connection the kernel proves is the claim's pid again (peer pid + foreground leader). Else `ERR denied`.
  `unset harness_act`, no act, two words or a bad token: `ERR usage: meta set harness_act <act>`. Recorded as
  timeline `harness-act` `act=<act> holder=<holder>` (not pushed on `events`), `status last_act=<act|->`. It counts
  as a meta change, so the record copy is re-read with every act.
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

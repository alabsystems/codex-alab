#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Wall several interactive codex-alab sessions of one shared pool at once, and prove that
the followers move once the first session's report proved the destination.

Each session is a real codex-alab TUI under its own native host fixture (aterm
native_renewal_fixture.rs), and every host shares one pool (`shared_pool_root`): one
roster, one state and ledger, one receipt store. The loopback provider answers each
session's first model request and holds its second at a barrier until every session has
sent it; then it refuses them all at once on the source account, whose week reads spent
from then on, while the destination account is never spent. Exactly one session (the
canary) is charged a rotation and moved. The others are turned away while it moves: each
asks the host again every 5 s, and the host either tells it to wait (`prepare` answered
`waiting`) or answers its `prepare` ready and then retires that receipt at its `commit`:
while the destination is unproven `commit_pool` refuses the selection, and the receipt is
retired (aterm codex_renewal_control.rs `commit`, codex_rotation_control.rs `commit_pool`).
Neither answer names its cause. The host answers `waiting` too while a follower's own read
of the accounts is pending, after the pool was proven as well, and `commit_pool` refuses a
selection that lapsed or changed, not only one whose destination is unproven. The canary's successor
resumes its walled turn on the destination and, once that turn was served, consumes its
receipt and reports the turn (`codex-rotation complete`); the host proves the destination
for the pool, and each follower's commit is taken at its next ask.

The run (`follow`) passes when one rotation was charged, the canary's report was answered
`completed`, every follower was turned away after the canary was answered ready and before
its report, and had its first commit taken only after the report, within FOLLOW_SECONDS
of it (not at the canary hold of CANARY_HOLD_SECONDS), no session took the canary, and the
pool ends proven with no canary held. Each session's turn is resumed with its walled input
unchanged, on the destination, its own report is answered completed, and its successor
comes to rest. That a follower was turned away before the report shows it asked then and
was not moved, not what turned it away; what ties its move to the report is that it came
after the report and within FOLLOW_SECONDS of it, and that the control below, with the
report alone withheld, moves none.

`--control` withholds the report and nothing else, with switches the host fixture already
has and a lock a real process can hold. Every host's first consume ack is dropped and
paused (`drop-consume-ack`, `pause-consume-ack`). The canary's successor sends that consume
after its resumed turn was served on the destination, so the turn is served with nothing
held. When the canary's host has traced the dropped ack, the controller takes the
destination login's custody lock (`auth.json.lock`, aterm's AuthLock, an exclusive flock)
and lets the ack go: codex-alab reads the receipt's status (consumed: a read that takes no
login lock), sends consume again and reports the turn, and the host, which must take that
lock to record the proof, answers `waiting` until the report's budget (5 s) is spent. The
lock covers the consume's retry and the reports, not the turn. It is then let go, and the
controller watches the followers for CONTROL_WATCH_SECONDS with nothing held: no
follower's commit may be taken, the pool must stay unproven and its canary hold unlapsed,
and the followers go on asking and are only ever turned away. From the canary's ready to
the watch's end, every `prepare`, `commit` and `status` of a follower is answered
`waiting`, `ready` or `retired`: a follower refused by name, by an after-effect of the lock
or for any other cause, fails the control, which would otherwise count a refusal as a
wait. That the same layout with only the report withheld moves no follower is what shows
the `follow` verdict discriminates. `--lost-consume-ack` drops and pauses the same ack and
takes no lock: it must pass the `follow` verdict, so the control differs from a passing
run by the lock alone.

Every account, token and reply is this run's own, under its output directory, served from
loopback. A trial host or package qualifies nothing: the report says what each is worth.
"""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import threading
import time

import capacity_exec as probe
import native_host_record
import package_standing
import provider_fixture
import renewal_follow as renewal

# The fewest sessions a follow is shown with: a canary and at least two followers.
MIN_SESSIONS = 3
MAX_SESSIONS = 6
# How far apart the provider's refusals of the sessions' walled requests may be.
WALL_SPREAD_US = 1_000_000
# How soon after the canary's report was answered completed each follower must have had
# its commit taken. A follower asks again every 5 s while it is held (alab-autopilot
# renewal_follow.rs, point_recheck); without the report it waits the canary hold out.
FOLLOW_SECONDS = 60
# aterm accounts.rs CANARY_HOLD_S: how long followers wait behind a canary that never
# proves the destination before the next of them takes the canary in its place.
CANARY_HOLD_SECONDS = 600
# How long the barrier holds a session's walled request for the others.
BARRIER_SECONDS = 120
# codex-alab's report budget (alab-autopilot renewal_completion.rs, COMPLETION_BUDGET): how
# long it asks again while the host answers `waiting`.
REPORT_BUDGET_SECONDS = 5
# How long the control holds the custody lock past that budget, and how long no report
# may have been sent before the lock is let go.
REPORT_SETTLE_SECONDS = 1.5
# How long the control holds the lock with no report sent before it lets go; a control
# that saw no report while it held the lock fails by name.
HOLD_CAP_SECONDS = 60
# How long the control watches the followers with nothing held, by default, and at most:
# well short of the canary hold, so that every follower that waits is held by the canary
# and not by a lapse.
CONTROL_WATCH_SECONDS = 120
CONTROL_WATCH_MAX_SECONDS = 420
# Each follower must still be asking, and still be turned away, this close to the
# watch's end.
QUIET_TAIL_SECONDS = 15
# (drive, guardian) seconds of a mode; the control adds its watch to both.
BUDGETS = {"follow": (270, 300), "lost-consume-ack": (270, 300), "control": (150, 180)}


def prompt(index):
    """Session `index`'s original prompt: what attributes each model request to it."""
    return f"{probe.PROMPT} [pool session {index}]"


def user_texts(body):
    """The text of each user message part of a model request body."""
    return [part.get("text") for item in body.get("input", [])
            if isinstance(item, dict) and item.get("type") == "message" and item.get("role") == "user"
            for part in item.get("content", []) if isinstance(part, dict)]


class Pool(renewal.Model):
    """The loopback provider of one shared pool: source account A, destination B.

    Each session's first request on A is answered (the plan call). Its second waits at
    the barrier until every session has sent its own; then A's week is spent and they
    are all refused at once (`refusedUs`), and A refuses every later request. B is never
    spent and answers each session's resumed turn. Title requests are answered for
    either. A request no session's prompt attributes is unknown and fails the run.
    """

    def __init__(self, root, count):
        super().__init__(root, weekly_quota=True)
        self.prompts = [prompt(index) for index in range(count)]
        self.by_session = [[] for _ in range(count)]
        self.arrived = set()
        self.barrier = threading.Condition()
        # Monotonic microseconds the provider refused each session's walled request.
        self.refused_us = {}

    def week_spent(self, old_token, new_token, next_token=False):
        # A's week is spent from the barrier on; B's never is.
        return bool(old_token and self.source_walled)

    def usage(self, old_token, new_token, next_token=False):
        probe.require((old_token or new_token) and not next_token, "unexpected usage identity")
        with self.lock:
            self.usage_reads.append("source" if old_token else "target")
        return self.usage_body("old" if old_token else "new")

    def reply(self, body, generation=None):
        kind = renewal.request_kind(body)
        record = {"at": time.monotonic(), "unixAt": time.time(), "monotonicUs": renewal.monotonic_us(),
                  "kind": kind, "generation": generation, "body": body}
        if kind == "thread-title":
            with self.lock:
                self.auxiliary_requests.append(record)
            item = {"type": "message", "role": "assistant", "id": "fixture-title",
                    "content": [{"type": "output_text", "text": json.dumps({"title": "Recover pool work"})}]}
            self.answered(record, generation)
            record["status"] = 200
            return 200, probe.response("fixture-title", item), "text/event-stream", None, record
        matched = [index for index, text in enumerate(self.prompts)
                   if kind == "foreground" and user_texts(body).count(text) == 1]
        if len(matched) != 1:
            with self.lock:
                self.unknown_requests.append(record)
            return 400, b'{"error":"unrecognized fixture request"}', "application/json", None, record
        index = matched[0]
        with self.lock:
            record["session"] = index
            self.requests.append(record)
            self.by_session[index].append(record)
            number = len(self.by_session[index])
        record["number"] = number
        if generation == "old" and number == 2:
            with self.barrier:
                self.arrived.add(index)
                self.barrier.notify_all()
                met = self.barrier.wait_for(lambda: len(self.arrived) == len(self.prompts), BARRIER_SECONDS)
                if met:
                    with self.lock:
                        self.source_walled = True
            if not met:
                # Seen by the controller's loop (require_no_fixture_fault).
                with self.lock:
                    self.fixture_faults.append({"path": "/responses", "error":
                        f"BarrierFault: {len(self.arrived)} of {len(self.prompts)} sessions sent "
                        f"their walled request within {BARRIER_SECONDS} s"})
                record["status"] = 500
                return 500, b'{"error":"fixture barrier"}', "application/json", None, record
        if generation == "old" and number >= 2:
            with self.lock:
                self.refused_us.setdefault(index, renewal.monotonic_us())
            record["status"] = 429
            return self.refused(record, "old", "Shared fixture weekly quota exhausted")
        if generation == "old":
            item = {"type": "function_call", "call_id": probe.CALL_ID, "name": "update_plan",
                    "arguments": json.dumps({"plan": [{"step": probe.PLAN, "status": "completed"}]})}
        elif generation == "new":
            item = {"type": "message", "role": "assistant", "id": f"pool-answer-{index}",
                    "content": [{"type": "output_text", "text": f"Shared pool session {index} recovered."}]}
        else:
            record["status"] = 401
            return self.unauthorized(record)
        self.answered(record, generation)
        record["status"] = 200
        return 200, probe.response(f"pool-{index}-{number}", item), "text/event-stream", None, record


def first_ready(calls):
    """The stamp of a host's first `codex-rotation prepare` answered ready, or None."""
    return next((stamp for verb, reply, stamp in calls
                 if verb == "prepare" and renewal.reply_state(reply) == "ready"), None)


def unmoved(reply):
    """Whether a `codex-rotation` answer leaves its session neither moved nor refused: a
    bare `waiting`, a published receipt (`ready`) or a retired one.

    A `waiting` that names a receipt (`id=`) is a receipt being committed (aterm
    codex_renewal.rs `Receipt::reply`, `Committing`): the session is moving. A bare
    `OK codex-rotation state=waiting` names none. aterm answers it while the host cannot
    serve the ask yet, and to a `commit` when `commit_pool` refuses the selection and the
    receipt cannot be retired (codex_renewal_control.rs `commit`, codex_rotation_control.rs
    `retire_ready`), which takes nothing.
    """
    state = renewal.reply_state(reply)
    return state in ("ready", "retired") or (state == "waiting" and "id" not in renewal.reply_fields(reply))


def commit_outcome(reply):
    """What a `codex-rotation commit` answer did: "refused" (no `OK`), "turned away"
    (unmoved) or "taken": `waiting` with the receipt's id while the host restarts the
    session, or `committed`."""
    if renewal.reply_state(reply) is None:
        return "refused"
    return "turned away" if unmoved(reply) else "taken"


def first_taken(calls):
    """The first `codex-rotation commit` a host took (commit_outcome), as (reply, stamp), or
    None."""
    return next(((reply, stamp) for verb, reply, stamp in calls
                 if verb == "commit" and commit_outcome(reply) == "taken"), None)


def turned_away(calls, after_us, before_us):
    """The stamps of a host's asks it turned away between `after_us` and `before_us`: a
    `prepare` answered a bare `waiting`, or a `commit` turned away (commit_outcome).

    That shows the session asked and was not moved, not what held it: the host answers a
    `prepare` `waiting` while the session's own read of the accounts is pending as well
    as while a canary holds the pool.
    """
    return [stamp for verb, reply, stamp in calls if after_us < stamp < before_us
            and ((verb == "prepare" and renewal.reply_state(reply) == "waiting" and unmoved(reply))
                 or (verb == "commit" and commit_outcome(reply) == "turned away"))]


# The verbs a session neither moved nor refused sends: it asks, commits what it was offered,
# and reads that receipt's status.
UNMOVED_VERBS = ("prepare", "commit", "status")


def unmoved_answers(calls, after_us, until_us):
    """A session's calls after `after_us` and up to `until_us`, split into those answered
    as a session neither moved nor refused (an UNMOVED_VERBS verb, answered unmoved), as
    counts of "<verb> <state>", and every other, as "<verb> <reply>": a refusal, a receipt
    committed, consumed or completed, or another verb."""
    counts, others = {}, []
    for verb, reply, stamp in calls:
        if not after_us < stamp <= until_us:
            continue
        if verb in UNMOVED_VERBS and unmoved(reply):
            key = f"{verb} {renewal.reply_state(reply)}"
            counts[key] = counts.get(key, 0) + 1
        else:
            others.append(f"{verb} {reply[:60]}")
    return dict(sorted(counts.items())), others


def charged_rotations(ledger):
    """The pool's charged rotations: `rotate` rows that are no canary's taking."""
    return [event for event in ledger if event.get("kind") == "rotate"
            and not str(event.get("why") or "").startswith("canary:")]


def canary_takings(ledger):
    """The rows of a session that took the canary in place of one that never proved it."""
    return [event for event in ledger if event.get("kind") == "rotate"
            and str(event.get("why") or "").startswith("canary:")]


def walls_verdict(sessions, spread_us=WALL_SPREAD_US):
    """Every session was refused on the shared account, all within `spread_us`."""
    walls = [session.get("refusedUs") for session in sessions]
    missing = [index for index, wall in enumerate(walls) if type(wall) is not int]
    probe.require(not missing, "session(s) " + ", ".join(map(str, missing))
                  + " were never refused on the shared account")
    spread = max(walls) - min(walls)
    probe.require(spread <= spread_us, f"the sessions were walled {spread / 1000:.0f} ms apart, "
                  f"not within {spread_us / 1000:.0f} ms")
    return {"firstUs": min(walls), "spreadMs": round(spread / 1000, 3)}


def single_charge(sessions, ledger, state, canary):
    """One rotation was charged, to the canary, onto the pool's active account."""
    charged = charged_rotations(ledger)
    probe.require(len(charged) == 1 and len(state.get("rotations") or []) == 1,
        f"{len(charged)} rotations were charged ({len(state.get('rotations') or [])} on record), not one")
    probe.require(charged[0].get("sid") == sessions[canary]["sid"],
        f"the charged rotation was session {charged[0].get('sid')}'s, not the canary's "
        f"(session {canary}, the first whose commit was taken)")
    probe.require(charged[0].get("to") == state.get("active"),
        "the charged rotation did not move the pool onto its active account")
    return charged[0]


def pool_verdict(sessions, ledger, state, follow_seconds=FOLLOW_SECONDS, spread_us=WALL_SPREAD_US):
    """The followers moved after the canary's report, not at its hold's lapse.

    `sessions` are, per session: its host `sid`, its host's traced `codex-rotation`
    `calls` (renewal_follow.rotation_calls), when the provider refused its walled request
    (`refusedUs`, the hosts' clock) and its consumed `receipt` (renewal_follow
    consumed_receipt, with its `sid`). `ledger` and `state` are the shared pool's. The
    canary is the session whose commit was taken first, and the one charged. Its report is
    answered completed (renewal_follow.completion_verdict, with the pool proven once and
    no canary taken). Each follower was turned away (turned_away) after the canary was
    answered ready and before its report; its first commit taken came after the report,
    within `follow_seconds`, and was its consumed receipt's; and its own successor's report
    was answered completed too.

    Being turned away shows a follower asked in that window and was not moved; it does not
    show what held it, since a `prepare` is answered `waiting` while a follower's own read
    is pending too. What ties each follower's move to the report is the order: taken only
    after it, and within `follow_seconds` of it.
    """
    probe.require(len(sessions) >= MIN_SESSIONS, f"a follow is shown with {MIN_SESSIONS} sessions at least")
    walls = walls_verdict(sessions, spread_us)
    taken = [first_taken(session["calls"]) for session in sessions]
    untaken = [index for index, commit in enumerate(taken) if commit is None]
    probe.require(not untaken, "session(s) " + ", ".join(map(str, untaken)) + " never had a commit taken")
    canary = min(range(len(sessions)), key=lambda index: taken[index][1])
    canary_ready = first_ready(sessions[canary]["calls"])
    takings = canary_takings(ledger)
    probe.require(not takings, "a session took the canary: " + str(takings[0].get("why") if takings else ""))
    charge = single_charge(sessions, ledger, state, canary)
    probe.require(state.get("active") == charge.get("to") and state.get("proven") is True
                  and state.get("canary") is None, "the pool did not end proven with no canary held")
    report = renewal.completion_verdict(sessions[canary]["calls"], sessions[canary]["receipt"], state, ledger)
    completed = report["completedUs"]
    followers = []
    for index, session in enumerate(sessions):
        if index == canary:
            continue
        held = turned_away(session["calls"], canary_ready, completed)
        probe.require(bool(held), f"follower {index} was never turned away between the canary's ready and "
                      "its report: nothing shows it asked then and was not moved")
        reply, stamp = taken[index]
        probe.require(stamp > completed, f"follower {index}'s commit was taken "
                      f"{(completed - stamp) / 1_000_000:.3f} s before the canary's report was answered "
                      "completed")
        lag = (stamp - completed) / 1_000_000
        probe.require(lag <= follow_seconds, f"follower {index}'s commit was taken {lag:.1f} s after the "
                      f"canary's report, not within {follow_seconds} s: it waited out the canary hold")
        probe.require(renewal.reply_fields(reply).get("id") == session["receipt"]["id"],
                      f"follower {index}'s first commit taken is not its consumed receipt's")
        own = renewal.completion_verdict(session["calls"], session["receipt"])
        followers.append({"session": index, "turnedAway": len(held),
                          "commitsRetired": sum(verb == "commit" and renewal.reply_state(answer) == "retired"
                                                for verb, answer, _ in session["calls"]),
                          "commitsRetiredBeforeReport": sum(
                              verb == "commit" and renewal.reply_state(answer) == "retired"
                              and canary_ready < stamp < completed for verb, answer, stamp in session["calls"]),
                          "takenSecondsAfterReport": round(lag, 3),
                          "takenSecondsAfterWall": round((stamp - session["refusedUs"]) / 1_000_000, 3),
                          "ownReportSecondsAfterLastConsume": own["secondsAfterLastConsume"]})
    receipts = [session["receipt"] for session in sessions]
    probe.require(len({session["sid"] for session in sessions}) == len(sessions)
                  and all(receipt.get("sid") == session["sid"] for receipt, session in zip(receipts, sessions)),
                  "a consumed receipt is not its own session's")
    probe.require(len({receipt["intent"]["thread"] for receipt in receipts}) == len(receipts)
                  and len({receipt["id"] for receipt in receipts}) == len(receipts),
                  "two sessions share a thread or a receipt")
    probe.require(all(receipt["intent"].get("purpose") == "weekly_quota" for receipt in receipts)
                  and len({(receipt["old_auth"], receipt["new_auth"]) for receipt in receipts}) == 1
                  and receipts[0]["old_auth"] != receipts[0]["new_auth"],
                  "the sessions were not all moved from the one walled login to the one destination")
    return {"canary": canary, "walls": walls,
            "canaryReadySecondsAfterWall": round((canary_ready - sessions[canary]["refusedUs"]) / 1_000_000, 3),
            "canaryCommitSecondsAfterWall": round((taken[canary][1] - sessions[canary]["refusedUs"]) / 1_000_000, 3),
            "canaryReport": report,
            "reportSecondsAfterWall": round((completed - sessions[canary]["refusedUs"]) / 1_000_000, 3),
            "followers": followers,
            "slowestFollowerSecondsAfterReport": max(item["takenSecondsAfterReport"] for item in followers),
            "chargedRotations": 1, "canaryTaken": False, "provenAccount": state["active"]}


def control_verdict(sessions, ledger, state, held, watch_end_us, now_unix,
                    hold_seconds=CANARY_HOLD_SECONDS, quiet_tail_seconds=QUIET_TAIL_SECONDS):
    """With the canary's report withheld, every follower was held toward the canary hold.

    `sessions` are as pool_verdict's, without receipts. `held` is when the controller
    held the destination's custody lock (`fromUs`, `untilUs`, the hosts' clock),
    `watch_end_us` when it stopped watching, and `now_unix` the wall clock then. The
    canary (the one session whose commit was taken) reported its resumed turn, and every
    report was answered `waiting`, all while the lock was held. No follower's commit was
    taken, and from the canary's ready to the watch's end every call of a follower was a
    `prepare`, `commit` or `status` answered as a session neither moved nor refused
    (unmoved_answers): a follower refused, during the lock or after it, is not shown held.
    Each went on asking after the lock was let go and was turned away up to the watch's
    last `quiet_tail_seconds`. One rotation was charged, no session took the canary, the
    pool was never proven, and the canary hold has not lapsed.
    """
    probe.require(len(sessions) >= MIN_SESSIONS, f"a control is shown with {MIN_SESSIONS} sessions at least")
    walls = walls_verdict(sessions)
    taken = [index for index, session in enumerate(sessions) if first_taken(session["calls"]) is not None]
    probe.require(len(taken) == 1, f"{len(taken)} sessions had a commit taken with the report withheld, "
                  "not the canary alone: " + ", ".join(map(str, taken)))
    canary = taken[0]
    canary_ready = first_ready(sessions[canary]["calls"])
    reports = [(reply, stamp) for verb, reply, stamp in sessions[canary]["calls"] if verb == "complete"]
    probe.require(bool(reports), "the canary sent no report while the custody lock was held, so "
                  "nothing was withheld")
    probe.require(all(reply == renewal.WAITING_REPLY for reply, _ in reports),
                  "a withheld report was answered otherwise than waiting: "
                  + ", ".join(sorted({reply[:60] for reply, _ in reports if reply != renewal.WAITING_REPLY})))
    probe.require(all(held["fromUs"] <= stamp <= held["untilUs"] for _, stamp in reports),
                  "the canary reported while the custody lock was not held")
    takings = canary_takings(ledger)
    probe.require(not takings, "a session took the canary: " + str(takings[0].get("why") if takings else ""))
    charge = single_charge(sessions, ledger, state, canary)
    proven = [event for event in ledger if event.get("kind") == "proven"]
    probe.require(not proven and state.get("proven") is False,
                  "the pool was proven with the canary's report withheld")
    probe.require(isinstance(state.get("canary"), dict) and state["canary"].get("sid") == sessions[canary]["sid"],
                  "the canary hold is not the canary's")
    remaining = int(charge["t"]) + hold_seconds - now_unix
    probe.require(remaining > 0, "the canary hold lapsed during the watch: the followers' wait proves "
                  "nothing of the report")
    followers = []
    for index, session in enumerate(sessions):
        if index == canary:
            continue
        answers, others = unmoved_answers(session["calls"], canary_ready, watch_end_us)
        probe.require(not others, f"follower {index} was refused or moved after the canary's ready, not only "
                      "turned away, so its wait is not shown to be the canary's: " + "; ".join(sorted(set(others))))
        away = turned_away(session["calls"], held["untilUs"], watch_end_us + 1)
        probe.require(bool(away), f"follower {index} was never turned away after the lock was let go")
        quiet = (watch_end_us - away[-1]) / 1_000_000
        probe.require(quiet <= quiet_tail_seconds, f"follower {index} was last turned away {quiet:.1f} s "
                      f"before the watch ended, not within {quiet_tail_seconds} s: it stopped asking")
        followers.append({"session": index, "turnedAwayAfterRelease": len(away),
                          "turnedAwayInAll": len(turned_away(session["calls"], canary_ready, watch_end_us + 1)),
                          "answersAfterCanaryReady": answers,
                          "lastTurnedAwaySecondsBeforeWatchEnd": round(quiet, 3)})
    return {"canary": canary, "walls": walls,
            "canaryReadySecondsAfterWall": round((canary_ready - sessions[canary]["refusedUs"]) / 1_000_000, 3),
            "lockHeldSeconds": round((held["untilUs"] - held["fromUs"]) / 1_000_000, 3),
            "reportsWhileHeld": len(reports),
            "reportSpanSeconds": round((reports[-1][1] - reports[0][1]) / 1_000_000, 3),
            "watchedSecondsAfterRelease": round((watch_end_us - held["untilUs"]) / 1_000_000, 3),
            "followersHeldSecondsAfterCanaryReady": round((watch_end_us - canary_ready) / 1_000_000, 3),
            "canaryHoldRemainingSeconds": round(remaining, 3), "followers": followers,
            "chargedRotations": 1, "canaryTaken": False, "poolProven": False}


def requests_verdict(requests, index, moved):
    """Session `index`'s model requests: answered on A, refused on A, and (`moved`) its
    walled input resumed unchanged on B, with its completed plan call kept once."""
    shape = [(request.get("generation"), request.get("status")) for request in requests]
    expected = [("old", 200), ("old", 429)] + ([("new", 200)] if moved else [])
    probe.require(shape == expected, f"session {index}'s model requests were {shape}, not {expected}")
    probe.require(all(user_texts(request["body"]).count(prompt(index)) == 1 for request in requests),
                  f"session {index}'s original input was lost or duplicated")
    if not moved:
        return {"session": index, "requests": len(requests)}
    probe.require(requests[2]["body"]["input"] == requests[1]["body"]["input"],
                  f"session {index}'s resumed turn changed its walled input")
    outputs = [item for item in requests[2]["body"]["input"] if item.get("type") == "function_call_output"]
    probe.require(len(outputs) == 1 and outputs[0].get("call_id") == probe.CALL_ID
                  and "Plan updated" in str(outputs[0].get("output")),
                  f"session {index}'s completed plan output was lost or duplicated")
    return {"session": index, "requests": len(requests), "walledInputResumed": True}


def pool_receipts(pool):
    """The shared store's receipts, as written."""
    found = []
    for path in sorted((pool / "renewal-follow").glob("*.json")):
        if not re.fullmatch(r"[0-9a-f]{32}", path.stem):
            continue
        try:
            value = json.loads(path.read_text())
        except (OSError, ValueError):
            continue
        if isinstance(value, dict):
            found.append(value)
    return found


def take_custody(path):
    """Hold a login's custody lock as aterm's AuthLock does (an exclusive flock on
    `auth.json.lock`), or None while another holder has it."""
    descriptor = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    try:
        fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        os.close(descriptor)
        return None
    return descriptor


class Session:
    """One host and its session, as the loop follows it."""

    def __init__(self, index, root):
        self.index = index
        self.root = root
        self.process = None
        self.env = None
        self.metadata = None
        self.rest_at = None
        self.receipt = None
        self.successor = None

    @property
    def trace(self):
        return self.root / "renewal-control-trace.jsonl"

    def read_metadata(self):
        """The host's first stdout line, once written."""
        try:
            data = (self.root / "host-stdout.jsonl").read_bytes()
        except OSError:
            return
        probe.require(len(data) <= 1024 * 1024, "native host metadata oversized")
        if b"\n" not in data:
            return
        metadata = json.loads(data.split(b"\n", 1)[0])
        probe.require(metadata.get("host_pid") == self.process.pid,
                      f"host {self.index}'s metadata did not name the owned child")
        probe.require(isinstance(metadata.get("sid"), str)
                      and re.fullmatch(r"[a-zA-Z0-9_-]{1,128}", metadata["sid"]),
                      f"host {self.index}'s native session ID is malformed")
        self.metadata = metadata


def main():
    started = time.monotonic()
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--host", required=True, type=Path)
    parser.add_argument("--package", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--sessions", type=int, default=MIN_SESSIONS)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--control", action="store_true",
                      help="Withhold the canary's report and watch the followers wait")
    mode.add_argument("--lost-consume-ack", action="store_true",
                      help="Drop and pause the canary's consume ack, as the control does, and hold no lock")
    parser.add_argument("--control-watch", type=int, default=CONTROL_WATCH_SECONDS,
                        help="Seconds the control watches the followers with nothing held")
    parser.add_argument("--under-guardian", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if not MIN_SESSIONS <= args.sessions <= MAX_SESSIONS:
        parser.error(f"--sessions is {MIN_SESSIONS} to {MAX_SESSIONS}")
    if not 30 <= args.control_watch <= CONTROL_WATCH_MAX_SECONDS:
        parser.error(f"--control-watch is 30 to {CONTROL_WATCH_MAX_SECONDS} s, short of the canary hold")
    name = "control" if args.control else "lost-consume-ack" if args.lost_consume_ack else "follow"
    drive_seconds, guardian_seconds = BUDGETS[name]
    if args.control:
        drive_seconds += args.control_watch
        guardian_seconds += args.control_watch
    host = args.host.resolve(strict=True)
    host_record = native_host_record.load(host)
    native_host_record.require_aterm_name(host)
    package = args.package.resolve(strict=True)
    build, manifest, codex = probe.package_records(package)
    root = args.output.resolve()
    if not args.under_guardian:
        # The controller owns and reaps each host; a surrounding guardian owns the whole run,
        # PTY holders included.
        guard_result = root.with_name(root.name + "-guardian-result.json")
        command = [sys.executable, "-I", str(probe.GUARD_PATH), "--",
            sys.executable, "-I", str(probe.GUARD_PATH), "--result-file", str(guard_result),
            "--inherited-group-timeout", str(guardian_seconds), "--", sys.executable, str(Path(__file__).resolve()),
            "--host", str(host), "--package", str(package), "--output", str(root),
            "--sessions", str(args.sessions), "--control-watch", str(args.control_watch), "--under-guardian"]
        if args.control:
            command.append("--control")
        if args.lost_consume_ack:
            command.append("--lost-consume-ack")
        completed = subprocess.run(command)
        result_path = root / "result.json"
        if result_path.exists() and guard_result.exists():
            result = json.loads(result_path.read_text())
            result["guardian"] = json.loads(guard_result.read_text())
            result["passed"] = bool(result.get("passed") and completed.returncode == 0
                and result["guardian"].get("returncode") == 0 and not result["guardian"].get("timed_out"))
            result_path.write_text(json.dumps(result, indent=2) + "\n")
        return completed.returncode
    root.mkdir(parents=True, exist_ok=False)
    probe.require(len(str(root / "w0" / "control.sock")) < 103, "the output path is too long for a host's control socket")
    common = probe.fresh_environment(root)
    common.update(ATERM_HARNESS_STATE=str(root / "host-state"), XDG_CONFIG_HOME=str(root / "config"))
    pool = root / "host-state" / "accounts-codex"
    model = Pool(root, args.sessions)
    server, server_thread = renewal.serve(model)
    host_summary = native_host_record.summary(host_record)
    package_summary = package_standing.summary(build)
    report = {"passed": False, "mode": name,
              "scope": "isolated native hosts and real Codex PTYs over one shared account pool",
              "source": build["source"], "version": manifest["version"], "sessions": args.sessions,
              "observationBudgetSeconds": {"drive": drive_seconds, "guardian": guardian_seconds},
              "nativeHost": host_summary, "package": package_summary,
              "packageRecords": {"build": build, "manifest": manifest},
              # A trial host or package qualifies nothing, whatever `passed` says of the run.
              "qualifies": bool(host_summary["qualifies"] and package_summary["qualifies"]),
              "controllerSha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              "fixtureSha256": {Path(module.__file__).name: hashlib.sha256(
                  Path(module.__file__).read_bytes()).hexdigest()
                  for module in (renewal, provider_fixture, native_host_record, package_standing)},
              "sharedPool": str(root / "host-state")}
    if args.control:
        report["controlWatchSeconds"] = args.control_watch
    sessions = [Session(index, root / f"w{index}") for index in range(args.sessions)]
    held = {}
    custody = None
    try:
        port = server.server_port
        issuer = f"https://127.0.0.1:{port}"
        old, new = root / "auth-old" / "auth.json", root / "auth-new" / "auth.json"
        renewal.fake_auth(old, "old", issuer, "source-account")
        renewal.fake_auth(new, "new", issuer, "target-account", email="target@example.invalid", user="target-user")
        settings = root / "config" / "aterm"
        settings.mkdir(parents=True)
        (settings / "aterm.toml").write_text(
            "[harness]\naccounts='early'\nrelaunch=true\nresume_limits=true\nhuman_grace_s=0\n")
        (settings / "accounts-codex.toml").write_text(
            "[[account]]\nlabel='main'\nclass='dedicated'\ndir=" + json.dumps(str(old.parent))
            + "\nemail='fixture@example.invalid'\n\n[[account]]\nlabel='target'\nclass='dedicated'\ndir="
            + json.dumps(str(new.parent)) + "\nemail='target@example.invalid'\n")
        for session in sessions:
            worker = session.root
            worker.mkdir()
            env = probe.fresh_environment(worker)
            env.update({key: common[key] for key in ("HOME", "ATERM_HARNESS_STATE", "XDG_CONFIG_HOME")})
            probe.configure(worker, port)
            config_file = worker / "codex-home" / "config.toml"
            text = config_file.read_text().replace("requires_openai_auth = false", "requires_openai_auth = true")
            text = text.replace('name = "local capacity smoke"', 'name = "OpenAI"')
            text = text.replace("http://127.0.0.1:", "https://127.0.0.1:")
            text = text.replace("[tools.update_plan]", "[features]\nenable_request_compression = false\n[tools.update_plan]")
            probe.require('name = "OpenAI"' in text and "requires_openai_auth = true" in text,
                          "fixture does not use the actual first-party provider contract")
            config_file.write_text(f'cli_auth_credentials_store = "file"\nchatgpt_base_url = "{issuer}"\n' + text)
            shutil.copyfile(root / "fixture-ca.der", worker / "fixture-ca.der")
            fixture = {"root": str(worker), "shared_pool_root": str(root), "codex": str(codex),
                "cwd": str(worker / "work"), "auth_path": str(old), "successor_auth_path": str(new),
                "weekly_quota": True, "human_grace_s": 0,
                "args": ["--no-alt-screen", "--dangerously-bypass-approvals-and-sandbox",
                         "--cd", str(worker / "work"), prompt(session.index)],
                "env": {"CODEX_HOME": env["CODEX_HOME"], "PATH": env["PATH"],
                        "XDG_CONFIG_HOME": env["XDG_CONFIG_HOME"], "NO_COLOR": "1", "LANG": "en_US.UTF-8",
                        "RUST_LOG": "codex_alab_autopilot=debug",
                        "CODEX_CA_CERTIFICATE": str(root / "fixture-ca.pem")}}
            probe.write_json(worker / "fixture.json", fixture)
            (worker / "fixture.json").chmod(0o600)
            if args.control or args.lost_consume_ack:
                # Which session is the canary is not known yet: every host drops and pauses
                # its first consume ack, and the followers' switches go once the canary's
                # host has used its own.
                (worker / "drop-consume-ack").touch()
                (worker / "pause-consume-ack").touch()
            session.env = env
        for session in sessions:
            command = [str(host), "--renewal-config", str(session.root / "fixture.json")]
            probe.write_json(session.root / "host-command.json", command)
            with (session.root / "host-stdout.jsonl").open("xb") as stdout, \
                    (session.root / "host-stderr.txt").open("xb") as stderr:
                session.process = subprocess.Popen(command, cwd=session.root / "work", env=session.env,
                    stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr)
        report["hostsStartedSeconds"] = round(time.monotonic() - started, 3)

        deadline = time.monotonic() + drive_seconds
        canary = None
        watch_until = None
        ended = None
        try:
            while time.monotonic() < deadline:
                renewal.require_no_fixture_fault(model)
                for session in sessions:
                    if session.metadata is None:
                        session.read_metadata()
                    probe.require(session.process.poll() is None,
                                  f"the native host of session {session.index} exited before the run ended")
                if args.control or args.lost_consume_ack:
                    if canary is None:
                        dropped = [session for session in sessions if (session.root / "consume-ack-dropped").exists()]
                        probe.require(len(dropped) <= 1, "more than one host dropped a consume ack")
                        if dropped:
                            canary = dropped[0]
                            for session in sessions:
                                if session is not canary:
                                    (session.root / "drop-consume-ack").unlink(missing_ok=True)
                                    (session.root / "pause-consume-ack").unlink(missing_ok=True)
                            if args.control:
                                lock_deadline = time.monotonic() + 2
                                while custody is None and time.monotonic() < lock_deadline:
                                    custody = take_custody(new.parent / "auth.json.lock")
                                    if custody is None:
                                        time.sleep(0.005)
                                probe.require(custody is not None,
                                              "the destination's custody lock stayed held by another process")
                                held["fromUs"] = renewal.monotonic_us()
                                held["canary"] = canary.index
                            (canary.root / "continue-consume-ack").touch()
                            report["consumeAckDroppedSession"] = canary.index
                    elif args.control and custody is not None:
                        now = renewal.monotonic_us()
                        reports = [stamp for verb, _, stamp in renewal.rotation_calls(canary.trace)
                                   if verb == "complete"]
                        spent = bool(reports) and (now - reports[0] >= (REPORT_BUDGET_SECONDS + REPORT_SETTLE_SECONDS) * 1_000_000
                                                   and now - reports[-1] >= REPORT_SETTLE_SECONDS * 1_000_000)
                        if spent or now - held["fromUs"] >= HOLD_CAP_SECONDS * 1_000_000:
                            fcntl.flock(custody, fcntl.LOCK_UN)
                            os.close(custody)
                            custody = None
                            held["untilUs"] = renewal.monotonic_us()
                            held["reportsSeen"] = len(reports)
                            watch_until = time.monotonic() + args.control_watch
                if args.control:
                    # A follower released, or a proof, ends the control at once: the verdict
                    # names which.
                    if all(session.metadata for session in sessions) and canary is not None:
                        released = [session.index for session in sessions if session is not canary
                                    and first_taken(renewal.rotation_calls(session.trace)) is not None]
                        state_path = pool / "state.json"
                        proven = state_path.is_file() and json.loads(state_path.read_text()).get("proven") is True
                        if released or proven:
                            ended = f"followers whose commit was taken: {released}; pool proven: {proven}"
                            break
                    if watch_until is not None and time.monotonic() >= watch_until:
                        ended = "watched"
                        break
                elif all(session.metadata for session in sessions):
                    raw = pool_receipts(pool)
                    for session in sessions:
                        if session.receipt is not None:
                            continue
                        mine = [receipt for receipt in raw if receipt.get("sid") == session.metadata["sid"]
                                and receipt.get("phase") == "consumed"]
                        if len(mine) != 1:
                            probe.require(len(mine) <= 1, f"session {session.index} consumed two receipts")
                            continue
                        receipt = mine[0]
                        thread = receipt["intent"]["thread"]
                        successor = next((record for record in renewal.records(session.root / "codex-home")
                                          if record.get("pid") == receipt.get("successor_pid")), None)
                        if successor is None or renewal.successor_rest(successor, thread) is None:
                            continue
                        if session.rest_at is None:
                            session.rest_at = time.monotonic()
                        if (renewal.completion_seen(renewal.rotation_calls(session.trace), receipt["id"]) is None
                                and time.monotonic() < session.rest_at + renewal.COMPLETION_WAIT_SECONDS):
                            continue
                        proof = renewal.consumed_receipt(root, thread, receipt["intent"]["wall"],
                                                         receipt["successor_pid"])
                        if proof is None:
                            continue
                        session.receipt = dict(proof, sid=receipt["sid"])
                        session.successor = renewal.successor_rest(successor, thread)
                    if all(session.receipt is not None for session in sessions):
                        ended = "moved"
                        break
                time.sleep(0.1)
            report["loopEnded"] = ended or "budget"
            report["loopEndedSeconds"] = round(time.monotonic() - started, 3)
            watch_end_us = renewal.monotonic_us()
            now_unix = time.time()
            if args.control and ended is None:
                report["loopEnded"] = ("the canary's host never dropped its consume ack" if canary is None
                                       else "the custody lock was never let go" if custody is not None
                                       else "the watch outlived the run's budget")
            probe.require(ended is not None, f"the {name} run reached no outcome within its {drive_seconds} s "
                          f"({report['loopEnded']}): "
                          + "; ".join(f"session {session.index}: " + ", ".join(
                              f"{verb} {renewal.reply_state(reply) or reply[:40]}"
                              for verb, reply, _ in renewal.rotation_calls(session.trace)[-4:])
                              for session in sessions))
        finally:
            if custody is not None:
                fcntl.flock(custody, fcntl.LOCK_UN)
                os.close(custody)
                custody = None
                held.setdefault("untilUs", renewal.monotonic_us())
            for session in sessions:
                (session.root / "stop").touch()
            waits = []
            for session in sessions:
                if session.process is None:
                    continue
                stalled = False
                try:
                    code = session.process.wait(timeout=renewal.HOST_EXIT_SECONDS)
                except subprocess.TimeoutExpired:
                    stalled = True
                    try:
                        renewal.host_exit_diagnostics(session.root, session.process,
                            min(20, started + guardian_seconds - time.monotonic() - 20))
                    finally:
                        session.process.terminate()
                    code = session.process.wait(timeout=10)
                waits.append({"session": session.index, "pid": session.process.pid,
                              "returncode": code, "stalled": stalled})
            report["hostWaits"] = waits
        probe.require(all(not wait["stalled"] and wait["returncode"] == 0 for wait in report["hostWaits"]),
                      "a native host did not exit cleanly: " + json.dumps(report["hostWaits"]))
        renewal.require_no_fixture_fault(model)
        probe.require(not model.unknown_requests, "unknown model request shape")
        probe.require({"source", "target"}.issubset(model.usage_reads),
                      "the shared pool did not read both accounts' usage")
        ledger = renewal.pool_ledger(pool)
        state = json.loads((pool / "state.json").read_text())
        report["pool"] = {"active": state.get("active"), "proven": state.get("proven"),
                          "canary": state.get("canary"), "rotations": len(state.get("rotations") or []),
                          "ledgerKinds": [event.get("kind") for event in ledger]}
        records = []
        for session in sessions:
            calls = renewal.rotation_calls(session.trace)
            with model.lock:
                refused = model.refused_us.get(session.index)
            records.append({"sid": session.metadata["sid"], "calls": calls, "refusedUs": refused,
                            "receipt": session.receipt})
            report.setdefault("rotationCalls", []).append(
                {"session": session.index, "sid": session.metadata["sid"], "refusedUs": refused,
                 "verbs": [f"{verb} {renewal.reply_state(reply) or reply[:60]}" for verb, reply, _ in calls]})
        if args.control:
            report["held"] = dict(held)
            report["completionWithheld"] = control_verdict(records, ledger, state, held, watch_end_us, now_unix)
            canary_index = report["completionWithheld"]["canary"]
            probe.require(canary_index == held.get("canary"),
                          "the session that dropped its consume ack is not the one answered ready")
            consumes = [stamp for verb, _, stamp in records[canary_index]["calls"] if verb == "consume"]
            probe.require(len(consumes) >= 2 and consumes[0] < held["fromUs"] < consumes[-1],
                          "the canary's consume was not dropped before the lock and asked again under it")
            report["requests"] = [requests_verdict(model.by_session[session.index], session.index,
                                                   session.index == canary_index) for session in sessions]
        else:
            report["followedAfterCompletion"] = pool_verdict(records, ledger, state)
            if args.lost_consume_ack:
                canary_index = report["followedAfterCompletion"]["canary"]
                probe.require(report.get("consumeAckDroppedSession") == canary_index
                              and (sessions[canary_index].root / "consume-ack-dropped").is_file()
                              and sum(verb == "consume" for verb, _, _ in records[canary_index]["calls"]) >= 2,
                              "the canary's consume ack was not dropped and asked again")
                report["consumeAckDropped"] = True
            report["successorsAtRest"] = [session.successor for session in sessions]
            report["requests"] = [requests_verdict(model.by_session[session.index], session.index, True)
                                  for session in sessions]
        report["auxiliaryTitleRequestCount"] = len(model.auxiliary_requests)
        report["passed"] = True
    except Exception as error:
        report["error"] = f"{type(error).__name__}: {error}"
    finally:
        if custody is not None:
            fcntl.flock(custody, fcntl.LOCK_UN)
            os.close(custody)
        for session in sessions:
            if session.root.is_dir():
                (session.root / "stop").touch()
        for session in sessions:
            if session.process is not None and session.process.poll() is None:
                session.process.terminate()
                try:
                    session.process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    session.process.kill()
        server.shutdown()
        server_thread.join()
        server.server_close()
        report["requestAttribution"] = {"foreground": len(model.requests),
            "threadTitle": len(model.auxiliary_requests), "unknown": len(model.unknown_requests),
            "unroutedPosts": len(model.unrouted_posts),
            "bySession": [len(requests) for requests in model.by_session]}
        probe.write_json(root / "requests.json", model.requests)
        probe.write_json(root / "auxiliary-requests.json", model.auxiliary_requests)
        probe.write_json(root / "unknown-requests.json", model.unknown_requests)
        probe.write_json(root / "unrouted-posts.json", model.unrouted_posts)
        probe.write_json(root / "http-events.json", model.http_events)
        probe.write_json(root / "result.json", report)
    print(json.dumps({key: report.get(key) for key in ("passed", "mode", "qualifies", "error", "loopEnded",
                                                         "followedAfterCompletion", "completionWithheld")},
                     indent=2))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())

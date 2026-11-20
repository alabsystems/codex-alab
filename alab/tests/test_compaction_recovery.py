# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import compaction_recovery as smoke


class CompactionRecoveryTests(unittest.TestCase):
    def fixture(self, lane, scenario, mid_turn=False):
        model = smoke.CompactionModel(lane, scenario, mid_turn)
        probe = smoke.probe
        prompt = {"role": "user", "content": [{"type": "input_text", "text": probe.PROMPT}]}
        plan_result = {"type": "function_call_output", "call_id": probe.CALL_ID, "output": "Plan updated"}
        meta = {"turn_id": "compact", "thread_id": "thread", "request_kind": "compaction", "compaction": {
            "implementation": "responses" if lane == "legacy" else "responses_compaction_v2"}}
        if mid_turn:
            meta["compaction"].update(trigger="auto", reason="context_limit", phase="mid_turn")
        else:
            meta["compaction"].update(trigger="manual", reason="user_requested", phase="standalone_turn")
        body = {"model": probe.MODEL, "input": [prompt, probe.plan_call(), plan_result],
                "client_metadata": {"x-codex-turn-metadata": json.dumps(meta)}}
        denied = scenario == "permission-denied"
        legacy_summary = "Summary:\n" + smoke.SUMMARY_ANSWER
        times = [1., 2., 3., 5. if scenario == "opening-header" else 7. + smoke.FRAME_DELAY]
        if not denied:
            times.append(times[-1] + 3.)
        if mid_turn:
            times.pop(0)
            if not denied:
                times.append(times[-1] + 1.)
        for index, t in enumerate(times):
            request = copy.deepcopy(body)
            resumed = mid_turn and not denied and index == len(times) - 1
            if index < (1 if mid_turn else 2) or resumed:
                sample_meta = {"thread_id": "thread", "turn_id": "compact" if mid_turn else "seed", "request_kind": "turn"}
                request["client_metadata"]["x-codex-turn-metadata"] = json.dumps(sample_meta)
                if index == 0:
                    request["input"] = [prompt]
                elif resumed:
                    replacement = ({"type": "compaction", "encrypted_content": "fixture-compacted-context"}
                                   if lane == "v2" else {"role": "user", "content": [
                                       {"type": "input_text", "text": legacy_summary}]})
                    request["input"] = [prompt, replacement]
            elif lane == "legacy":
                request["input"].append({"role": "user", "content": smoke.LEGACY_PROMPT})
            status, _, _, advice, entry = model.reply(request)
            entry.update(status=status, retryAfter=advice, requestStartedAt=t, headerStartedAt=t, headerAt=t)
            if "failureFrameDelay" in entry:
                frame = t + entry["failureFrameDelay"]
                entry.update(failureFrameStartedAt=frame, failureFrameFlushedAt=frame)
        events = []

        def emit(method, **params):
            events.append({"method": method, "params": {"threadId": "thread", **params}})

        first_turn = "compact" if mid_turn else "seed"
        emit("turn/started", turn={"id": first_turn, "status": "inProgress"})
        message = {"type": "userMessage", "id": "prompt", "content": [{"type": "text", "text": probe.PROMPT}]}
        for method in ("item/started", "item/completed"):
            emit(method, turnId=first_turn, item=copy.deepcopy(message))
        emit("turn/plan/updated", turnId=first_turn, plan=[{"step": probe.PLAN, "status": "completed"}])
        if not mid_turn:
            emit("item/completed", turnId="seed", item={"type": "agentMessage", "text": probe.ANSWER})
            emit("turn/completed", turn={"id": "seed", "status": "completed"})
            emit("turn/started", turn={"id": "compact", "status": "inProgress"})
        item = {"id": "item", "type": "contextCompaction"}
        emit("item/started", turnId="compact", item=item)
        emit("error", turnId="compact", willRetry=True, error={"codexErrorInfo": "serverOverloaded"})
        reason = {"httpConnectionFailed": {"httpStatusCode": 403}} if denied else "policyCapacityUnavailable"
        last_error = {"codexErrorInfo": reason, "message": "Compaction forbidden" if denied else "Temporary policy capacity"}
        emit("error", turnId="compact", willRetry=not denied, error=last_error)
        if not denied:
            emit("item/completed", turnId="compact", item=item)
            if mid_turn:
                emit("item/completed", turnId="compact", item={"type": "agentMessage", "text": probe.ANSWER})
        emit("turn/completed", turn={"id": "compact", "status": "failed" if denied else "completed",
                                     "error": copy.deepcopy(last_error) if denied else None})
        return model.requests, events, "thread", None if mid_turn else "seed", "compact", lane, scenario, legacy_summary

    def test_all_scenarios_require_the_applicable_provider_deadline(self):
        for name, lane, scenario, mid_turn in smoke.COMPACTION_CASES:
            with self.subTest(case=name):
                fixture = self.fixture(lane, scenario, mid_turn)
                delays = smoke.check_compaction(*fixture)
                self.assertEqual(delays[0], 2 if scenario == "opening-header" else 4)
                fixture[0][2 if mid_turn else 3]["requestStartedAt"] -= 0.1
                with self.assertRaisesRegex(RuntimeError, "Retry-After bypassed"):
                    smoke.check_compaction(*fixture)

    def test_frame_advice_must_shorten_a_live_opening_deadline(self):
        fixture = self.fixture("legacy", "frame-advice", mid_turn=True)
        failed, retry = fixture[0][1:3]
        self.assertEqual(failed["retryAfter"], 324)
        delay = failed["headerStartedAt"] + 324 - retry["requestStartedAt"]
        for request in fixture[0][2:]:
            for stamp in ("requestStartedAt", "headerStartedAt", "headerAt"):
                request[stamp] += delay
        with self.assertRaisesRegex(RuntimeError, "shortened the opening deadline"):
            smoke.check_compaction(*fixture)

    def test_opening_deadline_uses_the_interval_the_provider_sent(self):
        for mid_turn in (False, True):
            with self.subTest(mid_turn=mid_turn), patch.dict(smoke.OPENING_WAITS, {"opening-header": 7}):
                fixture = self.fixture("v2", "opening-header", mid_turn=mid_turn)
                requests = fixture[0]
                first = 1 if mid_turn else 2
                failed = requests[first]
                self.assertEqual(failed["retryAfter"], 7)
                self.assertEqual(failed["failureFrame"].get("headers"),
                                 {"retry-after": "invalid"} if mid_turn else None)
                with self.assertRaisesRegex(RuntimeError, "Retry-After bypassed"):
                    smoke.check_compaction(*fixture)  # The recorded retry arrived after two seconds.
                for request in requests[first + 1:]:
                    for stamp in ("requestStartedAt", "headerStartedAt", "headerAt"):
                        request[stamp] += 5
                self.assertEqual(smoke.check_compaction(*fixture)[0], 7)

    def test_envelope_case_rejects_advice_moved_into_the_error(self):
        fixture = self.fixture("v2", "frame-advice", mid_turn=True)
        failed = fixture[0][1]["failureFrame"]
        failed["response"]["error"]["headers"] = failed.pop("headers")
        with self.assertRaisesRegex(RuntimeError, "expected provider advice"):
            smoke.check_compaction(*fixture)

    def test_each_refusal_requires_its_single_numeric_retry_header(self):
        for name, lane, scenario, mid_turn in smoke.COMPACTION_CASES:
            for refusal in (0, 1):
                with self.subTest(case=name, refusal=refusal):
                    fixture = self.fixture(lane, scenario, mid_turn)
                    failed = fixture[0][(1 if mid_turn else 2) + refusal]
                    failed["responseHeaders"] = {"x-request-id": "fixture"}
                    smoke.check_compaction(*fixture)
                    failed["responseHeaders"]["rEtRy-AfTeR"] = "Thu, 01 Jan 1970 00:00:00 GMT"
                    with self.assertRaisesRegex(ValueError, "Retry-After override"):
                        smoke.check_compaction(*fixture)
                    del failed["responseHeaders"]["rEtRy-AfTeR"]
                    failed["retryAfter"] = float(failed["retryAfter"])
                    with self.assertRaisesRegex(ValueError, "expected a capacity response with Retry-After"):
                        smoke.check_compaction(*fixture)

    def test_frame_deadline_requires_a_delayed_and_measured_frame_write(self):
        for changes in ({"failureFrameStartedAt": None}, {"failureFrameFlushedAt": float("nan")},
                        {"failureFrameStartedAt": 3.}, {"failureFrameFlushedAt": 4.06}):
            with self.subTest(changes=changes):
                fixture = self.fixture("v2", "frame-advice", mid_turn=True)
                fixture[0][1].update(changes)
                with self.assertRaisesRegex(RuntimeError, "frame timing|emission of failure frame"):
                    smoke.check_compaction(*fixture)

    def test_sampling_and_compaction_each_keep_their_original_reasoning_settings(self):
        for name, lane, scenario, mid_turn in smoke.COMPACTION_CASES:
            fixture = self.fixture(lane, scenario, mid_turn)
            posts = fixture[0]
            for post in posts:
                kind = smoke.turn_metadata(post["body"])["request_kind"]
                post["body"]["reasoning"] = {"effort": "high" if kind == "turn" else "medium", "summary": "auto"}
            smoke.check_compaction(*fixture)
            for index, post in enumerate(posts):
                body = post["body"]
                kind = smoke.turn_metadata(body)["request_kind"]
                if index == 0 or index == (1 if mid_turn else 2):
                    continue  # The first request of each kind sets its policy.
                with self.subTest(case=name, kind=kind, index=index):
                    summary = body["reasoning"].pop("summary")
                    with self.assertRaisesRegex(ValueError, "original reasoning settings"):
                        smoke.check_compaction(*fixture)
                    body["reasoning"]["summary"] = summary

    def test_retried_compaction_must_match_lane_and_turn(self):
        fixture = self.fixture("v2", "opening-header")
        metadata = fixture[0][-1]["body"]["client_metadata"]
        original = json.loads(metadata["x-codex-turn-metadata"])
        for change in ({"turn_id": "seed"}, {"thread_id": "another"}, {"request_kind": "turn"},
                       {"compaction": {"implementation": "responses"}}):
            with self.subTest(change=change):
                metadata["x-codex-turn-metadata"] = json.dumps({**original, **change})
                with self.assertRaisesRegex(RuntimeError, "expected compaction"):
                    smoke.check_compaction(*fixture)

    def test_explicit_seed_requests_belong_to_the_original_turn_and_complete_its_plan(self):
        for index in (0, 1):
            for field, value in (("thread_id", "foreign"), ("turn_id", "compact"), ("request_kind", "compaction")):
                fixture = self.fixture("legacy", "frame-advice")
                metadata = fixture[0][index]["body"]["client_metadata"]
                meta = json.loads(metadata["x-codex-turn-metadata"])
                metadata["x-codex-turn-metadata"] = json.dumps({**meta, field: value})
                with self.subTest(index=index, field=field), self.assertRaisesRegex(RuntimeError, "original turn"):
                    smoke.check_compaction(*fixture)
        fixture = self.fixture("v2", "frame-advice")
        fixture[0][1]["body"]["input"].pop()
        with self.assertRaisesRegex(RuntimeError, "completed plan output"):
            smoke.check_compaction(*fixture)
        fixture[0][0]["body"]["input"].append(smoke.probe.plan_call())
        with self.assertRaisesRegex(RuntimeError, "already contained tool work"):
            smoke.check_compaction(*fixture)

    def test_explicit_compaction_checks_manual_metadata_on_every_attempt(self):
        for index in (2, 3, 4):
            for field, value in (("trigger", "auto"), ("reason", "context_limit"), ("phase", "mid_turn")):
                fixture = self.fixture("v2", "frame-advice")
                metadata = fixture[0][index]["body"]["client_metadata"]
                meta = json.loads(metadata["x-codex-turn-metadata"])
                meta["compaction"][field] = value
                metadata["x-codex-turn-metadata"] = json.dumps(meta)
                with self.subTest(index=index, field=field), self.assertRaisesRegex(RuntimeError, "trigger, reason or phase"):
                    smoke.check_compaction(*fixture)

    def test_denied_compaction_must_end_its_retry_notifications(self):
        fixture = self.fixture("legacy", "permission-denied")
        fixture[1][-2]["params"]["willRetry"] = True
        with self.assertRaisesRegex(RuntimeError, "retry decision"):
            smoke.check_compaction(*fixture)

    def test_successful_seed_and_compaction_cannot_carry_a_stale_error(self):
        fixture = self.fixture("legacy", "frame-advice")
        turns = [e["params"]["turn"] for e in fixture[1] if e["method"] in ("turn/started", "turn/completed")]
        for turn in turns:
            with self.subTest(turn=turn), patch.dict(turn, error={"message": "stale"}), self.assertRaisesRegex(RuntimeError, "turn outcome"):
                smoke.check_compaction(*fixture)
            if turn["status"] == "inProgress":
                with patch.dict(turn, status="completed"), self.assertRaisesRegex(RuntimeError, "turn outcome"):
                    smoke.check_compaction(*fixture)

    def test_failed_compaction_must_retain_the_terminal_notification_error(self):
        fixture = self.fixture("v2", "permission-denied", mid_turn=True)
        turn = fixture[1][-1]["params"]["turn"]
        original = turn["error"]
        for error in (None, {**original, "message": "earlier overload"}, {**original, "codexErrorInfo": "serverOverloaded"}):
            with self.subTest(error=error), patch.dict(turn, error=error), self.assertRaisesRegex(RuntimeError, "terminal refusal"):
                smoke.check_compaction(*fixture)
        for e in (original, fixture[1][-2]["params"]["error"]):
            e["codexErrorInfo"] = {"httpConnectionFailed": {"httpStatusCode": 403.0}}
        with self.assertRaisesRegex(RuntimeError, "terminal refusal"):
            smoke.check_compaction(*fixture)

    def test_capacity_decisions_must_arrive_while_compaction_is_active(self):
        for name, lane, scenario, mid_turn in smoke.COMPACTION_CASES:
            for late in (False, True):
                with self.subTest(case=name, late=late):
                    fixture = self.fixture(lane, scenario, mid_turn)
                    events = fixture[1]
                    errors = [e for e in events if e["method"] == "error"]
                    moved = errors[-1] if late else errors[0]
                    events.remove(moved)
                    events.insert(len(events) if late else 0, moved)
                    with self.assertRaisesRegex(RuntimeError, "order of compaction"):
                        smoke.check_compaction(*fixture)

    def test_compaction_completion_must_close_the_original_item(self):
        for wrong in ({"turnId": "seed"}, {"item": {"type": "contextCompaction", "id": "replacement"}}):
            with self.subTest(wrong=wrong):
                fixture = self.fixture("v2", "frame-advice")
                done = next(e for e in fixture[1] if e["method"] == "item/completed"
                            and e["params"]["item"]["type"] == "contextCompaction")
                done["params"].update(wrong)
                with self.assertRaisesRegex(RuntimeError, "item lifecycle"):
                    smoke.check_compaction(*fixture)

    def test_compaction_and_resumed_sampling_cannot_introduce_user_input(self):
        for name, lane, scenario, mid_turn in smoke.COMPACTION_CASES:
            fixture = self.fixture(lane, scenario, mid_turn)
            requests = fixture[0]
            # Context may be dropped. The operator prompt must survive.
            requests[0]["body"]["input"].insert(0, {"role": "user", "content": "initial context"})
            smoke.check_compaction(*fixture)
            first = 1 if mid_turn else 2
            compactions = range(first, first + (2 if scenario == "permission-denied" else 3))
            positions = [compactions]
            if not mid_turn:
                positions.append([1])
            elif scenario != "permission-denied":
                positions.append([-1])
            for indexes in positions:
                with self.subTest(case=name, indexes=indexes):
                    changed = copy.deepcopy(requests)
                    for index in indexes:
                        changed[index]["body"]["input"].append({"role": "user", "content": "continue"})
                    with self.assertRaisesRegex(RuntimeError, f"{lane} recovery changed the original user input"):
                        smoke.check_compaction(changed, *fixture[1:])

    def test_legacy_text_must_match_the_configured_instruction_and_persisted_summary(self):
        fixture = self.fixture("legacy", "frame-advice", True)
        requests = copy.deepcopy(fixture[0])
        for request in requests[1:-1]:
            request["body"]["input"][-1]["content"] = "continue"
        with self.assertRaisesRegex(RuntimeError, "legacy recovery changed the original user input"):
            smoke.check_compaction(requests, *fixture[1:])
        fixture[0][-1]["body"]["input"][-1]["content"][0]["text"] = "continue\n" + smoke.SUMMARY_ANSWER
        with self.assertRaisesRegex(RuntimeError, "did not use the compaction result"):
            smoke.check_compaction(*fixture)

    def test_legacy_summary_must_be_recorded_once_in_the_selected_thread(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            sessions = root / "codex-home/sessions"
            sessions.mkdir(parents=True)
            summary = "Summary\u2028prefix\n" + smoke.SUMMARY_ANSWER
            row = json.dumps({"type": "compacted", "payload": {"message": summary}}, ensure_ascii=False) + "\n"
            (sessions / "rollout-foreign.jsonl").write_text(row)
            target = sessions / "rollout-selected.jsonl"
            for contents in ("", row + row):
                target.write_text(contents)
                with self.assertRaisesRegex(RuntimeError, "one persisted compaction summary"):
                    smoke.recorded_legacy_summary(root, "selected")
            target.write_text(row)
            self.assertEqual(smoke.recorded_legacy_summary(root, "selected"), summary)
            target.write_text(row.replace(smoke.SUMMARY_ANSWER, "continue"))
            with self.assertRaisesRegex(RuntimeError, "differs from the provider reply"):
                smoke.recorded_legacy_summary(root, "selected")

    def test_recovery_requires_one_operator_prompt_before_the_capacity_wait(self):
        for name, lane, scenario, mid_turn in smoke.COMPACTION_CASES:
            with self.subTest(case=name):
                fixture = self.fixture(lane, scenario, mid_turn)
                events = fixture[1]
                retry = next(i for i, event in enumerate(events) if event["method"] == "error")
                nudge = copy.deepcopy(events[1:3])
                for event in nudge:
                    event["params"]["turnId"] = "compact"
                    event["params"]["item"].update(id="nudge", content=[{"type": "text", "text": "continue"}])
                events[retry + 1:retry + 1] = nudge
                with self.assertRaisesRegex(RuntimeError, "order of compaction"):
                    smoke.check_compaction(*fixture)
                for event in nudge:
                    event["params"]["threadId"] = "unrelated"
                smoke.check_compaction(*fixture)

    def test_original_prompt_receipts_must_match_the_item_turn_and_content(self):
        for changes in ({"id": "other"}, {"content": [{"type": "text", "text": "continue"}]},
                        {"content": [{"type": "text", "text": smoke.probe.PROMPT}, {"type": "image"}]}):
            with self.subTest(changes=changes):
                fixture = self.fixture("v2", "frame-advice")
                fixture[1][2]["params"]["item"].update(changes)
                with self.assertRaisesRegex(RuntimeError, "unexpected operator input"):
                    smoke.check_compaction(*fixture)
        fixture = self.fixture("legacy", "frame-advice")
        fixture[1][1]["params"]["turnId"] = "compact"
        with self.assertRaisesRegex(RuntimeError, "unexpected operator input"):
            smoke.check_compaction(*fixture)

    def test_final_sampling_must_use_replacement_in_the_original_turn(self):
        for lane in ("legacy", "v2"):
            with self.subTest(lane=lane):
                fixture = self.fixture(lane, "frame-advice", mid_turn=True)
                body = fixture[0][-1]["body"]
                metadata = body["client_metadata"]
                original = metadata["x-codex-turn-metadata"]
                metadata["x-codex-turn-metadata"] = json.dumps({**json.loads(original), "turn_id": "another"})
                with self.assertRaisesRegex(RuntimeError, "left the original turn"):
                    smoke.check_compaction(*fixture)
                metadata["x-codex-turn-metadata"] = original
                body["input"].pop()
                with self.assertRaisesRegex(RuntimeError, "did not use the compaction result"):
                    smoke.check_compaction(*fixture)

    def test_a_preexisting_result_cannot_prove_automatic_compaction_installed_it(self):
        for lane in ("legacy", "v2"):
            fixture = self.fixture(lane, "frame-advice", mid_turn=True)
            result = fixture[0][-1]["body"]["input"][-1]
            for position in ("seed", "compaction retries"):
                with self.subTest(lane=lane, position=position):
                    requests = copy.deepcopy(fixture[0])
                    for request in (requests[:1] if position == "seed" else requests[1:-1]):
                        request["body"]["input"].append(copy.deepcopy(result))
                    with self.assertRaisesRegex(RuntimeError, "already present"):
                        smoke.check_compaction(requests, *fixture[1:])

    def test_automatic_compaction_cannot_replay_a_completed_plan(self):
        fixture = self.fixture("legacy", "frame-advice", mid_turn=True)
        plan = next(event for event in fixture[1] if event["method"] == "turn/plan/updated")
        fixture[1].append(plan)
        with self.assertRaisesRegex(RuntimeError, "order of compaction"):
            smoke.check_compaction(*fixture)

    def test_plan_and_answer_belong_to_the_sampling_turn_and_arrive_before_it_ends(self):
        for mid_turn in (False, True):
            fixture = self.fixture("v2", "frame-advice", mid_turn)
            for label, method in (("plan", "turn/plan/updated"), ("answer", "item/completed")):
                for change in ("missing", "late", "other turn", "changed"):
                    events = copy.deepcopy(fixture[1])
                    event = next(e for e in events if e["method"] == method and
                                 (label == "plan" or e["params"]["item"]["type"] == "agentMessage"))
                    if change in ("missing", "late"):
                        events.remove(event)
                        if change == "late":
                            events.append(event)
                    elif change == "other turn":
                        event["params"]["turnId"] = "other"
                    elif label == "plan":
                        event["params"]["plan"][0]["status"] = "pending"
                    else:
                        event["params"]["item"]["text"] = "continue"
                    with self.subTest(mid_turn=mid_turn, label=label, change=change), self.assertRaises(RuntimeError):
                        smoke.check_compaction(fixture[0], events, *fixture[2:])

    def test_compaction_retries_cannot_preserve_the_same_corrupt_tool_history(self):
        for name, lane, scenario, mid_turn in smoke.COMPACTION_CASES:
            for defect in ("replay", "orphan output", "foreign namespace", "call ID collision"):
                with self.subTest(case=name, defect=defect):
                    fixture = self.fixture(lane, scenario, mid_turn)
                    first = 1 if mid_turn else 2
                    count = 2 if scenario == "permission-denied" else 3
                    for request in fixture[0][first:first + count]:
                        history = request["body"]["input"]
                        call, output = (item for item in history if item.get("call_id") == smoke.probe.CALL_ID)
                        if defect == "replay":
                            history.extend([dict(call, call_id="replay"), dict(output, call_id="replay")])
                        elif defect == "orphan output":
                            history.append(dict(output, call_id="orphan"))
                        elif defect == "foreign namespace":
                            call["namespace"] = "other"
                        else:
                            history.append({"type": "message", "call_id": smoke.probe.CALL_ID})
                    with self.assertRaisesRegex(RuntimeError, "completed plan"):
                        smoke.check_compaction(*fixture)

# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""App-server compaction scenarios for request_recovery.py --compaction-only."""
from collections import Counter
import json

import capacity_exec as probe
from provider_fixture import user_message_texts
from quota_resume import Rpc


FRAME_DELAY = 1
POLICY_RETRY_SECONDS = 3
LEGACY_PROMPT = "Summarize the completed plan for the capacity recovery fixture."
SUMMARY_ANSWER = "The original plan completed."
ENCRYPTED_SUMMARY = "fixture-compacted-context"
OPENING_WAITS = {"opening-header": 2, "frame-advice": 324, "permission-denied": 2}
COMPACTION_CASES = tuple(
    (f"{prefix}{lane}-{scenario}", lane, scenario, mid_turn)
    for prefix, mid_turn in (("", False), ("mid-turn-", True))
    for lane in ("legacy", "v2")
    for scenario in OPENING_WAITS
)


class CompactionModel(probe.Mock):
    def __init__(self, lane, scenario, mid_turn=False):
        super().__init__(negative=False)
        self.lane = lane
        self.scenario = scenario
        self.mid_turn = mid_turn

    def reply_for_attempt(self, number, record):
        if number == 1:
            if self.mid_turn:
                # Force the real context-limit path while the plan still needs a follow-up.
                usage = {"input_tokens": 210000, "output_tokens": 0, "total_tokens": 210000}
                return 200, probe.response("seed-plan", probe.plan_call(), usage), "text/event-stream", None, record
            return super().reply_for_attempt(1, record)
        if number == 2 and not self.mid_turn:
            return super().reply_for_attempt(3, record)
        attempt = number - (1 if self.mid_turn else 2)
        if attempt == 1:
            error = {"code": "server_is_overloaded", "message": "Model is at capacity."}
            frame = {"type": "response.failed", "response": {
                "id": "refused-compaction", "status": "failed", "error": error}}
            if self.scenario != "opening-header":
                advised = frame if self.mid_turn and self.scenario == "frame-advice" else error
                advised["headers"] = {"retry-after": "4"}
                record["failureFrameDelay"] = FRAME_DELAY
            elif self.mid_turn:
                frame["headers"] = {"retry-after": "invalid"}
            record["failureFrame"] = frame
            return 200, probe.sse([frame]), "text/event-stream", OPENING_WAITS[self.scenario], record
        if attempt == 2:
            denied = self.scenario == "permission-denied"
            error = {"error": {"code": "policy_capacity_unavailable",
                               "message": "Compaction forbidden" if denied else "Temporary policy capacity"}}
            return (403 if denied else 503), json.dumps(error).encode(), "application/json", POLICY_RETRY_SECONDS, record
        if attempt == 3 and self.scenario != "permission-denied":
            if self.lane == "v2":
                item = {"type": "compaction", "encrypted_content": ENCRYPTED_SUMMARY}
            else:
                item = {"type": "message", "role": "assistant", "id": "summary",
                        "content": [{"type": "output_text", "text": SUMMARY_ANSWER}]}
            return 200, probe.response("compacted", item), "text/event-stream", None, record
        if attempt == 4 and self.mid_turn and self.scenario != "permission-denied":
            return super().reply_for_attempt(3, record)
        return 400, b'{"error":{"message":"Unexpected extra request"}}', "application/json", None, record


def recorded_legacy_summary(root, thread):
    summaries = []
    for path in (root / "codex-home/sessions").rglob("rollout-*.jsonl"):
        if path.name.endswith(f"-{thread}.jsonl"):
            for record in probe.provider_fixture.read_stdout_events(path):
                probe.require(isinstance(record, dict), "invalid compaction rollout record")
                if record.get("type") == "compacted":
                    payload = record.get("payload")
                    probe.require(isinstance(payload, dict), "invalid persisted compaction payload")
                    summaries.append(payload.get("message"))
    probe.require(len(summaries) == 1, "expected one persisted compaction summary for the fixture thread")
    summary = summaries[0]
    probe.require(isinstance(summary, str) and summary.endswith("\n" + SUMMARY_ANSWER),
                  "persisted compaction summary differs from the provider reply")
    return summary


def turn_metadata(body):
    client = body.get("client_metadata")
    raw = client.get("x-codex-turn-metadata") if isinstance(client, dict) else None
    probe.require(isinstance(raw, str), "missing provider request turn metadata")
    meta = json.loads(raw)
    probe.require(isinstance(meta, dict), "invalid provider request turn metadata")
    return meta


def check_compaction(requests, events, thread, seed, compact, lane, scenario, legacy_summary=None, *,
                     legacy_prompt=LEGACY_PROMPT):
    denied = scenario == "permission-denied"
    # An explicit compaction has its own turn after the seed answer. A mid-turn
    # compaction has one initial sample and resumes sampling only after success.
    mid_turn = seed is None
    sampling = requests[:1] if mid_turn else requests[:2]
    compactions = requests[len(sampling):]
    if mid_turn and not denied:
        sampling += compactions[-1:]
        compactions = compactions[:-1]
    probe.require(len(sampling) == (1 if mid_turn and denied else 2)
                  and all(r["status"] == 200 for r in sampling)
                  and [r["status"] for r in compactions] == ([200, 403] if denied else [200, 503, 200]),
                  "unexpected compaction request sequence")
    probe.provider_fixture.check_reasoning_settings(sampling)
    probe.provider_fixture.check_reasoning_settings(compactions)
    sampling_turn = compact if mid_turn else seed
    for request in sampling:
        body = request["body"]
        meta = turn_metadata(body)
        probe.require((meta.get("thread_id"), meta.get("turn_id"), meta.get("request_kind"), body.get("model")) ==
                      (thread, sampling_turn, "turn", probe.MODEL), "sampling left the original turn")
    if mid_turn and not denied:
        check_compaction_result(requests, lane, legacy_summary)
    original = Counter(user_message_texts(sampling[0]["body"]["input"]))
    probe.require(original[probe.PROMPT] == 1, "sampling lost or duplicated the original prompt")
    # Legacy compaction sends its instruction as user text. Only the resumed
    # sample can contain the generated summary. V2 uses an encrypted item.
    resumed_summary = legacy_summary if lane == "legacy" and mid_turn and not denied else None
    instruction = legacy_prompt if lane == "legacy" else None
    for group, extra in ((sampling[1:], resumed_summary), (compactions, instruction)):
        allowed = original.copy()
        if extra is not None:
            allowed[extra] += 1
        for request in group:
            texts = Counter(user_message_texts(request["body"]["input"]))
            probe.require(texts[probe.PROMPT] == 1 and texts <= allowed and (extra is None or texts[extra] == 1),
                          f"{lane} recovery changed the original user input")
    tools = probe.provider_fixture.TOOL_CALL_TYPES | probe.provider_fixture.TOOL_OUTPUT_TYPES
    probe.require(not any(item.get("type") in tools for item in sampling[0]["body"]["input"]),
                  "initial sampling already contained tool work")
    if not mid_turn:
        probe.completed_plan_records(sampling[1]["body"]["input"])
    frame = compactions[0]["failureFrame"]
    frame_error = frame["response"]["error"]
    opening_wait = OPENING_WAITS[scenario]
    probe.provider_fixture.check_capacity_header(compactions[0], opening_wait)
    probe.provider_fixture.check_capacity_header(compactions[1], POLICY_RETRY_SECONDS)
    envelope_advice = mid_turn and scenario == "frame-advice"
    expected_advice = {"retry-after": "4"} if scenario != "opening-header" else None
    envelope_headers = expected_advice if envelope_advice else None
    if mid_turn and scenario == "opening-header":
        envelope_headers = {"retry-after": "invalid"}
    probe.require(frame.get("type") == "response.failed" and frame_error["code"] == "server_is_overloaded"
                  and frame_error.get("headers") == (None if envelope_advice else expected_advice)
                  and frame.get("headers") == envelope_headers,
                  "compaction trace did not exercise the expected provider advice")
    inputs = compactions[0]["body"]["input"]
    plan = probe.completed_plan_records(inputs)
    probe.require([item for item in inputs if item.get("call_id") == probe.CALL_ID] == plan,
                  "unexpected compaction record shares the completed plan call ID")
    implementation = "responses_compaction_v2" if lane == "v2" else "responses"
    phase = ("auto", "context_limit", "mid_turn") if mid_turn else ("manual", "user_requested", "standalone_turn")
    for request in compactions:
        body = request["body"]
        meta = turn_metadata(body)
        detail = meta.get("compaction")
        probe.require(isinstance(detail, dict), "missing compaction details on provider request")
        probe.require((meta.get("thread_id"), meta.get("turn_id"), meta.get("request_kind"),
                       detail.get("implementation")) == (thread, compact, "compaction", implementation),
                      "request did not belong to the expected compaction")
        probe.require((detail.get("trigger"), detail.get("reason"), detail.get("phase")) == phase,
                      "unexpected compaction trigger, reason or phase")
        probe.require(body["model"] == probe.MODEL and body["input"] == inputs, "compaction changed model or input")

    # Preserve wire order across event types. A retry reported after the turn
    # ended would otherwise look exactly like one that kept the turn active.
    lifecycle = []
    for event in events:
        params = event.get("params", {})
        if params.get("threadId") != thread:
            continue
        method = event.get("method")
        item_type = params.get("item", {}).get("type")
        if method in ("turn/started", "turn/completed", "turn/plan/updated", "error"):
            lifecycle.append((method, params))
        elif item_type in ("userMessage", "contextCompaction") or (item_type == "agentMessage" and method == "item/completed"):
            lifecycle.append(((item_type, method), params))
    answer = ("agentMessage", "item/completed")
    methods = ["turn/started", ("userMessage", "item/started"), ("userMessage", "item/completed"), "turn/plan/updated"]
    if not mid_turn:
        methods.extend([answer, "turn/completed", "turn/started"])
    methods.extend([("contextCompaction", "item/started"), "error", "error"])
    if not denied:
        methods.append(("contextCompaction", "item/completed"))
        if mid_turn:
            methods.append(answer)
    probe.require([method for method, _ in lifecycle] == [*methods, "turn/completed"],
                  "unexpected order of compaction lifecycle notifications")
    plan_event, = (params for method, params in lifecycle if method == "turn/plan/updated")
    probe.require(plan_event.get("turnId") == sampling_turn and
                  plan_event.get("plan") == [{"step": probe.PLAN, "status": "completed"}],
                  "expected a completed plan update in the sampling turn")
    for method, params in lifecycle:
        if method == answer:
            probe.require(params.get("turnId") == sampling_turn and params["item"].get("text") == probe.ANSWER,
                          "unexpected answer in the sampling turn")

    turns = [params["turn"] for method, params in lifecycle if method in ("turn/started", "turn/completed")]
    turn_ids = [compact, compact] if mid_turn else [seed, seed, compact, compact]
    statuses = ([] if mid_turn else ["inProgress", "completed"]) + ["inProgress", "failed" if denied else "completed"]
    probe.require(seed != compact and [turn["id"] for turn in turns] == turn_ids
                  and [turn.get("status") for turn in turns] == statuses
                  and all(turn.get("error") is None for turn in turns if turn["status"] != "failed"),
                  "unexpected seed or compaction turn outcome")
    errors = [params for method, params in lifecycle if method == "error"]
    second_reason = {"httpConnectionFailed": {"httpStatusCode": 403}} if denied else "policyCapacityUnavailable"
    reasons = ["serverOverloaded", second_reason]
    probe.require(all(v["turnId"] == compact and v["willRetry"] is retry and v["error"]["codexErrorInfo"] == reason
                      for v, retry, reason in zip(errors, (True, not denied), reasons, strict=True)),
                  "compaction reported an unexpected retry decision or reason")
    if denied:
        refusal = errors[-1]["error"]
        # Python compares 403.0 equal to 403. The native status must deserialize as an integer.
        probe.require(type(refusal["codexErrorInfo"]["httpConnectionFailed"]["httpStatusCode"]) is int
                      and isinstance(refusal.get("message"), str) and refusal["message"]
                      and json.dumps(turns[-1].get("error"), sort_keys=True) == json.dumps(refusal, sort_keys=True),
                      "failed compaction turn did not retain its terminal refusal")
    messages = [params for _, params in lifecycle if params.get("item", {}).get("type") == "userMessage"]
    original_id = messages[0]["item"].get("id")
    for message in messages:
        item = message["item"]
        content = item.get("content", [])
        probe.require(message.get("turnId") == turn_ids[0] and isinstance(original_id, str) and original_id
                      and item.get("id") == original_id and isinstance(content, list) and len(content) == 1
                      and isinstance(content[0], dict) and content[0].get("type") == "text"
                      and content[0].get("text") == probe.PROMPT, "unexpected operator input during compaction")
    items = [params for _, params in lifecycle if params.get("item", {}).get("type") == "contextCompaction"]
    probe.require(all(v["turnId"] == compact for v in items) and len({v["item"]["id"] for v in items}) == 1,
                  "compaction item lifecycle disagrees with outcome")
    frame_advice = scenario != "opening-header"
    waits = [probe.retry_after_delay(compactions[0], compactions[1], 4 if frame_advice else opening_wait,
                                     frame_delay=FRAME_DELAY if frame_advice else None)]
    if scenario == "frame-advice":
        # A retry after both deadlines cannot establish which advice the client used.
        probe.require(compactions[1]["requestStartedAt"] < compactions[0]["headerStartedAt"] + opening_wait,
                      "retry did not demonstrate that the frame shortened the opening deadline")
    if not denied:
        waits.append(probe.retry_after_delay(compactions[1], compactions[2], POLICY_RETRY_SECONDS))
    return waits


def check_compaction_result(requests, lane, legacy_summary):
    histories = [request["body"]["input"] for request in requests]
    if lane == "v2":
        expected = ENCRYPTED_SUMMARY
        contents = [[item.get("encrypted_content") for item in history
                     if isinstance(item, dict) and item.get("type") == "compaction"] for history in histories]
        retained = contents[-1] == [expected]
    else:
        expected = legacy_summary
        contents = [user_message_texts(history) for history in histories]
        retained = isinstance(expected, str) and contents[-1].count(expected) == 1
    probe.require(retained, "sampling did not use the compaction result")
    probe.require(all(expected not in earlier for earlier in contents[:-1]),
                  "the compaction result was already present before the provider returned it")


def compaction_case(codex, output, name, lane, scenario, mid_turn=False):
    root = output / name
    root.mkdir()
    env = probe.fresh_environment(root)
    model = CompactionModel(lane, scenario, mid_turn)
    server, worker = probe.serve(model)
    result = {"case": name, "passed": False}
    primary = None
    try:
        probe.configure(root, server.server_port, stream_max_retries=0)
        with (root / "codex-home/config.toml").open("a") as config:
            capability = "v2" if lane == "v2" else "unsupported"
            config.write(f'capabilities = {{ remote_compaction = "{capability}" }}\n')

        def compact(process, stdout):
            rpc = Rpc(process, stdout)
            rpc.initialize()
            thread = rpc.request("thread/start", {"cwd": str(root / "work"), "model": probe.MODEL})["thread"]["id"]
            # Turn completion includes provider backoff within the guarded command.
            seed = rpc.turn(thread, probe.PROMPT, completion_timeout=probe.COMMAND_TIMEOUT_SECONDS)
            if mid_turn:
                result.update(threadId=thread, compactionTurnId=seed["id"], compactionStatus=seed["status"])
                return
            probe.require(seed["status"] == "completed", "seed turn failed before compaction")
            rpc.request("thread/compact/start", {"threadId": thread})
            end = rpc.wait(lambda v: v.get("method") == "turn/completed" and v["params"]["threadId"] == thread
                           and v["params"]["turn"]["id"] != seed["id"],
                           timeout=probe.COMMAND_TIMEOUT_SECONDS)["params"]["turn"]
            result.update(threadId=thread, seedTurnId=seed["id"], compactionTurnId=end["id"],
                          compactionStatus=end["status"])

        args = ["app-server", "--listen", "stdio://"]
        if lane == "legacy":
            args.extend(["-c", f"compact_prompt={json.dumps(LEGACY_PROMPT)}"])
        if mid_turn:
            args.extend(["-c", "model_auto_compact_token_limit=200000", "-c", "model_context_window=300000"])
        result["returncode"] = probe.run(codex, args, root, env, "app", interact=compact)
        result["requestCount"] = len(model.requests)
        events = probe.read_case_stdout(root, result, compaction=True)
        if events is None:
            return result
        try:
            probe.require(result["returncode"] == 0, "app-server did not exit normally")
            summary = None
            if lane == "legacy" and mid_turn and scenario != "permission-denied":
                summary = recorded_legacy_summary(root, result["threadId"])
            result["retryDelays"] = check_compaction(model.requests, events, result["threadId"], result.get("seedTurnId"),
                                                    result["compactionTurnId"], lane, scenario, summary)
            result["passed"] = True
        except (ValueError, RuntimeError) as error:
            result["error"] = str(error)
        return result
    except BaseException as error:
        primary = error
        raise
    finally:
        probe.cleanup_case(server, worker, lambda: probe.write_json(root / "requests.json", model.requests), result, primary)

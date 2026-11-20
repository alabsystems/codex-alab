# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Queue a follow-up in a real CLI before its active turn returns a typed refusal."""

import argparse
import json
from pathlib import Path
import sys
import threading
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import capacity_exec as fixture
import model_resume
from terminal_controls import drive_pty


ORIGINAL = "Name one common tree."
QUEUED = "Name one common flower."
FRESH = "Name one common bird."
ANSWER = "Fixture second turn complete."
MESSAGE = "Fixture policy review is required."
CODE = "misalignment_policy_violation"
EXPLANATION = "Offline fixture asks for a fresh task."
STEER = "Please ask a new question about nature."
QUEUE_HEADING = "Queued follow-up inputs"
PAUSED_QUEUE_HEADING = "Follow-up inputs (automatic sending paused)"
WAIT = 60
# The passive case admits two requests and at most two fixture messages per
# request. Retain the first excess of each for diagnosis, and count the rest.
REQUEST_SAMPLE_LIMIT = 3
PROMPT_SAMPLE_LIMIT = 3


def mark_input_error(decoder):
    """Keep the original exception and distinguish input failures from handler faults."""
    def decode(value):
        try:
            return decoder(value)
        except (ValueError, TypeError, AttributeError, RecursionError) as error:
            error._queued_refusal_invalid_input = True
            raise
    return decode


@mark_input_error
def request_evidence(body):
    metadata = json.loads(body.get("client_metadata", {}).get("x-codex-turn-metadata", "{}"))
    markers = (ORIGINAL, QUEUED, FRESH, STEER)
    # Decode the entire input, even when a late item invalidates a request.
    prompts = []
    omitted_prompts = 0
    for text in fixture.provider_fixture.user_message_texts(body.get("input")):
        if any(marker in text for marker in markers):
            if len(prompts) < PROMPT_SAMPLE_LIMIT:
                prompts.append(text if text in markers else "<combined fixture input>")
            else:
                omitted_prompts += 1

    def identity(key):
        value = metadata.get(key)
        # The fixture uses a source name and two UUIDs. An oversized or
        # structured value must still fail identity checks when sampled.
        if value is None or (isinstance(value, str) and len(value) <= 64):
            return value
        return "<invalid metadata>"

    row = {"source": identity("thread_source"), "thread": identity("thread_id"),
           "turn": identity("turn_id"), "prompts": prompts}
    if omitted_prompts:
        row["omitted_prompts"] = omitted_prompts
    return row


class Provider:
    def __init__(self, events):
        self.events = events
        self.lock = threading.Lock()
        self.post_count = 0
        self.decoded_count = 0
        self.handler_errors = 0
        self.reported_unexpected_error = False
        self.pending = threading.Event()
        self.release = threading.Event()

    def serve(self):
        return fixture.serve(self, on_post=self.record_post, prepare=self.prepare_server, wrap_input=mark_input_error)

    def prepare_server(self, server):
        # Join every accepted request before judging the complete request list.
        server.daemon_threads = False
        original = server.handle_error
        traceback_lock = threading.Lock()

        def handle_error(request, address):
            invalid_input = getattr(sys.exc_info()[1], "_queued_refusal_invalid_input", False)
            with self.lock:
                self.handler_errors += 1
                emit = self.handler_errors == 1 or (not invalid_input and not self.reported_unexpected_error)
                if not invalid_input:
                    self.reported_unexpected_error = True
            if emit:
                # Keep socketserver's real traceback, including a later, distinct
                # transport or handler fault even after malformed input flooded it.
                with traceback_lock:
                    original(request, address)

        server.handle_error = handle_error

    def record_post(self):
        # Count every POST to this listener, including rejected routes and invalid bodies.
        with self.lock:
            self.post_count += 1

    def reply(self, body):
        row = request_evidence(body)
        with self.lock:
            self.decoded_count += 1
            index = self.decoded_count
            row["event"] = f"request-{index}"
            if len(self.events) < REQUEST_SAMPLE_LIMIT:
                self.events.append(row)
        if index == 1:
            self.pending.set()
            if not self.release.wait(WAIT):
                return 504, b"{}", "application/json", None, row
            finding = {"error_type": "unintended_action", "detailed_explanation": EXPLANATION,
                       "steer": {"message": STEER}}
            error = {"code": CODE, "message": MESSAGE, "misalignment": finding}
            return 403, json.dumps({"error": error}).encode(), "application/json", None, row
        item = {"type": "message", "id": f"msg-{index}", "role": "assistant",
                "content": [{"type": "output_text", "text": ANSWER}]}
        return 200, fixture.response(f"resp-{index}", item), "text/event-stream", None, row

    def expect(self, *expected):
        with self.lock:
            post_count = self.post_count
            decoded_count = self.decoded_count
            handler_errors = self.handler_errors
            actual = [(row["source"], row["prompts"]) for row in self.events]
            complete = decoded_count == len(actual) and not any(row.get("omitted_prompts") for row in self.events)
        expected_rows = [("user", prompts) for prompts in expected]
        fixture.require(complete and post_count == len(expected) and actual == expected_rows and not handler_errors,
                        f"unexpected provider requests: {post_count} POSTs, {decoded_count} decoded, "
                        f"handler errors={handler_errors}, sample {actual!r}")


def configure(root, port):
    # Interactive review must come from the process launch, even with this setting explicitly off.
    home = root / "codex-home"
    catalog = home / "models.json"
    fixture.write_json(catalog, {"models": [model_resume.model(fixture.MODEL)]})
    fixture.write_json(home / "auth.json", {"OPENAI_API_KEY": "fixture-local-only"})
    fixture.configure(root, port)
    config = home / "config.toml"
    config.write_text(f'''cli_auth_credentials_store = "file"
model_catalog_json = {json.dumps(str(catalog))}
check_for_update_on_startup = false
analytics.enabled = false
features.daemon_auto_start = false
features.api_key_model_discovery = false
features.enable_request_compression = false
features.misalignment_review = false
chatgpt_base_url = "http://127.0.0.1:{port}/backend-api"
openai_base_url = "http://127.0.0.1:{port}/v1"
''' + config.read_text() + f'\n[projects.{json.dumps(str(root / "work"))}]\ntrust_level = "trusted"\n')


def observe_paint(terminal, report, name, offset, *markers):
    terminal.displayed(*markers, offset=offset)
    end = terminal.raw.stat().st_size
    report["frames"][name] = {"start": offset, "end": end, "markers": list(markers)}
    return end


def run_terminal(binary, root, env, report, provider, *, interactive=False):
    def interact(terminal):
        def frame(name, width, draft=None):
            def observe(rows):
                # Frozen clients can retain the same input under the earlier heading.
                for i, row in enumerate(rows[:-1]):
                    title = row.strip().removeprefix("• ")
                    if (title in (QUEUE_HEADING, PAUSED_QUEUE_HEADING)
                            and rows[i + 1].strip() == f"↳ {QUEUED}"):
                        lines = [row.strip(), rows[i + 1].strip()]
                        if draft is None:
                            return lines
                        lines.extend(line.strip() for line in rows if draft in line)
                        if len(lines) == 3:
                            return lines
                return None

            report["frames"][name] = terminal.frame(width, observe, "held follow-up and composer")

        fixture.require(provider.pending.wait(WAIT), "provider never received the original turn")
        provider.expect([ORIGINAL])
        terminal.paste(QUEUED, QUEUED)
        offset = terminal.raw.stat().st_size
        terminal.send(b"\t")
        if interactive:
            offset = observe_paint(terminal, report, "queued_before_refusal", offset, QUEUE_HEADING, QUEUED)
        else:
            terminal.displayed(QUEUE_HEADING, offset=offset)
        provider.release.set()
        if interactive:
            observe_paint(terminal, report, "paused", offset, "Chat paused as a precaution", "Review findings")
        else:
            terminal.displayed(EXPLANATION)
        report["refused_turn"] = model_resume.wait_for_turn_completion(
            root, provider, terminal.process, None, time.monotonic() + WAIT, event_kind="request-1",
            expected_error={"message": MESSAGE, "codex_error_info": CODE})
        provider.expect([ORIGINAL])
        if interactive:
            offset = terminal.raw.stat().st_size
            terminal.send(b"\r")  # Open Review findings.
            observe_paint(terminal, report, "review", offset, "What we detected", "Continuation request", STEER)
            provider.expect([ORIGINAL])
            offset = terminal.raw.stat().st_size
            terminal.send(b"q")  # Close the pager without accepting the continuation.
            observe_paint(terminal, report, "confirmation", offset, "Acknowledge findings and continue")
            provider.expect([ORIGINAL])
            terminal.quit()
            return
        terminal.paste(FRESH, FRESH)
        frame("queued_with_fresh_draft", 90, FRESH)
        provider.expect([ORIGINAL])
        terminal.send(b"\r")
        terminal.displayed(ANSWER)
        provider.expect([ORIGINAL], [ORIGINAL, FRESH])
        report["fresh_turn"] = model_resume.wait_for_turn_completion(
            root, provider, terminal.process, ANSWER, time.monotonic() + WAIT, event_kind="request-2")
        fixture.require(report["fresh_turn"] != report["refused_turn"], "fresh input reused the refused turn")
        frame("queued_after_fresh_completion", 76)
        terminal.quit()

    flags = ["--enable", "misalignment_review"] if interactive else []
    drive_pty([str(binary), *flags, "--no-daemon", "--no-alt-screen", ORIGINAL], root, env, "terminal", interact)


def exercise(binary, root, *, interactive=False):
    root.mkdir(mode=0o700)
    env = fixture.fresh_environment(root)
    env.update(TERM="xterm-256color", TERM_PROGRAM="xterm", RUST_MIN_STACK="16777216",
               HTTP_PROXY="http://127.0.0.1:1", HTTPS_PROXY="http://127.0.0.1:1", NO_PROXY="127.0.0.1,localhost,::1")
    report = {"passed": False, "binary_sha256": model_resume.CHECK["sha256"](binary), "requests": [], "frames": {}}
    provider = Provider(report["requests"])
    listener = None
    try:
        try:
            listener = provider.serve()
            configure(root, listener[0].server_port)
            run_terminal(binary, root, env, report, provider, interactive=interactive)
        finally:
            fixture.cleanup_resources((("provider", *listener),) if listener else (), (), report, sys.exc_info()[1],
                                      stop=provider.release.set)
        fixture.require("cleanupErrors" not in report, report.get("error", "provider cleanup failed"))
        if interactive:
            provider.expect([ORIGINAL])
        else:
            provider.expect([ORIGINAL], [ORIGINAL, FRESH])
            first, second = report["requests"]
            fixture.require(first["thread"] == second["thread"], "fresh input switched threads")
        fixture.require(model_resume.CHECK["sha256"](binary) == report["binary_sha256"], "binary changed")
        report["passed"] = True
    except Exception as error:
        report.update(fixture.case_error_fields(error))
    finally:
        with provider.lock:
            report["post_count"] = provider.post_count
            if provider.handler_errors:
                report["handler_errors"] = provider.handler_errors
            if provider.decoded_count > len(provider.events):
                report["omitted_requests"] = provider.decoded_count - len(provider.events)
        fixture.write_json(root / "result.json", report)
    fixture.require(report["passed"], report.get("error", "queued refusal fixture failed"))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--interactive-review", action="store_true", help="open review without acknowledging the finding")
    args = parser.parse_args()
    exercise(args.codex.resolve(strict=True), args.output.resolve(), interactive=args.interactive_review)

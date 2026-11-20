# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import contextlib
import http.client
import io
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from feature_recipes.native_ui_controls import startup_ready
from feature_recipes import queued_refusal as refusal


class StartupTitleTests(unittest.TestCase):
    def test_ready_follows_starting_for_the_same_project(self):
        for project in (b"work", b"codex-alab", b"My Project"):
            with self.subTest(project=project):
                start = b"\x1b]0;Starting | " + project + b"\x07"
                ready = b"\x1b]0;Ready | " + project + b"\x1b\\"
                self.assertFalse(startup_ready(ready + start))
                self.assertFalse(startup_ready(start + b"\x1b]0;Ready | other\x07"))
                self.assertFalse(startup_ready(start + ready[:-1]))
                self.assertTrue(startup_ready(ready + start + ready))


class QueuedRefusalProviderTests(unittest.TestCase):
    def setUp(self):
        self.provider = refusal.Provider([])
        self.provider.release.set()
        self.server, thread = self.provider.serve()
        self.addCleanup(self.server.server_close)
        self.addCleanup(thread.join)
        self.addCleanup(self.server.shutdown)

    def post(self, body, path="/v1/responses", *, include_response=False):
        connection = http.client.HTTPConnection("127.0.0.1", self.server.server_port, timeout=60)
        try:
            connection.request("POST", path, body)
            response = connection.getresponse()
            data = response.read()
            return (response.status, data) if include_response else response.status
        finally:
            connection.close()

    @staticmethod
    def body(*prompts):
        return {"client_metadata": {"x-codex-turn-metadata": json.dumps({"thread_source": "user"})},
                "input": [{"role": "user", "content": [{"type": "input_text", "text": text}]} for text in prompts]}

    def check_rejected_post(self, body, path="/v1/responses"):
        self.assertEqual(self.post(json.dumps(self.body(refusal.ORIGINAL))), 403)
        self.provider.expect([refusal.ORIGINAL])
        # Some decoders reject a request by closing its connection before writing a response.
        with contextlib.redirect_stderr(io.StringIO()):
            try:
                status = self.post(body, path)
            except http.client.RemoteDisconnected:
                pass
            else:
                self.assertGreaterEqual(status, 400)
        self.assertEqual(self.post(json.dumps(self.body(refusal.ORIGINAL, refusal.FRESH))), 200)
        with self.assertRaisesRegex(RuntimeError, "unexpected provider"):
            self.provider.expect([refusal.ORIGINAL], [refusal.ORIGINAL, refusal.FRESH])

    def test_queued_image_replay_is_counted_even_when_text_decoding_fails(self):
        body = self.body(refusal.QUEUED)
        body["input"][0]["content"].append({"type": "input_image", "image_url": "data:image/png;base64,AA=="})
        self.check_rejected_post(json.dumps(body))

    def test_post_to_a_rejected_route_is_counted(self):
        self.check_rejected_post(json.dumps(self.body(refusal.QUEUED)), "/wrong-route")

    def test_invalid_json_is_counted_before_body_decoding(self):
        self.check_rejected_post(b"{")

    def test_empty_body_is_counted_before_length_validation(self):
        self.check_rejected_post(b"")

    def test_original_then_fresh_post_passes(self):
        self.assertEqual(self.post(json.dumps(self.body(refusal.ORIGINAL))), 403)
        self.assertEqual(self.post(json.dumps(self.body(refusal.ORIGINAL, refusal.FRESH))), 200)
        self.provider.expect([refusal.ORIGINAL], [refusal.ORIGINAL, refusal.FRESH])

    def test_retry_flood_counts_rejected_and_decoded_posts_with_distinct_responses(self):
        thread = "624f7aaf-f309-49df-a39d-626f4e7f99d9"
        turns = ["d985fe45-2b93-462c-9e6b-b91fe5d1f3b3", "8327192e-8eac-45c9-a60b-e60ad6bcd438"]
        for index, prompts in enumerate(([refusal.ORIGINAL], [refusal.ORIGINAL, refusal.FRESH])):
            body = self.body(*prompts)
            body["client_metadata"]["x-codex-turn-metadata"] = json.dumps(
                {"thread_source": "user", "thread_id": thread, "turn_id": turns[index]})
            self.assertEqual(self.post(json.dumps(body)), (403, 200)[index])
        self.provider.expect([refusal.ORIGINAL], [refusal.ORIGINAL, refusal.FRESH])
        first, second = self.provider.events
        self.assertEqual([(row["thread"], row["turn"]) for row in (first, second)],
                         [(thread, turns[0]), (thread, turns[1])])

        excess = json.dumps(self.body(refusal.QUEUED))
        for index in range(3, 35):
            self.assertEqual(self.post(excess, "/wrong-route"), 404)
            self.assertEqual(self.post(b""), 413)
            status, data = self.post(excess, include_response=True)
            self.assertEqual(status, 200)
            self.assertIn(f'"resp-{index}"'.encode(), data)
        with self.assertRaisesRegex(RuntimeError, "98 POSTs, 34 decoded"):
            self.provider.expect([refusal.ORIGINAL], [refusal.ORIGINAL, refusal.FRESH])
        self.assertEqual([row["event"] for row in self.provider.events], ["request-1", "request-2", "request-3"])
        self.assertEqual(self.provider.events[2]["prompts"], [refusal.QUEUED])
        for row, status in zip(self.provider.events, (403, 200, 200)):
            self.assertEqual(row["status"], status)
            self.assertLessEqual(row["requestStartedAt"], row["headerStartedAt"])
            self.assertLessEqual(row["headerStartedAt"], row["headerAt"])

    def test_repeated_inputs_keep_first_excess_and_still_reject_combined_stale_prompt(self):
        self.assertEqual(self.post(json.dumps(self.body(refusal.ORIGINAL))), 403)
        combined = refusal.FRESH + " " + refusal.QUEUED
        body = self.body(refusal.ORIGINAL, refusal.FRESH, *([combined] * 512))
        self.assertEqual(self.post(json.dumps(body)), 200)
        second = self.provider.events[1]
        with self.assertRaisesRegex(RuntimeError, "unexpected provider") as caught:
            self.provider.expect([refusal.ORIGINAL], [refusal.ORIGINAL, refusal.FRESH])
        self.assertEqual(second["prompts"], [refusal.ORIGINAL, refusal.FRESH, "<combined fixture input>"])
        self.assertEqual(second["omitted_prompts"], 511)
        self.assertLess(len(str(caught.exception)), 512)

    def test_oversize_and_structured_metadata_cannot_become_valid_identities(self):
        body = self.body(refusal.ORIGINAL)
        value = "user" + "X" * 8192
        body["client_metadata"]["x-codex-turn-metadata"] = json.dumps(
            {"thread_source": value, "thread_id": [value], "turn_id": value})
        self.assertEqual(self.post(json.dumps(body)), 403)
        with self.assertRaisesRegex(RuntimeError, "unexpected provider") as caught:
            self.provider.expect([refusal.ORIGINAL])
        self.assertLess(len(str(caught.exception)), 256)
        row = self.provider.events[0]
        self.assertLess(len(json.dumps(row)), 512)
        for field in ("thread", "turn"):
            self.assertIsNone(refusal.model_resume.CHECK["MODEL_UUID"].fullmatch(row[field]))

    def test_handler_stderr_keeps_first_input_error_and_first_independent_fault(self):
        late_invalid = self.body(refusal.ORIGINAL)
        late_invalid["input"].append({"role": "user", "content": [{"type": "invalid"}]})
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            for _ in range(16):
                for data in (b"{", json.dumps(late_invalid)):
                    with self.assertRaises(http.client.RemoteDisconnected):
                        self.post(data)
            real_reply = self.provider.reply

            def broken_reply(body):
                real_reply(body)
                raise ValueError("failure after recording the request")

            self.provider.reply = broken_reply
            for _ in range(2):
                with self.assertRaises(http.client.RemoteDisconnected):
                    self.post(json.dumps(self.body(refusal.FRESH)))

        traces = stderr.getvalue()
        self.assertEqual(traces.count("Traceback (most recent call last):"), 2)
        self.assertIn("JSONDecodeError:", traces)
        self.assertEqual(traces.count("ValueError: failure after recording the request"), 1)
        self.assertEqual(self.provider.handler_errors, 34)
        self.assertEqual(self.provider.post_count, 34)
        self.assertEqual(self.provider.decoded_count, 2)

    def test_otherwise_matching_request_counts_cannot_hide_a_handler_failure(self):
        self.assertEqual(self.post(json.dumps(self.body(refusal.ORIGINAL))), 403)
        real_reply = self.provider.reply

        def broken_reply(body):
            real_reply(body)
            raise RuntimeError("response writer failed")

        self.provider.reply = broken_reply
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(http.client.RemoteDisconnected):
            self.post(json.dumps(self.body(refusal.ORIGINAL, refusal.FRESH)))
        self.assertEqual(len(self.provider.events), 2)
        self.assertEqual(self.provider.post_count, 2)
        with self.assertRaisesRegex(RuntimeError, "handler errors=1"):
            self.provider.expect([refusal.ORIGINAL], [refusal.ORIGINAL, refusal.FRESH])

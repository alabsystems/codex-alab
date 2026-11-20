# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Completion routing for the app-server client shared by the recovery smokes."""
import io
import json
import os
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
from quota_resume import Rpc


class RecoveryRpcTurnTests(unittest.TestCase):
    def open_rpc(self):
        read_fd, write_fd = os.pipe()
        reader = self.enterContext(os.fdopen(read_fd, "rb"))
        writer = self.enterContext(os.fdopen(write_fd, "wb", buffering=0))
        return Rpc(SimpleNamespace(stdout=reader), io.BytesIO()), writer

    def test_eof_delimits_only_the_last_record_and_preserves_the_transcript(self):
        rpc, writer = self.open_rpc()
        data = b'{"id":1}\n{"id":2}'
        writer.write(data)
        self.assertEqual(rpc.wait(lambda value: value["id"] == 1), {"id": 1})
        self.assertIsNone(rpc.poll(lambda value: value["id"] == 2, 0.01))
        writer.close()
        self.assertEqual(rpc.wait(lambda value: value["id"] == 2), {"id": 2})
        self.assertEqual(rpc.output.getvalue(), data)
        with self.assertRaisesRegex(RuntimeError, "exited before its protocol reply"):
            rpc.wait(lambda _: True)

    def test_invalid_messages_fail_before_matching(self):
        for data, error in ((b'{"id":', json.JSONDecodeError), (b"null\n", RuntimeError),
                            ('{"id":1}'.encode("utf-16le") + b"\n", json.JSONDecodeError)):
            with self.subTest(data=data):
                rpc, writer = self.open_rpc()
                writer.write(data)
                writer.close()
                with self.assertRaises(error):
                    rpc.wait(lambda _: self.fail("invalid message reached a matcher"))
                self.assertEqual(rpc.output.getvalue(), data)

    def test_reply_before_a_malformed_record_survives_either_pipe_grouping(self):
        replies = b'{"id":1}\n{"id":2}\n'
        damaged = b'{"id":\n'
        for together in (False, True):
            with self.subTest(together=together):
                rpc, writer = self.open_rpc()
                writer.write(replies + damaged if together else replies)
                self.assertEqual(rpc.wait(lambda v: v["id"] == 2), {"id": 2})
                self.assertEqual(rpc.poll(lambda v: v["id"] == 1, 0), {"id": 1})
                if not together:
                    writer.write(damaged)
                with self.assertRaises(json.JSONDecodeError):
                    rpc.wait(lambda _: True)
                self.assertEqual(rpc.output.getvalue(), replies + damaged)

    def test_frame_limit_applies_to_each_record_in_a_read(self):
        with mock.patch("quota_resume.MAX_FRAME_BYTES", 8):
            rpc, writer = self.open_rpc()
            writer.write(b'{"id":1}\n{"id":2}\n{"id":123}')
            self.assertEqual(rpc.wait(lambda _: True), {"id": 1})
            self.assertEqual(rpc.poll(lambda _: True, 0), {"id": 2})
            with self.assertRaisesRegex(RuntimeError, "oversized app-server frame"):
                rpc.poll(lambda _: True, 0)

    def test_turn_consumes_only_its_acknowledged_completion(self):
        rpc = Rpc(SimpleNamespace(stdin=io.BytesIO()), None)

        def completed(thread_id, turn_id):
            return {"method": "turn/completed", "params": {
                "threadId": thread_id, "turn": {"id": turn_id, "status": "completed"}}}

        sibling = completed("sibling", "new")
        previous = completed("thread", "previous")
        current = completed("thread", "new")
        rpc.pending = [sibling, previous, current, {"id": 1, "result": {"turn": {"id": "new"}}}]
        self.assertIs(rpc.turn("thread", "continue"), current["params"]["turn"])
        self.assertEqual(rpc.pending, [sibling, previous])

    def test_turn_can_complete_after_the_request_deadline(self):
        for options, expected in (({}, 60), ({"completion_timeout": 120}, 65)):
            with self.subTest(options=options):
                rpc, writer = self.open_rpc()
                rpc.process.stdin = io.BytesIO()
                rpc.pending = [{"id": 1, "result": {"turn": {"id": "turn"}}}]
                turn = {"id": "turn", "status": "completed"}
                writer.write(json.dumps({"method": "turn/completed", "params": {
                    "threadId": "thread", "turn": turn}}).encode() + b"\n")
                elapsed = 0

                def select(readers, _writers, _errors, timeout):
                    nonlocal elapsed
                    elapsed += timeout
                    return (readers if elapsed >= 65 else []), [], []

                with mock.patch("quota_resume.time", SimpleNamespace(monotonic=lambda: elapsed)), \
                        mock.patch("quota_resume.select", SimpleNamespace(select=select)):
                    if options:
                        self.assertEqual(rpc.turn("thread", "continue", **options), turn)
                    else:
                        with self.assertRaisesRegex(RuntimeError, "protocol deadline expired"):
                            rpc.turn("thread", "continue")
                self.assertEqual(elapsed, expected)


if __name__ == "__main__":
    unittest.main()

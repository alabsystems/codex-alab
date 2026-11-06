#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The shared loopback model's Retry-After timing and header stamps, never a server or executable."""
import importlib.util
import io
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock


SOURCE = Path(__file__).resolve().parents[1] / "smoke/capacity_exec.py"
FIXTURE_PARENT = None
FIXTURES = []


class CapacityExecTimingTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        spec = importlib.util.spec_from_file_location("capacity_exec_timing", SOURCE)
        cls.probe = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.probe)

    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="capacity-header-timing-", dir=FIXTURE_PARENT))
        FIXTURES.append(str(self.root))

    def test_missing_invalid_or_unbounded_header_timing_refuses(self):
        for label, values in [
            ("missing", {"headerStartedAt": 13.0}),
            ("boolean", {"headerStartedAt": 13.0, "headerAt": True}),
            ("nan", {"headerStartedAt": 13.0, "headerAt": float("nan")}),
            ("infinite", {"headerStartedAt": 13.0, "headerAt": float("inf")}),
            ("backwards", {"headerStartedAt": 13.1, "headerAt": 13.0}),
            ("inconclusive", {"headerStartedAt": 13.0, "headerAt": 13.1}),
        ]:
            with self.subTest(label=label):
                with self.assertRaises(RuntimeError):
                    self.probe.retry_after_delay(values, {"requestStartedAt": 20.0})
        for arrival in ({}, {"at": 20.0}, {"requestStartedAt": True},
                        {"requestStartedAt": float("nan")}, {"requestStartedAt": float("inf")}):
            with self.subTest(arrival=arrival):
                with self.assertRaisesRegex(RuntimeError, "missing or invalid"):
                    self.probe.retry_after_delay({"headerStartedAt": 13.0, "headerAt": 13.001}, arrival)

    def test_handler_stamps_the_exact_response_after_headers_before_body(self):
        probe = self.probe
        fake = probe.Mock(False)
        fake.reply({"preexisting": True})
        captured = {}
        server = SimpleNamespace(serve_forever=lambda: None)
        thread = SimpleNamespace(start=lambda: None)

        def fake_server(address, handler):
            self.assertEqual(address, ("127.0.0.1", 0))
            captured["handler"] = handler
            return server

        with mock.patch.object(probe.http.server, "ThreadingHTTPServer", side_effect=fake_server), \
                mock.patch.object(probe.threading, "Thread", return_value=thread):
            self.assertEqual(probe.serve(fake), (server, thread))
        handler = object.__new__(captured["handler"])
        body = json.dumps({"request": "overload"}).encode()
        handler.path = "/v1/responses"
        handler.headers = {"content-length": str(len(body))}
        handler.rfile = io.BytesIO(body)
        handler.send_response = lambda _status: None

        def send_header(name, _value):
            if name == "content-type":
                # Another record arrives before this response sends its headers.
                fake.reply({"other": True})

        handler.send_header = send_header
        handler.end_headers = lambda: None

        def body_write(data):
            self.assertEqual(fake.requests[1]["requestStartedAt"], 10.0)
            self.assertEqual(fake.requests[1]["at"], 12.0)
            self.assertEqual(fake.requests[1]["headerStartedAt"], 13.0)
            self.assertEqual(fake.requests[1]["headerAt"], 13.001)
            self.assertNotIn("headerAt", fake.requests[2])
            return len(data)

        handler.wfile = SimpleNamespace(write=body_write)
        with mock.patch.object(probe.time, "monotonic", side_effect=[10.0, 12.0, 12.5, 13.0, 13.001]):
            handler.do_POST()
        fourth = fake.reply({"unexpected": True})
        self.assertEqual(fourth[0], 400)
        self.assertIs(fourth[4], fake.requests[3])
        negative = probe.Mock(True)
        refusal = negative.reply({"negative": True})
        self.assertEqual(refusal[0], 403)
        self.assertIs(refusal[4], negative.requests[0])
        probe.write_json(self.root / "handler-requests.json", fake.requests)


if __name__ == "__main__":
    unittest.main()

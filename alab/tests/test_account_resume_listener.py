# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Check that account evidence includes a GET already accepted when the listener closes."""
import http.client
import socket
import socketserver
import sys
import threading
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import account_resume as resume


class AccountResumeListenerTests(unittest.TestCase):
    def test_catalog_workspace_and_model_requests_require_unambiguous_login_headers(self):
        gateway = resume.Gateway({"events": [], "requests": []})
        gateway.tokens.update({"Bearer A": "source", "Bearer B": "destination"})
        server, worker = resume.fixture.start_listener(gateway.handler(), bind=resume.AccountResumeServer)
        try:
            for path in ("/v1/models", "/backend-api/wham/accounts/check", "/v1/responses"):
                for extra, login, workspace in (
                    (None, "destination", "workspace-destination"),
                    (("aUThorization", "Bearer A"), None, "workspace-destination"),
                    (("Authorization", "Bearer B"), None, "workspace-destination"),
                    (("CHATGPT-ACCOUNT-ID", "workspace-source"), "destination", None),
                    (("ChatGPT-Account-Id", "workspace-destination"), "destination", None),
                ):
                    with self.subTest(path=path, extra=extra):
                        post = path == "/v1/responses"
                        client = http.client.HTTPConnection(*server.server_address, timeout=5)
                        try:
                            client.putrequest("POST" if post else "GET", path)
                            client.putheader("Authorization", "Bearer B")
                            client.putheader("ChatGPT-Account-Id", "workspace-destination")
                            if extra:
                                client.putheader(*extra)
                            if post:
                                client.putheader("Content-Length", "2")
                            client.endheaders(b"{}" if post else None)
                            reply = client.getresponse()
                            reply.read()
                            denied = extra and path == "/backend-api/wham/accounts/check"
                            self.assertEqual(reply.status, 401 if denied else 200)
                        finally:
                            client.close()
                        row = (gateway.requests if post else gateway.reads)[-1]
                        self.assertEqual((row["login"], row["account"]), (login, workspace))
        finally:
            server.shutdown()
            worker.join(5)
            server.server_close()

    def test_accepted_wrong_account_read_completes_before_cleanup_returns(self):
        report = {"events": [], "requests": []}
        gateway = resume.Gateway(report)
        gateway.resumed = True
        gateway.tokens["Bearer synthetic-source"] = "source"
        accepted = threading.Event()

        class Handler(gateway.handler()):
            def setup(self):
                super().setup()
                accepted.set()

        listener = resume.fixture.start_listener(Handler, bind=resume.AccountResumeServer)
        server, worker = listener
        # Hold the final header delimiter until the accepting socket has closed.
        close_socket = socketserver.TCPServer.server_close
        with socket.create_connection(server.server_address, timeout=5) as client:
            try:
                client.sendall(b"GET /backend-api/wham/accounts/check HTTP/1.0\r\n"
                               b"Authorization: Bearer synthetic-source\r\n"
                               b"ChatGPT-Account-ID: workspace-source\r\n")
                self.assertTrue(accepted.wait(5))

                def close_and_complete(bound):
                    close_socket(bound)
                    client.sendall(b"\r\n")

                with patch.object(socketserver.TCPServer, "server_close", close_and_complete):
                    resume.fixture.cleanup_resources((("gateway", *listener),), (), report, None)
                self.assertNotIn("cleanupErrors", report)
                self.assertEqual(gateway.reads, [{"path": "/backend-api/wham/accounts/check", "login": "source",
                                                 "account": "workspace-source", "phase": "resume"}])
            finally:
                client.close()
                server.shutdown()
                worker.join(5)
                server.server_close()


if __name__ == "__main__":
    unittest.main()

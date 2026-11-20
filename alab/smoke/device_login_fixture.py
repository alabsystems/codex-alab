# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""A loopback device issuer for exercising the real Codex login command with scratch slots."""

from contextlib import contextmanager
from dataclasses import dataclass
import hashlib
import http.server
import json
from pathlib import Path
import re
import secrets
import ssl
import subprocess
import threading
from urllib.parse import parse_qs

from provider_fixture import LoopbackHTTPServer
from renewal_follow import access_token, identity_claims, token_part


@dataclass(frozen=True)
class Slot:
    generation: str
    account: str
    user: str
    email: str
    auth_path: Path


def login_args(port, index):
    return ["login", "--device-auth", "--experimental_issuer", f"https://127.0.0.1:{port}/signin/{index}",
            "--experimental_client-id", "fixture-client"]


class DeviceIssuer:
    """Accept one ordered device grant per slot; only boolean bearer comparisons leave the fixture."""

    def __init__(self, slots):
        self.slots = tuple(slots)
        self.steps = [0] * len(self.slots)
        self._bearers = {}
        self.lock = threading.Lock()
        if len({s.generation for s in self.slots}) != len(self.slots):
            raise ValueError("device issuer requires distinct generations")
        paths = [s.auth_path for s in self.slots]
        if any(not p.is_absolute() for p in paths) or len({p.resolve() for p in paths}) != len(paths):
            raise ValueError("device issuer requires distinct absolute credential paths")

    def bearer_matches(self, generation, authorization):
        if not isinstance(authorization, str):
            return False
        with self.lock:
            return self._bearers.get(generation) == hashlib.sha256(authorization.encode()).digest()

    def reply(self, index, endpoint, body):
        if not 0 <= index < len(self.slots):
            raise ValueError("device issuer account is outside the fixture")
        slot = self.slots[index]
        device = f"fixture-device-{index}"
        user_code = f"fixture-user-code-{index}"
        code = f"fixture-code-{index}"
        verifier = f"fixture-verifier-{index}"
        with self.lock:
            step = self.steps[index]
            if (step == 0 and endpoint == "api/accounts/deviceauth/usercode"
                    and json.loads(body).get("client_id") == "fixture-client"):
                response = {"device_auth_id": device, "user_code": user_code, "interval": "0"}
            elif (step == 1 and endpoint == "api/accounts/deviceauth/token"
                    and json.loads(body) == {"device_auth_id": device, "user_code": user_code}):
                response = {"authorization_code": code, "code_challenge": "fixture-challenge",
                            "code_verifier": verifier}
            elif step == 2 and endpoint == "oauth/token":
                form = parse_qs(body.decode())
                if any(form.get(k) != [v] for k, v in {
                    "grant_type": "authorization_code", "code": code, "client_id": "fixture-client",
                    "code_verifier": verifier}.items()):
                    raise ValueError("device token exchange does not match this login")
                # The fixture's JWT signature is opaque to the clients. Give every exchange
                # fresh bytes so even a prewritten token with identical identity claims fails.
                claims = access_token(slot.generation, slot.account, email=slot.email, user=slot.user)
                token = claims.rsplit(".", 1)[0] + "." + secrets.token_hex(16)
                self._bearers[slot.generation] = hashlib.sha256(("Bearer " + token).encode()).digest()
                identity = token_part(identity_claims(slot.account, slot.email, slot.user))
                response = {"access_token": token, "id_token": token_part({"alg": "none"}) + "." + identity + ".sig",
                            "refresh_token": f"fixture-refresh-{slot.generation}"}
            else:
                raise ValueError("unexpected device issuer request")
            self.steps[index] += 1
            return json.dumps(response).encode()


class LoginListener(DeviceIssuer):
    def __init__(self, slots, ca_path):
        super().__init__(slots)
        self.ca_path = ca_path
        self._listener = None
        self.port = None

    def __enter__(self):
        if self._listener is not None:
            raise RuntimeError("device issuer is already listening")
        listener = _listen(self)
        self.port = listener.__enter__()
        self._listener = listener
        return self

    def __exit__(self, *exc):
        listener, self._listener = self._listener, None
        self.port = None
        return listener.__exit__(*exc)

    def login(self, binary, codex_home, slot, env):
        """Return generation, issuerSteps=3, cliExit=0 and slotCreated=true after a complete login."""
        if self.port is None:
            raise RuntimeError("device issuer is not listening")
        index = self.slots.index(slot)
        auth = slot.auth_path
        if auth.exists() or auth.is_symlink():
            raise ValueError("device login slot was already initialized")
        auth.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        home = codex_home.resolve(strict=True)
        command = [binary.resolve(strict=True), *login_args(self.port, index)]
        result = subprocess.run(command, cwd=home,
                                env=dict(env, CODEX_HOME=str(home), CODEX_AUTH_PATH=str(auth),
                                         CODEX_CA_CERTIFICATE=str(self.ca_path.resolve(strict=True))),
                                stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                timeout=45)
        if result.returncode != 0 or self.steps[index] != 3 or auth.is_symlink() or not auth.is_file():
            raise RuntimeError(f"device login did not complete (exit {result.returncode}, issuer steps {self.steps[index]})")
        return {"generation": slot.generation, "issuerSteps": 3, "cliExit": 0, "slotCreated": True}


def device_login_issuer(ca_path, slots):
    """Construct a grant matcher now; entering it serves the leaf of the existing fixture_tls CA."""
    return LoginListener(slots, ca_path)


@contextmanager
def _listen(issuer):
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_POST(self):
            route = re.fullmatch(r"/signin/([0-9]+)/(.+)", self.path)
            if not route:
                self.send_error(404)
                return
            try:
                length = int(self.headers.get("Content-Length", "0"))
                if not 0 < length <= 65536:
                    raise ValueError("invalid request length")
                response = issuer.reply(int(route[1]), route[2], self.rfile.read(length))
            except (TypeError, ValueError, AttributeError):
                self.send_error(400)
                return
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(response)))
            self.end_headers()
            self.wfile.write(response)

    ca = issuer.ca_path.resolve(strict=True)
    tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    tls.load_cert_chain(ca.with_name("fixture-server.pem"), ca.with_name("fixture-server.key"))
    with LoopbackHTTPServer(("127.0.0.1", 0), Handler) as server:
        server.socket = tls.wrap_socket(server.socket, server_side=True)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            yield server.server_port
        finally:
            server.shutdown()
            thread.join()

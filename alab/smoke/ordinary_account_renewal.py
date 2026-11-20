#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Verify separate and overlapping ChatGPT credential renewal using synthetic slots on macOS."""

import argparse
from concurrent.futures import ThreadPoolExecutor
from contextlib import ExitStack, contextmanager
import hashlib
import http.server
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import time
from urllib.parse import urlsplit

import capacity_exec as probe
from live_account_quota import read_identity, read_quota
from package_standing import summary
from provider_fixture import LoopbackHTTPServer, usage
from quota_resume import Rpc
from renewal_follow import access_token, fake_auth, fixture_tls, identity_claims, image_digest, token_part


class Slot:
    def __init__(self, root, name, expiry_seconds, issuer, *, account=None, weekly_used=10, usage_allowed=True):
        self.name = name
        self.account = account or f"account-{name}"
        self.email = f"{name}@example.invalid"
        self.user = f"user-{name}"
        self.weekly_used = weekly_used
        self.usage_allowed = usage_allowed
        self.path = root / name / "auth.json"
        self.expiry_seconds = expiry_seconds
        self.expires = int(time.time()) + expiry_seconds
        self.exchanges = self.usage_reads = 0
        self.new_access = access_token("renewed-" + name, self.account, email=self.email, user=self.user)
        self.new_refresh = f"fixture-renewed-{name}"
        fake_auth(self.path, name, issuer, self.account, email=self.email, user=self.user)
        auth = json.loads(self.path.read_text())
        self.id_token = auth["tokens"]["id_token"]
        claims = dict(identity_claims(self.account, self.email, self.user), exp=self.expires)
        auth["tokens"]["access_token"] = token_part({"alg": "none"}) + "." + token_part(claims) + ".sig"
        self.path.write_text(json.dumps(auth))

    def exchange(self, payload):
        probe.require(isinstance(payload, dict) and payload.get("grant_type") == "refresh_token"
                      and payload.get("client_id") == "fixture-client"
                      and payload.get("refresh_token") == "fixture-refresh-" + self.name
                      and payload.get("refresh_reason") == "proactive_expiry",
                      "unexpected grant, credential, or refresh reason")
        remaining = self.expires - time.time()
        probe.require((0 < remaining <= 300) if self.expiry_seconds > 0 else (remaining < 0),
                      "the access token was outside its intended expiry window at exchange")
        probe.require(self.exchanges == 0, "repeated exchange for one slot")
        self.exchanges += 1
        return {"access_token": self.new_access, "refresh_token": self.new_refresh, "id_token": self.id_token}

    def usage(self):
        self.usage_reads += 1
        return usage(self.account, self.user, week_used=self.weekly_used, allowed=self.usage_allowed)

    def check_saved_credential(self):
        tokens = json.loads(self.path.read_text())["tokens"]
        probe.require(tokens["access_token"] == self.new_access and tokens["refresh_token"] == self.new_refresh
                      and tokens["id_token"] == self.id_token and tokens["account_id"] == self.account,
                      "saved credential differs from the issuer response")


def fingerprint(path):
    stat = path.stat()
    return (stat.st_dev, stat.st_ino, stat.st_mtime_ns, hashlib.sha256(path.read_bytes()).digest())


def check_first_write(slot, before):
    persisted = fingerprint(slot.path)
    probe.require(persisted[:2] != before[:2] and persisted[3] != before[3],
                  "first use did not atomically replace the selected credential")
    slot.check_saved_credential()
    probe.require(slot.exchanges == 1 and slot.usage_reads == 1, "first use did not renew and use exactly once")
    return persisted


def check_slot(slot, sibling, read):
    before, untouched = fingerprint(slot.path), fingerprint(sibling.path)
    read(slot)
    persisted = check_first_write(slot, before)
    probe.require(fingerprint(sibling.path) == untouched, "renewing one slot changed the sibling")
    read(slot)
    probe.require(fingerprint(slot.path) == persisted and fingerprint(sibling.path) == untouched,
                  "reopening changed a saved credential")
    probe.require(slot.exchanges == 1 and slot.usage_reads == 2, "reopen did not reuse the renewed credential")
    return {"slot": slot.name, "initialAccessExpirySeconds": slot.expiry_seconds,
            "issuerExchanges": slot.exchanges, "usageReadsWithIssuedCredential": slot.usage_reads,
            "siblingUnchanged": True, "reopenReusedSavedCredential": True}


class PendingExchanges:
    """Both distinct issuers and the controller meet before either response can leave."""
    def __init__(self, names, timeout=15):
        self.replies = {name: threading.Event() for name in names}
        probe.require(len(self.replies) == 2, "overlap requires two distinct slots")
        self.peers = threading.Barrier(3, timeout=timeout)
        self.timeout = timeout
        self.admission = threading.Lock()
        self.arrived = set()
        self.aborted = threading.Event()
        self.confirmed = False

    def rendezvous(self):
        try:
            self.peers.wait()
        except threading.BrokenBarrierError:
            raise RuntimeError("two distinct issuer requests did not overlap") from None

    def hold(self, name):
        with self.admission:
            probe.require(name in self.replies and name not in self.arrived, "unexpected or duplicate pending slot")
            self.arrived.add(name)
        self.rendezvous()
        probe.require(self.replies[name].wait(self.timeout) and not self.aborted.is_set(),
                      "issuer did not receive its release")

    def wait_until_pending(self):
        self.rendezvous()
        self.confirmed = True

    def release(self, name):
        probe.require(self.confirmed and not self.aborted.is_set(), "cannot release an unconfirmed exchange")
        self.replies[name].set()

    def abort(self):
        self.aborted.set()
        self.peers.abort()
        for reply in self.replies.values():
            reply.set()


def check_overlapping_slots(slots, session_for, overlap):
    first, second = slots
    originals = {slot: fingerprint(slot.path) for slot in slots}
    with ExitStack() as live, ThreadPoolExecutor(max_workers=2) as workers:
        try:
            clients = {slot: live.enter_context(session_for(slot)) for slot in slots}
            reads = {slot: workers.submit(start_slot, slot, clients[slot]) for slot in slots}
            overlap.wait_until_pending()
            probe.require(all(fingerprint(slot.path) == originals[slot] for slot in slots),
                          "credentials changed before either issuer replied")
            overlap.release(first.name)
            reads[first].result(timeout=overlap.timeout)
            first_saved = check_first_write(first, originals[first])
            probe.require(fingerprint(second.path) == originals[second], "first renewal changed the held sibling")
            overlap.release(second.name)
            reads[second].result(timeout=overlap.timeout)
            second_saved = check_first_write(second, originals[second])
            probe.require(fingerprint(first.path) == first_saved, "second renewal changed the saved sibling")
            for slot in slots:
                check_rpc_identity(slot, clients[slot])
                probe.require(slot.exchanges == 1 and slot.usage_reads == 2,
                              "original app server did not reuse the renewed credential")
        finally:
            overlap.abort()
    reopened_quota = {}
    for slot in reversed(slots):
        with session_for(slot) as reopened:
            reopened_quota[slot] = start_slot(slot, reopened)
        probe.require(fingerprint(first.path) == first_saved and fingerprint(second.path) == second_saved,
                      "reopening changed a saved credential")
        probe.require(slot.exchanges == 1 and slot.usage_reads == 3, "reopen did not reuse the renewed credential")
    return {"bothIssuerRequestsPending": overlap.confirmed, "releaseOrder": [first.name, second.name],
            "slots": [{"slot": slot.name, "issuerExchanges": slot.exchanges,
                       "usageReadsWithIssuedCredential": slot.usage_reads,
                       **reopened_quota[slot]} for slot in slots],
            "siblingUnchangedAtEachWrite": True, "bothOriginalSessionsRechecked": True,
            "reverseReopenReusedSavedCredentials": True}


class Server(LoopbackHTTPServer):
    """One issuer routes each request by the credential presented to it."""
    def register_slots(self, slots, overlap=None):
        self.grants = {"fixture-refresh-" + slot.name: slot for slot in slots}
        self.bearers = {"Bearer " + slot.new_access: slot for slot in slots}
        probe.require(len(self.grants) == len(self.bearers) == len(slots), "duplicate fixture credentials")
        self.validation_lock = threading.Lock()
        self.overlap = overlap
        self.fault = False

    def reject(self):
        self.fault = True
        if self.overlap is not None:
            self.overlap.abort()

    def authenticated_slot(self, headers):
        bearers = headers.get_all("Authorization", [])
        slot = self.bearers.get(bearers[0]) if len(bearers) == 1 else None
        probe.require(slot is not None and slot.exchanges == 1
                      and headers.get_all("ChatGPT-Account-Id", []) == [slot.account],
                      "request did not present one issued bearer and its matching account")
        return slot


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def send_error(self, *args):
        self.server.reject()
        super().send_error(*args)

    def respond(self):
        route = urlsplit(self.path).path
        try:
            if self.command == "GET":
                slot = self.server.authenticated_slot(self.headers)
            if self.command == "POST" and route == "/oauth/token":
                length = int(self.headers.get("content-length", "0"))
                probe.require(0 < length <= 16384, "invalid exchange length")
                payload = json.loads(self.rfile.read(length))
                probe.require(isinstance(payload, dict), "invalid exchange")
                with self.server.validation_lock:
                    slot = self.server.grants[payload["refresh_token"]]
                    response = slot.exchange(payload)
                overlap = self.server.overlap
                if overlap is not None and slot.name in overlap.replies:
                    overlap.hold(slot.name)
            elif self.command == "GET" and route == "/backend-api/wham/usage":
                with self.server.validation_lock:
                    response = slot.usage()
            elif self.command == "GET" and route == "/backend-api/wham/accounts/check":
                response = {"accounts": [{"id": slot.account, "name": slot.name, "plan_type": "pro",
                             "can_access_with_session": True, "is_deactivated": False,
                             "workspace_backend_origin": "NO_CONSTRAINT", "account_routing_override": "NO_CONSTRAINT"}],
                            "account_ordering": [slot.account], "default_account_id": slot.account}
            elif self.command == "GET" and route == "/v1/models":
                response = {"models": []}
            else:
                raise RuntimeError("unexpected fixture endpoint")
            status = 200
        except (RuntimeError, ValueError, TypeError, KeyError):
            self.server.reject()
            response = {"error": "fixture rejected request"}
            status = 400
        body = json.dumps(response).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    do_GET = do_POST = respond


def sandbox_prefix(port, env):
    # Permit the fixture only. Exercise the deny rule against another *listening*
    # loopback port so ECONNREFUSED cannot pass for a policy denial.
    profile = f'(version 1)(allow default)(deny network-outbound)(allow network-outbound (remote ip "localhost:{port}"))'
    prefix = ["/usr/bin/sandbox-exec", "-p", profile]
    with socket.socket() as other:
        other.bind(("127.0.0.1", 0))
        other.listen()
        control = ("import errno,socket,sys\n"
                   "with socket.socket() as s:\n"
                   " s.settimeout(2)\n"
                   " code=s.connect_ex(('127.0.0.1',int(sys.argv[1])))\n"
                   "sys.exit(0 if code in (errno.EPERM,errno.EACCES) else 1)")
        subprocess.run([*prefix, sys.executable, "-IS", "-c", control, str(other.getsockname()[1])],
                       env=env, check=True, timeout=10)
    return prefix


def check_rpc_identity(slot, rpc):
    identity = read_identity(rpc, slot.email)
    quota = read_quota(rpc, identity)
    probe.require(quota.account_id == slot.account and quota.row["plan"] == "pro", "wrong RPC account or plan")
    weekly = [w["usedPercent"] for w in quota.row["windows"] if w["windowDurationMins"] == 10080]
    probe.require(weekly == [slot.weekly_used], "wrong RPC weekly usage")
    allowed = quota.row["ordinaryUsageAllowed"]
    probe.require(allowed is slot.usage_allowed and not quota.row.get("ordinaryUsageConflict"),
                  "wrong RPC ordinary usage permission")
    return {"verifiedWeeklyUsedPercent": weekly[0], "verifiedOrdinaryUsageAllowed": allowed}


def start_slot(slot, rpc):
    rpc.initialize()
    probe.require(slot.exchanges == 1, "ordinary startup did not complete the renewal")
    slot.check_saved_credential()
    return check_rpc_identity(slot, rpc)


@contextmanager
def open_slot(slot, command, root, env):
    with open(os.devnull, "wb") as sink, subprocess.Popen(
            command, cwd=root / "work", env=dict(env, CODEX_AUTH_PATH=str(slot.path)),
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=sink) as process:
        try:
            yield Rpc(process, sink)
        finally:
            try:
                process.communicate(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.communicate()
        probe.require(process.returncode == 0, "app server did not shut down cleanly")
        probe.require(not (root / "codex-home/auth.json").exists(), "credential leaked into shared storage")


def exercise(executable, root):
    env = probe.fresh_environment(root)
    with Server(("127.0.0.1", 0), Handler) as server:
        server.socket = fixture_tls(root).wrap_socket(server.socket, server_side=True)
        issuer = f"https://127.0.0.1:{server.server_port}"
        env.update(CODEX_CA_CERTIFICATE=str(root / "fixture-ca.pem"), CODEX_REFRESH_TOKEN_URL_OVERRIDE=issuer + "/oauth/token")
        (root / "codex-home/config.toml").write_text(
            f'cli_auth_credentials_store = "file"\nchatgpt_base_url = "{issuer}/backend-api"\n'
            f'openai_base_url = "{issuer}/v1"\nfeatures.daemon_auto_start = false\n'
            'analytics.enabled = false\ncheck_for_update_on_startup = false\n')
        command = [*sandbox_prefix(server.server_port, env), str(executable), "app-server", "--stdio"]
        near = Slot(root, "near-expiry", 120, issuer)
        expired = Slot(root, "expired", -600, issuer)
        pair = (Slot(root, "overlap-near-expiry", 120, issuer, weekly_used=17),
                Slot(root, "overlap-expired", -600, issuer, weekly_used=100, usage_allowed=False))
        overlap = PendingExchanges(slot.name for slot in pair)
        server.register_slots((near, expired, *pair), overlap)
        thread = threading.Thread(target=server.serve_forever)
        thread.start()
        try:
            def session_for(slot):
                return open_slot(slot, command, root, env)

            def read(slot):
                with session_for(slot) as rpc:
                    start_slot(slot, rpc)
                probe.require(not server.fault, "loopback fixture encountered a rejected request")

            separate = [check_slot(near, expired, read), check_slot(expired, near, read)]
            concurrent = check_overlapping_slots(pair, session_for, overlap)
        finally:
            overlap.abort()
            server.shutdown()
            thread.join()
        probe.require(not server.fault and all((slot.exchanges, slot.usage_reads) == (1, 2)
                                              for slot in (near, expired)), "unexpected final request counts")
        probe.require(all((slot.exchanges, slot.usage_reads) == (1, 3) for slot in pair),
                      "unexpected final overlap request counts")
        concurrent.update(sharedIssuer=True, sharedOAuthClient=True)
        return {"slots": separate, "overlap": concurrent}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="new directory for result.json")
    args = parser.parse_args()
    if sys.platform != "darwin" or not os.access("/usr/bin/sandbox-exec", os.X_OK):
        parser.error("this qualification requires macOS with sandbox-exec for loopback isolation")
    package = args.package.resolve(strict=True)
    build, manifest, executable = probe.package_records(package)
    args.output.mkdir(mode=0o700, parents=True, exist_ok=False)
    report = {"passed": False, "source": build["source"], "version": manifest["version"],
              "package": summary(build), "executableSha256": image_digest(executable)[1]}
    try:
        with tempfile.TemporaryDirectory() as scratch:
            report.update(exercise(executable, Path(scratch).resolve()))
        probe.require(image_digest(executable)[1] == report["executableSha256"], "executable changed")
        report["passed"] = True
    except Exception as error:
        report["errorType"] = type(error).__name__
        raise
    finally:
        probe.write_json(args.output / "result.json", report)


if __name__ == "__main__":
    main()

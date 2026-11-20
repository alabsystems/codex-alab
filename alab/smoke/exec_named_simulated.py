#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Run the debug exec integration test under FakeAterm and a loopback model, with synthetic logins."""
import argparse
import json
import os
from pathlib import Path
import pty
import re
import secrets
import select
import signal
import tempfile
import time
from contextlib import ExitStack

import capacity_exec as probe
import exec_accounts
import exec_named_limit as named
import native as terminal
import renewal_follow as fixture


LIMIT = fixture.provider_fixture.GOLDEN_MODEL_LIMIT
STEPS = ["prepare", "arm", "reserve", "bind-tree", "execute", "load", "consume", "complete"]
PREFIX = "OK codex-exec-handoff "
UUID = re.compile(r"[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}")


class OrdinaryWindowsModel(exec_accounts.Model):
    """The ordinary source becomes spent only when the provider refuses it."""
    def week_spent(self, old_token, new_token, next_token=False):
        return old_token and self.source_walled

    def limit_headers(self, generation, wall=False, usage=None):
        return fixture.Model.limit_headers(self, generation, wall, usage)


def recorded_windows(root, thread, limit):
    windows = []
    for event in fixture.thread_rollout(root, thread):
        payload = event.get("payload", {})
        if event.get("type") != "event_msg" or payload.get("type") != "token_count":
            continue
        rates = payload.get("rate_limits") or {}
        if rates.get("limit_id") != limit:
            continue
        pair = [rates.get("primary") or {}, rates.get("secondary") or {}]
        if [window.get("window_minutes") for window in pair] == [300, 10080]:
            values = [window.get("used_percent") for window in pair]
            probe.require(all(type(value) in (int, float) for value in values), "invalid saved quota percentage")
            windows.append(values)
    return windows


class SimulatedMove:
    """A single scripted grant. The client drives each transition by its actual wire request."""
    def __init__(self, root, model):
        self.auth = root / "auth-new/auth.json"
        self.model = model
        self.steps = []
        self.thread = self.turn = self.checkpoint = None
        self.ticket = {"id": secrets.token_hex(16), "cap": secrets.token_hex(32)}
        self.source = fixture.fixture_renewal_identity("old")
        self.target = fixture.fixture_renewal_identity("new")
        self.deadline = None

    def answer(self, line, sid, *, wait=False):
        selector, protocol, verb, *words = line.split()
        probe.require((selector, protocol) == (f"@{sid}", "codex-exec-handoff"), "unexpected simulated request")
        if verb in ("info", "tree-info"):
            probe.require(not words, "unexpected contract fields")
            return PREFIX + ("contract=3" if verb == "info" else "tree-contract=1")
        fields = dict(word.split("=", 1) for word in words)
        probe.require(len(fields) == len(words), "duplicate simulated request field")
        if verb == "cancel":
            probe.require("arm" in self.steps and fields == self.ticket, "cancel for another simulated grant")
            self.steps.append(verb)
            return PREFIX + "state=cancelled"
        probe.require(len(self.steps) < len(STEPS) and verb == STEPS[len(self.steps)],
                      f"unexpected simulated exec stage {verb} after {self.steps}")
        source = {"source_account": self.source["account"], "source_user": self.source["user"]}
        target = {"target_account": self.target["account"], "target_user": self.target["user"]}
        scope = {} if self.model.limit == "codex" else {"limit": self.model.limit}
        if verb == "prepare":
            self.thread, self.turn = fields["thread"], fields["turn"]
            expected = {"thread": self.thread, "turn": self.turn, **source, **scope}
            probe.require(fields == expected and UUID.fullmatch(self.thread) and UUID.fullmatch(self.turn),
                          "prepare did not bind the original identity and quota scope")
            with self.model.lock:
                wall = self.model.requests[1]
                probe.require(self.model.source_walled and wall["requestIdentity"] == {
                    "thread_id": self.thread, "turn_id": self.turn} and
                    wall["responseHeaders"].get("x-codex-active-limit") == self.model.limit, "prepare did not follow the provider wall")
            if wait:
                return PREFIX + "state=waiting"
            answer = "state=selected " + self.identities(source, target)
        elif verb == "arm":
            self.checkpoint = fields.pop("checkpoint")
            checkpoint = json.loads(bytes.fromhex(self.checkpoint))
            probe.require(fields == {"thread": self.thread, "turn": self.turn, **source, **target, **scope} and
                          (checkpoint.get("thread"), checkpoint.get("turn")) == (self.thread, self.turn) and
                          (checkpoint.get("limit") == self.model.limit if scope else "limit" not in checkpoint),
                          "arm or checkpoint changed the quota selection")
            self.deadline = fixture.monotonic_us() + 90_000_000
            answer = self.grant("armed", source, target)
        elif verb == "bind-tree":
            manifest = json.loads(bytes.fromhex(fields.pop("manifest")))
            probe.require(fields == self.ticket and manifest.get("root") == self.thread and manifest.get("turn") == self.turn
                          and manifest.get("members") == [{"thread": self.thread, "parent": None, "turn": self.turn}]
                          and re.fullmatch(r"[0-9a-f]{64}", manifest["digest"]), "tree differs from the original turn")
            answer = "state=tree-bound digest=" + manifest["digest"]
        else:
            probe.require(fields == self.ticket, "successor used another simulated ticket")
            if verb in ("reserve", "execute"):
                answer = f"state={'reserved' if verb == 'reserve' else 'execute'} deadline_us={self.deadline}"
            elif verb in ("load", "consume"):
                answer = self.grant("loaded" if verb == "load" else "consumed", source, target)
            else:
                answer = "state=completed"
        self.steps.append(verb)
        return PREFIX + answer

    def identities(self, source, target):
        fields = {"thread": self.thread, "turn": self.turn, **source, **target}
        return " ".join(f"{k}={v}" for k, v in fields.items())

    def grant(self, state, source, target):
        return (f"state={state} id={self.ticket['id']} cap={self.ticket['cap']} deadline_us={self.deadline} "
                f"{self.identities(source, target)} auth={os.fsencode(self.auth).hex()} checkpoint={self.checkpoint}")


def run_turn(root, env, codex, host, model, report, *, restore_policy=False):
    move = SimulatedMove(root, model)
    pid, master = pty.fork()
    if pid == 0:
        try:
            os.chdir(root / "work")
            os.execve(codex, [str(codex), "exec", "--json", "--color", "never", "--skip-git-repo-check",
                             "--dangerously-bypass-approvals-and-sandbox", probe.PROMPT], env)
        finally:
            os._exit(127)
    status = None
    index = 0
    status_report = report["simulatedHost"]
    image_observations = {}
    held_prepare = None
    disabled_until = None
    policy_reads = 0
    policy_request = f"@{host.sid} harness policy"
    try:
        probe.require(host.cmd(f"leader {pid}") == "OK", "simulated peer failed to set the fixture leader")
        with (root / "pty.log").open("xb") as log:
            deadline = time.monotonic() + 150
            while time.monotonic() < deadline:
                fixture.require_no_fixture_fault(model)
                if disabled_until is not None:
                    state = host.state()
                    if state["claim"] is None and state["requests"].count(policy_request) >= policy_reads + 2:
                        probe.require(host.cmd("policy codex_rotation 1") == "OK", "could not restore rotation")
                        disabled_until = None
                    else:
                        probe.require(time.monotonic() < disabled_until,
                                      "the reused claim did not release and recheck policy for the same quota wait")
                if select.select([master], [], [], .03)[0]:
                    try:
                        data = os.read(master, 65536)
                    except OSError:
                        data = b""
                    log.write(data)
                    log.flush()
                line = host.cmd(f"exec-next {index}")
                probe.require(line.startswith("REQUEST "), "simulated host cannot observe the exec protocol")
                request = json.loads(line.removeprefix("REQUEST "))
                if request:
                    verb = request.split(" ", 3)[2]
                    wait = restore_policy and verb == "prepare" and held_prepare is None
                    if wait:
                        held_prepare = request
                        policy_reads = host.state()["requests"].count(policy_request)
                        probe.require(host.cmd("policy codex_rotation 0") == "OK", "could not suspend rotation")
                        disabled_until = time.monotonic() + 20
                    elif restore_policy and verb == "prepare":
                        probe.require(request == held_prepare and disabled_until is None,
                                      "exec prepared another request or kept its disabled claim")
                    reply = move.answer(request, host.sid, wait=wait)
                    if verb in ("arm", "consume"):
                        image = terminal.exe_of(pid)
                        probe.require(Path(image).resolve() == codex and os.getpgid(pid) == pid,
                                      "the original foreground process no longer runs the supplied test image")
                        image_observations[verb] = pid
                    probe.require(host.cmd(f"exec-reply {index} {json.dumps(reply)}") == "OK", "simulated reply was refused")
                    index += 1
                done, waited = os.waitpid(pid, os.WNOHANG)
                if done:
                    status = waited
                    break
            probe.require(status is not None, "the loopback exec did not finish within 150 seconds")
            # The child can exit between a PTY read and waitpid. Drain its final output.
            while select.select([master], [], [], .1)[0]:
                try:
                    data = os.read(master, 65536)
                except OSError:
                    break
                if not data:
                    break
                log.write(data)
        code = os.waitstatus_to_exitcode(status)
        probe.require(code == 0, f"exec test returned {code}")
        probe.require(move.steps == STEPS and image_observations == {"arm": pid, "consume": pid},
                      "the same process did not finish every simulated handoff stage")
        evidence = named.summary(model, root)
        provider = {key: evidence[key] for key in ("providerCalls", "foregroundModels", "requestIdentities", "refusalHeaders")}
        named.verify_provider(root, move.thread, move.turn, model.requests, provider)
        windows = recorded_windows(root, move.thread, model.limit)
        probe.require(len(windows) >= 3 and windows[0] == windows[-1] == [0, 0] and [0, 100] in windows[1:-1],
                      "the rollout lost the source, refused or resumed numeric quota windows")
        report["recordedUsagePercent"] = windows
        events = [json.loads(line) for line in (root / "pty.log").read_text(errors="replace").splitlines()
                  if line.startswith("{")]
        types = [e.get("type") for e in events]
        probe.require(all(types.count(key) == 1 for key in ("thread.started", "turn.started", "turn.completed"))
                      and "turn.failed" not in types and not exec_accounts.stream_problems(events),
                      "the exec output did not complete the original turn and items exactly once")
        report.update(originalThread=move.thread, originalTurn=move.turn, originalInputAndCompletedPlanPreserved=True)
        if restore_policy:
            probe.require(held_prepare is not None and disabled_until is None, "policy transition was not observed")
            report["sameWallPolicyReadmission"] = True
        report["passed"] = True
    finally:
        status_report.update(stages=move.steps, foregroundPidAtStage=image_observations)
        evidence = named.summary(model, root)
        report["provider"] = {key: evidence[key] for key in ("providerCalls", "foregroundModels", "requestIdentities", "refusalHeaders")}
        if status is None:
            try:
                os.killpg(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            os.waitpid(pid, 0)
        os.close(master)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--test-driver", type=Path, required=True)
    parser.add_argument("--test-driver-source", required=True)
    parser.add_argument("--test-driver-sha256", required=True)
    parser.add_argument("--fake-aterm", type=Path, required=True, help="test-support fake_aterm example")
    parser.add_argument("--limit", choices=(LIMIT, "codex"), default=LIMIT,
                        help="test the named handoff or its ordinary weekly control with numeric windows")
    parser.add_argument("--restore-policy", action="store_true",
                        help="disable then restore rotation while the same named request waits")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.restore_policy and args.limit == "codex":
        parser.error("--restore-policy requires the named limit")
    codex, report = named.test_driver(args)
    fake = args.fake_aterm.resolve(strict=True)
    root = args.output.resolve()
    root.mkdir(mode=0o700, exist_ok=False)
    env = probe.fresh_environment(root)
    named.require_declaration(codex, report["testDriver"], env, root)
    report.update(passed=False, limit=args.limit, simulatedHost={"kind": "FakeAterm", "sha256": fixture.image_digest(fake)[1]})
    try:
        with ExitStack() as stack:
            model = OrdinaryWindowsModel(root) if args.limit == "codex" else exec_accounts.Model(root, limit=args.limit)
            server, worker = fixture.serve(model)
            stack.callback(server.server_close)
            stack.callback(worker.join)
            stack.callback(server.shutdown)
            base = f"https://127.0.0.1:{server.server_port}"
            probe.configure(root, server.server_port)
            config = root / "codex-home/config.toml"
            config.write_text(f'cli_auth_credentials_store = "file"\nchatgpt_base_url = "{base}"\n' +
                              exec_accounts.exec_provider_config(config))
            source, target = root / "auth-old/auth.json", root / "auth-new/auth.json"
            fixture.fake_auth(source, "old", base, "source-account")
            fixture.fake_auth(target, "new", base, "target-account", email="target@example.invalid", user="target-user")
            # Keep the macOS rendezvous under the Unix-domain socket path limit.
            short = Path(stack.enter_context(tempfile.TemporaryDirectory(dir="/private/tmp/.ubd", prefix="fx-")))
            env.update(XDG_RUNTIME_DIR=str(short), XDG_CONFIG_HOME=str(root / "config"), TERM="xterm-256color",
                       CODEX_AUTH_PATH=str(source), CODEX_CA_CERTIFICATE=str(root / "fixture-ca.pem"),
                       RUST_LOG="codex_alab_exec_handoff=debug")
            host = terminal.Fake(fake, short / "aterm", env=env)
            stack.callback(host.close)
            policy = [("accounts", "wall"), ("codex_rotation", "1")]
            if args.limit != "codex":
                policy.extend([("codex_rotation_limits", "1"), ("codex_exec_rotation_limits", "1")])
            for key, value in policy:
                probe.require(host.cmd(f"policy {key} {value}") == "OK", f"simulated {key} policy unavailable")
            probe.require(host.cmd("watch-exec") == "OK", "this fake host cannot hold exec requests")
            env["ATERM_PARENT_SESSION_ID"] = host.sid
            run_turn(root, env, codex, host, model, report, restore_policy=args.restore_policy)
    except Exception as error:
        report["failure"] = str(error)
        raise
    finally:
        probe.write_json(root / "result.json", report)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""A REAL smoke of codex-alab's native harness: the codex binary under a pty, a scratch
CODEX_HOME, a mock Responses server, and the FAKE aterm (alab-aterm's `fake_aterm` example)
as its aterm session. Never a real aterm, never ~/.codex.

1. Two copies of one codex build (`cp -c`: two inodes), a twin symlink naming copy A, and
   codex launched as `codex` through PATH (its launch path is the twin). With
   --next-codex, use two distinct package executables and verify version/byte
   identities instead; retain their package-relative resources.
2. The session's Autopilot word is Land (FOREVER LAND, aterm owner direction 2026-10-19:
   "if landing LAND EVERYTHING INTO REMOTE MAIN NO EXCEPTIONS"; under Cruise the task never
   ends, so nothing would leave the re-exec quiet). The fake proves codex-alab's claim; a
   person's first turn asks a `request_user_input` question; the TUI's overlay is answered
   by codex-alab under Land with Land's text in its notes (`answer-land@v1`; this fake sends
   no `land_question_text`, so codex-alab's copy), and the model's next request carries that
   answer (it reached core); codex-alab then types the land instruction (2 min after the
   person's turn, which worked under min_work: the short streak's back-off), the reply defends
   `Nothing left:` with no need, the repository probe finds nothing (the work folder is no
   repository), and the task is landed (`landed@v1`, never typed).
3. The twin is re-pointed at copy B: codex-alab leaves like /exit and re-execs onto the
   twin, KEEPING ITS PID; the new image's record says execGeneration 1; its claim is proven
   again; nothing more is typed (the landing crossed the re-exec in the record's `owed`).

usage: native.py --codex target/debug/codex --fake target/debug/examples/fake_aterm
"""

import argparse
import ctypes  # noqa: F401  (exe_of)
import http.server
import hashlib
import json
import os
import pty
import select
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import fcntl
import threading
import time

import provider_fixture

# Words only the land instruction and its nudge carry (alab-autopilot `words::LAND`,
# `words::LAND_NUDGE`).
LAND_WORDS = ("Autopilot is Land", "Still landing")
# Words any act codex-alab types carries: the continuation, Cruise's defend prompt, Land's.
TYPED_WORDS = ("keep going", "Autopilot is", "Still landing", "Nobody is here to choose")

QUESTION = {
    "questions": [
        {
            "id": "cache",
            "header": "Cache",
            "question": "Where should the cache live?",
            "options": [
                {"label": "On disk", "description": "Survives restarts."},
                {"label": "In memory (Recommended)", "description": "Fastest."},
            ],
        }
    ]
}


def sse(events):
    out = []
    for ev in events:
        out.append("event: %s\ndata: %s\n\n" % (ev["type"], json.dumps(ev)))
    return "".join(out)


def created(rid):
    return {"type": "response.created", "response": {"id": rid}}


def completed(rid):
    return {
        "type": "response.completed",
        "response": {
            "id": rid,
            "usage": {
                "input_tokens": 0,
                "input_tokens_details": None,
                "output_tokens": 0,
                "output_tokens_details": None,
                "total_tokens": 0,
            },
        },
    }


def message(mid, text):
    return {
        "type": "response.output_item.done",
        "item": {
            "type": "message",
            "role": "assistant",
            "id": mid,
            "content": [{"type": "output_text", "text": text}],
        },
    }


def function_call(call_id, name, arguments):
    return {
        "type": "response.output_item.done",
        "item": {
            "type": "function_call",
            "call_id": call_id,
            "name": name,
            "arguments": arguments,
        },
    }


class Mock:
    """The model: the person's turn asks the question; its answer reports the work; the land
    instruction gets a clean `Nothing left:` defense."""

    def __init__(self):
        self.requests = []
        self.answers = []
        self.asked = False
        self.lock = threading.Lock()

    def reply(self, body):
        """By what the request carries, not by its number (codex also sends side requests,
        such as a thread title's)."""
        items = body.get("input", []) if isinstance(body, dict) else []
        landing = any(word in json.dumps(items[-1:]) for word in LAND_WORDS)
        answered = any(
            i.get("type") == "function_call_output" and i.get("call_id") == "call_q1"
            for i in items
        )
        asked = any(
            i.get("type") == "message" and i.get("role") == "user"
            and "start the cache work" in json.dumps(i.get("content"))
            for i in items
        )
        with self.lock:
            self.requests.append(body)
            n = len(self.requests)
            if answered:
                self.answers.append(body)
        if landing:
            # A clean defense (alab-autopilot's words_tests read this one as clean).
            return sse([
                created("r%d" % n),
                message("m%d" % n, "The cache lives in memory now.\nNothing left: the cache "
                        "work is verified, and there is no repository here to push.\n"
                        "NEEDS YOU: nothing"),
                completed("r%d" % n),
            ])
        if answered:
            return sse([
                created("r%d" % n),
                message("m%d" % n, "The cache lives in memory now."),
                completed("r%d" % n),
            ])
        if asked and not self.asked:
            self.asked = True
            return sse([
                created("r%d" % n),
                function_call("call_q1", "request_user_input", json.dumps(QUESTION)),
                completed("r%d" % n),
            ])
        return sse([created("r%d" % n), message("m%d" % n, "ok"), completed("r%d" % n)])


def serve(mock):
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_GET(self):
            body = b'{"models":[]}' if "models" in self.path else b"{}"
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self):
            length = int(self.headers.get("content-length", "0"))
            raw = self.rfile.read(length)
            if not self.path.endswith("/responses"):
                self.send_response(404)
                self.end_headers()
                return
            try:
                body = json.loads(raw)
            except ValueError:
                body = {}
            out = mock.reply(body).encode()
            self.send_response(200)
            self.send_header("content-type", "text/event-stream")
            self.send_header("content-length", str(len(out)))
            self.end_headers()
            self.wfile.write(out)

    server = provider_fixture.LoopbackHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


class Fake:
    def __init__(self, exe, rendezvous):
        self.proc = subprocess.Popen(
            [exe, rendezvous], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True
        )
        line = self.proc.stdout.readline().strip()
        assert line.startswith("SID "), line
        self.sid = line[4:]

    def cmd(self, line):
        self.proc.stdin.write(line + "\n")
        self.proc.stdin.flush()
        return self.proc.stdout.readline().strip()

    def state(self):
        line = self.cmd("dump")
        assert line.startswith("STATE "), line
        return json.loads(line[6:])

    def close(self):
        try:
            self.cmd("quit")
        except (BrokenPipeError, ValueError):
            pass
        self.proc.wait(timeout=10)


class Pty:
    """The codex process on a pty; answers the terminal queries a TUI sends at start."""

    def __init__(self, argv, env, cwd, log):
        pid, fd = pty.fork()
        if pid == 0:
            os.chdir(cwd)
            os.execvpe(argv[0], argv, env)
        self.pid, self.fd = pid, fd
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
        self.log = open(log, "wb")
        self.screen = b""
        self.alive = True
        threading.Thread(target=self.pump, daemon=True).start()

    def pump(self):
        while self.alive:
            try:
                ready, _, _ = select.select([self.fd], [], [], 0.2)
            except (OSError, ValueError):
                return
            if not ready:
                continue
            try:
                data = os.read(self.fd, 65536)
            except OSError:
                self.alive = False
                return
            if not data:
                self.alive = False
                return
            self.log.write(data)
            self.log.flush()
            self.screen = (self.screen + data)[-200000:]
            if b"\x1b[6n" in data:
                os.write(self.fd, b"\x1b[1;1R")
            if b"\x1b[c" in data or b"\x1b[0c" in data:
                os.write(self.fd, b"\x1b[?62;22c")
            if b"\x1b[?u" in data:
                os.write(self.fd, b"\x1b[?0u")

    def type(self, text):
        for ch in text:
            os.write(self.fd, ch.encode())
            time.sleep(0.03)


def exe_of(pid):
    """The file the process runs (the kernel's vnode path: symlinks resolved), or ""."""
    import ctypes
    import ctypes.util

    libproc = ctypes.CDLL(ctypes.util.find_library("proc") or "/usr/lib/libproc.dylib")
    buf = ctypes.create_string_buffer(4096)
    n = libproc.proc_pidpath(pid, buf, ctypes.c_uint32(4096))
    return buf.value.decode() if n > 0 else ""


def wait(what, done, timeout=90):
    """A hang detector, never a latency budget."""
    deadline = time.time() + timeout
    while True:
        value = done()
        if value:
            return value
        if time.time() > deadline:
            raise SystemExit("SMOKE FAILED: never happened: %s" % what)
        time.sleep(0.2)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--codex", required=True)
    ap.add_argument("--next-codex", help="a distinct next-version package executable for actual version-transition verification")
    ap.add_argument("--starting-question-mode", choices=("land", "recommended"), default="land",
                    help="expected starting-version answer contract; recommended requires --next-codex")
    ap.add_argument("--fake", required=True)
    ap.add_argument("--scratch")
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    if args.starting_question_mode == "recommended" and not args.next_codex:
        ap.error("--starting-question-mode recommended requires a distinct --next-codex")
    scratch = args.scratch or tempfile.mkdtemp(prefix="alab-smoke-")
    os.makedirs(scratch, exist_ok=True)
    print("scratch:", scratch)
    home = os.path.join(scratch, "codex-home")
    work = os.path.join(scratch, "work")
    xdg = os.path.join(scratch, "xdg")
    if len(xdg) > 60:
        # The control socket's path must fit a sockaddr_un (104 bytes on macOS).
        xdg = tempfile.mkdtemp(prefix="alab-x-")
    twin_dir = os.path.join(scratch, "agents")
    a = os.path.join(scratch, "store", "a", "bin", "codex")
    b = os.path.join(scratch, "store", "b", "bin", "codex")
    for d in (home, work, xdg, twin_dir, os.path.dirname(a), os.path.dirname(b)):
        os.makedirs(d, exist_ok=True)
    # Preserve package-relative resources for a real next-version executable.
    # Without --next-codex, retain the original two-inode same-build probe.
    if not os.path.lexists(a):
        os.symlink(os.path.realpath(args.codex), a)
    if not os.path.lexists(b):
        if args.next_codex:
            os.symlink(os.path.realpath(args.next_codex), b)
        else:
            subprocess.run(["cp", "-c", args.codex, b], check=True)
    a = os.path.realpath(a)
    b = os.path.realpath(b)
    def image(path):
        with open(path, "rb") as f:
            digest = hashlib.file_digest(f, "sha256").hexdigest()
        version = subprocess.check_output(
            [path, "--version"], text=True, env={**os.environ, "CODEX_HOME": home}
        ).strip()
        return {"path": path, "sha256": digest, "version": version}
    before_image, next_image = image(a), image(b)
    if args.next_codex:
        assert before_image["sha256"] != next_image["sha256"], "next-version bytes must differ"
        assert before_image["version"] != next_image["version"], "next-version identity must differ"
    assert os.stat(a).st_ino != os.stat(b).st_ino
    twin = os.path.join(twin_dir, "codex")
    if os.path.lexists(twin):
        os.remove(twin)
    os.symlink(a, twin)
    work = os.path.realpath(work)

    mock = Mock()
    server = serve(mock)
    port = server.server_address[1]
    with open(os.path.join(home, "config.toml"), "w") as f:
        f.write(
            'model = "gpt-5"\n'
            'model_provider = "mock"\n'
            'approval_policy = "never"\n'
            'sandbox_mode = "read-only"\n'
            "[model_providers.mock]\n"
            'name = "mock"\n'
            'base_url = "http://127.0.0.1:%d/v1"\n'
            'wire_api = "responses"\n'
            "requires_openai_auth = false\n"
            "stream_max_retries = 0\n"
            "request_max_retries = 0\n"
            '[projects."%s"]\n'
            'trust_level = "trusted"\n' % (port, work)
        )

    fake = Fake(args.fake, os.path.join(xdg, "aterm"))
    print("fake aterm sid:", fake.sid)
    assert fake.cmd("policy human_grace_s 1") == "OK"
    assert fake.cmd("status autopilot land") == "OK"

    env = {
        "PATH": twin_dir + ":/usr/bin:/bin",
        "HOME": os.environ["HOME"],
        "USER": os.environ.get("USER", ""),
        "TERM": "xterm-256color",
        "CODEX_HOME": home,
        "XDG_RUNTIME_DIR": xdg,
        "ATERM_PARENT_SESSION_ID": fake.sid,
        "RUST_LOG": "codex_alab_autopilot=debug",
    }
    argv = ["codex", "--enable", "default_mode_request_user_input"]
    term = Pty(argv, env, work, os.path.join(scratch, "pty.log"))
    pid = term.pid
    assert fake.cmd("leader %d" % pid) == "OK"
    wait("the child execs codex", lambda: "codex" in os.path.basename(exe_of(pid)))
    print("codex pid:", pid, "running", exe_of(pid))
    try:
        assert os.path.realpath(exe_of(pid)) == os.path.realpath(a), exe_of(pid)
        wait("the claim is proven", lambda: (fake.state()["claim"] or {}).get("proven_pid") == pid)
        print("1. claim proven:", fake.state()["claim"])
        time.sleep(1.5)
        term.type("start the cache work")
        time.sleep(0.3)
        os.write(term.fd, b"\r")
        wait("the model gets the answer", lambda: mock.answers)
        outputs = [
            item for item in mock.answers[0].get("input", [])
            if item.get("type") == "function_call_output" and item.get("call_id") == "call_q1"
        ]
        print("2. the question's answer reached core and the model:", outputs[0].get("output"))
        if args.starting_question_mode == "recommended":
            # The published 0.2 starting image predates Land-specific question answers.
            # Verify its exact supported contract before exercising the handoff.
            assert json.loads(outputs[0]["output"]) == {
                "answers": {"cache": {"answers": ["In memory (Recommended)"]}}
            }, outputs[0]
            expected_answer_rule = "answer-recommended@v1"
        else:
            # alab-autopilot `words::LAND_QUESTION_TEXT`: the current-image control.
            assert "take the option that lands all of this work on remote main" in json.dumps(outputs[0]), outputs[0]
            expected_answer_rule = "answer-land@v1"
        # The person's turn worked under codex-alab's min_work (2 min), so the land instruction
        # that answers it is paced by the short streak, as aterm paces every act: it goes 2 min
        # after that turn ends (decide_tests a_persons_short_turn_backs_the_continue_off). The
        # clean defense it gets then lands at once. A hang detector, so room past the 2 min.
        wait("the task is landed", lambda: "landed@v1" in fake.state()["acts"], timeout=300)
        acts = fake.state()["acts"]
        print("   acts:", acts)
        assert expected_answer_rule in acts, acts
        assert "land@v1" in acts, acts

        record_path = os.path.join(home, "alab", "sessions", "%d.json" % pid)
        wait("the record has a resumeArgv", lambda: os.path.exists(record_path)
             and json.load(open(record_path)).get("resumeArgv"))
        record = json.load(open(record_path))
        print("3. record before:", {k: record[k] for k in ("execGeneration", "launchArgv0", "exe", "resumeArgv", "status")})
        before_session = record["sessionId"]
        before_requests = len(mock.requests)

        nxt = twin + ".next"
        os.symlink(b, nxt)
        os.rename(nxt, twin)
        print("   twin re-pointed at", os.path.realpath(twin))
        wait("the process re-execs onto copy B",
             lambda: os.path.realpath(exe_of(pid) or "/nonexistent") == os.path.realpath(b), timeout=120)
        print("4. same pid", pid, "now runs", exe_of(pid))
        assert "upgrade@v1" in fake.state()["acts"], fake.state()["acts"]
        wait("the new image displays the resumed conversation (execGeneration 1)", lambda: os.path.exists(record_path)
             and json.load(open(record_path)).get("execGeneration") == 1
             and json.load(open(record_path)).get("status") != "upgrading"
             and json.load(open(record_path)).get("displayedThreadId") == before_session)
        record = json.load(open(record_path))
        assert record["sessionId"] == before_session, "the conversation changed during live update"
        assert record["displayedThreadId"] == before_session, "the resumed conversation is not displayed"
        print("   record after:", {k: record[k] for k in ("execGeneration", "status", "sessionId", "exe")})
        wait("the new image's claim is proven", lambda: (fake.state()["claim"] or {}).get("proven_pid") == pid)
        print("5. claim proven again:", fake.state()["claim"])
        time.sleep(3)
        typed = [
            r for r in mock.requests[before_requests:]
            if any(word in json.dumps(r.get("input", [])[-1:]) for word in TYPED_WORDS)
        ]
        assert not typed, "nothing is typed after the re-exec (a landed task)"
        print("   nothing typed after the re-exec;", len(mock.requests) - before_requests, "side request(s)")
        receipt = {
            "schema": "alab.native-live-update.v1",
            "startingQuestionMode": args.starting_question_mode,
            "startingAnswerRule": expected_answer_rule,
            "before": before_image, "next": next_image,
            "pid": pid, "loadedExecutable": exe_of(pid),
            "sessionIdBefore": before_session, "sessionIdAfter": record["sessionId"],
            "execGeneration": record["execGeneration"],
            "claimProven": fake.state()["claim"]["proven_pid"] == pid,
            "noPostLandingTyping": not typed,
            "distinctVersion": before_image["version"] != next_image["version"],
        }
        assert image(a) == before_image and image(b) == next_image, "package bytes changed during probe"
        with open(os.path.join(scratch, "live-update-receipt.json"), "w") as f:
            json.dump(receipt, f, indent=2)
            f.write("\n")
        print("SMOKE OK")
    finally:
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        term.alive = False
        try:
            os.waitpid(pid, 0)
        except ChildProcessError:
            pass
        fake.close()
        server.shutdown()
        if not args.keep:
            shutil.rmtree(scratch, ignore_errors=True)


if __name__ == "__main__":
    main()

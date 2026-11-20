# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Drive image attachment, submission, transcript scrolling and local session replay.

A public localImage item refers to its source path. An unavailable file keeps
its numbered label. A public inline image has durable bytes for its own item
and supplies the original thumbnail pixels when the thread is reopened.
"""

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import struct
import subprocess
import sys
import tempfile
import time
import unittest
import zlib


PROMPT = "Compare the two color samples."
ROWS = [f"Swatch row {n:03}." for n in range(1, 64)]
DEADLINE = 16
COLORS = [(240, 16, 8), (12, 32, 232)]


def solid_png(color):
    width = height = 32

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    pixels = (b"\0" + bytes(color) * width) * height
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(pixels)) + chunk(b"IEND", b""))


def painted_swatches(fragment):
    """Read the top-left RGB pixel of each complete, direct Kitty PNG draw."""
    colors = []
    for encoded in re.findall(rb"\x1b_Ga=T,t=d,f=100,[^;]*,m=0,[^;]*;([A-Za-z0-9+/=]+)\x1b", fragment):
        png = base64.b64decode(encoded, validate=True)
        position, compressed = 8, bytearray()
        while position + 12 <= len(png):
            size, kind = struct.unpack_from(">I4s", png, position)
            data = png[position + 8:position + size + 8]
            if kind == b"IHDR":
                # Upstream encodes terminal thumbnails as eight-bit RGB(A).
                if data[8:10] not in (b"\x08\x02", b"\x08\x06"):
                    break
            elif kind == b"IDAT":
                compressed.extend(data)
            elif kind == b"IEND":
                # The predictor of the first pixel in the first row is zero for
                # all five PNG filters. Only those three bytes are required.
                first = zlib.decompressobj().decompress(compressed, 4)
                color = tuple(first[1:4])
                if color in COLORS:
                    colors.append(color)
                break
            position += size + 12
    return colors


def startup_ready(raw):
    # The terminal title names the repository when the fixture runs inside one.
    # A provisional Ready can precede Starting. Require the subsequent Ready
    # for the same project in complete OSC title sequences.
    project = None
    for match in re.finditer(rb"\x1b\]0;(Starting|Ready) \| ([^\x00-\x1f\x7f]+)(?:\x07|\x1b\\)", raw):
        state, name = match.groups()
        if state == b"Starting":
            project = name
        elif name == project:
            return True
    return False


def exercise(binary, workspace, root):
    smoke = workspace / "alab/smoke"
    sys.path.insert(0, str(smoke))
    import capacity_exec as fixture
    import model_resume
    from quota_resume import Rpc

    fixture.require(hasattr(os, "waitid") and hasattr(os, "WNOWAIT"), "PTY smoke requires os.waitid and WNOWAIT")
    root.mkdir(mode=0o700)
    env = fixture.fresh_environment(root)
    env.update(TERM="xterm-256color", TERM_PROGRAM="xterm", KITTY_WINDOW_ID="1",
               HTTP_PROXY="http://127.0.0.1:1", HTTPS_PROXY="http://127.0.0.1:1",
               NO_PROXY="localhost,127.0.0.1,::1")
    images = [root / "work" / name for name in ("red-swatch.png", "blue-swatch.png")]
    for path, color in zip(images, COLORS):
        path.write_bytes(solid_png(color))
    expected = [hashlib.sha256(path.read_bytes()).hexdigest() for path in images]
    report = {"passed": False, "binary_sha256": model_resume.CHECK["sha256"](binary),
              "turns": [], "views": {}, "source_png_sha256": expected}

    class Backend(model_resume.Gateway):
        def response(self, body, headers):
            messages = body.get("input", [])
            users = [item["content"] for item in messages if item.get("role") == "user"]
            contents = [c for c in users if any(PROMPT in part.get("text", "") for part in c)]
            text = "Color samples"
            if contents:
                fixture.require(len(contents) == 1, "unexpected second prompt")
                urls = [part["image_url"] for part in contents[0] if part.get("type") == "input_image"]
                hashes = [hashlib.sha256(base64.b64decode(url.removeprefix("data:image/png;base64,"),
                                                          validate=True)).hexdigest() for url in urls]
                with self.lock:
                    report["turns"].append({"thread": headers.get("thread-id"),
                                            "turn": json.loads(headers.get("x-codex-turn-metadata", "{}")).get("turn_id"),
                                            "png_sha256": hashes})
                text = "\n\n".join(ROWS)
            return fixture.response("swatches", {"type": "message", "role": "assistant", "id": "swatches",
                                    "content": [{"type": "output_text", "text": text}]})

    def terminal(label, args, *, want_pixels=True):
        raw = root / (label + ".raw")
        spec = root / "tmp" / (label + ".json")
        custody = root / (label + "-custody.json")
        cli = [str(binary), *args]
        spec.write_text(json.dumps({"argv": cli, "cwd": str(root / "work"), "env": env,
                                    "raw": str(raw), "result": str(custody)}))

        def interact(process, log):
            command_id = 0

            def send(keys):
                nonlocal command_id
                command_id += 1
                data = {"id": command_id, "write": base64.b64encode(keys).decode()}
                process.stdin.write((json.dumps(data) + "\n").encode())
                process.stdin.flush()

            def wait_for_paint(matches, description):
                end = time.monotonic() + DEADLINE
                while time.monotonic() < end and process.poll() is None:
                    if raw.is_file() and matches(raw.read_bytes()):
                        return
                    time.sleep(.04)
                raise RuntimeError(f"{label}: terminal did not paint {description}")

            def displayed(text, after=0):
                # Ratatui can move the cursor over an unchanged space instead
                # of writing it during repaint. Match the emitted text runs.
                needle = b"".join(text.encode().split())

                def matches(data):
                    runs = re.sub(rb"\x1b\[[0-?]*[ -/]*[@-~]", b"", data[after:])
                    return needle in b"".join(runs.split())

                wait_for_paint(matches, repr(text))

            def paste(text, marker):
                start = raw.stat().st_size
                send(b"\x1b[200~" + text.encode() + b"\x1b[201~")
                displayed(marker, start)

            if label == "submitted":
                displayed(model_resume.OLD)
                # Startup paints a provisional composer before the thread owns
                # its input. Wait for the terminal's completed startup transition.
                wait_for_paint(startup_ready, "a Starting then Ready terminal title for the same project")
                paste(shlex.quote(str(images[0])), "[Image #1]")
                paste(shlex.quote(str(images[1])), "[Image #2]")
                paste(" " + PROMPT, "samples.")
                send(b"\r")
            displayed(ROWS[-1])
            # Scroll the primary transcript from the completed answer to its user turn.
            top_offset = raw.stat().st_size
            colors = []
            for _ in range(8):
                send(b"\x1b[5~")
                time.sleep(.10)
                colors = painted_swatches(raw.read_bytes()[top_offset:])
                if want_pixels and all(color in colors for color in COLORS):
                    break
            if want_pixels:
                fixture.require(all(color in colors for color in COLORS),
                                f"{label}: scrolling failed to render the two PNG colors: {colors}")
            else:
                # The item still exists in the transcript even if the protocol
                # provides only local paths whose files have since gone away.
                for marker in ("[Image #1]", "[Image #2]", PROMPT):
                    displayed(marker, top_offset)
            report["views"][label] = {"scrolled_thumbnail_pixels": colors}
            for _ in range(8):
                send(b"\x1b[6~")
                time.sleep(.09)
            tail = raw.stat().st_size
            command_id += 1
            process.stdin.write((json.dumps({"id": command_id, "resize": [32, 90]}) + "\n").encode())
            process.stdin.flush()
            displayed(ROWS[-1], tail)
            for seconds in (1, 12):
                send(b"\x03")
                try:
                    process.wait(timeout=seconds)
                    break
                except subprocess.TimeoutExpired:
                    if seconds == 12:
                        raise

        args = ["-I", "-B", "-S", str(smoke / "history_pagination.py"), "--guardian", str(spec)]
        fixture.require(fixture.run(sys.executable, args, root, env, label, interact=interact) == 0,
                        label + " PTY exited abnormally")
        model_resume.CHECK["check_model_terminal_exit"](json.loads(custody.read_text()), cli)
        if not want_pixels:
            draw_count = raw.read_bytes().count(b"\x1b_Ga=T,")
            fixture.require(draw_count == 0, f"{label}: emitted graphics for unavailable local images")
            report["views"][label]["numbered_labels"] = ["[Image #1]", "[Image #2]"]
            report["views"][label]["kitty_draw_count"] = draw_count

    listener = None
    try:
        listener = fixture.start_listener(Backend([]).handler())
        model_resume.configure(root, listener[0].server_port, "high")
        (root / "codex-home/auth.json").write_text(json.dumps({"OPENAI_API_KEY": "sk-loopback-fixture"}))
        terminal("submitted", [])
        fixture.require(len(report["turns"]) == 1 and report["turns"][0]["png_sha256"] == expected,
                        "one submitted turn must contain both source PNGs in paste order")
        thread = report["turns"][0]["thread"]
        fixture.require(thread and report["turns"][0]["turn"], "missing provider thread/turn identity")
        terminal("present", ["resume", thread])
        for path in images:
            path.unlink()
        terminal("resumed", ["resume", thread], want_pixels=False)
        fixture.require(len(report["turns"]) == 1, "resume resubmitted the image turn")

        def read_public_item(process, log):
            rpc = Rpc(process, log)
            rpc.initialize()
            saved = rpc.request("thread/read", {"threadId": thread, "includeTurns": True})["thread"]
            fixture.require(saved["id"] == thread and len(saved["turns"]) == 1, "unexpected public turn history")
            messages = [item for item in saved["turns"][0]["items"] if item["type"] == "userMessage"]
            fixture.require(len(messages) == 1, "unexpected count of public user messages")
            content = messages[0]["content"]
            fixture.require([item["type"] for item in content] == ["localImage", "localImage", "text"]
                            and [Path(item["path"]) for item in content[:2]] == images,
                            "unexpected image references in replayed local user message")
            report["public_item"] = {"id": messages[0]["id"], "image_references": ["localImage", "localImage"]}
            fixture.require(not report["views"]["resumed"]["scrolled_thumbnail_pixels"],
                            "the terminal drew obsolete local files in a path-only message")

        fixture.require(fixture.run(binary, ["app-server", "--listen", "stdio://"], root, env,
                                    "public-item", interact=read_public_item) == 0, "public read failed")
        def seed_inline(process, log):
            rpc = Rpc(process, log)
            rpc.initialize()
            inline_thread = rpc.request("thread/start", {"cwd": str(root / "work")})["thread"]["id"]
            fixture.require(inline_thread != thread, "inline control reused the path-backed thread")
            urls = ["data:image/png;base64," + base64.b64encode(solid_png(color)).decode() for color in COLORS]
            inputs = [{"type": "image", "url": url} for url in urls]
            inputs.append({"type": "text", "text": PROMPT, "text_elements": []})
            started = rpc.request("turn/start", {"threadId": inline_thread, "input": inputs})["turn"]["id"]
            complete = rpc.wait(lambda msg: msg.get("method") == "turn/completed"
                                and msg["params"]["threadId"] == inline_thread
                                and msg["params"]["turn"]["id"] == started)
            fixture.require(complete["params"]["turn"]["status"] == "completed", "inline control turn failed")
            saved = rpc.request("thread/read", {"threadId": inline_thread, "includeTurns": True})["thread"]
            fixture.require(len(saved["turns"]) == 1 and saved["turns"][0]["id"] == started,
                            "unexpected public inline turn history")
            messages = [item for item in saved["turns"][0]["items"] if item["type"] == "userMessage"]
            fixture.require(len(messages) == 1, "unexpected public inline message count")
            content = messages[0]["content"]
            fixture.require([item["type"] for item in content] == ["image", "image", "text"]
                            and [item.get("url") for item in content[:2]] == urls,
                            "inline references changed in the saved public message")
            report["inline_item"] = {"thread": inline_thread, "turn": started, "id": messages[0]["id"]}

        fixture.require(fixture.run(binary, ["app-server", "--listen", "stdio://"], root, env,
                                    "inline-control", interact=seed_inline) == 0, "inline control failed")
        inline = report["inline_item"]
        terminal("inline", ["resume", inline["thread"]])
        fixture.require(len(report["turns"]) == 2 and report["turns"][1] == {
            "thread": inline["thread"], "turn": inline["turn"], "png_sha256": expected,
        }, "inline resume changed the submitted images or generated an extra turn")
        fixture.require(not any(path.exists() for path in images), "source images unexpectedly returned")
        fixture.require(model_resume.CHECK["sha256"](binary) == report["binary_sha256"], "binary changed")
        report["passed"] = True
    except Exception as error:
        report.update(fixture.case_error_fields(error))
    finally:
        fixture.cleanup_resources((("gateway", *listener),) if listener else (), (), report, sys.exc_info()[1])
        fixture.write_json(root / "result.json", report)
    fixture.require(report["passed"], report.get("error", "image fixture failed"))


class NativeUI(unittest.TestCase):
    def check_side_chat(self, name, *, resume):
        workspace = Path(os.environ["ALAB_FEATURE_WORKSPACE"]).resolve(strict=True)
        binary = Path(os.environ["ALAB_FEATURE_BINARY"]).resolve(strict=True)
        output = Path(os.environ["ALAB_FEATURE_RESULTS"]).resolve() / name
        # The executable under test creates the saved parent and child through the public TUI.
        seed = ["--seed-binary", binary] if resume else []
        subprocess.run([sys.executable, "-E", "-s", "-B", workspace / "alab/smoke/sidebar_account_focus.py",
                        "--codex", binary, "--output", output, *seed], check=True)

    def check_queued_refusal(self, name, *flags):
        binary = Path(os.environ["ALAB_FEATURE_BINARY"]).resolve(strict=True)
        root = Path(os.environ["ALAB_FEATURE_RESULTS"]).resolve() / name
        subprocess.run([sys.executable, "-I", "-B", Path(__file__).with_name("queued_refusal.py"),
                        "--codex", binary, "--output", root, *flags], check=True)

    def test_passive_refusal_holds_queued_input_until_new_user_input(self):
        self.check_queued_refusal("queued-refusal")

    def test_interactive_refusal_review_sends_neither_queue_nor_continuation(self):
        self.check_queued_refusal("interactive-queued-refusal", "--interactive-review")

    def test_fresh_side_chat_keeps_drafts_on_their_own_threads(self):
        self.check_side_chat("side-chat-fresh", resume=False)

    def test_resumed_side_chat_keeps_drafts_on_their_own_threads(self):
        self.check_side_chat("side-chat-resumed", resume=True)

    def test_pasted_images_survive_scroll_and_resume(self):
        workspace = Path(os.environ["ALAB_FEATURE_WORKSPACE"]).resolve(strict=True)
        binary = Path(os.environ["ALAB_FEATURE_BINARY"]).resolve(strict=True)
        root = Path(os.environ["ALAB_FEATURE_RESULTS"]).resolve() / "pasted-images"
        exercise(binary, workspace, root)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--workspace", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    output = args.output.resolve() if args.output else Path(tempfile.mkdtemp(prefix="codex-image-smoke-")) / "result"
    exercise(args.binary.resolve(strict=True), args.workspace.resolve(strict=True), output)
    print(output / "result.json")

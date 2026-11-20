#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise fresh model discovery, image support, expired catalogs and account denial on a CLI."""

import argparse
import base64
import http.server
import json
from pathlib import Path
import sys
from urllib.parse import urlsplit

import capacity_exec as fixture
from model_resume import CHECK, configure, model


TEXT = "readiness-fixture-orchid"
VISION = "readiness-fixture-iris"
PROMPT = "Acknowledge this model readiness check."
ANSWER = "Readiness request received."
DENIAL = "This fixture account has no access to the requested model."
PIXEL = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII="


class Service:
    def __init__(self):
        self.vision_released = False
        self.events = []

    def handler(self):
        service = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def send_body(self, status, kind, data):
                self.send_response(status)
                self.send_header("Content-Type", kind)
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def account(self):
                return {"Bearer sk-readiness-allowed": "eligible",
                        "Bearer sk-readiness-denied": "unavailable"}.get(self.headers.get("Authorization"))

            def reject(self):
                self.send_body(403, "application/json", json.dumps(
                    {"error": {"code": "permission_denied", "message": DENIAL}}).encode())

            def do_GET(self):
                if urlsplit(self.path).path != "/v1/models":
                    self.send_error(404)
                    return
                account = self.account()
                entries = [dict(model(TEXT), input_modalities=["text"])]
                if service.vision_released:
                    entries.append(dict(model(VISION), input_modalities=["text", "image"]))
                service.events.append({"route": "models", "account": account, "status": 200 if account == "eligible" else 403})
                if account != "eligible":
                    self.reject()
                    return
                self.send_body(200, "application/json", json.dumps({"models": entries}).encode())

            def do_POST(self):
                count = int(self.headers.get("Content-Length", "0"))
                if self.path != "/v1/responses" or not 0 < count <= 8 * 1024 * 1024:
                    self.send_error(400)
                    return
                body = json.loads(self.rfile.read(count))
                content = [part for item in body["input"] if item.get("role") == "user"
                           for part in item.get("content", []) if isinstance(part, dict)]
                images = [part["image_url"] for part in content if part.get("type") == "input_image"]
                account = self.account()
                service.events.append({"route": "responses", "account": account, "model": body["model"],
                                       "effort": (body.get("reasoning") or {}).get("effort"),
                                       "prompt": any(p.get("text") == PROMPT for p in content),
                                       "images": images,
                                       "image_omitted": any("image content omitted" in p.get("text", "") for p in content),
                                       "status": 200 if account == "eligible" else 403})
                if account != "eligible":
                    self.reject()
                    return
                item = {"type": "message", "id": "answer", "role": "assistant",
                        "content": [{"type": "output_text", "text": ANSWER}]}
                self.send_body(200, "text/event-stream", fixture.response("readiness-turn", item))

        return Handler


def catalog(binary, home, env, label, *extra):
    code = fixture.run(binary, ["debug", "models", *extra], home, env, label)
    fixture.require(code == 0, f"{label} returned {code}")
    rows = json.loads((home / f"{label}-stdout.jsonl").read_text())["models"]
    return {row["slug"]: row for row in rows}


def inference(binary, home, env, service, label, slug, account, *, image=False):
    before = len(service.events)
    args = ["exec", "--json", "--ephemeral", "--skip-git-repo-check", "--model", slug]
    if image:
        args.extend(["--image", str(home / "pixel.png")])
    code = fixture.run(binary, [*args, "--", PROMPT], home, env, label)
    requests = [e for e in service.events[before:] if e["route"] == "responses"]
    fixture.require(len(requests) == 1, f"{label} made {len(requests)} inference attempts")
    request = requests[0]
    fixture.require(request["account"] == account and request["model"] == slug and request["prompt"],
                    f"{label} changed the account, model or prompt")
    output = [json.loads(line) for line in (home / f"{label}-stdout.jsonl").read_text().splitlines()]
    if account == "unavailable":
        failures = [event["error"]["message"] for event in output if event["type"] == "turn.failed"]
        fixture.require(code != 0 and request["status"] == 403 and len(failures) == 1 and DENIAL in failures[0],
                        "the denied account failed to surface its provider refusal")
    else:
        answers = [event["item"].get("text") for event in output if event["type"] == "item.completed"
                   and event["item"]["type"] == "agent_message"]
        fixture.require(code == 0 and output[-1]["type"] == "turn.completed" and answers == [ANSWER]
                        and request["effort"] == "high", f"{label} did not complete with the selected effort")
    return request


def run(binary, root):
    root.mkdir(mode=0o700)
    service = Service()
    report = {"binary_sha256": CHECK["sha256"](binary), "events": service.events, "passed": False}
    listener = None
    try:
        listener = fixture.start_listener(service.handler())
        homes = {}
        for account, key in [("eligible", "allowed"), ("unavailable", "denied")]:
            home = root / account
            home.mkdir()
            env = fixture.fresh_environment(home)
            configure(home, listener[0].server_port, "high", model_preference=TEXT)
            (home / "codex-home/auth.json").write_text(json.dumps({"OPENAI_API_KEY": f"sk-readiness-{key}"}))
            (home / "pixel.png").write_bytes(base64.b64decode(PIXEL))
            homes[account] = home, env

        home, env = homes["eligible"]
        bundled = catalog(binary, home, env, "bundled", "--bundled")
        fixture.require(TEXT not in bundled and VISION not in bundled and not service.events,
                        "bundled discovery contacted the service or already contains a fixture model")
        fresh = catalog(binary, home, env, "fresh")
        fixture.require(TEXT in fresh and VISION not in fresh, "fresh catalog did not expose the first release")
        inference(binary, home, env, service, "catalog-only", TEXT, "eligible")

        caches = []
        for path in (home / "codex-home").glob("models_cache*.json"):
            entry = json.loads(path.read_text())
            if any(row["slug"] == TEXT for row in entry["models"]):
                entry["fetched_at"] = "2000-01-01T00:00:00Z"
                path.write_text(json.dumps(entry))
                caches.append(path.name)
        fixture.require(caches, "the real client did not persist its initial catalog")
        report["expired_caches"] = caches
        service.vision_released = True
        cut = len(service.events)
        updated = catalog(binary, home, env, "revalidated")
        fixture.require(any(e["route"] == "models" and e["status"] == 200 for e in service.events[cut:]),
                        "the expired catalog was not revalidated")
        fixture.require(updated[VISION]["input_modalities"] == ["text", "image"],
                        "the newly advertised model did not expose image inputs")
        photo = inference(binary, home, env, service, "image-capable", VISION, "eligible", image=True)
        text = inference(binary, home, env, service, "explicit-text-model", TEXT, "eligible", image=True)
        fixture.require(photo["images"] == ["data:image/png;base64," + PIXEL] and not photo["image_omitted"]
                        and not text["images"] and text["image_omitted"],
                        "the outgoing requests did not distinguish image-capable and text-only models")

        home, env = homes["unavailable"]
        cut = len(service.events)
        inaccessible = catalog(binary, home, env, "denied-catalog")
        fixture.require(VISION not in inaccessible and any(e["account"] == "unavailable" and e["status"] == 403
                        for e in service.events[cut:]), "the denied account received the eligible catalog")
        inference(binary, home, env, service, "denied-inference", VISION, "unavailable")
        fixture.require(CHECK["sha256"](binary) == report["binary_sha256"], "the executable changed during the probe")
        report["passed"] = True
    except Exception as error:
        report.update(fixture.case_error_fields(error))
    finally:
        fixture.cleanup_resources((("catalog", *listener),) if listener else (), (), report, sys.exc_info()[1])
        fixture.write_json(root / "result.json", report)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    sys.exit(run(args.codex.resolve(strict=True), args.output.absolute()))

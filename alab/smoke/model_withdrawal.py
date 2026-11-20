#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise serving-catalog withdrawal against real, persisted app-server threads."""

import argparse
import http.server
import json
from pathlib import Path
import sys
from urllib.parse import urlsplit

import capacity_exec as fixture
import model_resume
from quota_resume import Rpc
from renewal_follow import fixture_tls
import weekly_reset


SAVED = "gpt-6.1-sol"
NEXT = "gpt-6.2-sol"
FOLLOWING = "gpt-6.3-sol"
FOREIGN = "gpt-99-sol"
UNLINKED = "gpt-88-sol"
ALTERNATE = "gpt-6.2-astra"


def serving_rows(stage, custom):
    if stage == "seed":
        return [model_resume.model(SAVED)]
    if stage == "withdrawn":
        return [model_resume.model(UNLINKED if custom else ALTERNATE)]
    if stage == "following":
        successor = model_resume.model(FOLLOWING)
        successor["multi_agent_reasoning_effort"] = "xhigh"
        if custom:
            return [model_resume.model(NEXT, FOLLOWING), successor, model_resume.model(UNLINKED)]
        return [successor]
    rows = [model_resume.model(NEXT)]
    if stage == "low":
        rows[0].update(supported_reasoning_levels=[{"effort": "low", "description": "Low only"}],
                       multi_agent_reasoning_effort="low")
    else:
        rows[0]["multi_agent_reasoning_effort"] = "xhigh"
    if custom:
        rows.extend([model_resume.model(SAVED, NEXT), model_resume.model(UNLINKED)])
    return rows


class Provider:
    def __init__(self, custom, record):
        self.custom = custom
        self.events = record["events"]
        self.stage = "seed"
        self.url = None
        self.token = None

    def handler(self):
        provider = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def reply(self, content_type, body):
                self.send_response(200)
                self.send_header("Content-Type", content_type)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def do_GET(self):
                path = urlsplit(self.path).path
                if path.endswith("/wham/accounts/check"):
                    account = {"id": weekly_reset.ACCOUNT, "name": "Loopback account",
                               "workspace_backend_origin": provider.url, "account_routing_override": "NO_CONSTRAINT"}
                    value = {"accounts": [account], "default_account_id": weekly_reset.ACCOUNT}
                elif path.endswith("/wham/config/bundle"):
                    value = {}
                elif path in ("/backend-api/codex/models", "/v1/models", "/foreign/models"):
                    rows = [model_resume.model(FOREIGN)] if path == "/foreign/models" else serving_rows(
                        provider.stage, provider.custom)
                    provider.events.append({"type": "catalog", "stage": provider.stage, "path": path,
                                            "models": [m["slug"] for m in rows],
                                            "authenticated": self.headers.get("Authorization") == "Bearer " + provider.token})
                    value = {"models": rows}
                elif path.endswith("/responses"):
                    self.send_error(426)
                    return
                else:
                    self.send_error(404)
                    return
                self.reply("application/json", json.dumps(value).encode())

            def do_POST(self):
                path = urlsplit(self.path).path
                size = int(self.headers.get("Content-Length", 0))
                if not path.endswith("/responses") or not 0 < size <= 8 * 1024 * 1024:
                    self.send_error(400)
                    return
                body = json.loads(self.rfile.read(size))
                effort = (body.get("reasoning") or {}).get("effort")
                overrides = [item["reasoning"]["effort"] for item in body["input"]
                             if item.get("type") == "configuration_update" and (item.get("reasoning") or {}).get("effort")]
                users = model_resume.messages(body, "user")
                provider.events.append({"type": "request", "stage": provider.stage, "path": path,
                                        "thread": self.headers.get("thread-id"), "model": body["model"],
                                        "authenticated": self.headers.get("Authorization") == "Bearer " + provider.token,
                                        "account": self.headers.get("ChatGPT-Account-Id"),
                                        "wire_effort": effort, "effective_effort": overrides[-1] if overrides else effort,
                                        "has_seed": model_resume.SEED in users,
                                        "has_reply": model_resume.REPLY in model_resume.messages(body, "assistant")})
                text = model_resume.REPLY if provider.stage == "seed" else model_resume.ANSWER
                item = {"type": "message", "id": provider.stage, "role": "assistant",
                        "content": [{"type": "output_text", "text": text}]}
                self.reply("text/event-stream", fixture.response(provider.stage, item))

        return Handler


def run_case(binary, root, custom, record):
    root.mkdir(mode=0o700)
    env = fixture.fresh_environment(root)
    provider = Provider(custom, record)
    listener = None
    try:
        tls = fixture_tls(root)

        def secure(server):
            server.socket = tls.wrap_socket(server.socket, server_side=True)

        listener = fixture.start_listener(provider.handler(), prepare=secure)
        port = listener[0].server_port
        url = provider.url = f"https://127.0.0.1:{port}"
        env.update(CODEX_CA_CERTIFICATE=str(root / "fixture-ca.pem"), SSL_CERT_FILE=str(root / "fixture-ca.pem"),
                   HTTPS_PROXY=url, HTTP_PROXY=url, NO_PROXY="127.0.0.1,localhost")
        if custom:
            provider.token = "sk-withdrawal-fixture"
            fixture.write_json(root / "codex-home/auth.json", {"OPENAI_API_KEY": provider.token})
        else:
            provider.token = weekly_reset.configure(root, port)

        def configure(effort):
            provider_config = 'model_provider = "gateway"\n' if custom else ""
            (root / "codex-home/config.toml").write_text(provider_config + f'''model = "{SAVED}"
model_reasoning_effort = "{effort}"
openai_base_url = "{url}/backend-api/codex"
chatgpt_base_url = "{url}/backend-api"
features.daemon_auto_start = false
features.enable_request_compression = false
features.api_key_model_discovery = true
analytics.enabled = false
check_for_update_on_startup = false
approval_policy = "never"
sandbox_mode = "read-only"
[projects.{json.dumps(str(root / "work"))}]
trust_level = "trusted"
[model_providers.gateway]
name = "Serving gateway"
base_url = "{url}/v1"
model_catalog_url = "{url}/v1/models"
requires_openai_auth = true
supports_websockets = false
[model_providers.foreign]
name = "Other serving namespace"
base_url = "{url}/foreign"
model_catalog_url = "{url}/foreign/models"
requires_openai_auth = true
supports_websockets = false
''')

        def app(label, interact):
            def connected(process, log):
                rpc = Rpc(process, log)
                rpc.initialize()
                interact(rpc)

            status = fixture.run(binary, ["app-server", "--listen", "stdio://"], root, env, label, interact=connected)
            fixture.require(status == 0, f"{label} exited with {status}")

        def turn(rpc, stage, expected, effort):
            provider.stage = stage
            start = len(provider.events)
            try:
                response = rpc.turn(record["thread"], model_resume.SEED if stage == "seed" else f"Continue at {stage}.")
            except RuntimeError as error:
                if expected is not None or "currently offers no eligible" not in str(error):
                    raise
                response = {"status": "refused", "error": str(error)}
            events = provider.events[start:]
            requests = [event for event in events if event["type"] == "request"]
            record["turns"][stage] = {"status": response["status"], "requests": len(requests)}
            if expected is None:
                fixture.require(not requests and response["status"] in ("refused", "failed")
                                and "currently offers no eligible" in str(response.get("error")),
                                f"{stage} did not refuse locally: {response}, {requests}")
                return
            fixture.require(response["status"] == "completed" and len(requests) == 1,
                            f"{stage} did not send exactly one successful inference: {response}, {requests}")
            request = requests[0]
            path = "/v1/responses" if custom else "/backend-api/codex/responses"
            fixture.require(request["thread"] == record["thread"] and request["path"] == path
                            and request["authenticated"] and (custom or request["account"] == weekly_reset.ACCOUNT)
                            and request["model"] == expected and request["wire_effort"] == effort
                            and request["effective_effort"] == effort
                            and request["has_seed"] and (stage == "seed" or request["has_reply"]),
                            f"{stage} changed model, effort, authority or history: {request}")

        params = {"cwd": str(root / "work")}

        def seed(rpc):
            saved = rpc.request("thread/start", dict(params, model=SAVED, ephemeral=False))
            record["thread"] = saved["thread"]["id"]
            fixture.require(saved["reasoningEffort"] == "ultra", "seed lost its Ultra preference")
            turn(rpc, "seed", SAVED, "high")

        def resume(rpc, expected=NEXT):
            saved = rpc.request("thread/resume", dict(params, threadId=record["thread"]))
            fixture.require((saved["thread"]["id"], saved["model"], saved["reasoningEffort"]) ==
                            (record["thread"], expected, "ultra"), f"cold resume lost its saved settings: {saved['model']}")

        def live(rpc):
            resume(rpc)
            turn(rpc, "resume", NEXT, "xhigh")
            listing = rpc.request("model/list", {"modelProvider": "foreign"})
            fixture.require(FOREIGN in {row["model"] for row in listing["data"]}
                            and any(e.get("path") == "/foreign/models" for e in provider.events),
                            "foreign provider did not advertise the higher release")
            provider.stage = "withdrawn"
            listing = rpc.request("model/list", {})
            fixture.require((UNLINKED if custom else ALTERNATE) in {r["model"] for r in listing["data"]},
                            "serving picker did not observe withdrawal")
            turn(rpc, "withdrawn", None, None)
            # Reappear with limited effort. Retry directly, without refreshing the picker.
            turn(rpc, "low", NEXT, "low")

        configure("ultra")
        app("seed", seed)
        configure("low")
        provider.stage = "resume"
        app("live", live)
        provider.stage = "cold"

        def cold(rpc):
            resume(rpc)
            turn(rpc, "cold", NEXT, "xhigh")

        app("cold", cold)
        # The original model has left the catalog by the second release.
        provider.stage = "following"

        def following(rpc):
            resume(rpc, FOLLOWING)
            turn(rpc, "following", FOLLOWING, "xhigh")

        app("following", following)
        requests = [event for event in record["events"] if event["type"] == "request"]
        fixture.require([event["stage"] for event in requests] == ["seed", "resume", "low", "cold", "following"],
                        "unexpected extra inference outside the requested turns")
        serving_path = "/v1/models" if custom else "/backend-api/codex/models"
        for stage in ("seed", "resume", "withdrawn", "low", "cold", "following"):
            fixture.require(any(event["stage"] == stage and event["path"] == serving_path and event["authenticated"]
                                for event in record["events"]), f"{stage} used no fresh authenticated serving catalog")
        record["passed"] = True
    except Exception as error:
        record.update(fixture.case_error_fields(error))
    finally:
        fixture.cleanup_resources((("catalog", *listener),) if listener else (), (), record, sys.exc_info()[1])


def run(binary, output):
    output.mkdir(mode=0o700)
    digest = model_resume.CHECK["sha256"]
    report = {"passed": False, "binary_sha256": digest(binary), "cases": {}}
    try:
        for name in ("first_party", "custom"):
            case = {"passed": False, "events": [], "turns": {}}
            report["cases"][name] = case
            run_case(binary, output / name, name == "custom", case)
        fixture.require(all(case["passed"] for case in report["cases"].values()), "one or more provider cases failed")
        fixture.require(digest(binary) == report["binary_sha256"], "runtime executable changed")
        report["passed"] = True
    except Exception as error:
        report.update(fixture.case_error_fields(error))
    finally:
        fixture.write_json(output / "result.json", report)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    sys.exit(run(args.codex.resolve(strict=True), args.output.absolute()))

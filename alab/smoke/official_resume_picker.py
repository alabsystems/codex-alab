#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Measure an official Codex release's bare picker in B's other project after A's exec turn."""

import argparse
import json
from pathlib import Path
import sys
import time

import account_resume as accounts
from account_resume_daemon import ManagedDaemon
import capacity_exec as fixture
import model_resume as terminal
from picker_screen import SYNC_END, visible_lines
from quota_resume import Rpc
import renewal_follow as logins


def empty_picker(raw, process, deadline):
    while process.poll() is None and time.monotonic() < deadline:
        data = raw.read_bytes() if raw.is_file() else b""
        lines = visible_lines(data)
        if (any("Resume a previous session" in line for line in lines)
                and any(line.strip() == "No sessions yet" for line in lines)
                and any(" 0 / 0 · 100%" in line for line in lines)
                and not any(word in line for line in lines for word in ("Loading", "Searching", "Remember the cedar"))):
            return data.rindex(SYNC_END) + len(SYNC_END)
        time.sleep(.05)
    raise RuntimeError("official bare resume did not settle on the empty picker")


def list_controls(rpc, thread, work, sibling, report):
    rpc.initialize()
    identity = rpc.request("account/read", {"refreshToken": False})
    me = identity["account"]
    workspace = (identity.get("workspaceRouting") or {}).get("chatgptAccountId")
    report["rpc_account"] = {"email": me["email"], "workspace": workspace}
    fixture.require(me["type"] == "chatgpt" and me["email"] == "destination@example.invalid"
                    and workspace == "workspace-destination", "list controls did not use B")
    kinds = ["cli", "vscode", "exec", "appServer"]
    queries = [("broad", False, None, kinds), ("account_only", True, None, kinds),
               ("interactive_only", False, None, kinds[:2]), ("cwd_only", False, sibling, kinds),
               ("original_cwd", False, work, kinds), ("combined", True, sibling, kinds[:2])]
    controls = report["list_controls"] = {}
    for name, current, cwd, origins in queries:
        params = {"archived": False, "limit": 25, "sortKey": "updated_at", "modelProviders": [],
                  "useStateDbOnly": True, "currentAccountOnly": current, "sourceKinds": origins}
        if cwd:
            params["cwd"] = str(cwd)
        page = rpc.request("thread/list", params)
        rows = [{k: row[k] for k in ("id", "source", "cwd")} for row in page["data"]]
        controls[name] = {"params": params, "rows": rows, "nextCursor": page["nextCursor"]}
        expected = [{"id": thread, "source": "exec", "cwd": str(work)}] if name in ("broad", "original_cwd") else []
        fixture.require(rows == expected and page["nextCursor"] is None, f"unexpected {name} thread/list result")


def run(binary, seed, root, expected_version="0.170.0"):
    root.mkdir(mode=0o700)
    env = fixture.fresh_environment(root)
    env.update(TERM="xterm-256color", TERM_PROGRAM="xterm", HTTP_PROXY="http://127.0.0.1:1",
               HTTPS_PROXY="http://127.0.0.1:1", NO_PROXY="localhost,127.0.0.1,::1")
    digest = terminal.CHECK["sha256"]
    report = {"passed": False, "events": [], "requests": [], "binary_sha256": digest(binary),
              "seed_sha256": digest(seed)}
    gateway = accounts.Gateway(report)
    listener = daemon = None
    try:
        for label, executable in (("version", binary), ("seed_version", seed)):
            fixture.require(fixture.run(executable, ["--version"], root, env, label) == 0, f"{label} probe failed")
            report[label] = (root / (label + "-stdout.jsonl")).read_text().strip()
        fixture.require(report["version"] == f"codex-cli {expected_version}",
                        f"this baseline requires official codex-cli {expected_version}")

        def secure(server):
            server.socket = logins.fixture_tls(root).wrap_socket(server.socket, server_side=True,
                                                                 do_handshake_on_connect=False)

        listener = fixture.start_listener(gateway.handler(), bind=accounts.AccountResumeServer, prepare=secure)
        port = listener[0].server_port
        issuer = f"https://127.0.0.1:{port}"
        env["CODEX_CA_CERTIFICATE"] = str(root / "fixture-ca.pem")
        terminal.configure(root, port, "ultra", tls=True)
        work, sibling = root / "work", root / "sibling"
        sibling.mkdir()
        config = root / "codex-home/config.toml"
        settings = config.read_text().replace('sandbox_mode = "read-only"', 'sandbox_mode = "workspace-write"')
        config.write_text('cli_auth_credentials_store = "file"\n' + settings
                          + f'\n[projects.{json.dumps(str(sibling))}]\ntrust_level = "trusted"\n')
        fixture.require(fixture.run(Path("/usr/bin/git"), ["init", "--quiet"], root, env, "git-init") == 0,
                        "could not initialize A's work directory")
        (work / "cedar-marker.txt").write_text("cedar-baseline\n")
        accounts.select_login(root, gateway, "source", issuer)
        thread, turn = accounts.exec_turn(seed, ["exec", "--json", terminal.SEED], root, env, "seed", terminal.REPLY)
        report.update(thread=thread, seed_turn=turn, rollout=accounts.only_seed_rollout(root, thread))
        fixture.require((work / "cedar-calls").read_text() == "cedar-baseline\n", "expected one real seed shell call")
        accounts.select_login(root, gateway, "destination", issuer)
        with gateway.lock:
            gateway.resumed = True
        state = root / "codex-home/app-server-daemon"
        state.mkdir(exist_ok=True)
        # Keep the supplied package fixed. Vanilla still selects and starts its own daemon.
        fixture.write_json(state / "settings.json", {"updater": {"autoUpdateEnabled": False}})
        daemon = ManagedDaemon(binary, root, env, report["binary_sha256"], report.setdefault("daemon", {}))
        setting = "features.daemon_auto_start = false\n"
        fixture.require(config.read_text().count(setting) == 1, "missing embedded seed configuration")
        config.write_text(config.read_text().replace(setting, ""))
        custody, end, _ = terminal.terminal_session(binary, root, env, "official-picker", ["resume"], gateway, [],
                                                    cwd=sibling, observe_bare_picker=empty_picker)
        terminal.CHECK["check_model_terminal_exit"](custody, [str(binary), "resume"])
        raw = (root / "terminal.raw").read_bytes()[:end]
        report["picker"] = {"cwd": str(sibling), "before_input_end_byte": end,
                            "screen": [line.rstrip() for line in visible_lines(raw) if line.strip()]}
        fixture.require(daemon.socket.is_socket(), "ordinary vanilla resume did not start its private daemon")
        status = fixture.run(binary, ["app-server", "--listen", "stdio://"], root, env, "lists", cwd=sibling,
                             interact=lambda proc, log: list_controls(Rpc(proc, log), thread, work, sibling, report))
        fixture.require(status == 0 and accounts.only_seed_rollout(root, thread) == report["rollout"],
                        "RPC list controls failed or changed the sole source rollout")
        fixture.require(digest(binary) == report["binary_sha256"] and digest(seed) == report["seed_sha256"],
                        "one of the supplied executables changed during the measurement")
        report["evidence_sha256"] = {name: digest(root / name) for name in
                                     ("terminal.raw", "terminal-custody.json", "lists-stdout.jsonl", "seed-stdout.jsonl")}
        report["passed"] = True
    except Exception as error:
        report.update(fixture.case_error_fields(error))
    finally:
        def stop():
            # Vanilla's start reconciles the updater against its settings, including a pidfile
            # left by incomplete startup. Both lifecycle operations use only this private home.
            try:
                if any(state.glob("*updater.pid")):
                    daemon.command("start")
            finally:
                result = daemon.command("stop")
            fixture.require(result["status"] in ("stopped", "notRunning") and not daemon.socket.exists()
                            and not any(state.glob("*updater.pid")), "private daemon or updater did not retire")
            if report["passed"]:
                copied = Path(result["managedCodexPath"]).resolve(strict=True)
                fixture.require(result["status"] == "stopped" and copied.is_relative_to(daemon.home / "packages")
                                and digest(copied) == report["binary_sha256"], "private daemon package changed")
            report["daemon"]["updater_pid_absent"] = True

        try:
            fixture.cleanup_resources((("gateway", *listener),) if listener else (), (), report, sys.exc_info()[1],
                                      stop=stop if daemon else None)
            if report["passed"]:
                seed_requests = report["requests"]
                fixture.require(len([r for r in seed_requests if r["history"]]) == 2
                                and all(r["login"] == "source" and r["account"] == "workspace-source"
                                        for r in seed_requests), "unexpected inference while seeding or browsing")
        except Exception as error:
            report.update(passed=False, **fixture.case_error_fields(error))
        except BaseException:
            report["passed"] = False
            raise
        finally:
            fixture.write_json(root / "result.json", report)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", required=True, type=Path, help="Official OpenAI package executable.")
    parser.add_argument("--expected-version", default="0.170.0",
                        help="Required version reported by the official executable (default: 0.170.0).")
    parser.add_argument("--seed-codex", required=True, type=Path, help="Executable used for A's original exec turn.")
    parser.add_argument("--output", required=True, type=Path, help="Fresh short path for Unix sockets and evidence.")
    args = parser.parse_args()
    sys.exit(run(args.codex.resolve(strict=True), args.seed_codex.resolve(strict=True), args.output.resolve(),
                 args.expected_version))
